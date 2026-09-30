use crate::{
    Type,
    expr::{self, Expr},
};

#[derive(Debug)]
pub enum GraphStmt {
    Run {
        name: String,
        task: String,
        input: Expr,
    },
    Gateway {
        kind: String,
        branches: Vec<Branch>,
        join: String,
        output: Option<Type>,
    },
    Return(Expr),
}

#[derive(Debug)]
pub struct Branch {
    pub label: Option<String>,
    pub condition: Option<Expr>,
    pub body: Vec<GraphStmt>,
}

pub fn parse(lines: &[String]) -> Result<Vec<GraphStmt>, String> {
    let mut index = 0;
    let body = block(lines, &mut index, 2)?;
    if index != lines.len() {
        return Err(format!("unexpected graph line: {}", lines[index]));
    }
    Ok(body)
}

fn block(lines: &[String], index: &mut usize, indent: usize) -> Result<Vec<GraphStmt>, String> {
    let mut body = Vec::new();
    while *index < lines.len() {
        let line = &lines[*index];
        let spaces = line.len() - line.trim_start_matches(' ').len();
        if spaces < indent {
            break;
        }
        if spaces != indent {
            return Err(format!("unexpected indentation: {line}"));
        }
        let statement = &line[indent..];
        if let Some(text) = statement.strip_prefix("run ") {
            let (name, call) = text
                .split_once(" = ")
                .ok_or_else(|| format!("invalid task link: {line}"))?;
            let (task, argument) = call
                .split_once('(')
                .ok_or_else(|| format!("invalid task call: {line}"))?;
            let argument = argument
                .strip_suffix(')')
                .ok_or_else(|| format!("invalid task call: {line}"))?;
            if !super::identifier(name) || !super::identifier(task) {
                return Err(format!("invalid task link: {line}"));
            }
            body.push(GraphStmt::Run {
                name: name.into(),
                task: task.into(),
                input: expr::expression(argument)?,
            });
            *index += 1;
        } else if matches!(statement, "and:" | "or:" | "xor:") {
            let kind = statement.trim_end_matches(':').to_owned();
            *index += 1;
            let mut branches = Vec::new();
            let mut fallback = false;
            while *index < lines.len() && lines[*index].starts_with(&" ".repeat(indent + 2)) {
                let line = &lines[*index];
                if !line.starts_with(&" ".repeat(indent + 2))
                    || line.starts_with(&" ".repeat(indent + 3))
                {
                    return Err(format!("unexpected indentation: {line}"));
                }
                let header = &line[indent + 2..];
                let (label, condition) = if let Some(name) = header
                    .strip_prefix("branch ")
                    .and_then(|s| s.strip_suffix(':'))
                {
                    if kind != "and" || !super::identifier(name) {
                        return Err(format!("invalid branch: {line}"));
                    }
                    (Some(name.into()), None)
                } else if let Some(text) = header
                    .strip_prefix("when ")
                    .and_then(|s| s.strip_suffix(':'))
                {
                    if kind == "and" || fallback {
                        return Err(format!("invalid gateway condition: {line}"));
                    }
                    (None, Some(expr::expression(text)?))
                } else if header == "else:" && kind != "and" && !fallback {
                    fallback = true;
                    (None, None)
                } else {
                    return Err(format!("invalid gateway branch: {line}"));
                };
                *index += 1;
                let branch_body = block(lines, index, indent + 4)?;
                if branch_body.is_empty() {
                    return Err(format!("empty gateway branch: {line}"));
                }
                branches.push(Branch {
                    label,
                    condition,
                    body: branch_body,
                });
            }
            if branches.is_empty() {
                return Err(format!("empty gateway: {statement}"));
            }
            if kind != "and" && !branches.iter().any(|branch| branch.condition.is_some()) {
                return Err(format!("missing condition for {kind} gateway"));
            }
            if kind != "and" && !fallback {
                return Err(format!("missing else fallback for {kind} gateway"));
            }
            let line = lines
                .get(*index)
                .ok_or_else(|| format!("missing join for {kind} gateway"))?;
            let text = line
                .strip_prefix(&format!("{}join ", " ".repeat(indent)))
                .ok_or_else(|| format!("missing join for {kind} gateway"))?;
            let (name, output) = if let Some((name, ty)) = text.split_once(": ") {
                (name, Some(super::type_ref(ty)?))
            } else {
                (text, None)
            };
            if !super::identifier(name) {
                return Err(format!("invalid join: {line}"));
            }
            if (kind == "and") != output.is_some() {
                return Err(format!("invalid {kind} join type: {line}"));
            }
            body.push(GraphStmt::Gateway {
                kind,
                branches,
                join: name.into(),
                output,
            });
            *index += 1;
        } else if let Some(value) = statement.strip_prefix("return ") {
            body.push(GraphStmt::Return(expr::expression(value)?));
            *index += 1;
        } else {
            return Err(format!("invalid graph statement: {statement}"));
        }
    }
    Ok(body)
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
                        if words.len() == 0 || words.len() > 4 || words.len() % 2 != 0 {
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
