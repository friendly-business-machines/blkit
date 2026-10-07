use super::*;

fn bound_value(
    graph: &crate::graph::SourceGraph,
    target: &str,
    port: &str,
    program: &Program,
) -> Result<String, String> {
    bound_value_from(graph, target, port, None, program)
}

fn bound_value_from(
    graph: &crate::graph::SourceGraph,
    target: &str,
    port: &str,
    source: Option<&str>,
    program: &Program,
) -> Result<String, String> {
    let candidates: Vec<_> = graph
        .bindings
        .iter()
        .filter(|bind| {
            bind.target == target
                && bind.input == port
                && source.is_none_or(|name| name == bind.source)
        })
        .collect();
    if candidates.is_empty() {
        return Err(format!("missing binding: {target}.{port}"));
    }
    let accesses = candidates
        .iter()
        .map(|binding| {
            let origin = if program.peer_nodes.iter().any(|peer| {
                peer.name == binding.source && matches!(peer.kind, PeerKind::Start { .. })
            }) {
                "source"
            } else {
                "values"
            };
            let key = if origin == "source" {
                &binding.output
            } else {
                &binding.source
            };
            let field = if origin == "values"
                && (program.decisions.iter().any(|task| task.name == binding.source && task.outputs.len() > 1)
                    || program.peer_nodes.iter().any(|peer| peer.name == binding.source && matches!(&peer.kind, PeerKind::Subprocess { outputs, .. } if outputs.len() > 1)))
            {
                format!(".and_then(|value| value.get({:?}))", binding.output)
            } else {
                String::new()
            };
            format!("{origin}.get({key:?}){field}.cloned()")
        })
        .collect::<Vec<_>>();
    if accesses.len() == 1 {
        Ok(format!(
            "{}.ok_or(\"bound value unavailable at {target}.{port}\")?",
            accesses[0]
        ))
    } else {
        Ok(format!(
            "{{ let mut selected = None; for value in [{}] {{ if let Some(value) = value {{ if selected.replace(value).is_some() {{ return Err(\"ambiguous bound value at {target}.{port}\".into()); }} }} }} selected.ok_or(\"bound value unavailable at {target}.{port}\")? }}",
            accesses.join(", ")
        ))
    }
}

fn route_closure(expression: &Expr, program: &Program) -> Result<String, String> {
    let (expression, refs) = crate::graph::route_expression(expression, program)?;
    let mut vars = String::new();
    for (index, (name, port, ty)) in refs.iter().enumerate() {
        let origin = if program
            .peer_nodes
            .iter()
            .any(|peer| peer.name == *name && matches!(peer.kind, PeerKind::Start { .. }))
        {
            "source"
        } else {
            "values"
        };
        let key = if origin == "source" { port } else { name };
        let field = if origin == "values" && (program.decisions.iter().any(|task| task.name == *name && task.outputs.len() > 1)
            || program.peer_nodes.iter().any(|peer| peer.name == *name && matches!(&peer.kind, PeerKind::Subprocess { outputs, .. } if outputs.len() > 1))) { format!(".and_then(|value| value.get({port:?}))") } else { String::new() };
        vars.push_str(&format!("let __bl_route_{index}: {} = serde_json::from_value({origin}.get({key:?}){field}.cloned().ok_or(\"route source unavailable\")?).map_err(|e| e.to_string())?;", rust_type(ty)));
    }
    let env: HashMap<_, _> = refs
        .iter()
        .enumerate()
        .map(|(index, (_, _, ty))| (format!("__bl_route_{index}"), ty.clone()))
        .collect();
    Ok(format!(
        "std::sync::Arc::new(|source, values| {{ {vars} serde_json::to_value({}).map_err(|e| e.to_string()) }})",
        emit_typed_expr(&expression, program, &[], &env)
    ))
}

fn emit_source_graph(
    process: &crate::compiler::Process,
    program: &Program,
    out: &mut String,
) -> Result<(), String> {
    let graph = process.source_graph.as_ref().unwrap();
    let start = program
        .peer_nodes
        .iter()
        .find(|peer| {
            matches!(peer.kind, PeerKind::Start { .. })
                && graph.flows.iter().any(|(source, _)| source == &peer.name)
        })
        .ok_or("missing start event")?;
    let PeerKind::Start { outputs } = &start.kind else {
        unreachable!()
    };
    let retry = process.retry.as_ref().map_or("None".into(), |policy| format!("Some(blkit::RetryPolicy {{ max_retries: {}, retry_for: std::time::Duration::from_millis({}), retry_delay: std::time::Duration::from_millis({}), backoff: {:?} }})", policy.max_retries, policy.retry_for.as_millis(), policy.retry_delay.as_millis(), policy.backoff));
    let deadline = process.deadline.as_ref().map_or("None".into(), |policy| format!("Some(blkit::DeadlinePolicy {{ origin: {:?}, duration: std::time::Duration::from_millis({}) }})", policy.origin, policy.duration.as_millis()));
    out.push_str(&format!("blkit::compiled_graph::GraphDefinition {{ namespace: NAMESPACE, version: VERSION, name: {:?}, retry: {retry}, deadline: {deadline}, decode_input: Box::new(|value| {{ let object = value.as_object().ok_or(\"start input must be an object\")?; if object.len() != {} {{ return Err(\"invalid start input ports\".into()); }} let mut result = serde_json::Map::new();\n", process.name, outputs.len()));
    for (name, ty) in outputs {
        out.push_str(&format!("let typed: {} = serde_json::from_value(object.get({name:?}).ok_or(\"missing start input port: {name}\")?.clone()).map_err(|e| e.to_string())?; result.insert({name:?}.into(), serde_json::to_value(typed).map_err(|e| e.to_string())?);\n", rust_type(ty)));
    }
    out.push_str("Ok(serde_json::Value::Object(result)) }), nodes: vec![\n");
    let mut emitted = std::collections::HashSet::new();
    for name in std::iter::once(&start.name).chain(graph.flows.iter().map(|(_, target)| target)) {
        if !emitted.insert(name.as_str()) {
            continue;
        }
        let kind = if name == &start.name {
            "blkit::compiled_graph::GraphNodeKind::Start".to_owned()
        } else if let Some(peer) = program.peer_nodes.iter().find(|peer| peer.name == *name) {
            match &peer.kind {
                PeerKind::End { .. } => "blkit::compiled_graph::GraphNodeKind::End".into(),
                PeerKind::Split { kind } => {
                    format!("blkit::compiled_graph::GraphNodeKind::Split({kind:?})")
                }
                PeerKind::Join { kind, split, .. } => format!(
                    "blkit::compiled_graph::GraphNodeKind::Join {{ kind: {kind:?}, split: {split:?} }}"
                ),
                PeerKind::Terminal { kind } => {
                    format!("blkit::compiled_graph::GraphNodeKind::{kind}")
                }
                PeerKind::PauseFor(duration) => format!(
                    "blkit::compiled_graph::GraphNodeKind::PauseFor(std::time::Duration::from_millis({}))",
                    duration.as_millis()
                ),
                PeerKind::PauseUntil { input } => format!(
                    "blkit::compiled_graph::GraphNodeKind::PauseUntil(std::sync::Arc::new(|source, values| Ok({})))",
                    bound_value(graph, name, input, program)?
                ),
                PeerKind::Subprocess {
                    process: child,
                    inputs,
                    ..
                } => {
                    let mut fields = String::from("let mut object = serde_json::Map::new();");
                    for (port, _) in inputs {
                        fields.push_str(&format!(
                            "object.insert({port:?}.into(), {});",
                            bound_value(graph, name, port, program)?
                        ));
                    }
                    format!(
                        "blkit::compiled_graph::GraphNodeKind::Subprocess {{ process: {child:?}, input: std::sync::Arc::new(|source, values| {{ {fields} Ok(serde_json::Value::Object(object)) }}) }}"
                    )
                }
                PeerKind::Start { .. } => return Err("multiple start events".into()),
            }
        } else {
            let decision = program
                .decisions
                .iter()
                .find(|task| task.name == *name)
                .ok_or_else(|| format!("unknown decision task: {name}"))?;
            if let Some(multi) = graph
                .multi_instances
                .iter()
                .find(|multi| multi.node == *name)
            {
                let ty = &decision.inputs[0].1;
                let items = route_closure(&multi.items, program)?;
                format!(
                    "blkit::compiled_graph::GraphNodeKind::MultiInstance {{ task: std::sync::Arc::new(|item, _| {{ let typed: {} = serde_json::from_value(item.clone()).map_err(|e| e.to_string())?; serde_json::to_value(self::{}(typed)?).map_err(|e| e.to_string()) }}), items: {items}, parallel: {} }}",
                    rust_type(ty),
                    decision.name,
                    multi.parallel
                )
            } else {
                let mut args = Vec::new();
                let mut bindings = String::new();
                for (index, (port, ty)) in decision.inputs.iter().enumerate() {
                    let value = bound_value(graph, name, port, program)?;
                    let variable = format!("input_{index}");
                    bindings.push_str(&format!("let {variable}: {} = serde_json::from_value({value}).map_err(|e| e.to_string())?;", rust_type(ty)));
                    args.push(variable);
                }
                let call = format!(
                    "std::sync::Arc::new(|source, values| {{ {bindings} serde_json::to_value(self::{}({})?).map_err(|e| e.to_string()) }})",
                    decision.name,
                    args.join(", ")
                );
                if let Some(repeat) = graph.repetitions.iter().find(|repeat| repeat.node == *name) {
                    let condition = route_closure(&repeat.condition, program)?;
                    let initial = repeat
                        .initial
                        .as_ref()
                        .map(|expr| {
                            route_closure(expr, program).map(|closure| format!("Some({closure})"))
                        })
                        .transpose()?
                        .unwrap_or_else(|| "None".into());
                    let max_iterations = repeat
                        .max_iterations
                        .map_or("None".into(), |limit| format!("Some({limit})"));
                    let max_duration = repeat.max_duration.map_or("None".into(), |limit| {
                        format!(
                            "Some(std::time::Duration::from_millis({}))",
                            limit.as_millis()
                        )
                    });
                    format!(
                        "blkit::compiled_graph::GraphNodeKind::TaskLoop({call}, blkit::compiled_graph::LoopPolicy {{ condition: {condition}, initial: {initial}, before: {}, max_iterations: {max_iterations}, max_duration: {max_duration} }})",
                        repeat.before
                    )
                } else {
                    format!("blkit::compiled_graph::GraphNodeKind::Task({call})")
                }
            }
        };
        out.push_str(&format!(
            "blkit::compiled_graph::GraphNode {{ name: {name:?}, kind: {kind} }},\n"
        ));
    }
    out.push_str("], links: vec![\n");
    for (source, target) in &graph.flows {
        let value = if let Some(peer) = program.peer_nodes.iter().find(|peer| peer.name == *target)
        {
            if let PeerKind::End { inputs } = &peer.kind {
                let expression = if inputs.len() == 1 {
                    bound_value(graph, target, &inputs[0].0, program)?
                } else {
                    let mut fields = String::from("let mut object = serde_json::Map::new();");
                    for (port, _) in inputs {
                        fields.push_str(&format!(
                            "object.insert({port:?}.into(), {});",
                            bound_value(graph, target, port, program)?
                        ));
                    }
                    format!("{{ {fields} serde_json::Value::Object(object) }}")
                };
                format!("Some(std::sync::Arc::new(|source, values| {{ Ok({expression}) }}))")
            } else if let PeerKind::Join { .. } = &peer.kind {
                let binding = graph
                    .bindings
                    .iter()
                    .find(|bind| bind.target == *target && bind.source == *source)
                    .ok_or_else(|| {
                        format!("join {target} missing incoming binding from {source}")
                    })?;
                let expression =
                    bound_value_from(graph, target, &binding.input, Some(source), program)?;
                format!("Some(std::sync::Arc::new(|source, values| {{ Ok({expression}) }}))")
            } else {
                "None".into()
            }
        } else {
            "None".into()
        };
        let route = graph
            .routes
            .iter()
            .find(|route| route.source == *source && route.target == *target);
        let condition = route
            .and_then(|route| route.condition.as_ref())
            .map(|predicate| {
                route_closure(predicate, program).map(|closure| format!("Some({closure})"))
            })
            .transpose()?
            .unwrap_or_else(|| "None".into());
        let fallback = route.is_some_and(|route| route.fallback);
        let label = route.and_then(|route| route.outcome.as_deref().or(route.label.as_deref()));
        out.push_str(&format!("blkit::compiled_graph::GraphLink {{ source: {source:?}, target: {target:?}, value: {value}, condition: {condition}, fallback: {fallback}, label: {label:?} }},\n"));
    }
    out.push_str("] },\n");
    Ok(())
}

pub(super) fn emit_named_graphs(program: &Program, out: &mut String) -> Result<(), String> {
    out.push_str("#[allow(unused_variables, unused_parens)]\npub fn named_graph_definitions() -> Vec<blkit::compiled_graph::GraphDefinition> { vec![\n");
    for process in program
        .processes
        .iter()
        .filter(|item| item.named_graph.is_some())
    {
        let graph = process.named_graph.as_ref().unwrap();
        let scopes = semantic::named_scopes(graph, process, program)?;
        let emit =
            |expr: &Expr, env: &HashMap<String, Type>| emit_typed_expr(expr, program, &[], env);
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
                            emit(argument, &env),
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
                        emit(expression, &scopes[node.name.as_str()]),
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
                    let expression = format!("self::{model}({})?", emit(argument, &env));
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
                            emit(argument, &env),
                            &env,
                            &process.input,
                            &process.input_type,
                        );
                        format!(
                            "blkit::compiled_graph::GraphNodeKind::AsyncTask({})",
                            external_graph_closure(task, external, input)?
                        )
                    } else {
                        let expression = task_call(task, &emit(argument, &env), program);
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
                    let items =
                        graph_closure(emit(items, &env), &env, &process.input, &process.input_type);
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
                            emit(argument, argument_env),
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
                                task_call(task, &emit(argument, argument_env), program),
                                argument_env,
                                &process.input,
                                &process.input_type,
                            ),
                        )
                    };
                    let condition = graph_closure(
                        emit(condition, &scopes[node.name.as_str()]),
                        &scopes[node.name.as_str()],
                        &process.input,
                        &process.input_type,
                    );
                    let initial = initial.as_ref().map_or("None".into(), |value| {
                        format!(
                            "Some({})",
                            graph_closure(
                                emit(value, &initial_env),
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
                    graph_closure(emit(expr, env), env, &process.input, &process.input_type)
                )
            });
            let condition = link.condition.as_ref().map_or("None".into(), |expr| {
                format!(
                    "Some({})",
                    graph_closure(emit(expr, env), env, &process.input, &process.input_type)
                )
            });
            out.push_str(&format!("blkit::compiled_graph::GraphLink {{ source: {:?}, target: {:?}, value: {value}, condition: {condition}, fallback: {}, label: {:?} }},\n", link.source, link.target, link.fallback, link.outcome.as_ref().or(link.label.as_ref())));
        }
        out.push_str("] },\n");
    }
    for process in program
        .processes
        .iter()
        .filter(|item| item.source_graph.is_some())
    {
        emit_source_graph(process, program, out)?;
    }
    out.push_str("] }\n");
    Ok(())
}
