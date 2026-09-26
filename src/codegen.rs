use std::collections::HashMap;

use crate::{Program, Type, expr::{Expr, Stmt}, graph::GraphStmt};

fn emit_expr(expr: &Expr, program: &Program) -> String {
    match expr {
        Expr::Number(value) => format!("Number::from_str_exact({value:?}).unwrap()"),
        Expr::String(value) => format!("String::from({value:?})"),
        Expr::Bool(value) => value.to_string(),
        Expr::Name(name) => name.clone(),
        Expr::Field(base, field) => {
            if let Expr::Name(name) = base.as_ref()
                && program.enums.iter().any(|item| item.name == *name)
            {
                return format!("{name}::{field}");
            }
            format!("({}.{}).clone()", emit_expr(base, program), field)
        }
        Expr::List(elements) => format!("vec![{}]", elements.iter()
            .map(|item| emit_expr(item, program)).collect::<Vec<_>>().join(", ")),
        Expr::Not(value) => format!("(!{})", emit_expr(value, program)),
        Expr::Binary(left, op, right) => {
            let operator = match op.as_str() { "and" => "&&", "or" => "||", other => other };
            format!("({} {operator} {})", emit_expr(left, program), emit_expr(right, program))
        }
    }
}

fn emit_stmt(stmt: &Stmt, program: &Program, out: &mut String) {
    match stmt {
        Stmt::Return(value) => out.push_str(&format!("return {};\n", emit_expr(value, program))),
        Stmt::If(condition, yes, no) => {
            out.push_str(&format!("if {} {{\n", emit_expr(condition, program)));
            for branch in yes { emit_stmt(branch, program, out); }
            out.push_str("}\n");
            if !no.is_empty() {
                out.push_str("else {\n");
                for branch in no { emit_stmt(branch, program, out); }
                out.push_str("}\n");
            }
        }
    }
}

fn rust_type(ty: &Type) -> String {
    match ty {
        Type::Named(name) => match name.as_str() {
            "Bool" => "bool".into(),
            "String" => "String".into(),
            _ => name.clone(),
        },
        Type::Generic(_, inner) => format!("Vec<{}>", rust_type(inner)),
    }
}

fn graph_closure(expr: String, env: &HashMap<String, Type>, input: &str, input_type: &Type) -> String {
    let mut names = vec![input.to_owned()];
    let mut decoded = vec![format!("serde_json::from_value::<{}>(source.clone()).map_err(|e| e.to_string())?", rust_type(input_type))];
    let mut keys: Vec<_> = env.iter().filter(|(name, _)| name.as_str() != input).collect();
    keys.sort_by_key(|(name, _)| name.as_str());
    for (name, ty) in keys {
        names.push(name.clone());
        decoded.push(format!("serde_json::from_value::<{}>(values.get({name:?}).ok_or(\"missing graph value: {name}\")?.clone()).map_err(|e| e.to_string())?", rust_type(ty)));
    }
    format!("std::sync::Arc::new(|source, values| {{ let ({},) = ({},); serde_json::to_value({expr}).map_err(|e| e.to_string()) }})", names.join(", "), decoded.join(", "))
}

fn emit_graph_steps(
    body: &[GraphStmt], env: &mut HashMap<String, Type>, program: &Program,
    input: &str, input_type: &Type,
) -> Result<(String, Option<Type>), String> {
    let mut steps = Vec::new();
    let mut last = None;
    for stmt in body {
        match stmt {
            GraphStmt::Run { name, task, input: arg } => {
                let definition = program.tasks.iter().find(|item| item.name == *task).ok_or("missing task")?;
                let expression = format!("self::{task}({})", emit_expr(arg, program));
                steps.push(format!("blkit::runtime::Step::Run {{ name: {name:?}, call: {}, cancel: std::sync::Arc::new(|| {{}}) }}", graph_closure(expression, env, input, input_type)));
                env.insert(name.clone(), definition.output.clone());
                last = Some(definition.output.clone());
            }
            GraphStmt::Gateway { kind, branches, join, output } => {
                let mut emitted = Vec::new();
                let mut result = None;
                for branch in branches {
                    let condition = branch.condition.as_ref().map(|expr| graph_closure(emit_expr(expr, program), env, input, input_type));
                    let mut scoped = env.clone();
                    let (statements, ty) = emit_graph_steps(&branch.body, &mut scoped, program, input, input_type)?;
                    if result.is_none() { result = ty; }
                    emitted.push(format!("blkit::runtime::Branch {{ label: {:?}, condition: {}, steps: {statements} }}", branch.label.as_deref(), condition.map_or("None".into(), |callback| format!("Some({callback})"))));
                }
                let ty = if kind == "and" { output.as_ref().ok_or("missing join type")?.clone() }
                    else if kind == "or" { Type::Generic("List".into(), Box::new(result.ok_or("empty gateway")?)) }
                    else { result.ok_or("empty gateway")? };
                steps.push(format!("blkit::runtime::Step::Gateway {{ kind: {kind:?}, branches: vec![{}], join: {join:?} }}", emitted.join(", ")));
                env.insert(join.clone(), ty.clone());
                last = Some(ty);
            }
            GraphStmt::Return(value) => steps.push(format!("blkit::runtime::Step::Return({})", graph_closure(emit_expr(value, program), env, input, input_type))),
        }
    }
    Ok((format!("vec![{}]", steps.join(", ")), last))
}

pub fn generate(program: &Program) -> Result<String, String> {
    let mut out = format!(
        "pub type Number = rust_decimal::Decimal;\npub const NAMESPACE: &str = {:?};\npub const VERSION: &str = {:?};\n",
        program.namespace, program.version,
    );
    let has_graph = program.processes.iter().any(|item| !item.graph.is_empty());
    let serde = if has_graph { ", serde::Serialize, serde::Deserialize" } else { "" };
    for record in &program.records {
        out.push_str(&format!("#[derive(Debug, Clone, PartialEq{serde})]\npub struct {} {{\n", record.name));
        for (field, ty) in &record.fields {
            out.push_str(&format!("  pub {field}: {},\n", rust_type(ty)));
        }
        out.push_str("}\n");
    }
    for item in &program.enums {
        out.push_str(&format!("#[allow(non_camel_case_types)]\n#[derive(Debug, Clone, PartialEq, Eq{serde})]\npub enum {} {{\n", item.name));
        for variant in &item.variants {
            out.push_str(&format!("  {variant},\n"));
        }
        out.push_str("}\n");
    }
    for process in program.tasks.iter().chain(program.processes.iter().filter(|item| item.graph.is_empty())) {
        if has_graph { out.push_str("#[allow(unused_variables)]\n"); }
        out.push_str(&format!(
            "pub fn {}({}: {}) -> {} {{\n",
            process.name, process.input, rust_type(&process.input_type), rust_type(&process.output),
        ));
        for stmt in &process.body {
            emit_stmt(stmt, program, &mut out);
        }
        out.push_str("}\n");
    }
    if has_graph {
        out.push_str("#[allow(unused_variables, unused_parens)]\npub fn graph_definitions() -> Vec<blkit::runtime::Definition> { vec![\n");
        for process in program.processes.iter().filter(|item| !item.graph.is_empty()) {
            let mut env = HashMap::from([(process.input.clone(), process.input_type.clone())]);
            let (steps, _) = emit_graph_steps(&process.graph, &mut env, program, &process.input, &process.input_type)?;
            out.push_str(&format!("blkit::runtime::Definition {{ namespace: NAMESPACE, version: VERSION, name: {:?}, steps: {steps}, decode_input: Box::new(|value| {{ let typed: {} = serde_json::from_value(value).map_err(|e| e.to_string())?; serde_json::to_value(typed).map_err(|e| e.to_string()) }}) }},\n", process.name, rust_type(&process.input_type)));
        }
        out.push_str("] }\n");
    }
    Ok(out)
}
