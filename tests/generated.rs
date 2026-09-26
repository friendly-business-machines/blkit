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
