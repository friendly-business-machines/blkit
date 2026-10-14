use blkit_transpiler::transpile;
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn nested_target() -> std::path::PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(Into::into)
        .unwrap_or_else(|| std::env::temp_dir().join("blkit-generated-test-target"))
}

fn compile_and_test(source: &str, assertion: &str) {
    let generated = transpile(source).unwrap();
    let directory = std::env::temp_dir().join(format!(
        "blkit-generated-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(directory.join("src")).unwrap();
    let name = directory.file_name().unwrap().to_string_lossy();
    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../blkit-core");
    fs::write(directory.join("Cargo.toml"), format!("[package]\nname = {name:?}\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\nblkit-core = {{ path = {core:?} }}\nrust_decimal = \"1.39\"\nchrono = {{ version = \"0.4\", features = [\"serde\"] }}\nserde = \"1\"\nserde_json = \"1\"\n")).unwrap();
    fs::write(directory.join("src/lib.rs"), format!("{generated}\n#[cfg(test)] mod generated_checks {{ use blkit_core as blkit; use super::*; #[test] fn behavior() {{ {assertion} }} }}")).unwrap();
    let result = Command::new("cargo")
        .args(["test", "--offline", "--manifest-path"])
        .arg(directory.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", nested_target())
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
fn direct_source_compiles_with_core_without_transpiler_dependency() {
    let source = "namespace orders; version \"1\"; start_event start { output value: Number; } end_event done { input result: Number; } process route { flow start -> done; bind start.value -> done.result; }";
    let generated = transpile(source).unwrap();
    assert!(
        generated.contains("blkit_core::compiled_graph"),
        "{generated}"
    );
    let directory = std::env::temp_dir().join(format!(
        "blkit-direct-core-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(directory.join("src")).unwrap();
    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../blkit-core");
    fs::write(
        directory.join("Cargo.toml"),
        format!(
            "[package]\nname = \"direct_core_consumer\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\nblkit-core = {{ version = \"=0.1.0\", path = {core:?}, default-features = false }}\nrust_decimal = {{ version = \"1.39\", features = [\"serde-str\"] }}\nchrono = {{ version = \"0.4\", features = [\"serde\"] }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\n"
        ),
    )
    .unwrap();
    fs::write(directory.join("src/lib.rs"), generated).unwrap();
    let output = Command::new("cargo")
        .args(["check", "--offline", "--manifest-path"])
        .arg(directory.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", nested_target())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn numeric_process_route_propagates_arithmetic_errors_and_keeps_number_ports() {
    compile_and_test_graph(
        r#"namespace numbers; version "1";
start_event start { output value: Number; }
xor_split gate {}
xor_join joined { split gate; input value: Number; output result: Number; }
end_event done { input result: Number; }
decision_task divide { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression 10 / value; } }
decision_task fallback { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression 0; } }
process route {
  flow start -> gate;
  flow gate -> divide when start.value + 1 > 0 and 10 / start.value > 0;
  flow gate -> fallback else;
  flow divide -> joined;
  flow fallback -> joined;
  flow joined -> done;
  bind start.value -> divide.value;
  bind start.value -> fallback.value;
  bind divide.result -> joined.value;
  bind fallback.result -> joined.value;
  bind joined.result -> done.result;
}
"#,
        r#"let graph = named_graph_definitions().remove(0); for (input, expected) in [("4", "2.50"), ("-5", "0")] { let value = serde_json::json!({"value":input}); let mut state = graph.checkpoint(&value).unwrap(); assert_eq!(graph.run(&value, &mut state).unwrap_or_else(|error| panic!("{input}: {error}")), serde_json::json!(expected)); } for input in ["0", "79228162514264337593543950335"] { let value = serde_json::json!({"value":input}); assert!(graph.checkpoint(&value).and_then(|mut state| graph.run(&value, &mut state)).is_err(), "{input}"); }"#,
    );
}

#[test]
fn standalone_list_knowledge_and_context_compose_and_report_invalid_positions() {
    compile_and_test(
        r#"namespace lists; version "1";
decision_task summarize {
  input values: List<Number>;
  input position: Number;
  output result: Number = total;
  knowledge selected {
    input items: List<Number>;
    output result: List<Number>;
    expression sort(distinct(items[.item > 0]), "desc");
  }
  context total {
    output result: Number;
    entry subtotal: Number = sum(selected(values));
    entry remainder: List<Number> = sublist(values, position);
    result subtotal + count(remainder);
  }
}"#,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; assert_eq!(summarize(vec![n("3"), n("-1"), n("3"), n("2")], n("2")).unwrap(), n("8")); for position in ["0", "1.5", "2"] { let error = summarize(vec![n("1")], n(position)).unwrap_err(); assert!(error.to_string().contains("list position"), "{position}: {error}"); }"#,
    );
}

#[test]
fn graph_list_condition_routes_and_reports_invalid_index() {
    compile_and_test_graph(
        r#"namespace lists; version "1";
start_event start { output values: List<Number>; }
xor_split gate {}
end_event done { input result: Number; }
decision_task calculate { input values: List<Number>; output result: Number = calc; literal_expression calc { output result: Number; expression sum(sort(distinct(values))); } }
decision_task fallback { input values: List<Number>; output result: Number = calc; literal_expression calc { output result: Number; expression 0; } }
process route {
  flow start -> gate;
  flow gate -> calculate when start.values[1] > 0 and count(start.values[.item > 0]) > 0;
  flow gate -> fallback else;
  flow calculate -> done;
  flow fallback -> done;
  bind start.values -> calculate.values;
  bind start.values -> fallback.values;
  bind calculate.result -> done.result;
  bind fallback.result -> done.result;
}"#,
        r#"let graph = named_graph_definitions().remove(0); for (values, expected) in [(serde_json::json!(["2","1","2"]), serde_json::json!("3")), (serde_json::json!(["-1","2"]), serde_json::json!("0"))] { let input = serde_json::json!({"values":values}); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), expected); } let input = serde_json::json!({"values":[]}); let error = graph.checkpoint(&input).and_then(|mut state| graph.run(&input, &mut state)).unwrap_err(); assert!(error.contains("list position out of bounds"), "{error}");"#,
    );
}

#[test]
fn generated_list_expressions_preserve_order_scope_and_checked_errors() {
    let mut source =
        String::from("namespace lists; version \"1\";\nOrder = {name: String, amount: Number};\n");
    for (name, input, output, expression) in [
        ("index", "List<Number>", "Number", "value[-1]"),
        (
            "filter",
            "List<Number>",
            "List<Number>",
            "value[.item mod 2 == 0]",
        ),
        ("project", "List<Order>", "List<String>", "value.name"),
        ("member", "List<Number>", "Bool", "2 in value"),
        ("equal", "List<Number>", "Bool", "value == [1,2]"),
        (
            "mapped",
            "List<Number>",
            "List<Number>",
            "for x in value, y in [x, x + 1] return y",
        ),
        (
            "all",
            "List<Number>",
            "Bool",
            "every x in value, y in [x, x + 1] satisfies y >= x",
        ),
        (
            "any",
            "List<Number>",
            "Bool",
            "some x in value, y in [x, x + 1] satisfies y > 2",
        ),
        ("sequence", "Number", "List<Number>", "4:2"),
    ] {
        source.push_str(&format!("decision_task {name} {{ input value: {input}; output result: {output} = calc; literal_expression calc {{ output result: {output}; expression {expression}; }} }}\n"));
    }
    compile_and_test_graph(
        &source,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; assert_eq!(index(vec![n("1"),n("2")]).unwrap(), n("2")); assert!(index(vec![]).is_err()); assert_eq!(filter(vec![n("1"),n("2"),n("3"),n("4")]).unwrap(), vec![n("2"),n("4")]); assert!(filter(vec![n("1"),n("2")]).is_ok()); assert_eq!(project(vec![Order { name: "A".into(), amount: n("1") }, Order { name: "B".into(), amount: n("2") }]).unwrap(), vec!["A", "B"]); assert!(member(vec![n("2")] ).unwrap()); assert!(equal(vec![n("1"),n("2")]).unwrap()); assert_eq!(mapped(vec![n("1"),n("2")]).unwrap(), vec![n("1"),n("2"),n("2"),n("3")]); assert!(all(vec![]).unwrap()); assert!(!any(vec![]).unwrap()); assert!(any(vec![n("2")]).unwrap()); assert_eq!(sequence(n("0")).unwrap(), vec![n("4"),n("3"),n("2")]);"#,
    );
}

#[test]
fn generated_readme_list_expression_examples_compile_and_execute() {
    let mut source =
        String::from("namespace lists; version \"1\";\nOrder = {name: String, item: Number};\n");
    for (name, input, output, expression) in [
        ("first", "Number", "Number", "[1,2,3,4][1]"),
        ("from_end", "Number", "Number", "[1,2,3,4][-2]"),
        ("filtered", "Number", "List<Number>", "[1,2,3,4][.item > 2]"),
        (
            "even",
            "Number",
            "List<Number>",
            "[1,2,3,4][.item mod 2 == 0]",
        ),
        ("odd_values", "Number", "List<Number>", "[1,2][odd(.item)]"),
        ("same", "Number", "Bool", "[1,2] == [1,2]"),
        ("different", "Number", "Bool", "[1,2] != [2,1]"),
        ("member", "Number", "Bool", "2 in [1,2,3]"),
        (
            "pairs",
            "Number",
            "List<Number>",
            "for x in [1,2], y in [3,4] return x * y",
        ),
        (
            "any_pair",
            "Number",
            "Bool",
            "some x in [1,2], y in [3,4] satisfies x + y > 5",
        ),
        (
            "all_pairs",
            "Number",
            "Bool",
            "every x in [1,2], y in [3,4] satisfies x + y >= 4",
        ),
        ("descending", "Number", "List<Number>", "4:2"),
        ("empty", "Number", "List<Number>", "[]"),
        ("position", "Number", "Number", "[4,5][item]"),
        ("field", "Order", "Number", "order.item"),
        ("names", "List<Order>", "List<String>", "orders.name"),
    ] {
        let input_name = match name {
            "position" => "item",
            "field" => "order",
            "names" => "orders",
            _ => "unused",
        };
        source.push_str(&format!("decision_task {name} {{ input {input_name}: {input}; output result: {output} = calc; literal_expression calc {{ output result: {output}; expression {expression}; }} }}\n"));
    }
    compile_and_test_graph(
        &source,
        r#"let n = |v: i64| Number::from(v);
        assert_eq!(first(n(0)).unwrap(), n(1));
        assert_eq!(from_end(n(0)).unwrap(), n(3));
        assert_eq!(filtered(n(0)).unwrap(), vec![n(3), n(4)]);
        assert_eq!(even(n(0)).unwrap(), vec![n(2), n(4)]);
        assert_eq!(odd_values(n(0)).unwrap(), vec![n(1)]);
        assert!(same(n(0)).unwrap());
        assert!(different(n(0)).unwrap());
        assert!(member(n(0)).unwrap());
        assert_eq!(pairs(n(0)).unwrap(), vec![n(3), n(4), n(6), n(8)]);
        assert!(any_pair(n(0)).unwrap());
        assert!(all_pairs(n(0)).unwrap());
        assert_eq!(descending(n(0)).unwrap(), vec![n(4), n(3), n(2)]);
        assert_eq!(empty(n(0)).unwrap(), Vec::<Number>::new());
        assert_eq!(position(n(2)).unwrap(), n(5));
        assert_eq!(field(Order { name: "A".into(), item: n(7) }).unwrap(), n(7));
        assert_eq!(names(vec![Order { name: "A".into(), item: n(7) }, Order { name: "B".into(), item: n(8) }]).unwrap(), vec!["A", "B"]);"#,
    );
}

#[test]
fn generated_readme_sort_join_sequence_and_boolean_examples_compile_and_execute() {
    let mut source = String::from("namespace lists; version \"1\";\n");
    let mut assertions = String::from("let n = |value: &str| value.parse::<Number>().unwrap();\n");
    let readme = include_str!("../../../README.md");
    for (name, output, expression, expected) in [
        (
            "natural",
            "List<Number>",
            "sort([3,1,2])",
            "vec![n(\"1\"),n(\"2\"),n(\"3\")]",
        ),
        (
            "descending",
            "List<Number>",
            "sort([3,1,2], \"desc\")",
            "vec![n(\"3\"),n(\"2\"),n(\"1\")]",
        ),
        (
            "priority",
            "List<String>",
            "sort([\"m\",\"s\",\"xl\"], [\"s\",\"m\",\"l\"])",
            "vec![\"s\",\"m\",\"xl\"]",
        ),
        ("joined", "String", "stringJoin([\"a\",\"b\"])", "\"ab\""),
        (
            "wrapped",
            "String",
            "stringJoin([\"a\",\"b\"], \"-\", \"[\", \"]\")",
            "\"[a-b]\"",
        ),
        (
            "empty_join",
            "String",
            "stringJoin([], \"-\", \"[\", \"]\")",
            "\"[]\"",
        ),
        (
            "zip_scalar",
            "List<String>",
            "zipStringJoin([[\"a\",\"b\"],[\"1\",\"2\"]], \"-\", \"[\", \"]\")",
            "vec![\"[a-1]\",\"[b-2]\"]",
        ),
        (
            "zip_columns",
            "List<String>",
            "zipStringJoin([[\"a\",\"b\"],[\"1\",\"2\"],[\"x\",\"y\"]], [\"-\", \":\"], [\"<\",\"(\",\"{\"], [\">\",\")\",\"}\"])",
            "vec![\"<a>-(1):{x}\",\"<b>-(2):{y}\"]",
        ),
        (
            "zip_empty",
            "List<String>",
            "zipStringJoin([], [], [], [])",
            "Vec::<String>::new()",
        ),
        (
            "forward",
            "List<Number>",
            "seq(5,10)",
            "vec![n(\"5\"),n(\"6\"),n(\"7\"),n(\"8\"),n(\"9\"),n(\"10\")]",
        ),
        (
            "backward",
            "List<Number>",
            "seq(10,5)",
            "vec![n(\"10\"),n(\"9\"),n(\"8\"),n(\"7\"),n(\"6\"),n(\"5\")]",
        ),
        (
            "fractional",
            "List<Number>",
            "seq(0,1,0.25)",
            "vec![n(\"0\"),n(\"0.25\"),n(\"0.5\"),n(\"0.75\"),n(\"1\")]",
        ),
        (
            "opposite",
            "List<Number>",
            "seq(5,10,-1)",
            "vec![n(\"5\"),n(\"6\"),n(\"7\"),n(\"8\"),n(\"9\"),n(\"10\")]",
        ),
        (
            "unreached",
            "List<Number>",
            "seq(0,0.95,0.1)",
            "vec![n(\"0\"),n(\"0.1\"),n(\"0.2\"),n(\"0.3\"),n(\"0.4\"),n(\"0.5\"),n(\"0.6\"),n(\"0.7\"),n(\"0.8\"),n(\"0.9\")]",
        ),
        (
            "colon",
            "List<Number>",
            "10:5",
            "vec![n(\"10\"),n(\"9\"),n(\"8\"),n(\"7\"),n(\"6\"),n(\"5\")]",
        ),
        ("all_values", "Bool", "all([true,true,false])", "false"),
        ("any_values", "Bool", "any([false,false,true])", "true"),
        ("all_empty", "Bool", "all([])", "true"),
        ("any_empty", "Bool", "any([])", "false"),
    ] {
        assert!(
            readme.contains(&format!("| `{expression}` |")),
            "README missing {expression}"
        );
        source.push_str(&format!("decision_task {name} {{ input unused: Number; output result: {output} = calc; literal_expression calc {{ output result: {output}; expression {expression}; }} }}\n"));
        assertions.push_str(&format!(
            "assert_eq!({name}(Number::ZERO).unwrap(), {expected});\n"
        ));
    }
    compile_and_test(&source, &assertions);
}

#[test]
fn generated_readme_aggregate_migration_and_examples_compile_and_execute() {
    let migration = r#"decision_task modes {
  input values: List<Number>;
  output result: List<Number> = calc;
  literal_expression calc {
    output result: List<Number>;
    expression mode(values);
  }
}"#;
    let readme = include_str!("../../../README.md");
    assert!(readme.contains(migration), "README missing mode migration");
    let mut source = format!("namespace aggregates; version \"1\";\n{migration}\n");
    let mut assertions = String::from(
        "let n = |v: &str| v.parse::<Number>().unwrap();\n\
         assert_eq!(modes(vec![n(\"3\"),n(\"2\"),n(\"3\"),n(\"2\")]).unwrap(), vec![n(\"2\"),n(\"3\")]);\n\
         assert_eq!(modes(vec![n(\"3\"),n(\"2\"),n(\"3\")]).unwrap(), vec![n(\"3\")]);\n\
         assert!(modes(vec![]).is_err());\n\
         assert!(modes(vec![n(\"1\"); 1025]).is_err());\n",
    );
    for (name, output, expression, displayed, expected) in [
        (
            "ties",
            "List<Number>",
            "mode([6,1,6,1])",
            "[1,6]",
            "vec![n(\"1\"),n(\"6\")]",
        ),
        (
            "winner",
            "List<Number>",
            "mode([3,2,3])",
            "[3]",
            "vec![n(\"3\")]",
        ),
        ("first", "Number", "mode([6,1,6,1])[1]", "1", "n(\"1\")"),
        (
            "hours",
            "DTDuration",
            "sum([dtDuration(\"PT1H\"),dtDuration(\"PT2H\")])",
            "dtDuration(\"PT3H\")",
            "\"PT3H\"",
        ),
        (
            "months",
            "YMDuration",
            "sum([ymDuration(\"P1Y\"),ymDuration(\"P6M\")])",
            "ymDuration(\"P1Y6M\")",
            "\"P1Y6M\"",
        ),
        (
            "average",
            "DTDuration",
            "mean([dtDuration(\"PT1H\"),dtDuration(\"PT2H\")])",
            "dtDuration(\"PT1H30M\")",
            "\"PT1H30M\"",
        ),
        (
            "middle",
            "YMDuration",
            "median([ymDuration(\"P1Y\"),ymDuration(\"P2Y\")])",
            "ymDuration(\"P1Y6M\")",
            "\"P1Y6M\"",
        ),
        (
            "low",
            "String",
            "min([\"banana\",\"Apple\"])",
            "\"Apple\"",
            "\"Apple\"",
        ),
        (
            "high",
            "Date",
            "max([date(\"2025-01-01\"),date(\"2025-02-01\")])",
            "date(\"2025-02-01\")",
            "\"2025-02-01\"",
        ),
        (
            "multiplied",
            "Number",
            "product([2,3,4])",
            "24",
            "n(\"24\")",
        ),
        ("deviation", "Number", "stddev([1,2,3])", "1", "n(\"1\")"),
        ("zero", "Number", "sum([])", "0", "n(\"0\")"),
        ("one", "Number", "product([])", "1", "n(\"1\")"),
        (
            "zero_duration",
            "DTDuration",
            "sum([])",
            "dtDuration(\"PT0S\")",
            "\"PT0S\"",
        ),
    ] {
        assert!(
            readme.contains(&format!("| `{expression}` | `{output}` | `{displayed}` |")),
            "README missing {expression}: {output} = {displayed}"
        );
        source.push_str(&format!("decision_task {name} {{ input unused: Number; output result: {output} = calc; literal_expression calc {{ output result: {output}; expression {expression}; }} }}\n"));
        if output == "DTDuration" || output == "YMDuration" || output == "Date" {
            assertions.push_str(&format!(
                "assert_eq!({name}(Number::ZERO).unwrap().to_string(), {expected});\n"
            ));
        } else {
            assertions.push_str(&format!(
                "assert_eq!({name}(Number::ZERO).unwrap(), {expected});\n"
            ));
        }
    }
    compile_and_test(&source, &assertions);
}

#[test]
fn generated_readme_list_operation_examples_compile_and_execute() {
    let mut source = String::from("namespace lists; version \"1\";\n");
    let mut assertions = String::from("let n = |v: i64| Number::from(v);\n");
    for (name, output, expression, expected) in [
        ("count_three", "Number", "count([1,2,3])", "n(3)"),
        ("is_empty", "Bool", "isEmpty(input)", "true"),
        ("contains", "Bool", "listContains([1,2,3], 2)", "true"),
        (
            "positions",
            "List<Number>",
            "indexOf([1,2,3,2], 2)",
            "vec![n(2),n(4)]",
        ),
        (
            "slice",
            "List<Number>",
            "sublist([1,2,3], -2)",
            "vec![n(2),n(3)]",
        ),
        (
            "append",
            "List<Number>",
            "append([1], 2, 3)",
            "vec![n(1),n(2),n(3)]",
        ),
        (
            "prepend",
            "List<Number>",
            "prepend([2,3], 1)",
            "vec![n(1),n(2),n(3)]",
        ),
        (
            "concatenate",
            "List<Number>",
            "concatenate([1,2], [3])",
            "vec![n(1),n(2),n(3)]",
        ),
        (
            "union",
            "List<Number>",
            "union([2,1], [1,3])",
            "vec![n(2),n(1),n(3)]",
        ),
        (
            "reverse",
            "List<Number>",
            "reverse([1,2,3])",
            "vec![n(3),n(2),n(1)]",
        ),
        (
            "flatten",
            "List<Number>",
            "flatten([[[1,2]],[[3,4]]])",
            "vec![n(1),n(2),n(3),n(4)]",
        ),
        (
            "before_first",
            "List<Number>",
            "insertBefore([1,3], 1, 2)",
            "vec![n(2),n(1),n(3)]",
        ),
        (
            "before_last",
            "List<Number>",
            "insertBefore([10,20,30], -1, 5)",
            "vec![n(10),n(20),n(5),n(30)]",
        ),
        (
            "after_last",
            "List<Number>",
            "insertAfter([10,20,30], -1, 5)",
            "vec![n(10),n(20),n(30),n(5)]",
        ),
        (
            "spread",
            "List<Number>",
            "insertAfter([1,5], 1, [2,3,4])",
            "vec![n(1),n(2),n(3),n(4),n(5)]",
        ),
        (
            "nested",
            "List<List<Number>>",
            "insertBefore([[1],[4]], 2, [[2,3]])",
            "vec![vec![n(1)],vec![n(2),n(3)],vec![n(4)]]",
        ),
        (
            "remove_second",
            "List<Number>",
            "remove([1,2,3], 2)",
            "vec![n(1),n(3)]",
        ),
        (
            "remove_last",
            "List<Number>",
            "remove([10,20,30], -1)",
            "vec![n(10),n(20)]",
        ),
        (
            "replace_second",
            "List<Number>",
            "listReplace([1,2,3], 2, 9)",
            "vec![n(1),n(9),n(3)]",
        ),
        (
            "replace_last",
            "List<Number>",
            "listReplace([10,20,30], -1, 5)",
            "vec![n(10),n(20),n(5)]",
        ),
        (
            "remove_matches",
            "List<Number>",
            "remove([1,2,3,2], function(i) i == 2)",
            "vec![n(1),n(3)]",
        ),
        (
            "replace_matches",
            "List<Number>",
            "listReplace([2,4,7], function(i) i < 5, 5)",
            "vec![n(5),n(5),n(7)]",
        ),
        (
            "distinct",
            "List<Number>",
            "distinct([2,1,2,3,1])",
            "vec![n(2),n(1),n(3)]",
        ),
        (
            "duplicates",
            "List<Number>",
            "duplicateValues([2,1,1,2,2])",
            "vec![n(2),n(1)]",
        ),
        (
            "intersection",
            "List<Number>",
            "intersection([3,2,3,1], [2,3])",
            "vec![n(3),n(2)]",
        ),
    ] {
        let name = format!("example_{name}");
        source.push_str(&format!("decision_task {name} {{ input input: List<Number>; output result: {output} = calc; literal_expression calc {{ output result: {output}; expression {expression}; }} }}\n"));
        assertions.push_str(&format!(
            "assert_eq!({name}(vec![]).unwrap(), {expected});\n"
        ));
    }
    compile_and_test(&source, &assertions);
}

#[test]
fn generated_context_item_filters_and_numeric_entry_positions_remain_distinct() {
    let source = r#"namespace lists; version "1";
decision_task filter { input input: Number; output result: Number = value; context value { output result: Number; entry item: Number = sum(selected); entry selected: List<Number> = [1,2][odd(.item)]; result item; } }
decision_task position { input input: Number; output result: Number = value; context value { output result: Number; entry selected: Number = [4,5][item]; entry item: Number = input; result selected; } }
"#;
    compile_and_test_graph(
        source,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; assert_eq!(filter(n("0")).unwrap(), n("1")); assert_eq!(position(n("2")).unwrap(), n("5")); assert!(position(n("0")).is_err());"#,
    );
}

#[test]
fn generated_item_filters_and_numeric_input_positions_remain_distinct() {
    let source = r#"namespace lists;
version "1";
decision_task field_filter { input orders: List<Order>; output result: List<Order> = selected; literal_expression selected { output result: List<Order>; expression orders[.item.active];}}
Order = {active: Bool};
decision_task call_filter { input input: List<Number>; output result: List<Number> = selected; knowledge positive { input value: Number; output result: Bool; expression value > 0; } literal_expression selected { output result: List<Number>; expression input[positive(.item)]; } }
decision_task numeric_position { input item: Number; output result: Number = selected; literal_expression selected { output result: Number; expression [4,5][item]; } }
"#;
    compile_and_test_graph(
        source,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; assert_eq!(field_filter(vec![Order { active: false }, Order { active: true }]).unwrap(), vec![Order { active: true }]); assert_eq!(call_filter(vec![n("-1"),n("2")]).unwrap(), vec![n("2")]); assert_eq!(numeric_position(n("2")).unwrap(), n("5")); assert!(numeric_position(n("0")).is_err());"#,
    );
}

#[test]
fn generated_explicit_filter_item_preserves_outer_names_and_nested_scope() {
    let source = r#"namespace lists; version "1";
decision_task outer { input values: List<Number>; input item: Bool; output result: List<Number> = selected; literal_expression selected { output result: List<Number>; expression values[.item > 1 and item]; } }
decision_task nested { input value: Number; output result: List<Number> = selected; literal_expression selected { output result: List<Number>; expression [1,2][[.item,3][.item > 1] == [3]]; } }
decision_task node { input value: Number; output result: Number = item; literal_expression item { output result: Number; expression value + 1; } }
"#;
    compile_and_test_graph(
        source,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; assert_eq!(outer(vec![n("1"),n("2")], true).unwrap(), vec![n("2")]); assert_eq!(outer(vec![n("1"),n("2")], false).unwrap(), Vec::<Number>::new()); assert_eq!(nested(n("0")).unwrap(), vec![n("1")]); assert_eq!(node(n("2")).unwrap(), n("3"));"#,
    );
}

#[test]
fn generated_context_mixed_explicit_filter_and_numeric_item_dependencies() {
    let source = r#"namespace lists; version "1";
decision_task mixed { input input: Number; output result: Number = value; context value { output result: Number; entry selected: Number = [4,5][item]; entry subtotal: Number = sum(values); entry item: Number = subtotal; entry values: List<Number> = [1,2][odd(.item)]; result selected; } }
"#;
    compile_and_test_graph(
        source,
        r#"assert_eq!(mixed(Number::ZERO).unwrap(), Number::from(4));"#,
    );
}

#[test]
fn generated_decision_item_node_is_a_checked_numeric_index_dependency() {
    let source = r#"namespace lists; version "1";
decision_task indexed { input position: Number; output result: Number = selected;
  literal_expression selected { output result: Number; expression [4,5][item]; }
  literal_expression item { output result: Number; expression position; }
}"#;
    compile_and_test_graph(
        source,
        r#"assert_eq!(indexed(Number::from(2)).unwrap(), Number::from(5)); assert!(indexed(Number::ZERO).is_err());"#,
    );
}

#[test]
fn generated_list_operands_do_not_capture_user_names() {
    let source = "namespace lists; version \"1\"; decision_task sequence { input current: Number; output result: List<Number> = calc; literal_expression calc { output result: List<Number>; expression 1:current; } } decision_task position { input items: Number; output result: Number = calc; literal_expression calc { output result: Number; expression [4,5][items]; } } decision_task filter { input result: Number; output selected: List<Number> = calc; literal_expression calc { output selected: List<Number>; expression [1,2,3][.item > result]; } }";
    compile_and_test_graph(
        source,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; assert_eq!(sequence(n("3")).unwrap(), vec![n("1"),n("2"),n("3")]); assert_eq!(position(n("2")).unwrap(), n("5")); assert_eq!(filter(n("1")).unwrap(), vec![n("2"),n("3")]);"#,
    );
}

#[test]
fn generated_iteration_enum_variant_and_shadowed_dictionary_field_are_distinct() {
    let source = "namespace lists; version \"1\"; enum Status:\n  ready;\ndecision_task variant { input input: Number; output result: List<Status> = value; literal_expression value { output result: List<Status>; expression for x in [Status.ready] return x; } } decision_task shadow { input input: Number; output result: List<Bool> = value; literal_expression value { output result: List<Bool>; expression for Status in [{ready: true}] return Status.ready; } }";
    compile_and_test_graph(
        source,
        r#"assert_eq!(variant(Number::ZERO).unwrap(), vec![Status::ready]); assert_eq!(shadow(Number::ZERO).unwrap(), vec![true]);"#,
    );
}

#[test]
fn generated_iteration_temporaries_do_not_capture_user_names() {
    let source = "namespace lists; version \"1\"; decision_task mapped { input __bl_iteration: Number; output result: List<Number> = calc; literal_expression calc { output result: List<Number>; expression for x in [1] return x + __bl_iteration; } } decision_task every_match { input __bl_every: Number; output result: Bool = calc; literal_expression calc { output result: Bool; expression every x in [1] satisfies x < __bl_every; } } decision_task some_match { input __bl_some: Number; output result: Bool = calc; literal_expression calc { output result: Bool; expression some x in [1] satisfies x == __bl_some; } }";
    compile_and_test_graph(
        source,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; assert_eq!(mapped(n("2")).unwrap(), vec![n("3")]); assert!(every_match(n("2")).unwrap()); assert!(some_match(n("1")).unwrap());"#,
    );
}

#[test]
fn generated_record_projection_composes_with_sum() {
    let source = "namespace lists; version \"1\"; Order = {amount: Number}; decision_task total { input orders: List<Order>; output result: Number = calc; literal_expression calc { output result: Number; expression sum(orders.amount); } }";
    compile_and_test_graph(
        source,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; assert_eq!(total(vec![Order { amount: n("2") }, Order { amount: n("3") }]).unwrap(), n("5"));"#,
    );
}

#[test]
fn generated_empty_filter_uses_contextual_element_type() {
    let source = "namespace lists; version \"1\"; decision_task filtered { input value: Number; output result: List<Number> = calc; literal_expression calc { output result: List<Number>; expression [][.item > 0]; } }";
    compile_and_test_graph(
        source,
        r#"assert_eq!(filtered(Number::ZERO).unwrap(), Vec::<Number>::new());"#,
    );
}

#[test]
fn generated_empty_filter_in_knowledge_argument_uses_parameter_type() {
    let source = "namespace lists; version \"1\"; decision_task filtered { input value: Number; output result: List<Number> = calc; knowledge positive_list { input items: List<Number>; output result: List<Number>; expression items; } literal_expression calc { output result: List<Number>; expression positive_list([][.item > 0]); } }";
    compile_and_test_graph(
        source,
        r#"assert_eq!(filtered(Number::ZERO).unwrap(), Vec::<Number>::new());"#,
    );
}

#[test]
fn generated_nested_empty_filter_uses_output_element_type() {
    let source = "namespace lists; version \"1\"; decision_task filtered { input value: Number; output result: List<List<Number>> = calc; literal_expression calc { output result: List<List<Number>>; expression [[][.item > 0]]; } }";
    compile_and_test_graph(
        source,
        r#"assert_eq!(filtered(Number::ZERO).unwrap(), vec![Vec::<Number>::new()]);"#,
    );
}

#[test]
fn generated_nested_empty_filter_in_knowledge_argument_uses_parameter_element_type() {
    let source = "namespace lists; version \"1\"; decision_task filtered { input value: Number; output result: List<List<Number>> = calc; knowledge consume { input items: List<List<Number>>; output result: List<List<Number>>; expression items; } literal_expression calc { output result: List<List<Number>>; expression consume([[][.item > 0]]); } }";
    compile_and_test_graph(
        source,
        r#"assert_eq!(filtered(Number::ZERO).unwrap(), vec![Vec::<Number>::new()]);"#,
    );
}

#[test]
fn generated_empty_iteration_source_infers_element_from_body() {
    let source = "namespace lists; version \"1\"; decision_task mapped { input value: Number; output result: List<Bool> = calc; literal_expression calc { output result: List<Bool>; expression for x in [] return x > 0; } }";
    compile_and_test_graph(
        source,
        r#"assert_eq!(mapped(Number::ZERO).unwrap(), Vec::<bool>::new());"#,
    );
}

#[test]
fn generated_empty_iteration_sources_use_body_types_for_map_and_quantifiers() {
    let source = "namespace lists; version \"1\"; decision_task mapped { input input: Number; output result: List<Bool> = calc; knowledge positive { input n: Number; output result: Bool; expression n > 0; } literal_expression calc { output result: List<Bool>; expression for x in [] return positive(x); } } decision_task all { input input: Number; output result: Bool = calc; literal_expression calc { output result: Bool; expression every x in [] satisfies x > 0; } } decision_task any { input input: Number; output result: Bool = calc; literal_expression calc { output result: Bool; expression some x in [] satisfies odd(x); } }";
    compile_and_test_graph(
        source,
        r#"assert_eq!(mapped(Number::ZERO).unwrap(), Vec::<bool>::new()); assert!(all(Number::ZERO).unwrap()); assert!(!any(Number::ZERO).unwrap());"#,
    );
}

#[test]
fn generated_empty_cartesian_binding_uses_later_source_constraint() {
    let source = "namespace lists; version \"1\"; decision_task mapped { input value: Number; output result: List<Number> = calc; literal_expression calc { output result: List<Number>; expression for x in [], y in [x, x + 1] return 1; } }";
    compile_and_test_graph(
        source,
        r#"assert_eq!(mapped(Number::ONE).unwrap(), Vec::<Number>::new());"#,
    );
}

#[test]
fn generated_unused_empty_binding_can_be_shadowed_by_nested_iteration() {
    let source = "namespace lists; version \"1\"; decision_task mapped { input value: Number; output result: List<List<Bool>> = calc; literal_expression calc { output result: List<List<Bool>>; expression for x in [] return for x in [true] return x; } }";
    compile_and_test_graph(
        source,
        r#"assert_eq!(mapped(Number::ONE).unwrap(), Vec::<Vec<bool>>::new());"#,
    );
}

#[test]
fn generated_empty_iteration_calls_compile_with_shared_binding_types() {
    let source = "namespace lists; version \"1\"; decision_task aggregate { input input: Number; output result: List<Number> = calc; literal_expression calc { output result: List<Number>; expression for x in [] return x + min([1, 2]); } } decision_task relation { input input: Number; output result: List<Bool> = calc; literal_expression calc { output result: List<Bool>; expression for x in [] return before(0, [x..5]); } }";
    compile_and_test_graph(
        source,
        r#"assert_eq!(aggregate(Number::ZERO).unwrap(), Vec::<Number>::new()); assert_eq!(relation(Number::ZERO).unwrap(), Vec::<bool>::new());"#,
    );
}

#[test]
fn generated_empty_iteration_source_uses_duration_result_constraint() {
    let source = "namespace lists; version \"1\"; decision_task mapped { input value: Number; output result: List<DTDuration> = calc; literal_expression calc { output result: List<DTDuration>; expression for x in [] return 0 - x; } }";
    compile_and_test_graph(
        source,
        r#"assert_eq!(mapped(Number::ONE).unwrap(), Vec::<blkit_core::temporal::DTDuration>::new());"#,
    );
}

#[test]
fn generated_empty_cartesian_source_uses_output_element_type() {
    let source = "namespace lists; version \"1\"; decision_task mapped { input value: Number; output result: List<Number> = calc; literal_expression calc { output result: List<Number>; expression for x in [1], y in [] return y; } }";
    compile_and_test_graph(
        source,
        r#"assert_eq!(mapped(Number::ONE).unwrap(), Vec::<Number>::new());"#,
    );
}

#[test]
fn generated_nested_for_keeps_nested_lists_without_comma_bindings() {
    let source = "namespace lists; version \"1\"; decision_task nested { input value: List<Number>; output result: List<List<Number>> = calc; literal_expression calc { output result: List<List<Number>>; expression for x in value return (for y in [x, x + 1] return y); } }";
    compile_and_test_graph(
        source,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; assert_eq!(nested(vec![n("1"),n("2")]).unwrap(), vec![vec![n("1"),n("2")],vec![n("2"),n("3")]]);"#,
    );
}

#[test]
fn generated_dictionary_list_projection_checks_missing_keys() {
    let source = "namespace lists; version \"1\"; decision_task projected { input value: List<Dictionary<Number>>; output result: List<Number> = calc; literal_expression calc { output result: List<Number>; expression value.score; } }";
    compile_and_test_graph(
        source,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; let row = std::collections::BTreeMap::from([("score".into(), n("7"))]); assert_eq!(projected(vec![row.clone()]).unwrap(), vec![n("7")]); assert!(projected(vec![std::collections::BTreeMap::new()]).is_err());"#,
    );
}

#[test]
fn generated_some_short_circuits_before_later_predicate_errors() {
    let source = "namespace lists; version \"1\"; decision_task any { input value: List<Number>; output result: Bool = calc; literal_expression calc { output result: Bool; expression some x in value, y in [1, 0] satisfies 1 / y > 0; } }";
    compile_and_test_graph(
        source,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; assert!(!any(vec![]).unwrap()); assert!(any(vec![n("1")]).unwrap());"#,
    );
}

#[test]
fn generated_list_dynamic_positions_and_predicate_errors() {
    let source = "namespace lists; version \"1\"; decision_task at { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression [1,2][value]; } } decision_task filtered { input value: Number; output result: List<Number> = calc; literal_expression calc { output result: List<Number>; expression [1,2][.item / value > 0]; } } decision_task remainder { input value: Number; output result: List<Number> = calc; literal_expression calc { output result: List<Number>; expression [1,2,3][.item mod value == 0]; } }";
    compile_and_test_graph(
        source,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; for bad in ["0", "3", "-3", "1.5"] { assert!(at(n(bad)).is_err(), "{bad}"); } assert_eq!(at(n("-1")).unwrap(), n("2")); assert!(filtered(n("0")).is_err()); assert_eq!(remainder(n("2")).unwrap(), vec![n("2")]); assert!(remainder(n("0")).is_err());"#,
    );
}

#[test]
fn generated_numeric_points_compare_finite_range_endpoints() {
    let mut source = String::from("namespace numbers; version \"1\";\n");
    for (name, expression) in [
        ("point_before", "before(value, (1..5])"),
        ("point_after", "after(value, [1..5])"),
        ("range_before", "before([1..5], value)"),
        ("point_meets", "meets(value, (1..5])"),
        ("range_metby", "metBy([1..5], value)"),
        ("unbounded", "before(value, (null..5])"),
        ("unbounded_other", "metBy((null..5], value)"),
        ("point_before_named", "before(value, [2..5])"),
    ] {
        source.push_str(&format!("decision_task {name} {{ input value: Number; output result: Bool = calc; literal_expression calc {{ output result: Bool; expression {expression}; }} }}\n"));
    }
    compile_and_test_graph(
        &source,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; assert!(point_before(n("0")).unwrap()); assert!(!point_before(n("1")).unwrap()); assert!(point_after(n("6")).unwrap()); assert!(range_before(n("6")).unwrap()); assert!(point_meets(n("1")).unwrap()); assert!(range_metby(n("1")).unwrap()); assert!(!unbounded(n("0")).unwrap()); assert!(!unbounded_other(n("2")).unwrap()); assert!(point_before_named(n("1")).unwrap());"#,
    );
}

#[test]
fn generated_numeric_aggregates_handle_empty_lists_and_sample_stddev() {
    let mut source = String::from("namespace numbers; version \"1\";\n");
    for name in [
        "min", "max", "sum", "mean", "median", "product", "stddev", "mode",
    ] {
        let output = if name == "mode" {
            "List<Number>"
        } else {
            "Number"
        };
        source.push_str(&format!("decision_task {name}_values {{ input values: List<Number>; output result: {output} = calc; literal_expression calc {{ output result: {output}; expression {name}(values); }} }}\n"));
    }
    compile_and_test_graph(
        &source,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; let list = |v: &[&str]| v.iter().map(|s| n(s)).collect(); assert_eq!(min_values(list(&["3","1","2"])).unwrap(), n("1")); assert_eq!(max_values(list(&["3","1","2"])).unwrap(), n("3")); assert_eq!(sum_values(list(&["1","2","3"])).unwrap(), n("6")); assert_eq!(mean_values(list(&["1","2","3"])).unwrap(), n("2")); assert_eq!(median_values(list(&["1","2","3","4"])).unwrap(), n("2.5")); assert_eq!(product_values(list(&["2","3","4"])).unwrap(), n("24")); assert_eq!(product_values(list(&["-0.5","2","3"])).unwrap(), n("-3")); assert_eq!(stddev_values(list(&["1","2","3"])).unwrap(), n("1")); assert_eq!(mode_values(list(&["3","2","3","2"])).unwrap(), list(&["2","3"])); assert_eq!(mode_values(list(&["3","2","3","1"])).unwrap(), list(&["3"])); assert_eq!(mode_values(list(&["6","1","6","1"])).unwrap(), list(&["1","6"])); assert_eq!(sum_values(vec![]).unwrap(), Number::ZERO); assert_eq!(product_values(vec![]).unwrap(), Number::ONE); for result in [min_values(vec![]), max_values(vec![]), mean_values(vec![]), median_values(vec![]), stddev_values(vec![]), stddev_values(vec![n("5")]), sum_values(vec![Number::MAX, Number::ONE]), product_values(vec![Number::MAX, n("2")]), stddev_values(vec![Number::MAX, -Number::MAX])] { assert!(result.is_err()); } assert!(mode_values(vec![]).is_err());"#,
    );
}

#[test]
fn generated_duration_aggregates_keep_kind_and_check_empty_and_precision() {
    let mut source = String::from("namespace durations; version \"1\";\n");
    for (kind, suffix) in [("DTDuration", "dt"), ("YMDuration", "ym")] {
        for name in ["sum", "mean", "median", "min", "max"] {
            source.push_str(&format!("decision_task {name}_{suffix} {{ input values: List<{kind}>; output result: {kind} = calc; literal_expression calc {{ output result: {kind}; expression {name}(values); }} }}\n"));
        }
    }
    compile_and_test_graph(
        &source,
        r#"let dt = |values: &[&str]| values.iter().map(|s| s.parse().unwrap()).collect();
let ym = |values: &[&str]| values.iter().map(|s| s.parse().unwrap()).collect();
assert_eq!(sum_dt(dt(&["PT1H", "PT2H"])).unwrap().to_string(), "PT3H");
assert_eq!(sum_dt(dt(&[])).unwrap().to_string(), "PT0S");
assert_eq!(mean_dt(dt(&["PT1H", "PT2H"])).unwrap().to_string(), "PT1H30M");
assert_eq!(median_dt(dt(&["PT3H", "PT1H", "PT2H", "PT4H"])).unwrap().to_string(), "PT2H30M");
assert_eq!(min_dt(dt(&["PT2H", "PT1H"])).unwrap().to_string(), "PT1H");
assert_eq!(max_dt(dt(&["PT2H", "PT1H"])).unwrap().to_string(), "PT2H");
assert_eq!(sum_ym(ym(&["P1Y", "P6M"])).unwrap().to_string(), "P1Y6M");
assert_eq!(sum_ym(ym(&[])).unwrap().to_string(), "P0M");
assert_eq!(mean_ym(ym(&["P1Y", "P2Y"])).unwrap().to_string(), "P1Y6M");
assert_eq!(median_ym(ym(&["P3Y", "P1Y", "P2Y", "P4Y"])).unwrap().to_string(), "P2Y6M");
assert_eq!(mean_dt(vec![DTDuration(Number::MAX); 2]).unwrap(), DTDuration(Number::MAX));
assert_eq!(median_ym(vec![YMDuration(-Number::MAX), YMDuration(Number::MAX)]).unwrap(), YMDuration(Number::ZERO));
assert_eq!(min_ym(ym(&["P2Y", "P1Y"])).unwrap().to_string(), "P1Y");
assert_eq!(max_ym(ym(&["P2Y", "P1Y"])).unwrap().to_string(), "P2Y");
assert!(mean_dt(dt(&[])).is_err()); assert!(median_ym(ym(&[])).is_err());
assert!(sum_dt(dt(&["PT79228162514264337593543950335S", "PT1S"])).is_err());"#,
    );
}

#[test]
fn generated_comparable_aggregates_order_strings_and_temporal_values() {
    compile_and_test_graph(
        r#"namespace comparable; version "1";
decision_task low_string { input values: List<String>; output result: String = calc; literal_expression calc { output result: String; expression min(values); } }
decision_task high_string { input values: List<String>; output result: String = calc; literal_expression calc { output result: String; expression max(values); } }
decision_task low_date { input values: List<Date>; output result: Date = calc; literal_expression calc { output result: Date; expression min(values); } }
decision_task high_time { input values: List<Time>; output result: Time = calc; literal_expression calc { output result: Time; expression max(values); } }
decision_task low_datetime { input values: List<DateTime>; output result: DateTime = calc; literal_expression calc { output result: DateTime; expression min(values); } }
"#,
        r#"assert_eq!(low_string(vec!["é".into(), "z".into()]).unwrap(), "z");
assert_eq!(high_string(vec!["é".into(), "z".into()]).unwrap(), "é");
assert_eq!(low_date(vec!["2025-02-01".parse().unwrap(), "2025-01-01".parse().unwrap()]).unwrap().to_string(), "2025-01-01");
assert_eq!(high_time(vec!["11:00:00Z".parse().unwrap(), "12:00:00+01:00".parse().unwrap()]).unwrap().to_string(), "11:00:00Z");
assert_eq!(low_datetime(vec!["2025-01-01T09:00:00Z".parse().unwrap(), "2025-01-01T09:30:00+01:00".parse().unwrap()]).unwrap().to_string(), "2025-01-01T09:30:00+01:00");
assert!(low_string(vec![]).is_err()); assert!(high_time(vec![]).is_err());
assert!(high_time(vec!["11:00:00".parse().unwrap(), "12:00:00Z".parse().unwrap()]).is_err());"#,
    );
}

#[test]
fn generated_unordered_modes_keep_first_occurrence_ties_and_unique_winners() {
    compile_and_test_graph(
        r#"namespace modes; version "1";
decision_task strings { input values: List<String>; output result: List<String> = calc; literal_expression calc { output result: List<String>; expression mode(values); } }
decision_task booleans { input values: List<Bool>; output result: List<Bool> = calc; literal_expression calc { output result: List<Bool>; expression mode(values); } }
"#,
        r#"assert_eq!(strings(vec!["z".into(), "a".into(), "z".into()]).unwrap(), vec!["z"]);
assert_eq!(booleans(vec![true, false, true, false]).unwrap(), vec![true, false]);
assert_eq!(booleans(vec![false, true, false]).unwrap(), vec![false]);
assert!(strings(vec![]).is_err());
assert!(booleans(vec![true; 1025]).is_err());"#,
    );
}

#[test]
fn generated_calendar_mode_uses_calendar_equality() {
    compile_and_test_graph(
        r#"namespace modes; version "1";
decision_task calendars { input values: List<Calendar>; output result: List<Calendar> = calc; literal_expression calc { output result: List<Calendar>; expression mode(values); } }
"#,
        r##"let a: Calendar = serde_json::from_str(r#"{"validFrom":"2025-01-01","validTo":"2025-01-31","entries":[]}"#).unwrap();
let b: Calendar = serde_json::from_str(r#"{"validFrom":"2025-02-01","validTo":"2025-02-28","entries":[]}"#).unwrap();
assert_eq!(calendars(vec![a.clone(), b.clone(), a.clone(), b.clone()]).unwrap(), vec![a, b]);"##,
    );
}

#[test]
fn generated_nested_and_dynamic_modes_preserve_first_occurrence_ties() {
    compile_and_test_graph(
        r#"namespace modes; version "1";
decision_task nested { input values: List<List<Number>>; output result: List<List<Number>> = calc; literal_expression calc { output result: List<List<Number>>; expression mode(values); } }
decision_task dynamic { input values: List<Value>; output result: List<Value> = calc; literal_expression calc { output result: List<Value>; expression mode(values); } }
"#,
        r#"assert_eq!(nested(vec![vec![Number::TWO], vec![Number::ONE], vec![Number::TWO], vec![Number::ONE]]).unwrap(), vec![vec![Number::TWO], vec![Number::ONE]]);
assert_eq!(dynamic(vec![serde_json::json!({"x": 1}), serde_json::json!([1]), serde_json::json!({"x": 1}), serde_json::json!([1])]).unwrap(), vec![serde_json::json!({"x": 1}), serde_json::json!([1])]);"#,
    );
}

#[test]
fn generated_aggregates_infer_empty_iteration_bindings_from_results() {
    compile_and_test_graph(
        r#"namespace aggregates; version "1";
decision_task sum_empty { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression sum(for x in [] return x); } }
decision_task mode_empty { input value: Number; output result: List<Number> = calc; literal_expression calc { output result: List<Number>; expression mode(for x in [] return x); } }
"#,
        "assert_eq!(sum_empty(Number::ZERO).unwrap(), Number::ZERO); assert!(mode_empty(Number::ZERO).is_err());",
    );
}

#[test]
fn generated_empty_mode_literal_reports_evaluation_error() {
    compile_and_test_graph(
        r#"namespace modes; version "1";
decision_task empty_mode { input value: Number; output result: List<Bool> = calc; literal_expression calc { output result: List<Bool>; expression mode([]); } }
"#,
        "assert!(empty_mode(Number::ZERO).is_err());",
    );
}

#[test]
fn generated_temporal_modes_compare_instants_and_reject_incompatible_zones() {
    compile_and_test_graph(
        r#"namespace modes; version "1";
decision_task modes { input values: List<DateTime>; output result: List<DateTime> = calc; literal_expression calc { output result: List<DateTime>; expression mode(values); } }
"#,
        r#"let instant = "2025-01-01T10:00:00Z".parse().unwrap(); let equivalent = "2025-01-01T11:00:00+01:00".parse().unwrap(); let other = "2025-01-01T09:00:00Z".parse().unwrap(); assert_eq!(modes(vec![instant, equivalent, other]).unwrap()[0].to_string(), "2025-01-01T10:00:00Z"); assert!(modes(vec![instant, "2025-01-01T10:00:00".parse().unwrap()]).is_err());"#,
    );
}

#[test]
fn generated_number_text_conversion_rejects_invalid_dynamic_inputs() {
    compile_and_test_graph(
        r#"namespace numbers; version "1";
decision_task plain { input text: String; output result: Number = calc; literal_expression calc { output result: Number; expression number(text); } }
decision_task localized { input text: String; output result: Number = calc; literal_expression calc { output result: Number; expression number(text, ".", ","); } }
"#,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; assert_eq!(plain("1500.50".into()).unwrap(), n("1500.5")); assert_eq!(localized("1.500,50".into()).unwrap(), n("1500.5")); for text in ["nope", "", "1,000", "1e3"] { assert!(plain(text.into()).is_err(), "{text}"); } for text in ["12.34,50", "1.00,50", "1.500,5,0"] { assert!(localized(text.into()).is_err(), "{text}"); }"#,
    );
}

#[test]
fn generated_numeric_math_and_predicates_report_domain_errors() {
    let mut source = String::from("namespace numbers; version \"1\";\n");
    for (name, output, expression) in [
        ("absolute", "Number", "abs(value)"),
        ("remainder", "Number", "modulo(value, 3)"),
        ("root", "Number", "sqrt(value)"),
        ("exponential", "Number", "exp(value)"),
        ("natural_log", "Number", "ln(value)"),
        ("base10", "Number", "log(value)"),
        ("base2", "Number", "log(value, 2)"),
        ("bounded", "Number", "clamp(value, 0, 100)"),
        ("bad_bounds", "Number", "clamp(1, value, 0)"),
        ("odd_check", "Bool", "odd(value)"),
        ("even_check", "Bool", "even(value)"),
        ("positive", "Bool", "isPositive(value)"),
        ("negative", "Bool", "isNegative(value)"),
        ("zero", "Bool", "isZero(value)"),
        ("bad_base", "Number", "log(8, value)"),
        ("bad_modulo", "Number", "modulo(1, value)"),
    ] {
        source.push_str(&format!("decision_task {name} {{ input value: Number; output result: {output} = calc; literal_expression calc {{ output result: {output}; expression {expression}; }} }}\n"));
    }
    compile_and_test_graph(
        &source,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; assert_eq!(absolute(n("-10")).unwrap(), n("10")); assert_eq!(remainder(n("-10")).unwrap(), n("2")); assert_eq!(root(n("16")).unwrap(), n("4")); assert!(root(Number::MAX).is_ok()); assert!((exponential(n("1")).unwrap() - n("2.718281828459045235360287471")).abs() < n("0.000000000000000000000001")); assert_eq!(natural_log(n("1")).unwrap(), n("0")); assert_eq!(base10(n("100")).unwrap(), n("2")); assert_eq!(base2(n("8")).unwrap(), n("3")); assert_eq!(bounded(n("150")).unwrap(), n("100")); assert!(bad_bounds(n("5")).is_err()); assert!(bad_modulo(n("0")).is_err()); assert!(root(n("-1")).is_err()); assert!(natural_log(n("0")).is_err()); assert!(bad_base(n("1")).is_err()); assert!(odd_check(n("5")).unwrap()); assert!(even_check(n("2")).unwrap()); assert!(odd_check(n("1.5")).is_err()); assert!(positive(n("5")).unwrap()); assert!(negative(n("-3")).unwrap()); assert!(zero(n("0")).unwrap()); assert!(!positive(n("0")).unwrap());"#,
    );
}

#[test]
fn generated_rounding_uses_decimal_ties_and_validates_dynamic_scale() {
    compile_and_test_graph(
        r#"namespace numbers;
version "1";
decision_task half_up { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression round(value, 2); } }
decision_task away { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression roundUp(value, 0); } }
decision_task toward { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression roundDown(value, 0); } }
decision_task half_down { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression roundHalfDown(value, 0); } }
decision_task half_even { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression roundHalfEven(value, 0); } }
decision_task down { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression floor(value, 1); } }
decision_task up { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression ceiling(value, 1); } }
decision_task tens { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression round(value, -2); } }
decision_task tens_away { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression roundUp(value, -1); } }
decision_task dynamic_scale { input scale: Number; output result: Number = calc; literal_expression calc { output result: Number; expression roundHalfUp(1.25, scale); } }
"#,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; assert_eq!(half_up(n("2.345")).unwrap(), n("2.35")); assert_eq!(away(n("5.1")).unwrap(), n("6")); assert_eq!(toward(n("-5.9")).unwrap(), n("-5")); assert_eq!(half_down(n("5.5")).unwrap(), n("5")); assert_eq!(half_even(n("2.5")).unwrap(), n("2")); assert_eq!(down(n("-1.56")).unwrap(), n("-1.6")); assert_eq!(up(n("-1.56")).unwrap(), n("-1.5")); assert_eq!(tens(n("1250")).unwrap(), n("1300")); assert_eq!(tens_away(n("0.0000000000000000000000000001")).unwrap(), n("10")); assert!(dynamic_scale(n("0.5")).is_err()); assert!(dynamic_scale(n("29")).is_err());"#,
    );
}

#[test]
fn generated_number_arithmetic_is_decimal_and_reports_errors() {
    compile_and_test_graph(
        r#"namespace numbers;
version "1";
decision_task sum { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression value + 0.2; } }
decision_task overflow { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression value + 1; } }
decision_task knowledge_sum { input value: Number; output result: Number = calc; knowledge increment { input item: Number; output result: Number; expression item + 1; } literal_expression calc { output result: Number; expression increment(value); } }
decision_task context_sum { input value: Number; output result: Number = calc; context calc { output result: Number; entry part: Number = value + 1; result part + 1; } }
decision_task table_sum { input value: Number; output result: Number = calc.result; decision_table calc { output result: Number; policy FIRST; input item: Number = value; output result: Number; rule item == item -> item + 1; } }
decision_task divide { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression 10 / value; } }
decision_task power { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression value ** 0.5; } }
decision_task precedence { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression -2 ** 3 ** 2 + value * 2; } }
decision_task scientific { input value: Number; output result: Number = calc; literal_expression calc { output result: Number; expression 1.5e3 + value; } }
decision_task normalized { input value: Number; output result: String = calc; literal_expression calc { output result: String; expression string(1500.50); } }
"#,
        r#"let n = |v: &str| -> Number { v.parse().unwrap() }; assert_eq!(sum(n("0.1")).unwrap(), n("0.3")); assert!(overflow(Number::MAX).is_err()); assert_eq!(knowledge_sum(n("2")).unwrap(), n("3")); assert!(knowledge_sum(Number::MAX).is_err()); assert_eq!(context_sum(n("2")).unwrap(), n("4")); assert!(context_sum(Number::MAX).is_err()); assert_eq!(table_sum(n("2")).unwrap(), n("3")); assert!(table_sum(Number::MAX).is_err()); assert_eq!(divide(n("4")).unwrap(), n("2.5")); assert!(divide(Number::ZERO).is_err()); assert_eq!(power(n("9")).unwrap(), n("3")); assert_eq!(precedence(Number::ZERO).unwrap(), n("-512")); assert_eq!(scientific(Number::ZERO).unwrap(), n("1500")); assert_eq!(normalized(Number::ZERO).unwrap(), "1500.5");"#,
    );
}

#[test]
fn generated_string_functions_evaluate_decision_tasks() {
    compile_and_test_graph(
        r#"namespace strings;
version "1";
decision_task inspect {
  input text: String;
  output result: String = shortened;
  literal_expression shortened { output result: String; expression substring(text, 1, 1); }
}
decision_task join {
  input text: String;
  output result: String = value;
  literal_expression value { output result: String; expression stringJoin([text, upperCase(text)], ", "); }
}
decision_task member {
  input text: String;
  output result: Bool = value;
  literal_expression value { output result: Bool; expression text in ["active", "pending"]; }
}
decision_task convert {
  input value: Number;
  output result: String = text;
  literal_expression text { output result: String; expression "order-" + string(value); }
}
decision_task convert_date { input value: Date; output result: String = text; literal_expression text { output result: String; expression string(value); } }
decision_task convert_time { input value: Time; output result: String = text; literal_expression text { output result: String; expression string(value); } }
decision_task convert_instant { input value: DateTime; output result: String = text; literal_expression text { output result: String; expression string(value); } }
decision_task regex {
  input text: String;
  output result: Bool = value;
  literal_expression value { output result: Bool; expression matches(text, text); }
}
decision_task extracts {
  input text: String;
  output result: List<List<String>> = value;
  literal_expression value { output result: List<List<String>>; expression extract(text, "(a)(b)?"); }
}"#,
        r#"assert_eq!(inspect("e\u{301}x".into()).unwrap(), "e\u{301}"); assert!(inspect("".into()).is_err()); assert_eq!(join("ab".into()).unwrap(), "ab, AB"); assert!(member("active".into()).unwrap()); assert!(!member("ACTIVE".into()).unwrap()); assert_eq!(convert(Number::from(123)).unwrap(), "order-123"); assert_eq!(convert_date("2026-01-01".parse().unwrap()).unwrap(), "2026-01-01"); assert_eq!(convert_time(serde_json::from_value(serde_json::json!("12:30:00")).unwrap()).unwrap(), "12:30:00"); assert_eq!(convert_instant("2026-01-01T00:00:00+02:00".parse().unwrap()).unwrap(), "2026-01-01T00:00:00+02:00"); assert!(regex("[".into()).is_err()); assert_eq!(extracts("ab a".into()).unwrap(), vec![vec!["a", "b"], vec!["a"]]);"#,
    );
}

#[test]
fn generated_string_edges_and_table_propagate_errors() {
    compile_and_test_graph(
        r#"namespace strings;
version "1";
decision_task text {
  input value: String;
  output result: String = transformed;
  literal_expression transformed { output result: String; expression replace(repeat(padTrailing(padLeading(value, 3, "x"), 5), 2), "(a)", "x$1", "i"); }
}
decision_task slice {
  input value: String;
  output result: String = sliced;
  literal_expression sliced { output result: String; expression substring(value, 1, 2); }
}
decision_task trim_case { input value: String; output result: String = text; literal_expression text { output result: String; expression upperCase(trimTrailing(trimLeading(value))); } }
decision_task pieces {
  input value: String;
  output result: List<String> = separated;
  literal_expression separated { output result: List<String>; expression split(value, [",", ";"]); }
}
decision_task table_test {
  input value: String;
  output result: Bool = table.result;
  decision_table table {
    output result: Bool;
    policy FIRST;
    input item: String = value;
    output ok: Bool;
    rule matches(item, value) -> true;
    default false;
  }
}"#,
        r#"assert_eq!(text("a".into()).unwrap(), "xxxa  xxxa  "); assert_eq!(trim_case("  é  ".into()).unwrap(), "É"); assert_eq!(slice("e\u{301}x".into()).unwrap(), "e\u{301}x"); assert!(slice("".into()).is_err()); assert_eq!(pieces("a,b;c".into()).unwrap(), ["a", "b", "c"]); assert!(table_test("abc".into()).unwrap()); assert!(table_test("[".into()).is_err());"#,
    );
}

#[test]
fn generated_financial_differences_and_fiscal_years() {
    compile_and_test(
        r#"namespace finance; version "1";
decision_task days { input a: Date; input b: Date; output result: Number = out; literal_expression out { output result: Number; expression daysBetween(a, b); } }
decision_task precise_days { input a: DateTime; input b: DateTime; output result: Number = out; literal_expression out { output result: Number; expression daysBetween(a, b, true); } }
decision_task months { input a: Date; input b: Date; input basis: String; output result: Number = out; literal_expression out { output result: Number; expression monthsBetween(a, b, basis); } }
decision_task years { input a: Date; input b: Date; input basis: String; output result: Number = out; literal_expression out { output result: Number; expression yearsBetween(a, b, basis); } }
decision_task precise_months { input a: DateTime; input b: DateTime; output result: Number = out; literal_expression out { output result: Number; expression monthsBetween(a, b, "calendar", true); } }
decision_task precise_years { input a: DateTime; input b: DateTime; output result: Number = out; literal_expression out { output result: Number; expression yearsBetween(a, b, true); } }
decision_task precise_isda { input a: DateTime; input b: DateTime; output result: Number = out; literal_expression out { output result: Number; expression yearsBetween(a, b, "actual/actual", true); } }
decision_task fy { input value: Date; input basis: String; output result: String = out; literal_expression out { output result: String; expression financialYear(value, basis); } }
decision_task fyq { input value: Date; input basis: String; output result: String = out; literal_expression out { output result: String; expression financialYearQuarter(value, basis); } }
decision_task fy_month { input value: Date; output result: String = out; literal_expression out { output result: String; expression financialYearQuarter(value, 7); } }"#,
        r#"let d = |text: &str| text.parse::<Date>().unwrap();
let decimal = |text: &str| Number::from_str_exact(text).unwrap();
assert_eq!(days(d("2025-01-01"), d("2025-03-15")).unwrap(), Number::from(73));
assert_eq!(days(d("2025-03-15"), d("2025-01-01")).unwrap(), Number::from(-73));
assert!((days(d("2025-01-01+14:00"), d("2025-01-01-12:00")).unwrap() - decimal("26") / decimal("24")).abs() < decimal("0.00000001"));
assert_eq!(precise_days("2025-01-15T00:00:00".parse().unwrap(), "2025-01-16T12:00:00".parse().unwrap()).unwrap(), decimal("1.5"));
assert!((precise_months("2025-01-01T00:00:00".parse().unwrap(), "2025-01-16T12:00:00".parse().unwrap()).unwrap() - decimal("15.5") / decimal("31")).abs() < decimal("0.00000001"));
assert!((precise_years("2025-01-01T00:00:00".parse().unwrap(), "2025-01-16T12:00:00".parse().unwrap()).unwrap() - decimal("15.5") / decimal("365")).abs() < decimal("0.00000001"));
assert!((months(d("2024-01-10"), d("2025-07-25"), "calendar".into()).unwrap() - decimal("18.4839")).abs() < decimal("0.0001"));
assert!((years(d("2024-01-10"), d("2025-07-25"), "calendar".into()).unwrap() - decimal("1.5370")).abs() < decimal("0.0001"));
assert_eq!(years(d("2024-01-01"), d("2025-01-01"), "actual/actual".into()).unwrap(), Number::ONE);
assert_eq!(years(d("2023-12-31"), d("2024-01-02"), "actual/actual".into()).unwrap(), Number::ONE / decimal("365") + Number::ONE / decimal("366"));
assert_eq!(precise_isda("2023-12-31T12:00:00".parse().unwrap(), "2024-01-01T12:00:00".parse().unwrap()).unwrap(), decimal("0.5") / decimal("365") + decimal("0.5") / decimal("366"));
assert!((precise_isda("2025-01-01T10:00:00+14:00".parse().unwrap(), "2025-01-01T09:00:00-12:00".parse().unwrap()).unwrap() - decimal("25") / (decimal("24") * decimal("365"))).abs() < decimal("0.00000001"));
assert_eq!(years(d("2024-02-28"), d("2024-03-01"), "actual/365".into()).unwrap(), decimal("2") / decimal("365"));
assert_eq!(months(d("2025-02-28"), d("2025-03-31"), "30/360".into()).unwrap(), Number::ONE);
assert!(months(d("2025-02-28"), d("2025-03-31"), "30E/360".into()).unwrap() > Number::ONE);
assert_eq!(years(d("2025-01-01"), d("2026-01-01"), "actual/360".into()).unwrap(), decimal("365") / decimal("360"));
assert!(months(d("2025-01-01"), d("2025-01-02"), "invalid".into()).is_err());
assert_eq!(fy(d("2024-06-30"), "AU".into()).unwrap(), "FY2024");
assert_eq!(fy(d("2024-07-01"), "AU".into()).unwrap(), "FY2025");
assert_eq!(fyq(d("2025-01-15"), "AU".into()).unwrap(), "FY2025Q3");
assert_eq!(fy_month(d("2024-08-01")).unwrap(), "FY2025Q1");
assert_eq!(fyq(d("2025-04-05"), "UK".into()).unwrap(), "FY2025Q4");
assert_eq!(fyq(d("2025-04-06"), "UK".into()).unwrap(), "FY2026Q1");
assert!(fy(d("2025-01-01"), "bad".into()).is_err());"#,
    );
}

#[test]
fn generated_business_dates_handle_holidays_boundaries_and_datetime_fields() {
    compile_and_test(
        r#"namespace business; version "1";
decision_task next_business { input value: Date; input holidays: Calendar; output result: Date = out; literal_expression out { output result: Date; expression nextBusinessDay(value, holidays); } }
decision_task add_business { input value: Date; input holidays: Calendar; output result: Date = out; literal_expression out { output result: Date; expression addBusinessDays(value, 2, holidays); } }
decision_task zero_business { input value: Date; input holidays: Calendar; output result: Date = out; literal_expression out { output result: Date; expression subtractBusinessDays(value, 0, holidays); } }
decision_task strict_business { input value: Date; input holidays: Calendar; output result: Date = out; literal_expression out { output result: Date; expression addBusinessDays(value, 1, holidays, true); } }
decision_task holiday { input value: Date; input holidays: Calendar; output result: Bool = out; literal_expression out { output result: Bool; expression isPublicHoliday(value, holidays) and not isBusinessDay(value, holidays); } }
decision_task last_day { input value: Date; output result: Date = out; literal_expression out { output result: Date; expression lastDayOfMonth(value); } }
decision_task first_weekday { input value: Date; output result: Date = out; literal_expression out { output result: Date; expression firstDayOfWeekInMonth(value, 1); } }
decision_task last_weekday { input value: Date; output result: Date = out; literal_expression out { output result: Date; expression nthDayOfWeekInMonth(value, -1, 1); } }
decision_task next_monday { input value: Date; output result: Date = out; literal_expression out { output result: Date; expression nextDayOfWeek(value, 1); } }
decision_task next_weekday { input value: Date; output result: Date = out; literal_expression out { output result: Date; expression nextWeekday(value); } }
decision_task weekday_count { input a: Date; input b: Date; output result: Number = out; literal_expression out { output result: Number; expression weekdaysBetween(a, b); } }
decision_task business_count { input a: Date; input b: Date; input holidays: Calendar; output result: Number = out; literal_expression out { output result: Number; expression businessDaysBetween(a, b, holidays); } }
decision_task weekdays_datetime { input a: DateTime; input b: DateTime; output result: Number = out; literal_expression out { output result: Number; expression weekdaysBetween(a, b); } }
decision_task datetime_business { input value: DateTime; input holidays: Calendar; output result: DateTime = out; literal_expression out { output result: DateTime; expression nextBusinessDay(value, holidays); } }
decision_task datetime_weekday { input value: DateTime; output result: DateTime = out; literal_expression out { output result: DateTime; expression nextWeekday(value); } }
decision_task prev_business { input value: Date; input holidays: Calendar; output result: Date = out; literal_expression out { output result: Date; expression prevBusinessDay(value, holidays); } }
decision_task subtract_business { input value: Date; input holidays: Calendar; output result: Date = out; literal_expression out { output result: Date; expression subtractBusinessDays(value, 2, holidays); } }
decision_task first_next_month { input value: Date; output result: Date = out; literal_expression out { output result: Date; expression firstDayOfNextMonth(value); } }
decision_task last_prev_month { input value: Date; output result: Date = out; literal_expression out { output result: Date; expression lastDayOfPrevMonth(value); } }
decision_task last_friday { input value: Date; output result: Date = out; literal_expression out { output result: Date; expression lastDayOfWeekInMonth(value, 5); } }
decision_task prev_monday { input value: Date; output result: Date = out; literal_expression out { output result: Date; expression prevDayOfWeek(value, 1); } }
decision_task weekend { input value: Date; output result: Bool = out; literal_expression out { output result: Bool; expression isWeekend(value); } }"#,
        r##"let holidays: Calendar = serde_json::from_str(r#"{"validFrom":"2025-04-01","validTo":"2025-04-30","entries":[{"value":"2025-04-18","name":"Good Friday"},{"value":"2025-04-21","name":"Easter Monday"}]}"#).unwrap();
let d = |text: &str| text.parse::<Date>().unwrap();
assert_eq!(next_business(d("2025-04-17"), holidays.clone()).unwrap().to_string(), "2025-04-22");
assert_eq!(add_business(d("2025-04-17"), holidays.clone()).unwrap().to_string(), "2025-04-23");
assert_eq!(zero_business(d("2025-04-18"), holidays.clone()).unwrap().to_string(), "2025-04-18");
assert!(holiday(d("2025-04-18"), holidays.clone()).unwrap());
assert_eq!(last_day(d("2024-02-10")).unwrap().to_string(), "2024-02-29");
assert_eq!(first_weekday(d("2025-05-10")).unwrap().to_string(), "2025-05-05");
assert_eq!(last_weekday(d("2025-05-10")).unwrap().to_string(), "2025-05-26");
assert_eq!(next_monday(d("2025-04-21")).unwrap().to_string(), "2025-04-28");
assert_eq!(next_weekday(d("2025-05-02")).unwrap().to_string(), "2025-05-05");
assert_eq!(weekday_count(d("2025-04-17"), d("2025-04-23")).unwrap(), Number::from(5));
assert_eq!(business_count(d("2025-04-17"), d("2025-04-23"), holidays.clone()).unwrap(), Number::from(3));
assert!(weekdays_datetime("2025-04-17T10:00:00".parse().unwrap(), "2025-04-18T10:00:00Z".parse().unwrap()).is_err());
assert_eq!(datetime_business("2025-04-17T14:30:00".parse().unwrap(), holidays.clone()).unwrap().to_string(), "2025-04-22T14:30:00");
assert_eq!(datetime_weekday("2025-05-02T14:30:00+01:00".parse().unwrap()).unwrap().to_string(), "2025-05-05T14:30:00+01:00");
assert_eq!(prev_business(d("2025-04-22"), holidays.clone()).unwrap().to_string(), "2025-04-17");
assert_eq!(subtract_business(d("2025-04-23"), holidays.clone()).unwrap().to_string(), "2025-04-17");
assert_eq!(first_next_month(d("2025-01-31")).unwrap().to_string(), "2025-02-01");
assert_eq!(last_prev_month(d("2025-03-15")).unwrap().to_string(), "2025-02-28");
assert_eq!(last_friday(d("2025-05-10")).unwrap().to_string(), "2025-05-30");
assert_eq!(prev_monday(d("2025-04-21")).unwrap().to_string(), "2025-04-14");
assert!(weekend(d("2025-04-19")).unwrap());
assert_eq!(weekday_count(d("2025-04-23"), d("2025-04-17")).unwrap(), Number::from(5));
assert!(strict_business(d("2025-04-30"), holidays.clone()).unwrap_err().contains("bl.CalendarRangeError"));
assert!(next_business(d("2025-04-30"), holidays).is_ok());"##,
    );
}

#[test]
fn generated_calendar_filters_merge_and_named_options_are_checked() {
    compile_and_test(
        r#"namespace schedules; version "1";
decision_task drop_name { input c: Calendar; output result: Calendar = out; literal_expression out { output result: Calendar; expression calendarDrop(c, "Good Friday"); } }
decision_task keep_pattern { input c: Calendar; output result: Calendar = out; literal_expression out { output result: Calendar; expression calendarKeep(c, pattern("^Easter.*")); } }
decision_task keep_list { input c: Calendar; output result: Calendar = out; literal_expression out { output result: Calendar; expression calendarKeep(c, [date("2025-04-18"), date("2025-04-21")]); } }
decision_task drop_overlap { input c: Calendar; output result: Calendar = out; literal_expression out { output result: Calendar; expression calendarDrop(c, [date("2025-04-19")..date("2025-04-22")], rangeMatch="overlap"); } }
decision_task drop_exact_range { input c: Calendar; output result: Calendar = out; literal_expression out { output result: Calendar; expression calendarDrop(c, [date("2025-04-18")..date("2025-04-20")]); } }
decision_task keep_within { input c: Calendar; output result: Calendar = out; literal_expression out { output result: Calendar; expression calendarKeep(c, [date("2025-04-18")..date("2025-04-20")], rangeMatch="entryWithin"); } }
decision_task keep_enclosing { input c: Calendar; output result: Calendar = out; literal_expression out { output result: Calendar; expression calendarKeep(c, [date("2025-04-19")..date("2025-04-19")], rangeMatch="entryEncloses"); } }
decision_task mixed_targets { input c: Calendar; output result: Calendar = out; literal_expression out { output result: Calendar; expression calendarKeep(c, [date("2025-04-18"), pattern("^Easter")]); } }
decision_task nested_heterogeneous_targets { input c: Calendar; output result: Calendar = out; literal_expression out { output result: Calendar; expression calendarKeep(c, ["Good Friday", [date("2025-04-21"), [date("2025-04-18")..date("2025-04-20")], pattern("^Not Found$")]]); } }
decision_task merge { input c: Calendar; output result: Calendar = out; literal_expression out { output result: Calendar; expression calendarMerge([c, c], dedupeBy="value", tiebreak="first"); } }
decision_task merge_named { input a: Calendar; input b: Calendar; output result: Calendar = out; literal_expression out { output result: Calendar; expression calendarMerge([a, b], tiebreak="name", dedupeBy="value"); } }
decision_task merge_by_both { input a: Calendar; input b: Calendar; output result: Calendar = out; literal_expression out { output result: Calendar; expression calendarMerge([a, b], dedupeBy="valueAndName"); } }"#,
        r##"let c: Calendar = serde_json::from_str(r#"{"validFrom":"2025-01-01","validTo":"2025-12-31","entries":[{"value":"2025-04-18","name":"Good Friday"},{"value":{"start":"2025-04-18","end":"2025-04-20","includeStart":true,"includeEnd":true},"name":"Easter break"},{"value":"2025-04-21","name":"Easter Monday"}]}"#).unwrap();
assert_eq!(drop_name(c.clone()).unwrap().entries().len(), 2);
assert_eq!(keep_pattern(c.clone()).unwrap().entries().len(), 2);
assert_eq!(keep_list(c.clone()).unwrap().entries().len(), 2);
assert_eq!(drop_overlap(c.clone()).unwrap().entries().len(), 1);
assert_eq!(drop_exact_range(c.clone()).unwrap().entries().len(), 2);
assert_eq!(keep_within(c.clone()).unwrap().entries().len(), 2);
assert_eq!(keep_enclosing(c.clone()).unwrap().entries().len(), 1);
assert_eq!(mixed_targets(c.clone()).unwrap().entries().len(), 3);
let nested = nested_heterogeneous_targets(c.clone()).unwrap();
assert_eq!(nested.entries().len(), 3);
assert_eq!(nested.entries()[0].name.as_deref(), Some("Good Friday"));
assert_eq!(nested.entries()[1].name.as_deref(), Some("Easter break"));
assert_eq!(nested.entries()[2].name.as_deref(), Some("Easter Monday"));
assert_eq!(merge(c.clone()).unwrap().entries().len(), 3);
let mut changed = serde_json::to_value(&c).unwrap();
changed["entries"][0]["name"] = "A name".into();
let changed: Calendar = serde_json::from_value(changed).unwrap();
assert_eq!(merge_by_both(c.clone(), changed.clone()).unwrap().entries().len(), 4);
let chosen = merge_named(c.clone(), changed).unwrap();
assert_eq!(chosen.entries().len(), 3);
assert_eq!(chosen.entries()[0].name.as_deref(), Some("A name"));
let zoned: Calendar = serde_json::from_str(r#"{"validFrom":"2025-01-01Z","validTo":"2025-12-31Z","entries":[]}"#).unwrap();
assert!(merge_named(c.clone(), zoned).is_err());
assert_eq!(c.entries().len(), 3);"##,
    );
}

#[test]
fn generated_calendar_queries_cover_ranges_bounds_names_and_traversal() {
    compile_and_test(
        r#"namespace schedules; version "1";
decision_task size { input c: Calendar; output result: Number = out; literal_expression out { output result: Number; expression count(c); } }
decision_task empty { input c: Calendar; output result: Bool = out; literal_expression out { output result: Bool; expression isEmpty(c); } }
decision_task labels { input c: Calendar; output result: List<String> = out; literal_expression out { output result: List<String>; expression names(c); } }
decision_task found { input c: Calendar; output result: List<CalendarEntry> = out; literal_expression out { output result: List<CalendarEntry>; expression find(c, "Good Friday"); } }
decision_task values { input c: Calendar; output result: List<CalendarEntry> = out; literal_expression out { output result: List<CalendarEntry>; expression entries(c); } }
decision_task covered { input c: Calendar; output result: Bool = out; literal_expression out { output result: Bool; expression date("2025-04-19") in c; } }
decision_task matching { input c: Calendar; output result: List<CalendarEntry> = out; literal_expression out { output result: List<CalendarEntry>; expression entriesFor(c, date("2025-04-19")); } }
decision_task intersecting { input c: Calendar; output result: Bool = out; literal_expression out { output result: Bool; expression overlaps(c, [date("2025-04-19")..date("2025-04-22")]); } }
decision_task within { input c: Calendar; output result: List<CalendarEntry> = out; literal_expression out { output result: List<CalendarEntry>; expression entriesIn(c, [date("2025-04-19")..date("2025-04-22")]); } }
decision_task next_one { input c: Calendar; output result: CalendarEntry = out; literal_expression out { output result: CalendarEntry; expression next(c, date("2025-04-19")); } }
decision_task next_two { input c: Calendar; output result: CalendarEntry = out; literal_expression out { output result: CalendarEntry; expression next(c, date("2025-04-01"), 2); } }
decision_task previous { input c: Calendar; output result: CalendarEntry = out; literal_expression out { output result: CalendarEntry; expression prev(c, date("2025-04-21")); } }
decision_task invalid_n { input c: Calendar; output result: CalendarEntry = out; literal_expression out { output result: CalendarEntry; expression next(c, date("2025-04-01"), 0); } }
decision_task label { input e: CalendarEntry; output result: String = out; literal_expression out { output result: String; expression entryName(e); } }
decision_task in_bounds { input c: Calendar; output result: Bool = out; literal_expression out { output result: Bool; expression date("2025-04-18") in validRange(c); } }
decision_task start { input c: Calendar; output result: Bool = out; literal_expression out { output result: Bool; expression validFrom(c) == date("2025-01-01") and validTo(c) == date("2025-12-31"); } }
decision_task value { input c: Calendar; output result: Bool = out; literal_expression out { output result: Bool; expression entryValue(next(c, date("2025-04-01"))) == date("2025-04-18"); } }
decision_task range_value { input e: CalendarEntry; output result: Bool = out; literal_expression out { output result: Bool; expression date("2025-04-19") in entryValue(e) and entryValue(e) == [date("2025-04-18")..date("2025-04-20")]; } }"#,
        r##"let text = r#"{"validFrom":"2025-01-01","validTo":"2025-12-31","entries":[{"value":"2025-04-18","name":"Good Friday"},{"value":{"start":"2025-04-18","end":"2025-04-20","includeStart":true,"includeEnd":true},"name":"Weekend"},{"value":"2025-04-21","name":"Easter Monday"},{"value":"2025-05-01","name":"Weekend"}]}"#;
let c: Calendar = serde_json::from_str(text).unwrap();
assert_eq!(size(c.clone()).unwrap(), Number::from(4));
assert!(!empty(c.clone()).unwrap());
assert_eq!(labels(c.clone()).unwrap(), ["Good Friday", "Weekend", "Easter Monday"]);
assert_eq!(found(c.clone()).unwrap().len(), 1);
assert_eq!(values(c.clone()).unwrap().len(), 4);
assert!(covered(c.clone()).unwrap());
assert_eq!(matching(c.clone()).unwrap().len(), 1);
assert!(intersecting(c.clone()).unwrap());
assert_eq!(within(c.clone()).unwrap().len(), 2);
assert_eq!(next_two(c.clone()).unwrap().name.as_deref(), Some("Weekend"));
assert_eq!(next_one(c.clone()).unwrap().name.as_deref(), Some("Easter Monday"));
assert_eq!(previous(c.clone()).unwrap().name.as_deref(), Some("Weekend"));
assert!(invalid_n(c.clone()).is_err());
assert_eq!(label(found(c.clone()).unwrap()[0].clone()).unwrap(), "Good Friday");
assert!(start(c.clone()).unwrap() && in_bounds(c.clone()).unwrap() && value(c.clone()).unwrap());
assert!(range_value(c.entries()[1].clone()).unwrap());
assert!(!range_value(c.entries()[0].clone()).unwrap());
assert!(label(serde_json::from_value(serde_json::json!({"value":"2025-04-18"})).unwrap()).is_err());"##,
    );
}

#[test]
fn generated_calendar_ports_accept_validated_json_and_roundtrip() {
    compile_and_test(
        r#"namespace schedules; version "1";
decision_task echo { input input: Calendar; output result: Calendar = out; literal_expression out { output result: Calendar; expression input; } }
decision_task identical { input input: Calendar; output result: Bool = out; literal_expression out { output result: Bool; expression input == input; } }"#,
        r##"let text = r#"{"validFrom":"2025-01-01","validTo":"2025-12-31","entries":[{"value":"2025-04-18","name":"Good Friday"}]}"#;
        let calendar: Calendar = serde_json::from_str(text).unwrap();
        assert!(identical(calendar.clone()).unwrap());
        assert_eq!(serde_json::to_value(echo(calendar).unwrap()).unwrap(), serde_json::from_str::<serde_json::Value>(text).unwrap());
        assert!(serde_json::from_str::<Calendar>(&text.replace("2025-04-18", "2025-02-30")).is_err());"##,
    );
}

#[test]
fn generated_duration_rounding_and_between_are_signed_and_typed() {
    compile_and_test(
        r#"namespace intervals; version "1";
decision_task half_up { input value: DTDuration; output result: DTDuration = out; literal_expression out { output result: DTDuration; expression roundHalfUp(value, dtDuration("PT15M")); } }
decision_task half_down { input value: DTDuration; output result: DTDuration = out; literal_expression out { output result: DTDuration; expression roundHalfDown(value, dtDuration("PT15M")); } }
decision_task half_even { input value: DTDuration; output result: DTDuration = out; literal_expression out { output result: DTDuration; expression roundHalfEven(value, dtDuration("PT15M")); } }
decision_task variable_step { input step: DTDuration; output result: DTDuration = out; literal_expression out { output result: DTDuration; expression roundHalfUp(dtDuration("PT22M30S"), step); } }
decision_task away { input value: YMDuration; output result: YMDuration = out; literal_expression out { output result: YMDuration; expression roundUp(value, ymDuration("P1Y")); } }
decision_task positive { input value: DTDuration; output result: DTDuration = out; literal_expression out { output result: DTDuration; expression abs(value); } }
decision_task negative { input value: DTDuration; output result: Bool = out; literal_expression out { output result: Bool; expression isNegative(value); } }
decision_task days_between { input from: Date; output result: DTDuration = out; literal_expression out { output result: DTDuration; expression dtDurationBetween(from, date("2025-03-28")); } }
decision_task months_between { input from: Date; output result: YMDuration = out; literal_expression out { output result: YMDuration; expression ymDurationBetween(from, date("2013-08-24")); } }
decision_task months_zoned { input from: DateTime; input to: DateTime; output result: YMDuration = out; literal_expression out { output result: YMDuration; expression ymDurationBetween(from, to); } }"#,
        r#"let tie: DTDuration = "PT22M30S".parse().unwrap();
assert_eq!(half_up(tie).unwrap().to_string(), "PT30M");
assert_eq!(half_down(tie).unwrap().to_string(), "PT15M");
assert_eq!(half_even(tie).unwrap().to_string(), "PT30M");
assert_eq!(half_even("PT7M30S".parse().unwrap()).unwrap().to_string(), "PT0S");
assert_eq!(half_even("-PT22M30S".parse().unwrap()).unwrap().to_string(), "-PT30M");
assert!(variable_step("PT0S".parse().unwrap()).is_err());
assert!(variable_step("-PT15M".parse().unwrap()).is_err());
assert_eq!(away("-P1M".parse().unwrap()).unwrap().to_string(), "-P1Y");
assert_eq!(positive("-PT20S".parse().unwrap()).unwrap().to_string(), "PT20S");
assert!(negative("-PT20S".parse().unwrap()).unwrap());
assert!(!negative("PT0S".parse().unwrap()).unwrap());
assert_eq!(days_between("2025-01-01".parse().unwrap()).unwrap().to_string(), "P86D");
assert_eq!(months_between("2011-12-22".parse().unwrap()).unwrap().to_string(), "P1Y8M");
assert_eq!(months_between("2014-09-24".parse().unwrap()).unwrap().to_string(), "-P1Y1M");
assert_eq!(months_zoned("2025-02-01T00:30:00+14:00".parse().unwrap(), "2025-01-31T23:30:00-12:00".parse().unwrap()).unwrap().to_string(), "P0M");"#,
    );
}

#[test]
fn generated_temporal_zones_and_membership_preserve_instant_and_reject_mixing() {
    compile_and_test(
        r#"namespace zones; version "1";
decision_task reoffset { input value: DateTime; output result: DateTime = out; literal_expression out { output result: DateTime; expression withOffset(value, dtDuration("PT2H")); } }
decision_task rezone { input value: DateTime; output result: DateTime = out; literal_expression out { output result: DateTime; expression withTimezone(value, "Europe/Paris"); } }
decision_task strip { input value: DateTime; output result: DateTime = out; literal_expression out { output result: DateTime; expression withoutOffset(value); } }
decision_task strip_all { input value: Date; output result: Date = out; literal_expression out { output result: Date; expression withoutOffsetOrTimezone(value); } }
decision_task strip_iana { input value: DateTime; output result: DateTime = out; literal_expression out { output result: DateTime; expression withoutTimezone(value); } }
decision_task reoffset_time { input value: Time; output result: Time = out; literal_expression out { output result: Time; expression withOffset(value, dtDuration("PT2H")); } }
decision_task ranged { input value: DateTime; output result: Bool = out; literal_expression out { output result: Bool; expression value between datetime("2025-01-01T00:00:00Z") and datetime("2025-01-03T00:00:00Z"); } }
decision_task listed { input value: DateTime; output result: Bool = out; literal_expression out { output result: Bool; expression value in [datetime("2025-01-01T00:00:00Z")]; } }"#,
        r#"assert_eq!(reoffset("2025-03-28T14:30:00+01:00".parse().unwrap()).unwrap().to_string(), "2025-03-28T15:30:00+02:00");
assert_eq!(rezone("2025-03-28T14:30:00Z".parse().unwrap()).unwrap().to_string(), "2025-03-28T15:30:00[Europe/Paris]");
assert!(rezone("2025-03-28T14:30:00".parse().unwrap()).is_err());
assert_eq!(strip("2025-03-28T14:30:00+01:00".parse().unwrap()).unwrap().to_string(), "2025-03-28T14:30:00");
assert_eq!(strip_all("2025-03-28[Europe/Paris]".parse().unwrap()).unwrap().to_string(), "2025-03-28");
assert_eq!(strip_iana("2025-03-28T14:30:00[Europe/Paris]".parse().unwrap()).unwrap().to_string(), "2025-03-28T14:30:00");
assert_eq!(reoffset_time("14:30:00+01:00".parse().unwrap()).unwrap().to_string(), "15:30:00+02:00");
assert!(reoffset_time("14:30:00".parse().unwrap()).is_err());
assert!(ranged("2025-01-02T00:00:00Z".parse().unwrap()).unwrap());
assert!(ranged("2025-01-02T00:00:00".parse().unwrap()).is_err());
assert!(listed("2025-01-01T00:00:00".parse().unwrap()).is_err());"#,
    );
}

#[test]
fn generated_temporal_point_arithmetic_and_comparison_are_checked() {
    compile_and_test(
        r#"namespace dates; version "1";
decision_task clamp { input value: Date; output result: Date = out; literal_expression out { output result: Date; expression value + ymDuration("P1M"); } }
decision_task wrap { input value: Time; output result: Time = out; literal_expression out { output result: Time; expression value + dtDuration("PT2H"); } }
decision_task midnight_gap { input value: Date; output result: DTDuration = out; literal_expression out { output result: DTDuration; expression value - date("2025-03-28-05:00"); } }
decision_task cross_dst { input value: DateTime; output result: DateTime = out; literal_expression out { output result: DateTime; expression value + dtDuration("PT2H"); } }
decision_task month_datetime { input value: DateTime; output result: DateTime = out; literal_expression out { output result: DateTime; expression value + ymDuration("P1M"); } }
decision_task high_precision { input value: Time; output result: Time = out; literal_expression out { output result: Time; expression value + dtDuration("PT0.1234567891S"); } }
decision_task mixed { input value: DateTime; output result: Bool = out; literal_expression out { output result: Bool; expression value == datetime("2025-01-01T00:00:00Z"); } }
decision_task whole_days { input value: Date; output result: Date = out; literal_expression out { output result: Date; expression value + dtDuration("PT25H"); } }
decision_task fraction_month { input value: Date; output result: Date = out; literal_expression out { output result: Date; expression value + ymDuration("P0.5M"); } }
decision_task skipped_midnight { input value: Date; output result: Date = out; literal_expression out { output result: Date; expression value + dtDuration("P1D"); } }
decision_task high_precision_datetime { input value: DateTime; output result: DateTime = out; literal_expression out { output result: DateTime; expression value + dtDuration("PT0.1234567891S"); } }
decision_task compare_time { input value: Time; output result: Bool = out; literal_expression out { output result: Bool; expression value < time("12:00:00Z"); } }"#,
        r#"assert_eq!(clamp("2025-01-31".parse().unwrap()).unwrap().to_string(), "2025-02-28");
assert_eq!(wrap("23:00:00".parse().unwrap()).unwrap().to_string(), "01:00:00");
assert_eq!(midnight_gap("2025-03-28+05:30".parse().unwrap()).unwrap().to_string(), "-PT10H30M");
assert_eq!(cross_dst("2025-03-30T01:30:00[Europe/Paris]".parse().unwrap()).unwrap().to_string(), "2025-03-30T04:30:00[Europe/Paris]");
assert_eq!(month_datetime("2025-01-31T12:00:00".parse().unwrap()).unwrap().to_string(), "2025-02-28T12:00:00");
assert!(high_precision("12:00:00".parse().unwrap()).is_err());
assert!(mixed("2025-01-01T00:00:00".parse().unwrap()).is_err());
assert_eq!(whole_days("2025-01-31".parse().unwrap()).unwrap().to_string(), "2025-02-01");
assert!(fraction_month("2025-01-31".parse().unwrap()).is_err());
assert!(skipped_midnight("2011-12-29[Pacific/Apia]".parse().unwrap()).is_err());
assert!(high_precision_datetime("2025-01-31T12:00:00".parse().unwrap()).is_err());
assert!(compare_time("12:00:00+01:00".parse().unwrap()).unwrap());
assert!(compare_time("12:00:00".parse().unwrap()).is_err());"#,
    );
}

#[test]
fn generated_temporal_constructors_properties_and_clock_share_one_evaluation() {
    compile_and_test(
        r#"namespace timing; version "1";
decision_task make_date { input value: Number; output result: Date = out; literal_expression out { output result: Date; expression date(2025, 2, value); } }
decision_task dynamic_date { input value: String; output result: Date = out; literal_expression out { output result: Date; expression date(value); } }
decision_task fractional_second { input value: Number; output result: Time = out; literal_expression out { output result: Time; expression time(14, 30, value); } }
decision_task make_time { input offset: DTDuration; output result: Time = out; literal_expression out { output result: Time; expression time(14, 30, 0, offset); } }
decision_task make_datetime { input day: Date; output result: DateTime = out; literal_expression out { output result: DateTime; expression datetime(day, time("14:30:00")); } }
decision_task year_week { input day: Date; output result: String = out; literal_expression out { output result: String; expression day.isoYearWeek; } }
decision_task calendar_props { input day: Date; output result: Bool = out; literal_expression out { output result: Bool; expression day.year == 2025 and day.month == 12 and day.day == 29 and day.dayOfYear == 363 and day.weekOfYear == 52 and day.isoWeekOfYear == 1 and day.quarter == 4 and day.yearQuarter == "2025Q4" and day.dayName == "Monday" and day.dayNameShort == "Mon" and day.monthName == "December" and day.monthNameShort == "Dec"; } }
decision_task clock_props { input moment: DateTime; output result: Bool = out; literal_expression out { output result: Bool; expression moment.hour == 14 and moment.minute == 30 and moment.second == 0.25 and moment.timezone == "Europe/Paris"; } }
decision_task offset { input day: Date; output result: DTDuration = out; literal_expression out { output result: DTDuration; expression day.offset; } }
decision_task stable_now { input value: Bool; output result: Bool = out; literal_expression out { output result: Bool; expression now() == now() and today() == today(); } }
decision_task stable_knowledge { input value: Bool; output result: Bool = out; knowledge read_clock { input item: Bool; output result: Bool; expression now() == now() and today() == today(); } literal_expression out { output result: Bool; expression read_clock(value); } }"#,
        r#"let n = |text: &str| text.parse::<Number>().unwrap();
        assert_eq!(make_date(n("28")).unwrap().to_string(), "2025-02-28");
        assert!(make_date(n("30")).is_err());
        assert_eq!(dynamic_date("2025-02-28".into()).unwrap().to_string(), "2025-02-28");
        assert!(dynamic_date("2025-02-30".into()).is_err());
        assert_eq!(fractional_second(n("0.25")).unwrap().to_string(), "14:30:00.250");
        assert!(fractional_second(n("4294967296")).is_err());
        assert_eq!(make_time("PT2H".parse().unwrap()).unwrap().to_string(), "14:30:00+02:00");
        assert_eq!(make_datetime("2025-03-28".parse().unwrap()).unwrap().to_string(), "2025-03-28T14:30:00");
        assert!(make_datetime("2025-03-28Z".parse().unwrap()).is_err());
        assert_eq!(year_week("2025-12-29".parse().unwrap()).unwrap(), "2026W1");
        assert!(calendar_props("2025-12-29".parse().unwrap()).unwrap());
        assert!(clock_props("2025-02-28T14:30:00.250[Europe/Paris]".parse().unwrap()).unwrap());
        assert_eq!(offset("2025-03-28+05:30".parse().unwrap()).unwrap().to_string(), "PT5H30M");
        assert!(stable_now(true).unwrap());
        assert!(stable_knowledge(true).unwrap());"#,
    );
}

#[test]
fn generated_duration_expressions_use_typed_values_and_checked_decimal_arithmetic() {
    compile_and_test(
        r#"namespace durations; version "1";
decision_task parse_time { input text: String; output result: DTDuration = value; literal_expression value { output result: DTDuration; expression dtDuration(text); } }
decision_task divide_time { input factor: Number; output result: DTDuration = value; literal_expression value { output result: DTDuration; expression dtDuration("PT1H") / factor; } }
decision_task combine_time { input value: DTDuration; output result: Number = total; literal_expression total { output result: Number; expression (value + dtDuration("PT30M")).totalMinutes; } }
decision_task months { input factor: Number; output result: YMDuration = value; literal_expression value { output result: YMDuration; expression ymDuration("P1Y") / factor; } }
decision_task years { input value: YMDuration; output result: Number = total; literal_expression total { output result: Number; expression value.totalYears; } }"#,
        r#"let n = |text: &str| text.parse::<Number>().unwrap();
        assert_eq!(parse_time("p1.5d".into()).unwrap().to_string(), "P1DT12H");
        assert!(parse_time("PT1e2S".into()).is_err());
        assert_eq!(divide_time(n("7")).unwrap().total_seconds(), n("514.28571428571428571428571429"));
        assert!(divide_time(Number::ZERO).is_err());
        assert_eq!(combine_time("PT30M".parse().unwrap()).unwrap(), n("60"));
        assert_eq!(years(months(n("4")).unwrap()).unwrap(), n("0.25"));
        assert_eq!(serde_json::to_value("PT0.1234567891S".parse::<DTDuration>().unwrap()).unwrap(), serde_json::json!("PT0.1234567891S"));"#,
    );
}

#[test]
fn generated_temporal_json_preserves_naive_offsets_and_iana_zones() {
    compile_and_test(
        r#"namespace timing; version "1";
decision_task preserve_date { input value: Date; output result: Date = copy; literal_expression copy { output result: Date; expression value; } }
decision_task preserve_time { input value: Time; output result: Time = copy; literal_expression copy { output result: Time; expression value; } }
decision_task preserve_datetime { input value: DateTime; output result: DateTime = copy; literal_expression copy { output result: DateTime; expression value; } }"#,
        r#"for (day, time, instant) in [
            ("2026-10-02", "24:00:00", "2026-10-02T09:30:00"),
            ("2026-10-02+05:30", "09:30:00+02:00", "2026-10-02T09:30:00+02:00"),
            ("2026-10-02[Europe/Paris]", "09:30:00[Europe/Paris]", "2026-10-02T09:30:00[Europe/Paris]"),
        ] {
            let date: Date = serde_json::from_value(serde_json::json!(day)).unwrap();
            let time_value: Time = serde_json::from_value(serde_json::json!(time)).unwrap();
            let datetime: DateTime = serde_json::from_value(serde_json::json!(instant)).unwrap();
            assert_eq!(serde_json::to_value(preserve_date(date).unwrap()).unwrap(), serde_json::json!(day));
            assert_eq!(serde_json::to_value(preserve_time(time_value).unwrap()).unwrap(), serde_json::json!(if time == "24:00:00" { "00:00:00" } else { time }));
            assert_eq!(serde_json::to_value(preserve_datetime(datetime).unwrap()).unwrap(), serde_json::json!(instant));
        }"#,
    );
}

#[test]
fn generated_temporal_values_roundtrip_and_order() {
    compile_and_test(
        r#"namespace timing;
version "1";
Clock = {day: Date, time: Time, instant: DateTime};
decision_task later {
  input value: Clock;
  output result: Bool = checked;
  literal_expression checked { output result: Bool; expression value.day < date("2026-10-03") and value.time < time("12:00:00") and value.instant == datetime("2026-10-02T08:30:00Z"); }
}
decision_task before {
  input opens: DateTime;
  input closes: DateTime;
  output result: Bool = checked;
  literal_expression checked { output result: Bool; expression opens < closes; }
}"#,
        r#"let day: Date = serde_json::from_value(serde_json::json!("2026-10-02")).unwrap(); let time: Time = serde_json::from_value(serde_json::json!("09:30:00.250")).unwrap(); let instant: DateTime = serde_json::from_value(serde_json::json!("2026-10-02T09:30:00+01:00")).unwrap(); assert!(later(Clock { day, time, instant }).unwrap()); assert_eq!(serde_json::to_value(day).unwrap(), serde_json::json!("2026-10-02")); assert_eq!(serde_json::to_value(time).unwrap(), serde_json::json!("09:30:00.250")); assert!(before("2026-10-02T09:30:00+02:00".parse().unwrap(), "2026-10-02T08:31:00+01:00".parse().unwrap()).unwrap()); for invalid in ["2026-02-30", "not-a-date", "2026-1-2"] { assert!(serde_json::from_value::<Date>(serde_json::json!(invalid)).is_err()); } for invalid in ["24:00:01", "09:30:00+99:00", "23:59:60", "9:30:00"] { assert!(serde_json::from_value::<Time>(serde_json::json!(invalid)).is_err(), "{invalid}"); } assert!(serde_json::from_value::<DateTime>(serde_json::json!("2025-03-30T02:30:00[Europe/Paris]")).is_err());"#,
    );
}

#[test]
fn generated_ranges_obey_boundaries_and_dynamic_empty_semantics() {
    compile_and_test(
        r#"namespace ranges;
version "1";
Bounds = {low: Number, high: Number};
decision_task inclusive { input value: Number; output result: Bool = check; literal_expression check { output result: Bool; expression value in [1..5]; } }
decision_task exclusive { input value: Number; output result: Bool = check; literal_expression check { output result: Bool; expression value in (1..5); } }
decision_task upper_open { input value: Number; output result: Bool = check; literal_expression check { output result: Bool; expression value in [1..null); } }
decision_task dynamic { input bounds: Bounds; output result: Bool = check; literal_expression check { output result: Bool; expression bounds.low in [bounds.low..bounds.high]; } }
decision_task relations { input value: Number; output result: Bool = check; literal_expression check { output result: Bool; expression before([1..2], [3..4]) and meets([1..2], [2..3]) and overlaps([5..10], [1..6]) and includes([1..10], 5) and not (starts(1, (1..5])) and overlaps((null..5), [4..null)) and not (overlaps([value..1], [1..2])); } }
decision_task instant { input value: DateTime; output result: Bool = check; literal_expression check { output result: Bool; expression value in [datetime("2026-10-02T09:00:00+01:00")..null); } }
decision_task adjacent { input value: Date; output result: Bool = check; literal_expression check { output result: Bool; expression not (overlaps((date("2026-01-01")..date("2026-01-03")), [date("2026-01-03")..null))) and not (overlaps((date("2026-01-01")..date("2026-01-02")), [date("2026-01-01")..date("2026-01-02")])); } }"#,
        r#"let n = |text: &str| text.parse::<Number>().unwrap(); assert!(inclusive(n("1")).unwrap()); assert!(inclusive(n("5")).unwrap()); assert!(!exclusive(n("1")).unwrap()); assert!(exclusive(n("3")).unwrap()); assert!(!exclusive(n("5")).unwrap()); assert!(upper_open(n("100")).unwrap()); assert!(!dynamic(Bounds { low: n("5"), high: n("1") }).unwrap()); assert!(relations(n("5")).unwrap()); assert!(instant("2026-10-02T08:30:00Z".parse().unwrap()).unwrap()); assert!(adjacent("2026-01-01".parse().unwrap()).unwrap());"#,
    );
}

// Braced table policy, multi-column, context, knowledge, and branch tests below
// cover the former duplicate legacy decision fixtures without nested recompiles.
#[test]
fn knowledge_names_can_coexist_with_range_builtins_and_date_parameters() {
    compile_and_test(
        r#"namespace compatibility;
version "1";
decision_task earlier {
  input value: Number;
  output result: Bool = check;
  knowledge before { input a: Number; input b: Number; output result: Bool; expression a < b; }
  literal_expression check { output result: Bool; expression before(value, 10); }
}
decision_task daylight {
  input value: Number;
  output result: Bool = check;
  knowledge date_check { input day: Date; output result: Bool; expression day in [day..day]; }
  knowledge datetime_check { input at: DateTime; output result: Bool; expression at == at; }
  literal_expression check { output result: Bool; expression value == value; }
}
decision_task intervals { input value: Number; output result: Bool = check; literal_expression check { output result: Bool; expression before([1..2], [3..4]); } }"#,
        "assert!(earlier(Number::ONE).unwrap()); assert!(!earlier(Number::from(11)).unwrap()); assert!(daylight(Number::ZERO).unwrap()); assert!(intervals(Number::ZERO).unwrap());",
    );
}

#[test]
fn table_outputs_accept_dictionary_literals_with_commas() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task table_value { input flag: Bool; output result: Dictionary<Number> = table; decision_table table { output result: Dictionary<Number>; policy FIRST; input value: Bool = flag; output item: Dictionary<Number>; rule value == true -> {a: 1, b: 2}; default {a: 0, b: 0}; } }"#,
        r#"assert_eq!(table_value(true).unwrap()["b"], Number::from(2));"#,
    );
}

#[test]
fn nested_named_dictionary_literals_are_emitted_in_typed_contexts() {
    compile_and_test(
        r#"namespace orders; version "1";
Order = {total: Number, blocked: Bool};
decision_task many { input amount: Number; output result: List<Order> = value; literal_expression value { output result: List<Order>; expression [{total: amount, blocked: false}]; } }
decision_task calculated { input amount: Number; output result: Order = value; knowledge make { input n: Number; output result: Order; expression {total: n, blocked: false}; } literal_expression value { output result: Order; expression make(amount); } }"#,
        r#"assert_eq!(many(Number::from(2)).unwrap()[0].total, Number::from(2)); assert_eq!(calculated(Number::from(3)).unwrap().total, Number::from(3));"#,
    );
}

#[test]
fn dynamic_ports_reject_json_null_values() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task echo { input payload: Dictionary; output result: Dictionary = value; literal_expression value { output result: Dictionary; expression payload; } }
decision_task raw { input payload: Value; output result: Value = value; literal_expression value { output result: Value; expression payload; } }"#,
        r#"let bad = serde_json::from_value(serde_json::json!({"nested":[1,null]})).unwrap(); assert!(echo(bad).is_err()); assert!(raw(serde_json::Value::Null).is_err()); let good = serde_json::from_value(serde_json::json!({"nested":[1,2]})).unwrap(); assert!(echo(good).is_ok());"#,
    );
}

#[test]
fn generated_dictionary_bindings_cannot_shadow_source_keys() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task collision { input input: Number; output result: Dictionary<Number> = value; literal_expression value { output result: Dictionary<Number>; expression {"my key": 1, __bl_literal_0: 2}; } }"#,
        r#"let result = collision(Number::ZERO).unwrap(); assert_eq!(result["my key"], Number::ONE); assert_eq!(result["__bl_literal_0"], Number::from(2));"#,
    );
}

#[test]
fn named_dictionary_fields_cannot_collide_after_lowering() {
    compile_and_test(
        r#"namespace orders; version "1";
Order = {"my key": Number, __bl_field_0: Bool};
decision_task projection { input order: Order; output result: Number = value; literal_expression value { output result: Number; expression order["my key"]; } }"#,
        r#"let order: Order = serde_json::from_value(serde_json::json!({"my key":"3","__bl_field_0":false})).unwrap(); assert_eq!(projection(order).unwrap(), Number::from(3));"#,
    );
}

#[test]
fn typed_nested_collections_keep_numeric_json_at_dynamic_boundary() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task wrap { input scores: Dictionary<Dictionary<Number>>; input amounts: List<Number>; output result: Dictionary = value; literal_expression value { output result: Dictionary; expression {scores: scores, amounts: amounts, label: "ok"}; } }"#,
        r#"let scores: std::collections::BTreeMap<String, std::collections::BTreeMap<String, Number>> = [("alice".into(), [("sum".into(), Number::from(3))].into())].into(); assert_eq!(serde_json::to_value(wrap(scores, vec![Number::ONE, Number::from(2)]).unwrap()).unwrap(), serde_json::json!({"scores":{"alice":{"sum":3}},"amounts":[1,2],"label":"ok"}));"#,
    );
}

#[test]
fn dependent_dynamic_dictionary_numbers_remain_numbers() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task dependent { input input: Number; output result: Dictionary = value; literal_expression value { output result: Dictionary; expression {count: 2, increased: count + 1, done: true}; } }"#,
        r#"assert_eq!(serde_json::to_value(dependent(Number::ZERO).unwrap()).unwrap(), serde_json::json!({"count":2,"increased":3,"done":true}));"#,
    );
}

#[test]
fn nested_numbers_keep_json_kind_in_dynamic_dictionary() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task mixed { input input: Number; output result: Dictionary = value; literal_expression value { output result: Dictionary; expression {label: "x", child: {amount: 3}, amounts: [1, 2]}; } }"#,
        r#"assert_eq!(serde_json::to_value(mixed(Number::ZERO).unwrap()).unwrap(), serde_json::json!({"label":"x","child":{"amount":3},"amounts":[1,2]}));"#,
    );
}

#[test]
fn variable_dictionary_paths_are_typed_from_the_port() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task pick { input source: Dictionary<Number>; input path: List<String>; output result: Number = value; literal_expression value { output result: Number; expression getValue(source, path); } }
decision_task replace { input source: Dictionary; input path: List<String>; output result: Dictionary = value; literal_expression value { output result: Dictionary; expression dictionaryPut(source, path, 2); } }"#,
        r#"let source: std::collections::BTreeMap<String, Number> = [("a".into(), Number::ONE)].into(); assert_eq!(pick(source, vec!["a".into()]).unwrap(), Number::ONE); let dynamic: std::collections::BTreeMap<String, serde_json::Value> = serde_json::from_value(serde_json::json!({"a":{"b":1}})).unwrap(); assert_eq!(replace(dynamic, vec!["a".into(), "b".into()]).unwrap()["a"]["b"], serde_json::json!(2));"#,
    );
}

#[test]
fn documented_dictionary_literal_examples_run() {
    compile_and_test(
        r#"namespace examples; version "1";
decision_task empty { input input: Number; output result: Dictionary = value; literal_expression value { output result: Dictionary; expression {}; } }
decision_task special { input input: Number; output result: Dictionary<Number> = value; literal_expression value { output result: Dictionary<Number>; expression {"my key": 1}; } }
decision_task nested { input input: Number; output result: Number = value; literal_expression value { output result: Number; expression {a: {b: 3}}.a.b; } }
decision_task fetched { input input: Number; output result: Number = value; literal_expression value { output result: Number; expression getValue({foo: 123}, "foo"); } }
decision_task entries { input input: Number; output result: List<DictionaryEntry<Number>> = value; literal_expression value { output result: List<DictionaryEntry<Number>>; expression getEntries({foo: 123}); } }
decision_task updated { input input: Number; output result: Dictionary = value; literal_expression value { output result: Dictionary; expression dictionaryPut({x: 1, y: {z: 0}}, ["y", "z"], 2); } }
decision_task merged { input input: Number; output result: Dictionary = value; literal_expression value { output result: Dictionary; expression dictionaryMerge([{x: 1}, {y: 2}]); } }
decision_task removed { input input: Number; output result: Dictionary = value; literal_expression value { output result: Dictionary; expression dictionaryRemove({a: 1, b: 2}, "a"); } }"#,
        r#"assert!(empty(Number::ZERO).unwrap().is_empty()); assert_eq!(special(Number::ZERO).unwrap()["my key"], Number::ONE); assert_eq!(nested(Number::ZERO).unwrap(), Number::from(3)); assert_eq!(fetched(Number::ZERO).unwrap(), Number::from(123)); let items = entries(Number::ZERO).unwrap(); assert_eq!(items[0].key, "foo"); assert_eq!(items[0].value, Number::from(123)); assert_eq!(updated(Number::ZERO).unwrap()["y"]["z"], serde_json::json!(2)); assert_eq!(merged(Number::ZERO).unwrap()["y"], serde_json::json!(2)); let result = removed(Number::ZERO).unwrap(); assert!(!result.contains_key("a")); assert_eq!(result["b"], serde_json::json!(2));"#,
    );
}

#[test]
fn dynamic_values_require_runtime_checks_for_typed_outputs() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task amount { input payload: Dictionary; output result: Number = value; literal_expression value { output result: Number; expression payload.amount; } }
decision_task increment { input payload: Dictionary; output result: Number = value; literal_expression value { output result: Number; expression payload.amount + 1; } }"#,
        r#"let valid: std::collections::BTreeMap<String, serde_json::Value> = serde_json::from_value(serde_json::json!({"amount":12.5})).unwrap(); assert_eq!(amount(valid.clone()).unwrap(), Number::from_str_exact("12.5").unwrap()); assert_eq!(increment(valid).unwrap(), Number::from_str_exact("13.5").unwrap()); let wrong = serde_json::from_value(serde_json::json!({"amount":"twelve"})).unwrap(); assert!(amount(wrong).is_err());"#,
    );
}

#[test]
fn iteration_body_errors_propagate_and_shadow_outer_names() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task parse_all { input texts: List<String>; output result: List<Number> = value; literal_expression value { output result: List<Number>; expression for texts in texts return number(texts); } }
decision_task check_all { input scores: Dictionary<Number>; output result: Bool = value; literal_expression value { output result: Bool; expression every scores in values(scores) satisfies scores >= 0; } }"#,
        r#"assert_eq!(parse_all(vec!["12".into(), "3".into()]).unwrap(), vec![Number::from(12), Number::from(3)]); assert!(parse_all(vec!["12".into(), "bad".into()]).is_err()); assert!(check_all([("a".into(), Number::from(1))].into()).unwrap());"#,
    );
}

#[test]
fn dictionary_empty_size_and_presence_overloads_work() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task empty { input input: Dictionary; output result: Bool = value; literal_expression value { output result: Bool; expression isEmpty(input); } }
decision_task count { input input: Dictionary; output result: Number = value; literal_expression value { output result: Number; expression size(input); } }
decision_task present { input input: Dictionary; output result: Bool = value; literal_expression value { output result: Bool; expression has(input, "a"); } }"#,
        r#"let blank = Default::default(); assert!(empty(blank).unwrap()); let values: std::collections::BTreeMap<String, serde_json::Value> = serde_json::from_value(serde_json::json!({"a":1,"b":false})).unwrap(); assert_eq!(count(values.clone()).unwrap(), Number::from(2)); assert!(present(values.clone()).unwrap()); assert!(!empty(values).unwrap());"#,
    );
}

#[test]
fn nested_dynamic_dictionary_fields_fail_when_not_objects() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task nested { input payload: Dictionary; output result: Value = value; literal_expression value { output result: Value; expression payload.address.postcode; } }"#,
        r#"let good = serde_json::from_value(serde_json::json!({"address":{"postcode":"SW1"}})).unwrap(); assert_eq!(nested(good).unwrap(), serde_json::json!("SW1")); let bad = serde_json::from_value(serde_json::json!({"address":12})).unwrap(); assert!(nested(bad).is_err());"#,
    );
}

#[test]
fn named_dictionary_supports_runtime_bracket_keys() {
    compile_and_test(
        r#"namespace orders; version "1";
Applicant = {"my key": Number, name: String};
decision_task lookup { input applicant: Applicant; input key: String; output result: Value = value; literal_expression value { output result: Value; expression applicant[key]; } }"#,
        r#"let applicant: Applicant = serde_json::from_value(serde_json::json!({"my key":"12","name":"Ada"})).unwrap(); assert_eq!(lookup(applicant.clone(), "my key".into()).unwrap(), serde_json::json!(12)); assert_eq!(lookup(applicant.clone(), "name".into()).unwrap(), serde_json::json!("Ada")); assert!(lookup(applicant, "missing".into()).is_err());"#,
    );
}

#[test]
fn dynamic_dictionary_json_retains_numeric_precision_and_nested_values() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task echo { input payload: Dictionary; output result: Dictionary = value; literal_expression value { output result: Dictionary; expression payload; } }"#,
        r#"let number: serde_json::Value = serde_json::from_str("123.4567890123456789012345678").unwrap(); let payload = serde_json::json!({"number":number,"nested":{"names":["A","B"]}}); let dictionary = serde_json::from_value(payload.clone()).unwrap(); let result = echo(dictionary).unwrap(); assert_eq!(serde_json::to_value(result).unwrap(), payload);"#,
    );
}

#[test]
fn dictionary_literals_evaluate_in_order_and_compare_structurally() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task computed { input input: Number; output result: Number = value; literal_expression value { output result: Number; expression {a: 2, b: a * 2}.b; } }
decision_task equality { input input: Number; output result: Bool = value; literal_expression value { output result: Bool; expression {a: 1, b: 2} == {b: 2, a: 1}; } }"#,
        r#"assert_eq!(computed(Number::ZERO).unwrap(), Number::from(4)); assert!(equality(Number::ZERO).unwrap());"#,
    );
}

#[test]
fn empty_for_iteration_uses_contextual_result_type() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task empty { input input: Number; output result: List<Number> = value; literal_expression value { output result: List<Number>; expression for v in [] return 1; } }"#,
        r#"assert_eq!(empty(Number::ZERO).unwrap(), Vec::<Number>::new());"#,
    );
}

#[test]
fn dictionary_entry_iteration_is_sorted_and_scoped() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task passes { input scores: Dictionary<Number>; output result: Bool = value; literal_expression value { output result: Bool; expression every v in values(scores) satisfies v >= 50; } }
decision_task flags { input scores: Dictionary<Number>; output result: List<Bool> = value; literal_expression value { output result: List<Bool>; expression for e in getEntries(scores) return e.value > 80; } }"#,
        r#"let scores: std::collections::BTreeMap<String, Number> = [("bob".into(), Number::from(75)), ("alice".into(), Number::from(90))].into(); assert!(passes(scores.clone()).unwrap()); assert_eq!(flags(scores).unwrap(), vec![true,false]); assert!(passes(Default::default()).unwrap()); assert!(flags(Default::default()).unwrap().is_empty());"#,
    );
}

#[test]
fn dictionary_transformations_preserve_inputs_and_validate_paths() {
    compile_and_test(
        r#"namespace orders; version "1";
Order = {total: Number, blocked: Bool};
decision_task put { input source: Dictionary<Number>; output result: Dictionary = value; literal_expression value { output result: Dictionary; expression dictionaryPut(source, "y", 2); } }
decision_task nested { input source: Dictionary; output result: Dictionary = value; literal_expression value { output result: Dictionary; expression dictionaryPut(source, ["y", "z"], 2); } }
decision_task merge { input source: Dictionary; output result: Dictionary = value; literal_expression value { output result: Dictionary; expression dictionaryMerge([source, {y: 2}]); } }
decision_task remove { input source: Dictionary; output result: Dictionary = value; literal_expression value { output result: Dictionary; expression dictionaryRemove(source, "x"); } }
decision_task extend { input source: Order; output result: Dictionary = value; literal_expression value { output result: Dictionary; expression dictionaryPut(source, "note", "ok"); } }
decision_task empty_path { input source: Dictionary; output result: Value = value; literal_expression value { output result: Value; expression getValue(source, []); } }"#,
        r#"let before = [("x".into(), Number::from(1))].into(); let result = put(before).unwrap(); assert_eq!(result.get("x").unwrap(), &serde_json::json!(1)); assert_eq!(result.get("y").unwrap(), &serde_json::json!(2)); let source: std::collections::BTreeMap<String, serde_json::Value> = serde_json::from_value(serde_json::json!({"y":{"z":0},"x":1})).unwrap(); let changed = nested(source.clone()).unwrap(); assert_eq!(changed["y"]["z"], serde_json::json!(2)); assert_eq!(source["y"]["z"], serde_json::json!(0)); let merged = merge(source.clone()).unwrap(); assert_eq!(merged["y"], serde_json::json!(2)); let removed = remove(source).unwrap(); assert!(!removed.contains_key("x")); assert!(nested(serde_json::from_value(serde_json::json!({"y":1})).unwrap()).is_err()); let order = Order { total: Number::from(3), blocked: false }; let after = extend(order.clone()).unwrap(); assert_eq!(after["total"], serde_json::json!(3)); assert_eq!(after["note"], serde_json::json!("ok")); assert_eq!(order.total, Number::from(3)); let input = serde_json::from_value(serde_json::json!({"x":1})).unwrap(); assert!(empty_path(input).is_err());"#,
    );
}

#[test]
fn dictionary_values_entries_and_named_lookup_work() {
    compile_and_test(
        r#"namespace orders; version "1";
Order = {total: Number, blocked: Bool};
decision_task totals { input scores: Dictionary<Number>; output result: Number = value; literal_expression value { output result: Number; expression sum(values(scores)); } }
decision_task entries { input scores: Dictionary<Number>; output result: List<DictionaryEntry<Number>> = value; literal_expression value { output result: List<DictionaryEntry<Number>>; expression getEntries(scores); } }
decision_task selected { input order: Order; output result: Number = value; literal_expression value { output result: Number; expression getValue(order, "total"); } }
decision_task contained { input order: Order; output result: Bool = value; literal_expression value { output result: Bool; expression has(order, "blocked"); } }
decision_task named_values { input order: Order; output result: List<Value> = value; literal_expression value { output result: List<Value>; expression values(order); } }
decision_task dynamic_lookup { input order: Order; input key: String; output result: Value = value; literal_expression value { output result: Value; expression getValue(order, key); } }
decision_task path_lookup { input order: Order; input path: List<String>; output result: Value = value; literal_expression value { output result: Value; expression getValue(order, path); } }"#,
        r#"let scores: std::collections::BTreeMap<String, Number> = [("bob".into(), Number::from(75)), ("alice".into(), Number::from(90))].into(); assert_eq!(totals(scores.clone()).unwrap(), Number::from(165)); let result = entries(scores).unwrap(); assert_eq!(result[0].key, "alice"); assert_eq!(result[0].value, Number::from(90)); let order = Order { total: Number::from(3), blocked: false }; assert_eq!(selected(order.clone()).unwrap(), Number::from(3)); assert!(contained(order.clone()).unwrap()); assert_eq!(named_values(order.clone()).unwrap(), vec![serde_json::json!(false), serde_json::json!(3)]); assert_eq!(dynamic_lookup(order.clone(), "total".into()).unwrap(), serde_json::json!(3)); assert_eq!(path_lookup(order, vec!["total".into()]).unwrap(), serde_json::json!(3));"#,
    );
}

#[test]
fn dictionary_queries_sort_keys_and_report_missing_paths() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task sorted { input input: Number; output result: List<String> = value; literal_expression value { output result: List<String>; expression keys({b: 2, a: 1}); } }
decision_task picked { input input: Number; output result: Number = value; literal_expression value { output result: Number; expression getValue({x: 1, y: {z: 0}}, ["y", "z"]); } }
decision_task absent { input input: Number; output result: Number = value; literal_expression value { output result: Number; expression getValue({a: 1}, "missing"); } }"#,
        r#"assert_eq!(sorted(Number::ZERO).unwrap(), vec!["a", "b"]); assert_eq!(picked(Number::ZERO).unwrap(), Number::ZERO); assert!(absent(Number::ZERO).is_err());"#,
    );
}

#[test]
fn named_dictionary_quoted_keys_roundtrip_and_access() {
    compile_and_test(
        r#"namespace orders; version "1";
Applicant = {"my key": Number};
decision_task selected { input applicant: Applicant; output result: Number = value; literal_expression value { output result: Number; expression applicant["my key"]; } }"#,
        r#"let person: Applicant = serde_json::from_value(serde_json::json!({"my key":"12"})).unwrap(); assert_eq!(selected(person).unwrap(), Number::from(12));"#,
    );
}

#[test]
fn literal_dictionaries_support_mixed_decimal_and_named_output() {
    compile_and_test(
        r#"namespace orders; version "1";
Order = {total: Number, blocked: Bool};
decision_task named { input input: Number; output result: Order = value; literal_expression value { output result: Order; expression {total: input, blocked: false}; } }
decision_task mixed { input input: Number; output result: Dictionary = value; literal_expression value { output result: Dictionary; expression {total: input, name: "Ada"}; } }"#,
        r#"let order = named(Number::from(2)).unwrap(); assert_eq!(serde_json::to_value(order).unwrap(), serde_json::json!({"total":"2","blocked":false})); let dictionary = mixed(Number::from_str_exact("123.4567890123456789012345678").unwrap()).unwrap(); assert_eq!(dictionary.get("total").unwrap().to_string(), "123.4567890123456789012345678"); assert_eq!(dictionary.get("name").unwrap(), "Ada");"#,
    );
}

#[test]
fn dictionary_ports_preserve_decimal_values_and_named_shapes() {
    compile_and_test(
        r#"namespace orders; version "1";
Order = {total: Number, blocked: Bool};
decision_task amount { input order: Order; output result: Number = value; literal_expression value { output result: Number; expression order.total; } }
decision_task scores { input input: Dictionary<Number>; output result: Number = value; literal_expression value { output result: Number; expression input.alice; } }"#,
        r#"let order: Order = serde_json::from_value(serde_json::json!({"total":"123.456","blocked":false})).unwrap(); assert_eq!(amount(order).unwrap().to_string(), "123.456"); assert!(serde_json::from_value::<Order>(serde_json::json!({"total":"123.456","blocked":false,"extra":true})).is_err()); let points = serde_json::from_value(serde_json::json!({"alice":"90.01"})).unwrap(); assert_eq!(scores(points).unwrap().to_string(), "90.01");"#,
    );
}

#[test]
fn generated_records_enums_and_decimal_literals_compile() {
    compile_and_test(
        r#"namespace orders;
version "1.0";
Order = {total: Number, tags: List<String>};
enum Decision:
  approved;
  review;
decision_task amount { input order: Order; output result: Number = value; literal_expression value { output result: Number; expression 12.50; } }"#,
        "let order = Order { total: Number::ONE, tags: vec![] }; assert_eq!(amount(order).unwrap(), \"12.50\".parse::<Number>().unwrap());",
    );
}

#[test]
fn generated_subprocess_calls_registered_typed_child() {
    compile_and_test_graph(
        r#"namespace orders;
version "1";
start_event start { output input: Bool; }
end_event done { input result: Number; }
decision_task four { input input: Bool; output result: Number = value; literal_expression value { output result: Number; expression 4; } }
subprocess called { process child; input input: Bool; output result: Number; }
process child { flow start -> four; flow four -> done; bind start.input -> four.input; bind four.result -> done.result; }
process parent { flow start -> called; flow called -> done; bind start.input -> called.input; bind called.result -> done.result; }"#,
        "let mut definitions = named_graph_definitions(); assert_eq!(definitions.len(), 2); let parent = definitions.remove(1); assert!(blkit::runtime::Registry::new(vec![parent]).is_err()); let registry = blkit::runtime::Registry::new(named_graph_definitions()).unwrap(); let store = blkit::runtime::LocalStore::open(&std::env::temp_dir().join(format!(\"generated-child-{}.db\", std::process::id()))).await.unwrap(); let engine = blkit::runtime::Engine::new(registry, store, 1).unwrap(); let id = engine.start(\"orders\", \"1\", \"parent\", serde_json::json!({\"input\":true})).await.unwrap(); let result = tokio::time::timeout(std::time::Duration::from_secs(5), async { loop { let status = engine.status(&id).await.unwrap().unwrap(); if status.status != \"pending\" && status.status != \"running\" { break status; } tokio::time::sleep(std::time::Duration::from_millis(10)).await; } }).await.unwrap(); assert_eq!(result.result, Some(serde_json::json!(\"4\")));",
    );
}

#[test]
fn documented_subprocess_example_routes_success_and_each_child_outcome() {
    compile_and_test_graph(
        include_str!("../../../examples/subprocess.bl"),
        "let store = blkit::runtime::LocalStore::open(&std::env::temp_dir().join(format!(\"documented-subprocess-{}.db\", std::process::id()))).await.unwrap(); let engine = blkit::runtime::Engine::new(blkit::runtime::Registry::new(named_graph_definitions()).unwrap(), store, 1).unwrap(); for (input, expected) in [(\"2\", \"2\"), (\"0\", \"100\"), (\"-1\", \"200\"), (\"11\", \"300\")] { let id = engine.start(\"orders\", \"1.0\", \"parent\", serde_json::json!({\"input\":input})).await.unwrap(); let status = tokio::time::timeout(std::time::Duration::from_secs(5), async { loop { let item = engine.status(&id).await.unwrap().unwrap(); if matches!(item.status.as_str(), \"completed\" | \"failed\" | \"business-error\") { break item; } tokio::time::sleep(std::time::Duration::from_millis(10)).await; } }).await.unwrap(); assert_eq!(status.status, \"completed\", \"{input}: {:?}\", status.error); assert_eq!(status.result, Some(serde_json::json!(expected))); }",
    );
}

#[test]
fn generated_deadline_policy_is_preserved() {
    let generated = transpile(r#"namespace timing;
version "1";
start_event start { output input: Number; }
end_event done { input result: Number; }
pause_for wait { duration "1m"; }
process later { deadline queued "10m"; flow start -> wait; flow wait -> done; bind start.input -> done.result; }"#).unwrap();
    assert!(
        generated
            .contains("origin: \"queued\", duration: std::time::Duration::from_millis(600000)")
    );
}

#[test]
fn generated_wait_nodes_checkpoint_once_and_resume() {
    compile_and_test_graph(
        r#"namespace timing;
version "1";
start_event start { output input: DateTime; }
end_event done { input result: DateTime; }
pause_until wait { input at: DateTime; }
process later { flow start -> wait; flow wait -> done; bind start.input -> wait.at; bind start.input -> done.result; }"#,
        "let graph = named_graph_definitions().remove(0); let input = serde_json::json!({\"input\":\"2026-10-02T09:30:00+02:00\"}); let mut state = graph.checkpoint(&input).unwrap(); assert!(graph.waiting_until(&state).is_some()); graph.resume_due(&input, &mut state, i64::MAX).unwrap(); assert_eq!(state.outcome, Some(serde_json::json!(\"2026-10-02T09:30:00+02:00\")));",
    );
}

#[test]
fn documented_example_compiles() {
    compile_and_test_graph(
        include_str!("../../../examples/approve.bl"),
        "let graph = named_graph_definitions().remove(0); let input = serde_json::json!({\"input\":{\"total\":\"1250\",\"blocked\":false}}); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!(\"review\"));",
    );
}

#[test]
fn pricing_and_iteration_examples_compile_and_execute() {
    compile_and_test_graph(
        include_str!("../../../examples/pricing.bl"),
        "let graph = named_graph_definitions().remove(0); for (amount, expected) in [(\"200\", \"5\"), (\"10\", \"2\")] { let input = serde_json::json!({\"amount\":amount}); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!(expected)); }",
    );
    compile_and_test_graph(
        include_str!("../../../examples/iteration.bl"),
        "let graph = named_graph_definitions(); let gather = graph.iter().find(|item| item.name == \"gather\").unwrap(); let input = serde_json::json!({\"values\":[\"3\", \"1\"]}); let mut state = gather.checkpoint(&input).unwrap(); assert_eq!(gather.run(&input, &mut state).unwrap(), serde_json::json!([\"3\", \"1\"])); let repeat = graph.iter().find(|item| item.name == \"repeat_until_timeout\").unwrap(); assert_eq!(repeat.deadline.as_ref().unwrap().duration.as_secs(), 2);",
    );
}

#[test]
fn generated_decision_task_branches_with_typed_list_input() {
    compile_and_test(
        r#"namespace orders;
version "1.0";
Order = {total: Number, amounts: List<Number>};
enum Decision:
  approved;
  review;
decision_task decide {
  input request: Order;
  output result: Decision = choice;
  decision_table choice {
    output result: Decision;
    policy FIRST;
    input total: Number = request.total;
    input amounts: List<Number> = request.amounts;
    output decision: Decision;
    rule total > 1000 and amounts == [1, 2.5] -> Decision.review;
    default Decision.approved;
  }
}"#,
        "let order = |total: &str| Order { total: total.parse().unwrap(), amounts: vec![Number::ONE, \"2.5\".parse().unwrap()] }; assert_eq!(decide(order(\"1001\")).unwrap(), Decision::review); assert_eq!(decide(order(\"999\")).unwrap(), Decision::approved);",
    );
}

fn compile_and_test_graph(source: &str, assertion: &str) {
    let generated = transpile(source).unwrap();
    let directory = std::env::temp_dir().join(format!(
        "blkit-graph-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(directory.join("src")).unwrap();
    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../blkit-core");
    let name = directory.file_name().unwrap().to_string_lossy();
    fs::write(directory.join("Cargo.toml"), format!("[package]\nname = {name:?}\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\nblkit-core = {{ path = {core:?}, default-features = false, features = [\"worker\", \"local-persistence\"] }}\nrust_decimal = {{ version = \"1.39\", features = [\"serde-str\"] }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\nchrono = {{ version = \"0.4\", features = [\"serde\"] }}\ntokio = {{ version = \"1\", features = [\"macros\", \"rt-multi-thread\", \"time\"] }}\n")).unwrap();
    fs::write(directory.join("src/lib.rs"), format!("{generated}\n#[cfg(test)] mod checks {{ use blkit_core as blkit; use super::*; #[tokio::test] async fn graph() {{ {assertion} }} }}")).unwrap();
    let result = Command::new("cargo")
        .args(["test", "--manifest-path"])
        .arg(directory.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", nested_target())
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
fn generated_decision_task_receives_bound_start_port_and_returns_end_port() {
    compile_and_test_graph(
        include_str!("../../../examples/minimal.bl"),
        r#"let graph = named_graph_definitions().remove(0); let input = serde_json::json!({"amount":"7"}); assert!((graph.decode_input)(input.clone()).is_ok()); assert!((graph.decode_input)(serde_json::json!("7")).is_err()); assert!((graph.decode_input)(serde_json::json!({"amount":"7", "extra":1})).is_err()); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!("7"));"#,
    );
}

#[test]
fn process_rejects_unbound_and_mistyped_decision_inputs() {
    let source = r#"namespace demo;
version "1.0";
start_event start { output amount: Number; }
decision_task calculate {
  input amount: Number;
  output result: Number = compute;
  literal_expression compute { output result: Number; expression amount; }
}
end_event done { input result: Number; }
process example {
  flow start -> calculate;
  flow calculate -> done;
  bind start.amount -> calculate.amount;
  bind calculate.result -> done.result;
}"#;
    assert!(transpile(source).is_ok());
    let unbound = source.replace("bind start.amount -> calculate.amount;", "");
    assert!(transpile(&unbound).unwrap_err().contains("missing binding"));
    let mismatched = source.replace("input amount: Number;", "input amount: String;");
    assert!(
        transpile(&mismatched)
            .unwrap_err()
            .contains("type mismatch")
    );
}

#[test]
fn decision_nodes_infer_dependencies_and_validate_output_ports() {
    let source = r#"namespace demo;
version "1";
start_event start { output amount: Number; }
decision_task calculate {
  input amount: Number;
  output result: Number = doubled.result;
  literal_expression doubled { output result: Number; expression base.result; }
  literal_expression base { output result: Number; expression amount; }
}
end_event done { input result: Number; }
process example {
  flow start -> calculate;
  flow calculate -> done;
  bind start.amount -> calculate.amount;
  bind calculate.result -> done.result;
}"#;
    compile_and_test_graph(
        source,
        r#"let graph = named_graph_definitions().remove(0); let input = serde_json::json!({"amount":"7"}); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!("7"));"#,
    );
    assert!(
        transpile(&source.replace("doubled.result;", "doubled.missing;"))
            .unwrap_err()
            .contains("output port")
    );
    assert!(
        transpile(&source.replace("expression base.result;", "expression doubled.result;"))
            .unwrap_err()
            .contains("cycle")
    );
    assert!(
        transpile(&source.replace("expression amount; }", "expression doubled.result; }"))
            .unwrap_err()
            .contains("cycle")
    );
    assert!(
        transpile(&source.replace("expression base.result;", "expression base.missing;"))
            .unwrap_err()
            .contains("output port")
    );
    assert!(
        transpile(&source.replace("expression base.result;", "expression absent.result;"))
            .unwrap_err()
            .contains("unknown")
    );
}

#[test]
fn decision_tasks_accept_multiple_named_inputs_and_outputs() {
    compile_and_test_graph(
        r#"namespace demo;
version "1";
start_event start { output amount: Number; output other: Number; }
decision_task calculate {
  input amount: Number;
  input other: Number;
  output left: Number = original;
  output right: Number = second.result;
  literal_expression original { output result: Number; expression amount; }
  literal_expression second { output result: Number; expression other; }
}
end_event done { input left: Number; input right: Number; }
process example {
  flow start -> calculate;
  flow calculate -> done;
  bind start.amount -> calculate.amount;
  bind start.other -> calculate.other;
  bind calculate.left -> done.left;
  bind calculate.right -> done.right;
}"#,
        r#"let graph = named_graph_definitions().remove(0); let input = serde_json::json!({"amount":"7","other":"9"}); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!({"left":"7","right":"9"}));"#,
    );
}

#[test]
fn braced_tables_preserve_hit_policies_defaults_and_aggregations() {
    let mut source = String::from("namespace policies;\nversion \"1\";\n");
    for (name, policy, ty, priorities) in [
        ("first", "FIRST", "Number", ""),
        ("unique", "UNIQUE", "Number", ""),
        ("any", "ANY", "Number", ""),
        ("ranked", "PRIORITY", "Number", "priority 5; priority 9;"),
        ("ordered", "RULE_ORDER", "List<Number>", ""),
        (
            "sorted",
            "OUTPUT_ORDER",
            "List<Number>",
            "priority 5; priority 9;",
        ),
        ("collected", "COLLECT", "List<Number>", ""),
        ("sum", "COLLECT SUM", "Number", ""),
        ("min", "COLLECT MIN", "Number", ""),
        ("max", "COLLECT MAX", "Number", ""),
        ("count", "COLLECT COUNT", "Number", ""),
    ] {
        source.push_str(&format!("decision_task {name} {{ input amount: Number; output result: {ty} = table.result; decision_table table {{ output result: {ty}; policy {policy}; input value: Number = amount; output price: Number; {priorities} rule value > 5 -> 9; rule value > 1 -> 5; }} }}\n"));
    }
    compile_and_test(
        &source,
        r#"let n = |v: &str| -> Number { v.parse().unwrap() }; let seven = n("7"); let zero = n("0"); assert_eq!(first(seven).unwrap(), n("9")); assert!(unique(seven).unwrap_err().contains("UNIQUE")); assert!(any(seven).unwrap_err().contains("ANY")); assert_eq!(ranked(seven).unwrap(), n("5")); assert_eq!(ordered(seven).unwrap(), vec![n("9"), n("5")]); assert_eq!(sorted(seven).unwrap(), vec![n("5"), n("9")]); assert_eq!(collected(seven).unwrap(), vec![n("9"), n("5")]); assert_eq!(sum(seven).unwrap(), n("14")); assert_eq!(min(seven).unwrap(), n("5")); assert_eq!(max(seven).unwrap(), n("9")); assert_eq!(count(seven).unwrap(), n("2")); assert!(first(zero).is_err()); assert!(sum(zero).is_err()); assert_eq!(count(zero).unwrap(), n("0"));"#,
    );
    let default = r#"namespace policies;
version "1";
decision_task first {
  input amount: Number;
  output result: Number = table;
  decision_table table {
    output result: Number;
    policy FIRST;
    input value: Number = amount;
    output price: Number;
    rule value > 5 -> 9;
    default 2;
  }
}"#;
    compile_and_test(default, "assert_eq!(first(0.into()).unwrap(), 2.into());");
    let agreeing = source.replace("rule value > 1 -> 5;", "rule value > 1 -> 9;");
    compile_and_test(&agreeing, "assert_eq!(any(7.into()).unwrap(), 9.into());");
}

#[test]
fn braced_tables_validate_typed_multicolumn_results_and_unary_tests() {
    let source = r#"namespace policies;
version "1";
Quote = {price: Number, tier: String};
decision_task quotes {
  input amount: Number;
  output result: List<Quote> = table.result;
  decision_table table {
    output result: List<Quote>;
    policy RULE_ORDER;
    input value: Number = amount;
    output price: Number;
    output tier: String;
    rule value matches (< 10, [20..30]) and value > 5 -> 2, "low";
    rule value matches (>= 5, [20..30]) -> 3, "high";
    default 0, "none";
  }
}"#;
    compile_and_test(
        source,
        r#"let result = quotes("7".parse().unwrap()).unwrap(); assert_eq!(result, vec![Quote { price: 2.into(), tier: "low".into() }, Quote { price: 3.into(), tier: "high".into() }]); assert_eq!(quotes("-2".parse().unwrap()).unwrap(), vec![Quote { price: 0.into(), tier: "none".into() }]);"#,
    );
    let invalid = source
        .replace("List<Quote>", "Number")
        .replace("policy RULE_ORDER", "policy COLLECT SUM");
    assert!(transpile(&invalid).unwrap_err().contains("aggregation"));
    assert!(
        transpile(&source.replace("= table.result;", "= table.price;"))
            .unwrap_err()
            .contains("output port")
    );
    assert!(
        transpile(&source.replace("default 0, \"none\";", "default \"wrong\", \"none\";"))
            .unwrap_err()
            .contains("type mismatch")
    );
}

#[test]
fn braced_contexts_and_knowledge_infer_dependencies_and_evaluate_in_order() {
    let source = r#"namespace demo;
version "1";
decision_task quote {
  input amount: Number;
  output result: Number = summary.result;
  knowledge identity {
    input value: Number;
    output result: Number;
    expression value;
  }
  context summary {
    output result: Number;
    entry complete: Number = identity(initial);
    entry initial: Number = identity(base.result);
    result complete;
  }
  literal_expression base { output result: Number; expression amount; }
}"#;
    compile_and_test(source, "assert_eq!(quote(7.into()).unwrap(), 7.into());");
    assert!(
        transpile(&source.replace("identity(initial)", "identity(missing)"))
            .unwrap_err()
            .contains("unknown")
    );
    assert!(
        transpile(&source.replace("identity(base.result)", "identity(complete)"))
            .unwrap_err()
            .contains("cycle")
    );
    assert!(
        transpile(&source.replace("identity(initial)", "identity(\"wrong\")"))
            .unwrap_err()
            .contains("knowledge argument type")
    );
    assert!(
        transpile(&source.replace("result complete;", "result amount > 0;"))
            .unwrap_err()
            .contains("type mismatch")
    );
}

#[test]
fn braced_knowledge_captures_task_inputs_and_infers_node_dependencies() {
    let source = r#"namespace demo;
version "1";
decision_task quote {
  input amount: Number;
  output result: Number = answer;
  knowledge select { input other: Number; output result: Number; expression base.result; }
  literal_expression answer { output result: Number; expression select(amount); }
  literal_expression base { output result: Number; expression amount; }
}"#;
    compile_and_test(source, "assert_eq!(quote(7.into()).unwrap(), 7.into());");
    let captured_input = source.replace("expression base.result; }", "expression amount; }");
    compile_and_test(
        &captured_input,
        "assert_eq!(quote(7.into()).unwrap(), 7.into());",
    );
    let cyclic = source.replace("expression amount; }", "expression select(amount); }");
    assert!(transpile(&cyclic).unwrap_err().contains("cycle"));
}

#[test]
fn braced_knowledge_cycles_are_rejected() {
    let source = r#"namespace demo;
version "1";
decision_task quote {
  input amount: Number;
  output result: Number = answer;
  knowledge first { input value: Number; output result: Number; expression second(value); }
  knowledge second { input value: Number; output result: Number; expression first(value); }
  literal_expression answer { output result: Number; expression first(amount); }
}"#;
    assert!(transpile(source).unwrap_err().contains("cycle"));
}

#[test]
fn pricing_example_compiles_and_runs_braced_decision_graph() {
    compile_and_test_graph(
        include_str!("../../../examples/pricing.bl"),
        r#"let graph = named_graph_definitions().remove(0); for (amount, expected) in [("120", "5"), ("10", "2")] { let input = serde_json::json!({"amount":amount}); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!(expected)); }"#,
    );
}

#[test]
fn braced_xor_routes_bind_only_the_active_branch_and_return_multiple_ports() {
    let source = r#"namespace routes;
version "1";
start_event start { output amount: Number; output label: String; }
xor_split choice {}
xor_join chosen { split choice; input value: Number; output result: Number; }
decision_task high { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression amount; } }
decision_task low { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression 2; } }
end_event done { input total: Number; input label: String; }
process route {
  flow start -> choice;
  flow choice -> high when start.amount > 100;
  flow choice -> low else;
  flow high -> chosen;
  flow low -> chosen;
  flow chosen -> done;
  bind start.amount -> high.amount;
  bind start.amount -> low.amount;
  bind high.result -> chosen.value;
  bind low.result -> chosen.value;
  bind chosen.result -> done.total;
  bind start.label -> done.label;
}"#;
    compile_and_test_graph(
        source,
        r#"let graph = named_graph_definitions().remove(0); for (amount, expected) in [("120", "120"), ("10", "2")] { let input = serde_json::json!({"amount":amount,"label":"offer"}); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!({"total":expected,"label":"offer"})); }"#,
    );
    let unavailable = source.replace(
        "bind chosen.result -> done.total;",
        "bind high.result -> done.total;",
    );
    assert!(transpile(&unavailable).unwrap_err().contains("unavailable"));
    let route_unavailable = source.replace("when start.amount > 100", "when high.result > 100");
    assert!(
        transpile(&route_unavailable)
            .unwrap_err()
            .contains("unavailable")
    );
    let wrong_shape = source.replace(
        "end_event done { input total: Number; input label: String; }",
        "end_event done { input total: Number; input label: Number; }",
    );
    assert!(
        transpile(&wrong_shape)
            .unwrap_err()
            .contains("type mismatch")
    );
}

#[test]
fn braced_and_or_routes_join_activated_branches_in_order() {
    let source = r#"namespace routes;
version "1";
Pair = {left: Number, right: Number};
start_event start { output amount: Number; }
end_event pair_done { input result: Pair; }
end_event list_done { input result: List<Number>; }
and_split fork {}
and_join both { split fork; input left: Number; input right: Number; output result: Pair; }
or_split choice {}
or_join picked { split choice; input value: Number; output result: List<Number>; }
decision_task high { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression amount; } }
decision_task low { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression 2; } }
decision_task fallback { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression 0; } }
process parallel {
  flow start -> fork;
  flow fork -> high as left;
  flow fork -> low as right;
  flow high -> both;
  flow low -> both;
  flow both -> pair_done;
  bind start.amount -> high.amount;
  bind start.amount -> low.amount;
  bind high.result -> both.left;
  bind low.result -> both.right;
  bind both.result -> pair_done.result;
}
process optional {
  flow start -> choice;
  flow choice -> high when start.amount > 50;
  flow choice -> low when start.amount > 5;
  flow choice -> fallback else;
  flow high -> picked;
  flow low -> picked;
  flow fallback -> picked;
  flow picked -> list_done;
  bind start.amount -> high.amount;
  bind start.amount -> low.amount;
  bind start.amount -> fallback.amount;
  bind high.result -> picked.value;
  bind low.result -> picked.value;
  bind fallback.result -> picked.value;
  bind picked.result -> list_done.result;
}"#;
    compile_and_test_graph(
        source,
        r#"let graphs = named_graph_definitions(); let parallel = graphs.iter().find(|graph| graph.name == "parallel").unwrap(); let optional = graphs.iter().find(|graph| graph.name == "optional").unwrap(); let run = |graph: &blkit::compiled_graph::GraphDefinition, amount: &str| { let input = serde_json::json!({"amount":amount}); let mut state = graph.checkpoint(&input).unwrap(); graph.run(&input, &mut state).unwrap() }; assert_eq!(run(parallel, "120"), serde_json::json!({"left":"120","right":"2"})); assert_eq!(run(optional, "120"), serde_json::json!(["120","2"])); assert_eq!(run(optional, "10"), serde_json::json!(["2"])); assert_eq!(run(optional, "2"), serde_json::json!(["0"]));"#,
    );
    let crossed = source
        .replace(
            "bind high.result -> both.left;",
            "bind high.result -> both.right;",
        )
        .replace(
            "bind low.result -> both.right;",
            "bind low.result -> both.left;",
        );
    assert!(transpile(&crossed).unwrap_err().contains("branch label"));
    let missing_branch = source.replace("flow fallback -> picked;", "flow fallback -> list_done;");
    assert!(
        transpile(&missing_branch)
            .unwrap_err()
            .contains("missing split branch")
    );
}

#[test]
fn braced_xor_routes_bind_inputs_from_the_active_branch() {
    let source = r#"namespace routes;
version "1";
start_event start { output amount: Number; }
xor_split choice {}
decision_task high { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression amount; } }
decision_task low { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression 2; } }
decision_task settle { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression amount; } }
end_event done { input result: Number; }
process route {
  flow start -> choice;
  flow choice -> high when start.amount > 100;
  flow choice -> low else;
  flow high -> settle;
  flow low -> settle;
  flow settle -> done;
  bind start.amount -> high.amount;
  bind start.amount -> low.amount;
  bind high.result -> settle.amount;
  bind low.result -> settle.amount;
  bind settle.result -> done.result;
}"#;
    compile_and_test_graph(
        source,
        r#"let graph = named_graph_definitions().remove(0); for (amount, expected) in [("120", "120"), ("10", "2")] { let input = serde_json::json!({"amount":amount}); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!(expected)); }"#,
    );
    assert!(
        transpile(&source.replace(
            "bind low.result -> settle.amount;",
            "bind high.result -> settle.amount;"
        ))
        .unwrap_err()
        .contains("ambiguous")
    );
}

#[test]
fn braced_process_rejects_ambiguous_bindings_and_incompatible_normal_ends() {
    let source = include_str!("../../../examples/minimal.bl");
    let ambiguous = source.replace(
        "bind start.amount -> calculate.amount;",
        "bind start.amount -> calculate.amount; bind start.amount -> calculate.amount;",
    );
    assert!(transpile(&ambiguous).unwrap_err().contains("ambiguous"));
    let different_end = source.replace("end_event done { input result: Number; }", "end_event done { input result: Number; } end_event alternative { input other: Number; } xor_split choice {}")
        .replace("flow calculate -> done;", "flow calculate -> choice; flow choice -> done when start.amount > 0; flow choice -> alternative else;")
        .replace("bind calculate.result -> done.result;", "bind calculate.result -> done.result; bind calculate.result -> alternative.other;");
    assert!(
        transpile(&different_end)
            .unwrap_err()
            .contains("end event port shape")
    );
}

#[test]
fn braced_exceptional_events_retry_deadline_and_waits_lower_to_runtime() {
    compile_and_test_graph(
        r#"namespace routes;
version "1";
start_event start { output amount: Number; output until: DateTime; }
end_event done { input result: Number; }
error_event failure {}
cancel_event stopped {}
terminate_event halted {}
pause_for hold { duration "1ms"; }
pause_until later { input at: DateTime; }
process pause {
  retry max_retries 2 retry_for "10s" retry_delay "1ms" backoff exponential;
  deadline queued "20s";
  flow start -> hold;
  flow hold -> later;
  flow later -> done;
  bind start.until -> later.at;
  bind start.amount -> done.result;
}
process error { flow start -> failure; }
process cancel { flow start -> stopped; }
process terminate { flow start -> halted; }"#,
        r#"let graphs = named_graph_definitions(); let wait = graphs.iter().find(|graph| graph.name == "pause").unwrap(); assert_eq!(wait.retry.as_ref().unwrap().max_retries, 2); assert_eq!(wait.deadline.as_ref().unwrap().duration.as_secs(), 20); let input = serde_json::json!({"amount":"7","until":"2020-01-01T00:00:00Z"}); let mut state = wait.checkpoint(&input).unwrap(); let first = wait.waiting_until(&state).unwrap(); wait.resume_due(&input, &mut state, first).unwrap(); let second = wait.waiting_until(&state).unwrap(); wait.resume_due(&input, &mut state, second).unwrap(); assert_eq!(wait.run(&input, &mut state).unwrap(), serde_json::json!("7")); for (name, terminal) in [("error", "failure"), ("cancel", "stopped"), ("terminate", "halted")] { let graph = graphs.iter().find(|graph| graph.name == name).unwrap(); let mut state = graph.checkpoint(&input).unwrap(); assert!(graph.run(&input, &mut state).unwrap_err().contains(terminal)); }"#,
    );
}

#[test]
fn braced_subprocess_routes_normal_and_exceptional_outcomes() {
    compile_and_test_graph(
        r#"namespace routes;
version "1";
start_event start { output amount: Number; }
end_event done { input result: Number; }
end_event handled_success { input result: Number; }
error_event broken {}
subprocess child_call { process child; input amount: Number; output result: Number; }
subprocess failed_call { process failing; input amount: Number; output result: Number; }
decision_task passthrough { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression amount; } }
decision_task recover { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression 0; } }
process child { flow start -> passthrough; flow passthrough -> done; bind start.amount -> passthrough.amount; bind passthrough.result -> done.result; }
process failing { flow start -> broken; }
process parent { flow start -> child_call; flow child_call -> done; bind start.amount -> child_call.amount; bind child_call.result -> done.result; }
process handled { flow start -> failed_call; flow failed_call -> handled_success; flow failed_call -> recover on error; flow recover -> done; bind start.amount -> failed_call.amount; bind failed_call.result -> handled_success.result; bind start.amount -> recover.amount; bind recover.result -> done.result; }"#,
        r#"let graphs = named_graph_definitions(); let path = std::env::temp_dir().join(format!("braced-subprocess-{}.db", std::process::id())); let _ = std::fs::remove_file(&path); let store = blkit::runtime::LocalStore::open(&path).await.unwrap(); let engine = blkit::runtime::Engine::new(blkit::runtime::Registry::new(graphs).unwrap(), store.clone(), 1).unwrap(); for (name, expected) in [("parent", "7"), ("handled", "0")] { let id = engine.start("routes", "1", name, serde_json::json!({"amount":"7"})).await.unwrap(); let actual = tokio::time::timeout(std::time::Duration::from_secs(3), async { loop { let status = engine.status(&id).await.unwrap().unwrap(); if status.status == "completed" { break status.result.unwrap(); } if status.status == "failed" { panic!("{name} failed: {:?}", status.error); } tokio::time::sleep(std::time::Duration::from_millis(5)).await; } }).await.unwrap(); assert_eq!(actual, serde_json::json!(expected)); } drop(engine); drop(store); std::fs::remove_file(path).unwrap();"#,
    );
}

#[test]
fn braced_wait_policies_and_outcome_routes_reject_invalid_inputs() {
    let source = include_str!("../../../examples/minimal.bl");
    let retry = source.replace("process example {", "process example { retry max_retries 1 retry_for \"0s\" retry_delay \"1s\" backoff exponential;");
    assert!(transpile(&retry).unwrap_err().contains("retry_for"));
    let wait = source
        .replace(
            "end_event done",
            "pause_until hold { input at: DateTime; } end_event done",
        )
        .replace(
            "flow start -> calculate;",
            "flow start -> hold; flow hold -> calculate; bind start.amount -> hold.at;",
        );
    assert!(transpile(&wait).unwrap_err().contains("type mismatch"));
    let outcome = source.replace(
        "flow calculate -> done;",
        "flow calculate -> done on error;",
    );
    assert!(
        transpile(&outcome)
            .unwrap_err()
            .contains("outcome flow requires subprocess")
    );
}

#[test]
fn braced_decision_task_loops_are_bounded_and_respect_pre_post_checks() {
    let source = r#"namespace loops;
version "1";
start_event start { output amount: Number; }
end_event done { input result: Number; }
decision_task echo { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression amount; } }
process post { repeat_post echo while echo.result < 3 max_iterations 2; flow start -> echo; flow echo -> done; bind start.amount -> echo.amount; bind echo.result -> done.result; }
process pre { repeat_pre echo while echo.result < 3 initial 7 max_iterations 2; flow start -> echo; flow echo -> done; bind start.amount -> echo.amount; bind echo.result -> done.result; }"#;
    compile_and_test_graph(
        source,
        r#"let graphs = named_graph_definitions(); let post = graphs.iter().find(|graph| graph.name == "post").unwrap(); let pre = graphs.iter().find(|graph| graph.name == "pre").unwrap(); let input = serde_json::json!({"amount":"7"}); let mut state = post.checkpoint(&input).unwrap(); assert_eq!(post.run(&input, &mut state).unwrap(), serde_json::json!("7")); let small = serde_json::json!({"amount":"1"}); let mut state = post.checkpoint(&small).unwrap(); assert!(post.run(&small, &mut state).unwrap_err().contains("task-iteration-limit")); let mut state = pre.checkpoint(&small).unwrap(); assert!(pre.ready(&state).is_empty()); assert_eq!(pre.run(&small, &mut state).unwrap(), serde_json::json!("7"));"#,
    );
    let zero = source.replace("max_iterations 2", "max_iterations 0");
    assert!(transpile(&zero).unwrap_err().contains("max_iterations"));
    let missing_initial = source.replace(" initial 7", "");
    assert!(transpile(&missing_initial).unwrap_err().contains("initial"));
    let bad_initial = source.replace("initial 7", "initial \"bad\"");
    assert!(
        transpile(&bad_initial)
            .unwrap_err()
            .contains("type mismatch")
    );
    let duration = source.replace("max_iterations 2", "max_duration \"10s\"");
    assert!(transpile(&duration).is_ok());
    let zero_duration = source.replace("max_iterations 2", "max_duration \"0s\"");
    assert!(
        transpile(&zero_duration)
            .unwrap_err()
            .contains("max_duration")
    );
}

#[test]
fn braced_decision_task_multi_instance_supports_sequential_and_parallel() {
    compile_and_test_graph(
        r#"namespace loops;
version "1";
start_event start { output amounts: List<Number>; }
end_event done { input result: List<Number>; }
decision_task echo { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression amount; } }
process sequential { multi_instance echo each start.amounts sequential; flow start -> echo; flow echo -> done; bind start.amounts -> echo.amount; bind echo.result -> done.result; }
process parallel { multi_instance echo each start.amounts parallel; flow start -> echo; flow echo -> done; bind start.amounts -> echo.amount; bind echo.result -> done.result; }"#,
        r#"let graphs = named_graph_definitions(); let input = serde_json::json!({"amounts":["1","2"]}); for (name, ready) in [("sequential",1), ("parallel",2)] { let graph = graphs.iter().find(|graph| graph.name == name).unwrap(); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.ready(&state).len(), ready); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!(["1","2"])); }"#,
    );
}

#[test]
fn string_errors_propagate_through_named_graph_tasks_and_routes() {
    compile_and_test_graph(
        r#"namespace string_graph;
version "1";
start_event start { output text: String; output pattern: String; }
end_event done { input result: Bool; }
xor_split choice {}
xor_join joined { split choice; input value: Bool; output result: Bool; }
decision_task verify {
  input text: String;
  input pattern: String;
  output result: Bool = check;
  literal_expression check { output result: Bool; expression matches(text, pattern); }
}
decision_task fallback {
  input text: String;
  input pattern: String;
  output result: Bool = check;
  literal_expression check { output result: Bool; expression matches(text, pattern); }
}
process execute {
  flow start -> verify;
  flow verify -> done;
  bind start.text -> verify.text;
  bind start.pattern -> verify.pattern;
  bind verify.result -> done.result;
}
process route {
  flow start -> choice;
  flow choice -> verify when matches(start.text, start.pattern);
  flow choice -> fallback else;
  flow verify -> joined;
  flow fallback -> joined;
  flow joined -> done;
  bind start.text -> verify.text;
  bind start.pattern -> verify.pattern;
  bind start.text -> fallback.text;
  bind start.pattern -> fallback.pattern;
  bind verify.result -> joined.value;
  bind fallback.result -> joined.value;
  bind joined.result -> done.result;
}"#,
        r#"let graphs = named_graph_definitions(); for name in ["execute", "route"] { let graph = graphs.iter().find(|g| g.name == name).unwrap(); let good = serde_json::json!({"text":"abc", "pattern":"b"}); let mut state = graph.checkpoint(&good).unwrap(); assert_eq!(graph.run(&good, &mut state).unwrap(), serde_json::json!(true)); let bad = serde_json::json!({"text":"abc", "pattern":"["}); assert!(graph.checkpoint(&bad).and_then(|mut state| graph.run(&bad, &mut state)).is_err(), "{name} swallowed invalid regex"); }"#,
    );
}

#[test]
fn string_errors_propagate_through_multi_instance_and_task_loops() {
    compile_and_test_graph(
        r#"namespace string_graph;
version "1";
start_event list_start { output input: List<String>; }
start_event text_start { output input: String; }
end_event list_done { input result: List<String>; }
end_event text_done { input result: String; }
decision_task initial { input input: String; output result: String = value; literal_expression value { output result: String; expression charAt(input, 1); } }
process batch {
  multi_instance initial each list_start.input sequential;
  flow list_start -> initial;
  flow initial -> list_done;
  bind list_start.input -> initial.input;
  bind initial.result -> list_done.result;
}
process cycle {
  repeat_post initial while initial.result == "" max_iterations 2;
  flow text_start -> initial;
  flow initial -> text_done;
  bind text_start.input -> initial.input;
  bind initial.result -> text_done.result;
}"#,
        r#"let graphs = named_graph_definitions(); for (name, good, bad, result) in [("batch", serde_json::json!({"input":["ab", "cd"]}), serde_json::json!({"input":["ab", ""]}), serde_json::json!(["a", "c"])), ("cycle", serde_json::json!({"input":"ab"}), serde_json::json!({"input":""}), serde_json::json!("a"))] { let graph = graphs.iter().find(|g| g.name == name).unwrap(); let mut state = graph.checkpoint(&good).unwrap(); assert_eq!(graph.run(&good, &mut state).unwrap(), result); assert!(graph.checkpoint(&bad).and_then(|mut state| graph.run(&bad, &mut state)).is_err(), "{name} swallowed invalid position"); }"#,
    );
}

#[test]
fn invalid_retry_policy_is_rejected_before_rust_generation() {
    let source = r#"namespace orders;
version "1.0";
start_event start { output input: Number; }
end_event done { input result: Number; }
process route { retry max_retries 2 retry_for "0s" retry_delay "1s" backoff exponential; flow start -> done; bind start.input -> done.result; }"#;
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
    compile_and_test_graph(
        r#"namespace orders;
version "1.0";
start_event start { output input: Number; }
end_event done { input result: Number; }
xor_split gate {}
xor_join joined { split gate; input value: Number; output result: Number; }
decision_task echo { input input: Number; output result: Number = value; literal_expression value { output result: Number; expression input; } }
decision_task high { input input: Number; output result: Number = value; literal_expression value { output result: Number; expression input; } }
process route {
  retry max_retries 2 retry_for "10s" retry_delay "1s" backoff exponential;
  flow start -> gate;
  flow gate -> high when start.input > 10;
  flow gate -> echo else;
  flow high -> joined;
  flow echo -> joined;
  flow joined -> done;
  bind start.input -> high.input;
  bind start.input -> echo.input;
  bind high.result -> joined.value;
  bind echo.result -> joined.value;
  bind joined.result -> done.result;
}"#,
        r#"let graph = named_graph_definitions().remove(0); assert_eq!((graph.namespace, graph.version, graph.name), ("orders", "1.0", "route")); let retry = graph.retry.as_ref().unwrap(); assert_eq!(retry.max_retries, 2); assert_eq!(retry.retry_for.as_secs(), 10); assert_eq!(retry.retry_delay.as_secs(), 1); assert_eq!(retry.backoff, "exponential"); let input = serde_json::json!({"input":"12"}); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!("12"));"#,
    );
}

#[test]
fn generated_named_graph_emits_exceptional_terminal_nodes_without_retry_by_default() {
    compile_and_test_graph(
        r#"namespace orders;
version "1.0";
start_event start { output input: Number; }
end_event done { input result: Number; }
error_event failure {}
cancel_event stop {}
terminate_event halt {}
xor_split gate {}
decision_task echo { input input: Number; output result: Number = value; literal_expression value { output result: Number; expression input; } }
process route {
  flow start -> gate;
  flow gate -> echo when start.input > 0;
  flow gate -> failure when start.input == 0;
  flow gate -> stop when start.input < 0;
  flow gate -> halt else;
  flow echo -> done;
  bind start.input -> echo.input;
  bind echo.result -> done.result;
}"#,
        r#"let graph = named_graph_definitions().remove(0); assert!(graph.retry.is_none()); assert!(graph.nodes.iter().any(|node| matches!(node.kind, blkit::compiled_graph::GraphNodeKind::Error))); assert!(graph.nodes.iter().any(|node| matches!(node.kind, blkit::compiled_graph::GraphNodeKind::Cancel))); assert!(graph.nodes.iter().any(|node| matches!(node.kind, blkit::compiled_graph::GraphNodeKind::Terminate))); let input = serde_json::json!({"input":"2"}); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!("2"));"#,
    );
}

#[test]
fn generated_named_graph_compiles_as_a_runtime_definition() {
    compile_and_test_graph(
        include_str!("../../../tools/dev-harness/examples/graph.bl"),
        "let graph = named_graph_definitions(); let run = |name: &str, total: &str| { let process = graph.iter().find(|g| g.name == name).unwrap(); let input = serde_json::json!({\"total\": total}); let mut state = process.checkpoint(&input).unwrap(); process.run(&input, &mut state).unwrap() }; assert_eq!(run(\"decide\", \"1200\"), serde_json::json!(\"review\")); assert_eq!(run(\"decide\", \"900\"), serde_json::json!(\"approved\")); assert_eq!(run(\"parallel\", \"42\"), serde_json::json!({\"left\":\"42\",\"right\":\"42\"})); assert_eq!(run(\"offers\", \"1200\"), serde_json::json!([\"1200\",\"1200\"])); assert_eq!(run(\"offers\", \"10\"), serde_json::json!([\"10\"]));",
    );
}

#[test]
fn graph_generated_identifiers_do_not_shadow_user_names() {
    compile_and_test_graph(
        r#"namespace t;
version "1";
start_event start { output values: Number; }
end_event done { input result: Number; }
decision_task echo { input item: Number; output result: Number = value; literal_expression value { output result: Number; expression item; } }
decision_task next { input item: Number; output result: Number = value; literal_expression value { output result: Number; expression item; } }
process route {
  flow start -> echo;
  flow echo -> next;
  flow next -> done;
  bind start.values -> echo.item;
  bind echo.result -> next.item;
  bind next.result -> done.result;
}"#,
        r#"let graph = named_graph_definitions().remove(0); let input = serde_json::json!({"values":"7"}); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!("7"));"#,
    );
}

#[test]
fn generated_definition_name_is_reserved_in_graph_programs() {
    let source = r#"namespace t;
version "1";
start_event start { output item: Number; }
end_event done { input result: Number; }
decision_task graph_definitions { input item: Number; output result: Number = value; literal_expression value { output result: Number; expression item; } }
process route { flow start -> graph_definitions; flow graph_definitions -> done; bind start.item -> graph_definitions.item; bind graph_definitions.result -> done.result; }"#;
    assert!(
        transpile(source)
            .unwrap_err()
            .contains("reserved generated name")
    );
}

// One generated decision per function keeps failures attributable to that overload.
fn compile_list_operation(input: &str, output: &str, expression: &str, assertion: &str) {
    let source = format!(
        "namespace lists; version \"1\"; decision_task evaluate {{ input input: {input}; input position: Number; output result: {output} = value; literal_expression value {{ output result: {output}; expression {expression}; }} }}"
    );
    compile_and_test(&source, assertion);
}

#[test]
fn generated_list_count_handles_empty_inputs() {
    compile_list_operation(
        "List<Number>",
        "Number",
        "count(input)",
        "assert_eq!(evaluate(vec![Number::ONE], Number::ZERO).unwrap(), Number::ONE); assert_eq!(evaluate(vec![], Number::ZERO).unwrap(), Number::ZERO);",
    );
}

#[test]
fn generated_sort_numbers_is_stable_natural_descending_and_priority_ordered() {
    compile_and_test(
        r#"namespace sorting; version "1";
decision_task natural { input value: List<Number>; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression sort(value); } }
decision_task ascending { input value: List<Number>; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression sort(value, "asc"); } }
decision_task descending { input value: List<Number>; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression sort(value, "desc"); } }
decision_task priority { input value: List<Number>; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression sort(value, [3,1,3]); } }
decision_task invalid_order { input value: String; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression sort([1,2], value); } }"#,
        r#"let n = |x| Number::from(x); let values = vec![n(3),n(1),n(2),n(1)]; assert_eq!(natural(values.clone()).unwrap(), vec![n(1),n(1),n(2),n(3)]); assert_eq!(ascending(values.clone()).unwrap(), vec![n(1),n(1),n(2),n(3)]); assert_eq!(descending(values.clone()).unwrap(), vec![n(3),n(2),n(1),n(1)]); assert_eq!(priority(vec![n(2),n(1),n(3),n(2),n(1)]).unwrap(), vec![n(3),n(1),n(1),n(2),n(2)]); assert!(natural(vec![]).unwrap().is_empty()); assert!(invalid_order("sideways".into()).is_err());"#,
    );
}

#[test]
fn generated_sort_priority_reads_original_values_after_reversing_source() {
    compile_and_test(
        r#"namespace sorting; version "1";
decision_task prioritize { input values: List<Number>; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression sort(reverse(values), values); } }"#,
        "let n = |x| Number::from(x); assert_eq!(prioritize(vec![n(1), n(2), n(3)]).unwrap(), vec![n(1), n(2), n(3)]);",
    );
}

#[test]
fn generated_sort_empty_priority_keeps_input_unchanged() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "sort(input, [])",
        "let values = vec![Number::from(2),Number::ZERO,Number::ONE]; assert_eq!(evaluate(values.clone(), Number::ZERO).unwrap(), [Number::ZERO,Number::ONE,Number::from(2)]); assert_eq!(values, [Number::from(2),Number::ZERO,Number::ONE]);",
    );
}

#[test]
fn generated_sort_descending_reverses_natural_order() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "sort(input, \"desc\")",
        "let n = |x| Number::from(x); assert_eq!(evaluate(vec![n(2),n(1),n(3)], Number::ZERO).unwrap(), [n(3),n(2),n(1)]);",
    );
}

#[test]
fn generated_sort_strings_uses_unicode_code_point_order_and_unlisted_natural_order() {
    compile_list_operation(
        "List<String>",
        "List<String>",
        "sort(input)",
        r#"assert_eq!(evaluate(vec!["café".into(), "cafz".into(), "banana".into(), "Apple".into(), "apple".into(), "app".into()], Number::ZERO).unwrap(), ["Apple","app","apple","banana","cafz","café"]);"#,
    );
    compile_list_operation(
        "List<String>",
        "List<String>",
        "sort(input, [\"s\",\"m\",\"l\"])",
        r#"assert_eq!(evaluate(vec!["xl".into(), "m".into(), "s".into(), "café".into(), "cafz".into(), "banana".into(), "Apple".into(), "app".into(), "apple".into()], Number::ZERO).unwrap(), ["s","m","Apple","app","apple","banana","cafz","café","xl"]);"#,
    );
}

#[test]
fn generated_sort_temporal_kinds_compare_and_reject_incompatible_zones() {
    for (ty, first, second) in [
        ("Date", "2026-01-02", "2026-01-01"),
        ("Time", "12:00:00", "11:00:00"),
        ("DateTime", "2026-01-02T00:00:00Z", "2026-01-01T00:00:00Z"),
        ("DTDuration", "PT2H", "PT1H"),
        ("YMDuration", "P2M", "P1M"),
    ] {
        compile_list_operation(
            ty,
            &format!("List<{ty}>"),
            &format!(
                "sort([input, {constructor}(\"{second}\")])",
                constructor = match ty {
                    "Date" => "date",
                    "Time" => "time",
                    "DateTime" => "datetime",
                    "DTDuration" => "dtDuration",
                    _ => "ymDuration",
                }
            ),
            &format!(
                "assert_eq!(serde_json::to_value(evaluate(serde_json::from_value(serde_json::json!({first:?})).unwrap(), Number::ZERO).unwrap()).unwrap(), serde_json::json!([{second:?}, {first:?}]));"
            ),
        );
    }
    compile_list_operation(
        "DateTime",
        "List<DateTime>",
        "sort([input, datetime(\"2026-01-01T00:00:00Z\")])",
        r#"assert!(evaluate("2026-01-01T00:00:00".parse().unwrap(), Number::ZERO).is_err()); let first = "2026-01-01T01:00:00+01:00".parse().unwrap(); assert_eq!(serde_json::to_value(evaluate(first, Number::ZERO).unwrap()).unwrap(), serde_json::json!(["2026-01-01T01:00:00+01:00","2026-01-01T00:00:00Z"]));"#,
    );
}

#[test]
fn generated_string_join_wraps_the_completed_string_and_empty_list() {
    compile_list_operation(
        "List<String>",
        "String",
        "stringJoin(input, \"-\", \"[\", \"]\")",
        "assert_eq!(evaluate(vec![\"a\".into(),\"b\".into()], Number::ZERO).unwrap(), \"[a-b]\"); assert_eq!(evaluate(vec![], Number::ZERO).unwrap(), \"[]\");",
    );
}

#[test]
fn generated_string_join_defaults_to_empty_separator() {
    compile_list_operation(
        "List<String>",
        "String",
        "stringJoin(input)",
        "assert_eq!(evaluate(vec![\"a\".into(),\"b\".into()], Number::ZERO).unwrap(), \"ab\"); assert_eq!(evaluate(vec![], Number::ZERO).unwrap(), \"\");",
    );
}

#[test]
fn generated_zip_string_join_defaults_to_empty_delimiter_and_wrappers() {
    compile_list_operation(
        "List<List<String>>",
        "List<String>",
        "zipStringJoin(input)",
        "assert_eq!(evaluate(vec![vec![\"a\".into()],vec![\"b\".into()]], Number::ZERO).unwrap(), [\"ab\"]);",
    );
}

#[test]
fn generated_zip_string_join_supports_scalar_delimiter_and_wrappers() {
    compile_list_operation(
        "List<List<String>>",
        "List<String>",
        "zipStringJoin(input, \"-\", \"[\", \"]\")",
        r#"assert_eq!(evaluate(vec![vec!["a".into(),"b".into()],vec!["1".into(),"2".into()]], Number::ZERO).unwrap(), ["[a-1]","[b-2]"]); assert_eq!(evaluate(vec![vec![],vec![]], Number::ZERO).unwrap(), Vec::<String>::new()); assert_eq!(evaluate(vec![], Number::ZERO).unwrap(), Vec::<String>::new()); assert!(evaluate(vec![vec!["a".into()], vec![]], Number::ZERO).is_err());"#,
    );
}

#[test]
fn generated_zip_string_join_supports_per_gap_and_per_column_wrappers() {
    compile_list_operation(
        "List<List<String>>",
        "List<String>",
        "zipStringJoin(input, [\"-\", \":\"], [\"<\",\"(\",\"{\"], [\">\",\")\",\"}\"])",
        r#"assert_eq!(evaluate(vec![vec!["a".into(),"b".into()],vec!["1".into(),"2".into()],vec!["x".into(),"y".into()]], Number::ZERO).unwrap(), ["<a>-(1):{x}","<b>-(2):{y}"]);"#,
    );
}

#[test]
fn generated_zip_string_join_allows_independent_scalar_and_column_wrappers() {
    compile_list_operation(
        "List<List<String>>",
        "List<String>",
        "zipStringJoin(input, [\"-\",\":\"], [\"<\",\"(\",\"{\"], \"!\")",
        r#"assert_eq!(evaluate(vec![vec!["a".into()],vec!["1".into()],vec!["x".into()]], Number::ZERO).unwrap(), ["<a-(1:{x!"]);"#,
    );
    compile_list_operation(
        "List<List<String>>",
        "List<String>",
        "zipStringJoin(input, \"-\", \"[\", [\"]\",\")\"])",
        r#"assert_eq!(evaluate(vec![vec!["a".into()],vec!["1".into()]], Number::ZERO).unwrap(), ["[a]-1)"]);"#,
    );
}

#[test]
fn generated_zip_string_join_rejects_bad_delimiter_and_wrapper_lengths() {
    for (expression, columns) in [
        ("zipStringJoin(input, [\"-\",\":\"])", 2),
        ("zipStringJoin(input, \"-\", [\"[\"], \"]\")", 2),
        ("zipStringJoin(input, \"-\", \"[\", [\"]\"])", 2),
    ] {
        compile_list_operation(
            "List<List<String>>",
            "List<String>",
            expression,
            &format!(
                "let input = vec![vec![String::from(\"a\")]; {columns}]; assert!(evaluate(input, Number::ZERO).is_err());"
            ),
        );
    }
}

#[test]
fn generated_zip_string_join_empty_columns_validate_format_lengths() {
    compile_list_operation(
        "List<List<String>>",
        "List<String>",
        r#"zipStringJoin(input, ["-"], "[", "]")"#,
        r#"assert_eq!(evaluate(vec![vec![],vec![]], Number::ZERO).unwrap(), Vec::<String>::new()); assert!(evaluate(vec![vec![]], Number::ZERO).is_err());"#,
    );
}

#[test]
fn generated_zip_string_join_zero_columns_require_empty_list_formatters() {
    compile_list_operation(
        "Number",
        "List<String>",
        "zipStringJoin([], [], [], [])",
        "assert_eq!(evaluate(Number::ZERO, Number::ZERO).unwrap(), Vec::<String>::new());",
    );
    compile_list_operation(
        "List<List<String>>",
        "List<String>",
        "zipStringJoin(input, [], [], [])",
        "assert_eq!(evaluate(vec![], Number::ZERO).unwrap(), Vec::<String>::new()); assert!(evaluate(vec![vec![]], Number::ZERO).is_err());",
    );
    compile_list_operation(
        "List<List<String>>",
        "List<String>",
        "zipStringJoin(input, [\"-\"], [], [])",
        "assert!(evaluate(vec![], Number::ZERO).is_err());",
    );
}

#[test]
fn generated_seq_includes_reached_endpoints_and_stops_before_unreached_endpoints() {
    compile_and_test(
        r#"namespace sequences; version "1";
decision_task forward { input value: Number; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression seq(5,10); } }
decision_task backward { input value: Number; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression seq(10,5); } }
decision_task fractional { input value: Number; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression seq(0,1,0.25); } }
decision_task unreached { input value: Number; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression seq(0,0.95,0.1); } }
decision_task opposite { input value: Number; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression seq(5,10,-1); } }"#,
        r#"let n = |v: &str| v.parse::<Number>().unwrap(); assert_eq!(forward(n("0")).unwrap(), (5..=10).map(Number::from).collect::<Vec<_>>()); assert_eq!(backward(n("0")).unwrap(), (5..=10).rev().map(Number::from).collect::<Vec<_>>()); assert_eq!(fractional(n("0")).unwrap(), [n("0"),n("0.25"),n("0.5"),n("0.75"),n("1")]); assert_eq!(unreached(n("0")).unwrap().last(), Some(&n("0.9"))); assert_eq!(opposite(n("0")).unwrap(), forward(n("0")).unwrap());"#,
    );
}

#[test]
fn generated_seq_opposite_sign_step_follows_endpoint_direction() {
    compile_list_operation(
        "Number",
        "List<Number>",
        "seq(5,10,-1)",
        "let n = |x| Number::from(x); assert_eq!(evaluate(n(0), Number::ZERO).unwrap(), (5..=10).map(n).collect::<Vec<_>>());",
    );
}

#[test]
fn generated_seq_rejects_zero_step_and_unrepresentable_result_sizes() {
    compile_and_test(
        r#"namespace sequences; version "1";
decision_task steps { input value: Number; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression seq(0,1,value); } }
decision_task enormous { input value: Number; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression seq(0,value); } }
decision_task endpoints { input value: Number; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression [value - 1, value]; } }
decision_task precision_limit { input value: Number; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression seq(value - 1, value, 0.0000000000000000000000000001); } }"#,
        r#"assert!(steps(Number::ZERO).is_err()); assert!(steps("0.0000000000000000000000000001".parse().unwrap()).is_err()); assert!(enormous(Number::MAX).is_err()); assert_eq!(endpoints(Number::MAX).unwrap(), [Number::MAX.checked_sub(Number::ONE).unwrap(), Number::MAX]); assert!(precision_limit(Number::MAX).is_err());"#,
    );
}

#[test]
fn generated_all_any_use_boolean_values_and_empty_identities() {
    compile_and_test(
        r#"namespace bools; version "1";
decision_task all_values { input value: List<Bool>; output result: Bool = out; literal_expression out { output result: Bool; expression all(value); } }
decision_task any_values { input value: List<Bool>; output result: Bool = out; literal_expression out { output result: Bool; expression any(value); } }"#,
        r#"assert!(all_values(vec![]).unwrap()); assert!(!any_values(vec![]).unwrap()); assert!(!all_values(vec![true,true,false]).unwrap()); assert!(all_values(vec![true,true]).unwrap()); assert!(any_values(vec![false,false,true]).unwrap()); assert!(!any_values(vec![false,false]).unwrap());"#,
    );
}

#[test]
fn generated_boolean_aggregates_propagate_list_element_errors() {
    compile_and_test(
        r#"namespace bools; version "1";
decision_task all_values { input value: Number; output result: Bool = out; literal_expression out { output result: Bool; expression all([false, 1 / value > 0]); } }
decision_task any_values { input value: Number; output result: Bool = out; literal_expression out { output result: Bool; expression any([true, 1 / value > 0]); } }"#,
        "assert!(all_values(Number::ZERO).is_err()); assert!(any_values(Number::ZERO).is_err());",
    );
}

#[test]
fn generated_boolean_aggregates_preserve_knowledge_shadowing() {
    compile_and_test(
        r#"namespace shadows; version "1";
decision_task check { input value: Number; output result: Number = out; knowledge all { input number: Number; output result: Number; expression number + 1; } knowledge any { input number: Number; output result: Number; expression number + 2; } literal_expression out { output result: Number; expression all(value) + any(value); } }"#,
        "assert_eq!(check(Number::from(3)).unwrap(), Number::from(9));",
    );
}

#[test]
fn generated_any_boolean_list_uses_false_empty_identity() {
    compile_list_operation(
        "List<Bool>",
        "Bool",
        "any(input)",
        "assert!(!evaluate(vec![], Number::ZERO).unwrap()); assert!(evaluate(vec![false,true], Number::ZERO).unwrap());",
    );
}

#[test]
fn generated_colon_sequences_keep_range_and_dictionary_colons() {
    compile_and_test(
        r#"namespace colons; version "1";
decision_task descending { input value: Number; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression 10:5; } }
decision_task fractional { input value: Number; output result: List<Number> = out; literal_expression out { output result: List<Number>; expression 0.5:2.5; } }
decision_task range_member { input value: Number; output result: Bool = out; literal_expression out { output result: Bool; expression value in [1..3]; } }
decision_task dictionary_key { input value: Number; output result: Number = out; literal_expression out { output result: Number; expression {score: value}.score; } }"#,
        r#"let n = |v: i64| Number::from(v); assert_eq!(descending(n(0)).unwrap(), (5..=10).rev().map(n).collect::<Vec<_>>()); assert_eq!(fractional(n(0)).unwrap(), ["0.5".parse().unwrap(), "1.5".parse().unwrap(), "2.5".parse().unwrap()]); assert!(range_member(n(2)).unwrap()); assert_eq!(dictionary_key(n(7)).unwrap(), n(7));"#,
    );
}

#[test]
fn generated_list_is_empty_handles_empty_inputs() {
    compile_list_operation(
        "List<Number>",
        "Bool",
        "isEmpty(input)",
        "assert!(evaluate(vec![], Number::ZERO).unwrap()); assert!(!evaluate(vec![Number::ONE], Number::ZERO).unwrap());",
    );
}

#[test]
fn generated_list_contains_uses_element_equality() {
    compile_list_operation(
        "List<List<Number>>",
        "Bool",
        "listContains(input, [1,2])",
        "let n = |v: i64| Number::from(v); assert!(evaluate(vec![vec![n(1),n(2)],vec![n(3)]], n(0)).unwrap()); assert!(!evaluate(vec![vec![n(2),n(1)]], n(0)).unwrap());",
    );
}

#[test]
fn generated_list_contains_contextually_typed_empty_literal() {
    compile_list_operation(
        "List<Number>",
        "Bool",
        "listContains([], 2)",
        "assert!(!evaluate(vec![Number::ONE], Number::ZERO).unwrap());",
    );
}

#[test]
fn generated_list_index_of_infers_empty_source_from_match() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "indexOf([], 2)",
        "assert_eq!(evaluate(vec![Number::ONE], Number::ZERO).unwrap(), Vec::<Number>::new());",
    );
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "indexOf([], input)",
        "assert_eq!(evaluate(vec![Number::ONE], Number::ZERO).unwrap(), Vec::<Number>::new());",
    );
    compile_list_operation(
        "List<Dictionary<Number>>",
        "List<Number>",
        "indexOf([], {x: 1})",
        "assert_eq!(evaluate(vec![], Number::ZERO).unwrap(), Vec::<Number>::new());",
    );
}

#[test]
fn generated_list_index_of_matches_structural_dictionaries() {
    compile_list_operation(
        "List<Dictionary<Number>>",
        "List<Number>",
        "indexOf(input, {x: 1})",
        "let a = [(String::from(\"x\"), Number::from(1))].into(); let b = [(String::from(\"x\"), Number::from(2))].into(); assert_eq!(evaluate(vec![a, b, [(String::from(\"x\"), Number::from(1))].into()], Number::ZERO).unwrap(), vec![Number::from(1), Number::from(3)]);",
    );
}

#[test]
fn generated_list_index_of_does_not_shadow_needle_input() {
    compile_and_test(
        r#"namespace lists; version "1";
decision_task positions { input needle: List<Number>; output result: List<Number> = found; literal_expression found { output result: List<Number>; expression indexOf(needle, 2); } }"#,
        "let n = |v: i64| Number::from(v); assert_eq!(positions(vec![n(1),n(2),n(2)]).unwrap(), vec![n(2),n(3)]);",
    );
}

#[test]
fn generated_list_index_of_returns_all_one_based_matches() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "indexOf(input, 2)",
        "let n = |v: i64| Number::from(v); assert_eq!(evaluate(vec![n(1),n(2),n(3),n(2)], n(0)).unwrap(), vec![n(2),n(4)]); assert_eq!(evaluate(vec![n(1)], n(0)).unwrap(), Vec::<Number>::new());",
    );
}

#[test]
fn generated_sublist_checks_signed_positions_and_lengths() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "sublist(input, position)",
        "let n = |v: i64| Number::from(v); let input = vec![n(1),n(2),n(3)]; assert_eq!(evaluate(input.clone(), n(1)).unwrap(), input); assert_eq!(evaluate(input.clone(), n(-2)).unwrap(), vec![n(2),n(3)]); assert_eq!(evaluate(input.clone(), n(-1)).unwrap(), vec![n(3)]); for bad in [n(0),n(4),n(-4), \"1.5\".parse().unwrap()] { assert!(evaluate(input.clone(), bad).is_err(), \"{bad}\"); } assert!(evaluate(vec![], n(1)).is_err());",
    );
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "sublist(input, 2, position)",
        "let n = |v: i64| Number::from(v); let input = vec![n(1),n(2),n(3)]; assert_eq!(evaluate(input.clone(), n(0)).unwrap(), Vec::<Number>::new()); assert_eq!(evaluate(input.clone(), n(9)).unwrap(), vec![n(2),n(3)]); assert_eq!(evaluate(input.clone(), \"9999999999999999999999999999\".parse().unwrap()).unwrap(), vec![n(2),n(3)]); for bad in [n(-1), \"1.5\".parse().unwrap()] { assert!(evaluate(input.clone(), bad).is_err(), \"{bad}\"); }",
    );
}

#[test]
fn generated_sublist_contextually_types_empty_source_before_reporting_invalid_start() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "sublist([], 1)",
        "assert!(evaluate(vec![Number::ONE], Number::ZERO).is_err());",
    );
}

#[test]
fn generated_append_does_not_mutate_input_or_reorder_elements() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "append(input, 2, 3)",
        "let n = |v: i64| Number::from(v); let original = vec![n(1)]; assert_eq!(evaluate(original.clone(), n(0)).unwrap(), vec![n(1),n(2),n(3)]); assert_eq!(original, vec![n(1)]);",
    );
}

#[test]
fn generated_append_contextually_types_two_empty_nested_lists() {
    compile_list_operation(
        "List<Number>",
        "List<List<Number>>",
        "append([], [])",
        "assert_eq!(evaluate(vec![], Number::ZERO).unwrap(), vec![Vec::<Number>::new()]);",
    );
}

#[test]
fn generated_prepend_contextually_types_two_empty_nested_lists() {
    compile_list_operation(
        "List<Number>",
        "List<List<Number>>",
        "prepend([], [])",
        "assert_eq!(evaluate(vec![], Number::ZERO).unwrap(), vec![Vec::<Number>::new()]);",
    );
}

#[test]
fn generated_prepend_does_not_mutate_input_or_reorder_elements() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "prepend(input, 1, 2)",
        "let n = |v: i64| Number::from(v); let original = vec![n(3)]; assert_eq!(evaluate(original.clone(), n(0)).unwrap(), vec![n(1),n(2),n(3)]); assert_eq!(original, vec![n(3)]);",
    );
}

#[test]
fn generated_concatenate_preserves_argument_order() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "concatenate(input, [3], [4])",
        "let n = |v: i64| Number::from(v); let original = vec![n(1),n(2)]; assert_eq!(evaluate(original.clone(), n(0)).unwrap(), vec![n(1),n(2),n(3),n(4)]); assert_eq!(original, vec![n(1),n(2)]);",
    );
}

#[test]
fn generated_union_stably_removes_structural_duplicates() {
    compile_list_operation(
        "List<List<Number>>",
        "List<List<Number>>",
        "union(input, [[2,1], [3]])",
        "let n = |v: i64| Number::from(v); let original = vec![vec![n(1),n(2)],vec![n(2),n(1)]]; assert_eq!(evaluate(original.clone(), n(0)).unwrap(), vec![vec![n(1),n(2)],vec![n(2),n(1)],vec![n(3)]]); assert_eq!(original.len(), 2);",
    );
}

#[test]
fn generated_union_does_not_capture_result_iteration_binding() {
    compile_list_operation(
        "List<Number>",
        "List<List<Number>>",
        "for result in [1] return union([result], [result])",
        "assert_eq!(evaluate(vec![], Number::ZERO).unwrap(), vec![vec![Number::ONE]]);",
    );
}

#[test]
fn generated_union_deduplicates_dictionaries_regardless_of_key_insertion_order() {
    compile_list_operation(
        "List<Dictionary<Number>>",
        "List<Dictionary<Number>>",
        "union(input, [{y: 2, x: 1}, {x: 3, y: 4}])",
        "let first = [(String::from(\"x\"), Number::ONE), (String::from(\"y\"), Number::from(2))].into(); let distinct = [(String::from(\"x\"), Number::from(3)), (String::from(\"y\"), Number::from(4))].into(); let original = vec![first]; assert_eq!(evaluate(original.clone(), Number::ZERO).unwrap(), vec![original[0].clone(), distinct]); assert_eq!(original.len(), 1);",
    );
}

#[test]
fn generated_list_reverse_does_not_change_string_reverse() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "reverse(input)",
        "let n = |v: i64| Number::from(v); let original = vec![n(1),n(2),n(3)]; assert_eq!(evaluate(original.clone(), n(0)).unwrap(), vec![n(3),n(2),n(1)]); assert_eq!(original, vec![n(1),n(2),n(3)]);",
    );
}

#[test]
fn generated_flatten_recurses_through_uniformly_nested_lists() {
    compile_list_operation(
        "List<List<List<Number>>>",
        "List<Number>",
        "flatten(input)",
        "let n = |v: i64| Number::from(v); let original = vec![vec![vec![n(1),n(2)]],vec![vec![n(3),n(4)]]]; assert_eq!(evaluate(original.clone(), n(0)).unwrap(), vec![n(1),n(2),n(3),n(4)]); assert_eq!(original.len(), 2);",
    );
}

#[test]
fn generated_list_transformations_handle_empty_and_structural_values() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "append([], input[1])",
        "let n = |v: i64| Number::from(v); assert_eq!(evaluate(vec![n(4)], n(0)).unwrap(), vec![n(4)]);",
    );
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "prepend([], input[1])",
        "let n = |v: i64| Number::from(v); assert_eq!(evaluate(vec![n(4)], n(0)).unwrap(), vec![n(4)]);",
    );
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "concatenate([], input)",
        "let n = |v: i64| Number::from(v); assert_eq!(evaluate(vec![n(4)], n(0)).unwrap(), vec![n(4)]);",
    );
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "union([], input, [4,5])",
        "let n = |v: i64| Number::from(v); assert_eq!(evaluate(vec![n(4)], n(0)).unwrap(), vec![n(4),n(5)]);",
    );
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "reverse([])",
        "assert_eq!(evaluate(vec![Number::ONE], Number::ZERO).unwrap(), Vec::<Number>::new());",
    );
    compile_list_operation(
        "List<List<Number>>",
        "List<Number>",
        "flatten(input)",
        "let n = |v: i64| Number::from(v); assert_eq!(evaluate(vec![vec![n(1),n(2)],vec![n(3)]], n(0)).unwrap(), vec![n(1),n(2),n(3)]); assert_eq!(evaluate(vec![], n(0)).unwrap(), Vec::<Number>::new());",
    );
    compile_list_operation(
        "List<List<Number>>",
        "List<List<Number>>",
        "union(input, [[1,2], [3]])",
        "let n = |v: i64| Number::from(v); let input = vec![vec![n(1),n(2)]]; assert_eq!(evaluate(input.clone(), n(0)).unwrap(), vec![vec![n(1),n(2)],vec![n(3)]]); assert_eq!(input.len(), 1);",
    );
}

#[test]
fn generated_insert_before_spreads_or_wraps_nested_items() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "insertBefore(input, position, 5)",
        "let n = |v: i64| Number::from(v); let original = vec![n(10),n(20),n(30)]; assert_eq!(evaluate(original.clone(), n(1)).unwrap(), vec![n(5),n(10),n(20),n(30)]); assert_eq!(evaluate(original.clone(), n(2)).unwrap(), vec![n(10),n(5),n(20),n(30)]); assert_eq!(evaluate(original.clone(), n(-1)).unwrap(), vec![n(10),n(20),n(5),n(30)]); for bad in [n(0),n(4),n(-4), \"1.5\".parse().unwrap()] { assert!(evaluate(original.clone(), bad).is_err(), \"{bad}\"); } assert_eq!(original.len(), 3);",
    );
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "insertBefore(input, 2, [2,3])",
        "let n = |v: i64| Number::from(v); assert_eq!(evaluate(vec![n(1),n(4)], n(0)).unwrap(), vec![n(1),n(2),n(3),n(4)]);",
    );
    compile_list_operation(
        "List<List<Number>>",
        "List<List<Number>>",
        "insertBefore(input, 2, [[2,3]])",
        "let n = |v: i64| Number::from(v); assert_eq!(evaluate(vec![vec![n(1)],vec![n(4)]], n(0)).unwrap(), vec![vec![n(1)],vec![n(2),n(3)],vec![n(4)]]);",
    );
}

#[test]
fn generated_insert_before_infers_nested_empty_binding_as_spread() {
    compile_list_operation(
        "List<List<Number>>",
        "List<List<List<Number>>>",
        "for x in [] return insertBefore(input, 1, x)",
        "assert_eq!(evaluate(vec![vec![Number::ONE]], Number::ZERO).unwrap(), Vec::<Vec<Vec<Number>>>::new());",
    );
}

#[test]
fn generated_insert_after_spreads_items_and_checks_positions() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "insertAfter(input, 1, 5)",
        "let n = |v: i64| Number::from(v); assert_eq!(evaluate(vec![n(1),n(2)], n(0)).unwrap(), vec![n(1),n(5),n(2)]);",
    );
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "insertAfter(input, position, [2,3,4])",
        "let n = |v: i64| Number::from(v); let original = vec![n(1),n(5)]; assert_eq!(evaluate(original.clone(), n(1)).unwrap(), vec![n(1),n(2),n(3),n(4),n(5)]); assert_eq!(evaluate(original.clone(), n(-1)).unwrap(), vec![n(1),n(5),n(2),n(3),n(4)]); for bad in [n(0),n(3),n(-3), \"1.5\".parse().unwrap()] { assert!(evaluate(original.clone(), bad).is_err(), \"{bad}\"); } assert_eq!(original.len(), 2);",
    );
}

#[test]
fn generated_remove_by_position_checks_one_based_boundaries() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "remove(input, position)",
        "let n = |v: i64| Number::from(v); let original = vec![n(1),n(2),n(3)]; assert_eq!(evaluate(original.clone(), n(2)).unwrap(), vec![n(1),n(3)]); assert_eq!(evaluate(original.clone(), n(-1)).unwrap(), vec![n(1),n(2)]); for bad in [n(0),n(4),n(-4), \"1.5\".parse().unwrap()] { assert!(evaluate(original.clone(), bad).is_err(), \"{bad}\"); } assert_eq!(original.len(), 3);",
    );
}

#[test]
fn generated_list_replace_by_position_checks_one_based_boundaries() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "listReplace(input, position, 9)",
        "let n = |v: i64| Number::from(v); let original = vec![n(1),n(2),n(3)]; assert_eq!(evaluate(original.clone(), n(2)).unwrap(), vec![n(1),n(9),n(3)]); assert_eq!(evaluate(original.clone(), n(-1)).unwrap(), vec![n(1),n(2),n(9)]); for bad in [n(0),n(4),n(-4), \"1.5\".parse().unwrap()] { assert!(evaluate(original.clone(), bad).is_err(), \"{bad}\"); } assert_eq!(original.len(), 3);",
    );
}

#[test]
fn generated_positional_edits_accept_first_negative_and_reject_empty_positions() {
    for (expression, expected) in [
        ("insertBefore(input, position, 5)", "vec![n(5),n(1),n(2)]"),
        ("insertAfter(input, position, 5)", "vec![n(1),n(5),n(2)]"),
        ("remove(input, position)", "vec![n(2)]"),
        ("listReplace(input, position, 5)", "vec![n(5),n(2)]"),
    ] {
        compile_list_operation(
            "List<Number>",
            "List<Number>",
            expression,
            &format!(
                "let n = |v: i64| Number::from(v); assert_eq!(evaluate(vec![n(1),n(2)], n(-2)).unwrap(), {expected}); assert!(evaluate(vec![], n(1)).is_err()); assert!(evaluate(vec![], n(-1)).is_err());"
            ),
        );
    }
}

#[test]
fn generated_knowledge_remove_preserves_forward_entry_dependency() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task check { input input: List<Number>; output result: List<Number> = value;
knowledge position { input item: Number; output result: Number; expression item; }
knowledge remove { input items: List<Number>; input position: Number; output result: List<Number>; expression items; }
context value { output result: List<Number>; entry selected: List<Number> = remove(input, position(i)); entry i: Number = 2; result selected; } }"#,
        "assert_eq!(check(vec![Number::from(1), Number::from(2)]).unwrap(), vec![Number::from(1), Number::from(2)]);",
    );
}

#[test]
fn generated_remove_by_predicate_removes_every_match() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "remove(input, function(i) i == 2)",
        "let n = |v: i64| Number::from(v); let original = vec![n(1),n(2),n(3),n(2)]; assert_eq!(evaluate(original.clone(), n(0)).unwrap(), vec![n(1),n(3)]); assert_eq!(original.len(), 4);",
    );
}

#[test]
fn generated_list_replace_by_predicate_replaces_every_match() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "listReplace(input, function(i) i < 5, 5)",
        "let n = |v: i64| Number::from(v); let original = vec![n(2),n(4),n(7)]; assert_eq!(evaluate(original.clone(), n(0)).unwrap(), vec![n(5),n(5),n(7)]); assert_eq!(original, vec![n(2),n(4),n(7)]);",
    );
}

#[test]
fn generated_predicate_edits_propagate_checked_errors() {
    for expression in [
        "remove(input, function(i) 1 / (i - 2) > 0)",
        "listReplace(input, function(i) 1 / (i - 2) > 0, 9)",
    ] {
        compile_list_operation(
            "List<Number>",
            "List<Number>",
            expression,
            "let n = |v: i64| Number::from(v); assert!(evaluate(vec![n(1),n(2),n(3)], n(0)).is_err());",
        );
    }
}

#[test]
fn generated_predicate_edits_keep_outer_binding_and_nested_scopes() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "listReplace(input, function(position) count(remove([1,2], function(position) position == 2)) == position, position)",
        "let n = |v: i64| Number::from(v); let original = vec![n(1),n(2),n(1)]; assert_eq!(evaluate(original.clone(), n(7)).unwrap(), vec![n(7),n(2),n(7)]); assert_eq!(original, vec![n(1),n(2),n(1)]);",
    );
}

#[test]
fn generated_predicate_edits_type_empty_lists() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "remove([], function(i) i == position)",
        "assert!(evaluate(vec![Number::ONE], Number::ONE).unwrap().is_empty());",
    );
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "listReplace([], function(i) i == position, 9)",
        "assert!(evaluate(vec![Number::ONE], Number::ONE).unwrap().is_empty());",
    );
}

#[test]
fn generated_predicate_edits_avoid_internal_name_capture() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "remove(input, function(__bl_match_result) __bl_match_result == position)",
        "let n = |v: i64| Number::from(v); assert_eq!(evaluate(vec![n(1),n(2),n(1)], n(1)).unwrap(), vec![n(2)]);",
    );
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "listReplace(input, function(__bl_match_items) __bl_match_items == 2, position)",
        "let n = |v: i64| Number::from(v); assert_eq!(evaluate(vec![n(1),n(2),n(2)], n(7)).unwrap(), vec![n(1),n(7),n(7)]);",
    );
}

#[test]
fn generated_predicate_edit_preserves_forward_context_dependency() {
    compile_and_test(
        r#"namespace orders; version "1";
decision_task check { input input: List<Number>; output result: List<Number> = value;
context value { output result: List<Number>; entry selected: List<Number> = remove(input, function(item) item > threshold); entry threshold: Number = 2; result selected; } }"#,
        "let n = |v: i64| Number::from(v); assert_eq!(check(vec![n(1),n(3),n(2),n(4)]).unwrap(), vec![n(1),n(2)]);",
    );
}

#[test]
fn generated_distinct_retains_first_structural_occurrence() {
    compile_list_operation(
        "List<List<Number>>",
        "List<List<Number>>",
        "distinct(input)",
        "let n = |v: i64| Number::from(v); assert_eq!(evaluate(vec![vec![n(2)],vec![n(1)],vec![n(2)],vec![n(1)]], n(0)).unwrap(), vec![vec![n(2)],vec![n(1)]]);",
    );
}

#[test]
fn generated_duplicate_values_retains_first_duplicate_order() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "duplicateValues(input)",
        "let n = |v: i64| Number::from(v); assert_eq!(evaluate(vec![n(2),n(1),n(1),n(2),n(2)], n(0)).unwrap(), vec![n(2),n(1)]); assert_eq!(evaluate(vec![n(1)], n(0)).unwrap(), Vec::<Number>::new());",
    );
}

#[test]
fn generated_intersection_retains_first_list_order_once() {
    compile_list_operation(
        "List<Number>",
        "List<Number>",
        "intersection(input, [2,3,2], [2,4,3])",
        "let n = |v: i64| Number::from(v); let original = vec![n(3),n(2),n(3),n(1)]; assert_eq!(evaluate(original.clone(), n(0)).unwrap(), vec![n(3),n(2)]); assert_eq!(original.len(), 4);",
    );
}

#[test]
fn generated_set_functions_handle_empty_lists_and_repeated_nested_values() {
    compile_list_operation(
        "List<List<Number>>",
        "List<List<Number>>",
        "distinct(input)",
        "let n = |v: i64| Number::from(v); assert_eq!(evaluate(vec![], n(0)).unwrap(), Vec::<Vec<Number>>::new()); assert_eq!(evaluate(vec![vec![n(1)],vec![n(1)],vec![n(2)]], n(0)).unwrap(), vec![vec![n(1)],vec![n(2)]]);",
    );
    compile_list_operation(
        "List<List<Number>>",
        "List<List<Number>>",
        "duplicateValues(input)",
        "let n = |v: i64| Number::from(v); assert_eq!(evaluate(vec![], n(0)).unwrap(), Vec::<Vec<Number>>::new()); assert_eq!(evaluate(vec![vec![n(2)],vec![n(1)],vec![n(1)],vec![n(2)],vec![n(2)]], n(0)).unwrap(), vec![vec![n(2)],vec![n(1)]]);",
    );
    compile_list_operation(
        "List<List<Number>>",
        "List<List<Number>>",
        "intersection(input, [[2], [1]], [[1], [2]])",
        "let n = |v: i64| Number::from(v); let original = vec![vec![n(2)],vec![n(1)],vec![n(2)]]; assert_eq!(evaluate(original.clone(), n(0)).unwrap(), vec![vec![n(2)],vec![n(1)]]); assert_eq!(evaluate(vec![], n(0)).unwrap(), Vec::<Vec<Number>>::new()); assert_eq!(original.len(), 3);",
    );
}

#[test]
fn generated_set_functions_do_not_capture_iteration_bindings() {
    for expression in [
        "distinct([result, result])",
        "duplicateValues([result, result])",
        "intersection([result, result], [result])",
    ] {
        compile_list_operation(
            "List<Number>",
            "List<List<Number>>",
            &format!("for result in [1] return {expression}"),
            "assert_eq!(evaluate(vec![], Number::ZERO).unwrap(), vec![vec![Number::ONE]]);",
        );
    }
}

#[test]
fn generated_existing_string_overloads_survive_list_dispatch() {
    compile_and_test(
        r#"namespace strings; version "1";
decision_task position { input value: String; output result: Number = out; literal_expression out { output result: Number; expression indexOf(value, "b"); } }
decision_task backward { input value: String; output result: String = out; literal_expression out { output result: String; expression reverse(value); } }
decision_task empty { input value: String; output result: Bool = out; literal_expression out { output result: Bool; expression isEmpty(value); } }"#,
        "assert_eq!(position(\"abc\".into()).unwrap(), Number::from(2)); assert_eq!(backward(\"abc\".into()).unwrap(), \"cba\"); assert!(empty(\"\".into()).unwrap());",
    );
}

#[test]
fn generated_distinct_uses_structural_dictionary_equality() {
    compile_list_operation(
        "List<Dictionary<Number>>",
        "List<Dictionary<Number>>",
        "distinct(input)",
        "let a = [(String::from(\"x\"), Number::from(1))].into(); let b = [(String::from(\"x\"), Number::from(2))].into(); let original = vec![a, b, [(String::from(\"x\"), Number::from(1))].into()]; assert_eq!(evaluate(original.clone(), Number::ZERO).unwrap(), vec![original[0].clone(), original[1].clone()]); assert_eq!(original.len(), 3);",
    );
}

#[test]
fn generated_duplicate_values_uses_structural_dictionary_equality() {
    compile_list_operation(
        "List<Dictionary<Number>>",
        "List<Dictionary<Number>>",
        "duplicateValues(input)",
        "let a = [(String::from(\"x\"), Number::from(1))].into(); let b = [(String::from(\"x\"), Number::from(2))].into(); let original = vec![a, b, [(String::from(\"x\"), Number::from(1))].into(), [(String::from(\"x\"), Number::from(2))].into(), [(String::from(\"x\"), Number::from(1))].into()]; assert_eq!(evaluate(original.clone(), Number::ZERO).unwrap(), vec![original[0].clone(), original[1].clone()]); assert_eq!(original.len(), 5);",
    );
}

#[test]
fn generated_intersection_uses_structural_dictionary_equality() {
    compile_list_operation(
        "List<Dictionary<Number>>",
        "List<Dictionary<Number>>",
        "intersection(input, [{x: 2}, {x: 1}])",
        "let a = [(String::from(\"x\"), Number::from(1))].into(); let b = [(String::from(\"x\"), Number::from(2))].into(); let original = vec![a, b, [(String::from(\"x\"), Number::from(3))].into(), [(String::from(\"x\"), Number::from(1))].into()]; assert_eq!(evaluate(original.clone(), Number::ZERO).unwrap(), vec![original[0].clone(), original[1].clone()]); assert_eq!(original.len(), 4);",
    );
}

#[test]
fn generated_list_dictionary_fixture_compiles() {
    compile_list_operation(
        "List<Dictionary<Number>>",
        "List<Dictionary<Number>>",
        "input",
        "let original: Vec<std::collections::BTreeMap<String, Number>> = vec![[(String::from(\"x\"), Number::from(1))].into()]; assert_eq!(evaluate(original.clone(), Number::ZERO).unwrap(), original);",
    );
}
