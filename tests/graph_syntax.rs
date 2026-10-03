use blkit::{graph::NodeKind, parse, validate};

const HEADER: &str = "namespace orders\nversion \"1.0\"\n";

#[test]
fn parses_explicit_nodes_links_and_retry_policy() {
    let source = format!(
        "{HEADER}task echo(input: Number) -> Number:\n  return input\nprocess route(input: Number) -> Number:\n  retry max_retries 3 retry_for \"10m\" retry_delay \"1s\" backoff exponential\n  node start = start\n  node fork = xor_split\n  node high = task echo(input)\n  node low = task echo(input)\n  node joined = xor_join(fork)\n  node done = end\n  node bad = error\n  node stop = cancel\n  node halt = terminate\n  link start -> fork\n  link fork -> high when input > 10\n  link fork -> low else\n  link high -> joined(high)\n  link low -> joined(low)\n  link joined -> done(joined)\n"
    );
    let program = parse(&source).unwrap();
    let graph = program.processes[0].named_graph.as_ref().unwrap();
    assert_eq!(graph.nodes.len(), 9);
    assert_eq!(graph.links.len(), 6);
    assert_eq!(graph.links[1].source, "fork");
    assert_eq!(graph.links[2].target, "low");
    assert!(graph.links[1].condition.is_some());
    assert!(graph.links[2].fallback);
    let retry = program.processes[0].retry.as_ref().unwrap();
    assert_eq!(retry.max_retries, 3);
    assert_eq!(retry.retry_for.as_secs(), 600);
    assert_eq!(retry.retry_delay.as_secs(), 1);
    assert_eq!(program.tasks[0].body.len(), 1);
}

#[test]
fn business_rule_node_is_typed_and_exposes_its_result() {
    let source = format!(
        "{HEADER}decision price(input: Number) -> Number:\n  node result: Number = literal input\n  output result\nprocess route(input: Number) -> Number:\n  node start = start\n  node quote = business_rule price(input)\n  node done = end\n  link start -> quote\n  link quote -> done(quote)\n"
    );
    validate(&parse(&source).unwrap()).unwrap();
    assert!(
        validate(
            &parse(&source.replace("business_rule price(input)", "business_rule price(true)"))
                .unwrap()
        )
        .unwrap_err()
        .contains("input type mismatch")
    );
}

#[test]
fn parses_and_or_joins_and_labeled_links() {
    let source = format!(
        "{HEADER}type Pair:\n  left: Number\n  right: Number\nprocess route(input: Number) -> Pair:\n  node start = start\n  node split = and_split\n  node left = or_split\n  node right = task echo(input)\n  node or_joined = or_join(left)\n  node joined = and_join(split): Pair\n  node done = end\n  link start -> split\n  link split -> left as left\n  link split -> right as right\n  link left -> or_joined(input) else\n  link right -> joined(right)\n  link or_joined -> joined(or_joined)\n  link joined -> done(joined)\n"
    );
    let graph = parse(&source)
        .unwrap()
        .processes
        .remove(0)
        .named_graph
        .unwrap();
    assert_eq!(graph.nodes.len(), 7);
    assert_eq!(graph.links[1].label.as_deref(), Some("left"));
    assert_eq!(graph.links[2].label.as_deref(), Some("right"));
    assert!(graph.links[3].fallback);
}

#[test]
fn parses_subprocess_and_per_outcome_links() {
    let source = format!(
        "{HEADER}process child(input: Number) -> Number:\n  node start = start\n  node done = end\n  link start -> done(input)\nprocess parent(input: Number) -> Number:\n  node start = start\n  node called = subprocess child(input)\n  node done = end\n  node failed = error\n  node cancelled = cancel\n  node stopped = terminate\n  link start -> called\n  link called -> done(called)\n  link called -> failed on error\n  link called -> cancelled on cancel\n  link called -> stopped on terminate\n"
    );
    let program = parse(&source).unwrap();
    let graph = program.processes[1].named_graph.as_ref().unwrap();
    assert!(
        matches!(&graph.nodes[1].kind, NodeKind::Subprocess { process, .. } if process == "child")
    );
    assert_eq!(graph.links.len(), 5);
    assert_eq!(graph.links[1].outcome, None);
    for (link, target, outcome) in [
        (&graph.links[2], "failed", "error"),
        (&graph.links[3], "cancelled", "cancel"),
        (&graph.links[4], "stopped", "terminate"),
    ] {
        assert_eq!(link.target, target);
        assert_eq!(link.outcome.as_deref(), Some(outcome));
        assert!(link.value.is_none());
    }
}

#[test]
fn documented_graph_and_approval_use_explicit_links() {
    for source in [
        include_str!("../examples/graph.bl"),
        include_str!("../examples/approve.bl"),
        include_str!("../examples/pricing.bl"),
    ] {
        let program = parse(source).unwrap();
        validate(&program).unwrap();
        assert!(
            program
                .processes
                .iter()
                .all(|process| process.named_graph.is_some())
        );
    }
}

#[test]
fn explicit_graph_rejects_missing_fallback_and_invalid_conditions() {
    let source = format!(
        "{HEADER}task echo(input: Number) -> Number:\n  return input\nprocess route(input: Number) -> Number:\n  node start = start\n  node gate = xor_split\n  node a = task echo(input)\n  node b = task echo(input)\n  node chosen = xor_join(gate)\n  node done = end\n  link start -> gate\n  link gate -> a when input > 10\n  link gate -> b else\n  link a -> chosen(a)\n  link b -> chosen(b)\n  link chosen -> done(chosen)\n"
    );
    let missing = source.replace("  link gate -> b else", "  link gate -> b when input > 20");
    assert!(
        validate(&parse(&missing).unwrap())
            .unwrap_err()
            .contains("fallback")
    );
    assert!(
        parse(&source.replace("when input > 10", "when input >"))
            .unwrap_err()
            .contains("expression")
    );
}
