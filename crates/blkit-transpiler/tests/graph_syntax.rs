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

#[test]
fn input_request_parses_literal_prompt_and_typed_response_ports() {
    let source = r#"namespace requests; version "1";
Order = {total: Number};
start_event start {}
input_request review { prompt "Approve this order?"; output approved: Bool; output order: Order; }
end_event done { input approved: Bool; }
process run { flow start -> review; flow review -> done; bind review.approved -> done.approved; }"#;
    let program = parse(source).unwrap();
    let review = program
        .peer_nodes
        .iter()
        .find(|node| node.name == "review")
        .unwrap();
    let PeerKind::InputRequest { prompt, outputs } = &review.kind else {
        panic!("expected input request");
    };
    assert_eq!(prompt, "Approve this order?");
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0].0, "approved");
    assert_eq!(outputs[0].1.to_string(), "Bool");
    assert_eq!(outputs[1].1.to_string(), "Order");
    validate(&program).unwrap();
    let escaped = parse(&source.replace("Approve this order?", r"Approve\tthis order?")).unwrap();
    let PeerKind::InputRequest { prompt, .. } = &escaped
        .peer_nodes
        .iter()
        .find(|node| node.name == "review")
        .unwrap()
        .kind
    else {
        panic!("expected input request");
    };
    assert_eq!(prompt, r"Approve\tthis order?");
}

#[test]
fn input_request_declaration_errors_identify_the_named_request() {
    let source = r#"namespace requests; version "1";
input_request first { prompt "First?"; output yes: Bool; }
input_request second { prompt "Second?"; output yes: Bool; }"#;
    for (bad, reason) in [
        ("prompt \"Second?\";", "requires an output"),
        ("output yes: Bool;", "requires prompt"),
        (
            "prompt \"Second?\"; prompt \"Again?\"; output yes: Bool;",
            "duplicate input_request prompt",
        ),
        (
            "prompt \"Second?\"; output yes: Bool; output yes: Bool;",
            "duplicate input_request port",
        ),
        ("prompt \"\"; output yes: Bool;", "prompt must be nonempty"),
    ] {
        let broken = source.replace("prompt \"Second?\"; output yes: Bool;", bad);
        let error = transpile_diagnostic(&broken, "requests.bl").unwrap_err();
        assert_eq!(error.source_name, "requests.bl");
        assert!(error.message.contains("second"), "{bad}: {error}");
        assert!(error.message.contains(reason), "{bad}: {error}");
    }
}

#[test]
fn input_request_rejects_malformed_statements_and_missing_parts() {
    let source = "namespace requests; version \"1\"; input_request review { prompt \"Approve?\"; output approved: Bool; }";
    for (bad, expected) in [
        ("prompt \"\";", "prompt"),
        ("prompt \"   \";", "prompt"),
        ("output approved: Bool;", "prompt"),
        ("prompt \"Approve?\";", "output"),
        ("prompt \"Again?\";", "prompt"),
        ("output approved: Bool; output approved: Bool;", "duplicate"),
        (
            "input value: Bool;",
            "invalid or duplicate input_request port",
        ),
        ("duration \"1m\";", "invalid input_request statement"),
    ] {
        let altered = match bad {
            "prompt \"\";" | "prompt \"   \";" => source.replace("prompt \"Approve?\";", bad),
            "output approved: Bool;" => source.replace("prompt \"Approve?\"; ", ""),
            "prompt \"Approve?\";" => source.replace("output approved: Bool; ", ""),
            "prompt \"Again?\";" => source.replace(
                "prompt \"Approve?\";",
                "prompt \"Approve?\"; prompt \"Again?\";",
            ),
            "output approved: Bool; output approved: Bool;" => {
                source.replace("output approved: Bool;", bad)
            }
            _ => source.replace("output approved: Bool;", bad),
        };
        let error = parse(&altered).unwrap_err();
        assert!(error.contains(expected), "{bad}: {error}");
    }
    for malformed in [
        "prompt name;",
        "prompt 5;",
        "output approved Bool;",
        "output : Bool;",
    ] {
        let altered = source.replace("prompt \"Approve?\";", malformed);
        assert!(parse(&altered).is_err(), "accepted {malformed}");
    }
}
