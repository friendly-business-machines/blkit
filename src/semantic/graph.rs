use super::*;

fn reaches(graph: &SourceGraph, first: &str, target: &str) -> bool {
    let mut seen = HashSet::from([first.to_owned()]);
    loop {
        let count = seen.len();
        for (source, next) in &graph.flows {
            if seen.contains(source) {
                seen.insert(next.clone());
            }
        }
        if seen.len() == count {
            break;
        }
    }
    seen.contains(target)
}

fn check_route_type(
    expression: &Expr,
    at: &str,
    expected: &Type,
    program: &Program,
    nodes: &HashSet<&str>,
    dominators: &HashMap<&str, HashSet<&str>>,
) -> Result<(), String> {
    let (expression, refs) = crate::graph::route_expression(expression, program)?;
    let mut env = HashMap::new();
    for (index, (source, _, ty)) in refs.iter().enumerate() {
        if !nodes.contains(source.as_str()) || !dominators[at].contains(source.as_str()) {
            return Err(format!("value unavailable at {at}"));
        }
        env.insert(format!("__bl_route_{index}"), ty.clone());
    }
    let actual = infer_with(&expression, Some(expected), &env, program, &[])?;
    if &actual != expected {
        return Err(format!(
            "type mismatch at {at}: expected {expected}, got {actual}"
        ));
    }
    Ok(())
}

pub(super) fn check_source_graph(
    graph: &SourceGraph,
    program: &Program,
    has_deadline: bool,
) -> Result<(), String> {
    let mut nodes = HashSet::new();
    for (source, target) in &graph.flows {
        nodes.insert(source.as_str());
        nodes.insert(target.as_str());
    }
    let start_nodes: Vec<_> = program
        .peer_nodes
        .iter()
        .filter(|node| {
            nodes.contains(node.name.as_str()) && matches!(node.kind, PeerKind::Start { .. })
        })
        .collect();
    if start_nodes.len() != 1 {
        return Err("process requires exactly one start event".into());
    }
    let starts = start_nodes[0];
    let find = |name: &str| {
        program.peer_nodes.iter().any(|node| node.name == name)
            || program.decisions.iter().any(|task| task.name == name)
    };
    for (source, target) in &graph.flows {
        if !find(source) || !find(target) {
            return Err(format!("unknown flow node: {source} -> {target}"));
        }
        if program.peer_nodes.iter().any(|peer| {
            peer.name == *source
                && matches!(peer.kind, PeerKind::End { .. } | PeerKind::Terminal { .. })
        }) || target == &starts.name
        {
            return Err(format!("invalid flow: {source} -> {target}"));
        }
    }
    if !graph.flows.iter().any(|(_, target)| {
        program.peer_nodes.iter().any(|peer| {
            peer.name == *target
                && matches!(peer.kind, PeerKind::End { .. } | PeerKind::Terminal { .. })
        })
    }) {
        return Err("process requires an end event or exceptional terminal".into());
    }
    let mut reachable = HashSet::from([starts.name.as_str()]);
    loop {
        let count = reachable.len();
        for (source, target) in &graph.flows {
            if reachable.contains(source.as_str()) {
                reachable.insert(target);
            }
        }
        if reachable.len() == count {
            break;
        }
    }
    if let Some(node) = nodes.iter().find(|node| !reachable.contains(**node)) {
        return Err(format!("unreachable node: {node}"));
    }
    // ponytail: This scans edges per node; use DFS if process maps grow large.
    if !has_deadline
        && graph
            .flows
            .iter()
            .any(|(source, next)| reaches(graph, next, source))
    {
        return Err("process cycle requires deadline".into());
    }
    for &node in &nodes {
        let outgoing = graph
            .flows
            .iter()
            .filter(|(source, _)| source == node)
            .count();
        let peer = program.peer_nodes.iter().find(|peer| peer.name == node);
        if outgoing == 0
            && !peer.is_some_and(|peer| {
                matches!(peer.kind, PeerKind::End { .. } | PeerKind::Terminal { .. })
            })
        {
            return Err(format!("dead end at node: {node}"));
        }
        if outgoing > 1
            && !peer.is_some_and(|peer| {
                matches!(
                    peer.kind,
                    PeerKind::Split { .. } | PeerKind::Subprocess { .. }
                )
            })
        {
            return Err(format!("node {node} needs a split for multiple routes"));
        }
        if !program.peer_nodes.iter().any(|peer| {
            matches!(peer.kind, PeerKind::End { .. } | PeerKind::Terminal { .. })
                && nodes.contains(peer.name.as_str())
                && reaches(graph, node, &peer.name)
        }) {
            return Err(format!("no exit from node: {node}"));
        }
    }
    let mut end_shape: Option<&Vec<(String, Type)>> = None;
    for peer in &program.peer_nodes {
        if !nodes.contains(peer.name.as_str()) {
            continue;
        }
        if let PeerKind::End { inputs } = &peer.kind {
            if end_shape.is_some_and(|shape| {
                shape.len() != inputs.len() || !shape.iter().all(|port| inputs.contains(port))
            }) {
                return Err("incompatible end event port shape".into());
            }
            end_shape = Some(inputs);
        }
    }
    let mut dominators: HashMap<&str, HashSet<&str>> = nodes
        .iter()
        .map(|node| {
            (
                *node,
                if *node == starts.name {
                    HashSet::from([*node])
                } else {
                    nodes.clone()
                },
            )
        })
        .collect();
    loop {
        let mut changed = false;
        for &node in &nodes {
            if node == starts.name {
                continue;
            }
            let mut incoming = graph
                .flows
                .iter()
                .filter(|(_, target)| target == node)
                .map(|(source, _)| source.as_str());
            let Some(first) = incoming.next() else {
                continue;
            };
            let mut common = dominators[first].clone();
            for parent in incoming {
                common.retain(|entry| dominators[parent].contains(entry));
            }
            common.insert(node);
            if dominators[node] != common {
                dominators.insert(node, common);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for repeat in &graph.repetitions {
        let task = program
            .decisions
            .iter()
            .find(|task| task.name == repeat.node)
            .filter(|_| nodes.contains(repeat.node.as_str()))
            .ok_or_else(|| format!("unknown repeated decision_task: {}", repeat.node))?;
        if graph
            .multi_instances
            .iter()
            .any(|multi| multi.node == repeat.node)
        {
            return Err(format!(
                "decision_task {} cannot repeat and run as multi_instance",
                repeat.node
            ));
        }
        check_route_type(
            &repeat.condition,
            &repeat.node,
            &Type::Named("Bool".into()),
            program,
            &nodes,
            &dominators,
        )?;
        if let Some(initial) = &repeat.initial {
            check_route_type(
                initial,
                &repeat.node,
                &task.output,
                program,
                &nodes,
                &dominators,
            )?;
        }
    }
    for multi in &graph.multi_instances {
        let task = program
            .decisions
            .iter()
            .find(|task| task.name == multi.node)
            .filter(|_| nodes.contains(multi.node.as_str()))
            .ok_or_else(|| format!("unknown multi_instance decision_task: {}", multi.node))?;
        let [(_, input)] = task.inputs.as_slice() else {
            return Err(format!(
                "multi_instance {} requires one task input",
                multi.node
            ));
        };
        if task.outputs.len() != 1 {
            return Err(format!(
                "multi_instance {} requires one task output",
                multi.node
            ));
        }
        check_route_type(
            &multi.items,
            &multi.node,
            &Type::Generic("List".into(), Box::new(input.clone())),
            program,
            &nodes,
            &dominators,
        )?;
    }
    let exclusive = |target: &str, sources: &[&str]| {
        if sources.len() < 2 || sources.iter().collect::<HashSet<_>>().len() != sources.len() {
            return false;
        }
        program.peer_nodes.iter().any(|peer| {
            if !matches!(peer.kind, PeerKind::Split { kind: "xor" })
                || !dominators[target].contains(peer.name.as_str())
            {
                return false;
            }
            let branches: Vec<_> = graph
                .flows
                .iter()
                .filter(|(from, _)| from == &peer.name)
                .map(|(_, to)| to.as_str())
                .collect();
            let mut used = HashSet::new();
            for source in sources {
                let mut selected = None;
                for (index, branch) in branches.iter().enumerate() {
                    if reaches(graph, branch, source) && selected.replace(index).is_some() {
                        return false;
                    }
                }
                if !selected.is_some_and(|index| used.insert(index)) {
                    return false;
                }
            }
            true
        })
    };
    for peer in &program.peer_nodes {
        if !nodes.contains(peer.name.as_str()) {
            continue;
        }
        match &peer.kind {
            PeerKind::Split { kind } => {
                let outgoing: Vec<_> = graph
                    .flows
                    .iter()
                    .filter(|(source, _)| source == &peer.name)
                    .collect();
                if outgoing.len() < 2 {
                    return Err(format!("split {} requires branches", peer.name));
                }
                let mut labels = HashSet::new();
                let mut fallback = 0;
                for (_, target) in outgoing {
                    let route = graph
                        .routes
                        .iter()
                        .find(|route| route.source == peer.name && route.target == *target);
                    if *kind == "and" {
                        let label = route
                            .and_then(|route| route.label.as_deref())
                            .ok_or("AND branch needs label")?;
                        if !labels.insert(label) {
                            return Err("duplicate AND branch label".into());
                        }
                    } else if let Some(route) = route {
                        if route.fallback {
                            fallback += 1;
                        }
                        if route.condition.is_none() && !route.fallback {
                            return Err("conditional branch needs when or else".into());
                        }
                    } else {
                        return Err("conditional branch needs when or else".into());
                    }
                }
                if *kind != "and" && fallback != 1 {
                    return Err("conditional split needs one else branch".into());
                }
            }
            PeerKind::PauseUntil { .. } => {}
            PeerKind::Subprocess {
                process: child,
                inputs,
                outputs,
            } => {
                let mut outcomes = HashSet::new();
                let mut success = 0;
                for (_, target) in graph
                    .flows
                    .iter()
                    .filter(|(source, _)| source == &peer.name)
                {
                    let outcome = graph
                        .routes
                        .iter()
                        .find(|route| route.source == peer.name && route.target == *target)
                        .and_then(|route| route.outcome.as_deref());
                    if let Some(outcome) = outcome {
                        if !outcomes.insert(outcome) {
                            return Err(format!("duplicate subprocess outcome flow: {outcome}"));
                        }
                    } else {
                        success += 1;
                    }
                }
                if success != 1 {
                    return Err(format!(
                        "subprocess {} requires exactly one success flow",
                        peer.name
                    ));
                }
                let process = program
                    .processes
                    .iter()
                    .find(|process| process.name == *child)
                    .ok_or_else(|| format!("unknown subprocess: {child}"))?;
                let child_graph = process
                    .source_graph
                    .as_ref()
                    .ok_or_else(|| format!("subprocess {child} requires braced process"))?;
                let child_start = program
                    .peer_nodes
                    .iter()
                    .find(|node| {
                        matches!(node.kind, PeerKind::Start { .. })
                            && child_graph
                                .flows
                                .iter()
                                .any(|(source, _)| source == &node.name)
                    })
                    .ok_or("subprocess child missing start")?;
                if !matches!(&child_start.kind, PeerKind::Start { outputs } if outputs.len() == inputs.len() && outputs.iter().all(|port| inputs.contains(port)))
                {
                    return Err(format!("subprocess {child} input port shape mismatch"));
                }
                for end in program.peer_nodes.iter().filter(|node| {
                    child_graph
                        .flows
                        .iter()
                        .any(|(_, target)| target == &node.name)
                }) {
                    if let PeerKind::End {
                        inputs: child_outputs,
                    } = &end.kind
                        && (child_outputs.len() != outputs.len()
                            || !child_outputs.iter().all(|port| outputs.contains(port)))
                    {
                        return Err(format!("subprocess {child} output port shape mismatch"));
                    }
                }
            }
            PeerKind::Join {
                kind,
                split,
                inputs,
                outputs,
            } => {
                let source = program
                    .peer_nodes
                    .iter()
                    .find(|node| node.name == *split)
                    .ok_or_else(|| format!("unknown split: {split}"))?;
                if !matches!(source.kind, PeerKind::Split { kind: other } if other == *kind)
                    || !dominators[peer.name.as_str()].contains(split.as_str())
                {
                    return Err(format!("join {} has incompatible split", peer.name));
                }
                let shape = match (*kind, inputs.as_slice(), outputs.as_slice()) {
                    ("xor", [(_, input)], [(_, output)]) => input == output,
                    ("or", [(_, input)], [(_, Type::Generic(list, output))]) => {
                        list == "List" && input == output.as_ref()
                    }
                    ("and", _, [(_, Type::Named(record))]) => program
                        .records
                        .iter()
                        .any(|item| item.name == *record && item.fields == *inputs),
                    _ => false,
                };
                if !shape {
                    return Err(format!(
                        "join {} input/output port shape mismatch",
                        peer.name
                    ));
                }
                for (_, branch) in graph.flows.iter().filter(|(from, _)| from == split) {
                    if !reaches(graph, branch, &peer.name)
                        && !program.peer_nodes.iter().any(|node| {
                            matches!(node.kind, PeerKind::Terminal { .. })
                                && reaches(graph, branch, &node.name)
                        })
                    {
                        return Err(format!("join {} missing split branch", peer.name));
                    }
                }
                for (branch, _) in graph
                    .flows
                    .iter()
                    .filter(|(_, target)| target == &peer.name)
                {
                    let matches: Vec<_> = graph
                        .bindings
                        .iter()
                        .filter(|binding| binding.source == *branch && binding.target == peer.name)
                        .collect();
                    if matches.len() != 1 {
                        return Err(format!(
                            "join {} requires one binding per incoming branch",
                            peer.name
                        ));
                    }
                    if *kind == "and" {
                        let routes: Vec<_> = graph
                            .flows
                            .iter()
                            .filter(|(from, first)| from == split && reaches(graph, first, branch))
                            .collect();
                        let label = routes
                            .first()
                            .and_then(|(from, first)| {
                                graph
                                    .routes
                                    .iter()
                                    .find(|route| route.source == *from && route.target == *first)
                            })
                            .and_then(|route| route.label.as_deref());
                        if routes.len() != 1 || label != Some(matches[0].input.as_str()) {
                            return Err(format!(
                                "AND branch label does not match join input at {}",
                                peer.name
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for route in &graph.routes {
        let split = program
            .peer_nodes
            .iter()
            .find(|peer| peer.name == route.source);
        if route.outcome.is_some() {
            if !split.is_some_and(|peer| matches!(peer.kind, PeerKind::Subprocess { .. })) {
                return Err("outcome flow requires subprocess".into());
            }
            continue;
        }
        if !split.is_some_and(|peer| matches!(peer.kind, PeerKind::Split { .. })) {
            return Err("flow annotation requires split gateway".into());
        }
        if let Some(condition) = &route.condition {
            let (expression, refs) = crate::graph::route_expression(condition, program)?;
            let mut env = HashMap::new();
            for (index, (node, _, ty)) in refs.iter().enumerate() {
                if !nodes.contains(node.as_str())
                    || !dominators[route.source.as_str()].contains(node.as_str())
                {
                    return Err(format!("route value unavailable at {}", route.source));
                }
                env.insert(format!("__bl_route_{index}"), ty.clone());
            }
            if infer_with(
                &expression,
                Some(&Type::Named("Bool".into())),
                &env,
                program,
                &[],
            )? != Type::Named("Bool".into())
            {
                return Err("route condition must be Bool".into());
            }
        }
    }
    let outputs = |name: &str| -> Option<Vec<(String, Type)>> {
        if let Some(peer) = program.peer_nodes.iter().find(|peer| peer.name == name) {
            return match &peer.kind {
                PeerKind::Start { outputs }
                | PeerKind::Join { outputs, .. }
                | PeerKind::Subprocess { outputs, .. } => Some(outputs.clone()),
                PeerKind::End { .. }
                | PeerKind::Split { .. }
                | PeerKind::Terminal { .. }
                | PeerKind::PauseFor(_)
                | PeerKind::PauseUntil { .. } => None,
            };
        }
        program
            .decisions
            .iter()
            .find(|task| task.name == name)
            .map(|task| {
                task.outputs
                    .iter()
                    .map(|(port, ty, _)| (port.clone(), ty.clone()))
                    .collect()
            })
    };
    let inputs = |name: &str| -> Option<Vec<(String, Type)>> {
        if let Some(peer) = program.peer_nodes.iter().find(|peer| peer.name == name) {
            return match &peer.kind {
                PeerKind::End { inputs }
                | PeerKind::Join { inputs, .. }
                | PeerKind::Subprocess { inputs, .. } => Some(inputs.clone()),
                PeerKind::PauseUntil { input } => {
                    Some(vec![(input.clone(), Type::Named("DateTime".into()))])
                }
                PeerKind::Start { .. }
                | PeerKind::Split { .. }
                | PeerKind::Terminal { .. }
                | PeerKind::PauseFor(_) => None,
            };
        }
        program
            .decisions
            .iter()
            .find(|task| task.name == name)
            .map(|task| task.inputs.clone())
    };
    for binding in &graph.bindings {
        if program.peer_nodes.iter().any(|peer| {
            peer.name == binding.source && matches!(peer.kind, PeerKind::Subprocess { .. })
        }) && graph.routes.iter().any(|route| {
            route.source == binding.source
                && route.outcome.is_some()
                && reaches(graph, &route.target, &binding.target)
        }) {
            return Err(format!(
                "subprocess output unavailable on exceptional route: {} -> {}",
                binding.source, binding.target
            ));
        }
        if graph
            .bindings
            .iter()
            .filter(|item| {
                item.source == binding.source
                    && item.output == binding.output
                    && item.target == binding.target
                    && item.input == binding.input
            })
            .count()
            > 1
        {
            return Err(format!(
                "ambiguous binding: {}.{} -> {}.{}",
                binding.source, binding.output, binding.target, binding.input
            ));
        }
        if !nodes.contains(binding.source.as_str()) || !nodes.contains(binding.target.as_str()) {
            return Err(format!(
                "binding node outside process: {} -> {}",
                binding.source, binding.target
            ));
        }
        let output = outputs(&binding.source)
            .and_then(|ports| ports.into_iter().find(|(name, _)| name == &binding.output))
            .ok_or_else(|| {
                format!(
                    "unknown binding output: {}.{}",
                    binding.source, binding.output
                )
            })?;
        let input = inputs(&binding.target)
            .and_then(|ports| ports.into_iter().find(|(name, _)| name == &binding.input))
            .ok_or_else(|| {
                format!(
                    "unknown binding input: {}.{}",
                    binding.target, binding.input
                )
            })?;
        let source_is_multi = graph
            .multi_instances
            .iter()
            .any(|multi| multi.node == binding.source);
        let target_is_multi = graph
            .multi_instances
            .iter()
            .any(|multi| multi.node == binding.target);
        let compatible = output.1 == input.1
            || (source_is_multi
                && Type::Generic("List".into(), Box::new(output.1.clone())) == input.1)
            || (target_is_multi
                && output.1 == Type::Generic("List".into(), Box::new(input.1.clone())));
        if !compatible {
            return Err(format!(
                "binding type mismatch: {}.{} -> {}.{}",
                binding.source, binding.output, binding.target, binding.input
            ));
        }
        let joins_from_source = matches!(
            program
                .peer_nodes
                .iter()
                .find(|peer| peer.name == binding.target)
                .map(|peer| &peer.kind),
            Some(PeerKind::Join { .. })
        ) && graph
            .flows
            .iter()
            .any(|(source, target)| source == &binding.source && target == &binding.target);
        let candidates: Vec<_> = graph
            .bindings
            .iter()
            .filter(|item| item.target == binding.target && item.input == binding.input)
            .map(|item| item.source.as_str())
            .collect();
        if binding.source == binding.target
            || !reaches(graph, &binding.source, &binding.target)
            || (!joins_from_source
                && !dominators[binding.target.as_str()].contains(binding.source.as_str())
                && !exclusive(&binding.target, &candidates))
        {
            return Err(format!("binding source unavailable at {}", binding.target));
        }
    }
    for node in &nodes {
        if program
            .peer_nodes
            .iter()
            .any(|peer| peer.name == *node && matches!(peer.kind, PeerKind::Join { .. }))
        {
            continue;
        }
        if let Some(ports) = inputs(node) {
            for (port, _) in ports {
                let candidates: Vec<_> = graph
                    .bindings
                    .iter()
                    .filter(|bind| bind.target == *node && bind.input == port)
                    .map(|bind| bind.source.as_str())
                    .collect();
                if candidates.len() != 1 && !exclusive(node, &candidates) {
                    return Err(format!(
                        "missing binding or ambiguous binding: {node}.{port}"
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn check_named_graph(graph: &NamedGraph, has_deadline: bool) -> Result<(), String> {
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

pub(super) fn route_scope(
    scopes: &HashMap<&str, HashMap<String, Type>>,
    link: &crate::graph::Link,
) -> HashMap<String, Type> {
    let mut env = scopes[link.source.as_str()].clone();
    if link.outcome.is_some() {
        env.remove(&link.source);
    }
    env
}

pub(super) fn task_types<'a>(
    program: &'a Program,
    task: &str,
) -> Result<(&'a Type, &'a Type), String> {
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
