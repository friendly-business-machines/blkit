use crate::{codegen, expr, graph, semantic};

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
    pub graph: Vec<graph::GraphStmt>,
}

#[derive(Debug)]
pub struct Program {
    pub namespace: String,
    pub version: String,
    pub records: Vec<Record>,
    pub enums: Vec<Enum>,
    pub processes: Vec<Process>,
    pub tasks: Vec<Process>,
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

pub(crate) fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn parse(source: &str) -> Result<Program, String> {
    let lines: Vec<_> = source
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty() && !line.trim_start().starts_with('#'))
        .collect();
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
    };
    let mut i = 2;
    while i < lines.len() {
        let header = lines[i];
        let (kind, name) = header
            .split_once(' ')
            .ok_or_else(|| format!("invalid declaration: {header}"))?;
        if kind == "process" || kind == "task" {
            let (name, rest) = name
                .split_once('(')
                .ok_or_else(|| format!("invalid process signature: {header}"))?;
            let (input, output) = rest
                .split_once(") -> ")
                .ok_or_else(|| format!("invalid process signature: {header}"))?;
            let (input_name, input_type) = input
                .split_once(": ")
                .ok_or_else(|| format!("invalid process signature: {header}"))?;
            let output = output
                .strip_suffix(':')
                .ok_or_else(|| format!("invalid process signature: {header}"))?;
            if !identifier(name) || !identifier(input_name) {
                return Err(format!("invalid process signature: {header}"));
            }
            let process = Process {
                name: name.into(),
                input: input_name.into(),
                input_type: type_ref(input_type)?,
                output: type_ref(output)?,
                body: Vec::new(),
                graph: Vec::new(),
            };
            if kind == "task" { program.tasks.push(process); } else { program.processes.push(process); }
        } else {
            let name = name
                .strip_suffix(':')
                .filter(|name| identifier(name))
                .ok_or_else(|| format!("invalid declaration: {header}"))?;
            match kind {
                "type" => program.records.push(Record {
                    name: name.into(),
                    fields: Vec::new(),
                }),
                "enum" => program.enums.push(Enum {
                    name: name.into(),
                    variants: Vec::new(),
                }),
                _ => return Err(format!("unknown declaration: {header}")),
            }
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
                "type" => {
                    let (field, ty) = line[2..]
                        .split_once(": ")
                        .ok_or_else(|| format!("invalid field: {line}"))?;
                    if !identifier(field) {
                        return Err(format!("invalid field: {line}"));
                    }
                    program.records.last_mut().unwrap().fields.push((field.into(), type_ref(ty)?));
                }
                "enum" => {
                    let variant = &line[2..];
                    if !identifier(variant) {
                        return Err(format!("invalid variant: {line}"));
                    }
                    program.enums.last_mut().unwrap().variants.push(variant.into());
                }
                _ => {
                    // Process blocks are parsed together to preserve nested indentation.
                },
            }
        }
        if kind == "process" || kind == "task" {
            let body: Vec<String> = lines[start..i].iter().map(|line| (*line).into()).collect();
            let item = if kind == "task" { program.tasks.last_mut().unwrap() } else { program.processes.last_mut().unwrap() };
            if kind == "process" && (body[0].starts_with("  run ") || ["  and:", "  or:", "  xor:"].contains(&body[0].as_str())) {
                item.graph = graph::parse(&body)?;
            } else {
                item.body = expr::body(&body)?;
            }
        }
    }
    Ok(program)
}
