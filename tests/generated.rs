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
    fs::write(directory.join("Cargo.toml"), "[package]\nname = \"blkit_generated_test\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\nrust_decimal = \"1.39\"\n").unwrap();
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
fn generated_records_enums_and_decimal_literals_compile() {
    compile_and_test(
        "namespace orders\nversion \"1.0\"\ntype Order:\n  total: Number\n  tags: List<String>\nenum Decision:\n  approved\n  review\ntask amount(input: Order) -> Number:\n  return 12.50\n",
        "let order = Order { total: Number::ONE, tags: vec![] }; assert_eq!(amount(order), \"12.50\".parse::<Number>().unwrap());",
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
    fs::write(directory.join("Cargo.toml"), format!("[package]\nname = \"blkit_generated_graph\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\nblkit = {{ path = {root:?} }}\nrust_decimal = {{ version = \"1.39\", features = [\"serde-str\"] }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\ntokio = {{ version = \"1\", features = [\"macros\", \"rt-multi-thread\"] }}\n")).unwrap();
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
