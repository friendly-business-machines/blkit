use crate::{
    Type,
    expr::{self, Expr},
    identifier, type_ref,
};

#[derive(Debug)]
pub struct DecisionModel {
    pub name: String,
    pub input: String,
    pub input_type: Type,
    pub output: Type,
    pub output_node: String,
    pub nodes: Vec<DecisionNode>,
    pub links: Vec<(String, String)>,
    pub knowledge: Vec<Knowledge>,
}

#[derive(Debug)]
pub struct DecisionNode {
    pub name: String,
    pub output: Type,
    pub kind: DecisionKind,
}

#[derive(Debug)]
pub enum DecisionKind {
    Literal(Expr),
    Context {
        entries: Vec<(String, Type, Expr)>,
        result: Expr,
    },
    Table(DecisionTable),
}

#[derive(Debug)]
pub struct DecisionTable {
    pub policy: String,
    pub aggregation: Option<String>,
    pub inputs: Vec<(String, Type, Expr)>,
    pub outputs: Vec<(String, Type)>,
    pub rules: Vec<(Expr, Vec<Expr>)>,
    pub priorities: Vec<Vec<Expr>>,
    pub default: Option<Vec<Expr>>,
}

fn expressions(text: &str) -> Result<Vec<Expr>, String> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0_i32;
    let mut quoted = false;
    for (i, ch) in text.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            '[' | '(' if !quoted => depth += 1,
            ']' | ')' if !quoted => depth -= 1,
            ',' if !quoted && depth == 0 => {
                parts.push(expr::expression(text[start..i].trim())?);
                start = i + 1;
            }
            _ => {}
        }
        if depth < 0 {
            return Err("invalid table expression".into());
        }
    }
    if quoted || depth != 0 {
        return Err("invalid table expression".into());
    }
    parts.push(expr::expression(text[start..].trim())?);
    Ok(parts)
}

#[derive(Debug)]
pub struct Knowledge {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub output: Type,
    pub body: Expr,
}

fn signature(text: &str) -> Result<(String, Vec<(String, Type)>, Type), String> {
    let (name, rest) = text.split_once('(').ok_or("invalid decision signature")?;
    let (params, output) = rest
        .split_once(") -> ")
        .ok_or("invalid decision signature")?;
    if !identifier(name) {
        return Err("invalid decision signature".into());
    }
    let params = if params.is_empty() {
        vec![]
    } else {
        params
            .split(", ")
            .map(|part| {
                let (name, ty) = part.split_once(": ").ok_or("invalid decision parameter")?;
                if !identifier(name) {
                    return Err("invalid decision parameter".into());
                }
                Ok((name.into(), type_ref(ty)?))
            })
            .collect::<Result<Vec<_>, String>>()?
    };
    Ok((name.into(), params, type_ref(output)?))
}

pub fn parse(header: &str, lines: &[String]) -> Result<DecisionModel, String> {
    let (name, params, output) = signature(
        header
            .strip_suffix(':')
            .ok_or("invalid decision signature")?,
    )?;
    let [(input, input_type)] = params.as_slice() else {
        return Err("decision requires one input parameter".into());
    };
    let mut model = DecisionModel {
        name,
        input: input.clone(),
        input_type: input_type.clone(),
        output,
        output_node: String::new(),
        nodes: vec![],
        links: vec![],
        knowledge: vec![],
    };
    let mut i = 0;
    while i < lines.len() {
        let statement = lines[i]
            .strip_prefix("  ")
            .filter(|s| !s.starts_with(' '))
            .ok_or_else(|| format!("invalid decision indentation: {}", lines[i]))?;
        if let Some(text) = statement.strip_prefix("knowledge ") {
            let (sig, body) = text
                .split_once(" = ")
                .ok_or("invalid knowledge declaration")?;
            let (name, params, output) = signature(sig)?;
            model.knowledge.push(Knowledge {
                name,
                params,
                output,
                body: expr::expression(body)?,
            });
        } else if let Some(text) = statement.strip_prefix("node ") {
            let (sig, body) = text.split_once(" = ").ok_or("invalid decision node")?;
            let (name, ty) = sig.split_once(": ").ok_or("invalid decision node")?;
            if !identifier(name) {
                return Err("invalid decision node".into());
            }
            let kind = if let Some(value) = body.strip_prefix("literal ") {
                DecisionKind::Literal(expr::expression(value)?)
            } else if body == "context" {
                let mut entries = vec![];
                let mut result = None;
                while i + 1 < lines.len() && lines[i + 1].starts_with("    ") {
                    i += 1;
                    let nested = lines[i]
                        .strip_prefix("    ")
                        .filter(|s| !s.starts_with(' '))
                        .ok_or("invalid context indentation")?;
                    if let Some(value) = nested.strip_prefix("entry ") {
                        if result.is_some() {
                            return Err("entry after context result".into());
                        }
                        let (sig, expr) = value.split_once(" = ").ok_or("invalid context entry")?;
                        let (name, ty) = sig.split_once(": ").ok_or("invalid context entry")?;
                        if !identifier(name) {
                            return Err("invalid context entry".into());
                        }
                        entries.push((name.into(), type_ref(ty)?, expr::expression(expr)?));
                    } else if let Some(value) = nested.strip_prefix("result ") {
                        if result.is_some() {
                            return Err("duplicate context result".into());
                        }
                        result = Some(expr::expression(value)?);
                    } else {
                        return Err("invalid context statement".into());
                    }
                }
                DecisionKind::Context {
                    entries,
                    result: result.ok_or("missing context result")?,
                }
            } else if let Some(policy) = body.strip_prefix("table ") {
                let mut parts = policy.split(' ');
                let kind = parts.next().unwrap();
                let aggregation = parts.next().map(str::to_owned);
                if parts.next().is_some()
                    || !matches!(
                        kind,
                        "UNIQUE"
                            | "ANY"
                            | "FIRST"
                            | "PRIORITY"
                            | "RULE_ORDER"
                            | "OUTPUT_ORDER"
                            | "COLLECT"
                    )
                {
                    return Err("invalid table policy".into());
                }
                let mut table = DecisionTable {
                    policy: kind.into(),
                    aggregation,
                    inputs: vec![],
                    outputs: vec![],
                    rules: vec![],
                    priorities: vec![],
                    default: None,
                };
                while i + 1 < lines.len() && lines[i + 1].starts_with("    ") {
                    i += 1;
                    let nested = lines[i]
                        .strip_prefix("    ")
                        .filter(|s| !s.starts_with(' '))
                        .ok_or("invalid table indentation")?;
                    if let Some(text) = nested.strip_prefix("input ") {
                        let (column, expression) =
                            text.split_once(" = ").ok_or("invalid table input")?;
                        let (name, ty) = column.split_once(": ").ok_or("invalid table input")?;
                        if !identifier(name) {
                            return Err("invalid table input".into());
                        }
                        table.inputs.push((
                            name.into(),
                            type_ref(ty)?,
                            expr::expression(expression)?,
                        ));
                    } else if let Some(text) = nested.strip_prefix("output ") {
                        let (name, ty) = text.split_once(": ").ok_or("invalid table output")?;
                        if !identifier(name) {
                            return Err("invalid table output".into());
                        }
                        table.outputs.push((name.into(), type_ref(ty)?));
                    } else if let Some(text) = nested.strip_prefix("rule ") {
                        let (condition, values) =
                            text.split_once(" -> ").ok_or("invalid table rule")?;
                        table
                            .rules
                            .push((expr::expression(condition)?, expressions(values)?));
                    } else if let Some(text) = nested.strip_prefix("priority ") {
                        table.priorities.push(expressions(text)?);
                    } else if let Some(text) = nested.strip_prefix("default ") {
                        if table.default.is_some() {
                            return Err("duplicate table default".into());
                        }
                        table.default = Some(expressions(text)?);
                    } else {
                        return Err("invalid table statement".into());
                    }
                }
                DecisionKind::Table(table)
            } else {
                return Err("invalid decision node kind".into());
            };
            model.nodes.push(DecisionNode {
                name: name.into(),
                output: type_ref(ty)?,
                kind,
            });
        } else if let Some(text) = statement.strip_prefix("link ") {
            let (from, to) = text.split_once(" -> ").ok_or("invalid decision link")?;
            if !identifier(from) || !identifier(to) {
                return Err("invalid decision link".into());
            }
            model.links.push((from.into(), to.into()));
        } else if let Some(text) = statement.strip_prefix("output ") {
            if !identifier(text) || !model.output_node.is_empty() {
                return Err("invalid decision output".into());
            }
            model.output_node = text.into();
        } else {
            return Err(format!("invalid decision statement: {statement}"));
        }
        i += 1;
    }
    if model.output_node.is_empty() {
        return Err("missing decision output".into());
    }
    Ok(model)
}
