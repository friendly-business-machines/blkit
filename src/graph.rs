use crate::{Type, expr::{self, Expr}};

#[derive(Debug)]
pub enum GraphStmt {
    Run { name: String, task: String, input: Expr },
    Gateway { kind: String, branches: Vec<Branch>, join: String, output: Option<Type> },
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
        if spaces < indent { break; }
        if spaces != indent { return Err(format!("unexpected indentation: {line}")); }
        let statement = &line[indent..];
        if let Some(text) = statement.strip_prefix("run ") {
            let (name, call) = text.split_once(" = ").ok_or_else(|| format!("invalid task link: {line}"))?;
            let (task, argument) = call.split_once('(').ok_or_else(|| format!("invalid task call: {line}"))?;
            let argument = argument.strip_suffix(')').ok_or_else(|| format!("invalid task call: {line}"))?;
            if !super::identifier(name) || !super::identifier(task) {
                return Err(format!("invalid task link: {line}"));
            }
            body.push(GraphStmt::Run { name: name.into(), task: task.into(), input: expr::expression(argument)? });
            *index += 1;
        } else if matches!(statement, "and:" | "or:" | "xor:") {
            let kind = statement.trim_end_matches(':').to_owned();
            *index += 1;
            let mut branches = Vec::new();
            let mut fallback = false;
            while *index < lines.len() && lines[*index].starts_with(&" ".repeat(indent + 2)) {
                let line = &lines[*index];
                if !line.starts_with(&" ".repeat(indent + 2)) || line.starts_with(&" ".repeat(indent + 3)) {
                    return Err(format!("unexpected indentation: {line}"));
                }
                let header = &line[indent + 2..];
                let (label, condition) = if let Some(name) = header.strip_prefix("branch ").and_then(|s| s.strip_suffix(':')) {
                    if kind != "and" || !super::identifier(name) { return Err(format!("invalid branch: {line}")); }
                    (Some(name.into()), None)
                } else if let Some(text) = header.strip_prefix("when ").and_then(|s| s.strip_suffix(':')) {
                    if kind == "and" || fallback { return Err(format!("invalid gateway condition: {line}")); }
                    (None, Some(expr::expression(text)?))
                } else if header == "else:" && kind != "and" && !fallback {
                    fallback = true;
                    (None, None)
                } else {
                    return Err(format!("invalid gateway branch: {line}"));
                };
                *index += 1;
                let branch_body = block(lines, index, indent + 4)?;
                if branch_body.is_empty() { return Err(format!("empty gateway branch: {line}")); }
                branches.push(Branch { label, condition, body: branch_body });
            }
            if branches.is_empty() { return Err(format!("empty gateway: {statement}")); }
            if kind != "and" && !branches.iter().any(|branch| branch.condition.is_some()) {
                return Err(format!("missing condition for {kind} gateway"));
            }
            if kind != "and" && !fallback { return Err(format!("missing else fallback for {kind} gateway")); }
            let line = lines.get(*index).ok_or_else(|| format!("missing join for {kind} gateway"))?;
            let text = line.strip_prefix(&format!("{}join ", " ".repeat(indent)))
                .ok_or_else(|| format!("missing join for {kind} gateway"))?;
            let (name, output) = if let Some((name, ty)) = text.split_once(": ") {
                (name, Some(super::type_ref(ty)?))
            } else { (text, None) };
            if !super::identifier(name) { return Err(format!("invalid join: {line}")); }
            if (kind == "and") != output.is_some() { return Err(format!("invalid {kind} join type: {line}")); }
            body.push(GraphStmt::Gateway { kind, branches, join: name.into(), output });
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
