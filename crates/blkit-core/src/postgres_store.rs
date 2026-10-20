use std::{str::FromStr, time::Duration};

use deadpool_postgres::{Manager, Pool, Runtime};
use tokio_postgres::{Config, NoTls};

use crate::{
    DeadlinePolicy, RetryPolicy,
    compiled_graph::{GraphCheckpoint, InputRequest},
    evaluation::next_retry_at,
    store::Instance,
};

#[derive(Clone)]
pub struct PostgresStore(Pool, Config);

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
    pub requests: Vec<InputRequest>,
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
        Self::connect_with_pool_size(url, 4).await
    }

    pub async fn connect_with_pool_size(url: &str, max: usize) -> Result<Self, String> {
        if max == 0 {
            return Err("postgres pool maximum must be positive".into());
        }
        let config =
            Config::from_str(url).map_err(|e| format!("invalid postgres configuration: {e}"))?;
        let pool = Pool::builder(Manager::new(config.clone(), NoTls))
            .max_size(max)
            .runtime(Runtime::Tokio1)
            .wait_timeout(Some(Duration::from_secs(1)))
            .create_timeout(Some(Duration::from_secs(2)))
            .recycle_timeout(Some(Duration::from_secs(1)))
            .build()
            .map_err(|e| format!("postgres pool configuration: {e}"))?;
        let client = pool
            .get()
            .await
            .map_err(|e| format!("postgres startup checkout: {e}"))?;
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
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS input_resume BOOLEAN NOT NULL DEFAULT FALSE;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS retry_policy TEXT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS queued_at_ms BIGINT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS first_claim_at_ms BIGINT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS wake_at_ms BIGINT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS deadline_origin TEXT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS deadline_duration_ms BIGINT;
            ALTER TABLE instances ADD COLUMN IF NOT EXISTS deadline_at_ms BIGINT;
            UPDATE instances SET queued_at_ms=created_at*1000 WHERE queued_at_ms IS NULL;
            CREATE TABLE IF NOT EXISTS input_requests (
                instance_id TEXT NOT NULL REFERENCES instances(id), request_id TEXT NOT NULL,
                prompt TEXT NOT NULL, schema TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'outstanding' CHECK (status IN ('outstanding','answered','consumed','retired')),
                response TEXT, PRIMARY KEY (instance_id, request_id)
            );
            CREATE INDEX IF NOT EXISTS input_requests_answered ON input_requests(instance_id) WHERE status='answered';
            CREATE TABLE IF NOT EXISTS instance_interests (
                instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
                channel TEXT NOT NULL CHECK (octet_length(channel) BETWEEN 1 AND 63),
                expires_at TIMESTAMPTZ NOT NULL,
                PRIMARY KEY (instance_id, channel)
            );
            CREATE OR REPLACE FUNCTION notify_instance_interests() RETURNS trigger LANGUAGE plpgsql AS $$
            DECLARE changed_id TEXT;
            BEGIN
                IF TG_TABLE_NAME = 'instances' THEN
                    IF NEW.status NOT IN ('completed', 'business-error', 'cancelled', 'terminated', 'failed')
                        OR NEW.status = OLD.status THEN
                        RETURN NEW;
                    END IF;
                    changed_id := NEW.id;
                ELSE
                    IF TG_OP = 'INSERT' THEN
                        IF NEW.status <> 'outstanding' THEN RETURN NEW; END IF;
                    ELSIF OLD.status <> 'outstanding' OR NEW.status NOT IN ('answered', 'retired') THEN
                        RETURN NEW;
                    END IF;
                    changed_id := NEW.instance_id;
                END IF;
                PERFORM pg_notify(interest.channel, changed_id)
                    FROM instance_interests interest
                    WHERE interest.instance_id = changed_id AND interest.expires_at > clock_timestamp();
                RETURN NEW;
            END $$;
            DO $$ BEGIN
                IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='input_requests'::regclass AND tgname='input_request_interests_notify') THEN
                    CREATE TRIGGER input_request_interests_notify
                        AFTER INSERT OR UPDATE OF status ON input_requests
                        FOR EACH ROW EXECUTE FUNCTION notify_instance_interests();
                END IF;
                IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='instances'::regclass AND tgname='instance_terminal_interests_notify') THEN
                    CREATE TRIGGER instance_terminal_interests_notify
                        AFTER UPDATE OF status ON instances
                        FOR EACH ROW EXECUTE FUNCTION notify_instance_interests();
                END IF;
            END $$;
            CREATE TABLE IF NOT EXISTS workers (
                id TEXT PRIMARY KEY, heartbeat_at BIGINT NOT NULL, draining BOOLEAN NOT NULL DEFAULT FALSE
            );
            CREATE TABLE IF NOT EXISTS worker_capabilities (
                worker_id TEXT NOT NULL REFERENCES workers(id) ON DELETE CASCADE,
                namespace TEXT NOT NULL, version TEXT NOT NULL, process TEXT NOT NULL,
                PRIMARY KEY (worker_id, namespace, version, process)
            );
            CREATE TABLE IF NOT EXISTS process_policies (
                namespace TEXT NOT NULL, version TEXT NOT NULL, process TEXT NOT NULL,
                retry_policy TEXT, deadline_origin TEXT, deadline_duration_ms BIGINT,
                PRIMARY KEY (namespace, version, process)
            );",
        ).await.map_err(|e| e.to_string())?;
        Ok(Self(pool, config))
    }

    pub(crate) fn listener_config(&self) -> Config {
        self.1.clone()
    }

    // The instance lock serializes registration with worker and terminal updates before snapshot.
    pub(crate) async fn register_interest(&self, id: &str, channel: &str) -> Result<bool, String> {
        self.register_interest_inner(id, channel, false)
            .await?
            .ok_or("interest lock timeout".into())
    }

    // A timed-out renewal must roll back on the server, freeing the pool for other instances.
    pub(crate) async fn renew_interest(
        &self,
        id: &str,
        channel: &str,
    ) -> Result<Option<bool>, String> {
        self.register_interest_inner(id, channel, true).await
    }

    async fn register_interest_inner(
        &self,
        id: &str,
        channel: &str,
        renewal: bool,
    ) -> Result<Option<bool>, String> {
        let mut client = self.0.get().await.map_err(|e| e.to_string())?;
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        if renewal {
            tx.batch_execute("SET LOCAL lock_timeout = '750ms'")
                .await
                .map_err(|e| e.to_string())?;
        }
        let registered = async {
            if tx.query_opt("SELECT id FROM instances WHERE id=$1 FOR SHARE", &[&id]).await?.is_none() {
                return Ok(None);
            }
            let live = tx.query_opt("SELECT expires_at > clock_timestamp() FROM instance_interests WHERE instance_id=$1 AND channel=$2 FOR UPDATE", &[&id, &channel]).await?
                .is_some_and(|row| row.get::<_, bool>(0));
            tx.execute("INSERT INTO instance_interests (instance_id, channel, expires_at) VALUES ($1,$2,clock_timestamp() + interval '10 seconds') ON CONFLICT (instance_id, channel) DO UPDATE SET expires_at=EXCLUDED.expires_at", &[&id, &channel]).await?;
            Ok::<_, tokio_postgres::Error>(Some(live))
        }.await;
        match registered {
            Ok(Some(live)) => {
                tx.commit().await.map_err(|e| e.to_string())?;
                Ok(Some(live))
            }
            Ok(None) => Err("unknown instance".into()),
            Err(error)
                if renewal
                    && error.code()
                        == Some(&tokio_postgres::error::SqlState::LOCK_NOT_AVAILABLE) =>
            {
                tx.rollback().await.map_err(|e| e.to_string())?;
                Ok(None)
            }
            Err(error) => Err(error.to_string()),
        }
    }

    pub(crate) async fn remove_interest(&self, id: &str, channel: &str) -> Result<(), String> {
        self.0
            .get()
            .await
            .map_err(|e| e.to_string())?
            .execute(
                "DELETE FROM instance_interests WHERE instance_id=$1 AND channel=$2",
                &[&id, &channel],
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
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
        let mut client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO instances (id, namespace, version, process, input, status, result, error, created_at, updated_at, checkpoint, attempt, first_failure_at, next_eligible_at, terminal_name, retry_policy, queued_at_ms, first_claim_at_ms, wake_at_ms, deadline_origin, deadline_duration_ms, deadline_at_ms)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22)",
            &[&instance.id, &instance.namespace, &instance.version, &instance.process,
              &instance.input.to_string(), &instance.status, &result, &instance.error,
              &instance.created_at, &instance.updated_at, &checkpoint, &attempt,
              &instance.first_failure_at, &instance.next_eligible_at, &instance.terminal_name, &retry_policy,
              &instance.queued_at_ms, &instance.first_claim_at_ms, &instance.wake_at_ms,
              &instance.deadline_origin, &instance.deadline_duration_ms, &instance.deadline_at_ms]
        ).await.map_err(|e| e.to_string())?;
        if let Some(checkpoint) = &instance.checkpoint {
            Self::sync_requests(&tx, &instance.id, checkpoint).await?;
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn admit(
        &self,
        id: &str,
        namespace: &str,
        version: &str,
        process: &str,
        input: &serde_json::Value,
    ) -> Result<(), String> {
        let json = input.to_string();
        let inserted = self.0.get().await.map_err(|e| format!("postgres checkout: {e}"))?.query_opt(
            "WITH stamp AS (SELECT (EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT AS now_ms)
             INSERT INTO instances (id, namespace, version, process, input, status, created_at, updated_at, retry_policy,
                                    queued_at_ms, deadline_origin, deadline_duration_ms, deadline_at_ms)
             SELECT $1, p.namespace, p.version, p.process, $5, 'pending', stamp.now_ms/1000, stamp.now_ms/1000,
                    p.retry_policy, stamp.now_ms, p.deadline_origin, p.deadline_duration_ms,
                    CASE WHEN p.deadline_origin='queued' THEN stamp.now_ms+p.deadline_duration_ms END
             FROM process_policies p CROSS JOIN stamp
             WHERE p.namespace=$2 AND p.version=$3 AND p.process=$4
               AND EXISTS (
                   SELECT 1 FROM worker_capabilities c JOIN workers w ON w.id=c.worker_id
                   WHERE c.namespace=p.namespace AND c.version=p.version AND c.process=p.process
                     AND NOT w.draining AND w.heartbeat_at >= stamp.now_ms - 30000
               )
             RETURNING id",
            &[&id, &namespace, &version, &process, &json],
        ).await.map_err(|e| e.to_string())?;
        inserted.map(|_| ()).ok_or("worker unavailable".into())
    }

    async fn sync_requests(
        tx: &tokio_postgres::Transaction<'_>,
        id: &str,
        checkpoint: &GraphCheckpoint,
    ) -> Result<(), String> {
        let requests = checkpoint.requests();
        let ids: Vec<&str> = requests.iter().map(|request| request.id.as_str()).collect();
        for request in &requests {
            tx.execute(
                "INSERT INTO input_requests (instance_id, request_id, prompt, schema) VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING",
                &[&id, &request.id, &request.prompt, &serde_json::to_string(&request.schema).map_err(|e| e.to_string())?],
            ).await.map_err(|e| e.to_string())?;
        }
        tx.execute("UPDATE input_requests SET status='retired' WHERE instance_id=$1 AND status='outstanding' AND NOT (request_id=ANY($2))", &[&id, &ids]).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn requests(&self, id: &str) -> Result<Vec<InputRequest>, String> {
        let client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
        let rows = client.query("SELECT request_id, prompt, schema FROM input_requests WHERE instance_id=$1 AND status='outstanding' ORDER BY request_id", &[&id]).await.map_err(|e| e.to_string())?;
        rows.into_iter()
            .map(|row| {
                Ok(InputRequest {
                    id: row.get(0),
                    prompt: row.get(1),
                    schema: serde_json::from_str(&row.get::<_, String>(2))
                        .map_err(|e| e.to_string())?,
                })
            })
            .collect()
    }

    pub async fn answers(&self, id: &str) -> Result<Vec<(String, serde_json::Value)>, String> {
        let client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
        let rows = client.query("SELECT request_id, response FROM input_requests WHERE instance_id=$1 AND status='answered' ORDER BY request_id", &[&id]).await.map_err(|e| e.to_string())?;
        rows.into_iter()
            .map(|row| {
                Ok((
                    row.get(0),
                    serde_json::from_str(&row.get::<_, String>(1)).map_err(|e| e.to_string())?,
                ))
            })
            .collect()
    }

    pub async fn accept_response(
        &self,
        id: &str,
        request_id: &str,
        response: &serde_json::Value,
    ) -> Result<(), String> {
        let mut client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        let row = tx
            .query_opt(
                "SELECT status, deadline_at_ms FROM instances WHERE id=$1 FOR UPDATE",
                &[&id],
            )
            .await
            .map_err(|e| e.to_string())?
            .ok_or("unknown instance")?;
        let now: i64 = tx
            .query_one(
                "SELECT (EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT",
                &[],
            )
            .await
            .map_err(|e| e.to_string())?
            .get(0);
        if !matches!(
            row.get::<_, String>(0).as_str(),
            "pending" | "running" | "waiting" | "waiting-for-input"
        ) || row.get::<_, Option<i64>>(1).is_some_and(|at| at <= now)
        {
            return Err("instance not active".into());
        }
        let row = tx
            .query_opt(
                "SELECT status, schema FROM input_requests WHERE instance_id=$1 AND request_id=$2",
                &[&id, &request_id],
            )
            .await
            .map_err(|e| e.to_string())?
            .ok_or("unknown request")?;
        if row.get::<_, String>(0) != "outstanding" {
            return Err("request not outstanding".into());
        }
        let schema: crate::compiled_graph::RequestSchema =
            serde_json::from_str(&row.get::<_, String>(1)).map_err(|e| e.to_string())?;
        schema
            .validate_response(response)
            .map_err(|error| format!("invalid response: {error}"))?;
        tx.execute("UPDATE input_requests SET status='answered', response=$3 WHERE instance_id=$1 AND request_id=$2 AND status='outstanding'", &[&id, &request_id, &response.to_string()]).await.map_err(|e| e.to_string())?;
        tx.execute("UPDATE instances SET status='pending', input_resume=TRUE, updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT WHERE id=$1 AND status IN ('waiting-for-input','waiting')", &[&id]).await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())
    }

    pub async fn consume_response(
        &self,
        id: &str,
        worker_id: &str,
        generation: i64,
        request_id: &str,
        checkpoint: &GraphCheckpoint,
    ) -> Result<bool, String> {
        let value = serde_json::to_string(checkpoint).map_err(|e| e.to_string())?;
        let mut client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        let changed = tx.execute("UPDATE instances SET checkpoint=$4, updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT WHERE id=$1 AND owner_id=$2 AND generation=$3 AND status='running' AND lease_until>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT) AND EXISTS (SELECT 1 FROM input_requests WHERE instance_id=$1 AND request_id=$5 AND status='answered')", &[&id, &worker_id, &generation, &value, &request_id]).await.map_err(|e| e.to_string())?;
        if changed != 1 {
            return Ok(false);
        }
        tx.execute("UPDATE input_requests SET status='consumed' WHERE instance_id=$1 AND request_id=$2 AND status='answered'", &[&id, &request_id]).await.map_err(|e| e.to_string())?;
        Self::sync_requests(&tx, id, checkpoint).await?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(true)
    }

    pub async fn release_input_owned(
        &self,
        id: &str,
        worker_id: &str,
        generation: i64,
        checkpoint: &GraphCheckpoint,
    ) -> Result<bool, String> {
        let value = serde_json::to_string(checkpoint).map_err(|e| e.to_string())?;
        let ids: Vec<_> = checkpoint
            .requests()
            .into_iter()
            .map(|request| request.id)
            .collect();
        let mut client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        // Acquire the instance lock before checking replies, so a competing POST's commit is visible.
        tx.query_opt("SELECT id FROM instances WHERE id=$1 FOR UPDATE", &[&id])
            .await
            .map_err(|e| e.to_string())?;
        let changed = tx.execute("UPDATE instances SET status='waiting-for-input', checkpoint=$4, wake_at_ms=NULL, owner_id=NULL, lease_until=NULL, updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT WHERE id=$1 AND owner_id=$2 AND generation=$3 AND status='running' AND lease_until>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT) AND NOT EXISTS (SELECT 1 FROM input_requests WHERE instance_id=$1 AND request_id=ANY($5) AND status='answered')", &[&id, &worker_id, &generation, &value, &ids]).await.map_err(|e| e.to_string())?;
        if changed != 1 {
            return Ok(false);
        }
        Self::sync_requests(&tx, id, checkpoint).await?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(true)
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
        let request_ids: Vec<_> = checkpoint
            .requests()
            .into_iter()
            .map(|request| request.id)
            .collect();
        let mut client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        // Serialize a reply and the wait handoff before testing answered requests.
        tx.query_opt("SELECT id FROM instances WHERE id=$1 FOR UPDATE", &[&id])
            .await
            .map_err(|e| e.to_string())?;
        let changed = tx.execute(
            "UPDATE instances SET status='waiting', checkpoint=$4, wake_at_ms=$5, owner_id=NULL, lease_until=NULL, updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT WHERE id=$1 AND owner_id=$2 AND generation=$3 AND status='running' AND lease_until>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT) AND NOT EXISTS (SELECT 1 FROM input_requests WHERE instance_id=$1 AND request_id=ANY($6) AND status='answered')",
            &[&id, &worker_id, &generation, &value, &wake_at_ms, &request_ids]
        ).await.map_err(|e| e.to_string())?;
        if changed != 1 {
            return Ok(false);
        }
        Self::sync_requests(&tx, id, checkpoint).await?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(true)
    }

    pub async fn expire_due(&self) -> Result<Vec<String>, String> {
        let mut client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        let rows = tx.query(
            "UPDATE instances SET status='business-error', terminal_name='timeout', wake_at_ms=NULL, owner_id=NULL, lease_until=NULL, updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT WHERE deadline_at_ms IS NOT NULL AND deadline_at_ms<=(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT AND status IN ('pending','running','retry-waiting','waiting','waiting-for-input') RETURNING id",
            &[]
        ).await.map_err(|e| e.to_string())?;
        let ids: Vec<String> = rows.iter().map(|row| row.get(0)).collect();
        tx.execute("UPDATE input_requests SET status='retired' WHERE instance_id=ANY($1) AND status='outstanding'", &[&ids]).await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(ids)
    }

    pub async fn resume_due_waits(&self) -> Result<u64, String> {
        self.0.get().await.map_err(|e| format!("postgres checkout: {e}"))?.execute(
            "UPDATE instances SET status='pending' WHERE status='waiting' AND owner_id IS NULL AND wake_at_ms<=(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT",
            &[]
        ).await.map_err(|e| e.to_string())
    }

    pub async fn cancel(&self, id: &str) -> Result<(), String> {
        let mut client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        let changed = tx
            .execute(
                "UPDATE instances SET status='cancelled', owner_id=NULL, lease_until=NULL,
                updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
             WHERE id=$1 AND status IN ('pending', 'running', 'retry-waiting', 'waiting', 'waiting-for-input', 'cancelling') AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)",
                &[&id],
            )
            .await
            .map_err(|e| e.to_string())?;
        if changed == 1 {
            tx.execute("UPDATE input_requests SET status='retired' WHERE instance_id=$1 AND status='outstanding'", &[&id]).await.map_err(|e| e.to_string())?;
            tx.commit().await.map_err(|e| e.to_string())?;
            return Ok(());
        }
        match tx
            .query_opt("SELECT status FROM instances WHERE id=$1", &[&id])
            .await
            .map_err(|e| e.to_string())?
        {
            None => Err("unknown instance".into()),
            Some(row) if row.get::<_, String>(0) == "cancelled" => Ok(()),
            Some(_) => Err("instance already terminal".into()),
        }
    }

    pub(crate) async fn ensure_supported_incomplete(&self) -> Result<(), String> {
        let rows = self.0.get().await.map_err(|e| format!("postgres checkout: {e}"))?.query(
            "SELECT id, checkpoint FROM instances WHERE status IN ('pending', 'running', 'retry-waiting', 'waiting', 'waiting-for-input') AND checkpoint IS NOT NULL",
            &[],
        ).await.map_err(|e| e.to_string())?;
        for row in rows {
            let id: String = row.get(0);
            let text: String = row.get(1);
            let checkpoint: GraphCheckpoint =
                serde_json::from_str(&text).map_err(|e| format!("{id}: {e}"))?;
            checkpoint
                .ensure_supported()
                .map_err(|e| format!("{id}: {e}"))?;
        }
        Ok(())
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
        let mut client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        // Lock the worker before instances so drain cannot acknowledge an in-flight claim.
        let worker = tx
            .query_opt(
                "SELECT draining FROM workers WHERE id=$1 FOR SHARE",
                &[&worker_id],
            )
            .await
            .map_err(|e| e.to_string())?;
        if !matches!(worker, Some(row) if !row.get::<_, bool>(0)) {
            return Ok(Vec::new());
        }
        let eligible = tx.query(
            "SELECT i.id, i.checkpoint FROM instances i
                JOIN worker_capabilities c ON (i.namespace, i.version, i.process) = (c.namespace, c.version, c.process)
                JOIN workers w ON w.id=c.worker_id
                WHERE c.worker_id=$1 AND NOT w.draining AND i.owner_id IS NULL
                    AND (i.deadline_at_ms IS NULL OR i.deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)
                    AND (i.status='pending' OR (i.status='retry-waiting' AND i.next_eligible_at <= (EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT))
                ORDER BY i.created_at, i.id LIMIT $2 FOR UPDATE OF i SKIP LOCKED",
            &[&worker_id, &limit]
        ).await.map_err(|e| e.to_string())?;
        let mut ids = Vec::with_capacity(eligible.len());
        for row in eligible {
            let id: String = row.get(0);
            if let Some(text) = row.get::<_, Option<String>>(1) {
                let checkpoint: GraphCheckpoint =
                    serde_json::from_str(&text).map_err(|e| e.to_string())?;
                checkpoint
                    .ensure_supported()
                    .map_err(|e| format!("{id}: {e}"))?;
            }
            ids.push(id);
        }
        let rows = tx.query(
            "UPDATE instances i SET status='running', owner_id=$1,
                lease_until=(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT+$2,
                generation=generation+1, attempt=CASE WHEN i.status='pending' AND (i.wake_at_ms IS NOT NULL OR i.input_resume) AND i.attempt>0 THEN i.attempt ELSE i.attempt+1 END, input_resume=FALSE, next_eligible_at=NULL, wake_at_ms=NULL,
                first_claim_at_ms=COALESCE(first_claim_at_ms, (EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT),
                deadline_at_ms=CASE WHEN deadline_origin='first_claimed' THEN COALESCE(deadline_at_ms, (EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT + deadline_duration_ms) ELSE deadline_at_ms END,
                updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
            WHERE i.id=ANY($3) AND (i.deadline_at_ms IS NULL OR i.deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT) RETURNING i.id",
            &[&worker_id, &lease_ms, &ids]
        ).await.map_err(|e| e.to_string())?;
        let ids: Vec<String> = rows.iter().map(|row| row.get(0)).collect();
        tx.commit().await.map_err(|e| e.to_string())?;
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
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?
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
        let changed = self.0.get().await.map_err(|e| format!("postgres checkout: {e}"))?.execute(
            "UPDATE instances SET lease_until=(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT+$4
             WHERE id=$1 AND owner_id=$2 AND generation=$3 AND status='running'
                AND lease_until>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)",
            &[&id, &worker_id, &generation, &lease_ms]
        ).await.map_err(|e| e.to_string())?;
        Ok(changed == 1)
    }

    pub async fn commit_initial_checkpoint(
        &self,
        id: &str,
        worker_id: &str,
        generation: i64,
        input: &serde_json::Value,
        checkpoint: &GraphCheckpoint,
    ) -> Result<bool, String> {
        let value = serde_json::to_string(checkpoint).map_err(|e| e.to_string())?;
        let mut client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        let changed = tx.execute(
            "UPDATE instances SET input=$4, checkpoint=$5, updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
             WHERE id=$1 AND owner_id=$2 AND generation=$3 AND status='running' AND checkpoint IS NULL
               AND lease_until>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT
               AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)",
            &[&id, &worker_id, &generation, &input.to_string(), &value]
        ).await.map_err(|e| e.to_string())?;
        if changed != 1 {
            return Ok(false);
        }
        Self::sync_requests(&tx, id, checkpoint).await?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(true)
    }

    pub async fn commit_checkpoint(
        &self,
        id: &str,
        worker_id: &str,
        generation: i64,
        checkpoint: &GraphCheckpoint,
    ) -> Result<bool, String> {
        let value = serde_json::to_string(checkpoint).map_err(|e| e.to_string())?;
        if checkpoint.requests().is_empty() {
            let client = self
                .0
                .get()
                .await
                .map_err(|e| format!("postgres checkout: {e}"))?;
            let row = client.query_one(
                "WITH saved AS (
                    UPDATE instances SET checkpoint=$4, updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
                    WHERE id=$1 AND owner_id=$2 AND generation=$3 AND status='running'
                        AND lease_until>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT
                        AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)
                    RETURNING id
                ), retired AS (
                    UPDATE input_requests SET status='retired' WHERE instance_id IN (SELECT id FROM saved) AND status='outstanding'
                    RETURNING request_id
                ) SELECT count(*) FROM saved", &[&id, &worker_id, &generation, &value]
            ).await.map_err(|e| e.to_string())?;
            return Ok(row.get::<_, i64>(0) == 1);
        }
        let mut client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        let changed = tx.execute(
            "UPDATE instances SET checkpoint=$4, updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
             WHERE id=$1 AND owner_id=$2 AND generation=$3 AND status='running'
               AND lease_until>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)",
            &[&id, &worker_id, &generation, &value]
        ).await.map_err(|e| e.to_string())?;
        if changed != 1 {
            return Ok(false);
        }
        Self::sync_requests(&tx, id, checkpoint).await?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(true)
    }

    pub async fn fail_owned(
        &self,
        id: &str,
        worker_id: &str,
        generation: i64,
        policy: Option<&RetryPolicy>,
        error: &str,
    ) -> Result<Option<&'static str>, String> {
        let mut client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
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
        if status == "failed" {
            tx.execute("UPDATE input_requests SET status='retired' WHERE instance_id=$1 AND status='outstanding'", &[&id]).await.map_err(|e| e.to_string())?;
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
        let mut client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        let changed = tx.execute(
            "UPDATE instances SET status=$4, result=$5, terminal_name=$6, error=$7,
                owner_id=NULL, lease_until=NULL, updated_at=EXTRACT(EPOCH FROM clock_timestamp())::BIGINT
             WHERE id=$1 AND owner_id=$2 AND generation=$3 AND status='running'
               AND lease_until>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT AND (deadline_at_ms IS NULL OR deadline_at_ms>(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)",
            &[&id, &worker_id, &generation, &status, &result, &terminal_name, &error]
        ).await.map_err(|e| e.to_string())?;
        if changed != 1 {
            return Ok(false);
        }
        tx.execute("UPDATE input_requests SET status='retired' WHERE instance_id=$1 AND status='outstanding'", &[&id]).await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(true)
    }

    pub async fn expired_claims(&self) -> Result<Vec<DistributedInstance>, String> {
        let rows = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?
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
        let mut client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
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
        if status == "failed" {
            tx.execute("UPDATE input_requests SET status='retired' WHERE instance_id=$1 AND status='outstanding'", &[&id]).await.map_err(|e| e.to_string())?;
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(Some(status))
    }

    pub async fn due_waits(&self, at_ms: i64) -> Result<Vec<DistributedInstance>, String> {
        let rows = self.0.get().await.map_err(|e| format!("postgres checkout: {e}"))?.query("SELECT id FROM instances WHERE status='waiting' AND wake_at_ms<=$1 ORDER BY wake_at_ms", &[&at_ms]).await.map_err(|e| e.to_string())?;
        let mut due = Vec::new();
        for row in rows {
            if let Some(item) = self.get(row.get::<_, String>(0).as_str()).await? {
                due.push(item);
            }
        }
        Ok(due)
    }

    pub async fn get(&self, id: &str) -> Result<Option<DistributedInstance>, String> {
        let client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
        let row = client.query_opt(
                "SELECT id, namespace, version, process, input, status, result, error,
                    created_at, updated_at, checkpoint, attempt, first_failure_at,
                    next_eligible_at, terminal_name, owner_id, lease_until, generation,
                    queued_at_ms, first_claim_at_ms, wake_at_ms, deadline_origin, deadline_duration_ms, deadline_at_ms,
                    r.request_ids, r.prompts, r.schemas
             FROM instances i LEFT JOIN LATERAL (
                 SELECT array_agg(request_id ORDER BY request_id) AS request_ids,
                        array_agg(prompt ORDER BY request_id) AS prompts,
                        array_agg(schema ORDER BY request_id) AS schemas
                 FROM input_requests WHERE instance_id=i.id AND status='outstanding'
             ) r ON TRUE WHERE i.id=$1",
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
        let requests = row
            .get::<_, Option<Vec<String>>>(24)
            .unwrap_or_default()
            .into_iter()
            .zip(row.get::<_, Option<Vec<String>>>(25).unwrap_or_default())
            .zip(row.get::<_, Option<Vec<String>>>(26).unwrap_or_default())
            .map(|((id, prompt), schema)| {
                Ok(InputRequest {
                    id,
                    prompt,
                    schema: serde_json::from_str(&schema).map_err(|e| e.to_string())?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let item = DistributedInstance {
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
            requests,
        };
        Ok(Some(item))
    }

    pub async fn register_worker(
        &self,
        id: &str,
        identities: &[(&str, &str, &str)],
    ) -> Result<(), String> {
        let policies: Vec<_> = identities
            .iter()
            .map(|&(n, v, p)| (n, v, p, None, None))
            .collect();
        self.register_worker_with_policies(id, &policies).await
    }

    pub(crate) async fn register_worker_with_policies(
        &self,
        id: &str,
        policies: &[(
            &str,
            &str,
            &str,
            Option<&RetryPolicy>,
            Option<&DeadlinePolicy>,
        )],
    ) -> Result<(), String> {
        let mut client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
        let tx = client.transaction().await.map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO workers (id, heartbeat_at) VALUES ($1, (EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT)", &[&id])
            .await.map_err(|e| e.to_string())?;
        for &(namespace, version, process, retry, deadline) in policies {
            let retry_policy = retry
                .map(|policy| -> Result<String, String> {
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
            let origin = deadline.map(|policy| policy.origin);
            let duration = deadline
                .map(|policy| i64::try_from(policy.duration.as_millis()).map_err(|e| e.to_string()))
                .transpose()?;
            if duration.is_some_and(|ms| ms > i64::MAX / 2) {
                return Err("deadline duration exceeds representable range".into());
            }
            tx.execute(
                "INSERT INTO process_policies VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING",
                &[
                    &namespace,
                    &version,
                    &process,
                    &retry_policy,
                    &origin,
                    &duration,
                ],
            )
            .await
            .map_err(|e| e.to_string())?;
            let row = tx.query_one(
                "SELECT retry_policy, deadline_origin, deadline_duration_ms FROM process_policies WHERE namespace=$1 AND version=$2 AND process=$3 FOR UPDATE",
                &[&namespace, &version, &process],
            ).await.map_err(|e| e.to_string())?;
            if row.get::<_, Option<String>>(0) != retry_policy
                || row.get::<_, Option<String>>(1).as_deref() != origin
                || row.get::<_, Option<i64>>(2) != duration
            {
                return Err(format!(
                    "conflicting policy for {namespace}/{version}/{process}"
                ));
            }
            tx.execute(
                "INSERT INTO worker_capabilities VALUES ($1,$2,$3,$4)",
                &[&id, &namespace, &version, &process],
            )
            .await
            .map_err(|e| e.to_string())?;
        }
        tx.commit().await.map_err(|e| e.to_string())
    }

    pub async fn drain(&self, id: &str) -> Result<bool, String> {
        let changed = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?
            .execute("UPDATE workers SET draining=TRUE WHERE id=$1", &[&id])
            .await
            .map_err(|e| e.to_string())?;
        Ok(changed == 1)
    }

    pub async fn unregister_drained(&self, id: &str) -> Result<bool, String> {
        let changed = self.0.get().await.map_err(|e| format!("postgres checkout: {e}"))?.execute(
            "DELETE FROM workers w WHERE id=$1 AND draining
                AND NOT EXISTS (SELECT 1 FROM instances i WHERE i.owner_id=w.id AND i.status='running')", &[&id]
        ).await.map_err(|e| e.to_string())?;
        Ok(changed == 1)
    }

    pub async fn heartbeat(&self, id: &str) -> Result<bool, String> {
        let changed = self.0.get().await.map_err(|e| format!("postgres checkout: {e}"))?.execute(
            "UPDATE workers SET heartbeat_at=(EXTRACT(EPOCH FROM clock_timestamp())*1000)::BIGINT WHERE id=$1", &[&id]
        ).await.map_err(|e| e.to_string())?;
        Ok(changed == 1)
    }

    pub async fn get_worker(&self, id: &str) -> Result<Option<Worker>, String> {
        let client = self
            .0
            .get()
            .await
            .map_err(|e| format!("postgres checkout: {e}"))?;
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

#[cfg(test)]
mod pool_tests {
    use super::PostgresStore;
    use testcontainers_modules::{
        postgres::Postgres,
        testcontainers::{ImageExt, runners::AsyncRunner},
    };

    #[tokio::test]
    async fn zero_pool_size_is_rejected_before_connecting() {
        let error = PostgresStore::connect_with_pool_size("postgres://127.0.0.1:1/unused", 0)
            .await
            .err()
            .expect("zero must be rejected");
        assert!(error.contains("positive"), "{error}");
    }

    #[tokio::test]
    async fn unavailable_database_prevents_startup() {
        let error = PostgresStore::connect_with_pool_size("postgres://127.0.0.1:1/unused", 2)
            .await
            .err()
            .expect("connection must be checked during startup");
        assert!(error.contains("postgres"), "{error}");
    }

    #[tokio::test]
    async fn constructor_initializes_schema_with_one_lazy_session() {
        let node = Postgres::default()
            .with_tag("17.6-alpine")
            .start()
            .await
            .unwrap();
        let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
            .unwrap_or(node.get_host().await.unwrap().to_string());
        let url = format!(
            "postgres://postgres:postgres@{host}:{}/postgres?application_name=blkit_constructor_test",
            node.get_host_port_ipv4(5432).await.unwrap()
        );
        let store = PostgresStore::connect_with_pool_size(&url, 4)
            .await
            .unwrap();
        let (observer, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
            .await
            .unwrap();
        tokio::spawn(async move { connection.await.unwrap() });
        let row = observer
            .query_one(
                "SELECT to_regclass('instances') IS NOT NULL, to_regclass('workers') IS NOT NULL",
                &[],
            )
            .await
            .unwrap();
        assert!(row.get::<_, bool>(0) && row.get::<_, bool>(1));
        let row = observer
            .query_one(
                "SELECT count(*) FROM pg_stat_activity WHERE application_name='blkit_constructor_test'",
                &[],
            )
            .await
            .unwrap();
        // Observer uses the same application name; only one store session is open.
        assert_eq!(row.get::<_, i64>(0), 2);
        assert!(store.get_worker("absent").await.unwrap().is_none());
        let default_store = PostgresStore::connect(&url).await.unwrap();
        assert_eq!(default_store.0.status().max_size, 4);
    }
}
