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
    pub queued_at_ms: i64,
    pub first_claim_at_ms: Option<i64>,
    #[serde(rename = "wake_at")]
    pub wake_at_ms: Option<i64>,
    pub deadline_origin: Option<String>,
    pub deadline_duration_ms: Option<i64>,
    pub deadline_at_ms: Option<i64>,
}

impl Instance {
    pub fn with_deadline(&mut self, policy: &crate::DeadlinePolicy) -> Result<(), String> {
        let duration = i64::try_from(policy.duration.as_millis())
            .map_err(|_| "deadline duration too large")?;
        self.deadline_origin = Some(policy.origin.into());
        self.deadline_duration_ms = Some(duration);
        if policy.origin == "queued" {
            self.deadline_at_ms = Some(
                self.queued_at_ms
                    .checked_add(duration)
                    .ok_or("deadline overflow")?,
            );
        }
        Ok(())
    }

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
            queued_at_ms: now_ms(),
            first_claim_at_ms: None,
            wake_at_ms: None,
            deadline_origin: None,
            deadline_duration_ms: None,
            deadline_at_ms: None,
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
            ("queued_at_ms", "INTEGER"),
            ("first_claim_at_ms", "INTEGER"),
            ("wake_at_ms", "INTEGER"),
            ("deadline_origin", "TEXT"),
            ("deadline_duration_ms", "INTEGER"),
            ("deadline_at_ms", "INTEGER"),
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
        conn.execute(
            "UPDATE instances SET queued_at_ms=created_at*1000 WHERE queued_at_ms IS NULL",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;
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
            "INSERT INTO instances (id, namespace, version, process, input, status, result, error, created_at, updated_at, checkpoint, attempt, first_failure_at, next_eligible_at, terminal_name, queued_at_ms, first_claim_at_ms, wake_at_ms, deadline_origin, deadline_duration_ms, deadline_at_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
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
                instance.terminal_name.as_deref(),
                instance.queued_at_ms,
                instance.first_claim_at_ms,
                instance.wake_at_ms,
                instance.deadline_origin.as_deref(),
                instance.deadline_duration_ms,
                instance.deadline_at_ms
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
        let affected = tx.execute("UPDATE instances SET checkpoint=?1, updated_at=?2 WHERE id=?3 AND status IN ('pending', 'running') AND (deadline_at_ms IS NULL OR deadline_at_ms>?4)", turso::params![value, now(), id, now_ms()]).await.map_err(|e| e.to_string())?;
        if affected != 1 {
            return Err(format!("instance not active: {id}"));
        }
        tx.commit().await.map_err(|e| e.to_string())
    }

    pub async fn set_wait(
        &self,
        id: &str,
        checkpoint: &GraphCheckpoint,
        wake_at_ms: i64,
    ) -> Result<(), String> {
        let value = serde_json::to_string(checkpoint).map_err(|e| e.to_string())?;
        let conn = self.0.connect().map_err(|e| e.to_string())?;
        let changed = conn.execute("UPDATE instances SET status='waiting', checkpoint=?2, wake_at_ms=?3, updated_at=?4 WHERE id=?1 AND status IN ('pending','running') AND (deadline_at_ms IS NULL OR deadline_at_ms>?5)", turso::params![id, value, wake_at_ms, now(), now_ms()]).await.map_err(|e| e.to_string())?;
        if changed != 1 {
            return Err(format!("instance not active: {id}"));
        }
        Ok(())
    }

    pub async fn resume_wait(
        &self,
        id: &str,
        checkpoint: &GraphCheckpoint,
    ) -> Result<Option<&'static str>, String> {
        let value = serde_json::to_string(checkpoint).map_err(|e| e.to_string())?;
        let conn = self.0.connect().map_err(|e| e.to_string())?;
        let changed = conn.execute("UPDATE instances SET status=CASE WHEN attempt>0 THEN 'running' ELSE 'pending' END, checkpoint=?2, wake_at_ms=NULL, updated_at=?3 WHERE id=?1 AND status='waiting' AND wake_at_ms<=?4 AND (deadline_at_ms IS NULL OR deadline_at_ms>?4)", turso::params![id, value, now(), now_ms()]).await.map_err(|e| e.to_string())?;
        if changed != 1 {
            return Ok(None);
        }
        let state = self.get(id).await?.ok_or("missing resumed wait")?;
        Ok(Some(if state.status == "pending" {
            "pending"
        } else {
            "running"
        }))
    }

    pub async fn begin_attempt(&self, id: &str) -> Result<(), String> {
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        let affected = tx.execute("UPDATE instances SET attempt=attempt+1, status='running', next_eligible_at=NULL, first_claim_at_ms=COALESCE(first_claim_at_ms, ?3), deadline_at_ms=CASE WHEN deadline_origin='first_claimed' THEN COALESCE(deadline_at_ms, ?3+deadline_duration_ms) ELSE deadline_at_ms END, updated_at=?1 WHERE id=?2 AND (status='pending' OR (status='retry-waiting' AND next_eligible_at<=?3)) AND (deadline_at_ms IS NULL OR deadline_at_ms>?3)", turso::params![now(), id, now_ms()]).await.map_err(|e| e.to_string())?;
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
        let affected = tx.execute("UPDATE instances SET attempt=?1, first_failure_at=?2, next_eligible_at=?3, status=?4, error=?5, updated_at=?6 WHERE id=?7 AND status IN ('pending', 'running', 'retry-waiting') AND (deadline_at_ms IS NULL OR deadline_at_ms>?8)", turso::params![i64::from(attempt), first_failure_at, next_eligible_at, status, error, now(), id, now_ms()]).await.map_err(|e| e.to_string())?;
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
                "UPDATE instances SET status=?1, result=?2, error=?3, terminal_name=?4, updated_at=?5 WHERE id=?6 AND status IN ('pending','running','retry-waiting','waiting','cancelling') AND (deadline_at_ms IS NULL OR deadline_at_ms>?7 OR status='cancelling')",
                turso::params![status, result.map(|v| v.to_string()), error, terminal_name, now(), id, now_ms()],
            )
            .await
            .map_err(|e| e.to_string())?;
        if affected != 1 {
            return Err(format!("instance not active: {id}"));
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
        let mut rows = conn.query("SELECT id FROM instances WHERE checkpoint IS NOT NULL AND status IN ('pending', 'running', 'retry-waiting', 'waiting')", ()).await.map_err(|e| e.to_string())?;
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

    pub async fn expire_due(&self, at_ms: i64) -> Result<Vec<String>, String> {
        let conn = self.0.connect().map_err(|e| e.to_string())?;
        let mut rows = conn.query("SELECT id FROM instances WHERE deadline_at_ms IS NOT NULL AND deadline_at_ms<=?1 AND status IN ('pending','running','retry-waiting','waiting')", [at_ms]).await.map_err(|e| e.to_string())?;
        let mut ids = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
            ids.push(row.get::<String>(0).map_err(|e| e.to_string())?);
        }
        drop(rows);
        let mut expired = Vec::new();
        for id in ids {
            let changed = conn.execute("UPDATE instances SET status='business-error', terminal_name='timeout', wake_at_ms=NULL, updated_at=?3 WHERE id=?1 AND deadline_at_ms<=?2 AND status IN ('pending','running','retry-waiting','waiting')", turso::params![id.as_str(), at_ms, now()]).await.map_err(|e| e.to_string())?;
            if changed == 1 {
                expired.push(id);
            }
        }
        Ok(expired)
    }

    pub async fn due_waits(&self, at_ms: i64) -> Result<Vec<Instance>, String> {
        let conn = self.0.connect().map_err(|e| e.to_string())?;
        let mut rows = conn
            .query(
                "SELECT id FROM instances WHERE status='waiting' AND wake_at_ms<=?1",
                [at_ms],
            )
            .await
            .map_err(|e| e.to_string())?;
        let mut ids = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
            ids.push(row.get::<String>(0).map_err(|e| e.to_string())?);
        }
        drop(rows);
        let mut due = Vec::new();
        for id in ids {
            if let Some(item) = self.get(&id).await? {
                due.push(item);
            }
        }
        Ok(due)
    }

    pub async fn get(&self, id: &str) -> Result<Option<Instance>, String> {
        let conn = self.0.connect().map_err(|e| e.to_string())?;
        let mut rows = conn.query("SELECT id, namespace, version, process, input, status, result, error, created_at, updated_at, checkpoint, attempt, first_failure_at, next_eligible_at, terminal_name, queued_at_ms, first_claim_at_ms, wake_at_ms, deadline_origin, deadline_duration_ms, deadline_at_ms FROM instances WHERE id=?1", [id])
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
            queued_at_ms: row.get(15).map_err(|e| e.to_string())?,
            first_claim_at_ms: row.get(16).map_err(|e| e.to_string())?,
            wake_at_ms: row.get(17).map_err(|e| e.to_string())?,
            deadline_origin: row.get(18).map_err(|e| e.to_string())?,
            deadline_duration_ms: row.get(19).map_err(|e| e.to_string())?,
            deadline_at_ms: row.get(20).map_err(|e| e.to_string())?,
        }))
    }
}
