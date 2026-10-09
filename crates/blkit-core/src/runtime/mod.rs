use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use serde_json::Value;
#[cfg(feature = "local-persistence")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "local-persistence")]
use tokio::sync::Notify;
use tokio::{
    sync::{Mutex, Semaphore},
    task::JoinSet,
};

use crate::compiled_graph::{
    ChildActivation, GraphCheckpoint, GraphDefinition, GraphNodeKind, GraphTerminal,
};
#[cfg(feature = "remote-persistence")]
use crate::postgres_store::{ClaimActivity, DistributedInstance, PostgresStore};

pub use crate::store::Instance;
#[cfg(feature = "local-persistence")]
pub use crate::store::Store as LocalStore;

pub use crate::evaluation::{AsyncEvaluate, Cancel, Evaluate, Values, next_retry_at};

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

#[derive(Clone)]
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

#[cfg(feature = "remote-persistence")]
#[derive(Clone)]
struct Claim {
    store: PostgresStore,
    worker_id: String,
    generation: i64,
}

#[derive(Clone)]
struct Context {
    state: Arc<Mutex<Running>>,
    #[cfg(feature = "local-persistence")]
    store: Option<LocalStore>,
    #[cfg(feature = "remote-persistence")]
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

    async fn persist_terminal(
        &self,
        status: &str,
        result: Option<Value>,
        terminal: Option<&str>,
    ) -> Result<(), String> {
        #[cfg(feature = "remote-persistence")]
        if let Some(claim) = &self.claim {
            return claim
                .store
                .finish_owned(
                    &self.id,
                    &claim.worker_id,
                    claim.generation,
                    status,
                    result,
                    terminal,
                    None,
                )
                .await?
                .then_some(())
                .ok_or("lost claim before terminal write".into());
        }
        #[cfg(feature = "local-persistence")]
        if let Some(store) = &self.store {
            return if let Some(name) = terminal {
                store
                    .finish_named(&self.id, status, name, result, None)
                    .await
            } else {
                store.finish(&self.id, status, result, None).await
            };
        }
        Err("missing instance store".into())
    }

    async fn checkpoint(&self, state: &GraphCheckpoint) -> Result<(), String> {
        #[cfg(feature = "remote-persistence")]
        if let Some(claim) = &self.claim {
            return claim
                .store
                .commit_checkpoint(&self.id, &claim.worker_id, claim.generation, state)
                .await?
                .then_some(())
                .ok_or("lost claim before checkpoint".into());
        }
        #[cfg(feature = "local-persistence")]
        if let Some(store) = &self.store {
            return store.commit_checkpoint(&self.id, state).await;
        }
        Err("missing instance store".into())
    }

    async fn wait_until(&self, state: &GraphCheckpoint, wake: i64) -> Result<(), String> {
        #[cfg(feature = "remote-persistence")]
        if let Some(claim) = &self.claim {
            return claim
                .store
                .release_wait_owned(&self.id, &claim.worker_id, claim.generation, state, wake)
                .await?
                .then_some(())
                .ok_or("lost claim before wait checkpoint".into());
        }
        #[cfg(feature = "local-persistence")]
        if let Some(store) = &self.store {
            return store.set_wait(&self.id, state, wake).await;
        }
        Err("missing instance store".into())
    }

    #[cfg(feature = "local-persistence")]
    async fn begin_attempt(&self) -> Result<(), String> {
        if let Some(store) = &self.store {
            store.begin_attempt(&self.id).await?;
        }
        Ok(())
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
        #[cfg(feature = "remote-persistence")]
        let remote = if let Some(claim) = &self.claim {
            let status = claim
                .store
                .fail_owned(&self.id, &claim.worker_id, claim.generation, policy, error)
                .await?
                .ok_or("lost claim before recording failure")?;
            Some((status, None))
        } else {
            None
        };
        #[cfg(not(feature = "remote-persistence"))]
        let remote: Option<(&'static str, Option<i64>)> = None;
        let (status, next) = if let Some(remote) = remote {
            remote
        } else {
            #[cfg(feature = "local-persistence")]
            {
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
            }
            #[cfg(not(feature = "local-persistence"))]
            {
                return Err("missing instance store".into());
            }
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
        self.persist_terminal(status, None, Some(name)).await?;
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
        self.persist_terminal("completed", Some(value), None)
            .await?;
        state.status = "completed";
        Ok(())
    }
}

#[cfg(feature = "local-persistence")]
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

#[cfg(feature = "local-persistence")]
pub struct Engine {
    registry: Registry,
    store: LocalStore,
    permits: Arc<Semaphore>,
    active: Arc<Mutex<HashMap<String, Context>>>,
    wake: Arc<Notify>,
    alive: Arc<()>,
    worker_started: AtomicBool,
}

#[cfg(feature = "local-persistence")]
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
            wake: Arc::new(Notify::new()),
            alive: Arc::new(()),
            worker_started: AtomicBool::new(false),
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
                    #[cfg(feature = "remote-persistence")]
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
        }
        self.start_worker();
        Ok(())
    }

    fn start_worker(&self) {
        if self.worker_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let registry = self.registry.clone();
        let store = self.store.clone();
        let permits = self.permits.clone();
        let active = self.active.clone();
        let wake = self.wake.clone();
        let alive = Arc::downgrade(&self.alive);
        tokio::spawn(async move {
            while alive.upgrade().is_some() {
                if let Err(error) = expire_local_deadlines(&store, &active).await {
                    eprintln!("local worker deadline check failed: {error}");
                }
                match store.incomplete().await {
                    Ok(instances) => {
                        for instance in instances {
                            let now = crate::store::now_ms();
                            let eligible = match instance.status.as_str() {
                                "pending" => true,
                                "retry-waiting" => {
                                    instance.next_eligible_at.is_some_and(|at| at <= now)
                                }
                                "waiting" => instance.wake_at_ms.is_some_and(|at| at <= now),
                                _ => false,
                            };
                            if !eligible || active.lock().await.contains_key(&instance.id) {
                                continue;
                            }
                            if let Some(graph) = registry.get(
                                &instance.namespace,
                                &instance.version,
                                &instance.process,
                            ) {
                                Self::spawn_named(
                                    &store, &active, &permits, &registry, graph, instance,
                                )
                                .await;
                            } else {
                                let _ = store
                                    .finish(
                                        &instance.id,
                                        "failed",
                                        None,
                                        Some("compiled process unavailable after restart"),
                                    )
                                    .await;
                            }
                        }
                    }
                    Err(error) => eprintln!("local worker queue poll failed: {error}"),
                }
                tokio::select! {
                    _ = wake.notified() => {},
                    _ = tokio::time::sleep(Duration::from_millis(20)) => {},
                }
            }
        });
    }

    async fn spawn_named(
        store: &LocalStore,
        active: &Arc<Mutex<HashMap<String, Context>>>,
        permits: &Arc<Semaphore>,
        registry: &Registry,
        graph: Arc<GraphDefinition>,
        instance: Instance,
    ) {
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
            store: Some(store.clone()),
            #[cfg(feature = "remote-persistence")]
            claim: None,
            id: instance.id.clone(),
        };
        active
            .lock()
            .await
            .insert(instance.id.clone(), context.clone());
        if instance.deadline_origin.is_some() {
            let store = store.clone();
            let active = active.clone();
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
        let permits = permits.clone();
        let active = active.clone();
        let definitions = registry.named.clone();
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
        self.start_worker();
        self.wake.notify_one();
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
                Some(item)
                    if matches!(
                        item.status.as_str(),
                        "pending" | "retry-waiting" | "waiting"
                    ) =>
                {
                    self.store.finish(id, "cancelled", None, None).await
                }
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

#[cfg(feature = "remote-persistence")]
mod claimed;
mod scheduler;
mod subprocess;

#[cfg(feature = "remote-persistence")]
pub(crate) use claimed::execute_claimed;
use scheduler::execute_named;
#[cfg(feature = "local-persistence")]
use scheduler::run_named;
use subprocess::*;

enum NamedOutcome {
    Completed(Value),
    Terminal(GraphTerminal),
    Waiting(i64),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "local-persistence")]
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
                    #[cfg(feature = "remote-persistence")]
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

    #[cfg(feature = "remote-persistence")]
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
