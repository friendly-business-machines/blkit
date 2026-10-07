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
    let name = directory.file_name().unwrap().to_string_lossy();
    let root = env!("CARGO_MANIFEST_DIR").replace('\\', "\\\\");
    fs::write(directory.join("Cargo.toml"), format!("[package]\nname = {name:?}\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\nblkit = {{ path = {root:?} }}\nrust_decimal = \"1.39\"\nchrono = {{ version = \"0.4\", features = [\"serde\"] }}\nserde = \"1\"\nserde_json = \"1\"\n")).unwrap();
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
        source.push_str(&format!("decision_task {name}_values {{ input values: List<Number>; output result: Number = calc; literal_expression calc {{ output result: Number; expression {name}(values); }} }}\n"));
    }
    compile_and_test_graph(
        &source,
        r#"let n = |s: &str| -> Number { s.parse().unwrap() }; let list = |v: &[&str]| v.iter().map(|s| n(s)).collect(); assert_eq!(min_values(list(&["3","1","2"])).unwrap(), n("1")); assert_eq!(max_values(list(&["3","1","2"])).unwrap(), n("3")); assert_eq!(sum_values(list(&["1","2","3"])).unwrap(), n("6")); assert_eq!(mean_values(list(&["1","2","3"])).unwrap(), n("2")); assert_eq!(median_values(list(&["1","2","3","4"])).unwrap(), n("2.5")); assert_eq!(product_values(list(&["2","3","4"])).unwrap(), n("24")); assert_eq!(stddev_values(list(&["1","2","3"])).unwrap(), n("1")); assert_eq!(mode_values(list(&["3","2","3","2"])).unwrap(), n("2")); assert_eq!(sum_values(vec![]).unwrap(), Number::ZERO); assert_eq!(product_values(vec![]).unwrap(), Number::ONE); for result in [min_values(vec![]), max_values(vec![]), mean_values(vec![]), median_values(vec![]), mode_values(vec![]), stddev_values(vec![]), stddev_values(vec![n("5")]), sum_values(vec![Number::MAX, Number::ONE]), product_values(vec![Number::MAX, n("2")])] { assert!(result.is_err()); }"#,
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
fn generated_temporal_values_roundtrip_and_order() {
    compile_and_test(
        r#"namespace timing;
version "1";
type Clock:
  day: Date;
  time: Time;
  instant: DateTime;
decision_task later {
  input value: Clock;
  output result: Bool = checked;
  literal_expression checked { output result: Bool; expression value.day < date("2026-10-03") and value.time < time("12:00:00") and value.instant == dateTime("2026-10-02T08:30:00Z"); }
}
decision_task before {
  input opens: DateTime;
  input closes: DateTime;
  output result: Bool = checked;
  literal_expression checked { output result: Bool; expression opens < closes; }
}"#,
        r#"let day: Date = serde_json::from_value(serde_json::json!("2026-10-02")).unwrap(); let time: Time = serde_json::from_value(serde_json::json!("09:30:00.250")).unwrap(); let instant: DateTime = serde_json::from_value(serde_json::json!("2026-10-02T09:30:00+01:00")).unwrap(); assert!(later(Clock { day, time, instant }).unwrap()); assert_eq!(serde_json::to_value(day).unwrap(), serde_json::json!("2026-10-02")); assert_eq!(serde_json::to_value(time).unwrap(), serde_json::json!("09:30:00.250")); assert!(before("2026-10-02T09:30:00+02:00".parse().unwrap(), "2026-10-02T08:31:00+01:00".parse().unwrap()).unwrap()); for invalid in ["2026-02-30", "not-a-date", "2026-1-2"] { assert!(serde_json::from_value::<Date>(serde_json::json!(invalid)).is_err()); } for invalid in ["24:00:00", "09:30:00+02:00", "23:59:60", "9:30:00"] { assert!(serde_json::from_value::<Time>(serde_json::json!(invalid)).is_err(), "{invalid}"); } assert!(serde_json::from_value::<DateTime>(serde_json::json!("2026-10-02T09:30:00")).is_err());"#,
    );
}

#[test]
fn generated_ranges_obey_boundaries_and_dynamic_empty_semantics() {
    compile_and_test(
        r#"namespace ranges;
version "1";
type Bounds:
  low: Number;
  high: Number;
decision_task inclusive { input value: Number; output result: Bool = check; literal_expression check { output result: Bool; expression value in [1..5]; } }
decision_task exclusive { input value: Number; output result: Bool = check; literal_expression check { output result: Bool; expression value in (1..5); } }
decision_task upper_open { input value: Number; output result: Bool = check; literal_expression check { output result: Bool; expression value in [1..null); } }
decision_task dynamic { input bounds: Bounds; output result: Bool = check; literal_expression check { output result: Bool; expression bounds.low in [bounds.low..bounds.high]; } }
decision_task relations { input value: Number; output result: Bool = check; literal_expression check { output result: Bool; expression before([1..2], [3..4]) and meets([1..2], [2..3]) and overlaps([5..10], [1..6]) and includes([1..10], 5) and not (starts(1, (1..5])) and overlaps((null..5), [4..null)) and not (overlaps([value..1], [1..2])); } }
decision_task instant { input value: DateTime; output result: Bool = check; literal_expression check { output result: Bool; expression value in [dateTime("2026-10-02T09:00:00+01:00")..null); } }
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
fn generated_records_enums_and_decimal_literals_compile() {
    compile_and_test(
        r#"namespace orders;
version "1.0";
type Order:
  total: Number;
  tags: List<String>;
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
        include_str!("../examples/subprocess.bl"),
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
        include_str!("../examples/approve.bl"),
        "let graph = named_graph_definitions().remove(0); let input = serde_json::json!({\"input\":{\"total\":\"1250\",\"blocked\":false}}); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!(\"review\"));",
    );
}

#[test]
fn pricing_and_iteration_examples_compile_and_execute() {
    compile_and_test_graph(
        include_str!("../examples/pricing.bl"),
        "let graph = named_graph_definitions().remove(0); for (amount, expected) in [(\"200\", \"5\"), (\"10\", \"2\")] { let input = serde_json::json!({\"amount\":amount}); let mut state = graph.checkpoint(&input).unwrap(); assert_eq!(graph.run(&input, &mut state).unwrap(), serde_json::json!(expected)); }",
    );
    compile_and_test_graph(
        include_str!("../examples/iteration.bl"),
        "let graph = named_graph_definitions(); let gather = graph.iter().find(|item| item.name == \"gather\").unwrap(); let input = serde_json::json!({\"values\":[\"3\", \"1\"]}); let mut state = gather.checkpoint(&input).unwrap(); assert_eq!(gather.run(&input, &mut state).unwrap(), serde_json::json!([\"3\", \"1\"])); let repeat = graph.iter().find(|item| item.name == \"repeat_until_timeout\").unwrap(); assert_eq!(repeat.deadline.as_ref().unwrap().duration.as_secs(), 2);",
    );
}

#[test]
fn generated_decision_task_branches_with_typed_list_input() {
    compile_and_test(
        r#"namespace orders;
version "1.0";
type Order:
  total: Number;
  amounts: List<Number>;
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
    let root = env!("CARGO_MANIFEST_DIR").replace('\\', "\\\\");
    let name = directory.file_name().unwrap().to_string_lossy();
    fs::write(directory.join("Cargo.toml"), format!("[package]\nname = {name:?}\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\nblkit = {{ path = {root:?} }}\nrust_decimal = {{ version = \"1.39\", features = [\"serde-str\"] }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\nchrono = {{ version = \"0.4\", features = [\"serde\"] }}\ntokio = {{ version = \"1\", features = [\"macros\", \"rt-multi-thread\", \"time\"] }}\n")).unwrap();
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
fn generated_decision_task_receives_bound_start_port_and_returns_end_port() {
    compile_and_test_graph(
        include_str!("../examples/minimal.bl"),
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
type Quote:
  price: Number;
  tier: String;
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
        include_str!("../examples/pricing.bl"),
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
type Pair:
  left: Number;
  right: Number;
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
    let source = include_str!("../examples/minimal.bl");
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
    let source = include_str!("../examples/minimal.bl");
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
        include_str!("../examples/graph.bl"),
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
