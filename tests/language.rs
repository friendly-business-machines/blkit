use blkit::{parse, transpile, validate};

const HEADER: &str = "namespace orders\nversion \"1.0\"\n";

#[test]
fn declarations_and_typed_process_parse() {
    let source = format!(
        "{HEADER}\ntype Order:\n  total: Number\n  tags: List<String>\n\nenum Decision:\n  approved\n  review\n\nprocess approve(input: Order) -> Decision:\n  node start = start\n  node done = end\n  link start -> done(Decision.approved)\n"
    );
    let ast = parse(&source).unwrap();
    assert_eq!(ast.namespace, "orders");
    assert_eq!(ast.version, "1.0");
    assert_eq!(ast.records.len(), 1);
    assert_eq!(ast.enums.len(), 1);
    assert_eq!(ast.processes.len(), 1);
}

#[test]
fn decision_model_parses_literal_context_dependencies_and_knowledge() {
    let source = format!(
        "{HEADER}decision price(input: Number) -> Number:\n  knowledge add(a: Number, b: Number) -> Number = a\n  node base: Number = literal input\n  node final: Number = context\n    entry subtotal: Number = base\n    result subtotal\n  link base -> final\n  output final\n"
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
        "{HEADER}decision price(input: Number) -> Number:\n  node value: Number = context\n    result input\n  output value\n"
    );
    for broken in [
        source.replace("    result input", "   result input"),
        source.replace("  output value", "  output value extra"),
        source.replace(
            "  node value: Number = context",
            "  node value: Number = mystery",
        ),
    ] {
        assert!(parse(&broken).is_err(), "accepted: {broken}");
    }
}

#[test]
fn decision_models_validate_dependent_types_and_knowledge_calls() {
    let source = format!(
        "{HEADER}decision price(input: Number) -> Number:\n  knowledge fee(amount: Number) -> Number = amount\n  node base: Number = literal input\n  node outcome: Number = context\n    entry subtotal: Number = fee(base)\n    result subtotal\n  link base -> outcome\n  output outcome\n"
    );
    validate(&parse(&source).unwrap()).unwrap();
    for (broken, expected) in [
        (
            source.replace("fee(base)", "fee(true)"),
            "knowledge argument",
        ),
        (
            source.replace("fee(base)", "unknown(base)"),
            "unknown knowledge",
        ),
        (
            source.replace("entry subtotal: Number", "entry subtotal: Bool"),
            "decision result type",
        ),
        (
            source.replace("entry subtotal: Number", "entry subtotal: Mystery"),
            "unknown type",
        ),
        (
            source.replace("  link base -> outcome\n", ""),
            "unknown name",
        ),
        (
            source.replace(
                "knowledge fee(amount: Number) -> Number = amount",
                "knowledge fee(amount: Number) -> Number = fee(amount)",
            ),
            "knowledge cycle",
        ),
        (
            source.replace("link base -> outcome", "link unknown -> outcome"),
            "unknown decision node",
        ),
        (
            source.replace(
                "  output outcome",
                "  link outcome -> base\n  output outcome",
            ),
            "decision cycle",
        ),
        (
            source.replace("result subtotal", "result missing"),
            "unknown name",
        ),
        (
            source.replace("node outcome: Number", "node outcome: Bool"),
            "decision result type",
        ),
        (
            source.replace(
                "  output outcome",
                "  node base: Number = literal input\n  output outcome",
            ),
            "duplicate",
        ),
    ] {
        let error = validate(&parse(&broken).unwrap()).unwrap_err();
        assert!(error.contains(expected), "{broken}: {error}");
    }
}

#[test]
fn knowledge_calls_are_scoped_to_their_decision_model() {
    let source = format!(
        "{HEADER}decision first(input: Number) -> Number:\n  knowledge fee(amount: Number) -> Number = amount\n  node result: Number = literal fee(input)\n  output result\ndecision second(input: Bool) -> Bool:\n  knowledge fee(amount: Bool) -> Bool = amount\n  node result: Bool = literal fee(input)\n  output result\n"
    );
    validate(&parse(&source).unwrap()).unwrap();
    let outside = format!("{source}task misuse(input: Number) -> Number:\n  return fee(input)\n");
    assert!(
        validate(&parse(&outside).unwrap())
            .unwrap_err()
            .contains("unknown knowledge")
    );
}

#[test]
fn decision_tables_parse_expressions_multiple_outputs_and_defaults() {
    let source = format!(
        "{HEADER}type Quote:\n  price: Number\n  tier: String\ndecision quote(input: Number) -> Quote:\n  node result: Quote = table PRIORITY\n    input amount: Number = input\n    output price: Number\n    output tier: String\n    priority 5, \"express\"\n    priority 2, \"regular\"\n    rule amount > 100 -> 5, \"express\"\n    rule amount <= 100 -> 2, \"regular\"\n    default 0, \"none\"\n  output result\n"
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
                "rule amount > 100 -> 5, \"express\"",
                "rule amount > 100 -> 5",
            ),
            "output",
        ),
        (
            source.replace("table PRIORITY", "table COLLECT SUM"),
            "aggregation",
        ),
        (
            source.replace("    priority 2, \"regular\"", "    priority 5, \"express\""),
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
fn datetime_comparisons_and_wait_expressions_are_typed() {
    let source = format!(
        "{HEADER}type Window:\n  opens: DateTime\n  closes: DateTime\ntask early(input: Window) -> Bool:\n  return input.opens < input.closes\nprocess later(input: Window) -> Window:\n  node start = start\n  node delay = pause_for \"10m\"\n  node until = pause_until input.closes\n  node done = end\n  link start -> delay\n  link delay -> until\n  link until -> done(input)\n"
    );
    validate(&parse(&source).unwrap()).unwrap();
    for (broken, expected) in [
        (
            source.replace("pause_until input.closes", "pause_until true"),
            "DateTime",
        ),
        (
            source.replace("pause_for \"10m\"", "pause_for \"0s\""),
            "duration",
        ),
        (
            source.replace(
                "return input.opens < input.closes",
                "return input.opens < true",
            ),
            "matching types",
        ),
    ] {
        let error = parse(&broken)
            .and_then(|program| validate(&program))
            .unwrap_err();
        assert!(error.contains(expected), "{broken}: {error}");
    }
}

#[test]
fn missing_header_is_reported() {
    assert!(
        parse("version \"1.0\"\n")
            .unwrap_err()
            .contains("namespace")
    );
    assert!(parse("namespace orders\n").unwrap_err().contains("version"));
}

#[test]
fn malformed_signature_is_reported() {
    let source =
        format!("{HEADER}process approve(input Order) -> Decision:\n  return Decision.approved\n");
    assert!(parse(&source).unwrap_err().contains("process signature"));
}

#[test]
fn nested_branch_and_boolean_expression_parse() {
    let source = format!(
        "{HEADER}task approve(input: Order) -> Decision:\n  if input.total > 1000 and not input.blocked:\n    if input.tags == [\"vip\", \"repeat\"]:\n      return Decision.review\n    else:\n      return Decision.approved\n  else:\n    return Decision.rejected\n"
    );
    let ast = parse(&source).unwrap();
    assert_eq!(ast.tasks[0].body.len(), 1);
}

#[test]
fn malformed_expression_is_rejected() {
    let source =
        format!("{HEADER}task approve(input: Order) -> Decision:\n  return input.total >\n");
    assert!(parse(&source).unwrap_err().contains("expression"));
}

#[test]
fn known_declarations_resolve_and_duplicates_fail() {
    let source = format!(
        "{HEADER}type Order:\n  total: Number\nenum Decision:\n  approved\nprocess approve(input: Order) -> Decision:\n  node start = start\n  node done = end\n  link start -> done(Decision.approved)\n"
    );
    validate(&parse(&source).unwrap()).unwrap();
    let duplicate = source.replace(
        "enum Decision:",
        "type Order:\n  total: Number\nenum Decision:",
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
            "{HEADER}type Order:\n  total: {ty}\ntask echo(input: Order) -> Order:\n  return input\n"
        );
        assert!(validate(&parse(&source).unwrap()).unwrap_err().contains(ty));
    }
}

#[test]
fn expression_types_and_list_elements_are_checked() {
    let valid = format!(
        "{HEADER}type Order:\n  total: Number\n  blocked: Bool\nenum Decision:\n  approved\n  review\ntask decide(input: Order) -> Decision:\n  if input.total > 1000 and not input.blocked:\n    return Decision.review\n  else:\n    return Decision.approved\ntask amounts(input: Order) -> List<Number>:\n  return [1, 2.5]\n"
    );
    validate(&parse(&valid).unwrap()).unwrap();
    let bad = valid.replace("[1, 2.5]", "[1, \"two\"]");
    assert!(
        validate(&parse(&bad).unwrap())
            .unwrap_err()
            .contains("List<Number>")
    );
}

#[test]
fn generated_rust_names_are_checked() {
    for declarations in [
        "task match(input: Number) -> Number:\n  return input\n",
        "type Vec:\n  value: Number\ntask echo(input: Vec) -> Vec:\n  return input\n",
        "type NAMESPACE:\n  value: Number\ntask echo(input: NAMESPACE) -> NAMESPACE:\n  return input\n",
        "type Order:\n  match: Number\ntask echo(input: Order) -> Order:\n  return input\n",
        "enum Decision:\n  match\ntask echo(input: Number) -> Decision:\n  return Decision.match\n",
        "task echo(match: Number) -> Number:\n  return match\n",
    ] {
        let source = format!("{HEADER}{declarations}");
        assert!(
            transpile(&source).is_err(),
            "accepted invalid generated name: {source}"
        );
    }
}

#[test]
fn empty_list_equality_does_not_depend_on_operand_order() {
    let left = format!("{HEADER}task empty(input: List<Number>) -> Bool:\n  return [] == input\n");
    let right = left.replace("[] == input", "input == []");
    validate(&parse(&left).unwrap()).unwrap();
    validate(&parse(&right).unwrap()).unwrap();
}

#[test]
fn by_value_record_cycles_fail_but_list_recursion_is_allowed() {
    let direct = format!(
        "{HEADER}type Node:\n  next: Node\ntask echo(input: Node) -> Node:\n  return input\n"
    );
    assert!(
        validate(&parse(&direct).unwrap())
            .unwrap_err()
            .contains("recursive")
    );
    let indirect = format!(
        "{HEADER}type A:\n  b: B\ntype B:\n  a: A\ntask echo(input: A) -> A:\n  return input\n"
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
fn every_branch_must_return() {
    let base =
        format!("{HEADER}task decide(input: Number) -> Number:\n  if input > 10:\n    return 1\n");
    assert!(
        validate(&parse(&base).unwrap())
            .unwrap_err()
            .contains("missing return")
    );
    let complete = format!("{base}  else:\n    return 2\n");
    validate(&parse(&complete).unwrap()).unwrap();
}

#[test]
fn field_variant_and_return_errors_are_reported() {
    let base = format!(
        "{HEADER}type Order:\n  total: Number\nenum Decision:\n  approved\ntask decide(input: Order) -> Decision:\n  return Decision.approved\n"
    );
    for (broken, expected) in [
        (
            base.replace("Decision.approved", "Decision.missing"),
            "variant",
        ),
        (base.replace("Decision.approved", "input.missing"), "field"),
        (
            base.replace("Decision.approved", "input.total"),
            "return type",
        ),
        (base.replace("Decision.approved", "1 and true"), "Bool"),
    ] {
        assert!(
            validate(&parse(&broken).unwrap())
                .unwrap_err()
                .contains(expected)
        );
    }
}
