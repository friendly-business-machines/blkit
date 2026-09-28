use blkit::{parse, transpile, validate};

const EXPLICIT: &str = "namespace orders\nversion \"1.0\"\ntask echo(input: Number) -> Number:\n  return input\nprocess route(input: Number) -> Number:\n  node start = start\n  node first = task echo(input)\n  node done = end\n  link start -> first\n  link first -> done(first)\n";

const TYPED_XOR: &str = "namespace orders\nversion \"1.0\"\ntask echo(input: Number) -> Number:\n  return input\nprocess route(input: Number) -> Number:\n  node start = start\n  node gate = xor_split\n  node high = task echo(input)\n  node low = task echo(input)\n  node chosen = xor_join(gate)\n  node done = end\n  link start -> gate\n  link gate -> high when input > 10\n  link gate -> low else\n  link high -> chosen(high)\n  link low -> chosen(low)\n  link chosen -> done(chosen)\n";

#[test]
fn deadlines_validate_origin_bounds_and_reserved_timeout_name() {
    for origin in ["queued", "first_claimed"] {
        let source = EXPLICIT.replace(
            "  node start = start",
            &format!("  deadline {origin} \"5h\"\n  node start = start"),
        );
        validate(&parse(&source).unwrap()).unwrap();
        let repeated = source.replace(
            "  node start = start",
            "  deadline queued \"5h\"\n  node start = start",
        );
        assert!(parse(&repeated).is_err());
    }
    for clause in [
        "deadline queued \"0s\"",
        "deadline first_claimed \"nope\"",
        "deadline unknown \"5h\"",
    ] {
        assert!(
            parse(&EXPLICIT.replace(
                "  node start = start",
                &format!("  {clause}\n  node start = start")
            ))
            .is_err(),
            "{clause}"
        );
    }
    assert!(
        validate(
            &parse(&EXPLICIT.replace("node done = end", "node timeout = error\n  node done = end"))
                .unwrap()
        )
        .unwrap_err()
        .contains("reserved")
    );
    let cycle = EXPLICIT.replace(
        "  link first -> done(first)",
        "  link first -> first\n  link first -> done(first)",
    );
    assert!(
        validate(&parse(&cycle).unwrap())
            .unwrap_err()
            .contains("deadline")
    );
}

const CYCLIC: &str = "namespace orders\nversion \"1.0\"\ntask echo(input: Number) -> Number:\n  return input\nprocess repeat(input: Number) -> Number:\n  deadline queued \"1s\"\n  node start = start\n  node gate = xor_split\n  node work = task echo(input)\n  node joined = xor_join(gate)\n  node stop = error\n  link start -> gate\n  link gate -> work when input > 0\n  link gate -> stop else\n  link work -> joined(work)\n  link joined -> gate\n";

#[test]
fn cyclic_routes_require_deadlines_reachable_exits_and_definite_values() {
    validate(&parse(CYCLIC).unwrap()).unwrap();
    assert!(transpile(CYCLIC).is_ok());
    assert!(
        validate(&parse(&CYCLIC.replace("  deadline queued \"1s\"\n", "")).unwrap())
            .unwrap_err()
            .contains("deadline")
    );
    let no_exit = CYCLIC
        .replace("  node stop = error\n", "  node skip = task echo(input)\n")
        .replace("link gate -> stop else", "link gate -> skip else")
        .replace(
            "link joined -> gate",
            "link skip -> joined(skip)\n  link joined -> gate",
        );
    assert!(
        validate(&parse(&no_exit).unwrap())
            .unwrap_err()
            .contains("exit")
    );
    let uninitialized = CYCLIC.replace("when input > 0", "when work > 0");
    assert!(
        validate(&parse(&uninitialized).unwrap())
            .unwrap_err()
            .contains("unknown name")
    );
    let invalid_join = CYCLIC.replace(
        "link work -> joined(work)",
        "link start -> joined(input)\n  link work -> joined(work)",
    );
    assert!(validate(&parse(&invalid_join).unwrap()).is_err());
}

#[test]
fn task_free_cycle_with_deadline_is_a_valid_graph() {
    let source = "namespace example\nversion \"1\"\nprocess spin(input: Number) -> Number:\n  deadline queued \"1s\"\n  node start = start\n  node gate = xor_split\n  node joined = xor_join(gate)\n  node failed = error\n  link start -> gate\n  link gate -> joined(input) when input > 0\n  link gate -> failed else\n  link joined -> gate\n";
    validate(&parse(source).unwrap()).unwrap();
}

#[test]
fn task_loop_bounds_conditions_and_initial_values_are_typed() {
    let post = EXPLICIT.replace(
        "task echo(input)",
        "task echo(input) repeat_post(first < 3) max_iterations 3",
    );
    validate(&parse(&post).unwrap()).unwrap();
    let pre = EXPLICIT.replace(
        "task echo(input)",
        "task echo(input) repeat_pre(first < 3) max_duration \"1m\" initial 0",
    );
    validate(&parse(&pre).unwrap()).unwrap();
    for invalid in [
        "task echo(input) repeat_post(first < 3) max_iterations 0",
        "task echo(input) repeat_post(first < 3)",
        "task echo(input) repeat_pre(first < 3) max_iterations 3",
        "task echo(input) repeat_pre(1) max_iterations 3 initial 0",
        "task echo(input) repeat_pre(first < 3) max_iterations 3 initial true",
    ] {
        let source = EXPLICIT.replace("task echo(input)", invalid);
        assert!(
            parse(&source)
                .and_then(|program| validate(&program))
                .is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn multi_instance_tasks_require_typed_lists_and_known_mode() {
    let source = "namespace orders\nversion \"1\"\ntask echo(input: Number) -> Number:\n  return input\nprocess group(input: List<Number>) -> List<Number>:\n  node start = start\n  node batch = task echo each input sequential\n  node done = end\n  link start -> batch\n  link batch -> done(batch)\n";
    validate(&parse(source).unwrap()).unwrap();
    validate(&parse(&source.replace("sequential", "parallel")).unwrap()).unwrap();
    assert!(parse(&source.replace("sequential", "random")).is_err());
    assert!(
        validate(
            &parse(&source.replace(
                "List<Number>) -> List<Number>",
                "List<Bool>) -> List<Number>"
            ))
            .unwrap()
        )
        .is_err()
    );
    assert!(
        validate(
            &parse(&source.replace(
                "process group(input: List<Number>)",
                "process group(input: Number)"
            ))
            .unwrap()
        )
        .is_err()
    );
}

#[test]
fn explicit_links_validate_types_and_path_availability() {
    validate(&parse(TYPED_XOR).unwrap()).unwrap();
    let invalid = |text: &str| validate(&parse(text).unwrap()).unwrap_err();
    assert!(invalid(&TYPED_XOR.replace("when input > 10", "when input")).contains("Bool"));
    assert!(invalid(&TYPED_XOR.replace("echo(input)", "echo(true)")).contains("task input"));
    assert!(invalid(&TYPED_XOR.replace("done(chosen)", "done(true)")).contains("end"));
    assert!(
        invalid(&TYPED_XOR.replace("link low -> chosen(low)", "link low -> chosen(high)"))
            .contains("unknown name")
    );
}

#[test]
fn gateway_conditions_use_only_values_available_on_every_route() {
    let source = TYPED_XOR
        .replace(
            "node gate = xor_split",
            "node base = task echo(input)\n  node gate = xor_split",
        )
        .replace(
            "link start -> gate",
            "link start -> base\n  link base -> gate",
        )
        .replace("when input > 10", "when base > input");
    validate(&parse(&source).unwrap()).unwrap();
    let unavailable = source.replace("when base > input", "when high > input");
    assert!(
        validate(&parse(&unavailable).unwrap())
            .unwrap_err()
            .contains("unknown name")
    );
}

#[test]
fn explicit_and_join_matches_branch_labels_to_record_fields() {
    let source = "namespace orders\nversion \"1.0\"\ntype Pair:\n  left: Number\n  right: Number\ntask echo(input: Number) -> Number:\n  return input\nprocess route(input: Number) -> Pair:\n  node start = start\n  node split = and_split\n  node left = task echo(input)\n  node right = task echo(input)\n  node both = and_join(split): Pair\n  node done = end\n  link start -> split\n  link split -> left as left\n  link split -> right as right\n  link left -> both(left)\n  link right -> both(right)\n  link both -> done(both)\n";
    validate(&parse(source).unwrap()).unwrap();
    let invalid = |text: &str| validate(&parse(text).unwrap()).unwrap_err();
    assert!(invalid(&source.replace("right: Number", "right: Bool")).contains("AND join"));
    assert!(invalid(&source.replace("-> right as right", "-> right as left")).contains("branch"));
}

#[test]
fn explicit_or_requires_fallback_and_compatible_results() {
    let source = "namespace orders\nversion \"1.0\"\ntask echo(input: Number) -> Number:\n  return input\nprocess route(input: Number) -> List<Number>:\n  node start = start\n  node split = or_split\n  node a = task echo(input)\n  node b = task echo(input)\n  node c = task echo(input)\n  node joined = or_join(split)\n  node done = end\n  link start -> split\n  link split -> a when input > 1\n  link split -> b when input > 2\n  link split -> c else\n  link a -> joined(a)\n  link b -> joined(b)\n  link c -> joined(c)\n  link joined -> done(joined)\n";
    validate(&parse(source).unwrap()).unwrap();
    let invalid = |text: &str| validate(&parse(text).unwrap()).unwrap_err();
    assert!(
        invalid(&source.replace("link split -> c else", "link split -> c when input > 3"))
            .contains("fallback")
    );
    assert!(invalid(&source.replace("joined(c)", "joined(true)")).contains("matching"));
}

#[test]
fn normal_split_branches_must_reach_their_matching_join() {
    let bypass = TYPED_XOR.replace("link low -> chosen(low)", "link low -> done(low)");
    assert!(
        validate(&parse(&bypass).unwrap())
            .unwrap_err()
            .contains("join")
    );
}

#[test]
fn join_rejects_routes_that_do_not_come_from_its_split() {
    let extra = TYPED_XOR.replace(
        "link start -> gate",
        "link start -> gate\n  link start -> chosen(input)",
    );
    assert!(validate(&parse(&extra).unwrap()).is_err());
}

#[test]
fn split_rejects_multiple_matching_joins() {
    let extra = TYPED_XOR
        .replace(
            "node done = end",
            "node other = xor_join(gate)\n  node done = end",
        )
        .replace(
            "link low -> chosen(low)",
            "link low -> other(low)\n  link other -> chosen(other)",
        );
    assert!(
        validate(&parse(&extra).unwrap())
            .unwrap_err()
            .contains("join")
    );
}

#[test]
fn branches_cannot_merge_before_their_join() {
    let merged = TYPED_XOR.replace("link low -> chosen(low)", "link low -> high");
    assert!(
        validate(&parse(&merged).unwrap())
            .unwrap_err()
            .contains("join")
    );
}

#[test]
fn ordinary_nodes_cannot_fork_without_a_split() {
    let fork = EXPLICIT
        .replace("node done = end", "node other = end\n  node done = end")
        .replace(
            "link first -> done(first)",
            "link first -> done(first)\n  link first -> other(first)",
        );
    assert!(
        validate(&parse(&fork).unwrap())
            .unwrap_err()
            .contains("split")
    );
}

#[test]
fn values_are_allowed_only_on_join_and_normal_end_links() {
    let extra = EXPLICIT.replace("link start -> first", "link start -> first(true)");
    assert!(
        validate(&parse(&extra).unwrap())
            .unwrap_err()
            .contains("value")
    );
}

#[test]
fn routing_annotations_require_their_matching_gateway() {
    let fallback = EXPLICIT.replace("link start -> first", "link start -> first else");
    assert!(
        validate(&parse(&fallback).unwrap())
            .unwrap_err()
            .contains("fallback")
    );
    let label = EXPLICIT.replace("link start -> first", "link start -> first as extra");
    assert!(
        validate(&parse(&label).unwrap())
            .unwrap_err()
            .contains("label")
    );
}

#[test]
fn exceptional_terminals_reject_payloads() {
    for terminal in ["error", "cancel", "terminate"] {
        let source = EXPLICIT
            .replace("node done = end", &format!("node done = {terminal}"))
            .replace("link first -> done(first)", "link first -> done(first)");
        assert!(
            validate(&parse(&source).unwrap())
                .unwrap_err()
                .contains("payload"),
            "{terminal}"
        );
        let valid = source.replace("done(first)", "done");
        validate(&parse(&valid).unwrap()).unwrap();
    }
}

#[test]
fn exceptional_branch_does_not_need_normal_end_output() {
    let source = TYPED_XOR
        .replace("node low = task echo(input)", "node failure = error")
        .replace("link gate -> low else", "link gate -> failure else")
        .replace("  link low -> chosen(low)\n", "");
    validate(&parse(&source).unwrap()).unwrap();
}

#[test]
fn explicit_split_and_join_require_typed_branch_values() {
    let invalid = |text: &str| validate(&parse(text).unwrap()).unwrap_err();
    assert!(invalid(&TYPED_XOR.replace("chosen(low)", "chosen(true)")).contains("matching"));
    assert!(invalid(&TYPED_XOR.replace("xor_join(gate)", "or_join(gate)")).contains("matching"));
}

#[test]
fn explicit_graph_rejects_missing_duplicate_and_unreachable_nodes() {
    validate(&parse(EXPLICIT).unwrap()).unwrap();
    let invalid = |text: &str| validate(&parse(text).unwrap()).unwrap_err();
    assert!(
        invalid(&EXPLICIT.replace("link start -> first", "link start -> missing"))
            .contains("unknown node")
    );
    assert!(
        invalid(&EXPLICIT.replace("  node done = end", "  node first = end\n  node done = end"))
            .contains("duplicate node")
    );
    assert!(
        invalid(&EXPLICIT.replace(
            "  node done = end",
            "  node unused = error\n  node done = end"
        ))
        .contains("unreachable")
    );
}

#[test]
fn explicit_graph_rejects_cycles_dead_ends_and_invalid_joins() {
    let invalid = |text: &str| validate(&parse(text).unwrap()).unwrap_err();
    assert!(
        invalid(&EXPLICIT.replace(
            "  link first -> done(first)",
            "  link first -> first\n  link first -> done(first)"
        ))
        .contains("cycle")
    );
    assert!(invalid(&EXPLICIT.replace("  link first -> done(first)", "")).contains("dead end"));
    assert!(
        invalid(&EXPLICIT.replace(
            "  node first = task echo(input)",
            "  node first = xor_join(missing)"
        ))
        .contains("split")
    );
}

const SOURCE: &str = "namespace orders\nversion \"1.0\"\ntask echo(input: Number) -> Number:\n  return input\nprocess route(input: Number) -> Number:\n  run first = echo(input)\n  run second = echo(first)\n  return second\n";

#[test]
fn old_process_returns_and_implicit_runs_require_explicit_graphs() {
    assert!(transpile(SOURCE).unwrap_err().contains("explicit"));
    let single_body = "namespace orders\nversion \"1.0\"\nprocess route(input: Number) -> Number:\n  return input\n";
    assert!(transpile(single_body).unwrap_err().contains("explicit"));
    let mixed = EXPLICIT.replace("link first -> done(first)", "return first");
    assert!(transpile(&mixed).unwrap_err().contains("explicit end"));
    validate(&parse(EXPLICIT).unwrap()).unwrap();
}
