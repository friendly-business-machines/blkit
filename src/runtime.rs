use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use futures_util::future::try_join_all;
use serde_json::Value;
use tokio::{
    sync::{Mutex, Semaphore},
    task::JoinSet,
};

use crate::{
    named_runtime::{GraphDefinition, GraphNodeKind, GraphTerminal},
    postgres_store::{ClaimActivity, DistributedInstance, PostgresStore},
};

pub use crate::store::{Instance, Store};

pub type Values = HashMap<String, Value>;
pub type Evaluate = Arc<dyn Fn(&Value, &Values) -> Result<Value, String> + Send + Sync>;
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

pub struct Definition {
    pub namespace: &'static str,
    pub version: &'static str,
    pub name: &'static str,
    pub steps: Vec<Step>,
    pub decode_input: Box<dyn Fn(Value) -> Result<Value, String> + Send + Sync>,
}

pub enum Step {
    Run {
        name: &'static str,
        call: Evaluate,
        cancel: Cancel,
    },
    Gateway {
        kind: &'static str,
        branches: Vec<Branch>,
        join: &'static str,
    },
    Return(Evaluate),
}

pub struct Branch {
    pub label: Option<&'static str>,
    pub condition: Option<Evaluate>,
    pub steps: Vec<Step>,
}

pub struct Registry {
    legacy: HashMap<(String, String, String), Arc<Definition>>,
    named: HashMap<(String, String, String), Arc<GraphDefinition>>,
}

impl Registry {
    pub fn new(definitions: Vec<Definition>) -> Result<Self, String> {
        let mut entries = HashMap::new();
        for definition in definitions {
            let key = (
                definition.namespace.into(),
                definition.version.into(),
                definition.name.into(),
            );
            if entries.insert(key, Arc::new(definition)).is_some() {
                return Err("duplicate process identity".into());
            }
        }
        Ok(Self {
            legacy: entries,
            named: HashMap::new(),
        })
    }

    pub fn new_named(definitions: Vec<GraphDefinition>) -> Result<Self, String> {
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
        Ok(Self {
            legacy: HashMap::new(),
            named,
        })
    }

    pub fn get(&self, namespace: &str, version: &str, name: &str) -> Option<Arc<Definition>> {
        self.legacy
            .get(&(namespace.into(), version.into(), name.into()))
            .cloned()
    }

    pub fn get_named(
        &self,
        namespace: &str,
        version: &str,
        name: &str,
    ) -> Option<Arc<GraphDefinition>> {
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
    store: Option<Store>,
    claim: Option<Claim>,
    id: String,
}

impl Context {
    fn standalone() -> Self {
        Self {
            state: Arc::new(Mutex::new(Running {
                status: "running",
                next: 0,
                in_flight: HashMap::new(),
            })),
            store: None,
            claim: None,
            id: String::new(),
        }
    }

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

    async fn stop(&self, status: &'static str, error: Option<&str>) -> Result<(), String> {
        let mut state = self.state.lock().await;
        if !matches!(state.status, "pending" | "running") {
            return Ok(());
        }
        if let Some(store) = &self.store {
            store.finish(&self.id, status, None, error).await?;
        }
        state.status = status;
        let hooks: Vec<_> = state.in_flight.values().cloned().collect();
        drop(state);
        for hook in hooks {
            hook();
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

impl Definition {
    pub async fn evaluate(&self, input: Value) -> Result<Value, String> {
        self.evaluate_limited(input, Arc::new(Semaphore::new(32)))
            .await
    }

    pub async fn evaluate_limited(
        &self,
        input: Value,
        permits: Arc<Semaphore>,
    ) -> Result<Value, String> {
        let input = (self.decode_input)(input)?;
        let mut values = Values::new();
        execute(
            &self.steps,
            &input,
            &mut values,
            &permits,
            &Context::standalone(),
        )
        .await
    }
}

async fn expire_local_deadlines(
    store: &Store,
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
    store: Store,
    permits: Arc<Semaphore>,
    active: Arc<Mutex<HashMap<String, Context>>>,
}

impl Engine {
    pub fn new(registry: Registry, store: Store, limit: usize) -> Result<Self, String> {
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
        expire_local_deadlines(&self.store, &self.active).await?;
        self.store.recover_interrupted().await?;
        for mut instance in self.store.incomplete().await? {
            if self.active.lock().await.contains_key(&instance.id) {
                continue;
            }
            let Some(graph) =
                self.registry
                    .get_named(&instance.namespace, &instance.version, &instance.process)
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
        tokio::spawn(async move {
            let id = instance.id.clone();
            run_named(graph, instance, permits, context).await;
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
        let definition = self.registry.get(namespace, version, name);
        let named = self.registry.get_named(namespace, version, name);
        let input = if let Some(definition) = &definition {
            (definition.decode_input)(input)
        } else if let Some(graph) = &named {
            (graph.decode_input)(input)
        } else {
            return Err("unknown process".into());
        }
        .map_err(|error| format!("invalid input: {error}"))?;
        let id = uuid::Uuid::new_v4().to_string();
        let mut instance = Instance::new(&id, namespace, version, name, input.clone());
        if let Some(graph) = &named {
            if let Some(policy) = &graph.deadline {
                instance.with_deadline(policy)?;
            }
            let mut checkpoint = graph.checkpoint(&input)?;
            graph.resume_due(&input, &mut checkpoint, crate::store::now_ms())?;
            if graph.ready(&checkpoint).is_empty() && !graph.has_pending(&checkpoint) {
                if let Some(at_ms) = graph.waiting_until(&checkpoint) {
                    instance.status = "waiting".into();
                    instance.wake_at_ms = Some(at_ms);
                }
            }
            instance.checkpoint = Some(checkpoint);
        }
        self.store.create(&instance).await?;
        if let Some(graph) = named {
            self.spawn_named(graph, instance).await;
            return Ok(id);
        }
        let context = Context {
            state: Arc::new(Mutex::new(Running {
                status: "pending",
                next: 0,
                in_flight: HashMap::new(),
            })),
            store: Some(self.store.clone()),
            claim: None,
            id: id.clone(),
        };
        self.active.lock().await.insert(id.clone(), context.clone());
        let permits = self.permits.clone();
        let active = self.active.clone();
        let instance_id = id.clone();
        tokio::spawn(async move {
            let mut values = Values::new();
            let definition = definition.unwrap();
            match execute(&definition.steps, &input, &mut values, &permits, &context).await {
                Ok(value) => {
                    let _ = context.complete(value).await;
                }
                Err(error) => {
                    let _ = context.stop("failed", Some(&error)).await;
                }
            }
            active.lock().await.remove(&instance_id);
        });
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

enum NamedOutcome {
    Completed(Value),
    Terminal(GraphTerminal),
    Waiting(i64),
}

pub(crate) async fn execute_claimed(
    graph: Arc<GraphDefinition>,
    claimed: DistributedInstance,
    store: PostgresStore,
    worker_id: String,
    permits: Arc<Semaphore>,
) -> Result<(), String> {
    if claimed.instance.status == "cancelled" {
        return Ok(());
    }
    if claimed.owner_id.as_deref() != Some(worker_id.as_str()) {
        return Err("instance is not owned by this worker".into());
    }
    let context = Context {
        state: Arc::new(Mutex::new(Running {
            status: "running",
            next: 0,
            in_flight: HashMap::new(),
        })),
        store: None,
        claim: Some(Claim {
            store: store.clone(),
            worker_id: worker_id.clone(),
            generation: claimed.generation,
        }),
        id: claimed.instance.id.clone(),
    };
    let checkpoint = claimed
        .instance
        .checkpoint
        .ok_or("claimed instance has no checkpoint")?;
    let watch = async {
        loop {
            tokio::time::sleep(Duration::from_millis(20)).await;
            match store
                .claim_activity(&context.id, &worker_id, claimed.generation)
                .await?
            {
                ClaimActivity::Active => {}
                ClaimActivity::Cancelled => {
                    context.signal_cancel().await;
                    return Ok::<bool, String>(true);
                }
                ClaimActivity::Lost => {
                    context.signal_cancel().await;
                    return Ok(false);
                }
            }
        }
    };
    let outcome = tokio::select! {
        result = execute_named(&graph, &claimed.instance.input, checkpoint, &permits, &context) => result,
        cancelled = watch => {
            return if cancelled? { Ok(()) } else { Err("lost claim during execution".into()) };
        }
    };
    match outcome {
        Ok(NamedOutcome::Completed(value)) => context.complete(value).await,
        Ok(NamedOutcome::Terminal(terminal)) => context.named_terminal(&terminal).await,
        Ok(NamedOutcome::Waiting(_)) => Ok(()),
        Err(error) => {
            if store
                .get(&context.id)
                .await?
                .is_some_and(|row| row.instance.status == "cancelled")
            {
                context.signal_cancel().await;
                Ok(())
            } else {
                context
                    .fail_attempt(graph.retry.as_ref(), &error)
                    .await
                    .map(|_| ())
            }
        }
    }
}

async fn run_named(
    graph: Arc<GraphDefinition>,
    instance: Instance,
    permits: Arc<Semaphore>,
    context: Context,
) {
    let Some(mut checkpoint) = instance.checkpoint else {
        return;
    };
    let mut next_eligible_at = instance.next_eligible_at;
    let mut wake_at_ms = instance.wake_at_ms;
    loop {
        if let Some(wake) = wake_at_ms.take() {
            tokio::time::sleep(Duration::from_millis(
                wake.saturating_sub(crate::store::now_ms()).max(0) as u64,
            ))
            .await;
            let mut status = context.state.lock().await;
            let Ok(Some(saved)) = context.store.as_ref().unwrap().get(&instance.id).await else {
                break;
            };
            if saved.status != "waiting" {
                break;
            }
            let Some(mut next) = saved.checkpoint else {
                break;
            };
            if graph
                .resume_due(&instance.input, &mut next, crate::store::now_ms())
                .is_err()
            {
                break;
            }
            let Ok(Some(resumed)) = context
                .store
                .as_ref()
                .unwrap()
                .resume_wait(&instance.id, &next)
                .await
            else {
                break;
            };
            *status = Running {
                status: resumed,
                next: status.next,
                in_flight: HashMap::new(),
            };
            checkpoint = next;
        }
        if let Some(next) = next_eligible_at.take() {
            tokio::time::sleep(Duration::from_millis(
                next.saturating_sub(crate::store::now_ms()).max(0) as u64,
            ))
            .await;
            let Ok(Some(saved)) = context.store.as_ref().unwrap().get(&instance.id).await else {
                break;
            };
            if saved.status != "retry-waiting" {
                break;
            }
            let Some(state) = saved.checkpoint else {
                break;
            };
            checkpoint = state;
        }
        match execute_named(
            &graph,
            &instance.input,
            checkpoint.clone(),
            &permits,
            &context,
        )
        .await
        {
            Ok(NamedOutcome::Completed(value)) => {
                let _ = context.complete(value).await;
                break;
            }
            Ok(NamedOutcome::Terminal(terminal)) => {
                let _ = context.named_terminal(&terminal).await;
                break;
            }
            Ok(NamedOutcome::Waiting(wake)) => {
                context.state.lock().await.status = "waiting";
                let Ok(Some(saved)) = context.store.as_ref().unwrap().get(&instance.id).await
                else {
                    break;
                };
                let Some(next) = saved.checkpoint else {
                    break;
                };
                checkpoint = next;
                wake_at_ms = Some(wake);
            }
            Err(error) => {
                let Ok(Some(next)) = context.fail_attempt(graph.retry.as_ref(), &error).await
                else {
                    break;
                };
                next_eligible_at = Some(next);
            }
        }
    }
}

async fn save_wait(
    context: &Context,
    checkpoint: &crate::named_runtime::GraphCheckpoint,
    wake: i64,
) -> Result<(), String> {
    if let Some(claim) = &context.claim {
        if !claim
            .store
            .release_wait_owned(
                &context.id,
                &claim.worker_id,
                claim.generation,
                checkpoint,
                wake,
            )
            .await?
        {
            return Err("lost claim before wait checkpoint".into());
        }
    } else if let Some(store) = &context.store {
        store.set_wait(&context.id, checkpoint, wake).await?;
    }
    Ok(())
}

async fn execute_named(
    graph: &GraphDefinition,
    input: &Value,
    mut checkpoint: crate::named_runtime::GraphCheckpoint,
    permits: &Arc<Semaphore>,
    context: &Context,
) -> Result<NamedOutcome, String> {
    graph.migrate_checkpoint(&mut checkpoint);
    let mut tasks = JoinSet::new();
    let mut in_flight = HashSet::new();
    loop {
        graph.check_loop_bounds(&mut checkpoint, crate::store::now_ms());
        if let Some(terminal) = &checkpoint.terminal {
            return Ok(NamedOutcome::Terminal(terminal.clone()));
        }
        if let Some(result) = &checkpoint.outcome {
            return Ok(NamedOutcome::Completed(result.clone()));
        }
        if graph
            .waiting_until(&checkpoint)
            .is_some_and(|at| at <= crate::store::now_ms())
        {
            let mut next = checkpoint.clone();
            graph.resume_due(input, &mut next, crate::store::now_ms())?;
            if let Some(claim) = &context.claim {
                if !claim
                    .store
                    .commit_checkpoint(&context.id, &claim.worker_id, claim.generation, &next)
                    .await?
                {
                    return Err("lost claim before checkpoint".into());
                }
            } else if let Some(store) = &context.store {
                store.commit_checkpoint(&context.id, &next).await?;
            }
            checkpoint = next;
            continue;
        }
        if tasks.is_empty()
            && graph.ready(&checkpoint).is_empty()
            && !graph.has_pending(&checkpoint)
        {
            if let Some(wake) = graph.waiting_until(&checkpoint) {
                save_wait(context, &checkpoint, wake).await?;
                return Ok(NamedOutcome::Waiting(wake));
            }
        }
        for (activation, name) in graph.ready_activations(&checkpoint) {
            if in_flight.contains(&activation) {
                continue;
            }
            let permit = match permits.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(tokio::sync::TryAcquireError::NoPermits)
                    if tasks.is_empty() && !graph.has_pending(&checkpoint) =>
                {
                    permits
                        .clone()
                        .acquire_owned()
                        .await
                        .map_err(|error| error.to_string())?
                }
                Err(tokio::sync::TryAcquireError::NoPermits) => break,
                Err(error) => return Err(error.to_string()),
            };
            let node = graph
                .nodes
                .iter()
                .find(|node| node.name == name)
                .ok_or("unknown task node")?;
            let (call, cancel) = match &node.kind {
                GraphNodeKind::Task(call) | GraphNodeKind::TaskLoop(call, _) => {
                    (call, Arc::new(|| {}) as Cancel)
                }
                GraphNodeKind::MultiInstance { task, .. } => (task, Arc::new(|| {}) as Cancel),
                GraphNodeKind::TaskWithCancel(call, cancel) => (call, cancel.clone()),
                _ => return Err("ready node is not a task".into()),
            };
            let mut state = context.state.lock().await;
            if !matches!(state.status, "pending" | "running" | "retry-waiting") {
                return Err("instance cancelled".into());
            }
            if matches!(state.status, "pending" | "retry-waiting") {
                if let Some(store) = &context.store {
                    store.begin_attempt(&context.id).await?;
                }
                state.status = "running";
            }
            let key = state.next;
            state.next += 1;
            state.in_flight.insert(key, cancel);
            drop(state);
            let call = call.clone();
            let source = graph.activation_input(&checkpoint, activation, input)?;
            let values = graph.activation_values(&checkpoint, activation)?;
            in_flight.insert(activation);
            tasks.spawn_blocking(move || {
                let _permit = permit;
                (key, activation, call(&source, &values))
            });
        }
        let completed = if graph.has_pending(&checkpoint) {
            if let Some(done) = tasks.try_join_next() {
                done
            } else {
                let mut state = context.state.lock().await;
                if !matches!(state.status, "pending" | "running" | "retry-waiting") {
                    return Err("instance cancelled".into());
                }
                if matches!(state.status, "pending" | "retry-waiting") {
                    if let Some(store) = &context.store {
                        store.begin_attempt(&context.id).await?;
                    }
                    state.status = "running";
                }
                let mut next = checkpoint.clone();
                graph.resume_pending(input, &mut next)?;
                if let Some(claim) = &context.claim {
                    if !claim
                        .store
                        .commit_checkpoint(&context.id, &claim.worker_id, claim.generation, &next)
                        .await?
                    {
                        return Err("lost claim before checkpoint".into());
                    }
                } else if let Some(store) = &context.store {
                    store.commit_checkpoint(&context.id, &next).await?;
                }
                checkpoint = next;
                drop(state);
                tokio::task::yield_now().await;
                continue;
            }
        } else {
            tasks.join_next().await.ok_or("graph has no ready tasks")?
        };
        let (key, activation, result) = completed.map_err(|error| error.to_string())?;
        in_flight.remove(&activation);
        let mut state = context.state.lock().await;
        state.in_flight.remove(&key);
        if state.status != "running" {
            return Err("instance cancelled".into());
        }
        let result = match result {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let mut next = checkpoint.clone();
        graph.complete_activation(input, &mut next, activation, result)?;
        if tasks.is_empty() && graph.ready(&next).is_empty() && !graph.has_pending(&next) {
            if let Some(wake) = graph.waiting_until(&next) {
                save_wait(context, &next, wake).await?;
                return Ok(NamedOutcome::Waiting(wake));
            }
        }
        if let Some(claim) = &context.claim {
            if !claim
                .store
                .commit_checkpoint(&context.id, &claim.worker_id, claim.generation, &next)
                .await?
            {
                return Err("lost claim before checkpoint".into());
            }
        } else if let Some(store) = &context.store {
            store.commit_checkpoint(&context.id, &next).await?;
        }
        checkpoint = next;
        drop(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn terminal_named_instance_rejects_late_cancellation() {
        let path =
            std::env::temp_dir().join(format!("blkit-terminal-cancel-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = Store::open(&path).await.unwrap();
        let engine = Engine::new(Registry::new_named(vec![]).unwrap(), store.clone(), 1).unwrap();
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
        use crate::named_runtime::{GraphLink, GraphNode};
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
                Arc::new(Semaphore::new(1))
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

fn execute<'a>(
    steps: &'a [Step],
    input: &'a Value,
    values: &'a mut Values,
    permits: &'a Arc<Semaphore>,
    context: &'a Context,
) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>> {
    Box::pin(async move {
        let mut last = None;
        for step in steps {
            match step {
                Step::Run { name, call, cancel } => {
                    let permit = permits
                        .clone()
                        .acquire_owned()
                        .await
                        .map_err(|e| e.to_string())?;
                    let mut state = context.state.lock().await;
                    if !matches!(state.status, "pending" | "running") {
                        return Err("instance cancelled".into());
                    }
                    if state.status == "pending" {
                        if let Some(store) = &context.store {
                            store.finish(&context.id, "running", None, None).await?;
                        }
                        state.status = "running";
                    }
                    let key = state.next;
                    state.next += 1;
                    state.in_flight.insert(key, cancel.clone());
                    drop(state);
                    let call = call.clone();
                    let source = input.clone();
                    let data = values.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        let _permit = permit;
                        call(&source, &data)
                    })
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|result| result);
                    let mut state = context.state.lock().await;
                    state.in_flight.remove(&key);
                    if state.status != "running" {
                        return Err("instance cancelled".into());
                    }
                    drop(state);
                    let result = match result {
                        Ok(value) => value,
                        Err(error) => {
                            context.stop("failed", Some(&error)).await?;
                            return Err(error);
                        }
                    };
                    values.insert((*name).into(), result.clone());
                    last = Some(result);
                }
                Step::Gateway {
                    kind,
                    branches,
                    join,
                } => {
                    let state = context.state.lock().await;
                    if !matches!(state.status, "pending" | "running") {
                        return Err("instance cancelled".into());
                    }
                    let selected: Vec<usize> = match *kind {
                        "and" => (0..branches.len()).collect(),
                        "xor" | "or" => {
                            let mut matches = Vec::new();
                            let mut fallback = None;
                            for (index, branch) in branches.iter().enumerate() {
                                match &branch.condition {
                                    Some(condition)
                                        if condition(input, values)? == Value::Bool(true) =>
                                    {
                                        matches.push(index);
                                        if *kind == "xor" {
                                            break;
                                        }
                                    }
                                    None => fallback = Some(index),
                                    _ => {}
                                }
                            }
                            if matches.is_empty() {
                                vec![fallback.ok_or("missing gateway fallback")?]
                            } else {
                                matches
                            }
                        }
                        _ => return Err(format!("unknown gateway: {kind}")),
                    };
                    drop(state);
                    let pending = selected.into_iter().map(|index| {
                        let mut branch_values = values.clone();
                        let context = context.clone();
                        async move {
                            execute(
                                &branches[index].steps,
                                input,
                                &mut branch_values,
                                permits,
                                &context,
                            )
                            .await
                            .map(|result| (index, result))
                        }
                    });
                    let results: Vec<_> = try_join_all(pending).await?;
                    let result = match *kind {
                        "and" => Value::Object(
                            results
                                .into_iter()
                                .map(|(index, value)| {
                                    (branches[index].label.unwrap_or_default().into(), value)
                                })
                                .collect(),
                        ),
                        "or" => Value::Array(results.into_iter().map(|(_, value)| value).collect()),
                        _ => results.into_iter().next().ok_or("empty gateway")?.1,
                    };
                    let state = context.state.lock().await;
                    if state.status != "running" {
                        return Err("instance cancelled".into());
                    }
                    values.insert((*join).into(), result.clone());
                    last = Some(result);
                }
                Step::Return(calculate) => {
                    let state = context.state.lock().await;
                    if !matches!(state.status, "pending" | "running") {
                        return Err("instance cancelled".into());
                    }
                    return calculate(input, values);
                }
            }
        }
        last.ok_or("empty graph branch".into())
    })
}
