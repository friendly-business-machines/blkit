use super::*;

pub(super) fn check_decision(
    model: &DecisionModel,
    program: &Program,
    types: &HashSet<&str>,
) -> Result<(), String> {
    check_name(&model.input)?;
    resolve(&model.input_type, types)?;
    resolve(&model.output, types)?;
    let mut names = HashSet::from([model.input.as_str()]);
    for knowledge in &model.knowledge {
        check_name(&knowledge.name)?;
        if !names.insert(&knowledge.name) {
            return Err(format!("duplicate knowledge model: {}", knowledge.name));
        }
        resolve(&knowledge.output, types)?;
        let mut env = HashMap::new();
        for (param, ty) in &knowledge.params {
            check_name(param)?;
            resolve(ty, types)?;
            if env.insert(param.clone(), ty.clone()).is_some() {
                return Err(format!("duplicate knowledge parameter: {param}"));
            }
        }
        let result = infer_with(
            &knowledge.body,
            Some(&knowledge.output),
            &env,
            program,
            &model.knowledge,
        )?;
        if result != knowledge.output {
            return Err(format!(
                "knowledge result type mismatch: {}",
                knowledge.name
            ));
        }
    }
    for node in &model.nodes {
        check_name(&node.name)?;
        resolve(&node.output, types)?;
        if !names.insert(&node.name) {
            return Err(format!("duplicate decision node: {}", node.name));
        }
    }
    for (source, target) in &model.links {
        if !model.nodes.iter().any(|n| n.name == *source)
            || !model.nodes.iter().any(|n| n.name == *target)
        {
            return Err(format!(
                "unknown decision node in link: {source} -> {target}"
            ));
        }
    }
    let knowledge_names: HashSet<_> = model
        .knowledge
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    fn calls<'a>(expr: &'a Expr, found: &mut Vec<&'a str>) {
        match expr {
            Expr::Call(name, args) => {
                found.push(name);
                for arg in args {
                    calls(arg, found);
                }
            }
            Expr::Field(base, _) | Expr::Not(base) => calls(base, found),
            Expr::Binary(left, _, right) => {
                calls(left, found);
                calls(right, found);
            }
            Expr::List(items) => {
                for item in items {
                    calls(item, found);
                }
            }
            Expr::Range(lower, upper, _, _) => {
                for bound in lower.iter().chain(upper.iter()) {
                    calls(bound, found);
                }
            }
            _ => {}
        }
    }
    for item in &model.knowledge {
        let mut found = vec![];
        calls(&item.body, &mut found);
        for name in found {
            if !knowledge_names.contains(name)
                && !range_relation(name)
                && !string_builtin(name)
                && !matches!(name, "date" | "time" | "dateTime")
            {
                return Err(format!("unknown knowledge model: {name}"));
            }
        }
    }
    fn visit_knowledge<'a>(
        name: &'a str,
        model: &'a DecisionModel,
        active: &mut HashSet<&'a str>,
        done: &mut HashSet<&'a str>,
    ) -> Result<(), String> {
        if done.contains(name) {
            return Ok(());
        }
        if !active.insert(name) {
            return Err(format!("knowledge cycle at {name}"));
        }
        let item = model
            .knowledge
            .iter()
            .find(|item| item.name == name)
            .ok_or("unknown knowledge model")?;
        let mut deps = vec![];
        calls(&item.body, &mut deps);
        for dep in deps {
            if model.knowledge.iter().any(|item| item.name == dep) {
                visit_knowledge(dep, model, active, done)?;
            }
        }
        active.remove(name);
        done.insert(name);
        Ok(())
    }
    let mut knowledge_done = HashSet::new();
    for item in &model.knowledge {
        visit_knowledge(&item.name, model, &mut HashSet::new(), &mut knowledge_done)?;
    }
    fn visit<'a>(
        name: &'a str,
        model: &'a DecisionModel,
        program: &Program,
        types: &HashSet<&str>,
        knowledge_names: &HashSet<&str>,
        active: &mut HashSet<&'a str>,
        done: &mut HashMap<String, Type>,
    ) -> Result<(), String> {
        if done.contains_key(name) {
            return Ok(());
        }
        if !active.insert(name) {
            return Err(format!("decision cycle at {name}"));
        }
        let node = model
            .nodes
            .iter()
            .find(|n| n.name == name)
            .ok_or_else(|| format!("unknown decision node: {name}"))?;
        let mut env = HashMap::from([(model.input.clone(), model.input_type.clone())]);
        for (source, target) in &model.links {
            if target == name {
                visit(source, model, program, types, knowledge_names, active, done)?;
                env.insert(source.clone(), done[source].clone());
            }
        }
        let check = |expression: &Expr,
                     expected: &Type,
                     env: &HashMap<String, Type>|
         -> Result<(), String> {
            let mut called = vec![];
            calls(expression, &mut called);
            for name in called {
                if !knowledge_names.contains(name)
                    && !range_relation(name)
                    && !string_builtin(name)
                    && !matches!(name, "date" | "time" | "dateTime")
                {
                    return Err(format!("unknown knowledge model: {name}"));
                }
            }
            let actual = infer_with(expression, Some(expected), env, program, &model.knowledge)?;
            if actual != *expected {
                return Err(format!(
                    "decision result type mismatch for {}: expected {expected}, got {actual}",
                    node.name
                ));
            }
            Ok(())
        };
        match &node.kind {
            DecisionKind::Literal(expr) => check(expr, &node.output, &env)?,
            DecisionKind::Context { entries, result } => {
                for (key, ty, expr) in entries {
                    check_name(key)?;
                    resolve(ty, types)?;
                    if env.contains_key(key) {
                        return Err(format!("duplicate context entry: {key}"));
                    }
                    check(expr, ty, &env)?;
                    env.insert(key.clone(), ty.clone());
                }
                check(result, &node.output, &env)?;
            }
            DecisionKind::Table(table) => {
                check_table(table, &node.output, &mut env, model, program, types)?
            }
        }
        active.remove(name);
        done.insert(name.into(), node.output.clone());
        Ok(())
    }
    let mut done = HashMap::new();
    for node in &model.nodes {
        visit(
            &node.name,
            model,
            program,
            types,
            &knowledge_names,
            &mut HashSet::new(),
            &mut done,
        )?;
    }
    let actual = done
        .get(&model.output_node)
        .ok_or_else(|| format!("unknown decision node: {}", model.output_node))?;
    if actual != &model.output {
        return Err(format!("decision result type mismatch for {}", model.name));
    }
    Ok(())
}

pub(super) fn check_table(
    table: &DecisionTable,
    result: &Type,
    env: &mut HashMap<String, Type>,
    model: &DecisionModel,
    program: &Program,
    types: &HashSet<&str>,
) -> Result<(), String> {
    if table.inputs.is_empty() || table.outputs.is_empty() || table.rules.is_empty() {
        return Err("table requires input, output, and rules".into());
    }
    let collected = matches!(
        table.policy.as_str(),
        "RULE_ORDER" | "OUTPUT_ORDER" | "COLLECT"
    ) && table.aggregation.is_none();
    if let Some(aggregation) = table.aggregation.as_deref() {
        if table.policy != "COLLECT" || !matches!(aggregation, "SUM" | "MIN" | "MAX" | "COUNT") {
            return Err("invalid table aggregation policy".into());
        }
        if aggregation != "COUNT"
            && (table.outputs.len() != 1 || table.outputs[0].1 != Type::Named("Number".into()))
        {
            return Err("table aggregation requires one Number output".into());
        }
    }
    if matches!(table.policy.as_str(), "PRIORITY" | "OUTPUT_ORDER") {
        if table.priorities.is_empty() {
            return Err("table priority order required".into());
        }
    } else if !table.priorities.is_empty() {
        return Err("priority requires PRIORITY or OUTPUT_ORDER policy".into());
    }
    let item = if table.aggregation.as_deref() == Some("COUNT") {
        Type::Named("Number".into())
    } else if table.outputs.len() == 1 {
        table.outputs[0].1.clone()
    } else {
        let Type::Named(name) = (if collected {
            match result {
                Type::Generic(kind, inner) if kind == "List" => inner.as_ref(),
                _ => return Err("table outputs require a List<record> result".into()),
            }
        } else {
            result
        }) else {
            return Err("table outputs require a record result".into());
        };
        let record = program
            .records
            .iter()
            .find(|r| r.name == *name)
            .ok_or("unknown table output record")?;
        if record.fields != table.outputs {
            return Err("table output columns do not match result record".into());
        }
        Type::Named(name.clone())
    };
    let expected = if table.aggregation.is_some() {
        Type::Named("Number".into())
    } else if collected {
        Type::Generic("List".into(), Box::new(item))
    } else {
        item
    };
    if &expected != result {
        return Err(format!(
            "table result type mismatch: expected {expected}, got {result}"
        ));
    }
    for (name, ty, expression) in &table.inputs {
        check_name(name)?;
        resolve(ty, types)?;
        if env.contains_key(name) {
            return Err(format!("duplicate table input: {name}"));
        }
        let actual = infer_with(expression, Some(ty), env, program, &model.knowledge)?;
        if &actual != ty {
            return Err(format!("table input type mismatch: {name}"));
        }
        env.insert(name.clone(), ty.clone());
    }
    let mut outputs = HashSet::new();
    for (name, ty) in &table.outputs {
        check_name(name)?;
        resolve(ty, types)?;
        if !outputs.insert(name) {
            return Err(format!("duplicate table output: {name}"));
        }
    }
    let row = |values: &[Expr], location: &str| -> Result<(), String> {
        if values.len() != table.outputs.len() {
            return Err(format!(
                "{location} output count does not match table columns"
            ));
        }
        for (expression, (_, ty)) in values.iter().zip(&table.outputs) {
            let actual = infer_with(expression, Some(ty), env, program, &model.knowledge)?;
            if &actual != ty {
                return Err(format!(
                    "{location} output type mismatch: expected {ty}, got {actual}"
                ));
            }
        }
        Ok(())
    };
    for (condition, values) in &table.rules {
        if infer_with(condition, None, env, program, &model.knowledge)?
            != Type::Named("Bool".into())
        {
            return Err("table rule condition must be Bool".into());
        }
        row(values, "rule")?;
    }
    for (index, values) in table.priorities.iter().enumerate() {
        row(values, "priority")?;
        if table.priorities[..index].contains(values) {
            return Err("duplicate priority value".into());
        }
    }
    if let Some(values) = &table.default {
        if table.aggregation.as_deref() == Some("COUNT") {
            if values.len() != 1
                || infer_with(&values[0], Some(result), env, program, &model.knowledge)? != *result
            {
                return Err("COUNT default must be Number".into());
            }
        } else {
            row(values, "default")?;
        }
    }
    Ok(())
}
