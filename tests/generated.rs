use blkit::transpile;
use std::{fs, process::Command, sync::atomic::{AtomicUsize, Ordering}};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn compile_and_test(source: &str, assertion: &str) {
    let generated = transpile(source).unwrap();
    let directory = std::env::temp_dir().join(format!("blkit-generated-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir_all(directory.join("src")).unwrap();
    fs::write(directory.join("Cargo.toml"), "[package]\nname = \"blkit_generated_test\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\nrust_decimal = \"1.39\"\n").unwrap();
    fs::write(directory.join("src/lib.rs"), format!("{generated}\n#[cfg(test)] mod generated_checks {{ use super::*; #[test] fn behavior() {{ {assertion} }} }}")).unwrap();
    let result = Command::new("cargo")
        .args(["test", "--offline", "--manifest-path"])
        .arg(directory.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", directory.join("target"))
        .output().unwrap();
    assert!(result.status.success(), "generated crate failed: {}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn generated_records_enums_and_decimal_literals_compile() {
    compile_and_test("namespace orders\nversion \"1.0\"\ntype Order:\n  total: Number\n  tags: List<String>\nenum Decision:\n  approved\n  review\nprocess amount(input: Order) -> Number:\n  return 12.50\n", "let order = Order { total: Number::ONE, tags: vec![] }; assert_eq!(amount(order), \"12.50\".parse::<Number>().unwrap());");
}

#[test]
fn documented_example_compiles() {
    compile_and_test(include_str!("../examples/approve.bl"), "let order = Order { total: \"1250\".parse().unwrap(), blocked: false }; assert_eq!(approve(order), Decision::review);");
}

#[test]
fn generated_process_branches_with_typed_list_input() {
    let source = "namespace orders\nversion \"1.0\"\ntype Order:\n  total: Number\n  amounts: List<Number>\nenum Decision:\n  approved\n  review\nprocess decide(input: Order) -> Decision:\n  if input.total > 1000 and input.amounts == [1, 2.5]:\n    return Decision.review\n  else:\n    return Decision.approved\n";
    let assertion = "let order = |total: &str| Order { total: total.parse().unwrap(), amounts: vec![Number::ONE, \"2.5\".parse().unwrap()] }; assert_eq!(decide(order(\"1001\")), Decision::review); assert_eq!(decide(order(\"999\")), Decision::approved);";
    compile_and_test(source, assertion);
}

fn compile_and_test_graph(source: &str, assertion: &str) {
    let generated = transpile(source).unwrap();
    let directory = std::env::temp_dir().join(format!("blkit-graph-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir_all(directory.join("src")).unwrap();
    let root = env!("CARGO_MANIFEST_DIR").replace('\\', "\\\\");
    fs::write(directory.join("Cargo.toml"), format!("[package]\nname = \"blkit_generated_graph\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\nblkit = {{ path = {root:?} }}\nrust_decimal = {{ version = \"1.39\", features = [\"serde-str\"] }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\ntokio = {{ version = \"1\", features = [\"macros\", \"rt-multi-thread\"] }}\n")).unwrap();
    fs::write(directory.join("src/lib.rs"), format!("{generated}\n#[cfg(test)] mod checks {{ use super::*; #[tokio::test] async fn graph() {{ {assertion} }} }}")).unwrap();
    let result = Command::new("cargo").args(["test", "--manifest-path"]).arg(directory.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", std::env::temp_dir().join("blkit-graph-test-target")).output().unwrap();
    assert!(result.status.success(), "generated graph failed: {}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn generated_graph_compiles_as_a_runtime_definition() {
    compile_and_test_graph(include_str!("../examples/graph.bl"), "let registry = blkit::runtime::Registry::new(graph_definitions()).unwrap(); let definition = registry.get(\"orders\", \"1.0\", \"decide\").unwrap(); assert_eq!(definition.evaluate(serde_json::json!({\"total\":\"1200\"})).await.unwrap(), serde_json::json!(\"review\")); assert_eq!(definition.evaluate(serde_json::json!({\"total\":\"900\"})).await.unwrap(), serde_json::json!(\"approved\")); let parallel = registry.get(\"orders\", \"1.0\", \"parallel\").unwrap(); assert_eq!(parallel.evaluate(serde_json::json!({\"total\":\"42\"})).await.unwrap(), serde_json::json!({\"left\":\"42\",\"right\":\"42\"})); let offers = registry.get(\"orders\", \"1.0\", \"offers\").unwrap(); assert_eq!(offers.evaluate(serde_json::json!({\"total\":\"1200\"})).await.unwrap(), serde_json::json!([\"1200\",\"1200\"])); assert_eq!(offers.evaluate(serde_json::json!({\"total\":\"10\"})).await.unwrap(), serde_json::json!([\"10\"]));");
}

#[test]
fn graph_generated_identifiers_do_not_shadow_user_names() {
    let source = "namespace t\nversion \"1\"\ntask echo(item: Number) -> Number:\n  return item\nprocess route(values: Number) -> Number:\n  run echo = echo(values)\n  run next = echo(echo)\n  return next\n";
    compile_and_test_graph(source, "let definition = blkit::runtime::Registry::new(graph_definitions()).unwrap(); assert_eq!(definition.get(\"t\", \"1\", \"route\").unwrap().evaluate(serde_json::json!(\"7\")).await.unwrap(), serde_json::json!(\"7\"));");
}

#[test]
fn generated_definition_name_is_reserved_in_graph_programs() {
    let source = "namespace t\nversion \"1\"\ntask graph_definitions(item: Number) -> Number:\n  return item\nprocess route(input: Number) -> Number:\n  run node = graph_definitions(input)\n  return node\n";
    assert!(transpile(source).unwrap_err().contains("reserved generated name"));
}
