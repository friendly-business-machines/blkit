use blkit::transpile;
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn compile_and_test(source: &str, assertion: &str) {
    let generated = transpile(source).unwrap();
    let directory = std::env::temp_dir().join(format!(
        "blkit-generated-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(directory.join("src")).unwrap();
    fs::write(directory.join("Cargo.toml"), "[package]\nname = \"blkit_generated_test\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\nrust_decimal = \"1.39\"\nchrono = { version = \"0.4\", features = [\"serde\"] }\nserde_json = \"1\"\n").unwrap();
    fs::write(directory.join("src/lib.rs"), format!("{generated}\n#[cfg(test)] mod generated_checks {{ use super::*; #[test] fn behavior() {{ {assertion} }} }}")).unwrap();
    let result = Command::new("cargo")
        .args(["test", "--offline", "--manifest-path"])
        .arg(directory.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", directory.join("target"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "generated crate failed: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn generated_datetime_inputs_require_offset_and_compare_instants() {
    compile_and_test(
        "namespace timing\nversion \"1\"\ntype Window:\n  opens: DateTime\n  closes: DateTime\ntask before(input: Window) -> Bool:\n  return input.opens < input.closes\n",
        "let opens: DateTime = serde_json::from_value(serde_json::json!(\"2026-10-02T09:30:00+02:00\")).unwrap(); let closes: DateTime = serde_json::from_value(serde_json::json!(\"2026-10-02T08:31:00+01:00\")).unwrap(); assert!(before(Window { opens, closes })); assert!(serde_json::from_value::<DateTime>(serde_json::json!(\"2026-10-02T09:30:00\")).is_err());",
    );
}

#[test]
fn generated_records_enums_and_decimal_literals_compile() {
    compile_and_test(
        "namespace orders\nversion \"1.0\"\ntype Order:\n  total: Number\n  tags: List<String>\nenum Decision:\n  approved\n  review\ntask amount(input: Order) -> Number:\n  return 12.50\n",
        "let order = Order { total: Number::ONE, tags: vec![] }; assert_eq!(amount(order), \"12.50\".parse::<Number>().unwrap());",
    );
}

#[test]
fn generated_decision_table_evaluates_first_match_and_default() {
    compile_and_test(
        "namespace pricing\nversion \"1\"\ndecision price(input: Number) -> Number:\n  node result: Number = table FIRST\n    input amount: Number = input\n    output price: Number\n    rule amount > 100 -> 5\n    rule amount > 10 -> 2\n    default 0\n  output result\n",
        "assert_eq!(price(\"200\".parse().unwrap()).unwrap(), \"5\".parse::<Number>().unwrap()); assert_eq!(price(\"20\".parse().unwrap()).unwrap(), \"2\".parse::<Number>().unwrap()); assert_eq!(price(Number::ZERO).unwrap(), Number::ZERO);",
    );
}

#[test]
fn generated_decision_models_evaluate_dependencies_context_and_knowledge() {
    compile_and_test(
        "namespace pricing\nversion \"1\"\ndecision price(input: Number) -> Number:\n  knowledge fee(amount: Number) -> Number = amount\n  node base: Number = literal input\n  node outcome: Number = context\n    entry subtotal: Number = fee(base)\n    result subtotal\n  link base -> outcome\n  output outcome\n",
        "assert_eq!(price(\"9\".parse().unwrap()).unwrap(), \"9\".parse::<Number>().unwrap());",
    );
}

#[test]
fn generated_decision_tables_cover_hit_policies_and_aggregations() {
    let mut source = String::from("namespace pricing\nversion \"1\"\n");
    for (name, policy, rules, priority) in [
        (
            "unique",
            "UNIQUE",
            "    rule amount > 100 -> 5\n    rule amount > 10 -> 2\n",
            "",
        ),
        (
            "any",
            "ANY",
            "    rule amount > 100 -> 5\n    rule amount > 10 -> 5\n",
            "",
        ),
        (
            "first",
            "FIRST",
            "    rule amount > 100 -> 5\n    rule amount > 10 -> 2\n",
            "",
        ),
        (
            "priority",
            "PRIORITY",
            "    rule amount > 100 -> 5\n    rule amount > 10 -> 2\n",
            "    priority 2\n    priority 5\n",
        ),
        (
            "rule_order",
            "RULE_ORDER",
            "    rule amount > 100 -> 5\n    rule amount > 10 -> 2\n",
            "",
        ),
        (
            "output_order",
            "OUTPUT_ORDER",
            "    rule amount > 100 -> 5\n    rule amount > 10 -> 2\n",
            "    priority 2\n    priority 5\n",
        ),
        (
            "collect",
            "COLLECT",
            "    rule amount > 100 -> 5\n    rule amount > 10 -> 2\n",
            "",
        ),
        (
            "sum",
            "COLLECT SUM",
            "    rule amount > 100 -> 5\n    rule amount > 10 -> 2\n",
            "",
        ),
        (
            "min",
            "COLLECT MIN",
            "    rule amount > 100 -> 5\n    rule amount > 10 -> 2\n",
            "",
        ),
        (
            "max",
            "COLLECT MAX",
            "    rule amount > 100 -> 5\n    rule amount > 10 -> 2\n",
            "",
        ),
        (
            "count",
            "COLLECT COUNT",
            "    rule amount > 100 -> 5\n    rule amount > 10 -> 2\n",
            "",
        ),
    ] {
        let output = if matches!(name, "rule_order" | "output_order" | "collect") {
            "List<Number>"
        } else {
            "Number"
        };
        source.push_str(&format!("decision {name}(input: Number) -> {output}:\n  node result: {output} = table {policy}\n    input amount: Number = input\n    output price: Number\n{priority}{rules}  output result\n"));
    }
    compile_and_test(
        &source,
        "let value = \"200\".parse().unwrap(); assert!(unique(value).unwrap_err().contains(\"UNIQUE\")); assert_eq!(any(value).unwrap(), \"5\".parse::<Number>().unwrap()); assert_eq!(first(value).unwrap(), \"5\".parse::<Number>().unwrap()); assert_eq!(priority(value).unwrap(), \"2\".parse::<Number>().unwrap()); let pair = vec![\"5\".parse::<Number>().unwrap(), \"2\".parse().unwrap()]; assert_eq!(rule_order(value).unwrap(), pair); assert_eq!(collect(value).unwrap(), pair); assert_eq!(output_order(value).unwrap(), vec![pair[1], pair[0]]); assert_eq!(sum(value).unwrap(), \"7\".parse::<Number>().unwrap()); assert_eq!(min(value).unwrap(), pair[1]); assert_eq!(max(value).unwrap(), pair[0]); assert_eq!(count(value).unwrap(), \"2\".parse::<Number>().unwrap()); assert!(sum(Number::ZERO).unwrap_err().contains(\"no matching\")); assert!(rule_order(Number::ZERO).unwrap().is_empty()); assert_eq!(count(Number::ZERO).unwrap(), Number::ZERO);",
    );
}

#[test]
fn generated_multi_output_table_orders_results_and_rejects_unranked_values() {
    compile_and_test(
        "namespace pricing\nversion \"1\"\ntype Quote:\n  price: Number\n  tier: String\ndecision quotes(input: Number) -> List<Quote>:\n  node result: List<Quote> = table OUTPUT_ORDER\n    input amount: Number = input\n    output price: Number\n    output tier: String\n    priority 2, \"regular\"\n    priority 5, \"express\"\n    rule amount > 100 -> 5, \"express\"\n    rule amount > 10 -> 2, \"regular\"\n  output result\n",
        "let items = quotes(\"200\".parse().unwrap()).unwrap(); assert_eq!(items.len(), 2); assert_eq!(items[0].tier, \"regular\"); assert_eq!(items[1].tier, \"express\"); assert!(quotes(Number::ZERO).unwrap().is_empty());",
    );
}

#[test]
fn collect_count_accepts_multiple_output_columns() {
    compile_and_test(
        "namespace pricing\nversion \"1\"\ntype Quote:\n  price: Number\n  tier: String\ndecision count(input: Number) -> Number:\n  node result: Number = table COLLECT COUNT\n    input amount: Number = input\n    output price: Number\n    output tier: String\n    rule amount > 100 -> 5, \"express\"\n    rule amount > 10 -> 2, \"regular\"\n  output result\n",
        "assert_eq!(count(\"200\".parse().unwrap()).unwrap(), \"2\".parse::<Number>().unwrap());",
    );
}

#[test]
fn collect_count_uses_configured_default_on_no_match() {
    compile_and_test(
        "namespace pricing\nversion \"1\"\ndecision count(input: Number) -> Number:\n  node result: Number = table COLLECT COUNT\n    input amount: Number = input\n    output tier: String\n    rule amount > 100 -> \"express\"\n    default 42\n  output result\n",
        "assert_eq!(count(Number::ZERO).unwrap(), \"42\".parse::<Number>().unwrap()); assert_eq!(count(\"200\".parse().unwrap()).unwrap(), Number::ONE);",
    );
}

#[test]
fn generated_any_and_priority_reject_conflicting_or_unranked_matches() {
    compile_and_test(
        "namespace pricing\nversion \"1\"\ndecision any_price(input: Number) -> Number:\n  node result: Number = table ANY\n    input amount: Number = input\n    output price: Number\n    rule amount > 100 -> 5\n    rule amount > 10 -> 2\n  output result\ndecision ranked(input: Number) -> Number:\n  node result: Number = table PRIORITY\n    input amount: Number = input\n    output price: Number\n    priority 2\n    rule amount > 100 -> 5\n    rule amount > 10 -> 2\n  output result\n",
        "let n = \"200\".parse().unwrap(); assert!(any_price(n).unwrap_err().contains(\"ANY\")); assert!(ranked(n).unwrap_err().contains(\"unranked\"));",
    );
}

#[test]
fn generated_multi_instance_task_evaluates_typed_list() {
    compile_and_test_graph(
        "namespace repeaters\nversion \"1\"\ntask echo(input: Number) -> Number:\n  return input\nprocess gather(input: List<Number>) -> List<Number>:\n  node start = start\n  node batch = task echo each input parallel\n  node done = end\n  link start -> batch\n  link batch -> done(batch)\n",
        "let graph = named_graph_definitions().remove(0); for input in [serde_json::json!([]), serde_json::json!([\"3\",\"1\",\"2\"])] { let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), input); }",
    );
}

#[test]
fn generated_task_loops_cover_zero_and_single_iteration() {
    compile_and_test_graph(
        "namespace repeaters\nversion \"1\"\ntask echo(input: Number) -> Number:\n  return input\nprocess skip(input: Number) -> Number:\n  node start = start\n  node repeated = task echo(repeated) repeat_pre(repeated < 3) max_iterations 3 initial input\n  node done = end\n  link start -> repeated\n  link repeated -> done(repeated)\nprocess once(input: Number) -> Number:\n  node start = start\n  node repeated = task echo(input) repeat_post(repeated < 0) max_iterations 3\n  node done = end\n  link start -> repeated\n  link repeated -> done(repeated)\n",
        "for graph in named_graph_definitions() { let input = serde_json::json!(\"3\"); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), input); }",
    );
}

#[test]
fn generated_deadline_policy_is_preserved() {
    let generated = transpile("namespace timing\nversion \"1\"\nprocess later(input: Number) -> Number:\n  deadline queued \"10m\"\n  node start = start\n  node wait = pause_for \"1m\"\n  node done = end\n  link start -> wait\n  link wait -> done(input)\n").unwrap();
    assert!(
        generated
            .contains("origin: \"queued\", duration: std::time::Duration::from_millis(600000)")
    );
}

#[test]
fn generated_wait_nodes_checkpoint_once_and_resume() {
    compile_and_test_graph(
        "namespace timing\nversion \"1\"\nprocess later(input: DateTime) -> DateTime:\n  node start = start\n  node wait = pause_until input\n  node done = end\n  link start -> wait\n  link wait -> done(input)\n",
        "let graph = named_graph_definitions().remove(0); let input = serde_json::json!(\"2026-10-02T09:30:00+02:00\"); let mut state = graph.checkpoint(&input).unwrap(); assert!(graph.waiting_until(&state).is_some()); graph.resume_due(&input, &mut state, i64::MAX).unwrap(); assert_eq!(state.outcome, Some(input));",
    );
}

#[test]
fn generated_business_rule_node_executes_decision_table() {
    compile_and_test_graph(
        "namespace pricing\nversion \"1\"\ndecision price(input: Number) -> Number:\n  node result: Number = table FIRST\n    input amount: Number = input\n    output price: Number\n    rule amount > 100 -> 5\n    default 2\n  output result\nprocess quote(input: Number) -> Number:\n  node start = start\n  node decision = business_rule price(input)\n  node done = end\n  link start -> decision\n  link decision -> done(decision)\n",
        "let graph = named_graph_definitions().remove(0); let input = serde_json::json!(\"200\"); let mut checkpoint = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut checkpoint).unwrap(), serde_json::json!(\"5\"));",
    );
}

#[test]
fn documented_example_compiles() {
    compile_and_test_graph(
        include_str!("../examples/approve.bl"),
        "let graph = named_graph_definitions().remove(0); let input = serde_json::json!({\"total\":\"1250\",\"blocked\":false}); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!(\"review\"));",
    );
}

#[test]
fn generated_task_branches_with_typed_list_input() {
    let source = "namespace orders\nversion \"1.0\"\ntype Order:\n  total: Number\n  amounts: List<Number>\nenum Decision:\n  approved\n  review\ntask decide(input: Order) -> Decision:\n  if input.total > 1000 and input.amounts == [1, 2.5]:\n    return Decision.review\n  else:\n    return Decision.approved\n";
    let assertion = "let order = |total: &str| Order { total: total.parse().unwrap(), amounts: vec![Number::ONE, \"2.5\".parse().unwrap()] }; assert_eq!(decide(order(\"1001\")), Decision::review); assert_eq!(decide(order(\"999\")), Decision::approved);";
    compile_and_test(source, assertion);
}

fn compile_and_test_graph(source: &str, assertion: &str) {
    let generated = transpile(source).unwrap();
    let directory = std::env::temp_dir().join(format!(
        "blkit-graph-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(directory.join("src")).unwrap();
    let root = env!("CARGO_MANIFEST_DIR").replace('\\', "\\\\");
    fs::write(directory.join("Cargo.toml"), format!("[package]\nname = \"blkit_generated_graph\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\nblkit = {{ path = {root:?} }}\nrust_decimal = {{ version = \"1.39\", features = [\"serde-str\"] }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\nchrono = {{ version = \"0.4\", features = [\"serde\"] }}\ntokio = {{ version = \"1\", features = [\"macros\", \"rt-multi-thread\"] }}\n")).unwrap();
    fs::write(directory.join("src/lib.rs"), format!("{generated}\n#[cfg(test)] mod checks {{ use super::*; #[tokio::test] async fn graph() {{ {assertion} }} }}")).unwrap();
    let result = Command::new("cargo")
        .args(["test", "--manifest-path"])
        .arg(directory.join("Cargo.toml"))
        .env(
            "CARGO_TARGET_DIR",
            std::env::temp_dir().join("blkit-graph-test-target"),
        )
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "generated graph failed: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn invalid_retry_policy_is_rejected_before_rust_generation() {
    let source = "namespace orders\nversion \"1.0\"\nprocess route(input: Number) -> Number:\n  retry max_retries 2 retry_for \"0s\" retry_delay \"1s\" backoff exponential\n  node start = start\n  node done = end\n  link start -> done(input)\n";
    assert!(transpile(source).unwrap_err().contains("retry_for"));
    assert!(
        transpile(&source.replace(
            "retry_for \"0s\" retry_delay \"1s\"",
            "retry_for \"10s\" retry_delay \"0s\""
        ))
        .unwrap_err()
        .contains("retry_delay")
    );
}

#[test]
fn generated_named_graph_carries_identity_retry_and_runs_gateway() {
    let source = "namespace orders\nversion \"1.0\"\ntask echo(input: Number) -> Number:\n  return input\nprocess route(input: Number) -> Number:\n  retry max_retries 2 retry_for \"10s\" retry_delay \"1s\" backoff exponential\n  node start = start\n  node gate = xor_split\n  node high = task echo(input)\n  node low = task echo(input)\n  node joined = xor_join(gate)\n  node done = end\n  link start -> gate\n  link gate -> high when input > 10\n  link gate -> low else\n  link high -> joined(high)\n  link low -> joined(low)\n  link joined -> done(joined)\n";
    compile_and_test_graph(
        source,
        "let graph = named_graph_definitions().remove(0); assert_eq!((graph.namespace, graph.version, graph.name), (\"orders\", \"1.0\", \"route\")); let retry = graph.retry.as_ref().unwrap(); assert_eq!(retry.max_retries, 2); assert_eq!(retry.retry_for.as_secs(), 10); assert_eq!(retry.retry_delay.as_secs(), 1); assert_eq!(retry.backoff, \"exponential\"); let input = serde_json::json!(\"12\"); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!(\"12\"));",
    );
}

#[test]
fn generated_named_graph_emits_exceptional_terminal_nodes_without_retry_by_default() {
    let source = "namespace orders\nversion \"1.0\"\ntask echo(input: Number) -> Number:\n  return input\nprocess route(input: Number) -> Number:\n  node start = start\n  node gate = xor_split\n  node good = task echo(input)\n  node joined = xor_join(gate)\n  node done = end\n  node failure = error\n  node stop = cancel\n  node halt = terminate\n  link start -> gate\n  link gate -> good when input > 0\n  link gate -> failure when input == 0\n  link gate -> stop when input < 0\n  link gate -> halt else\n  link good -> joined(good)\n  link joined -> done(joined)\n";
    compile_and_test_graph(
        source,
        "let graph = named_graph_definitions().remove(0); assert!(graph.retry.is_none()); assert!(graph.nodes.iter().any(|node| matches!(node.kind, blkit::named_runtime::GraphNodeKind::Error))); assert!(graph.nodes.iter().any(|node| matches!(node.kind, blkit::named_runtime::GraphNodeKind::Cancel))); assert!(graph.nodes.iter().any(|node| matches!(node.kind, blkit::named_runtime::GraphNodeKind::Terminate))); let input = serde_json::json!(\"2\"); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!(\"2\"));",
    );
}

#[test]
fn generated_named_graph_compiles_as_a_runtime_definition() {
    compile_and_test_graph(
        include_str!("../examples/graph.bl"),
        "let graph = named_graph_definitions(); let run = |name: &str, total: &str| { let process = graph.iter().find(|g| g.name == name).unwrap(); let input = serde_json::json!({\"total\": total}); let mut state = process.checkpoint(&input).unwrap(); process.run(&input, &mut state).unwrap() }; assert_eq!(run(\"decide\", \"1200\"), serde_json::json!(\"review\")); assert_eq!(run(\"decide\", \"900\"), serde_json::json!(\"approved\")); assert_eq!(run(\"parallel\", \"42\"), serde_json::json!({\"left\":\"42\",\"right\":\"42\"})); assert_eq!(run(\"offers\", \"1200\"), serde_json::json!([\"1200\",\"1200\"])); assert_eq!(run(\"offers\", \"10\"), serde_json::json!([\"10\"]));",
    );
}

#[test]
fn graph_generated_identifiers_do_not_shadow_user_names() {
    let source = "namespace t\nversion \"1\"\ntask echo(item: Number) -> Number:\n  return item\nprocess route(values: Number) -> Number:\n  node start = start\n  node echo = task echo(values)\n  node next = task echo(echo)\n  node done = end\n  link start -> echo\n  link echo -> next\n  link next -> done(next)\n";
    compile_and_test_graph(
        source,
        "let graph = named_graph_definitions().remove(0); let input = serde_json::json!(\"7\"); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!(\"7\"));",
    );
}

#[test]
fn generated_definition_name_is_reserved_in_graph_programs() {
    let source = "namespace t\nversion \"1\"\ntask graph_definitions(item: Number) -> Number:\n  return item\nprocess route(input: Number) -> Number:\n  node start = start\n  node result = task graph_definitions(input)\n  node done = end\n  link start -> result\n  link result -> done(result)\n";
    assert!(
        transpile(source)
            .unwrap_err()
            .contains("reserved generated name")
    );
}
