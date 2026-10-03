use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, NaiveTime, Timelike};
use rust_decimal::Decimal;

use crate::{
    Program, Type,
    decision::{DecisionKind, DecisionModel, DecisionTable, Knowledge},
    expr::{Expr, Stmt},
    graph::{GraphStmt, NamedGraph, NodeKind},
};

pub fn validate(program: &Program) -> Result<(), String> {
    let mut names: HashSet<&str> = [
        "Bool", "String", "Number", "Date", "DateTime", "Time", "List",
    ]
    .into_iter()
    .collect();
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
        .chain(program.decisions.iter().map(|d| d.name.as_str()))
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
    for (task, definition) in &program.external_tasks {
        resolve(&definition.input, &names).map_err(|e| format!("{task}: {e}"))?;
        resolve(&definition.output, &names).map_err(|e| format!("{task}: {e}"))?;
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
    for model in &program.decisions {
        check_decision(model, program, &names)?;
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
            check_named_graph(graph, process.deadline.is_some())?;
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
    fn visit_process<'a>(
        name: &'a str,
        program: &'a Program,
        active: &mut HashSet<&'a str>,
        done: &mut HashSet<&'a str>,
    ) -> Result<(), String> {
        if done.contains(name) {
            return Ok(());
        }
        if !active.insert(name) {
            return Err(format!("recursive subprocess call: {name}"));
        }
        let process = program
            .processes
            .iter()
            .find(|item| item.name == name)
            .unwrap();
        if let Some(graph) = &process.named_graph {
            for node in &graph.nodes {
                if let NodeKind::Subprocess { process: child, .. } = &node.kind {
                    let child = program
                        .processes
                        .iter()
                        .find(|item| item.name == *child)
                        .unwrap();
                    visit_process(&child.name, program, active, done)?;
                }
            }
        }
        active.remove(name);
        done.insert(name);
        Ok(())
    }
    let mut done = HashSet::new();
    for process in &program.processes {
        visit_process(&process.name, program, &mut HashSet::new(), &mut done)?;
    }
    Ok(())
}

fn check_decision(
    model: &DecisionModel,
    program: &Program,
    types: &HashSet<&str>,
) -> Result<(), String> {
    check_name(&model.input)?;
    resolve(&model.input_type, types)?;
    resolve(&model.output, types)?;
    let mut names = HashSet::from([model.input.as_str()]);
    for knowledge in &model.knowledge {
        check_name(&knowledge.name)?;
        if !names.insert(&knowledge.name) {
            return Err(format!("duplicate knowledge model: {}", knowledge.name));
        }
        resolve(&knowledge.output, types)?;
        let mut env = HashMap::new();
        for (param, ty) in &knowledge.params {
            check_name(param)?;
            resolve(ty, types)?;
            if env.insert(param.clone(), ty.clone()).is_some() {
                return Err(format!("duplicate knowledge parameter: {param}"));
            }
        }
        let result = infer_with(
            &knowledge.body,
            Some(&knowledge.output),
            &env,
            program,
            &model.knowledge,
        )?;
        if result != knowledge.output {
            return Err(format!(
                "knowledge result type mismatch: {}",
                knowledge.name
            ));
        }
    }
    for node in &model.nodes {
        check_name(&node.name)?;
        resolve(&node.output, types)?;
        if !names.insert(&node.name) {
            return Err(format!("duplicate decision node: {}", node.name));
        }
    }
    for (source, target) in &model.links {
        if !model.nodes.iter().any(|n| n.name == *source)
            || !model.nodes.iter().any(|n| n.name == *target)
        {
            return Err(format!(
                "unknown decision node in link: {source} -> {target}"
            ));
        }
    }
    let knowledge_names: HashSet<_> = model
        .knowledge
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    fn calls<'a>(expr: &'a Expr, found: &mut Vec<&'a str>) {
        match expr {
            Expr::Call(name, args) => {
                found.push(name);
                for arg in args {
                    calls(arg, found);
                }
            }
            Expr::Field(base, _) | Expr::Not(base) => calls(base, found),
            Expr::Binary(left, _, right) => {
                calls(left, found);
                calls(right, found);
            }
            Expr::List(items) => {
                for item in items {
                    calls(item, found);
                }
            }
            Expr::Range(lower, upper, _, _) => {
                for bound in lower.iter().chain(upper.iter()) {
                    calls(bound, found);
                }
            }
            _ => {}
        }
    }
    for item in &model.knowledge {
        let mut found = vec![];
        calls(&item.body, &mut found);
        for name in found {
            if !knowledge_names.contains(name)
                && !range_relation(name)
                && !string_builtin(name)
                && !matches!(name, "date" | "time" | "dateTime")
            {
                return Err(format!("unknown knowledge model: {name}"));
            }
        }
    }
    fn visit_knowledge<'a>(
        name: &'a str,
        model: &'a DecisionModel,
        active: &mut HashSet<&'a str>,
        done: &mut HashSet<&'a str>,
    ) -> Result<(), String> {
        if done.contains(name) {
            return Ok(());
        }
        if !active.insert(name) {
            return Err(format!("knowledge cycle at {name}"));
        }
        let item = model
            .knowledge
            .iter()
            .find(|item| item.name == name)
            .ok_or("unknown knowledge model")?;
        let mut deps = vec![];
        calls(&item.body, &mut deps);
        for dep in deps {
            if model.knowledge.iter().any(|item| item.name == dep) {
                visit_knowledge(dep, model, active, done)?;
            }
        }
        active.remove(name);
        done.insert(name);
        Ok(())
    }
    let mut knowledge_done = HashSet::new();
    for item in &model.knowledge {
        visit_knowledge(&item.name, model, &mut HashSet::new(), &mut knowledge_done)?;
    }
    fn visit<'a>(
        name: &'a str,
        model: &'a DecisionModel,
        program: &Program,
        types: &HashSet<&str>,
        knowledge_names: &HashSet<&str>,
        active: &mut HashSet<&'a str>,
        done: &mut HashMap<String, Type>,
    ) -> Result<(), String> {
        if done.contains_key(name) {
            return Ok(());
        }
        if !active.insert(name) {
            return Err(format!("decision cycle at {name}"));
        }
        let node = model
            .nodes
            .iter()
            .find(|n| n.name == name)
            .ok_or_else(|| format!("unknown decision node: {name}"))?;
        let mut env = HashMap::from([(model.input.clone(), model.input_type.clone())]);
        for (source, target) in &model.links {
            if target == name {
                visit(source, model, program, types, knowledge_names, active, done)?;
                env.insert(source.clone(), done[source].clone());
            }
        }
        let check = |expression: &Expr,
                     expected: &Type,
                     env: &HashMap<String, Type>|
         -> Result<(), String> {
            let mut called = vec![];
            calls(expression, &mut called);
            for name in called {
                if !knowledge_names.contains(name)
                    && !range_relation(name)
                    && !string_builtin(name)
                    && !matches!(name, "date" | "time" | "dateTime")
                {
                    return Err(format!("unknown knowledge model: {name}"));
                }
            }
            let actual = infer_with(expression, Some(expected), env, program, &model.knowledge)?;
            if actual != *expected {
                return Err(format!(
                    "decision result type mismatch for {}: expected {expected}, got {actual}",
                    node.name
                ));
            }
            Ok(())
        };
        match &node.kind {
            DecisionKind::Literal(expr) => check(expr, &node.output, &env)?,
            DecisionKind::Context { entries, result } => {
                for (key, ty, expr) in entries {
                    check_name(key)?;
                    resolve(ty, types)?;
                    if env.contains_key(key) {
                        return Err(format!("duplicate context entry: {key}"));
                    }
                    check(expr, ty, &env)?;
                    env.insert(key.clone(), ty.clone());
                }
                check(result, &node.output, &env)?;
            }
            DecisionKind::Table(table) => {
                check_table(table, &node.output, &mut env, model, program, types)?
            }
        }
        active.remove(name);
        done.insert(name.into(), node.output.clone());
        Ok(())
    }
    let mut done = HashMap::new();
    for node in &model.nodes {
        visit(
            &node.name,
            model,
            program,
            types,
            &knowledge_names,
            &mut HashSet::new(),
            &mut done,
        )?;
    }
    let actual = done
        .get(&model.output_node)
        .ok_or_else(|| format!("unknown decision node: {}", model.output_node))?;
    if actual != &model.output {
        return Err(format!("decision result type mismatch for {}", model.name));
    }
    Ok(())
}

fn check_table(
    table: &DecisionTable,
    result: &Type,
    env: &mut HashMap<String, Type>,
    model: &DecisionModel,
    program: &Program,
    types: &HashSet<&str>,
) -> Result<(), String> {
    if table.inputs.is_empty() || table.outputs.is_empty() || table.rules.is_empty() {
        return Err("table requires input, output, and rules".into());
    }
    let collected = matches!(
        table.policy.as_str(),
        "RULE_ORDER" | "OUTPUT_ORDER" | "COLLECT"
    ) && table.aggregation.is_none();
    if let Some(aggregation) = table.aggregation.as_deref() {
        if table.policy != "COLLECT" || !matches!(aggregation, "SUM" | "MIN" | "MAX" | "COUNT") {
            return Err("invalid table aggregation policy".into());
        }
        if aggregation != "COUNT"
            && (table.outputs.len() != 1 || table.outputs[0].1 != Type::Named("Number".into()))
        {
            return Err("table aggregation requires one Number output".into());
        }
    }
    if matches!(table.policy.as_str(), "PRIORITY" | "OUTPUT_ORDER") {
        if table.priorities.is_empty() {
            return Err("table priority order required".into());
        }
    } else if !table.priorities.is_empty() {
        return Err("priority requires PRIORITY or OUTPUT_ORDER policy".into());
    }
    let item = if table.aggregation.as_deref() == Some("COUNT") {
        Type::Named("Number".into())
    } else if table.outputs.len() == 1 {
        table.outputs[0].1.clone()
    } else {
        let Type::Named(name) = (if collected {
            match result {
                Type::Generic(kind, inner) if kind == "List" => inner.as_ref(),
                _ => return Err("table outputs require a List<record> result".into()),
            }
        } else {
            result
        }) else {
            return Err("table outputs require a record result".into());
        };
        let record = program
            .records
            .iter()
            .find(|r| r.name == *name)
            .ok_or("unknown table output record")?;
        if record.fields != table.outputs {
            return Err("table output columns do not match result record".into());
        }
        Type::Named(name.clone())
    };
    let expected = if table.aggregation.is_some() {
        Type::Named("Number".into())
    } else if collected {
        Type::Generic("List".into(), Box::new(item))
    } else {
        item
    };
    if &expected != result {
        return Err(format!(
            "table result type mismatch: expected {expected}, got {result}"
        ));
    }
    for (name, ty, expression) in &table.inputs {
        check_name(name)?;
        resolve(ty, types)?;
        if env.contains_key(name) {
            return Err(format!("duplicate table input: {name}"));
        }
        let actual = infer_with(expression, Some(ty), env, program, &model.knowledge)?;
        if &actual != ty {
            return Err(format!("table input type mismatch: {name}"));
        }
        env.insert(name.clone(), ty.clone());
    }
    let mut outputs = HashSet::new();
    for (name, ty) in &table.outputs {
        check_name(name)?;
        resolve(ty, types)?;
        if !outputs.insert(name) {
            return Err(format!("duplicate table output: {name}"));
        }
    }
    let row = |values: &[Expr], location: &str| -> Result<(), String> {
        if values.len() != table.outputs.len() {
            return Err(format!(
                "{location} output count does not match table columns"
            ));
        }
        for (expression, (_, ty)) in values.iter().zip(&table.outputs) {
            let actual = infer_with(expression, Some(ty), env, program, &model.knowledge)?;
            if &actual != ty {
                return Err(format!(
                    "{location} output type mismatch: expected {ty}, got {actual}"
                ));
            }
        }
        Ok(())
    };
    for (condition, values) in &table.rules {
        if infer_with(condition, None, env, program, &model.knowledge)?
            != Type::Named("Bool".into())
        {
            return Err("table rule condition must be Bool".into());
        }
        row(values, "rule")?;
    }
    for (index, values) in table.priorities.iter().enumerate() {
        row(values, "priority")?;
        if table.priorities[..index].contains(values) {
            return Err("duplicate priority value".into());
        }
    }
    if let Some(values) = &table.default {
        if table.aggregation.as_deref() == Some("COUNT") {
            if values.len() != 1
                || infer_with(&values[0], Some(result), env, program, &model.knowledge)? != *result
            {
                return Err("COUNT default must be Number".into());
            }
        } else {
            row(values, "default")?;
        }
    }
    Ok(())
}

fn check_named_graph(graph: &NamedGraph, has_deadline: bool) -> Result<(), String> {
    let mut nodes = HashMap::new();
    let mut start = None;
    for node in &graph.nodes {
        if node.name == "timeout" || node.name == "task_iteration_limit" {
            return Err(format!("reserved process node name: {}", node.name));
        }
        check_name(&node.name)?;
        if nodes.insert(node.name.as_str(), &node.kind).is_some() {
            return Err(format!("duplicate node: {}", node.name));
        }
        if matches!(node.kind, NodeKind::Start) && start.replace(node.name.as_str()).is_some() {
            return Err("multiple start nodes".into());
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
        if link.outcome.is_some() {
            if !matches!(source, NodeKind::Subprocess { .. }) {
                return Err(format!("outcome link requires subprocess: {}", link.source));
            }
            if link.value.is_some()
                || link.condition.is_some()
                || link.fallback
                || link.label.is_some()
            {
                return Err(format!(
                    "outcome link cannot carry a payload or gateway annotation: {}",
                    link.source
                ));
            }
        }
        links.entry(&link.source).or_default().push(&link.target);
    }
    fn visit<'a>(
        name: &'a str,
        links: &HashMap<&str, Vec<&'a str>>,
        active: &mut HashSet<&'a str>,
        seen: &mut HashSet<&'a str>,
        has_deadline: bool,
    ) -> Result<(), String> {
        if active.contains(name) {
            return if has_deadline {
                Ok(())
            } else {
                Err(format!("process cycle requires deadline at node: {name}"))
            };
        }
        if seen.contains(name) {
            return Ok(());
        }
        active.insert(name);
        if let Some(targets) = links.get(name) {
            for target in targets {
                visit(target, links, active, seen, has_deadline)?;
            }
        }
        active.remove(name);
        seen.insert(name);
        Ok(())
    }
    let mut seen = HashSet::new();
    for name in nodes.keys() {
        visit(name, &links, &mut HashSet::new(), &mut seen, has_deadline)?;
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
    visit(
        start,
        &links,
        &mut HashSet::new(),
        &mut reachable,
        has_deadline,
    )?;
    if let Some(name) = nodes.keys().find(|name| !reachable.contains(**name)) {
        return Err(format!("unreachable node: {name}"));
    }
    let mut can_exit: HashSet<&str> = nodes
        .iter()
        .filter_map(|(name, kind)| {
            matches!(
                kind,
                NodeKind::End | NodeKind::Error | NodeKind::Cancel | NodeKind::Terminate
            )
            .then_some(*name)
        })
        .collect();
    let mut pending: Vec<_> = can_exit.iter().copied().collect();
    while let Some(target) = pending.pop() {
        for link in graph.links.iter().filter(|link| link.target == target) {
            if can_exit.insert(&link.source) {
                pending.push(&link.source);
            }
        }
    }
    if let Some(name) = nodes.keys().find(|name| !can_exit.contains(**name)) {
        return Err(format!("node {name} has no reachable exit"));
    }
    for node in &graph.nodes {
        if matches!(node.kind, NodeKind::Subprocess { .. }) {
            let mut outcomes = HashSet::new();
            let mut success = 0;
            for link in graph.links.iter().filter(|link| link.source == node.name) {
                if let Some(outcome) = &link.outcome {
                    if !outcomes.insert(outcome) {
                        return Err(format!("duplicate subprocess outcome link: {outcome}"));
                    }
                } else {
                    success += 1;
                }
            }
            if success != 1 {
                return Err(format!(
                    "subprocess {} requires exactly one success link",
                    node.name
                ));
            }
        }
        if matches!(
            node.kind,
            NodeKind::Start
                | NodeKind::Task { .. }
                | NodeKind::TaskLoop { .. }
                | NodeKind::MultiInstance { .. }
                | NodeKind::BusinessRule { .. }
                | NodeKind::PauseFor(_)
                | NodeKind::PauseUntil(_)
                | NodeKind::Join { .. }
        ) && links
            .get(node.name.as_str())
            .is_some_and(|outgoing| outgoing.len() != 1)
        {
            return Err(format!(
                "node {} needs a split for multiple routes",
                node.name
            ));
        }
        if let NodeKind::Join { kind, split, .. } = &node.kind
            && !matches!(nodes.get(split.as_str()), Some(NodeKind::Split(actual)) if actual == kind)
        {
            return Err(format!(
                "join {} requires a matching {kind} split: {split}",
                node.name
            ));
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

fn route_scope(
    scopes: &HashMap<&str, HashMap<String, Type>>,
    link: &crate::graph::Link,
) -> HashMap<String, Type> {
    let mut env = scopes[link.source.as_str()].clone();
    if link.outcome.is_some() {
        env.remove(&link.source);
    }
    env
}

fn task_types<'a>(program: &'a Program, task: &str) -> Result<(&'a Type, &'a Type), String> {
    if let Some(source) = program.tasks.iter().find(|item| item.name == task) {
        return Ok((&source.input_type, &source.output));
    }
    program
        .external_tasks
        .get(task)
        .map(|ext| (&ext.input, &ext.output))
        .ok_or_else(|| format!("unknown task: {task}"))
}

pub(crate) fn named_scopes<'a>(
    graph: &'a NamedGraph,
    process: &crate::compiler::Process,
    program: &Program,
) -> Result<HashMap<&'a str, HashMap<String, Type>>, String> {
    let input = HashMap::from([(process.input.clone(), process.input_type.clone())]);
    let mut universe = input.clone();
    for node in &graph.nodes {
        match &node.kind {
            NodeKind::Subprocess { process, .. } => {
                let child = program
                    .processes
                    .iter()
                    .find(|item| item.name == *process)
                    .ok_or_else(|| format!("unknown process: {process}"))?;
                universe.insert(node.name.clone(), child.output.clone());
            }
            NodeKind::Task { task, .. }
            | NodeKind::TaskLoop { task, .. }
            | NodeKind::MultiInstance { task, .. } => {
                let (_, output) = task_types(program, task)?;
                let output = if matches!(node.kind, NodeKind::MultiInstance { .. }) {
                    Type::Generic("List".into(), Box::new(output.clone()))
                } else {
                    output.clone()
                };
                universe.insert(node.name.clone(), output);
            }
            NodeKind::BusinessRule { model, .. } => {
                let definition = program
                    .decisions
                    .iter()
                    .find(|item| item.name == *model)
                    .ok_or_else(|| format!("unknown decision model: {model}"))?;
                universe.insert(node.name.clone(), definition.output.clone());
            }
            NodeKind::Join {
                kind,
                output: Some(output),
                ..
            } if kind == "and" => {
                universe.insert(node.name.clone(), output.clone());
            }
            _ => {}
        }
    }
    let mut unresolved: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| matches!(&node.kind, NodeKind::Join { kind, .. } if kind != "and"))
        .collect();
    while !unresolved.is_empty() {
        let before = unresolved.len();
        unresolved.retain(|node| {
            let Some(link) = graph.links.iter().find(|link| link.target == node.name) else {
                return true;
            };
            let Some(value) = &link.value else {
                return true;
            };
            let Ok(ty) = infer(value, None, &universe, program) else {
                return true;
            };
            let ty = if matches!(&node.kind, NodeKind::Join { kind, .. } if kind == "or") {
                Type::Generic("List".into(), Box::new(ty))
            } else {
                ty
            };
            universe.insert(node.name.clone(), ty);
            false
        });
        if unresolved.len() == before {
            break;
        }
    }
    let mut scopes: HashMap<&str, HashMap<String, Type>> = graph
        .nodes
        .iter()
        .map(|node| {
            (
                node.name.as_str(),
                if matches!(node.kind, NodeKind::Start) {
                    input.clone()
                } else {
                    universe.clone()
                },
            )
        })
        .collect();
    loop {
        let mut changed = false;
        for node in &graph.nodes {
            if matches!(node.kind, NodeKind::Start) {
                continue;
            }
            let incoming: Vec<_> = graph
                .links
                .iter()
                .filter(|link| link.target == node.name)
                .collect();
            let mut env = incoming
                .first()
                .map_or_else(|| input.clone(), |link| route_scope(&scopes, link));
            env.retain(|name, ty| {
                incoming
                    .iter()
                    .all(|link| route_scope(&scopes, link).get(name) == Some(ty))
            });
            if let Some(ty) = universe.get(&node.name) {
                env.insert(node.name.clone(), ty.clone());
            }
            if scopes[node.name.as_str()] != env {
                scopes.insert(node.name.as_str(), env);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for node in &graph.nodes {
        let incoming: Vec<_> = graph
            .links
            .iter()
            .filter(|link| link.target == node.name)
            .collect();
        let mut env = scopes[node.name.as_str()].clone();
        match &node.kind {
            NodeKind::Start => {}
            NodeKind::Subprocess { process, input } => {
                let child = program
                    .processes
                    .iter()
                    .find(|item| item.name == *process)
                    .ok_or_else(|| format!("unknown process: {process}"))?;
                if child.named_graph.is_none() {
                    return Err(format!(
                        "subprocess requires a named process graph: {process}"
                    ));
                }
                let mut before = env.clone();
                before.remove(&node.name);
                let actual = infer(input, Some(&child.input_type), &before, program)?;
                if actual != child.input_type {
                    return Err(format!(
                        "subprocess input type mismatch for {process}: expected {}, got {actual}",
                        child.input_type
                    ));
                }
                env.insert(node.name.clone(), child.output.clone());
            }
            NodeKind::PauseFor(_) => {}
            NodeKind::PauseUntil(value) => {
                let actual = infer(value, Some(&Type::Named("DateTime".into())), &env, program)?;
                if actual != Type::Named("DateTime".into()) {
                    return Err(format!("pause_until requires DateTime, got {actual}"));
                }
            }
            NodeKind::BusinessRule { model, input } => {
                let definition = program
                    .decisions
                    .iter()
                    .find(|item| item.name == *model)
                    .ok_or_else(|| format!("unknown decision model: {model}"))?;
                let mut before = env.clone();
                before.remove(&node.name);
                let actual = infer(input, Some(&definition.input_type), &before, program)?;
                if actual != definition.input_type {
                    return Err(format!(
                        "business rule input type mismatch for {model}: expected {}, got {actual}",
                        definition.input_type
                    ));
                }
                env.insert(node.name.clone(), definition.output.clone());
            }
            NodeKind::Task { task, input } => {
                let (expected, output) = task_types(program, task)?;
                let mut before = env.clone();
                before.remove(&node.name);
                let actual = infer(input, Some(expected), &before, program)?;
                if actual != *expected {
                    return Err(format!(
                        "task input type mismatch for {task}: expected {expected}, got {actual}"
                    ));
                }
                env.insert(node.name.clone(), output.clone());
            }
            NodeKind::MultiInstance { task, items, .. } => {
                let (input_type, output) = task_types(program, task)?;
                let expected = Type::Generic("List".into(), Box::new(input_type.clone()));
                let mut before = env.clone();
                before.remove(&node.name);
                let actual = infer(items, Some(&expected), &before, program)?;
                if actual != expected {
                    return Err(format!(
                        "multi-instance input type mismatch: expected {expected}, got {actual}"
                    ));
                }
                env.insert(
                    node.name.clone(),
                    Type::Generic("List".into(), Box::new(output.clone())),
                );
            }
            NodeKind::TaskLoop {
                task,
                input,
                condition,
                before,
                initial,
                ..
            } => {
                let (input_type, output) = task_types(program, task)?;
                let mut initial_env = env.clone();
                initial_env.remove(&node.name);
                if let Some(initial) = initial {
                    let actual = infer(initial, Some(output), &initial_env, program)?;
                    if actual != *output {
                        return Err(format!(
                            "task loop initial result type mismatch: expected {output}, got {actual}"
                        ));
                    }
                }
                let argument_env = if *before { &env } else { &initial_env };
                let actual = infer(input, Some(input_type), argument_env, program)?;
                if actual != *input_type {
                    return Err(format!(
                        "task loop input type mismatch for {task}: expected {input_type}, got {actual}"
                    ));
                }
                let actual = infer(condition, Some(&Type::Named("Bool".into())), &env, program)?;
                if actual != Type::Named("Bool".into()) {
                    return Err(format!("task loop condition must be Bool, got {actual}"));
                }
                env.insert(node.name.clone(), output.clone());
            }
            NodeKind::Join { kind, output, .. } => {
                let results: Vec<Type> = incoming
                    .iter()
                    .map(|link| {
                        let value = link
                            .value
                            .as_ref()
                            .ok_or_else(|| format!("join {} requires a value", node.name))?;
                        infer(value, None, &route_scope(&scopes, link), program)
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
                    if record.fields.len() != incoming.len() || branches.len() != incoming.len() {
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
                        &route_scope(&scopes, link),
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
            if link.value.is_some() && !matches!(target, NodeKind::Join { .. } | NodeKind::End) {
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
            if link.fallback && !matches!(node.kind, NodeKind::Split(ref kind) if kind != "and") {
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
                infer(value, None, &route_scope(&scopes, link), program)?;
            }
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
    infer_with(expr, expected, env, program, &[])
}

fn infer_with(
    expr: &Expr,
    expected: Option<&Type>,
    env: &HashMap<String, Type>,
    program: &Program,
    knowledge: &[Knowledge],
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
        Call(name, args) => {
            if let Some(definition) = knowledge.iter().find(|item| item.name == *name) {
                if args.len() != definition.params.len() {
                    return Err(format!("knowledge argument count for {name}"));
                }
                for (arg, (_, ty)) in args.iter().zip(&definition.params) {
                    let actual = infer_with(arg, Some(ty), env, program, knowledge)?;
                    if actual != *ty {
                        return Err(format!(
                            "knowledge argument type for {name}: expected {ty}, got {actual}"
                        ));
                    }
                }
                return Ok(definition.output.clone());
            }
            if range_relation(name) {
                let [first, second] = args.as_slice() else {
                    return Err(format!("{name} requires two arguments"));
                };
                let (value, range) =
                    if matches!(name.as_str(), "includes" | "startedBy" | "finishedBy") {
                        (second, first)
                    } else {
                        (first, second)
                    };
                if matches!(
                    name.as_str(),
                    "includes" | "during" | "starts" | "startedBy" | "finishes" | "finishedBy"
                ) {
                    let ty = infer_with(value, None, env, program, knowledge)?;
                    let expected = Type::Generic("Range".into(), Box::new(ty));
                    let actual = infer_with(range, Some(&expected), env, program, knowledge)?;
                    if actual != expected {
                        return Err(format!("{name} requires matching scalar and range types"));
                    }
                } else {
                    let ty = if matches!(first, Range(None, None, _, _)) {
                        infer_with(second, None, env, program, knowledge)?
                    } else {
                        infer_with(first, None, env, program, knowledge)?
                    };
                    let first_ty = infer_with(first, Some(&ty), env, program, knowledge)?;
                    let second_ty = infer_with(second, Some(&ty), env, program, knowledge)?;
                    if first_ty != second_ty
                        || !matches!(ty, Type::Generic(ref name, _) if name == "Range")
                    {
                        return Err(format!("{name} requires matching range types"));
                    }
                }
                return Ok(named("Bool"));
            }
            if matches!(name.as_str(), "date" | "time" | "dateTime") {
                let [Expr::String(value)] = args.as_slice() else {
                    return Err(format!("{name} requires a string literal"));
                };
                let valid = match name.as_str() {
                    "date" => NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok(),
                    "time" => {
                        valid_time_format(value)
                            && NaiveTime::parse_from_str(value, "%H:%M:%S%.f")
                                .is_ok_and(|time| time.nanosecond() < 1_000_000_000)
                    }
                    _ => DateTime::parse_from_rfc3339(value).is_ok(),
                };
                if !valid {
                    return Err(format!("invalid {name} literal: {value}"));
                }
                return Ok(named(match name.as_str() {
                    "date" => "Date",
                    "time" => "Time",
                    _ => "DateTime",
                }));
            }
            let text = named("String");
            let number = named("Number");
            let boolean = named("Bool");
            let texts = Type::Generic("List".into(), Box::new(text.clone()));
            if name == "string" {
                let [value] = args.as_slice() else {
                    return Err("string requires one argument".into());
                };
                let ty = infer_with(value, None, env, program, knowledge)?;
                if !matches!(&ty, Type::Named(n) if matches!(n.as_str(), "String" | "Number" | "Bool" | "Date" | "Time" | "DateTime"))
                {
                    return Err(format!("string cannot convert {ty}"));
                }
                return Ok(text);
            }
            if name == "split" {
                let [value, delimiters] = args.as_slice() else {
                    return Err("split requires two arguments".into());
                };
                if infer_with(value, Some(&text), env, program, knowledge)? != text {
                    return Err("split requires String input".into());
                }
                let delimiter_type = match delimiters {
                    List(_) => infer_with(delimiters, Some(&texts), env, program, knowledge)?,
                    _ => infer_with(delimiters, None, env, program, knowledge)?,
                };
                if delimiter_type != text && delimiter_type != texts {
                    return Err("split requires String or List<String> delimiters".into());
                }
                return Ok(texts);
            }
            let (parameters, optional, output) = match name.as_str() {
                "stringJoin" => (vec![texts, text.clone()], false, text.clone()),
                "stringLength" | "indexOf" => (
                    if name == "indexOf" {
                        vec![text.clone(), text.clone()]
                    } else {
                        vec![text.clone()]
                    },
                    false,
                    number.clone(),
                ),
                "substring" => (
                    vec![text.clone(), number.clone(), number.clone()],
                    true,
                    text.clone(),
                ),
                "charAt" => (vec![text.clone(), number.clone()], false, text.clone()),
                "padLeading" | "padTrailing" => (
                    vec![text.clone(), number.clone(), text.clone()],
                    true,
                    text.clone(),
                ),
                "repeat" => (vec![text.clone(), number.clone()], false, text.clone()),
                "substringBefore" | "substringAfter" => {
                    (vec![text.clone(), text.clone()], false, text.clone())
                }
                "upperCase" | "lowerCase" | "trim" | "trimLeading" | "trimTrailing" | "reverse" => {
                    (vec![text.clone()], false, text.clone())
                }
                "contains" | "startsWith" | "endsWith" => {
                    (vec![text.clone(), text.clone()], false, boolean.clone())
                }
                "isBlank" | "isEmpty" => (vec![text.clone()], false, boolean.clone()),
                "matches" => (
                    vec![text.clone(), text.clone(), text.clone()],
                    true,
                    boolean.clone(),
                ),
                "replace" => (
                    vec![text.clone(), text.clone(), text.clone(), text.clone()],
                    true,
                    text.clone(),
                ),
                "extract" => (
                    vec![text.clone(), text.clone(), text.clone()],
                    true,
                    Type::Generic(
                        "List".into(),
                        Box::new(Type::Generic("List".into(), Box::new(text.clone()))),
                    ),
                ),
                _ => return Err(format!("unknown knowledge model: {name}")),
            };
            if args.len() != parameters.len() && !(optional && args.len() == parameters.len() - 1) {
                return Err(format!(
                    "{name} requires {}{} arguments",
                    parameters.len() - usize::from(optional),
                    if optional { " or one more" } else { "" }
                ));
            }
            for (arg, ty) in args.iter().zip(&parameters) {
                let actual = infer_with(arg, Some(ty), env, program, knowledge)?;
                if actual != *ty {
                    return Err(format!("{name} requires {ty}, got {actual}"));
                }
            }
            if matches!(name.as_str(), "matches" | "replace" | "extract") {
                let flags = args.get(if name == "replace" { 3 } else { 2 });
                let flags = match flags {
                    Some(String(value)) if !value.chars().all(|c| matches!(c, 'i' | 'm' | 's')) => {
                        return Err(format!("invalid regex flag: {value}"));
                    }
                    Some(String(value)) => value.as_str(),
                    _ => "",
                };
                if let String(pattern) = &args[1] {
                    let mut builder = regex::RegexBuilder::new(pattern);
                    builder
                        .case_insensitive(flags.contains('i'))
                        .multi_line(flags.contains('m'))
                        .dot_matches_new_line(flags.contains('s'));
                    builder
                        .build()
                        .map_err(|error| format!("invalid regex: {error}"))?;
                }
            }
            Ok(output)
        }
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
            let ty = infer_with(base, None, env, program, knowledge)?;
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
        Range(lower, upper, _, _) => {
            let context = match expected {
                Some(Type::Generic(name, inner)) if name == "Range" => Some(inner.as_ref().clone()),
                _ => None,
            };
            let ty = if let Some(bound) = lower.as_ref().or(upper.as_ref()) {
                infer_with(bound, context.as_ref(), env, program, knowledge)?
            } else {
                context.ok_or("cannot infer unbounded range type")?
            };
            if !matches!(&ty, Type::Named(name) if matches!(name.as_str(), "Number" | "Date" | "DateTime" | "Time"))
            {
                return Err(format!("unsupported range bound type: {ty}"));
            }
            for bound in lower.iter().chain(upper.iter()) {
                let actual = infer_with(bound, Some(&ty), env, program, knowledge)?;
                if actual != ty {
                    return Err(format!(
                        "range bounds require matching types, got {ty} and {actual}"
                    ));
                }
            }
            if let (Some(a), Some(b)) = (lower, upper)
                && reversed_constants(a, b, &ty)
            {
                return Err("inverted range bounds".into());
            }
            Ok(Type::Generic("Range".into(), Box::new(ty)))
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
                        .and_then(|element| infer_with(element, None, env, program, knowledge).ok())
                })
                .ok_or("cannot infer empty list type")?;
            for element in elements {
                let actual = infer_with(element, Some(&element_type), env, program, knowledge)?;
                if actual != element_type {
                    return Err(format!("List<{element_type}> element has type {actual}"));
                }
            }
            Ok(Type::Generic("List".into(), Box::new(element_type)))
        }
        Not(value) => {
            let ty = infer_with(value, None, env, program, knowledge)?;
            if ty != named("Bool") {
                return Err(format!("not requires Bool, got {ty}"));
            }
            Ok(named("Bool"))
        }
        Binary(left, op, right) => {
            if op == "in" {
                let lhs = infer_with(left, None, env, program, knowledge)?;
                let kind = if lhs == named("String") && !matches!(right.as_ref(), Range(..)) {
                    "List"
                } else {
                    "Range"
                };
                let expected = Type::Generic(kind.into(), Box::new(lhs.clone()));
                let rhs = infer_with(right, Some(&expected), env, program, knowledge)?;
                if rhs != expected {
                    return Err(format!("in requires a {kind} of {lhs}, got {rhs}"));
                }
                return Ok(named("Bool"));
            }
            let lhs = if matches!(left.as_ref(), Range(None, None, _, _))
                || matches!(left.as_ref(), List(elements) if elements.is_empty())
            {
                let other = infer_with(right, None, env, program, knowledge)?;
                infer_with(left, Some(&other), env, program, knowledge)?
            } else {
                infer_with(left, None, env, program, knowledge)?
            };
            let rhs = infer_with(right, Some(&lhs), env, program, knowledge)?;
            if lhs != rhs {
                return Err(format!("{op} requires matching types, got {lhs} and {rhs}"));
            }
            match op.as_str() {
                "and" | "or" if lhs == named("Bool") => Ok(named("Bool")),
                "+" if lhs == named("String") => Ok(named("String")),
                "==" | "!=" => Ok(named("Bool")),
                ">" | ">=" | "<" | "<="
                    if lhs == named("Number")
                        || lhs == named("String")
                        || lhs == named("DateTime")
                        || lhs == named("Date")
                        || lhs == named("Time") =>
                {
                    Ok(named("Bool"))
                }
                _ => Err(format!(
                    "{op} does not support {lhs}; expected Bool for boolean operations"
                )),
            }
        }
    }
}

fn valid_time_format(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() >= 8
        && b[2] == b':'
        && b[5] == b':'
        && [0, 1, 3, 4, 6, 7].iter().all(|&i| b[i].is_ascii_digit())
        && (b.len() == 8 || (b.len() > 9 && b[8] == b'.' && b[9..].iter().all(u8::is_ascii_digit)))
}

pub(crate) fn string_builtin(name: &str) -> bool {
    matches!(
        name,
        "string"
            | "stringJoin"
            | "stringLength"
            | "substring"
            | "substringBefore"
            | "substringAfter"
            | "upperCase"
            | "lowerCase"
            | "trim"
            | "trimLeading"
            | "trimTrailing"
            | "contains"
            | "startsWith"
            | "endsWith"
            | "matches"
            | "replace"
            | "split"
            | "extract"
            | "isBlank"
            | "isEmpty"
            | "indexOf"
            | "charAt"
            | "reverse"
            | "padLeading"
            | "padTrailing"
            | "repeat"
    )
}

fn range_relation(name: &str) -> bool {
    matches!(
        name,
        "before"
            | "after"
            | "meets"
            | "metBy"
            | "overlaps"
            | "overlapsBefore"
            | "overlapsAfter"
            | "includes"
            | "during"
            | "starts"
            | "startedBy"
            | "finishes"
            | "finishedBy"
            | "coincides"
    )
}

fn reversed_constants(a: &Expr, b: &Expr, ty: &Type) -> bool {
    match (a, b, ty) {
        (Expr::Number(a), Expr::Number(b), Type::Named(name)) if name == "Number" => {
            Decimal::from_str(a)
                .ok()
                .zip(Decimal::from_str(b).ok())
                .is_some_and(|(a, b)| a > b)
        }
        (Expr::Call(a_name, a), Expr::Call(b_name, b), Type::Named(name)) if a_name == b_name => {
            let ([Expr::String(a)], [Expr::String(b)]) = (a.as_slice(), b.as_slice()) else {
                return false;
            };
            match name.as_str() {
                "Date" => NaiveDate::parse_from_str(a, "%Y-%m-%d")
                    .ok()
                    .zip(NaiveDate::parse_from_str(b, "%Y-%m-%d").ok())
                    .is_some_and(|(a, b)| a > b),
                "Time" => NaiveTime::parse_from_str(a, "%H:%M:%S%.f")
                    .ok()
                    .zip(NaiveTime::parse_from_str(b, "%H:%M:%S%.f").ok())
                    .is_some_and(|(a, b)| a > b),
                "DateTime" => DateTime::parse_from_rfc3339(a)
                    .ok()
                    .zip(DateTime::parse_from_rfc3339(b).ok())
                    .is_some_and(|(a, b)| a > b),
                _ => false,
            }
        }
        _ => false,
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
