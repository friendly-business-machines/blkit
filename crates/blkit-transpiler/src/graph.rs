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
        locals: &[String],
    ) -> Result<(), String> {
        if let Expr::Field(base, port) = expression
            && let Expr::Name(source) = base.as_ref()
            && !locals.contains(source)
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
            Expr::Field(base, _) | Expr::Not(base) | Expr::NamedArgument(_, base) => {
                rewrite(base, program, refs, locals)?
            }
            Expr::Some {
                bindings,
                predicate,
            } => {
                let mut scoped = locals.to_vec();
                for (binding, source) in bindings {
                    rewrite(source, program, refs, &scoped)?;
                    scoped.push(binding.clone());
                }
                rewrite(predicate, program, refs, &scoped)?;
            }
            Expr::Binary(left, _, right)
            | Expr::Index(left, right)
            | Expr::Sequence(left, right) => {
                rewrite(left, program, refs, locals)?;
                rewrite(right, program, refs, locals)?;
            }
            Expr::Call(_, args) | Expr::List(args) => {
                for arg in args {
                    rewrite(arg, program, refs, locals)?;
                }
            }
            Expr::Iteration {
                binding,
                source,
                body,
                ..
            } => {
                rewrite(source, program, refs, locals)?;
                let mut scoped = locals.to_vec();
                scoped.push(binding.clone());
                rewrite(body, program, refs, &scoped)?;
            }
            Expr::Dictionary(entries) => {
                for (_, value) in entries {
                    rewrite(value, program, refs, locals)?;
                }
            }
            Expr::Range(lower, upper, _, _) => {
                for bound in lower.iter_mut().chain(upper.iter_mut()) {
                    rewrite(bound, program, refs, locals)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut expression = expression.clone();
    let mut refs = Vec::new();
    rewrite(&mut expression, program, &mut refs, &[])?;
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

// Active braced grammar: consume original spanned tokens, not rendered statements.
#[derive(Debug)]
enum GraphStatement {
    Port(String, String, Type),
    Reference(String, String),
    Duration(String, chumsky::span::SimpleSpan),
    Retry(
        u32,
        (String, chumsky::span::SimpleSpan),
        (String, chumsky::span::SimpleSpan),
    ),
    Deadline(String, String, chumsky::span::SimpleSpan),
    Repeat(
        String,
        bool,
        Expr,
        Option<Expr>,
        Vec<(String, (String, chumsky::span::SimpleSpan))>,
    ),
    Multi(String, Expr, String),
    Flow(String, String, Option<Expr>, Option<FlowModifier>),
    Bind(String, String, String, String),
}

#[derive(Debug, Clone)]
enum FlowModifier {
    Else,
    Label(String),
    Outcome(String),
}

pub(crate) fn edge<'a, O, P, A, B>(
    source: &'a str,
    left: A,
    right: B,
) -> impl chumsky::Parser<'a, crate::compiler::Tokens<'a>, (O, P), crate::compiler::ParseError<'a>> + Clone
where
    A: chumsky::Parser<'a, crate::compiler::Tokens<'a>, O, crate::compiler::ParseError<'a>> + Clone,
    B: chumsky::Parser<'a, crate::compiler::Tokens<'a>, P, crate::compiler::ParseError<'a>> + Clone,
{
    use chumsky::prelude::*;
    left.map_with(|value, extra| (value, extra.span()))
        .then(just(crate::lexer::Token::Arrow).map_with(|_, extra| extra.span()))
        .then(right.map_with(|value, extra| (value, extra.span())))
        .try_map_with(move |(((left, a), arrow), (right, b)), extra| {
            if &source[a.end..arrow.start] == " " && &source[arrow.end..b.start] == " " {
                Ok((left, right))
            } else {
                Err(Rich::custom(extra.span(), "invalid arrow spacing"))
            }
        })
}

fn graph_statement<'a>(
    source: &'a str,
) -> impl chumsky::Parser<'a, crate::compiler::Tokens<'a>, GraphStatement, crate::compiler::ParseError<'a>>
{
    use crate::{
        compiler::{name, number, quoted, typed_port, word},
        lexer::Token,
    };
    use chumsky::prelude::*;
    let name = name(source);
    let port = choice((
        word(source, "input").to("input"),
        word(source, "output").to("output"),
    ))
    .then(typed_port(source))
    .map(|(direction, (name, ty))| GraphStatement::Port(direction.into(), name, ty));
    let reference = choice((
        word(source, "process").to("process"),
        word(source, "split").to("split"),
    ))
    .then(name.clone())
    .map(|(kind, name)| GraphStatement::Reference(kind.into(), name));
    let duration = word(source, "duration").ignore_then(
        quoted(source).map_with(|value, extra| GraphStatement::Duration(value, extra.span())),
    );
    let retry = word(source, "retry")
        .ignore_then(word(source, "max_retries"))
        .ignore_then(number(source).map(|value| value.parse::<u32>().ok()))
        .then_ignore(word(source, "retry_for"))
        .then(quoted(source).map_with(|value, extra| (value, extra.span())))
        .then_ignore(word(source, "retry_delay"))
        .then(quoted(source).map_with(|value, extra| (value, extra.span())))
        .then_ignore(word(source, "backoff"))
        .then_ignore(word(source, "exponential"))
        .try_map_with(|((max, window), delay), extra| {
            max.map(|max| GraphStatement::Retry(max, window, delay))
                .ok_or_else(|| Rich::custom(extra.span(), "invalid max_retries"))
        });
    let deadline = word(source, "deadline")
        .ignore_then(name.clone())
        .then(quoted(source).map_with(|value, extra| (value, extra.span())))
        .map(|(origin, (value, span))| GraphStatement::Deadline(origin, value, span));
    let expr = expr::expression_parser(source, &[]).boxed();
    let bound = choice((
        word(source, "max_iterations").to("max_iterations"),
        word(source, "max_duration").to("max_duration"),
    ))
    .then(choice((quoted(source), number(source))).map_with(|value, extra| (value, extra.span())));
    let repeat = choice((
        word(source, "repeat_pre").to(true),
        word(source, "repeat_post").to(false),
    ))
    .then(name.clone())
    .then_ignore(word(source, "while"))
    .then(expr.clone())
    .then(word(source, "initial").ignore_then(expr.clone()).or_not())
    .then(bound.repeated().at_least(1).at_most(2).collect::<Vec<_>>())
    .map(|((((before, node), condition), initial), bounds)| {
        GraphStatement::Repeat(
            node,
            before,
            condition,
            initial,
            bounds
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    });
    let multi = word(source, "multi_instance")
        .ignore_then(name.clone())
        .then_ignore(word(source, "each"))
        .then(expr.clone())
        .then(name.clone())
        .map(|((node, items), mode)| GraphStatement::Multi(node, items, mode));
    let modifier = choice((
        word(source, "when")
            .ignore_then(expr)
            .map(|expr| (Some(expr), None)),
        word(source, "else").to((None, Some(FlowModifier::Else))),
        word(source, "as")
            .ignore_then(name.clone())
            .map(|name| (None, Some(FlowModifier::Label(name)))),
        word(source, "on")
            .ignore_then(name.clone())
            .map(|name| (None, Some(FlowModifier::Outcome(name)))),
    ));
    let flow = word(source, "flow")
        .ignore_then(edge(source, name.clone(), name.clone()))
        .then(modifier.or_not())
        .map(|((source, target), modifier)| {
            let (condition, tail) = modifier.unwrap_or_default();
            GraphStatement::Flow(source, target, condition, tail)
        });
    let dotted = name
        .clone()
        .map_with(|value, extra| (value, extra.span()))
        .then(just(Token::Dot).map_with(|_, extra| extra.span()))
        .then(name.clone().map_with(|value, extra| (value, extra.span())))
        .try_map_with(|(((node, a), dot), (port, b)), extra| {
            if a.end == dot.start && dot.end == b.start {
                Ok((node, port))
            } else {
                Err(Rich::custom(extra.span(), "invalid binding port"))
            }
        });
    let bind = word(source, "bind")
        .ignore_then(edge(source, dotted.clone(), dotted))
        .map(|((source, output), (target, input))| {
            GraphStatement::Bind(source, output, target, input)
        });
    choice((
        port.boxed(),
        reference.boxed(),
        duration.boxed(),
        retry.boxed(),
        deadline.boxed(),
        repeat.boxed(),
        multi.boxed(),
        flow.boxed(),
        bind.boxed(),
    ))
}

pub(crate) fn check_statement(
    source: &str,
    tokens: &[(crate::lexer::Token, chumsky::span::SimpleSpan)],
    terminator: chumsky::span::SimpleSpan,
) -> Result<(), crate::compiler::CompilerError> {
    parse_graph_statement(source, tokens, terminator).map(|_| ())
}

fn parse_graph_statement(
    source: &str,
    tokens: &[(crate::lexer::Token, chumsky::span::SimpleSpan)],
    terminator: chumsky::span::SimpleSpan,
) -> Result<GraphStatement, crate::compiler::CompilerError> {
    crate::compiler::statement(tokens, terminator, graph_statement(source)).map_err(|error| {
        if error.message.contains("end of input") {
            let label = if tokens
                .first()
                .is_some_and(|(_, span)| &source[span.into_range()] == "flow")
                && tokens
                    .iter()
                    .skip(1)
                    .any(|(_, span)| &source[span.into_range()] == "bind")
            {
                "invalid flow"
            } else {
                "missing semicolon"
            };
            error.context(label)
        } else {
            error
        }
    })
}

fn statements(
    source: &str,
    body: &[crate::compiler::BodyItem],
) -> Result<(Vec<GraphStatement>, bool), crate::compiler::CompilerError> {
    let mut parsed = Vec::new();
    let mut damaged = false;
    let mut errors: Option<crate::compiler::CompilerError> = None;
    for item in body {
        let result = match item {
            crate::compiler::BodyItem::Invalid => {
                damaged = true;
                continue;
            }
            crate::compiler::BodyItem::Statement(tokens, terminator) => {
                parse_graph_statement(source, tokens, *terminator)
            }
            crate::compiler::BodyItem::Block(.., kind_span) => {
                Err(crate::compiler::CompilerError::located(
                    "invalid graph block",
                    kind_span.into_range(),
                ))
            }
        };
        match result {
            Ok(statement) => parsed.push(statement),
            Err(error) => {
                if let Some(errors) = &mut errors {
                    errors.append(error);
                } else {
                    errors = Some(error);
                }
            }
        }
    }
    errors.map_or(Ok((parsed, damaged)), Err)
}

pub(crate) fn parse_peer_tokens(
    source: &str,
    kind: &str,
    name: &str,
    body: &[crate::compiler::BodyItem],
) -> Result<Option<Peer>, crate::compiler::CompilerError> {
    let (items, damaged) = statements(source, body)?;
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    let mut split = None;
    let mut process = None;
    let mut pause = None;
    for item in items {
        match item {
            GraphStatement::Port(direction, port, ty)
                if matches!(
                    kind,
                    "start_event"
                        | "end_event"
                        | "pause_until"
                        | "subprocess"
                        | "xor_join"
                        | "and_join"
                        | "or_join"
                ) =>
            {
                let target = if direction == "output" {
                    &mut outputs
                } else {
                    &mut inputs
                };
                if (kind == "start_event" && direction != "output")
                    || (matches!(kind, "end_event" | "pause_until") && direction != "input")
                    || target
                        .iter()
                        .any(|(used, _): &(String, Type)| used == &port)
                {
                    return Err(format!("invalid or duplicate {kind} port: {port}").into());
                }
                target.push((port, ty));
            }
            GraphStatement::Reference(key, value)
                if key == "split" && kind.ends_with("_join") && split.is_none() =>
            {
                split = Some(value);
            }
            GraphStatement::Reference(key, value)
                if key == "process" && kind == "subprocess" && process.is_none() =>
            {
                process = Some(value);
            }
            GraphStatement::Duration(value, span) if kind == "pause_for" && pause.is_none() => {
                pause = Some(duration(&value).map_err(|error| {
                    crate::compiler::CompilerError::located(error, span.into_range())
                })?)
            }
            _ if matches!(kind, "error_event" | "cancel_event" | "terminate_event") => {
                return Err(format!("terminal {name} does not accept statements").into());
            }
            _ => return Err(format!("invalid {kind} statement").into()),
        }
    }
    if damaged {
        return Ok(None);
    }
    let kind = match kind {
        "start_event" => PeerKind::Start { outputs },
        "end_event" => PeerKind::End { inputs },
        "xor_split" | "and_split" | "or_split"
            if inputs.is_empty()
                && outputs.is_empty()
                && split.is_none()
                && process.is_none()
                && pause.is_none() =>
        {
            PeerKind::Split {
                kind: match kind {
                    "xor_split" => "xor",
                    "and_split" => "and",
                    _ => "or",
                },
            }
        }
        "xor_join" | "and_join" | "or_join" => PeerKind::Join {
            kind: match kind {
                "xor_join" => "xor",
                "and_join" => "and",
                _ => "or",
            },
            split: split.ok_or("join requires split")?,
            inputs,
            outputs,
        },
        "subprocess" => PeerKind::Subprocess {
            process: process.ok_or("subprocess requires process target")?,
            inputs,
            outputs,
        },
        "pause_until" => {
            let [(input, Type::Named(ty))] = inputs.as_slice() else {
                return Err("pause_until requires one DateTime input".into());
            };
            if ty != "DateTime" {
                return Err("pause_until input must be DateTime".into());
            }
            PeerKind::PauseUntil {
                input: input.clone(),
            }
        }
        "pause_for" => {
            let duration = pause.ok_or("pause_for requires duration")?;
            if duration.is_zero() {
                return Err("pause_for duration must be positive".into());
            }
            PeerKind::PauseFor(duration)
        }
        "error_event" | "cancel_event" | "terminate_event" => PeerKind::Terminal {
            kind: match kind {
                "error_event" => "Error",
                "cancel_event" => "Cancel",
                _ => "Terminate",
            },
        },
        _ => return Err(format!("invalid peer: {kind}").into()),
    };
    Ok(Some(Peer {
        name: name.into(),
        kind,
    }))
}

type ParsedSourceGraph = (
    SourceGraph,
    Option<crate::compiler::RetryPolicy>,
    Option<crate::compiler::DeadlinePolicy>,
);

pub(crate) fn parse_source_tokens(
    source: &str,
    body: &[crate::compiler::BodyItem],
) -> Result<Option<ParsedSourceGraph>, crate::compiler::CompilerError> {
    let mut retry = None;
    let mut deadline = None;
    let mut graph = SourceGraph {
        flows: vec![],
        routes: vec![],
        bindings: vec![],
        repetitions: vec![],
        multi_instances: vec![],
    };
    let (items, damaged) =
        statements(source, body).map_err(|error| error.context("invalid process statement"))?;
    for item in items {
        match item {
            GraphStatement::Retry(
                max_retries,
                (retry_for, for_span),
                (retry_delay, delay_span),
            ) => {
                if retry.is_some() || !graph.flows.is_empty() || !graph.bindings.is_empty() {
                    return Err("retry declaration must appear once before flows".into());
                }
                retry = Some(crate::compiler::RetryPolicy {
                    max_retries,
                    retry_for: duration(&retry_for).map_err(|error| {
                        crate::compiler::CompilerError::located(error, for_span.into_range())
                    })?,
                    retry_delay: duration(&retry_delay).map_err(|error| {
                        crate::compiler::CompilerError::located(error, delay_span.into_range())
                    })?,
                    backoff: "exponential",
                });
            }
            GraphStatement::Deadline(origin, value, span) => {
                if !matches!(origin.as_str(), "queued" | "first_claimed")
                    || deadline.is_some()
                    || !graph.flows.is_empty()
                    || !graph.bindings.is_empty()
                {
                    return Err("invalid or duplicate deadline declaration".into());
                }
                let duration = duration(&value).map_err(|error| {
                    crate::compiler::CompilerError::located(error, span.into_range())
                })?;
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
            }
            GraphStatement::Repeat(node, before, condition, initial, bounds) => {
                if graph
                    .repetitions
                    .iter()
                    .any(|item: &Repetition| item.node == node)
                {
                    return Err(format!("duplicate repeat node: {node}").into());
                }
                if before != initial.is_some() {
                    return Err("repeat_pre requires initial; repeat_post forbids initial".into());
                }
                let (mut max_iterations, mut max_duration) = (None, None);
                for (key, (value, value_span)) in bounds {
                    match key.as_str() {
                        "max_iterations" if max_iterations.is_none() => {
                            let count =
                                value.parse::<u32>().map_err(|_| "invalid max_iterations")?;
                            if count == 0 {
                                return Err("max_iterations must be positive".into());
                            }
                            max_iterations = Some(count);
                        }
                        "max_duration" if max_duration.is_none() => {
                            let duration = duration(&value).map_err(|error| {
                                crate::compiler::CompilerError::located(
                                    error,
                                    value_span.into_range(),
                                )
                            })?;
                            if duration.is_zero() || i64::try_from(duration.as_millis()).is_err() {
                                return Err(crate::compiler::CompilerError::located(
                                    "max_duration must be positive and fit milliseconds",
                                    value_span.into_range(),
                                ));
                            }
                            max_duration = Some(duration);
                        }
                        _ => return Err("invalid repeat bounds".into()),
                    }
                }
                graph.repetitions.push(Repetition {
                    node,
                    condition,
                    before,
                    initial,
                    max_iterations,
                    max_duration,
                });
            }
            GraphStatement::Multi(node, items, mode) => {
                if !matches!(mode.as_str(), "parallel" | "sequential")
                    || graph
                        .multi_instances
                        .iter()
                        .any(|item: &MultiInstance| item.node == node)
                {
                    return Err("invalid or duplicate multi_instance declaration".into());
                }
                graph.multi_instances.push(MultiInstance {
                    node,
                    items,
                    parallel: mode == "parallel",
                });
            }
            GraphStatement::Flow(from, to, condition, tail) => {
                if graph.flows.iter().any(|(a, b)| a == &from && b == &to) {
                    return Err(format!("duplicate flow: {from} -> {to}").into());
                }
                let (fallback, label, outcome) = match tail {
                    None => (false, None, None),
                    Some(FlowModifier::Else) => (true, None, None),
                    Some(FlowModifier::Label(label)) => (false, Some(label), None),
                    Some(FlowModifier::Outcome(outcome))
                        if matches!(outcome.as_str(), "error" | "cancel" | "terminate") =>
                    {
                        (false, None, Some(outcome))
                    }
                    Some(FlowModifier::Outcome(outcome)) => {
                        return Err(format!("invalid subprocess outcome: {outcome}").into());
                    }
                };
                if condition.is_some() || fallback || label.is_some() || outcome.is_some() {
                    graph.routes.push(Route {
                        source: from.clone(),
                        target: to.clone(),
                        condition,
                        fallback,
                        label,
                        outcome,
                    });
                }
                graph.flows.push((from, to));
            }
            GraphStatement::Bind(source, output, target, input) => graph.bindings.push(Binding {
                source,
                output,
                target,
                input,
            }),
            _ => return Err("invalid process statement".into()),
        }
    }
    Ok((!damaged).then_some((graph, retry, deadline)))
}
