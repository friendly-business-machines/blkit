use super::constraints::{self, Constraints, Term};
use super::*;

pub(super) fn record_cycle(
    index: usize,
    program: &Program,
    active: &mut HashSet<usize>,
    visited: &mut HashSet<usize>,
) -> bool {
    if visited.contains(&index) {
        return false;
    }
    if !active.insert(index) {
        return true;
    }
    for (_, ty) in &program.records[index].fields {
        if let Type::Named(name) = ty
            && let Some(next) = program
                .records
                .iter()
                .position(|record| record.name == *name)
            && record_cycle(next, program, active, visited)
        {
            return true;
        }
    }
    active.remove(&index);
    visited.insert(index);
    false
}

pub(crate) fn check_name(name: &str) -> Result<(), String> {
    if matches!(
        name,
        "_" | "as"
            | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "gen"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "try"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "union"
    ) {
        return Err(format!("reserved Rust identifier: {name}"));
    }
    Ok(())
}

pub(super) fn returns(body: &[Stmt]) -> bool {
    body.iter().any(|stmt| match stmt {
        Stmt::Return(_) => true,
        Stmt::If(_, yes, no) => returns(yes) && returns(no),
    })
}

pub(super) fn check_block(
    body: &[Stmt],
    input: &str,
    input_type: &Type,
    output: &Type,
    program: &Program,
) -> Result<(), String> {
    let env = HashMap::from([(input.to_owned(), input_type.clone())]);
    for stmt in body {
        match stmt {
            Stmt::Return(value) => {
                let actual = infer(value, Some(output), &env, program)?;
                if &actual != output {
                    return Err(format!(
                        "return type mismatch: expected {output}, got {actual}"
                    ));
                }
            }
            Stmt::If(condition, yes, no) => {
                let actual = infer(condition, None, &env, program)?;
                if actual != Type::Named("Bool".into()) {
                    return Err(format!("if condition must be Bool, got {actual}"));
                }
                check_block(yes, input, input_type, output, program)?;
                check_block(no, input, input_type, output, program)?;
            }
        }
    }
    Ok(())
}

pub(super) fn infer(
    expr: &Expr,
    expected: Option<&Type>,
    env: &HashMap<String, Type>,
    program: &Program,
) -> Result<Type, String> {
    infer_with(expr, expected, env, program, &[])
}

fn constant_number(expr: &Expr) -> Option<Result<Decimal, String>> {
    match expr {
        Expr::Number(value) => Some(
            if value.contains(['e', 'E']) {
                Decimal::from_scientific(value)
            } else {
                Decimal::from_str_exact(value)
            }
            .map_err(|e| e.to_string()),
        ),
        Expr::Binary(left, op, right) if matches!(op.as_str(), "+" | "-" | "*" | "/" | "**") => {
            let a = constant_number(left)?;
            let b = constant_number(right)?;
            Some(a.and_then(|a| b.and_then(|b| crate::number_ops::arithmetic(op, a, b))))
        }
        _ => None,
    }
}

fn call_types_match(name: &str, arguments: &[Type], result: &Type) -> bool {
    !constraints::call_branches(
        &Constraints::default(),
        &constraints::fixed_call_signatures(name).expect("known built-in"),
        &arguments.iter().map(Term::from).collect::<Vec<_>>(),
        &Term::from(result),
    )
    .is_empty()
}

fn calendar_call_types_match(name: &str, arguments: &[Type], result: &Type) -> bool {
    !constraints::call_branches(
        &Constraints::default(),
        &constraints::calendar_call_signatures(name).expect("known calendar call"),
        &arguments.iter().map(Term::from).collect::<Vec<_>>(),
        &Term::from(result),
    )
    .is_empty()
}

fn check_call_signature(
    name: &str,
    args: &[Expr],
    signatures: &[(Vec<Type>, Type)],
    env: &HashMap<String, Type>,
    program: &Program,
    knowledge: &[Knowledge],
) -> Result<Type, String> {
    let Some((parameters, output)) = signatures
        .iter()
        .find(|(parameters, _)| parameters.len() == args.len())
    else {
        return Err(format!("invalid {name} argument count"));
    };
    let actual = args
        .iter()
        .zip(parameters)
        .map(|(arg, ty)| infer_with(arg, Some(ty), env, program, knowledge))
        .collect::<Result<Vec<_>, _>>()?;
    let arguments: Vec<_> = actual.iter().map(Term::from).collect();
    if constraints::call_branches(
        &Constraints::default(),
        signatures,
        &arguments,
        &Term::from(output),
    )
    .is_empty()
    {
        let (expected, got) = parameters
            .iter()
            .zip(&actual)
            .find(|(expected, got)| *expected != *got)
            .unwrap();
        return Err(format!("{name} requires {expected}, got {got}"));
    }
    Ok(output.clone())
}

fn binding_used(expr: &Expr, name: &str) -> bool {
    match expr {
        Expr::Name(value) => value == name,
        Expr::Iteration {
            binding,
            source,
            body,
            ..
        } => binding_used(source, name) || binding != name && binding_used(body, name),
        Expr::InlinePredicate { binding, body } => binding != name && binding_used(body, name),
        Expr::NamedArgument(_, value) => binding_used(value, name),
        Expr::Some {
            bindings,
            predicate,
        } => {
            let mut shadowed = false;
            let mut used = false;
            for (binding, source) in bindings {
                used |= !shadowed && binding_used(source, name);
                shadowed |= binding == name;
            }
            used || !shadowed && binding_used(predicate, name)
        }
        Expr::Not(value) | Expr::Field(value, _) => binding_used(value, name),
        Expr::Index(base, key) | Expr::Sequence(base, key) => {
            binding_used(base, name) || binding_used(key, name)
        }
        Expr::Binary(left, _, right) => binding_used(left, name) || binding_used(right, name),
        Expr::Dictionary(entries) => {
            let mut shadowed = false;
            entries.iter().any(|(key, value)| {
                let used = !shadowed && binding_used(value, name);
                shadowed |= key == name;
                used
            })
        }
        Expr::List(items) | Expr::Call(_, items) => {
            items.iter().any(|item| binding_used(item, name))
        }
        Expr::Range(lower, upper, _, _) => lower
            .iter()
            .chain(upper.iter())
            .any(|item| binding_used(item, name)),
        _ => false,
    }
}

pub(crate) fn empty_iteration_element(
    expr: &Expr,
    expected: Option<&Type>,
    env: &HashMap<String, Type>,
    program: &Program,
    knowledge: &[Knowledge],
) -> Result<Option<Type>, String> {
    let Expr::Iteration {
        binding,
        source,
        body,
        ..
    } = expr
    else {
        return Ok(None);
    };
    if !matches!(source.as_ref(), Expr::List(items) if items.is_empty())
        || !binding_used(body, binding)
    {
        return Ok(None);
    }
    let result = expected.map_or(Term::Var(0), Term::from);
    let symbolic_env = env
        .iter()
        .map(|(name, ty)| (name.clone(), Term::from(ty)))
        .collect();
    let mut bindings = HashMap::new();
    let branches = constraints::expression_branches_with_bindings(
        expr,
        &result,
        &symbolic_env,
        &program.records,
        knowledge,
        &program.enums,
        &mut bindings,
    )
    .ok_or("cannot constrain empty iteration source")?;
    let binding_term = bindings
        .get(&std::ptr::from_ref(expr))
        .expect("iteration binding");
    Constraints::unique_type(&branches, binding_term)
        .map(Some)
        .ok_or("ambiguous empty iteration source binding type".into())
}

pub(crate) fn empty_some_element(
    expr: &Expr,
    source: &Expr,
    env: &HashMap<String, Type>,
    program: &Program,
    knowledge: &[Knowledge],
) -> Result<Type, String> {
    let Expr::Some {
        bindings: sources,
        predicate,
    } = expr
    else {
        return Err("expected some expression".into());
    };
    if let Some(index) = sources
        .iter()
        .position(|(_, value)| std::ptr::eq(value, source))
    {
        let binding = &sources[index].0;
        let later = &sources[index + 1..];
        let mut used = false;
        let mut shadowed = false;
        for (name, value) in later {
            used |= !shadowed && binding_used(value, binding);
            shadowed |= name == binding;
        }
        if !used && (shadowed || !binding_used(predicate, binding)) {
            return Ok(Type::Named("Value".into()));
        }
    }
    let symbolic_env = env
        .iter()
        .map(|(name, ty)| (name.clone(), Term::from(ty)))
        .collect();
    let mut bindings = HashMap::new();
    let branches = constraints::expression_branches_with_bindings(
        expr,
        &Term::from(&Type::Named("Bool".into())),
        &symbolic_env,
        &program.records,
        knowledge,
        &program.enums,
        &mut bindings,
    )
    .ok_or("cannot constrain empty some source")?;
    let binding = bindings
        .get(&std::ptr::from_ref(source))
        .ok_or("missing some binding")?;
    Constraints::unique_type(&branches, binding)
        .ok_or("ambiguous empty some source binding type".into())
}

pub(crate) fn infer_with(
    expr: &Expr,
    expected: Option<&Type>,
    env: &HashMap<String, Type>,
    program: &Program,
    knowledge: &[Knowledge],
) -> Result<Type, String> {
    use Expr::*;
    let named = |name: &str| Type::Named(name.into());
    match expr {
        Number(value) => {
            if value.contains(['e', 'E']) {
                Decimal::from_scientific(value)
            } else {
                Decimal::from_str_exact(value)
            }
            .map_err(|_| format!("invalid Number: {value}"))?;
            Ok(named("Number"))
        }
        String(_) => Ok(named("String")),
        Bool(_) => Ok(named("Bool")),
        Name(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown name: {name}")),
        FilterItem => env
            .get(".item")
            .cloned()
            .ok_or(".item requires a list filter predicate".into()),
        InlinePredicate { .. } => {
            Err("inline predicate requires remove or listReplace match argument".into())
        }
        NamedArgument(..) => Err("named argument requires a calendar transformation".into()),
        Some {
            bindings,
            predicate,
        } => {
            let mut local = env.clone();
            for (binding, source) in bindings {
                check_name(binding)?;
                let hint = if matches!(source, Expr::List(items) if items.is_empty()) {
                    Some(Type::Generic(
                        "List".into(),
                        Box::new(empty_some_element(expr, source, env, program, knowledge)?),
                    ))
                } else {
                    None
                };
                let ty = infer_with(source, hint.as_ref(), &local, program, knowledge)?;
                let Type::Generic(kind, element) = ty else {
                    return Err("iteration source must be List".into());
                };
                if kind != "List" {
                    return Err("iteration source must be List".into());
                }
                local.insert(binding.clone(), *element);
            }
            if infer_with(predicate, Some(&named("Bool")), &local, program, knowledge)?
                != named("Bool")
            {
                return Err("some predicate must be Bool".into());
            }
            Ok(named("Bool"))
        }
        Dictionary(entries) => {
            let schema = expected.and_then(|ty| match ty {
                Type::Named(name) => program.records.iter().find(|record| &record.name == name),
                _ => None,
            });
            if let Some(schema) = schema
                && schema.fields.len() != entries.len()
            {
                return Err(format!("dictionary {} requires all fields", schema.name));
            }
            let element_type = match expected {
                Some(Type::Generic(kind, inner)) if kind == "Dictionary" => Some(inner.as_ref()),
                _ => None,
            };
            let mut scope = env.clone();
            let mut homogeneous = None;
            let mut fields = Vec::new();
            for (key, value) in entries {
                let field_type = schema.and_then(|schema| {
                    schema
                        .fields
                        .iter()
                        .find(|(field, _)| field == key)
                        .map(|(_, ty)| ty)
                });
                if schema.is_some() && field_type.is_none() {
                    return Err(format!("unknown dictionary field: {key}"));
                }
                let ty = infer_with(
                    value,
                    field_type.or(element_type),
                    &scope,
                    program,
                    knowledge,
                )?;
                if field_type
                    .or(element_type)
                    .is_some_and(|wanted| wanted != &ty)
                {
                    return Err(format!(
                        "dictionary field {key} has incompatible type: {ty}"
                    ));
                }
                if homogeneous.as_ref().is_some_and(|previous| previous != &ty) {
                    homogeneous = Some(named("Value"));
                } else if homogeneous.is_none() {
                    homogeneous = Some(ty.clone());
                }
                fields.push((key.clone(), Term::from(&ty)));
                if crate::compiler::identifier(key) {
                    scope.insert(key.clone(), ty);
                }
            }
            let result = if let Some(ty) = expected {
                if schema.is_some() || element_type.is_some() {
                    ty.clone()
                } else {
                    Type::Named("Dictionary".into())
                }
            } else if let Some(ty) = homogeneous {
                if ty == named("Value") {
                    named("Dictionary")
                } else {
                    Type::Generic("Dictionary".into(), Box::new(ty))
                }
            } else {
                named("Dictionary")
            };
            if constraints::dictionary_literal_branches(
                &Constraints::default(),
                &fields,
                &Term::from(&result),
                &program.records,
            )
            .is_empty()
            {
                return Err("dictionary fields have incompatible types".into());
            }
            Ok(result)
        }
        Sequence(start, end) => {
            for bound in [start, end] {
                if infer_with(bound, Some(&named("Number")), env, program, knowledge)?
                    != named("Number")
                {
                    return Err("sequence bounds require Number".into());
                }
            }
            Ok(Type::Generic("List".into(), Box::new(named("Number"))))
        }
        Index(base, key) => {
            let base_expected = if matches!(base.as_ref(), List(elements) if elements.is_empty())
                && matches!(expected, Some(Type::Generic(kind, _)) if kind == "List")
            {
                expected
            } else {
                None
            };
            let base_type = infer_with(base, base_expected, env, program, knowledge)?;
            if let Type::Generic(kind, element) = &base_type
                && kind == "List"
            {
                if infer_with(key, None, env, program, knowledge).ok() == Some(named("Number")) {
                    let result = *element.clone();
                    if constraints::index_branches(
                        &Constraints::default(),
                        &Term::from(&base_type),
                        &Term::from(&named("Number")),
                        &Term::from(&result),
                        &program.records,
                        None,
                    )
                    .is_empty()
                    {
                        return Err("list index requires Number".into());
                    }
                    return Ok(result);
                }
                let mut local = env.clone();
                local.insert(".item".into(), *element.clone());
                let predicate = infer_with(key, None, &local, program, knowledge)?;
                if constraints::index_branches(
                    &Constraints::default(),
                    &Term::from(&base_type),
                    &Term::from(&predicate),
                    &Term::from(&base_type),
                    &program.records,
                    None,
                )
                .is_empty()
                {
                    return Err("list predicate must be Bool".into());
                }
                return Ok(base_type);
            }
            if infer_with(key, Some(&named("String")), env, program, knowledge)? != named("String")
            {
                return Err("dictionary key requires String".into());
            }
            match base_type {
                Type::Generic(kind, ty) if kind == "Dictionary" => {
                    let result = *ty;
                    if constraints::index_branches(
                        &Constraints::default(),
                        &Term::from(&Type::Generic(kind, Box::new(result.clone()))),
                        &Term::from(&named("String")),
                        &Term::from(&result),
                        &program.records,
                        None,
                    )
                    .is_empty()
                    {
                        return Err("dictionary index requires a dictionary".into());
                    }
                    Ok(result)
                }
                Type::Named(name) if matches!(name.as_str(), "Dictionary" | "Value") => {
                    let result = expected.cloned().unwrap_or_else(|| named("Value"));
                    if constraints::index_branches(
                        &Constraints::default(),
                        &Term::from(&named(&name)),
                        &Term::from(&named("String")),
                        &Term::from(&result),
                        &program.records,
                        None,
                    )
                    .is_empty()
                    {
                        return Err("dictionary index requires a dictionary".into());
                    }
                    Ok(result)
                }
                Type::Named(name) => {
                    let record = program
                        .records
                        .iter()
                        .find(|record| record.name == name)
                        .ok_or("dictionary index requires a dictionary")?;
                    let Expr::String(key) = key.as_ref() else {
                        let result = named("Value");
                        if constraints::index_branches(
                            &Constraints::default(),
                            &Term::from(&named(&name)),
                            &Term::from(&named("String")),
                            &Term::from(&result),
                            &program.records,
                            None,
                        )
                        .is_empty()
                        {
                            return Err("dictionary index requires a dictionary".into());
                        }
                        return Ok(result);
                    };
                    let result = record
                        .fields
                        .iter()
                        .find(|(field, _)| field == key)
                        .map(|(_, ty)| ty.clone())
                        .ok_or_else(|| format!("unknown dictionary field: {key}"))?;
                    if constraints::index_branches(
                        &Constraints::default(),
                        &Term::from(&named(&name)),
                        &Term::from(&named("String")),
                        &Term::from(&result),
                        &program.records,
                        Some(key),
                    )
                    .is_empty()
                    {
                        return Err(format!("unknown dictionary field: {key}"));
                    }
                    Ok(result)
                }
                _ => Err("dictionary index requires a dictionary".into()),
            }
        }
        Call(name, args) => {
            if matches!(
                name.as_str(),
                "insertBefore" | "insertAfter" | "remove" | "listReplace"
            ) && !knowledge.iter().any(|definition| definition.name == *name)
                && !matches!(args.get(1), Some(InlinePredicate { .. }))
            {
                let arity = if name == "remove" { 2 } else { 3 };
                if args.len() != arity {
                    return Err(format!("invalid {name} argument count"));
                }
                let item_hint = if matches!(name.as_str(), "insertBefore" | "insertAfter") {
                    args.get(2).and_then(|arg| {
                        infer_with(arg, None, env, program, knowledge)
                            .ok()
                            .map(|ty| {
                                if matches!(ty, Type::Generic(ref kind, _) if kind == "List") {
                                    ty
                                } else {
                                    Type::Generic("List".into(), Box::new(ty))
                                }
                            })
                    })
                } else {
                    None
                };
                let source = infer_with(
                    &args[0],
                    item_hint.as_ref().or(expected),
                    env,
                    program,
                    knowledge,
                )?;
                let Type::Generic(kind, item) = &source else {
                    return Err(format!("{name} requires List<T>, got {source}"));
                };
                if kind != "List" {
                    return Err(format!("{name} requires List<T>, got {source}"));
                }
                let position =
                    infer_with(&args[1], Some(&named("Number")), env, program, knowledge)?;
                if position != named("Number") {
                    return Err(format!("{name} requires Number, got {position}"));
                }
                if let Some(argument) = args.get(2) {
                    let required = if name == "listReplace" {
                        item.as_ref()
                    } else {
                        &source
                    };
                    let got = infer_with(argument, Some(required), env, program, knowledge)?;
                    if &got != required
                        && (name == "listReplace"
                            || &got != item.as_ref()
                            || matches!(got, Type::Generic(ref kind, _) if kind == "List"))
                    {
                        return Err(format!("{name} requires {required}, got {got}"));
                    }
                }
                return Ok(source);
            }
            if matches!(name.as_str(), "remove" | "listReplace")
                && !knowledge.iter().any(|definition| definition.name == *name)
                && matches!(args.get(1), Some(InlinePredicate { .. }))
            {
                let valid_arity = if name == "remove" { 2 } else { 3 };
                if args.len() != valid_arity {
                    return Err(format!("invalid {name} argument count"));
                }
                let source = infer_with(&args[0], expected, env, program, knowledge)?;
                let Type::Generic(kind, item) = &source else {
                    return Err(format!("{name} requires List"));
                };
                if kind != "List" {
                    return Err(format!("{name} requires List"));
                }
                let InlinePredicate { binding, body } = &args[1] else {
                    unreachable!()
                };
                check_name(binding)?;
                let mut local = env.clone();
                local.insert(binding.clone(), *item.clone());
                let predicate = infer_with(body, Some(&named("Bool")), &local, program, knowledge)?;
                if predicate != named("Bool") {
                    return Err(format!("{name} predicate must be Bool, got {predicate}"));
                }
                if name == "listReplace" {
                    let replacement = infer_with(&args[2], Some(item), env, program, knowledge)?;
                    if &replacement != item.as_ref() {
                        return Err(format!("listReplace requires {item}, got {replacement}"));
                    }
                }
                return Ok(source);
            }
            if matches!(name.as_str(), "dictionaryPut" | "dictionaryRemove") {
                let Some(source) = args.first() else {
                    return Err(format!("{name} needs a dictionary"));
                };
                let ty = infer_with(source, None, env, program, knowledge)?;
                if !matches!(ty, Type::Generic(ref kind, _) if kind == "Dictionary")
                    && !matches!(ty, Type::Named(ref kind) if kind == "Dictionary" || program.records.iter().any(|record| &record.name == kind))
                {
                    return Err(format!("{name} needs a dictionary"));
                }
                if args.len() != if name == "dictionaryPut" { 3 } else { 2 } {
                    return Err(format!("invalid {name} arguments"));
                }
                let list = Type::Generic("List".into(), Box::new(named("String")));
                let key = infer_with(
                    &args[1],
                    if matches!(&args[1], Expr::List(items) if items.is_empty()) {
                        Some(&list)
                    } else {
                        None
                    },
                    env,
                    program,
                    knowledge,
                )?;
                if key != named("String") && !(name == "dictionaryPut" && key == list) {
                    return Err(format!("invalid {name} arguments"));
                }
                let mut arguments = vec![Term::from(&ty), Term::from(&key)];
                if name == "dictionaryPut" {
                    arguments.push(Term::from(&infer_with(
                        &args[2], None, env, program, knowledge,
                    )?));
                }
                if constraints::dictionary_call_branches(
                    &Constraints::default(),
                    name,
                    &arguments,
                    &Term::from(&named("Dictionary")),
                    &program.records,
                    None,
                )
                .is_empty()
                {
                    return Err(format!("invalid {name} arguments"));
                }
                return Ok(named("Dictionary"));
            }
            if name == "dictionaryMerge" {
                if args.len() != 1 {
                    return Err("dictionaryMerge requires a list".into());
                }
                let ty = infer_with(
                    &args[0],
                    Some(&Type::Generic("List".into(), Box::new(named("Dictionary")))),
                    env,
                    program,
                    knowledge,
                )?;
                let Type::Generic(kind, element) = ty else {
                    return Err("dictionaryMerge requires a list of dictionaries".into());
                };
                if kind != "List"
                    || constraints::dictionary_call_branches(
                        &Constraints::default(),
                        name,
                        &[Term::from(&Type::Generic(kind, element))],
                        &Term::from(&named("Dictionary")),
                        &program.records,
                        None,
                    )
                    .is_empty()
                {
                    return Err("dictionaryMerge requires a list of dictionaries".into());
                }
                return Ok(named("Dictionary"));
            }
            if matches!(name.as_str(), "keys" | "values" | "getEntries" | "size" | "isEmpty" | "has" | "getValue")
                && args.first().is_some_and(|first| matches!(infer_with(first, None, env, program, knowledge),
                    Ok(Type::Generic(ref kind, _)) if kind == "Dictionary")
                    || matches!(infer_with(first, None, env, program, knowledge),
                        Ok(Type::Named(ref kind)) if matches!(kind.as_str(), "Dictionary" | "Value") || program.records.iter().any(|record| &record.name == kind)))
            {
                let dictionary_type = infer_with(&args[0], None, env, program, knowledge)?;
                let mut inner = match &dictionary_type {
                    Type::Generic(_, inner) => inner.as_ref().clone(),
                    Type::Named(shape) if name == "getValue" && args.len() == 2 => {
                        let key = match &args[1] { Expr::String(key) => Some(key), Expr::List(items) => items.first().and_then(|item| if let Expr::String(key) = item { Some(key) } else { None }), _ => None };
                        program.records.iter().find(|record| &record.name == shape)
                            .and_then(|record| record.fields.iter().find(|(field, _)| Some(field) == key))
                            .map(|(_, ty)| ty.clone()).unwrap_or_else(|| named("Value"))
                    }
                    _ => named("Value"),
                };
                let text = named("String");
                let key_type = if matches!(name.as_str(), "has" | "getValue") && args.len() == 2 {
                    Some(if matches!(&args[1], Expr::List(items) if items.is_empty()) && name == "getValue" {
                        infer_with(&args[1], Some(&Type::Generic("List".into(), Box::new(text.clone()))), env, program, knowledge)?
                    } else { infer_with(&args[1], Some(&text), env, program, knowledge)? })
                } else { None };
                let arguments = std::iter::once(Term::from(&dictionary_type))
                    .chain(key_type.as_ref().map(Term::from)).collect::<Vec<_>>();
                if name == "getValue" && let Expr::List(path) = &args[1] {
                    for _ in 1..path.len() {
                        inner = if let Type::Generic(kind, value) = inner {
                            if kind == "Dictionary" { *value } else { named("Value") }
                        } else { named("Value") };
                    }
                }
                let result_type = match name.as_str() {
                    "keys" => Type::Generic("List".into(), Box::new(text.clone())),
                    "values" => Type::Generic("List".into(), Box::new(inner.clone())),
                    "getEntries" => Type::Generic("List".into(), Box::new(Type::Generic("DictionaryEntry".into(), Box::new(inner.clone())))),
                    "size" => named("Number"),
                    "has" | "isEmpty" => named("Bool"),
                    _ => inner.clone(),
                };
                let path = if name == "getValue" {
                    match &args[1] {
                        Expr::String(key) => Some((1, Some(key.as_str()))),
                        Expr::List(items) => Some((items.len(), items.first().and_then(|item| match item {
                            Expr::String(key) => Some(key.as_str()), _ => None,
                        }))),
                        _ => None,
                    }
                } else { None };
                if constraints::dictionary_call_branches(&Constraints::default(), name, &arguments, &Term::from(&result_type), &program.records, path).is_empty() {
                    return Err(format!("invalid {name} dictionary arguments"));
                }
                if name == "getValue" {
                    let path: Vec<_> = match &args[1] {
                        Expr::String(key) => vec![key.as_str()],
                        Expr::List(items) => items.iter().filter_map(|item| if let Expr::String(key) = item { Some(key.as_str()) } else { None }).collect(),
                        _ => Vec::new(),
                    };
                    if matches!(&args[0], Expr::Dictionary(_)) && !path.is_empty() {
                        let mut value = &args[0];
                        for key in path {
                            if let Expr::Dictionary(entries) = value
                                && let Some((_, entry)) = entries.iter().find(|(name, _)| name == key) {
                                value = entry;
                                continue;
                            }
                            value = &args[0];
                            break;
                        }
                        if value != &args[0] && let Ok(ty) = infer_with(value, None, env, program, knowledge) {
                            return Ok(ty);
                        }
                    }
                }
                if name == "getValue" && inner == named("Value") && let Some(target) = expected {
                    inner = target.clone();
                }
                return Ok(match name.as_str() {
                    "keys" => Type::Generic("List".into(), Box::new(text)),
                    "values" => Type::Generic("List".into(), Box::new(inner)),
                    "getEntries" => Type::Generic("List".into(), Box::new(Type::Generic("DictionaryEntry".into(), Box::new(inner)))),
                    "size" => named("Number"),
                    "has" | "isEmpty" => named("Bool"),
                    _ => inner,
                });
            }
            if let Some(definition) = knowledge.iter().find(|item| item.name == *name) {
                if args.len() != definition.params.len() {
                    return Err(format!("knowledge argument count for {name}"));
                }
                let parameters: Vec<_> =
                    definition.params.iter().map(|(_, ty)| ty.clone()).collect();
                return check_call_signature(
                    name,
                    args,
                    &[(parameters, definition.output.clone())],
                    env,
                    program,
                    knowledge,
                )
                .map_err(|error| {
                    if let Some((expected, got)) = error
                        .strip_prefix(&format!("{name} requires "))
                        .and_then(|message| message.split_once(", got "))
                    {
                        format!(
                            "knowledge argument type for {name}: expected {expected}, got {got}"
                        )
                    } else {
                        error
                    }
                });
            }
            if name == "sort" {
                if !matches!(args.len(), 1 | 2) {
                    return Err("invalid sort argument count".into());
                }
                let list = infer_with(&args[0], expected, env, program, knowledge)?;
                let Type::Generic(kind, element) = &list else {
                    return Err(format!("sort requires List<T>, got {list}"));
                };
                if kind != "List" {
                    return Err(format!("sort requires List<T>, got {list}"));
                }
                let signatures = constraints::sort_call_signatures();
                if !signatures
                    .iter()
                    .any(|(parameters, _)| parameters[0] == list)
                {
                    return Err(format!("sort does not support {element}"));
                }
                let mut arguments = vec![Term::from(&list)];
                if let Some(order) = args.get(1) {
                    let hint = if matches!(order, Expr::List(_)) {
                        Some(&list)
                    } else {
                        None
                    };
                    let order = infer_with(order, hint, env, program, knowledge)?;
                    arguments.push(Term::from(&order));
                }
                if constraints::call_branches(
                    &Constraints::default(),
                    &signatures,
                    &arguments,
                    &Term::from(&list),
                )
                .is_empty()
                {
                    return Err(format!("sort requires String or {list} order"));
                }
                return Ok(list);
            }
            if matches!(
                name.as_str(),
                "count"
                    | "isEmpty"
                    | "listContains"
                    | "indexOf"
                    | "sublist"
                    | "append"
                    | "prepend"
                    | "concatenate"
                    | "union"
                    | "distinct"
                    | "duplicateValues"
                    | "intersection"
                    | "reverse"
                    | "flatten"
            ) && (!matches!(name.as_str(), "count" | "isEmpty" | "indexOf" | "reverse")
                || matches!(name.as_str(), "indexOf" | "reverse")
                    && matches!(args.first(), Some(List(items)) if items.is_empty())
                || args.first().is_some_and(|arg| {
                    matches!(
                        infer_with(arg, None, env, program, knowledge),
                        Ok(Type::Generic(ref kind, _)) if kind == "List"
                    )
                }))
            {
                if matches!(name.as_str(), "distinct" | "duplicateValues") && args.len() != 1 {
                    return Err(format!("invalid {name} argument count"));
                }
                let Some(first) = args.first() else {
                    return Err(format!("invalid {name} argument count"));
                };
                let list_hint = if matches!(
                    name.as_str(),
                    "sublist"
                        | "reverse"
                        | "append"
                        | "prepend"
                        | "concatenate"
                        | "union"
                        | "distinct"
                        | "duplicateValues"
                        | "intersection"
                ) {
                    expected
                        .filter(|ty| matches!(ty, Type::Generic(kind, _) if kind == "List"))
                        .cloned()
                } else {
                    None
                };
                let list_hint = if list_hint.is_none()
                    && matches!(first, List(items) if items.is_empty())
                {
                    if matches!(
                        name.as_str(),
                        "listContains" | "indexOf" | "append" | "prepend"
                    ) {
                        args.get(1)
                            .map(|item| infer_with(item, None, env, program, knowledge))
                            .transpose()?
                            .map(|ty| Type::Generic("List".into(), Box::new(ty)))
                    } else if matches!(name.as_str(), "concatenate" | "union" | "intersection") {
                        args.get(1)
                            .map(|arg| infer_with(arg, None, env, program, knowledge))
                            .transpose()?
                    } else {
                        None
                    }
                } else {
                    list_hint
                };
                let list = infer_with(first, list_hint.as_ref(), env, program, knowledge)?;
                let Type::Generic(kind, element) = &list else {
                    return Err(format!("{name} requires List<T>, got {list}"));
                };
                if kind != "List" {
                    return Err(format!("{name} requires List<T>, got {list}"));
                }
                let output = match name.as_str() {
                    "count" => named("Number"),
                    "isEmpty" | "listContains" => named("Bool"),
                    "indexOf" => Type::Generic("List".into(), Box::new(named("Number"))),
                    "sublist" | "append" | "prepend" | "concatenate" | "union" | "intersection"
                    | "distinct" | "duplicateValues" | "reverse" => list.clone(),
                    "flatten" => {
                        let mut leaf = element.as_ref();
                        while let Type::Generic(kind, inner) = leaf {
                            if kind != "List" {
                                break;
                            }
                            leaf = inner;
                        }
                        Type::Generic("List".into(), Box::new(leaf.clone()))
                    }
                    _ => unreachable!(),
                };
                let number = named("Number");
                let mut actual = vec![list.clone()];
                for arg in &args[1..] {
                    let required = if matches!(
                        name.as_str(),
                        "listContains" | "indexOf" | "append" | "prepend"
                    ) {
                        element.as_ref()
                    } else if matches!(name.as_str(), "concatenate" | "union" | "intersection") {
                        &list
                    } else {
                        &number
                    };
                    let got = infer_with(arg, Some(required), env, program, knowledge)?;
                    if &got != required {
                        return Err(format!("{name} requires {required}, got {got}"));
                    }
                    actual.push(got);
                }
                if constraints::list_call_branches(
                    &Constraints::default(),
                    name,
                    &actual.iter().map(Term::from).collect::<Vec<_>>(),
                    &Term::from(&output),
                )
                .is_empty()
                {
                    return Err(format!("invalid {name} argument count"));
                }
                return Ok(output);
            }
            if matches!(
                name.as_str(),
                "daysBetween"
                    | "monthsBetween"
                    | "yearsBetween"
                    | "financialYear"
                    | "financialYearQuarter"
            ) {
                let Some(first) = args.first() else {
                    return Err(format!("{name} requires Date or DateTime"));
                };
                let point = infer_with(first, None, env, program, knowledge)?;
                if !matches!(point, Type::Named(ref ty) if matches!(ty.as_str(), "Date" | "DateTime"))
                {
                    return Err(format!("{name} requires Date or DateTime"));
                }
                let actual = args[1..]
                    .iter()
                    .map(|arg| infer_with(arg, None, env, program, knowledge))
                    .collect::<Result<Vec<_>, _>>()?;
                let types: Vec<_> = std::iter::once(point.clone()).chain(actual).collect();
                let result = named(if name.starts_with("financialYear") {
                    "String"
                } else {
                    "Number"
                });
                if !call_types_match(name, &types, &result) {
                    if name.starts_with("financialYear") {
                        return Err(if args.len() != 2 {
                            format!("{name} requires a financial year basis")
                        } else {
                            "invalid financial year basis type".into()
                        });
                    }
                    if types.get(1).is_some_and(|ty| ty != &point) {
                        return Err(format!("{name} requires two matching points"));
                    }
                    if types.get(2).is_some_and(|ty| ty == &named("Bool")) && point == named("Date")
                        || types.last() == Some(&named("Bool"))
                            && point == named("Date")
                            && types.len() > 3
                    {
                        return Err("includeTime requires DateTime and Bool".into());
                    }
                    return Err(format!("invalid {name} arguments"));
                }
                if name.starts_with("financialYear") {
                    if let Expr::String(value) = &args[1]
                        && !matches!(
                            value.as_str(),
                            "AU" | "UK" | "US" | "IN" | "JP" | "CA" | "NZ"
                        )
                    {
                        return Err(format!("invalid financial year basis: {value}"));
                    }
                    if let Expr::Number(value) = &args[1]
                        && !matches!(value.parse::<u32>(), Ok(1..=12))
                    {
                        return Err(format!("invalid financial year month: {value}"));
                    }
                } else if name != "daysBetween"
                    && let Some(Expr::String(value)) = args.get(2)
                    && !matches!(
                        value.as_str(),
                        "calendar"
                            | "actual/365"
                            | "actual/360"
                            | "actual/actual"
                            | "30/360"
                            | "30E/360"
                    )
                {
                    return Err(format!("invalid day-count basis: {value}"));
                }
                return Ok(result);
            }
            if matches!(
                name.as_str(),
                "isWeekday"
                    | "isWeekend"
                    | "isPublicHoliday"
                    | "isBusinessDay"
                    | "lastDayOfMonth"
                    | "firstDayOfMonth"
                    | "lastDayOfPrevMonth"
                    | "firstDayOfNextMonth"
                    | "firstDayOfWeekInMonth"
                    | "lastDayOfWeekInMonth"
                    | "nthDayOfWeekInMonth"
                    | "nextDayOfWeek"
                    | "prevDayOfWeek"
                    | "nextWeekday"
                    | "prevWeekday"
                    | "nextBusinessDay"
                    | "prevBusinessDay"
                    | "addBusinessDays"
                    | "subtractBusinessDays"
                    | "weekdaysBetween"
                    | "businessDaysBetween"
            ) {
                let Some(first) = args.first() else {
                    return Err(format!("{name} requires Date or DateTime"));
                };
                let point = infer_with(first, None, env, program, knowledge)?;
                if !matches!(point, Type::Named(ref kind) if matches!(kind.as_str(), "Date" | "DateTime"))
                {
                    return Err(format!("{name} requires Date or DateTime"));
                }
                let rest = args[1..]
                    .iter()
                    .map(|arg| infer_with(arg, None, env, program, knowledge))
                    .collect::<Result<Vec<_>, _>>()?;
                let types: Vec<_> = std::iter::once(point.clone()).chain(rest).collect();
                let result = if matches!(
                    name.as_str(),
                    "isWeekday" | "isWeekend" | "isPublicHoliday" | "isBusinessDay"
                ) {
                    named("Bool")
                } else if matches!(name.as_str(), "weekdaysBetween" | "businessDaysBetween") {
                    named("Number")
                } else {
                    point
                };
                if !call_types_match(name, &types, &result) {
                    let signatures = constraints::fixed_call_signatures(name).unwrap();
                    let nearest = signatures
                        .iter()
                        .filter(|(parameters, output)| {
                            output == &result && parameters[0] == types[0]
                        })
                        .max_by_key(|(parameters, _)| {
                            (
                                parameters
                                    .iter()
                                    .zip(&types)
                                    .take_while(|(expected, actual)| *expected == *actual)
                                    .count(),
                                parameters.len(),
                            )
                        });
                    if let Some((parameters, _)) = nearest {
                        if let Some(index) = parameters
                            .iter()
                            .zip(&types)
                            .position(|(expected, actual)| expected != actual)
                        {
                            return Err(match parameters[index].to_string().as_str() {
                                "Date" | "DateTime" => {
                                    format!("{name} requires two matching points")
                                }
                                "Number" => format!("{name} requires Number"),
                                "Calendar" => format!("{name} requires Calendar"),
                                "Bool" => "strictCalendarRange requires Bool".into(),
                                _ => format!("invalid {name} arguments"),
                            });
                        }
                        if parameters.len() > types.len() {
                            if parameters[types.len()] == named("Number") {
                                let numeric = parameters
                                    .iter()
                                    .filter(|ty| **ty == named("Number"))
                                    .count();
                                return Err(format!("{name} requires {numeric} Number arguments"));
                            }
                            if name == "isPublicHoliday" {
                                return Err("isPublicHoliday requires Calendar".into());
                            }
                            if matches!(name.as_str(), "weekdaysBetween" | "businessDaysBetween") {
                                return Err(format!("{name} requires two matching points"));
                            }
                        }
                    }
                    return Err(format!("invalid {name} argument count"));
                }
                return Ok(result);
            }
            if matches!(
                name.as_str(),
                "calendarDrop" | "calendarKeep" | "calendarMerge"
            ) {
                let calendar = named("Calendar");
                if name == "calendarMerge" {
                    let Some(list) = args.first() else {
                        return Err("calendarMerge requires List<Calendar>".into());
                    };
                    let expected = Type::Generic("List".into(), Box::new(calendar.clone()));
                    if infer_with(list, Some(&expected), env, program, knowledge)? != expected {
                        return Err("calendarMerge requires List<Calendar>".into());
                    }
                    let mut types = vec![expected];
                    for option in &args[1..] {
                        let Expr::NamedArgument(key, value) = option else {
                            return Err("calendarMerge options must be named".into());
                        };
                        if !matches!(key.as_str(), "dedupeBy" | "tiebreak") {
                            return Err(format!("unknown calendarMerge option: {key}"));
                        }
                        let ty =
                            infer_with(value, Some(&named("String")), env, program, knowledge)?;
                        if ty != named("String") {
                            return Err(format!("{key} requires String"));
                        }
                        types.push(ty);
                        if let Expr::String(value) = value.as_ref() {
                            let valid = if key == "dedupeBy" {
                                matches!(value.as_str(), "value" | "valueAndName")
                            } else {
                                matches!(value.as_str(), "first" | "name")
                            };
                            if !valid {
                                return Err(format!("invalid {key}: {value}"));
                            }
                        }
                    }
                    if !calendar_call_types_match(name, &types, &calendar) {
                        return Err("invalid calendarMerge arguments".into());
                    }
                } else {
                    let [first, target, options @ ..] = args.as_slice() else {
                        return Err(format!("{name} requires Calendar and target"));
                    };
                    if infer_with(first, Some(&calendar), env, program, knowledge)? != calendar {
                        return Err(format!("{name} requires Calendar"));
                    }
                    fn check_target(
                        name: &str,
                        expr: &Expr,
                        env: &HashMap<std::string::String, Type>,
                        program: &Program,
                        knowledge: &[Knowledge],
                    ) -> Result<(), std::string::String> {
                        match expr {
                            Expr::List(items) => {
                                for item in items {
                                    check_target(name, item, env, program, knowledge)?;
                                }
                                Ok(())
                            }
                            Expr::Call(call, args) if call == "pattern" => {
                                let [pattern] = args.as_slice() else {
                                    return Err("pattern requires one String".into());
                                };
                                if infer_with(pattern, None, env, program, knowledge)?
                                    != Type::Named("String".into())
                                {
                                    return Err("pattern requires String".into());
                                }
                                if let Expr::String(value) = pattern {
                                    regex::Regex::new(value).map_err(|e| e.to_string())?;
                                }
                                if !calendar_call_types_match(
                                    name,
                                    &[Type::Named("Calendar".into()), Type::Named("String".into())],
                                    &Type::Named("Calendar".into()),
                                ) {
                                    return Err("invalid calendar target".into());
                                }
                                Ok(())
                            }
                            other => {
                                let ty = infer_with(other, None, env, program, knowledge)?;
                                if calendar_call_types_match(
                                    name,
                                    &[Type::Named("Calendar".into()), ty.clone()],
                                    &Type::Named("Calendar".into()),
                                ) {
                                    Ok(())
                                } else {
                                    Err(format!("invalid calendar target: {ty}"))
                                }
                            }
                        }
                    }
                    check_target(name, target, env, program, knowledge)?;
                    if options.len() > 1 {
                        return Err(format!("{name} accepts only rangeMatch"));
                    }
                    let mut option_type = None;
                    if let Some(option) = options.first() {
                        let Expr::NamedArgument(key, value) = option else {
                            return Err("rangeMatch must be named".into());
                        };
                        if key != "rangeMatch" {
                            return Err(format!("unknown {name} option: {key}"));
                        }
                        let ty =
                            infer_with(value, Some(&named("String")), env, program, knowledge)?;
                        if ty != named("String") {
                            return Err("rangeMatch requires String".into());
                        }
                        option_type = Some(ty);
                        if let Expr::String(value) = value.as_ref()
                            && !matches!(
                                value.as_str(),
                                "equality" | "entryWithin" | "entryEncloses" | "overlap"
                            )
                        {
                            return Err(format!("invalid rangeMatch: {value}"));
                        }
                    }
                    // Lists and patterns have AST-specific target validation above; each leaf
                    // uses the same call relation, and String is an existential witness here.
                    let mut types = vec![calendar.clone(), named("String")];
                    types.extend(option_type);
                    if !calendar_call_types_match(name, &types, &calendar) {
                        return Err(format!("invalid {name} arguments"));
                    }
                }
                let symbolic_env = env
                    .iter()
                    .map(|(name, ty)| (name.clone(), Term::from(ty)))
                    .collect();
                if constraints::expression_branches(
                    expr,
                    &Term::from(&calendar),
                    &symbolic_env,
                    &program.records,
                    knowledge,
                    &program.enums,
                )
                .is_none_or(|branches| branches.is_empty())
                {
                    return Err(format!("invalid {name} arguments"));
                }
                return Ok(calendar);
            }
            if matches!(name.as_str(), "entryName" | "entryValue") {
                let [entry] = args.as_slice() else {
                    return Err(format!("{name} requires one CalendarEntry"));
                };
                let actual = infer_with(entry, None, env, program, knowledge)?;
                let result = named(if name == "entryName" {
                    "String"
                } else {
                    "CalendarValue"
                });
                if !calendar_call_types_match(name, &[actual], &result) {
                    return Err(format!("{name} requires CalendarEntry"));
                }
                return Ok(result);
            }
            if matches!(
                name.as_str(),
                "count"
                    | "isEmpty"
                    | "entries"
                    | "names"
                    | "find"
                    | "contains"
                    | "entriesFor"
                    | "overlaps"
                    | "entriesIn"
                    | "validFrom"
                    | "validTo"
                    | "validRange"
                    | "next"
                    | "prev"
            ) && args.first().is_some_and(|arg| {
                infer_with(arg, None, env, program, knowledge).ok() == Some(named("Calendar"))
            }) {
                let rest = args[1..]
                    .iter()
                    .map(|arg| infer_with(arg, None, env, program, knowledge))
                    .collect::<Result<Vec<_>, _>>()?;
                let types: Vec<_> = std::iter::once(named("Calendar")).chain(rest).collect();
                let result = match name.as_str() {
                    "count" => named("Number"),
                    "isEmpty" | "contains" | "overlaps" => named("Bool"),
                    "entries" | "find" | "entriesFor" | "entriesIn" => {
                        Type::Generic("List".into(), Box::new(named("CalendarEntry")))
                    }
                    "names" => Type::Generic("List".into(), Box::new(named("String"))),
                    "validFrom" | "validTo" => named("CalendarPoint"),
                    "validRange" => named("CalendarRange"),
                    _ => named("CalendarEntry"),
                };
                if !calendar_call_types_match(name, &types, &result) {
                    if matches!(name.as_str(), "next" | "prev")
                        && types.len() == 3
                        && types[2] != named("Number")
                    {
                        return Err(format!("{name} requires numeric n"));
                    }
                    return Err(format!("invalid {name} arguments"));
                }
                return Ok(result);
            }
            if range_relation(name) {
                let [first, second] = args.as_slice() else {
                    return Err(format!("{name} requires two arguments"));
                };
                if matches!(name.as_str(), "before" | "after" | "meets" | "metBy") {
                    let number = named("Number");
                    let range_type = Type::Generic("Range".into(), Box::new(number.clone()));
                    for (point, range) in [(first, second), (second, first)] {
                        if infer_with(point, None, env, program, knowledge).ok()
                            == Some(number.clone())
                            && infer_with(range, Some(&range_type), env, program, knowledge).ok()
                                == Some(range_type.clone())
                            && !constraints::range_relation_branches(
                                &Constraints::default(),
                                name,
                                &[Term::from(&number), Term::from(&range_type)],
                                &Term::from(&named("Bool")),
                            )
                            .is_empty()
                        {
                            return Ok(named("Bool"));
                        }
                    }
                }
                let (value, range) =
                    if matches!(name.as_str(), "includes" | "startedBy" | "finishedBy") {
                        (second, first)
                    } else {
                        (first, second)
                    };
                if matches!(
                    name.as_str(),
                    "includes" | "during" | "starts" | "startedBy" | "finishes" | "finishedBy"
                ) {
                    let ty = infer_with(value, None, env, program, knowledge)?;
                    let expected = Type::Generic("Range".into(), Box::new(ty));
                    let actual = infer_with(range, Some(&expected), env, program, knowledge)?;
                    if actual != expected {
                        return Err(format!("{name} requires matching scalar and range types"));
                    }
                } else {
                    let ty = if matches!(first, Range(None, None, _, _)) {
                        infer_with(second, None, env, program, knowledge)?
                    } else {
                        infer_with(first, None, env, program, knowledge)?
                    };
                    let first_ty = infer_with(first, Some(&ty), env, program, knowledge)?;
                    let second_ty = infer_with(second, Some(&ty), env, program, knowledge)?;
                    if first_ty != second_ty
                        || !matches!(ty, Type::Generic(ref name, _) if name == "Range")
                    {
                        return Err(format!("{name} requires matching range types"));
                    }
                }
                let symbolic_env = env
                    .iter()
                    .map(|(name, ty)| (name.clone(), Term::from(ty)))
                    .collect();
                if constraints::expression_branches(
                    expr,
                    &Term::from(&named("Bool")),
                    &symbolic_env,
                    &program.records,
                    knowledge,
                    &program.enums,
                )
                .is_none_or(|branches| branches.is_empty())
                {
                    return Err(format!("{name} requires matching range types"));
                }
                return Ok(named("Bool"));
            }
            if name == "dateTime" {
                return Err("dateTime(...) was renamed to datetime(...)".into());
            }
            if matches!(name.as_str(), "date" | "time" | "datetime") {
                let result = named(match name.as_str() {
                    "date" => "Date",
                    "time" => "Time",
                    _ => "DateTime",
                });
                let actual: Vec<_> = args
                    .iter()
                    .map(|arg| infer_with(arg, None, env, program, knowledge))
                    .collect::<Result<_, _>>()?;
                if !call_types_match(name, &actual, &result) {
                    return Err(format!("invalid {name} arguments"));
                }
                match (name.as_str(), args.as_slice()) {
                    ("date" | "time" | "datetime", [Expr::String(value)]) => {
                        let valid = match name.as_str() {
                            "date" => value.parse::<crate::temporal::Date>().is_ok(),
                            "time" => value.parse::<crate::temporal::Time>().is_ok(),
                            _ => value.parse::<crate::temporal::DateTime>().is_ok(),
                        };
                        if !valid {
                            return Err(format!("invalid {name} literal: {value}"));
                        }
                    }
                    ("date" | "time", [_, _, _]) => {
                        if let [Some(Ok(a)), Some(Ok(b)), Some(Ok(c))] = args
                            .iter()
                            .map(constant_number)
                            .collect::<Vec<_>>()
                            .as_slice()
                        {
                            let valid = if name == "date" {
                                crate::temporal::date_from_parts(*a, *b, *c).is_ok()
                            } else {
                                crate::temporal::time_from_parts(*a, *b, *c, None).is_ok()
                            };
                            if !valid {
                                return Err(format!("invalid {name} components"));
                            }
                        }
                    }
                    ("datetime", [_, _]) => {
                        if let [Expr::Call(_, day), Expr::Call(_, time)] = args.as_slice()
                            && let ([Expr::String(day)], [Expr::String(time)]) =
                                (day.as_slice(), time.as_slice())
                        {
                            let day = day.parse::<crate::temporal::Date>()?;
                            let time = time.parse::<crate::temporal::Time>()?;
                            crate::temporal::combine(day, time)?;
                        }
                    }
                    _ => {}
                }
                return Ok(result);
            }
            if matches!(name.as_str(), "today" | "now") {
                let result = named(if name == "today" { "Date" } else { "DateTime" });
                if !args.is_empty() || !call_types_match(name, &[], &result) {
                    return Err(format!("{name} requires no arguments"));
                }
                return Ok(result);
            }
            if matches!(
                name.as_str(),
                "withOffset"
                    | "withTimezone"
                    | "withoutOffset"
                    | "withoutTimezone"
                    | "withoutOffsetOrTimezone"
            ) {
                let actual = args
                    .iter()
                    .map(|arg| infer_with(arg, None, env, program, knowledge))
                    .collect::<Result<Vec<_>, _>>()?;
                let Some(result) = actual.first() else {
                    return Err(format!("invalid {name} arguments"));
                };
                if !call_types_match(name, &actual, result) {
                    return Err(format!("invalid {name} arguments"));
                }
                return Ok(result.clone());
            }
            let text = named("String");
            let number = named("Number");
            if matches!(name.as_str(), "dtDuration" | "ymDuration") {
                let [arg] = args.as_slice() else {
                    return Err(format!("{name} requires one String argument"));
                };
                let actual = infer_with(arg, Some(&text), env, program, knowledge)?;
                let result = named(if name == "dtDuration" {
                    "DTDuration"
                } else {
                    "YMDuration"
                });
                if !call_types_match(name, &[actual], &result) {
                    return Err(format!("{name} requires String"));
                }
                if let Expr::String(value) = arg {
                    if name == "dtDuration" {
                        value.parse::<crate::temporal::DTDuration>()?;
                    } else {
                        value.parse::<crate::temporal::YMDuration>()?;
                    }
                }
                return Ok(result);
            }
            if matches!(name.as_str(), "dtDurationBetween" | "ymDurationBetween") {
                let [from, to] = args.as_slice() else {
                    return Err(format!("{name} requires two points"));
                };
                let first = infer_with(from, None, env, program, knowledge)?;
                let second = infer_with(to, Some(&first), env, program, knowledge)?;
                let result = named(if name == "dtDurationBetween" {
                    "DTDuration"
                } else {
                    "YMDuration"
                });
                if !call_types_match(name, &[first, second], &result) {
                    return Err(format!("{name} requires two Dates or two DateTimes"));
                }
                return Ok(result);
            }
            if matches!(
                name.as_str(),
                "min" | "max" | "sum" | "mean" | "median" | "product" | "stddev" | "mode"
            ) {
                let [values] = args.as_slice() else {
                    return Err(format!("{name} requires one List argument"));
                };
                let hint = expected.and_then(|ty| {
                    if name == "mode" {
                        matches!(ty, Type::Generic(kind, _) if kind == "List").then_some(ty.clone())
                    } else {
                        Some(Type::Generic("List".into(), Box::new(ty.clone())))
                    }
                });
                let default = Type::Generic("List".into(), Box::new(number.clone()));
                let actual = infer_with(
                    values,
                    if matches!(values, Expr::List(items) if items.is_empty())
                        || matches!(values, Expr::Iteration { .. })
                    {
                        Some(hint.as_ref().unwrap_or(&default))
                    } else {
                        None
                    },
                    env,
                    program,
                    knowledge,
                )?;
                let Type::Generic(kind, element) = &actual else {
                    return Err(format!("{name} requires a List"));
                };
                if kind != "List"
                    || (name != "mode"
                        && !call_types_match(name, std::slice::from_ref(&actual), element))
                    || (name == "mode"
                        && constraints::list_call_branches(
                            &Constraints::default(),
                            name,
                            &[Term::from(&actual)],
                            &Term::from(&actual),
                        )
                        .is_empty())
                {
                    return Err(format!("{name} does not support {actual}"));
                }
                return Ok(if name == "mode" {
                    actual
                } else {
                    *element.clone()
                });
            }
            if name == "number" {
                check_call_signature(
                    name,
                    args,
                    &constraints::fixed_call_signatures(name).unwrap(),
                    env,
                    program,
                    knowledge,
                )?;
                if let [Expr::String(group), Expr::String(decimal)] = &args[1..] {
                    crate::number_ops::number("0", Some((group, decimal)))?;
                }
                if let Some(values) = args
                    .iter()
                    .map(|arg| match arg {
                        Expr::String(value) => Some(value.as_str()),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()
                {
                    crate::number_ops::number(
                        values[0],
                        if values.len() == 3 {
                            Some((values[1], values[2]))
                        } else {
                            None
                        },
                    )?;
                }
                return Ok(number);
            }
            if matches!(name.as_str(), "abs" | "isNegative") && args.len() == 1 {
                let actual = infer_with(&args[0], None, env, program, knowledge)?;
                if matches!(actual, Type::Named(ref ty) if matches!(ty.as_str(), "DTDuration" | "YMDuration"))
                {
                    let result = if name == "abs" {
                        actual.clone()
                    } else {
                        named("Bool")
                    };
                    if call_types_match(name, &[actual], &result) {
                        return Ok(result);
                    }
                }
            }
            if matches!(
                name.as_str(),
                "round"
                    | "roundUp"
                    | "roundDown"
                    | "roundHalfUp"
                    | "roundHalfDown"
                    | "roundHalfEven"
            ) && args.len() == 2
            {
                let first = infer_with(&args[0], None, env, program, knowledge)?;
                if matches!(first, Type::Named(ref ty) if matches!(ty.as_str(), "DTDuration" | "YMDuration"))
                {
                    let second = infer_with(&args[1], Some(&first), env, program, knowledge)?;
                    if !call_types_match(name, &[first.clone(), second], &first) {
                        return Err(format!("{name} requires two matching durations"));
                    }
                    return Ok(first);
                }
            }
            if matches!(
                name.as_str(),
                "round"
                    | "roundUp"
                    | "roundDown"
                    | "roundHalfUp"
                    | "roundHalfDown"
                    | "roundHalfEven"
                    | "floor"
                    | "ceiling"
            ) {
                check_call_signature(
                    name,
                    args,
                    &constraints::fixed_call_signatures(name).unwrap(),
                    env,
                    program,
                    knowledge,
                )?;
                if let Some(Some(scale)) = args.get(1).map(constant_number) {
                    crate::number_ops::round(Decimal::ZERO, scale?, name)?;
                }
                return Ok(number);
            }
            if matches!(
                name.as_str(),
                "abs"
                    | "modulo"
                    | "sqrt"
                    | "exp"
                    | "ln"
                    | "log"
                    | "clamp"
                    | "odd"
                    | "even"
                    | "isPositive"
                    | "isNegative"
                    | "isZero"
            ) {
                let output = check_call_signature(
                    name,
                    args,
                    &constraints::fixed_call_signatures(name).unwrap(),
                    env,
                    program,
                    knowledge,
                )?;
                if let Some(constants) =
                    args.iter().map(constant_number).collect::<Option<Vec<_>>>()
                {
                    let constants = constants.into_iter().collect::<Result<Vec<_>, _>>()?;
                    if matches!(
                        name.as_str(),
                        "odd" | "even" | "isPositive" | "isNegative" | "isZero"
                    ) {
                        crate::number_ops::predicate(name, constants[0])?;
                    } else {
                        crate::number_ops::math(name, &constants)?;
                    }
                }
                return Ok(output);
            }
            let texts = Type::Generic("List".into(), Box::new(text.clone()));
            if name == "string" {
                let [value] = args.as_slice() else {
                    return Err("string requires one argument".into());
                };
                let ty = infer_with(value, None, env, program, knowledge)?;
                if !call_types_match(name, std::slice::from_ref(&ty), &text) {
                    return Err(format!("string cannot convert {ty}"));
                }
                return Ok(text);
            }
            if name == "split" {
                let [value, delimiters] = args.as_slice() else {
                    return Err("split requires two arguments".into());
                };
                if infer_with(value, Some(&text), env, program, knowledge)? != text {
                    return Err("split requires String input".into());
                }
                let delimiter_type = match delimiters {
                    List(_) => infer_with(delimiters, Some(&texts), env, program, knowledge)?,
                    _ => infer_with(delimiters, None, env, program, knowledge)?,
                };
                if !call_types_match(name, &[text.clone(), delimiter_type], &texts) {
                    return Err("split requires String or List<String> delimiters".into());
                }
                return Ok(texts);
            }
            if name == "zipStringJoin" {
                if !matches!(args.len(), 1 | 2 | 4) {
                    return Err("invalid zipStringJoin argument count".into());
                }
                let columns = Type::Generic("List".into(), Box::new(texts.clone()));
                let actual = args
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        let hint = if index == 0 {
                            Some(&columns)
                        } else if matches!(arg, List(_)) {
                            Some(&texts)
                        } else {
                            Some(&text)
                        };
                        infer_with(arg, hint, env, program, knowledge)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if !call_types_match(name, &actual, &texts) {
                    return Err("zipStringJoin requires String columns and String or List<String> formatting".into());
                }
                return Ok(texts);
            }
            let Some(signatures) = constraints::fixed_call_signatures(name) else {
                return Err(format!("unknown knowledge model: {name}"));
            };
            let output = check_call_signature(name, args, &signatures, env, program, knowledge)?;
            if matches!(name.as_str(), "matches" | "replace" | "extract") {
                let flags = args.get(if name == "replace" { 3 } else { 2 });
                let flags = match flags {
                    Some(String(value)) if !value.chars().all(|c| matches!(c, 'i' | 'm' | 's')) => {
                        return Err(format!("invalid regex flag: {value}"));
                    }
                    Some(String(value)) => value.as_str(),
                    _ => "",
                };
                if let String(pattern) = &args[1] {
                    let mut builder = regex::RegexBuilder::new(pattern);
                    builder
                        .case_insensitive(flags.contains('i'))
                        .multi_line(flags.contains('m'))
                        .dot_matches_new_line(flags.contains('s'));
                    builder
                        .build()
                        .map_err(|error| format!("invalid regex: {error}"))?;
                }
            }
            Ok(output)
        }
        Field(base, field) => {
            if let Dictionary(entries) = base.as_ref() {
                infer_with(base, None, env, program, knowledge)?;
                let mut local = env.clone();
                for (key, value) in entries {
                    let ty = infer_with(value, None, &local, program, knowledge)?;
                    if key == field {
                        return Ok(ty);
                    }
                    if crate::compiler::identifier(key) {
                        local.insert(key.clone(), ty);
                    }
                }
            }
            if let Name(name) = base.as_ref()
                && !env.contains_key(name)
                && let Some(item) = program.enums.iter().find(|item| item.name == *name)
            {
                return if item.variants.contains(field) {
                    Ok(named(name))
                } else {
                    Err(format!("unknown enum variant: {name}.{field}"))
                };
            }
            let ty = infer_with(base, None, env, program, knowledge)?;
            if let Type::Generic(kind, inner) = &ty {
                if kind == "List" {
                    let projection_supported = match inner.as_ref() {
                        Type::Generic(kind, _) => {
                            matches!(kind.as_str(), "Dictionary" | "DictionaryEntry")
                        }
                        Type::Named(name) => {
                            matches!(name.as_str(), "Dictionary" | "Value")
                                || program.records.iter().any(|record| record.name == *name)
                        }
                    };
                    if !projection_supported {
                        return Err(format!(
                            "field projection requires dictionary elements, got {inner}"
                        ));
                    }
                    if let Expr::List(elements) = base.as_ref() {
                        for element in elements {
                            if let Expr::Dictionary(entries) = element
                                && !entries.iter().any(|(key, _)| key == field)
                            {
                                return Err(format!("missing dictionary field: {field}"));
                            }
                        }
                    }
                    let item = infer_with(
                        &Expr::Field(Box::new(Expr::Name("item".into())), field.clone()),
                        None,
                        &HashMap::from([("item".into(), *inner.clone())]),
                        program,
                        knowledge,
                    )?;
                    let projected = Type::Generic("List".into(), Box::new(item));
                    if constraints::field_branches(
                        &Constraints::default(),
                        field,
                        &Term::from(&ty),
                        &Term::from(&projected),
                        &program.records,
                    )
                    .is_empty()
                    {
                        return Err(format!("unknown field: {field}"));
                    }
                    return Ok(projected);
                }
                if matches!(kind.as_str(), "Dictionary" | "DictionaryEntry") {
                    let result = Term::Var(0);
                    let branches = constraints::field_branches(
                        &Constraints::default(),
                        field,
                        &Term::from(&ty),
                        &result,
                        &program.records,
                    );
                    return Constraints::unique_type(&branches, &result)
                        .ok_or_else(|| format!("unknown {kind} field: {field}"));
                }
            }
            if let Type::Named(name) = ty {
                if matches!(name.as_str(), "Dictionary" | "Value") {
                    let result = expected.cloned().unwrap_or_else(|| named("Value"));
                    if constraints::field_branches(
                        &Constraints::default(),
                        field,
                        &Term::from(&named(&name)),
                        &Term::from(&result),
                        &program.records,
                    )
                    .is_empty()
                    {
                        return Err(format!("unknown field: {field}"));
                    }
                    return Ok(result);
                }
                if matches!(
                    name.as_str(),
                    "Date" | "Time" | "DateTime" | "DTDuration" | "YMDuration"
                ) {
                    let result = Term::Var(0);
                    let branches = constraints::field_branches(
                        &Constraints::default(),
                        field,
                        &Term::from(&named(&name)),
                        &result,
                        &program.records,
                    );
                    if let Some(ty) = Constraints::unique_type(&branches, &result) {
                        return Ok(ty);
                    }
                }
                if program.records.iter().any(|item| item.name == name) {
                    let result = Term::Var(0);
                    let branches = constraints::field_branches(
                        &Constraints::default(),
                        field,
                        &Term::from(&named(&name)),
                        &result,
                        &program.records,
                    );
                    return Constraints::unique_type(&branches, &result)
                        .ok_or_else(|| format!("unknown field: {name}.{field}"));
                }
            }
            Err(format!("unknown field: {field}"))
        }
        Range(lower, upper, _, _) => {
            let context = match expected {
                Some(Type::Generic(name, inner)) if name == "Range" => Some(inner.as_ref().clone()),
                _ => None,
            };
            let ty = if let Some(bound) = lower.as_ref().or(upper.as_ref()) {
                infer_with(bound, context.as_ref(), env, program, knowledge)?
            } else {
                context.ok_or("cannot infer unbounded range type")?
            };
            if !matches!(&ty, Type::Named(name) if matches!(name.as_str(), "Number" | "Date" | "DateTime" | "Time"))
            {
                return Err(format!("unsupported range bound type: {ty}"));
            }
            for bound in lower.iter().chain(upper.iter()) {
                let actual = infer_with(bound, Some(&ty), env, program, knowledge)?;
                if actual != ty {
                    return Err(format!(
                        "range bounds require matching types, got {ty} and {actual}"
                    ));
                }
            }
            let range_type = Type::Generic("Range".into(), Box::new(ty.clone()));
            if constraints::range_branches(
                &Constraints::default(),
                &lower
                    .iter()
                    .chain(upper.iter())
                    .map(|_| Term::from(&ty))
                    .collect::<Vec<_>>(),
                &Term::from(&range_type),
            )
            .is_empty()
            {
                return Err(format!("unsupported range bound type: {ty}"));
            }
            if let (Some(a), Some(b)) = (lower, upper)
                && reversed_constants(a, b, &ty)
            {
                return Err("inverted range bounds".into());
            }
            let symbolic_env = env
                .iter()
                .map(|(name, ty)| (name.clone(), Term::from(ty)))
                .collect();
            if constraints::expression_branches(
                expr,
                &Term::from(&range_type),
                &symbolic_env,
                &program.records,
                knowledge,
                &program.enums,
            )
            .is_some_and(|branches| branches.is_empty())
            {
                return Err("range bounds require matching types".into());
            }
            Ok(range_type)
        }
        Iteration {
            every,
            flatten,
            binding,
            source,
            body,
        } => {
            check_name(binding)?;
            let resolved = empty_iteration_element(expr, expected, env, program, knowledge)?;
            let source_element = resolved.unwrap_or_else(|| {
                if !*every {
                    match expected {
                        Some(Type::Generic(kind, element)) if kind == "List" => *element.clone(),
                        _ => named("Value"),
                    }
                } else {
                    named("Value")
                }
            });
            let empty_source = Type::Generic("List".into(), Box::new(source_element));
            let context = if matches!(source.as_ref(), Expr::List(elements) if elements.is_empty())
            {
                Some(&empty_source)
            } else {
                None
            };
            let Type::Generic(kind, element) =
                infer_with(source, context, env, program, knowledge)?
            else {
                return Err("iteration source must be List".into());
            };
            if kind != "List" {
                return Err("iteration source must be List".into());
            }
            let mut local = env.clone();
            let binding_type = *element;
            local.insert(binding.clone(), binding_type.clone());
            let boolean = named("Bool");
            let result_type = if *every {
                Some(&boolean)
            } else if let Some(Type::Generic(kind, inner)) = expected {
                if kind == "List" {
                    if *flatten {
                        Some(expected.unwrap())
                    } else {
                        Some(inner.as_ref())
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let result = infer_with(body, result_type, &local, program, knowledge)?;
            if *every && result != named("Bool") {
                return Err("every predicate must be Bool".into());
            }
            if *flatten && !matches!(result, Type::Generic(ref kind, _) if kind == "List") {
                return Err("iteration binding body must be List".into());
            }
            let output = if *every {
                named("Bool")
            } else if *flatten {
                result.clone()
            } else {
                Type::Generic("List".into(), Box::new(result.clone()))
            };
            if constraints::iteration_branches(
                &Constraints::default(),
                &Term::from(&Type::Generic(
                    "List".into(),
                    Box::new(binding_type.clone()),
                )),
                &Term::from(&binding_type),
                &Term::from(&result),
                &Term::from(&output),
                if *every {
                    constraints::IterationKind::Every
                } else if *flatten {
                    constraints::IterationKind::Flatten
                } else {
                    constraints::IterationKind::Map
                },
                &Term::Var(0),
            )
            .is_empty()
            {
                return Err("iteration source and body have incompatible types".into());
            }
            Ok(output)
        }
        List(elements) => {
            let element_type = if let Some(Type::Generic(name, inner)) = expected {
                if name == "List" {
                    Some(inner.as_ref().clone())
                } else {
                    None
                }
            } else {
                None
            };
            let element_type = element_type
                .or_else(|| {
                    elements
                        .first()
                        .and_then(|element| infer_with(element, None, env, program, knowledge).ok())
                })
                .ok_or("cannot infer empty list type")?;
            for element in elements {
                let actual = infer_with(element, Some(&element_type), env, program, knowledge)?;
                if constraints::list_branches(
                    &Constraints::default(),
                    &[Term::from(&actual)],
                    &Term::from(&Type::Generic(
                        "List".into(),
                        Box::new(element_type.clone()),
                    )),
                )
                .is_empty()
                    && !(element_type == named("Dictionary")
                        && (matches!(actual, Type::Generic(ref kind, _) if kind == "Dictionary")
                            || matches!(actual, Type::Named(ref kind) if program.records.iter().any(|record| &record.name == kind))))
                {
                    return Err(format!("List<{element_type}> element has type {actual}"));
                }
            }
            Ok(Type::Generic("List".into(), Box::new(element_type)))
        }
        Not(value) => {
            let ty = infer_with(value, None, env, program, knowledge)?;
            if constraints::not_branches(
                &Constraints::default(),
                &Term::from(&ty),
                &Term::from(&named("Bool")),
            )
            .is_empty()
            {
                return Err(format!("not requires Bool, got {ty}"));
            }
            Ok(named("Bool"))
        }
        Binary(left, op, right) => {
            if op == "in" {
                let lhs = infer_with(left, None, env, program, knowledge)?;
                if matches!(lhs, Type::Named(ref name) if matches!(name.as_str(), "Date" | "DateTime"))
                {
                    let rhs = infer_with(right, None, env, program, knowledge);
                    if let Ok(rhs) = rhs
                        && !constraints::binary_branches(
                            &Constraints::default(),
                            op,
                            false,
                            &Term::from(&lhs),
                            &Term::from(&rhs),
                            &Term::from(&named("Bool")),
                        )
                        .is_empty()
                    {
                        return Ok(named("Bool"));
                    }
                }
                let kind = if matches!(right.as_ref(), List(..))
                    || matches!(infer_with(right, None, env, program, knowledge), Ok(Type::Generic(ref kind, _)) if kind == "List")
                    || lhs == named("String") && !matches!(right.as_ref(), Range(..))
                {
                    "List"
                } else {
                    "Range"
                };
                let expected = Type::Generic(kind.into(), Box::new(lhs.clone()));
                let rhs = infer_with(right, Some(&expected), env, program, knowledge)?;
                if constraints::binary_branches(
                    &Constraints::default(),
                    op,
                    false,
                    &Term::from(&lhs),
                    &Term::from(&rhs),
                    &Term::from(&named("Bool")),
                )
                .is_empty()
                {
                    return Err(format!("in requires a {kind} of {lhs}, got {rhs}"));
                }
                return Ok(named("Bool"));
            }
            let mut lhs = if matches!(left.as_ref(), Range(None, None, _, _))
                || matches!(left.as_ref(), List(elements) if elements.is_empty())
            {
                let other = infer_with(right, None, env, program, knowledge)?;
                infer_with(left, Some(&other), env, program, knowledge)?
            } else {
                infer_with(left, None, env, program, knowledge)?
            };
            if lhs == named("Value") {
                let other = infer_with(right, None, env, program, knowledge)?;
                if other != named("Value") {
                    lhs = infer_with(left, Some(&other), env, program, knowledge)?;
                }
            }
            let duration = |ty: &Type| matches!(ty, Type::Named(name) if matches!(name.as_str(), "DTDuration" | "YMDuration"));
            let number = named("Number");
            let rhs = infer_with(
                right,
                Some(if duration(&lhs) && matches!(op.as_str(), "*" | "/") {
                    &number
                } else {
                    &lhs
                }),
                env,
                program,
                knowledge,
            )?;
            let result = Term::Var(usize::MAX);
            let branches = constraints::binary_branches(
                &Constraints::default(),
                op,
                matches!(left.as_ref(), Expr::Number(value) if value == "0"),
                &Term::from(&lhs),
                &Term::from(&rhs),
                &result,
            );
            if let Some(ty) = Constraints::unique_type(&branches, &result) {
                if lhs == named("Number")
                    && rhs == named("Number")
                    && matches!(op.as_str(), "+" | "-" | "*" | "/" | "**" | "mod")
                {
                    if op == "/"
                        && constant_number(right).is_some_and(|value| value == Ok(Decimal::ZERO))
                    {
                        return Err("division by zero".into());
                    }
                    if let Some(Err(error)) = constant_number(expr) {
                        return Err(format!("invalid constant arithmetic: {error}"));
                    }
                }
                return Ok(ty);
            }
            if lhs == named("Time")
                && rhs == named("YMDuration")
                && matches!(op.as_str(), "+" | "-")
            {
                return Err("Time only supports DTDuration".into());
            }
            if lhs != rhs {
                return Err(format!("{op} requires matching types, got {lhs} and {rhs}"));
            }
            if lhs == named("Time") && op == "-" {
                return Err("Time point subtraction is not supported".into());
            }
            if duration(&lhs) {
                return Err(format!("{op} does not support {lhs}"));
            }
            Err(format!(
                "{op} does not support {lhs}; expected Bool for boolean operations"
            ))
        }
    }
}

pub(crate) fn builtin(name: &str) -> bool {
    matches!(
        name,
        "keys"
            | "values"
            | "getEntries"
            | "size"
            | "has"
            | "getValue"
            | "dictionaryPut"
            | "dictionaryMerge"
            | "dictionaryRemove"
    ) || matches!(
        name,
        "daysBetween"
            | "monthsBetween"
            | "yearsBetween"
            | "financialYear"
            | "financialYearQuarter"
            | "isWeekday"
            | "isWeekend"
            | "isPublicHoliday"
            | "isBusinessDay"
            | "lastDayOfMonth"
            | "firstDayOfMonth"
            | "lastDayOfPrevMonth"
            | "firstDayOfNextMonth"
            | "firstDayOfWeekInMonth"
            | "lastDayOfWeekInMonth"
            | "nthDayOfWeekInMonth"
            | "nextDayOfWeek"
            | "prevDayOfWeek"
            | "nextWeekday"
            | "prevWeekday"
            | "nextBusinessDay"
            | "prevBusinessDay"
            | "addBusinessDays"
            | "subtractBusinessDays"
            | "weekdaysBetween"
            | "businessDaysBetween"
            | "calendarDrop"
            | "calendarKeep"
            | "calendarMerge"
            | "pattern"
            | "count"
            | "listContains"
            | "sublist"
            | "append"
            | "prepend"
            | "concatenate"
            | "union"
            | "distinct"
            | "duplicateValues"
            | "intersection"
            | "flatten"
            | "sort"
            | "seq"
            | "all"
            | "any"
            | "insertBefore"
            | "insertAfter"
            | "remove"
            | "listReplace"
            | "entries"
            | "names"
            | "find"
            | "entriesFor"
            | "entriesIn"
            | "validFrom"
            | "validTo"
            | "validRange"
            | "entryValue"
            | "entryName"
            | "next"
            | "prev"
            | "dtDurationBetween"
            | "ymDurationBetween"
            | "withOffset"
            | "withTimezone"
            | "withoutOffset"
            | "withoutTimezone"
            | "withoutOffsetOrTimezone"
            | "round"
            | "roundUp"
            | "roundDown"
            | "roundHalfUp"
            | "roundHalfDown"
            | "roundHalfEven"
            | "floor"
            | "ceiling"
            | "min"
            | "max"
            | "sum"
            | "mean"
            | "median"
            | "product"
            | "stddev"
            | "mode"
            | "number"
            | "abs"
            | "modulo"
            | "sqrt"
            | "exp"
            | "ln"
            | "log"
            | "clamp"
            | "odd"
            | "even"
            | "isPositive"
            | "isNegative"
            | "isZero"
            | "string"
            | "stringJoin"
            | "zipStringJoin"
            | "stringLength"
            | "substring"
            | "substringBefore"
            | "substringAfter"
            | "upperCase"
            | "lowerCase"
            | "trim"
            | "trimLeading"
            | "trimTrailing"
            | "contains"
            | "startsWith"
            | "endsWith"
            | "matches"
            | "replace"
            | "split"
            | "extract"
            | "isBlank"
            | "isEmpty"
            | "indexOf"
            | "charAt"
            | "reverse"
            | "padLeading"
            | "padTrailing"
            | "repeat"
    )
}

pub(super) fn range_relation(name: &str) -> bool {
    matches!(
        name,
        "before"
            | "after"
            | "meets"
            | "metBy"
            | "overlaps"
            | "overlapsBefore"
            | "overlapsAfter"
            | "includes"
            | "during"
            | "starts"
            | "startedBy"
            | "finishes"
            | "finishedBy"
            | "coincides"
    )
}

pub(super) fn reversed_constants(a: &Expr, b: &Expr, ty: &Type) -> bool {
    match (a, b, ty) {
        (Expr::Number(a), Expr::Number(b), Type::Named(name)) if name == "Number" => {
            Decimal::from_str(a)
                .ok()
                .zip(Decimal::from_str(b).ok())
                .is_some_and(|(a, b)| a > b)
        }
        (Expr::Call(a_name, a), Expr::Call(b_name, b), Type::Named(name)) if a_name == b_name => {
            let ([Expr::String(a)], [Expr::String(b)]) = (a.as_slice(), b.as_slice()) else {
                return false;
            };
            match name.as_str() {
                "Date" => NaiveDate::parse_from_str(a, "%Y-%m-%d")
                    .ok()
                    .zip(NaiveDate::parse_from_str(b, "%Y-%m-%d").ok())
                    .is_some_and(|(a, b)| a > b),
                "Time" => NaiveTime::parse_from_str(a, "%H:%M:%S%.f")
                    .ok()
                    .zip(NaiveTime::parse_from_str(b, "%H:%M:%S%.f").ok())
                    .is_some_and(|(a, b)| a > b),
                "DateTime" => DateTime::parse_from_rfc3339(a)
                    .ok()
                    .zip(DateTime::parse_from_rfc3339(b).ok())
                    .is_some_and(|(a, b)| a > b),
                _ => false,
            }
        }
        _ => false,
    }
}

pub(super) fn resolve(ty: &Type, names: &HashSet<&str>) -> Result<(), String> {
    match ty {
        Type::Named(name) if names.contains(name.as_str()) && name != "List" => Ok(()),
        Type::Generic(name, inner)
            if matches!(name.as_str(), "List" | "Dictionary" | "DictionaryEntry") =>
        {
            resolve(inner, names)
        }
        Type::Named(name) => Err(format!("unknown type: {name}")),
        Type::Generic(_, _) => Err(format!("unsupported type: {ty}")),
    }
}
