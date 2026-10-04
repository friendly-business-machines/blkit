use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use serde_json::Value;
use tokio::{
    sync::{Mutex, Semaphore},
    task::JoinSet,
};

use crate::{
    compiled_graph::{
        ChildActivation, GraphCheckpoint, GraphDefinition, GraphNodeKind, GraphTerminal,
    },
    postgres_store::{ClaimActivity, DistributedInstance, PostgresStore},
};

pub use crate::store::Instance;
pub use crate::store::Store as LocalStore;

pub type Values = HashMap<String, Value>;
pub type Evaluate = Arc<dyn Fn(&Value, &Values) -> Result<Value, String> + Send + Sync>;
pub type AsyncEvaluate = Arc<
    dyn Fn(Value, Values) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send>>
        + Send
        + Sync,
>;
pub type Cancel = Arc<dyn Fn() + Send + Sync>;

pub fn next_retry_at(
    policy: &crate::RetryPolicy,
    attempt: u32,
    first_failure_ms: i64,
    now_ms: i64,
) -> Option<i64> {
    if attempt == 0 || attempt > policy.max_retries {
        return None;
    }
    let window_end =
        first_failure_ms.checked_add(i64::try_from(policy.retry_for.as_millis()).ok()?)?;
    let multiplier = 1_i64.checked_shl(attempt - 1)?;
    let delay = i64::try_from(policy.retry_delay.as_millis())
        .ok()?
        .checked_mul(multiplier)?;
    let eligible = now_ms.checked_add(delay)?;
    (eligible <= window_end).then_some(eligible)
}

pub(crate) type NamedRegistry = HashMap<(String, String, String), Arc<GraphDefinition>>;

pub(crate) fn validate_named_registry(entries: &NamedRegistry) -> Result<(), String> {
    fn visit<'a>(
        graph: &'a GraphDefinition,
        entries: &'a NamedRegistry,
        active: &mut HashSet<(&'a str, &'a str, &'a str)>,
        done: &mut HashSet<(&'a str, &'a str, &'a str)>,
    ) -> Result<(), String> {
        let key = (graph.namespace, graph.version, graph.name);
        if done.contains(&key) {
            return Ok(());
        }
        if !active.insert(key) {
            return Err(format!("recursive subprocess call: {}", graph.name));
        }
        for child in graph.child_processes() {
            let definition = entries
                .get(&(graph.namespace.into(), graph.version.into(), child.into()))
                .ok_or_else(|| format!("missing compiled subprocess: {child}"))?;
            visit(definition, entries, active, done)?;
        }
        active.remove(&key);
        done.insert(key);
        Ok(())
    }
    let mut done = HashSet::new();
    for graph in entries.values() {
        visit(graph, entries, &mut HashSet::new(), &mut done)?;
    }
    Ok(())
}

pub struct Registry {
    named: Arc<NamedRegistry>,
}

impl Registry {
    pub fn new(definitions: Vec<GraphDefinition>) -> Result<Self, String> {
        let mut named = HashMap::new();
        for definition in definitions {
            let key = (
                definition.namespace.into(),
                definition.version.into(),
                definition.name.into(),
            );
            if named.insert(key, Arc::new(definition)).is_some() {
                return Err("duplicate process identity".into());
            }
        }
        validate_named_registry(&named)?;
        Ok(Self {
            named: Arc::new(named),
        })
    }

    pub fn get(&self, namespace: &str, version: &str, name: &str) -> Option<Arc<GraphDefinition>> {
        self.named
            .get(&(namespace.into(), version.into(), name.into()))
            .cloned()
    }
}

struct Running {
    status: &'static str,
    next: usize,
    in_flight: HashMap<usize, Cancel>,
}

#[derive(Clone)]
struct Claim {
    store: PostgresStore,
    worker_id: String,
    generation: i64,
}

#[derive(Clone)]
struct Context {
    state: Arc<Mutex<Running>>,
    store: Option<LocalStore>,
    claim: Option<Claim>,
    id: String,
}

impl Context {
    async fn signal_cancel(&self) {
        let mut state = self.state.lock().await;
        if state.status != "running" {
            return;
        }
        state.status = "cancelled";
        let hooks: Vec<_> = state.in_flight.drain().map(|(_, hook)| hook).collect();
        drop(state);
        for hook in hooks {
            hook();
        }
    }

    async fn fail_attempt(
        &self,
        policy: Option<&crate::RetryPolicy>,
        error: &str,
    ) -> Result<Option<i64>, String> {
        let mut state = self.state.lock().await;
        if !matches!(state.status, "pending" | "running") {
            return Ok(None);
        }
        let (status, next) = if let Some(claim) = &self.claim {
            let status = claim
                .store
                .fail_owned(&self.id, &claim.worker_id, claim.generation, policy, error)
                .await?
                .ok_or("lost claim before recording failure")?;
            (status, None)
        } else {
            let store = self.store.as_ref().ok_or("missing instance store")?;
            let instance = store.get(&self.id).await?.ok_or("missing instance")?;
            let first = instance
                .first_failure_at
                .unwrap_or_else(crate::store::now_ms);
            let next = policy.and_then(|policy| {
                next_retry_at(policy, instance.attempt, first, crate::store::now_ms())
            });
            store
                .record_retry(&self.id, instance.attempt, first, next, error)
                .await?;
            (
                if next.is_some() {
                    "retry-waiting"
                } else {
                    "failed"
                },
                next,
            )
        };
        state.status = status;
        let hooks: Vec<_> = state.in_flight.drain().map(|(_, hook)| hook).collect();
        drop(state);
        for hook in hooks {
            hook();
        }
        Ok(next)
    }

    async fn named_terminal(&self, terminal: &GraphTerminal) -> Result<(), String> {
        let (status, name) = match terminal {
            GraphTerminal::Error(name) => ("business-error", name),
            GraphTerminal::Cancel(name) => ("cancelled", name),
            GraphTerminal::Terminate(name) => ("terminated", name),
        };
        let mut state = self.state.lock().await;
        if !matches!(state.status, "pending" | "running") {
            return Ok(());
        }
        if let Some(claim) = &self.claim {
            if !claim
                .store
                .finish_owned(
                    &self.id,
                    &claim.worker_id,
                    claim.generation,
                    status,
                    None,
                    Some(name),
                    None,
                )
                .await?
            {
                return Err("lost claim before terminal write".into());
            }
        } else if let Some(store) = &self.store {
            store
                .finish_named(&self.id, status, name, None, None)
                .await?;
        }
        state.status = status;
        let hooks: Vec<_> = state.in_flight.values().cloned().collect();
        drop(state);
        for hook in hooks {
            hook();
        }
        Ok(())
    }

    async fn complete(&self, value: Value) -> Result<(), String> {
        let mut state = self.state.lock().await;
        if !matches!(state.status, "pending" | "running") {
            return Ok(());
        }
        if let Some(claim) = &self.claim {
            if !claim
                .store
                .finish_owned(
                    &self.id,
                    &claim.worker_id,
                    claim.generation,
                    "completed",
                    Some(value),
                    None,
                    None,
                )
                .await?
            {
                return Err("lost claim before completion".into());
            }
        } else if let Some(store) = &self.store {
            store
                .finish(&self.id, "completed", Some(value), None)
                .await?;
        }
        state.status = "completed";
        Ok(())
    }
}

async fn expire_local_deadlines(
    store: &LocalStore,
    active: &Arc<Mutex<HashMap<String, Context>>>,
) -> Result<(), String> {
    for id in store.expire_due(crate::store::now_ms()).await? {
        let Some(context) = active.lock().await.get(&id).cloned() else {
            continue;
        };
        let mut state = context.state.lock().await;
        state.status = "business-error";
        let hooks: Vec<_> = state.in_flight.drain().map(|(_, hook)| hook).collect();
        drop(state);
        for hook in hooks {
            hook();
        }
    }
    Ok(())
}

pub struct Engine {
    registry: Registry,
    store: LocalStore,
    permits: Arc<Semaphore>,
    active: Arc<Mutex<HashMap<String, Context>>>,
}

impl Engine {
    pub fn new(registry: Registry, store: LocalStore, limit: usize) -> Result<Self, String> {
        if limit == 0 {
            return Err("task limit must be positive".into());
        }
        Ok(Self {
            registry,
            store,
            permits: Arc::new(Semaphore::new(limit)),
            active: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn recover(&self) -> Result<(), String> {
        // Validate before recovery updates statuses or deadlines; old checkpoints stay intact.
        for instance in self.store.incomplete().await? {
            if let Some(checkpoint) = &instance.checkpoint {
                checkpoint.ensure_supported()?;
            }
        }
        expire_local_deadlines(&self.store, &self.active).await?;
        self.store.recover_interrupted().await?;
        for mut instance in self.store.incomplete().await? {
            if self.active.lock().await.contains_key(&instance.id) {
                continue;
            }
            let Some(graph) =
                self.registry
                    .get(&instance.namespace, &instance.version, &instance.process)
            else {
                self.store
                    .finish(
                        &instance.id,
                        "failed",
                        None,
                        Some("compiled process unavailable after restart"),
                    )
                    .await?;
                continue;
            };
            if instance.status == "running" {
                let context = Context {
                    state: Arc::new(Mutex::new(Running {
                        status: "running",
                        next: 0,
                        in_flight: HashMap::new(),
                    })),
                    store: Some(self.store.clone()),
                    claim: None,
                    id: instance.id.clone(),
                };
                context
                    .fail_attempt(graph.retry.as_ref(), "interrupted by server restart")
                    .await?;
                instance = self
                    .store
                    .get(&instance.id)
                    .await?
                    .ok_or("missing instance")?;
            }
            if matches!(
                instance.status.as_str(),
                "pending" | "retry-waiting" | "waiting"
            ) {
                self.spawn_named(graph, instance).await;
            }
        }
        Ok(())
    }

    async fn spawn_named(&self, graph: Arc<GraphDefinition>, instance: Instance) {
        let status = match instance.status.as_str() {
            "retry-waiting" => "retry-waiting",
            "waiting" => "waiting",
            _ => "pending",
        };
        let context = Context {
            state: Arc::new(Mutex::new(Running {
                status,
                next: 0,
                in_flight: HashMap::new(),
            })),
            store: Some(self.store.clone()),
            claim: None,
            id: instance.id.clone(),
        };
        self.active
            .lock()
            .await
            .insert(instance.id.clone(), context.clone());
        if instance.deadline_origin.is_some() {
            let store = self.store.clone();
            let active = self.active.clone();
            let id = instance.id.clone();
            tokio::spawn(async move {
                loop {
                    let Ok(Some(item)) = store.get(&id).await else {
                        break;
                    };
                    if !matches!(
                        item.status.as_str(),
                        "pending" | "running" | "retry-waiting" | "waiting"
                    ) {
                        break;
                    }
                    if let Some(at) = item.deadline_at_ms {
                        tokio::time::sleep(Duration::from_millis(
                            at.saturating_sub(crate::store::now_ms()).max(0) as u64,
                        ))
                        .await;
                        let _ = expire_local_deadlines(&store, &active).await;
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            });
        }
        let permits = self.permits.clone();
        let active = self.active.clone();
        let definitions = self.registry.named.clone();
        tokio::spawn(async move {
            let id = instance.id.clone();
            run_named(graph, instance, permits, context, definitions).await;
            active.lock().await.remove(&id);
        });
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
            .get(namespace, version, name)
            .ok_or("unknown process")?;
        let input =
            (graph.decode_input)(input).map_err(|error| format!("invalid input: {error}"))?;
        let id = uuid::Uuid::new_v4().to_string();
        let mut instance = Instance::new(&id, namespace, version, name, input.clone());
        if let Some(policy) = &graph.deadline {
            instance.with_deadline(policy)?;
        }
        let mut checkpoint = graph.checkpoint(&input)?;
        graph.resume_due(&input, &mut checkpoint, crate::store::now_ms())?;
        if graph.ready(&checkpoint).is_empty()
            && !graph.has_pending(&checkpoint)
            && let Some(at_ms) = graph.waiting_until(&checkpoint)
        {
            instance.status = "waiting".into();
            instance.wake_at_ms = Some(at_ms);
        }
        instance.checkpoint = Some(checkpoint);
        self.store.create(&instance).await?;
        self.spawn_named(graph, instance).await;
        Ok(id)
    }

    pub async fn status(&self, id: &str) -> Result<Option<Instance>, String> {
        expire_local_deadlines(&self.store, &self.active).await?;
        self.store.get(id).await
    }

    pub async fn cancel(&self, id: &str) -> Result<(), String> {
        expire_local_deadlines(&self.store, &self.active).await?;
        let context = self.active.lock().await.get(id).cloned();
        let Some(context) = context else {
            return match self.store.get(id).await? {
                None => Err("unknown instance".into()),
                Some(item) if item.status == "cancelled" => Ok(()),
                Some(_) => Err("instance already terminal".into()),
            };
        };
        let mut state = context.state.lock().await;
        if matches!(state.status, "cancelled" | "cancelling") {
            return Ok(());
        }
        if !matches!(
            state.status,
            "pending" | "running" | "retry-waiting" | "waiting"
        ) {
            return Err("instance already terminal".into());
        }
        self.store.finish(id, "cancelling", None, None).await?;
        state.status = "cancelling";
        let hooks: Vec<_> = state.in_flight.values().cloned().collect();
        drop(state);
        for hook in hooks {
            hook();
        }
        let mut state = context.state.lock().await;
        self.store.finish(id, "cancelled", None, None).await?;
        state.status = "cancelled";
        Ok(())
    }
}

mod claimed;
mod scheduler;
mod subprocess;

pub(crate) use claimed::execute_claimed;
use scheduler::{execute_named, run_named};
use subprocess::*;

enum NamedOutcome {
    Completed(Value),
    Terminal(GraphTerminal),
    Waiting(i64),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn terminal_named_instance_rejects_late_cancellation() {
        let path =
            std::env::temp_dir().join(format!("blkit-terminal-cancel-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = LocalStore::open(&path).await.unwrap();
        let engine = Engine::new(Registry::new(vec![]).unwrap(), store.clone(), 1).unwrap();
        for status in ["business-error", "terminated"] {
            store
                .create(&Instance::new(status, "test", "1", "test", Value::Null))
                .await
                .unwrap();
            store
                .finish_named(status, status, "exit", None, None)
                .await
                .unwrap();
            engine.active.lock().await.insert(
                status.into(),
                Context {
                    state: Arc::new(Mutex::new(Running {
                        status,
                        next: 0,
                        in_flight: HashMap::new(),
                    })),
                    store: Some(store.clone()),
                    claim: None,
                    id: status.into(),
                },
            );
            assert!(
                engine
                    .cancel(status)
                    .await
                    .unwrap_err()
                    .contains("terminal")
            );
            let item = store.get(status).await.unwrap().unwrap();
            assert_eq!(item.status, status);
            assert_eq!(item.terminal_name.as_deref(), Some("exit"));
        }
        drop(engine);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn cancelled_claim_fetched_after_assignment_is_harmless() {
        use crate::compiled_graph::{GraphLink, GraphNode};
        use std::sync::atomic::{AtomicBool, Ordering};
        use testcontainers_modules::{
            postgres::Postgres,
            testcontainers::{ImageExt, runners::AsyncRunner},
        };

        let node = Postgres::default()
            .with_tag("17.6-alpine")
            .start()
            .await
            .unwrap();
        let host = std::env::var("BLKIT_TESTCONTAINERS_HOST")
            .unwrap_or(node.get_host().await.unwrap().to_string());
        let url = format!(
            "postgres://postgres:postgres@{host}:{}/postgres",
            node.get_host_port_ipv4(5432).await.unwrap()
        );
        let store = PostgresStore::connect(&url).await.unwrap();
        let invoked = Arc::new(AtomicBool::new(false));
        let graph = Arc::new(GraphDefinition {
            namespace: "test",
            version: "1",
            name: "work",
            retry: None,
            deadline: None,
            decode_input: Box::new(Ok::<Value, String>),
            nodes: vec![
                GraphNode {
                    name: "start",
                    kind: GraphNodeKind::Start,
                },
                GraphNode {
                    name: "task",
                    kind: GraphNodeKind::Task(Arc::new({
                        let invoked = invoked.clone();
                        move |_, _| {
                            invoked.store(true, Ordering::SeqCst);
                            Ok(Value::Null)
                        }
                    })),
                },
                GraphNode {
                    name: "done",
                    kind: GraphNodeKind::End,
                },
            ],
            links: vec![
                GraphLink {
                    source: "start",
                    target: "task",
                    value: None,
                    condition: None,
                    fallback: false,
                    label: None,
                },
                GraphLink {
                    source: "task",
                    target: "done",
                    value: Some(Arc::new(|_, _| Ok(Value::Null))),
                    condition: None,
                    fallback: false,
                    label: None,
                },
            ],
        });
        store
            .register_worker("worker", &[("test", "1", "work")])
            .await
            .unwrap();
        let mut instance = Instance::new("case", "test", "1", "work", Value::Null);
        instance.checkpoint = Some(graph.checkpoint(&instance.input).unwrap());
        store.create(&instance).await.unwrap();
        store.claim("worker", 1, 1000).await.unwrap();
        store.cancel("case").await.unwrap();
        let cancelled = store.get("case").await.unwrap().unwrap();
        assert!(
            execute_claimed(
                graph,
                cancelled,
                store.clone(),
                "worker".into(),
                Arc::new(Semaphore::new(1)),
                Arc::new(HashMap::new()),
            )
            .await
            .is_ok()
        );
        assert!(!invoked.load(Ordering::SeqCst));
        assert_eq!(
            store.get("case").await.unwrap().unwrap().instance.status,
            "cancelled"
        );
        drop(node);
    }
}
