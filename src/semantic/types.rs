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

pub(super) fn check_name(name: &str) -> Result<(), String> {
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

pub(super) fn infer_with(
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
            Decimal::from_str(value).map_err(|_| format!("invalid Number: {value}"))?;
            Ok(named("Number"))
        }
        String(_) => Ok(named("String")),
        Bool(_) => Ok(named("Bool")),
        Name(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown name: {name}")),
        Call(name, args) => {
            if let Some(definition) = knowledge.iter().find(|item| item.name == *name) {
                if args.len() != definition.params.len() {
                    return Err(format!("knowledge argument count for {name}"));
                }
                for (arg, (_, ty)) in args.iter().zip(&definition.params) {
                    let actual = infer_with(arg, Some(ty), env, program, knowledge)?;
                    if actual != *ty {
                        return Err(format!(
                            "knowledge argument type for {name}: expected {ty}, got {actual}"
                        ));
                    }
                }
                return Ok(definition.output.clone());
            }
            if range_relation(name) {
                let [first, second] = args.as_slice() else {
                    return Err(format!("{name} requires two arguments"));
                };
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
                return Ok(named("Bool"));
            }
            if matches!(name.as_str(), "date" | "time" | "dateTime") {
                let [Expr::String(value)] = args.as_slice() else {
                    return Err(format!("{name} requires a string literal"));
                };
                let valid = match name.as_str() {
                    "date" => NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok(),
                    "time" => {
                        valid_time_format(value)
                            && NaiveTime::parse_from_str(value, "%H:%M:%S%.f")
                                .is_ok_and(|time| time.nanosecond() < 1_000_000_000)
                    }
                    _ => DateTime::parse_from_rfc3339(value).is_ok(),
                };
                if !valid {
                    return Err(format!("invalid {name} literal: {value}"));
                }
                return Ok(named(match name.as_str() {
                    "date" => "Date",
                    "time" => "Time",
                    _ => "DateTime",
                }));
            }
            let text = named("String");
            let number = named("Number");
            let boolean = named("Bool");
            let texts = Type::Generic("List".into(), Box::new(text.clone()));
            if name == "string" {
                let [value] = args.as_slice() else {
                    return Err("string requires one argument".into());
                };
                let ty = infer_with(value, None, env, program, knowledge)?;
                if !matches!(&ty, Type::Named(n) if matches!(n.as_str(), "String" | "Number" | "Bool" | "Date" | "Time" | "DateTime"))
                {
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
                if delimiter_type != text && delimiter_type != texts {
                    return Err("split requires String or List<String> delimiters".into());
                }
                return Ok(texts);
            }
            let (parameters, optional, output) = match name.as_str() {
                "stringJoin" => (vec![texts, text.clone()], false, text.clone()),
                "stringLength" | "indexOf" => (
                    if name == "indexOf" {
                        vec![text.clone(), text.clone()]
                    } else {
                        vec![text.clone()]
                    },
                    false,
                    number.clone(),
                ),
                "substring" => (
                    vec![text.clone(), number.clone(), number.clone()],
                    true,
                    text.clone(),
                ),
                "charAt" => (vec![text.clone(), number.clone()], false, text.clone()),
                "padLeading" | "padTrailing" => (
                    vec![text.clone(), number.clone(), text.clone()],
                    true,
                    text.clone(),
                ),
                "repeat" => (vec![text.clone(), number.clone()], false, text.clone()),
                "substringBefore" | "substringAfter" => {
                    (vec![text.clone(), text.clone()], false, text.clone())
                }
                "upperCase" | "lowerCase" | "trim" | "trimLeading" | "trimTrailing" | "reverse" => {
                    (vec![text.clone()], false, text.clone())
                }
                "contains" | "startsWith" | "endsWith" => {
                    (vec![text.clone(), text.clone()], false, boolean.clone())
                }
                "isBlank" | "isEmpty" => (vec![text.clone()], false, boolean.clone()),
                "matches" => (
                    vec![text.clone(), text.clone(), text.clone()],
                    true,
                    boolean.clone(),
                ),
                "replace" => (
                    vec![text.clone(), text.clone(), text.clone(), text.clone()],
                    true,
                    text.clone(),
                ),
                "extract" => (
                    vec![text.clone(), text.clone(), text.clone()],
                    true,
                    Type::Generic(
                        "List".into(),
                        Box::new(Type::Generic("List".into(), Box::new(text.clone()))),
                    ),
                ),
                _ => return Err(format!("unknown knowledge model: {name}")),
            };
            if args.len() != parameters.len() && !(optional && args.len() == parameters.len() - 1) {
                return Err(format!(
                    "{name} requires {}{} arguments",
                    parameters.len() - usize::from(optional),
                    if optional { " or one more" } else { "" }
                ));
            }
            for (arg, ty) in args.iter().zip(&parameters) {
                let actual = infer_with(arg, Some(ty), env, program, knowledge)?;
                if actual != *ty {
                    return Err(format!("{name} requires {ty}, got {actual}"));
                }
            }
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
            if let Name(name) = base.as_ref()
                && let Some(item) = program.enums.iter().find(|item| item.name == *name)
            {
                return if item.variants.contains(field) {
                    Ok(named(name))
                } else {
                    Err(format!("unknown enum variant: {name}.{field}"))
                };
            }
            let ty = infer_with(base, None, env, program, knowledge)?;
            if let Type::Named(name) = ty
                && let Some(record) = program.records.iter().find(|item| item.name == name)
            {
                return record
                    .fields
                    .iter()
                    .find(|(key, _)| key == field)
                    .map(|(_, value)| value.clone())
                    .ok_or_else(|| format!("unknown field: {name}.{field}"));
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
            if let (Some(a), Some(b)) = (lower, upper)
                && reversed_constants(a, b, &ty)
            {
                return Err("inverted range bounds".into());
            }
            Ok(Type::Generic("Range".into(), Box::new(ty)))
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
                if actual != element_type {
                    return Err(format!("List<{element_type}> element has type {actual}"));
                }
            }
            Ok(Type::Generic("List".into(), Box::new(element_type)))
        }
        Not(value) => {
            let ty = infer_with(value, None, env, program, knowledge)?;
            if ty != named("Bool") {
                return Err(format!("not requires Bool, got {ty}"));
            }
            Ok(named("Bool"))
        }
        Binary(left, op, right) => {
            if op == "in" {
                let lhs = infer_with(left, None, env, program, knowledge)?;
                let kind = if lhs == named("String") && !matches!(right.as_ref(), Range(..)) {
                    "List"
                } else {
                    "Range"
                };
                let expected = Type::Generic(kind.into(), Box::new(lhs.clone()));
                let rhs = infer_with(right, Some(&expected), env, program, knowledge)?;
                if rhs != expected {
                    return Err(format!("in requires a {kind} of {lhs}, got {rhs}"));
                }
                return Ok(named("Bool"));
            }
            let lhs = if matches!(left.as_ref(), Range(None, None, _, _))
                || matches!(left.as_ref(), List(elements) if elements.is_empty())
            {
                let other = infer_with(right, None, env, program, knowledge)?;
                infer_with(left, Some(&other), env, program, knowledge)?
            } else {
                infer_with(left, None, env, program, knowledge)?
            };
            let rhs = infer_with(right, Some(&lhs), env, program, knowledge)?;
            if lhs != rhs {
                return Err(format!("{op} requires matching types, got {lhs} and {rhs}"));
            }
            match op.as_str() {
                "and" | "or" if lhs == named("Bool") => Ok(named("Bool")),
                "+" if lhs == named("String") => Ok(named("String")),
                "==" | "!=" => Ok(named("Bool")),
                ">" | ">=" | "<" | "<="
                    if lhs == named("Number")
                        || lhs == named("String")
                        || lhs == named("DateTime")
                        || lhs == named("Date")
                        || lhs == named("Time") =>
                {
                    Ok(named("Bool"))
                }
                _ => Err(format!(
                    "{op} does not support {lhs}; expected Bool for boolean operations"
                )),
            }
        }
    }
}

pub(super) fn valid_time_format(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() >= 8
        && b[2] == b':'
        && b[5] == b':'
        && [0, 1, 3, 4, 6, 7].iter().all(|&i| b[i].is_ascii_digit())
        && (b.len() == 8 || (b.len() > 9 && b[8] == b'.' && b[9..].iter().all(u8::is_ascii_digit)))
}

pub(crate) fn string_builtin(name: &str) -> bool {
    matches!(
        name,
        "string"
            | "stringJoin"
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
        Type::Generic(name, inner) if name == "List" => resolve(inner, names),
        Type::Named(name) => Err(format!("unknown type: {name}")),
        Type::Generic(_, _) => Err(format!("unsupported type: {ty}")),
    }
}
