use std::collections::HashMap;

use crate::{
    Program, Type,
    decision::{DecisionKind, DecisionModel, DecisionTable, Knowledge},
    expr::{Expr, Stmt},
    graph::{GraphStmt, NodeKind},
    semantic,
};

fn emit_expr(expr: &Expr, program: &Program) -> String {
    emit_expr_with(expr, program, &[])
}

fn emit_expr_with(expr: &Expr, program: &Program, knowledge: &[Knowledge]) -> String {
    match expr {
        Expr::Number(value) => format!("Number::from_str_exact({value:?}).unwrap()"),
        Expr::String(value) => format!("String::from({value:?})"),
        Expr::Bool(value) => value.to_string(),
        Expr::Name(name) => name.clone(),
        Expr::Call(name, args)
            if matches!(name.as_str(), "date" | "time" | "dateTime")
                && !knowledge.iter().any(|item| item.name == *name) =>
        {
            let Expr::String(value) = &args[0] else {
                unreachable!()
            };
            match name.as_str() {
                "date" => format!("{value:?}.parse::<Date>().unwrap()"),
                "time" => format!(
                    "Time(chrono::NaiveTime::parse_from_str({value:?}, \"%H:%M:%S%.f\").unwrap())"
                ),
                _ => format!("chrono::DateTime::parse_from_rfc3339({value:?}).unwrap()"),
            }
        }
        Expr::Call(name, args)
            if matches!(
                name.as_str(),
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
            ) && !knowledge.iter().any(|item| item.name == *name) =>
        {
            let a = emit_expr_with(&args[0], program, knowledge);
            let b = emit_expr_with(&args[1], program, knowledge);
            match name.as_str() {
                "before" | "meets" | "overlaps" | "overlapsBefore" => {
                    format!("({a}).{name}(&({b}))")
                }
                "after" => format!("({b}).before(&({a}))"),
                "metBy" => format!("({b}).meets(&({a}))"),
                "overlapsAfter" => format!("({b}).overlapsBefore(&({a}))"),
                "coincides" => format!("({a}) == ({b})"),
                "includes" | "startedBy" | "finishedBy" => {
                    let method = match name.as_str() {
                        "includes" => "contains",
                        "startedBy" => "starts",
                        _ => "finishes",
                    };
                    format!("({a}).{method}(&({b}))")
                }
                _ => format!(
                    "({b}).{}(&({a}))",
                    if name == "during" { "contains" } else { name }
                ),
            }
        }
        Expr::Call(name, args) => format!(
            "{name}({})",
            args.iter()
                .map(|arg| emit_expr_with(arg, program, knowledge))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Field(base, field) => {
            if let Expr::Name(name) = base.as_ref()
                && program.enums.iter().any(|item| item.name == *name)
            {
                return format!("{name}::{field}");
            }
            format!(
                "({}.{}).clone()",
                emit_expr_with(base, program, knowledge),
                field
            )
        }
        Expr::Range(lower, upper, include_lower, include_upper) => format!(
            "BlRange {{ lower: {}, upper: {}, include_lower: {include_lower}, include_upper: {include_upper} }}",
            lower.as_ref().map_or("None".into(), |value| format!(
                "Some({})",
                emit_expr_with(value, program, knowledge)
            )),
            upper.as_ref().map_or("None".into(), |value| format!(
                "Some({})",
                emit_expr_with(value, program, knowledge)
            ))
        ),
        Expr::List(elements) => format!(
            "vec![{}]",
            elements
                .iter()
                .map(|item| emit_expr_with(item, program, knowledge))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Not(value) => format!("(!{})", emit_expr_with(value, program, knowledge)),
        Expr::Binary(left, op, right) => {
            if op == "in" {
                return format!(
                    "({}).contains(&({}))",
                    emit_expr_with(right, program, knowledge),
                    emit_expr_with(left, program, knowledge)
                );
            }
            let operator = match op.as_str() {
                "and" => "&&",
                "or" => "||",
                other => other,
            };
            format!(
                "({} {operator} {})",
                emit_expr_with(left, program, knowledge),
                emit_expr_with(right, program, knowledge)
            )
        }
    }
}

fn emit_stmt(stmt: &Stmt, program: &Program, out: &mut String) {
    match stmt {
        Stmt::Return(value) => out.push_str(&format!("return {};\n", emit_expr(value, program))),
        Stmt::If(condition, yes, no) => {
            out.push_str(&format!("if {} {{\n", emit_expr(condition, program)));
            for branch in yes {
                emit_stmt(branch, program, out);
            }
            out.push_str("}\n");
            if !no.is_empty() {
                out.push_str("else {\n");
                for branch in no {
                    emit_stmt(branch, program, out);
                }
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

fn graph_closure(
    expr: String,
    env: &HashMap<String, Type>,
    input: &str,
    input_type: &Type,
) -> String {
    let mut names = vec![input.to_owned()];
    let mut decoded = vec![format!(
        "serde_json::from_value::<{}>(source.clone()).map_err(|e| e.to_string())?",
        rust_type(input_type)
    )];
    let mut keys: Vec<_> = env
        .iter()
        .filter(|(name, _)| name.as_str() != input)
        .collect();
    keys.sort_by_key(|(name, _)| name.as_str());
    for (name, ty) in keys {
        names.push(name.clone());
        decoded.push(format!("serde_json::from_value::<{}>(values.get({name:?}).ok_or(\"missing graph value: {name}\")?.clone()).map_err(|e| e.to_string())?", rust_type(ty)));
    }
    format!(
        "std::sync::Arc::new(|source, values| {{ let ({},) = ({},); serde_json::to_value({expr}).map_err(|e| e.to_string()) }})",
        names.join(", "),
        decoded.join(", ")
    )
}

fn external_graph_closure(
    task: &str,
    external: &crate::compiler::ExternalTask,
    input: String,
) -> Result<String, String> {
    let (provider, _) = task.split_once('.').ok_or("invalid qualified task")?;
    let output = rust_type(&external.output);
    Ok(format!(
        "std::sync::Arc::new(|source: serde_json::Value, values: blkit::runtime::Values| {{ let argument: blkit::runtime::Evaluate = {input}; Box::pin(async move {{ let value = argument(&source, &values)?; let result = {provider}::{}(value).await?; let typed: {output} = serde_json::from_value(result).map_err(|e| e.to_string())?; serde_json::to_value(typed).map_err(|e| e.to_string()) }}) }})",
        external.function
    ))
}

fn emit_graph_steps(
    body: &[GraphStmt],
    env: &mut HashMap<String, Type>,
    program: &Program,
    input: &str,
    input_type: &Type,
) -> Result<(String, Option<Type>), String> {
    let mut steps = Vec::new();
    let mut last = None;
    for stmt in body {
        match stmt {
            GraphStmt::Run {
                name,
                task,
                input: arg,
            } => {
                let definition = program
                    .tasks
                    .iter()
                    .find(|item| item.name == *task)
                    .ok_or("missing task")?;
                let expression = format!("self::{task}({})", emit_expr(arg, program));
                steps.push(format!("blkit::runtime::Step::Run {{ name: {name:?}, call: {}, cancel: std::sync::Arc::new(|| {{}}) }}", graph_closure(expression, env, input, input_type)));
                env.insert(name.clone(), definition.output.clone());
                last = Some(definition.output.clone());
            }
            GraphStmt::Gateway {
                kind,
                branches,
                join,
                output,
            } => {
                let mut emitted = Vec::new();
                let mut result = None;
                for branch in branches {
                    let condition = branch.condition.as_ref().map(|expr| {
                        graph_closure(emit_expr(expr, program), env, input, input_type)
                    });
                    let mut scoped = env.clone();
                    let (statements, ty) =
                        emit_graph_steps(&branch.body, &mut scoped, program, input, input_type)?;
                    if result.is_none() {
                        result = ty;
                    }
                    emitted.push(format!("blkit::runtime::Branch {{ label: {:?}, condition: {}, steps: {statements} }}", branch.label.as_deref(), condition.map_or("None".into(), |callback| format!("Some({callback})"))));
                }
                let ty = if kind == "and" {
                    output.as_ref().ok_or("missing join type")?.clone()
                } else if kind == "or" {
                    Type::Generic("List".into(), Box::new(result.ok_or("empty gateway")?))
                } else {
                    result.ok_or("empty gateway")?
                };
                steps.push(format!("blkit::runtime::Step::Gateway {{ kind: {kind:?}, branches: vec![{}], join: {join:?} }}", emitted.join(", ")));
                env.insert(join.clone(), ty.clone());
                last = Some(ty);
            }
            GraphStmt::Return(value) => steps.push(format!(
                "blkit::runtime::Step::Return({})",
                graph_closure(emit_expr(value, program), env, input, input_type)
            )),
        }
    }
    Ok((format!("vec![{}]", steps.join(", ")), last))
}

fn table_item(
    table: &DecisionTable,
    values: &[Expr],
    result: &Type,
    program: &Program,
    knowledge: &[Knowledge],
) -> String {
    if table.outputs.len() == 1 {
        emit_expr_with(&values[0], program, knowledge)
    } else {
        let Type::Named(name) = (if matches!(result, Type::Generic(_, _)) {
            match result {
                Type::Generic(_, inner) => inner.as_ref(),
                _ => unreachable!(),
            }
        } else {
            result
        }) else {
            unreachable!()
        };
        format!(
            "{name} {{ {} }}",
            table
                .outputs
                .iter()
                .zip(values)
                .map(|((field, _), expr)| format!(
                    "{field}: {}",
                    emit_expr_with(expr, program, knowledge)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn emit_table(
    table: &DecisionTable,
    result: &Type,
    program: &Program,
    knowledge: &[Knowledge],
) -> String {
    let mut code = String::from("{\n");
    for (name, ty, expression) in &table.inputs {
        code.push_str(&format!(
            "let {name}: {} = {};\n",
            rust_type(ty),
            emit_expr_with(expression, program, knowledge)
        ));
    }
    let item_type = if matches!(
        table.policy.as_str(),
        "RULE_ORDER" | "OUTPUT_ORDER" | "COLLECT"
    ) && table.aggregation.is_none()
    {
        if let Type::Generic(_, inner) = result {
            rust_type(inner)
        } else {
            rust_type(result)
        }
    } else if table.aggregation.as_deref() == Some("COUNT") {
        "()".into()
    } else {
        rust_type(result)
    };
    code.push_str(&format!(
        "let mut __bl_matches: Vec<{item_type}> = Vec::new();\n"
    ));
    for (condition, values) in &table.rules {
        code.push_str(&format!(
            "if {} {{ __bl_matches.push({}); }}\n",
            emit_expr_with(condition, program, knowledge),
            if table.aggregation.as_deref() == Some("COUNT") {
                "()".into()
            } else {
                table_item(table, values, result, program, knowledge)
            }
        ));
    }
    let fallback = table
        .default
        .as_ref()
        .map(|values| table_item(table, values, result, program, knowledge));
    let absent = fallback
        .map(|value| format!("Ok({value})"))
        .unwrap_or_else(|| "Err(String::from(\"no matching decision rule\"))".into());
    let result_expr = match (table.policy.as_str(), table.aggregation.as_deref()) {
        ("UNIQUE", _) => format!(
            "if __bl_matches.len() > 1 {{ Err(String::from(\"UNIQUE policy violation\")) }} else {{ __bl_matches.pop().map(Ok).unwrap_or_else(|| {absent}) }}"
        ),
        ("ANY", _) => format!(
            "if __bl_matches.windows(2).any(|pair| pair[0] != pair[1]) {{ Err(String::from(\"ANY policy violation\")) }} else {{ __bl_matches.pop().map(Ok).unwrap_or_else(|| {absent}) }}"
        ),
        ("FIRST", _) => format!(
            "if __bl_matches.is_empty() {{ {absent} }} else {{ Ok(__bl_matches.remove(0)) }}"
        ),
        ("PRIORITY" | "OUTPUT_ORDER", _) => {
            let priority = table
                .priorities
                .iter()
                .map(|row| table_item(table, row, result, program, knowledge))
                .collect::<Vec<_>>()
                .join(", ");
            let mut expression = format!(
                "let __bl_priorities = vec![{priority}]; let mut __bl_ranked = __bl_matches.into_iter().enumerate().map(|(position, value)| __bl_priorities.iter().position(|ranked| *ranked == value).map(|rank| (rank, position, value)).ok_or_else(|| String::from(\"unranked decision output\"))).collect::<Result<Vec<_>, String>>()?; __bl_ranked.sort_by_key(|(rank, position, _)| (*rank, *position));"
            );
            if table.policy == "PRIORITY" {
                expression.push_str(&format!(" if __bl_ranked.is_empty() {{ {absent} }} else {{ Ok(__bl_ranked.remove(0).2) }}"));
            } else {
                let absent = table
                    .default
                    .as_ref()
                    .map(|values| {
                        format!(
                            "vec![{}]",
                            table_item(table, values, result, program, knowledge)
                        )
                    })
                    .unwrap_or_else(|| "vec![]".into());
                expression.push_str(&format!(" if __bl_ranked.is_empty() {{ Ok({absent}) }} else {{ Ok(__bl_ranked.into_iter().map(|(_, _, value)| value).collect()) }}"));
            }
            expression
        }
        ("RULE_ORDER" | "COLLECT", None) => {
            let absent = table
                .default
                .as_ref()
                .map(|values| {
                    format!(
                        "vec![{}]",
                        table_item(table, values, result, program, knowledge)
                    )
                })
                .unwrap_or_else(|| "vec![]".into());
            format!("if __bl_matches.is_empty() {{ Ok({absent}) }} else {{ Ok(__bl_matches) }}")
        }
        ("COLLECT", Some("COUNT")) => {
            let absent = table
                .default
                .as_ref()
                .map(|values| emit_expr_with(&values[0], program, knowledge))
                .unwrap_or_else(|| "Number::ZERO".into());
            format!(
                "if __bl_matches.is_empty() {{ Ok({absent}) }} else {{ Ok(Number::from(__bl_matches.len() as u64)) }}"
            )
        }
        ("COLLECT", Some("SUM" | "MIN" | "MAX")) => {
            let operation = match table.aggregation.as_deref().unwrap() {
                "SUM" => "sum::<Number>()",
                "MIN" => "min().unwrap()",
                _ => "max().unwrap()",
            };
            format!(
                "if __bl_matches.is_empty() {{ {absent} }} else {{ Ok(__bl_matches.into_iter().{operation}) }}"
            )
        }
        _ => unreachable!(),
    };
    code.push_str(&result_expr);
    code.push_str("\n}");
    code
}

fn emit_decision(model: &DecisionModel, program: &Program, out: &mut String) {
    out.push_str(&format!(
        "pub fn {}({}: {}) -> Result<{}, String> {{\n",
        model.name,
        model.input,
        rust_type(&model.input_type),
        rust_type(&model.output)
    ));
    for item in &model.knowledge {
        out.push_str(&format!(
            "fn {}({}) -> {} {{ {} }}\n",
            item.name,
            item.params
                .iter()
                .map(|(name, ty)| format!("{name}: {}", rust_type(ty)))
                .collect::<Vec<_>>()
                .join(", "),
            rust_type(&item.output),
            emit_expr_with(&item.body, program, &model.knowledge)
        ));
    }
    let mut emitted = std::collections::HashSet::new();
    while emitted.len() < model.nodes.len() {
        for node in &model.nodes {
            if emitted.contains(&node.name)
                || model
                    .links
                    .iter()
                    .any(|(from, to)| to == &node.name && !emitted.contains(from))
            {
                continue;
            }
            let value = match &node.kind {
                DecisionKind::Literal(expr) => emit_expr_with(expr, program, &model.knowledge),
                DecisionKind::Context { entries, result } => format!(
                    "{{ {} {} }}",
                    entries
                        .iter()
                        .map(|(name, ty, expr)| format!(
                            "let {name}: {} = {};",
                            rust_type(ty),
                            emit_expr_with(expr, program, &model.knowledge)
                        ))
                        .collect::<Vec<_>>()
                        .join(" "),
                    emit_expr_with(result, program, &model.knowledge)
                ),
                DecisionKind::Table(table) => format!(
                    "(|| -> Result<{}, String> {{ {} }})()?",
                    rust_type(&node.output),
                    emit_table(table, &node.output, program, &model.knowledge)
                ),
            };
            out.push_str(&format!(
                "let {}: {} = {value};\n",
                node.name,
                rust_type(&node.output)
            ));
            emitted.insert(&node.name);
        }
    }
    out.push_str(&format!("Ok({})\n}}\n", model.output_node));
}

pub fn generate(program: &Program) -> Result<String, String> {
    let mut out = format!(
        "pub type Number = rust_decimal::Decimal;\npub const NAMESPACE: &str = {:?};\npub const VERSION: &str = {:?};\n",
        program.namespace, program.version,
    );
    fn datetime(ty: &Type) -> bool {
        match ty {
            Type::Named(name) => name == "DateTime",
            Type::Generic(_, inner) => datetime(inner),
        }
    }
    if program
        .records
        .iter()
        .flat_map(|r| &r.fields)
        .any(|(_, ty)| datetime(ty))
        || program
            .processes
            .iter()
            .chain(&program.tasks)
            .any(|item| datetime(&item.input_type) || datetime(&item.output))
        || program.decisions.iter().any(|item| {
            datetime(&item.input_type)
                || datetime(&item.output)
                || item.knowledge.iter().any(|model| {
                    datetime(&model.output) || model.params.iter().any(|(_, ty)| datetime(ty))
                })
                || item.nodes.iter().any(|node| {
                    datetime(&node.output)
                        || match &node.kind {
                            DecisionKind::Table(table) => {
                                table.inputs.iter().any(|(_, ty, _)| datetime(ty))
                                    || table.outputs.iter().any(|(_, ty)| datetime(ty))
                            }
                            DecisionKind::Context { entries, .. } => {
                                entries.iter().any(|(_, ty, _)| datetime(ty))
                            }
                            DecisionKind::Literal(_) => false,
                        }
                })
        })
    {
        out.push_str("pub type DateTime = chrono::DateTime<chrono::FixedOffset>;\n");
    }
    for model in &program.decisions {
        emit_decision(model, program, &mut out);
    }
    let has_legacy = program.processes.iter().any(|item| !item.graph.is_empty());
    let has_named = program
        .processes
        .iter()
        .any(|item| item.named_graph.is_some());
    let has_graph = has_legacy || has_named;
    let serde = if has_graph {
        ", serde::Serialize, serde::Deserialize"
    } else {
        ""
    };
    for record in &program.records {
        out.push_str(&format!(
            "#[derive(Debug, Clone, PartialEq{serde})]\npub struct {} {{\n",
            record.name
        ));
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
    for process in program.tasks.iter().chain(
        program
            .processes
            .iter()
            .filter(|item| item.graph.is_empty() && item.named_graph.is_none()),
    ) {
        if has_graph {
            out.push_str("#[allow(unused_variables)]\n");
        }
        out.push_str(&format!(
            "pub fn {}({}: {}) -> {} {{\n",
            process.name,
            process.input,
            rust_type(&process.input_type),
            rust_type(&process.output),
        ));
        for stmt in &process.body {
            emit_stmt(stmt, program, &mut out);
        }
        out.push_str("}\n");
    }
    if has_legacy {
        out.push_str("#[allow(unused_variables, unused_parens)]\npub fn graph_definitions() -> Vec<blkit::runtime::Definition> { vec![\n");
        for process in program
            .processes
            .iter()
            .filter(|item| !item.graph.is_empty())
        {
            let mut env = HashMap::from([(process.input.clone(), process.input_type.clone())]);
            let (steps, _) = emit_graph_steps(
                &process.graph,
                &mut env,
                program,
                &process.input,
                &process.input_type,
            )?;
            out.push_str(&format!("blkit::runtime::Definition {{ namespace: NAMESPACE, version: VERSION, name: {:?}, steps: {steps}, decode_input: Box::new(|value| {{ let typed: {} = serde_json::from_value(value).map_err(|e| e.to_string())?; serde_json::to_value(typed).map_err(|e| e.to_string()) }}) }},\n", process.name, rust_type(&process.input_type)));
        }
        out.push_str("] }\n");
    }
    if has_named {
        out.push_str("#[allow(unused_variables, unused_parens)]\npub fn named_graph_definitions() -> Vec<blkit::named_runtime::GraphDefinition> { vec![\n");
        for process in program
            .processes
            .iter()
            .filter(|item| item.named_graph.is_some())
        {
            let graph = process.named_graph.as_ref().unwrap();
            let scopes = semantic::named_scopes(graph, process, program)?;
            let retry = process.retry.as_ref().map_or("None".into(), |policy| format!("Some(blkit::RetryPolicy {{ max_retries: {}, retry_for: std::time::Duration::from_millis({}), retry_delay: std::time::Duration::from_millis({}), backoff: {:?} }})", policy.max_retries, policy.retry_for.as_millis(), policy.retry_delay.as_millis(), policy.backoff));
            let deadline = process.deadline.as_ref().map_or("None".into(), |policy| format!("Some(blkit::DeadlinePolicy {{ origin: {:?}, duration: std::time::Duration::from_millis({}) }})", policy.origin, policy.duration.as_millis()));
            out.push_str(&format!("blkit::named_runtime::GraphDefinition {{ namespace: NAMESPACE, version: VERSION, name: {:?}, retry: {retry}, deadline: {deadline}, decode_input: Box::new(|value| {{ let typed: {} = serde_json::from_value(value).map_err(|e| e.to_string())?; serde_json::to_value(typed).map_err(|e| e.to_string()) }}), nodes: vec![\n", process.name, rust_type(&process.input_type)));
            for node in &graph.nodes {
                let kind = match &node.kind {
                    NodeKind::Start => "blkit::named_runtime::GraphNodeKind::Start".into(),
                    NodeKind::PauseFor(duration) => format!(
                        "blkit::named_runtime::GraphNodeKind::PauseFor(std::time::Duration::from_millis({}))",
                        duration.as_millis()
                    ),
                    NodeKind::PauseUntil(expression) => format!(
                        "blkit::named_runtime::GraphNodeKind::PauseUntil({})",
                        graph_closure(
                            emit_expr(expression, program),
                            &scopes[node.name.as_str()],
                            &process.input,
                            &process.input_type
                        )
                    ),
                    NodeKind::BusinessRule {
                        model,
                        input: argument,
                    } => {
                        let mut env = scopes[node.name.as_str()].clone();
                        env.remove(&node.name);
                        let expression =
                            format!("self::{model}({})?", emit_expr(argument, program));
                        format!(
                            "blkit::named_runtime::GraphNodeKind::Task({})",
                            graph_closure(expression, &env, &process.input, &process.input_type)
                        )
                    }
                    NodeKind::Task {
                        task,
                        input: argument,
                    } => {
                        let mut env = scopes[node.name.as_str()].clone();
                        env.remove(&node.name);
                        if let Some(external) = program.external_tasks.get(task) {
                            let input = graph_closure(
                                emit_expr(argument, program),
                                &env,
                                &process.input,
                                &process.input_type,
                            );
                            format!(
                                "blkit::named_runtime::GraphNodeKind::AsyncTask({})",
                                external_graph_closure(task, external, input)?
                            )
                        } else {
                            let expression =
                                format!("self::{task}({})", emit_expr(argument, program));
                            format!(
                                "blkit::named_runtime::GraphNodeKind::Task({})",
                                graph_closure(
                                    expression,
                                    &env,
                                    &process.input,
                                    &process.input_type
                                )
                            )
                        }
                    }
                    NodeKind::MultiInstance {
                        task,
                        items,
                        parallel,
                    } => {
                        let mut env = scopes[node.name.as_str()].clone();
                        env.remove(&node.name);
                        let items = graph_closure(
                            emit_expr(items, program),
                            &env,
                            &process.input,
                            &process.input_type,
                        );
                        if let Some(external) = program.external_tasks.get(task) {
                            let argument = format!(
                                "std::sync::Arc::new(|item, _| {{ let typed: {} = serde_json::from_value(item.clone()).map_err(|e| e.to_string())?; serde_json::to_value(typed).map_err(|e| e.to_string()) }})",
                                rust_type(&external.input)
                            );
                            let call = external_graph_closure(task, external, argument)?;
                            format!(
                                "blkit::named_runtime::GraphNodeKind::AsyncMultiInstance {{ task: {call}, items: {items}, parallel: {parallel} }}"
                            )
                        } else {
                            let definition = program
                                .tasks
                                .iter()
                                .find(|item| item.name == *task)
                                .ok_or_else(|| format!("unknown task: {task}"))?;
                            format!(
                                "blkit::named_runtime::GraphNodeKind::MultiInstance {{ task: std::sync::Arc::new(|item, _| {{ let typed: {} = serde_json::from_value(item.clone()).map_err(|e| e.to_string())?; serde_json::to_value(self::{task}(typed)).map_err(|e| e.to_string()) }}), items: {items}, parallel: {parallel} }}",
                                rust_type(&definition.input_type)
                            )
                        }
                    }
                    NodeKind::TaskLoop {
                        task,
                        input: argument,
                        condition,
                        before,
                        initial,
                        max_iterations,
                        max_duration,
                    } => {
                        let mut initial_env = scopes[node.name.as_str()].clone();
                        initial_env.remove(&node.name);
                        let argument_env = if *before {
                            &scopes[node.name.as_str()]
                        } else {
                            &initial_env
                        };
                        let (kind, call) = if let Some(external) = program.external_tasks.get(task)
                        {
                            let input = graph_closure(
                                emit_expr(argument, program),
                                argument_env,
                                &process.input,
                                &process.input_type,
                            );
                            (
                                "AsyncTaskLoop",
                                external_graph_closure(task, external, input)?,
                            )
                        } else {
                            (
                                "TaskLoop",
                                graph_closure(
                                    format!("self::{task}({})", emit_expr(argument, program)),
                                    argument_env,
                                    &process.input,
                                    &process.input_type,
                                ),
                            )
                        };
                        let condition = graph_closure(
                            emit_expr(condition, program),
                            &scopes[node.name.as_str()],
                            &process.input,
                            &process.input_type,
                        );
                        let initial = initial.as_ref().map_or("None".into(), |value| {
                            format!(
                                "Some({})",
                                graph_closure(
                                    emit_expr(value, program),
                                    &initial_env,
                                    &process.input,
                                    &process.input_type
                                )
                            )
                        });
                        let max_iterations =
                            max_iterations.map_or("None".into(), |count| format!("Some({count})"));
                        let max_duration = max_duration.map_or("None".into(), |duration| {
                            format!(
                                "Some(std::time::Duration::from_millis({}))",
                                duration.as_millis()
                            )
                        });
                        format!(
                            "blkit::named_runtime::GraphNodeKind::{kind}({call}, blkit::named_runtime::LoopPolicy {{ condition: {condition}, initial: {initial}, before: {before}, max_iterations: {max_iterations}, max_duration: {max_duration} }})"
                        )
                    }
                    NodeKind::Split(kind) => {
                        format!("blkit::named_runtime::GraphNodeKind::Split({kind:?})")
                    }
                    NodeKind::Join { kind, split, .. } => format!(
                        "blkit::named_runtime::GraphNodeKind::Join {{ kind: {kind:?}, split: {split:?} }}"
                    ),
                    NodeKind::End => "blkit::named_runtime::GraphNodeKind::End".into(),
                    NodeKind::Error => "blkit::named_runtime::GraphNodeKind::Error".into(),
                    NodeKind::Cancel => "blkit::named_runtime::GraphNodeKind::Cancel".into(),
                    NodeKind::Terminate => "blkit::named_runtime::GraphNodeKind::Terminate".into(),
                };
                out.push_str(&format!(
                    "blkit::named_runtime::GraphNode {{ name: {:?}, kind: {kind} }},\n",
                    node.name
                ));
            }
            out.push_str("], links: vec![\n");
            for link in &graph.links {
                let env = &scopes[link.source.as_str()];
                let value = link.value.as_ref().map_or("None".into(), |expr| {
                    format!(
                        "Some({})",
                        graph_closure(
                            emit_expr(expr, program),
                            env,
                            &process.input,
                            &process.input_type
                        )
                    )
                });
                let condition = link.condition.as_ref().map_or("None".into(), |expr| {
                    format!(
                        "Some({})",
                        graph_closure(
                            emit_expr(expr, program),
                            env,
                            &process.input,
                            &process.input_type
                        )
                    )
                });
                out.push_str(&format!("blkit::named_runtime::GraphLink {{ source: {:?}, target: {:?}, value: {value}, condition: {condition}, fallback: {}, label: {:?} }},\n", link.source, link.target, link.fallback, link.label));
            }
            out.push_str("] },\n");
        }
        out.push_str("] }\n");
    }
    if out.contains("BlRange { lower:") {
        for name in program
            .records
            .iter()
            .map(|item| item.name.as_str())
            .chain(program.enums.iter().map(|item| item.name.as_str()))
            .chain(
                program
                    .tasks
                    .iter()
                    .chain(&program.processes)
                    .map(|item| item.name.as_str()),
            )
            .chain(program.decisions.iter().map(|item| item.name.as_str()))
        {
            if matches!(name, "BlRange" | "BlRangeValue" | "lower_cmp" | "upper_cmp") {
                return Err(format!("reserved generated name: {name}"));
            }
        }
        out.push_str(r#"
trait BlRangeValue: Ord + Clone {
    fn adjacent(&self, _next: &Self) -> bool { false }
}
impl BlRangeValue for Number {}
impl BlRangeValue for chrono::DateTime<chrono::FixedOffset> {}
#[derive(Debug, Clone)]
struct BlRange<T> { lower: Option<T>, upper: Option<T>, include_lower: bool, include_upper: bool }
impl<T: BlRangeValue> BlRange<T> {
    fn empty(&self) -> bool {
        self.lower.as_ref().zip(self.upper.as_ref()).is_some_and(|(a, b)|
            a > b || (a == b && !(self.include_lower && self.include_upper))
                || (!self.include_lower && !self.include_upper && a.adjacent(b)))
    }
    fn contains(&self, value: &T) -> bool {
        !self.empty() && self.lower.as_ref().is_none_or(|a| if self.include_lower { value >= a } else { value > a })
            && self.upper.as_ref().is_none_or(|b| if self.include_upper { value <= b } else { value < b })
    }
    fn starts(&self, value: &T) -> bool {
        !self.empty() && self.include_lower && self.lower.as_ref() == Some(value)
    }
    fn finishes(&self, value: &T) -> bool {
        !self.empty() && self.include_upper && self.upper.as_ref() == Some(value)
    }
    fn before(&self, other: &Self) -> bool {
        !self.empty() && !other.empty() && self.upper.as_ref().zip(other.lower.as_ref()).is_some_and(|(a, b)| a < b)
    }
    fn meets(&self, other: &Self) -> bool {
        !self.empty() && !other.empty() && self.upper.as_ref().zip(other.lower.as_ref()).is_some_and(|(a, b)| a == b)
    }
    fn overlapsBefore(&self, other: &Self) -> bool {
        !self.empty() && !other.empty()
            && lower_cmp(self.lower.as_ref(), other.lower.as_ref()).is_lt()
            && self.upper.as_ref().is_some_and(|end| other.lower.as_ref().is_none_or(|start| start < end))
            && upper_cmp(self.upper.as_ref(), other.upper.as_ref()).is_lt()
    }
    fn overlaps(&self, other: &Self) -> bool {
        if self.empty() || other.empty() { return false; }
        let (lower, include_lower) = match (self.lower.as_ref(), other.lower.as_ref()) {
            (None, None) => (None, false),
            (Some(a), None) => (Some(a.clone()), self.include_lower),
            (None, Some(b)) => (Some(b.clone()), other.include_lower),
            (Some(a), Some(b)) if a > b => (Some(a.clone()), self.include_lower),
            (Some(a), Some(b)) if b > a => (Some(b.clone()), other.include_lower),
            (Some(a), Some(_)) => (Some(a.clone()), self.include_lower && other.include_lower),
        };
        let (upper, include_upper) = match (self.upper.as_ref(), other.upper.as_ref()) {
            (None, None) => (None, false),
            (Some(a), None) => (Some(a.clone()), self.include_upper),
            (None, Some(b)) => (Some(b.clone()), other.include_upper),
            (Some(a), Some(b)) if a < b => (Some(a.clone()), self.include_upper),
            (Some(a), Some(b)) if b < a => (Some(b.clone()), other.include_upper),
            (Some(a), Some(_)) => (Some(a.clone()), self.include_upper && other.include_upper),
        };
        !Self { lower, upper, include_lower, include_upper }.empty()
    }
}
fn lower_cmp<T: Ord>(a: Option<&T>, b: Option<&T>) -> std::cmp::Ordering {
    match (a, b) { (None, None) => std::cmp::Ordering::Equal, (None, _) => std::cmp::Ordering::Less, (_, None) => std::cmp::Ordering::Greater, (Some(a), Some(b)) => a.cmp(b) }
}
fn upper_cmp<T: Ord>(a: Option<&T>, b: Option<&T>) -> std::cmp::Ordering {
    match (a, b) { (None, None) => std::cmp::Ordering::Equal, (None, _) => std::cmp::Ordering::Greater, (_, None) => std::cmp::Ordering::Less, (Some(a), Some(b)) => a.cmp(b) }
}
impl<T: BlRangeValue> PartialEq for BlRange<T> {
    fn eq(&self, other: &Self) -> bool {
        self.lower == other.lower && self.upper == other.upper
            && (self.lower.is_none() || self.include_lower == other.include_lower)
            && (self.upper.is_none() || self.include_upper == other.include_upper)
    }
}
"#);
    }
    fn date(ty: &Type) -> bool {
        match ty {
            Type::Named(name) => name == "Date",
            Type::Generic(_, inner) => date(inner),
        }
    }
    if out.contains(".parse::<Date>()")
        || program
            .records
            .iter()
            .flat_map(|r| &r.fields)
            .any(|(_, ty)| date(ty))
        || program
            .processes
            .iter()
            .chain(&program.tasks)
            .any(|item| date(&item.input_type) || date(&item.output))
        || program.decisions.iter().any(|item| {
            date(&item.input_type)
                || date(&item.output)
                || item
                    .knowledge
                    .iter()
                    .any(|model| date(&model.output) || model.params.iter().any(|(_, ty)| date(ty)))
                || item.nodes.iter().any(|node| {
                    date(&node.output)
                        || match &node.kind {
                            DecisionKind::Table(table) => {
                                table.inputs.iter().any(|(_, ty, _)| date(ty))
                                    || table.outputs.iter().any(|(_, ty)| date(ty))
                            }
                            DecisionKind::Context { entries, .. } => {
                                entries.iter().any(|(_, ty, _)| date(ty))
                            }
                            DecisionKind::Literal(_) => false,
                        }
                })
        })
    {
        if out.contains("struct BlRange<T>") {
            out.push_str("impl BlRangeValue for Date { fn adjacent(&self, next: &Self) -> bool { self.0.succ_opt() == Some(next.0) } }\n");
        }
        out.push_str(
            r#"
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date(pub chrono::NaiveDate);
impl std::str::FromStr for Date {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let value = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|e| e.to_string())?;
        if text.len() != 10 || value.format("%Y-%m-%d").to_string() != text {
            return Err(String::from("expected YYYY-MM-DD"));
        }
        Ok(Self(value))
    }
}
impl serde::Serialize for Date {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.format("%Y-%m-%d").to_string())
    }
}
impl<'de> serde::Deserialize<'de> for Date {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}
"#,
        );
    }
    if out.contains("Time(")
        || out.contains(": Time")
        || out.contains("-> Time")
        || out.contains("<Time>")
    {
        if out.contains("struct BlRange<T>") {
            out.push_str("impl BlRangeValue for Time {}\n");
        }
        out.push_str(
            r#"
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Time(pub chrono::NaiveTime);
impl serde::Serialize for Time {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}
impl<'de> serde::Deserialize<'de> for Time {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        let b = text.as_bytes();
        if b.len() < 8 || b[2] != b':' || b[5] != b':' || ![0, 1, 3, 4, 6, 7].iter().all(|&i| b[i].is_ascii_digit())
            || !(b.len() == 8 || (b.len() > 9 && b[8] == b'.' && b[9..].iter().all(u8::is_ascii_digit))) {
            return Err(serde::de::Error::custom("expected HH:MM:SS[.fraction]"));
        }
        let time = chrono::NaiveTime::parse_from_str(&text, "%H:%M:%S%.f")
            .map_err(serde::de::Error::custom)?;
        if chrono::Timelike::nanosecond(&time) >= 1_000_000_000 {
            return Err(serde::de::Error::custom("leap seconds are not supported"));
        }
        Ok(Self(time))
    }
}
"#,
        );
    }
    Ok(out)
}
