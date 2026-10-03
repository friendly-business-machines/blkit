use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::{sync::Semaphore, task::JoinSet};

use crate::{
    named_runtime::GraphDefinition,
    postgres_store::{DistributedInstance, PostgresStore},
    runtime::{Instance, NamedRegistry, Registry, execute_claimed, validate_named_registry},
};
use serde_json::Value;

pub struct DistributedControl {
    store: PostgresStore,
    registry: Registry,
}

impl DistributedControl {
    pub fn new(store: PostgresStore, definitions: Vec<GraphDefinition>) -> Result<Self, String> {
        Ok(Self {
            store,
            registry: Registry::new_named(definitions)?,
        })
    }

    pub async fn start(
        &self,
        namespace: &str,
        version: &str,
        name: &str,
        input: Value,
    ) -> Result<String, String> {
        let graph = self
            .registry
            .get_named(namespace, version, name)
            .ok_or("unknown process")?;
        let input =
            (graph.decode_input)(input).map_err(|error| format!("invalid input: {error}"))?;
        let id = uuid::Uuid::new_v4().to_string();
        let mut instance = Instance::new(&id, namespace, version, name, input);
        if let Some(policy) = &graph.deadline {
            instance.with_deadline(policy)?;
        }
        let mut checkpoint = graph.checkpoint(&instance.input)?;
        graph.resume_due(&instance.input, &mut checkpoint, crate::store::now_ms())?;
        if graph.ready(&checkpoint).is_empty()
            && !graph.has_pending(&checkpoint)
            && let Some(wake) = graph.waiting_until(&checkpoint)
        {
            instance.status = "waiting".into();
            instance.wake_at_ms = Some(wake);
        }
        instance.checkpoint = Some(checkpoint);
        self.store
            .create_with_policy(&instance, graph.retry.as_ref())
            .await?;
        Ok(id)
    }

    pub async fn status(&self, id: &str) -> Result<Option<DistributedInstance>, String> {
        self.store.get(id).await
    }

    pub async fn cancel(&self, id: &str) -> Result<(), String> {
        self.store.expire_due().await?;
        self.store.cancel(id).await
    }

    pub async fn reconcile_once(&self) -> Result<usize, String> {
        let mut reconciled = self.store.expire_due().await?.len();
        reconciled += self.store.resume_due_waits().await? as usize;
        for expired in self.store.expired_claims().await? {
            let graph = self.registry.get_named(
                &expired.instance.namespace,
                &expired.instance.version,
                &expired.instance.process,
            );
            if self
                .store
                .reconcile_expired(
                    &expired.instance.id,
                    graph.as_ref().and_then(|graph| graph.retry.as_ref()),
                )
                .await?
                .is_some()
            {
                reconciled += 1;
            }
        }
        Ok(reconciled)
    }
}

pub struct DistributedWorker {
    store: PostgresStore,
    id: String,
    definitions: Arc<NamedRegistry>,
    permits: Arc<Semaphore>,
    limit: usize,
    lease_ms: i64,
}

impl DistributedWorker {
    pub fn new(
        store: PostgresStore,
        id: &str,
        definitions: Vec<GraphDefinition>,
        limit: usize,
        lease_ms: i64,
    ) -> Result<Self, String> {
        if limit == 0 || lease_ms <= 0 {
            return Err("task limit and lease duration must be positive".into());
        }
        let mut entries = HashMap::new();
        for graph in definitions {
            let key = (
                graph.namespace.into(),
                graph.version.into(),
                graph.name.into(),
            );
            if entries.insert(key, Arc::new(graph)).is_some() {
                return Err("duplicate process identity".into());
            }
        }
        validate_named_registry(&entries)?;
        Ok(Self {
            store,
            id: id.into(),
            definitions: Arc::new(entries),
            permits: Arc::new(Semaphore::new(limit)),
            limit,
            lease_ms,
        })
    }

    pub async fn advertise(&self) -> Result<(), String> {
        let identities: Vec<_> = self
            .definitions
            .values()
            .map(|graph| (graph.namespace, graph.version, graph.name))
            .collect();
        self.store.register_worker(&self.id, &identities).await
    }

    pub async fn drain_if_requested(&self) -> Result<bool, String> {
        let worker = self
            .store
            .get_worker(&self.id)
            .await?
            .ok_or("worker not registered")?;
        if !worker.draining {
            return Ok(false);
        }
        if !self.store.unregister_drained(&self.id).await? {
            return Err("draining worker still owns instances".into());
        }
        Ok(true)
    }

    pub async fn run_once(&self) -> Result<usize, String> {
        if !self.store.heartbeat(&self.id).await? {
            return Err("worker not registered".into());
        }
        for expired in self.store.expired_claims().await? {
            let key = (
                expired.instance.namespace,
                expired.instance.version,
                expired.instance.process,
            );
            self.store
                .reconcile_expired(
                    &expired.instance.id,
                    self.definitions
                        .get(&key)
                        .and_then(|graph| graph.retry.as_ref()),
                )
                .await?;
        }
        self.store.expire_due().await?;
        self.store.resume_due_waits().await?;
        let claimed = self
            .store
            .claim(&self.id, self.limit, self.lease_ms)
            .await?;
        let count = claimed.len();
        let mut active = HashMap::new();
        let mut tasks = JoinSet::new();
        for instance in claimed {
            let key = (
                instance.instance.namespace.clone(),
                instance.instance.version.clone(),
                instance.instance.process.clone(),
            );
            let graph = self
                .definitions
                .get(&key)
                .ok_or("claimed process is not linked into worker")?
                .clone();
            let id = instance.instance.id.clone();
            active.insert(id.clone(), instance.generation);
            let store = self.store.clone();
            let worker_id = self.id.clone();
            let permits = self.permits.clone();
            let definitions = self.definitions.clone();
            tasks.spawn(async move {
                (
                    id,
                    execute_claimed(graph, instance, store, worker_id, permits, definitions).await,
                )
            });
        }
        let mut heartbeat =
            tokio::time::interval(Duration::from_millis((self.lease_ms / 3).max(1) as u64));
        while !active.is_empty() {
            tokio::select! {
                _ = heartbeat.tick() => {
                    if !self.store.heartbeat(&self.id).await? { return Err("worker registration lost".into()); }
                    for (id, generation) in &active {
                        if !self.store.renew_claim(id, &self.id, *generation, self.lease_ms).await? {
                            tracing::error!(worker_id = %self.id, instance_id = %id, "instance claim lost");
                        }
                    }
                }
                completed = tasks.join_next() => {
                    let (id, result) = completed.ok_or("missing worker task")?.map_err(|e| e.to_string())?;
                    active.remove(&id);
                    if let Err(error) = result {
                        let reason = if error == "lost claim during execution" {
                            "claim lost during execution"
                        } else {
                            "execution infrastructure failure"
                        };
                        tracing::error!(worker_id = %self.id, instance_id = %id, reason, "instance execution stopped");
                    }
                }
            }
        }
        Ok(count)
    }
}
