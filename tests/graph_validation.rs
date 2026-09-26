use blkit::{parse, validate};

const SOURCE: &str = "namespace orders\nversion \"1.0\"\ntask echo(input: Number) -> Number:\n  return input\nprocess route(input: Number) -> Number:\n  run first = echo(input)\n  run second = echo(first)\n  return second\n";

fn error(source: &str) -> String { validate(&parse(source).unwrap()).unwrap_err() }

#[test]
fn validates_named_task_links_and_process_output() {
    validate(&parse(SOURCE).unwrap()).unwrap();
    assert!(error(&SOURCE.replace("echo(first)", "missing(first)")).contains("unknown task"));
    assert!(error(&SOURCE.replace("echo(first)", "echo(second)")).contains("unknown name"));
    assert!(error(&SOURCE.replace("return second", "return true")).contains("return type"));
}

#[test]
fn rejects_unreachable_and_duplicate_nodes() {
    assert!(error(&SOURCE.replace("  return second", "  return second\n  run unused = echo(input)")).contains("unreachable"));
    assert!(error(&SOURCE.replace("run second =", "run first =")).contains("duplicate"));
}

#[test]
fn rejects_incompatible_task_input() {
    assert!(error(&SOURCE.replace("echo(first)", "echo(true)")).contains("task input"));
}

#[test]
fn validates_typed_xor_and_or_joins() {
    let xor = "namespace orders\nversion \"1.0\"\ntask echo(input: Number) -> Number:\n  return input\nprocess route(input: Number) -> Number:\n  xor:\n    when input > 10:\n      run a = echo(input)\n    else:\n      run b = echo(input)\n  join chosen\n  return chosen\n";
    validate(&parse(xor).unwrap()).unwrap();
    assert!(error(&xor.replace("when input > 10:", "when input:")).contains("Bool"));
    assert!(error(&xor.replace("return chosen", "return a")).contains("unknown name"));
    let or = xor.replace("-> Number:\n  xor:", "-> List<Number>:\n  or:");
    validate(&parse(&or).unwrap()).unwrap();
    assert!(error(&or.replace("-> List<Number>:\n  or:", "-> Number:\n  or:")).contains("return type"));
}

#[test]
fn validates_and_record_join_and_all_branches() {
    let source = "namespace orders\nversion \"1.0\"\ntype Pair:\n  left: Number\n  right: Number\ntask echo(input: Number) -> Number:\n  return input\nprocess route(input: Number) -> Pair:\n  and:\n    branch left:\n      run a = echo(input)\n    branch right:\n      run b = echo(input)\n  join together: Pair\n  return together\n";
    validate(&parse(source).unwrap()).unwrap();
    assert!(error(&source.replace("right: Number", "right: Bool")).contains("join"));
    assert!(error(&source.replace("branch right:", "branch left:")).contains("duplicate"));
    assert!(error(&source.replace("run b = echo(input)", "run b = missing(input)")).contains("unknown task"));
}
