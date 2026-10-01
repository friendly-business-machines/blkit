use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_postgres::{Client, NoTls};

use crate::{
    RetryPolicy,
    named_runtime::GraphCheckpoint,
    runtime::{Instance, next_retry_at},
};

#[derive(Clone)]
pub struct PostgresStore(Arc<Mutex<Client>>);

pub(crate) enum ClaimActivity {
    Active,
    Cancelled,
    Lost,
}

#[derive(serde::Serialize)]
pub struct DistributedInstance {
    #[serde(flatten)]
    pub instance: Instance,
    pub owner_id: Option<String>,
    pub lease_until: Option<i64>,
    pub generation: i64,
}

pub struct Worker {
    pub id: String,
    pub heartbeat_at: i64,
    pub draining: bool,
    pub identities: Vec<(String, String, String)>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredRetryPolicy {
    max_retries: u32,
    retry_for_ms: u64,
    retry_delay_ms: u64,
}

fn read_retry_policy(text: Option<String>) -> Result<Option<RetryPolicy>, String> {
    text.map(|text| {
        let stored: StoredRetryPolicy = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        Ok(RetryPolicy {
            max_retries: stored.max_retries,
            retry_for: std::time::Duration::from_millis(stored.retry_for_ms),
            retry_delay: std::time::Duration::from_millis(stored.retry_delay_ms),
            backoff: "exponential",
        })
    })
    .transpose()
}

impl PostgresStore {
    pub async fn connect(url: &str) -> Result<Self, String> {
        let (client, connection) = tokio_postgres::connect(url, NoTls)
            .await
            .map_err(|e| e.to_string())?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                eprintln!("postgres connection failed: {error}");
            }
        });
        client.batch_execute(
            "CREATE TABLE IF NOT EXISTS instances (
                id TEXT PRIMARY KEY, namespace TEXT NOT NULL, version TEXT NOT NULL,
                process TEXT NOT NULL, input TEXT NOT NULL, status TEXT NOT NULL,
                result TEXT, error TEXT, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL,
                checkpoint TEXT, attempt BIGINT NOT NULL DEFAULT 0,
                first_failure_at BIGINT, next_eligible_at BIGINT, terminal_name TEXT,
                owner_id TEXT, lease_until BIGINT, generation BIGINT NOT NULL DEFAULT 0,
                retry_policy TEXT,
                queued_at_ms BIGINT, first_claim_at_ms BIGINT, wake_at_ms BIGINT,
                deadline_origin TEXT, deadline_duration_ms BIGINT, deadline_at_ms BIGINT
            );
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS retry_policy TEXT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS queued_at_ms BIGINT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS first_claim_at_ms BIGINT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS wake_at_ms BIGINT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS deadline_origin TEXT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS deadline_duration_ms BIGINT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS deadline_at_ms BIGINT;
            UPDATE instances SET queued_at_ms=created_at*1000 WHERE queued_at_ms IS NULL;
            CREATE TABLE IF NOT EXISTS workers (
                id TEXT PRIMARY KEY, heartbeat_at BIGINT NOT NULL, draining BOOLEAN NOT NULL DEFAULT FALSE
            );
            CREATE TABLE IF NOT EXISTS worker_capabilities (
                worker_id TEXT NOT NULL REFERENCES workers(id) ON DELETE CASCADE,
                namespace TEXT NOT NULL, version TEXT NOT NULL, process TEXT NOT NULL,
                PRIMARY KEY (worker_id, namespace, version, process)
            );",
        ).await.map_err(|e| e.to_string())?;
        Ok(Self(Arc::new(Mutex::new(client))))
    }

    pub async fn create(&self, instance: &Instance) -> Result<(), String> {
        self.create_with_policy(instance, None).await
    }

    pub async fn create_with_policy(
        &self,
        instance: &Instance,
        policy: Option<&RetryPolicy>,
    ) -> Result<(), String> {
        let retry_policy = policy
            .map(|policy| {
                let stored = StoredRetryPolicy {
                    max_retries: policy.max_retries,
                    retry_for_ms: u64::try_from(policy.retry_for.as_millis())
                        .map_err(|e| e.to_string())?,
                    retry_delay_ms: u64::try_from(policy.retry_delay.as_millis())
                        .map_err(|e| e.to_string())?,
                };
                serde_json::to_string(&stored).map_err(|e| e.to_string())
            })
            .transpose()?;
        let result = instance
            .result
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| e.to_string())?;
        let checkpoint = instance
            .checkpoint
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| e.to_string())?;
        let attempt = i64::from(instance.attempt);
        self.0.lock().await.execute(
            "INSERT INTO instances (id, namespace, version, process, input, status, result, error, created_at, updated_at, checkpoint, attempt, first_failure_at, next_eligible_at, terminal_name, retry_policy, queued_at_ms, first_claim_at_ms, wake_at_ms, deadline_origin, deadline_duration_ms, deadline_at_ms)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22)",
            &[&instance.id, &instance.namespace, &instance.version, &instance.process,
              &instance.input.to_string(), &instance.status, &result, &instance.error,
              &instance.created_at, &instance.updated_at, &checkpoint, &attempt,
              &instance.first_failure_at, &instance.next_eligible_at, &instance.terminal_name, &retry_policy,
              &instance.queued_at_ms, &instance.first_claim_at_ms, &instance.wake_at_ms,
              &instance.deadline_origin, &instance.deadline_duration_ms, &instance.deadline_at_ms]
        ).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn release_wait_owned(
        &self,
        id: &str,
        worker_id: &str,
        generation: i64,
        checkpoint: &GraphCheckpoint,
        wake_at_ms: i64,
    ) -> Result<bool, String> {
        let value = serde_json::to_string(checkpoint).map_err(|e| e.to_string())?;
        let changed = self.0.lock().await.execute(
            "UPDATE instances SET status='waiting', checkpoint=$4, wake_at_ms=$5, owner_id=NULL, lease_until=NULL, updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT WHERE id=$1 AND owner_id=$2 AND generation=$3 AND status='running' AND lease_until>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)",
            &[&id, &worker_id, &generation, &value, &wake_at_ms]
        ).await.map_err(|e| e.to_string())?;
        Ok(changed == 1)
    }

    pub async fn expire_due(&self) -> Result<Vec<String>, String> {
        let rows = self.0.lock().await.query(
            "UPDATE instances SET status='business-error', terminal_name='timeout', wake_at_ms=NULL, owner_id=NULL, lease_until=NULL, updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT WHERE deadline_at_ms IS NOT NULL AND deadline_at_ms<=(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT AND status IN ('pending','running','retry-waiting','waiting') RETURNING id",
            &[]
        ).await.map_err(|e| e.to_string())?;
        Ok(rows.iter().map(|row| row.get(0)).collect())
    }

    pub async fn resume_due_waits(&self) -> Result<u64, String> {
        self.0.lock().await.execute(
            "UPDATE instances SET status='pending' WHERE status='waiting' AND owner_id IS NULL AND wake_at_ms<=(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT",
            &[]
        ).await.map_err(|e| e.to_string())
    }

    pub async fn cancel(&self, id: &str) -> Result<(), String> {
        let client = self.0.lock().await;
        let changed = client
            .execute(
                "UPDATE instances SET status='cancelled', owner_id=NULL, lease_until=NULL,
                updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
             WHERE id=$1 AND status IN ('pending', 'running', 'retry-waiting', 'waiting', 'cancelling') AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)",
                &[&id],
            )
            .await
            .map_err(|e| e.to_string())?;
        if changed == 1 {
            return Ok(());
        }
        match client
            .query_opt("SELECT status FROM instances WHERE id=$1", &[&id])
            .await
            .map_err(|e| e.to_string())?
        {
            None => Err("unknown instance".into()),
            Some(row) if row.get::<_, String>(0) == "cancelled" => Ok(()),
            Some(_) => Err("instance already terminal".into()),
        }
    }

    pub async fn claim(
        &self,
        worker_id: &str,
        limit: usize,
        lease_ms: i64,
    ) -> Result<Vec<DistributedInstance>, String> {
        if lease_ms <= 0 {
            return Err("lease must be positive".into());
        }
        let limit = i64::try_from(limit).map_err(|e| e.to_string())?;
        let client = self.0.lock().await;
        let rows = client.query(
            "WITH eligible AS (
                SELECT i.id FROM instances i
                JOIN worker_capabilities c ON (i.namespace, i.version, i.process) = (c.namespace, c.version, c.process)
                JOIN workers w ON w.id=c.worker_id
                WHERE c.worker_id=$1 AND NOT w.draining AND i.owner_id IS NULL
                    AND (i.deadline_at_ms IS NULL OR i.deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)
                    AND (i.status='pending' OR (i.status='retry-waiting' AND i.next_eligible_at <= (EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT))
                ORDER BY i.created_at, i.id LIMIT $2 FOR UPDATE OF i SKIP LOCKED
            )
            UPDATE instances i SET status='running', owner_id=$1,
                lease_until=(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT+$3,
                generation=generation+1, attempt=CASE WHEN i.status='pending' AND i.wake_at_ms IS NOT NULL AND i.attempt>0 THEN i.attempt ELSE i.attempt+1 END, next_eligible_at=NULL, wake_at_ms=NULL,
                first_claim_at_ms=COALESCE(first_claim_at_ms, (EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT),
                deadline_at_ms=CASE WHEN deadline_origin='first_claimed' THEN COALESCE(deadline_at_ms, (EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT + deadline_duration_ms) ELSE deadline_at_ms END,
                updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
            FROM eligible WHERE i.id=eligible.id RETURNING i.id",
            &[&worker_id, &limit, &lease_ms]
        ).await.map_err(|e| e.to_string())?;
        let ids: Vec<String> = rows.iter().map(|row| row.get(0)).collect();
        drop(client);
        let mut claimed = Vec::with_capacity(ids.len());
        for id in ids {
            claimed.push(self.get(&id).await?.ok_or("missing claimed instance")?);
        }
        Ok(claimed)
    }

    pub(crate) async fn claim_activity(
        &self,
        id: &str,
        worker_id: &str,
        generation: i64,
    ) -> Result<ClaimActivity, String> {
        let row = self
            .0
            .lock()
            .await
            .query_opt(
                "SELECT status, COALESCE(owner_id=$2 AND generation=$3 AND
                lease_until>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT), FALSE)
             FROM instances WHERE id=$1",
                &[&id, &worker_id, &generation],
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(match row {
            Some(row) if row.get::<_, String>(0) == "cancelled" => ClaimActivity::Cancelled,
            Some(row) if row.get::<_, String>(0) == "running" && row.get::<_, bool>(1) => {
                ClaimActivity::Active
            }
            _ => ClaimActivity::Lost,
        })
    }

    pub async fn renew_claim(
        &self,
        id: &str,
        worker_id: &str,
        generation: i64,
        lease_ms: i64,
    ) -> Result<bool, String> {
        if lease_ms <= 0 {
            return Err("lease must be positive".into());
        }
        let changed = self.0.lock().await.execute(
            "UPDATE instances SET lease_until=(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT+$4
             WHERE id=$1 AND owner_id=$2 AND generation=$3 AND status='running'
                AND lease_until>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)",
            &[&id, &worker_id, &generation, &lease_ms]
        ).await.map_err(|e| e.to_string())?;
        Ok(changed == 1)
    }

    pub async fn commit_checkpoint(
        &self,
        id: &str,
        worker_id: &str,
        generation: i64,
        checkpoint: &GraphCheckpoint,
    ) -> Result<bool, String> {
        let value = serde_json::to_string(checkpoint).map_err(|e| e.to_string())?;
        let changed = self.0.lock().await.execute(
            "UPDATE instances SET checkpoint=$4, updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
             WHERE id=$1 AND owner_id=$2 AND generation=$3 AND status='running'
               AND lease_until>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)",
            &[&id, &worker_id, &generation, &value]
        ).await.map_err(|e| e.to_string())?;
        Ok(changed == 1)
    }

    pub async fn fail_owned(
        &self,
        id: &str,
        worker_id: &str,
        generation: i64,
        policy: Option<&RetryPolicy>,
        error: &str,
    ) -> Result<Option<&'static str>, String> {
        let mut client = self.0.lock().await;
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        let row = tx
            .query_opt(
                "SELECT attempt, first_failure_at, retry_policy FROM instances
             WHERE id=$1 AND owner_id=$2 AND generation=$3 AND status='running'
               AND lease_until>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)
             FOR UPDATE SKIP LOCKED",
                &[&id, &worker_id, &generation],
            )
            .await
            .map_err(|e| e.to_string())?;
        let Some(row) = row else {
            return Ok(None);
        };
        let now: i64 = tx
            .query_one(
                "SELECT (EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT",
                &[],
            )
            .await
            .map_err(|e| e.to_string())?
            .get(0);
        let first = row.get::<_, Option<i64>>(1).unwrap_or(now);
        let attempt = u32::try_from(row.get::<_, i64>(0)).map_err(|e| e.to_string())?;
        let stored_policy = read_retry_policy(row.get(2))?;
        let next = stored_policy
            .as_ref()
            .or(policy)
            .and_then(|p| next_retry_at(p, attempt, first, now));
        let status = if next.is_some() {
            "retry-waiting"
        } else {
            "failed"
        };
        let changed = tx.execute(
            "UPDATE instances SET status=$2, error=$3, first_failure_at=$4, next_eligible_at=$5,
                owner_id=NULL, lease_until=NULL, updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT WHERE id=$1 AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)",
            &[&id, &status, &error, &first, &next]
        ).await.map_err(|e| e.to_string())?;
        if changed != 1 {
            return Ok(None);
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(Some(status))
    }

    // Keep the ownership and terminal fields explicit at this storage boundary.
    #[allow(clippy::too_many_arguments)]
    pub async fn finish_owned(
        &self,
        id: &str,
        worker_id: &str,
        generation: i64,
        status: &str,
        result: Option<serde_json::Value>,
        terminal_name: Option<&str>,
        error: Option<&str>,
    ) -> Result<bool, String> {
        if !matches!(
            status,
            "completed" | "business-error" | "cancelled" | "terminated" | "failed"
        ) {
            return Err(format!("invalid terminal status: {status}"));
        }
        let result = result
            .map(|value| serde_json::to_string(&value))
            .transpose()
            .map_err(|e| e.to_string())?;
        let changed = self.0.lock().await.execute(
            "UPDATE instances SET status=$4, result=$5, terminal_name=$6, error=$7,
                owner_id=NULL, lease_until=NULL, updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
             WHERE id=$1 AND owner_id=$2 AND generation=$3 AND status='running'
               AND lease_until>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)",
            &[&id, &worker_id, &generation, &status, &result, &terminal_name, &error]
        ).await.map_err(|e| e.to_string())?;
        Ok(changed == 1)
    }

    pub async fn expired_claims(&self) -> Result<Vec<DistributedInstance>, String> {
        let rows = self
            .0
            .lock()
            .await
            .query(
                "SELECT id FROM instances WHERE status='running' AND owner_id IS NOT NULL
                AND lease_until<=(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT
             ORDER BY lease_until LIMIT 100",
                &[],
            )
            .await
            .map_err(|e| e.to_string())?;
        let mut expired = Vec::new();
        for row in rows {
            if let Some(instance) = self.get(row.get::<_, String>(0).as_str()).await? {
                expired.push(instance);
            }
        }
        Ok(expired)
    }

    pub async fn reconcile_expired(
        &self,
        id: &str,
        policy: Option<&RetryPolicy>,
    ) -> Result<Option<&'static str>, String> {
        let mut client = self.0.lock().await;
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        let row = tx
            .query_opt(
                "SELECT attempt, first_failure_at, retry_policy FROM instances
             WHERE id=$1 AND owner_id IS NOT NULL AND status='running'
               AND lease_until<=(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT
             FOR UPDATE SKIP LOCKED",
                &[&id],
            )
            .await
            .map_err(|e| e.to_string())?;
        let Some(row) = row else {
            return Ok(None);
        };
        let now: i64 = tx
            .query_one(
                "SELECT (EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT",
                &[],
            )
            .await
            .map_err(|e| e.to_string())?
            .get(0);
        let first = row.get::<_, Option<i64>>(1).unwrap_or(now);
        let attempt = u32::try_from(row.get::<_, i64>(0)).map_err(|e| e.to_string())?;
        let stored_policy = read_retry_policy(row.get(2))?;
        let next = stored_policy
            .as_ref()
            .or(policy)
            .and_then(|p| next_retry_at(p, attempt, first, now));
        let status = if next.is_some() {
            "retry-waiting"
        } else {
            "failed"
        };
        let changed = tx.execute(
            "UPDATE instances SET status=$2, error='owner lease expired', first_failure_at=$3,
                next_eligible_at=$4, owner_id=NULL, lease_until=NULL,
                updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT WHERE id=$1 AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)",
            &[&id, &status, &first, &next],
        )
        .await
        .map_err(|e| e.to_string())?;
        if changed != 1 {
            return Ok(None);
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(Some(status))
    }

    pub async fn due_waits(&self, at_ms: i64) -> Result<Vec<DistributedInstance>, String> {
        let rows = self.0.lock().await.query("SELECT id FROM instances WHERE status='waiting' AND wake_at_ms<=$1 ORDER BY wake_at_ms", &[&at_ms]).await.map_err(|e| e.to_string())?;
        let mut due = Vec::new();
        for row in rows {
            if let Some(item) = self.get(row.get::<_, String>(0).as_str()).await? {
                due.push(item);
            }
        }
        Ok(due)
    }

    pub async fn get(&self, id: &str) -> Result<Option<DistributedInstance>, String> {
        let row = self
            .0
            .lock()
            .await
            .query_opt(
                "SELECT id, namespace, version, process, input, status, result, error,
                    created_at, updated_at, checkpoint, attempt, first_failure_at,
                    next_eligible_at, terminal_name, owner_id, lease_until, generation,
                    queued_at_ms, first_claim_at_ms, wake_at_ms, deadline_origin, deadline_duration_ms, deadline_at_ms
             FROM instances WHERE id=$1",
                &[&id],
            )
            .await
            .map_err(|e| e.to_string())?;
        let Some(row) = row else {
            return Ok(None);
        };
        let json = |text: String| {
            serde_json::from_str(&text).map_err(|e: serde_json::Error| e.to_string())
        };
        let value = |text: Option<String>| text.map(&json).transpose();
        let attempt: i64 = row.get(11);
        Ok(Some(DistributedInstance {
            instance: Instance {
                id: row.get(0),
                namespace: row.get(1),
                version: row.get(2),
                process: row.get(3),
                input: json(row.get(4))?,
                status: row.get(5),
                result: value(row.get(6))?,
                error: row.get(7),
                created_at: row.get(8),
                updated_at: row.get(9),
                checkpoint: row
                    .get::<_, Option<String>>(10)
                    .map(|text| serde_json::from_str::<GraphCheckpoint>(&text))
                    .transpose()
                    .map_err(|e| e.to_string())?,
                attempt: u32::try_from(attempt).map_err(|e| e.to_string())?,
                first_failure_at: row.get(12),
                next_eligible_at: row.get(13),
                terminal_name: row.get(14),
                queued_at_ms: row.get(18),
                first_claim_at_ms: row.get(19),
                wake_at_ms: row.get(20),
                deadline_origin: row.get(21),
                deadline_duration_ms: row.get(22),
                deadline_at_ms: row.get(23),
            },
            owner_id: row.get(15),
            lease_until: row.get(16),
            generation: row.get(17),
        }))
    }

    pub async fn register_worker(
        &self,
        id: &str,
        identities: &[(&str, &str, &str)],
    ) -> Result<(), String> {
        let mut client = self.0.lock().await;
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO workers (id, heartbeat_at) VALUES ($1, (EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)", &[&id])
            .await.map_err(|e| e.to_string())?;
        for (namespace, version, process) in identities {
            tx.execute(
                "INSERT INTO worker_capabilities VALUES ($1,$2,$3,$4)",
                &[&id, namespace, version, process],
            )
            .await
            .map_err(|e| e.to_string())?;
        }
        tx.commit().await.map_err(|e| e.to_string())
    }

    pub async fn drain(&self, id: &str) -> Result<bool, String> {
        let changed = self
            .0
            .lock()
            .await
            .execute("UPDATE workers SET draining=TRUE WHERE id=$1", &[&id])
            .await
            .map_err(|e| e.to_string())?;
        Ok(changed == 1)
    }

    pub async fn unregister_drained(&self, id: &str) -> Result<bool, String> {
        let changed = self.0.lock().await.execute(
            "DELETE FROM workers w WHERE id=$1 AND draining
                AND NOT EXISTS (SELECT 1 FROM instances i WHERE i.owner_id=w.id AND i.status='running')", &[&id]
        ).await.map_err(|e| e.to_string())?;
        Ok(changed == 1)
    }

    pub async fn heartbeat(&self, id: &str) -> Result<bool, String> {
        let changed = self.0.lock().await.execute(
            "UPDATE workers SET heartbeat_at=(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT WHERE id=$1", &[&id]
        ).await.map_err(|e| e.to_string())?;
        Ok(changed == 1)
    }

    pub async fn get_worker(&self, id: &str) -> Result<Option<Worker>, String> {
        let client = self.0.lock().await;
        let Some(row) = client
            .query_opt(
                "SELECT id, heartbeat_at, draining FROM workers WHERE id=$1",
                &[&id],
            )
            .await
            .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        let identities = client.query(
            "SELECT namespace, version, process FROM worker_capabilities WHERE worker_id=$1 ORDER BY namespace, version, process", &[&id]
        ).await.map_err(|e| e.to_string())?.into_iter()
            .map(|row| (row.get(0), row.get(1), row.get(2))).collect();
        Ok(Some(Worker {
            id: row.get(0),
            heartbeat_at: row.get(1),
            draining: row.get(2),
            identities,
        }))
    }
}
