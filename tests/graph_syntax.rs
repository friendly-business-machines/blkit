use blkit::{graph::PeerKind, parse, validate};

#[test]
fn parses_peer_flows_bindings_and_retry_policy() {
    let source = include_str!("../examples/graph.bl").replace(
        "process decide {",
        "process decide { retry max_retries 3 retry_for \"10m\" retry_delay \"1s\" backoff exponential;",
    );
    let program = parse(&source).unwrap();
    let process = program
        .processes
        .iter()
        .find(|p| p.name == "decide")
        .unwrap();
    let graph = process.source_graph.as_ref().unwrap();
    assert_eq!(graph.flows.len(), 2);
    assert_eq!(graph.bindings.len(), 2);
    assert_eq!(graph.flows[0], ("start".into(), "review_order".into()));
    let retry = process.retry.as_ref().unwrap();
    assert_eq!(retry.max_retries, 3);
    assert_eq!(retry.retry_for.as_secs(), 600);
    assert_eq!(retry.retry_delay.as_secs(), 1);
    validate(&program).unwrap();
}

#[test]
fn decision_task_ports_are_checked_against_bindings() {
    let source = include_str!("../examples/minimal.bl");
    validate(&parse(source).unwrap()).unwrap();
    let invalid = source.replace(
        "decision_task calculate {\n  input amount: Number;",
        "decision_task calculate {\n  input amount: String;",
    );
    assert!(
        validate(&parse(&invalid).unwrap())
            .unwrap_err()
            .contains("type mismatch")
    );
}

#[test]
fn parses_labeled_and_conditional_gateway_flows() {
    let program = parse(include_str!("../examples/graph.bl")).unwrap();
    let parallel = program
        .processes
        .iter()
        .find(|p| p.name == "parallel")
        .unwrap()
        .source_graph
        .as_ref()
        .unwrap();
    assert_eq!(parallel.routes[0].label.as_deref(), Some("left"));
    assert_eq!(parallel.routes[1].label.as_deref(), Some("right"));
    let offers = program
        .processes
        .iter()
        .find(|p| p.name == "offers")
        .unwrap()
        .source_graph
        .as_ref()
        .unwrap();
    assert!(offers.routes.iter().any(|route| route.condition.is_some()));
    assert!(offers.routes.iter().any(|route| route.fallback));
    validate(&program).unwrap();
}

#[test]
fn parses_subprocess_and_distinct_outcome_routes() {
    let program = parse(include_str!("../examples/subprocess.bl")).unwrap();
    assert!(program.peer_nodes.iter().any(|node| {
        node.name == "called"
            && matches!(&node.kind, PeerKind::Subprocess { process, .. } if process == "child")
    }));
    let graph = program
        .processes
        .iter()
        .find(|p| p.name == "parent")
        .unwrap()
        .source_graph
        .as_ref()
        .unwrap();
    for outcome in ["error", "cancel", "terminate"] {
        assert!(
            graph
                .routes
                .iter()
                .any(|route| route.outcome.as_deref() == Some(outcome))
        );
    }
    validate(&program).unwrap();
}

#[test]
fn documented_graphs_use_braced_peer_references() {
    for source in [
        include_str!("../examples/graph.bl"),
        include_str!("../examples/approve.bl"),
        include_str!("../examples/pricing.bl"),
    ] {
        let program = parse(source).unwrap();
        validate(&program).unwrap();
        assert!(
            program
                .processes
                .iter()
                .all(|process| process.source_graph.is_some())
        );
    }
}

#[test]
fn conditional_gateway_requires_fallback_and_valid_condition() {
    let source = include_str!("../examples/graph.bl");
    let missing = source.replace(
        "flow offer_fork -> fallback else;",
        "flow offer_fork -> fallback when start.total > 5000;",
    );
    assert!(
        validate(&parse(&missing).unwrap())
            .unwrap_err()
            .contains("else")
    );
    let invalid = source.replace(
        "flow offer_fork -> left when start.total > 500;",
        "flow offer_fork -> left when start.total >;",
    );
    assert!(parse(&invalid).is_err());
}
