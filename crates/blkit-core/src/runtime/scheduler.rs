use super::*;

#[cfg(feature = "local-persistence")]
pub(super) async fn run_named(
    graph: Arc<GraphDefinition>,
    instance: Instance,
    permits: Arc<Semaphore>,
    context: Context,
    definitions: Arc<NamedRegistry>,
) {
    let Some(mut checkpoint) = instance.checkpoint else {
        return;
    };
    let mut next_eligible_at = instance.next_eligible_at;
    let mut wake_at_ms = instance.wake_at_ms;
    loop {
        if let Some(wake) = wake_at_ms.take() {
            tokio::time::sleep(Duration::from_millis(
                wake.saturating_sub(crate::store::now_ms()).max(0) as u64,
            ))
            .await;
            let mut status = context.state.lock().await;
            let Ok(Some(saved)) = context.store.as_ref().unwrap().get(&instance.id).await else {
                break;
            };
            if saved.status != "waiting" {
                break;
            }
            let Some(mut next) = saved.checkpoint else {
                break;
            };
            if graph
                .resume_due(&instance.input, &mut next, crate::store::now_ms())
                .is_err()
            {
                break;
            }
            let Ok(Some(resumed)) = context
                .store
                .as_ref()
                .unwrap()
                .resume_wait(&instance.id, &next)
                .await
            else {
                break;
            };
            *status = Running {
                status: resumed,
                next: status.next,
                in_flight: HashMap::new(),
            };
            checkpoint = next;
        }
        if let Some(next) = next_eligible_at.take() {
            tokio::time::sleep(Duration::from_millis(
                next.saturating_sub(crate::store::now_ms()).max(0) as u64,
            ))
            .await;
            let Ok(Some(saved)) = context.store.as_ref().unwrap().get(&instance.id).await else {
                break;
            };
            if saved.status != "retry-waiting" {
                break;
            }
            let Some(state) = saved.checkpoint else {
                break;
            };
            checkpoint = state;
        }
        match execute_named(
            &graph,
            &instance.input,
            checkpoint.clone(),
            &permits,
            &context,
            &definitions,
        )
        .await
        {
            Ok(NamedOutcome::Completed(value)) => {
                let _ = context.complete(value).await;
                break;
            }
            Ok(NamedOutcome::Terminal(terminal)) => {
                let _ = context.named_terminal(&terminal).await;
                break;
            }
            Ok(NamedOutcome::Waiting(wake)) => {
                context.state.lock().await.status = "waiting";
                let Ok(Some(saved)) = context.store.as_ref().unwrap().get(&instance.id).await
                else {
                    break;
                };
                let Some(next) = saved.checkpoint else {
                    break;
                };
                checkpoint = next;
                wake_at_ms = Some(wake);
            }
            Err(error) => {
                let Ok(Some(next)) = context.fail_attempt(graph.retry.as_ref(), &error).await
                else {
                    break;
                };
                next_eligible_at = Some(next);
            }
        }
    }
}

async fn save_wait(
    context: &Context,
    checkpoint: &crate::compiled_graph::GraphCheckpoint,
    wake: i64,
) -> Result<(), String> {
    context.wait_until(checkpoint, wake).await
}

struct ReadyWork {
    path: Vec<u64>,
    activation: u64,
    input: Value,
    source: Value,
    values: Values,
    call: Option<Evaluate>,
    async_call: Option<AsyncEvaluate>,
    cancel: Cancel,
}

fn collect_ready(
    graph: &GraphDefinition,
    input: &Value,
    state: &GraphCheckpoint,
    path: &[u64],
    definitions: &NamedRegistry,
    work: &mut Vec<ReadyWork>,
) -> Result<(), String> {
    for (activation, name) in graph.ready_activations(state) {
        let node = graph
            .nodes
            .iter()
            .find(|node| node.name == name)
            .ok_or("unknown task node")?;
        if let GraphNodeKind::Subprocess { process, .. } = &node.kind {
            let child = compiled_child(graph, process, definitions)?;
            let nested = state
                .children
                .get(&activation)
                .ok_or("missing child activation")?;
            if nested.next_eligible_at_ms.is_some() {
                continue;
            }
            let mut child_path = path.to_vec();
            child_path.push(activation);
            collect_ready(
                child,
                &nested.input,
                &nested.checkpoint,
                &child_path,
                definitions,
                work,
            )?;
            continue;
        }
        let (call, async_call, cancel) = match &node.kind {
            GraphNodeKind::Task(call) | GraphNodeKind::TaskLoop(call, _) => {
                (Some(call.clone()), None, Arc::new(|| {}) as Cancel)
            }
            GraphNodeKind::MultiInstance { task, .. } => {
                (Some(task.clone()), None, Arc::new(|| {}) as Cancel)
            }
            GraphNodeKind::TaskWithCancel(call, cancel) => {
                (Some(call.clone()), None, cancel.clone())
            }
            GraphNodeKind::AsyncTask(call)
            | GraphNodeKind::AsyncTaskLoop(call, _)
            | GraphNodeKind::AsyncMultiInstance { task: call, .. } => {
                (None, Some(call.clone()), Arc::new(|| {}) as Cancel)
            }
            _ => return Err("ready node is not a task".into()),
        };
        work.push(ReadyWork {
            path: path.to_vec(),
            activation,
            input: input.clone(),
            source: graph.activation_input(state, activation, input)?,
            values: graph.activation_values(state, activation)?,
            call,
            async_call,
            cancel,
        });
    }
    Ok(())
}

pub(super) async fn execute_named(
    graph: &GraphDefinition,
    input: &Value,
    mut checkpoint: crate::compiled_graph::GraphCheckpoint,
    permits: &Arc<Semaphore>,
    context: &Context,
    definitions: &NamedRegistry,
) -> Result<NamedOutcome, String> {
    checkpoint.ensure_supported()?;
    let mut tasks = JoinSet::new();
    let mut in_flight = HashSet::new();
    let mut scopes = HashMap::new();
    let mut task_ids = HashMap::new();
    'drive: loop {
        graph.check_loop_bounds(&mut checkpoint, crate::store::now_ms())?;
        let mut next = checkpoint.clone();
        let mut ended = Vec::new();
        if resolve_children(graph, input, &mut next, definitions, &[], &mut ended)? {
            signal_child_scopes(context, &ended, &mut scopes, &mut in_flight).await;
            context.checkpoint(&next).await?;
            checkpoint = next;
            continue;
        }
        if let Some(terminal) = &checkpoint.terminal {
            return Ok(NamedOutcome::Terminal(terminal.clone()));
        }
        if let Some(result) = &checkpoint.outcome {
            return Ok(NamedOutcome::Completed(result.clone()));
        }
        if graph
            .waiting_until(&checkpoint)
            .is_some_and(|at| at <= crate::store::now_ms())
        {
            let mut next = checkpoint.clone();
            graph.resume_due(input, &mut next, crate::store::now_ms())?;
            context.checkpoint(&next).await?;
            checkpoint = next;
            continue;
        }
        let mut work = Vec::new();
        collect_ready(graph, input, &checkpoint, &[], definitions, &mut work)?;
        if tasks.is_empty()
            && work.is_empty()
            && !nested_pending(graph, &checkpoint, definitions)?
            && let Some(wake) = nested_wake(graph, &checkpoint, definitions)?
        {
            save_wait(context, &checkpoint, wake).await?;
            return Ok(NamedOutcome::Waiting(wake));
        }
        for ReadyWork {
            path,
            activation,
            input: child_input,
            source,
            values,
            call,
            async_call,
            cancel,
        } in work
        {
            if in_flight.contains(&(path.clone(), activation)) {
                continue;
            }
            let permit = match permits.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(tokio::sync::TryAcquireError::NoPermits)
                    if tasks.is_empty() && !nested_pending(graph, &checkpoint, definitions)? =>
                {
                    if let Some(wake) = nested_wake(graph, &checkpoint, definitions)? {
                        tokio::select! {
                            permit = permits.clone().acquire_owned() => permit.map_err(|error| error.to_string())?,
                            _ = tokio::time::sleep(Duration::from_millis(wake.saturating_sub(crate::store::now_ms()) as u64)) => continue 'drive,
                        }
                    } else {
                        permits
                            .clone()
                            .acquire_owned()
                            .await
                            .map_err(|error| error.to_string())?
                    }
                }
                Err(tokio::sync::TryAcquireError::NoPermits) => break,
                Err(error) => return Err(error.to_string()),
            };
            let mut state = context.state.lock().await;
            if !matches!(state.status, "pending" | "running" | "retry-waiting") {
                return Err("instance cancelled".into());
            }
            if matches!(state.status, "pending" | "retry-waiting") {
                #[cfg(feature = "local-persistence")]
                context.begin_attempt().await?;
                state.status = "running";
            }
            let key = state.next;
            state.next += 1;
            state.in_flight.insert(key, cancel);
            drop(state);
            in_flight.insert((path.clone(), activation));
            scopes.insert(key, (path.clone(), activation));
            if let Some(call) = async_call {
                let handle = tasks.spawn(async move {
                    let _permit = permit;
                    (
                        key,
                        path,
                        activation,
                        child_input,
                        call(source, values).await,
                    )
                });
                task_ids.insert(handle.id(), key);
                let abort: Cancel = Arc::new(move || handle.abort());
                let mut state = context.state.lock().await;
                if state.status == "running" {
                    state.in_flight.insert(key, abort);
                } else {
                    abort();
                }
            } else {
                let call = call.ok_or("missing task call")?;
                let handle = tasks.spawn_blocking(move || {
                    let _permit = permit;
                    (key, path, activation, child_input, call(&source, &values))
                });
                task_ids.insert(handle.id(), key);
            }
        }
        let completed = if nested_pending(graph, &checkpoint, definitions)? {
            if let Some(done) = tasks.try_join_next_with_id() {
                done
            } else {
                let mut state = context.state.lock().await;
                if !matches!(state.status, "pending" | "running" | "retry-waiting") {
                    return Err("instance cancelled".into());
                }
                if matches!(state.status, "pending" | "retry-waiting") {
                    #[cfg(feature = "local-persistence")]
                    context.begin_attempt().await?;
                    state.status = "running";
                }
                let mut next = checkpoint.clone();
                resume_nested_pending(graph, input, &mut next, definitions)?;
                context.checkpoint(&next).await?;
                checkpoint = next;
                drop(state);
                tokio::task::yield_now().await;
                continue;
            }
        } else if let Some(wake) = nested_wake(graph, &checkpoint, definitions)? {
            tokio::select! {
                done = tasks.join_next_with_id() => done.ok_or("graph has no ready tasks")?,
                _ = tokio::time::sleep(Duration::from_millis(wake.saturating_sub(crate::store::now_ms()) as u64)) => continue 'drive,
            }
        } else {
            tasks
                .join_next_with_id()
                .await
                .ok_or("graph has no ready tasks")?
        };
        let (task_id, (key, path, activation, child_input, result)) = match completed {
            Ok(completed) => completed,
            Err(error) => {
                let key = task_ids.remove(&error.id());
                if key.is_some_and(|key| !scopes.contains_key(&key)) {
                    continue;
                }
                return Err(error.to_string());
            }
        };
        task_ids.remove(&task_id);
        if scopes.remove(&key).is_none() {
            continue;
        }
        in_flight.remove(&(path.clone(), activation));
        let mut state = context.state.lock().await;
        state.in_flight.remove(&key);
        if state.status != "running" {
            return Err("instance cancelled".into());
        }
        drop(state);
        let mut current_graph = graph;
        let mut current_state = &checkpoint;
        let mut graphs = vec![graph];
        for id in &path {
            let child = current_state
                .children
                .get(id)
                .ok_or("missing child activation")?;
            current_graph = compiled_child(current_graph, &child.process, definitions)?;
            graphs.push(current_graph);
            current_state = &child.checkpoint;
        }
        let result = match result {
            Ok(value) => value,
            Err(error) => {
                for depth in (1..=path.len()).rev() {
                    let Some(policy) = graphs[depth].retry.as_ref() else {
                        continue;
                    };
                    let mut next = checkpoint.clone();
                    let child = nested_state(&mut next, &path[..depth - 1])?
                        .children
                        .get_mut(&path[depth - 1])
                        .ok_or("missing child activation")?;
                    let now = crate::store::now_ms();
                    let first = *child.first_failure_at_ms.get_or_insert(now);
                    if let Some(wake) = next_retry_at(policy, child.attempt, first, now) {
                        child.next_eligible_at_ms = Some(wake);
                        signal_child_scopes(
                            context,
                            &[path[..depth].to_vec()],
                            &mut scopes,
                            &mut in_flight,
                        )
                        .await;
                        context.checkpoint(&next).await?;
                        checkpoint = next;
                        continue 'drive;
                    }
                }
                return Err(error);
            }
        };
        let mut next = checkpoint.clone();
        current_graph.complete_activation(
            &child_input,
            nested_state(&mut next, &path)?,
            activation,
            result,
        )?;
        let mut ended = Vec::new();
        let advanced = resolve_children(graph, input, &mut next, definitions, &[], &mut ended)?;
        signal_child_scopes(context, &ended, &mut scopes, &mut in_flight).await;
        if tasks.is_empty() && !advanced && !nested_pending(graph, &next, definitions)? {
            let mut work = Vec::new();
            collect_ready(graph, input, &next, &[], definitions, &mut work)?;
            if work.is_empty()
                && let Some(wake) = nested_wake(graph, &next, definitions)?
            {
                save_wait(context, &next, wake).await?;
                return Ok(NamedOutcome::Waiting(wake));
            }
        }
        context.checkpoint(&next).await?;
        checkpoint = next;
    }
}
