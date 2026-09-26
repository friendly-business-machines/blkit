use blkit::{parse, validate};

const HEADER: &str = "namespace orders\nversion \"1.0\"\n";

#[test]
fn documented_graph_parses_alongside_legacy_example() {
    let graph = parse(include_str!("../examples/graph.bl")).unwrap();
    assert!(!graph.processes[0].graph.is_empty());
    validate(&graph).unwrap();
    assert!(parse(include_str!("../examples/approve.bl")).unwrap().processes[0].graph.is_empty());
}

#[test]
fn parses_source_defined_tasks_and_nested_gateways() {
    let source = format!("{HEADER}task echo(input: Number) -> Number:\n  return input\nprocess route(input: Number) -> Number:\n  run base = echo(input)\n  and:\n    branch left:\n      run a = echo(base)\n    branch right:\n      xor:\n        when input > 10:\n          run b = echo(base)\n        else:\n          run c = echo(input)\n      join chosen\n  join pair: Pair\n  return pair.left\ntype Pair:\n  left: Number\n  right: Number\n");
    let program = parse(&source).unwrap();
    assert_eq!(program.tasks.len(), 1);
    assert_eq!(program.processes[0].graph.len(), 3);
}

#[test]
fn parses_or_with_conditions_and_fallback() {
    let source = format!("{HEADER}task echo(input: Number) -> Number:\n  return input\nprocess route(input: Number) -> List<Number>:\n  or:\n    when input > 10:\n      run a = echo(input)\n    when input > 20:\n      run b = echo(input)\n    else:\n      run c = echo(input)\n  join result\n  return result\n");
    let program = parse(&source).unwrap();
    assert_eq!(program.processes[0].graph.len(), 2);
}

#[test]
fn rejects_missing_fallback_and_malformed_gateway_conditions() {
    let source = format!("{HEADER}process route(input: Number) -> Number:\n  xor:\n    when input > 10:\n      run a = echo(input)\n    else:\n      run b = echo(input)\n  join choice\n  return choice\n");
    assert!(parse(&source.replace("    else:\n      run b = echo(input)\n", "")).unwrap_err().contains("fallback"));
    assert!(parse(&source.replace("when input > 10:", "when input >:" )).unwrap_err().contains("expression"));
    assert!(parse(&source.replace("run a = echo(input)", "run a = echo(input" )).unwrap_err().contains("task call"));
}

#[test]
fn rejects_gateway_with_only_fallback() {
    let source = format!("{HEADER}process route(input: Number) -> Number:\n  xor:\n    else:\n      run a = echo(input)\n  join choice\n  return choice\n");
    assert!(parse(&source).unwrap_err().contains("condition"));
}
