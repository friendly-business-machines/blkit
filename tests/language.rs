use blkit::{parse, transpile, validate};

const HEADER: &str = "namespace orders;\nversion \"1.0\";\n";

#[test]
fn headers_and_domain_members_require_semicolons_outside_quotes() {
    let source = "namespace demo; version \"1;0\";\ntype Order:\n  amount: Number;\nenum Status:\n  ready;\n";
    let program = parse(source).unwrap();
    assert_eq!(program.version, "1;0");
    assert_eq!(program.records[0].fields.len(), 1);
    assert_eq!(program.enums[0].variants, ["ready"]);
    for broken in [
        source.replace("namespace demo;", "namespace demo"),
        source.replace("amount: Number;", "amount: Number"),
        source.replace("  ready;", "  ready"),
    ] {
        assert!(parse(&broken).is_err(), "accepted: {broken}");
    }
}

#[test]
fn braced_peers_and_semicolon_statements_parse() {
    let source = r#"namespace demo; version "1;0";
start_event start { output amount: Number; }
decision_task calculate {
  input amount: Number;
  output result: Number = compute;
  literal_expression compute { output result: Number; expression amount; }
}
end_event done { input result: Number; }
process example {
  flow start -> calculate;
  flow calculate -> done;
  bind start.amount -> calculate.amount;
  bind calculate.result -> done.result;
}"#;
    let program = parse(source).unwrap();
    assert_eq!(program.namespace, "demo");
    assert_eq!(program.version, "1;0");
    assert_eq!(program.processes.len(), 1);
    assert_eq!(program.peer_nodes.len(), 2);
    assert!(matches!(
        &program.peer_nodes[0].kind,
        blkit::graph::PeerKind::Start { outputs }
            if outputs[0].0 == "amount" && outputs[0].1.to_string() == "Number"
    ));
    assert_eq!(program.decisions.len(), 1);
    assert_eq!(program.decisions[0].nodes.len(), 1);
    assert_eq!(program.decisions[0].input, "amount");
    assert_eq!(program.decisions[0].outputs[0].2, "compute");
    let graph = program.processes[0].source_graph.as_ref().unwrap();
    assert_eq!(graph.flows[0], ("start".into(), "calculate".into()));
    assert_eq!(graph.flows.len(), 2);
    assert_eq!(graph.bindings.len(), 2);
    assert_eq!(graph.bindings[0].output, "amount");
}

#[test]
fn missing_braces_and_semicolons_fail_parsing() {
    let source = r#"namespace demo;
version "1.0";
start_event start { output amount: Number; }
end_event done { input result: Number; }
process example { flow start -> done; bind start.amount -> done.result; }"#;
    parse(source).unwrap();
    for (broken, expected) in [
        (
            source.replace("flow start -> done;", "flow start -> done"),
            "invalid flow",
        ),
        (
            source.replace("output amount: Number;", "output amount: Number"),
            "missing semicolon",
        ),
        (
            source.replace(
                "bind start.amount -> done.result; }",
                "bind start.amount -> done.result;",
            ),
            "unclosed brace",
        ),
    ] {
        let error = parse(&broken).unwrap_err();
        assert!(error.contains(expected), "{broken}: {error}");
    }
}

#[test]
fn legacy_declaration_and_node_keywords_are_rejected() {
    let source = format!(
        "{HEADER}process example(input: Number) -> Number:\n  node start = start\n  node done = end\n  link start -> done(input)\n"
    );
    for legacy in [
        source.clone(),
        format!("{HEADER}task echo(input: Number) -> Number:\n  return input\n"),
        format!(
            "{HEADER}decision result(input: Number) -> Number:\n  node value: Number = literal input\n  output value\n"
        ),
        "namespace demo; version \"1.0\"; process p { node work = start; }".into(),
        "namespace demo; version \"1.0\"; process p { link start -> done; }".into(),
    ] {
        assert!(parse(&legacy).is_err(), "accepted: {legacy}");
    }
}

fn decision_expression(input_type: &str, output_type: &str, expression: &str) -> String {
    format!(
        "{HEADER}decision_task check {{ input input: {input_type}; output result: {output_type} = value; literal_expression value {{ output result: {output_type}; expression {expression}; }} }}"
    )
}

#[test]
fn declarations_and_typed_process_parse() {
    let source = format!(
        "{HEADER}type Order:
  total: Number;
  tags: List<String>;
enum Decision:
  approved;
  review;
start_event start {{ output order: Order; }}
end_event done {{ input decision: Decision; }}
decision_task approve {{ input order: Order; output decision: Decision = value; literal_expression value {{ output result: Decision; expression Decision.approved; }} }}
process approval {{ flow start -> approve; flow approve -> done; bind start.order -> approve.order; bind approve.decision -> done.decision; }}"
    );
    let ast = parse(&source).unwrap();
    assert_eq!(ast.namespace, "orders");
    assert_eq!(ast.version, "1.0");
    assert_eq!(ast.records.len(), 1);
    assert_eq!(ast.enums.len(), 1);
    assert_eq!(ast.decisions.len(), 1);
    assert_eq!(ast.processes.len(), 1);
    validate(&ast).unwrap();
}

#[test]
fn decision_model_parses_literal_context_dependencies_and_knowledge() {
    let source = format!(
        "{HEADER}decision_task price {{ input amount: Number; output result: Number = final; knowledge add {{ input a: Number; input b: Number; output result: Number; expression a; }} literal_expression base {{ output result: Number; expression amount; }} context final {{ output result: Number; entry subtotal: Number = add(base.result, amount); result subtotal; }} }}"
    );
    let model = &parse(&source).unwrap().decisions[0];
    assert_eq!(model.name, "price");
    assert_eq!(model.nodes.len(), 2);
    assert_eq!(model.knowledge.len(), 1);
    assert_eq!(model.links, vec![("base".into(), "final".into())]);
    assert_eq!(model.output_node, "final");
}

#[test]
fn malformed_decision_declarations_fail_during_parsing() {
    let source = format!(
        "{HEADER}decision_task price {{ input amount: Number; output result: Number = value; context value {{ output result: Number; result amount; }} }}"
    );
    for broken in [
        source.replace("result amount;", "result amount"),
        source.replace(
            "output result: Number = value;",
            "output result: Number = value extra;",
        ),
        source.replace("context value {", "mystery value {"),
    ] {
        assert!(parse(&broken).is_err(), "accepted: {broken}");
    }
}

#[test]
fn decision_models_validate_dependent_types_and_knowledge_calls() {
    let source = format!(
        "{HEADER}decision_task price {{ input amount: Number; output result: Number = outcome; knowledge fee {{ input value: Number; output result: Number; expression value; }} literal_expression base {{ output result: Number; expression amount; }} context outcome {{ output result: Number; entry subtotal: Number = fee(base.result); result subtotal; }} }}"
    );
    validate(&parse(&source).unwrap()).unwrap();
    for (broken, expected) in [
        (source.replace("fee(base.result)", "fee(true)"), "knowledge argument"),
        (source.replace("fee(base.result)", "unknown(base.result)"), "unknown knowledge"),
        (source.replace("entry subtotal: Number", "entry subtotal: Bool"), "decision result type"),
        (source.replace("entry subtotal: Number", "entry subtotal: Mystery"), "unknown type"),
        (source.replace("fee(base.result)", "fee(missing)"), "unknown name"),
        (source.replace("expression value;", "expression fee(value);"), "knowledge cycle"),
        (source.replace("fee(base.result)", "fee(absent.result)"), "unknown"),
        (source.replace("expression amount;", "expression outcome.result;"), "decision cycle"),
        (source.replace("result subtotal;", "result missing;"), "unknown name"),
        (source.replace("context outcome { output result: Number;", "context outcome { output result: Bool;"), "decision output type mismatch"),
        (source.replace("context outcome {", "literal_expression base { output result: Number; expression amount; } context outcome {"), "duplicate"),
    ] {
        let error = parse(&broken).and_then(|program| validate(&program)).unwrap_err();
        assert!(error.contains(expected), "{broken}: {error}");
    }
}

#[test]
fn knowledge_calls_are_scoped_to_their_decision_model() {
    let source = format!(
        "{HEADER}decision_task first {{ input amount: Number; output result: Number = value; knowledge fee {{ input amount: Number; output result: Number; expression amount; }} literal_expression value {{ output result: Number; expression fee(amount); }} }} decision_task second {{ input flag: Bool; output result: Bool = value; knowledge fee {{ input flag: Bool; output result: Bool; expression flag; }} literal_expression value {{ output result: Bool; expression fee(flag); }} }}"
    );
    validate(&parse(&source).unwrap()).unwrap();
    let outside = format!(
        "{source}decision_task misuse {{ input amount: Number; output result: Number = value; literal_expression value {{ output result: Number; expression fee(amount); }} }}"
    );
    assert!(
        validate(&parse(&outside).unwrap())
            .unwrap_err()
            .contains("unknown knowledge")
    );
}

#[test]
fn decision_tables_parse_expressions_multiple_outputs_and_defaults() {
    let source = format!(
        "{HEADER}type Quote:
  price: Number;
  tier: String;
decision_task quote {{ input amount: Number; output result: Quote = table; decision_table table {{ output result: Quote; policy PRIORITY; input value: Number = amount; output price: Number; output tier: String; priority 5, \"express\"; priority 2, \"regular\"; rule value > 100 -> 5, \"express\"; rule value <= 100 -> 2, \"regular\"; default 0, \"none\"; }} }}"
    );
    let program = parse(&source).unwrap();
    validate(&program).unwrap();
    assert_eq!(program.decisions[0].nodes.len(), 1);
    for (broken, expected) in [
        (
            source.replace("priority 2, \"regular\"", "priority 2"),
            "priority",
        ),
        (
            source.replace(
                "rule value > 100 -> 5, \"express\"",
                "rule value > 100 -> 5",
            ),
            "output",
        ),
        (
            source.replace("policy PRIORITY", "policy COLLECT SUM"),
            "aggregation",
        ),
        (
            source.replace("priority 2, \"regular\"", "priority 5, \"express\""),
            "duplicate priority",
        ),
    ] {
        let err = parse(&broken)
            .and_then(|program| validate(&program))
            .unwrap_err();
        assert!(err.contains(expected), "{broken}: {err}");
    }
}

#[test]
fn pause_until_requires_a_bound_datetime_port() {
    let source = r#"namespace timing; version "1";
start_event start { output until: DateTime; output number: Number; }
pause_until hold { input at: DateTime; }
end_event done { input result: DateTime; }
process later {
  flow start -> hold;
  flow hold -> done;
  bind start.until -> hold.at;
  bind start.until -> done.result;
}"#;
    transpile(source).unwrap();
    let missing = source.replace("bind start.until -> hold.at;", "");
    assert!(transpile(&missing).unwrap_err().contains("missing binding"));
    let mismatched = source.replace(
        "bind start.until -> hold.at;",
        "bind start.number -> hold.at;",
    );
    assert!(
        transpile(&mismatched)
            .unwrap_err()
            .contains("type mismatch")
    );
    let obsolete = source.replace("input at: DateTime;", "at start.until;");
    assert!(parse(&obsolete).is_err());
}

#[test]
fn datetime_comparisons_and_wait_expressions_are_typed() {
    let source = format!(
        "{HEADER}type Window:
  opens: DateTime;
  closes: DateTime;
start_event start {{ output window: Window; output closes: DateTime; }} pause_for delay {{ duration \"10m\"; }} pause_until until {{ input at: DateTime; }} end_event done {{ input result: Bool; }} decision_task early {{ input input: Window; output result: Bool = value; literal_expression value {{ output result: Bool; expression input.opens < input.closes; }} }} process later {{ flow start -> early; flow early -> delay; flow delay -> until; flow until -> done; bind start.window -> early.input; bind start.closes -> until.at; bind early.result -> done.result; }}"
    );
    transpile(&source).unwrap();
    for (broken, expected) in [
        (
            source.replace(
                "bind start.closes -> until.at;",
                "bind start.window -> until.at;",
            ),
            "type mismatch",
        ),
        (
            source.replace("duration \"10m\"", "duration \"0s\""),
            "duration",
        ),
        (
            source.replace("input.opens < input.closes", "input.opens < true"),
            "matching types",
        ),
    ] {
        let error = transpile(&broken).unwrap_err();
        assert!(error.contains(expected), "{broken}: {error}");
    }
}

#[test]
fn financial_dates_reject_invalid_basis_types_and_date_only_include_time() {
    for (kind, expression) in [
        ("Date", "daysBetween(input, input, true)"),
        ("Date", "monthsBetween(input, input, \"bad\")"),
        ("Date", "yearsBetween(input, input, \"calendar\", true)"),
        ("Date", "financialYear(input, \"bad\")"),
        ("Date", "financialYearQuarter(input, 13)"),
        ("DateTime", "monthsBetween(input, input, \"calendar\", 1)"),
    ] {
        let output = if expression.starts_with("financialYear") {
            "String"
        } else {
            "Number"
        };
        let source = decision_expression(kind, output, expression);
        assert!(transpile(&source).is_err(), "accepted: {expression}");
    }
}

#[test]
fn calendar_named_arguments_reject_unknown_duplicate_and_misplaced_options() {
    for expression in [
        "calendarDrop(input, date(\"2025-04-18\"), unknown=\"overlap\")",
        "calendarDrop(input, date(\"2025-04-18\"), rangeMatch=\"bad\")",
        "calendarDrop(input, date(\"2025-04-18\"), rangeMatch=\"overlap\", rangeMatch=\"equality\")",
        "calendarKeep(input, \"A\", rangeMatch=\"overlap\", \"B\")",
        "calendarKeep(input, pattern(\"[\"))",
        "calendarDrop(input, 7)",
        "calendarMerge([input], dedupeBy=\"invalid\")",
        "calendarMerge([input], tiebreak=\"invalid\")",
        "calendarMerge([input], dedupeBy=\"value\", dedupeBy=\"valueAndName\")",
        "input = input",
        "calendarDrop(input, {rangeMatch: \"overlap\"})",
    ] {
        let source = decision_expression("Calendar", "Calendar", expression);
        assert!(transpile(&source).is_err(), "accepted: {expression}");
    }
}

#[test]
fn temporal_constructor_migration_and_components_are_typed() {
    for (input, output, expr) in [
        ("Number", "Date", "date(2025, 2, 28)"),
        ("Number", "Time", "time(14, 30, 0)"),
        ("Date", "DateTime", "datetime(input, time(14, 30, 0))"),
        ("DateTime", "Date", "date(input)"),
        ("DateTime", "Time", "time(input)"),
        ("Number", "Date", "today()"),
        ("Number", "DateTime", "now()"),
        ("Date", "Number", "input.isoWeekOfYear"),
        ("Date", "String", "input.isoYearWeek"),
        ("Time", "Number", "input.hour"),
    ] {
        let source = decision_expression(input, output, expr);
        transpile(&source).unwrap_or_else(|error| panic!("{expr}: {error}"));
    }
    for (output, expr) in [
        ("DateTime", "dateTime(\"2026-10-02T09:30:00Z\")"),
        ("Date", "date(2025, 2)"),
        ("Time", "time(24, 1, 0)"),
        ("Date", "date(2025, 2, 30)"),
        (
            "DateTime",
            "datetime(date(\"2025-01-01Z\"), time(\"12:00:00\"))",
        ),
        ("Bool", "date(\"2025-01-01\") = date(\"2025-01-01\")"),
    ] {
        let source = decision_expression("Number", output, expr);
        let error = transpile(&source).expect_err(&format!("accepted: {expr}"));
        if expr.starts_with("dateTime(") {
            assert!(error.contains("datetime"), "{error}");
        }
    }
}

#[test]
fn temporal_literals_accept_the_same_formats_as_typed_json() {
    for (ty, literal) in [
        ("Date", "date(\"2026-10-02+05:30\")"),
        ("Date", "date(\"2026-10-02[Europe/Paris]\")"),
        ("Time", "time(\"24:00:00\")"),
        ("Time", "time(\"09:30:00[Europe/Paris]\")"),
        ("DateTime", "datetime(\"2026-10-02T09:30:00\")"),
        (
            "DateTime",
            "datetime(\"2026-10-02T09:30:00[Europe/Paris]\")",
        ),
    ] {
        let source = decision_expression(ty, ty, literal);
        transpile(&source).unwrap_or_else(|error| panic!("{literal}: {error}"));
    }
}

#[test]
fn temporal_types_and_literal_constructors_validate() {
    let source = format!(
        "{HEADER}type Clock:
  day: Date;
  time: Time;
  instant: DateTime;
decision_task check {{ input input: Clock; output result: Bool = value; literal_expression value {{ output result: Bool; expression input.day < date(\"2026-10-02\") and input.time <= time(\"09:30:00.250\") and input.instant == datetime(\"2026-10-02T09:30:00+02:00\"); }} }}"
    );
    transpile(&source).unwrap();
    for (from, to) in [
        ("2026-10-02", "2026-02-30"),
        ("2026-10-02", "2026-1-2"),
        ("09:30:00.250", "24:00:01"),
        ("09:30:00.250", "9:30:00"),
        ("09:30:00.250", "09:30:00+99:00"),
        (
            "2026-10-02T09:30:00+02:00",
            "2025-03-30T02:30:00[Europe/Paris]",
        ),
        ("date(\"2026-10-02\")", "date(input.day)"),
    ] {
        assert!(
            transpile(&source.replace(from, to)).is_err(),
            "accepted {to}"
        );
    }
}

#[test]
fn ranges_parse_boundaries_and_infer_temporal_types() {
    for expr in [
        "input in [1..5]",
        "input in (1..5)",
        "input in [1..5)",
        "input in (1..5]",
        "input in [null..5]",
        "input in (1..null)",
        "input in (null..null)",
        "input between 1 and 5",
        "input in [1.25..2.50]",
        "input in ([1..5])",
        "[1..5] == [1..5]",
        "[1..2.0] == [1..2.0]",
        "input in (null..null) == true",
    ] {
        let source = decision_expression("Number", "Bool", expr);
        transpile(&source).unwrap_or_else(|e| panic!("{expr}: {e}"));
    }
    for (ty, literal) in [
        ("Date", "date(\"2026-10-02\")"),
        ("DateTime", "datetime(\"2026-10-02T09:30:00Z\")"),
        ("Time", "time(\"09:30:00\")"),
    ] {
        let source = decision_expression(ty, "Bool", &format!("input in [{literal}..null)"));
        transpile(&source).unwrap_or_else(|e| panic!("{ty}: {e}"));
    }
    for expr in [
        "input in [1..date(\"2026-10-02\")]",
        "input in [\"a\"..\"z\"]",
        "input in [5..1]",
        "null == null",
        "[null..null] == [null..null]",
    ] {
        let source = decision_expression("Number", "Bool", expr);
        assert!(transpile(&source).is_err(), "accepted: {expr}");
    }
    for expr in ["[1, 2.5]", "[1.25, 2.50]"] {
        let source = decision_expression("Number", "List<Number>", expr);
        transpile(&source).unwrap();
    }
}

#[test]
fn range_relations_reject_mismatched_and_non_range_arguments() {
    for (expr, valid) in [
        ("before([1..2], [3..4])", true),
        ("after([3..4], [1..2])", true),
        ("meets([1..2], [2..3])", true),
        ("metBy([2..3], [1..2])", true),
        ("overlaps([1..2], [2..3])", true),
        ("overlapsBefore([1..3], [2..4])", true),
        ("overlapsAfter([2..4], [1..3])", true),
        ("includes([1..3], input)", true),
        ("during(input, [1..3])", true),
        ("starts(input, [1..3])", true),
        ("startedBy([1..3], input)", true),
        ("finishes(input, [1..3])", true),
        ("finishedBy([1..3], input)", true),
        ("coincides([1..3], [1..3])", true),
        ("before([1..2], [date(\"2026-01-01\")..null))", false),
        ("during(input, [date(\"2026-01-01\")..null))", false),
        ("includes(input, input)", false),
        ("starts(input, [null..null])", true),
    ] {
        let source = decision_expression("Number", "Bool", expr);
        assert_eq!(transpile(&source).is_ok(), valid, "{expr}");
    }
}

#[test]
fn decision_table_unary_tests_are_typed_and_table_only() {
    let base = format!(
        "{HEADER}decision_task price {{ input amount: Number; output result: Number = table; decision_table table {{ output result: Number; policy FIRST; input value: Number = amount; output price: Number; rule value matches (< 10, [20..30]) or value > 100 -> 5; default 0; }} }}"
    );
    transpile(&base).unwrap();
    for broken in [
        base.replace("(< 10, [20..30])", "()"),
        base.replace("(< 10, [20..30])", "(< 10, [date(\"2026-01-01\")..null))"),
        base.replace("value matches", "unknown matches"),
        base.replace("(< 10, [20..30])", "(< 10, >= date(\"2026-01-01\"))"),
    ] {
        assert!(transpile(&broken).is_err(), "accepted: {broken}");
    }
    let outside = decision_expression("Number", "Bool", "input matches (< 10, [20..30])");
    assert!(transpile(&outside).is_err());
    for (ty, literal) in [
        ("Date", "date(\"2026-01-01\")"),
        ("DateTime", "datetime(\"2026-01-01T00:00:00Z\")"),
        ("Time", "time(\"09:00:00\")"),
    ] {
        let source = format!(
            "{HEADER}decision_task eligible {{ input value: {ty}; output result: Bool = table; decision_table table {{ output result: Bool; policy FIRST; input day: {ty} = value; output approved: Bool; rule day matches ([{literal}..null), >= {literal}) -> true; default false; }} }}"
        );
        transpile(&source).unwrap_or_else(|e| panic!("{ty}: {e}"));
    }
}

#[test]
fn generated_range_helpers_cannot_collide_with_declarations() {
    for name in ["BlRange", "BlRangeValue", "lower_cmp", "upper_cmp"] {
        let source = format!(
            "{HEADER}type {name}:
  value: Number;
{}",
            decision_expression(name, "Bool", "input.value in [1..2]")
                .strip_prefix(HEADER)
                .unwrap()
        );
        assert!(
            transpile(&source).is_err(),
            "generated Rust would collide with {name}"
        );
    }
}

#[test]
fn missing_header_is_reported() {
    assert!(
        parse("version \"1.0\";\n")
            .unwrap_err()
            .contains("namespace")
    );
    assert!(
        parse("namespace orders;\n")
            .unwrap_err()
            .contains("version")
    );
}

#[test]
fn legacy_process_signature_is_rejected() {
    let source = format!("{HEADER}process approve(input Order) -> Decision {{ }}");
    assert!(parse(&source).unwrap_err().contains("declaration name"));
}

#[test]
fn decision_boolean_expression_replaces_legacy_task_branches() {
    // Generic if/return bodies are unsupported; express the predicate in a decision node.
    let source = format!("{HEADER}type Order:
  total: Number;
  blocked: Bool;
  tags: List<String>;
enum Decision:
  approved;
  review;
decision_task approve {{ input input: Order; output result: Bool = value; literal_expression value {{ output result: Bool; expression input.total > 1000 and not input.blocked and input.tags == [\"vip\", \"repeat\"]; }} }}");
    let ast = parse(&source).unwrap();
    validate(&ast).unwrap();
    assert_eq!(ast.decisions[0].nodes.len(), 1);
    assert!(
        parse(&format!(
            "{HEADER}task approve(input: Order) -> Decision:
  if true:
    return Decision.review
"
        ))
        .is_err()
    );
}

#[test]
fn malformed_expression_is_rejected() {
    let source = decision_expression("Number", "Bool", "input >");
    assert!(parse(&source).unwrap_err().contains("expression"));
}

#[test]
fn known_declarations_resolve_and_duplicates_fail() {
    let source = format!(
        "{HEADER}type Order:
  total: Number;
enum Decision:
  approved;
{}",
        decision_expression("Order", "Decision", "Decision.approved")
            .strip_prefix(HEADER)
            .unwrap()
    );
    validate(&parse(&source).unwrap()).unwrap();
    let duplicate = source.replace(
        "enum Decision:",
        "type Order:
  total: Number;
enum Decision:",
    );
    assert!(
        validate(&parse(&duplicate).unwrap())
            .unwrap_err()
            .contains("duplicate")
    );
}

#[test]
fn unknown_and_deferred_types_are_rejected() {
    for ty in ["Mystery", "Table<Number>"] {
        let source = format!(
            "{HEADER}type Order:
  total: {ty};
{}",
            decision_expression("Order", "Order", "input")
                .strip_prefix(HEADER)
                .unwrap()
        );
        assert!(validate(&parse(&source).unwrap()).unwrap_err().contains(ty));
    }
}

#[test]
fn expression_types_and_list_elements_are_checked() {
    let source = format!("{HEADER}type Order:
  total: Number;
  blocked: Bool;
enum Decision:
  approved;
  review;
decision_task decide {{ input input: Order; output result: Bool = value; literal_expression value {{ output result: Bool; expression input.total > 1000 and not input.blocked; }} }} decision_task amounts {{ input input: Order; output result: List<Number> = value; literal_expression value {{ output result: List<Number>; expression [1, 2.5]; }} }}");
    validate(&parse(&source).unwrap()).unwrap();
    let bad = source.replace("[1, 2.5]", "[1, \"two\"]");
    assert!(
        validate(&parse(&bad).unwrap())
            .unwrap_err()
            .contains("List<Number>")
    );
}

#[test]
fn generated_rust_names_are_checked() {
    for source in [
        decision_expression("Number", "Number", "input")
            .replace("decision_task check", "decision_task match"),
        format!(
            "{HEADER}type Vec:
  value: Number;
{}",
            decision_expression("Vec", "Vec", "input")
                .strip_prefix(HEADER)
                .unwrap()
        ),
        format!(
            "{HEADER}type NAMESPACE:
  value: Number;
{}",
            decision_expression("NAMESPACE", "NAMESPACE", "input")
                .strip_prefix(HEADER)
                .unwrap()
        ),
        format!(
            "{HEADER}type Order:
  match: Number;
{}",
            decision_expression("Order", "Order", "input")
                .strip_prefix(HEADER)
                .unwrap()
        ),
        format!(
            "{HEADER}enum Decision:
  match;
{}",
            decision_expression("Number", "Decision", "Decision.match")
                .strip_prefix(HEADER)
                .unwrap()
        ),
        decision_expression("Number", "Number", "match")
            .replace("input input: Number;", "input match: Number;"),
    ] {
        assert!(
            transpile(&source).is_err(),
            "accepted invalid generated name: {source}"
        );
    }
}

#[test]
fn empty_list_equality_does_not_depend_on_operand_order() {
    let left = decision_expression("List<Number>", "Bool", "[] == input");
    let right = left.replace("[] == input", "input == []");
    validate(&parse(&left).unwrap()).unwrap();
    validate(&parse(&right).unwrap()).unwrap();
}

#[test]
fn by_value_record_cycles_fail_but_list_recursion_is_allowed() {
    let direct = format!(
        "{HEADER}type Node:
  next: Node;
{}",
        decision_expression("Node", "Node", "input")
            .strip_prefix(HEADER)
            .unwrap()
    );
    assert!(
        validate(&parse(&direct).unwrap())
            .unwrap_err()
            .contains("recursive")
    );
    let indirect = format!(
        "{HEADER}type A:
  b: B;
type B:
  a: A;
{}",
        decision_expression("A", "A", "input")
            .strip_prefix(HEADER)
            .unwrap()
    );
    assert!(
        validate(&parse(&indirect).unwrap())
            .unwrap_err()
            .contains("recursive")
    );
    let list = direct.replace("next: Node", "next: List<Node>");
    validate(&parse(&list).unwrap()).unwrap();
}

#[test]
fn decision_expression_requires_a_result() {
    // Generic if/return branches have no authored task kind; a decision node requires an expression.
    let base = decision_expression("Number", "Number", "input");
    let missing = base.replace("expression input;", "");
    assert!(
        parse(&missing)
            .unwrap_err()
            .contains("missing literal expression")
    );
    validate(&parse(&base).unwrap()).unwrap();
}

#[test]
fn field_variant_and_result_errors_are_reported() {
    let base = format!(
        "{HEADER}type Order:
  total: Number;
enum Decision:
  approved;
{}",
        decision_expression("Order", "Decision", "Decision.approved")
            .strip_prefix(HEADER)
            .unwrap()
    );
    for (broken, expected) in [
        (
            base.replace(
                "expression Decision.approved;",
                "expression Decision.missing;",
            ),
            "variant",
        ),
        (
            base.replace("expression Decision.approved;", "expression input.missing;"),
            "field",
        ),
        (
            base.replace("expression Decision.approved;", "expression input.total;"),
            "decision result type",
        ),
        (
            base.replace("expression Decision.approved;", "expression 1 and true;"),
            "Bool",
        ),
    ] {
        assert!(
            validate(&parse(&broken).unwrap())
                .unwrap_err()
                .contains(expected)
        );
    }
}

#[test]
fn string_operators_parse_and_typecheck_without_changing_range_or_equality() {
    use blkit::expr::Expr;
    let source = format!(
        "{HEADER}decision_task compose {{ input input: String; output result: String = value; literal_expression value {{ output result: String; expression \"foo\" + input + \"bar\"; }} }} decision_task member {{ input input: String; output result: Bool = value; literal_expression value {{ output result: Bool; expression input in [\"active\", \"pending\"]; }} }} decision_task negative {{ input input: String; output result: Bool = value; literal_expression value {{ output result: Bool; expression -1 == -1; }} }} decision_task compare {{ input input: String; output result: Bool = value; literal_expression value {{ output result: Bool; expression \"a\" + \"b\" == \"ab\"; }} }}"
    );
    let program = parse(&source).unwrap();
    validate(&program).unwrap();
    let blkit::decision::DecisionKind::Literal(Expr::Binary(_, op, _)) =
        &program.decisions[3].nodes[0].kind
    else {
        panic!("expected comparison");
    };
    assert_eq!(op, "==", "concatenation binds more tightly than equality");
    for broken in [
        source.replace("\"foo\" + input", "\"foo\" + 1"),
        source.replace("input in [\"active\", \"pending\"]", "input in [1, 2]"),
        source.replace("\"a\" + \"b\" == \"ab\"", "\"a\" = \"A\""),
    ] {
        assert!(
            parse(&broken).and_then(|p| validate(&p)).is_err(),
            "accepted {broken}"
        );
    }
    validate(&parse(&decision_expression("Number", "Bool", "input in [1..5]")).unwrap()).unwrap();
}

#[test]
fn numeric_point_interval_relations_validate_overloads() {
    for expression in [
        "before(0, [1..5])",
        "after(6, [1..5])",
        "before([1..5], 6)",
        "meets(1, (1..5])",
        "metBy([1..5], 1)",
        "before(0, (null..5])",
    ] {
        assert!(
            transpile(&decision_expression("Number", "Bool", expression)).is_ok(),
            "rejected: {expression}"
        );
    }
    for expression in [
        "before(1, 2)",
        "meets(\"1\", [1..5])",
        "overlaps(1, [1..5])",
        "coincides([1..5], 2)",
    ] {
        assert!(
            transpile(&decision_expression("Number", "Bool", expression)).is_err(),
            "accepted: {expression}"
        );
    }
}

#[test]
fn numeric_aggregates_require_one_list_and_infer_empty_lists() {
    for name in [
        "min", "max", "sum", "mean", "median", "product", "stddev", "mode",
    ] {
        for argument in ["[1, 2, 3]", "[]"] {
            let expression = format!("{name}({argument})");
            assert!(
                transpile(&decision_expression("Number", "Number", &expression)).is_ok(),
                "rejected: {expression}"
            );
        }
        for argument in ["1", "[1, \"x\"]", "[true]", "[1], [2]"] {
            let expression = format!("{name}({argument})");
            assert!(
                transpile(&decision_expression("Number", "Number", &expression)).is_err(),
                "accepted: {expression}"
            );
        }
    }
}

#[test]
fn number_text_conversion_validates_constants_and_types() {
    for expression in [
        "number(\"1500.50\")",
        "number(\"1.500,50\", \".\", \",\")",
        "number(input)",
    ] {
        assert!(
            transpile(&decision_expression("String", "Number", expression)).is_ok(),
            "rejected: {expression}"
        );
    }
    for expression in [
        "number(1)",
        "number(\"nope\")",
        "number(\"12.34,50\", \".\", \",\")",
        "number(\"1\", \"..\", \",\")",
        "number(\"1\", \".\", \".\")",
        "number(\"1\", \".\")",
    ] {
        assert!(
            transpile(&decision_expression("String", "Number", expression)).is_err(),
            "accepted: {expression}"
        );
    }
}

#[test]
fn numeric_math_calls_are_typed() {
    for (expression, output) in [
        ("abs(-10)", "Number"),
        ("modulo(-10, 3)", "Number"),
        ("sqrt(16)", "Number"),
        ("exp(1)", "Number"),
        ("ln(1)", "Number"),
        ("log(100)", "Number"),
        ("log(8, 2)", "Number"),
        ("clamp(5, 0, 10)", "Number"),
        ("odd(5)", "Bool"),
        ("even(2)", "Bool"),
        ("isPositive(5)", "Bool"),
        ("isNegative(-3)", "Bool"),
        ("isZero(0)", "Bool"),
    ] {
        assert!(
            transpile(&decision_expression("Number", output, expression)).is_ok(),
            "rejected: {expression}"
        );
    }
    for expression in [
        "sqrt()",
        "modulo(1)",
        "clamp(1, 2)",
        "log(1, 2, 3)",
        "odd(\"five\")",
    ] {
        assert!(
            transpile(&decision_expression("Number", "Number", expression)).is_err(),
            "accepted: {expression}"
        );
    }
}

#[test]
fn numeric_rounding_calls_require_number_arguments_and_scales() {
    for expression in [
        "round(2.345, 2)",
        "roundUp(-5.1, 0)",
        "roundDown(5.9, 0)",
        "roundHalfUp(2.5, 0)",
        "roundHalfDown(2.5, 0)",
        "roundHalfEven(2.5, 0)",
        "floor(1.9)",
        "ceiling(1.9, -1)",
    ] {
        let output = transpile(&decision_expression("Number", "Number", expression));
        assert!(output.is_ok(), "rejected: {expression}: {:?}", output.err());
    }
    for expression in [
        "round(1)",
        "roundUp(1, 2, 3)",
        "floor(1, 0.5, 2)",
        "ceiling(\"x\")",
        "round(2, 29)",
    ] {
        assert!(
            transpile(&decision_expression("Number", "Number", expression)).is_err(),
            "accepted: {expression}"
        );
    }
}

#[test]
fn constant_numeric_errors_fail_validation() {
    for expression in ["1 / 0", "0 ** -1", "(-1) ** 0.5"] {
        let source = decision_expression("Number", "Number", expression);
        assert!(transpile(&source).is_err(), "accepted: {expression}");
    }
}

#[test]
fn number_arithmetic_parses_and_typechecks_with_precedence() {
    use blkit::expr::{self, Expr};
    for (expression, output) in [
        ("1.5e3", "Number"),
        ("1.5e-3", "Number"),
        ("-5", "Number"),
        ("-(7)", "Number"),
        ("10-4", "Number"),
        ("2 + 3 * 4", "Number"),
        ("10 / 4", "Number"),
        ("9 ** 0.5", "Number"),
        ("3.0 == 3.00", "Bool"),
        ("\"a\" + \"b\"", "String"),
    ] {
        let source = decision_expression("Number", output, expression);
        validate(&parse(&source).unwrap()).unwrap_or_else(|error| panic!("{expression}: {error}"));
    }
    let Expr::Binary(_, op, right) = expr::expression("-2 ** 3 ** 2").unwrap() else {
        panic!("expected unary negation");
    };
    assert_eq!(op, "-");
    let Expr::Binary(_, op, right) = *right else {
        panic!("exponent must bind more tightly than negation");
    };
    assert_eq!(op, "**");
    assert!(matches!(*right, Expr::Binary(_, ref op, _) if op == "**"));
    for expression in ["3.0 = 3.00", "1 + true", "2 ** \"two\"", "1.5e"] {
        let source = decision_expression("Number", "Number", expression);
        assert!(
            parse(&source)
                .and_then(|program| validate(&program))
                .is_err(),
            "accepted {expression}"
        );
    }
}

#[test]
fn string_builtin_signatures_and_literal_regexes_are_validated() {
    let good = [
        ("string(123)", "String"),
        ("string(true)", "String"),
        ("string(date(\"2026-01-01\"))", "String"),
        ("string(time(\"12:30:00\"))", "String"),
        ("string(datetime(\"2026-01-01T00:00:00Z\"))", "String"),
        ("stringJoin([], \",\")", "String"),
        ("stringLength(\"é\")", "Number"),
        ("substring(\"abc\", -1)", "String"),
        ("substring(\"abc\", 1, 2)", "String"),
        ("substringBefore(\"a:b\", \":\")", "String"),
        ("substringAfter(\"a:b\", \":\")", "String"),
        ("upperCase(\"a\")", "String"),
        ("lowerCase(\"A\")", "String"),
        ("trim(\" a \" )", "String"),
        ("trimLeading(\" a\")", "String"),
        ("trimTrailing(\"a \" )", "String"),
        ("contains(\"ab\", \"a\")", "Bool"),
        ("startsWith(\"ab\", \"a\")", "Bool"),
        ("endsWith(\"ab\", \"b\")", "Bool"),
        ("matches(\"ab\", \"a\", \"im\")", "Bool"),
        ("replace(\"ab\", \"a\", \"x\", \"s\")", "String"),
        ("split(\"a,b\", \",\")", "List<String>"),
        ("split(\"a,b\", [\",\", \";\"])", "List<String>"),
        ("extract(\"ab\", \"(a)(b)\")", "List<List<String>>"),
        ("isBlank(\" \" )", "Bool"),
        ("isEmpty(\"\")", "Bool"),
        ("indexOf(\"abc\", \"b\")", "Number"),
        ("charAt(\"abc\", -1)", "String"),
        ("reverse(\"abc\")", "String"),
        ("padLeading(\"a\", 3)", "String"),
        ("padTrailing(\"a\", 3, \"x\")", "String"),
        ("repeat(\"ab\", 2)", "String"),
    ];
    for (expr, ty) in good {
        let source = decision_expression("String", ty, expr);
        validate(&parse(&source).unwrap()).unwrap_or_else(|e| panic!("{expr}: {e}"));
    }
    let decision = decision_expression("String", "List<List<String>>", "extract(input, \"(a)\")");
    validate(&parse(&decision).unwrap()).unwrap();
    for (expr, diagnostic) in [
        ("string([1])", "string"),
        ("stringJoin([1], \",\")", "List<String>"),
        ("split(\"x\", 1)", "split"),
        ("matches(\"x\", \"[\")", "regex"),
        ("matches(\"x\", \"x\", \"q\")", "flag"),
        ("replace(\"x\", \"[\", \"y\")", "regex"),
        ("extract(\"x\", \"[\")", "regex"),
        ("substring(\"x\")", "substring"),
        ("padLeading(\"a\", \"3\")", "Number"),
        ("contains(\"abc\", 1)", "contains"),
    ] {
        let source = decision_expression("String", "String", expr);
        let error = validate(&parse(&source).unwrap()).unwrap_err();
        assert!(error.contains(diagnostic), "{expr}: {error}");
    }
}
