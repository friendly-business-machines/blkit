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
        Call(name, args) => {
            if name == "__bl_dictionary" {
                let schema = expected.and_then(|ty| match ty {
                    Type::Named(name) => program.records.iter().find(|record| &record.name == name),
                    _ => None,
                });
                if let Some(schema) = schema
                    && schema.fields.len() != args.len() / 2
                {
                    return Err(format!("dictionary {} requires all fields", schema.name));
                }
                let element_type = match expected {
                    Some(Type::Generic(kind, inner)) if kind == "Dictionary" => {
                        Some(inner.as_ref())
                    }
                    _ => None,
                };
                let mut scope = env.clone();
                let mut homogeneous = None;
                for entry in args.chunks_exact(2) {
                    let Expr::String(key) = &entry[0] else {
                        return Err("invalid dictionary key".into());
                    };
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
                        &entry[1],
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
                    if crate::compiler::identifier(key) {
                        scope.insert(key.clone(), ty);
                    }
                }
                return Ok(if let Some(ty) = expected {
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
                });
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
                if name == "dictionaryPut" {
                    infer_with(&args[2], None, env, program, knowledge)?;
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
                    || !matches!(*element,
                    Type::Named(ref kind) if kind == "Dictionary" || program.records.iter().any(|record| &record.name == kind))
                        && !matches!(*element, Type::Generic(ref kind, _) if kind == "Dictionary")
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
                let valid = match name.as_str() {
                    "has" => args.len() == 2 && infer_with(&args[1], Some(&text), env, program, knowledge)? == text,
                    "getValue" => args.len() == 2 && {
                        let key = if matches!(&args[1], Expr::List(items) if items.is_empty()) {
                            infer_with(&args[1], Some(&Type::Generic("List".into(), Box::new(text.clone()))), env, program, knowledge)?
                        } else { infer_with(&args[1], None, env, program, knowledge)? };
                        key == text || key == Type::Generic("List".into(), Box::new(text.clone()))
                    },
                    _ => args.len() == 1,
                };
                if !valid { return Err(format!("invalid {name} dictionary arguments")); }
                if name == "getValue" {
                    let path: Vec<_> = match &args[1] {
                        Expr::String(key) => vec![key.as_str()],
                        Expr::List(items) => items.iter().filter_map(|item| if let Expr::String(key) = item { Some(key.as_str()) } else { None }).collect(),
                        _ => Vec::new(),
                    };
                    if let Expr::Call(kind, _) = &args[0] && kind == "__bl_dictionary" && !path.is_empty() {
                        let mut value = &args[0];
                        for key in path {
                            if let Expr::Call(kind, entries) = value && kind == "__bl_dictionary"
                                && let Some(entry) = entries.chunks_exact(2).find(|entry| matches!(&entry[0], Expr::String(name) if name == key)) {
                                value = &entry[1];
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
                if name == "getValue" && let Expr::List(path) = &args[1] {
                    for _ in 1..path.len() {
                        inner = if let Type::Generic(kind, value) = inner {
                            if kind == "Dictionary" { *value } else { named("Value") }
                        } else { named("Value") };
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
            if name == "__bl_index" {
                let [base, key] = args.as_slice() else {
                    return Err("dictionary index requires key".into());
                };
                if infer_with(key, Some(&named("String")), env, program, knowledge)?
                    != named("String")
                {
                    return Err("dictionary key requires String".into());
                }
                let base_type = infer_with(base, None, env, program, knowledge)?;
                return match base_type {
                    Type::Generic(kind, ty) if kind == "Dictionary" => Ok(*ty),
                    Type::Named(name) if matches!(name.as_str(), "Dictionary" | "Value") => {
                        Ok(expected.cloned().unwrap_or_else(|| named("Value")))
                    }
                    Type::Named(name) => {
                        let record = program
                            .records
                            .iter()
                            .find(|record| record.name == name)
                            .ok_or("dictionary index requires a dictionary")?;
                        let Expr::String(key) = key else {
                            return Ok(named("Value"));
                        };
                        record
                            .fields
                            .iter()
                            .find(|(field, _)| field == key)
                            .map(|(_, ty)| ty.clone())
                            .ok_or_else(|| format!("unknown dictionary field: {key}"))
                    }
                    _ => Err("dictionary index requires a dictionary".into()),
                };
            }
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
                let is_datetime = point == named("DateTime");
                if matches!(name.as_str(), "financialYear" | "financialYearQuarter") {
                    if args.len() != 2 {
                        return Err(format!("{name} requires a financial year basis"));
                    }
                    let basis = infer_with(&args[1], None, env, program, knowledge)?;
                    if basis != named("String") && basis != named("Number") {
                        return Err("invalid financial year basis type".into());
                    }
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
                    return Ok(named("String"));
                }
                if args.len() < 2 || args.len() > if name == "daysBetween" { 3 } else { 4 } {
                    return Err(format!("invalid {name} argument count"));
                }
                if infer_with(&args[1], Some(&point), env, program, knowledge)? != point {
                    return Err(format!("{name} requires two matching points"));
                }
                let options = &args[2..];
                if name == "daysBetween" {
                    if let Some(flag) = options.first()
                        && (!is_datetime
                            || infer_with(flag, Some(&named("Bool")), env, program, knowledge)?
                                != named("Bool"))
                    {
                        return Err("includeTime requires DateTime and Bool".into());
                    }
                } else {
                    let mut remaining = options;
                    if let Some(basis) = remaining.first()
                        && infer_with(basis, None, env, program, knowledge)? == named("String")
                    {
                        if let Expr::String(value) = basis
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
                        remaining = &remaining[1..];
                    }
                    if let Some(flag) = remaining.first()
                        && (!is_datetime
                            || remaining.len() != 1
                            || infer_with(flag, Some(&named("Bool")), env, program, knowledge)?
                                != named("Bool"))
                    {
                        return Err("includeTime requires DateTime and Bool".into());
                    }
                }
                return Ok(named("Number"));
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
                let Some(value) = args.first() else {
                    return Err(format!("{name} requires Date or DateTime"));
                };
                let point = infer_with(value, None, env, program, knowledge)?;
                if !matches!(point, Type::Named(ref kind) if matches!(kind.as_str(), "Date" | "DateTime"))
                {
                    return Err(format!("{name} requires Date or DateTime"));
                }
                let number = named("Number");
                let calendar = named("Calendar");
                let boolean = named("Bool");
                let mut index = 1;
                if matches!(name.as_str(), "weekdaysBetween" | "businessDaysBetween") {
                    if args.get(index).is_none_or(|arg| {
                        infer_with(arg, Some(&point), env, program, knowledge).ok()
                            != Some(point.clone())
                    }) {
                        return Err(format!("{name} requires two matching points"));
                    }
                    index += 1;
                }
                let numeric = match name.as_str() {
                    "nthDayOfWeekInMonth" => 2,
                    "firstDayOfWeekInMonth"
                    | "lastDayOfWeekInMonth"
                    | "nextDayOfWeek"
                    | "prevDayOfWeek"
                    | "addBusinessDays"
                    | "subtractBusinessDays" => 1,
                    _ => 0,
                };
                for arg in args.iter().skip(index).take(numeric) {
                    if infer_with(arg, Some(&number), env, program, knowledge)? != number {
                        return Err(format!("{name} requires Number"));
                    }
                }
                if args.len() < index + numeric {
                    return Err(format!("{name} requires {numeric} Number arguments"));
                }
                index += numeric;
                let accepts_calendar = matches!(
                    name.as_str(),
                    "isPublicHoliday"
                        | "isBusinessDay"
                        | "nextBusinessDay"
                        | "prevBusinessDay"
                        | "addBusinessDays"
                        | "subtractBusinessDays"
                        | "businessDaysBetween"
                );
                if name == "isPublicHoliday" && args.len() == index {
                    return Err("isPublicHoliday requires Calendar".into());
                }
                if accepts_calendar && args.len() > index {
                    if infer_with(&args[index], Some(&calendar), env, program, knowledge)?
                        != calendar
                    {
                        return Err(format!("{name} requires Calendar"));
                    }
                    index += 1;
                }
                let strict = matches!(
                    name.as_str(),
                    "nextBusinessDay"
                        | "prevBusinessDay"
                        | "addBusinessDays"
                        | "subtractBusinessDays"
                        | "businessDaysBetween"
                );
                if strict && args.len() > index {
                    if infer_with(&args[index], Some(&boolean), env, program, knowledge)? != boolean
                    {
                        return Err("strictCalendarRange requires Bool".into());
                    }
                    index += 1;
                }
                if args.len() != index {
                    return Err(format!("invalid {name} argument count"));
                }
                return Ok(
                    if matches!(
                        name.as_str(),
                        "isWeekday" | "isWeekend" | "isPublicHoliday" | "isBusinessDay"
                    ) {
                        boolean
                    } else if matches!(name.as_str(), "weekdaysBetween" | "businessDaysBetween") {
                        number
                    } else {
                        point
                    },
                );
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
                    for option in &args[1..] {
                        let Expr::Call(key, values) = option else {
                            return Err("calendarMerge options must be named".into());
                        };
                        if !matches!(key.as_str(), "__bl_named_dedupeBy" | "__bl_named_tiebreak")
                            || values.len() != 1
                        {
                            return Err(format!("unknown calendarMerge option: {key}"));
                        }
                        if infer_with(&values[0], Some(&named("String")), env, program, knowledge)?
                            != named("String")
                        {
                            return Err(format!("{key} requires String"));
                        }
                        if let Expr::String(value) = &values[0] {
                            let valid = if key.ends_with("dedupeBy") {
                                matches!(value.as_str(), "value" | "valueAndName")
                            } else {
                                matches!(value.as_str(), "first" | "name")
                            };
                            if !valid {
                                return Err(format!("invalid {key}: {value}"));
                            }
                        }
                    }
                } else {
                    let [first, target, options @ ..] = args.as_slice() else {
                        return Err(format!("{name} requires Calendar and target"));
                    };
                    if infer_with(first, Some(&calendar), env, program, knowledge)? != calendar {
                        return Err(format!("{name} requires Calendar"));
                    }
                    fn check_target(
                        expr: &Expr,
                        env: &HashMap<std::string::String, Type>,
                        program: &Program,
                        knowledge: &[Knowledge],
                    ) -> Result<(), std::string::String> {
                        match expr {
                            Expr::List(items) => {
                                for item in items {
                                    check_target(item, env, program, knowledge)?;
                                }
                                Ok(())
                            }
                            Expr::Call(name, args) if name == "pattern" => {
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
                                Ok(())
                            }
                            other => {
                                let ty = infer_with(other, None, env, program, knowledge)?;
                                if matches!(ty, Type::Named(ref kind) if matches!(kind.as_str(), "String" | "Date" | "DateTime"))
                                    || matches!(ty, Type::Generic(ref kind, ref inner) if kind == "Range" && matches!(inner.as_ref(), Type::Named(point) if matches!(point.as_str(), "Date" | "DateTime")))
                                {
                                    Ok(())
                                } else {
                                    Err(format!("invalid calendar target: {ty}"))
                                }
                            }
                        }
                    }
                    check_target(target, env, program, knowledge)?;
                    if options.len() > 1 {
                        return Err(format!("{name} accepts only rangeMatch"));
                    }
                    if let Some(option) = options.first() {
                        let Expr::Call(key, values) = option else {
                            return Err("rangeMatch must be named".into());
                        };
                        if key != "__bl_named_rangeMatch" || values.len() != 1 {
                            return Err(format!("unknown {name} option: {key}"));
                        }
                        if infer_with(&values[0], Some(&named("String")), env, program, knowledge)?
                            != named("String")
                        {
                            return Err("rangeMatch requires String".into());
                        }
                        if let Expr::String(value) = &values[0]
                            && !matches!(
                                value.as_str(),
                                "equality" | "entryWithin" | "entryEncloses" | "overlap"
                            )
                        {
                            return Err(format!("invalid rangeMatch: {value}"));
                        }
                    }
                }
                return Ok(calendar);
            }
            if matches!(name.as_str(), "entryName" | "entryValue") {
                let [entry] = args.as_slice() else {
                    return Err(format!("{name} requires one CalendarEntry"));
                };
                if infer_with(entry, None, env, program, knowledge)? != named("CalendarEntry") {
                    return Err(format!("{name} requires CalendarEntry"));
                }
                return Ok(named(if name == "entryName" {
                    "String"
                } else {
                    "CalendarValue"
                }));
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
                let [_, rest @ ..] = args.as_slice() else {
                    unreachable!()
                };
                let point = |arg: &Expr| -> Result<(), std::string::String> {
                    if matches!(infer_with(arg, None, env, program, knowledge)?, Type::Named(kind) if matches!(kind.as_str(), "Date" | "DateTime"))
                    {
                        Ok(())
                    } else {
                        Err(format!("{name} requires a Date or DateTime point"))
                    }
                };
                let result = match (name.as_str(), rest) {
                    ("count", []) => named("Number"),
                    ("isEmpty", []) | ("contains", [_]) | ("overlaps", [_]) => named("Bool"),
                    ("entries", []) | ("find", [_]) | ("entriesFor", [_]) | ("entriesIn", [_]) => {
                        Type::Generic("List".into(), Box::new(named("CalendarEntry")))
                    }
                    ("names", []) => Type::Generic("List".into(), Box::new(named("String"))),
                    ("validFrom", []) | ("validTo", []) => named("CalendarPoint"),
                    ("validRange", []) => named("CalendarRange"),
                    ("next" | "prev", [_] | [_, _]) => named("CalendarEntry"),
                    _ => return Err(format!("invalid {name} arguments")),
                };
                match (name.as_str(), rest) {
                    ("find", [value]) => {
                        if infer_with(value, Some(&named("String")), env, program, knowledge)?
                            != named("String")
                        {
                            return Err("find requires String name".into());
                        }
                    }
                    ("contains" | "entriesFor" | "next" | "prev", [value, ..]) => point(value)?,
                    ("overlaps" | "entriesIn", [range]) => {
                        let ty = infer_with(range, None, env, program, knowledge)?;
                        if !matches!(ty, Type::Generic(ref kind, ref inner) if kind == "Range" && matches!(inner.as_ref(), Type::Named(point) if matches!(point.as_str(), "Date" | "DateTime")))
                            && ty != named("CalendarRange")
                        {
                            return Err(format!("{name} requires a Date/DateTime range"));
                        }
                    }
                    _ => {}
                }
                if let ("next" | "prev", [_, n]) = (name.as_str(), rest)
                    && infer_with(n, Some(&named("Number")), env, program, knowledge)?
                        != named("Number")
                {
                    return Err(format!("{name} requires numeric n"));
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
                return Ok(named("Bool"));
            }
            if name == "dateTime" {
                return Err("dateTime(...) was renamed to datetime(...)".into());
            }
            if matches!(name.as_str(), "date" | "time" | "datetime") {
                let number = named("Number");
                let result = named(match name.as_str() {
                    "date" => "Date",
                    "time" => "Time",
                    _ => "DateTime",
                });
                let actual: Vec<_> = args
                    .iter()
                    .map(|arg| infer_with(arg, None, env, program, knowledge))
                    .collect::<Result<_, _>>()?;
                let valid = match (name.as_str(), actual.as_slice()) {
                    ("date" | "time" | "datetime", [ty]) if *ty == named("String") => {
                        if let [Expr::String(value)] = args.as_slice() {
                            let valid = match name.as_str() {
                                "date" => value.parse::<crate::temporal::Date>().is_ok(),
                                "time" => value.parse::<crate::temporal::Time>().is_ok(),
                                _ => value.parse::<crate::temporal::DateTime>().is_ok(),
                            };
                            if !valid {
                                return Err(format!("invalid {name} literal: {value}"));
                            }
                        }
                        true
                    }
                    ("date" | "time", [ty]) if *ty == named("DateTime") => true,
                    ("date", [y, m, d]) | ("time", [y, m, d])
                        if [y, m, d].iter().all(|ty| **ty == number) =>
                    {
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
                        true
                    }
                    ("time", [a, b, c, offset])
                        if [a, b, c].iter().all(|ty| *ty == &number)
                            && *offset == named("DTDuration") =>
                    {
                        true
                    }
                    ("datetime", [day, time])
                        if *day == named("Date") && *time == named("Time") =>
                    {
                        if let [Expr::Call(_, day), Expr::Call(_, time)] = args.as_slice()
                            && let ([Expr::String(day)], [Expr::String(time)]) =
                                (day.as_slice(), time.as_slice())
                        {
                            let day = day.parse::<crate::temporal::Date>()?;
                            let time = time.parse::<crate::temporal::Time>()?;
                            crate::temporal::combine(day, time)?;
                        }
                        true
                    }
                    _ => false,
                };
                if !valid {
                    return Err(format!("invalid {name} arguments"));
                }
                return Ok(result);
            }
            if matches!(name.as_str(), "today" | "now") {
                if !args.is_empty() {
                    return Err(format!("{name} requires no arguments"));
                }
                return Ok(named(if name == "today" { "Date" } else { "DateTime" }));
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
                return match (name.as_str(), actual.as_slice()) {
                    ("withOffset", [value, offset])
                        if matches!(value, Type::Named(ty) if matches!(ty.as_str(), "Time" | "DateTime"))
                            && *offset == named("DTDuration") =>
                    {
                        Ok(value.clone())
                    }
                    ("withTimezone", [value, zone])
                        if *value == named("DateTime") && *zone == named("String") =>
                    {
                        Ok(value.clone())
                    }
                    ("withoutOffset" | "withoutTimezone" | "withoutOffsetOrTimezone", [value])
                        if matches!(value, Type::Named(ty) if matches!(ty.as_str(), "Date" | "DateTime")) =>
                    {
                        Ok(value.clone())
                    }
                    _ => Err(format!("invalid {name} arguments")),
                };
            }
            let text = named("String");
            let number = named("Number");
            if matches!(name.as_str(), "dtDuration" | "ymDuration") {
                let [arg] = args.as_slice() else {
                    return Err(format!("{name} requires one String argument"));
                };
                if infer_with(arg, Some(&text), env, program, knowledge)? != text {
                    return Err(format!("{name} requires String"));
                }
                if let Expr::String(value) = arg {
                    if name == "dtDuration" {
                        value.parse::<crate::temporal::DTDuration>()?;
                    } else {
                        value.parse::<crate::temporal::YMDuration>()?;
                    }
                }
                return Ok(named(if name == "dtDuration" {
                    "DTDuration"
                } else {
                    "YMDuration"
                }));
            }
            if matches!(name.as_str(), "dtDurationBetween" | "ymDurationBetween") {
                let [from, to] = args.as_slice() else {
                    return Err(format!("{name} requires two points"));
                };
                let first = infer_with(from, None, env, program, knowledge)?;
                let second = infer_with(to, Some(&first), env, program, knowledge)?;
                if first != second
                    || !matches!(first, Type::Named(ref ty) if matches!(ty.as_str(), "Date" | "DateTime"))
                {
                    return Err(format!("{name} requires two Dates or two DateTimes"));
                }
                return Ok(named(if name == "dtDurationBetween" {
                    "DTDuration"
                } else {
                    "YMDuration"
                }));
            }
            if matches!(
                name.as_str(),
                "min" | "max" | "sum" | "mean" | "median" | "product" | "stddev" | "mode"
            ) {
                let [values] = args.as_slice() else {
                    return Err(format!("{name} requires one List<Number> argument"));
                };
                let list = Type::Generic("List".into(), Box::new(number.clone()));
                if infer_with(values, Some(&list), env, program, knowledge)? != list {
                    return Err(format!("{name} requires List<Number>"));
                }
                return Ok(number);
            }
            if name == "number" {
                if !matches!(args.len(), 1 | 3) {
                    return Err("number requires one or three String arguments".into());
                }
                for arg in args {
                    if infer_with(arg, Some(&text), env, program, knowledge)? != text {
                        return Err("number requires String arguments".into());
                    }
                }
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
                    return Ok(if name == "abs" { actual } else { named("Bool") });
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
                    if infer_with(&args[1], Some(&first), env, program, knowledge)? != first {
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
                let optional = matches!(name.as_str(), "floor" | "ceiling");
                if args.len() != 2 && !(optional && args.len() == 1) {
                    return Err(format!(
                        "{name} requires {} Number arguments",
                        if optional { "one or two" } else { "two" }
                    ));
                }
                for arg in args {
                    if infer_with(arg, Some(&number), env, program, knowledge)? != number {
                        return Err(format!("{name} requires Number arguments"));
                    }
                }
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
                let valid_arity = match name.as_str() {
                    "modulo" => args.len() == 2,
                    "log" => matches!(args.len(), 1 | 2),
                    "clamp" => args.len() == 3,
                    _ => args.len() == 1,
                };
                if !valid_arity {
                    return Err(format!("invalid {name} argument count"));
                }
                for arg in args {
                    if infer_with(arg, Some(&number), env, program, knowledge)? != number {
                        return Err(format!("{name} requires Number arguments"));
                    }
                }
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
                return Ok(
                    if matches!(
                        name.as_str(),
                        "odd" | "even" | "isPositive" | "isNegative" | "isZero"
                    ) {
                        named("Bool")
                    } else {
                        number
                    },
                );
            }
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
            if let Call(name, entries) = base.as_ref()
                && name == "__bl_dictionary"
            {
                infer_with(base, None, env, program, knowledge)?;
                let mut local = env.clone();
                for entry in entries.chunks_exact(2) {
                    let String(key) = &entry[0] else {
                        unreachable!()
                    };
                    let ty = infer_with(&entry[1], None, &local, program, knowledge)?;
                    if key == field {
                        return Ok(ty);
                    }
                    if crate::compiler::identifier(key) {
                        local.insert(key.clone(), ty);
                    }
                }
            }
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
            if let Type::Generic(kind, inner) = &ty {
                if kind == "Dictionary" {
                    return Ok(inner.as_ref().clone());
                }
                if kind == "DictionaryEntry" {
                    return match field.as_str() {
                        "key" => Ok(named("String")),
                        "value" => Ok(inner.as_ref().clone()),
                        _ => Err(format!("unknown dictionary entry field: {field}")),
                    };
                }
            }
            if let Type::Named(name) = ty {
                if matches!(name.as_str(), "Dictionary" | "Value") {
                    return Ok(expected.cloned().unwrap_or_else(|| named("Value")));
                }
                if matches!(name.as_str(), "Date" | "Time" | "DateTime") {
                    let calendar = matches!(name.as_str(), "Date" | "DateTime");
                    let clock = matches!(name.as_str(), "Time" | "DateTime");
                    let kind = match field.as_str() {
                        "year" | "month" | "day" | "dayOfYear" | "weekOfYear" | "isoWeekOfYear"
                        | "quarter"
                            if calendar =>
                        {
                            Some("Number")
                        }
                        "hour" | "minute" | "second" if clock => Some("Number"),
                        "offset" => Some("DTDuration"),
                        "timezone" | "dayName" | "dayNameShort" | "isoYearWeek" | "monthName"
                        | "monthNameShort" | "yearQuarter"
                            if calendar || field == "timezone" =>
                        {
                            Some("String")
                        }
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        return Ok(named(kind));
                    }
                }
                if matches!(name.as_str(), "DTDuration" | "YMDuration") {
                    let valid = if name == "DTDuration" {
                        matches!(
                            field.as_str(),
                            "days"
                                | "hours"
                                | "minutes"
                                | "seconds"
                                | "totalSeconds"
                                | "totalMinutes"
                                | "totalHours"
                                | "totalDays"
                        )
                    } else {
                        matches!(
                            field.as_str(),
                            "years" | "months" | "totalMonths" | "totalYears"
                        )
                    };
                    if valid {
                        return Ok(named("Number"));
                    }
                }
                if let Some(record) = program.records.iter().find(|item| item.name == name) {
                    return record
                        .fields
                        .iter()
                        .find(|(key, _)| key == field)
                        .map(|(_, value)| value.clone())
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
            if let (Some(a), Some(b)) = (lower, upper)
                && reversed_constants(a, b, &ty)
            {
                return Err("inverted range bounds".into());
            }
            Ok(Type::Generic("Range".into(), Box::new(ty)))
        }
        Iteration {
            every,
            binding,
            source,
            body,
        } => {
            check_name(binding)?;
            let empty_source = Type::Generic("List".into(), Box::new(named("Value")));
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
            local.insert(binding.clone(), *element);
            let boolean = named("Bool");
            let result_type = if *every {
                Some(&boolean)
            } else if let Some(Type::Generic(kind, inner)) = expected {
                if kind == "List" {
                    Some(inner.as_ref())
                } else {
                    None
                }
            } else {
                None
            };
            let result = infer_with(body, result_type, &local, program, knowledge)?;
            if *every {
                if result != named("Bool") {
                    return Err("every predicate must be Bool".into());
                }
                Ok(named("Bool"))
            } else {
                Ok(Type::Generic("List".into(), Box::new(result)))
            }
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
                if actual != element_type
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
            if ty != named("Bool") {
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
                    if matches!(rhs, Ok(Type::Named(ref name)) if matches!(name.as_str(), "Calendar" | "CalendarRange" | "CalendarValue"))
                    {
                        return Ok(named("Bool"));
                    }
                }
                let kind = if matches!(right.as_ref(), List(..))
                    || lhs == named("String") && !matches!(right.as_ref(), Range(..))
                {
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
            if duration(&lhs) && rhs == named("Number") && matches!(op.as_str(), "*" | "/") {
                return Ok(lhs);
            }
            if lhs == named("Number") && duration(&rhs) && op == "*" {
                return Ok(rhs);
            }
            if lhs == named("Number")
                && duration(&rhs)
                && op == "-"
                && matches!(left.as_ref(), Expr::Number(value) if value == "0")
            {
                return Ok(rhs);
            }
            let point = |ty: &Type| matches!(ty, Type::Named(name) if matches!(name.as_str(), "Date" | "Time" | "DateTime"));
            if point(&lhs) && duration(&rhs) && matches!(op.as_str(), "+" | "-") {
                if lhs == named("Time") && rhs != named("DTDuration") {
                    return Err("Time only supports DTDuration".into());
                }
                return Ok(lhs);
            }
            if lhs != rhs {
                if matches!(op.as_str(), "==" | "!=")
                    && (matches!((&lhs, &rhs), (Type::Named(value), Type::Named(point)) if matches!(value.as_str(), "CalendarValue" | "CalendarPoint") && matches!(point.as_str(), "Date" | "DateTime"))
                        || matches!((&rhs, &lhs), (Type::Named(value), Type::Named(point)) if matches!(value.as_str(), "CalendarValue" | "CalendarPoint") && matches!(point.as_str(), "Date" | "DateTime"))
                        || matches!((&lhs, &rhs), (Type::Named(value), Type::Generic(kind, inner)) if value == "CalendarValue" && kind == "Range" && matches!(inner.as_ref(), Type::Named(point) if matches!(point.as_str(), "Date" | "DateTime")))
                        || matches!((&rhs, &lhs), (Type::Named(value), Type::Generic(kind, inner)) if value == "CalendarValue" && kind == "Range" && matches!(inner.as_ref(), Type::Named(point) if matches!(point.as_str(), "Date" | "DateTime"))))
                {
                    return Ok(named("Bool"));
                }
                return Err(format!("{op} requires matching types, got {lhs} and {rhs}"));
            }
            if point(&lhs) && op == "-" {
                return if lhs == named("Time") {
                    Err("Time point subtraction is not supported".into())
                } else {
                    Ok(named("DTDuration"))
                };
            }
            if duration(&lhs) {
                return match op.as_str() {
                    "+" | "-" => Ok(lhs),
                    "==" | "!=" | "<" | "<=" | ">" | ">=" => Ok(named("Bool")),
                    _ => Err(format!("{op} does not support {lhs}")),
                };
            }
            if lhs == named("Number") && matches!(op.as_str(), "+" | "-" | "*" | "/" | "**") {
                if op == "/"
                    && constant_number(right).is_some_and(|value| value == Ok(Decimal::ZERO))
                {
                    return Err("division by zero".into());
                }
                if let Some(Err(error)) = constant_number(expr) {
                    return Err(format!("invalid constant arithmetic: {error}"));
                }
            }
            match op.as_str() {
                "and" | "or" if lhs == named("Bool") => Ok(named("Bool")),
                "+" if lhs == named("String") => Ok(named("String")),
                "+" | "-" | "*" | "/" | "**" if lhs == named("Number") => Ok(named("Number")),
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

pub(crate) fn builtin(name: &str) -> bool {
    name.starts_with("__bl_named_")
        || matches!(
            name,
            "__bl_dictionary"
                | "__bl_index"
                | "keys"
                | "values"
                | "getEntries"
                | "size"
                | "has"
                | "getValue"
                | "dictionaryPut"
                | "dictionaryMerge"
                | "dictionaryRemove"
        )
        || matches!(
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
