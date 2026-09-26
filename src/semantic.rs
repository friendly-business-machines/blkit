use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use rust_decimal::Decimal;

use crate::{Program, Type, expr::{Expr, Stmt}, graph::GraphStmt};

pub fn validate(program: &Program) -> Result<(), String> {
    let mut names: HashSet<&str> = ["Bool", "String", "Number", "List"].into_iter().collect();
    let has_graph = program.processes.iter().any(|process| !process.graph.is_empty());
    for name in program.records.iter().map(|r| r.name.as_str())
        .chain(program.enums.iter().map(|e| e.name.as_str()))
        .chain(program.processes.iter().map(|p| p.name.as_str()))
        .chain(program.tasks.iter().map(|t| t.name.as_str()))
    {
        check_name(name)?;
        if matches!(name, "Vec" | "NAMESPACE" | "VERSION" | "rust_decimal")
            || (has_graph && name == "graph_definitions") {
            return Err(format!("reserved generated name: {name}"));
        }
        if !names.insert(name) {
            return Err(format!("duplicate declaration: {name}"));
        }
    }
    for record in &program.records {
        let mut fields = HashSet::new();
        for (name, ty) in &record.fields {
            check_name(name)?;
            if !fields.insert(name) {
                return Err(format!("duplicate field: {name}"));
            }
            resolve(ty, &names)?;
        }
    }
    let mut visited = HashSet::new();
    for index in 0..program.records.len() {
        if record_cycle(index, program, &mut HashSet::new(), &mut visited) {
            return Err(format!("recursive by-value record: {}", program.records[index].name));
        }
    }
    for item in &program.enums {
        let mut variants = HashSet::new();
        for variant in &item.variants {
            check_name(variant)?;
            if !variants.insert(variant) {
                return Err(format!("duplicate variant: {variant}"));
            }
        }
    }
    for process in program.tasks.iter().chain(&program.processes) {
        check_name(&process.input)?;
        resolve(&process.input_type, &names)?;
        resolve(&process.output, &names)?;
        if process.graph.is_empty() {
            check_block(&process.body, &process.input, &process.input_type, &process.output, program)?;
            if !returns(&process.body) {
                return Err(format!("missing return in process {}", process.name));
            }
        } else {
            let mut env = HashMap::from([(process.input.clone(), process.input_type.clone())]);
            check_graph(&process.graph, &mut env, &process.output, program, false)?;
        }
    }
    Ok(())
}

fn check_graph(
    body: &[GraphStmt], env: &mut HashMap<String, Type>, output: &Type, program: &Program,
    branch: bool,
) -> Result<Type, String> {
    let mut last = None;
    let mut returned = false;
    for stmt in body {
        if returned { return Err("unreachable graph node after return".into()); }
        match stmt {
            GraphStmt::Run { name, task, input } => {
                check_name(name)?;
                if env.contains_key(name) { return Err(format!("duplicate graph node: {name}")); }
                let definition = program.tasks.iter().find(|item| item.name == *task)
                    .ok_or_else(|| format!("unknown task: {task}"))?;
                let actual = infer(input, Some(&definition.input_type), env, program)?;
                if actual != definition.input_type {
                    return Err(format!("task input type mismatch for {task}: expected {}, got {actual}", definition.input_type));
                }
                last = Some(definition.output.clone());
                env.insert(name.clone(), definition.output.clone());
            }
            GraphStmt::Gateway { kind, branches, join, output: join_type } => {
                check_name(join)?;
                if env.contains_key(join) { return Err(format!("duplicate graph node: {join}")); }
                let mut results = Vec::new();
                let mut labels = HashSet::new();
                for arm in branches {
                    if let Some(condition) = &arm.condition {
                        let actual = infer(condition, None, env, program)?;
                        if actual != Type::Named("Bool".into()) {
                            return Err(format!("gateway condition must be Bool, got {actual}"));
                        }
                    }
                    if let Some(label) = &arm.label {
                        check_name(label)?;
                        if !labels.insert(label) { return Err(format!("duplicate branch: {label}")); }
                    }
                    let mut local = env.clone();
                    results.push(check_graph(&arm.body, &mut local, output, program, true)?);
                }
                let ty = if kind == "and" {
                    let Type::Named(name) = join_type.as_ref().ok_or("missing AND join record")? else {
                        return Err("AND join needs a record type".into());
                    };
                    let record = program.records.iter().find(|record| record.name == *name)
                        .ok_or_else(|| format!("unknown join record: {name}"))?;
                    if record.fields.len() != branches.len() || branches.iter().zip(&results).any(|(arm, actual)| {
                        record.fields.iter().find(|(field, _)| Some(field) == arm.label.as_ref())
                            .is_none_or(|(_, expected)| expected != actual)
                    }) {
                        return Err(format!("AND join {join} does not match record {name}"));
                    }
                    Type::Named(name.clone())
                } else {
                    let first = results.first().ok_or("empty gateway")?;
                    if results.iter().any(|ty| ty != first) {
                        return Err(format!("{kind} join {join} requires matching branch types"));
                    }
                    if kind == "or" { Type::Generic("List".into(), Box::new(first.clone())) }
                    else { first.clone() }
                };
                last = Some(ty.clone());
                env.insert(join.clone(), ty);
            }
            GraphStmt::Return(value) => {
                if branch { return Err("return inside gateway branch; use join".into()); }
                let actual = infer(value, Some(output), env, program)?;
                if &actual != output { return Err(format!("return type mismatch: expected {output}, got {actual}")); }
                returned = true;
            }
        }
    }
    if branch { last.ok_or_else(|| "empty gateway branch".into()) }
    else if returned { Ok(output.clone()) }
    else { Err("missing return in graph process".into()) }
}

fn record_cycle(
    index: usize, program: &Program, active: &mut HashSet<usize>, visited: &mut HashSet<usize>,
) -> bool {
    if visited.contains(&index) {
        return false;
    }
    if !active.insert(index) {
        return true;
    }
    for (_, ty) in &program.records[index].fields {
        if let Type::Named(name) = ty
            && let Some(next) = program.records.iter().position(|record| record.name == *name)
            && record_cycle(next, program, active, visited)
        {
            return true;
        }
    }
    active.remove(&index);
    visited.insert(index);
    false
}

fn check_name(name: &str) -> Result<(), String> {
    if matches!(name, "_" | "as" | "async" | "await" | "break" | "const" | "continue"
        | "crate" | "dyn" | "else" | "enum" | "extern" | "false" | "fn" | "for"
        | "gen" | "if" | "impl" | "in" | "let" | "loop" | "match" | "mod"
        | "move" | "mut" | "pub" | "ref" | "return" | "self" | "Self"
        | "static" | "struct" | "super" | "trait" | "true" | "type" | "unsafe"
        | "use" | "where" | "while" | "abstract" | "become" | "box" | "do"
        | "final" | "macro" | "override" | "priv" | "try" | "typeof" | "unsized"
        | "virtual" | "yield" | "union") {
        return Err(format!("reserved Rust identifier: {name}"));
    }
    Ok(())
}

fn returns(body: &[Stmt]) -> bool {
    body.iter().any(|stmt| match stmt {
        Stmt::Return(_) => true,
        Stmt::If(_, yes, no) => returns(yes) && returns(no),
    })
}

fn check_block(
    body: &[Stmt], input: &str, input_type: &Type, output: &Type, program: &Program,
) -> Result<(), String> {
    let env = HashMap::from([(input.to_owned(), input_type.clone())]);
    for stmt in body {
        match stmt {
            Stmt::Return(value) => {
                let actual = infer(value, Some(output), &env, program)?;
                if &actual != output {
                    return Err(format!("return type mismatch: expected {output}, got {actual}"));
                }
            }
            Stmt::If(condition, yes, no) => {
                let actual = infer(condition, None, &env, program)?;
                if actual != Type::Named("Bool".into()) {
                    return Err(format!("if condition must be Bool, got {actual}"));
                }
                check_block(yes, input, input_type, output, program)?;
                check_block(no, input, input_type, output, program)?;
            }
        }
    }
    Ok(())
}

fn infer(
    expr: &Expr, expected: Option<&Type>, env: &HashMap<String, Type>, program: &Program,
) -> Result<Type, String> {
    use Expr::*;
    let named = |name: &str| Type::Named(name.into());
    match expr {
        Number(value) => {
            Decimal::from_str(value).map_err(|_| format!("invalid Number: {value}"))?;
            Ok(named("Number"))
        }
        String(_) => Ok(named("String")),
        Bool(_) => Ok(named("Bool")),
        Name(name) => env.get(name).cloned().ok_or_else(|| format!("unknown name: {name}")),
        Field(base, field) => {
            if let Name(name) = base.as_ref()
                && let Some(item) = program.enums.iter().find(|item| item.name == *name)
            {
                return if item.variants.contains(field) {
                    Ok(named(name))
                } else {
                    Err(format!("unknown enum variant: {name}.{field}"))
                };
            }
            let ty = infer(base, None, env, program)?;
            if let Type::Named(name) = ty
                && let Some(record) = program.records.iter().find(|item| item.name == name)
            {
                return record.fields.iter()
                    .find(|(key, _)| key == field)
                    .map(|(_, value)| value.clone())
                    .ok_or_else(|| format!("unknown field: {name}.{field}"));
            }
            Err(format!("unknown field: {field}"))
        }
        List(elements) => {
            let element_type = if let Some(Type::Generic(name, inner)) = expected {
                if name == "List" { Some(inner.as_ref().clone()) } else { None }
            } else {
                None
            };
            let element_type = element_type.or_else(|| {
                elements.first().and_then(|element| infer(element, None, env, program).ok())
            }).ok_or("cannot infer empty list type")?;
            for element in elements {
                let actual = infer(element, Some(&element_type), env, program)?;
                if actual != element_type {
                    return Err(format!("List<{element_type}> element has type {actual}"));
                }
            }
            Ok(Type::Generic("List".into(), Box::new(element_type)))
        }
        Not(value) => {
            let ty = infer(value, None, env, program)?;
            if ty != named("Bool") {
                return Err(format!("not requires Bool, got {ty}"));
            }
            Ok(named("Bool"))
        }
        Binary(left, op, right) => {
            let lhs = if matches!(left.as_ref(), List(elements) if elements.is_empty()) {
                let other = infer(right, None, env, program)?;
                infer(left, Some(&other), env, program)?
            } else {
                infer(left, None, env, program)?
            };
            let rhs = infer(right, Some(&lhs), env, program)?;
            if lhs != rhs {
                return Err(format!("{op} requires matching types, got {lhs} and {rhs}"));
            }
            match op.as_str() {
                "and" | "or" if lhs == named("Bool") => Ok(named("Bool")),
                "==" | "!=" => Ok(named("Bool")),
                ">" | ">=" | "<" | "<=" if lhs == named("Number") || lhs == named("String") => Ok(named("Bool")),
                _ => Err(format!("{op} does not support {lhs}; expected Bool for boolean operations")),
            }
        }
    }
}

fn resolve(ty: &Type, names: &HashSet<&str>) -> Result<(), String> {
    match ty {
        Type::Named(name) if names.contains(name.as_str()) && name != "List" => Ok(()),
        Type::Generic(name, inner) if name == "List" => resolve(inner, names),
        Type::Named(name) => Err(format!("unknown type: {name}")),
        Type::Generic(_, _) => Err(format!("unsupported type: {ty}")),
    }
}
