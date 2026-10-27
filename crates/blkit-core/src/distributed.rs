#[cfg(feature = "api-server")]
use std::collections::{HashSet, VecDeque};
#[cfg(any(feature = "worker", feature = "api-server"))]
use std::{collections::HashMap, sync::Arc, time::Duration};
#[cfg(feature = "worker")]
use tokio::sync::Semaphore;
#[cfg(any(feature = "worker", feature = "api-server"))]
use tokio::task::JoinSet;

use crate::postgres_store::{DistributedInstance, PostgresStore};
#[cfg(feature = "worker")]
use crate::{
    compiled_graph::GraphDefinition,
    runtime::{NamedRegistry, execute_claimed, validate_named_registry},
};
use serde_json::Value;

#[cfg(feature = "api-server")]
use std::sync::OnceLock;
#[cfg(feature = "api-server")]
use tokio::{
    sync::{Mutex, Notify, broadcast, mpsc},
    time,
};
#[cfg(feature = "api-server")]
use tokio_postgres::AsyncMessage;

#[cfg(feature = "api-server")]
struct RemoteEvents {
    channel: String,
    state: Mutex<RemoteState>,
    ready: Notify,
    started: OnceLock<()>,
}

#[cfg(feature = "api-server")]
#[derive(Default)]
struct RemoteState {
    listening: bool,
    watchers: HashMap<String, Arc<RemoteWatcher>>,
}

#[cfg(feature = "api-server")]
struct RemoteWatcher {
    sender: broadcast::Sender<Value>,
    order: Mutex<()>,
    hints: Mutex<PendingHints>,
}

#[cfg(feature = "api-server")]
#[derive(Default)]
struct PendingHints {
    running: bool,
    pending: bool,
}

#[cfg(feature = "api-server")]
#[derive(Clone, Copy)]
enum RefreshMode {
    Reconnect,
    Renew,
    Retry,
}

pub struct DistributedControl {
    store: PostgresStore,
    #[cfg(feature = "api-server")]
    events: Arc<RemoteEvents>,
}

impl DistributedControl {
    pub fn new(store: PostgresStore) -> Self {
        Self {
            store,
            #[cfg(feature = "api-server")]
            events: Arc::new(RemoteEvents {
                channel: format!("blkit_{}", uuid::Uuid::new_v4().simple()),
                state: Mutex::new(RemoteState::default()),
                ready: Notify::new(),
                started: OnceLock::new(),
            }),
        }
    }

    #[cfg(feature = "api-server")]
    pub async fn subscribe(
        &self,
        id: &str,
    ) -> Result<(Value, broadcast::Receiver<Value>, RemoteSubscription), String> {
        self.events.started.get_or_init(|| {
            let store = self.store.clone();
            let events = Arc::downgrade(&self.events);
            tokio::spawn(async move { run_listener(store, events).await });
        });
        let deadline = time::Instant::now() + Duration::from_secs(2);
        loop {
            let notified = self.events.ready.notified();
            let watcher = {
                let mut state = self.events.state.lock().await;
                if state.listening {
                    // Subscribe before registration and the persisted snapshot.
                    let watcher = state
                        .watchers
                        .entry(id.to_owned())
                        .or_insert_with(|| {
                            let (sender, _) = broadcast::channel(8);
                            Arc::new(RemoteWatcher {
                                sender,
                                order: Mutex::new(()),
                                hints: Mutex::new(PendingHints::default()),
                            })
                        })
                        .clone();
                    Some((watcher.clone(), watcher.sender.subscribe()))
                } else {
                    None
                }
            };
            if let Some((watcher, receiver)) = watcher {
                // A cancelled HTTP request must also release its in-flight registration.
                let registration = RemoteSubscription {
                    id: id.to_owned(),
                    store: self.store.clone(),
                    events: self.events.clone(),
                };
                let result = async {
                    let _order = watcher.order.lock().await;
                    let live = self
                        .store
                        .register_interest(id, &self.events.channel)
                        .await?;
                    let instance = self.store.get(id).await?.ok_or("unknown instance")?;
                    let snapshot = public_instance(instance);
                    if !live && watcher.sender.receiver_count() > 1 {
                        let _ = watcher.sender.send(snapshot.clone());
                    }
                    Ok::<_, String>(snapshot)
                }
                .await;
                return match result {
                    Ok(snapshot) => Ok((snapshot, receiver, registration)),
                    Err(error) => {
                        drop(receiver);
                        Err(error)
                    }
                };
            }
            time::timeout_at(deadline, notified)
                .await
                .map_err(|_| "SSE listener unavailable".to_string())?;
        }
    }

    pub async fn start(
        &self,
        namespace: &str,
        version: &str,
        name: &str,
        input: Value,
    ) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        self.store
            .admit(&id, namespace, version, name, &input)
            .await?;
        Ok(id)
    }

    pub async fn can_start(
        &self,
        namespace: &str,
        version: &str,
        name: &str,
    ) -> Result<bool, String> {
        self.store.can_admit(namespace, version, name).await
    }

    pub async fn start_with_attachments(
        &self,
        id: String,
        namespace: &str,
        version: &str,
        name: &str,
        input: Value,
        attachments: &[crate::store::Attachment],
    ) -> Result<String, String> {
        self.store
            .admit_with_attachments(&id, namespace, version, name, &input, attachments)
            .await?;
        Ok(id)
    }

    pub async fn start_attachments(
        &self,
        id: &str,
    ) -> Result<Vec<crate::store::Attachment>, String> {
        self.store.attachments(id, None).await
    }

    pub async fn status(&self, id: &str) -> Result<Option<DistributedInstance>, String> {
        self.store.get(id).await
    }

    pub async fn respond(&self, id: &str, request_id: &str, response: Value) -> Result<(), String> {
        self.store.accept_response(id, request_id, &response).await
    }

    pub async fn validate_reply(
        &self,
        id: &str,
        request_id: &str,
        response: &Value,
    ) -> Result<(), String> {
        self.store.validate_reply(id, request_id, response).await
    }

    pub async fn respond_with_attachments(
        &self,
        id: &str,
        request_id: &str,
        response: Value,
        attachments: &[crate::store::Attachment],
    ) -> Result<(), String> {
        self.store
            .accept_response_with_attachments(id, request_id, &response, attachments)
            .await
    }

    pub async fn reply_attachments(
        &self,
        id: &str,
        request_id: &str,
    ) -> Result<Vec<crate::store::Attachment>, String> {
        self.store.attachments(id, Some(request_id)).await
    }

    pub async fn cancel(&self, id: &str) -> Result<(), String> {
        self.store.ensure_supported_incomplete().await?;
        self.store.expire_due().await?;
        self.store.cancel(id).await
    }

    pub async fn reconcile_once(&self) -> Result<usize, String> {
        self.store.ensure_supported_incomplete().await?;
        let mut reconciled = self.store.expire_due().await?.len();
        reconciled += self.store.resume_due_waits().await? as usize;
        for expired in self.store.expired_claims().await? {
            if self
                .store
                .reconcile_expired(&expired.instance.id, None)
                .await?
                .is_some()
            {
                reconciled += 1;
            }
        }
        Ok(reconciled)
    }
}

#[cfg(feature = "api-server")]
fn public_instance(instance: DistributedInstance) -> Value {
    let mut value = serde_json::to_value(instance).expect("serializable instance");
    value.as_object_mut().unwrap().remove("checkpoint");
    value
}

#[cfg(feature = "api-server")]
pub struct RemoteSubscription {
    id: String,
    store: PostgresStore,
    events: Arc<RemoteEvents>,
}

#[cfg(feature = "api-server")]
impl Drop for RemoteSubscription {
    fn drop(&mut self) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            // Without a runtime, the persisted interest expires on its own.
            return;
        };
        let id = self.id.clone();
        let store = self.store.clone();
        let events = self.events.clone();
        runtime.spawn(async move {
            let watcher = events.state.lock().await.watchers.get(&id).cloned();
            if let Some(watcher) = watcher {
                let _order = watcher.order.lock().await;
                if watcher.sender.receiver_count() == 0 {
                    if let Err(error) = store.remove_interest(&id, &events.channel).await {
                        tracing::error!(instance_id = %id, %error, "SSE interest cleanup failed");
                    }
                    let mut state = events.state.lock().await;
                    if watcher.sender.receiver_count() == 0
                        && state
                            .watchers
                            .get(&id)
                            .is_some_and(|current| Arc::ptr_eq(current, &watcher))
                    {
                        state.watchers.remove(&id);
                    }
                }
            }
        });
    }
}

#[cfg(feature = "api-server")]
async fn run_listener(store: PostgresStore, events: std::sync::Weak<RemoteEvents>) {
    while let Some(shared) = events.upgrade() {
        let mut connector = store.listener_connection();
        connector.config.application_name(&shared.channel);
        let result: Result<(), String> = async {
            let (client, mut connection) = connector.connect_raw().await.map_err(|_| "postgres listener connection failed; check endpoint, credentials, TLS support, and certificate trust/hostname/validity".to_string())?;
            let (messages, mut incoming) = mpsc::channel(32);
            tokio::spawn(async move {
                while let Some(message) = std::future::poll_fn(|cx| connection.poll_message(cx)).await {
                    // Never forward raw server errors or notices into logs or the hint queue.
                    let message = match message {
                        Ok(AsyncMessage::Notification(notification)) => Ok(AsyncMessage::Notification(notification)),
                        Ok(_) => continue,
                        Err(_) => Err("postgres listener session failed; check database availability and transport"),
                    };
                    if messages.try_send(message).is_err() {
                        tracing::error!("SSE listener notification buffer overflow; reconnecting");
                        return;
                    }
                }
            });
            // The connection must be polled while LISTEN executes. Autocommit completes
            // before any interest can be registered or snapshot queried.
            client.batch_execute(&format!("LISTEN {}", shared.channel)).await.map_err(|_| "postgres listener LISTEN failed".to_string())?;
            let mut pending_resync: VecDeque<_> = shared.state.lock().await.watchers.clone().into_iter().collect();
            let mut deferred_resync = VecDeque::new();
            let mut resynced = HashSet::new();
            let mut resync = JoinSet::new();
            if pending_resync.is_empty() {
                shared.state.lock().await.listening = true;
                shared.ready.notify_waiters();
            }
            let mut jobs = JoinSet::new();
            // Reserve pool capacity for hints while renewing watched interests.
            let mut renewals = JoinSet::new();
            let mut pending_renewals: VecDeque<(String, Arc<RemoteWatcher>, RefreshMode)> = VecDeque::new();
            let mut deferred_renewals = VecDeque::new();
            let mut scheduled_renewals = HashSet::new();
            let mut renew = time::interval(Duration::from_secs(2));
            renew.tick().await;
            loop {
                while resync.len() < 2 {
                    let Some((id, watcher)) = pending_resync.pop_front() else { break; };
                    let store = store.clone();
                    let channel = shared.channel.clone();
                    resync.spawn(async move {
                        let result = refresh_watcher(&store, &channel, &id, &watcher, RefreshMode::Reconnect).await;
                        (id, watcher, result)
                    });
                }
                while renewals.len() < 2 {
                    let Some((id, watcher, mode)) = pending_renewals.pop_front() else { break; };
                    let store = store.clone();
                    let channel = shared.channel.clone();
                    renewals.spawn(async move {
                        let result = refresh_watcher(&store, &channel, &id, &watcher, mode).await;
                        (id, watcher, result)
                    });
                }
                tokio::select! {
                    result = resync.join_next(), if !resync.is_empty() => {
                        let (id, watcher, result) = result.expect("resync task exists").map_err(|e| e.to_string())?;
                        match result? {
                            Some(()) => { resynced.insert(id); }
                            None => { deferred_resync.push_back((id, watcher)); }
                        }
                        if resync.is_empty() && pending_resync.is_empty() && deferred_resync.is_empty() {
                            shared.state.lock().await.listening = true;
                            shared.ready.notify_waiters();
                        }
                    }
                    result = jobs.join_next(), if !jobs.is_empty() => {
                        result.expect("listener job exists").map_err(|e| e.to_string())??;
                    }
                    result = renewals.join_next(), if !renewals.is_empty() => {
                        let (id, watcher, result) = result.expect("renewal exists").map_err(|e| e.to_string())?;
                        match result? {
                            Some(()) => { scheduled_renewals.remove(&id); }
                            None => {
                                tracing::warn!(instance_id = %id, "SSE interest renewal deferred; retrying");
                                deferred_renewals.push_back((id, watcher, RefreshMode::Retry));
                            }
                        }
                    }
                    message = incoming.recv() => {
                        match message {
                            Some(Ok(AsyncMessage::Notification(hint))) => {
                                let id = hint.payload().to_owned();
                                let watcher = shared.state.lock().await.watchers.get(&id).cloned();
                                if let Some(watcher) = watcher {
                                    // Coalesce duplicate hints, but reread if one arrives during a lookup.
                                    let mut hints = watcher.hints.lock().await;
                                    hints.pending = true;
                                    let start = !hints.running;
                                    hints.running = true;
                                    drop(hints);
                                    if start {
                                        let store = store.clone();
                                        jobs.spawn(async move { deliver_hints(&store, &id, &watcher).await });
                                    }
                                }
                            }
                            Some(Ok(_)) => {},
                            Some(Err(error)) => return Err(error.to_string()),
                            None => return Err("listener connection closed".into()),
                        }
                    }
                    _ = renew.tick() => {
                        if Arc::strong_count(&shared) == 1 {
                            return Ok(());
                        }
                        // Busy resyncs retry on the next tick, behind any first attempts.
                        pending_resync.append(&mut deferred_resync);
                        // Retry at most once per tick; new interests can run after busy retries.
                        pending_renewals.append(&mut deferred_renewals);
                        let state = shared.state.lock().await;
                        for (id, watcher) in &state.watchers {
                            if (state.listening || resynced.contains(id)) && scheduled_renewals.insert(id.clone()) {
                                pending_renewals.push_back((id.clone(), watcher.clone(), RefreshMode::Renew));
                            }
                        }
                    }
                }
            }
        }.await;
        if let Err(error) = result {
            tracing::error!(%error, channel = %shared.channel, "SSE listener disconnected; resynchronizing");
        }
        let watchers = {
            let mut state = shared.state.lock().await;
            state.listening = false;
            state.watchers.values().cloned().collect::<Vec<_>>()
        };
        for watcher in watchers {
            *watcher.hints.lock().await = PendingHints::default();
        }
        drop(shared);
        time::sleep(Duration::from_millis(200)).await;
    }
}

#[cfg(feature = "api-server")]
async fn deliver_hints(
    store: &PostgresStore,
    id: &str,
    watcher: &RemoteWatcher,
) -> Result<(), String> {
    loop {
        let mut hints = watcher.hints.lock().await;
        if !hints.pending {
            hints.running = false;
            return Ok(());
        }
        hints.pending = false;
        drop(hints);
        let _order = watcher.order.lock().await;
        // Hints select only an instance; always query persisted state before delivery.
        if watcher.sender.receiver_count() > 0 && store.get(id).await?.is_some() {
            let _ = watcher.sender.send(serde_json::json!({"id": id}));
        }
    }
}

#[cfg(feature = "api-server")]
async fn refresh_watcher(
    store: &PostgresStore,
    channel: &str,
    id: &str,
    watcher: &RemoteWatcher,
    mode: RefreshMode,
) -> Result<Option<()>, String> {
    // A busy reconnect or retry defers without holding a pool or resync slot.
    let Ok(_order) = watcher.order.try_lock() else {
        return Ok(matches!(mode, RefreshMode::Renew).then_some(()));
    };
    if watcher.sender.receiver_count() == 0 {
        return Ok(Some(()));
    }
    let live = store.renew_interest(id, channel).await?;
    let Some(live) = live else {
        return Ok(None);
    };
    if (matches!(mode, RefreshMode::Reconnect | RefreshMode::Retry) || !live)
        && let Some(instance) = store.get(id).await?
    {
        let _ = watcher.sender.send(public_instance(instance));
    }
    Ok(Some(()))
}

#[cfg(all(test, feature = "api-server"))]
#[tokio::test]
async fn notify_waiters_wakes_a_created_but_unpolled_readiness_waiter() {
    let ready = Notify::new();
    let notified = ready.notified();
    ready.notify_waiters();
    time::timeout(Duration::from_millis(100), notified)
        .await
        .expect("readiness notification was lost before first poll");
}

#[cfg(feature = "worker")]
pub struct DistributedWorker {
    store: PostgresStore,
    id: String,
    definitions: Arc<NamedRegistry>,
    permits: Arc<Semaphore>,
    limit: usize,
    lease_ms: i64,
}

#[cfg(feature = "worker")]
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
        let policies: Vec<_> = self
            .definitions
            .values()
            .map(|graph| {
                (
                    graph.namespace,
                    graph.version,
                    graph.name,
                    graph.retry.as_ref(),
                    graph.deadline.as_ref(),
                )
            })
            .collect();
        self.store
            .register_worker_with_policies(&self.id, &policies)
            .await
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
        self.maintain().await?;
        let mut count = 0;
        let mut active = HashMap::new();
        let mut tasks = JoinSet::new();
        let mut heartbeat =
            tokio::time::interval(Duration::from_millis((self.lease_ms / 3).max(1) as u64));
        let mut poll = tokio::time::interval_at(
            tokio::time::Instant::now() + Duration::from_millis(100),
            Duration::from_millis(100),
        );
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut maintenance = tokio::time::interval_at(
            tokio::time::Instant::now() + Duration::from_secs(1),
            Duration::from_secs(1),
        );
        maintenance.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut claim_available = true;
        let mut pending_error = None;
        loop {
            if claim_available && active.len() < self.limit && pending_error.is_none() {
                claim_available = false;
                let claimed = match self
                    .store
                    .claim(&self.id, self.limit - active.len(), self.lease_ms)
                    .await
                {
                    Ok(claimed) => claimed,
                    Err(error) if active.is_empty() => return Err(error),
                    Err(error) => {
                        pending_error = Some(error);
                        continue;
                    }
                };
                count += claimed.len();
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
                            execute_claimed(
                                graph,
                                instance,
                                store,
                                worker_id,
                                permits,
                                definitions,
                            )
                            .await,
                        )
                    });
                }
                if active.is_empty() {
                    return Ok(count);
                }
            }
            if active.is_empty() {
                return Err(pending_error.expect("active claims ended after an error"));
            }
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
                    claim_available = true;
                    if let Err(error) = result {
                        let reason = if error == "lost claim during execution" {
                            "claim lost during execution"
                        } else {
                            "execution infrastructure failure"
                        };
                        tracing::error!(worker_id = %self.id, instance_id = %id, reason, "instance execution stopped");
                    }
                }
                _ = poll.tick(), if active.len() < self.limit && pending_error.is_none() => { claim_available = true; }
                _ = maintenance.tick(), if pending_error.is_none() => {
                    if let Err(error) = self.maintain().await {
                        pending_error = Some(error);
                    }
                }
            }
        }
    }

    async fn maintain(&self) -> Result<(), String> {
        self.store.ensure_supported_incomplete().await?;
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
        Ok(())
    }
}
