use blkit::transpile;
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
    fs::write(directory.join("Cargo.toml"), "[package]\nname = \"blkit_generated_test\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\nrust_decimal = \"1.39\"\nchrono = { version = \"0.4\", features = [\"serde\"] }\nserde = \"1\"\nserde_json = \"1\"\n").unwrap();
    fs::write(directory.join("src/lib.rs"), format!("{generated}\n#[cfg(test)] mod generated_checks {{ use super::*; #[test] fn behavior() {{ {assertion} }} }}")).unwrap();
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
fn generated_string_functions_evaluate_typed_tasks_and_decisions() {
    compile_and_test_graph(
        r#"namespace strings
version "1"
task concat(input: String) -> String:
  return "order-" + input
task join(input: String) -> String:
  return stringJoin([input, upperCase(input)], ", ")
task member(input: String) -> Bool:
  return input in ["active", "pending"]
task last(input: String) -> String:
  return charAt(input, -1)
task slice(input: String) -> String:
  return substring(input, -1)
task length(input: String) -> Number:
  return stringLength(input)
task regex(input: String) -> Bool:
  return matches(input, input)
task extracts(input: String) -> List<List<String>>:
  return extract(input, "(a)(b)?")
task convert(input: Number) -> String:
  return "order-" + string(input)
task convert_date(input: Date) -> String:
  return string(input)
task convert_time(input: Time) -> String:
  return string(input)
task convert_instant(input: DateTime) -> String:
  return string(input)
decision inspect(input: String) -> String:
  node result: String = context
    entry shortened: String = substring(input, 1, 1)
    result shortened
  output result
decision judge(input: String) -> Bool:
  knowledge check(s: String) -> Bool = matches(s, "^a")
  node result: Bool = literal check(input)
  output result
"#,
        r#"assert_eq!(concat("123".into()), "order-123"); assert_eq!(join("ab".into()), "ab, AB"); assert!(member("active".into())); assert!(!member("ACTIVE".into())); assert_eq!(last("e\u{301}x".into()).unwrap(), "x"); assert_eq!(slice("e\u{301}x".into()).unwrap(), "x"); assert_eq!(length("e\u{301}x".into()), Number::from(2)); assert!(regex("[".into()).is_err()); assert_eq!(extracts("ab a".into()).unwrap(), vec![vec!["a", "b"], vec!["a"]]); assert_eq!(convert(Number::from(123)).unwrap(), "order-123"); assert_eq!(convert_date("2026-01-01".parse().unwrap()).unwrap(), "2026-01-01"); assert_eq!(convert_time(serde_json::from_value(serde_json::json!("12:30:00")).unwrap()).unwrap(), "12:30:00"); assert_eq!(convert_instant("2026-01-01T00:00:00+02:00".parse().unwrap()).unwrap(), "2026-01-01T00:00:00+02:00"); assert_eq!(inspect("e\u{301}x".into()).unwrap(), "e\u{301}"); assert!(inspect("".into()).is_err()); assert!(judge("abc".into()).unwrap()); assert!(judge("[".into()).unwrap() == false);"#,
    );
}

#[test]
fn generated_string_edges_and_decision_table_propagate_errors() {
    compile_and_test_graph(
        r#"namespace strings
version "1"
task trim_case(input: String) -> String:
  return upperCase(trimTrailing(trimLeading(input)))
task slice(input: String) -> String:
  return substring(input, 1, 2)
task prefix(input: String) -> String:
  return substringBefore(input, ":")
task suffix(input: String) -> String:
  return substringAfter(input, ":")
task pieces(input: String) -> List<String>:
  return split(input, [",", ";"])
task padded(input: String) -> String:
  return padTrailing(padLeading(input, 3, "x"), 5)
task repeated(input: String) -> String:
  return repeat(input, 2)
task replaced(input: String) -> String:
  return replace(input, "(a)", "x$1", "i")
task test_text(input: String) -> Bool:
  return contains(input, "a") and startsWith(input, "a") and endsWith(input, "b") and not isBlank(input) and not isEmpty(input)
task index(input: String) -> Number:
  return indexOf(input, "b")
task reverses(input: String) -> String:
  return reverse(lowerCase(input))
decision table_test(input: String) -> Bool:
  node result: Bool = table FIRST
    input value: String = input
    output ok: Bool
    rule matches(value, input) -> true
    default false
  output result
"#,
        r#"assert_eq!(trim_case("  é  ".into()), "É"); assert_eq!(slice("e\u{301}x".into()).unwrap(), "e\u{301}x"); assert!(slice("".into()).is_err()); assert_eq!(prefix("a:b".into()), "a"); assert_eq!(suffix("a:b".into()), "b"); assert_eq!(pieces("a,b;c".into()).unwrap(), ["a", "b", "c"]); assert_eq!(padded("a".into()).unwrap(), "xxa  "); assert_eq!(repeated("ab".into()).unwrap(), "abab"); assert_eq!(replaced("aA".into()).unwrap(), "xaxA"); assert!(test_text("ab".into())); assert_eq!(index("ab".into()), Number::from(2)); assert_eq!(reverses("Ab".into()), "ba"); assert!(table_test("abc".into()).unwrap()); assert!(table_test("[".into()).is_err());"#,
    );
}

#[test]
fn generated_datetime_inputs_require_offset_and_compare_instants() {
    compile_and_test(
        "namespace timing\nversion \"1\"\ntype Window:\n  opens: DateTime\n  closes: DateTime\ntask before(input: Window) -> Bool:\n  return input.opens < input.closes\n",
        "let opens: DateTime = serde_json::from_value(serde_json::json!(\"2026-10-02T09:30:00+02:00\")).unwrap(); let closes: DateTime = serde_json::from_value(serde_json::json!(\"2026-10-02T08:31:00+01:00\")).unwrap(); assert!(before(Window { opens, closes })); assert!(serde_json::from_value::<DateTime>(serde_json::json!(\"2026-10-02T09:30:00\")).is_err());",
    );
}

#[test]
fn generated_temporal_values_roundtrip_and_order() {
    compile_and_test(
        "namespace timing\nversion \"1\"\ntype Clock:\n  day: Date\n  time: Time\n  instant: DateTime\ntask day(input: Clock) -> Date:\n  return input.day\ntask time_of(input: Clock) -> Time:\n  return input.time\ntask later(input: Clock) -> Bool:\n  return input.day < date(\"2026-10-03\") and input.time < time(\"12:00:00\") and input.instant == dateTime(\"2026-10-02T08:30:00Z\")\n",
        "let day_value: Date = serde_json::from_value(serde_json::json!(\"2026-10-02\")).unwrap(); let time: Time = serde_json::from_value(serde_json::json!(\"09:30:00.250\")).unwrap(); let instant: DateTime = serde_json::from_value(serde_json::json!(\"2026-10-02T09:30:00+01:00\")).unwrap(); let input = Clock { day: day_value, time, instant }; assert_eq!(serde_json::to_value(day(input.clone())).unwrap(), serde_json::json!(\"2026-10-02\")); assert_eq!(serde_json::to_value(time_of(input.clone())).unwrap(), serde_json::json!(\"09:30:00.250\")); assert!(later(input)); for invalid in [\"2026-02-30\", \"not-a-date\", \"2026-1-2\"] { assert!(serde_json::from_value::<Date>(serde_json::json!(invalid)).is_err()); } for invalid in [\"24:00:00\", \"09:30:00+02:00\", \"23:59:60\", \"9:30:00\"] { assert!(serde_json::from_value::<Time>(serde_json::json!(invalid)).is_err(), \"{invalid}\"); } assert!(serde_json::from_value::<DateTime>(serde_json::json!(\"2026-10-02T09:30:00\")).is_err());",
    );
}

#[test]
fn generated_ranges_obey_boundaries_and_dynamic_empty_semantics() {
    compile_and_test(
        "namespace ranges\nversion \"1\"\ntype Bounds:\n  low: Number\n  high: Number\ntask inclusive(input: Number) -> Bool:\n  return input in [1..5]\ntask exclusive(input: Number) -> Bool:\n  return input in (1..5)\ntask left_open(input: Number) -> Bool:\n  return input in (1..5]\ntask right_open(input: Number) -> Bool:\n  return input in [1..5)\ntask upper_open(input: Number) -> Bool:\n  return input in [1..null)\ntask lower_open(input: Number) -> Bool:\n  return input in (null..0)\ntask all(input: Number) -> Bool:\n  return input in (null..null)\ntask between(input: Number) -> Bool:\n  return input between 1 and 5\ntask same(input: Number) -> Bool:\n  return [1..5] == [1..5] and [1..5] != [1..5)\ntask dynamic(input: Bounds) -> Bool:\n  return input.low in [input.low..input.high]\ntask singleton(input: Number) -> Bool:\n  return input in [1..1] and not (input in (1..1])\ntask instant(input: DateTime) -> Bool:\n  return input in [dateTime(\"2026-10-02T09:00:00+01:00\")..null)\n",
        "let n = |text: &str| text.parse::<Number>().unwrap(); assert!(inclusive(n(\"1\"))); assert!(inclusive(n(\"5\"))); assert!(!exclusive(n(\"1\"))); assert!(exclusive(n(\"3\"))); assert!(!exclusive(n(\"5\"))); assert!(!left_open(n(\"1\"))); assert!(left_open(n(\"5\"))); assert!(right_open(n(\"1\"))); assert!(!right_open(n(\"5\"))); assert!(upper_open(n(\"100\"))); assert!(lower_open(n(\"-1\"))); assert!(!lower_open(n(\"0\"))); assert!(all(n(\"0\"))); assert!(between(n(\"5\"))); assert!(same(n(\"0\"))); assert!(singleton(n(\"1\"))); assert!(!dynamic(Bounds { low: n(\"5\"), high: n(\"1\") })); assert!(instant(\"2026-10-02T08:30:00Z\".parse().unwrap()));",
    );
}

#[test]
fn generated_interval_relations_handle_open_empty_and_adjacent_dates() {
    compile_and_test(
        "namespace relations\nversion \"1\"\ntask ordered(input: Number) -> Bool:\n  return before([1..2], [3..4]) and after([3..4], [1..2]) and meets([1..2], [2..3]) and metBy([2..3], [1..2])\ntask intersection(input: Number) -> Bool:\n  return overlaps([5..10], [1..6]) and overlapsBefore([1..5], [4..10]) and overlapsAfter([4..10], [1..5]) and not (overlaps([1..2), [2..3])) and not (overlaps([input..1], [1..2]))\ntask points(input: Number) -> Bool:\n  return includes([1..10], 5) and during(5, [1..10]) and starts(1, [1..5]) and startedBy([1..5], 1) and finishes(5, [1..5]) and finishedBy([1..5], 5) and coincides([1..5], [1..5])\ntask open(input: Number) -> Bool:\n  return not (starts(1, (1..5])) and not (finishes(5, [1..5))) and not (starts(1, (null..5])) and overlaps((null..5), [4..null))\ntask adjacent(input: Date) -> Bool:\n  return not (overlaps((date(\"2026-01-01\")..date(\"2026-01-03\")), [date(\"2026-01-03\")..null))) and not (overlaps((date(\"2026-01-01\")..date(\"2026-01-02\")), [date(\"2026-01-01\")..date(\"2026-01-02\")]))\n",
        "assert!(ordered(Number::ZERO)); assert!(intersection(Number::from(5))); assert!(points(Number::ZERO)); assert!(open(Number::ZERO)); assert!(adjacent(\"2026-01-01\".parse().unwrap()));",
    );
}

#[test]
fn generated_table_unary_tests_preserve_outputs_and_hit_policies() {
    compile_and_test(
        "namespace pricing\nversion \"1\"\ntype Quote:\n  price: Number\n  tier: String\ndecision quotes(input: Number) -> List<Quote>:\n  node result: List<Quote> = table RULE_ORDER\n    input amount: Number = input\n    output price: Number\n    output tier: String\n    rule amount matches ([2..5], (2..5]) -> 1, \"range\"\n    rule amount matches (< 10, [20..30]) and amount > 3 -> 2, \"mixed\"\n    default 0, \"none\"\n  output result\n",
        "let n = |value: &str| value.parse::<Number>().unwrap(); let two = quotes(n(\"2\")).unwrap(); assert_eq!(two.len(), 1); assert_eq!(two[0].tier, \"range\"); let five = quotes(n(\"5\")).unwrap(); assert_eq!(five.len(), 2); assert_eq!(five[0].tier, \"range\"); assert_eq!(five[1].tier, \"mixed\"); assert_eq!(quotes(n(\"6\")).unwrap()[0].tier, \"mixed\"); assert_eq!(quotes(n(\"25\")).unwrap()[0].tier, \"mixed\"); assert_eq!(quotes(n(\"15\")).unwrap()[0].tier, \"none\");",
    );
    compile_and_test(
        "namespace dates\nversion \"1\"\ndecision in_season(input: Date) -> Bool:\n  node result: Bool = table FIRST\n    input day: Date = input\n    output approved: Bool\n    rule day matches ([date(\"2026-01-01\")..date(\"2026-01-31\")], >= date(\"2026-12-01\")) -> true\n    default false\n  output result\n",
        "for (date, expected) in [(\"2026-01-15\", true), (\"2026-12-01\", true), (\"2026-02-01\", false)] { assert_eq!(in_season(date.parse().unwrap()).unwrap(), expected); }",
    );
}

#[test]
fn knowledge_names_can_coexist_with_range_builtins_and_date_parameters() {
    compile_and_test(
        "namespace compatibility\nversion \"1\"\ndecision earlier(input: Number) -> Bool:\n  knowledge before(a: Number, b: Number) -> Bool = a < b\n  node result: Bool = literal before(input, 10)\n  output result\ndecision daylight(input: Number) -> Bool:\n  knowledge check(day: Date) -> Bool = day in [day..day]\n  knowledge datetime_check(at: DateTime) -> Bool = at == at\n  node result: Bool = literal true\n  output result\ndecision intervals(input: Number) -> Bool:\n  node result: Bool = literal before([1..2], [3..4])\n  output result\n",
        "assert!(earlier(Number::ONE).unwrap()); assert!(!earlier(Number::from(11)).unwrap()); assert!(daylight(Number::ZERO).unwrap()); assert!(intervals(Number::ZERO).unwrap());",
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
fn generated_subprocess_calls_registered_typed_child() {
    compile_and_test_graph(
        "namespace orders\nversion \"1\"\ntask four(input: Bool) -> Number:\n  return 4\nprocess child(input: Bool) -> Number:\n  node start = start\n  node work = task four(input)\n  node done = end\n  link start -> work\n  link work -> done(work)\nprocess parent(input: Bool) -> Number:\n  node start = start\n  node called = subprocess child(input)\n  node done = end\n  link start -> called\n  link called -> done(called)\n",
        "let mut definitions = named_graph_definitions(); assert_eq!(definitions.len(), 2); let parent = definitions.remove(1); assert!(blkit::runtime::Registry::new(vec![parent]).is_err()); let registry = blkit::runtime::Registry::new(named_graph_definitions()).unwrap(); let store = blkit::runtime::LocalStore::open(&std::env::temp_dir().join(format!(\"generated-child-{}.db\", std::process::id()))).await.unwrap(); let engine = blkit::runtime::Engine::new(registry, store, 1).unwrap(); let id = engine.start(\"orders\", \"1\", \"parent\", serde_json::json!(true)).await.unwrap(); let result = tokio::time::timeout(std::time::Duration::from_secs(5), async { loop { let status = engine.status(&id).await.unwrap().unwrap(); if status.status != \"pending\" && status.status != \"running\" { break status; } tokio::time::sleep(std::time::Duration::from_millis(10)).await; } }).await.unwrap(); assert_eq!(result.result, Some(serde_json::json!(\"4\")));",
    );
}

#[test]
fn documented_subprocess_example_routes_success_and_each_child_outcome() {
    compile_and_test_graph(
        include_str!("../examples/subprocess.bl"),
        "let store = blkit::runtime::LocalStore::open(&std::env::temp_dir().join(format!(\"documented-subprocess-{}.db\", std::process::id()))).await.unwrap(); let engine = blkit::runtime::Engine::new(blkit::runtime::Registry::new(named_graph_definitions()).unwrap(), store, 1).unwrap(); for (input, expected) in [(\"2\", \"2\"), (\"0\", \"100\"), (\"-1\", \"200\"), (\"11\", \"300\")] { let id = engine.start(\"orders\", \"1.0\", \"parent\", serde_json::json!(input)).await.unwrap(); let status = tokio::time::timeout(std::time::Duration::from_secs(5), async { loop { let item = engine.status(&id).await.unwrap().unwrap(); if matches!(item.status.as_str(), \"completed\" | \"failed\" | \"business-error\") { break item; } tokio::time::sleep(std::time::Duration::from_millis(10)).await; } }).await.unwrap(); assert_eq!(status.status, \"completed\", \"{input}: {:?}\", status.error); assert_eq!(status.result, Some(serde_json::json!(expected))); }",
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
fn pricing_and_iteration_examples_compile_and_execute() {
    compile_and_test_graph(
        include_str!("../examples/pricing.bl"),
        "let graph = named_graph_definitions().remove(0); for (amount, expected) in [(\"200\", \"5\"), (\"10\", \"2\")] { let input = serde_json::json!(amount); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!(expected)); }",
    );
    compile_and_test_graph(
        include_str!("../examples/iteration.bl"),
        "let graph = named_graph_definitions(); let gather = graph.iter().find(|item| item.name == \"gather\").unwrap(); let input = serde_json::json!([\"3\", \"1\"]); let mut state = gather.checkpoint(&input).unwrap(); assert_eq!(gather.run(&input, &mut state).unwrap(), input); let repeat = graph.iter().find(|item| item.name == \"repeat_until_timeout\").unwrap(); assert_eq!(repeat.deadline.as_ref().unwrap().duration.as_secs(), 2);",
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
    fs::write(directory.join("Cargo.toml"), format!("[package]\nname = \"blkit_generated_graph\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\nblkit = {{ path = {root:?} }}\nrust_decimal = {{ version = \"1.39\", features = [\"serde-str\"] }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\nchrono = {{ version = \"0.4\", features = [\"serde\"] }}\ntokio = {{ version = \"1\", features = [\"macros\", \"rt-multi-thread\", \"time\"] }}\n")).unwrap();
    fs::write(directory.join("src/lib.rs"), format!("{generated}\n#[cfg(test)] mod checks {{ use super::*; #[tokio::test] async fn graph() {{ {assertion} }} }}")).unwrap();
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
fn string_errors_propagate_through_named_graph_tasks_and_routes() {
    compile_and_test_graph(
        r#"namespace string_graph
version "1"
type Request:
  text: String
  pattern: String
task verify(input: Request) -> Bool:
  return matches(input.text, input.pattern)
process execute(input: Request) -> Bool:
  node start = start
  node checked = task verify(input)
  node done = end
  link start -> checked
  link checked -> done(checked)
process route(input: Request) -> Bool:
  node start = start
  node choice = xor_split
  node yes = task verify(input)
  node no = task verify(input)
  node join = xor_join(choice)
  node done = end
  link start -> choice
  link choice -> yes when matches(input.text, input.pattern)
  link choice -> no else
  link yes -> join(yes)
  link no -> join(no)
  link join -> done(join)
"#,
        r#"let graphs = named_graph_definitions(); for name in ["execute", "route"] { let graph = graphs.iter().find(|g| g.name == name).unwrap(); let good = serde_json::json!({"text":"abc", "pattern":"b"}); let mut state = graph.checkpoint(&good).unwrap(); assert_eq!(graph.run(&good, &mut state).unwrap(), serde_json::json!(true)); let bad = serde_json::json!({"text":"abc", "pattern":"["}); assert!(graph.checkpoint(&bad).and_then(|mut state| graph.run(&bad, &mut state)).is_err(), "{name} swallowed invalid regex"); }"#,
    );
}

#[test]
fn string_errors_propagate_through_multi_instance_and_task_loops() {
    compile_and_test_graph(
        r#"namespace string_graph
version "1"
task initial(input: String) -> String:
  return charAt(input, 1)
process batch(input: List<String>) -> List<String>:
  node start = start
  node many = task initial each input sequential
  node done = end
  link start -> many
  link many -> done(many)
process cycle(input: String) -> String:
  node start = start
  node one = task initial(input) repeat_post(one == "") max_iterations 2
  node done = end
  link start -> one
  link one -> done(one)
"#,
        r#"let graphs = named_graph_definitions(); for (name, good, bad, result) in [("batch", serde_json::json!(["ab", "cd"]), serde_json::json!(["ab", ""]), serde_json::json!(["a", "c"])), ("cycle", serde_json::json!("ab"), serde_json::json!(""), serde_json::json!("a"))] { let graph = graphs.iter().find(|g| g.name == name).unwrap(); let mut state = graph.checkpoint(&good).unwrap(); assert_eq!(graph.run(&good, &mut state).unwrap(), result); assert!(graph.checkpoint(&bad).and_then(|mut state| graph.run(&bad, &mut state)).is_err(), "{name} swallowed invalid position"); }"#,
    );
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
        "let graph = named_graph_definitions().remove(0); assert!(graph.retry.is_none()); assert!(graph.nodes.iter().any(|node| matches!(node.kind, blkit::compiled_graph::GraphNodeKind::Error))); assert!(graph.nodes.iter().any(|node| matches!(node.kind, blkit::compiled_graph::GraphNodeKind::Cancel))); assert!(graph.nodes.iter().any(|node| matches!(node.kind, blkit::compiled_graph::GraphNodeKind::Terminate))); let input = serde_json::json!(\"2\"); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!(\"2\"));",
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
