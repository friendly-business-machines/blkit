use crate::{codegen, decision, expr, graph, semantic};
use std::collections::BTreeMap;

pub use blkit_core::{DeadlinePolicy, RetryPolicy};

pub fn transpile(source: &str) -> Result<String, String> {
    let program = parse(source)?;
    semantic::validate(&program)?;
    codegen::generate(&program)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Named(String),
    Generic(String, Box<Type>),
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Named(name) => write!(f, "{name}"),
            Self::Generic(name, inner) => write!(f, "{name}<{inner}>"),
        }
    }
}

#[derive(Debug)]
pub struct Record {
    pub name: String,
    pub fields: Vec<(String, Type)>,
}

#[derive(Debug)]
pub struct Enum {
    pub name: String,
    pub variants: Vec<String>,
}

#[derive(Debug)]
pub struct Process {
    pub name: String,
    pub input: String,
    pub input_type: Type,
    pub output: Type,
    pub body: Vec<expr::Stmt>,
    pub named_graph: Option<graph::NamedGraph>,
    pub source_graph: Option<graph::SourceGraph>,
    pub retry: Option<RetryPolicy>,
    pub deadline: Option<DeadlinePolicy>,
}

#[derive(Debug, Clone)]
pub struct ExternalTask {
    pub function: String,
    pub input: Type,
    pub output: Type,
}

#[derive(Debug)]
pub struct Program {
    pub namespace: String,
    pub version: String,
    pub records: Vec<Record>,
    pub enums: Vec<Enum>,
    pub processes: Vec<Process>,
    pub tasks: Vec<Process>,
    pub decisions: Vec<decision::DecisionModel>,
    pub peer_nodes: Vec<graph::Peer>,
    pub external_tasks: BTreeMap<String, ExternalTask>,
}

pub(crate) fn type_ref(text: &str) -> Result<Type, String> {
    if let Some((name, rest)) = text.split_once('<') {
        let inner = rest.strip_suffix('>').ok_or("unclosed type argument")?;
        if name.is_empty() || inner.is_empty() {
            return Err("invalid type argument".into());
        }
        Ok(Type::Generic(name.into(), Box::new(type_ref(inner)?)))
    } else if identifier(text) {
        Ok(Type::Named(text.into()))
    } else {
        Err(format!("invalid type {text}"))
    }
}

pub(crate) fn split_top_level(text: &str, separator: char) -> Result<Vec<&str>, String> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0_i32;
    let mut quoted = false;
    for (index, ch) in text.char_indices() {
        if ch == '"' {
            quoted = !quoted;
        }
        if !quoted {
            match ch {
                '{' | '[' | '(' => depth += 1,
                '}' | ']' | ')' => depth -= 1,
                _ => {}
            }
            if depth < 0 {
                return Err("unbalanced dictionary expression".into());
            }
            if ch == separator && depth == 0 {
                parts.push(&text[start..index]);
                start = index + ch.len_utf8();
            }
        }
    }
    if quoted || depth != 0 {
        return Err("unbalanced dictionary expression".into());
    }
    parts.push(&text[start..]);
    Ok(parts)
}

pub(crate) fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn source_items(source: &str) -> Result<Vec<String>, String> {
    let mut items = Vec::new();
    let mut pending = String::new();
    let mut quoted = false;
    let mut depth = 0usize;
    let mut literal_depth = 0usize;
    for line in source
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
    {
        for ch in line.chars() {
            match ch {
                '"' => {
                    quoted = !quoted;
                    pending.push(ch);
                }
                ';' if !quoted => {
                    let text = pending.trim_end();
                    if text.trim().is_empty() {
                        return Err("empty statement".into());
                    }
                    items.push(if text.starts_with("  ") && depth == 0 {
                        text.to_owned()
                    } else {
                        text.trim_start().to_owned()
                    });
                    pending.clear();
                }
                '{' if !quoted => {
                    let first = pending.split_whitespace().next().unwrap_or("");
                    let declaration = matches!(
                        first,
                        "start_event"
                            | "end_event"
                            | "decision_task"
                            | "process"
                            | "xor_split"
                            | "xor_join"
                            | "and_split"
                            | "and_join"
                            | "or_split"
                            | "or_join"
                            | "error_event"
                            | "cancel_event"
                            | "terminate_event"
                            | "pause_for"
                            | "pause_until"
                            | "subprocess"
                            | "literal_expression"
                            | "decision_table"
                            | "context"
                            | "knowledge"
                    );
                    if literal_depth > 0 || !declaration {
                        pending.push(ch);
                        literal_depth += 1;
                    } else {
                        items.push(pending.trim().to_owned());
                        items.push("{".into());
                        pending.clear();
                        depth += 1;
                    }
                }
                '}' if !quoted && literal_depth > 0 => {
                    pending.push(ch);
                    literal_depth -= 1;
                }
                '}' if !quoted => {
                    if !pending.trim().is_empty() || depth == 0 {
                        return Err("missing semicolon or unexpected '}'".into());
                    }
                    depth -= 1;
                    items.push("}".into());
                }
                _ => pending.push(ch),
            }
        }
        if quoted {
            return Err("unterminated string".into());
        }
        if pending.trim_end().ends_with(':') && depth == 0 {
            items.push(pending.trim().to_owned());
            pending.clear();
        } else if !pending.is_empty() {
            pending.push(' ');
        }
    }
    if !pending.trim().is_empty() {
        return Err("missing semicolon".into());
    }
    if depth != 0 || literal_depth != 0 {
        return Err("unclosed brace".into());
    }
    Ok(items)
}

pub(crate) fn block_items(items: &[String], index: &mut usize) -> Result<Vec<String>, String> {
    if items.get(*index).map(String::as_str) != Some("{") {
        return Err("expected '{' after declaration".into());
    }
    *index += 1;
    let start = *index;
    let mut depth = 1;
    while *index < items.len() {
        match items[*index].as_str() {
            "{" => depth += 1,
            "}" => {
                depth -= 1;
                if depth == 0 {
                    let body = items[start..*index].to_vec();
                    *index += 1;
                    return Ok(body);
                }
            }
            _ => {}
        }
        *index += 1;
    }
    Err("unclosed declaration brace".into())
}

pub fn parse(source: &str) -> Result<Program, String> {
    let lines = source_items(source)?;
    let namespace = lines
        .first()
        .and_then(|line| line.strip_prefix("namespace "))
        .filter(|name| identifier(name))
        .ok_or("expected namespace declaration")?
        .to_owned();
    let version = lines
        .get(1)
        .and_then(|line| line.strip_prefix("version \""))
        .and_then(|text| text.strip_suffix('"'))
        .filter(|value| !value.is_empty())
        .ok_or("expected version declaration")?
        .to_owned();
    let mut program = Program {
        namespace,
        version,
        records: Vec::new(),
        enums: Vec::new(),
        processes: Vec::new(),
        tasks: Vec::new(),
        decisions: Vec::new(),
        peer_nodes: Vec::new(),
        external_tasks: BTreeMap::new(),
    };
    let mut i = 2;
    while i < lines.len() {
        let header = lines[i].as_str();
        if let Some((name, value)) = header.split_once(" = ")
            && identifier(name)
            && let Some(fields) = value
                .trim()
                .strip_prefix('{')
                .and_then(|v| v.strip_suffix('}'))
        {
            let mut record = Record {
                name: name.into(),
                fields: Vec::new(),
            };
            for member in split_top_level(fields, ',')?
                .into_iter()
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                let parts = split_top_level(member, ':')?;
                let [key, ty] = parts.as_slice() else {
                    return Err("invalid dictionary field".into());
                };
                let raw_key = key.trim();
                let quoted =
                    raw_key.starts_with('"') && raw_key.ends_with('"') && raw_key.len() >= 2;
                let key = if quoted {
                    &raw_key[1..raw_key.len() - 1]
                } else {
                    raw_key
                };
                if !identifier(key) && !quoted {
                    return Err(format!("invalid dictionary field: {member}"));
                }
                if record.fields.iter().any(|(field, _)| field == key) {
                    return Err(format!("duplicate dictionary field: {key}"));
                }
                record.fields.push((key.into(), type_ref(ty.trim())?));
            }
            program.records.push(record);
            i += 1;
            continue;
        }
        if let Some(name) = header
            .strip_prefix("type ")
            .and_then(|value| value.strip_suffix(':'))
        {
            return Err(format!(
                "type {name}: is unsupported; use {name} = {{field: Type, ...}};"
            ));
        }
        let (kind, name) = header
            .split_once(' ')
            .ok_or_else(|| format!("invalid declaration: {header}"))?;
        if matches!(
            kind,
            "start_event"
                | "end_event"
                | "decision_task"
                | "process"
                | "xor_split"
                | "xor_join"
                | "and_split"
                | "and_join"
                | "or_split"
                | "or_join"
                | "error_event"
                | "cancel_event"
                | "terminate_event"
                | "pause_for"
                | "pause_until"
                | "subprocess"
        ) {
            if !identifier(name) {
                return Err(format!("invalid declaration name: {header}"));
            }
            i += 1;
            let body = block_items(&lines, &mut i)?;
            match kind {
                "start_event" | "end_event" | "xor_split" | "xor_join" | "and_split"
                | "and_join" | "or_split" | "or_join" | "error_event" | "cancel_event"
                | "terminate_event" | "pause_for" | "pause_until" | "subprocess" => {
                    program
                        .peer_nodes
                        .push(graph::parse_peer(kind, name, &body)?);
                }
                "decision_task" => program.decisions.push(decision::parse_braced(name, &body)?),
                "process" => {
                    let (graph, retry, deadline) = graph::parse_source(&body)?;
                    program.processes.push(Process {
                        name: name.into(),
                        input: String::new(),
                        input_type: Type::Named("Number".into()),
                        output: Type::Named("Number".into()),
                        body: Vec::new(),
                        named_graph: None,
                        source_graph: Some(graph),
                        retry,
                        deadline,
                    });
                }
                _ => unreachable!(),
            }
            continue;
        }
        if matches!(kind, "task" | "decision") {
            return Err(format!("unsupported legacy declaration: {header}"));
        }
        let name = name
            .strip_suffix(':')
            .filter(|name| identifier(name))
            .ok_or_else(|| format!("invalid declaration: {header}"))?;
        match kind {
            "enum" => program.enums.push(Enum {
                name: name.into(),
                variants: Vec::new(),
            }),
            _ => return Err(format!("unknown declaration: {header}")),
        }
        i += 1;
        let start = i;
        while i < lines.len() && lines[i].starts_with(' ') {
            i += 1;
        }
        if start == i {
            return Err(format!("missing body for declaration: {header}"));
        }
        for line in &lines[start..i] {
            if !line.starts_with("  ") {
                return Err(format!("expected indentation: {line}"));
            }
            match kind {
                "enum" => {
                    let variant = &line[2..];
                    if !identifier(variant) {
                        return Err(format!("invalid variant: {line}"));
                    }
                    program
                        .enums
                        .last_mut()
                        .unwrap()
                        .variants
                        .push(variant.into());
                }
                _ => unreachable!(),
            }
        }
    }
    Ok(program)
}
