use blkit_transpiler::transpile;

fn task(name: &str, input: &str, output: &str, expression: &str) -> String {
    format!(
        "namespace tables; version \"1\"; Rate = {{rate: Number}}; decision_task {name} {{ input data: {input}; output result: {output} = value; literal_expression value {{ output result: {output}; expression {expression}; }} }}"
    )
}

#[test]
fn table_rows_type_check_valid_selection_projection_and_equality() {
    for (output, expression) in [
        ("Table<Rate>", "data[1]"),
        ("Table<Rate>", "data[rate > 2, [rate]]"),
        ("Table<Rate>", "data[[3, 1:2], rate]"),
        ("Table<Rate>", "data[, rate]"),
        ("List<Number>", "data.rate"),
        ("Number", "data[1, rate].toValue()"),
        ("Number", "data.nRows"),
        ("Number", "data.nCols"),
        ("List<String>", "data.colNames"),
        ("Bool", "hasColumn(data, \"rate\")"),
        ("Bool", "isEmpty(data)"),
        ("Bool", "data = data"),
        ("Bool", "data != data"),
        ("List<Number>", "for r in data return r.rate"),
        ("Bool", "some r in data satisfies r.rate > 2"),
        ("Bool", "every r in data satisfies r.rate > 2"),
        ("List<Number>", "data.toList()"),
        ("Dictionary", "data[1].toDict()"),
    ] {
        transpile(&task("valid", "Table<Rate>", output, expression))
            .unwrap_or_else(|error| panic!("{expression}: {error}"));
    }
}

#[test]
fn table_rows_reject_invalid_selectors_and_scoped_names() {
    for expression in [
        "data[\"rate\"]",
        "data[1, 2]",
        "data[]",
        "data[1, rate, rate]",
        "data[0]",
        "data[1.5]",
        "data[rate + 1]",
    ] {
        assert!(
            transpile(&task("invalid", "Table<Rate>", "Table<Rate>", expression)).is_err(),
            "{expression}"
        );
    }
    assert!(transpile(&task("invalid", "Number", "Bool", "data = data")).is_err());
    assert!(
        transpile(&task(
            "invalid",
            "Number",
            "Table<Rate>",
            "table([\"rate\"], [1])[missing > 1]"
        ))
        .is_err()
    );
    assert!(
        transpile(&task(
            "invalid",
            "Number",
            "Table<Rate>",
            "table([\"rate\"], [1])[1, missing]"
        ))
        .is_err()
    );
    assert!(transpile(&task("invalid", "Dictionary", "Bool", "data = data")).is_err());
    assert!(transpile(&task("invalid", "Table<Rate>", "Table<Rate>", "data[null]")).is_err());
}

#[test]
fn direct_table_expressions_require_known_names_and_infer_cell_types() {
    for (output, expression) in [
        ("Table<Rate>", "table([\"rate\"], [1])[1, rate]"),
        ("Table<Rate>", "table([\"rate\"])[, rate]"),
        ("List<Number>", "table([\"rate\"], [1]).rate"),
        ("List<Number>", "tableFromDicts([{rate: 1}]).rate"),
        ("List<Number>", "table([\"rate\"]).rate"),
    ] {
        transpile(&task("valid", "Number", output, expression))
            .unwrap_or_else(|error| panic!("{expression}: {error}"));
    }
    for (output, expression) in [
        ("List<Number>", "table([\"rate\"], [\"wrong\"]).rate"),
        ("List<Number>", "table([\"rate\"], [1], [\"wrong\"]).rate"),
        (
            "List<Number>",
            "tableFromDicts([{rate: 1}, {rate: \"wrong\"}]).rate",
        ),
        (
            "List<Number>",
            "tableFromDicts([{rate: 1}, {other: 2}]).rate",
        ),
        ("List<Number>", "table([\"rate\"], [1]).missing"),
        ("List<Number>", "table(data).rate"),
    ] {
        assert!(
            transpile(&task("invalid", "List<String>", output, expression)).is_err(),
            "{expression}"
        );
    }
}

#[test]
fn grouped_table_aggregation_requires_valid_keys_outputs_and_group_scope() {
    let source = r#"namespace tables; version "1";
Rate = {region: String, rate: Number?};
Summary = {region: String, total: Number, average: Number?, n: Number};
decision_task summary { input data: Table<Rate>; input adjustment: Number; output result: Table<Summary> = value; literal_expression value { output result: Table<Summary>; expression data.groupBy(region).agg(total = sum(rate) + var(adjustment), average = mean(rate), n = count()); } }
"#;
    transpile(source).unwrap_or_else(|error| panic!("valid grouped aggregation: {error}"));
    for invalid in [
        "data.groupBy().agg(total = sum(rate), average = mean(rate), n = count())",
        "data.groupBy(missing).agg(total = sum(rate), average = mean(rate), n = count())",
        "data.groupBy(region)",
        "data.agg(total = count())",
        "data.groupBy(region).agg(region = sum(rate), average = mean(rate), n = count())",
        "data.groupBy(region).agg(total = sum(missing), average = mean(rate), n = count())",
        "data.groupBy(region).agg(total = count(), total = mean(rate), n = count())",
        "data.groupBy(region).agg()",
    ] {
        assert!(transpile(&source.replace("data.groupBy(region).agg(total = sum(rate) + var(adjustment), average = mean(rate), n = count())", invalid)).is_err(), "{invalid}");
    }
    assert!(transpile(&task("invalid", "Number", "Number", "count()")).is_err());
}

#[test]
fn table_context_aliases_keep_declared_row_schema() {
    let source = "namespace tables; version \"1\"; Rate = {rate: Number}; decision_task alias { input data: Table<Rate>; output result: Table<Rate> = value; context value { output result: Table<Rate>; entry selected: Table<Rate> = data[1]; result selected[, rate]; } }";
    assert!(transpile(source).is_ok());
    assert!(
        transpile(&source.replace("selected[, rate]", "selected[, missing]"))
            .unwrap_err()
            .contains("unknown table column")
    );
}

#[test]
fn table_row_equality_is_only_valid_in_table_iteration() {
    let valid = "namespace tables; version \"1\"; Rate = {rate: Number}; decision_task contains { input data: Table<Rate>; input row: Dictionary; output result: Bool = value; literal_expression value { output result: Bool; expression some r in data satisfies r = row; } }";
    assert!(transpile(valid).is_ok());
    let invalid = task("invalid", "Dictionary", "Bool", "data = data");
    assert!(transpile(&invalid).is_err());
}

#[test]
fn table_conversions_reject_wrong_known_column_types() {
    for (output, expression) in [
        ("String", "data[1, rate].toValue()"),
        ("List<String>", "data[, rate].toList()"),
    ] {
        let source = task("invalid", "Table<Rate>", output, expression);
        assert!(
            transpile(&source).is_err(),
            "accepted {expression} as {output}"
        );
    }
}

#[test]
fn table_conversions_preserve_nullable_column_types() {
    for (output, expression) in [
        ("Number", "data[1, rate].toValue()"),
        ("List<Number>", "data[, rate].toList()"),
    ] {
        let source = task("invalid", "Table<Rate>", output, expression)
            .replace("rate: Number}", "rate: Number?}");
        assert!(
            transpile(&source).is_err(),
            "accepted {expression} as {output}"
        );
    }
}

#[test]
fn table_conversions_infer_nullable_values_for_coalesce_and_aggregates() {
    for (output, expression) in [
        ("Number?", "data[1, rate].toValue()"),
        ("List<Number?>", "data[, rate].toList()"),
        ("Number", "coalesce(data[1, rate].toValue(), 0)"),
        ("Number", "sum(data[, rate].toList())"),
    ] {
        let source = task("valid", "Table<Rate>", output, expression)
            .replace("rate: Number}", "rate: Number?}");
        transpile(&source).unwrap_or_else(|error| panic!("{expression}: {error}"));
    }
}

#[test]
fn table_to_list_rejects_wrong_known_column_type() {
    assert!(
        transpile(&task(
            "invalid",
            "Table<Rate>",
            "List<String>",
            "data[, rate].toList()"
        ))
        .is_err()
    );
}

#[test]
fn table_to_list_rejects_nullable_to_nonnull_elements() {
    let source = task(
        "invalid",
        "Table<Rate>",
        "List<Number>",
        "data[, rate].toList()",
    )
    .replace("rate: Number}", "rate: Number?}");
    assert!(transpile(&source).is_err());
}

#[test]
fn global_table_join_tracks_forward_node_dependencies_not_key_names() {
    let source = r#"namespace tables; version "1"; Rate = {rate: Number};
decision_task joined { input data: Table<Rate>; output result: Table<Rate> = value;
literal_expression value { output result: Table<Rate>; expression join(left.result, right.result, rate); }
literal_expression left { output result: Table<Rate>; expression data; }
literal_expression right { output result: Table<Rate>; expression data; }
literal_expression rate { output result: Number; expression 1; }
}"#;
    let program = blkit_transpiler::parse(source).unwrap();
    assert_eq!(
        program.decisions[0].links,
        [
            ("left".into(), "value".into()),
            ("right".into(), "value".into())
        ]
    );
    transpile(source).unwrap();
}

#[test]
fn ranked_sort_tracks_forward_node_dependencies_not_column_names() {
    let source = r#"namespace tables; version "1"; Rate = {rate: Number};
decision_task ranked { input data: Table<Rate>; output result: Table<Rate> = value;
literal_expression value { output result: Table<Rate>; expression data.sort(inOrder(rate, var(order))); }
literal_expression order { output result: List<Number>; expression [2, 1]; }
literal_expression rate { output result: Number; expression 1; }
}"#;
    let program = blkit_transpiler::parse(source).unwrap();
    assert_eq!(
        program.decisions[0].links,
        [("order".into(), "value".into())]
    );
    transpile(source).unwrap();
}

#[test]
fn table_join_and_rank_expressions_sort_forward_context_entries() {
    for (entries, result) in [
        (
            "entry joined: Table<Rate> = join(left, right, rate); entry left: Table<Rate> = data; entry right: Table<Rate> = data;",
            "joined",
        ),
        (
            "entry ranked: Table<Rate> = data.sort(inOrder(rate, var(order))); entry order: List<Number> = [2, 1];",
            "ranked",
        ),
    ] {
        let source = format!(
            r#"namespace tables; version "1"; Rate = {{rate: Number}}; decision_task check {{ input data: Table<Rate>; output result: Table<Rate> = value; context value {{ output result: Table<Rate>; {entries} result {result}; }} }}"#
        );
        transpile(&source).unwrap_or_else(|error| panic!("{entries}: {error}"));
    }
}

#[test]
fn table_join_and_rank_calls_keep_knowledge_capture_dependencies() {
    for expression in [
        "join(data, table([\"rate\"], [getRate()]), rate)",
        "data.sort(inOrder(rate, [getRate()]))",
    ] {
        let source = format!(
            r#"namespace tables; version "1"; Rate = {{rate: Number}};
decision_task captured {{ input data: Table<Rate>; output result: Table<Rate> = value;
knowledge getRate {{ output result: Number; expression captured; }}
literal_expression value {{ output result: Table<Rate>; expression {expression}; }}
literal_expression captured {{ output result: Number; expression 1; }} }}"#
        );
        let program = blkit_transpiler::parse(&source).unwrap();
        assert_eq!(
            program.decisions[0].links,
            [("captured".into(), "value".into())],
            "{expression}"
        );
        transpile(&source).unwrap_or_else(|error| panic!("{expression}: {error}"));
    }
}

#[test]
fn ranked_sort_knowledge_cycles_are_rejected() {
    let source = r#"namespace tables; version "1"; Rate = {rate: Number};
decision_task check { input data: Table<Rate>; output result: List<Number> = value;
knowledge ranks { output result: List<Number>; expression data.sort(inOrder(rate, ranks())).rate; }
literal_expression value { output result: List<Number>; expression ranks(); } }"#;
    let program = blkit_transpiler::parse(source).unwrap();
    let error = blkit_transpiler::validate(&program).unwrap_err();
    assert!(error.contains("knowledge cycle"), "{error}");
}

#[test]
fn table_selection_deduplicates_first_occurrences_in_static_schema() {
    for expression in [
        "data[, [region, rate, region, rate]]",
        "data.select(region, rate, region, rate)",
        "data.select(region, rate, region).distinct()",
    ] {
        let source = format!(
            r#"namespace tables; version "1";
Row = {{rate: Number, region: String}}; Selected = {{region: String, rate: Number}};
decision_task check {{ input data: Table<Row>; output result: Table<Selected> = value;
literal_expression value {{ output result: Table<Selected>; expression {expression}; }} }}"#
        );
        transpile(&source).unwrap_or_else(|error| panic!("{expression}: {error}"));
        assert!(
            transpile(&source.replace(
                "Selected = {region: String, rate: Number}",
                "Selected = {rate: Number, region: String}"
            ))
            .is_err(),
            "selection order must be preserved"
        );
    }
}

#[test]
fn deduplicated_one_column_selection_infers_scalar_conversion() {
    for expression in [
        "data[, [rate, rate]].toList()",
        "data.select(rate, rate).toList()",
    ] {
        transpile(&task("valid", "Table<Rate>", "List<Number>", expression))
            .unwrap_or_else(|error| panic!("{expression}: {error}"));
    }
}

#[test]
fn dictionary_valued_row_expressions_lower_nested_columns_and_outer_variables() {
    let source = r#"namespace tables; version "1";
Rate = {rate: Number}; Payload = {rate: Number, payload: Dictionary};
decision_task check { input data: Table<Rate>; input adjustment: Number; output result: Table<Payload> = value;
literal_expression value { output result: Table<Payload>; expression data.withColumn(payload = {amount: rate, nested: {adjusted: rate + var(adjustment)}, values: [rate]}); } }"#;
    transpile(source).unwrap();
    assert!(transpile(&source.replace("amount: rate", "amount: missing")).is_err());
    assert!(transpile(&source.replace("var(adjustment)", "adjustment")).is_err());
}

#[test]
fn table_dependency_walkers_preserve_iteration_binding_scope() {
    for expression in [
        "for left in [data] return join(left, data, rate).nRows",
        "for order in [[1, 2]] return data.sort(inOrder(rate, var(order))).nRows",
    ] {
        let source = format!(
            r#"namespace tables; version "1"; Rate = {{rate: Number}};
decision_task scoped {{ input data: Table<Rate>; output result: List<Number> = value;
literal_expression value {{ output result: List<Number>; expression {expression}; }}
literal_expression left {{ output result: Table<Rate>; expression data; }}
literal_expression order {{ output result: List<Number>; expression [1]; }} }}"#
        );
        let program = blkit_transpiler::parse(&source).unwrap();
        assert!(program.decisions[0].links.is_empty(), "{expression}");
        transpile(&source).unwrap_or_else(|error| panic!("{expression}: {error}"));
    }
}

#[test]
fn global_join_knowledge_cycles_are_rejected() {
    let source = r#"namespace tables; version "1"; Rate = {rate: Number};
decision_task check { input data: Table<Rate>; output result: Number = value;
knowledge rows { output result: Number; expression join(data, table(["rate"], [rows()]), rate).nRows; }
literal_expression value { output result: Number; expression rows(); } }"#;
    let program = blkit_transpiler::parse(source).unwrap();
    let error = blkit_transpiler::validate(&program).unwrap_err();
    assert!(error.contains("knowledge cycle"), "{error}");
}

#[test]
fn positional_table_lists_and_ranges_reject_current_row_columns() {
    for expression in [
        "data[[rate]]",
        "data[[rate + 1]]",
        "data[rate:2]",
        "data[1:rate]",
        "data[[1, rate:2]]",
        "data[[1, 2:rate]]",
        "data[[abs(rate)]]",
        "data[[{n: rate}.n]]",
        "data[{n: rate}.n:2]",
    ] {
        let source = task("invalid", "Table<Rate>", "Table<Rate>", expression);
        let program = blkit_transpiler::parse(&source).unwrap();
        assert!(
            blkit_transpiler::validate(&program).is_err(),
            "{expression}"
        );
    }
}

#[test]
fn positional_table_selectors_keep_outer_bounds_and_boolean_predicates() {
    for expression in [
        "data[[var(rate), var(rate):2]]",
        "data[var(rate):var(rate)]",
        "data[rate > var(rate)]",
    ] {
        let source = task("valid", "Table<Rate>", "Table<Rate>", expression).replace(
            "input data: Table<Rate>;",
            "input data: Table<Rate>; input rate: Number;",
        );
        transpile(&source).unwrap_or_else(|error| panic!("{expression}: {error}"));
        let nullable = source.replace("rate: Number}", "rate: Number?}");
        transpile(&nullable).unwrap_or_else(|error| panic!("nullable {expression}: {error}"));
    }
}

#[test]
fn structural_table_comparisons_do_not_require_named_row_identity_or_order() {
    let mut errors = Vec::new();
    for (right_row, right_expression) in [
        ("{y: String, x: Number}", "right"),
        ("{different: Bool}", "right"),
        ("{y: String, x: Number}", "table([\"other\"], [true])"),
    ] {
        for operator in ["=", "!="] {
            let source = format!(
                r#"namespace tables; version "1"; A = {{x: Number, y: String}}; B = {right_row};
decision_task check {{ input left: Table<A>; input right: Table<B>; output result: Bool = value;
literal_expression value {{ output result: Bool; expression left {operator} {right_expression}; }} }}"#
            );
            let program = blkit_transpiler::parse(&source).unwrap();
            if let Err(error) = blkit_transpiler::validate(&program) {
                errors.push(format!(
                    "{operator} {right_expression} with {right_row}: {error}"
                ));
            }
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn table_comparison_changes_preserve_non_table_and_iteration_equality_rules() {
    for expression in ["data = 1", "data != 1", "1 = data", "1 != data"] {
        let source = task("invalid", "Table<Rate>", "Bool", expression);
        let program = blkit_transpiler::parse(&source).unwrap();
        assert!(
            blkit_transpiler::validate(&program).is_err(),
            "{expression}"
        );
    }
    for (input, expression) in [
        ("Number", "data == 1"),
        ("Number", "data != 1"),
        ("Number", "[] != [1]"),
        ("Number", "[1] != []"),
        ("Dictionary", "data == data"),
    ] {
        transpile(&task("valid", input, "Bool", expression)).unwrap();
    }
    let source = r#"namespace tables; version "1"; A = {x: Number}; B = {x: Number};
decision_task check { input left: A; input right: B; output result: Bool = value;
literal_expression value { output result: Bool; expression left == right; } }"#;
    assert!(
        transpile(source).is_err(),
        "ordinary named record equality must retain identity"
    );
}

#[test]
fn dynamic_positional_table_rows_infer_every_header_from_list_elements() {
    for (element, fields, output, expression) in [
        (
            "Number",
            "a: Number, b: Number",
            "Table<Pair>",
            "table([\"a\", \"b\"], values)",
        ),
        (
            "Number",
            "a: Number?, b: Number?",
            "Table<Pair>",
            "table([\"a\", \"b\"], values)",
        ),
        (
            "Number?",
            "a: Number?, b: Number?",
            "Table<Pair>",
            "table([\"a\", \"b\"], values)",
        ),
        (
            "Number",
            "a: Number, b: Number",
            "List<Number>",
            "table([\"a\", \"b\"], values).b",
        ),
        (
            "Number?",
            "a: Number?, b: Number?",
            "List<Number?>",
            "table([\"a\", \"b\"], values).a",
        ),
        (
            "Number",
            "a: Number, b: Number",
            "List<Number>",
            "table([\"a\", \"b\"], values, [1, 2]).b",
        ),
    ] {
        let source = format!(
            r#"namespace tables; version "1"; Pair = {{{fields}}};
decision_task check {{ input values: List<{element}>; output result: {output} = value;
literal_expression value {{ output result: {output}; expression {expression}; }} }}"#
        );
        let program = blkit_transpiler::parse(&source).unwrap();
        blkit_transpiler::validate(&program)
            .unwrap_or_else(|error| panic!("{element} into {output}: {expression}: {error}"));
    }
}

#[test]
fn dynamic_positional_table_rows_keep_static_cell_and_nullability_checks() {
    for (element, fields, expression) in [
        (
            "String",
            "a: Number, b: Number",
            "table([\"a\", \"b\"], values)",
        ),
        (
            "Number",
            "a: Number, b: String",
            "table([\"a\", \"b\"], values)",
        ),
        (
            "Number?",
            "a: Number, b: Number?",
            "table([\"a\", \"b\"], values)",
        ),
        (
            "Number",
            "a: Number, b: Number",
            "table([\"a\", \"b\"], values, [1])",
        ),
    ] {
        let source = format!(
            r#"namespace tables; version "1"; Pair = {{{fields}}};
decision_task check {{ input values: List<{element}>; output result: Table<Pair> = value;
literal_expression value {{ output result: Table<Pair>; expression {expression}; }} }}"#
        );
        let program = blkit_transpiler::parse(&source).unwrap();
        assert!(
            blkit_transpiler::validate(&program).is_err(),
            "{element} into {fields}: {expression}"
        );
    }
}

#[test]
fn table_from_declared_dicts_adopts_structural_rows_in_canonical_order() {
    for (input_fields, output_fields, output, expression) in [
        (
            "z: Number, a: String",
            "a: String, z: Number",
            "Table<Canonical>",
            "tableFromDicts(rows)",
        ),
        (
            "z: Number, a: String",
            "a: String, z: Number?",
            "Table<Canonical>",
            "tableFromDicts(rows)",
        ),
        (
            "z: Number?, a: String",
            "a: String, z: Number?",
            "Table<Canonical>",
            "tableFromDicts(rows)",
        ),
        (
            "z: Number, a: String",
            "a: String, z: Number",
            "Table<Canonical>",
            "tableFromDicts([first, first])",
        ),
        (
            "z: Number?, a: String",
            "a: String, z: Number?",
            "List<Number?>",
            "tableFromDicts(rows).z",
        ),
        (
            "z: Number, a: String",
            "a: String, z: Number",
            "Table<Canonical>",
            "tableFromDicts(rows).select(a, z)",
        ),
        (
            "z: Number, a: String",
            "a: String, z: Number",
            "Table<Canonical>",
            "tableFromDicts(rows)[1]",
        ),
    ] {
        let source = format!(
            r#"namespace tables; version "1"; Input = {{{input_fields}}}; Canonical = {{{output_fields}}};
decision_task check {{ input rows: List<Input>; input first: Input; output result: {output} = value;
literal_expression value {{ output result: {output}; expression {expression}; }} }}"#
        );
        let program = blkit_transpiler::parse(&source).unwrap();
        blkit_transpiler::validate(&program).unwrap_or_else(|error| {
            panic!("{input_fields} -> {output_fields}, {expression}: {error}")
        });
    }
}

#[test]
fn table_from_declared_dicts_rejects_field_type_nullability_and_order_mismatches() {
    for (input_fields, output_fields) in [
        ("z: String, a: String", "a: String, z: Number"),
        ("z: Number?, a: String", "a: String, z: Number"),
        ("z: Number", "a: String, z: Number"),
        ("z: Number, a: String, extra: Bool", "a: String, z: Number"),
        ("z: Number, other: String", "a: String, z: Number"),
        ("z: Number, a: String", "z: Number, a: String"),
    ] {
        for expression in ["tableFromDicts(rows)", "tableFromDicts([first])"] {
            let source = format!(
                r#"namespace tables; version "1"; Input = {{{input_fields}}}; Canonical = {{{output_fields}}};
decision_task check {{ input rows: List<Input>; input first: Input; output result: Table<Canonical> = value;
literal_expression value {{ output result: Table<Canonical>; expression {expression}; }} }}"#
            );
            let program = blkit_transpiler::parse(&source).unwrap();
            assert!(
                blkit_transpiler::validate(&program).is_err(),
                "{input_fields} -> {output_fields}: {expression}"
            );
        }
    }
}

#[test]
fn structural_table_adoption_does_not_weaken_ordinary_named_record_assignment() {
    let source = r#"namespace tables; version "1"; Input = {z: Number, a: String}; Canonical = {a: String, z: Number};
decision_task check { input first: Input; output result: Canonical = value;
literal_expression value { output result: Canonical; expression first; } }"#;
    assert!(transpile(source).is_err());
}

#[test]
fn table_filter_methods_require_boolean_not_positional_selectors() {
    for method in ["filter", "filterOut"] {
        for predicate in ["1", "rate", "var(position)", "[1, 2]", "1:2", "\"wrong\""] {
            let expression = format!("data.{method}({predicate})");
            let source = task("invalid", "Table<Rate>", "Table<Rate>", &expression).replace(
                "input data: Table<Rate>;",
                "input data: Table<Rate>; input position: Number;",
            );
            let program = blkit_transpiler::parse(&source).unwrap();
            assert!(
                blkit_transpiler::validate(&program).is_err(),
                "{expression}"
            );
        }
    }
}

#[test]
fn table_filter_methods_keep_nullable_predicates_and_slice_keeps_positions() {
    for nullable in [false, true] {
        for expression in [
            "data.filter(rate > var(position))",
            "data.filterOut(rate > var(position))",
            "data.filter(true)",
            "data.filterOut(false)",
            "data.filter(var(keep))",
            "data.filterOut(var(keep))",
            "data.filter(keep)",
            "data.filterOut(keep)",
            "data[1]",
            "data[[1, 2]]",
            "data[1:2]",
            "data.slice(1)",
            "data.slice([1, 2])",
            "data.slice(1:2)",
        ] {
            let source = task("valid", "Table<Rate>", "Table<Rate>", expression)
                .replace(
                    "rate: Number}",
                    if nullable {
                        "rate: Number?, keep: Bool?}"
                    } else {
                        "rate: Number, keep: Bool}"
                    },
                )
                .replace(
                    "input data: Table<Rate>;",
                    "input data: Table<Rate>; input position: Number; input keep: Bool?;",
                );
            transpile(&source)
                .unwrap_or_else(|error| panic!("nullable {nullable}, {expression}: {error}"));
        }
    }
}

#[test]
fn table_declared_field_discovery_keeps_known_schema_through_receivers() {
    for (input, receiver) in [
        ("Holder", "data.events"),
        ("Envelope", "data.holder.events"),
        ("DictionaryEntry<Table<Event>>", "data.value"),
        ("DictionaryEntry<Holder>", "data.value.events"),
    ] {
        for (output, expression) in [
            ("Table<Event>", format!("{receiver}.sort(when)")),
            (
                "Table<Selected>",
                format!("{receiver}.select(amount, when)"),
            ),
            (
                "Table<Selected>",
                format!("{receiver}[amount > 0, [amount, when]]"),
            ),
            ("List<Number?>", format!("{receiver}.amount")),
            ("Number?", format!("{receiver}.amount[1] + 1")),
            ("List<Detail>", format!("{receiver}.detail")),
            ("Number", format!("sum({receiver}.amount)")),
        ] {
            let source = task("known", input, output, &expression).replace(
                "Rate = {rate: Number};",
                "Detail = {label: String}; Event = {when: DateTime, amount: Number?, detail: Detail}; Selected = {amount: Number?, when: DateTime}; Holder = {events: Table<Event>}; Envelope = {holder: Holder};",
            );
            transpile(&source).unwrap_or_else(|error| panic!("{input}: {expression}: {error}"));
        }
    }
}

#[test]
fn table_declared_field_discovery_rejects_unknown_columns_and_incompatible_types() {
    for (input, receiver) in [
        ("Holder", "data.events"),
        ("Envelope", "data.holder.events"),
        ("DictionaryEntry<Table<Event>>", "data.value"),
    ] {
        for expression in [
            format!("{receiver}.sort(missing)"),
            format!("{receiver}.select(missing)"),
            format!("{receiver}[, missing]"),
            format!("{receiver}.missing"),
        ] {
            let source = task("unknown", input, "Table<Event>", &expression).replace(
                "Rate = {rate: Number};",
                "Event = {when: DateTime, amount: Number?}; Holder = {events: Table<Event>}; Envelope = {holder: Holder};",
            );
            let error = transpile(&source).unwrap_err();
            assert!(
                error.contains("unknown table column"),
                "{expression}: {error}"
            );
        }
        for (output, expression) in [
            ("List<Number>", format!("{receiver}.amount")),
            ("Number", format!("{receiver}.amount[1] + 1")),
            ("List<String>", format!("{receiver}.amount")),
            ("Table<NonNull>", format!("{receiver}.select(amount)")),
            ("Table<Reordered>", format!("{receiver}.sort(when)")),
        ] {
            let source = task("incompatible", input, output, &expression).replace(
                "Rate = {rate: Number};",
                "Event = {when: DateTime, amount: Number?}; NonNull = {amount: Number}; Reordered = {amount: Number?, when: DateTime}; Holder = {events: Table<Event>}; Envelope = {holder: Holder};",
            );
            assert!(
                transpile(&source).is_err(),
                "accepted {expression} as {output}"
            );
        }
    }
}

#[test]
fn table_declared_field_discovery_does_not_invent_dynamic_or_nullable_table_shapes() {
    for input in [
        "Value",
        "Dictionary",
        "Dictionary<Value>",
        "DictionaryEntry<Value>",
        "Holder",
    ] {
        let receiver = if input == "Holder" {
            "data.events"
        } else {
            "data.value"
        };
        for expression in [
            format!("{receiver}.sort(when)"),
            format!("{receiver}[, when]"),
        ] {
            let source = task("dynamic", input, "Table<Event>", &expression).replace(
                "Rate = {rate: Number};",
                "Event = {when: DateTime}; Holder = {events: Table<Event>?};",
            );
            assert!(
                transpile(&source).is_err(),
                "accepted {input}: {expression}"
            );
        }
    }
}

#[test]
fn table_declared_field_discovery_does_not_infer_unknown_length_dictionary_paths() {
    let source = task(
        "dynamic_path",
        "Dictionary<Dictionary<Holder>>",
        "Table<Event>",
        "getValue(data, path).events.sort(when)",
    )
    .replace(
        "Rate = {rate: Number};",
        "Event = {when: DateTime}; Holder = {events: Table<Event>};",
    )
    .replace(
        "input data: Dictionary<Dictionary<Holder>>;",
        "input data: Dictionary<Dictionary<Holder>>; input path: List<String>;",
    );
    assert!(transpile(&source).is_err());
}
