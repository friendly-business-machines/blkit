use std::{path::Path, sync::Arc, time::{SystemTime, UNIX_EPOCH}};

use serde_json::Value;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
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
    pub created_at: i64,
    pub updated_at: i64,
}

impl Instance {
    pub fn new(id: &str, namespace: &str, version: &str, process: &str, input: Value) -> Self {
        Self { id: id.into(), namespace: namespace.into(), version: version.into(), process: process.into(),
            input, status: "pending".into(), result: None, error: None, created_at: now(), updated_at: now() }
    }
}

#[derive(Clone)]
pub struct Store(Arc<turso::Database>);

impl Store {
    pub async fn open(path: &Path) -> Result<Self, String> {
        let db = turso::Builder::new_local(path.to_str().ok_or("invalid database path")?)
            .build().await.map_err(|e| e.to_string())?;
        let conn = db.connect().map_err(|e| e.to_string())?;
        conn.execute("CREATE TABLE IF NOT EXISTS instances (id TEXT PRIMARY KEY, namespace TEXT NOT NULL, version TEXT NOT NULL, process TEXT NOT NULL, input TEXT NOT NULL, status TEXT NOT NULL, result TEXT, error TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL)", ())
            .await.map_err(|e| e.to_string())?;
        Ok(Self(Arc::new(db)))
    }

    pub async fn create(&self, instance: &Instance) -> Result<(), String> {
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO instances VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            turso::params![instance.id.as_str(), instance.namespace.as_str(), instance.version.as_str(), instance.process.as_str(), instance.input.to_string(), instance.status.as_str(), Option::<String>::None, Option::<String>::None, instance.created_at, instance.updated_at])
            .await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())
    }

    pub async fn finish(&self, id: &str, status: &str, result: Option<Value>, error: Option<&str>) -> Result<(), String> {
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        let affected = tx.execute("UPDATE instances SET status=?1, result=?2, error=?3, updated_at=?4 WHERE id=?5",
            turso::params![status, result.map(|v| v.to_string()), error, now(), id])
            .await.map_err(|e| e.to_string())?;
        if affected != 1 { return Err(format!("unknown instance: {id}")); }
        tx.commit().await.map_err(|e| e.to_string())
    }

    pub async fn recover_interrupted(&self) -> Result<(), String> {
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        tx.execute("UPDATE instances SET status='failed', error='interrupted by server restart', updated_at=?1 WHERE status IN ('pending', 'running', 'cancelling')", [now()])
            .await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())
    }

    pub async fn get(&self, id: &str) -> Result<Option<Instance>, String> {
        let conn = self.0.connect().map_err(|e| e.to_string())?;
        let mut rows = conn.query("SELECT id, namespace, version, process, input, status, result, error, created_at, updated_at FROM instances WHERE id=?1", [id])
            .await.map_err(|e| e.to_string())?;
        let Some(row) = rows.next().await.map_err(|e| e.to_string())? else { return Ok(None); };
        let input: String = row.get(4).map_err(|e| e.to_string())?;
        let result: Option<String> = row.get(6).map_err(|e| e.to_string())?;
        Ok(Some(Instance {
            id: row.get(0).map_err(|e| e.to_string())?,
            namespace: row.get(1).map_err(|e| e.to_string())?,
            version: row.get(2).map_err(|e| e.to_string())?,
            process: row.get(3).map_err(|e| e.to_string())?,
            input: serde_json::from_str(&input).map_err(|e| e.to_string())?,
            status: row.get(5).map_err(|e| e.to_string())?,
            result: result.map(|text| serde_json::from_str(&text)).transpose().map_err(|e: serde_json::Error| e.to_string())?,
            error: row.get(7).map_err(|e| e.to_string())?,
            created_at: row.get(8).map_err(|e| e.to_string())?,
            updated_at: row.get(9).map_err(|e| e.to_string())?,
        }))
    }
}
