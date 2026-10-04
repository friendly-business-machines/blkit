use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, NaiveTime, Timelike};
use rust_decimal::Decimal;

use crate::{
    Program, Type,
    decision::{DecisionKind, DecisionModel, DecisionTable, Knowledge},
    expr::{Expr, Stmt},
    graph::{NamedGraph, NodeKind},
};

mod decision;
mod graph;
mod types;

use decision::check_decision;
use graph::check_named_graph;
pub(crate) use graph::named_scopes;
pub(crate) use types::string_builtin;
use types::*;

pub fn validate(program: &Program) -> Result<(), String> {
    let mut names: HashSet<&str> = [
        "Bool", "String", "Number", "Date", "DateTime", "Time", "List",
    ]
    .into_iter()
    .collect();
    let has_graph = program
        .processes
        .iter()
        .any(|process| process.named_graph.is_some());
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
        } else {
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
