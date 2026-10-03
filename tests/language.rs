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
fn temporal_types_and_literal_constructors_validate() {
    let source = format!(
        "{HEADER}type Clock:\n  day: Date\n  time: Time\n  instant: DateTime\ntask valid(input: Clock) -> Bool:\n  return input.day < date(\"2026-10-02\") and input.time <= time(\"09:30:00.250\") and input.instant == dateTime(\"2026-10-02T09:30:00+02:00\")\n"
    );
    transpile(&source).unwrap();
    for (from, to) in [
        ("2026-10-02", "2026-02-30"),
        ("2026-10-02", "2026-1-2"),
        ("09:30:00.250", "24:00:00"),
        ("09:30:00.250", "9:30:00"),
        ("09:30:00.250", "09:30:00+02:00"),
        ("2026-10-02T09:30:00+02:00", "2026-10-02T09:30:00"),
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
        let source = format!("{HEADER}task check(input: Number) -> Bool:\n  return {expr}\n");
        transpile(&source).unwrap_or_else(|e| panic!("{expr}: {e}"));
    }
    for (ty, literal) in [
        ("Date", "date(\"2026-10-02\")"),
        ("DateTime", "dateTime(\"2026-10-02T09:30:00Z\")"),
        ("Time", "time(\"09:30:00\")"),
    ] {
        let source = format!(
            "{HEADER}task check(input: {ty}) -> Bool:\n  return input in [{literal}..null)\n"
        );
        transpile(&source).unwrap_or_else(|e| panic!("{ty}: {e}"));
    }
    for expr in [
        "input in [1..date(\"2026-10-02\")]",
        "input in [\"a\"..\"z\"]",
        "input in [5..1]",
        "null == null",
        "[null..null] == [null..null]",
    ] {
        let source = format!("{HEADER}task check(input: Number) -> Bool:\n  return {expr}\n");
        assert!(transpile(&source).is_err(), "accepted: {expr}");
    }
    for expr in ["[1, 2.5]", "[1.25, 2.50]"] {
        let source =
            format!("{HEADER}task items(input: Number) -> List<Number>:\n  return {expr}\n");
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
        let source = format!("{HEADER}task check(input: Number) -> Bool:\n  return {expr}\n");
        assert_eq!(transpile(&source).is_ok(), valid, "{expr}");
    }
}

#[test]
fn decision_table_unary_tests_are_typed_and_table_only() {
    let base = format!(
        "{HEADER}decision price(input: Number) -> Number:\n  node result: Number = table FIRST\n    input amount: Number = input\n    output price: Number\n    rule amount matches (< 10, [20..30]) or amount > 100 -> 5\n    default 0\n  output result\n"
    );
    transpile(&base).unwrap();
    for broken in [
        base.replace("(< 10, [20..30])", "()"),
        base.replace("(< 10, [20..30])", "(< 10, [date(\"2026-01-01\")..null))"),
        base.replace("amount matches", "unknown matches"),
        base.replace("(< 10, [20..30])", "(< 10, >= date(\"2026-01-01\"))"),
    ] {
        assert!(transpile(&broken).is_err(), "accepted: {broken}");
    }
    let outside = format!(
        "{HEADER}task test(input: Number) -> Bool:\n  return input matches (< 10, [20..30])\n"
    );
    assert!(transpile(&outside).is_err());
    for (ty, literal) in [
        ("Date", "date(\"2026-01-01\")"),
        ("DateTime", "dateTime(\"2026-01-01T00:00:00Z\")"),
        ("Time", "time(\"09:00:00\")"),
    ] {
        let source = format!(
            "{HEADER}decision eligible(input: {ty}) -> Bool:\n  node result: Bool = table FIRST\n    input value: {ty} = input\n    output approved: Bool\n    rule value matches ([{literal}..null), >= {literal}) -> true\n    default false\n  output result\n"
        );
        transpile(&source).unwrap_or_else(|e| panic!("{ty}: {e}"));
    }
}

#[test]
fn generated_range_helpers_cannot_collide_with_declarations() {
    for name in ["BlRange", "BlRangeValue", "lower_cmp", "upper_cmp"] {
        let source = format!(
            "{HEADER}type {name}:\n  value: Number\ntask check(input: {name}) -> Bool:\n  return input.value in [1..2]\n"
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

#[test]
fn string_operators_parse_and_typecheck_without_changing_range_or_equality() {
    use blkit::expr::{Expr, Stmt};
    let source = format!(
        "{HEADER}task compose(input: String) -> String:\n  return \"foo\" + input + \"bar\"\ntask member(input: String) -> Bool:\n  return input in [\"active\", \"pending\"]\ntask negative(input: String) -> Bool:\n  return -1 == -1\ntask compare(input: String) -> Bool:\n  return \"a\" + \"b\" == \"ab\"\n"
    );
    let program = parse(&source).unwrap();
    validate(&program).unwrap();
    let Stmt::Return(Expr::Binary(_, op, _)) = &program.tasks[3].body[0] else {
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
    let range = format!("{HEADER}task range(input: Number) -> Bool:\n  return input in [1..5]\n");
    validate(&parse(&range).unwrap()).unwrap();
}

#[test]
fn string_builtin_signatures_and_literal_regexes_are_validated() {
    let good = [
        ("string(123)", "String"),
        ("string(true)", "String"),
        ("string(date(\"2026-01-01\"))", "String"),
        ("string(time(\"12:30:00\"))", "String"),
        ("string(dateTime(\"2026-01-01T00:00:00Z\"))", "String"),
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
        let source = format!("{HEADER}task good(input: String) -> {ty}:\n  return {expr}\n");
        validate(&parse(&source).unwrap()).unwrap_or_else(|e| panic!("{expr}: {e}"));
    }
    let decision = format!(
        "{HEADER}decision find(input: String) -> List<List<String>>:\n  node result: List<List<String>> = literal extract(input, \"(a)\")\n  output result\n"
    );
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
        let source = format!("{HEADER}task bad(input: String) -> String:\n  return {expr}\n");
        let error = validate(&parse(&source).unwrap()).unwrap_err();
        assert!(error.contains(diagnostic), "{expr}: {error}");
    }
}
