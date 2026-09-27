use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use rust_decimal::Decimal;

use crate::{
    Program, Type,
    expr::{Expr, Stmt},
    graph::{GraphStmt, NamedGraph, NodeKind},
};

pub fn validate(program: &Program) -> Result<(), String> {
    let mut names: HashSet<&str> = ["Bool", "String", "Number", "List"].into_iter().collect();
    let has_graph = program
        .processes
        .iter()
        .any(|process| !process.graph.is_empty() || process.named_graph.is_some());
    for name in program
        .records
        .iter()
        .map(|r| r.name.as_str())
        .chain(program.enums.iter().map(|e| e.name.as_str()))
        .chain(program.processes.iter().map(|p| p.name.as_str()))
        .chain(program.tasks.iter().map(|t| t.name.as_str()))
    {
        check_name(name)?;
        if matches!(name, "Vec" | "NAMESPACE" | "VERSION" | "rust_decimal")
            || (has_graph && matches!(name, "graph_definitions" | "named_graph_definitions"))
        {
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
            return Err(format!(
                "recursive by-value record: {}",
                program.records[index].name
            ));
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
        if let Some(retry) = &process.retry {
            if retry.retry_for.is_zero() {
                return Err(format!("retry_for must be positive in {}", process.name));
            }
            if retry.retry_delay.is_zero() {
                return Err(format!("retry_delay must be positive in {}", process.name));
            }
        }
        check_name(&process.input)?;
        resolve(&process.input_type, &names)?;
        resolve(&process.output, &names)?;
        if let Some(graph) = &process.named_graph {
            check_named_graph(graph)?;
            named_scopes(graph, process, program)?;
        } else if process.graph.is_empty() {
            check_block(
                &process.body,
                &process.input,
                &process.input_type,
                &process.output,
                program,
            )?;
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

fn check_named_graph(graph: &NamedGraph) -> Result<(), String> {
    let mut nodes = HashMap::new();
    let mut start = None;
    for node in &graph.nodes {
        check_name(&node.name)?;
        if nodes.insert(node.name.as_str(), &node.kind).is_some() {
            return Err(format!("duplicate node: {}", node.name));
        }
        if matches!(node.kind, NodeKind::Start) {
            if start.replace(node.name.as_str()).is_some() {
                return Err("multiple start nodes".into());
            }
        }
    }
    let start = start.ok_or("missing start node")?;
    let mut links: HashMap<&str, Vec<&str>> = HashMap::new();
    for link in &graph.links {
        let source = nodes
            .get(link.source.as_str())
            .ok_or_else(|| format!("unknown node: {}", link.source))?;
        let target = nodes
            .get(link.target.as_str())
            .ok_or_else(|| format!("unknown node: {}", link.target))?;
        if matches!(
            source,
            NodeKind::End | NodeKind::Error | NodeKind::Cancel | NodeKind::Terminate
        ) {
            return Err(format!("link from terminal node: {}", link.source));
        }
        if matches!(target, NodeKind::Start) {
            return Err("link into start node".into());
        }
        links.entry(&link.source).or_default().push(&link.target);
    }
    fn visit<'a>(
        name: &'a str,
        links: &HashMap<&str, Vec<&'a str>>,
        active: &mut HashSet<&'a str>,
        seen: &mut HashSet<&'a str>,
    ) -> Result<(), String> {
        if active.contains(name) {
            return Err(format!("cycle at node: {name}"));
        }
        if seen.contains(name) {
            return Ok(());
        }
        active.insert(name);
        if let Some(targets) = links.get(name) {
            for target in targets {
                visit(target, links, active, seen)?;
            }
        }
        active.remove(name);
        seen.insert(name);
        Ok(())
    }
    let mut seen = HashSet::new();
    for name in nodes.keys() {
        visit(name, &links, &mut HashSet::new(), &mut seen)?;
    }
    for node in &graph.nodes {
        if !matches!(
            node.kind,
            NodeKind::End | NodeKind::Error | NodeKind::Cancel | NodeKind::Terminate
        ) && !links.contains_key(node.name.as_str())
        {
            return Err(format!("dead end at node: {}", node.name));
        }
    }
    let mut reachable = HashSet::new();
    visit(start, &links, &mut HashSet::new(), &mut reachable)?;
    if let Some(name) = nodes.keys().find(|name| !reachable.contains(**name)) {
        return Err(format!("unreachable node: {name}"));
    }
    for node in &graph.nodes {
        if matches!(
            node.kind,
            NodeKind::Start | NodeKind::Task { .. } | NodeKind::Join { .. }
        ) && links
            .get(node.name.as_str())
            .is_some_and(|outgoing| outgoing.len() != 1)
        {
            return Err(format!(
                "node {} needs a split for multiple routes",
                node.name
            ));
        }
        if let NodeKind::Join { kind, split, .. } = &node.kind {
            if !matches!(nodes.get(split.as_str()), Some(NodeKind::Split(actual)) if actual == kind)
            {
                return Err(format!(
                    "join {} requires a matching {kind} split: {split}",
                    node.name
                ));
            }
        }
        if let NodeKind::Split(kind) = &node.kind {
            let joins: Vec<_> = graph.nodes.iter().filter(|other| matches!(&other.kind, NodeKind::Join { kind: actual, split, .. } if actual == kind && split == &node.name)).collect();
            if joins.len() != 1 {
                return Err(format!(
                    "{kind} split {} requires exactly one matching join",
                    node.name
                ));
            }
            let join = &joins[0].name;
            // A join cannot be entered from outside its split region.
            let mut outside = vec![start];
            let mut visited = HashSet::new();
            while let Some(at) = outside.pop() {
                if at == join {
                    return Err(format!(
                        "join {join} has a route outside split {}",
                        node.name
                    ));
                }
                if at != node.name && visited.insert(at) {
                    outside.extend(links.get(at).into_iter().flatten().copied());
                }
            }
            // Branches must stay distinct until their join. Exceptional terminals
            // may bypass it because they stop the entire process.
            let mut branch_nodes = HashSet::new();
            for branch in graph.links.iter().filter(|link| link.source == node.name) {
                let mut pending = vec![branch.target.as_str()];
                let mut local = HashSet::new();
                while let Some(at) = pending.pop() {
                    if at == join {
                        continue;
                    }
                    if !local.insert(at) {
                        continue;
                    }
                    match nodes[at] {
                        NodeKind::End => {
                            return Err(format!(
                                "split {} bypasses join {join} to end {at}",
                                node.name
                            ));
                        }
                        NodeKind::Error | NodeKind::Cancel | NodeKind::Terminate => continue,
                        _ => {}
                    }
                    if !branch_nodes.insert(at) {
                        return Err(format!(
                            "split {} branches merge before join {join}",
                            node.name
                        ));
                    }
                    pending.extend(links.get(at).into_iter().flatten().copied());
                }
            }
            let outgoing: Vec<_> = graph
                .links
                .iter()
                .filter(|link| link.source == node.name)
                .collect();
            if kind == "and" {
                let mut labels = HashSet::new();
                for link in outgoing {
                    let label = link
                        .label
                        .as_ref()
                        .ok_or_else(|| format!("AND branch requires a label: {}", node.name))?;
                    if link.condition.is_some() || link.fallback || !labels.insert(label) {
                        return Err(format!("invalid or duplicate AND branch: {label}"));
                    }
                }
            } else if !outgoing.iter().any(|link| link.condition.is_some())
                || outgoing.last().is_none_or(|link| !link.fallback)
                || outgoing.iter().filter(|link| link.fallback).count() != 1
                || outgoing.iter().any(|link| {
                    link.label.is_some() || (!link.fallback && link.condition.is_none())
                })
            {
                return Err(format!(
                    "{kind} split {} requires conditions and a final fallback",
                    node.name
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn named_scopes<'a>(
    graph: &'a NamedGraph,
    process: &crate::compiler::Process,
    program: &Program,
) -> Result<HashMap<&'a str, HashMap<String, Type>>, String> {
    let mut scopes: HashMap<&str, HashMap<String, Type>> = HashMap::new();
    while scopes.len() < graph.nodes.len() {
        let mut progressed = false;
        for node in &graph.nodes {
            if scopes.contains_key(node.name.as_str()) {
                continue;
            }
            let incoming: Vec<_> = graph
                .links
                .iter()
                .filter(|link| link.target == node.name)
                .collect();
            if !incoming
                .iter()
                .all(|link| scopes.contains_key(link.source.as_str()))
            {
                continue;
            }
            let mut env = if let Some(first) = incoming.first() {
                scopes[first.source.as_str()].clone()
            } else {
                HashMap::from([(process.input.clone(), process.input_type.clone())])
            };
            env.retain(|name, ty| {
                incoming
                    .iter()
                    .all(|link| scopes[link.source.as_str()].get(name) == Some(ty))
            });
            match &node.kind {
                NodeKind::Start => {}
                NodeKind::Task { task, input } => {
                    let definition = program
                        .tasks
                        .iter()
                        .find(|item| item.name == *task)
                        .ok_or_else(|| format!("unknown task: {task}"))?;
                    let actual = infer(input, Some(&definition.input_type), &env, program)?;
                    if actual != definition.input_type {
                        return Err(format!(
                            "task input type mismatch for {task}: expected {}, got {actual}",
                            definition.input_type
                        ));
                    }
                    env.insert(node.name.clone(), definition.output.clone());
                }
                NodeKind::Join { kind, output, .. } => {
                    let results: Vec<Type> = incoming
                        .iter()
                        .map(|link| {
                            let value = link
                                .value
                                .as_ref()
                                .ok_or_else(|| format!("join {} requires a value", node.name))?;
                            infer(value, None, &scopes[link.source.as_str()], program)
                        })
                        .collect::<Result<_, _>>()?;
                    let first = results
                        .first()
                        .ok_or_else(|| format!("join {} has no inputs", node.name))?;
                    let ty = if kind == "and" {
                        let declared = output
                            .as_ref()
                            .ok_or_else(|| format!("AND join {} needs a record type", node.name))?;
                        let Type::Named(record_name) = declared else {
                            return Err("AND join requires a record type".into());
                        };
                        let record = program
                            .records
                            .iter()
                            .find(|record| record.name == *record_name)
                            .ok_or_else(|| format!("unknown AND join record: {record_name}"))?;
                        let NodeKind::Join { split, .. } = &node.kind else {
                            unreachable!()
                        };
                        let branches: Vec<_> = graph
                            .links
                            .iter()
                            .filter(|link| link.source == *split)
                            .collect();
                        let mut fields = HashSet::new();
                        if record.fields.len() != incoming.len() || branches.len() != incoming.len()
                        {
                            return Err(format!(
                                "AND join {} branch count does not match record {record_name}",
                                node.name
                            ));
                        }
                        for (link, actual) in incoming.iter().zip(&results) {
                            let matching: Vec<_> = branches
                                .iter()
                                .filter(|branch| {
                                    let mut pending = vec![branch.target.as_str()];
                                    let mut visited = HashSet::new();
                                    while let Some(at) = pending.pop() {
                                        if at == link.source {
                                            return true;
                                        }
                                        if visited.insert(at) && at != node.name {
                                            pending.extend(
                                                graph
                                                    .links
                                                    .iter()
                                                    .filter(|edge| edge.source == at)
                                                    .map(|edge| edge.target.as_str()),
                                            );
                                        }
                                    }
                                    false
                                })
                                .collect();
                            if matching.len() != 1 {
                                return Err(format!(
                                    "AND join {} has ambiguous branch route",
                                    node.name
                                ));
                            }
                            let label = matching[0]
                                .label
                                .as_ref()
                                .ok_or("missing AND branch label")?;
                            if !fields.insert(label)
                                || !record
                                    .fields
                                    .iter()
                                    .any(|(field, ty)| field == label && ty == actual)
                            {
                                return Err(format!(
                                    "AND join {} branch {label} does not match record {record_name}",
                                    node.name
                                ));
                            }
                        }
                        declared.clone()
                    } else {
                        if output.is_some() || results.iter().any(|ty| ty != first) {
                            return Err(format!(
                                "{kind} join {} requires matching branch types",
                                node.name
                            ));
                        }
                        if kind == "or" {
                            Type::Generic("List".into(), Box::new(first.clone()))
                        } else {
                            first.clone()
                        }
                    };
                    env.insert(node.name.clone(), ty);
                }
                NodeKind::End => {
                    for link in &incoming {
                        let value = link
                            .value
                            .as_ref()
                            .ok_or_else(|| format!("end {} requires an output", node.name))?;
                        let actual = infer(
                            value,
                            Some(&process.output),
                            &scopes[link.source.as_str()],
                            program,
                        )?;
                        if actual != process.output {
                            return Err(format!(
                                "end output type mismatch: expected {}, got {actual}",
                                process.output
                            ));
                        }
                    }
                }
                NodeKind::Split(_) | NodeKind::Error | NodeKind::Cancel | NodeKind::Terminate => {}
            }
            for link in graph.links.iter().filter(|link| link.source == node.name) {
                let target = &graph
                    .nodes
                    .iter()
                    .find(|target| target.name == link.target)
                    .unwrap()
                    .kind;
                if link.value.is_some() && !matches!(target, NodeKind::Join { .. } | NodeKind::End)
                {
                    if matches!(
                        target,
                        NodeKind::Error | NodeKind::Cancel | NodeKind::Terminate
                    ) {
                        return Err(format!(
                            "exceptional terminal {} cannot receive a payload",
                            link.target
                        ));
                    }
                    return Err(format!("link to {} cannot carry a value", link.target));
                }
                if link.label.is_some()
                    && !matches!(node.kind, NodeKind::Split(ref kind) if kind == "and")
                {
                    return Err(format!("branch label requires AND split: {}", node.name));
                }
                if link.fallback && !matches!(node.kind, NodeKind::Split(ref kind) if kind != "and")
                {
                    return Err(format!("fallback requires XOR or OR split: {}", node.name));
                }
                if let Some(condition) = &link.condition {
                    if !matches!(node.kind, NodeKind::Split(ref kind) if kind != "and") {
                        return Err(format!("condition requires XOR or OR split: {}", node.name));
                    }
                    let actual = infer(condition, None, &env, program)?;
                    if actual != Type::Named("Bool".into()) {
                        return Err(format!("gateway condition must be Bool, got {actual}"));
                    }
                }
                if let Some(value) = &link.value {
                    infer(value, None, &env, program)?;
                }
            }
            scopes.insert(node.name.as_str(), env);
            progressed = true;
        }
        if !progressed {
            return Err("graph cannot resolve node dependencies".into());
        }
    }
    Ok(scopes)
}

fn check_graph(
    body: &[GraphStmt],
    env: &mut HashMap<String, Type>,
    output: &Type,
    program: &Program,
    branch: bool,
) -> Result<Type, String> {
    let mut last = None;
    let mut returned = false;
    for stmt in body {
        if returned {
            return Err("unreachable graph node after return".into());
        }
        match stmt {
            GraphStmt::Run { name, task, input } => {
                check_name(name)?;
                if env.contains_key(name) {
                    return Err(format!("duplicate graph node: {name}"));
                }
                let definition = program
                    .tasks
                    .iter()
                    .find(|item| item.name == *task)
                    .ok_or_else(|| format!("unknown task: {task}"))?;
                let actual = infer(input, Some(&definition.input_type), env, program)?;
                if actual != definition.input_type {
                    return Err(format!(
                        "task input type mismatch for {task}: expected {}, got {actual}",
                        definition.input_type
                    ));
                }
                last = Some(definition.output.clone());
                env.insert(name.clone(), definition.output.clone());
            }
            GraphStmt::Gateway {
                kind,
                branches,
                join,
                output: join_type,
            } => {
                check_name(join)?;
                if env.contains_key(join) {
                    return Err(format!("duplicate graph node: {join}"));
                }
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
                        if !labels.insert(label) {
                            return Err(format!("duplicate branch: {label}"));
                        }
                    }
                    let mut local = env.clone();
                    results.push(check_graph(&arm.body, &mut local, output, program, true)?);
                }
                let ty = if kind == "and" {
                    let Type::Named(name) = join_type.as_ref().ok_or("missing AND join record")?
                    else {
                        return Err("AND join needs a record type".into());
                    };
                    let record = program
                        .records
                        .iter()
                        .find(|record| record.name == *name)
                        .ok_or_else(|| format!("unknown join record: {name}"))?;
                    if record.fields.len() != branches.len()
                        || branches.iter().zip(&results).any(|(arm, actual)| {
                            record
                                .fields
                                .iter()
                                .find(|(field, _)| Some(field) == arm.label.as_ref())
                                .is_none_or(|(_, expected)| expected != actual)
                        })
                    {
                        return Err(format!("AND join {join} does not match record {name}"));
                    }
                    Type::Named(name.clone())
                } else {
                    let first = results.first().ok_or("empty gateway")?;
                    if results.iter().any(|ty| ty != first) {
                        return Err(format!("{kind} join {join} requires matching branch types"));
                    }
                    if kind == "or" {
                        Type::Generic("List".into(), Box::new(first.clone()))
                    } else {
                        first.clone()
                    }
                };
                last = Some(ty.clone());
                env.insert(join.clone(), ty);
            }
            GraphStmt::Return(value) => {
                if branch {
                    return Err("return inside gateway branch; use join".into());
                }
                let actual = infer(value, Some(output), env, program)?;
                if &actual != output {
                    return Err(format!(
                        "return type mismatch: expected {output}, got {actual}"
                    ));
                }
                returned = true;
            }
        }
    }
    if branch {
        last.ok_or_else(|| "empty gateway branch".into())
    } else if returned {
        Ok(output.clone())
    } else {
        Err("missing return in graph process".into())
    }
}

fn record_cycle(
    index: usize,
    program: &Program,
    active: &mut HashSet<usize>,
    visited: &mut HashSet<usize>,
) -> bool {
    if visited.contains(&index) {
        return false;
    }
    if !active.insert(index) {
        return true;
    }
    for (_, ty) in &program.records[index].fields {
        if let Type::Named(name) = ty
            && let Some(next) = program
                .records
                .iter()
                .position(|record| record.name == *name)
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
    if matches!(
        name,
        "_" | "as"
            | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "gen"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "try"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "union"
    ) {
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
    body: &[Stmt],
    input: &str,
    input_type: &Type,
    output: &Type,
    program: &Program,
) -> Result<(), String> {
    let env = HashMap::from([(input.to_owned(), input_type.clone())]);
    for stmt in body {
        match stmt {
            Stmt::Return(value) => {
                let actual = infer(value, Some(output), &env, program)?;
                if &actual != output {
                    return Err(format!(
                        "return type mismatch: expected {output}, got {actual}"
                    ));
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
    expr: &Expr,
    expected: Option<&Type>,
    env: &HashMap<String, Type>,
    program: &Program,
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
        Name(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown name: {name}")),
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
                return record
                    .fields
                    .iter()
                    .find(|(key, _)| key == field)
                    .map(|(_, value)| value.clone())
                    .ok_or_else(|| format!("unknown field: {name}.{field}"));
            }
            Err(format!("unknown field: {field}"))
        }
        List(elements) => {
            let element_type = if let Some(Type::Generic(name, inner)) = expected {
                if name == "List" {
                    Some(inner.as_ref().clone())
                } else {
                    None
                }
            } else {
                None
            };
            let element_type = element_type
                .or_else(|| {
                    elements
                        .first()
                        .and_then(|element| infer(element, None, env, program).ok())
                })
                .ok_or("cannot infer empty list type")?;
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
                ">" | ">=" | "<" | "<=" if lhs == named("Number") || lhs == named("String") => {
                    Ok(named("Bool"))
                }
                _ => Err(format!(
                    "{op} does not support {lhs}; expected Bool for boolean operations"
                )),
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
