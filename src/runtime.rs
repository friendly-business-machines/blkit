use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use futures_util::future::try_join_all;
use serde_json::Value;
use tokio::sync::{Mutex, Semaphore};

pub use crate::store::{Instance, Store};

pub type Values = HashMap<String, Value>;
pub type Evaluate = Arc<dyn Fn(&Value, &Values) -> Result<Value, String> + Send + Sync>;
pub type Cancel = Arc<dyn Fn() + Send + Sync>;

pub struct Definition {
    pub namespace: &'static str,
    pub version: &'static str,
    pub name: &'static str,
    pub steps: Vec<Step>,
    pub decode_input: Box<dyn Fn(Value) -> Result<Value, String> + Send + Sync>,
}

pub enum Step {
    Run { name: &'static str, call: Evaluate, cancel: Cancel },
    Gateway { kind: &'static str, branches: Vec<Branch>, join: &'static str },
    Return(Evaluate),
}

pub struct Branch {
    pub label: Option<&'static str>,
    pub condition: Option<Evaluate>,
    pub steps: Vec<Step>,
}

pub struct Registry(HashMap<(String, String, String), Arc<Definition>>);

impl Registry {
    pub fn new(definitions: Vec<Definition>) -> Result<Self, String> {
        let mut entries = HashMap::new();
        for definition in definitions {
            let key = (definition.namespace.into(), definition.version.into(), definition.name.into());
            if entries.insert(key, Arc::new(definition)).is_some() {
                return Err("duplicate process identity".into());
            }
        }
        Ok(Self(entries))
    }

    pub fn get(&self, namespace: &str, version: &str, name: &str) -> Option<Arc<Definition>> {
        self.0.get(&(namespace.into(), version.into(), name.into())).cloned()
    }
}

struct Running {
    status: &'static str,
    next: usize,
    in_flight: HashMap<usize, Cancel>,
}

#[derive(Clone)]
struct Context {
    state: Arc<Mutex<Running>>,
    store: Option<Store>,
    id: String,
}

impl Context {
    fn standalone() -> Self {
        Self { state: Arc::new(Mutex::new(Running { status: "running", next: 0, in_flight: HashMap::new() })), store: None, id: String::new() }
    }

    async fn stop(&self, status: &'static str, error: Option<&str>) -> Result<(), String> {
        let mut state = self.state.lock().await;
        if !matches!(state.status, "pending" | "running") { return Ok(()); }
        if let Some(store) = &self.store { store.finish(&self.id, status, None, error).await?; }
        state.status = status;
        let hooks: Vec<_> = state.in_flight.values().cloned().collect();
        drop(state);
        for hook in hooks { hook(); }
        Ok(())
    }

    async fn complete(&self, value: Value) -> Result<(), String> {
        let mut state = self.state.lock().await;
        if !matches!(state.status, "pending" | "running") { return Ok(()); }
        if let Some(store) = &self.store { store.finish(&self.id, "completed", Some(value), None).await?; }
        state.status = "completed";
        Ok(())
    }
}

impl Definition {
    pub async fn evaluate(&self, input: Value) -> Result<Value, String> {
        self.evaluate_limited(input, Arc::new(Semaphore::new(32))).await
    }

    pub async fn evaluate_limited(&self, input: Value, permits: Arc<Semaphore>) -> Result<Value, String> {
        let input = (self.decode_input)(input)?;
        let mut values = Values::new();
        execute(&self.steps, &input, &mut values, &permits, &Context::standalone()).await
    }
}

pub struct Engine {
    registry: Registry,
    store: Store,
    permits: Arc<Semaphore>,
    active: Arc<Mutex<HashMap<String, Context>>>,
}

impl Engine {
    pub fn new(registry: Registry, store: Store, limit: usize) -> Result<Self, String> {
        if limit == 0 { return Err("task limit must be positive".into()); }
        Ok(Self { registry, store, permits: Arc::new(Semaphore::new(limit)), active: Arc::new(Mutex::new(HashMap::new())) })
    }

    pub async fn start(&self, namespace: &str, version: &str, name: &str, input: Value) -> Result<String, String> {
        let definition = self.registry.get(namespace, version, name).ok_or("unknown process")?;
        let input = (definition.decode_input)(input).map_err(|error| format!("invalid input: {error}"))?;
        let id = uuid::Uuid::new_v4().to_string();
        self.store.create(&Instance::new(&id, namespace, version, name, input.clone())).await?;
        let context = Context { state: Arc::new(Mutex::new(Running { status: "pending", next: 0, in_flight: HashMap::new() })), store: Some(self.store.clone()), id: id.clone() };
        self.active.lock().await.insert(id.clone(), context.clone());
        let permits = self.permits.clone();
        let active = self.active.clone();
        let instance_id = id.clone();
        tokio::spawn(async move {
            let mut values = Values::new();
            let result = execute(&definition.steps, &input, &mut values, &permits, &context).await;
            match result {
                Ok(value) => { let _ = context.complete(value).await; }
                Err(error) => { let _ = context.stop("failed", Some(&error)).await; }
            }
            active.lock().await.remove(&instance_id);
        });
        Ok(id)
    }

    pub async fn status(&self, id: &str) -> Result<Option<Instance>, String> { self.store.get(id).await }

    pub async fn cancel(&self, id: &str) -> Result<(), String> {
        let context = self.active.lock().await.get(id).cloned();
        let Some(context) = context else {
            return match self.store.get(id).await? {
                None => Err("unknown instance".into()),
                Some(item) if item.status == "cancelled" => Ok(()),
                Some(_) => Err("instance already terminal".into()),
            };
        };
        let mut state = context.state.lock().await;
        if matches!(state.status, "cancelled" | "cancelling") { return Ok(()); }
        if matches!(state.status, "completed" | "failed") { return Err("instance already terminal".into()); }
        self.store.finish(id, "cancelling", None, None).await?;
        state.status = "cancelling";
        let hooks: Vec<_> = state.in_flight.values().cloned().collect();
        drop(state);
        for hook in hooks { hook(); }
        let mut state = context.state.lock().await;
        self.store.finish(id, "cancelled", None, None).await?;
        state.status = "cancelled";
        Ok(())
    }
}

fn execute<'a>(
    steps: &'a [Step], input: &'a Value, values: &'a mut Values, permits: &'a Arc<Semaphore>, context: &'a Context,
) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>> {
    Box::pin(async move {
        let mut last = None;
        for step in steps {
            match step {
                Step::Run { name, call, cancel } => {
                    let permit = permits.clone().acquire_owned().await.map_err(|e| e.to_string())?;
                    let mut state = context.state.lock().await;
                    if !matches!(state.status, "pending" | "running") { return Err("instance cancelled".into()); }
                    if state.status == "pending" {
                        if let Some(store) = &context.store { store.finish(&context.id, "running", None, None).await?; }
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
                    }).await.map_err(|e| e.to_string()).and_then(|result| result);
                    let mut state = context.state.lock().await;
                    state.in_flight.remove(&key);
                    if state.status != "running" { return Err("instance cancelled".into()); }
                    drop(state);
                    let result = match result {
                        Ok(value) => value,
                        Err(error) => { context.stop("failed", Some(&error)).await?; return Err(error); }
                    };
                    values.insert((*name).into(), result.clone());
                    last = Some(result);
                }
                Step::Gateway { kind, branches, join } => {
                    let state = context.state.lock().await;
                    if !matches!(state.status, "pending" | "running") { return Err("instance cancelled".into()); }
                    let selected: Vec<usize> = match *kind {
                        "and" => (0..branches.len()).collect(),
                        "xor" | "or" => {
                            let mut matches = Vec::new();
                            let mut fallback = None;
                            for (index, branch) in branches.iter().enumerate() {
                                match &branch.condition {
                                    Some(condition) if condition(input, values)? == Value::Bool(true) => {
                                        matches.push(index);
                                        if *kind == "xor" { break; }
                                    }
                                    None => fallback = Some(index),
                                    _ => {}
                                }
                            }
                            if matches.is_empty() { vec![fallback.ok_or("missing gateway fallback")?] } else { matches }
                        }
                        _ => return Err(format!("unknown gateway: {kind}")),
                    };
                    drop(state);
                    let pending = selected.into_iter().map(|index| {
                        let mut branch_values = values.clone();
                        let context = context.clone();
                        async move {
                            execute(&branches[index].steps, input, &mut branch_values, permits, &context).await.map(|result| (index, result))
                        }
                    });
                    let results: Vec<_> = try_join_all(pending).await?;
                    let result = match *kind {
                        "and" => Value::Object(results.into_iter().map(|(index, value)|
                            (branches[index].label.unwrap_or_default().into(), value)).collect()),
                        "or" => Value::Array(results.into_iter().map(|(_, value)| value).collect()),
                        _ => results.into_iter().next().ok_or("empty gateway")?.1,
                    };
                    let state = context.state.lock().await;
                    if state.status != "running" { return Err("instance cancelled".into()); }
                    values.insert((*join).into(), result.clone());
                    last = Some(result);
                }
                Step::Return(calculate) => {
                    let state = context.state.lock().await;
                    if !matches!(state.status, "pending" | "running") { return Err("instance cancelled".into()); }
                    return calculate(input, values);
                }
            }
        }
        last.ok_or("empty graph branch".into())
    })
}
