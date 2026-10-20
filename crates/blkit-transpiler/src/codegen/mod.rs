use std::collections::HashMap;

use crate::{
    Program, Type,
    decision::{DecisionKind, DecisionModel, DecisionTable, Knowledge},
    expr::{Expr, Stmt},
    graph::{NodeKind, PeerKind},
    semantic,
};

fn expr_fallible(expr: &Expr, knowledge: &[Knowledge]) -> bool {
    match expr {
        Expr::Call(name, args) => {
            let shadowed = knowledge.iter().find(|item| item.name == *name);
            (matches!(
                name.as_str(),
                "__bl_list_index"
                    | "__bl_list_filter"
                    | "__bl_list_sublist"
                    | "__bl_list_insertBefore_scalar"
                    | "__bl_list_insertBefore_spread"
                    | "__bl_list_insertAfter_scalar"
                    | "__bl_list_insertAfter_spread"
                    | "__bl_list_remove"
                    | "__bl_list_listReplace"
                    | "__bl_list_remove_match"
                    | "__bl_list_listReplace_match"
                    | "seq"
                    | "sort"
                    | "__bl_dictionary_field"
                    | "keys"
                    | "values"
                    | "getEntries"
                    | "size"
                    | "has"
                    | "getValue"
                    | "__bl_dict_isEmpty"
                    | "dictionaryPut"
                    | "dictionaryMerge"
                    | "dictionaryRemove"
            ) || name.starts_with("__bl_sort_")
                || name.starts_with("__bl_calendar_")
                || matches!(
                    name.as_str(),
                    "calendarDrop"
                        | "calendarKeep"
                        | "calendarMerge"
                        | "daysBetween"
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
                )
                || name.starts_with("__bl_temporal_")
                || name.starts_with("__bl_duration_op_")
                || name.starts_with("__bl_dtDurationBetween_")
                || name.starts_with("__bl_ymDurationBetween_")
                || name == "__bl_with_offset_time"
                || matches!(
                    name.as_str(),
                    "__bl_date_parts" | "__bl_time_parts" | "__bl_datetime_parts"
                )
                || (shadowed.is_none()
                    && matches!(
                        name.as_str(),
                        "string"
                            | "zipStringJoin"
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
                            | "dtDuration"
                            | "ymDuration"
                            | "date"
                            | "time"
                            | "datetime"
                            | "withOffset"
                            | "withTimezone"
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
                            | "substring"
                            | "sublist"
                            | "charAt"
                            | "padLeading"
                            | "padTrailing"
                            | "repeat"
                            | "split"
                            | "matches"
                            | "replace"
                            | "extract"
                    )))
                || shadowed.is_some_and(|item| expr_fallible(&item.body, knowledge))
                || args.iter().any(|arg| expr_fallible(arg, knowledge))
        }
        Expr::Field(_, _) => true,
        Expr::Index(_, _) | Expr::Sequence(_, _) => true,
        Expr::InlinePredicate { body, .. } | Expr::NamedArgument(_, body) => {
            expr_fallible(body, knowledge)
        }
        Expr::Some {
            bindings,
            predicate,
        } => {
            bindings
                .iter()
                .any(|(_, source)| expr_fallible(source, knowledge))
                || expr_fallible(predicate, knowledge)
        }
        Expr::Not(value) => expr_fallible(value, knowledge),
        Expr::Binary(left, op, right) => {
            (matches!(op.as_str(), "num+" | "-" | "*" | "/" | "%" | "**" | "mod")
                || op.starts_with("duration")
                || op.starts_with("point")
                || op.starts_with("calendar"))
                || expr_fallible(left, knowledge)
                || expr_fallible(right, knowledge)
        }
        Expr::List(items) => items.iter().any(|item| expr_fallible(item, knowledge)),
        Expr::Dictionary(entries) => entries
            .iter()
            .any(|(_, value)| expr_fallible(value, knowledge)),
        Expr::Iteration { source, body, .. } => {
            expr_fallible(source, knowledge) || expr_fallible(body, knowledge)
        }
        Expr::Range(lower, upper, _, _) => lower
            .iter()
            .chain(upper)
            .any(|value| expr_fallible(value, knowledge)),
        _ => false,
    }
}

fn body_fallible(body: &[Stmt]) -> bool {
    body.iter().any(|stmt| match stmt {
        Stmt::Return(value) => expr_fallible(value, &[]),
        Stmt::If(condition, yes, no) => {
            expr_fallible(condition, &[]) || body_fallible(yes) || body_fallible(no)
        }
    })
}

fn specialize_expr(
    expr: &Expr,
    program: &Program,
    knowledge: &[Knowledge],
    env: &HashMap<String, Type>,
    expected: Option<&Type>,
) -> Expr {
    let mut result = expr.clone();
    fn visit(
        expr: &mut Expr,
        program: &Program,
        knowledge: &[Knowledge],
        env: &HashMap<String, Type>,
        expected: Option<&Type>,
    ) {
        let mixed_dictionary = matches!(semantic::infer_with(expr, None, env, program, knowledge),
            Ok(Type::Named(name)) if name == "Dictionary");
        let resolved_iteration = if matches!(expr, Expr::Iteration { source, .. } if matches!(source.as_ref(), Expr::List(items) if items.is_empty()))
        {
            semantic::empty_iteration_element(expr, expected, env, program, knowledge)
                .expect("validated empty iteration source")
        } else {
            None
        };
        let resolved_some = if let Expr::Some { bindings, .. } = &*expr {
            bindings
                .iter()
                .map(|(_, source)| {
                    if matches!(source, Expr::List(items) if items.is_empty()) {
                        semantic::empty_some_element(expr, source, env, program, knowledge).ok()
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        match expr {
            Expr::Binary(left, op, right) => {
                let lhs = semantic::infer_with(left, None, env, program, knowledge).ok();
                let rhs = semantic::infer_with(right, None, env, program, knowledge).ok();
                if lhs == Some(Type::Named("Value".into()))
                    && let Some(target) = &rhs
                    && *target != Type::Named("Value".into())
                {
                    **left = Expr::Call(
                        format!("__bl_cast_to_{}", rust_type(target)),
                        vec![*left.clone()],
                    );
                } else if rhs == Some(Type::Named("Value".into()))
                    && let Some(target) = &lhs
                    && *target != Type::Named("Value".into())
                {
                    **right = Expr::Call(
                        format!("__bl_cast_to_{}", rust_type(target)),
                        vec![*right.clone()],
                    );
                }
                if op == "+"
                    && (lhs == Some(Type::Named("Number".into()))
                        && rhs == Some(Type::Named("Value".into()))
                        || rhs == Some(Type::Named("Number".into()))
                            && lhs == Some(Type::Named("Value".into())))
                {
                    *op = "num+".into();
                }
                if op == "in"
                    && matches!(lhs, Some(Type::Named(ref name)) if matches!(name.as_str(), "Date" | "DateTime"))
                {
                    if rhs == Some(Type::Named("Calendar".into())) {
                        *op = "calendarin".into();
                    }
                    if rhs == Some(Type::Named("CalendarRange".into())) {
                        *op = "calendarinrange".into();
                    }
                    if rhs == Some(Type::Named("CalendarValue".into())) {
                        *op = "calendarinvalue".into();
                    }
                }
                if matches!(op.as_str(), "==" | "!=") {
                    let internal = |ty: &Option<Type>| matches!(ty, Some(Type::Named(name)) if matches!(name.as_str(), "CalendarPoint" | "CalendarValue"));
                    let point = |ty: &Option<Type>| matches!(ty, Some(Type::Named(name)) if matches!(name.as_str(), "Date" | "DateTime"));
                    if internal(&lhs) && point(&rhs) {
                        *op = format!("calendar_eq_left_{op}");
                    } else if internal(&rhs) && point(&lhs) {
                        *op = format!("calendar_eq_right_{op}");
                    } else if lhs == Some(Type::Named("CalendarValue".into()))
                        && matches!(rhs, Some(Type::Generic(ref kind, _)) if kind == "Range")
                    {
                        *op = format!("calendar_eq_range_left_{op}");
                    } else if rhs == Some(Type::Named("CalendarValue".into()))
                        && matches!(lhs, Some(Type::Generic(ref kind, _)) if kind == "Range")
                    {
                        *op = format!("calendar_eq_range_right_{op}");
                    }
                }
                let duration = |ty: &Option<Type>| matches!(ty, Some(Type::Named(name)) if matches!(name.as_str(), "DTDuration" | "YMDuration"));
                if duration(&lhs) && matches!(op.as_str(), "+" | "-" | "*" | "/") {
                    *op = format!("duration{op}");
                } else if duration(&rhs) && matches!(op.as_str(), "*" | "-") {
                    *op = format!("duration_right{op}");
                } else if let Some(Type::Named(name)) = &lhs {
                    if matches!(name.as_str(), "Date" | "Time" | "DateTime") {
                        if op == "in" {
                            let range =
                                Type::Generic("Range".into(), Box::new(Type::Named(name.clone())));
                            if semantic::infer_with(right, Some(&range), env, program, knowledge)
                                .ok()
                                == Some(range)
                            {
                                *op = format!("pointin_{name}");
                            } else {
                                *op = "pointinlist".into();
                            }
                        } else if matches!(op.as_str(), "==" | "!=" | "<" | "<=" | ">" | ">=") {
                            *op = format!("pointcmp{op}");
                        } else if let Some(Type::Named(other)) = &rhs {
                            if matches!(other.as_str(), "DTDuration" | "YMDuration")
                                && matches!(op.as_str(), "+" | "-")
                            {
                                *op = format!("point_{name}_{other}_{op}");
                            } else if other == name && op == "-" {
                                *op = format!("pointdiff_{name}");
                            }
                        }
                    } else if op == "+" && name == "Number" {
                        *op = "num+".into();
                    }
                }
                visit(left, program, knowledge, env, None);
                visit(right, program, knowledge, env, None);
            }
            Expr::Sequence(start, end) => {
                visit(
                    start,
                    program,
                    knowledge,
                    env,
                    Some(&Type::Named("Number".into())),
                );
                visit(
                    end,
                    program,
                    knowledge,
                    env,
                    Some(&Type::Named("Number".into())),
                );
            }
            Expr::Index(base, key) => {
                let empty_base = matches!(base.as_ref(), Expr::List(items) if items.is_empty());
                let base_expected = if empty_base
                    && matches!(expected, Some(Type::Generic(kind, _)) if kind == "List")
                {
                    expected
                } else {
                    None
                };
                if let Ok(Type::Generic(kind, element)) =
                    semantic::infer_with(base, base_expected, env, program, knowledge)
                    && kind == "List"
                {
                    let numeric = semantic::infer_with(key, None, env, program, knowledge).ok()
                        == Some(Type::Named("Number".into()));
                    let mut local = env.clone();
                    local.insert(".item".into(), *element.clone());
                    visit(base, program, knowledge, env, None);
                    if empty_base {
                        **base = Expr::Call(
                            format!("__bl_list_type_{}", rust_type(&element)),
                            vec![*base.clone()],
                        );
                    }
                    visit(
                        key,
                        program,
                        knowledge,
                        if numeric { env } else { &local },
                        None,
                    );
                    *expr = Expr::Call(
                        if numeric {
                            "__bl_list_index"
                        } else {
                            "__bl_list_filter"
                        }
                        .into(),
                        vec![*base.clone(), *key.clone()],
                    );
                    return;
                }
                if let Ok(Type::Named(shape)) =
                    semantic::infer_with(base, None, env, program, knowledge)
                    && let Expr::String(field) = key.as_ref()
                    && program.records.iter().any(|record| record.name == shape)
                {
                    *expr = Expr::Field(base.clone(), rust_record_field(program, &shape, field));
                    if let Expr::Field(base, _) = expr {
                        visit(base, program, knowledge, env, None);
                    }
                    return;
                }
                if matches!(semantic::infer_with(base, None, env, program, knowledge), Ok(Type::Named(ref shape)) if program.records.iter().any(|record| &record.name == shape))
                {
                    **base = dynamic_wrapper(*base.clone(), program, knowledge, env);
                    visit(base, program, knowledge, env, None);
                    visit(key, program, knowledge, env, None);
                    *expr =
                        Expr::Call("__bl_index_named".into(), vec![*base.clone(), *key.clone()]);
                    return;
                }
                visit(base, program, knowledge, env, None);
                visit(key, program, knowledge, env, None);
                *expr = Expr::Call(
                    "__bl_dictionary_field".into(),
                    vec![*base.clone(), *key.clone()],
                );
            }
            Expr::Call(name, args) => {
                if matches!(name.as_str(), "remove" | "listReplace")
                    && !knowledge.iter().any(|item| item.name == *name)
                    && let Some(Expr::InlinePredicate { binding, .. }) = args.get(1)
                    && let Ok(list @ Type::Generic(_, _)) =
                        semantic::infer_with(&args[0], expected, env, program, knowledge)
                {
                    let Type::Generic(_, element) = &list else {
                        unreachable!()
                    };
                    let mut local = env.clone();
                    local.insert(binding.clone(), *element.clone());
                    let empty = matches!(&args[0], Expr::List(items) if items.is_empty());
                    visit(&mut args[0], program, knowledge, env, Some(&list));
                    if empty {
                        args[0] = Expr::Call(
                            format!("__bl_list_type_{}", rust_type(element)),
                            vec![args[0].clone()],
                        );
                    }
                    if let Expr::InlinePredicate { body, .. } = &mut args[1] {
                        visit(
                            body,
                            program,
                            knowledge,
                            &local,
                            Some(&Type::Named("Bool".into())),
                        );
                    }
                    if let Some(replacement) = args.get_mut(2) {
                        visit(replacement, program, knowledge, env, Some(element));
                    }
                    *name = format!("__bl_list_{name}_match");
                    return;
                }
                if name == "getValue" && args.first().is_some_and(|source| matches!(semantic::infer_with(source, None, env, program, knowledge), Ok(Type::Named(ref ty)) if ty == "Dictionary"))
                    && matches!(semantic::infer_with(&Expr::Call(name.clone(), args.clone()), None, env, program, knowledge), Ok(Type::Named(ref ty)) if ty == "Number")
                { *name = "__bl_get_dynamic_number".into(); }
                if name == "isEmpty" && args.first().is_some_and(|first| matches!(semantic::infer_with(first, None, env, program, knowledge),
                    Ok(Type::Generic(ref kind, _)) if kind == "Dictionary")
                    || matches!(semantic::infer_with(first, None, env, program, knowledge),
                        Ok(Type::Named(ref kind)) if kind == "Dictionary" || program.records.iter().any(|record| &record.name == kind)))
                { *name = "__bl_dict_isEmpty".into(); }
                let aggregate_empty =
                    matches!(args.first(), Some(Expr::List(items)) if items.is_empty());
                let aggregate_hint = (aggregate_empty
                    || matches!(args.first(), Some(Expr::Iteration { .. })))
                .then(|| {
                    Type::Generic(
                        "List".into(),
                        Box::new(if name == "mode" {
                            match expected {
                                Some(Type::Generic(_, inner)) => *inner.clone(),
                                _ => Type::Named("Number".into()),
                            }
                        } else {
                            expected.cloned().unwrap_or(Type::Named("Number".into()))
                        }),
                    )
                });
                if matches!(
                    name.as_str(),
                    "min" | "max" | "sum" | "mean" | "median" | "product" | "stddev" | "mode"
                ) && !knowledge.iter().any(|item| item.name == *name)
                    && args.len() == 1
                    && let Ok(Type::Generic(kind, element)) = semantic::infer_with(
                        &args[0],
                        aggregate_hint.as_ref(),
                        env,
                        program,
                        knowledge,
                    )
                    && kind == "List"
                {
                    let empty = matches!(&args[0], Expr::List(items) if items.is_empty());
                    let list = Type::Generic(kind, element.clone());
                    visit(&mut args[0], program, knowledge, env, Some(&list));
                    if empty {
                        args[0] = Expr::Call(
                            format!("__bl_list_type_{}", rust_type(&element)),
                            vec![args[0].clone()],
                        );
                    }
                    *name = format!(
                        "__bl_aggregate_{}_{}",
                        name,
                        match element.as_ref() {
                            Type::Named(kind)
                                if matches!(kind.as_str(), "Date" | "Time" | "DateTime") =>
                                "point",
                            Type::Named(kind) if kind == "DTDuration" => "dt",
                            Type::Named(kind) if kind == "YMDuration" => "ym",
                            Type::Named(kind) if kind == "Number" => "number",
                            _ => "other",
                        }
                    );
                    return;
                }
                if name == "sort"
                    && !knowledge.iter().any(|item| item.name == *name)
                    && let Ok(Type::Generic(kind, element)) = semantic::infer_with(
                        &Expr::Call(name.clone(), args.clone()),
                        expected,
                        env,
                        program,
                        knowledge,
                    )
                    && kind == "List"
                {
                    let list = Type::Generic(kind, element.clone());
                    let empty = matches!(&args[0], Expr::List(items) if items.is_empty());
                    visit(&mut args[0], program, knowledge, env, Some(&list));
                    if empty {
                        args[0] = Expr::Call(
                            format!("__bl_list_type_{}", rust_type(&element)),
                            vec![args[0].clone()],
                        );
                    }
                    let order_kind = if let Some(order) = args.get_mut(1) {
                        let priority = matches!(semantic::infer_with(order, Some(&list), env, program, knowledge), Ok(ref ty) if *ty == list);
                        let string = Type::Named("String".into());
                        visit(
                            order,
                            program,
                            knowledge,
                            env,
                            Some(if priority { &list } else { &string }),
                        );
                        if priority { "priority" } else { "direction" }
                    } else {
                        "natural"
                    };
                    *name = format!(
                        "__bl_sort_{}_{}",
                        if matches!(element.as_ref(), Type::Named(kind) if matches!(kind.as_str(), "Date" | "Time" | "DateTime"))
                        {
                            "point"
                        } else {
                            "scalar"
                        },
                        order_kind
                    );
                    return;
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
                        | "insertBefore"
                        | "insertAfter"
                        | "remove"
                        | "listReplace"
                ) && !knowledge.iter().any(|item| item.name == *name)
                    && !matches!(args.get(1), Some(Expr::InlinePredicate { .. }))
                {
                    let hint = if matches!(args.first(), Some(Expr::List(items)) if items.is_empty())
                    {
                        if matches!(
                            name.as_str(),
                            "listContains" | "indexOf" | "append" | "prepend"
                        ) {
                            args.get(1)
                                .and_then(|arg| {
                                    semantic::infer_with(arg, None, env, program, knowledge).ok()
                                })
                                .map(|ty| Type::Generic("List".into(), Box::new(ty)))
                                .or_else(|| {
                                    matches!(name.as_str(), "append" | "prepend")
                                        .then(|| expected.cloned())
                                        .flatten()
                                })
                        } else if matches!(name.as_str(), "concatenate" | "union" | "intersection")
                        {
                            args.get(1)
                                .and_then(|arg| {
                                    semantic::infer_with(arg, None, env, program, knowledge).ok()
                                })
                                .or_else(|| expected.cloned())
                        } else {
                            expected.cloned()
                        }
                    } else if matches!(
                        name.as_str(),
                        "sublist"
                            | "reverse"
                            | "insertBefore"
                            | "insertAfter"
                            | "remove"
                            | "listReplace"
                            | "distinct"
                            | "duplicateValues"
                            | "intersection"
                    ) {
                        expected.cloned()
                    } else {
                        None
                    };
                    if let Some(Type::Generic(kind, element)) = args.first().and_then(|arg| {
                        semantic::infer_with(arg, hint.as_ref(), env, program, knowledge).ok()
                    }) && kind == "List"
                    {
                        let list = Type::Generic(kind, element.clone());
                        let empty = matches!(&args[0], Expr::List(items) if items.is_empty());
                        visit(&mut args[0], program, knowledge, env, Some(&list));
                        if empty {
                            args[0] = Expr::Call(
                                format!("__bl_list_type_{}", rust_type(&element)),
                                vec![args[0].clone()],
                            );
                        }
                        let item_type = Type::Named("Number".into());
                        let spread = matches!(name.as_str(), "insertBefore" | "insertAfter")
                            && args.get(2).and_then(|arg| {
                                semantic::infer_with(arg, Some(&list), env, program, knowledge).ok()
                            }) == Some(list.clone());
                        for (index, arg) in args[1..].iter_mut().enumerate() {
                            let hint = if index == 1 && spread
                                || matches!(name.as_str(), "concatenate" | "union" | "intersection")
                            {
                                &list
                            } else if index == 1 && name == "listReplace"
                                || matches!(
                                    name.as_str(),
                                    "listContains" | "indexOf" | "append" | "prepend"
                                )
                                || index == 1
                                    && matches!(name.as_str(), "insertBefore" | "insertAfter")
                            {
                                &element
                            } else {
                                &item_type
                            };
                            visit(arg, program, knowledge, env, Some(hint));
                        }
                        if matches!(name.as_str(), "insertBefore" | "insertAfter") {
                            *name = format!(
                                "__bl_list_{}_{}",
                                name,
                                if spread { "spread" } else { "scalar" }
                            );
                            return;
                        }
                        if name == "flatten" {
                            let mut depth = 0;
                            let mut ty = element.as_ref();
                            while let Type::Generic(kind, inner) = ty {
                                if kind != "List" {
                                    break;
                                }
                                depth += 1;
                                ty = inner;
                            }
                            *name = format!("__bl_list_flatten_{depth}");
                        } else {
                            *name = format!("__bl_list_{name}");
                        }
                        return;
                    }
                }
                if (matches!(name.as_str(), "values" | "getEntries")
                    || name == "getValue"
                        && matches!(semantic::infer_with(&Expr::Call(name.clone(), args.clone()), None, env, program, knowledge), Ok(Type::Named(ref ty)) if ty == "Value"))
                    && !args.is_empty()
                    && matches!(semantic::infer_with(&args[0], None, env, program, knowledge), Ok(Type::Named(ref shape)) if program.records.iter().any(|record| &record.name == shape))
                {
                    args[0] = dynamic_wrapper(args[0].clone(), program, knowledge, env);
                }
                if matches!(name.as_str(), "getValue" | "dictionaryPut")
                    && args.len() >= 2
                    && !matches!(&args[1], Expr::List(_))
                    && matches!(semantic::infer_with(&args[1], None, env, program, knowledge), Ok(Type::Generic(ref kind, ref inner)) if kind == "List" && **inner == Type::Named("String".into()))
                {
                    args[1] = Expr::Call("__bl_path_list".into(), vec![args[1].clone()]);
                }
                if !knowledge.iter().any(|item| item.name == *name) {
                    let kind = args.first().and_then(|arg| {
                        semantic::infer_with(arg, None, env, program, knowledge).ok()
                    });
                    if kind == Some(Type::Named("Calendar".into()))
                        && matches!(
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
                        )
                    {
                        let suffix = if matches!(name.as_str(), "overlaps" | "entriesIn")
                            && matches!(args.get(1).and_then(|arg| semantic::infer_with(arg, None, env, program, knowledge).ok()), Some(Type::Named(ty)) if ty == "CalendarRange")
                        {
                            "_calendar"
                        } else {
                            ""
                        };
                        *name = format!("__bl_calendar_{name}{suffix}");
                    } else if kind == Some(Type::Named("CalendarEntry".into()))
                        && matches!(name.as_str(), "entryName" | "entryValue")
                    {
                        *name = format!("__bl_calendar_{name}");
                    } else if matches!(name.as_str(), "date" | "time")
                        && kind == Some(Type::Named("DateTime".into()))
                    {
                        *name = format!("__bl_extract_{name}");
                    } else if name == "date" && args.len() == 3 {
                        *name = "__bl_date_parts".into();
                    } else if name == "time" && matches!(args.len(), 3 | 4) {
                        *name = "__bl_time_parts".into();
                    } else if name == "datetime" && args.len() == 2 {
                        *name = "__bl_datetime_parts".into();
                    } else if name == "withOffset" && kind == Some(Type::Named("Time".into())) {
                        *name = "__bl_with_offset_time".into();
                    } else if matches!(
                        name.as_str(),
                        "abs"
                            | "isNegative"
                            | "round"
                            | "roundUp"
                            | "roundDown"
                            | "roundHalfUp"
                            | "roundHalfDown"
                            | "roundHalfEven"
                    ) && matches!(kind, Some(Type::Named(ref ty)) if matches!(ty.as_str(), "DTDuration" | "YMDuration"))
                    {
                        *name = format!("__bl_duration_op_{name}");
                    } else if matches!(name.as_str(), "dtDurationBetween" | "ymDurationBetween")
                        && let Some(Type::Named(kind)) = kind
                    {
                        *name = format!("__bl_{name}_{kind}");
                    }
                }
                if name == "string"
                    && !knowledge.iter().any(|item| item.name == *name)
                    && semantic::infer_with(&args[0], None, env, program, knowledge).ok()
                        == Some(Type::Named("Number".into()))
                {
                    *name = "__bl_number_string".into();
                }
                if matches!(name.as_str(), "before" | "after" | "meets" | "metBy")
                    && !knowledge.iter().any(|item| item.name == *name)
                    && args.len() == 2
                {
                    let number = Type::Named("Number".into());
                    let range = Type::Generic("Range".into(), Box::new(number.clone()));
                    let point_first = semantic::infer_with(&args[0], None, env, program, knowledge)
                        .ok()
                        == Some(number.clone())
                        && semantic::infer_with(&args[1], Some(&range), env, program, knowledge)
                            .ok()
                            == Some(range.clone());
                    let range_first = semantic::infer_with(&args[1], None, env, program, knowledge)
                        .ok()
                        == Some(number)
                        && semantic::infer_with(&args[0], Some(&range), env, program, knowledge)
                            .ok()
                            == Some(range);
                    if point_first {
                        *name = format!("__bl_point_{name}");
                    } else if range_first {
                        *name = format!("__bl_range_{name}");
                    }
                }
                if let Some(definition) = knowledge.iter().find(|item| item.name == *name) {
                    for (arg, (_, ty)) in args.iter_mut().zip(&definition.params) {
                        visit(arg, program, knowledge, env, Some(ty));
                    }
                } else {
                    for arg in args {
                        visit(arg, program, knowledge, env, None);
                    }
                }
            }
            Expr::NamedArgument(_, value) => visit(value, program, knowledge, env, None),
            Expr::Some {
                bindings,
                predicate,
            } => {
                let mut local = env.clone();
                for ((binding, source), resolved) in bindings.iter_mut().zip(resolved_some) {
                    let element = resolved
                        .map(|item| Type::Generic("List".into(), Box::new(item)))
                        .or_else(|| {
                            semantic::infer_with(source, None, &local, program, knowledge).ok()
                        });
                    visit(source, program, knowledge, &local, element.as_ref());
                    if let Some(Type::Generic(_, inner)) = element {
                        *source = Expr::Call(
                            format!("__bl_list_type_{}", rust_type(&inner)),
                            vec![source.clone()],
                        );
                        local.insert(binding.clone(), *inner);
                    }
                }
                visit(
                    predicate,
                    program,
                    knowledge,
                    &local,
                    Some(&Type::Named("Bool".into())),
                );
            }
            Expr::Dictionary(entries) => {
                let mixed = (mixed_dictionary
                    || matches!(expected, Some(Type::Named(shape)) if shape == "Dictionary"))
                    && !matches!(expected, Some(Type::Named(shape)) if program.records.iter().any(|record| record.name == *shape));
                let mut scope = env.clone();
                for (key, value) in entries {
                    let source = match &*value {
                        Expr::Call(wrapper, args)
                            if wrapper.ends_with("_as_value")
                                && (args.len() == 1
                                    || wrapper == "__bl_collection_as_value"
                                        && args.len() == 2) =>
                        {
                            &args[0]
                        }
                        other => other,
                    };
                    let ty = semantic::infer_with(source, None, &scope, program, knowledge).ok();
                    if mixed {
                        *value = dynamic_wrapper(value.clone(), program, knowledge, &scope);
                    }
                    visit(value, program, knowledge, &scope, None);
                    if let Some(ty) = ty
                        && crate::compiler::identifier(key)
                    {
                        scope.insert(key.clone(), ty);
                    }
                }
            }

            Expr::List(args) => {
                let element_expected = if let Some(Type::Generic(kind, inner)) = expected {
                    (kind == "List").then_some(inner.as_ref())
                } else {
                    None
                };
                for arg in args {
                    visit(arg, program, knowledge, env, element_expected);
                }
            }
            Expr::Iteration {
                every,
                flatten,
                binding,
                source,
                body,
            } => {
                let boolean = Type::Named("Bool".into());
                let body_expected = if *every {
                    Some(&boolean)
                } else if *flatten {
                    expected
                } else if let Some(Type::Generic(kind, inner)) = expected {
                    (kind == "List").then_some(inner.as_ref())
                } else {
                    None
                };
                let element = if matches!(source.as_ref(), Expr::List(items) if items.is_empty()) {
                    let source_element = resolved_iteration.unwrap_or_else(|| {
                        if !*every {
                            match expected {
                                Some(Type::Generic(kind, inner)) if kind == "List" => {
                                    *inner.clone()
                                }
                                _ => Type::Named("Value".into()),
                            }
                        } else {
                            Type::Named("Value".into())
                        }
                    });
                    Some(Type::Generic("List".into(), Box::new(source_element)))
                } else {
                    semantic::infer_with(source, None, env, program, knowledge).ok()
                };
                visit(source, program, knowledge, env, None);
                let mut local = env.clone();
                if let Some(Type::Generic(_, inner)) = element {
                    **source = Expr::Call(
                        format!("__bl_list_type_{}", rust_type(&inner)),
                        vec![*source.clone()],
                    );
                    local.insert(binding.clone(), *inner);
                }
                visit(body, program, knowledge, &local, body_expected);
            }
            Expr::Field(base, field) => {
                if let Ok(Type::Generic(kind, element)) =
                    semantic::infer_with(base, None, env, program, knowledge)
                    && kind == "List"
                {
                    let field_name = if let Type::Named(shape) = element.as_ref() {
                        if program.records.iter().any(|record| record.name == *shape) {
                            rust_record_field(program, shape, field)
                        } else {
                            field.clone()
                        }
                    } else {
                        field.clone()
                    };
                    let projection = if matches!(element.as_ref(), Type::Generic(name, _) if name == "Dictionary")
                        || *element == Type::Named("Dictionary".into())
                    {
                        "__bl_projection_dictionary"
                    } else if *element == Type::Named("Value".into()) {
                        "__bl_projection_value"
                    } else {
                        "__bl_projection"
                    };
                    *expr = Expr::Call(
                        projection.into(),
                        vec![*base.clone(), Expr::String(field_name)],
                    );
                    if let Expr::Call(_, args) = expr {
                        visit(&mut args[0], program, knowledge, env, None);
                    }
                    return;
                }
                if matches!(semantic::infer_with(base, None, env, program, knowledge),
                    Ok(Type::Named(name)) if matches!(name.as_str(), "Dictionary" | "Value"))
                    || matches!(semantic::infer_with(base, None, env, program, knowledge),
                        Ok(Type::Generic(kind, _)) if kind == "Dictionary")
                {
                    let literal_type = if matches!(base.as_ref(), Expr::Dictionary(_))
                        && matches!(semantic::infer_with(base, None, env, program, knowledge), Ok(Type::Named(name)) if name == "Dictionary")
                    {
                        semantic::infer_with(
                            &Expr::Field(base.clone(), field.clone()),
                            None,
                            env,
                            program,
                            knowledge,
                        )
                        .ok()
                    } else {
                        None
                    };
                    *expr = Expr::Call(
                        "__bl_dictionary_field".into(),
                        vec![*base.clone(), Expr::String(field.clone())],
                    );
                    if let Expr::Call(_, args) = expr {
                        visit(&mut args[0], program, knowledge, env, None);
                    }
                    if let Some(ty) = literal_type
                        && ty != Type::Named("Value".into())
                    {
                        *expr = Expr::Call(
                            format!("__bl_cast_to_{}", rust_type(&ty)),
                            vec![expr.clone()],
                        );
                    }
                    return;
                }
                if let Ok(Type::Named(name)) =
                    semantic::infer_with(base, None, env, program, knowledge)
                {
                    if matches!(name.as_str(), "DTDuration" | "YMDuration") {
                        *expr = Expr::Call(format!("__bl_duration_{field}"), vec![*base.clone()]);
                    } else if matches!(name.as_str(), "Date" | "Time" | "DateTime") {
                        let class = if field == "offset" {
                            "offset"
                        } else if matches!(
                            field.as_str(),
                            "timezone"
                                | "dayName"
                                | "dayNameShort"
                                | "monthName"
                                | "monthNameShort"
                                | "isoYearWeek"
                                | "yearQuarter"
                        ) {
                            "text"
                        } else {
                            "number"
                        };
                        *expr = Expr::Call(
                            format!("__bl_temporal_{class}_{field}"),
                            vec![*base.clone()],
                        );
                    }
                }
                match expr {
                    Expr::Call(_, args) => visit(&mut args[0], program, knowledge, env, None),
                    Expr::Field(base, _) => visit(base, program, knowledge, env, None),
                    _ => unreachable!(),
                }
            }
            Expr::Not(base) => visit(base, program, knowledge, env, None),
            Expr::Range(lower, upper, _, _) => {
                for bound in lower.iter_mut().chain(upper.iter_mut()) {
                    visit(bound, program, knowledge, env, None);
                }
            }
            _ => {}
        }
    }
    visit(&mut result, program, knowledge, env, expected);
    result
}

fn dynamic_wrapper(
    expr: Expr,
    program: &Program,
    knowledge: &[Knowledge],
    env: &HashMap<String, Type>,
) -> Expr {
    if let Expr::Dictionary(entries) = &expr {
        if let Ok(ty) = semantic::infer_with(&expr, None, env, program, knowledge)
            && matches!(&ty, Type::Generic(kind, _) if kind == "Dictionary")
        {
            return Expr::Call(
                "__bl_collection_as_value".into(),
                vec![expr, Expr::String(type_label(&ty))],
            );
        }
        let mut fields = entries.clone();
        let mut scope = env.clone();
        for (key, entry) in &mut fields {
            let value = entry.clone();
            let ty = semantic::infer_with(&value, None, &scope, program, knowledge).ok();
            *entry = dynamic_wrapper(value, program, knowledge, &scope);
            if let Some(ty) = ty
                && crate::compiler::identifier(key)
            {
                scope.insert(key.clone(), ty);
            }
        }
        return Expr::Call("__bl_as_value".into(), vec![Expr::Dictionary(fields)]);
    }
    if let Expr::List(items) = &expr {
        return Expr::Call(
            "__bl_as_value".into(),
            vec![Expr::List(
                items
                    .iter()
                    .cloned()
                    .map(|item| dynamic_wrapper(item, program, knowledge, env))
                    .collect(),
            )],
        );
    }
    let method = match semantic::infer_with(&expr, None, env, program, knowledge) {
        Ok(Type::Named(name)) if name == "Number" => "__bl_number_as_value".to_owned(),
        Ok(Type::Generic(kind, inner)) if kind == "Dictionary" || kind == "List" => {
            let ty = Type::Generic(kind, inner);
            return Expr::Call(
                "__bl_collection_as_value".into(),
                vec![expr, Expr::String(type_label(&ty))],
            );
        }
        Ok(Type::Named(name)) if program.records.iter().any(|record| record.name == name) => {
            format!("__bl_named_as_value_{name}")
        }
        _ => "__bl_as_value".to_owned(),
    };
    Expr::Call(method, vec![expr])
}

fn emit_typed_expr(
    expr: &Expr,
    program: &Program,
    knowledge: &[Knowledge],
    env: &HashMap<String, Type>,
    expected: Option<&Type>,
) -> String {
    if let (Expr::List(items), Some(Type::Generic(kind, inner))) = (expr, expected)
        && kind == "List"
    {
        return format!(
            "vec![{}]",
            items
                .iter()
                .map(|item| emit_typed_expr(item, program, knowledge, env, Some(inner)))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let cast = expected.filter(|target| **target != Type::Named("Value".into())
        && matches!(semantic::infer_with(expr, None, env, program, knowledge), Ok(Type::Named(ref ty)) if ty == "Value"))
        .map(rust_type);
    let mut expr = expr.clone();
    if let Expr::Call(name, args) = &mut expr {
        match name.as_str() {
            "dictionaryPut" if args.len() == 3 => {
                args[0] = dynamic_wrapper(args[0].clone(), program, knowledge, env);
                args[2] = dynamic_wrapper(args[2].clone(), program, knowledge, env);
            }
            "dictionaryRemove" if args.len() == 2 => {
                args[0] = dynamic_wrapper(args[0].clone(), program, knowledge, env);
            }
            "dictionaryMerge" if args.len() == 1 => {
                if let Expr::List(items) = &mut args[0] {
                    for item in items {
                        *item = dynamic_wrapper(item.clone(), program, knowledge, env);
                    }
                }
            }
            _ => {}
        }
    }
    let mut expr = specialize_expr(&expr, program, knowledge, env, expected);
    if let (Expr::Dictionary(entries), Some(Type::Named(shape))) = (&expr, expected)
        && program.records.iter().any(|record| record.name == *shape)
    {
        let (statements, fields) = emit_dictionary_entries(entries, program, knowledge);
        let members = fields
            .into_iter()
            .map(|(key, value)| format!("{}: {value}", rust_record_field(program, shape, &key)))
            .collect::<Vec<_>>()
            .join(", ");
        return format!("{{ {statements} {shape} {{ {members} }} }}");
    }
    if let Some(target) = cast {
        expr = Expr::Call(format!("__bl_cast_to_{target}"), vec![expr]);
    }
    emit_expr_with(&expr, program, knowledge)
}

fn emit_calendar_target(expr: &Expr, program: &Program, knowledge: &[Knowledge]) -> String {
    match expr {
        Expr::List(items) => format!(
            "blkit_core::temporal::CalendarTarget::Any(vec![{}])",
            items
                .iter()
                .map(|item| emit_calendar_target(item, program, knowledge))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Call(name, args) if name == "pattern" => format!(
            "blkit_core::temporal::CalendarTarget::pattern(&({}))?",
            emit_expr_with(&args[0], program, knowledge)
        ),
        Expr::Range(..) => {
            let r = emit_expr_with(expr, program, knowledge);
            format!(
                "{{ let r = {r}; blkit_core::temporal::CalendarTarget::Range(r.lower.map(Into::into), r.upper.map(Into::into), r.include_lower, r.include_upper) }}"
            )
        }
        _ => format!(
            "blkit_core::temporal::CalendarTarget::from({})",
            emit_expr_with(expr, program, knowledge)
        ),
    }
}

fn type_label(ty: &Type) -> String {
    match ty {
        Type::Named(name) => name.clone(),
        Type::Generic(kind, inner) => format!("{kind}<{}>", type_label(inner)),
    }
}

fn emit_dynamic_value(source: &str, ty: &Type, program: &Program) -> String {
    match ty {
        Type::Named(name) if name == "Number" => {
            format!("serde_json::from_str::<serde_json::Value>(&({source}).to_string()).unwrap()")
        }
        Type::Generic(kind, inner) if kind == "Dictionary" => {
            let converted = emit_dynamic_value("value", inner, program);
            format!(
                "serde_json::Value::Object(({source}).into_iter().map(|(key, value)| (key, {converted})).collect())"
            )
        }
        Type::Generic(kind, inner) if kind == "List" => {
            let converted = emit_dynamic_value("value", inner, program);
            format!(
                "serde_json::Value::Array(({source}).into_iter().map(|value| {converted}).collect())"
            )
        }
        Type::Named(name)
            if let Some(record) = program.records.iter().find(|record| record.name == *name) =>
        {
            let mut converted = format!(
                "{{ let object = {source}; let mut value = serde_json::to_value(&object).unwrap(); "
            );
            for (field, ty) in &record.fields {
                let member = format!("object.{}.clone()", rust_record_field(program, name, field));
                converted.push_str(&format!(
                    "value[{field:?}] = {}; ",
                    emit_dynamic_value(&member, ty, program)
                ));
            }
            converted.push_str("value }");
            converted
        }
        _ => format!("serde_json::to_value({source}).unwrap()"),
    }
}

fn uses_name(expr: &Expr, name: &str) -> bool {
    match expr {
        Expr::Name(value) => value == name,
        Expr::Field(base, _) | Expr::Not(base) => uses_name(base, name),
        Expr::InlinePredicate { binding, body } => binding == name || uses_name(body, name),
        Expr::NamedArgument(_, value) => uses_name(value, name),
        Expr::Some {
            bindings,
            predicate,
        } => {
            bindings
                .iter()
                .any(|(binding, source)| binding == name || uses_name(source, name))
                || uses_name(predicate, name)
        }
        Expr::Call(_, args) | Expr::List(args) => args.iter().any(|arg| uses_name(arg, name)),
        Expr::Dictionary(entries) => entries
            .iter()
            .any(|(key, value)| key == name || uses_name(value, name)),
        Expr::Iteration {
            binding,
            source,
            body,
            ..
        } => binding == name || uses_name(source, name) || uses_name(body, name),
        Expr::Binary(left, _, right) | Expr::Index(left, right) | Expr::Sequence(left, right) => {
            uses_name(left, name) || uses_name(right, name)
        }
        Expr::Range(left, right, _, _) => left
            .iter()
            .chain(right.iter())
            .any(|bound| uses_name(bound, name)),
        _ => false,
    }
}

fn emit_dictionary_entries(
    entries: &[(String, Expr)],
    program: &Program,
    knowledge: &[Knowledge],
) -> (String, Vec<(String, String)>) {
    let mut statements = String::new();
    let mut fields = Vec::new();
    let mut used = std::collections::HashSet::new();
    for (index, (key, value)) in entries.iter().enumerate() {
        let binding = if crate::compiler::identifier(key) && semantic::check_name(key).is_ok() {
            key.clone()
        } else {
            let mut candidate = format!("__bl_literal_{index}");
            let mut suffix = 0;
            while used.contains(&candidate)
                || entries
                    .iter()
                    .any(|(key, value)| uses_name(value, &candidate) || key == &candidate)
            {
                suffix += 1;
                candidate = format!("__bl_literal_{index}_{suffix}");
            }
            candidate
        };
        used.insert(binding.clone());
        let (initial, mapped) = if let Expr::Call(wrapper, args) = value
            && (args.len() == 1
                && (matches!(wrapper.as_str(), "__bl_number_as_value" | "__bl_as_value")
                    || wrapper.starts_with("__bl_named_as_value_"))
                || wrapper == "__bl_collection_as_value" && args.len() == 2)
        {
            let mut mapped_args = vec![Expr::Name(binding.clone())];
            mapped_args.extend(args.iter().skip(1).cloned());
            (
                emit_expr_with(&args[0], program, knowledge),
                emit_expr_with(
                    &Expr::Call(wrapper.clone(), mapped_args),
                    program,
                    knowledge,
                ),
            )
        } else {
            (
                emit_expr_with(value, program, knowledge),
                format!("({binding}).clone()"),
            )
        };
        statements.push_str(&format!("let {binding} = {initial}; "));
        fields.push((key.clone(), mapped));
    }
    (statements, fields)
}

fn emit_expr(expr: &Expr, program: &Program) -> String {
    emit_expr_with(expr, program, &[])
}

fn emit_some_iteration(
    bindings: &[(String, Expr)],
    predicate: &Expr,
    program: &Program,
    knowledge: &[Knowledge],
    found: &str,
) -> String {
    if let Some(((binding, source), rest)) = bindings.split_first() {
        format!(
            "for {binding} in {} {{ {} }}",
            emit_expr_with(source, program, knowledge),
            emit_some_iteration(rest, predicate, program, knowledge, found)
        )
    } else {
        format!(
            "if {} {{ {found} = true; break '__bl_some; }}",
            emit_expr_with(predicate, program, knowledge)
        )
    }
}

fn fresh_local(expr: &Expr, base: &str, knowledge: &[Knowledge]) -> String {
    let mut name = base.to_owned();
    let mut suffix = 0;
    while uses_name(expr, &name) || knowledge.iter().any(|item| uses_name(&item.body, &name)) {
        name = format!("{base}_{suffix}");
        suffix += 1;
    }
    name
}

fn bind_filter_item(expr: &mut Expr, binding: &str) {
    match expr {
        Expr::FilterItem => *expr = Expr::Name(binding.into()),
        Expr::InlinePredicate { body, .. } | Expr::NamedArgument(_, body) => {
            bind_filter_item(body, binding)
        }
        Expr::Some {
            bindings,
            predicate,
        } => {
            for (_, source) in bindings {
                bind_filter_item(source, binding);
            }
            bind_filter_item(predicate, binding);
        }
        Expr::Field(base, _) | Expr::Not(base) => bind_filter_item(base, binding),
        Expr::Binary(left, _, right) | Expr::Index(left, right) | Expr::Sequence(left, right) => {
            bind_filter_item(left, binding);
            bind_filter_item(right, binding);
        }
        Expr::Call(name, args) if name == "__bl_list_filter" => {
            bind_filter_item(&mut args[0], binding);
        }
        Expr::Call(_, args) | Expr::List(args) => {
            for arg in args {
                bind_filter_item(arg, binding);
            }
        }
        Expr::Dictionary(entries) => {
            for (_, value) in entries {
                bind_filter_item(value, binding);
            }
        }
        Expr::Iteration { source, body, .. } => {
            bind_filter_item(source, binding);
            bind_filter_item(body, binding);
        }
        Expr::Range(lower, upper, _, _) => {
            for bound in lower.iter_mut().chain(upper.iter_mut()) {
                bind_filter_item(bound, binding);
            }
        }
        _ => {}
    }
}

fn emit_expr_with(expr: &Expr, program: &Program, knowledge: &[Knowledge]) -> String {
    match expr {
        Expr::Number(value) => {
            let parse = if value.contains(['e', 'E']) {
                "from_scientific"
            } else {
                "from_str_exact"
            };
            format!("Number::{parse}({value:?}).unwrap()")
        }
        Expr::String(value) => format!("String::from({value:?})"),
        Expr::Bool(value) => value.to_string(),
        Expr::Name(name) => format!("({name}).clone()"),
        Expr::FilterItem => unreachable!("filter item must be bound before emission"),
        Expr::InlinePredicate { binding, body } => {
            format!("|{binding}| {}", emit_expr_with(body, program, knowledge))
        }
        Expr::Call(name, args) if name == "__bl_path_list" => {
            emit_expr_with(&args[0], program, knowledge)
        }
        Expr::Call(name, args) if name.starts_with("__bl_cast_to_") => {
            let target = name.strip_prefix("__bl_cast_to_").unwrap();
            let source = emit_expr_with(&args[0], program, knowledge);
            if target == "Number" {
                format!("blkit_core::dictionary::cast_number({source})?")
            } else {
                format!("blkit_core::dictionary::cast::<{target}>({source})?")
            }
        }
        Expr::Iteration {
            every,
            flatten,
            binding,
            source,
            body,
        } => {
            let source = emit_expr_with(source, program, knowledge);
            let body = emit_expr_with(body, program, knowledge);
            if *every {
                let all = fresh_local(expr, "__bl_every", knowledge);
                format!(
                    "{{ let mut {all} = true; for {binding} in {source} {{ if !({body}) {{ {all} = false; break; }} }} {all} }}"
                )
            } else {
                let result = fresh_local(expr, "__bl_iteration", knowledge);
                let method = if *flatten { "extend" } else { "push" };
                format!(
                    "{{ let mut {result} = Vec::new(); for {binding} in {source} {{ {result}.{method}({body}); }} {result} }}"
                )
            }
        }
        Expr::Call(name, args) if let Some(element) = name.strip_prefix("__bl_list_type_") => {
            format!(
                "{{ let items: Vec<{element}> = {}; items }}",
                emit_expr_with(&args[0], program, knowledge)
            )
        }
        Expr::Call(name, args)
            if matches!(
                name.as_str(),
                "dictionaryPut" | "dictionaryMerge" | "dictionaryRemove"
            ) =>
        {
            let source = emit_expr_with(&args[0], program, knowledge);
            match name.as_str() {
                "dictionaryMerge" => format!("blkit_core::dictionary::merge({source})?"),
                "dictionaryRemove" => format!(
                    "blkit_core::dictionary::remove({source}, &{})?",
                    emit_expr_with(&args[1], program, knowledge)
                ),
                _ => {
                    let path = emit_expr_with(&args[1], program, knowledge);
                    let path = if matches!(&args[1], Expr::List(_))
                        || matches!(&args[1], Expr::Call(name, _) if name == "__bl_path_list")
                    {
                        path
                    } else {
                        format!("vec![{path}]")
                    };
                    let value = emit_expr_with(&args[2], program, knowledge);
                    format!("blkit_core::dictionary::put({source}, &{path}, {value})?")
                }
            }
        }
        Expr::Call(name, args)
            if matches!(
                name.as_str(),
                "keys" | "values" | "getEntries" | "size" | "__bl_dict_isEmpty"
            ) =>
        {
            let base = emit_expr_with(&args[0], program, knowledge);
            let method = if name == "__bl_dict_isEmpty" {
                "size"
            } else if name == "getEntries" {
                "entries"
            } else {
                name
            };
            let result = format!("blkit_core::dictionary::{method}({base})?");
            match name.as_str() {
                "size" => format!("Number::from({result})"),
                "__bl_dict_isEmpty" => format!("({result} == 0)"),
                "getEntries" => format!(
                    "{result}.into_iter().map(|(key, value)| DictionaryEntry {{ key, value }}).collect::<Vec<_>>()"
                ),
                _ => result,
            }
        }
        Expr::Call(name, args)
            if matches!(
                name.as_str(),
                "has" | "getValue" | "__bl_get_dynamic_number"
            ) =>
        {
            let base = emit_expr_with(&args[0], program, knowledge);
            let key = emit_expr_with(&args[1], program, knowledge);
            if name == "has" {
                format!("blkit_core::dictionary::has({base}, &{key})?")
            } else {
                let method = if name == "__bl_get_dynamic_number" {
                    "get_number"
                } else {
                    "get"
                };
                let path = if matches!(&args[1], Expr::List(_))
                    || matches!(&args[1], Expr::Call(name, _) if name == "__bl_path_list")
                {
                    key
                } else {
                    format!("vec![{key}]")
                };
                format!("blkit_core::dictionary::{method}({base}, &{path})?")
            }
        }
        Expr::Sequence(start, end) => format!(
            "blkit_core::number_ops::sequence({}, {}, Number::ONE)?",
            emit_expr_with(start, program, knowledge),
            emit_expr_with(end, program, knowledge)
        ),
        Expr::Index(base, key) => {
            let base = emit_expr_with(base, program, knowledge);
            let key = emit_expr_with(key, program, knowledge);
            format!(
                "({base}).get(&({key})).cloned().ok_or_else(|| format!(\"missing dictionary key: {{}}\", {key}))?"
            )
        }
        Expr::Dictionary(entries) => {
            let (statements, fields) = emit_dictionary_entries(entries, program, knowledge);
            let pairs = fields
                .into_iter()
                .map(|(key, value)| format!("({key:?}.to_owned(), {value})"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {statements} std::collections::BTreeMap::from([{pairs}]) }}")
        }
        Expr::Call(name, args) if name == "__bl_collection_as_value" => {
            let Expr::String(label) = &args[1] else {
                unreachable!()
            };
            let ty = crate::compiler::type_ref(label).expect("validated collection type");
            emit_dynamic_value(&emit_expr_with(&args[0], program, knowledge), &ty, program)
        }
        Expr::Call(name, args) if name.starts_with("__bl_named_as_value_") => {
            let shape = name.strip_prefix("__bl_named_as_value_").unwrap();
            emit_dynamic_value(
                &emit_expr_with(&args[0], program, knowledge),
                &Type::Named(shape.into()),
                program,
            )
        }
        Expr::Call(name, args)
            if matches!(name.as_str(), "__bl_number_as_value" | "__bl_as_value") =>
        {
            let value = emit_expr_with(&args[0], program, knowledge);
            if name == "__bl_number_as_value" {
                format!(
                    "serde_json::from_str::<serde_json::Value>(&({value}).to_string()).unwrap()"
                )
            } else {
                format!("serde_json::to_value({value}).unwrap()")
            }
        }
        Expr::Call(name, args)
            if matches!(
                name.as_str(),
                "__bl_projection_dictionary" | "__bl_projection_value"
            ) =>
        {
            let Expr::String(field) = &args[1] else {
                unreachable!()
            };
            let source = emit_expr_with(&args[0], program, knowledge);
            format!(
                "({source}).iter().map(|item| item.get({field:?}).cloned().ok_or_else(|| format!(\"missing dictionary key: {{}}\", {field:?}))).collect::<Result<Vec<_>, String>>()?"
            )
        }
        Expr::Call(name, args) if name == "__bl_projection" => {
            let Expr::String(field) = &args[1] else {
                unreachable!()
            };
            format!(
                "({}).iter().map(|item| item.{field}.clone()).collect::<Vec<_>>()",
                emit_expr_with(&args[0], program, knowledge)
            )
        }
        Expr::Some {
            bindings,
            predicate,
        } => {
            let found = fresh_local(expr, "__bl_some", knowledge);
            format!(
                "{{ let mut {found} = false; '__bl_some: {{ {} }} {found} }}",
                emit_some_iteration(bindings, predicate, program, knowledge, &found)
            )
        }
        Expr::NamedArgument(..) => {
            unreachable!("named arguments are emitted by their calendar call")
        }
        Expr::Call(name, args)
            if name == "seq" && !knowledge.iter().any(|item| item.name == *name) =>
        {
            let start = emit_expr_with(&args[0], program, knowledge);
            let end = emit_expr_with(&args[1], program, knowledge);
            let step = args.get(2).map_or("Number::ONE".to_owned(), |arg| {
                emit_expr_with(arg, program, knowledge)
            });
            format!("blkit_core::number_ops::sequence({start}, {end}, {step})?")
        }
        Expr::Call(name, args)
            if matches!(name.as_str(), "all" | "any")
                && !knowledge.iter().any(|item| item.name == *name) =>
        {
            let values = emit_expr_with(&args[0], program, knowledge);
            format!("({values}).into_iter().{name}(|value| value)")
        }
        Expr::Call(name, args) if name.starts_with("__bl_sort_") => {
            let values = fresh_local(expr, "__bl_sort_values", knowledge);
            let source = emit_expr_with(&args[0], program, knowledge);
            let compare = if name.starts_with("__bl_sort_point_") {
                "blkit_core::temporal::compare_checked(left, right, __bl_temporal_clock.clone())"
            } else {
                "Ok(left.cmp(right))"
            };
            let (setup, priorities, descending) = match name.rsplit('_').next().unwrap() {
                "natural" => (String::new(), "None".to_string(), "false".to_string()),
                "direction" => (format!("let order = {};", emit_expr_with(&args[1], program, knowledge)),
                    "None".to_string(),
                    "match order.as_str() { \"asc\" => false, \"desc\" => true, _ => return Err(\"invalid sort order: expected asc or desc\".into()) }".to_string()),
                "priority" => (format!("let priority = {};", emit_expr_with(&args[1], program, knowledge)),
                    "Some(priority.as_slice())".to_string(), "false".to_string()),
                _ => unreachable!(),
            };
            format!(
                "{{ let {values} = {source}; {setup} blkit_core::list_ops::sort_checked({values}, {priorities}, {descending}, |left, right| {compare})? }}"
            )
        }
        Expr::Call(name, args)
            if matches!(
                name.as_str(),
                "__bl_list_remove_match" | "__bl_list_listReplace_match"
            ) =>
        {
            let items = fresh_local(expr, "__bl_match_items", knowledge);
            let result = fresh_local(expr, "__bl_match_result", knowledge);
            let Expr::InlinePredicate { binding, body } = &args[1] else {
                unreachable!()
            };
            let source = emit_expr_with(&args[0], program, knowledge);
            let predicate = emit_expr_with(body, program, knowledge);
            let edit = if name == "__bl_list_remove_match" {
                format!("if !({predicate}) {{ {result}.push({binding}); }}")
            } else {
                let replacement = fresh_local(expr, "__bl_match_replacement", knowledge);
                let value = emit_expr_with(&args[2], program, knowledge);
                return format!(
                    "{{ let {items} = {source}; let {replacement} = {value}; let mut {result} = Vec::new(); for {binding} in {items} {{ if {predicate} {{ {result}.push({replacement}.clone()); }} else {{ {result}.push({binding}); }} }} {result} }}"
                );
            };
            format!(
                "{{ let {items} = {source}; let mut {result} = Vec::new(); for {binding} in {items} {{ {edit} }} {result} }}"
            )
        }
        Expr::Call(name, args)
            if name.starts_with("__bl_list_")
                && matches!(
                    name.as_str(),
                    "__bl_list_count"
                        | "__bl_list_isEmpty"
                        | "__bl_list_listContains"
                        | "__bl_list_indexOf"
                        | "__bl_list_sublist"
                        | "__bl_list_insertBefore_scalar"
                        | "__bl_list_insertBefore_spread"
                        | "__bl_list_insertAfter_scalar"
                        | "__bl_list_insertAfter_spread"
                        | "__bl_list_remove"
                        | "__bl_list_listReplace"
                        | "__bl_list_append"
                        | "__bl_list_prepend"
                        | "__bl_list_concatenate"
                        | "__bl_list_union"
                        | "__bl_list_distinct"
                        | "__bl_list_duplicateValues"
                        | "__bl_list_intersection"
                        | "__bl_list_reverse"
                )
                || name.starts_with("__bl_list_flatten_") =>
        {
            let a = emit_expr_with(&args[0], program, knowledge);
            match name.as_str() {
                "__bl_list_count" => format!("Number::from(({a}).len())"),
                "__bl_list_isEmpty" => format!("({a}).is_empty()"),
                "__bl_list_listContains" => {
                    let b = emit_expr_with(&args[1], program, knowledge);
                    format!("({a}).contains(&({b}))")
                }
                "__bl_list_indexOf" => {
                    let needle = fresh_local(expr, "__bl_index_of_needle", knowledge);
                    let b = emit_expr_with(&args[1], program, knowledge);
                    format!(
                        "{{ let {needle} = {b}; ({a}).iter().enumerate().filter_map(|(i, item)| (item == &{needle}).then(|| Number::from(i + 1))).collect::<Vec<_>>() }}"
                    )
                }
                "__bl_list_append"
                | "__bl_list_prepend"
                | "__bl_list_concatenate"
                | "__bl_list_union" => {
                    let remaining = args[1..]
                        .iter()
                        .map(|arg| emit_expr_with(arg, program, knowledge))
                        .collect::<Vec<_>>();
                    let lists = if matches!(name.as_str(), "__bl_list_append" | "__bl_list_prepend")
                    {
                        remaining
                            .iter()
                            .map(|value| format!("vec![{value}]"))
                            .collect::<Vec<_>>()
                    } else {
                        remaining
                    };
                    let sources = if name == "__bl_list_prepend" {
                        lists
                            .iter()
                            .map(|list| format!("({list}).into_iter()"))
                            .chain(std::iter::once(format!("({a}).into_iter()")))
                            .collect::<Vec<_>>()
                    } else {
                        std::iter::once(format!("({a}).into_iter()"))
                            .chain(lists.iter().map(|list| format!("({list}).into_iter()")))
                            .collect::<Vec<_>>()
                    };
                    let combined = format!(
                        "std::iter::empty(){}",
                        sources
                            .iter()
                            .map(|source| format!(".chain({source})"))
                            .collect::<String>()
                    );
                    if name == "__bl_list_union" {
                        let result = fresh_local(expr, "__bl_union_result", knowledge);
                        let item = fresh_local(expr, "__bl_union_item", knowledge);
                        format!(
                            "{{ let mut {result} = Vec::new(); for {item} in {combined} {{ if !{result}.contains(&{item}) {{ {result}.push({item}); }} }} {result} }}"
                        )
                    } else {
                        format!("({combined}).collect::<Vec<_>>()")
                    }
                }
                "__bl_list_distinct" => {
                    let result = fresh_local(expr, "__bl_distinct_result", knowledge);
                    let item = fresh_local(expr, "__bl_distinct_item", knowledge);
                    format!(
                        "{{ let mut {result} = Vec::new(); for {item} in {a} {{ if !{result}.contains(&{item}) {{ {result}.push({item}); }} }} {result} }}"
                    )
                }
                "__bl_list_duplicateValues" => {
                    let items = fresh_local(expr, "__bl_duplicate_items", knowledge);
                    let result = fresh_local(expr, "__bl_duplicate_result", knowledge);
                    let item = fresh_local(expr, "__bl_duplicate_item", knowledge);
                    format!(
                        "{{ let {items} = {a}; let mut {result} = Vec::new(); for {item} in &{items} {{ if !{result}.contains({item}) && {items}.iter().filter(|value| *value == {item}).take(2).count() == 2 {{ {result}.push({item}.clone()); }} }} {result} }}"
                    )
                }
                "__bl_list_intersection" => {
                    let lists = fresh_local(expr, "__bl_intersection_lists", knowledge);
                    let first = fresh_local(expr, "__bl_intersection_first", knowledge);
                    let result = fresh_local(expr, "__bl_intersection_result", knowledge);
                    let item = fresh_local(expr, "__bl_intersection_item", knowledge);
                    let sources = args
                        .iter()
                        .map(|arg| emit_expr_with(arg, program, knowledge))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "{{ let mut {lists} = vec![{sources}]; let {first} = {lists}.remove(0); let mut {result} = Vec::new(); for {item} in {first} {{ if !{result}.contains(&{item}) && {lists}.iter().all(|other| other.contains(&{item})) {{ {result}.push({item}); }} }} {result} }}"
                    )
                }
                "__bl_list_reverse" => format!("({a}).into_iter().rev().collect::<Vec<_>>()"),
                name if name.starts_with("__bl_list_flatten_") => {
                    let depth: usize = name
                        .trim_start_matches("__bl_list_flatten_")
                        .parse()
                        .expect("specialized flatten depth");
                    let mut flat = format!("({a}).into_iter()");
                    for _ in 0..depth {
                        flat.push_str(".flatten()");
                    }
                    format!("({flat}).collect::<Vec<_>>()")
                }
                "__bl_list_sublist" => {
                    let start = emit_expr_with(&args[1], program, knowledge);
                    let length = args.get(2).map_or("None".into(), |arg| {
                        format!("Some({})", emit_expr_with(arg, program, knowledge))
                    });
                    format!("blkit_core::list_ops::sublist(&({a}), {start}, {length})?")
                }
                "__bl_list_insertBefore_scalar"
                | "__bl_list_insertBefore_spread"
                | "__bl_list_insertAfter_scalar"
                | "__bl_list_insertAfter_spread"
                | "__bl_list_remove"
                | "__bl_list_listReplace" => {
                    let items = fresh_local(expr, "__bl_edit_items", knowledge);
                    let index = fresh_local(expr, "__bl_edit_index", knowledge);
                    let position = emit_expr_with(&args[1], program, knowledge);
                    let at = if name.starts_with("__bl_list_insertAfter") {
                        format!("{index} + 1")
                    } else {
                        index.clone()
                    };
                    let edit = if name == "__bl_list_remove" {
                        format!(
                            "{items}.into_iter().enumerate().filter_map(|(i, value)| (i != {index}).then_some(value)).collect::<Vec<_>>()"
                        )
                    } else {
                        let value = emit_expr_with(&args[2], program, knowledge);
                        if name == "__bl_list_listReplace" {
                            format!(
                                "{{ let replacement = {value}; {items}.into_iter().enumerate().map(|(i, value)| if i == {index} {{ replacement.clone() }} else {{ value }}).collect::<Vec<_>>() }}"
                            )
                        } else {
                            let inserted = if name.ends_with("_scalar") {
                                format!("vec![{value}]")
                            } else {
                                value
                            };
                            format!("{{ {items}.splice({at}..{at}, {inserted}); {items} }}")
                        }
                    };
                    let mutable = if name.starts_with("__bl_list_insert") {
                        "mut "
                    } else {
                        ""
                    };
                    format!(
                        "{{ let {mutable}{items} = {a}; let {index} = blkit_core::list_ops::position({items}.len(), {position})?; {edit} }}"
                    )
                }
                _ => unreachable!(),
            }
        }
        Expr::Call(name, args) if name == "__bl_list_index" => {
            let items = fresh_local(expr, "__bl_index_items", knowledge);
            let position = fresh_local(expr, "__bl_index_position", knowledge);
            let length = fresh_local(expr, "__bl_index_length", knowledge);
            let offset = fresh_local(expr, "__bl_index_offset", knowledge);
            let index = fresh_local(expr, "__bl_index_offset_usize", knowledge);
            let base = emit_expr_with(&args[0], program, knowledge);
            let key = emit_expr_with(&args[1], program, knowledge);
            format!(
                "{{ let {items} = {base}; let {position} = {key}; if {position}.fract() != Number::ZERO || {position} == Number::ZERO {{ return Err(String::from(\"invalid list position\")); }} let {length} = Number::from({items}.len()); let {offset} = if {position} > Number::ZERO {{ {position} - Number::ONE }} else {{ {length} + {position} }}; let {index} = rust_decimal::prelude::ToPrimitive::to_usize(&{offset}).ok_or_else(|| String::from(\"list position out of bounds\"))?; {items}.get({index}).cloned().ok_or_else(|| String::from(\"list position out of bounds\"))? }}"
            )
        }
        Expr::Call(name, args) if name == "__bl_list_filter" => {
            let result = fresh_local(expr, "__bl_filter_result", knowledge);
            let item = fresh_local(expr, "__bl_filter_item", knowledge);
            let base = emit_expr_with(&args[0], program, knowledge);
            let mut key = args[1].clone();
            bind_filter_item(&mut key, &item);
            let predicate = emit_expr_with(&key, program, knowledge);
            format!(
                "{{ let mut {result} = Vec::new(); for {item} in {base} {{ if {predicate} {{ {result}.push({item}); }} }} {result} }}"
            )
        }
        Expr::Call(name, args) if name == "__bl_index_named" => {
            format!(
                "blkit_core::dictionary::get({}, &[{}])?",
                emit_expr_with(&args[0], program, knowledge),
                emit_expr_with(&args[1], program, knowledge)
            )
        }
        Expr::Call(name, args) if name == "__bl_dictionary_field" => {
            let base = emit_expr_with(&args[0], program, knowledge);
            let key = emit_expr_with(&args[1], program, knowledge);
            format!(
                "({base}).get(&({key})).cloned().ok_or_else(|| format!(\"missing dictionary key: {{}}\", {key}))?"
            )
        }
        Expr::Call(name, args)
            if matches!(
                name.as_str(),
                "daysBetween"
                    | "monthsBetween"
                    | "yearsBetween"
                    | "financialYear"
                    | "financialYearQuarter"
            ) && !knowledge.iter().any(|item| item.name == *name) =>
        {
            let values = args
                .iter()
                .map(|arg| emit_expr_with(arg, program, knowledge))
                .collect::<Vec<_>>();
            if matches!(name.as_str(), "financialYear" | "financialYearQuarter") {
                format!(
                    "blkit_core::temporal::financial::financial_year({}, {}, {})?",
                    values[0],
                    values[1],
                    name == "financialYearQuarter"
                )
            } else if values.len() == 2 {
                format!(
                    "blkit_core::temporal::financial::difference({}, {}, {name:?}, \"calendar\", false)?",
                    values[0], values[1]
                )
            } else if values.len() == 3 {
                format!(
                    "blkit_core::temporal::financial::difference_three({}, {}, {name:?}, {})?",
                    values[0], values[1], values[2]
                )
            } else {
                format!(
                    "blkit_core::temporal::financial::difference({}, {}, {name:?}, &({}), {})?",
                    values[0], values[1], values[2], values[3]
                )
            }
        }
        Expr::Call(name, args)
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
            ) && !knowledge.iter().any(|item| item.name == *name) =>
        {
            let values = args
                .iter()
                .map(|arg| emit_expr_with(arg, program, knowledge))
                .collect::<Vec<_>>();
            let is_count = matches!(name.as_str(), "weekdaysBetween" | "businessDaysBetween");
            let is_predicate = matches!(
                name.as_str(),
                "isWeekday" | "isWeekend" | "isPublicHoliday" | "isBusinessDay"
            );
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
            let offset = if is_count { 2 } else { 1 };
            let first = if numeric > 0 {
                format!("Some({})", values[offset])
            } else {
                "None".into()
            };
            let second = if numeric > 1 {
                format!("Some({})", values[offset + 1])
            } else {
                "None".into()
            };
            let calendar_index = offset + numeric;
            let calendar = if name == "isPublicHoliday"
                || matches!(
                    name.as_str(),
                    "isBusinessDay"
                        | "nextBusinessDay"
                        | "prevBusinessDay"
                        | "addBusinessDays"
                        | "subtractBusinessDays"
                        | "businessDaysBetween"
                ) && values.len() > calendar_index
            {
                format!("Some(&({}))", values[calendar_index])
            } else {
                "None".into()
            };
            let strict = if values.len() > calendar_index + 1 {
                values[calendar_index + 1].clone()
            } else {
                "false".into()
            };
            if is_predicate {
                format!(
                    "blkit_core::temporal::business::predicate({}, {name:?}, {calendar})?",
                    values[0]
                )
            } else if is_count {
                format!(
                    "blkit_core::temporal::business::count_between({}, {}, {calendar}, {strict})?",
                    values[0], values[1]
                )
            } else {
                format!(
                    "blkit_core::temporal::business::operation({}, {name:?}, {first}, {second}, {calendar}, {strict})?",
                    values[0]
                )
            }
        }
        Expr::Call(name, args)
            if matches!(
                name.as_str(),
                "calendarDrop" | "calendarKeep" | "calendarMerge"
            ) && !knowledge.iter().any(|item| item.name == *name) =>
        {
            if name == "calendarMerge" {
                let calendars = emit_expr_with(&args[0], program, knowledge);
                let calendar_binding = fresh_local(expr, "__bl_calendars", knowledge);
                let dedupe = fresh_local(expr, "__bl_dedupe", knowledge);
                let tiebreak = fresh_local(expr, "__bl_tiebreak", knowledge);
                let mut options = String::new();
                for option in &args[1..] {
                    let Expr::NamedArgument(key, value) = option else {
                        unreachable!()
                    };
                    let emitted = emit_expr_with(value, program, knowledge);
                    if key == "dedupeBy" {
                        options.push_str(&format!("{dedupe} = Some({emitted}); "));
                    } else {
                        options.push_str(&format!("{tiebreak} = {emitted}; "));
                    }
                }
                format!(
                    "{{ let {calendar_binding} = {calendars}; let mut {dedupe}: Option<String> = None; let mut {tiebreak} = String::from(\"first\"); {options} blkit_core::temporal::Calendar::merge({calendar_binding}, {dedupe}.as_deref(), &{tiebreak})? }}"
                )
            } else {
                let calendar = emit_expr_with(&args[0], program, knowledge);
                let target = emit_calendar_target(&args[1], program, knowledge);
                let mode = args
                    .get(2)
                    .map_or("String::from(\"equality\")".to_string(), |option| {
                        let Expr::NamedArgument(_, value) = option else {
                            unreachable!()
                        };
                        emit_expr_with(value, program, knowledge)
                    });
                format!(
                    "({calendar}).filter(&({target}), {}, &({mode}))?",
                    name == "calendarKeep"
                )
            }
        }
        Expr::Call(name, args) if name.starts_with("__bl_calendar_") => {
            let name = name.strip_prefix("__bl_calendar_").unwrap();
            let a = emit_expr_with(&args[0], program, knowledge);
            let b = args
                .get(1)
                .map(|arg| emit_expr_with(arg, program, knowledge));
            match name {
                "count" => format!("({a}).count()"),
                "isEmpty" => format!("({a}).is_empty()"),
                "entries" => format!("({a}).entries()"),
                "names" => format!("({a}).names()"),
                "find" => format!("({a}).find(&({}))", b.unwrap()),
                "contains" => format!("({a}).contains({})?", b.unwrap()),
                "entriesFor" => format!("({a}).entries_for({})?", b.unwrap()),
                "validFrom" => format!("({a}).valid_from().clone()"),
                "validTo" => format!("({a}).valid_to().clone()"),
                "validRange" => format!("({a}).valid_range()"),
                "entryName" => format!("blkit_core::temporal::entry_name(&({a}))?"),
                "entryValue" => format!("({a}).value.clone()"),
                "next" | "prev" => format!(
                    "({a}).adjacent({}, {}, {})?",
                    b.unwrap(),
                    args.get(2)
                        .map_or("Number::ONE".to_string(), |arg| emit_expr_with(
                            arg, program, knowledge
                        )),
                    name == "next"
                ),
                "overlaps" | "entriesIn" | "overlaps_calendar" | "entriesIn_calendar" => {
                    let method = if name.starts_with("overlaps") {
                        "overlaps"
                    } else {
                        "entries_in"
                    };
                    let range = b.unwrap();
                    let fields = if name.ends_with("_calendar") {
                        "Some(r.start), Some(r.end), r.include_start, r.include_end"
                    } else {
                        "r.lower.map(Into::into), r.upper.map(Into::into), r.include_lower, r.include_upper"
                    };
                    format!("{{ let r = {range}; ({a}).{method}({fields})? }}")
                }
                _ => unreachable!(),
            }
        }
        Expr::Call(name, args) if name.starts_with("__bl_temporal_") => {
            let field = name.rsplit('_').next().unwrap();
            let value = emit_expr_with(&args[0], program, knowledge);
            if field == "offset" {
                format!(
                    "blkit_core::temporal::offset_property(&({value}), __bl_temporal_clock.clone())?"
                )
            } else if name.starts_with("__bl_temporal_text_") {
                format!("blkit_core::temporal::text_property(&({value}), {field:?})?")
            } else {
                format!("blkit_core::temporal::number_property(&({value}), {field:?})?")
            }
        }
        Expr::Call(name, args) if name.starts_with("__bl_duration_op_") => {
            let operation = name.strip_prefix("__bl_duration_op_").unwrap();
            let value = emit_expr_with(&args[0], program, knowledge);
            match operation {
                "abs" => format!("({value}).abs()"),
                "isNegative" => format!("({value}).is_negative()"),
                _ => format!(
                    "({value}).checked_round({}, {operation:?})?",
                    emit_expr_with(&args[1], program, knowledge)
                ),
            }
        }
        Expr::Call(name, args)
            if name.starts_with("__bl_dtDurationBetween_")
                || name.starts_with("__bl_ymDurationBetween_") =>
        {
            let method = match name.as_str() {
                "__bl_dtDurationBetween_Date" => "dt_between_dates",
                "__bl_dtDurationBetween_DateTime" => "dt_between_datetimes",
                "__bl_ymDurationBetween_Date" => "ym_between_dates",
                "__bl_ymDurationBetween_DateTime" => "ym_between_datetimes",
                _ => unreachable!(),
            };
            format!(
                "blkit_core::temporal::{method}({}, {})?",
                emit_expr_with(&args[0], program, knowledge),
                emit_expr_with(&args[1], program, knowledge)
            )
        }
        Expr::Call(name, args) if name.starts_with("__bl_duration_") => {
            let field = name.strip_prefix("__bl_duration_").unwrap();
            let method = match field {
                "totalSeconds" => "total_seconds",
                "totalMinutes" => "total_minutes",
                "totalHours" => "total_hours",
                "totalDays" => "total_days",
                "totalMonths" => "total_months",
                "totalYears" => "total_years",
                _ => field,
            };
            format!(
                "({}).{method}()",
                emit_expr_with(&args[0], program, knowledge)
            )
        }
        Expr::Call(name, args) if name == "__bl_number_string" => format!(
            "({}).normalize().to_string()",
            emit_expr_with(&args[0], program, knowledge)
        ),
        Expr::Call(name, args)
            if knowledge
                .iter()
                .any(|model| model.name == *name && model.braced) =>
        {
            let model = knowledge.iter().find(|model| model.name == *name).unwrap();
            let arguments = args
                .iter()
                .zip(&model.params)
                .enumerate()
                .map(|(index, (arg, (_, ty)))| {
                    format!(
                        "let __bl_knowledge_arg_{index}: {} = {};",
                        rust_type(ty),
                        emit_expr_with(arg, program, knowledge)
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            let params = model
                .params
                .iter()
                .enumerate()
                .map(|(index, (param, _))| format!("let {param} = __bl_knowledge_arg_{index};"))
                .collect::<Vec<_>>()
                .join(" ");
            let env = model.params.iter().cloned().collect();
            format!(
                "{{ {arguments} {params} {} }}",
                emit_typed_expr(&model.body, program, knowledge, &env, Some(&model.output))
            )
        }
        Expr::Call(name, args)
            if name.starts_with("__bl_point_") || name.starts_with("__bl_range_") =>
        {
            let point_first = name.starts_with("__bl_point_");
            let relation = name
                .strip_prefix(if point_first {
                    "__bl_point_"
                } else {
                    "__bl_range_"
                })
                .unwrap();
            let field = match (point_first, relation) {
                (true, "before" | "meets") | (false, "after" | "metBy") => "lower",
                _ => "upper",
            };
            let operator = match relation {
                "before" => "<",
                "after" => ">",
                _ => "==",
            };
            let a = emit_expr_with(&args[0], program, knowledge);
            let b = emit_expr_with(&args[1], program, knowledge);
            let range = if point_first {
                "__bl_right"
            } else {
                "__bl_left"
            };
            let comparison = if point_first {
                format!("__bl_left {operator} *bound")
            } else {
                format!("*bound {operator} __bl_right")
            };
            format!(
                "{{ let __bl_left = {a}; let __bl_right = {b}; {range}.{field}.as_ref().is_some_and(|bound| {comparison}) }}"
            )
        }
        Expr::Call(name, args) if name == "__bl_with_offset_time" => format!(
            "blkit_core::temporal::with_offset_time({}, {}, __bl_temporal_clock.clone())?",
            emit_expr_with(&args[0], program, knowledge),
            emit_expr_with(&args[1], program, knowledge)
        ),
        Expr::Call(name, args) if name == "withOffset" => format!(
            "blkit_core::temporal::with_offset_datetime({}, {})?",
            emit_expr_with(&args[0], program, knowledge),
            emit_expr_with(&args[1], program, knowledge)
        ),
        Expr::Call(name, args) if name == "withTimezone" => format!(
            "blkit_core::temporal::with_timezone({}, &({}))?",
            emit_expr_with(&args[0], program, knowledge),
            emit_expr_with(&args[1], program, knowledge)
        ),
        Expr::Call(name, args)
            if matches!(
                name.as_str(),
                "withoutOffset" | "withoutTimezone" | "withoutOffsetOrTimezone"
            ) =>
        {
            format!(
                "blkit_core::temporal::strip_zone({}, {name:?})",
                emit_expr_with(&args[0], program, knowledge)
            )
        }
        Expr::Call(name, args) if name == "__bl_date_parts" => {
            let a = args
                .iter()
                .map(|arg| emit_expr_with(arg, program, knowledge))
                .collect::<Vec<_>>();
            format!(
                "blkit_core::temporal::date_from_parts({}, {}, {})?",
                a[0], a[1], a[2]
            )
        }
        Expr::Call(name, args) if name == "__bl_time_parts" => {
            let a = args
                .iter()
                .map(|arg| emit_expr_with(arg, program, knowledge))
                .collect::<Vec<_>>();
            let offset = a
                .get(3)
                .map_or("None".to_string(), |value| format!("Some({value})"));
            format!(
                "blkit_core::temporal::time_from_parts({}, {}, {}, {offset})?",
                a[0], a[1], a[2]
            )
        }
        Expr::Call(name, args) if name == "__bl_datetime_parts" => format!(
            "blkit_core::temporal::combine({}, {})?",
            emit_expr_with(&args[0], program, knowledge),
            emit_expr_with(&args[1], program, knowledge)
        ),
        Expr::Call(name, args) if name == "__bl_extract_date" => format!(
            "blkit_core::temporal::extract_date({})",
            emit_expr_with(&args[0], program, knowledge)
        ),
        Expr::Call(name, args) if name == "__bl_extract_time" => format!(
            "blkit_core::temporal::extract_time({})",
            emit_expr_with(&args[0], program, knowledge)
        ),
        Expr::Call(name, _) if name == "today" => {
            "blkit_core::temporal::today(__bl_temporal_clock.clone())".into()
        }
        Expr::Call(name, _) if name == "now" => {
            "blkit_core::temporal::now(__bl_temporal_clock.clone())".into()
        }
        Expr::Call(name, args)
            if matches!(name.as_str(), "dtDuration" | "ymDuration")
                && !knowledge.iter().any(|item| item.name == *name) =>
        {
            let value = emit_expr_with(&args[0], program, knowledge);
            let ty = if name == "dtDuration" {
                "DTDuration"
            } else {
                "YMDuration"
            };
            format!("({value}).parse::<{ty}>()?")
        }
        Expr::Call(name, args)
            if matches!(name.as_str(), "date" | "time" | "datetime")
                && !knowledge.iter().any(|item| item.name == *name) =>
        {
            let ty = match name.as_str() {
                "date" => "Date",
                "time" => "Time",
                _ => "DateTime",
            };
            format!(
                "({}).parse::<{ty}>()?",
                emit_expr_with(&args[0], program, knowledge)
            )
        }
        Expr::Call(name, args)
            if matches!(
                name.as_str(),
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
            ) && !knowledge.iter().any(|item| item.name == *name) =>
        {
            let a = emit_expr_with(&args[0], program, knowledge);
            let b = emit_expr_with(&args[1], program, knowledge);
            match name.as_str() {
                "before" | "meets" | "overlaps" | "overlapsBefore" => {
                    format!("({a}).{name}(&({b}))")
                }
                "after" => format!("({b}).before(&({a}))"),
                "metBy" => format!("({b}).meets(&({a}))"),
                "overlapsAfter" => format!("({b}).overlapsBefore(&({a}))"),
                "coincides" => format!("({a}) == ({b})"),
                "includes" | "startedBy" | "finishedBy" => {
                    let method = match name.as_str() {
                        "includes" => "contains",
                        "startedBy" => "starts",
                        _ => "finishes",
                    };
                    format!("({a}).{method}(&({b}))")
                }
                _ => format!(
                    "({b}).{}(&({a}))",
                    if name == "during" { "contains" } else { name }
                ),
            }
        }
        Expr::Call(name, args)
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
            ) && !knowledge.iter().any(|item| item.name == *name) =>
        {
            let value = emit_expr_with(&args[0], program, knowledge);
            let scale = args.get(1).map_or("Number::ZERO".into(), |arg| {
                emit_expr_with(arg, program, knowledge)
            });
            format!("blkit_core::number_ops::round({value}, {scale}, {name:?})?")
        }
        Expr::Call(name, args) if name.starts_with("__bl_aggregate_") => {
            let operation = name.strip_prefix("__bl_aggregate_").unwrap();
            let (operation, kind) = operation.rsplit_once('_').unwrap();
            let values = emit_expr_with(&args[0], program, knowledge);
            match (operation, kind) {
                ("mode", "number") => format!(
                    "{{ let mut modes = blkit_core::list_ops::modes(&({values}))?; modes.sort(); modes }}"
                ),
                ("mode", "point") => format!(
                    "blkit_core::list_ops::modes_checked(&({values}), |left, right| blkit_core::temporal::compare_checked(left, right, __bl_temporal_clock.clone()).map(|order| order.is_eq()))?"
                ),
                ("mode", _) => format!("blkit_core::list_ops::modes(&({values}))?"),
                ("min" | "max", _) => {
                    let compare = if kind == "point" {
                        "blkit_core::temporal::compare_checked(left, right, __bl_temporal_clock.clone())"
                    } else {
                        "Ok(left.cmp(right))"
                    };
                    format!(
                        "blkit_core::list_ops::extreme(&({values}), {}, |left, right| {compare})?",
                        operation == "max"
                    )
                }
                ("sum" | "mean" | "median", "dt" | "ym") => format!(
                    "blkit_core::temporal::{}({operation:?}, &({values}))?",
                    if kind == "dt" {
                        "aggregate_dt"
                    } else {
                        "aggregate_ym"
                    }
                ),
                _ => format!("blkit_core::number_ops::aggregate({operation:?}, &({values}))?"),
            }
        }
        Expr::Call(name, args)
            if name == "number" && !knowledge.iter().any(|item| item.name == *name) =>
        {
            let text = emit_expr_with(&args[0], program, knowledge);
            let separators = if args.len() == 3 {
                let group = emit_expr_with(&args[1], program, knowledge);
                let decimal = emit_expr_with(&args[2], program, knowledge);
                format!("Some((({group}).as_str(), ({decimal}).as_str()))")
            } else {
                "None".into()
            };
            format!("blkit_core::number_ops::number(&({text}), {separators})?")
        }
        Expr::Call(name, args)
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
            ) && !knowledge.iter().any(|item| item.name == *name) =>
        {
            let values = args
                .iter()
                .map(|arg| emit_expr_with(arg, program, knowledge))
                .collect::<Vec<_>>();
            if matches!(
                name.as_str(),
                "odd" | "even" | "isPositive" | "isNegative" | "isZero"
            ) {
                format!(
                    "blkit_core::number_ops::predicate({name:?}, {})?",
                    values[0]
                )
            } else {
                format!(
                    "blkit_core::number_ops::math({name:?}, &[{}])?",
                    values.join(", ")
                )
            }
        }
        Expr::Call(name, args)
            if semantic::builtin(name) && !knowledge.iter().any(|item| item.name == *name) =>
        {
            let values: Vec<_> = args
                .iter()
                .map(|arg| emit_expr_with(arg, program, knowledge))
                .collect();
            let a = &values[0];
            let b = values.get(1).map(String::as_str).unwrap_or("");
            let c = values.get(2).map(String::as_str).unwrap_or("");
            let flags = |index: usize| {
                values
                    .get(index)
                    .map_or("None".to_owned(), |flag| format!("Some(({flag}).as_str())"))
            };
            match name.as_str() {
                "string" => format!("blkit_core::string_ops::scalar(&({a}))?"),
                "stringJoin" => format!(
                    "blkit_core::string_ops::join_wrapped(&({a}), &({}), &({}), &({}))",
                    values.get(1).map_or("String::new()", String::as_str),
                    values.get(2).map_or("String::new()", String::as_str),
                    values.get(3).map_or("String::new()", String::as_str),
                ),
                "zipStringJoin" => format!(
                    "blkit_core::string_ops::zip_join(&({a}), &({}), &({}), &({}))?",
                    values.get(1).map_or("String::new()", String::as_str),
                    values.get(2).map_or("String::new()", String::as_str),
                    values.get(3).map_or("String::new()", String::as_str),
                ),
                "stringLength" => format!("blkit_core::string_ops::string_length(&({a}))"),
                "indexOf" => format!("blkit_core::string_ops::index_of(&({a}), &({b}))"),
                "substring" => format!(
                    "blkit_core::string_ops::substring(&({a}), {b}, {})?",
                    values
                        .get(2)
                        .map_or("None".to_owned(), |length| format!("Some({length})"))
                ),
                "charAt" => format!("blkit_core::string_ops::char_at(&({a}), {b})?"),
                "reverse" => format!("blkit_core::string_ops::reverse(&({a}))"),
                "padLeading" | "padTrailing" => format!(
                    "blkit_core::string_ops::{}(&({a}), {b}, {})?",
                    if name == "padLeading" {
                        "pad_leading"
                    } else {
                        "pad_trailing"
                    },
                    flags(2)
                ),
                "repeat" => format!("blkit_core::string_ops::repeat(&({a}), {b})?"),
                "substringBefore" | "substringAfter" => format!(
                    "blkit_core::string_ops::{}(&({a}), &({b})).to_owned()",
                    if name == "substringBefore" {
                        "before"
                    } else {
                        "after"
                    }
                ),
                "upperCase" => format!("({a}).to_uppercase()"),
                "lowerCase" => format!("({a}).to_lowercase()"),
                "trim" | "trimLeading" | "trimTrailing" => format!(
                    "({a}).{}().to_owned()",
                    match name.as_str() {
                        "trim" => "trim",
                        "trimLeading" => "trim_start",
                        _ => "trim_end",
                    }
                ),
                "contains" | "startsWith" | "endsWith" => format!(
                    "({a}).{}(({b}).as_str())",
                    match name.as_str() {
                        "contains" => "contains",
                        "startsWith" => "starts_with",
                        _ => "ends_with",
                    }
                ),
                "isBlank" => format!("blkit_core::string_ops::is_blank(&({a}))"),
                "isEmpty" => format!("({a}).is_empty()"),
                "split" => format!("blkit_core::string_ops::split(&({a}), &({b}))?"),
                "matches" => format!(
                    "blkit_core::string_ops::matches(&({a}), &({b}), {})?",
                    flags(2)
                ),
                "replace" => format!(
                    "blkit_core::string_ops::replace(&({a}), &({b}), &({c}), {})?",
                    flags(3)
                ),
                "extract" => format!(
                    "blkit_core::string_ops::extract(&({a}), &({b}), {})?",
                    flags(2)
                ),
                _ => unreachable!(),
            }
        }
        Expr::Call(name, args) => {
            let arguments = args
                .iter()
                .map(|arg| emit_expr_with(arg, program, knowledge))
                .collect::<Vec<_>>()
                .join(", ");
            let suffix = if knowledge
                .iter()
                .any(|item| item.name == *name && expr_fallible(&item.body, knowledge))
            {
                "?"
            } else {
                ""
            };
            format!("{name}({arguments}){suffix}")
        }
        Expr::Field(base, field) => {
            if let Expr::Name(name) = base.as_ref()
                && program.enums.iter().any(|item| item.name == *name)
            {
                return format!("{name}::{field}");
            }
            format!(
                "({}.{}).clone()",
                emit_expr_with(base, program, knowledge),
                field
            )
        }
        Expr::Range(lower, upper, include_lower, include_upper) => format!(
            "BlRange {{ lower: {}, upper: {}, include_lower: {include_lower}, include_upper: {include_upper} }}",
            lower.as_ref().map_or("None".into(), |value| format!(
                "Some({})",
                emit_expr_with(value, program, knowledge)
            )),
            upper.as_ref().map_or("None".into(), |value| format!(
                "Some({})",
                emit_expr_with(value, program, knowledge)
            ))
        ),
        Expr::List(elements) => format!(
            "vec![{}]",
            elements
                .iter()
                .map(|item| emit_expr_with(item, program, knowledge))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Not(value) => format!("(!{})", emit_expr_with(value, program, knowledge)),
        Expr::Binary(left, op, right) => {
            if matches!(
                op.as_str(),
                "calendarin" | "calendarinrange" | "calendarinvalue"
            ) {
                let a = emit_expr_with(left, program, knowledge);
                let b = emit_expr_with(right, program, knowledge);
                return if op == "calendarin" {
                    format!("({b}).contains({a})?")
                } else {
                    format!("({b}).contains(&({a}).into())?")
                };
            }
            if let Some(rest) = op.strip_prefix("calendar_eq_range_") {
                let (internal, range) = if rest.starts_with("left_") {
                    (left, right)
                } else {
                    (right, left)
                };
                let a = emit_expr_with(internal, program, knowledge);
                let b = emit_expr_with(range, program, knowledge);
                let result = format!(
                    "{{ let r = {b}; ({a}).equals_range(r.lower.map(Into::into), r.upper.map(Into::into), r.include_lower, r.include_upper)? }}"
                );
                return if rest.ends_with("!=") {
                    format!("!({result})")
                } else {
                    result
                };
            }
            if let Some(rest) = op.strip_prefix("calendar_eq_") {
                let (side, operator) = rest.split_once('_').unwrap();
                let (internal, point) = if side == "left" {
                    (left, right)
                } else {
                    (right, left)
                };
                let internal = emit_expr_with(internal, program, knowledge);
                let point = emit_expr_with(point, program, knowledge);
                let result = format!("({internal}).equals_point({point})?");
                return if operator == "!=" {
                    format!("!({result})")
                } else {
                    result
                };
            }
            if op == "in" {
                return format!(
                    "({}).contains(&({}))",
                    emit_expr_with(right, program, knowledge),
                    emit_expr_with(left, program, knowledge)
                );
            }
            if op.starts_with("point") {
                let a = emit_expr_with(left, program, knowledge);
                let b = emit_expr_with(right, program, knowledge);
                if op == "pointinlist" {
                    return format!(
                        "{{ let value = {a}; ({b}).iter().try_fold(false, |found, item| if found {{ Ok(true) }} else {{ blkit_core::temporal::compare_checked(&value, item, __bl_temporal_clock.clone()).map(|order| order.is_eq()) }})? }}"
                    );
                }
                if let Some(kind) = op.strip_prefix("pointin_") {
                    let adjacent = if kind == "Date" {
                        "lower.zone == upper.zone && lower.date.succ_opt() == Some(upper.date)"
                    } else {
                        "false"
                    };
                    return format!(
                        "{{ let value = {a}; let range = {b}; let empty = if let (Some(lower), Some(upper)) = (range.lower.as_ref(), range.upper.as_ref()) {{ let order = blkit_core::temporal::compare_checked(lower, upper, __bl_temporal_clock.clone())?; order.is_gt() || (order.is_eq() && !(range.include_lower && range.include_upper)) || (!range.include_lower && !range.include_upper && ({adjacent})) }} else {{ false }}; !empty && match range.lower.as_ref() {{ Some(bound) => {{ let order = blkit_core::temporal::compare_checked(&value, bound, __bl_temporal_clock.clone())?; if range.include_lower {{ order.is_ge() }} else {{ order.is_gt() }} }}, None => true }} && match range.upper.as_ref() {{ Some(bound) => {{ let order = blkit_core::temporal::compare_checked(&value, bound, __bl_temporal_clock.clone())?; if range.include_upper {{ order.is_le() }} else {{ order.is_lt() }} }}, None => true }} }}"
                    );
                }
                if let Some(operator) = op.strip_prefix("pointcmp") {
                    let condition = match operator {
                        "==" => "is_eq()",
                        "!=" => "is_ne()",
                        "<" => "is_lt()",
                        "<=" => "is_le()",
                        ">" => "is_gt()",
                        ">=" => "is_ge()",
                        _ => unreachable!(),
                    };
                    return format!(
                        "blkit_core::temporal::compare_checked(&({a}), &({b}), __bl_temporal_clock.clone())?.{condition}"
                    );
                }
                if let Some(kind) = op.strip_prefix("pointdiff_") {
                    let method = if kind == "Date" {
                        "subtract_dates"
                    } else {
                        "subtract_datetimes"
                    };
                    return format!("blkit_core::temporal::{method}({a}, {b})?");
                }
                let (_, ty, dur, operator) = {
                    let mut parts = op.split('_');
                    (
                        parts.next(),
                        parts.next().unwrap(),
                        parts.next().unwrap(),
                        parts.next().unwrap(),
                    )
                };
                let method = match (ty, dur) {
                    ("Date", "DTDuration") => "add_date_dt",
                    ("Date", "YMDuration") => "add_date_ym",
                    ("Time", "DTDuration") => "add_time_dt",
                    ("DateTime", "DTDuration") => "add_datetime_dt",
                    ("DateTime", "YMDuration") => "add_datetime_ym",
                    _ => unreachable!(),
                };
                return if operator == "-" {
                    format!(
                        "blkit_core::temporal::{method}({a}, ({b}).checked_mul(-Number::ONE)?)?"
                    )
                } else {
                    format!("blkit_core::temporal::{method}({a}, {b})?")
                };
            }
            if op.starts_with("duration") {
                let a = emit_expr_with(left, program, knowledge);
                let b = emit_expr_with(right, program, knowledge);
                return match op.as_str() {
                    "duration+" => format!("({a}).checked_add({b})?"),
                    "duration-" => format!("({a}).checked_sub({b})?"),
                    "duration*" => format!("({a}).checked_mul({b})?"),
                    "duration/" => format!("({a}).checked_div({b})?"),
                    "duration_right*" => format!("({b}).checked_mul({a})?"),
                    "duration_right-" => format!("({b}).checked_mul(-Number::ONE)?"),
                    _ => unreachable!(),
                };
            }
            if op == "+" {
                return format!(
                    "format!(\"{{}}{{}}\", {}, {})",
                    emit_expr_with(left, program, knowledge),
                    emit_expr_with(right, program, knowledge)
                );
            }
            if op == "mod" {
                return format!(
                    "blkit_core::number_ops::math(\"modulo\", &[{}, {}])?",
                    emit_expr_with(left, program, knowledge),
                    emit_expr_with(right, program, knowledge)
                );
            }
            if matches!(op.as_str(), "num+" | "-" | "*" | "/" | "%" | "**") {
                let operator = if op == "num+" { "+" } else { op };
                return format!(
                    "blkit_core::number_ops::arithmetic({operator:?}, {}, {})?",
                    emit_expr_with(left, program, knowledge),
                    emit_expr_with(right, program, knowledge)
                );
            }
            let operator = match op.as_str() {
                "and" => "&&",
                "or" => "||",
                other => other,
            };
            format!(
                "({} {operator} {})",
                emit_expr_with(left, program, knowledge),
                emit_expr_with(right, program, knowledge)
            )
        }
    }
}

fn emit_stmt(stmt: &Stmt, program: &Program, out: &mut String, fallible: bool) {
    match stmt {
        Stmt::Return(value) => out.push_str(&format!(
            "return {}{}{};\n",
            if fallible { "Ok(" } else { "" },
            emit_expr(value, program),
            if fallible { ")" } else { "" }
        )),
        Stmt::If(condition, yes, no) => {
            out.push_str(&format!("if {} {{\n", emit_expr(condition, program)));
            for branch in yes {
                emit_stmt(branch, program, out, fallible);
            }
            out.push_str("}\n");
            if !no.is_empty() {
                out.push_str("else {\n");
                for branch in no {
                    emit_stmt(branch, program, out, fallible);
                }
                out.push_str("}\n");
            }
        }
    }
}

fn rust_record_field(program: &Program, shape: &str, field: &str) -> String {
    if crate::compiler::identifier(field) && semantic::check_name(field).is_ok() {
        return field.into();
    }
    let record = program
        .records
        .iter()
        .find(|record| record.name == shape)
        .expect("validated dictionary shape");
    let index = record
        .fields
        .iter()
        .position(|(name, _)| name == field)
        .expect("validated dictionary field");
    let mut candidate = format!("__bl_field_{index}");
    let mut suffix = 0;
    while record.fields.iter().any(|(name, _)| name == &candidate) {
        suffix += 1;
        candidate = format!("__bl_field_{index}_{suffix}");
    }
    candidate
}

fn rust_type(ty: &Type) -> String {
    match ty {
        Type::Named(name) => match name.as_str() {
            "Bool" => "bool".into(),
            "String" => "String".into(),
            "Dictionary" => "std::collections::BTreeMap<String, serde_json::Value>".into(),
            "Value" => "serde_json::Value".into(),
            _ => name.clone(),
        },
        Type::Generic(kind, inner) => match kind.as_str() {
            "Dictionary" => format!("std::collections::BTreeMap<String, {}>", rust_type(inner)),
            "DictionaryEntry" => format!("DictionaryEntry<{}>", rust_type(inner)),
            _ => format!("Vec<{}>", rust_type(inner)),
        },
    }
}

fn task_call(task: &str, argument: &str, program: &Program) -> String {
    let suffix = if program
        .tasks
        .iter()
        .any(|item| item.name == task && body_fallible(&item.body))
    {
        "?"
    } else {
        ""
    };
    format!("self::{task}({argument}){suffix}")
}

fn graph_closure(
    expr: String,
    env: &HashMap<String, Type>,
    input: &str,
    input_type: &Type,
) -> String {
    let mut names = vec![input.to_owned()];
    let mut decoded = vec![format!(
        "serde_json::from_value::<{}>(source.clone()).map_err(|e| e.to_string())?",
        rust_type(input_type)
    )];
    let mut keys: Vec<_> = env
        .iter()
        .filter(|(name, _)| name.as_str() != input)
        .collect();
    keys.sort_by_key(|(name, _)| name.as_str());
    for (name, ty) in keys {
        names.push(name.clone());
        decoded.push(format!("serde_json::from_value::<{}>(values.get({name:?}).ok_or(\"missing graph value: {name}\")?.clone()).map_err(|e| e.to_string())?", rust_type(ty)));
    }
    format!(
        "std::sync::Arc::new(|source, values| {{ let ({},) = ({},); serde_json::to_value({expr}).map_err(|e| e.to_string()) }})",
        names.join(", "),
        decoded.join(", ")
    )
}

fn external_graph_closure(
    task: &str,
    external: &crate::compiler::ExternalTask,
    input: String,
) -> Result<String, String> {
    let (provider, _) = task.split_once('.').ok_or("invalid qualified task")?;
    let output = rust_type(&external.output);
    Ok(format!(
        "std::sync::Arc::new(|source: serde_json::Value, values: blkit_core::runtime::Values| {{ let argument: blkit_core::runtime::Evaluate = {input}; Box::pin(async move {{ let value = argument(&source, &values)?; let result = {provider}::{}(value).await?; let typed: {output} = serde_json::from_value(result).map_err(|e| e.to_string())?; serde_json::to_value(typed).map_err(|e| e.to_string()) }}) }})",
        external.function
    ))
}

mod decision;
mod graph;

pub fn generate(program: &Program) -> Result<String, String> {
    let mut out = format!(
        "pub type Number = rust_decimal::Decimal;\n#[allow(unused_imports)] pub use blkit_core::temporal::{{Date, Time, DateTime, DTDuration, YMDuration, Calendar, CalendarEntry}};\npub const NAMESPACE: &str = {:?};\npub const VERSION: &str = {:?};\n",
        program.namespace, program.version,
    );
    for model in &program.decisions {
        decision::emit_decision(model, program, &mut out);
    }
    let has_named = program
        .processes
        .iter()
        .any(|item| item.named_graph.is_some() || item.source_graph.is_some());
    let serde = ", serde::Serialize, serde::Deserialize";
    out.push_str("#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]\n#[serde(deny_unknown_fields)]\npub struct DictionaryEntry<T> { pub key: String, pub value: T }\n");
    for record in &program.records {
        out.push_str(&format!(
            "#[derive(Debug, Clone, PartialEq{serde})]\n#[serde(deny_unknown_fields)]\npub struct {} {{\n",
            record.name
        ));
        for (field, ty) in &record.fields {
            let rust_field = rust_record_field(program, &record.name, field);
            out.push_str(&format!(
                "  #[serde(rename = {field:?})] pub {rust_field}: {},\n",
                rust_type(ty)
            ));
        }
        out.push_str("}\n");
    }
    for item in &program.enums {
        out.push_str(&format!("#[allow(non_camel_case_types)]\n#[derive(Debug, Clone, PartialEq, Eq{serde})]\npub enum {} {{\n", item.name));
        for variant in &item.variants {
            out.push_str(&format!("  {variant},\n"));
        }
        out.push_str("}\n");
    }
    for process in program.tasks.iter().chain(
        program
            .processes
            .iter()
            .filter(|item| item.named_graph.is_none() && item.source_graph.is_none()),
    ) {
        if has_named {
            out.push_str("#[allow(unused_variables)]\n");
        }
        let fallible = body_fallible(&process.body);
        out.push_str(&format!(
            "pub fn {}({}: {}) -> {} {{\n",
            process.name,
            process.input,
            rust_type(&process.input_type),
            if fallible {
                format!("Result<{}, String>", rust_type(&process.output))
            } else {
                rust_type(&process.output)
            },
        ));
        for stmt in &process.body {
            emit_stmt(stmt, program, &mut out, fallible);
        }
        out.push_str("}\n");
    }
    if has_named {
        graph::emit_named_graphs(program, &mut out)?;
    }
    if out.contains("BlRange { lower:") {
        for name in program
            .records
            .iter()
            .map(|item| item.name.as_str())
            .chain(program.enums.iter().map(|item| item.name.as_str()))
            .chain(
                program
                    .tasks
                    .iter()
                    .chain(&program.processes)
                    .map(|item| item.name.as_str()),
            )
            .chain(program.decisions.iter().map(|item| item.name.as_str()))
        {
            if matches!(name, "BlRange" | "BlRangeValue" | "lower_cmp" | "upper_cmp") {
                return Err(format!("reserved generated name: {name}"));
            }
        }
        out.push_str(r#"
trait BlRangeValue: PartialOrd + Clone {
    fn adjacent(&self, _next: &Self) -> bool { false }
}
impl BlRangeValue for Number {}
impl BlRangeValue for DateTime {}
#[derive(Debug, Clone)]
struct BlRange<T> { lower: Option<T>, upper: Option<T>, include_lower: bool, include_upper: bool }
impl<T: BlRangeValue> BlRange<T> {
    fn empty(&self) -> bool {
        self.lower.as_ref().zip(self.upper.as_ref()).is_some_and(|(a, b)|
            a > b || (a == b && !(self.include_lower && self.include_upper))
                || (!self.include_lower && !self.include_upper && a.adjacent(b)))
    }
    fn contains(&self, value: &T) -> bool {
        !self.empty() && self.lower.as_ref().is_none_or(|a| if self.include_lower { value >= a } else { value > a })
            && self.upper.as_ref().is_none_or(|b| if self.include_upper { value <= b } else { value < b })
    }
    fn starts(&self, value: &T) -> bool {
        !self.empty() && self.include_lower && self.lower.as_ref() == Some(value)
    }
    fn finishes(&self, value: &T) -> bool {
        !self.empty() && self.include_upper && self.upper.as_ref() == Some(value)
    }
    fn before(&self, other: &Self) -> bool {
        !self.empty() && !other.empty() && self.upper.as_ref().zip(other.lower.as_ref()).is_some_and(|(a, b)| a < b)
    }
    fn meets(&self, other: &Self) -> bool {
        !self.empty() && !other.empty() && self.upper.as_ref().zip(other.lower.as_ref()).is_some_and(|(a, b)| a == b)
    }
    fn overlapsBefore(&self, other: &Self) -> bool {
        !self.empty() && !other.empty()
            && lower_cmp(self.lower.as_ref(), other.lower.as_ref()).is_lt()
            && self.upper.as_ref().is_some_and(|end| other.lower.as_ref().is_none_or(|start| start < end))
            && upper_cmp(self.upper.as_ref(), other.upper.as_ref()).is_lt()
    }
    fn overlaps(&self, other: &Self) -> bool {
        if self.empty() || other.empty() { return false; }
        let (lower, include_lower) = match (self.lower.as_ref(), other.lower.as_ref()) {
            (None, None) => (None, false),
            (Some(a), None) => (Some(a.clone()), self.include_lower),
            (None, Some(b)) => (Some(b.clone()), other.include_lower),
            (Some(a), Some(b)) if a > b => (Some(a.clone()), self.include_lower),
            (Some(a), Some(b)) if b > a => (Some(b.clone()), other.include_lower),
            (Some(a), Some(_)) => (Some(a.clone()), self.include_lower && other.include_lower),
        };
        let (upper, include_upper) = match (self.upper.as_ref(), other.upper.as_ref()) {
            (None, None) => (None, false),
            (Some(a), None) => (Some(a.clone()), self.include_upper),
            (None, Some(b)) => (Some(b.clone()), other.include_upper),
            (Some(a), Some(b)) if a < b => (Some(a.clone()), self.include_upper),
            (Some(a), Some(b)) if b < a => (Some(b.clone()), other.include_upper),
            (Some(a), Some(_)) => (Some(a.clone()), self.include_upper && other.include_upper),
        };
        !Self { lower, upper, include_lower, include_upper }.empty()
    }
}
fn lower_cmp<T: PartialOrd>(a: Option<&T>, b: Option<&T>) -> std::cmp::Ordering {
    match (a, b) { (None, None) => std::cmp::Ordering::Equal, (None, _) => std::cmp::Ordering::Less, (_, None) => std::cmp::Ordering::Greater, (Some(a), Some(b)) => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal) }
}
fn upper_cmp<T: PartialOrd>(a: Option<&T>, b: Option<&T>) -> std::cmp::Ordering {
    match (a, b) { (None, None) => std::cmp::Ordering::Equal, (None, _) => std::cmp::Ordering::Greater, (_, None) => std::cmp::Ordering::Less, (Some(a), Some(b)) => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal) }
}
impl<T: BlRangeValue> PartialEq for BlRange<T> {
    fn eq(&self, other: &Self) -> bool {
        self.lower == other.lower && self.upper == other.upper
            && (self.lower.is_none() || self.include_lower == other.include_lower)
            && (self.upper.is_none() || self.include_upper == other.include_upper)
    }
}
"#);
    }
    if out.contains("struct BlRange<T>") {
        out.push_str("impl BlRangeValue for Date { fn adjacent(&self, next: &Self) -> bool { self.zone() == next.zone() && self.date().succ_opt() == Some(next.date()) } }\nimpl BlRangeValue for Time {}\n");
    }
    Ok(out)
}
