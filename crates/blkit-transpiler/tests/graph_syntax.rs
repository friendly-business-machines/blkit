use blkit::{graph::PeerKind, parse, transpile_diagnostic, validate};
use blkit_transpiler as blkit;

#[test]
fn parses_peer_flows_bindings_and_retry_policy() {
    let source = include_str!("../../../tools/dev-harness/examples/graph.bl").replace(
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
fn retry_duration_errors_locate_each_original_quoted_token() {
    for (field, invalid) in [("retry_for", "\"bogus\""), ("retry_delay", "\"bogus\"")] {
        let source = format!(
            "namespace demo; version \"1\";\n# café\nprocess route {{ retry max_retries 1 retry_for {} retry_delay {} backoff exponential; }}",
            if field == "retry_for" {
                invalid
            } else {
                "\"1s\""
            },
            if field == "retry_delay" {
                invalid
            } else {
                "\"1s\""
            }
        );
        let error = transpile_diagnostic(&source, "route.bl").unwrap_err();
        let start = source.find(invalid).unwrap();
        assert!(error.message.contains("invalid retry duration"), "{error}");
        assert_eq!(error.source_name, "route.bl");
        assert_eq!(
            error.span,
            Some(start..start + invalid.len()),
            "{field}: {error}"
        );
    }
}

#[test]
fn repeat_max_duration_errors_locate_original_quoted_token() {
    for invalid in ["\"bogus\"", "\"0s\"", "\"9223372036854776s\""] {
        let source = format!(
            "namespace demo; version \"1\";\n# café\nprocess route {{ repeat_post work while true max_duration {invalid}; }}"
        );
        let error = transpile_diagnostic(&source, "route.bl").unwrap_err();
        let start = source.find(invalid).unwrap();
        assert_eq!(error.source_name, "route.bl");
        assert_eq!(error.span, Some(start..start + invalid.len()), "{error}");
        assert!(
            error.message.contains(if invalid == "\"bogus\"" {
                "invalid retry duration"
            } else {
                "max_duration must be positive and fit milliseconds"
            }),
            "{error}"
        );
    }
}

#[test]
fn decision_task_ports_are_checked_against_bindings() {
    let source = include_str!("../../../examples/minimal.bl");
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
    let program = parse(include_str!("../../../tools/dev-harness/examples/graph.bl")).unwrap();
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
    let program = parse(include_str!("../../../examples/subprocess.bl")).unwrap();
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
        include_str!("../../../tools/dev-harness/examples/graph.bl"),
        include_str!("../../../examples/approve.bl"),
        include_str!("../../../examples/pricing.bl"),
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
    let source = include_str!("../../../tools/dev-harness/examples/graph.bl");
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

#[test]
fn graph_ast_preserves_peer_flow_route_and_binding_order() {
    use blkit::expr::Expr;

    let source = r##"namespace demo; version "v;#1";
# comment before declarations
start_event start { output first: Number; output second: Number; }
xor_split fork {}
end_event left { input first: Number; }
end_event right { input second: Number; }
process run {
  flow start -> fork;
  flow fork -> right when start.second > 0;
  flow fork -> left else;
  bind start.second -> right.second;
  bind start.first -> left.first;
}"##;
    let program = parse(source).unwrap();
    assert_eq!(
        program
            .peer_nodes
            .iter()
            .map(|peer| peer.name.as_str())
            .collect::<Vec<_>>(),
        ["start", "fork", "left", "right"]
    );
    let PeerKind::Start { outputs } = &program.peer_nodes[0].kind else {
        panic!("expected start")
    };
    assert_eq!(
        outputs
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    let graph = program.processes[0].source_graph.as_ref().unwrap();
    assert_eq!(
        graph.flows,
        [
            ("start".into(), "fork".into()),
            ("fork".into(), "right".into()),
            ("fork".into(), "left".into())
        ]
    );
    assert_eq!(
        graph
            .routes
            .iter()
            .map(|route| (route.target.as_str(), route.fallback))
            .collect::<Vec<_>>(),
        [("right", false), ("left", true)]
    );
    assert!(matches!(&graph.routes[0].condition, Some(Expr::Binary(_, op, _)) if op == ">"));
    assert_eq!(
        graph
            .bindings
            .iter()
            .map(|binding| (binding.output.as_str(), binding.input.as_str()))
            .collect::<Vec<_>>(),
        [("second", "second"), ("first", "first")]
    );
}

#[test]
fn repeat_bound_keyword_inside_quoted_condition_is_not_a_delimiter() {
    use blkit::expr::Expr;

    let source = r#"namespace demo; version "1";
process run { repeat_post work while " max_iterations " == " max_iterations " max_iterations 3; }"#;
    let program = parse(source).unwrap();
    let repetition = &program.processes[0]
        .source_graph
        .as_ref()
        .unwrap()
        .repetitions[0];
    assert_eq!(repetition.max_iterations, Some(3));
    assert!(
        matches!(&repetition.condition, Expr::Binary(left, op, right)
        if op == "==" && matches!(left.as_ref(), Expr::String(value) if value == " max_iterations ")
        && matches!(right.as_ref(), Expr::String(value) if value == " max_iterations "))
    );
}

#[test]
fn graph_port_and_edge_spacing_remains_part_of_the_existing_grammar() {
    let source = r#"namespace demo; version "1";
start_event start { output value: Number; }
end_event done { input value: Number; }
process run { flow start -> done; bind start.value -> done.value; }"#;
    for broken in [
        source.replace("output value: Number", "output value:Number"),
        source.replace("flow start -> done", "flow start->done"),
        source.replace(
            "bind start.value -> done.value",
            "bind start.value->done.value",
        ),
        source.replace(
            "bind start.value -> done.value",
            "bind start .value -> done.value",
        ),
    ] {
        assert!(parse(&broken).is_err(), "accepted: {broken}");
    }
}

#[test]
fn decision_assignment_spacing_remains_part_of_the_existing_grammar() {
    let source = r#"namespace demo; version "1";
decision_task check { input amount: Number; output result: Number = value;
literal_expression value { output result: Number; expression amount; } }"#;
    for broken in [
        source.replace("Number = value", "Number=value"),
        source.replace("Number = value", "Number = value .result"),
        source.replace("output result: Number", "output result:Number"),
    ] {
        assert!(parse(&broken).is_err(), "accepted: {broken}");
    }
}

#[test]
fn table_rule_arrow_requires_existing_spaces() {
    let source = r#"namespace demo; version "1";
decision_task check { input amount: Number; output result: Number = table;
decision_table table { output result: Number; policy FIRST;
input value: Number = amount; output result: Number; rule value > 0 -> 1; default 0; } }"#;
    parse(source).unwrap();
    assert!(parse(&source.replace("value > 0 -> 1", "value > 0->1")).is_err());
}

#[test]
fn process_statements_require_complete_delimiters_and_reject_inline_comments() {
    let source = r##"namespace demo; version "1";
start_event start { output value: Number; }
end_event done { input value: Number; }
process run { flow start -> done; bind start.value -> done.value; }"##;
    parse(source).unwrap();
    for broken in [
        source.replace("flow start -> done;", "flow start -> done"),
        source.replace(
            "bind start.value -> done.value;",
            "bind start.value -> done.value",
        ),
        source.replace("done.value; }", "done.value;"),
        source.replace("flow start -> done;", "flow start -> done; # inline"),
        source.replace(
            "bind start.value -> done.value;",
            "bind start.value -> done.value; # inline",
        ),
    ] {
        assert!(parse(&broken).is_err(), "accepted: {broken}");
    }
}
