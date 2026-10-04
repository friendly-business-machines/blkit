use super::*;

pub(super) fn compiled_child<'a>(
    graph: &GraphDefinition,
    name: &str,
    definitions: &'a NamedRegistry,
) -> Result<&'a Arc<GraphDefinition>, String> {
    definitions
        .get(&(graph.namespace.into(), graph.version.into(), name.into()))
        .ok_or_else(|| format!("missing compiled subprocess: {name}"))
}

pub(super) fn resolve_children(
    graph: &GraphDefinition,
    input: &Value,
    state: &mut GraphCheckpoint,
    definitions: &NamedRegistry,
    path: &[u64],
    ended: &mut Vec<Vec<u64>>,
) -> Result<bool, String> {
    let mut changed = false;
    for (id, name) in graph
        .ready_activations(state)
        .into_iter()
        .map(|(id, name)| (id, name.to_owned()))
        .collect::<Vec<_>>()
    {
        if state.terminal.is_some() || state.outcome.is_some() {
            break;
        }
        let Some(GraphNodeKind::Subprocess {
            process,
            input: evaluate,
        }) = graph
            .nodes
            .iter()
            .find(|node| node.name == name)
            .map(|node| &node.kind)
        else {
            continue;
        };
        let child = compiled_child(graph, process, definitions)?;
        if !state.children.contains_key(&id) {
            let source = graph.activation_input(state, id, input)?;
            let values = graph.activation_values(state, id)?;
            let child_input = (child.decode_input)(evaluate(&source, &values)?)?;
            let now = crate::store::now_ms();
            let entered = graph.activation_started_at(state, id)?;
            let entered = if entered == 0 { now } else { entered };
            let deadline_at_ms = child
                .deadline
                .as_ref()
                .map(|policy| {
                    let duration = i64::try_from(policy.duration.as_millis())
                        .map_err(|_| "deadline overflow")?;
                    let origin = if policy.origin == "queued" {
                        entered
                    } else {
                        now
                    };
                    origin.checked_add(duration).ok_or("deadline overflow")
                })
                .transpose()?;
            state.children.insert(
                id,
                ChildActivation {
                    process: (*process).into(),
                    checkpoint: Box::new(child.checkpoint(&child_input)?),
                    input: child_input,
                    entered_at_ms: entered,
                    first_claimed_at_ms: Some(now),
                    attempt: 1,
                    first_failure_at_ms: None,
                    next_eligible_at_ms: None,
                    deadline_at_ms,
                },
            );
            changed = true;
        }
        let nested = state
            .children
            .get_mut(&id)
            .ok_or("missing child activation")?;
        let now = crate::store::now_ms();
        nested.checkpoint.ensure_supported()?;
        if nested.deadline_at_ms.is_some_and(|at| at <= now) {
            nested.checkpoint.outcome = None;
            nested.checkpoint.terminal = Some(GraphTerminal::Error("timeout".into()));
        } else {
            child.check_loop_bounds(&mut nested.checkpoint, now)?;
        }
        if nested.next_eligible_at_ms.is_some_and(|at| at <= now) {
            nested.next_eligible_at_ms = None;
            nested.attempt = nested.attempt.saturating_add(1);
            changed = true;
        }
        if child
            .waiting_until(&nested.checkpoint)
            .is_some_and(|wake| wake <= now)
        {
            child.resume_due(&nested.input, &mut nested.checkpoint, now)?;
            changed = true;
        }
        let mut child_path = path.to_vec();
        child_path.push(id);
        changed |= resolve_children(
            child,
            &nested.input,
            &mut nested.checkpoint,
            definitions,
            &child_path,
            ended,
        )?;
        let outcome = nested.checkpoint.outcome.clone();
        let terminal = nested.checkpoint.terminal.clone();
        if let Some(value) = outcome {
            state.children.remove(&id);
            graph.complete_activation(input, state, id, value)?;
            changed = true;
        } else if let Some(terminal) = terminal {
            ended.push(child_path);
            graph.complete_subprocess_terminal(input, state, id, terminal)?;
            changed = true;
        }
    }
    Ok(changed)
}

pub(super) async fn signal_child_scopes(
    context: &Context,
    ended: &[Vec<u64>],
    scopes: &mut HashMap<usize, (Vec<u64>, u64)>,
    in_flight: &mut HashSet<(Vec<u64>, u64)>,
) {
    if ended.is_empty() {
        return;
    }
    let mut state = context.state.lock().await;
    let keys: Vec<_> = scopes
        .iter()
        .filter(|(_, (path, _))| ended.iter().any(|scope| path.starts_with(scope)))
        .map(|(key, _)| *key)
        .collect();
    let mut hooks = Vec::new();
    for key in keys {
        if let Some((path, id)) = scopes.remove(&key) {
            in_flight.remove(&(path, id));
        }
        if let Some(hook) = state.in_flight.remove(&key) {
            hooks.push(hook);
        }
    }
    drop(state);
    for hook in hooks {
        hook();
    }
}

pub(super) fn nested_wake(
    graph: &GraphDefinition,
    state: &GraphCheckpoint,
    definitions: &NamedRegistry,
) -> Result<Option<i64>, String> {
    let mut next = graph.waiting_until(state);
    for child in state.children.values() {
        for at in [child.deadline_at_ms, child.next_eligible_at_ms]
            .into_iter()
            .flatten()
        {
            next = Some(next.map_or(at, |prior| prior.min(at)));
        }
        let definition = compiled_child(graph, &child.process, definitions)?;
        if let Some(at) = nested_wake(definition, &child.checkpoint, definitions)? {
            next = Some(next.map_or(at, |prior| prior.min(at)));
        }
    }
    Ok(next)
}

pub(super) fn nested_pending(
    graph: &GraphDefinition,
    state: &GraphCheckpoint,
    definitions: &NamedRegistry,
) -> Result<bool, String> {
    if graph.has_pending(state) {
        return Ok(true);
    }
    for child in state.children.values() {
        let definition = compiled_child(graph, &child.process, definitions)?;
        if nested_pending(definition, &child.checkpoint, definitions)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn resume_nested_pending(
    graph: &GraphDefinition,
    input: &Value,
    state: &mut GraphCheckpoint,
    definitions: &NamedRegistry,
) -> Result<(), String> {
    if graph.has_pending(state) {
        graph.resume_pending(input, state)?;
    }
    for child in state.children.values_mut() {
        let definition = compiled_child(graph, &child.process, definitions)?;
        resume_nested_pending(definition, &child.input, &mut child.checkpoint, definitions)?;
    }
    Ok(())
}

pub(super) fn nested_state<'a>(
    state: &'a mut GraphCheckpoint,
    path: &[u64],
) -> Result<&'a mut GraphCheckpoint, String> {
    if let Some((first, rest)) = path.split_first() {
        nested_state(
            &mut state
                .children
                .get_mut(first)
                .ok_or("missing child activation")?
                .checkpoint,
            rest,
        )
    } else {
        Ok(state)
    }
}
