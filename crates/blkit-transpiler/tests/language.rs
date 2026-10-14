use blkit::{parse, transpile, validate};
use blkit_transpiler as blkit;

const HEADER: &str = "namespace orders;\nversion \"1.0\";\n";

#[test]
fn named_dictionary_declarations_replace_colon_records() {
    let source = format!(
        "{HEADER}Order = {{total: Number, blocked: Bool, tags: List<String>, \"my key\": String}};\nenum Status:\n  ready;\nstart_event start {{ output input: Order; }}"
    );
    let program = parse(&source).unwrap();
    assert_eq!(program.records[0].name, "Order");
    assert_eq!(program.records[0].fields[3].0, "my key");
    assert_eq!(program.records[0].fields[2].1.to_string(), "List<String>");
    assert_eq!(program.enums[0].variants, ["ready"]);
    validate(&program).unwrap();
    let old = format!("{HEADER}type Order:\n  total: Number;");
    let error = parse(&old).unwrap_err();
    assert!(error.contains("Order ="), "{error}");
    assert!(parse(&source.replace("};", "}")).is_err());
}

#[test]
fn dictionary_literals_parse_inside_braced_decisions() {
    let source = format!(
        "{HEADER}decision_task check {{ input input: Number; output result: Bool = value; literal_expression value {{ output result: Bool; expression {{a: 2, b: a * 2}}.b == 4 and {{\"my key\": 1}}[\"my key\"] == 1; }} }}"
    );
    let program = parse(&source).unwrap();
    assert_eq!(program.decisions.len(), 1);
    for expr in [
        "{}",
        "{a: {b: 3}}.a.b",
        "input[\"my key\"]",
        "[1, 2]",
        "[1..2]",
    ] {
        blkit::expr::expression(expr).unwrap();
    }
    for expr in ["{a: 1, a: 2}", "{a: 1", "{a 1}"] {
        assert!(blkit::expr::expression(expr).is_err(), "accepted {expr}");
    }
}

#[test]
fn headers_and_domain_members_require_semicolons_outside_quotes() {
    let source =
        "namespace demo; version \"1;0\";\nOrder = {amount: Number};\nenum Status:\n  ready;\n";
    let program = parse(source).unwrap();
    assert_eq!(program.version, "1;0");
    assert_eq!(program.records[0].fields.len(), 1);
    assert_eq!(program.enums[0].variants, ["ready"]);
    for broken in [
        source.replace("namespace demo;", "namespace demo"),
        source.replace("Order = {amount: Number};", "Order = {amount: Number}"),
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

fn assert_valid_expression(input_type: &str, output_type: &str, expression: &str) {
    let source = decision_expression(input_type, output_type, expression);
    let program = parse(&source).unwrap_or_else(|error| panic!("{expression}: {error}"));
    validate(&program).unwrap_or_else(|error| panic!("{expression}: {error}"));
}

#[test]
fn lists_accept_one_based_numeric_indexing() {
    assert_valid_expression("List<Number>", "Number", "input[1]");
}

#[test]
fn lists_accept_negative_indexing() {
    assert_valid_expression("List<Number>", "Number", "input[-1]");
}

#[test]
fn list_filters_bind_explicit_item() {
    assert_valid_expression("List<Number>", "List<Number>", "input[.item > 2]");
}

#[test]
fn empty_list_filter_uses_contextual_element_type() {
    assert_valid_expression("Number", "List<Number>", "[][.item > 0]");
}

#[test]
fn empty_iteration_source_infers_element_from_body_not_result() {
    assert_valid_expression("Number", "List<Bool>", "for x in [] return x > 0");
}

#[test]
fn empty_iteration_source_combines_binding_with_independent_min_call() {
    assert_valid_expression(
        "Number",
        "List<Number>",
        "for x in [] return x + min([1, 2])",
    );
}

#[test]
fn empty_iteration_source_combines_binding_with_independent_before_call() {
    assert_valid_expression(
        "Number",
        "List<Bool>",
        "for x in [] return x > 0 and before(0, [1..5])",
    );
}

#[test]
fn empty_iteration_source_uses_all_existing_numeric_aggregates() {
    for name in ["min", "max", "sum", "mean", "median", "product", "stddev"] {
        assert_valid_expression(
            "Number",
            "List<Number>",
            &format!("for x in [] return x + {name}([1, 2])"),
        );
    }
    assert!(
        transpile(&decision_expression(
            "Number",
            "List<Number>",
            "for x in [] return x + mode([1, 2])",
        ))
        .is_err()
    );
}

#[test]
fn empty_iteration_source_constrains_binding_through_min_call() {
    assert_valid_expression("Number", "List<Number>", "for x in [] return min([x, 2])");
}

#[test]
fn empty_iteration_source_constrains_binding_through_before_call() {
    assert_valid_expression(
        "Number",
        "List<Bool>",
        "for x in [] return before(0, [x..5])",
    );
}

#[test]
fn empty_iteration_source_preserves_overloaded_range_ambiguity() {
    let source = decision_expression(
        "Number",
        "List<Bool>",
        "for x in [] return before(x, [1..5])",
    );
    let error = validate(&parse(&source).unwrap()).unwrap_err();
    assert!(error.contains("ambiguous"), "{error}");
}

#[test]
fn empty_iteration_source_supports_existing_range_relation_shapes() {
    for relation in [
        "before",
        "after",
        "meets",
        "metBy",
        "overlaps",
        "overlapsBefore",
        "overlapsAfter",
        "coincides",
    ] {
        assert_valid_expression(
            "Number",
            "List<Bool>",
            &format!("for x in [] return {relation}([x..5], [1..5])"),
        );
    }
    for relation in ["includes", "startedBy", "finishedBy"] {
        assert_valid_expression(
            "Number",
            "List<Bool>",
            &format!("for x in [] return {relation}([1..5], x)"),
        );
    }
    for relation in ["during", "starts", "finishes"] {
        assert_valid_expression(
            "Number",
            "List<Bool>",
            &format!("for x in [] return {relation}(x, [1..5])"),
        );
    }
}

#[test]
fn empty_iteration_source_constrains_calendar_target() {
    assert_valid_expression(
        "Calendar",
        "List<Calendar>",
        "for x in [] return calendarKeep(input, [date(\"2025-04-18\"), [x..date(\"2025-04-19\")]], rangeMatch=\"overlap\")",
    );
}

#[test]
fn empty_iteration_source_constrains_calendar_option() {
    assert_valid_expression(
        "Calendar",
        "List<Calendar>",
        "for x in [] return calendarMerge([input], dedupeBy=x)",
    );
}

#[test]
fn empty_iteration_source_constrains_calendar_pattern() {
    assert_valid_expression(
        "Calendar",
        "List<Calendar>",
        "for x in [] return calendarDrop(input, pattern(x))",
    );
}

#[test]
fn empty_iteration_source_rejects_overloaded_calendar_target() {
    let source = decision_expression(
        "Calendar",
        "List<Calendar>",
        "for x in [] return calendarKeep(input, x)",
    );
    let error = validate(&parse(&source).unwrap()).unwrap_err();
    assert!(error.contains("ambiguous"), "{error}");
}

#[test]
fn empty_iteration_source_rejects_invalid_calendar_option() {
    let source = decision_expression(
        "Calendar",
        "List<Calendar>",
        "for x in [] return calendarMerge([input], unknown=x)",
    );
    assert!(validate(&parse(&source).unwrap()).is_err());
}

#[test]
fn empty_iteration_source_uses_result_type_to_disambiguate_zero_minus_duration() {
    assert_valid_expression("Number", "List<DTDuration>", "for x in [] return 0 - x");
}

#[test]
fn empty_iteration_source_uses_nested_numeric_result_constraint() {
    assert_valid_expression("Number", "List<Bool>", "for x in [] return x * 2 > 0");
}

#[test]
fn empty_iteration_source_uses_knowledge_parameter_type() {
    let source = format!(
        "{HEADER}decision_task check {{ input input: Number; output result: List<Bool> = value; knowledge positive {{ input n: Number; output result: Bool; expression n > 0; }} literal_expression value {{ output result: List<Bool>; expression for x in [] return positive(x); }} }}"
    );
    validate(&parse(&source).unwrap()).unwrap();
}

#[test]
fn empty_iteration_source_record_field_is_ambiguous_with_dictionary() {
    let source = format!(
        "{HEADER}Order = {{active: Bool}}; decision_task check {{ input input: Number; output result: List<Bool> = value; literal_expression value {{ output result: List<Bool>; expression for x in [] return x.active; }} }}"
    );
    let error = validate(&parse(&source).unwrap()).unwrap_err();
    assert!(error.contains("ambiguous"), "{error}");
}

#[test]
fn empty_quantifiers_infer_binding_from_boolean_body() {
    assert_valid_expression("Number", "Bool", "every x in [] satisfies x > 0");
    assert_valid_expression("Number", "Bool", "some x in [] satisfies odd(x)");
}

#[test]
fn empty_iteration_source_uses_expected_type_for_identity_body() {
    assert_valid_expression("Number", "List<Number>", "for x in [] return x");
}

#[test]
fn empty_iteration_source_rejects_multiple_record_matches() {
    let source = format!(
        "{HEADER}Order = {{active: Bool}}; Account = {{active: Bool}}; decision_task check {{ input input: Number; output result: List<Bool> = value; literal_expression value {{ output result: List<Bool>; expression for x in [] return x.active; }} }}"
    );
    let error = validate(&parse(&source).unwrap()).unwrap_err();
    assert!(error.contains("ambiguous"), "{error}");
}

#[test]
fn empty_cartesian_binding_is_constrained_by_later_source() {
    assert_valid_expression(
        "Number",
        "List<Number>",
        "for x in [], y in [x, x + 1] return 1",
    );
}

#[test]
fn empty_nested_binding_shadows_outer_binding_without_losing_its_type() {
    assert_valid_expression(
        "Number",
        "List<List<Bool>>",
        "for x in [] return [x > 0, (for x in [true] return x)[1]]",
    );
}

#[test]
fn empty_iteration_source_unused_when_dictionary_key_shadows_binding() {
    assert_valid_expression(
        "Number",
        "List<Number>",
        "for x in [] return {x: 1, y: x}.y",
    );
}

#[test]
fn empty_iteration_source_rejects_overloaded_string_argument() {
    let source = decision_expression("Number", "List<String>", "for x in [] return string(x)");
    let error = validate(&parse(&source).unwrap()).unwrap_err();
    assert!(error.contains("ambiguous"), "{error}");
}

#[test]
fn empty_iteration_source_rejects_open_dictionary_element_type() {
    let source = decision_expression("Number", "List<Number>", "for x in [] return size(x)");
    let error = validate(&parse(&source).unwrap()).unwrap_err();
    assert!(error.contains("ambiguous"), "{error}");
}

#[test]
fn empty_iteration_source_rejects_ambiguous_binding() {
    let source = decision_expression("Number", "List<Bool>", "for x in [] return x == x");
    let error = validate(&parse(&source).unwrap()).unwrap_err();
    assert!(error.contains("ambiguous"), "{error}");
}

#[test]
fn list_filters_parse_mod_at_multiplication_precedence() {
    use blkit::expr::Expr;

    let expression = "input[.item + 1 mod 2 == 0]";
    assert_valid_expression("List<Number>", "List<Number>", expression);

    let Expr::Binary(left, equality, _) = blkit::expr::expression(".item + 1 mod 2 == 0").unwrap()
    else {
        panic!("expected equality");
    };
    assert_eq!(equality, "==");
    let Expr::Binary(_, addition, right) = *left else {
        panic!("expected addition");
    };
    assert_eq!(addition, "+");
    assert!(matches!(*right, Expr::Binary(_, ref operator, _) if operator == "mod"));
}

#[test]
fn lists_of_dictionaries_project_typed_fields() {
    let decision = decision_expression("List<Order>", "List<String>", "input.name");
    let source = format!(
        "{HEADER}Order = {{name: String, amount: Number}};{}",
        decision.strip_prefix(HEADER).unwrap()
    );
    let program = parse(&source).unwrap();
    validate(&program).unwrap();
}

#[test]
fn lists_support_structural_equality_and_scalar_membership() {
    assert_valid_expression("List<Number>", "Bool", "input == [1, 2]");
    assert_valid_expression("List<Number>", "Bool", "2 in input");
}

#[test]
fn some_accepts_multiple_list_bindings() {
    assert_valid_expression(
        "Number",
        "Bool",
        "some x in [1, 2], y in [3, 4] satisfies x + y > 5",
    );
}

#[test]
fn iteration_enum_variants_and_bound_names_have_distinct_scopes() {
    let header = format!("{HEADER}enum Status:\n  ready;\n");
    for (expression, output, valid) in [
        ("for x in [Status.ready] return x", "List<Status>", true),
        (
            "for Status in [{ready: true}] return Status.ready",
            "List<Bool>",
            true,
        ),
        ("for x in [Status.missing] return x", "List<Status>", false),
        ("for Status in [1] return Status.ready", "List<Bool>", false),
    ] {
        let source = format!(
            "{header}decision_task check {{ input input: Number; output result: {output} = value; literal_expression value {{ output result: {output}; expression {expression}; }} }}"
        );
        let result = validate(&parse(&source).unwrap());
        assert_eq!(result.is_ok(), valid, "{expression}: {result:?}");
    }
    let missing = format!(
        "{header}decision_task check {{ input input: Number; output result: Status = value; literal_expression value {{ output result: Status; expression Status.missing; }} }}"
    );
    let error = validate(&parse(&missing).unwrap()).unwrap_err();
    assert!(error.contains("unknown enum variant"), "{error}");
}

#[test]
fn later_iteration_sources_can_use_earlier_bindings() {
    assert_valid_expression(
        "Number",
        "List<Number>",
        "for x in [1, 2], y in [x, x + 1] return y",
    );
}

#[test]
fn every_accepts_multiple_list_bindings() {
    assert_valid_expression(
        "Number",
        "Bool",
        "every x in [1, 2], y in [3, 4] satisfies x + y >= 4",
    );
}

#[test]
fn colon_expressions_produce_numeric_lists() {
    assert_valid_expression("Number", "List<Number>", "1:3");
}

#[test]
fn leading_filter_item_is_rejected_outside_filter_predicates() {
    let source = decision_expression("Number", "Number", ".item");
    assert!(
        parse(&source)
            .and_then(|program| validate(&program))
            .is_err()
    );
}

#[test]
fn list_predicates_and_projected_fields_are_checked() {
    for expression in [
        "input[.item.name]",
        "input[.item + 1 > 0 and .item.missing]",
    ] {
        let source = decision_expression("List<Number>", "List<Number>", expression);
        assert!(
            parse(&source)
                .and_then(|program| validate(&program))
                .is_err(),
            "{expression}"
        );
    }
    let source = format!(
        "{HEADER}Order = {{name: String}};{}",
        decision_expression("List<Order>", "List<Number>", "input.missing")
            .strip_prefix(HEADER)
            .unwrap()
    );
    assert!(
        parse(&source)
            .and_then(|program| validate(&program))
            .is_err()
    );
}

#[test]
fn unbound_plain_item_is_not_the_filter_element() {
    let source = decision_expression("List<Number>", "List<Number>", "input[item > 1]");
    assert!(
        parse(&source)
            .and_then(|program| validate(&program))
            .is_err()
    );
}

#[test]
fn plain_item_in_filter_uses_ordinary_input() {
    let source = decision_expression("Bool", "List<Number>", "[1,2][item]")
        .replace("input input: Bool", "input item: Bool");
    let program = parse(&source).unwrap();
    validate(&program).unwrap();
}

#[test]
fn literal_dictionary_projection_rejects_known_missing_fields() {
    let source = decision_expression("Number", "List<Number>", "[{a: 1}, {b: 2}].a");
    let error = parse(&source)
        .and_then(|program| validate(&program))
        .unwrap_err();
    assert!(error.contains("a"), "{error}");
}

#[test]
fn unsupported_list_date_field_fails_validation() {
    let source = decision_expression("List<Date>", "List<Number>", "input.year");
    assert!(
        parse(&source)
            .and_then(|program| validate(&program))
            .is_err()
    );
}

#[test]
fn unsupported_nested_list_field_fails_validation() {
    let source = decision_expression(
        "List<List<Dictionary<Number>>>",
        "List<List<Number>>",
        "input.score",
    );
    assert!(
        parse(&source)
            .and_then(|program| validate(&program))
            .is_err()
    );
}

#[test]
fn decision_nodes_can_be_named_item() {
    let literal = format!(
        "{HEADER}decision_task check {{ input input: Number; output result: Number = item; literal_expression item {{ output result: Number; expression input; }} }}"
    );
    let context = literal.replace(
        "literal_expression item { output result: Number; expression input; }",
        "context item { output result: Number; result input; }",
    );
    let table = format!(
        "{HEADER}decision_task check {{ input input: Number; output result: Number = item; decision_table item {{ output result: Number; policy FIRST; input value: Number = input; output result: Number; rule value > 0 -> 1; default 0; }} }}"
    );
    for source in [literal, context, table] {
        blkit::transpile(&source).unwrap();
    }
}

#[test]
fn decision_field_filter_accepts_explicit_item_and_forward_declared_record() {
    let source = format!(
        "{HEADER}decision_task check {{ input orders: List<Order>; output result: List<Order> = selected; literal_expression selected {{ output result: List<Order>; expression orders[.item.active];}}}}\nOrder = {{active: Bool}};"
    );
    blkit::transpile(&source).unwrap();
}

#[test]
fn decision_boolean_call_filter_accepts_explicit_item() {
    let source = format!(
        "{HEADER}decision_task check {{ input input: List<Number>; output result: List<Number> = selected; knowledge positive {{ input value: Number; output result: Bool; expression value > 0; }} literal_expression selected {{ output result: List<Number>; expression input[positive(.item)]; }} }}"
    );
    blkit::transpile(&source).unwrap();
}

#[test]
fn decision_numeric_input_named_item_indexes_lists() {
    let source = format!(
        "{HEADER}decision_task check {{ input item: Number; output result: Number = selected; literal_expression selected {{ output result: Number; expression [4,5][item]; }} }}"
    );
    blkit::transpile(&source).unwrap();
}

#[test]
fn iteration_binding_named_item_remains_valid() {
    assert_valid_expression(
        "Number",
        "List<Number>",
        "for item in [1,2] return item + 1",
    );
}

#[test]
fn context_entry_named_item_can_hold_an_explicit_item_filter() {
    let source = format!(
        "{HEADER}decision_task check {{ input input: Number; output result: List<Number> = value; context value {{ output result: List<Number>; entry item: List<Number> = [1,2][.item > 1]; result item; }} }}"
    );
    blkit::transpile(&source).unwrap();
}

#[test]
fn context_entry_named_item_allows_boolean_call_filters() {
    let source = format!(
        "{HEADER}decision_task check {{ input input: Number; output result: List<Number> = value; context value {{ output result: List<Number>; entry item: List<Number> = [1,2][odd(.item)]; result item; }} }}"
    );
    blkit::transpile(&source).unwrap();
}

#[test]
fn explicit_filter_item_is_not_a_forward_context_dependency() {
    let source = format!(
        "{HEADER}decision_task check {{ input input: Number; output result: Number = value; context value {{ output result: Number; entry item: Number = sum(selected); entry selected: List<Number> = [1,2][odd(.item)]; result item; }} }}"
    );
    blkit::transpile(&source).unwrap();
}

#[test]
fn explicit_filter_item_field_is_not_a_forward_context_dependency() {
    let source = format!(
        "{HEADER}Order = {{active: Bool}}; decision_task check {{ input orders: List<Order>; output result: List<Order> = value; context value {{ output result: List<Order>; entry item: List<Order> = selected; entry selected: List<Order> = orders[.item.active]; result item; }} }}"
    );
    blkit::transpile(&source).unwrap();
}

#[test]
fn explicit_filter_item_preserves_plain_item_numeric_dependency() {
    let source = format!(
        "{HEADER}decision_task check {{ input input: Number; output result: Number = value; context value {{ output result: Number; entry selected: Number = [4,5][item]; entry subtotal: Number = sum(values); entry item: Number = subtotal; entry values: List<Number> = [1,2][odd(.item)]; result selected; }} }}"
    );
    blkit::transpile(&source).unwrap();
}

#[test]
fn context_numeric_item_position_cycle_is_rejected() {
    let source = format!(
        "{HEADER}decision_task check {{ input input: Number; output result: Number = value; context value {{ output result: Number; entry item: Number = selected; entry selected: Number = [4,5][item]; result selected; }} }}"
    );
    assert!(blkit::transpile(&source).is_err());
}

#[test]
fn context_numeric_item_position_keeps_forward_entry_dependency() {
    let source = format!(
        "{HEADER}decision_task check {{ input input: Number; output result: Number = value; context value {{ output result: Number; entry selected: Number = [4,5][item]; entry item: Number = input; result selected; }} }}"
    );
    blkit::transpile(&source).unwrap();
}

#[test]
fn decision_numeric_list_position_keeps_its_dependency() {
    let source = format!(
        "{HEADER}decision_task check {{ input input: List<Number>; output result: Number = selected; literal_expression position {{ output result: Number; expression 1; }} literal_expression selected {{ output result: Number; expression input[position]; }} }}"
    );
    blkit::transpile(&source).unwrap();
}

#[test]
fn list_literals_reject_mixed_and_null_elements() {
    for expression in ["[1, \"two\"]", "[1, null]"] {
        let source = decision_expression("Number", "List<Number>", expression);
        assert!(
            parse(&source)
                .and_then(|program| validate(&program))
                .is_err(),
            "accepted: {expression}"
        );
    }
}

#[test]
fn mixed_list_literals_are_rejected_from_inferred_element_type() {
    let expression = "[1, \"two\"] == [1, 2]";
    let source = decision_expression("Number", "Bool", expression);
    let error = parse(&source)
        .and_then(|program| validate(&program))
        .expect_err("accepted a mixed list literal");
    assert!(
        error.contains("List<Number> element has type String"),
        "{expression}: {error}"
    );
}

#[test]
fn dictionary_lookup_ranges_and_record_item_fields_remain_typed() {
    let source = format!(
        "{HEADER}Order = {{total: Number, item: Number, \"my key\": String}};
         decision_task lookup {{ input input: Order; output result: String = value; literal_expression value {{ output result: String; expression input[\"my key\"]; }} }}
         decision_task field {{ input input: Order; output result: Number = value; literal_expression value {{ output result: Number; expression input.total; }} }}
         decision_task item_field {{ input order: Order; output result: Number = value; literal_expression value {{ output result: Number; expression order.item; }} }}
         decision_task range {{ input input: Number; output result: Bool = value; literal_expression value {{ output result: Bool; expression input in [1..5]; }} }}"
    );
    let program = parse(&source).unwrap();
    validate(&program).unwrap();
}

#[test]
fn declarations_and_typed_process_parse() {
    let source = format!(
        "{HEADER}Order = {{total: Number, tags: List<String>}};
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
        "{HEADER}Quote = {{price: Number, tier: String}};
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
        "{HEADER}Window = {{opens: DateTime, closes: DateTime}};
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
fn temporal_calendar_calls_keep_argument_errors_after_shared_typing() {
    for (input, output, expression, message) in [
        (
            "Date",
            "Number",
            "daysBetween(input, input, true)",
            "includeTime requires DateTime and Bool",
        ),
        (
            "Date",
            "String",
            "financialYear(input, true)",
            "invalid financial year basis type",
        ),
        (
            "Date",
            "Number",
            "daysBetween(input, date(2025, 1, 1), missing)",
            "unknown name: missing",
        ),
        (
            "Calendar",
            "CalendarEntry",
            "next(input, date(2025, 1, 1), true)",
            "numeric n",
        ),
    ] {
        let error = transpile(&decision_expression(input, output, expression)).unwrap_err();
        assert!(error.contains(message), "{expression}: {error}");
    }
}

#[test]
fn calendar_date_calls_preserve_specific_argument_diagnostics() {
    for (input, output, expression, message) in [
        (
            "Date",
            "Bool",
            "isPublicHoliday(input, 7)",
            "isPublicHoliday requires Calendar",
        ),
        (
            "Date",
            "Number",
            "weekdaysBetween(input, datetime(\"2025-04-18T12:00:00\"))",
            "weekdaysBetween requires two matching points",
        ),
        (
            "Date",
            "Date",
            "firstDayOfWeekInMonth(input, true)",
            "firstDayOfWeekInMonth requires Number",
        ),
        (
            "Date",
            "Date",
            "firstDayOfWeekInMonth(input)",
            "firstDayOfWeekInMonth requires 1 Number arguments",
        ),
        (
            "Calendar",
            "Date",
            "nextBusinessDay(date(\"2025-04-18\"), input, 7)",
            "strictCalendarRange requires Bool",
        ),
        (
            "Calendar",
            "Date",
            "nextBusinessDay(date(\"2025-04-18\"), input, input, true)",
            "strictCalendarRange requires Bool",
        ),
        (
            "Date",
            "Bool",
            "isWeekend(input, 1)",
            "invalid isWeekend argument count",
        ),
    ] {
        let error = transpile(&decision_expression(input, output, expression)).unwrap_err();
        assert!(error.contains(message), "{expression}: {error}");
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
fn calendar_target_rejects_typed_list_variables() {
    for name in ["calendarDrop", "calendarKeep"] {
        let source = format!(
            "{HEADER}decision_task check {{ input c: Calendar; input targets: List<Date>; output result: Calendar = out; literal_expression out {{ output result: Calendar; expression {name}(c, targets); }} }}"
        );
        let error = transpile(&source).unwrap_err();
        assert!(
            error.contains("invalid calendar target: List<Date>"),
            "{name}: {error}"
        );
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
        "{HEADER}Clock = {{day: Date, time: Time, instant: DateTime}};
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
    let source = format!("{HEADER}Order = {{total: Number, blocked: Bool, tags: List<String>}};
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
        "{HEADER}Order = {{total: Number}};
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
        "Order = {total: Number};
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
            "{HEADER}Order = {{total: {ty}}};
{}",
            decision_expression("Order", "Order", "input")
                .strip_prefix(HEADER)
                .unwrap()
        );
        assert!(validate(&parse(&source).unwrap()).unwrap_err().contains(ty));
    }
}

#[test]
fn named_schema_keys_preserve_commas_and_colons_inside_quotes() {
    let source = r#"namespace orders; version "1";
Order = {"last, first": String, "a:b": Number};
decision_task read { input order: Order; output result: Number = value; literal_expression value { output result: Number; expression order["a:b"]; } }"#;
    blkit::transpile(source).unwrap();
}

#[test]
fn earlier_dictionary_key_does_not_create_a_decision_dependency() {
    let source = r#"namespace orders; version "1";
decision_task calc { input input: Number; output result: Number = a; literal_expression a { output result: Number; expression {a: 2, b: a * 2}.b; } }"#;
    blkit::transpile(source).unwrap();
}

#[test]
fn merge_rejects_a_known_non_dictionary_list_variable() {
    let source = decision_expression("List<Number>", "Dictionary", "dictionaryMerge(input)");
    assert!(blkit::transpile(&source).is_err());
}

#[test]
fn list_iterations_require_lists_and_scoped_predicates() {
    for expression in [
        "every v in 1 satisfies true",
        "every v in [1] satisfies v",
        "for v in 1 return v",
    ] {
        let source = format!(
            "namespace demo; version \"1\"; decision_task test {{ input input: Number; output result: Bool = value; literal_expression value {{ output result: Bool; expression {expression}; }} }}"
        );
        assert!(blkit::transpile(&source).is_err(), "{expression}");
    }
    let source = r#"namespace demo; version "1"; decision_task test { input scores: Dictionary<Number>; output result: Bool = value; literal_expression value { output result: Bool; expression every scores in values(scores) satisfies scores >= 0; } }"#;
    blkit::transpile(source).unwrap();
}

#[test]
fn dictionaries_are_typed_and_malformed_values_are_rejected() {
    for (input, output, expr) in [
        ("Number", "Dictionary<Number>", "{alice: 90, bob: 75}"),
        ("Number", "Dictionary", "{name: \"Alice\", age: 30}"),
        ("Dictionary<Number>", "Number", "input.alice"),
        ("Number", "Number", "{foo: 123}.foo"),
        ("Dictionary", "Dictionary", "input"),
        ("Value", "Value", "input"),
        ("DictionaryEntry<Number>", "Number", "input.value"),
    ] {
        let source = decision_expression(input, output, expr);
        validate(&parse(&source).unwrap()).unwrap_or_else(|error| panic!("{expr}: {error}"));
    }
    for expr in ["{alice: true}", "{alice: 1, alice: 2}"] {
        let source = decision_expression("Number", "Dictionary<Number>", expr);
        assert!(parse(&source).and_then(|p| validate(&p)).is_err(), "{expr}");
    }
    let source = format!(
        "{HEADER}Order = {{total: Number, blocked: Bool}};decision_task check {{ input input: Order; output result: Number = value; literal_expression value {{ output result: Number; expression input.total; }} }}"
    );
    validate(&parse(&source).unwrap()).unwrap();
    let invalid_merge = decision_expression("Number", "Dictionary", "dictionaryMerge([1])");
    assert!(blkit::transpile(&invalid_merge).is_err());
    let unknown = source.replace("input.total", "input.unknown");
    assert!(validate(&parse(&unknown).unwrap()).is_err());
}

#[test]
fn expression_types_and_list_elements_are_checked() {
    let source = format!("{HEADER}Order = {{total: Number, blocked: Bool}};
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
            "{HEADER}Vec = {{value: Number}};
{}",
            decision_expression("Vec", "Vec", "input")
                .strip_prefix(HEADER)
                .unwrap()
        ),
        format!(
            "{HEADER}NAMESPACE = {{value: Number}};
{}",
            decision_expression("NAMESPACE", "NAMESPACE", "input")
                .strip_prefix(HEADER)
                .unwrap()
        ),
        format!(
            "{HEADER}Order = {{match: Number}};
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
        "{HEADER}Node = {{next: Node}};
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
        "{HEADER}A = {{b: B}};
B = {{a: A}};
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
        "{HEADER}Order = {{total: Number}};
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
    for name in ["min", "max", "sum", "mean", "median", "product", "stddev"] {
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
fn aggregate_overloads_validate_result_and_element_types() {
    for (input, output, expression) in [
        ("List<Number>", "List<Number>", "mode(input)"),
        ("List<Number>", "List<Number>", "mode([])"),
        ("List<String>", "List<String>", "mode(input)"),
        ("List<Bool>", "List<Bool>", "mode(input)"),
        ("List<Calendar>", "List<Calendar>", "mode(input)"),
        ("List<List<Number>>", "List<List<Number>>", "mode(input)"),
        ("List<Value>", "List<Value>", "mode(input)"),
        ("List<DTDuration>", "DTDuration", "sum(input)"),
        ("List<DTDuration>", "DTDuration", "sum([])"),
        ("List<DTDuration>", "DTDuration", "mean(input)"),
        ("List<DTDuration>", "DTDuration", "median(input)"),
        ("List<YMDuration>", "YMDuration", "sum(input)"),
        ("List<YMDuration>", "YMDuration", "sum([])"),
        ("List<YMDuration>", "YMDuration", "mean(input)"),
        ("List<YMDuration>", "YMDuration", "median(input)"),
        ("List<Number>", "Number", "min(input)"),
        ("List<String>", "String", "max(input)"),
        ("List<Date>", "Date", "min(input)"),
        ("List<Time>", "Time", "max(input)"),
        ("List<DateTime>", "DateTime", "min(input)"),
        ("List<DTDuration>", "DTDuration", "max(input)"),
        ("List<YMDuration>", "YMDuration", "min(input)"),
        ("List<Number>", "Number", "product(input)"),
        ("List<Number>", "Number", "stddev(input)"),
    ] {
        assert_valid_expression(input, output, expression);
    }
}

#[test]
fn aggregates_preserve_empty_iteration_result_constraints() {
    assert_valid_expression("Number", "Number", "sum(for x in [] return x)");
    assert_valid_expression("Number", "List<Number>", "mode(for x in [] return x)");
    assert_valid_expression(
        "Number",
        "DTDuration",
        "sum(for x in [] return x + dtDuration(\"PT1H\"))",
    );
}

#[test]
fn scalar_mode_outputs_are_rejected() {
    assert!(
        transpile(&decision_expression(
            "List<Number>",
            "Number",
            "mode(input)"
        ))
        .is_err()
    );
}

#[test]
fn aggregate_invalid_inputs_are_rejected() {
    for (input, output, expression) in [
        ("Number", "Number", "product(input)"),
        ("Number", "Number", "stddev(input, [1])"),
        ("List<String>", "Number", "product(input)"),
        ("List<DTDuration>", "Number", "stddev(input)"),
        ("List<Bool>", "Bool", "min(input)"),
        ("List<Number>", "Number", "sum([1, \"x\"])"),
        (
            "List<DTDuration>",
            "DTDuration",
            "sum([dtDuration(\"PT1H\"), ymDuration(\"P1M\")])",
        ),
        (
            "List<YMDuration>",
            "YMDuration",
            "median([ymDuration(\"P1M\"), dtDuration(\"PT1H\")])",
        ),
    ] {
        assert!(
            transpile(&decision_expression(input, output, expression)).is_err(),
            "accepted: {expression}"
        );
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

// Each signature is a separate test so a missing overload does not hide the next one.
macro_rules! list_signature {
    ($name:ident, $output:literal, $expression:literal) => {
        #[test]
        fn $name() {
            assert_valid_expression("List<Number>", $output, $expression);
        }
    };
}

list_signature!(list_count_accepts_typed_list, "Number", "count(input)");
list_signature!(list_is_empty_accepts_typed_list, "Bool", "isEmpty(input)");
list_signature!(
    list_contains_accepts_element,
    "Bool",
    "listContains(input, 2)"
);
list_signature!(
    list_index_of_returns_all_positions,
    "List<Number>",
    "indexOf(input, 2)"
);
#[test]
fn list_index_of_infers_empty_source_from_match() {
    assert_valid_expression("List<Number>", "List<Number>", "indexOf([], 2)");
    assert_valid_expression("List<Number>", "List<Number>", "indexOf([], input)");
    let source = decision_expression("Number", "List<Number>", "indexOf([], [])");
    assert!(validate(&parse(&source).unwrap()).is_err());
}
list_signature!(
    sublist_accepts_negative_start_and_length,
    "List<Number>",
    "sublist(input, -2, 1)"
);
#[test]
fn list_inspection_rejects_invalid_element_and_position_types() {
    for (expression, diagnostic) in [
        ("listContains([1,2], \"2\")", "Number"),
        ("indexOf([1,2], \"2\")", "Number"),
        ("sublist([1,2], \"1\")", "Number"),
        ("sublist([1,2], 1, \"2\")", "Number"),
        ("listContains([1], 1, 1)", "count"),
        ("sublist([1,2])", "count"),
    ] {
        let source = decision_expression("List<Number>", "List<Number>", expression);
        let error = validate(&parse(&source).unwrap()).unwrap_err();
        assert!(error.contains(diagnostic), "{expression}: {error}");
    }
}

list_signature!(
    append_accepts_multiple_elements,
    "List<Number>",
    "append(input, 2, 3)"
);
list_signature!(
    prepend_accepts_multiple_elements,
    "List<Number>",
    "prepend(input, 2, 3)"
);
list_signature!(
    concatenate_accepts_multiple_lists,
    "List<Number>",
    "concatenate(input, [3])"
);
list_signature!(
    union_accepts_multiple_lists,
    "List<Number>",
    "union(input, [3])"
);
list_signature!(
    list_reverse_retains_string_overload,
    "List<Number>",
    "reverse(input)"
);
list_signature!(
    flatten_recurses_over_uniform_nested_lists,
    "List<Number>",
    "flatten([[[1,2]],[[3,4]]])"
);
#[test]
fn list_transformations_infer_empty_sources_and_reject_mixed_depths() {
    for (input, output, expression) in [
        ("List<Number>", "List<Number>", "append([], input[1])"),
        ("List<Number>", "List<Number>", "prepend([], input[1])"),
        ("List<Number>", "List<Number>", "concatenate([], input)"),
        ("List<Number>", "List<Number>", "union([], input)"),
        ("List<Number>", "List<Number>", "reverse([])"),
        ("List<List<Number>>", "List<Number>", "flatten(input)"),
        (
            "List<Number>",
            "List<List<Number>>",
            "for x in [] return append([1], x)",
        ),
    ] {
        assert_valid_expression(input, output, expression);
    }
    for expression in [
        "flatten([[1,2],[[3]]])",
        "flatten([1,2], [3])",
        "append([1], [2])",
        "concatenate([1], [[2]])",
        "union([1], [[2]])",
    ] {
        let source = decision_expression("List<Number>", "List<Number>", expression);
        assert!(validate(&parse(&source).unwrap()).is_err(), "{expression}");
    }
    let source = decision_expression(
        "List<Number>",
        "List<List<Number>>",
        "for x in [] return flatten([x])",
    );
    assert!(
        validate(&parse(&source).unwrap()).is_err(),
        "flatten must not guess the type of a used empty binding"
    );
}

list_signature!(
    insert_before_accepts_one_based_position,
    "List<Number>",
    "insertBefore(input, 1, 9)"
);
list_signature!(
    insert_after_spreads_list_argument,
    "List<Number>",
    "insertAfter(input, 1, [2,3])"
);
list_signature!(
    remove_accepts_one_based_position,
    "List<Number>",
    "remove(input, 1)"
);
list_signature!(
    list_replace_accepts_one_based_position,
    "List<Number>",
    "listReplace(input, 1, 9)"
);
#[test]
fn positional_list_insertion_infers_nested_empty_binding_as_spread() {
    for name in ["insertBefore", "insertAfter"] {
        assert_valid_expression(
            "List<List<Number>>",
            "List<List<List<Number>>>",
            &format!("for x in [] return {name}(input, 1, x)"),
        );
    }
}

#[test]
fn positional_list_editing_checks_types_and_arity() {
    for expression in [
        "insertBefore(input, -1, 9)",
        "insertAfter(input, -1, [2,3])",
        "remove(input, -1)",
        "listReplace(input, -1, 9)",
        "insertBefore([], 1, input[1])",
    ] {
        assert_valid_expression("List<Number>", "List<Number>", expression);
    }
    assert_valid_expression(
        "List<List<Number>>",
        "List<List<Number>>",
        "insertBefore(input, 1, [[2,3]])",
    );
    for expression in [
        "for p in [] return insertBefore(input, p, 5)",
        "for p in [] return insertAfter(input, p, [5])",
        "for p in [] return remove(input, p)",
        "for p in [] return listReplace(input, p, 5)",
    ] {
        assert_valid_expression("List<Number>", "List<List<Number>>", expression);
    }
    for expression in [
        "insertBefore(input, 1, [2,3])",
        "insertAfter(input, 1, [2,3])",
        "insertBefore(input, 1, input[1])",
        "insertAfter(input, 1, input[1])",
    ] {
        let source = decision_expression("List<List<Number>>", "List<List<Number>>", expression);
        let error = validate(&parse(&source).unwrap()).unwrap_err();
        assert!(error.contains("List"), "{expression}: {error}");
    }
    for (expression, diagnostic) in [
        ("remove(input, \"1\")", "Number"),
        ("insertBefore(input, \"1\", 2)", "Number"),
        ("insertAfter(input, 1, [\"2\"])", "List<Number>"),
        ("listReplace(input, 1, [2])", "Number"),
        ("insertBefore(1, 1, 2)", "List"),
        ("insertAfter(1, 1, 2)", "List"),
        ("remove(1, 1)", "List"),
        ("listReplace(1, 1, 2)", "List"),
        ("insertBefore(input, 1)", "argument count"),
        ("insertAfter(input, 1, 2, 3)", "argument count"),
        ("remove(input)", "argument count"),
        ("remove(input, 1, 2)", "argument count"),
        ("listReplace(input, 1)", "argument count"),
        ("listReplace(input, 1, 2, 3)", "argument count"),
    ] {
        let source = decision_expression("List<Number>", "List<Number>", expression);
        let error = validate(&parse(&source).unwrap()).unwrap_err();
        assert!(error.contains(diagnostic), "{expression}: {error}");
    }
}

#[test]
fn inline_predicate_parses_only_as_edit_match_argument() {
    for text in [
        "remove(input, function(i) i == 2)",
        "listReplace(input, function(i) i < 3, 9)",
    ] {
        assert!(blkit::expr::expression(text).is_ok(), "{text}");
    }
    for text in [
        "function(i) i == 2",
        "append(input, function(i) i == 2)",
        "remove(function(i) i == 2, 1)",
        "remove(input, function() true)",
        "remove(input, function(i,j) true)",
        "remove(input, function(1) true)",
    ] {
        assert!(blkit::expr::expression(text).is_err(), "accepted {text}");
    }
}

#[test]
fn inline_predicate_accepts_unary_minus_body() {
    let text = "remove(input, function(i) -i < 0)";
    let expr = blkit::expr::expression(text).unwrap();
    assert!(
        matches!(expr, blkit::expr::Expr::Call(_, args) if matches!(&args[1], blkit::expr::Expr::InlinePredicate { .. })),
        "{text}"
    );
    assert_valid_expression("List<Number>", "List<Number>", text);
}

#[test]
fn inline_predicate_accepts_list_literal_body() {
    let text = "listReplace(input, function(i) [i] == [i], 9)";
    let expr = blkit::expr::expression(text).unwrap();
    assert!(
        matches!(expr, blkit::expr::Expr::Call(_, args) if matches!(&args[1], blkit::expr::Expr::InlinePredicate { .. })),
        "{text}"
    );
    assert_valid_expression("List<Number>", "List<Number>", text);
}

#[test]
fn inline_predicate_rejects_reserved_rust_bindings() {
    for binding in ["type", "match", "fn", "_"] {
        let text = format!("remove(input, function({binding}) {binding} > 0)");
        let source = decision_expression("List<Number>", "List<Number>", &text);
        let error = validate(&parse(&source).unwrap()).unwrap_err();
        assert!(
            error.contains("reserved Rust identifier"),
            "{text}: {error}"
        );
    }
}

#[test]
fn function_header_requires_predicate_body_in_edit_match_argument() {
    for text in [
        "remove(input, function(i))",
        "listReplace(input, function(i), 9)",
    ] {
        assert!(blkit::expr::expression(text).is_err(), "accepted {text}");
    }
    let text = "remove(input, function(i) - 1)";
    let expr = blkit::expr::expression(text).unwrap();
    assert!(
        matches!(expr, blkit::expr::Expr::Call(_, args) if matches!(&args[1], blkit::expr::Expr::InlinePredicate { .. })),
        "{text}"
    );
    let source = decision_expression("List<Number>", "List<Number>", text);
    assert!(validate(&parse(&source).unwrap()).is_err());
    for text in ["function(i)", "remove(input, function(i + 1))"] {
        let source = decision_expression("List<Number>", "List<Number>", text);
        assert!(
            validate(&parse(&source).unwrap()).is_err(),
            "accepted {text}"
        );
    }
    for text in [
        "remove(input, function(i) i > 0)",
        "listReplace(input, function(i) i > 0, 5)",
    ] {
        assert!(
            matches!(blkit::expr::expression(text).unwrap(), blkit::expr::Expr::Call(_, args) if matches!(&args[1], blkit::expr::Expr::InlinePredicate { .. })),
            "{text}"
        );
    }
    for text in [
        "remove(input, function(i) >)",
        "remove(input, function(i,j) i > 0)",
        "remove(input, function() true)",
    ] {
        assert!(blkit::expr::expression(text).is_err(), "accepted {text}");
    }
}

#[test]
fn function_is_not_a_valid_knowledge_declaration_name() {
    let braced = format!(
        "{HEADER}decision_task check {{ input input: Number; output result: Number = value; knowledge function {{ input item: Number; output result: Number; expression item; }} literal_expression value {{ output result: Number; expression input; }} }}"
    );
    assert!(
        validate(&parse(&braced).unwrap())
            .unwrap_err()
            .contains("function"),
        "braced knowledge function accepted"
    );
}

#[test]
fn inline_predicate_type_and_local_shadowing() {
    let source = decision_expression("List<Number>", "List<Number>", "function(1)");
    assert!(validate(&parse(&source).unwrap()).is_err());
    for text in [
        "remove(input, function(i) i > threshold)",
        "listReplace(input, function(i) i > threshold, threshold)",
        "remove(input, function(item) item > threshold)",
    ] {
        let source = format!(
            "{HEADER}decision_task check {{ input input: List<Number>; input threshold: Number; output result: List<Number> = value; literal_expression value {{ output result: List<Number>; expression {text}; }} }}"
        );
        validate(&parse(&source).unwrap()).unwrap_or_else(|e| panic!("{text}: {e}"));
    }
    let outer = format!(
        "{HEADER}decision_task check {{ input input: List<Number>; input i: Number; output result: List<Number> = value; literal_expression value {{ output result: List<Number>; expression listReplace(input, function(i) i > 0, i); }} }}"
    );
    validate(&parse(&outer).unwrap()).unwrap();
    for text in [
        "remove(input, function(i) i + 1)",
        "listReplace(input, function(i) i > 0, i)",
        "remove(input, function(i) missing > i)",
        "listReplace(input, function(i) i > 0, \"bad\")",
    ] {
        let source = decision_expression("List<Number>", "List<Number>", text);
        assert!(
            validate(&parse(&source).unwrap()).is_err(),
            "accepted {text}"
        );
    }
}

#[test]
fn inline_predicate_preserves_context_dependencies_and_scope() {
    let source = format!(
        "{HEADER}decision_task check {{ input input: List<Number>; output result: List<Number> = value; context value {{ output result: List<Number>; entry selected: List<Number> = remove(input, function(item) item > threshold); entry threshold: Number = 1; entry item: Number = count(selected); result selected; }} }}"
    );
    validate(&parse(&source).unwrap()).unwrap();
    let cycle = source.replace(
        "entry threshold: Number = 1",
        "entry threshold: Number = count(selected)",
    );
    assert!(
        parse(&cycle)
            .and_then(|program| validate(&program))
            .is_err()
    );
}

list_signature!(
    remove_accepts_scoped_predicate,
    "List<Number>",
    "remove(input, function(i) i == 2)"
);
list_signature!(
    list_replace_accepts_scoped_predicate,
    "List<Number>",
    "listReplace(input, function(i) i < 3, 9)"
);
#[test]
fn predicate_list_edits_require_bool_and_matching_replacement_type() {
    for (expression, diagnostic) in [
        ("remove(input, function(i) i + 1)", "Bool"),
        ("listReplace(input, function(i) i + 1, 9)", "Bool"),
        ("listReplace(input, function(i) i == 2, \"bad\")", "Number"),
    ] {
        let source = decision_expression("List<Number>", "List<Number>", expression);
        let error = validate(&parse(&source).unwrap()).unwrap_err();
        assert!(error.contains(diagnostic), "{expression}: {error}");
    }
}

list_signature!(
    distinct_accepts_typed_list,
    "List<Number>",
    "distinct(input)"
);
list_signature!(
    duplicate_values_accepts_typed_list,
    "List<Number>",
    "duplicateValues(input)"
);
list_signature!(
    intersection_accepts_multiple_lists,
    "List<Number>",
    "intersection(input, [2,3])"
);

#[test]
fn list_set_functions_reject_wrong_shapes_and_arity() {
    for (valid, invalid, output, diagnostic) in [
        (
            "distinct([1])",
            "distinct([1], [2])",
            "List<Number>",
            "argument count",
        ),
        (
            "duplicateValues([1])",
            "duplicateValues(1)",
            "List<Number>",
            "List",
        ),
        (
            "intersection([1], [2])",
            "intersection([1], [\"2\"])",
            "List<Number>",
            "String",
        ),
        (
            "intersection([1])",
            "intersection()",
            "List<Number>",
            "argument count",
        ),
        (
            "distinct([[1], [1]])",
            "distinct([[1], [\"1\"]])",
            "List<List<Number>>",
            "String",
        ),
    ] {
        assert_valid_expression("Number", output, valid);
        let source = decision_expression("Number", output, invalid);
        let error = validate(&parse(&source).unwrap()).unwrap_err();
        assert!(error.contains(diagnostic), "{invalid}: {error}");
    }
}

#[test]
fn list_functions_reject_incompatible_elements_and_invalid_predicates() {
    for (valid, invalid, output, expected_diagnostic) in [
        (
            "listContains([1,2], 2)",
            "listContains([1,2], \"2\")",
            "Bool",
            "String",
        ),
        (
            "indexOf([1,2], 2)",
            "indexOf([1,2], \"2\")",
            "List<Number>",
            "String",
        ),
        (
            "append([1], 2)",
            "append([1], \"2\")",
            "List<Number>",
            "String",
        ),
        (
            "prepend([1], 2)",
            "prepend([1], \"2\")",
            "List<Number>",
            "String",
        ),
        (
            "concatenate([1], [2])",
            "concatenate([1], [\"2\"])",
            "List<Number>",
            "String",
        ),
        (
            "union([1], [2])",
            "union([1], [\"2\"])",
            "List<Number>",
            "String",
        ),
        (
            "intersection([1], [2])",
            "intersection([1], [\"2\"])",
            "List<Number>",
            "String",
        ),
        (
            "sublist([1,2], 1)",
            "sublist([1,2], \"1\")",
            "List<Number>",
            "Number",
        ),
        (
            "sublist([1,2], 1, 2)",
            "sublist([1,2], 1, \"2\")",
            "List<Number>",
            "Number",
        ),
        (
            "insertBefore([1], 1, 2)",
            "insertBefore([1], 1, \"2\")",
            "List<Number>",
            "String",
        ),
        (
            "insertAfter([1], 1, 2)",
            "insertAfter([1], 1, \"2\")",
            "List<Number>",
            "String",
        ),
        (
            "listReplace([1], 1, 2)",
            "listReplace([1], 1, \"2\")",
            "List<Number>",
            "String",
        ),
        (
            "remove([1], function(i) i == 1)",
            "remove([1], function(i) i + 1)",
            "List<Number>",
            "Bool",
        ),
        (
            "listReplace([1], function(i) i == 1, 9)",
            "listReplace([1], function(i) i + 1, 9)",
            "List<Number>",
            "Bool",
        ),
    ] {
        assert_valid_expression("Number", output, valid);
        let source = decision_expression("Number", output, invalid);
        let program = parse(&source).unwrap_or_else(|e| panic!("{invalid} failed to parse: {e}"));
        let error = validate(&program).unwrap_err();
        assert!(
            !error.contains("unknown knowledge model") && error.contains(expected_diagnostic),
            "{invalid}: expected {expected_diagnostic} diagnostic, got {error}"
        );
    }
}

#[test]
fn list_set_functions_typecheck_structural_dictionary_elements() {
    for expression in [
        "distinct([{x: 2}, {x: 2}])",
        "duplicateValues([{x: 2}, {x: 2}])",
        "intersection([{x: 2}, {x: 1}], [{x: 2}])",
    ] {
        assert_valid_expression("Number", "List<Dictionary<Number>>", expression);
    }
}

#[test]
fn list_dictionary_fixture_has_a_homogeneous_element_type() {
    assert_valid_expression("Number", "List<Dictionary<Number>>", "[{x: 2}, {x: 2}]");
}

#[test]
fn sort_accepts_natural_ascending_for_numbers_and_code_point_strings() {
    for (expression, output) in [
        ("sort([3,1,2])", "List<Number>"),
        ("sort([3,1,2], \"asc\")", "List<Number>"),
        ("sort([\"banana\",\"Apple\",\"café\"])", "List<String>"),
        ("sort([])", "List<Number>"),
    ] {
        assert_valid_expression("Number", output, expression);
    }
}

#[test]
fn sort_accepts_descending_order() {
    assert_valid_expression("Number", "List<Number>", "sort([3,1,2], \"desc\")");
}

#[test]
fn sort_accepts_explicit_priority_order() {
    assert_valid_expression(
        "Number",
        "List<String>",
        "sort([\"m\",\"s\",\"xl\"], [\"s\",\"m\",\"l\"])",
    );
}

#[test]
fn sort_accepts_comparable_temporal_kinds() {
    for (expression, output) in [
        (
            "sort([date(\"2026-01-02\"),date(\"2026-01-01\")])",
            "List<Date>",
        ),
        (
            "sort([time(\"12:00:00\"),time(\"11:00:00\")])",
            "List<Time>",
        ),
        (
            "sort([datetime(\"2026-01-02T00:00:00Z\"),datetime(\"2026-01-01T00:00:00Z\")])",
            "List<DateTime>",
        ),
        (
            "sort([dtDuration(\"PT2H\"),dtDuration(\"PT1H\")])",
            "List<DTDuration>",
        ),
        (
            "sort([ymDuration(\"P2M\"),ymDuration(\"P1M\")])",
            "List<YMDuration>",
        ),
    ] {
        assert_valid_expression("Number", output, expression);
    }
}

#[test]
fn sort_constrains_empty_source_bindings_without_accepting_bool() {
    assert_valid_expression(
        "Number",
        "List<List<Number>>",
        "for x in [] return sort([x])",
    );
    let source = decision_expression("Number", "List<List<Bool>>", "for x in [] return sort([x])");
    assert!(parse(&source).and_then(|p| validate(&p)).is_err());
}

#[test]
fn sort_rejects_unsupported_elements_and_invalid_order_shapes() {
    for (expression, output, diagnostic) in [
        ("sort([true,false])", "List<Bool>", Some("Bool")),
        (
            "sort([[1],[2]])",
            "List<List<Number>>",
            Some("List<Number>"),
        ),
        ("sort([1,2], true)", "List<Number>", None),
        ("sort([1,2], [\"2\"])", "List<Number>", None),
        ("sort([1,2], 1)", "List<Number>", None),
        ("sort([1,2], \"asc\", \"desc\")", "List<Number>", None),
        (
            "sort([date(\"2026-01-01\"),datetime(\"2026-01-01T00:00:00Z\")])",
            "List<Number>",
            None,
        ),
    ] {
        let source = decision_expression("Number", output, expression);
        let error = parse(&source)
            .and_then(|p| validate(&p))
            .expect_err("invalid expression accepted");
        assert!(
            !error.contains("unknown knowledge model"),
            "{expression}: {error}"
        );
        if let Some(diagnostic) = diagnostic {
            assert!(
                !error.contains("decision result type mismatch"),
                "{expression}: {error}"
            );
            assert!(error.contains(diagnostic), "{expression}: {error}");
        }
    }
}

#[test]
fn string_join_accepts_omitted_separator() {
    assert_valid_expression("String", "String", "stringJoin([\"a\",\"b\"])");
}

#[test]
fn string_join_accepts_completed_string_wrappers() {
    for expression in [
        "stringJoin([\"a\",\"b\"], \"\", \"[\", \"]\")",
        "stringJoin([], \"-\", \"[\", \"]\")",
    ] {
        assert_valid_expression("String", "String", expression);
    }
}

#[test]
fn string_join_rejects_non_string_parts_and_invalid_wrapper_arity() {
    for expression in [
        "stringJoin([1], \"-\", \"[\", \"]\")",
        "stringJoin([\"a\"], 1, \"[\", \"]\")",
        "stringJoin([\"a\"], \"-\", [\"[\"], \"]\")",
        "stringJoin([\"a\"], \"-\", \"[\")",
        "stringJoin([\"a\"], \"-\", \"[\", \"]\", \"!\")",
    ] {
        let source = decision_expression("String", "String", expression);
        let error = parse(&source)
            .and_then(|p| validate(&p))
            .expect_err("invalid expression accepted");
        assert!(
            !error.contains("unknown knowledge model"),
            "{expression}: {error}"
        );
    }
}

#[test]
fn zip_string_join_accepts_scalar_and_per_column_formatting() {
    for expression in [
        "zipStringJoin([[\"a\",\"b\"],[\"1\",\"2\"]])",
        "zipStringJoin([[\"a\",\"b\"],[\"1\",\"2\"]], \"-\", \"[\", \"]\")",
        "zipStringJoin([[\"a\"],[\"1\"],[\"x\"]], [\"-\",\":\"], [\"<\",\"(\",\"{\"], [\">\",\")\",\"}\"])",
        "zipStringJoin([[],[]], \"-\")",
        "zipStringJoin([], [], [], [])",
        "zipStringJoin([], \"-\", \"[\", \"]\")",
    ] {
        assert_valid_expression("Number", "List<String>", expression);
    }
}

#[test]
fn zip_string_join_accepts_per_gap_delimiters_and_per_column_wrappers() {
    assert_valid_expression(
        "Number",
        "List<String>",
        "zipStringJoin([[\"a\"],[\"1\"],[\"x\"]], [\"-\",\":\"], [\"<\",\"(\",\"{\"], [\">\",\")\",\"}\"])",
    );
}

#[test]
fn zip_string_join_rejects_wrong_column_and_format_types() {
    for expression in [
        "zipStringJoin([\"a\",\"b\"])",
        "zipStringJoin([[1],[2]])",
        "zipStringJoin([[\"a\"]], 1)",
        "zipStringJoin([[\"a\"]], [1])",
        "zipStringJoin([[\"a\"]], \"-\", [1])",
        "zipStringJoin([[\"a\"]], \"-\", \"[\", [1])",
        "zipStringJoin([[\"a\"]], \"-\", \"[\", \"]\", \"!\")",
    ] {
        let source = decision_expression("Number", "List<String>", expression);
        let error = parse(&source)
            .and_then(|p| validate(&p))
            .expect_err("invalid expression accepted");
        assert!(
            !error.contains("unknown knowledge model"),
            "{expression}: {error}"
        );
    }
}

#[test]
fn seq_accepts_fractional_and_opposite_sign_steps() {
    for expression in ["seq(0,1,0.25)", "seq(5,10,-1)"] {
        assert_valid_expression("Number", "List<Number>", expression);
    }
}

#[test]
fn seq_and_colon_typecheck_numeric_endpoints_and_steps() {
    for expression in [
        "seq(5,10)",
        "seq(10,5)",
        "seq(0,1,0.25)",
        "seq(5,10,-1)",
        "10:5",
        "0.5:2.5",
    ] {
        assert_valid_expression("Number", "List<Number>", expression);
    }
    for expression in [
        "seq(1)",
        "seq(1,2,3,4)",
        "seq(\"1\",2)",
        "seq(1,2,true)",
        "\"1\":2",
    ] {
        let source = decision_expression("Number", "List<Number>", expression);
        let error = parse(&source)
            .and_then(|p| validate(&p))
            .expect_err("invalid expression accepted");
        assert!(
            !error.contains("unknown knowledge model"),
            "{expression}: {error}"
        );
    }
}

#[test]
fn any_accepts_typed_boolean_list_and_empty_list() {
    for expression in ["any([false,false,true])", "any([])"] {
        assert_valid_expression("Bool", "Bool", expression);
    }
}

#[test]
fn boolean_aggregates_constrain_empty_iteration_bindings() {
    for expression in ["for x in [] return all(x)", "for x in [] return any(x)"] {
        assert_valid_expression("Bool", "List<Bool>", expression);
    }
}

#[test]
fn boolean_aggregates_allow_knowledge_shadowing() {
    for name in ["all", "any"] {
        let source = format!(
            "{HEADER}decision_task check {{ input input: Number; output result: Number = value; knowledge {name} {{ input number: Number; output result: Number; expression number; }} literal_expression value {{ output result: Number; expression {name}(input); }} }}"
        );
        validate(&parse(&source).unwrap()).unwrap();
    }
}

#[test]
fn all_and_any_require_typed_boolean_lists() {
    for expression in [
        "all([true,true,false])",
        "any([false,false,true])",
        "all([true,true])",
        "any([false,false])",
        "all([])",
        "any([])",
    ] {
        assert_valid_expression("Bool", "Bool", expression);
    }
    for expression in ["all(input)", "any(input)"] {
        assert_valid_expression("List<Bool>", "Bool", expression);
    }
    for expression in [
        "all([1])",
        "any([1])",
        "all([true, 1])",
        "any([false, 1])",
        "all(true)",
        "any(1)",
        "all([true], [false])",
        "any([true], [false])",
    ] {
        let source = decision_expression("Bool", "Bool", expression);
        let error = parse(&source)
            .and_then(|p| validate(&p))
            .expect_err("invalid expression accepted");
        assert!(
            !error.contains("unknown knowledge model"),
            "{expression}: {error}"
        );
    }
}

#[test]
fn public_program_ast_preserves_declaration_and_field_order() {
    let source = r##"# file comment
namespace orders;
version "v;#1";
# preceding record comment
First = {"a,b:c#d": List<Dictionary<Number>>, second: Bool};
enum State:
  waiting;
  ready;
# between declarations
Second = {first: First};
enum Result:
  yes;
  no;
"##;
    let program = parse(source).unwrap();
    assert_eq!(
        (program.namespace.as_str(), program.version.as_str()),
        ("orders", "v;#1")
    );
    assert_eq!(
        program
            .records
            .iter()
            .map(|record| record.name.as_str())
            .collect::<Vec<_>>(),
        ["First", "Second"]
    );
    assert_eq!(program.records[0].fields[0].0, "a,b:c#d");
    assert_eq!(
        program.records[0].fields[0].1.to_string(),
        "List<Dictionary<Number>>"
    );
    assert_eq!(program.records[0].fields[1].0, "second");
    assert_eq!(
        program
            .enums
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["State", "Result"]
    );
    assert_eq!(program.enums[0].variants, ["waiting", "ready"]);
    assert_eq!(program.enums[1].variants, ["yes", "no"]);
}

#[test]
fn record_spacing_rejections_remain_rejected() {
    for declaration in [
        "Order={value: Number};",
        "Order = {value: List <Number>};",
        "Order = {value: List< Number>};",
    ] {
        let source = format!("{HEADER}{declaration}");
        assert!(parse(&source).is_err(), "accepted: {declaration}");
    }
}

#[test]
fn record_field_separators_do_not_hide_empty_fields() {
    let source = format!("{HEADER}Order = {{first: Number,, second: Bool}};");
    assert!(parse(&source).is_err(), "accepted an empty record field");
}

#[test]
fn enum_layout_and_record_delimiters_reject_malformed_sources() {
    for source in [
        format!("{HEADER}enum State:\n waiting;"),
        format!("{HEADER}enum State:\n    waiting;"),
        format!("{HEADER}enum State:\n  waiting"),
        format!("{HEADER}enum State:\n  waiting;\n  ready extra;"),
        format!("{HEADER}Record = {{value: List<Number>;"),
        format!("{HEADER}Record = {{value: Number}}; extra"),
        format!("{HEADER}Record = {{value: List<Number>}}"),
    ] {
        assert!(parse(&source).is_err(), "accepted: {source}");
    }
}

#[test]
fn quotes_preserve_backslashes_and_punctuation_but_not_inline_comments() {
    use blkit::expr::Expr;

    assert_eq!(
        blkit::expr::expression("\"a;,{#}..b\"").unwrap(),
        Expr::String("a;,{#}..b".into())
    );
    assert_eq!(
        blkit::expr::expression(r#""order-(\d+);#""#).unwrap(),
        Expr::String("order-(\\d+);#".into())
    );
    let source = format!(
        "{HEADER}# full-line comment\nOrder = {{\"key;#,:\": String}}; # not a full-line comment"
    );
    assert!(parse(&source).is_err());
    for text in [
        "\"unterminated",
        "1.5e",
        "1e+",
        "1 @ 2",
        "[1, 2",
        "{a: 1",
        "(1 + 2]",
    ] {
        assert!(blkit::expr::expression(text).is_err(), "accepted: {text}");
    }
}

#[test]
fn standalone_expression_rejects_full_line_comments() {
    assert!(blkit::expr::expression("# comment\n1").is_err());
}

#[test]
fn expression_rejects_escaped_quotes_as_before() {
    assert!(blkit::expr::expression(r#""a\"b""#).is_err());
}

#[test]
fn declaration_body_preserves_legacy_quoted_expression_operands() {
    let source = decision_expression("Number", "String", r#""a\" + "b""#);
    let program = parse(&source).unwrap();
    assert!(matches!(
        &program.decisions[0].nodes[0].kind,
        blkit::decision::DecisionKind::Literal(blkit::expr::Expr::Binary(_, op, _)) if op == "+"
    ));
}

#[test]
fn expression_splits_legacy_quoted_binary_operands() {
    use blkit::expr::{self, Expr};

    assert_eq!(
        expr::expression(r#""a\" + "b""#).unwrap(),
        Expr::Binary(
            Box::new(Expr::String("a\\".into())),
            "+".into(),
            Box::new(Expr::String("b".into()))
        )
    );
}

#[test]
fn expression_splits_legacy_quoted_list_elements_after_unicode() {
    use blkit::expr::{self, Expr};

    assert_eq!(
        expr::expression(r#"["é\", "b"]"#).unwrap(),
        Expr::List(vec![Expr::String("é\\".into()), Expr::String("b".into())])
    );
}

#[test]
fn expression_keeps_literal_backslash_before_closing_quote() {
    use blkit::expr::{self, Expr};

    assert_eq!(
        expr::expression(r#""a\""#).unwrap(),
        Expr::String("a\\".into())
    );
}

#[test]
fn expression_keeps_literal_backslash_before_closing_quote_in_call() {
    use blkit::expr::{self, Expr};

    assert_eq!(
        expr::expression(r#"string("a\")"#).unwrap(),
        Expr::Call("string".into(), vec![Expr::String("a\\".into())])
    );
}

#[test]
fn expression_keeps_literal_backslash_quote_after_unicode_prefix() {
    use blkit::expr::{self, Expr};

    assert_eq!(
        expr::expression(r#""é" + "a\""#).unwrap(),
        Expr::Binary(
            Box::new(Expr::String("é".into())),
            "+".into(),
            Box::new(Expr::String("a\\".into()))
        )
    );
}

#[test]
fn expression_error_after_recovered_quote_uses_original_byte_offset() {
    let error = blkit::expr::expression(r#""é" + "a\" @"#).unwrap_err();
    assert!(error.contains("12..13"), "{error}");
}

#[test]
fn expression_rejects_unterminated_quote_with_backslash() {
    assert!(blkit::expr::expression(r#""a\"#).is_err());
}

#[test]
fn leading_null_cannot_be_used_in_list_expression() {
    assert!(blkit::expr::expression("[null + 1]").is_err());
}

#[test]
fn leading_null_cannot_be_used_in_grouped_expression() {
    assert!(blkit::expr::expression("(null + 1)").is_err());
}

#[test]
fn expression_colon_and_or_share_left_associative_precedence() {
    use blkit::expr::{self, Expr};

    assert_eq!(
        expr::expression("1:2 or 3").unwrap(),
        Expr::Binary(
            Box::new(Expr::Call(
                "__bl_sequence".into(),
                vec![Expr::Number("1".into()), Expr::Number("2".into())]
            )),
            "or".into(),
            Box::new(Expr::Number("3".into()))
        )
    );
}

#[test]
fn null_as_sole_list_element_keeps_empty_list_lowering() {
    use blkit::expr::{self, Expr};

    assert_eq!(expr::expression("[null]").unwrap(), Expr::List(vec![]));
    assert!(expr::expression("[null, 1]").is_err());
}

#[test]
fn iteration_keyword_requires_matching_body_keyword() {
    use blkit::expr;

    for text in [
        "for x in [1] satisfies x",
        "some x in [1] return true",
        "every x in [1] return true",
    ] {
        assert!(expr::expression(text).is_err(), "accepted {text}");
    }
}

#[test]
fn expression_ast_preserves_precedence_and_lowerings() {
    use blkit::expr::{self, Expr};

    let expected = Expr::Binary(
        Box::new(Expr::Binary(
            Box::new(Expr::Number("0".into())),
            "-".into(),
            Box::new(Expr::Binary(
                Box::new(Expr::Number("2".into())),
                "**".into(),
                Box::new(Expr::Binary(
                    Box::new(Expr::Number("3".into())),
                    "**".into(),
                    Box::new(Expr::Number("2".into())),
                )),
            )),
        )),
        "+".into(),
        Box::new(Expr::Binary(
            Box::new(Expr::Number("4".into())),
            "*".into(),
            Box::new(Expr::Number("5".into())),
        )),
    );
    assert_eq!(expr::expression("-2 ** 3 ** 2 + 4 * 5").unwrap(), expected);
    assert_eq!(
        expr::expression("{a: 1}[\"a\"]").unwrap(),
        Expr::Call(
            "__bl_index".into(),
            vec![
                Expr::Call(
                    "__bl_dictionary".into(),
                    vec![Expr::String("a".into()), Expr::Number("1".into())]
                ),
                Expr::String("a".into()),
            ]
        )
    );
}

#[test]
fn range_ast_distinguishes_bounds_and_delimiters_from_lists() {
    use blkit::expr::{self, Expr};

    for (text, lower, upper, lower_closed, upper_closed) in [
        ("[1..5]", Some("1"), Some("5"), true, true),
        ("(1..5]", Some("1"), Some("5"), false, true),
        ("[null..5)", None, Some("5"), true, false),
        ("(1..null)", Some("1"), None, false, false),
    ] {
        assert_eq!(
            expr::expression(text).unwrap(),
            Expr::Range(
                lower.map(|n| Box::new(Expr::Number(n.into()))),
                upper.map(|n| Box::new(Expr::Number(n.into()))),
                lower_closed,
                upper_closed
            ),
            "{text}"
        );
    }
    assert_eq!(
        expr::expression("[1,5]").unwrap(),
        Expr::List(vec![Expr::Number("1".into()), Expr::Number("5".into())])
    );
    for text in ["(null)", "[1..5", "[1..5}"] {
        assert!(expr::expression(text).is_err(), "accepted: {text}");
    }
}

#[test]
fn decision_table_ast_preserves_rule_priority_and_default_order() {
    use blkit::{decision::DecisionKind, expr::Expr};

    let source = format!(
        "{HEADER}decision_task check {{ input amount: Number; output result: Number = table; decision_table table {{ output result: Number; policy PRIORITY; input value: Number = amount; output score: Number; priority 9; priority 3; rule value matches (< 10, [20..30]) -> 9; rule value >= 10 -> 3; default 0; }} }}"
    );
    let program = parse(&source).unwrap();
    let model = &program.decisions[0];
    assert_eq!(model.outputs[0].2, "table");
    let DecisionKind::Table(table) = &model.nodes[0].kind else {
        panic!("expected table")
    };
    assert_eq!(table.policy, "PRIORITY");
    assert_eq!(table.inputs[0].0, "value");
    assert_eq!(table.outputs[0].0, "score");
    assert_eq!(
        table.priorities,
        vec![
            vec![Expr::Number("9".into())],
            vec![Expr::Number("3".into())]
        ]
    );
    assert!(matches!(&table.rules[0].0, Expr::Binary(_, op, _) if op == "or"));
    assert!(matches!(&table.rules[1].0, Expr::Binary(_, op, _) if op == ">="));
    assert_eq!(table.rules[0].1, [Expr::Number("9".into())]);
    assert_eq!(table.rules[1].1, [Expr::Number("3".into())]);
    assert_eq!(
        table.default.as_deref(),
        Some([Expr::Number("0".into())].as_slice())
    );
    for broken in [
        source.replace("rule value >= 10 -> 3;", "rule value >= 10 -> ;"),
        source.replace("rule value >= 10 -> 3;", "rule other matches (< 10) -> 3;"),
        source.replace("policy PRIORITY;", "policy UNKNOWN;"),
    ] {
        assert!(parse(&broken).is_err(), "accepted: {broken}");
    }
    assert!(blkit::expr::expression("value matches (< 10)").is_err());
}

#[test]
fn context_ast_orders_dependencies_without_reordering_authored_nodes() {
    use blkit::{decision::DecisionKind, expr::Expr};

    let source = format!(
        "{HEADER}decision_task check {{ input amount: Number; output result: Number = final; context final {{ output result: Number; entry total: Number = base + 1; entry base: Number = amount; result total; }} literal_expression spare {{ output result: Number; expression amount; }} }}"
    );
    let model = &parse(&source).unwrap().decisions.remove(0);
    assert_eq!(
        model
            .nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect::<Vec<_>>(),
        ["final", "spare"]
    );
    let DecisionKind::Context { entries, result } = &model.nodes[0].kind else {
        panic!("expected context")
    };
    assert_eq!(
        entries
            .iter()
            .map(|(name, _, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["base", "total"]
    );
    assert_eq!(result, &Expr::Name("total".into()));
}
