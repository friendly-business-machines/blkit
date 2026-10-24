use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

use crate::evaluation::{AsyncEvaluate, Cancel, Evaluate, Values};
use crate::temporal::{Calendar, CalendarEntry, DTDuration, Date, DateTime, Time, YMDuration};
use rust_decimal::Decimal;
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestType {
    Named(String),
    List(Box<RequestType>),
    Dictionary(Box<RequestType>),
    DictionaryEntry(Box<RequestType>),
    Nullable(Box<RequestType>),
    Table(Vec<(String, RequestType)>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestSchema {
    pub ports: BTreeMap<String, RequestType>,
    pub records: BTreeMap<String, BTreeMap<String, RequestType>>,
    pub enums: BTreeMap<String, Vec<String>>,
}

impl RequestSchema {
    pub fn validate_response(&self, response: &Value) -> Result<(), String> {
        if let Some(name) = self.records.keys().chain(self.enums.keys()).find(|name| {
            matches!(
                name.as_str(),
                "Bool"
                    | "String"
                    | "Number"
                    | "Date"
                    | "Time"
                    | "DateTime"
                    | "DTDuration"
                    | "YMDuration"
                    | "Calendar"
                    | "CalendarEntry"
                    | "List"
                    | "Dictionary"
                    | "DictionaryEntry"
                    | "Table"
                    | "Value"
            ) || self.records.contains_key(*name) && self.enums.contains_key(*name)
        }) {
            return Err(format!("invalid response schema type: {name}"));
        }
        self.validate_fields(&self.ports, response, "response", false)
    }

    fn validate_fields(
        &self,
        fields: &BTreeMap<String, RequestType>,
        value: &Value,
        path: &str,
        table_cell: bool,
    ) -> Result<(), String> {
        let object = value
            .as_object()
            .ok_or_else(|| format!("{path} must be an object"))?;
        for name in object.keys() {
            if !fields.contains_key(name) {
                return Err(format!("unexpected field {path}.{name}"));
            }
        }
        for (name, ty) in fields {
            let field = object
                .get(name)
                .ok_or_else(|| format!("missing field {path}.{name}"))?;
            self.validate_type(ty, field, &format!("{path}.{name}"), table_cell)?;
        }
        Ok(())
    }

    fn validate_type(
        &self,
        ty: &RequestType,
        value: &Value,
        path: &str,
        table_cell: bool,
    ) -> Result<(), String> {
        let invalid = |error: String| format!("invalid {path}: {error}");
        match ty {
            RequestType::Named(name) => {
                if let Some(fields) = self.records.get(name) {
                    return self.validate_fields(fields, value, path, table_cell);
                }
                if let Some(variants) = self.enums.get(name) {
                    return if value
                        .as_str()
                        .is_some_and(|text| variants.iter().any(|v| v == text))
                    {
                        Ok(())
                    } else {
                        Err(invalid(format!("expected {name} variant")))
                    };
                }
                if name == "Number" && table_cell {
                    let number = value
                        .as_number()
                        .ok_or_else(|| invalid("expected Number".into()))?;
                    return Decimal::from_str_exact(&number.to_string())
                        .map(|_| ())
                        .map_err(|error| invalid(error.to_string()));
                }
                let result = match name.as_str() {
                    "Bool" => serde_json::from_value::<bool>(value.clone()).map(|_| ()),
                    "String" => serde_json::from_value::<String>(value.clone()).map(|_| ()),
                    "Number" => serde_json::from_value::<Decimal>(value.clone()).map(|_| ()),
                    "Date" => serde_json::from_value::<Date>(value.clone()).map(|_| ()),
                    "Time" => serde_json::from_value::<Time>(value.clone()).map(|_| ()),
                    "DateTime" => serde_json::from_value::<DateTime>(value.clone()).map(|_| ()),
                    "DTDuration" => serde_json::from_value::<DTDuration>(value.clone()).map(|_| ()),
                    "YMDuration" => serde_json::from_value::<YMDuration>(value.clone()).map(|_| ()),
                    "Calendar" => serde_json::from_value::<Calendar>(value.clone()).map(|_| ()),
                    "CalendarEntry" => {
                        serde_json::from_value::<CalendarEntry>(value.clone()).map(|_| ())
                    }
                    "Table" => serde_json::from_value::<crate::Table>(value.clone()).map(|_| ()),
                    "Dictionary" => {
                        serde_json::from_value::<BTreeMap<String, Value>>(value.clone()).map(|_| ())
                    }
                    "Value" => return Ok(()),
                    _ => return Err(invalid(format!("unknown type {name}"))),
                };
                result.map_err(|e| invalid(e.to_string()))
            }
            RequestType::Nullable(inner) => {
                if value.is_null() {
                    Ok(())
                } else {
                    self.validate_type(inner, value, path, table_cell)
                }
            }
            RequestType::List(inner) => {
                let items = value
                    .as_array()
                    .ok_or_else(|| invalid("expected list".into()))?;
                for (index, item) in items.iter().enumerate() {
                    self.validate_type(inner, item, &format!("{path}[{index}]"), table_cell)?;
                }
                Ok(())
            }
            RequestType::Dictionary(inner) => {
                let entries = value
                    .as_object()
                    .ok_or_else(|| invalid("expected dictionary".into()))?;
                for (key, item) in entries {
                    self.validate_type(inner, item, &format!("{path}.{key}"), table_cell)?;
                }
                Ok(())
            }
            RequestType::DictionaryEntry(inner) => {
                let entry = value
                    .as_object()
                    .ok_or_else(|| invalid("expected dictionary entry".into()))?;
                if entry.len() != 2
                    || !entry.get("key").is_some_and(Value::is_string)
                    || !entry.contains_key("value")
                {
                    return Err(invalid("expected exactly a string key and a value".into()));
                }
                self.validate_type(inner, &entry["value"], &format!("{path}.value"), table_cell)
            }
            RequestType::Table(fields) => {
                serde_json::from_value::<crate::Table>(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                let columns = value["columns"]
                    .as_array()
                    .ok_or_else(|| invalid("missing table columns".into()))?;
                if columns.len() != fields.len()
                    || !columns
                        .iter()
                        .zip(fields)
                        .all(|(column, (name, _))| column.as_str() == Some(name))
                {
                    return Err(invalid(
                        "table columns do not match declared row schema".into(),
                    ));
                }
                let row_fields = fields.iter().cloned().collect();
                for (index, row) in value["rows"]
                    .as_array()
                    .ok_or_else(|| invalid("missing table rows".into()))?
                    .iter()
                    .enumerate()
                {
                    self.validate_fields(&row_fields, row, &format!("{path}.rows[{index}]"), true)?;
                }
                Ok(())
            }
        }
    }

    /// Validate a declared table port and restore the declared temporal and enum cell types.
    pub fn decode_table_port(
        &self,
        value: &crate::Table,
        port: &str,
    ) -> Result<crate::Table, String> {
        let ty = self.ports.get(port).ok_or("unknown table port")?;
        let RequestType::Table(fields) = ty else {
            return Err("port is not a Table<Row>".into());
        };
        self.decode_table(value, fields)
    }

    fn decode_table(
        &self,
        value: &crate::Table,
        fields: &[(String, RequestType)],
    ) -> Result<crate::Table, String> {
        self.validate_type(
            &RequestType::Table(fields.to_vec()),
            &serde_json::to_value(value).map_err(|error| error.to_string())?,
            "table",
            false,
        )?;
        let rows = value
            .rows()
            .iter()
            .map(|row| {
                row.iter()
                    .zip(fields)
                    .map(|(cell, (_, ty))| {
                        cell.as_ref()
                            .map(|cell| self.decode_cell(ty, cell))
                            .transpose()
                    })
                    .collect::<Result<Vec<_>, String>>()
            })
            .collect::<Result<Vec<_>, String>>()?;
        crate::Table::from_optional(value.columns().to_vec(), rows)
    }

    fn decode_cell(&self, ty: &RequestType, cell: &crate::Cell) -> Result<crate::Cell, String> {
        use crate::Cell;
        let invalid = || format!("invalid declared table cell: {cell:?}");
        match ty {
            RequestType::Nullable(inner) if *cell == Cell::Null => Ok(Cell::Null),
            RequestType::Nullable(inner) => self.decode_cell(inner, cell),
            RequestType::Named(name) => {
                if let Some(fields) = self.records.get(name) {
                    let Cell::Dictionary(values) = cell else {
                        return Err(invalid());
                    };
                    return values
                        .iter()
                        .map(|(key, value)| {
                            Ok((
                                key.clone(),
                                self.decode_cell(fields.get(key).ok_or_else(invalid)?, value)?,
                            ))
                        })
                        .collect::<Result<BTreeMap<_, _>, String>>()
                        .map(Cell::Dictionary);
                }
                if self.enums.contains_key(name) {
                    return match cell {
                        Cell::String(variant) => Ok(Cell::Enum {
                            kind: name.clone(),
                            variant: variant.clone(),
                        }),
                        Cell::Enum { kind, .. } if kind == name => Ok(cell.clone()),
                        _ => Err(invalid()),
                    };
                }
                let json = serde_json::to_value(cell).map_err(|error| error.to_string())?;
                match name.as_str() {
                    "Date" => serde_json::from_value::<Date>(json).map(Cell::Date),
                    "Time" => serde_json::from_value::<Time>(json).map(Cell::Time),
                    "DateTime" => serde_json::from_value::<DateTime>(json).map(Cell::DateTime),
                    "DTDuration" => {
                        serde_json::from_value::<DTDuration>(json).map(Cell::DTDuration)
                    }
                    "YMDuration" => {
                        serde_json::from_value::<YMDuration>(json).map(Cell::YMDuration)
                    }
                    "Calendar" => serde_json::from_value::<Calendar>(json).map(Cell::Calendar),
                    "CalendarEntry" => {
                        serde_json::from_value::<CalendarEntry>(json).map(Cell::CalendarEntry)
                    }
                    _ => return Ok(cell.clone()),
                }
                .map_err(|error| error.to_string())
            }
            RequestType::List(inner) => {
                let Cell::List(values) = cell else {
                    return Err(invalid());
                };
                values
                    .iter()
                    .map(|item| self.decode_cell(inner, item))
                    .collect::<Result<Vec<_>, _>>()
                    .map(Cell::List)
            }
            RequestType::Dictionary(inner) => {
                let Cell::Dictionary(values) = cell else {
                    return Err(invalid());
                };
                values
                    .iter()
                    .map(|(key, item)| Ok((key.clone(), self.decode_cell(inner, item)?)))
                    .collect::<Result<BTreeMap<_, _>, String>>()
                    .map(Cell::Dictionary)
            }
            RequestType::Table(fields) => {
                let table: crate::Table = serde_json::from_value(
                    serde_json::to_value(cell).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                self.decode_table(&table, fields)
                    .map(|table| Cell::Table(Box::new(table)))
            }
            _ => Ok(cell.clone()),
        }
    }
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
    Subprocess {
        process: &'static str,
        input: Evaluate,
    },
    Task(Evaluate),
    AsyncTask(AsyncEvaluate),
    MultiInstance {
        task: Evaluate,
        items: Evaluate,
        parallel: bool,
    },
    AsyncMultiInstance {
        task: AsyncEvaluate,
        items: Evaluate,
        parallel: bool,
    },
    TaskLoop(Evaluate, LoopPolicy),
    AsyncTaskLoop(AsyncEvaluate, LoopPolicy),
    TaskWithCancel(Evaluate, Cancel),
    PauseFor(Duration),
    PauseUntil(Evaluate),
    InputRequest {
        prompt: &'static str,
        schema: RequestSchema,
        decode_response: Box<dyn Fn(Value) -> Result<Value, String> + Send + Sync>,
    },
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InputRequest {
    pub id: String,
    pub prompt: String,
    pub schema: RequestSchema,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RequestActivation {
    prompt: String,
    schema: RequestSchema,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ChildActivation {
    pub process: String,
    pub input: Value,
    pub checkpoint: Box<GraphCheckpoint>,
    #[serde(default)]
    pub entered_at_ms: i64,
    #[serde(default)]
    pub first_claimed_at_ms: Option<i64>,
    #[serde(default)]
    pub attempt: u32,
    #[serde(default)]
    pub first_failure_at_ms: Option<i64>,
    #[serde(default)]
    pub next_eligible_at_ms: Option<i64>,
    #[serde(default)]
    pub deadline_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    #[serde(default)]
    route_progress: HashMap<String, Vec<(usize, Value, Values)>>,
    // ponytail: committed activation history grows with visits; compact per generation if long-lived cycles become common.
    #[serde(default)]
    committed: HashMap<u64, Value>,
    #[serde(default)]
    multi: HashMap<u64, MultiProgress>,
    #[serde(default)]
    pub(crate) children: HashMap<u64, ChildActivation>,
    #[serde(default)]
    requests: BTreeMap<u64, RequestActivation>,
    pub outcome: Option<Value>,
    pub terminal: Option<GraphTerminal>,
}

impl Default for GraphCheckpoint {
    fn default() -> Self {
        Self {
            version: 2,
            next_activation_id: 1,
            ready: Vec::new(),
            pending: Vec::new(),
            waiting: Vec::new(),
            completed: Values::new(),
            selected: HashMap::new(),
            route_progress: HashMap::new(),
            committed: HashMap::new(),
            multi: HashMap::new(),
            children: HashMap::new(),
            requests: BTreeMap::new(),
            outcome: None,
            terminal: None,
        }
    }
}

impl GraphCheckpoint {
    pub fn requests(&self) -> Vec<InputRequest> {
        if self.outcome.is_some() || self.terminal.is_some() {
            return Vec::new();
        }
        let mut requests: Vec<_> = self
            .requests
            .iter()
            .map(|(id, request)| InputRequest {
                id: id.to_string(),
                prompt: request.prompt.clone(),
                schema: request.schema.clone(),
            })
            .collect();
        let mut children: Vec<_> = self.children.iter().collect();
        children.sort_by_key(|(id, _)| *id);
        for (id, child) in children {
            requests.extend(child.checkpoint.requests().into_iter().map(|mut request| {
                request.id = format!("{id}/{}", request.id);
                request
            }));
        }
        requests
    }

    pub fn ensure_supported(&self) -> Result<(), String> {
        if self.version != 2 {
            return Err(format!("unsupported checkpoint version: {}", self.version));
        }
        for child in self.children.values() {
            child.checkpoint.ensure_supported()?;
        }
        Ok(())
    }
}

impl GraphDefinition {
    pub fn child_processes(&self) -> impl Iterator<Item = &str> {
        self.nodes.iter().filter_map(|node| match &node.kind {
            GraphNodeKind::Subprocess { process, .. } => Some(*process),
            _ => None,
        })
    }

    pub fn checkpoint(&self, input: &Value) -> Result<GraphCheckpoint, String> {
        let mut state = GraphCheckpoint::default();
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
        state.ensure_supported()?;
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
            self.check_loop_bounds(state, crate::store::now_ms())?;
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
                GraphNodeKind::AsyncTask(_)
                | GraphNodeKind::AsyncMultiInstance { .. }
                | GraphNodeKind::AsyncTaskLoop(_, _) => {
                    return Err("async task requires async executor".into());
                }
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

    pub fn check_loop_bounds(
        &self,
        state: &mut GraphCheckpoint,
        now_ms: i64,
    ) -> Result<(), String> {
        state.ensure_supported()?;
        if state.terminal.is_some() || state.outcome.is_some() {
            return Ok(());
        }
        if state.ready.iter().any(|token| self.nodes.iter().any(|node| node.name == token.node && matches!(&node.kind, GraphNodeKind::TaskLoop(_, policy) | GraphNodeKind::AsyncTaskLoop(_, policy) if Self::loop_bound_reached(policy, token, now_ms)))) {
            state.terminal = Some(GraphTerminal::Error("task-iteration-limit".into()));
        }
        Ok(())
    }

    pub fn has_pending(&self, state: &GraphCheckpoint) -> bool {
        !state.pending.is_empty()
    }

    pub fn resume_pending(&self, input: &Value, state: &mut GraphCheckpoint) -> Result<(), String> {
        state.ensure_supported()?;
        let mut next = state.clone();
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

    pub(crate) fn activation_started_at(
        &self,
        state: &GraphCheckpoint,
        id: u64,
    ) -> Result<i64, String> {
        state.ensure_supported()?;
        state
            .ready
            .iter()
            .find(|token| token.id == id)
            .map(|token| token.started_at_ms)
            .ok_or("subprocess activation not ready".into())
    }

    pub fn activation_input(
        &self,
        state: &GraphCheckpoint,
        id: u64,
        input: &Value,
    ) -> Result<Value, String> {
        state.ensure_supported()?;
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
        state.ensure_supported()?;
        Ok(token.values.clone())
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
        state.ensure_supported()?;
        if state.outcome.is_some() || state.terminal.is_some() {
            return Ok(());
        }
        let mut next = state.clone();
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
        state.ensure_supported()?;
        let id = state
            .ready
            .iter()
            .find(|token| token.node == name)
            .ok_or_else(|| format!("task not ready: {name}"))?
            .id;
        self.complete_activation(input, state, id, value)
    }

    pub(crate) fn complete_subprocess_terminal(
        &self,
        input: &Value,
        state: &mut GraphCheckpoint,
        id: u64,
        terminal: GraphTerminal,
    ) -> Result<(), String> {
        state.ensure_supported()?;
        let mut next = state.clone();
        let index = next
            .ready
            .iter()
            .position(|token| token.id == id)
            .ok_or("subprocess activation not ready")?;
        let mut token = next.ready.remove(index);
        next.children.remove(&id);
        token.values.remove(&token.node);
        let outcome = match &terminal {
            GraphTerminal::Error(_) => "error",
            GraphTerminal::Cancel(_) => "cancel",
            GraphTerminal::Terminate(_) => "terminate",
        };
        if let Some(link) = self
            .links
            .iter()
            .find(|link| link.source == token.node && link.label == Some(outcome))
        {
            self.enter(
                input,
                &mut next,
                link,
                token.path,
                token.generations,
                token.values,
                &mut 64,
            )?;
        } else {
            next.terminal = Some(terminal);
        }
        *state = next;
        Ok(())
    }

    pub fn complete_request(
        &self,
        input: &Value,
        state: &mut GraphCheckpoint,
        request_id: &str,
        response: Value,
    ) -> Result<(), String> {
        let value = self.decode_request_response(state, request_id, response)?;
        let id = request_id
            .parse::<u64>()
            .map_err(|_| "invalid request ID")?;
        self.complete_decoded_activation(input, state, id, value)
    }

    pub(crate) fn decode_request_response(
        &self,
        state: &GraphCheckpoint,
        request_id: &str,
        response: Value,
    ) -> Result<Value, String> {
        state.ensure_supported()?;
        let id = request_id
            .parse::<u64>()
            .map_err(|_| "invalid request ID")?;
        if id.to_string() != request_id {
            return Err("request not outstanding".into());
        }
        state
            .requests
            .get(&id)
            .ok_or("request not outstanding")?
            .schema
            .validate_response(&response)?;
        let name = state
            .ready
            .iter()
            .find(|token| token.id == id)
            .ok_or("request activation not ready")?
            .node
            .as_str();
        let GraphNodeKind::InputRequest {
            decode_response, ..
        } = &self
            .nodes
            .iter()
            .find(|node| node.name == name)
            .ok_or("unknown request node")?
            .kind
        else {
            return Err("request activation is not an input request".into());
        };
        decode_response(response)
    }

    pub fn complete_activation(
        &self,
        input: &Value,
        state: &mut GraphCheckpoint,
        id: u64,
        value: Value,
    ) -> Result<(), String> {
        state.ensure_supported()?;
        let token = state
            .ready
            .iter()
            .find(|token| token.id == id)
            .ok_or_else(|| format!("task activation not ready: {id}"))?;
        if self.nodes.iter().any(|node| {
            node.name == token.node && matches!(node.kind, GraphNodeKind::InputRequest { .. })
        }) {
            return Err("input request requires a decoded response".into());
        }
        self.complete_decoded_activation(input, state, id, value)
    }

    fn complete_decoded_activation(
        &self,
        input: &Value,
        state: &mut GraphCheckpoint,
        id: u64,
        value: Value,
    ) -> Result<(), String> {
        // ponytail: copy the checkpoint per transition; replace with store-backed atomic updates if size becomes costly.
        state.ensure_supported()?;
        let mut next = state.clone();
        if next.outcome.is_some() || next.terminal.is_some() {
            return Err("graph already terminal".into());
        }
        let index = next
            .ready
            .iter()
            .position(|token| token.id == id)
            .ok_or_else(|| format!("task activation not ready: {id}"))?;
        let mut token = next.ready.remove(index);
        next.requests.remove(&id);
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
        if let Some(policy) =
            self.nodes
                .iter()
                .find(|node| node.name == token.node)
                .and_then(|node| match &node.kind {
                    GraphNodeKind::TaskLoop(_, policy)
                    | GraphNodeKind::AsyncTaskLoop(_, policy) => Some(policy),
                    _ => None,
                })
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

    // Recursive traversal carries its activation state explicitly.
    #[allow(clippy::too_many_arguments)]
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
        let kind = &self
            .nodes
            .iter()
            .find(|node| node.name == source)
            .ok_or_else(|| format!("unknown node: {source}"))?
            .kind;
        let outgoing: Vec<_> = self
            .links
            .iter()
            .enumerate()
            .filter(|(_, link)| {
                link.source == source
                    && (!matches!(kind, GraphNodeKind::Subprocess { .. }) || link.label.is_none())
            })
            .collect();
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

    #[allow(clippy::too_many_arguments)]
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
            GraphNodeKind::Subprocess { .. } => {
                let mut token = Self::activation(state, node.name, path, generations, values);
                token.started_at_ms = crate::store::now_ms();
                state.ready.push(token);
                Ok(())
            }
            GraphNodeKind::InputRequest { prompt, schema, .. } => {
                let token = Self::activation(state, node.name, path, generations, values);
                state.requests.insert(
                    token.id,
                    RequestActivation {
                        prompt: (*prompt).into(),
                        schema: schema.clone(),
                    },
                );
                state.ready.push(token);
                Ok(())
            }
            GraphNodeKind::Task(_)
            | GraphNodeKind::TaskWithCancel(_, _)
            | GraphNodeKind::AsyncTask(_) => {
                let token = Self::activation(state, node.name, path, generations, values);
                state.ready.push(token);
                Ok(())
            }
            GraphNodeKind::MultiInstance {
                items, parallel, ..
            }
            | GraphNodeKind::AsyncMultiInstance {
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
                for (index, item) in
                    entries
                        .iter()
                        .enumerate()
                        .take(if *parallel { total } else { 1 })
                {
                    let mut token = Self::activation(
                        state,
                        node.name,
                        path.clone(),
                        generations.clone(),
                        values.clone(),
                    );
                    token.batch_id = Some(batch_id);
                    token.item_index = Some(index);
                    token.item = Some(item.clone());
                    state.ready.push(token);
                }
                Ok(())
            }
            GraphNodeKind::TaskLoop(_, policy) | GraphNodeKind::AsyncTaskLoop(_, policy) => {
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
