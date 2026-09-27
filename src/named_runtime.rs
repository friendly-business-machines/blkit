use std::collections::HashMap;

use crate::runtime::{Evaluate, Values};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub struct GraphDefinition {
    pub namespace: &'static str,
    pub version: &'static str,
    pub name: &'static str,
    pub retry: Option<crate::RetryPolicy>,
    pub decode_input: Box<dyn Fn(Value) -> Result<Value, String> + Send + Sync>,
    pub nodes: Vec<GraphNode>,
    pub links: Vec<GraphLink>,
}

pub struct GraphNode {
    pub name: &'static str,
    pub kind: GraphNodeKind,
}

pub enum GraphNodeKind {
    Start,
    Task(Evaluate),
    TaskWithCancel(Evaluate, crate::runtime::Cancel),
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphTerminal {
    Error(String),
    Cancel(String),
    Terminate(String),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GraphCheckpoint {
    ready: Vec<Activation>,
    pub completed: Values,
    pub selected: HashMap<String, Vec<usize>>,
    progress: HashMap<String, Vec<(usize, Value)>>,
    pub outcome: Option<Value>,
    pub terminal: Option<GraphTerminal>,
}

impl GraphDefinition {
    pub fn checkpoint(&self, input: &Value) -> Result<GraphCheckpoint, String> {
        let mut state = GraphCheckpoint::default();
        let start = self
            .nodes
            .iter()
            .find(|node| matches!(node.kind, GraphNodeKind::Start))
            .ok_or("missing start")?;
        self.advance(input, &mut state, start.name, Vec::new())?;
        Ok(state)
    }

    pub fn run(&self, input: &Value, state: &mut GraphCheckpoint) -> Result<Value, String> {
        while state.outcome.is_none() {
            if let Some(terminal) = &state.terminal {
                return Err(format!("terminal event: {terminal:?}"));
            }
            let name = state
                .ready
                .first()
                .ok_or("graph has no active task or terminal")?
                .node
                .clone();
            let node = self
                .nodes
                .iter()
                .find(|node| node.name == name)
                .ok_or_else(|| format!("unknown task: {name}"))?;
            let call = match &node.kind {
                GraphNodeKind::Task(call) | GraphNodeKind::TaskWithCancel(call, _) => call,
                _ => return Err(format!("not a task: {name}")),
            };
            let output = call(input, &state.completed)?;
            self.complete(input, state, &name, output)?;
        }
        Ok(state.outcome.clone().unwrap())
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
        // ponytail: copy the checkpoint per transition; replace with store-backed atomic updates if size becomes costly.
        let mut next = state.clone();
        let index = next
            .ready
            .iter()
            .position(|token| token.node == name)
            .ok_or_else(|| format!("task not ready: {name}"))?;
        let token = next.ready.remove(index);
        next.completed.insert(name.into(), value);
        self.advance(input, &mut next, name, token.path)?;
        *state = next;
        Ok(())
    }

    fn advance(
        &self,
        input: &Value,
        state: &mut GraphCheckpoint,
        source: &str,
        path: Vec<(String, usize)>,
    ) -> Result<(), String> {
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
                    } else if link.condition.as_ref().ok_or("missing condition")?(
                        input,
                        &state.completed,
                    )? == Value::Bool(true)
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
        if matches!(kind, GraphNodeKind::Split(_)) {
            state.selected.insert(source.into(), selected.clone());
        }
        for index in selected {
            let link = &self.links[index];
            let mut route = path.clone();
            if matches!(kind, GraphNodeKind::Split(_)) {
                route.push((source.into(), index));
            }
            self.enter(input, state, link, route)?;
        }
        Ok(())
    }

    fn enter(
        &self,
        input: &Value,
        state: &mut GraphCheckpoint,
        link: &GraphLink,
        mut path: Vec<(String, usize)>,
    ) -> Result<(), String> {
        let node = self
            .nodes
            .iter()
            .find(|node| node.name == link.target)
            .ok_or_else(|| format!("unknown node: {}", link.target))?;
        match &node.kind {
            GraphNodeKind::Start => Err("link into start".into()),
            GraphNodeKind::Task(_) | GraphNodeKind::TaskWithCancel(_, _) => {
                state.ready.push(Activation {
                    node: node.name.into(),
                    path,
                });
                Ok(())
            }
            GraphNodeKind::Split(_) => self.advance(input, state, node.name, path),
            GraphNodeKind::Join { kind, split } => {
                let (route_split, branch) = path.pop().ok_or("join without split")?;
                if route_split != *split {
                    return Err(format!("wrong split at join {}", node.name));
                }
                let value =
                    link.value.as_ref().ok_or("join needs value")?(input, &state.completed)?;
                let progress = state.progress.entry(node.name.into()).or_default();
                progress.push((branch, value));
                let selected = &state.selected[*split];
                if progress.len() != selected.len() {
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
                                    .find(|(branch, _)| branch == index)
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
                                    .find(|(branch, _)| branch == index)
                                    .map(|(_, value)| value.clone())
                                    .ok_or("missing OR branch")
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                    "xor" => progress[0].1.clone(),
                    other => return Err(format!("unknown join: {other}")),
                };
                state.progress.remove(node.name);
                state.completed.insert(node.name.into(), result);
                self.advance(input, state, node.name, path)
            }
            GraphNodeKind::End => {
                state.outcome = Some(link.value.as_ref().ok_or("end needs value")?(
                    input,
                    &state.completed,
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
