use crate::{
    Type,
    expr::{self, Expr},
};

#[derive(Debug)]
pub struct Peer {
    pub name: String,
    pub kind: PeerKind,
}

#[derive(Debug)]
pub enum PeerKind {
    Start {
        outputs: Vec<(String, Type)>,
    },
    End {
        inputs: Vec<(String, Type)>,
    },
    Split {
        kind: &'static str,
    },
    Terminal {
        kind: &'static str,
    },
    PauseFor(std::time::Duration),
    PauseUntil {
        input: String,
    },
    Subprocess {
        process: String,
        inputs: Vec<(String, Type)>,
        outputs: Vec<(String, Type)>,
    },
    Join {
        kind: &'static str,
        split: String,
        inputs: Vec<(String, Type)>,
        outputs: Vec<(String, Type)>,
    },
}

#[derive(Debug)]
pub struct SourceGraph {
    pub flows: Vec<(String, String)>,
    pub routes: Vec<Route>,
    pub bindings: Vec<Binding>,
    pub repetitions: Vec<Repetition>,
    pub multi_instances: Vec<MultiInstance>,
}

#[derive(Debug)]
pub struct Repetition {
    pub node: String,
    pub condition: Expr,
    pub before: bool,
    pub initial: Option<Expr>,
    pub max_iterations: Option<u32>,
    pub max_duration: Option<std::time::Duration>,
}

#[derive(Debug)]
pub struct MultiInstance {
    pub node: String,
    pub items: Expr,
    pub parallel: bool,
}

#[derive(Debug)]
pub struct Route {
    pub source: String,
    pub target: String,
    pub condition: Option<Expr>,
    pub fallback: bool,
    pub label: Option<String>,
    pub outcome: Option<String>,
}

#[derive(Debug)]
pub struct Binding {
    pub source: String,
    pub output: String,
    pub target: String,
    pub input: String,
}

pub fn parse_peer(kind: &str, name: &str, body: &[String]) -> Result<Peer, String> {
    if matches!(kind, "error_event" | "cancel_event" | "terminate_event") {
        if !body.is_empty() {
            return Err(format!("terminal {name} does not accept statements"));
        }
        return Ok(Peer {
            name: name.into(),
            kind: PeerKind::Terminal {
                kind: match kind {
                    "error_event" => "Error",
                    "cancel_event" => "Cancel",
                    _ => "Terminate",
                },
            },
        });
    }
    if kind == "pause_for" {
        let [statement] = body else {
            return Err("pause_for requires duration".into());
        };
        let duration = duration(
            statement
                .strip_prefix("duration ")
                .ok_or("pause_for requires duration")?,
        )?;
        if duration.is_zero() {
            return Err("pause_for duration must be positive".into());
        }
        return Ok(Peer {
            name: name.into(),
            kind: PeerKind::PauseFor(duration),
        });
    }
    if kind == "pause_until" {
        let [statement] = body else {
            return Err("pause_until requires one input port".into());
        };
        let (input, ty) = statement
            .strip_prefix("input ")
            .and_then(|port| port.split_once(": "))
            .ok_or("pause_until requires a typed input port")?;
        if !crate::identifier(input) || crate::type_ref(ty)? != Type::Named("DateTime".into()) {
            return Err("pause_until input must be DateTime".into());
        }
        return Ok(Peer {
            name: name.into(),
            kind: PeerKind::PauseUntil {
                input: input.into(),
            },
        });
    }
    if kind == "subprocess" {
        let mut process = None;
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for statement in body {
            if let Some(value) = statement.strip_prefix("process ") {
                if !crate::identifier(value) || process.replace(value.to_owned()).is_some() {
                    return Err(format!("invalid subprocess target: {value}"));
                }
            } else if let Some((direction, value)) = statement.split_once(' ') {
                let ports: &mut Vec<(String, Type)> = match direction {
                    "input" => &mut inputs,
                    "output" => &mut outputs,
                    _ => return Err(format!("invalid subprocess statement: {statement}")),
                };
                let (port, ty) = value.split_once(": ").ok_or("invalid subprocess port")?;
                if !crate::identifier(port) || ports.iter().any(|(name, _)| name == port) {
                    return Err(format!("invalid or duplicate subprocess port: {port}"));
                }
                ports.push((port.into(), crate::type_ref(ty)?));
            } else {
                return Err(format!("invalid subprocess statement: {statement}"));
            }
        }
        return Ok(Peer {
            name: name.into(),
            kind: PeerKind::Subprocess {
                process: process.ok_or("subprocess requires process target")?,
                inputs,
                outputs,
            },
        });
    }
    if matches!(kind, "xor_split" | "or_split" | "and_split") {
        if !body.is_empty() {
            return Err(format!("split {name} does not accept statements"));
        }
        return Ok(Peer {
            name: name.into(),
            kind: PeerKind::Split {
                kind: if kind.starts_with("xor") {
                    "xor"
                } else if kind.starts_with("and") {
                    "and"
                } else {
                    "or"
                },
            },
        });
    }
    if matches!(kind, "xor_join" | "or_join" | "and_join") {
        let mut split = None;
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for statement in body {
            if let Some(value) = statement.strip_prefix("split ") {
                if !crate::identifier(value) || split.replace(value.into()).is_some() {
                    return Err(format!("invalid join split: {value}"));
                }
            } else if let Some(value) = statement
                .strip_prefix("input ")
                .or_else(|| statement.strip_prefix("output "))
            {
                let (port, ty) = value.split_once(": ").ok_or("invalid join port")?;
                let ports: &mut Vec<(String, Type)> = if statement.starts_with("input ") {
                    &mut inputs
                } else {
                    &mut outputs
                };
                if !crate::identifier(port) || ports.iter().any(|(name, _)| name == port) {
                    return Err(format!("invalid or duplicate join port: {port}"));
                }
                ports.push((port.into(), crate::type_ref(ty)?));
            } else {
                return Err(format!("invalid join statement: {statement}"));
            }
        }
        return Ok(Peer {
            name: name.into(),
            kind: PeerKind::Join {
                kind: if kind.starts_with("xor") {
                    "xor"
                } else if kind.starts_with("and") {
                    "and"
                } else {
                    "or"
                },
                split: split.ok_or("join requires split")?,
                inputs,
                outputs,
            },
        });
    }
    let mut ports = Vec::new();
    let keyword = if kind == "start_event" {
        "output "
    } else {
        "input "
    };
    for statement in body {
        let text = statement
            .strip_prefix(keyword)
            .ok_or_else(|| format!("invalid {kind} statement: {statement}"))?;
        let (port, ty) = text
            .split_once(": ")
            .ok_or_else(|| format!("invalid {kind} port: {statement}"))?;
        if !crate::identifier(port) || ports.iter().any(|(used, _)| used == port) {
            return Err(format!("invalid or duplicate {kind} port: {port}"));
        }
        ports.push((port.to_owned(), crate::type_ref(ty)?));
    }
    Ok(Peer {
        name: name.to_owned(),
        kind: if kind == "start_event" {
            PeerKind::Start { outputs: ports }
        } else {
            PeerKind::End { inputs: ports }
        },
    })
}

pub fn parse_source(
    body: &[String],
) -> Result<
    (
        SourceGraph,
        Option<crate::compiler::RetryPolicy>,
        Option<crate::compiler::DeadlinePolicy>,
    ),
    String,
> {
    let mut retry = None;
    let mut deadline = None;
    let mut graph = SourceGraph {
        flows: Vec::new(),
        routes: Vec::new(),
        bindings: Vec::new(),
        repetitions: Vec::new(),
        multi_instances: Vec::new(),
    };
    for statement in body {
        if let Some(text) = statement.strip_prefix("retry ") {
            let fields: Vec<_> = text.split_whitespace().collect();
            let [
                "max_retries",
                max,
                "retry_for",
                window,
                "retry_delay",
                delay,
                "backoff",
                "exponential",
            ] = fields.as_slice()
            else {
                return Err("invalid retry declaration".into());
            };
            if retry.is_some() || !graph.flows.is_empty() || !graph.bindings.is_empty() {
                return Err("retry declaration must appear once before flows".into());
            }
            retry = Some(crate::compiler::RetryPolicy {
                max_retries: max.parse().map_err(|_| "invalid max_retries")?,
                retry_for: duration(window)?,
                retry_delay: duration(delay)?,
                backoff: "exponential",
            });
        } else if let Some(text) = statement.strip_prefix("deadline ") {
            let (origin, span) = text.split_once(' ').ok_or("invalid deadline declaration")?;
            if !matches!(origin, "queued" | "first_claimed")
                || deadline.is_some()
                || !graph.flows.is_empty()
                || !graph.bindings.is_empty()
            {
                return Err("invalid or duplicate deadline declaration".into());
            }
            let duration = duration(span)?;
            if duration.is_zero() {
                return Err("deadline must be positive".into());
            }
            deadline = Some(crate::compiler::DeadlinePolicy {
                origin: if origin == "queued" {
                    "queued"
                } else {
                    "first_claimed"
                },
                duration,
            });
        } else if let Some(text) = statement
            .strip_prefix("repeat_pre ")
            .or_else(|| statement.strip_prefix("repeat_post "))
        {
            let before = statement.starts_with("repeat_pre ");
            let (node, body) = text
                .split_once(" while ")
                .ok_or("invalid repeat condition")?;
            if !crate::identifier(node) || graph.repetitions.iter().any(|item| item.node == node) {
                return Err(format!("invalid or duplicate repeat node: {node}"));
            }
            let bound = body
                .find(" max_iterations ")
                .or_else(|| body.find(" max_duration "))
                .ok_or("repeat requires positive bound")?;
            let (condition, bounds) = body.split_at(bound);
            let (condition, initial) =
                if let Some((condition, initial)) = condition.split_once(" initial ") {
                    (condition, Some(expr::expression(initial)?))
                } else {
                    (condition, None)
                };
            if before != initial.is_some() {
                return Err("repeat_pre requires initial; repeat_post forbids initial".into());
            }
            let parts: Vec<_> = bounds.split_whitespace().collect();
            if parts.len() % 2 != 0 || parts.is_empty() || parts.len() > 4 {
                return Err("invalid repeat bounds".into());
            }
            let (mut max_iterations, mut max_duration) = (None, None);
            for pair in parts.chunks_exact(2) {
                match pair[0] {
                    "max_iterations" if max_iterations.is_none() => {
                        let count = pair[1]
                            .parse::<u32>()
                            .map_err(|_| "invalid max_iterations")?;
                        if count == 0 {
                            return Err("max_iterations must be positive".into());
                        }
                        max_iterations = Some(count);
                    }
                    "max_duration" if max_duration.is_none() => {
                        let span = duration(pair[1])?;
                        if span.is_zero() || i64::try_from(span.as_millis()).is_err() {
                            return Err("max_duration must be positive and fit milliseconds".into());
                        }
                        max_duration = Some(span);
                    }
                    _ => return Err("invalid repeat bounds".into()),
                }
            }
            graph.repetitions.push(Repetition {
                node: node.into(),
                condition: expr::expression(condition)?,
                before,
                initial,
                max_iterations,
                max_duration,
            });
        } else if let Some(text) = statement.strip_prefix("multi_instance ") {
            let (node, items) = text
                .split_once(" each ")
                .ok_or("invalid multi_instance declaration")?;
            let (items, mode) = items
                .rsplit_once(' ')
                .ok_or("invalid multi_instance mode")?;
            if !crate::identifier(node)
                || !matches!(mode, "sequential" | "parallel")
                || graph.multi_instances.iter().any(|item| item.node == node)
            {
                return Err("invalid or duplicate multi_instance declaration".into());
            }
            graph.multi_instances.push(MultiInstance {
                node: node.into(),
                items: expr::expression(items)?,
                parallel: mode == "parallel",
            });
        } else if let Some(text) = statement.strip_prefix("flow ") {
            let (source, target) = text.split_once(" -> ").ok_or("invalid flow")?;
            let (target, condition, fallback, label, outcome) =
                if let Some((target, predicate)) = target.split_once(" when ") {
                    (
                        target,
                        Some(expr::expression(predicate)?),
                        false,
                        None,
                        None,
                    )
                } else if let Some(target) = target.strip_suffix(" else") {
                    (target, None, true, None, None)
                } else if let Some((target, label)) = target.split_once(" as ") {
                    if !crate::identifier(label) {
                        return Err(format!("invalid flow label: {label}"));
                    }
                    (target, None, false, Some(label.to_owned()), None)
                } else if let Some((target, outcome)) = target.split_once(" on ") {
                    if !matches!(outcome, "error" | "cancel" | "terminate") {
                        return Err(format!("invalid subprocess outcome: {outcome}"));
                    }
                    (target, None, false, None, Some(outcome.to_owned()))
                } else {
                    (target, None, false, None, None)
                };
            if !crate::identifier(source) || !crate::identifier(target) {
                return Err(format!("invalid flow: {statement}"));
            }
            if graph
                .flows
                .iter()
                .any(|(from, to)| from == source && to == target)
            {
                return Err(format!("duplicate flow: {source} -> {target}"));
            }
            if condition.is_some() || fallback || label.is_some() || outcome.is_some() {
                graph.routes.push(Route {
                    source: source.into(),
                    target: target.into(),
                    condition,
                    fallback,
                    label,
                    outcome,
                });
            }
            graph.flows.push((source.into(), target.into()));
        } else if let Some(text) = statement.strip_prefix("bind ") {
            let (source, target) = text.split_once(" -> ").ok_or("invalid bind")?;
            let (source, output) = source.split_once('.').ok_or("invalid bind output")?;
            let (target, input) = target.split_once('.').ok_or("invalid bind input")?;
            if [source, output, target, input]
                .iter()
                .any(|part| !crate::identifier(part))
            {
                return Err(format!("invalid bind: {statement}"));
            }
            graph.bindings.push(Binding {
                source: source.into(),
                output: output.into(),
                target: target.into(),
                input: input.into(),
            });
        } else {
            return Err(format!("invalid process statement: {statement}"));
        }
    }
    Ok((graph, retry, deadline))
}

pub(crate) type RouteReference = (String, String, Type);

pub(crate) fn route_expression(
    expression: &Expr,
    program: &crate::compiler::Program,
) -> Result<(Expr, Vec<RouteReference>), String> {
    fn rewrite(
        expression: &mut Expr,
        program: &crate::compiler::Program,
        refs: &mut Vec<RouteReference>,
    ) -> Result<(), String> {
        if let Expr::Field(base, port) = expression
            && let Expr::Name(source) = base.as_ref()
        {
            let ports = if let Some(peer) =
                program.peer_nodes.iter().find(|peer| peer.name == *source)
            {
                match &peer.kind {
                    PeerKind::Start { outputs }
                    | PeerKind::Join { outputs, .. }
                    | PeerKind::Subprocess { outputs, .. } => Some(outputs.as_slice()),
                    PeerKind::End { .. }
                    | PeerKind::Split { .. }
                    | PeerKind::Terminal { .. }
                    | PeerKind::PauseFor(_)
                    | PeerKind::PauseUntil { .. } => None,
                }
            } else if let Some(task) = program.decisions.iter().find(|task| task.name == *source) {
                let ty = task
                    .outputs
                    .iter()
                    .find(|(name, _, _)| name == port)
                    .map(|(_, ty, _)| ty.clone())
                    .ok_or_else(|| format!("unknown route output: {source}.{port}"))?;
                let reference = (source.clone(), port.clone(), ty);
                let index = if let Some(index) = refs
                    .iter()
                    .position(|(node, field, _)| node == source && field == port)
                {
                    index
                } else {
                    refs.push(reference);
                    refs.len() - 1
                };
                *expression = Expr::Name(format!("__bl_route_{index}"));
                return Ok(());
            } else {
                None
            };
            if let Some(ports) = ports {
                let ty = ports
                    .iter()
                    .find(|(name, _)| name == port)
                    .map(|(_, ty)| ty.clone())
                    .ok_or_else(|| format!("unknown route output: {source}.{port}"))?;
                let index = if let Some(index) = refs
                    .iter()
                    .position(|(node, field, _)| node == source && field == port)
                {
                    index
                } else {
                    refs.push((source.clone(), port.clone(), ty));
                    refs.len() - 1
                };
                *expression = Expr::Name(format!("__bl_route_{index}"));
                return Ok(());
            }
            if program.peer_nodes.iter().any(|peer| peer.name == *source) {
                return Err(format!("route source has no output: {source}"));
            }
        }
        match expression {
            Expr::Field(base, _) | Expr::Not(base) => rewrite(base, program, refs)?,
            Expr::Binary(left, _, right) => {
                rewrite(left, program, refs)?;
                rewrite(right, program, refs)?;
            }
            Expr::Call(_, args) | Expr::List(args) => {
                for arg in args {
                    rewrite(arg, program, refs)?;
                }
            }
            Expr::Range(lower, upper, _, _) => {
                for bound in lower.iter_mut().chain(upper.iter_mut()) {
                    rewrite(bound, program, refs)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut expression = expression.clone();
    let mut refs = Vec::new();
    rewrite(&mut expression, program, &mut refs)?;
    Ok((expression, refs))
}

#[derive(Debug)]
pub struct NamedGraph {
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
}

#[derive(Debug)]
pub struct Node {
    pub name: String,
    pub kind: NodeKind,
}

#[derive(Debug)]
pub enum NodeKind {
    Start,
    Subprocess {
        process: String,
        input: Expr,
    },
    Task {
        task: String,
        input: Expr,
    },
    MultiInstance {
        task: String,
        items: Expr,
        parallel: bool,
    },
    TaskLoop {
        task: String,
        input: Expr,
        condition: Expr,
        before: bool,
        initial: Option<Expr>,
        max_iterations: Option<u32>,
        max_duration: Option<std::time::Duration>,
    },
    BusinessRule {
        model: String,
        input: Expr,
    },
    PauseFor(std::time::Duration),
    PauseUntil(Expr),
    Split(String),
    Join {
        kind: String,
        split: String,
        output: Option<Type>,
    },
    End,
    Error,
    Cancel,
    Terminate,
}

#[derive(Debug)]
pub struct Link {
    pub source: String,
    pub target: String,
    pub value: Option<Expr>,
    pub condition: Option<Expr>,
    pub fallback: bool,
    pub label: Option<String>,
    pub outcome: Option<String>,
}

fn task_reference(value: &str) -> bool {
    super::identifier(value)
        || value
            .split_once('.')
            .is_some_and(|(provider, task)| super::identifier(provider) && super::identifier(task))
}

pub(crate) fn duration(value: &str) -> Result<std::time::Duration, String> {
    let value = value
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| format!("invalid retry duration: {value}"))?;
    let (digits, scale) = if let Some(n) = value.strip_suffix("ms") {
        (n, 1)
    } else if let Some(n) = value.strip_suffix('s') {
        (n, 1000)
    } else if let Some(n) = value.strip_suffix('m') {
        (n, 60_000)
    } else if let Some(n) = value.strip_suffix('h') {
        (n, 3_600_000)
    } else {
        return Err(format!("invalid retry duration: {value}"));
    };
    let millis: u64 = digits
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(scale))
        .ok_or_else(|| format!("invalid retry duration: {value}"))?;
    Ok(std::time::Duration::from_millis(millis))
}

pub fn parse_named(
    lines: &[String],
) -> Result<
    (
        NamedGraph,
        Option<crate::compiler::RetryPolicy>,
        Option<crate::compiler::DeadlinePolicy>,
    ),
    String,
> {
    let mut graph = NamedGraph {
        nodes: Vec::new(),
        links: Vec::new(),
    };
    let mut retry = None;
    let mut deadline = None;
    for line in lines {
        let statement = line
            .strip_prefix("  ")
            .filter(|text| !text.starts_with(' '))
            .ok_or_else(|| format!("unexpected indentation: {line}"))?;
        if let Some(text) = statement.strip_prefix("deadline ") {
            let (origin, span) = text.split_once(' ').ok_or("invalid deadline declaration")?;
            let origin = match origin {
                "queued" => "queued",
                "first_claimed" => "first_claimed",
                _ => return Err(format!("invalid deadline origin: {origin}")),
            };
            let duration = duration(span)?;
            if duration.is_zero()
                || deadline.is_some()
                || !graph.nodes.is_empty()
                || !graph.links.is_empty()
            {
                return Err(
                    "deadline must be declared once with positive duration before nodes".into(),
                );
            }
            deadline = Some(crate::compiler::DeadlinePolicy { origin, duration });
        } else if let Some(text) = statement.strip_prefix("retry ") {
            let fields: Vec<_> = text.split_whitespace().collect();
            let [
                "max_retries",
                max,
                "retry_for",
                window,
                "retry_delay",
                delay,
                "backoff",
                "exponential",
            ] = fields.as_slice()
            else {
                return Err(format!("invalid retry declaration: {line}"));
            };
            if retry.is_some() || !graph.nodes.is_empty() || !graph.links.is_empty() {
                return Err("retry declaration must appear once before nodes".into());
            }
            retry = Some(crate::compiler::RetryPolicy {
                max_retries: max
                    .parse()
                    .map_err(|_| format!("invalid max_retries: {max}"))?,
                retry_for: duration(window)?,
                retry_delay: duration(delay)?,
                backoff: "exponential",
            });
        } else if let Some(text) = statement.strip_prefix("node ") {
            let (name, declaration) = text
                .split_once(" = ")
                .ok_or_else(|| format!("invalid node: {line}"))?;
            if !super::identifier(name) {
                return Err(format!("invalid node: {line}"));
            }
            let kind = match declaration {
                "start" => NodeKind::Start,
                "end" => NodeKind::End,
                "error" => NodeKind::Error,
                "cancel" => NodeKind::Cancel,
                "terminate" => NodeKind::Terminate,
                "and_split" | "or_split" | "xor_split" => {
                    NodeKind::Split(declaration.trim_end_matches("_split").into())
                }
                _ if declaration.starts_with("pause_for ") => {
                    let duration = duration(declaration.strip_prefix("pause_for ").unwrap())?;
                    if duration.is_zero() {
                        return Err("pause_for duration must be positive".into());
                    }
                    NodeKind::PauseFor(duration)
                }
                _ if declaration.starts_with("pause_until ") => NodeKind::PauseUntil(
                    expr::expression(declaration.strip_prefix("pause_until ").unwrap())?,
                ),
                _ if declaration.starts_with("subprocess ") => {
                    let call = declaration.strip_prefix("subprocess ").unwrap();
                    let (process, argument) = call
                        .split_once('(')
                        .ok_or_else(|| format!("invalid subprocess node: {line}"))?;
                    let argument = argument
                        .strip_suffix(')')
                        .ok_or_else(|| format!("invalid subprocess node: {line}"))?;
                    if !super::identifier(process) {
                        return Err(format!("invalid subprocess node: {line}"));
                    }
                    NodeKind::Subprocess {
                        process: process.into(),
                        input: expr::expression(argument)?,
                    }
                }
                _ if declaration.starts_with("business_rule ") => {
                    let call = declaration.strip_prefix("business_rule ").unwrap();
                    let (model, argument) = call
                        .split_once('(')
                        .ok_or_else(|| format!("invalid business rule node: {line}"))?;
                    let argument = argument
                        .strip_suffix(')')
                        .ok_or_else(|| format!("invalid business rule node: {line}"))?;
                    if !super::identifier(model) {
                        return Err(format!("invalid business rule node: {line}"));
                    }
                    NodeKind::BusinessRule {
                        model: model.into(),
                        input: expr::expression(argument)?,
                    }
                }
                _ if declaration.starts_with("task ") && declaration.contains(" each ") => {
                    let call = declaration.strip_prefix("task ").unwrap();
                    let (task, items) = call
                        .split_once(" each ")
                        .ok_or_else(|| format!("invalid multi-instance node: {line}"))?;
                    let (items, mode) = items
                        .rsplit_once(' ')
                        .ok_or_else(|| format!("invalid multi-instance mode: {line}"))?;
                    if !task_reference(task) || !matches!(mode, "sequential" | "parallel") {
                        return Err(format!("invalid multi-instance node: {line}"));
                    }
                    NodeKind::MultiInstance {
                        task: task.into(),
                        items: expr::expression(items)?,
                        parallel: mode == "parallel",
                    }
                }
                _ if declaration.starts_with("task ") => {
                    let call = declaration.strip_prefix("task ").unwrap();
                    let (call, repeat) = if let Some((call, repeat)) = call.rsplit_once(") repeat_")
                    {
                        (format!("{call})"), Some(repeat))
                    } else {
                        (call.to_owned(), None)
                    };
                    let (task, argument) = call
                        .split_once('(')
                        .ok_or_else(|| format!("invalid task node: {line}"))?;
                    let argument = argument
                        .strip_suffix(')')
                        .ok_or_else(|| format!("invalid task node: {line}"))?;
                    if !task_reference(task) {
                        return Err(format!("invalid task node: {line}"));
                    }
                    let input = expr::expression(argument)?;
                    if let Some(repeat) = repeat {
                        let (before, body) = if let Some(body) = repeat.strip_prefix("pre(") {
                            (true, body)
                        } else if let Some(body) = repeat.strip_prefix("post(") {
                            (false, body)
                        } else {
                            return Err(format!("invalid task loop: {line}"));
                        };
                        let (condition, bounds) = body.split_once(") max_").ok_or_else(|| {
                            format!("task loop requires a positive bound: {line}")
                        })?;
                        let bounds = format!("max_{bounds}");
                        let (bounds, initial) =
                            if let Some((bounds, initial)) = bounds.split_once(" initial ") {
                                (bounds, Some(expr::expression(initial)?))
                            } else {
                                (bounds.as_str(), None)
                            };
                        if before != initial.is_some() {
                            return Err("pre-check task loop requires a typed initial result; post-check forbids one".into());
                        }
                        let words: Vec<_> = bounds.split_whitespace().collect();
                        if words.is_empty() || words.len() > 4 || words.len() % 2 != 0 {
                            return Err(format!("invalid task loop bounds: {line}"));
                        }
                        let (mut max_iterations, mut max_duration) = (None, None);
                        for pair in words.chunks_exact(2) {
                            match pair[0] {
                                "max_iterations" if max_iterations.is_none() => {
                                    let count: u32 = pair[1].parse().map_err(|_| {
                                        format!("invalid max_iterations: {}", pair[1])
                                    })?;
                                    if count == 0 {
                                        return Err("max_iterations must be positive".into());
                                    }
                                    max_iterations = Some(count);
                                }
                                "max_duration" if max_duration.is_none() => {
                                    let span = duration(pair[1])?;
                                    if span.is_zero() || i64::try_from(span.as_millis()).is_err() {
                                        return Err(
                                            "max_duration must be positive and fit milliseconds"
                                                .into(),
                                        );
                                    }
                                    max_duration = Some(span);
                                }
                                _ => return Err(format!("invalid task loop bounds: {line}")),
                            }
                        }
                        NodeKind::TaskLoop {
                            task: task.into(),
                            input,
                            condition: expr::expression(condition)?,
                            before,
                            initial,
                            max_iterations,
                            max_duration,
                        }
                    } else {
                        NodeKind::Task {
                            task: task.into(),
                            input,
                        }
                    }
                }
                _ => {
                    let (signature, output) =
                        if let Some((signature, ty)) = declaration.split_once(": ") {
                            (signature, Some(super::type_ref(ty)?))
                        } else {
                            (declaration, None)
                        };
                    let (kind, split) = signature
                        .split_once('(')
                        .ok_or_else(|| format!("invalid node kind: {line}"))?;
                    let split = split
                        .strip_suffix(')')
                        .ok_or_else(|| format!("invalid join node: {line}"))?;
                    if !matches!(kind, "and_join" | "or_join" | "xor_join")
                        || !super::identifier(split)
                    {
                        return Err(format!("invalid join node: {line}"));
                    }
                    NodeKind::Join {
                        kind: kind.trim_end_matches("_join").into(),
                        split: split.into(),
                        output,
                    }
                }
            };
            graph.nodes.push(Node {
                name: name.into(),
                kind,
            });
        } else if let Some(text) = statement.strip_prefix("link ") {
            let (source, target) = text
                .split_once(" -> ")
                .ok_or_else(|| format!("invalid link: {line}"))?;
            if !super::identifier(source) {
                return Err(format!("invalid link: {line}"));
            }
            let (target, outcome) = if let Some(target) = target.strip_suffix(" on error") {
                (target, Some("error".to_owned()))
            } else if let Some(target) = target.strip_suffix(" on cancel") {
                (target, Some("cancel".to_owned()))
            } else if let Some(target) = target.strip_suffix(" on terminate") {
                (target, Some("terminate".to_owned()))
            } else {
                (target, None)
            };
            let (target, condition, fallback, label) =
                if let Some((target, cond)) = target.split_once(" when ") {
                    (target, Some(expr::expression(cond)?), false, None)
                } else if let Some(target) = target.strip_suffix(" else") {
                    (target, None, true, None)
                } else if let Some((target, label)) = target.split_once(" as ") {
                    if !super::identifier(label) {
                        return Err(format!("invalid branch label: {line}"));
                    }
                    (target, None, false, Some(label.to_owned()))
                } else {
                    (target, None, false, None)
                };
            let (target, value) = if let Some((target, arg)) = target.split_once('(') {
                let arg = arg
                    .strip_suffix(')')
                    .ok_or_else(|| format!("invalid link value: {line}"))?;
                (target, Some(expr::expression(arg)?))
            } else {
                (target, None)
            };
            if !super::identifier(target) {
                return Err(format!("invalid link target: {line}"));
            }
            graph.links.push(Link {
                source: source.into(),
                target: target.into(),
                value,
                condition,
                fallback,
                label,
                outcome,
            });
        } else if statement.starts_with("return ") {
            return Err("process return is not a terminal; link to an explicit end node".into());
        } else {
            return Err(format!("invalid process graph statement: {statement}"));
        }
    }
    if graph.nodes.is_empty() {
        return Err("missing process graph nodes".into());
    }
    Ok((graph, retry, deadline))
}
