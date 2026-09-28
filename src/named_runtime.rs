use std::{collections::HashMap, time::Duration};

use crate::runtime::{Evaluate, Values};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub struct GraphDefinition {
    pub namespace: &'static str,
    pub version: &'static str,
    pub name: &'static str,
    pub retry: Option<crate::RetryPolicy>,
    pub deadline: Option<crate::DeadlinePolicy>,
    pub decode_input: Box<dyn Fn(Value) -> Result<Value, String> + Send + Sync>,
    pub nodes: Vec<GraphNode>,
    pub links: Vec<GraphLink>,
}

pub struct GraphNode {
    pub name: &'static str,
    pub kind: GraphNodeKind,
}

pub struct LoopPolicy {
    pub condition: Evaluate,
    pub initial: Option<Evaluate>,
    pub before: bool,
    pub max_iterations: Option<u32>,
    pub max_duration: Option<Duration>,
}

pub enum GraphNodeKind {
    Start,
    Task(Evaluate),
    MultiInstance {
        task: Evaluate,
        items: Evaluate,
        parallel: bool,
    },
    TaskLoop(Evaluate, LoopPolicy),
    TaskWithCancel(Evaluate, crate::runtime::Cancel),
    PauseFor(Duration),
    PauseUntil(Evaluate),
    Split(&'static str),
    Join {
        kind: &'static str,
        split: &'static str,
    },
    End,
    Error,
    Cancel,
    Terminate,
}

pub struct GraphLink {
    pub source: &'static str,
    pub target: &'static str,
    pub value: Option<Evaluate>,
    pub condition: Option<Evaluate>,
    pub fallback: bool,
    pub label: Option<&'static str>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Activation {
    node: String,
    path: Vec<(String, usize)>,
    #[serde(default)]
    id: u64,
    #[serde(default)]
    values: Values,
    #[serde(default)]
    generations: Vec<u64>,
    #[serde(default)]
    iterations: u32,
    #[serde(default)]
    started_at_ms: i64,
    #[serde(default)]
    batch_id: Option<u64>,
    #[serde(default)]
    item_index: Option<usize>,
    #[serde(default)]
    item: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct WaitActivation {
    token: Activation,
    at_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct MultiProgress {
    node: String,
    path: Vec<(String, usize)>,
    generations: Vec<u64>,
    values: Values,
    items: Vec<Value>,
    results: Vec<Option<Value>>,
    parallel: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphTerminal {
    Error(String),
    Cancel(String),
    Terminate(String),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GraphCheckpoint {
    #[serde(default)]
    version: u8,
    #[serde(default)]
    next_activation_id: u64,
    ready: Vec<Activation>,
    #[serde(default)]
    pending: Vec<Activation>,
    #[serde(default)]
    waiting: Vec<WaitActivation>,
    pub completed: Values,
    pub selected: HashMap<String, Vec<usize>>,
    progress: HashMap<String, Vec<(usize, Value)>>,
    #[serde(default)]
    route_progress: HashMap<String, Vec<(usize, Value, Values)>>,
    // ponytail: committed activation history grows with visits; compact per generation if long-lived cycles become common.
    #[serde(default)]
    committed: HashMap<u64, Value>,
    #[serde(default)]
    multi: HashMap<u64, MultiProgress>,
    pub outcome: Option<Value>,
    pub terminal: Option<GraphTerminal>,
}

impl GraphDefinition {
    pub fn checkpoint(&self, input: &Value) -> Result<GraphCheckpoint, String> {
        let mut state = GraphCheckpoint {
            version: 2,
            next_activation_id: 1,
            ..Default::default()
        };
        let start = self
            .nodes
            .iter()
            .find(|node| matches!(node.kind, GraphNodeKind::Start))
            .ok_or("missing start")?;
        self.advance_with_budget(
            input,
            &mut state,
            start.name,
            Vec::new(),
            Vec::new(),
            Values::new(),
            &mut 64,
        )?;
        Ok(state)
    }

    pub fn run(&self, input: &Value, state: &mut GraphCheckpoint) -> Result<Value, String> {
        self.migrate_checkpoint(state);
        let deadline = self
            .deadline
            .as_ref()
            .map(|policy| {
                let millis =
                    i64::try_from(policy.duration.as_millis()).map_err(|_| "deadline overflow")?;
                crate::store::now_ms()
                    .checked_add(millis)
                    .ok_or("deadline overflow")
            })
            .transpose()?;
        let mut immediate_slices = 0usize;
        while state.outcome.is_none() {
            if deadline.is_some_and(|at| crate::store::now_ms() >= at) {
                state.terminal = Some(GraphTerminal::Error("timeout".into()));
            }
            self.check_loop_bounds(state, crate::store::now_ms());
            if let Some(terminal) = &state.terminal {
                return Err(format!("terminal event: {terminal:?}"));
            }
            if state.ready.is_empty() && self.has_pending(state) {
                immediate_slices += 1;
                if deadline.is_none()
                    && immediate_slices > self.nodes.len().saturating_mul(2).max(1)
                {
                    return Err("unbounded immediate graph cycle requires deadline".into());
                }
                self.resume_pending(input, state)?;
                continue;
            }
            let token = state
                .ready
                .first()
                .ok_or("graph has no active task or terminal")?;
            let name = token.node.clone();
            let id = token.id;
            let node = self
                .nodes
                .iter()
                .find(|node| node.name == name)
                .ok_or_else(|| format!("unknown task: {name}"))?;
            let call = match &node.kind {
                GraphNodeKind::Task(call)
                | GraphNodeKind::TaskWithCancel(call, _)
                | GraphNodeKind::TaskLoop(call, _) => call,
                GraphNodeKind::MultiInstance { task, .. } => task,
                _ => return Err(format!("not a task: {name}")),
            };
            let output = call(
                &self.activation_input(state, id, input)?,
                &self.activation_values(state, id)?,
            )?;
            self.complete_activation(input, state, id, output)?;
            immediate_slices = 0;
        }
        if deadline.is_some_and(|at| crate::store::now_ms() >= at) {
            state.outcome = None;
            state.terminal = Some(GraphTerminal::Error("timeout".into()));
            return Err("terminal event: Error(\"timeout\")".into());
        }
        Ok(state.outcome.clone().unwrap())
    }

    pub fn migrate_checkpoint(&self, state: &mut GraphCheckpoint) {
        if state.version != 0 {
            return;
        }
        let mut next = state.next_activation_id.max(1);
        for token in state
            .ready
            .iter_mut()
            .chain(state.pending.iter_mut())
            .chain(state.waiting.iter_mut().map(|wait| &mut wait.token))
        {
            token.id = next;
            next += 1;
            token.values = state.completed.clone();
            token.generations = vec![0; token.path.len()];
        }
        state.next_activation_id = next;
        for (name, values) in std::mem::take(&mut state.progress) {
            state.route_progress.insert(
                name,
                values
                    .into_iter()
                    .map(|(branch, value)| (branch, value, state.completed.clone()))
                    .collect(),
            );
        }
        state.version = 2;
    }

    fn activation(
        state: &mut GraphCheckpoint,
        name: &str,
        path: Vec<(String, usize)>,
        generations: Vec<u64>,
        values: Values,
    ) -> Activation {
        let id = state.next_activation_id;
        state.next_activation_id += 1;
        Activation {
            node: name.into(),
            path,
            id,
            values,
            generations,
            iterations: 0,
            started_at_ms: 0,
            batch_id: None,
            item_index: None,
            item: None,
        }
    }

    fn loop_bound_reached(policy: &LoopPolicy, token: &Activation, now_ms: i64) -> bool {
        policy
            .max_iterations
            .is_some_and(|limit| token.iterations >= limit)
            || policy.max_duration.is_some_and(|limit| {
                i64::try_from(limit.as_millis())
                    .is_ok_and(|ms| now_ms.saturating_sub(token.started_at_ms) >= ms)
            })
    }

    pub fn check_loop_bounds(&self, state: &mut GraphCheckpoint, now_ms: i64) {
        if state.terminal.is_some() || state.outcome.is_some() {
            return;
        }
        if state.ready.iter().any(|token| self.nodes.iter().any(|node| node.name == token.node && matches!(&node.kind, GraphNodeKind::TaskLoop(_, policy) if Self::loop_bound_reached(policy, token, now_ms)))) {
            state.terminal = Some(GraphTerminal::Error("task-iteration-limit".into()));
        }
    }

    pub fn has_pending(&self, state: &GraphCheckpoint) -> bool {
        !state.pending.is_empty()
    }

    pub fn resume_pending(&self, input: &Value, state: &mut GraphCheckpoint) -> Result<(), String> {
        let mut next = state.clone();
        self.migrate_checkpoint(&mut next);
        let mut budget = 64;
        let mut deferred = Vec::new();
        for token in std::mem::take(&mut next.pending) {
            if budget == 0 || next.outcome.is_some() || next.terminal.is_some() {
                deferred.push(token);
                continue;
            }
            self.advance_with_budget(
                input,
                &mut next,
                &token.node,
                token.path,
                token.generations,
                token.values,
                &mut budget,
            )?;
        }
        deferred.append(&mut next.pending);
        next.pending = deferred;
        *state = next;
        Ok(())
    }

    pub fn ready_activations<'a>(&self, state: &'a GraphCheckpoint) -> Vec<(u64, &'a str)> {
        state
            .ready
            .iter()
            .map(|token| (token.id, token.node.as_str()))
            .collect()
    }

    pub fn activation_input(
        &self,
        state: &GraphCheckpoint,
        id: u64,
        input: &Value,
    ) -> Result<Value, String> {
        let token = state
            .ready
            .iter()
            .find(|token| token.id == id)
            .ok_or("task activation not ready")?;
        Ok(token.item.clone().unwrap_or_else(|| input.clone()))
    }

    pub fn activation_values(&self, state: &GraphCheckpoint, id: u64) -> Result<Values, String> {
        let token = state
            .ready
            .iter()
            .find(|token| token.id == id)
            .ok_or("task activation not ready")?;
        Ok(if state.version == 0 {
            state.completed.clone()
        } else {
            token.values.clone()
        })
    }

    pub fn waiting_until(&self, state: &GraphCheckpoint) -> Option<i64> {
        state.waiting.iter().map(|wait| wait.at_ms).min()
    }

    pub fn resume_due(
        &self,
        input: &Value,
        state: &mut GraphCheckpoint,
        at_ms: i64,
    ) -> Result<(), String> {
        if state.outcome.is_some() || state.terminal.is_some() {
            return Ok(());
        }
        let mut next = state.clone();
        self.migrate_checkpoint(&mut next);
        let mut future = Vec::new();
        let waiting = std::mem::take(&mut next.waiting);
        for wait in waiting {
            if wait.at_ms <= at_ms {
                self.advance(
                    input,
                    &mut next,
                    &wait.token.node,
                    wait.token.path,
                    wait.token.generations,
                    wait.token.values,
                )?;
            } else {
                future.push(wait);
            }
        }
        next.waiting.extend(future);
        *state = next;
        Ok(())
    }

    pub fn ready<'a>(&self, state: &'a GraphCheckpoint) -> Vec<&'a str> {
        state
            .ready
            .iter()
            .map(|token| token.node.as_str())
            .collect()
    }

    pub fn complete(
        &self,
        input: &Value,
        state: &mut GraphCheckpoint,
        name: &str,
        value: Value,
    ) -> Result<(), String> {
        let mut next = state.clone();
        self.migrate_checkpoint(&mut next);
        let id = next
            .ready
            .iter()
            .find(|token| token.node == name)
            .ok_or_else(|| format!("task not ready: {name}"))?
            .id;
        self.complete_activation(input, state, id, value)
    }

    pub fn complete_activation(
        &self,
        input: &Value,
        state: &mut GraphCheckpoint,
        id: u64,
        value: Value,
    ) -> Result<(), String> {
        // ponytail: copy the checkpoint per transition; replace with store-backed atomic updates if size becomes costly.
        let mut next = state.clone();
        self.migrate_checkpoint(&mut next);
        if next.outcome.is_some() || next.terminal.is_some() {
            return Err("graph already terminal".into());
        }
        let index = next
            .ready
            .iter()
            .position(|token| token.id == id)
            .ok_or_else(|| format!("task activation not ready: {id}"))?;
        let mut token = next.ready.remove(index);
        token.values.insert(token.node.clone(), value.clone());
        next.committed.insert(id, value.clone());
        if let Some(batch_id) = token.batch_id {
            let mut batch = next
                .multi
                .remove(&batch_id)
                .ok_or("missing multi-instance batch")?;
            let index = token.item_index.ok_or("missing multi-instance index")?;
            if batch.results.get(index).is_none_or(Option::is_some) {
                return Err("multi-instance item already committed".into());
            }
            batch.results[index] = Some(value);
            if batch.results.iter().all(Option::is_some) {
                let output = Value::Array(
                    batch
                        .results
                        .into_iter()
                        .map(|result| result.unwrap())
                        .collect(),
                );
                next.completed.insert(batch.node.clone(), output.clone());
                let mut values = batch.values;
                values.insert(batch.node.clone(), output);
                self.advance(
                    input,
                    &mut next,
                    &batch.node,
                    batch.path,
                    batch.generations,
                    values,
                )?;
            } else {
                if !batch.parallel {
                    let next_index = index + 1;
                    let mut pending = Self::activation(
                        &mut next,
                        &batch.node,
                        batch.path.clone(),
                        batch.generations.clone(),
                        batch.values.clone(),
                    );
                    pending.batch_id = Some(batch_id);
                    pending.item_index = Some(next_index);
                    pending.item = Some(
                        batch
                            .items
                            .get(next_index)
                            .ok_or("missing next multi-instance item")?
                            .clone(),
                    );
                    next.ready.push(pending);
                }
                next.multi.insert(batch_id, batch);
            }
            *state = next;
            return Ok(());
        }
        next.completed.insert(token.node.clone(), value.clone());
        if let Some(GraphNodeKind::TaskLoop(_, policy)) = self
            .nodes
            .iter()
            .find(|node| node.name == token.node)
            .map(|node| &node.kind)
        {
            token.iterations = token
                .iterations
                .checked_add(1)
                .ok_or("task iteration count overflow")?;
            let repeat = (policy.condition)(input, &token.values)?
                .as_bool()
                .ok_or("task loop condition must be Bool")?;
            if repeat {
                if Self::loop_bound_reached(policy, &token, crate::store::now_ms()) {
                    next.terminal = Some(GraphTerminal::Error("task-iteration-limit".into()));
                } else {
                    token.id = next.next_activation_id;
                    next.next_activation_id += 1;
                    next.ready.push(token);
                }
            } else {
                self.advance(
                    input,
                    &mut next,
                    &token.node,
                    token.path,
                    token.generations,
                    token.values,
                )?;
            }
        } else {
            self.advance(
                input,
                &mut next,
                &token.node,
                token.path,
                token.generations,
                token.values,
            )?;
        }
        *state = next;
        Ok(())
    }

    fn advance(
        &self,
        input: &Value,
        state: &mut GraphCheckpoint,
        source: &str,
        path: Vec<(String, usize)>,
        generations: Vec<u64>,
        values: Values,
    ) -> Result<(), String> {
        self.advance_with_budget(input, state, source, path, generations, values, &mut 64)
    }

    fn advance_with_budget(
        &self,
        input: &Value,
        state: &mut GraphCheckpoint,
        source: &str,
        path: Vec<(String, usize)>,
        generations: Vec<u64>,
        values: Values,
        budget: &mut usize,
    ) -> Result<(), String> {
        if *budget == 0 {
            let token = Self::activation(state, source, path, generations, values);
            state.pending.push(token);
            return Ok(());
        }
        *budget -= 1;
        let outgoing: Vec<_> = self
            .links
            .iter()
            .enumerate()
            .filter(|(_, link)| link.source == source)
            .collect();
        let kind = &self
            .nodes
            .iter()
            .find(|node| node.name == source)
            .ok_or_else(|| format!("unknown node: {source}"))?
            .kind;
        let selected: Vec<usize> = match kind {
            GraphNodeKind::Split("and") => outgoing.iter().map(|(index, _)| *index).collect(),
            GraphNodeKind::Split("xor" | "or") => {
                let mut matches = Vec::new();
                let mut fallback = None;
                for (index, link) in &outgoing {
                    if link.fallback {
                        fallback = Some(*index);
                    } else if link.condition.as_ref().ok_or("missing condition")?(input, &values)?
                        == Value::Bool(true)
                    {
                        matches.push(*index);
                        if matches!(kind, GraphNodeKind::Split("xor")) {
                            break;
                        }
                    }
                }
                if matches.is_empty() {
                    vec![fallback.ok_or("missing fallback")?]
                } else {
                    matches
                }
            }
            GraphNodeKind::Split(other) => return Err(format!("unknown split: {other}")),
            _ => outgoing.iter().map(|(index, _)| *index).collect(),
        };
        let generation = if matches!(kind, GraphNodeKind::Split(_)) {
            let id = state.next_activation_id;
            state.next_activation_id += 1;
            state
                .selected
                .insert(format!("{source}#{id}"), selected.clone());
            Some(id)
        } else {
            None
        };
        for index in selected {
            let link = &self.links[index];
            let mut route = path.clone();
            let mut route_generations = generations.clone();
            if let Some(id) = generation {
                route.push((source.into(), index));
                route_generations.push(id);
            }
            self.enter(
                input,
                state,
                link,
                route,
                route_generations,
                values.clone(),
                budget,
            )?;
        }
        Ok(())
    }

    fn enter(
        &self,
        input: &Value,
        state: &mut GraphCheckpoint,
        link: &GraphLink,
        mut path: Vec<(String, usize)>,
        mut generations: Vec<u64>,
        values: Values,
        budget: &mut usize,
    ) -> Result<(), String> {
        let node = self
            .nodes
            .iter()
            .find(|node| node.name == link.target)
            .ok_or_else(|| format!("unknown node: {}", link.target))?;
        match &node.kind {
            GraphNodeKind::Start => Err("link into start".into()),
            GraphNodeKind::Task(_) | GraphNodeKind::TaskWithCancel(_, _) => {
                let token = Self::activation(state, node.name, path, generations, values);
                state.ready.push(token);
                Ok(())
            }
            GraphNodeKind::MultiInstance {
                items, parallel, ..
            } => {
                let entries = items(input, &values)?
                    .as_array()
                    .cloned()
                    .ok_or("multi-instance requires List input")?;
                if entries.is_empty() {
                    let mut values = values;
                    values.insert(node.name.into(), Value::Array(Vec::new()));
                    state
                        .completed
                        .insert(node.name.into(), Value::Array(Vec::new()));
                    return self.advance_with_budget(
                        input,
                        state,
                        node.name,
                        path,
                        generations,
                        values,
                        budget,
                    );
                }
                let batch_id = state.next_activation_id;
                state.next_activation_id += 1;
                let total = entries.len();
                state.multi.insert(
                    batch_id,
                    MultiProgress {
                        node: node.name.into(),
                        path: path.clone(),
                        generations: generations.clone(),
                        values: values.clone(),
                        items: entries.clone(),
                        results: vec![None; total],
                        parallel: *parallel,
                    },
                );
                for index in 0..if *parallel { total } else { 1 } {
                    let mut token = Self::activation(
                        state,
                        node.name,
                        path.clone(),
                        generations.clone(),
                        values.clone(),
                    );
                    token.batch_id = Some(batch_id);
                    token.item_index = Some(index);
                    token.item = Some(entries[index].clone());
                    state.ready.push(token);
                }
                Ok(())
            }
            GraphNodeKind::TaskLoop(_, policy) => {
                let mut values = values;
                if policy.before {
                    let initial = (policy
                        .initial
                        .as_ref()
                        .ok_or("pre-check task loop requires initial result")?)(
                        input, &values
                    )?;
                    values.insert(node.name.into(), initial.clone());
                    if !(policy.condition)(input, &values)?
                        .as_bool()
                        .ok_or("task loop condition must be Bool")?
                    {
                        state.completed.insert(node.name.into(), initial);
                        return self.advance_with_budget(
                            input,
                            state,
                            node.name,
                            path,
                            generations,
                            values,
                            budget,
                        );
                    }
                }
                let mut token = Self::activation(state, node.name, path, generations, values);
                token.started_at_ms = crate::store::now_ms();
                state.ready.push(token);
                Ok(())
            }
            GraphNodeKind::PauseFor(duration) => {
                let millis = i64::try_from(duration.as_millis()).map_err(|e| e.to_string())?;
                let at_ms = crate::store::now_ms()
                    .checked_add(millis)
                    .ok_or("wait duration overflow")?;
                let token = Self::activation(state, node.name, path, generations, values);
                state.waiting.push(WaitActivation { token, at_ms });
                Ok(())
            }
            GraphNodeKind::PauseUntil(evaluate) => {
                let value = evaluate(input, &values)?;
                let text = value.as_str().ok_or("pause_until requires DateTime")?;
                let at_ms = chrono::DateTime::parse_from_rfc3339(text)
                    .map_err(|e| e.to_string())?
                    .timestamp_millis();
                let token = Self::activation(state, node.name, path, generations, values);
                state.waiting.push(WaitActivation { token, at_ms });
                Ok(())
            }
            GraphNodeKind::Split(_) => {
                self.advance_with_budget(input, state, node.name, path, generations, values, budget)
            }
            GraphNodeKind::Join { kind, split } => {
                let (route_split, branch) = path.pop().ok_or("join without split")?;
                let generation = generations.pop().unwrap_or(0);
                if route_split != *split {
                    return Err(format!("wrong split at join {}", node.name));
                }
                let selected_key = if generation == 0 {
                    split.to_string()
                } else {
                    format!("{split}#{generation}")
                };
                let progress_key = if generation == 0 {
                    node.name.into()
                } else {
                    format!("{}#{generation}", node.name)
                };
                let selected = state
                    .selected
                    .get(&selected_key)
                    .ok_or("missing split generation")?
                    .clone();
                let value = link.value.as_ref().ok_or("join needs value")?(input, &values)?;
                let progress = state
                    .route_progress
                    .entry(progress_key.clone())
                    .or_default();
                if !selected.contains(&branch)
                    || progress.iter().any(|(seen, _, _)| *seen == branch)
                {
                    return Err("duplicate or unexpected join branch".into());
                }
                progress.push((branch, value, values));
                if progress.len() < selected.len() {
                    return Ok(());
                }
                let result = match *kind {
                    "and" => Value::Object(
                        selected
                            .iter()
                            .map(|index| {
                                let label =
                                    self.links[*index].label.ok_or("AND branch needs label")?;
                                let value = progress
                                    .iter()
                                    .find(|(branch, _, _)| branch == index)
                                    .ok_or("missing AND branch")?
                                    .1
                                    .clone();
                                Ok((label.into(), value))
                            })
                            .collect::<Result<Map<String, Value>, String>>()?,
                    ),
                    "or" => Value::Array(
                        selected
                            .iter()
                            .map(|index| {
                                progress
                                    .iter()
                                    .find(|(branch, _, _)| branch == index)
                                    .map(|(_, value, _)| value.clone())
                                    .ok_or("missing OR branch")
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                    "xor" => progress[0].1.clone(),
                    other => return Err(format!("unknown join: {other}")),
                };
                let mut common = progress[0].2.clone();
                common.retain(|name, value| {
                    progress
                        .iter()
                        .all(|(_, _, route)| route.get(name) == Some(value))
                });
                state.route_progress.remove(&progress_key);
                state.selected.remove(&selected_key);
                state.completed.insert(node.name.into(), result.clone());
                common.insert(node.name.into(), result);
                self.advance_with_budget(input, state, node.name, path, generations, common, budget)
            }
            GraphNodeKind::End => {
                state.outcome = Some(link.value.as_ref().ok_or("end needs value")?(
                    input, &values,
                )?);
                Ok(())
            }
            GraphNodeKind::Error => {
                state.terminal = Some(GraphTerminal::Error(node.name.into()));
                Ok(())
            }
            GraphNodeKind::Cancel => {
                state.terminal = Some(GraphTerminal::Cancel(node.name.into()));
                Ok(())
            }
            GraphNodeKind::Terminate => {
                state.terminal = Some(GraphTerminal::Terminate(node.name.into()));
                Ok(())
            }
        }
    }
}
