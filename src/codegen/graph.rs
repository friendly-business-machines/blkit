use super::*;

pub(super) fn emit_named_graphs(program: &Program, out: &mut String) -> Result<(), String> {
    out.push_str("#[allow(unused_variables, unused_parens)]\npub fn named_graph_definitions() -> Vec<blkit::compiled_graph::GraphDefinition> { vec![\n");
    for process in program
        .processes
        .iter()
        .filter(|item| item.named_graph.is_some())
    {
        let graph = process.named_graph.as_ref().unwrap();
        let scopes = semantic::named_scopes(graph, process, program)?;
        let retry = process.retry.as_ref().map_or("None".into(), |policy| format!("Some(blkit::RetryPolicy {{ max_retries: {}, retry_for: std::time::Duration::from_millis({}), retry_delay: std::time::Duration::from_millis({}), backoff: {:?} }})", policy.max_retries, policy.retry_for.as_millis(), policy.retry_delay.as_millis(), policy.backoff));
        let deadline = process.deadline.as_ref().map_or("None".into(), |policy| format!("Some(blkit::DeadlinePolicy {{ origin: {:?}, duration: std::time::Duration::from_millis({}) }})", policy.origin, policy.duration.as_millis()));
        out.push_str(&format!("blkit::compiled_graph::GraphDefinition {{ namespace: NAMESPACE, version: VERSION, name: {:?}, retry: {retry}, deadline: {deadline}, decode_input: Box::new(|value| {{ let typed: {} = serde_json::from_value(value).map_err(|e| e.to_string())?; serde_json::to_value(typed).map_err(|e| e.to_string()) }}), nodes: vec![\n", process.name, rust_type(&process.input_type)));
        for node in &graph.nodes {
            let kind = match &node.kind {
                NodeKind::Start => "blkit::compiled_graph::GraphNodeKind::Start".into(),
                NodeKind::Subprocess {
                    process: child,
                    input: argument,
                } => {
                    let mut env = scopes[node.name.as_str()].clone();
                    env.remove(&node.name);
                    format!(
                        "blkit::compiled_graph::GraphNodeKind::Subprocess {{ process: {child:?}, input: {} }}",
                        graph_closure(
                            emit_expr(argument, program),
                            &env,
                            &process.input,
                            &process.input_type
                        )
                    )
                }
                NodeKind::PauseFor(duration) => format!(
                    "blkit::compiled_graph::GraphNodeKind::PauseFor(std::time::Duration::from_millis({}))",
                    duration.as_millis()
                ),
                NodeKind::PauseUntil(expression) => format!(
                    "blkit::compiled_graph::GraphNodeKind::PauseUntil({})",
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
                    let expression = format!("self::{model}({})?", emit_expr(argument, program));
                    format!(
                        "blkit::compiled_graph::GraphNodeKind::Task({})",
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
                            "blkit::compiled_graph::GraphNodeKind::AsyncTask({})",
                            external_graph_closure(task, external, input)?
                        )
                    } else {
                        let expression = task_call(task, &emit_expr(argument, program), program);
                        format!(
                            "blkit::compiled_graph::GraphNodeKind::Task({})",
                            graph_closure(expression, &env, &process.input, &process.input_type)
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
                            "blkit::compiled_graph::GraphNodeKind::AsyncMultiInstance {{ task: {call}, items: {items}, parallel: {parallel} }}"
                        )
                    } else {
                        let definition = program
                            .tasks
                            .iter()
                            .find(|item| item.name == *task)
                            .ok_or_else(|| format!("unknown task: {task}"))?;
                        let call = task_call(task, "typed", program);
                        format!(
                            "blkit::compiled_graph::GraphNodeKind::MultiInstance {{ task: std::sync::Arc::new(|item, _| {{ let typed: {} = serde_json::from_value(item.clone()).map_err(|e| e.to_string())?; serde_json::to_value({call}).map_err(|e| e.to_string()) }}), items: {items}, parallel: {parallel} }}",
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
                    let (kind, call) = if let Some(external) = program.external_tasks.get(task) {
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
                                task_call(task, &emit_expr(argument, program), program),
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
                        "blkit::compiled_graph::GraphNodeKind::{kind}({call}, blkit::compiled_graph::LoopPolicy {{ condition: {condition}, initial: {initial}, before: {before}, max_iterations: {max_iterations}, max_duration: {max_duration} }})"
                    )
                }
                NodeKind::Split(kind) => {
                    format!("blkit::compiled_graph::GraphNodeKind::Split({kind:?})")
                }
                NodeKind::Join { kind, split, .. } => format!(
                    "blkit::compiled_graph::GraphNodeKind::Join {{ kind: {kind:?}, split: {split:?} }}"
                ),
                NodeKind::End => "blkit::compiled_graph::GraphNodeKind::End".into(),
                NodeKind::Error => "blkit::compiled_graph::GraphNodeKind::Error".into(),
                NodeKind::Cancel => "blkit::compiled_graph::GraphNodeKind::Cancel".into(),
                NodeKind::Terminate => "blkit::compiled_graph::GraphNodeKind::Terminate".into(),
            };
            out.push_str(&format!(
                "blkit::compiled_graph::GraphNode {{ name: {:?}, kind: {kind} }},\n",
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
            out.push_str(&format!("blkit::compiled_graph::GraphLink {{ source: {:?}, target: {:?}, value: {value}, condition: {condition}, fallback: {}, label: {:?} }},\n", link.source, link.target, link.fallback, link.outcome.as_ref().or(link.label.as_ref())));
        }
        out.push_str("] },\n");
    }
    out.push_str("] }\n");
    Ok(())
}
