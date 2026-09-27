use std::{
    path::Path,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

use crate::named_runtime::GraphCheckpoint;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Instance {
    pub id: String,
    pub namespace: String,
    pub version: String,
    pub process: String,
    pub input: Value,
    pub status: String,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub checkpoint: Option<GraphCheckpoint>,
    pub attempt: u32,
    pub first_failure_at: Option<i64>,
    pub next_eligible_at: Option<i64>,
    pub terminal_name: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Instance {
    pub fn new(id: &str, namespace: &str, version: &str, process: &str, input: Value) -> Self {
        Self {
            id: id.into(),
            namespace: namespace.into(),
            version: version.into(),
            process: process.into(),
            input,
            status: "pending".into(),
            result: None,
            error: None,
            checkpoint: None,
            attempt: 0,
            first_failure_at: None,
            next_eligible_at: None,
            terminal_name: None,
            created_at: now(),
            updated_at: now(),
        }
    }
}

#[derive(Clone)]
pub struct Store(Arc<turso::Database>);

impl Store {
    pub async fn open(path: &Path) -> Result<Self, String> {
        let db = turso::Builder::new_local(path.to_str().ok_or("invalid database path")?)
            .build()
            .await
            .map_err(|e| e.to_string())?;
        let conn = db.connect().map_err(|e| e.to_string())?;
        conn.execute("CREATE TABLE IF NOT EXISTS instances (id TEXT PRIMARY KEY, namespace TEXT NOT NULL, version TEXT NOT NULL, process TEXT NOT NULL, input TEXT NOT NULL, status TEXT NOT NULL, result TEXT, error TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, checkpoint TEXT, attempt INTEGER NOT NULL DEFAULT 0, first_failure_at INTEGER, next_eligible_at INTEGER, terminal_name TEXT)", ())
            .await.map_err(|e| e.to_string())?;
        let mut rows = conn
            .query("PRAGMA table_info(instances)", ())
            .await
            .map_err(|e| e.to_string())?;
        let mut existing = std::collections::HashSet::new();
        while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
            existing.insert(row.get::<String>(1).map_err(|e| e.to_string())?);
        }
        drop(rows);
        for (name, definition) in [
            ("checkpoint", "TEXT"),
            ("attempt", "INTEGER NOT NULL DEFAULT 0"),
            ("first_failure_at", "INTEGER"),
            ("next_eligible_at", "INTEGER"),
            ("terminal_name", "TEXT"),
        ] {
            if !existing.contains(name) {
                conn.execute(
                    &format!("ALTER TABLE instances ADD COLUMN {name} {definition}"),
                    (),
                )
                .await
                .map_err(|e| e.to_string())?;
            }
        }
        Ok(Self(Arc::new(db)))
    }

    pub async fn create(&self, instance: &Instance) -> Result<(), String> {
        let checkpoint = instance
            .checkpoint
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| e.to_string())?;
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO instances (id, namespace, version, process, input, status, result, error, created_at, updated_at, checkpoint, attempt, first_failure_at, next_eligible_at, terminal_name) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            turso::params![
                instance.id.as_str(),
                instance.namespace.as_str(),
                instance.version.as_str(),
                instance.process.as_str(),
                instance.input.to_string(),
                instance.status.as_str(),
                instance.result.as_ref().map(Value::to_string),
                instance.error.as_deref(),
                instance.created_at,
                instance.updated_at,
                checkpoint,
                i64::from(instance.attempt),
                instance.first_failure_at,
                instance.next_eligible_at,
                instance.terminal_name.as_deref()
            ],
        )
        .await
        .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())
    }

    pub async fn commit_checkpoint(
        &self,
        id: &str,
        checkpoint: &GraphCheckpoint,
    ) -> Result<(), String> {
        let value = serde_json::to_string(checkpoint).map_err(|e| e.to_string())?;
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        let affected = tx.execute("UPDATE instances SET checkpoint=?1, updated_at=?2 WHERE id=?3 AND status IN ('pending', 'running')", turso::params![value, now(), id]).await.map_err(|e| e.to_string())?;
        if affected != 1 {
            return Err(format!("instance not active: {id}"));
        }
        tx.commit().await.map_err(|e| e.to_string())
    }

    pub async fn begin_attempt(&self, id: &str) -> Result<(), String> {
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        let affected = tx.execute("UPDATE instances SET attempt=attempt+1, status='running', next_eligible_at=NULL, updated_at=?1 WHERE id=?2 AND (status='pending' OR (status='retry-waiting' AND next_eligible_at<=?3))", turso::params![now(), id, now_ms()]).await.map_err(|e| e.to_string())?;
        if affected != 1 {
            return Err(format!("instance not eligible: {id}"));
        }
        tx.commit().await.map_err(|e| e.to_string())
    }

    pub async fn record_retry(
        &self,
        id: &str,
        attempt: u32,
        first_failure_at: i64,
        next_eligible_at: Option<i64>,
        error: &str,
    ) -> Result<(), String> {
        let status = if next_eligible_at.is_some() {
            "retry-waiting"
        } else {
            "failed"
        };
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        let affected = tx.execute("UPDATE instances SET attempt=?1, first_failure_at=?2, next_eligible_at=?3, status=?4, error=?5, updated_at=?6 WHERE id=?7 AND status IN ('pending', 'running', 'retry-waiting')", turso::params![i64::from(attempt), first_failure_at, next_eligible_at, status, error, now(), id]).await.map_err(|e| e.to_string())?;
        if affected != 1 {
            return Err(format!("instance not active: {id}"));
        }
        tx.commit().await.map_err(|e| e.to_string())
    }

    pub async fn finish_named(
        &self,
        id: &str,
        status: &str,
        terminal_name: &str,
        result: Option<Value>,
        error: Option<&str>,
    ) -> Result<(), String> {
        self.finish_with_terminal(id, status, Some(terminal_name), result, error)
            .await
    }

    pub async fn finish(
        &self,
        id: &str,
        status: &str,
        result: Option<Value>,
        error: Option<&str>,
    ) -> Result<(), String> {
        self.finish_with_terminal(id, status, None, result, error)
            .await
    }

    async fn finish_with_terminal(
        &self,
        id: &str,
        status: &str,
        terminal_name: Option<&str>,
        result: Option<Value>,
        error: Option<&str>,
    ) -> Result<(), String> {
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        let affected = tx
            .execute(
                "UPDATE instances SET status=?1, result=?2, error=?3, terminal_name=?4, updated_at=?5 WHERE id=?6",
                turso::params![status, result.map(|v| v.to_string()), error, terminal_name, now(), id],
            )
            .await
            .map_err(|e| e.to_string())?;
        if affected != 1 {
            return Err(format!("unknown instance: {id}"));
        }
        tx.commit().await.map_err(|e| e.to_string())
    }

    pub async fn recover_interrupted(&self) -> Result<(), String> {
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        tx.execute("UPDATE instances SET status='failed', error='interrupted by server restart', updated_at=?1 WHERE checkpoint IS NULL AND status IN ('pending', 'running', 'cancelling')", [now()])
            .await.map_err(|e| e.to_string())?;
        tx.execute("UPDATE instances SET status='cancelled', updated_at=?1 WHERE checkpoint IS NOT NULL AND status='cancelling'", [now()])
            .await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())
    }

    pub async fn incomplete(&self) -> Result<Vec<Instance>, String> {
        let conn = self.0.connect().map_err(|e| e.to_string())?;
        let mut rows = conn.query("SELECT id FROM instances WHERE checkpoint IS NOT NULL AND status IN ('pending', 'running', 'retry-waiting')", ()).await.map_err(|e| e.to_string())?;
        let mut ids = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
            ids.push(row.get::<String>(0).map_err(|e| e.to_string())?);
        }
        drop(rows);
        let mut instances = Vec::new();
        for id in ids {
            if let Some(instance) = self.get(&id).await? {
                instances.push(instance);
            }
        }
        Ok(instances)
    }

    pub async fn get(&self, id: &str) -> Result<Option<Instance>, String> {
        let conn = self.0.connect().map_err(|e| e.to_string())?;
        let mut rows = conn.query("SELECT id, namespace, version, process, input, status, result, error, created_at, updated_at, checkpoint, attempt, first_failure_at, next_eligible_at, terminal_name FROM instances WHERE id=?1", [id])
            .await.map_err(|e| e.to_string())?;
        let Some(row) = rows.next().await.map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        let input: String = row.get(4).map_err(|e| e.to_string())?;
        let result: Option<String> = row.get(6).map_err(|e| e.to_string())?;
        let checkpoint: Option<String> = row.get(10).map_err(|e| e.to_string())?;
        let attempt: i64 = row.get(11).map_err(|e| e.to_string())?;
        Ok(Some(Instance {
            id: row.get(0).map_err(|e| e.to_string())?,
            namespace: row.get(1).map_err(|e| e.to_string())?,
            version: row.get(2).map_err(|e| e.to_string())?,
            process: row.get(3).map_err(|e| e.to_string())?,
            input: serde_json::from_str(&input).map_err(|e| e.to_string())?,
            status: row.get(5).map_err(|e| e.to_string())?,
            result: result
                .map(|text| serde_json::from_str(&text))
                .transpose()
                .map_err(|e: serde_json::Error| e.to_string())?,
            error: row.get(7).map_err(|e| e.to_string())?,
            checkpoint: checkpoint
                .map(|text| serde_json::from_str(&text))
                .transpose()
                .map_err(|e: serde_json::Error| e.to_string())?,
            attempt: u32::try_from(attempt).map_err(|e| e.to_string())?,
            first_failure_at: row.get(12).map_err(|e| e.to_string())?,
            next_eligible_at: row.get(13).map_err(|e| e.to_string())?,
            terminal_name: row.get(14).map_err(|e| e.to_string())?,
            created_at: row.get(8).map_err(|e| e.to_string())?,
            updated_at: row.get(9).map_err(|e| e.to_string())?,
        }))
    }
}
