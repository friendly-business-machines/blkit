use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(feature = "local-persistence")]
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};
#[cfg(feature = "local-persistence")]
use tokio::sync::broadcast;

use serde_json::Value;

use crate::compiled_graph::GraphCheckpoint;
#[cfg(feature = "local-persistence")]
use crate::compiled_graph::InputRequest;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

// A failed COMMIT can still have persisted its transaction; never compensate on this error.
pub(crate) const UNCERTAIN_START_COMMIT: &str = "uncertain start commit: ";
pub(crate) const UNCERTAIN_REPLY_COMMIT: &str = "uncertain reply commit: ";

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub instance_id: String,
    pub request_id: Option<String>,
    pub ordinal: i64,
    pub object_key: String,
    pub filename: String,
    pub media_type: Option<String>,
    pub byte_length: i64,
}

#[cfg(any(feature = "local-persistence", feature = "remote-persistence"))]
pub(crate) fn validate_attachment_scope(
    attachments: &[Attachment],
    id: &str,
    request_id: Option<&str>,
) -> Result<(), String> {
    if attachments.iter().any(|attachment| {
        attachment.instance_id != id || attachment.request_id.as_deref() != request_id
    }) {
        return Err("attachment scope does not match instance and request".into());
    }
    Ok(())
}

#[cfg(all(test, feature = "local-persistence"))]
mod attachment_scope_tests {
    use super::{Attachment, Instance, Store};
    use serde_json::json;

    #[tokio::test]
    async fn reply_rejects_mismatched_existing_instance_or_request_without_answering() {
        let path =
            std::env::temp_dir().join(format!("blkit-reply-scope-{}.db", uuid::Uuid::new_v4()));
        let store = Store::open(&path).await.unwrap();
        for id in ["target", "other"] {
            store
                .create(&Instance::new(id, "ns", "1", "p", json!({})))
                .await
                .unwrap();
        }
        let conn = store.0.connect().unwrap();
        for (id, request) in [
            ("target", "answer"),
            ("target", "different"),
            ("other", "answer"),
        ] {
            conn.execute("INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES (?1, ?2, 'prompt', '{}')", turso::params![id, request]).await.unwrap();
        }
        for (id, request) in [
            ("other", Some("answer")),
            ("target", Some("different")),
            ("target", None),
        ] {
            let file = Attachment {
                instance_id: id.into(),
                request_id: request.map(str::to_owned),
                ordinal: 0,
                object_key: "objects/wrong".into(),
                filename: "wrong".into(),
                media_type: None,
                byte_length: 1,
            };
            let mut valid = file.clone();
            valid.instance_id = "target".into();
            valid.request_id = Some("answer".into());
            valid.ordinal = 1;
            let error = store
                .accept_response_with_attachments("target", "answer", &json!({}), &[valid, file])
                .await
                .unwrap_err();
            assert!(error.contains("attachment scope"), "{error}");
            assert!(store.attachments(id, request).await.unwrap().is_empty());
            assert!(
                store
                    .attachments("target", Some("answer"))
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert!(store.answers("target").await.unwrap().is_empty());
        }
        drop(conn);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(feature = "local-persistence")]
#[derive(Clone)]
pub struct Store(
    Arc<turso::Database>,
    Arc<Mutex<HashMap<String, broadcast::Sender<()>>>>,
);

#[cfg(feature = "local-persistence")]
pub struct Subscription {
    id: String,
    subscribers: Arc<Mutex<HashMap<String, broadcast::Sender<()>>>>,
    receiver: broadcast::Receiver<()>,
}

#[cfg(feature = "local-persistence")]
impl Subscription {
    pub async fn recv(&mut self) -> Result<(), broadcast::error::RecvError> {
        self.receiver.recv().await
    }
}

#[cfg(feature = "local-persistence")]
impl Drop for Subscription {
    fn drop(&mut self) {
        let mut subscribers = self.subscribers.lock().unwrap();
        if subscribers
            .get(&self.id)
            .is_some_and(|sender| sender.receiver_count() == 1)
        {
            subscribers.remove(&self.id);
        }
    }
}

#[cfg(feature = "local-persistence")]
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
        conn.execute("CREATE TABLE IF NOT EXISTS input_requests (instance_id TEXT NOT NULL, request_id TEXT NOT NULL, prompt TEXT NOT NULL, schema TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'outstanding', response TEXT, PRIMARY KEY (instance_id, request_id), FOREIGN KEY (instance_id) REFERENCES instances(id))", ())
            .await.map_err(|e| e.to_string())?;
        conn.execute("CREATE TABLE IF NOT EXISTS instance_attachments (instance_id TEXT NOT NULL REFERENCES instances(id), request_id TEXT, ordinal INTEGER NOT NULL CHECK (ordinal >= 0), object_key TEXT NOT NULL, filename TEXT NOT NULL, media_type TEXT, byte_length INTEGER NOT NULL CHECK (byte_length >= 0), FOREIGN KEY (instance_id, request_id) REFERENCES input_requests(instance_id, request_id))", ())
            .await.map_err(|e| e.to_string())?;
        conn.execute("CREATE UNIQUE INDEX IF NOT EXISTS instance_attachments_start ON instance_attachments(instance_id, ordinal) WHERE request_id IS NULL", ())
            .await.map_err(|e| e.to_string())?;
        conn.execute("CREATE UNIQUE INDEX IF NOT EXISTS instance_attachments_reply ON instance_attachments(instance_id, request_id, ordinal) WHERE request_id IS NOT NULL", ())
            .await.map_err(|e| e.to_string())?;
        Ok(Self(Arc::new(db), Arc::new(Mutex::new(HashMap::new()))))
    }

    pub async fn insert_attachment(&self, attachment: &Attachment) -> Result<(), String> {
        let conn = self.0.connect().map_err(|e| e.to_string())?;
        conn.execute("PRAGMA foreign_keys = ON", ())
            .await
            .map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO instance_attachments (instance_id, request_id, ordinal, object_key, filename, media_type, byte_length) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", turso::params![attachment.instance_id.as_str(), attachment.request_id.as_deref(), attachment.ordinal, attachment.object_key.as_str(), attachment.filename.as_str(), attachment.media_type.as_deref(), attachment.byte_length])
            .await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn attachments(
        &self,
        id: &str,
        request_id: Option<&str>,
    ) -> Result<Vec<Attachment>, String> {
        let conn = self.0.connect().map_err(|e| e.to_string())?;
        let mut rows = conn.query("SELECT instance_id, request_id, ordinal, object_key, filename, media_type, byte_length FROM instance_attachments WHERE instance_id=?1 AND request_id IS ?2 ORDER BY ordinal", turso::params![id, request_id])
            .await.map_err(|e| e.to_string())?;
        let mut attachments = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
            attachments.push(Attachment {
                instance_id: row.get(0).map_err(|e| e.to_string())?,
                request_id: row.get(1).map_err(|e| e.to_string())?,
                ordinal: row.get(2).map_err(|e| e.to_string())?,
                object_key: row.get(3).map_err(|e| e.to_string())?,
                filename: row.get(4).map_err(|e| e.to_string())?,
                media_type: row.get(5).map_err(|e| e.to_string())?,
                byte_length: row.get(6).map_err(|e| e.to_string())?,
            });
        }
        Ok(attachments)
    }

    pub fn subscribe(&self, id: &str) -> Subscription {
        let mut subscribers = self.1.lock().unwrap();
        let receiver = subscribers
            .entry(id.to_owned())
            .or_insert_with(|| broadcast::channel(1).0)
            .subscribe();
        Subscription {
            id: id.to_owned(),
            subscribers: self.1.clone(),
            receiver,
        }
    }

    fn changed(&self, id: &str) {
        if let Some(sender) = self.1.lock().unwrap().get(id) {
            let _ = sender.send(());
        }
    }

    pub async fn create(&self, instance: &Instance) -> Result<(), String> {
        self.create_with_attachments(instance, &[]).await
    }

    pub async fn create_with_attachments(
        &self,
        instance: &Instance,
        attachments: &[Attachment],
    ) -> Result<(), String> {
        validate_attachment_scope(attachments, &instance.id, None)?;
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
        if let Some(checkpoint) = &instance.checkpoint {
            Self::sync_requests(&tx, &instance.id, checkpoint).await?;
        }
        for attachment in attachments {
            tx.execute("INSERT INTO instance_attachments (instance_id, request_id, ordinal, object_key, filename, media_type, byte_length) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", turso::params![attachment.instance_id.as_str(), attachment.request_id.as_deref(), attachment.ordinal, attachment.object_key.as_str(), attachment.filename.as_str(), attachment.media_type.as_deref(), attachment.byte_length])
                .await.map_err(|e| e.to_string())?;
        }
        tx.commit()
            .await
            .map_err(|e| format!("{UNCERTAIN_START_COMMIT}{e}"))
    }

    async fn sync_requests(
        tx: &turso::transaction::Transaction<'_>,
        id: &str,
        checkpoint: &GraphCheckpoint,
    ) -> Result<bool, String> {
        let mut changed = false;
        let requests = checkpoint.requests();
        for request in &requests {
            changed |= tx.execute("INSERT OR IGNORE INTO input_requests (instance_id, request_id, prompt, schema) VALUES (?1, ?2, ?3, ?4)", turso::params![id, request.id.as_str(), request.prompt.as_str(), serde_json::to_string(&request.schema).map_err(|e| e.to_string())?])
                .await.map_err(|e| e.to_string())? != 0;
        }
        let active: std::collections::HashSet<_> =
            requests.iter().map(|request| request.id.as_str()).collect();
        let mut rows = tx.query("SELECT request_id FROM input_requests WHERE instance_id=?1 AND status='outstanding'", [id])
            .await.map_err(|e| e.to_string())?;
        let mut obsolete = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
            let request_id: String = row.get(0).map_err(|e| e.to_string())?;
            if !active.contains(request_id.as_str()) {
                obsolete.push(request_id);
            }
        }
        drop(rows);
        changed |= !obsolete.is_empty();
        for request_id in obsolete {
            tx.execute("UPDATE input_requests SET status='retired' WHERE instance_id=?1 AND request_id=?2 AND status='outstanding'", turso::params![id, request_id])
                .await.map_err(|e| e.to_string())?;
        }
        Ok(changed)
    }

    pub async fn requests(&self, id: &str) -> Result<Vec<InputRequest>, String> {
        let conn = self.0.connect().map_err(|e| e.to_string())?;
        Self::requests_from(&conn, id).await
    }

    async fn requests_from(
        conn: &turso::Connection,
        id: &str,
    ) -> Result<Vec<InputRequest>, String> {
        let mut rows = conn.query("SELECT request_id, prompt, schema FROM input_requests WHERE instance_id=?1 AND status='outstanding' ORDER BY request_id", [id])
            .await.map_err(|e| e.to_string())?;
        let mut requests = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
            let schema: String = row.get(2).map_err(|e| e.to_string())?;
            requests.push(InputRequest {
                id: row.get(0).map_err(|e| e.to_string())?,
                prompt: row.get(1).map_err(|e| e.to_string())?,
                schema: serde_json::from_str(&schema).map_err(|e| e.to_string())?,
            });
        }
        Ok(requests)
    }

    pub async fn snapshot(
        &self,
        id: &str,
    ) -> Result<Option<(Instance, Vec<InputRequest>)>, String> {
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        let instance = Self::get_from(&tx, id).await?;
        let requests = if instance.is_some() {
            Self::requests_from(&tx, id).await?
        } else {
            Vec::new()
        };
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(instance.map(|instance| (instance, requests)))
    }

    pub(crate) async fn request_exists(&self, id: &str, request_id: &str) -> Result<bool, String> {
        let conn = self.0.connect().map_err(|e| e.to_string())?;
        let mut rows = conn
            .query(
                "SELECT 1 FROM input_requests WHERE instance_id=?1 AND request_id=?2",
                turso::params![id, request_id],
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(rows.next().await.map_err(|e| e.to_string())?.is_some())
    }

    pub(crate) async fn answers(&self, id: &str) -> Result<Vec<(String, Value)>, String> {
        let conn = self.0.connect().map_err(|e| e.to_string())?;
        let mut rows = conn.query("SELECT request_id, response FROM input_requests WHERE instance_id=?1 AND status='answered' ORDER BY request_id", [id])
            .await.map_err(|e| e.to_string())?;
        let mut answers = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
            let response: String = row.get(1).map_err(|e| e.to_string())?;
            answers.push((
                row.get(0).map_err(|e| e.to_string())?,
                serde_json::from_str(&response).map_err(|e| e.to_string())?,
            ));
        }
        Ok(answers)
    }

    pub(crate) async fn accept_response_with_attachments(
        &self,
        id: &str,
        request_id: &str,
        response: &Value,
        attachments: &[Attachment],
    ) -> Result<(), String> {
        validate_attachment_scope(attachments, id, Some(request_id))?;
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        let changed = tx.execute("UPDATE input_requests SET status='answered', response=?3 WHERE instance_id=?1 AND request_id=?2 AND status='outstanding' AND EXISTS (SELECT 1 FROM instances WHERE id=?1 AND status IN ('pending','running','waiting','waiting-for-input') AND (deadline_at_ms IS NULL OR deadline_at_ms>?4))", turso::params![id, request_id, response.to_string(), now_ms()]).await.map_err(|e| e.to_string())?;
        if changed != 1 {
            return Err("request not outstanding or instance not active".into());
        }
        for attachment in attachments {
            tx.execute("INSERT INTO instance_attachments (instance_id, request_id, ordinal, object_key, filename, media_type, byte_length) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", turso::params![attachment.instance_id.as_str(), attachment.request_id.as_deref(), attachment.ordinal, attachment.object_key.as_str(), attachment.filename.as_str(), attachment.media_type.as_deref(), attachment.byte_length])
                .await.map_err(|e| e.to_string())?;
        }
        tx.execute("UPDATE instances SET status=CASE WHEN attempt>0 THEN 'running' ELSE 'pending' END, wake_at_ms=NULL, updated_at=?2 WHERE id=?1 AND status IN ('waiting-for-input','waiting')", turso::params![id, now()]).await.map_err(|e| e.to_string())?;
        tx.commit()
            .await
            .map_err(|e| format!("{UNCERTAIN_REPLY_COMMIT}{e}"))?;
        self.changed(id);
        Ok(())
    }

    pub(crate) async fn consume_response(
        &self,
        id: &str,
        request_id: &str,
        checkpoint: &GraphCheckpoint,
    ) -> Result<(), String> {
        let value = serde_json::to_string(checkpoint).map_err(|e| e.to_string())?;
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        let changed = tx.execute("UPDATE input_requests SET status='consumed' WHERE instance_id=?1 AND request_id=?2 AND status='answered' AND EXISTS (SELECT 1 FROM instances WHERE id=?1 AND status IN ('pending','running') AND (deadline_at_ms IS NULL OR deadline_at_ms>?3))", turso::params![id, request_id, now_ms()]).await.map_err(|e| e.to_string())?;
        if changed != 1 {
            return Err("request no longer answerable".into());
        }
        tx.execute(
            "UPDATE instances SET checkpoint=?2, updated_at=?3 WHERE id=?1",
            turso::params![id, value, now()],
        )
        .await
        .map_err(|e| e.to_string())?;
        let changed = Self::sync_requests(&tx, id, checkpoint).await?;
        tx.commit().await.map_err(|e| e.to_string())?;
        if changed {
            self.changed(id);
        }
        Ok(())
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
        let changed = Self::sync_requests(&tx, id, checkpoint).await?;
        tx.commit().await.map_err(|e| e.to_string())?;
        if changed {
            self.changed(id);
        }
        Ok(())
    }

    pub async fn set_input_wait(
        &self,
        id: &str,
        checkpoint: &GraphCheckpoint,
    ) -> Result<bool, String> {
        let value = serde_json::to_string(checkpoint).map_err(|e| e.to_string())?;
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        let changed_requests = Self::sync_requests(&tx, id, checkpoint).await?;
        for request in checkpoint.requests() {
            let mut rows = tx.query("SELECT 1 FROM input_requests WHERE instance_id=?1 AND request_id=?2 AND status='answered'", turso::params![id, request.id]).await.map_err(|e| e.to_string())?;
            if rows.next().await.map_err(|e| e.to_string())?.is_some() {
                return Ok(false);
            }
        }
        let claimed_at = now_ms();
        let changed = tx.execute("UPDATE instances SET status='waiting-for-input', checkpoint=?2, wake_at_ms=NULL, updated_at=?3, first_claim_at_ms=COALESCE(first_claim_at_ms, ?4), deadline_at_ms=CASE WHEN deadline_origin='first_claimed' THEN COALESCE(deadline_at_ms, COALESCE(first_claim_at_ms, ?4)+deadline_duration_ms) ELSE deadline_at_ms END WHERE id=?1 AND status IN ('pending','running') AND (deadline_at_ms IS NULL OR deadline_at_ms>?4)", turso::params![id, value, now(), claimed_at]).await.map_err(|e| e.to_string())?;
        if changed != 1 {
            return Err(format!("instance not active: {id}"));
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        if changed_requests {
            self.changed(id);
        }
        Ok(true)
    }

    pub async fn set_wait(
        &self,
        id: &str,
        checkpoint: &GraphCheckpoint,
        wake_at_ms: i64,
    ) -> Result<bool, String> {
        let value = serde_json::to_string(checkpoint).map_err(|e| e.to_string())?;
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        let changed_requests = Self::sync_requests(&tx, id, checkpoint).await?;
        for request in checkpoint.requests() {
            let mut rows = tx.query("SELECT 1 FROM input_requests WHERE instance_id=?1 AND request_id=?2 AND status='answered'", turso::params![id, request.id]).await.map_err(|e| e.to_string())?;
            if rows.next().await.map_err(|e| e.to_string())?.is_some() {
                return Ok(false);
            }
        }
        let changed = tx.execute("UPDATE instances SET status='waiting', checkpoint=?2, wake_at_ms=?3, updated_at=?4 WHERE id=?1 AND status IN ('pending','running') AND (deadline_at_ms IS NULL OR deadline_at_ms>?5)", turso::params![id, value, wake_at_ms, now(), now_ms()]).await.map_err(|e| e.to_string())?;
        if changed != 1 {
            return Err(format!("instance not active: {id}"));
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        if changed_requests {
            self.changed(id);
        }
        Ok(true)
    }

    pub async fn resume_wait(
        &self,
        id: &str,
        checkpoint: &GraphCheckpoint,
    ) -> Result<Option<&'static str>, String> {
        let value = serde_json::to_string(checkpoint).map_err(|e| e.to_string())?;
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        let changed = tx.execute("UPDATE instances SET status=CASE WHEN attempt>0 THEN 'running' ELSE 'pending' END, checkpoint=?2, wake_at_ms=NULL, updated_at=?3 WHERE id=?1 AND status='waiting' AND wake_at_ms<=?4 AND (deadline_at_ms IS NULL OR deadline_at_ms>?4)", turso::params![id, value, now(), now_ms()]).await.map_err(|e| e.to_string())?;
        if changed != 1 {
            return Ok(None);
        }
        let changed_requests = Self::sync_requests(&tx, id, checkpoint).await?;
        tx.commit().await.map_err(|e| e.to_string())?;
        if changed_requests {
            self.changed(id);
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
        if status == "failed" {
            tx.execute("UPDATE input_requests SET status='retired' WHERE instance_id=?1 AND status='outstanding'", [id])
                .await.map_err(|e| e.to_string())?;
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        if status == "failed" {
            self.changed(id);
        }
        Ok(())
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
                "UPDATE instances SET status=?1, result=?2, error=?3, terminal_name=?4, updated_at=?5 WHERE id=?6 AND status IN ('pending','running','retry-waiting','waiting','waiting-for-input','cancelling') AND (deadline_at_ms IS NULL OR deadline_at_ms>?7 OR status='cancelling')",
                turso::params![status, result.map(|v| v.to_string()), error, terminal_name, now(), id, now_ms()],
            )
            .await
            .map_err(|e| e.to_string())?;
        if affected != 1 {
            return Err(format!("instance not active: {id}"));
        }
        if matches!(
            status,
            "completed" | "cancelled" | "business-error" | "terminated" | "failed"
        ) {
            tx.execute("UPDATE input_requests SET status='retired' WHERE instance_id=?1 AND status='outstanding'", [id])
                .await.map_err(|e| e.to_string())?;
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        if matches!(
            status,
            "completed" | "cancelled" | "business-error" | "terminated" | "failed"
        ) {
            self.changed(id);
        }
        Ok(())
    }

    pub async fn recover_interrupted(&self) -> Result<(), String> {
        let mut conn = self.0.connect().map_err(|e| e.to_string())?;
        let tx = conn.transaction().await.map_err(|e| e.to_string())?;
        tx.execute("UPDATE instances SET status='failed', error='interrupted by server restart', updated_at=?1 WHERE checkpoint IS NULL AND status IN ('pending', 'running', 'cancelling')", [now()])
            .await.map_err(|e| e.to_string())?;
        tx.execute("UPDATE instances SET status='cancelled', updated_at=?1 WHERE checkpoint IS NOT NULL AND status='cancelling'", [now()])
            .await.map_err(|e| e.to_string())?;
        tx.execute("UPDATE input_requests SET status='retired' WHERE status='outstanding' AND instance_id IN (SELECT id FROM instances WHERE status IN ('failed','cancelled','completed','terminated','business-error'))", ())
            .await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())
    }

    pub async fn incomplete(&self) -> Result<Vec<Instance>, String> {
        let conn = self.0.connect().map_err(|e| e.to_string())?;
        let mut rows = conn.query("SELECT id FROM instances WHERE checkpoint IS NOT NULL AND status IN ('pending', 'running', 'retry-waiting', 'waiting', 'waiting-for-input')", ()).await.map_err(|e| e.to_string())?;
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
        let mut rows = conn.query("SELECT id FROM instances WHERE deadline_at_ms IS NOT NULL AND deadline_at_ms<=?1 AND status IN ('pending','running','retry-waiting','waiting','waiting-for-input')", [at_ms]).await.map_err(|e| e.to_string())?;
        let mut ids = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
            ids.push(row.get::<String>(0).map_err(|e| e.to_string())?);
        }
        drop(rows);
        let mut expired = Vec::new();
        for id in ids {
            let mut conn = self.0.connect().map_err(|e| e.to_string())?;
            let tx = conn.transaction().await.map_err(|e| e.to_string())?;
            let changed = tx.execute("UPDATE instances SET status='business-error', terminal_name='timeout', wake_at_ms=NULL, updated_at=?3 WHERE id=?1 AND deadline_at_ms<=?2 AND status IN ('pending','running','retry-waiting','waiting','waiting-for-input')", turso::params![id.as_str(), at_ms, now()]).await.map_err(|e| e.to_string())?;
            if changed == 1 {
                tx.execute("UPDATE input_requests SET status='retired' WHERE instance_id=?1 AND status='outstanding'", [id.as_str()])
                    .await.map_err(|e| e.to_string())?;
            }
            tx.commit().await.map_err(|e| e.to_string())?;
            if changed == 1 {
                self.changed(&id);
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
        Self::get_from(&conn, id).await
    }

    async fn get_from(conn: &turso::Connection, id: &str) -> Result<Option<Instance>, String> {
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
