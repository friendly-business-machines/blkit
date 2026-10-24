use blkit_transpiler::{parse, transpile, validate};

const SOURCE: &str = r#"namespace orders; version "1";
Order = {id: Number, price: Number?};
decision_task echo { input order: Order; output result: Order = value; literal_expression value { output result: Order; expression order; } }
decision_task price { input order: Order; output result: Number? = value; literal_expression value { output result: Number?; expression order.price; } }
"#;

#[test]
fn nullable_declarations_preserve_required_field_types() {
    let program = parse(SOURCE).unwrap();
    assert_eq!(program.records[0].fields[1].1.to_string(), "Number?");
    validate(&program).unwrap();
    assert!(transpile(SOURCE).is_ok());
    let nonnull = SOURCE.replace("output result: Number?", "output result: Number");
    assert!(
        transpile(&nonnull).is_err(),
        "nullable field cannot satisfy non-null output"
    );
    let arithmetic = SOURCE.replace("expression order.price", "expression order.price + 1");
    assert!(transpile(&arithmetic).is_ok());
    assert!(
        transpile(&arithmetic.replace("output result: Number?", "output result: Number")).is_err()
    );
}

#[test]
fn nullable_field_accepts_a_nonnull_literal_without_widening_a_nonnull_field() {
    let source = SOURCE.replace("expression order;", "expression {id: 1, price: 20};");
    assert!(
        transpile(&source).is_ok(),
        "non-null Number is valid for Number?"
    );
    let wrong = SOURCE.replace("expression order;", "expression {id: 1, price: true};");
    assert!(transpile(&wrong).is_err());
}

#[test]
fn nullable_named_field_accepts_nonnull_nested_dictionary() {
    let source = r#"namespace orders; version "1";
Detail = {amount: Number};
Order = {detail: Detail?};
decision_task make { input amount: Number; output result: Order = value; literal_expression value { output result: Order; expression {detail: {amount: amount}}; } }"#;
    transpile(source).unwrap();
}

#[test]
fn nullable_syntax_does_not_enable_authored_null() {
    for expression in ["null", "[null]"] {
        let source = SOURCE.replace(
            "expression order.price",
            &format!("expression {expression}"),
        );
        assert!(transpile(&source).is_err(), "accepted {expression}");
    }
}

#[test]
fn nullable_by_value_record_cycles_are_rejected() {
    for schema in [
        "Node = {next: Node?};",
        "A = {b: B?}; B = {a: A?};",
        "A = {b: B}; B = {a: A?};",
    ] {
        let source = format!("namespace cycles; version \"1\"; {schema}");
        let error = validate(&parse(&source).unwrap()).unwrap_err();
        assert!(error.contains("recursive by-value record"), "{error}");
    }
    transpile("namespace cycles; version \"1\"; Node = {next: List<Node>?};").unwrap();
}
