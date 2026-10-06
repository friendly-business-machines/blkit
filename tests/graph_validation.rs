use blkit::{
    graph::{Peer, PeerKind},
    parse, transpile, validate,
};

const SIMPLE: &str = r#"namespace routes;
version "1";
start_event start { output amount: Number; }
decision_task echo { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression amount; } }
end_event done { input result: Number; }
process route {
  flow start -> echo;
  flow echo -> done;
  bind start.amount -> echo.amount;
  bind echo.result -> done.result;
}"#;

const XOR: &str = r#"namespace routes;
version "1";
start_event start { output amount: Number; }
end_event done { input result: Number; }
xor_split gate {}
xor_join chosen { split gate; input value: Number; output result: Number; }
decision_task high { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression amount; } }
decision_task low { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression 2; } }
process route {
  flow start -> gate;
  flow gate -> high when start.amount > 10;
  flow gate -> low else;
  flow high -> chosen;
  flow low -> chosen;
  flow chosen -> done;
  bind start.amount -> high.amount;
  bind start.amount -> low.amount;
  bind high.result -> chosen.value;
  bind low.result -> chosen.value;
  bind chosen.result -> done.result;
}"#;

fn reject(source: &str, expected: &str) {
    let error = transpile(source).unwrap_err();
    assert!(
        error.contains(expected),
        "expected {expected:?}, got {error}"
    );
}

#[test]
fn subprocess_outcomes_require_valid_routes_and_typed_bindings() {
    let source = include_str!("../examples/subprocess.bl");
    transpile(source).unwrap();
    reject(
        &source.replace("process child;", "process missing;"),
        "unknown subprocess",
    );
    reject(
        &source.replace(
            "bind start.input -> called.input;",
            "bind start.input -> called.missing;",
        ),
        "unknown binding input",
    );
    reject(
        &source.replace(
            "flow called -> recover_error on error;",
            "flow called -> recover_error on error; flow called -> recover_error on error;",
        ),
        "duplicate",
    );
    reject(
        &source.replace(
            "flow called -> recover_error on error;",
            "flow called -> recover_error on unknown;",
        ),
        "outcome",
    );
}

#[test]
fn subprocess_normal_and_exceptional_outputs_are_exclusive() {
    let source = include_str!("../examples/subprocess.bl");
    let duplicate_error = source.replace(
        "flow called -> recover_error on error;",
        "flow called -> recover_error on error; flow called -> failed on error;",
    );
    reject(&duplicate_error, "duplicate subprocess outcome");
    let duplicate_success = source.replace(
        "flow called -> done;",
        "flow called -> done; flow called -> failed;",
    );
    reject(&duplicate_success, "one success");
    let missing_success = source.replace("flow called -> done;", "");
    reject(&missing_success, "one success");
    let exceptional_output = source.replace(
        "bind start.input -> recover_error.input;",
        "bind called.result -> recover_error.input;",
    );
    reject(&exceptional_output, "unavailable on exceptional route");
}

#[test]
fn subprocess_process_calls_must_be_acyclic() {
    let source = r#"namespace routes; version "1";
start_event start { output amount: Number; }
end_event done { input result: Number; }
subprocess called { process child; input amount: Number; output result: Number; }
process child { flow start -> done; bind start.amount -> done.result; }
process parent { flow start -> called; flow called -> done; bind start.amount -> called.amount; bind called.result -> done.result; }"#;
    transpile(source).unwrap();
    let recursive = source.replace(
        "process child { flow start -> done; bind start.amount -> done.result; }",
        "process child { flow start -> called; flow called -> done; bind start.amount -> called.amount; bind called.result -> done.result; }",
    );
    reject(&recursive, "recursive");
}

#[test]
fn deadlines_require_valid_origin_and_positive_duration() {
    for origin in ["queued", "first_claimed"] {
        let source = SIMPLE.replace(
            "process route {",
            &format!("process route {{ deadline {origin} \"5h\";"),
        );
        transpile(&source).unwrap();
        assert!(
            parse(&source.replace(
                "flow start -> echo;",
                "deadline queued \"5h\"; flow start -> echo;"
            ))
            .is_err()
        );
    }
    for clause in [
        "deadline queued \"0s\";",
        "deadline first_claimed \"nope\";",
        "deadline unknown \"5h\";",
    ] {
        assert!(
            parse(&SIMPLE.replace("process route {", &format!("process route {{ {clause}")))
                .is_err(),
            "{clause}"
        );
    }
    reject(
        &SIMPLE.replace("end_event done", "error_event timeout {} end_event done"),
        "reserved",
    );
}

#[test]
fn cycles_require_deadlines_and_reachable_exit() {
    let source = include_str!("../examples/iteration.bl");
    transpile(source).unwrap();
    reject(&source.replace("deadline queued \"2s\";", ""), "deadline");
    let no_exit = source.replace("flow gate -> stopped else;", "flow gate -> echo else;");
    assert!(transpile(&no_exit).is_err());
    let unavailable = source.replace("when number_start.input > 0", "when echo.result > 0");
    reject(&unavailable, "unavailable");
}

#[test]
fn repetition_and_multi_instance_require_typed_bounded_inputs() {
    let source = include_str!("../examples/iteration.bl");
    for property in [
        "repeat_post echo while echo.result < 3 max_iterations 0;",
        "repeat_pre echo while echo.result < 3 max_iterations 3;",
        "repeat_post echo while echo.result < 3;",
        "repeat_post echo while echo.result < 3 max_duration \"0s\";",
    ] {
        let invalid = source.replace(
            "deadline queued \"2s\";",
            &format!("deadline queued \"2s\"; {property}"),
        );
        assert!(transpile(&invalid).is_err(), "accepted: {property}");
    }
    assert!(
        parse(&source.replace(
            "multi_instance echo each list_start.values parallel;",
            "multi_instance echo each list_start.values random;"
        ))
        .is_err()
    );
    reject(
        &source.replace("output values: List<Number>;", "output values: List<Bool>;"),
        "type mismatch",
    );
}

#[test]
fn bindings_check_types_and_route_availability() {
    transpile(XOR).unwrap();
    reject(
        &XOR.replace("when start.amount > 10", "when start.amount"),
        "Bool",
    );
    reject(
        &XOR.replace("when start.amount > 10", "when high.result > 10"),
        "unavailable",
    );
    reject(
        &XOR.replace(
            "bind chosen.result -> done.result;",
            "bind high.result -> done.result;",
        ),
        "unavailable",
    );
    reject(
        &XOR.replace("input result: Number; }", "input result: Bool; }"),
        "type mismatch",
    );
    reject(
        &XOR.replace(
            "bind low.result -> chosen.value;",
            "bind high.result -> chosen.value;",
        ),
        "one binding per incoming branch",
    );
}

#[test]
fn gateway_splits_and_joins_validate_branch_shape() {
    let source = include_str!("../examples/graph.bl");
    transpile(source).unwrap();
    reject(
        &source.replace(
            "flow parallel_fork -> right as right;",
            "flow parallel_fork -> right as left;",
        ),
        "branch",
    );
    reject(
        &source.replace("right: Number;", "right: Bool;"),
        "type mismatch",
    );
    reject(
        &source.replace(
            "flow offer_fork -> fallback else;",
            "flow offer_fork -> fallback when start.total > 5000;",
        ),
        "else",
    );
    reject(
        &XOR.replace("flow low -> chosen;", "flow low -> done;"),
        "join",
    );
    reject(
        &XOR.replace(
            "xor_join chosen { split gate;",
            "or_join chosen { split gate;",
        ),
        "split",
    );
}

#[test]
fn ordinary_nodes_cannot_fork_or_merge_without_gateways() {
    let fork = SIMPLE
        .replace(
            "flow echo -> done;",
            "flow echo -> done; flow echo -> failure;",
        )
        .replace("end_event done", "error_event failure {} end_event done");
    reject(&fork, "split");
    reject(&SIMPLE.replace("flow echo -> done;", ""), "end event");
    let bad_condition = SIMPLE.replace("flow start -> echo;", "flow start -> echo else;");
    assert!(transpile(&bad_condition).is_err());
}

#[test]
fn graph_rejects_missing_duplicate_unreachable_and_invalid_nodes() {
    transpile(SIMPLE).unwrap();
    reject(
        &SIMPLE.replace("flow start -> echo;", "flow start -> absent;"),
        "unknown",
    );
    reject(
        &SIMPLE.replace(
            "process route {",
            "start_event start { output amount: Number; } process route {",
        ),
        "duplicate",
    );
    let disconnected = SIMPLE.replace("flow start -> echo;", "flow echo -> done;");
    assert!(transpile(&disconnected).is_err());
    let wrong_port = SIMPLE.replace(
        "bind start.amount -> echo.amount;",
        "bind start.amount -> echo.missing;",
    );
    reject(&wrong_port, "unknown binding input");
}

#[test]
fn exceptional_terminals_reject_payloads_but_need_no_normal_end() {
    for kind in ["error", "cancel", "terminate"] {
        let declaration = format!("{kind}_event failed {{}}");
        let source = SIMPLE
            .replace("end_event done { input result: Number; }", &declaration)
            .replace("flow echo -> done;", "flow echo -> failed;")
            .replace("bind echo.result -> done.result;", "");
        transpile(&source).unwrap();
        reject(
            &source.replace(
                &declaration,
                &format!("{kind}_event failed {{ input result: Number; }}"),
            ),
            "terminal",
        );
        reject(
            &source.replace("flow echo -> failed;", "flow echo -> failed on error;"),
            "subprocess",
        );
    }
}

#[test]
fn validation_rejects_programmatically_invalid_peer_kinds() {
    let mut program = parse(SIMPLE).unwrap();
    program.peer_nodes.push(Peer {
        name: "invalid".into(),
        kind: PeerKind::Terminal { kind: "NoSuch" },
    });
    assert!(validate(&program).unwrap_err().contains("terminal kind"));
    program.peer_nodes.pop();
    program.peer_nodes.push(Peer {
        name: "invalid".into(),
        kind: PeerKind::Split { kind: "NoSuch" },
    });
    assert!(validate(&program).unwrap_err().contains("gateway kind"));
}

#[test]
fn legacy_graph_forms_are_not_accepted() {
    for invalid in [
        SIMPLE.replace(
            "flow start -> echo;",
            "node work = task echo(start.amount);",
        ),
        SIMPLE.replace("flow start -> echo;", "link start -> echo;"),
        SIMPLE.replace("flow echo -> done;", "return echo.result;"),
    ] {
        assert!(parse(&invalid).is_err(), "accepted legacy graph: {invalid}");
    }
    validate(&parse(SIMPLE).unwrap()).unwrap();
}
