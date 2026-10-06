use super::*;

fn table_item(
    table: &DecisionTable,
    values: &[Expr],
    result: &Type,
    program: &Program,
    knowledge: &[Knowledge],
) -> String {
    if table.outputs.len() == 1 {
        emit_expr_with(&values[0], program, knowledge)
    } else {
        let Type::Named(name) = (if matches!(result, Type::Generic(_, _)) {
            match result {
                Type::Generic(_, inner) => inner.as_ref(),
                _ => unreachable!(),
            }
        } else {
            result
        }) else {
            unreachable!()
        };
        format!(
            "{name} {{ {} }}",
            table
                .outputs
                .iter()
                .zip(values)
                .map(|((field, _), expr)| format!(
                    "{field}: {}",
                    emit_expr_with(expr, program, knowledge)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn emit_table(
    table: &DecisionTable,
    result: &Type,
    program: &Program,
    knowledge: &[Knowledge],
) -> String {
    let mut code = String::from("{\n");
    for (name, ty, expression) in &table.inputs {
        code.push_str(&format!(
            "let {name}: {} = {};\n",
            rust_type(ty),
            emit_expr_with(expression, program, knowledge)
        ));
    }
    let item_type = if matches!(
        table.policy.as_str(),
        "RULE_ORDER" | "OUTPUT_ORDER" | "COLLECT"
    ) && table.aggregation.is_none()
    {
        if let Type::Generic(_, inner) = result {
            rust_type(inner)
        } else {
            rust_type(result)
        }
    } else if table.aggregation.as_deref() == Some("COUNT") {
        "()".into()
    } else {
        rust_type(result)
    };
    code.push_str(&format!(
        "let mut __bl_matches: Vec<{item_type}> = Vec::new();\n"
    ));
    for (condition, values) in &table.rules {
        code.push_str(&format!(
            "if {} {{ __bl_matches.push({}); }}\n",
            emit_expr_with(condition, program, knowledge),
            if table.aggregation.as_deref() == Some("COUNT") {
                "()".into()
            } else {
                table_item(table, values, result, program, knowledge)
            }
        ));
    }
    let fallback = table
        .default
        .as_ref()
        .map(|values| table_item(table, values, result, program, knowledge));
    let absent = fallback
        .map(|value| format!("Ok({value})"))
        .unwrap_or_else(|| "Err(String::from(\"no matching decision rule\"))".into());
    let result_expr = match (table.policy.as_str(), table.aggregation.as_deref()) {
        ("UNIQUE", _) => format!(
            "if __bl_matches.len() > 1 {{ Err(String::from(\"UNIQUE policy violation\")) }} else {{ __bl_matches.pop().map(Ok).unwrap_or_else(|| {absent}) }}"
        ),
        ("ANY", _) => format!(
            "if __bl_matches.windows(2).any(|pair| pair[0] != pair[1]) {{ Err(String::from(\"ANY policy violation\")) }} else {{ __bl_matches.pop().map(Ok).unwrap_or_else(|| {absent}) }}"
        ),
        ("FIRST", _) => format!(
            "if __bl_matches.is_empty() {{ {absent} }} else {{ Ok(__bl_matches.remove(0)) }}"
        ),
        ("PRIORITY" | "OUTPUT_ORDER", _) => {
            let priority = table
                .priorities
                .iter()
                .map(|row| table_item(table, row, result, program, knowledge))
                .collect::<Vec<_>>()
                .join(", ");
            let mut expression = format!(
                "let __bl_priorities = vec![{priority}]; let mut __bl_ranked = __bl_matches.into_iter().enumerate().map(|(position, value)| __bl_priorities.iter().position(|ranked| *ranked == value).map(|rank| (rank, position, value)).ok_or_else(|| String::from(\"unranked decision output\"))).collect::<Result<Vec<_>, String>>()?; __bl_ranked.sort_by_key(|(rank, position, _)| (*rank, *position));"
            );
            if table.policy == "PRIORITY" {
                expression.push_str(&format!(" if __bl_ranked.is_empty() {{ {absent} }} else {{ Ok(__bl_ranked.remove(0).2) }}"));
            } else {
                let absent = table
                    .default
                    .as_ref()
                    .map(|values| {
                        format!(
                            "vec![{}]",
                            table_item(table, values, result, program, knowledge)
                        )
                    })
                    .unwrap_or_else(|| "vec![]".into());
                expression.push_str(&format!(" if __bl_ranked.is_empty() {{ Ok({absent}) }} else {{ Ok(__bl_ranked.into_iter().map(|(_, _, value)| value).collect()) }}"));
            }
            expression
        }
        ("RULE_ORDER" | "COLLECT", None) => {
            let absent = table
                .default
                .as_ref()
                .map(|values| {
                    format!(
                        "vec![{}]",
                        table_item(table, values, result, program, knowledge)
                    )
                })
                .unwrap_or_else(|| "vec![]".into());
            format!("if __bl_matches.is_empty() {{ Ok({absent}) }} else {{ Ok(__bl_matches) }}")
        }
        ("COLLECT", Some("COUNT")) => {
            let absent = table
                .default
                .as_ref()
                .map(|values| emit_expr_with(&values[0], program, knowledge))
                .unwrap_or_else(|| "Number::ZERO".into());
            format!(
                "if __bl_matches.is_empty() {{ Ok({absent}) }} else {{ Ok(Number::from(__bl_matches.len() as u64)) }}"
            )
        }
        ("COLLECT", Some("SUM" | "MIN" | "MAX")) => {
            let operation = match table.aggregation.as_deref().unwrap() {
                "SUM" => "sum::<Number>()",
                "MIN" => "min().unwrap()",
                _ => "max().unwrap()",
            };
            format!(
                "if __bl_matches.is_empty() {{ {absent} }} else {{ Ok(__bl_matches.into_iter().{operation}) }}"
            )
        }
        _ => unreachable!(),
    };
    code.push_str(&result_expr);
    code.push_str("\n}");
    code
}

pub(super) fn emit_decision(model: &DecisionModel, program: &Program, out: &mut String) {
    let inputs = if model.braced {
        model
            .inputs
            .iter()
            .map(|(name, ty)| format!("{name}: {}", rust_type(ty)))
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        format!("{}: {}", model.input, rust_type(&model.input_type))
    };
    let output = if model.braced && model.outputs.len() > 1 {
        "serde_json::Value".into()
    } else {
        rust_type(&model.output)
    };
    out.push_str(&format!(
        "pub fn {}({inputs}) -> Result<{output}, String> {{\n",
        model.name
    ));
    for item in &model.knowledge {
        if item.braced {
            continue;
        }
        let fallible = expr_fallible(&item.body, &model.knowledge);
        out.push_str(&format!(
            "fn {}({}) -> {} {{ {}{}{} }}\n",
            item.name,
            item.params
                .iter()
                .map(|(name, ty)| format!("{name}: {}", rust_type(ty)))
                .collect::<Vec<_>>()
                .join(", "),
            if fallible {
                format!("Result<{}, String>", rust_type(&item.output))
            } else {
                rust_type(&item.output)
            },
            if fallible { "Ok(" } else { "" },
            emit_expr_with(&item.body, program, &model.knowledge),
            if fallible { ")" } else { "" }
        ));
    }
    let mut emitted = std::collections::HashSet::new();
    while emitted.len() < model.nodes.len() {
        for node in &model.nodes {
            if emitted.contains(&node.name)
                || model
                    .links
                    .iter()
                    .any(|(from, to)| to == &node.name && !emitted.contains(from))
            {
                continue;
            }
            let value = match &node.kind {
                DecisionKind::Literal(expr) => emit_expr_with(expr, program, &model.knowledge),
                DecisionKind::Context { entries, result } => format!(
                    "{{ {} {} }}",
                    entries
                        .iter()
                        .map(|(name, ty, expr)| format!(
                            "let {name}: {} = {};",
                            rust_type(ty),
                            emit_expr_with(expr, program, &model.knowledge)
                        ))
                        .collect::<Vec<_>>()
                        .join(" "),
                    emit_expr_with(result, program, &model.knowledge)
                ),
                DecisionKind::Table(table) => format!(
                    "(|| -> Result<{}, String> {{ {} }})()?",
                    rust_type(&node.output),
                    emit_table(table, &node.output, program, &model.knowledge)
                ),
            };
            out.push_str(&format!(
                "let {}: {} = {value};\n",
                node.name,
                rust_type(&node.output)
            ));
            emitted.insert(&node.name);
        }
    }
    if model.braced && model.outputs.len() > 1 {
        let fields = model
            .outputs
            .iter()
            .map(|(port, _, reference)| {
                format!("{port:?}: {}", reference.split('.').next().unwrap())
            })
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("Ok(serde_json::json!({{{fields}}}))\n}}\n"));
    } else {
        out.push_str(&format!("Ok({})\n}}\n", model.output_node));
    }
}
