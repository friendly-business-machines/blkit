use blkit::{parse, transpile, validate};

const HEADER: &str = "namespace orders\nversion \"1.0\"\n";

#[test]
fn declarations_and_typed_process_parse() {
    let source = format!("{HEADER}\ntype Order:\n  total: Number\n  tags: List<String>\n\nenum Decision:\n  approved\n  review\n\nprocess approve(input: Order) -> Decision:\n  return Decision.approved\n");
    let ast = parse(&source).unwrap();
    assert_eq!(ast.namespace, "orders");
    assert_eq!(ast.version, "1.0");
    assert_eq!(ast.records.len(), 1);
    assert_eq!(ast.enums.len(), 1);
    assert_eq!(ast.processes.len(), 1);
}

#[test]
fn missing_header_is_reported() {
    assert!(parse("version \"1.0\"\n").unwrap_err().contains("namespace"));
    assert!(parse("namespace orders\n").unwrap_err().contains("version"));
}

#[test]
fn malformed_signature_is_reported() {
    let source = format!("{HEADER}process approve(input Order) -> Decision:\n  return Decision.approved\n");
    assert!(parse(&source).unwrap_err().contains("process signature"));
}

#[test]
fn nested_branch_and_boolean_expression_parse() {
    let source = format!("{HEADER}process approve(input: Order) -> Decision:\n  if input.total > 1000 and not input.blocked:\n    if input.tags == [\"vip\", \"repeat\"]:\n      return Decision.review\n    else:\n      return Decision.approved\n  else:\n    return Decision.rejected\n");
    let ast = parse(&source).unwrap();
    assert_eq!(ast.processes[0].body.len(), 1);
}

#[test]
fn malformed_expression_is_rejected() {
    let source = format!("{HEADER}process approve(input: Order) -> Decision:\n  return input.total >\n");
    assert!(parse(&source).unwrap_err().contains("expression"));
}

#[test]
fn known_declarations_resolve_and_duplicates_fail() {
    let source = format!("{HEADER}type Order:\n  total: Number\nenum Decision:\n  approved\nprocess approve(input: Order) -> Decision:\n  return Decision.approved\n");
    validate(&parse(&source).unwrap()).unwrap();
    let duplicate = source.replace("enum Decision:", "type Order:\n  total: Number\nenum Decision:");
    assert!(validate(&parse(&duplicate).unwrap()).unwrap_err().contains("duplicate"));
}

#[test]
fn unknown_and_deferred_types_are_rejected() {
    for ty in ["Mystery", "Table<Number>"] {
        let source = format!("{HEADER}type Order:\n  total: {ty}\nprocess echo(input: Order) -> Order:\n  return input\n");
        assert!(validate(&parse(&source).unwrap()).unwrap_err().contains(ty));
    }
}

#[test]
fn expression_types_and_list_elements_are_checked() {
    let valid = format!("{HEADER}type Order:\n  total: Number\n  blocked: Bool\nenum Decision:\n  approved\n  review\nprocess decide(input: Order) -> Decision:\n  if input.total > 1000 and not input.blocked:\n    return Decision.review\n  else:\n    return Decision.approved\nprocess amounts(input: Order) -> List<Number>:\n  return [1, 2.5]\n");
    validate(&parse(&valid).unwrap()).unwrap();
    let bad = valid.replace("[1, 2.5]", "[1, \"two\"]");
    assert!(validate(&parse(&bad).unwrap()).unwrap_err().contains("List<Number>"));
}

#[test]
fn generated_rust_names_are_checked() {
    for declarations in [
        "process match(input: Number) -> Number:\n  return input\n",
        "type Vec:\n  value: Number\nprocess echo(input: Vec) -> Vec:\n  return input\n",
        "type NAMESPACE:\n  value: Number\nprocess echo(input: NAMESPACE) -> NAMESPACE:\n  return input\n",
        "type Order:\n  match: Number\nprocess echo(input: Order) -> Order:\n  return input\n",
        "enum Decision:\n  match\nprocess echo(input: Number) -> Decision:\n  return Decision.match\n",
        "process echo(match: Number) -> Number:\n  return match\n",
    ] {
        let source = format!("{HEADER}{declarations}");
        assert!(transpile(&source).is_err(), "accepted invalid generated name: {source}");
    }
}

#[test]
fn empty_list_equality_does_not_depend_on_operand_order() {
    let left = format!("{HEADER}process empty(input: List<Number>) -> Bool:\n  return [] == input\n");
    let right = left.replace("[] == input", "input == []");
    validate(&parse(&left).unwrap()).unwrap();
    validate(&parse(&right).unwrap()).unwrap();
}

#[test]
fn by_value_record_cycles_fail_but_list_recursion_is_allowed() {
    let direct = format!("{HEADER}type Node:\n  next: Node\nprocess echo(input: Node) -> Node:\n  return input\n");
    assert!(validate(&parse(&direct).unwrap()).unwrap_err().contains("recursive"));
    let indirect = format!("{HEADER}type A:\n  b: B\ntype B:\n  a: A\nprocess echo(input: A) -> A:\n  return input\n");
    assert!(validate(&parse(&indirect).unwrap()).unwrap_err().contains("recursive"));
    let list = direct.replace("next: Node", "next: List<Node>");
    validate(&parse(&list).unwrap()).unwrap();
}

#[test]
fn every_branch_must_return() {
    let base = format!("{HEADER}process decide(input: Number) -> Number:\n  if input > 10:\n    return 1\n");
    assert!(validate(&parse(&base).unwrap()).unwrap_err().contains("missing return"));
    let complete = format!("{base}  else:\n    return 2\n");
    validate(&parse(&complete).unwrap()).unwrap();
}

#[test]
fn field_variant_and_return_errors_are_reported() {
    let base = format!("{HEADER}type Order:\n  total: Number\nenum Decision:\n  approved\nprocess decide(input: Order) -> Decision:\n  return Decision.approved\n");
    for (broken, expected) in [
        (base.replace("Decision.approved", "Decision.missing"), "variant"),
        (base.replace("Decision.approved", "input.missing"), "field"),
        (base.replace("Decision.approved", "input.total"), "return type"),
        (base.replace("Decision.approved", "1 and true"), "Bool"),
    ] {
        assert!(validate(&parse(&broken).unwrap()).unwrap_err().contains(expected));
    }
}
