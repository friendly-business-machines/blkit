use std::collections::HashMap;

use super::{Expr, Knowledge};
use crate::{Enum, Record, Type};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Term {
    Named(String),
    Generic(String, Box<Term>),
    Var(usize),
}

impl From<&Type> for Term {
    fn from(ty: &Type) -> Self {
        match ty {
            Type::Named(name) => Self::Named(name.clone()),
            Type::Generic(name, inner) => {
                Self::Generic(name.clone(), Box::new(Self::from(inner.as_ref())))
            }
        }
    }
}

fn named(name: &str) -> Term {
    Term::Named(name.into())
}

fn generic(name: &str, inner: Term) -> Term {
    Term::Generic(name.into(), Box::new(inner))
}

// Fixed call signatures are shared by forward validation and symbolic constraints.
pub(super) fn fixed_call_signatures(name: &str) -> Option<Vec<(Vec<Type>, Type)>> {
    let named_type = |name: &str| Type::Named(name.into());
    let text = named_type("String");
    let texts = Type::Generic("List".into(), Box::new(text.clone()));
    let string_signatures = match name {
        "string" => ["String", "Number", "Bool", "Date", "Time", "DateTime"]
            .into_iter()
            .map(|ty| (vec![named_type(ty)], text.clone()))
            .collect(),
        "split" => vec![
            (vec![text.clone(), text.clone()], texts.clone()),
            (vec![text.clone(), texts.clone()], texts.clone()),
        ],
        "stringJoin" => vec![
            (vec![texts.clone()], text.clone()),
            (vec![texts.clone(), text.clone()], text.clone()),
            (
                vec![texts, text.clone(), text.clone(), text.clone()],
                text.clone(),
            ),
        ],
        "zipStringJoin" => {
            let columns = Type::Generic("List".into(), Box::new(texts.clone()));
            let output = texts.clone();
            let mut signatures = vec![(vec![columns.clone()], output.clone())];
            for delimiter in [&text, &texts] {
                signatures.push((vec![columns.clone(), delimiter.clone()], output.clone()));
                for prefix in [&text, &texts] {
                    for suffix in [&text, &texts] {
                        signatures.push((
                            vec![
                                columns.clone(),
                                delimiter.clone(),
                                prefix.clone(),
                                suffix.clone(),
                            ],
                            output.clone(),
                        ));
                    }
                }
            }
            signatures
        }
        "indexOf" => vec![(vec![text.clone(), text.clone()], named_type("Number"))],
        "contains" => vec![(vec![text.clone(), text.clone()], named_type("Bool"))],
        "reverse" => vec![(vec![text.clone()], text.clone())],
        "isEmpty" => vec![(vec![text], named_type("Bool"))],
        _ => vec![],
    };
    if !string_signatures.is_empty() {
        return Some(string_signatures);
    }
    if matches!(name, "all" | "any") {
        return Some(vec![(
            vec![Type::Generic("List".into(), Box::new(named_type("Bool")))],
            named_type("Bool"),
        )]);
    }
    if name == "seq" {
        let number = named_type("Number");
        let output = Type::Generic("List".into(), Box::new(number.clone()));
        return Some(vec![
            (vec![number.clone(), number.clone()], output.clone()),
            (vec![number.clone(), number.clone(), number], output),
        ]);
    }
    if matches!(
        name,
        "min" | "max" | "sum" | "mean" | "median" | "product" | "stddev"
    ) {
        let kinds: &[&str] = match name {
            "min" | "max" => &[
                "Number",
                "String",
                "Date",
                "Time",
                "DateTime",
                "DTDuration",
                "YMDuration",
            ],
            "sum" | "mean" | "median" => &["Number", "DTDuration", "YMDuration"],
            _ => &["Number"],
        };
        let mut signatures = kinds
            .iter()
            .map(|kind| {
                let element = named_type(kind);
                let result = if name == "mean" && *kind == "Number" {
                    Type::Generic("Nullable".into(), Box::new(element.clone()))
                } else {
                    element.clone()
                };
                (
                    vec![Type::Generic("List".into(), Box::new(element))],
                    result,
                )
            })
            .collect::<Vec<_>>();
        if matches!(name, "sum" | "mean") {
            let number = named_type("Number");
            let nullable = Type::Generic("Nullable".into(), Box::new(number.clone()));
            signatures.push((
                vec![Type::Generic("List".into(), Box::new(nullable.clone()))],
                if name == "mean" { nullable } else { number },
            ));
        }
        return Some(signatures);
    }
    let mut signatures: Vec<(Vec<&str>, &str)> = match name {
        "date" => vec![
            (vec!["String"], "Date"),
            (vec!["DateTime"], "Date"),
            (vec!["Number", "Number", "Number"], "Date"),
        ],
        "time" => vec![
            (vec!["String"], "Time"),
            (vec!["DateTime"], "Time"),
            (vec!["Number", "Number", "Number"], "Time"),
            (vec!["Number", "Number", "Number", "DTDuration"], "Time"),
        ],
        "datetime" => vec![
            (vec!["String"], "DateTime"),
            (vec!["Date", "Time"], "DateTime"),
        ],
        "today" => vec![(vec![], "Date")],
        "now" => vec![(vec![], "DateTime")],
        "dtDuration" => vec![(vec!["String"], "DTDuration")],
        "ymDuration" => vec![(vec!["String"], "YMDuration")],
        "withOffset" => ["Time", "DateTime"]
            .into_iter()
            .map(|point| (vec![point, "DTDuration"], point))
            .collect(),
        "withTimezone" => vec![(vec!["DateTime", "String"], "DateTime")],
        "withoutOffset" | "withoutTimezone" | "withoutOffsetOrTimezone" => ["Date", "DateTime"]
            .into_iter()
            .map(|point| (vec![point], point))
            .collect(),
        "dtDurationBetween" | "ymDurationBetween" => ["Date", "DateTime"]
            .into_iter()
            .map(|point| {
                (
                    vec![point, point],
                    if name == "dtDurationBetween" {
                        "DTDuration"
                    } else {
                        "YMDuration"
                    },
                )
            })
            .collect(),
        _ => vec![],
    };
    for point in ["Date", "DateTime"] {
        match name {
            "financialYear" | "financialYearQuarter" => {
                for basis in ["String", "Number"] {
                    signatures.push((vec![point, basis], "String"));
                }
            }
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
            | "businessDaysBetween" => {
                let output = if matches!(
                    name,
                    "isWeekday" | "isWeekend" | "isPublicHoliday" | "isBusinessDay"
                ) {
                    "Bool"
                } else if matches!(name, "weekdaysBetween" | "businessDaysBetween") {
                    "Number"
                } else {
                    point
                };
                let mut params = vec![point];
                if matches!(name, "weekdaysBetween" | "businessDaysBetween") {
                    params.push(point);
                }
                params.extend(std::iter::repeat_n(
                    "Number",
                    match name {
                        "nthDayOfWeekInMonth" => 2,
                        "firstDayOfWeekInMonth"
                        | "lastDayOfWeekInMonth"
                        | "nextDayOfWeek"
                        | "prevDayOfWeek"
                        | "addBusinessDays"
                        | "subtractBusinessDays" => 1,
                        _ => 0,
                    },
                ));
                if name != "isPublicHoliday" {
                    signatures.push((params.clone(), output));
                }
                if matches!(
                    name,
                    "isPublicHoliday"
                        | "isBusinessDay"
                        | "nextBusinessDay"
                        | "prevBusinessDay"
                        | "addBusinessDays"
                        | "subtractBusinessDays"
                        | "businessDaysBetween"
                ) {
                    params.push("Calendar");
                    signatures.push((params.clone(), output));
                }
                if matches!(
                    name,
                    "nextBusinessDay"
                        | "prevBusinessDay"
                        | "addBusinessDays"
                        | "subtractBusinessDays"
                        | "businessDaysBetween"
                ) {
                    params.push("Bool");
                    signatures.push((params, output));
                }
            }
            "daysBetween" | "monthsBetween" | "yearsBetween" => {
                signatures.push((vec![point, point], "Number"));
                if name != "daysBetween" {
                    signatures.push((vec![point, point, "String"], "Number"));
                }
                if point == "DateTime" {
                    signatures.push((vec![point, point, "Bool"], "Number"));
                    if name != "daysBetween" {
                        signatures.push((vec![point, point, "String", "Bool"], "Number"));
                    }
                }
            }
            _ => {}
        }
    }
    if !signatures.is_empty() {
        return Some(
            signatures
                .into_iter()
                .map(|(args, result)| {
                    (
                        args.into_iter().map(|ty| Type::Named(ty.into())).collect(),
                        Type::Named(result.into()),
                    )
                })
                .collect(),
        );
    }
    let (parameters, output, optional) = match name {
        "number" => (vec!["String", "String", "String"], "Number", 0),
        "round" | "roundUp" | "roundDown" | "roundHalfUp" | "roundHalfDown" | "roundHalfEven" => {
            (vec!["Number", "Number"], "Number", 0)
        }
        "floor" | "ceiling" => (vec!["Number", "Number"], "Number", 1),
        "modulo" => (vec!["Number", "Number"], "Number", 0),
        "log" => (vec!["Number", "Number"], "Number", 1),
        "clamp" => (vec!["Number", "Number", "Number"], "Number", 0),
        "odd" | "even" | "isPositive" | "isNegative" | "isZero" => (vec!["Number"], "Bool", 0),
        "abs" | "sqrt" | "exp" | "ln" => (vec!["Number"], "Number", 0),
        "stringLength" => (vec!["String"], "Number", 0),
        "substring" => (vec!["String", "Number", "Number"], "String", 1),
        "charAt" | "repeat" => (vec!["String", "Number"], "String", 0),
        "padLeading" | "padTrailing" => (vec!["String", "Number", "String"], "String", 1),
        "substringBefore" | "substringAfter" => (vec!["String", "String"], "String", 0),
        "upperCase" | "lowerCase" | "trim" | "trimLeading" | "trimTrailing" => {
            (vec!["String"], "String", 0)
        }
        "startsWith" | "endsWith" => (vec!["String", "String"], "Bool", 0),
        "isBlank" => (vec!["String"], "Bool", 0),
        "matches" => (vec!["String", "String", "String"], "Bool", 1),
        "replace" => (vec!["String", "String", "String", "String"], "String", 1),
        "extract" => (vec!["String", "String", "String"], "List<List<String>>", 1),
        _ => return None,
    };
    let parameters: Vec<Type> = parameters
        .into_iter()
        .map(|ty| Type::Named(ty.into()))
        .collect();
    let output = if output == "List<List<String>>" {
        Type::Generic(
            "List".into(),
            Box::new(Type::Generic(
                "List".into(),
                Box::new(Type::Named("String".into())),
            )),
        )
    } else {
        Type::Named(output.into())
    };
    let mut signatures = vec![(parameters.clone(), output.clone())];
    if optional > 0 {
        signatures.push((parameters[..parameters.len() - 1].to_vec(), output.clone()));
    }
    if name == "number" {
        signatures.push((vec![Type::Named("String".into())], output));
    }
    signatures.extend(signatures_for_duration(name));
    Some(signatures)
}

fn signatures_for_duration(name: &str) -> Vec<(Vec<Type>, Type)> {
    if !matches!(
        name,
        "abs"
            | "isNegative"
            | "round"
            | "roundUp"
            | "roundDown"
            | "roundHalfUp"
            | "roundHalfDown"
            | "roundHalfEven"
    ) {
        return Vec::new();
    }
    ["DTDuration", "YMDuration"]
        .into_iter()
        .map(|kind| {
            let duration = Type::Named(kind.into());
            let args = if matches!(name, "abs" | "isNegative") {
                vec![duration.clone()]
            } else {
                vec![duration.clone(), duration.clone()]
            };
            let result = if name == "isNegative" {
                Type::Named("Bool".into())
            } else {
                duration
            };
            (args, result)
        })
        .collect()
}

pub(super) fn calendar_call_signatures(name: &str) -> Option<Vec<(Vec<Type>, Type)>> {
    let named_type = |name: &str| Type::Named(name.into());
    let list = |name: &str| Type::Generic("List".into(), Box::new(named_type(name)));
    let range = |name: &str| Type::Generic("Range".into(), Box::new(named_type(name)));
    let calendar = named_type("Calendar");
    let entry = named_type("CalendarEntry");
    let calendar_signatures = match name {
        "entryName" => vec![(vec![entry.clone()], named_type("String"))],
        "entryValue" => vec![(vec![entry.clone()], named_type("CalendarValue"))],
        "calendarMerge" => (0..=2)
            .map(|options| {
                let mut args = vec![list("Calendar")];
                args.extend(std::iter::repeat_n(named_type("String"), options));
                (args, calendar.clone())
            })
            .collect(),
        "calendarDrop" | "calendarKeep" => {
            let mut signatures = Vec::new();
            for target in [
                named_type("String"),
                named_type("Date"),
                named_type("DateTime"),
                range("Date"),
                range("DateTime"),
            ] {
                signatures.push((vec![calendar.clone(), target.clone()], calendar.clone()));
                signatures.push((
                    vec![calendar.clone(), target, named_type("String")],
                    calendar.clone(),
                ));
            }
            signatures
        }
        "count" | "isEmpty" | "entries" | "names" | "find" | "contains" | "entriesFor"
        | "overlaps" | "entriesIn" | "validFrom" | "validTo" | "validRange" | "next" | "prev" => {
            let output = match name {
                "count" => named_type("Number"),
                "isEmpty" | "contains" | "overlaps" => named_type("Bool"),
                "entries" | "find" | "entriesFor" | "entriesIn" => list("CalendarEntry"),
                "names" => list("String"),
                "validFrom" | "validTo" => named_type("CalendarPoint"),
                "validRange" => named_type("CalendarRange"),
                _ => entry,
            };
            let mut params = vec![calendar];
            match name {
                "find" => params.push(named_type("String")),
                "contains" | "entriesFor" | "next" | "prev" => {
                    return Some(
                        ["Date", "DateTime"]
                            .into_iter()
                            .flat_map(|point| {
                                let mut args = params.clone();
                                args.push(named_type(point));
                                let mut signatures = vec![(args.clone(), output.clone())];
                                if matches!(name, "next" | "prev") {
                                    args.push(named_type("Number"));
                                    signatures.push((args, output.clone()));
                                }
                                signatures
                            })
                            .collect(),
                    );
                }
                "overlaps" | "entriesIn" => {
                    return Some(
                        [
                            range("Date"),
                            range("DateTime"),
                            named_type("CalendarRange"),
                        ]
                        .into_iter()
                        .map(|ty| (vec![params[0].clone(), ty], output.clone()))
                        .collect(),
                    );
                }
                _ => {}
            }
            vec![(params, output)]
        }
        _ => vec![],
    };
    if calendar_signatures.is_empty() {
        None
    } else {
        Some(calendar_signatures)
    }
}

pub(super) fn signature_branches(
    base: &Constraints,
    parameters: &[Type],
    output: &Type,
    arguments: &[Term],
    result: &Term,
) -> Vec<Constraints> {
    if parameters.len() != arguments.len() {
        return Vec::new();
    }
    let mut equations: Vec<_> = arguments
        .iter()
        .zip(parameters)
        .map(|(arg, ty)| (arg.clone(), Term::from(ty)))
        .collect();
    equations.push((result.clone(), Term::from(output)));
    base.branch(&equations).into_iter().collect()
}

pub(super) fn call_branches(
    base: &Constraints,
    signatures: &[(Vec<Type>, Type)],
    arguments: &[Term],
    result: &Term,
) -> Vec<Constraints> {
    signatures
        .iter()
        .flat_map(|(parameters, output)| {
            signature_branches(base, parameters, output, arguments, result)
        })
        .collect()
}

pub(super) fn sort_call_signatures() -> Vec<(Vec<Type>, Type)> {
    [
        "Number",
        "String",
        "Date",
        "Time",
        "DateTime",
        "DTDuration",
        "YMDuration",
    ]
    .into_iter()
    .flat_map(|name| {
        let list = Type::Generic("List".into(), Box::new(Type::Named(name.into())));
        [
            (vec![list.clone()], list.clone()),
            (
                vec![list.clone(), Type::Named("String".into())],
                list.clone(),
            ),
            (vec![list.clone(), list.clone()], list),
        ]
    })
    .collect()
}

pub(super) fn list_call_branches(
    base: &Constraints,
    name: &str,
    arguments: &[Term],
    result: &Term,
) -> Vec<Constraints> {
    let valid_arity = match name {
        "count" | "isEmpty" => arguments.len() == 1,
        "listContains" | "indexOf" => arguments.len() == 2,
        "sublist" => matches!(arguments.len(), 2 | 3),
        "append" | "prepend" => arguments.len() >= 2,
        "concatenate" | "union" | "intersection" => !arguments.is_empty(),
        "reverse" | "flatten" | "distinct" | "duplicateValues" | "mode" => arguments.len() == 1,
        "insertBefore" | "insertAfter" | "listReplace" => arguments.len() == 3,
        "remove" => arguments.len() == 2,
        _ => false,
    };
    if !valid_arity {
        return Vec::new();
    }
    let element = fresh_var(
        base,
        &arguments
            .iter()
            .chain(std::iter::once(result))
            .collect::<Vec<_>>(),
    );
    let mut equations = vec![(arguments[0].clone(), generic("List", element.clone()))];
    if matches!(name, "listContains" | "indexOf") {
        equations.push((arguments[1].clone(), element.clone()));
    } else if name == "sublist" {
        equations.extend(
            arguments[1..]
                .iter()
                .map(|arg| (arg.clone(), named("Number"))),
        );
    } else if matches!(name, "append" | "prepend") {
        equations.extend(
            arguments[1..]
                .iter()
                .map(|arg| (arg.clone(), element.clone())),
        );
    } else if matches!(
        name,
        "insertBefore" | "insertAfter" | "remove" | "listReplace"
    ) {
        equations.push((arguments[1].clone(), named("Number")));
        if arguments.len() == 3 {
            equations.push((arguments[2].clone(), element.clone()));
        }
    } else if matches!(name, "concatenate" | "union" | "intersection") {
        equations.extend(
            arguments[1..]
                .iter()
                .map(|arg| (arg.clone(), generic("List", element.clone()))),
        );
    }
    equations.push((
        result.clone(),
        match name {
            "count" => named("Number"),
            "isEmpty" | "listContains" => named("Bool"),
            "indexOf" => generic("List", named("Number")),
            "sublist" | "append" | "prepend" | "concatenate" | "union" | "intersection"
            | "distinct" | "duplicateValues" | "reverse" | "mode" | "insertBefore"
            | "insertAfter" | "remove" | "listReplace" => generic("List", element.clone()),
            "flatten" => generic("List", fresh_var(base, &[&arguments[0], result, &element])),
            _ => unreachable!(),
        },
    ));
    let mut branches: Vec<_> = base.branch(&equations).into_iter().collect();
    if matches!(name, "insertBefore" | "insertAfter") {
        branches = branches
            .into_iter()
            .filter_map(|branch| branch.with_not_list(&arguments[2]))
            .collect();
        equations[2].1 = generic("List", element);
        branches.extend(base.branch(&equations));
    }
    branches
}

pub(super) fn range_relation_branches(
    base: &Constraints,
    name: &str,
    arguments: &[Term],
    result: &Term,
) -> Vec<Constraints> {
    let [first, second] = arguments else {
        return Vec::new();
    };
    let mut branches = Vec::new();
    for kind in ["Number", "Date", "DateTime", "Time"] {
        let point = named(kind);
        let range = generic("Range", point.clone());
        let pairs = if matches!(name, "includes" | "startedBy" | "finishedBy") {
            vec![(range.clone(), point)]
        } else if matches!(name, "during" | "starts" | "finishes") {
            vec![(point, range.clone())]
        } else {
            let mut pairs = vec![(range.clone(), range)];
            if kind == "Number" && matches!(name, "before" | "after" | "meets" | "metBy") {
                pairs.extend([
                    (named("Number"), generic("Range", named("Number"))),
                    (generic("Range", named("Number")), named("Number")),
                ]);
            }
            pairs
        };
        for (left, right) in pairs {
            branches.extend(base.branch(&[
                (first.clone(), left),
                (second.clone(), right),
                (result.clone(), named("Bool")),
            ]));
        }
    }
    branches
}

fn fresh_var(base: &Constraints, terms: &[&Term]) -> Term {
    fn contains(term: &Term, index: usize) -> bool {
        match term {
            Term::Var(other) => *other == index,
            Term::Generic(_, inner) => contains(inner, index),
            Term::Named(_) => false,
        }
    }
    let mut index = base.reserved;
    while base.substitutions.contains_key(&index)
        || base
            .substitutions
            .values()
            .any(|term| contains(term, index))
        || terms.iter().any(|term| contains(term, index))
    {
        index += 1;
    }
    Term::Var(index)
}

pub(super) fn dictionary_call_branches(
    base: &Constraints,
    name: &str,
    arguments: &[Term],
    result: &Term,
    records: &[Record],
    path: Option<(usize, Option<&str>)>,
) -> Vec<Constraints> {
    if arguments.is_empty() {
        return Vec::new();
    }
    let value = fresh_var(
        base,
        &arguments
            .iter()
            .chain(std::iter::once(result))
            .collect::<Vec<_>>(),
    );
    let dynamic = fresh_var(
        base,
        &arguments
            .iter()
            .chain(std::iter::once(result))
            .chain(std::iter::once(&value))
            .collect::<Vec<_>>(),
    );
    let mut shapes = vec![
        (generic("Dictionary", value.clone()), value),
        (named("Dictionary"), named("Value")),
    ];
    shapes.extend(
        records
            .iter()
            .map(|record| (named(&record.name), named("Value"))),
    );
    if matches!(
        name,
        "keys" | "values" | "getEntries" | "size" | "isEmpty" | "has" | "getValue"
    ) {
        shapes.push((named("Value"), named("Value")));
    }
    let mut branches = Vec::new();
    'shape: for (shape, item) in shapes {
        let source = if name == "dictionaryMerge" {
            generic("List", shape.clone())
        } else {
            shape.clone()
        };
        let Some(mut branch) = base.branch(&[(source, arguments[0].clone())]) else {
            continue;
        };
        let mut output = match name {
            "keys" => generic("List", named("String")),
            "values" => generic("List", item.clone()),
            "getEntries" => generic("List", generic("DictionaryEntry", item.clone())),
            "size" => named("Number"),
            "isEmpty" | "has" => named("Bool"),
            "getValue" => {
                if let Term::Named(record_name) = &shape {
                    path.and_then(|(_, key)| key)
                        .and_then(|key| {
                            records
                                .iter()
                                .find(|record| &record.name == record_name)
                                .and_then(|record| {
                                    record.fields.iter().find(|(field, _)| field == key)
                                })
                                .map(|(_, ty)| Term::from(ty))
                        })
                        .unwrap_or_else(|| dynamic.clone())
                } else {
                    item.clone()
                }
            }
            "dictionaryPut" | "dictionaryRemove" | "dictionaryMerge" => named("Dictionary"),
            _ => continue,
        };
        if name == "getValue" {
            for _ in 1..path.map_or(1, |(steps, _)| steps.max(1)) {
                output = match branch.resolve(&output) {
                    Term::Generic(kind, inner) if kind == "Dictionary" => *inner,
                    Term::Var(_) => {
                        let next = fresh_var(&branch, &[&output, &dynamic, result]);
                        let Some(constrained) =
                            branch.branch(&[(output, generic("Dictionary", next.clone()))])
                        else {
                            continue 'shape;
                        };
                        branch = constrained;
                        next
                    }
                    _ => dynamic.clone(),
                };
            }
        }
        let Some(branch) = branch.branch(&[(result.clone(), output)]) else {
            continue;
        };
        let valid_arity = match name {
            "has" | "getValue" | "dictionaryRemove" => arguments.len() == 2,
            "dictionaryPut" => arguments.len() == 3,
            _ => arguments.len() == 1,
        };
        if !valid_arity {
            continue;
        }
        if arguments.len() == 1 {
            branches.push(branch);
        } else {
            let keys = if matches!(name, "getValue" | "dictionaryPut") {
                vec![named("String"), generic("List", named("String"))]
            } else {
                vec![named("String")]
            };
            for key in keys {
                branches.extend(branch.branch(&[(arguments[1].clone(), key)]));
            }
        }
    }
    branches
}

pub(super) fn index_branches(
    base: &Constraints,
    source: &Term,
    key: &Term,
    result: &Term,
    records: &[Record],
    literal_key: Option<&str>,
) -> Vec<Constraints> {
    let item = fresh_var(base, &[source, key, result]);
    let dynamic = fresh_var(base, &[source, key, result, &item]);
    let mut branches = Vec::new();
    for (shape, position, output) in [
        (generic("List", item.clone()), named("Number"), item.clone()),
        (
            generic("List", item.clone()),
            named("Bool"),
            generic("List", item.clone()),
        ),
        (generic("Dictionary", item.clone()), named("String"), item),
        (named("Dictionary"), named("String"), dynamic.clone()),
        (named("Value"), named("String"), dynamic),
    ] {
        branches.extend(base.branch(&[
            (source.clone(), shape),
            (key.clone(), position),
            (result.clone(), output),
        ]));
    }
    for record in records {
        let output = if let Some(key) = literal_key {
            let Some((_, ty)) = record.fields.iter().find(|(field, _)| field == key) else {
                continue;
            };
            Term::from(ty)
        } else {
            named("Value")
        };
        branches.extend(base.branch(&[
            (source.clone(), named(&record.name)),
            (key.clone(), named("String")),
            (result.clone(), output),
        ]));
    }
    branches
}

pub(super) fn list_branches(
    base: &Constraints,
    elements: &[Term],
    result: &Term,
) -> Vec<Constraints> {
    let item = fresh_var(
        base,
        &elements
            .iter()
            .chain(std::iter::once(result))
            .collect::<Vec<_>>(),
    );
    list_branches_with_item(base, elements, result, &item)
}

fn list_branches_with_item(
    base: &Constraints,
    elements: &[Term],
    result: &Term,
    item: &Term,
) -> Vec<Constraints> {
    let mut equations = vec![(result.clone(), generic("List", item.clone()))];
    equations.extend(
        elements
            .iter()
            .cloned()
            .map(|element| (element, item.clone())),
    );
    base.branch(&equations).into_iter().collect()
}

pub(super) fn range_branches(
    base: &Constraints,
    bounds: &[Term],
    result: &Term,
) -> Vec<Constraints> {
    ["Number", "Date", "DateTime", "Time"]
        .into_iter()
        .filter_map(|kind| {
            let mut equations = vec![(result.clone(), generic("Range", named(kind)))];
            equations.extend(bounds.iter().cloned().map(|bound| (bound, named(kind))));
            base.branch(&equations)
        })
        .collect()
}

pub(super) enum IterationKind {
    Every,
    Map,
    Flatten,
}

pub(super) fn iteration_branches(
    base: &Constraints,
    source: &Term,
    binding: &Term,
    body: &Term,
    result: &Term,
    kind: IterationKind,
    flatten_item: &Term,
) -> Vec<Constraints> {
    let output = match kind {
        IterationKind::Every => named("Bool"),
        IterationKind::Map => generic("List", body.clone()),
        IterationKind::Flatten => body.clone(),
    };
    let mut equations = vec![
        (source.clone(), generic("List", binding.clone())),
        (result.clone(), output),
    ];
    match kind {
        IterationKind::Every => equations.push((body.clone(), named("Bool"))),
        IterationKind::Flatten => {
            equations.push((body.clone(), generic("List", flatten_item.clone())))
        }
        IterationKind::Map => {}
    }
    base.branch(&equations).into_iter().collect()
}

// A partial traversal: None means an expression family not yet migrated into the relation.
// Empty branches mean the migrated expression has incompatible constraints.
pub(super) fn expression_branches(
    expr: &Expr,
    result: &Term,
    env: &HashMap<String, Term>,
    records: &[Record],
    knowledge: &[Knowledge],
    enums: &[Enum],
) -> Option<Vec<Constraints>> {
    expression_branches_with_bindings(
        expr,
        result,
        env,
        records,
        knowledge,
        enums,
        &mut HashMap::new(),
    )
}

pub(super) fn expression_branches_with_bindings(
    expr: &Expr,
    result: &Term,
    env: &HashMap<String, Term>,
    records: &[Record],
    knowledge: &[Knowledge],
    enums: &[Enum],
    bindings: &mut HashMap<*const Expr, Term>,
) -> Option<Vec<Constraints>> {
    fn max_var(term: &Term) -> usize {
        match term {
            Term::Var(index) => *index,
            Term::Generic(_, inner) => max_var(inner),
            Term::Named(_) => 0,
        }
    }
    fn visit_calendar_target(
        name: &str,
        target: &Expr,
        env: &HashMap<String, Term>,
        context: (&[Record], &[Knowledge], &[Enum]),
        bindings: &mut HashMap<*const Expr, Term>,
        next: &mut usize,
        branches: Vec<Constraints>,
    ) -> Option<Vec<Constraints>> {
        match target {
            Expr::List(items) => {
                let mut branches = branches;
                for item in items {
                    branches =
                        visit_calendar_target(name, item, env, context, bindings, next, branches)?;
                }
                Some(branches)
            }
            Expr::Call(call, args) if call == "pattern" => {
                let [pattern] = args.as_slice() else {
                    return Some(Vec::new());
                };
                visit(
                    pattern,
                    &named("String"),
                    env,
                    context,
                    bindings,
                    next,
                    branches,
                )
            }
            _ => {
                let target_type = Term::Var(*next);
                *next += 1;
                let signatures = calendar_call_signatures(name)?;
                let branches = branches
                    .iter()
                    .flat_map(|branch| {
                        call_branches(
                            branch,
                            &signatures,
                            &[named("Calendar"), target_type.clone()],
                            &named("Calendar"),
                        )
                    })
                    .collect();
                visit(target, &target_type, env, context, bindings, next, branches)
            }
        }
    }

    fn visit(
        expr: &Expr,
        result: &Term,
        env: &HashMap<String, Term>,
        context: (&[Record], &[Knowledge], &[Enum]),
        bindings: &mut HashMap<*const Expr, Term>,
        next: &mut usize,
        branches: Vec<Constraints>,
    ) -> Option<Vec<Constraints>> {
        let (records, knowledge, enums) = context;
        if let Some(used) = branches
            .iter()
            .flat_map(|branch| {
                branch
                    .substitutions
                    .iter()
                    .flat_map(|(index, value)| [*index, max_var(value)])
            })
            .max()
        {
            *next = (*next).max(used.saturating_add(1));
        }
        let mut allocate = || {
            let variable = Term::Var(*next);
            *next += 1;
            variable
        };
        let constrain = |branches: Vec<Constraints>, left: Term, right: Term| {
            branches
                .into_iter()
                .filter_map(|branch| branch.branch(&[(left.clone(), right.clone())]))
                .collect()
        };
        match expr {
            Expr::Number(_) => Some(constrain(branches, result.clone(), named("Number"))),
            Expr::String(_) => Some(constrain(branches, result.clone(), named("String"))),
            Expr::Bool(_) => Some(constrain(branches, result.clone(), named("Bool"))),
            Expr::Name(name) => Some(env.get(name).map_or_else(Vec::new, |ty| {
                constrain(branches, result.clone(), ty.clone())
            })),
            Expr::FilterItem => Some(env.get(".item").map_or_else(Vec::new, |item| {
                constrain(branches, result.clone(), item.clone())
            })),
            Expr::InlinePredicate { .. }
            | Expr::NamedArgument(..)
            | Expr::TableSelect { .. }
            | Expr::Column(_)
            | Expr::OuterVar(_)
            | Expr::TableRange(..)
            | Expr::TableMethod { .. }
            | Expr::TableJoin(..)
            | Expr::TableSortKey { .. }
            | Expr::Rename(..)
            | Expr::Output(..)
            | Expr::TableEqual(..) => Some(Vec::new()),
            Expr::Some {
                bindings: sources,
                predicate,
            } => {
                let mut branches = constrain(branches, result.clone(), named("Bool"));
                let mut local = env.clone();
                for (binding, source) in sources {
                    let item = Term::Var(*next);
                    *next += 1;
                    bindings.insert(std::ptr::from_ref(source), item.clone());
                    branches = visit(
                        source,
                        &generic("List", item.clone()),
                        &local,
                        context,
                        bindings,
                        next,
                        branches,
                    )?;
                    local.insert(binding.clone(), item);
                }
                visit(
                    predicate,
                    &named("Bool"),
                    &local,
                    context,
                    bindings,
                    next,
                    branches,
                )
            }
            Expr::List(items) => {
                let terms: Vec<_> = items.iter().map(|_| allocate()).collect();
                let item = allocate();
                let mut branches: Vec<_> = branches
                    .iter()
                    .flat_map(|branch| list_branches_with_item(branch, &terms, result, &item))
                    .collect();
                for (item, term) in items.iter().zip(terms) {
                    branches = visit(item, &term, env, context, bindings, next, branches)?;
                }
                Some(branches)
            }
            Expr::Range(lower, upper, _, _) => {
                let bounds: Vec<_> = lower
                    .iter()
                    .chain(upper.iter())
                    .map(|_| allocate())
                    .collect();
                let mut branches: Vec<_> = branches
                    .iter()
                    .flat_map(|branch| range_branches(branch, &bounds, result))
                    .collect();
                for (bound, term) in lower.iter().chain(upper.iter()).zip(bounds) {
                    branches = visit(bound, &term, env, context, bindings, next, branches)?;
                }
                Some(branches)
            }
            Expr::Iteration {
                every,
                flatten,
                binding,
                source,
                body,
            } => {
                let source_term = allocate();
                let binding_term = allocate();
                let body_term = allocate();
                let flatten_item = allocate();
                bindings.insert(std::ptr::from_ref(expr), binding_term.clone());
                let mut branches: Vec<_> = branches
                    .iter()
                    .flat_map(|branch| {
                        iteration_branches(
                            branch,
                            &source_term,
                            &binding_term,
                            &body_term,
                            result,
                            if *every {
                                IterationKind::Every
                            } else if *flatten {
                                IterationKind::Flatten
                            } else {
                                IterationKind::Map
                            },
                            &flatten_item,
                        )
                    })
                    .collect();
                branches = visit(source, &source_term, env, context, bindings, next, branches)?;
                let mut local = env.clone();
                local.insert(binding.clone(), binding_term);
                visit(body, &body_term, &local, context, bindings, next, branches)
            }
            Expr::Not(value) => {
                let operand = allocate();
                let branches = branches
                    .iter()
                    .flat_map(|branch| not_branches(branch, &operand, result))
                    .collect();
                visit(value, &operand, env, context, bindings, next, branches)
            }
            Expr::Binary(left, op, right) => {
                let lhs = allocate();
                let rhs = allocate();
                let branches = branches
                    .iter()
                    .flat_map(|branch| {
                        binary_branches(
                            branch,
                            op,
                            matches!(left.as_ref(), Expr::Number(value) if value == "0"),
                            &lhs,
                            &rhs,
                            result,
                        )
                    })
                    .collect();
                let branches = visit(left, &lhs, env, context, bindings, next, branches)?;
                visit(right, &rhs, env, context, bindings, next, branches)
            }
            Expr::Field(base, field) => {
                if let Expr::Call(name, args) = base.as_ref()
                    && name == "table"
                    && !knowledge.iter().any(|definition| definition.name == *name)
                    && let Some(Expr::List(headers)) = args.first()
                    && let Some(index) = headers.iter().position(
                        |header| matches!(header, Expr::String(column) if column == field),
                    )
                {
                    let element = allocate();
                    let mut branches =
                        constrain(branches, result.clone(), generic("List", element.clone()));
                    for row in &args[1..] {
                        let Expr::List(cells) = row else {
                            return Some(Vec::new());
                        };
                        let Some(cell) = cells.get(index) else {
                            return Some(Vec::new());
                        };
                        branches = visit(cell, &element, env, context, bindings, next, branches)?;
                    }
                    return Some(branches);
                }
                if let Expr::Name(name) = base.as_ref()
                    && !env.contains_key(name)
                    && let Some(variant) = enums.iter().find(|item| item.name == *name)
                {
                    return Some(if variant.variants.contains(field) {
                        constrain(branches, result.clone(), named(name))
                    } else {
                        Vec::new()
                    });
                }
                let source = allocate();
                let branches = branches
                    .iter()
                    .flat_map(|branch| field_branches(branch, field, &source, result, records))
                    .collect();
                visit(base, &source, env, context, bindings, next, branches)
            }
            Expr::Dictionary(entries) => {
                let mut fields = Vec::new();
                for (key, _) in entries {
                    fields.push((key.clone(), allocate()));
                }
                let mut branches: Vec<_> = branches
                    .iter()
                    .flat_map(|branch| {
                        dictionary_literal_branches(branch, &fields, result, records)
                    })
                    .collect();
                let mut local = env.clone();
                for ((_, value), (key, term)) in entries.iter().zip(fields) {
                    branches = visit(value, &term, &local, context, bindings, next, branches)?;
                    if crate::compiler::identifier(&key) {
                        local.insert(key, term);
                    }
                }
                Some(branches)
            }
            Expr::Sequence(start, end) => {
                let start_term = allocate();
                let end_term = allocate();
                let branches = constrain(branches, start_term.clone(), named("Number"));
                let branches = constrain(branches, end_term.clone(), named("Number"));
                let branches =
                    constrain(branches, result.clone(), generic("List", named("Number")));
                let branches = visit(start, &start_term, env, context, bindings, next, branches)?;
                visit(end, &end_term, env, context, bindings, next, branches)
            }
            Expr::Index(base, key) => {
                let source = allocate();
                let position = allocate();
                let literal_key = if let Expr::String(key) = key.as_ref() {
                    Some(key.as_str())
                } else {
                    None
                };
                let branches: Vec<_> = branches
                    .iter()
                    .flat_map(|branch| {
                        index_branches(branch, &source, &position, result, records, literal_key)
                    })
                    .collect();
                let branches = visit(base, &source, env, context, bindings, next, branches)?;
                let mut checked = Vec::new();
                for branch in branches {
                    let mut local = env.clone();
                    if let Term::Generic(kind, element) = branch.resolve(&source)
                        && kind == "List"
                        && branch.resolve(&position) == named("Bool")
                    {
                        local.insert(".item".into(), *element);
                    }
                    checked.extend(visit(
                        key,
                        &position,
                        &local,
                        context,
                        bindings,
                        next,
                        vec![branch],
                    )?);
                }
                Some(checked)
            }
            Expr::Call(name, args)
                if matches!(name.as_str(), "table" | "tableFromDicts")
                    && !knowledge.iter().any(|definition| definition.name == *name) =>
            {
                let mut branches = constrain(branches, result.clone(), named("Table"));
                if name == "table" {
                    let (headers, rows) = args.split_first()?;
                    branches = visit(
                        headers,
                        &generic("List", named("String")),
                        env,
                        context,
                        bindings,
                        next,
                        branches,
                    )?;
                    for row in rows {
                        if let Expr::List(cells) = row {
                            for cell in cells {
                                let cell_type = Term::Var(*next);
                                *next += 1;
                                branches = visit(
                                    cell, &cell_type, env, context, bindings, next, branches,
                                )?;
                            }
                        } else {
                            let cell_type = Term::Var(*next);
                            *next += 1;
                            branches = visit(
                                row,
                                &generic("List", cell_type),
                                env,
                                context,
                                bindings,
                                next,
                                branches,
                            )?;
                        }
                    }
                } else {
                    let [rows] = args.as_slice() else {
                        return Some(Vec::new());
                    };
                    if let Expr::List(items) = rows {
                        for item in items {
                            let dictionary = Term::Var(*next);
                            *next += 1;
                            branches =
                                visit(item, &dictionary, env, context, bindings, next, branches)?;
                        }
                    } else {
                        let dictionary = Term::Var(*next);
                        *next += 1;
                        branches = visit(
                            rows,
                            &generic("List", dictionary),
                            env,
                            context,
                            bindings,
                            next,
                            branches,
                        )?;
                    }
                }
                Some(branches)
            }
            Expr::Call(name, args) => {
                if matches!(name.as_str(), "remove" | "listReplace")
                    && !knowledge.iter().any(|definition| definition.name == *name)
                    && let Some(Expr::InlinePredicate { binding, body }) = args.get(1)
                {
                    if args.len() != if name == "remove" { 2 } else { 3 } {
                        return Some(Vec::new());
                    }
                    let item = allocate();
                    let list = generic("List", item.clone());
                    let branches = constrain(branches, result.clone(), list.clone());
                    let branches = visit(&args[0], &list, env, context, bindings, next, branches)?;
                    let mut local = env.clone();
                    local.insert(binding.clone(), item.clone());
                    let branches = visit(
                        body,
                        &named("Bool"),
                        &local,
                        context,
                        bindings,
                        next,
                        branches,
                    )?;
                    return if name == "listReplace" {
                        visit(&args[2], &item, env, context, bindings, next, branches)
                    } else {
                        Some(branches)
                    };
                }
                if name == "coalesce"
                    && !knowledge.iter().any(|definition| definition.name == *name)
                {
                    let [value, fallback] = args.as_slice() else {
                        return Some(Vec::new());
                    };
                    let inner = allocate();
                    let branches = constrain(branches, result.clone(), inner.clone());
                    let branches = visit(
                        value,
                        &generic("Nullable", inner.clone()),
                        env,
                        context,
                        bindings,
                        next,
                        branches,
                    )?;
                    return visit(fallback, &inner, env, context, bindings, next, branches);
                }
                if matches!(
                    name.as_str(),
                    "calendarDrop" | "calendarKeep" | "calendarMerge"
                ) && !knowledge.iter().any(|definition| definition.name == *name)
                {
                    let (first, rest) = args.split_first()?;
                    let first_type = if name == "calendarMerge" {
                        generic("List", named("Calendar"))
                    } else {
                        named("Calendar")
                    };
                    let mut branches = constrain(branches, result.clone(), named("Calendar"));
                    branches = visit(first, &first_type, env, context, bindings, next, branches)?;
                    let options = if name == "calendarMerge" {
                        rest
                    } else {
                        let Some((target, options)) = rest.split_first() else {
                            return Some(Vec::new());
                        };
                        branches = visit_calendar_target(
                            name, target, env, context, bindings, next, branches,
                        )?;
                        options
                    };
                    if options.len() > if name == "calendarMerge" { 2 } else { 1 } {
                        return Some(Vec::new());
                    }
                    for option in options {
                        let Expr::NamedArgument(key, value) = option else {
                            return Some(Vec::new());
                        };
                        if !matches!(
                            (name.as_str(), key.as_str()),
                            ("calendarMerge", "dedupeBy" | "tiebreak")
                                | ("calendarDrop" | "calendarKeep", "rangeMatch")
                        ) {
                            return Some(Vec::new());
                        }
                        branches = visit(
                            value,
                            &named("String"),
                            env,
                            context,
                            bindings,
                            next,
                            branches,
                        )?;
                    }
                    return Some(branches);
                }
                let terms: Vec<_> = args.iter().map(|_| allocate()).collect();
                let branches: Vec<_> = {
                    let definition = knowledge.iter().find(|item| item.name == *name);
                    let signatures = if let Some(definition) = definition {
                        Some(vec![(
                            definition.params.iter().map(|(_, ty)| ty.clone()).collect(),
                            definition.output.clone(),
                        )])
                    } else if name == "sort" {
                        Some(sort_call_signatures())
                    } else {
                        fixed_call_signatures(name)
                    };
                    let shadowed = definition.is_some();
                    let list_call = !shadowed
                        && matches!(
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
                                | "mode"
                        );
                    let calendar = if shadowed {
                        None
                    } else {
                        calendar_call_signatures(name)
                    };
                    let dictionary = !shadowed
                        && matches!(
                            name.as_str(),
                            "keys"
                                | "values"
                                | "getEntries"
                                | "size"
                                | "isEmpty"
                                | "has"
                                | "getValue"
                                | "dictionaryPut"
                                | "dictionaryRemove"
                                | "dictionaryMerge"
                        );
                    if signatures.is_none()
                        && calendar.is_none()
                        && !dictionary
                        && !list_call
                        && (shadowed || !super::types::range_relation(name))
                    {
                        return None;
                    }
                    let path = if name == "getValue" {
                        match args.get(1) {
                            Some(Expr::String(key)) => Some((1, Some(key.as_str()))),
                            Some(Expr::List(items)) => Some((
                                items.len(),
                                items.first().and_then(|item| {
                                    if let Expr::String(key) = item {
                                        Some(key.as_str())
                                    } else {
                                        None
                                    }
                                }),
                            )),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    branches
                        .iter()
                        .flat_map(|branch| {
                            let mut choices = Vec::new();
                            if let Some(ref signatures) = signatures {
                                choices.extend(call_branches(branch, signatures, &terms, result));
                            }
                            if list_call {
                                choices.extend(list_call_branches(branch, name, &terms, result));
                            }
                            if let Some(ref signatures) = calendar {
                                choices.extend(call_branches(branch, signatures, &terms, result));
                            }
                            if !shadowed && super::types::range_relation(name) {
                                choices
                                    .extend(range_relation_branches(branch, name, &terms, result));
                            }
                            if dictionary {
                                choices.extend(dictionary_call_branches(
                                    branch, name, &terms, result, records, path,
                                ));
                            }
                            choices
                        })
                        .collect()
                };
                let mut branches = branches;
                for (arg, term) in args.iter().zip(terms.iter()) {
                    branches = visit(arg, term, env, context, bindings, next, branches)?;
                }
                if name == "flatten" {
                    branches = branches
                        .into_iter()
                        .filter_map(|branch| {
                            let mut source = branch.resolve(&terms[0]);
                            while let Term::Generic(kind, inner) = source {
                                if kind != "List" {
                                    source = Term::Generic(kind, inner);
                                    break;
                                }
                                source = *inner;
                            }
                            if matches!(source, Term::Var(_)) {
                                return Some(branch);
                            }
                            branch.branch(&[(result.clone(), generic("List", source))])
                        })
                        .collect();
                }
                Some(branches)
            }
        }
    }
    let mut next = env
        .values()
        .map(max_var)
        .chain(std::iter::once(max_var(result)))
        .max()?
        .checked_add(1)?;
    let base = Constraints {
        reserved: next,
        ..Constraints::default()
    };
    visit(
        expr,
        result,
        env,
        (records, knowledge, enums),
        bindings,
        &mut next,
        vec![base],
    )
}

pub(super) fn dictionary_literal_branches(
    base: &Constraints,
    fields: &[(String, Term)],
    result: &Term,
    records: &[Record],
) -> Vec<Constraints> {
    let value = fresh_var(
        base,
        &fields
            .iter()
            .map(|(_, ty)| ty)
            .chain(std::iter::once(result))
            .collect::<Vec<_>>(),
    );
    let mut branches = Vec::new();
    let mut equations = vec![(result.clone(), generic("Dictionary", value.clone()))];
    equations.extend(fields.iter().map(|(_, ty)| (ty.clone(), value.clone())));
    branches.extend(base.branch(&equations));
    branches.extend(base.branch(&[(result.clone(), named("Dictionary"))]));
    for record in records {
        if record.fields.len() != fields.len() {
            continue;
        }
        let mut equations = vec![(result.clone(), named(&record.name))];
        for (field, ty) in fields {
            let Some((_, expected)) = record.fields.iter().find(|(key, _)| key == field) else {
                equations.clear();
                break;
            };
            let expected = Term::from(expected);
            let nullable_inner = match &expected {
                Term::Generic(kind, inner) if kind == "Nullable" => Some(inner.as_ref()),
                _ => None,
            };
            equations.push((
                ty.clone(),
                nullable_inner
                    .filter(|inner| *inner == ty)
                    .cloned()
                    .unwrap_or(expected),
            ));
        }
        if !equations.is_empty() {
            branches.extend(base.branch(&equations));
        }
    }
    branches
}

fn scalar_field_type(shape: &str, field: &str) -> Option<&'static str> {
    match shape {
        "Date" | "Time" | "DateTime" => {
            let calendar = shape != "Time";
            let clock = shape != "Date";
            match field {
                "year" | "month" | "day" | "dayOfYear" | "weekOfYear" | "isoWeekOfYear"
                | "quarter"
                    if calendar =>
                {
                    Some("Number")
                }
                "hour" | "minute" | "second" if clock => Some("Number"),
                "offset" => Some("DTDuration"),
                "timezone" => Some("String"),
                "dayName" | "dayNameShort" | "isoYearWeek" | "monthName" | "monthNameShort"
                | "yearQuarter"
                    if calendar =>
                {
                    Some("String")
                }
                _ => None,
            }
        }
        "DTDuration"
            if matches!(
                field,
                "days"
                    | "hours"
                    | "minutes"
                    | "seconds"
                    | "totalSeconds"
                    | "totalMinutes"
                    | "totalHours"
                    | "totalDays"
            ) =>
        {
            Some("Number")
        }
        "YMDuration" if matches!(field, "years" | "months" | "totalMonths" | "totalYears") => {
            Some("Number")
        }
        _ => None,
    }
}

pub(super) fn field_branches(
    base: &Constraints,
    field: &str,
    source: &Term,
    result: &Term,
    records: &[Record],
) -> Vec<Constraints> {
    let item = fresh_var(base, &[source, result]);
    let dynamic = fresh_var(base, &[source, result, &item]);
    let mut shapes = vec![
        (generic("Dictionary", item.clone()), item.clone()),
        (named("Dictionary"), dynamic.clone()),
        (named("Value"), dynamic),
    ];
    if field == "key" {
        shapes.push((generic("DictionaryEntry", item.clone()), named("String")));
    }
    if field == "value" {
        shapes.push((generic("DictionaryEntry", item.clone()), item));
    }
    shapes.extend(records.iter().filter_map(|record| {
        record
            .fields
            .iter()
            .find(|(key, _)| key == field)
            .map(|(_, ty)| (named(&record.name), Term::from(ty)))
    }));
    shapes.extend(
        ["Date", "Time", "DateTime", "DTDuration", "YMDuration"]
            .iter()
            .filter_map(|shape| {
                scalar_field_type(shape, field).map(|output| (named(shape), named(output)))
            }),
    );
    let mut branches = Vec::new();
    for (shape, output) in shapes {
        let projects = matches!(&shape, Term::Generic(kind, _) if matches!(kind.as_str(), "Dictionary" | "DictionaryEntry"))
            || matches!(&shape, Term::Named(name) if matches!(name.as_str(), "Dictionary" | "Value") || records.iter().any(|record| record.name == *name));
        branches.extend(base.branch(&[
            (source.clone(), shape.clone()),
            (result.clone(), output.clone()),
        ]));
        if projects {
            branches.extend(base.branch(&[
                (source.clone(), generic("List", shape)),
                (result.clone(), generic("List", output)),
            ]));
        }
    }
    branches
}

pub(super) fn not_branches(base: &Constraints, operand: &Term, result: &Term) -> Vec<Constraints> {
    base.branch(&[
        (operand.clone(), named("Bool")),
        (result.clone(), named("Bool")),
    ])
    .into_iter()
    .collect()
}

// The same overload alternatives are used by ordinary validation and symbolic reverse inference.
pub(super) fn binary_branches(
    base: &Constraints,
    op: &str,
    left_is_zero: bool,
    left: &Term,
    right: &Term,
    result: &Term,
) -> Vec<Constraints> {
    let mut branches = Vec::new();
    let mut add = |lhs: Term, rhs: Term, output: Term| {
        if let Some(branch) = base.branch(&[
            (left.clone(), lhs),
            (right.clone(), rhs),
            (result.clone(), output),
        ]) {
            branches.push(branch);
        }
    };
    let bool_type = named("Bool");
    match op {
        "and" | "or" => add(bool_type.clone(), bool_type.clone(), bool_type),
        "==" | "!=" => {
            add(left.clone(), left.clone(), bool_type.clone());
            for value in ["CalendarValue", "CalendarPoint"] {
                for point in ["Date", "DateTime"] {
                    add(named(value), named(point), bool_type.clone());
                    add(named(point), named(value), bool_type.clone());
                }
            }
            for point in ["Date", "DateTime"] {
                add(
                    named("CalendarValue"),
                    generic("Range", named(point)),
                    bool_type.clone(),
                );
                add(
                    generic("Range", named(point)),
                    named("CalendarValue"),
                    bool_type.clone(),
                );
            }
        }
        "in" => {
            add(
                left.clone(),
                generic("List", left.clone()),
                bool_type.clone(),
            );
            add(
                left.clone(),
                generic("Range", left.clone()),
                bool_type.clone(),
            );
            for point in ["Date", "DateTime"] {
                for collection in ["Calendar", "CalendarRange", "CalendarValue"] {
                    add(named(point), named(collection), bool_type.clone());
                }
            }
        }
        _ => {}
    }
    for ty in [
        "Number",
        "String",
        "Date",
        "Time",
        "DateTime",
        "DTDuration",
        "YMDuration",
    ] {
        let value = named(ty);
        if matches!(op, "<" | "<=" | ">" | ">=") {
            add(value.clone(), value.clone(), named("Bool"));
        }
        if matches!(op, "+" | "-") && matches!(ty, "Number" | "DTDuration" | "YMDuration")
            || op == "+" && ty == "String"
            || matches!(op, "*" | "/" | "**" | "mod") && ty == "Number"
        {
            add(value.clone(), value.clone(), value.clone());
        }
        if op == "-" && matches!(ty, "Date" | "DateTime") {
            add(value.clone(), value.clone(), named("DTDuration"));
        }
    }
    for duration in ["DTDuration", "YMDuration"] {
        let value = named(duration);
        if matches!(op, "*" | "/") {
            add(value.clone(), named("Number"), value.clone());
        }
        if op == "*" {
            add(named("Number"), value.clone(), value.clone());
        }
        if op == "-" && left_is_zero {
            add(named("Number"), value.clone(), value.clone());
        }
        if matches!(op, "+" | "-") {
            for point in ["Date", "DateTime"] {
                add(named(point), value.clone(), named(point));
            }
            if duration == "DTDuration" {
                add(named("Time"), value.clone(), named("Time"));
            }
        }
    }
    branches
}

#[derive(Clone, Default)]
pub(super) struct Constraints {
    substitutions: HashMap<usize, Term>,
    not_lists: Vec<Term>,
    reserved: usize,
}

impl Constraints {
    fn resolve(&self, term: &Term) -> Term {
        match term {
            Term::Var(index) => self
                .substitutions
                .get(index)
                .map_or_else(|| term.clone(), |value| self.resolve(value)),
            Term::Generic(name, inner) => {
                Term::Generic(name.clone(), Box::new(self.resolve(inner)))
            }
            Term::Named(_) => term.clone(),
        }
    }

    fn occurs(&self, index: usize, term: &Term) -> bool {
        match self.resolve(term) {
            Term::Var(other) => index == other,
            Term::Generic(_, inner) => self.occurs(index, &inner),
            Term::Named(_) => false,
        }
    }

    fn unify_inner(&mut self, left: &Term, right: &Term) -> bool {
        let left = self.resolve(left);
        let right = self.resolve(right);
        match (&left, &right) {
            (Term::Var(a), Term::Var(b)) if a == b => true,
            (Term::Var(index), value) | (value, Term::Var(index)) => {
                if self.occurs(*index, value) {
                    return false;
                }
                self.substitutions.insert(*index, value.clone());
                true
            }
            (Term::Named(a), Term::Named(b)) => a == b,
            (Term::Generic(a, inner_a), Term::Generic(b, inner_b)) => {
                a == b && self.unify_inner(inner_a, inner_b)
            }
            _ => false,
        }
    }

    fn with_not_list(mut self, term: &Term) -> Option<Self> {
        match self.resolve(term) {
            Term::Generic(name, _) if name == "List" => None,
            Term::Var(_) => {
                self.not_lists.push(term.clone());
                Some(self)
            }
            _ => Some(self),
        }
    }

    pub(super) fn unify(&mut self, left: &Term, right: &Term) -> bool {
        let mut trial = self.clone();
        if !trial.unify_inner(left, right)
            || trial
                .not_lists
                .iter()
                .any(|term| matches!(trial.resolve(term), Term::Generic(name, _) if name == "List"))
        {
            return false;
        }
        *self = trial;
        true
    }

    pub(super) fn branch(&self, equations: &[(Term, Term)]) -> Option<Self> {
        let mut trial = self.clone();
        for (left, right) in equations {
            if !trial.unify(left, right) {
                return None;
            }
        }
        Some(trial)
    }

    pub(super) fn concrete(&self, term: &Term) -> Option<Type> {
        match self.resolve(term) {
            Term::Named(name) => Some(Type::Named(name)),
            Term::Generic(name, inner) => {
                Some(Type::Generic(name, Box::new(self.concrete(&inner)?)))
            }
            Term::Var(_) => None,
        }
    }

    pub(super) fn unique_type(branches: &[Self], target: &Term) -> Option<Type> {
        let first = branches.first()?.concrete(target)?;
        for branch in &branches[1..] {
            if branch.concrete(target).as_ref() != Some(&first) {
                return None;
            }
        }
        Some(first)
    }
}

#[cfg(test)]
mod tests {
    use super::{Constraints, Term};
    use crate::Type;
    use std::collections::HashMap;

    fn named(name: &str) -> Term {
        Term::Named(name.into())
    }

    fn generic(name: &str, inner: Term) -> Term {
        Term::Generic(name.into(), Box::new(inner))
    }

    fn iteration_binding<'a>(
        expr: &crate::expr::Expr,
        name: &str,
        bindings: &'a HashMap<*const crate::expr::Expr, Term>,
    ) -> &'a Term {
        match expr {
            crate::expr::Expr::Iteration { binding, .. } if binding == name => {
                bindings.get(&std::ptr::from_ref(expr)).unwrap()
            }
            crate::expr::Expr::Iteration { body, .. } => iteration_binding(body, name, bindings),
            crate::expr::Expr::Some {
                bindings: sources, ..
            } => {
                let (_, source) = sources.iter().find(|(binding, _)| binding == name).unwrap();
                bindings.get(&std::ptr::from_ref(source)).unwrap()
            }
            crate::expr::Expr::Call(_, args) if args.len() == 1 => {
                iteration_binding(&args[0], name, bindings)
            }
            _ => panic!("missing iteration binding: {name}"),
        }
    }

    #[test]
    fn typed_knowledge_call_constrains_parameter_from_boolean_result() {
        let signature = vec![(
            vec![Type::Named("Number".into())],
            Type::Named("Bool".into()),
        )];
        let branches = super::call_branches(
            &Constraints::default(),
            &signature,
            &[Term::Var(0)],
            &named("Bool"),
        );
        assert_eq!(
            Constraints::unique_type(&branches, &Term::Var(0)),
            Some(Type::Named("Number".into()))
        );
        assert!(
            super::call_branches(&Constraints::default(), &signature, &[], &named("Bool"),)
                .is_empty()
        );
    }

    #[test]
    fn insertion_scalar_branch_rejects_lists_even_after_later_unification() {
        let nested = generic("List", generic("List", named("Number")));
        for name in ["insertBefore", "insertAfter"] {
            let args = [nested.clone(), named("Number"), Term::Var(0)];
            let branches = super::list_call_branches(&Constraints::default(), name, &args, &nested);
            assert_eq!(
                branches.len(),
                1,
                "{name}: known nested element is list-valued"
            );
            assert_eq!(
                Constraints::unique_type(&branches, &Term::Var(0))
                    .as_ref()
                    .map(ToString::to_string)
                    .as_deref(),
                Some("List<List<Number>>")
            );

            let args = [Term::Var(1), named("Number"), Term::Var(2)];
            let branches =
                super::list_call_branches(&Constraints::default(), name, &args, &Term::Var(3));
            assert_eq!(
                branches.len(),
                2,
                "{name}: unresolved shapes remain alternatives"
            );
            assert_eq!(Constraints::unique_type(&branches, &Term::Var(2)), None);
            assert_eq!(
                branches
                    .iter()
                    .filter(|branch| branch
                        .branch(&[(Term::Var(2), generic("List", named("Number")))])
                        .is_some())
                    .count(),
                1,
                "{name}: later list argument is spread only"
            );
            assert_eq!(
                branches
                    .iter()
                    .filter(|branch| branch.branch(&[(Term::Var(2), named("Number"))]).is_some())
                    .count(),
                1,
                "{name}: scalar Number remains valid"
            );
        }
    }

    #[test]
    fn aggregate_signatures_constrain_symbolic_duration_and_comparable_types() {
        for (name, kind) in [
            ("sum", "DTDuration"),
            ("mean", "YMDuration"),
            ("min", "DateTime"),
            ("max", "String"),
        ] {
            let branches = super::call_branches(
                &Constraints::default(),
                &super::fixed_call_signatures(name).unwrap(),
                &[generic("List", Term::Var(0))],
                &named(kind),
            );
            assert_eq!(
                Constraints::unique_type(&branches, &Term::Var(0)),
                Some(Type::Named(kind.into())),
                "{name}"
            );
        }
        let branches = super::list_call_branches(
            &Constraints::default(),
            "mode",
            &[generic("List", Term::Var(0))],
            &generic("List", named("Bool")),
        );
        assert_eq!(
            Constraints::unique_type(&branches, &Term::Var(0)),
            Some(Type::Named("Bool".into()))
        );
        assert!(
            super::list_call_branches(
                &Constraints::default(),
                "mode",
                &[named("Bool")],
                &generic("List", named("Bool"))
            )
            .is_empty()
        );
    }

    #[test]
    fn numeric_and_string_calls_constrain_arguments_and_reject_wrong_shapes() {
        for (name, arguments, output, inferred) in [
            ("odd", vec![Term::Var(0)], named("Bool"), "Number"),
            (
                "clamp",
                vec![Term::Var(0), named("Number"), named("Number")],
                named("Number"),
                "Number",
            ),
            (
                "substring",
                vec![Term::Var(0), named("Number")],
                named("String"),
                "String",
            ),
            (
                "matches",
                vec![Term::Var(0), named("String")],
                named("Bool"),
                "String",
            ),
        ] {
            let branches = super::call_branches(
                &Constraints::default(),
                &super::fixed_call_signatures(name).unwrap(),
                &arguments,
                &output,
            );
            assert_eq!(
                Constraints::unique_type(&branches, &Term::Var(0)),
                Some(Type::Named(inferred.into())),
                "{name}"
            );
        }
        for (name, arguments, output) in [
            ("odd", vec![], named("Bool")),
            ("odd", vec![named("String")], named("Bool")),
            (
                "sqrt",
                vec![named("Number"), named("Number")],
                named("Number"),
            ),
            ("substring", vec![named("String")], named("String")),
            (
                "number",
                vec![named("String"), named("String")],
                named("Number"),
            ),
            ("upperCase", vec![named("Number")], named("String")),
            (
                "startsWith",
                vec![named("String"), named("String")],
                named("String"),
            ),
        ] {
            assert!(
                super::call_branches(
                    &Constraints::default(),
                    &super::fixed_call_signatures(name).unwrap(),
                    &arguments,
                    &output
                )
                .is_empty(),
                "{name}"
            );
        }
    }

    #[test]
    fn string_and_duration_overloads_keep_distinct_symbolic_alternatives() {
        let base = Constraints::default();
        for (name, arguments, result, target, expected) in [
            (
                "split",
                vec![named("String"), Term::Var(0)],
                generic("List", named("String")),
                Term::Var(0),
                None,
            ),
            (
                "split",
                vec![Term::Var(0), generic("List", named("String"))],
                generic("List", named("String")),
                Term::Var(0),
                Some("String"),
            ),
            (
                "stringJoin",
                vec![Term::Var(0), named("String")],
                named("String"),
                Term::Var(0),
                Some("List<String>"),
            ),
            (
                "indexOf",
                vec![Term::Var(0), named("String")],
                named("Number"),
                Term::Var(0),
                Some("String"),
            ),
            (
                "contains",
                vec![named("String"), Term::Var(0)],
                named("Bool"),
                Term::Var(0),
                Some("String"),
            ),
            (
                "reverse",
                vec![Term::Var(0)],
                named("String"),
                Term::Var(0),
                Some("String"),
            ),
            (
                "isEmpty",
                vec![Term::Var(0)],
                named("Bool"),
                Term::Var(0),
                Some("String"),
            ),
            (
                "string",
                vec![Term::Var(0)],
                named("String"),
                Term::Var(0),
                None,
            ),
            (
                "abs",
                vec![Term::Var(0)],
                named("DTDuration"),
                Term::Var(0),
                Some("DTDuration"),
            ),
            (
                "isNegative",
                vec![Term::Var(0)],
                named("Bool"),
                Term::Var(0),
                None,
            ),
            (
                "roundHalfEven",
                vec![Term::Var(0), named("YMDuration")],
                named("YMDuration"),
                Term::Var(0),
                Some("YMDuration"),
            ),
        ] {
            let branches = super::call_branches(
                &base,
                &super::fixed_call_signatures(name).unwrap(),
                &arguments,
                &result,
            );
            assert!(!branches.is_empty(), "{name}");
            assert_eq!(
                Constraints::unique_type(&branches, &target)
                    .as_ref()
                    .map(ToString::to_string)
                    .as_deref(),
                expected,
                "{name}"
            );
        }
        let branches = super::call_branches(
            &base,
            &super::fixed_call_signatures("string").unwrap(),
            &[Term::Var(0)],
            &named("String"),
        );
        assert_eq!(branches.len(), 6);
        for (name, args, result) in [
            (
                "string",
                vec![generic("List", named("Number"))],
                named("String"),
            ),
            (
                "split",
                vec![named("String"), named("Number")],
                generic("List", named("String")),
            ),
            (
                "split",
                vec![named("String")],
                generic("List", named("String")),
            ),
            ("reverse", vec![named("Number")], named("String")),
            ("indexOf", vec![named("String")], named("Number")),
            (
                "round",
                vec![named("DTDuration"), named("YMDuration")],
                named("DTDuration"),
            ),
            (
                "abs",
                vec![named("DTDuration"), named("DTDuration")],
                named("DTDuration"),
            ),
        ] {
            assert!(
                super::call_branches(
                    &base,
                    &super::fixed_call_signatures(name).unwrap(),
                    &args,
                    &result
                )
                .is_empty(),
                "{name}"
            );
        }
    }

    #[test]
    fn temporal_constructors_constrain_symbolic_arguments_and_preserve_overloads() {
        let base = Constraints::default();
        for (name, args, result, expected) in [
            (
                "date",
                vec![Term::Var(0), named("Number"), named("Number")],
                named("Date"),
                Some("Number"),
            ),
            (
                "time",
                vec![
                    Term::Var(0),
                    named("Number"),
                    named("Number"),
                    named("DTDuration"),
                ],
                named("Time"),
                Some("Number"),
            ),
            (
                "datetime",
                vec![Term::Var(0), named("Time")],
                named("DateTime"),
                Some("Date"),
            ),
            (
                "dtDuration",
                vec![Term::Var(0)],
                named("DTDuration"),
                Some("String"),
            ),
            ("today", vec![], named("Date"), None),
        ] {
            let branches = super::call_branches(
                &base,
                &super::fixed_call_signatures(name).unwrap(),
                &args,
                &result,
            );
            assert!(!branches.is_empty(), "{name}");
            if let Some(expected) = expected {
                assert_eq!(
                    Constraints::unique_type(&branches, &Term::Var(0)),
                    Some(Type::Named(expected.into())),
                    "{name}"
                );
            }
        }
        let branches = super::call_branches(
            &base,
            &super::fixed_call_signatures("date").unwrap(),
            &[Term::Var(0)],
            &named("Date"),
        );
        assert_eq!(Constraints::unique_type(&branches, &Term::Var(0)), None); // String or DateTime
        for (name, args, output) in [
            (
                "datetime",
                vec![named("Date"), named("DateTime")],
                named("DateTime"),
            ),
            (
                "date",
                vec![named("Number"), named("Number")],
                named("Date"),
            ),
            ("ymDuration", vec![named("String")], named("DTDuration")),
        ] {
            assert!(
                super::call_branches(
                    &base,
                    &super::fixed_call_signatures(name).unwrap(),
                    &args,
                    &output
                )
                .is_empty(),
                "{name}"
            );
        }
    }

    #[test]
    fn temporal_point_duration_and_calendar_options_constrain_symbols() {
        let base = Constraints::default();
        for (name, args, result, inferred) in [
            (
                "dtDurationBetween",
                vec![Term::Var(0), named("DateTime")],
                named("DTDuration"),
                "DateTime",
            ),
            (
                "withOffset",
                vec![Term::Var(0), named("DTDuration")],
                named("Time"),
                "Time",
            ),
            ("withoutTimezone", vec![Term::Var(0)], named("Date"), "Date"),
            (
                "daysBetween",
                vec![Term::Var(0), named("DateTime"), named("Bool")],
                named("Number"),
                "DateTime",
            ),
            (
                "monthsBetween",
                vec![
                    Term::Var(0),
                    named("DateTime"),
                    named("String"),
                    named("Bool"),
                ],
                named("Number"),
                "DateTime",
            ),
            (
                "financialYear",
                vec![Term::Var(0), named("Number")],
                named("String"),
                "Date",
            ),
        ] {
            let branches = super::call_branches(
                &base,
                &super::fixed_call_signatures(name).unwrap(),
                &args,
                &result,
            );
            assert!(!branches.is_empty(), "{name}");
            if name != "financialYear" {
                assert_eq!(
                    Constraints::unique_type(&branches, &Term::Var(0)),
                    Some(Type::Named(inferred.into())),
                    "{name}"
                );
            } else {
                assert_eq!(Constraints::unique_type(&branches, &Term::Var(0)), None); // Date or DateTime
            }
        }
        for (name, args, output) in [
            (
                "dtDurationBetween",
                vec![named("Date"), named("DateTime")],
                named("DTDuration"),
            ),
            (
                "withOffset",
                vec![named("Date"), named("DTDuration")],
                named("Date"),
            ),
            (
                "daysBetween",
                vec![named("Date"), named("Date"), named("Bool")],
                named("Number"),
            ),
            (
                "yearsBetween",
                vec![named("Date"), named("Date"), named("Bool")],
                named("Number"),
            ),
            (
                "financialYear",
                vec![named("Date"), named("Bool")],
                named("String"),
            ),
        ] {
            assert!(
                super::call_branches(
                    &base,
                    &super::fixed_call_signatures(name).unwrap(),
                    &args,
                    &output
                )
                .is_empty(),
                "{name}"
            );
        }
    }

    #[test]
    fn calendar_date_functions_constrain_point_and_optional_arguments() {
        let base = Constraints::default();
        for (name, args, result, inferred) in [
            (
                "isPublicHoliday",
                vec![Term::Var(0), named("Calendar")],
                named("Bool"),
                "Date",
            ),
            (
                "nthDayOfWeekInMonth",
                vec![Term::Var(0), named("Number"), named("Number")],
                named("DateTime"),
                "DateTime",
            ),
            (
                "businessDaysBetween",
                vec![
                    Term::Var(0),
                    named("Date"),
                    named("Calendar"),
                    named("Bool"),
                ],
                named("Number"),
                "Date",
            ),
            (
                "nextBusinessDay",
                vec![Term::Var(0), named("Calendar"), named("Bool")],
                named("Date"),
                "Date",
            ),
        ] {
            let branches = super::call_branches(
                &base,
                &super::fixed_call_signatures(name).unwrap(),
                &args,
                &result,
            );
            assert!(!branches.is_empty(), "{name}");
            if name != "isPublicHoliday" {
                assert_eq!(
                    Constraints::unique_type(&branches, &Term::Var(0)),
                    Some(Type::Named(inferred.into())),
                    "{name}"
                );
            } else {
                assert_eq!(Constraints::unique_type(&branches, &Term::Var(0)), None); // Date or DateTime
            }
        }
        for (name, args, result) in [
            ("isPublicHoliday", vec![named("Date")], named("Bool")),
            (
                "nextWeekday",
                vec![named("Date"), named("Number")],
                named("Date"),
            ),
            (
                "weekdaysBetween",
                vec![named("Date"), named("DateTime")],
                named("Number"),
            ),
            (
                "nextBusinessDay",
                vec![named("Date"), named("Bool")],
                named("Date"),
            ),
        ] {
            assert!(
                super::call_branches(
                    &base,
                    &super::fixed_call_signatures(name).unwrap(),
                    &args,
                    &result
                )
                .is_empty(),
                "{name}"
            );
        }
    }

    #[test]
    fn calendar_queries_and_mutations_preserve_overload_alternatives() {
        let base = Constraints::default();
        for (name, args, result, inferred) in [
            (
                "entryValue",
                vec![Term::Var(0)],
                named("CalendarValue"),
                "CalendarEntry",
            ),
            (
                "find",
                vec![named("Calendar"), Term::Var(0)],
                generic("List", named("CalendarEntry")),
                "String",
            ),
            (
                "next",
                vec![named("Calendar"), Term::Var(0), named("Number")],
                named("CalendarEntry"),
                "DateTime",
            ),
            (
                "calendarMerge",
                vec![Term::Var(0)],
                named("Calendar"),
                "List<Calendar>",
            ),
            (
                "calendarDrop",
                vec![named("Calendar"), Term::Var(0), named("String")],
                named("Calendar"),
                "Date",
            ),
        ] {
            let branches = super::call_branches(
                &base,
                &super::calendar_call_signatures(name).unwrap(),
                &args,
                &result,
            );
            assert!(!branches.is_empty(), "{name}");
            if matches!(name, "next" | "calendarDrop") {
                assert_eq!(
                    Constraints::unique_type(&branches, &Term::Var(0)),
                    None,
                    "{name}"
                );
            } else {
                let ty = if inferred == "List<Calendar>" {
                    Type::Generic("List".into(), Box::new(Type::Named("Calendar".into())))
                } else {
                    Type::Named(inferred.into())
                };
                assert_eq!(
                    Constraints::unique_type(&branches, &Term::Var(0)),
                    Some(ty),
                    "{name}"
                );
            }
        }
        let merge_options = super::call_branches(
            &base,
            &super::calendar_call_signatures("calendarMerge").unwrap(),
            &[Term::Var(0), Term::Var(1), named("String")],
            &named("Calendar"),
        );
        assert_eq!(
            Constraints::unique_type(&merge_options, &Term::Var(0)),
            Some(Type::Generic(
                "List".into(),
                Box::new(Type::Named("Calendar".into()))
            ))
        );
        assert_eq!(
            Constraints::unique_type(&merge_options, &Term::Var(1)),
            Some(Type::Named("String".into()))
        );
        for (name, args, output) in [
            ("entryName", vec![named("String")], named("String")),
            (
                "next",
                vec![named("Calendar"), named("String")],
                named("CalendarEntry"),
            ),
            (
                "calendarDrop",
                vec![named("Calendar"), named("Number")],
                named("Calendar"),
            ),
            (
                "calendarDrop",
                vec![named("Calendar"), named("Date"), named("Bool")],
                named("Calendar"),
            ),
        ] {
            assert!(
                super::call_branches(
                    &base,
                    &super::calendar_call_signatures(name).unwrap(),
                    &args,
                    &output
                )
                .is_empty(),
                "{name}"
            );
        }
    }

    #[test]
    fn calendar_target_lists_are_only_valid_as_literal_syntax() {
        let base = Constraints::default();
        for ty in ["Date", "Number"] {
            assert!(
                super::call_branches(
                    &base,
                    &super::calendar_call_signatures("calendarKeep").unwrap(),
                    &[named("Calendar"), generic("List", named(ty))],
                    &named("Calendar")
                )
                .is_empty(),
                "{ty}"
            );
        }
        let branches = super::call_branches(
            &base,
            &super::calendar_call_signatures("calendarKeep").unwrap(),
            &[named("Calendar"), Term::Var(0)],
            &named("Calendar"),
        );
        assert_eq!(Constraints::unique_type(&branches, &Term::Var(0)), None);
    }

    #[test]
    fn operator_composition_pushes_boolean_result_into_nested_numeric_operands() {
        let base = Constraints::default();
        let product = super::binary_branches(
            &base,
            "*",
            false,
            &Term::Var(0),
            &named("Number"),
            &Term::Var(1),
        );
        let comparison: Vec<_> = product
            .iter()
            .flat_map(|branch| {
                super::binary_branches(
                    branch,
                    ">",
                    false,
                    &Term::Var(1),
                    &named("Number"),
                    &named("Bool"),
                )
            })
            .collect();
        assert_eq!(
            Constraints::unique_type(&comparison, &Term::Var(0)),
            Some(Type::Named("Number".into()))
        );
    }

    #[test]
    fn zero_minus_duration_retains_duration_alternative() {
        let base = Constraints::default()
            .branch(&[(
                generic("List", Term::Var(1)),
                generic("List", named("DTDuration")),
            )])
            .unwrap();
        let branches = super::binary_branches(
            &base,
            "-",
            true,
            &named("Number"),
            &Term::Var(0),
            &Term::Var(1),
        );
        assert_eq!(
            Constraints::unique_type(&branches, &Term::Var(0)),
            Some(Type::Named("DTDuration".into()))
        );
    }

    #[test]
    fn equality_of_unbound_names_does_not_invent_a_concrete_type() {
        let branches = super::binary_branches(
            &Constraints::default(),
            "==",
            false,
            &Term::Var(0),
            &Term::Var(0),
            &named("Bool"),
        );
        assert!(!branches.is_empty());
        assert_eq!(Constraints::unique_type(&branches, &Term::Var(0)), None);
    }

    #[test]
    fn operator_overloads_cover_lists_ranges_booleans_and_temporal_values() {
        let base = Constraints::default();
        for (op, left, right, result) in [
            ("and", named("Bool"), named("Bool"), named("Bool")),
            (
                "==",
                generic("List", named("Number")),
                generic("List", named("Number")),
                named("Bool"),
            ),
            (
                "in",
                named("Number"),
                generic("List", named("Number")),
                named("Bool"),
            ),
            (
                "in",
                named("Number"),
                generic("Range", named("Number")),
                named("Bool"),
            ),
            ("in", named("Date"), named("Calendar"), named("Bool")),
            ("+", named("Date"), named("YMDuration"), named("Date")),
            (
                "-",
                named("DateTime"),
                named("DateTime"),
                named("DTDuration"),
            ),
            (
                "*",
                named("DTDuration"),
                named("Number"),
                named("DTDuration"),
            ),
            (
                "*",
                named("Number"),
                named("YMDuration"),
                named("YMDuration"),
            ),
        ] {
            assert!(
                !super::binary_branches(&base, op, false, &left, &right, &result).is_empty(),
                "{op}: {left:?} {right:?}"
            );
        }
        assert!(
            super::binary_branches(
                &base,
                "+",
                false,
                &generic("List", named("Number")),
                &generic("List", named("Number")),
                &generic("List", named("Number"))
            )
            .is_empty()
        );
        assert!(
            super::binary_branches(
                &base,
                "-",
                false,
                &named("Number"),
                &named("DTDuration"),
                &named("DTDuration")
            )
            .is_empty()
        );
        assert!(!super::not_branches(&base, &Term::Var(0), &named("Bool")).is_empty());
        assert!(super::not_branches(&base, &named("Number"), &named("Bool")).is_empty());
    }

    #[test]
    fn dictionary_calls_preserve_open_generic_shapes() {
        let base = Constraints::default();
        let entries = super::dictionary_call_branches(
            &base,
            "getEntries",
            &[Term::Var(0)],
            &generic("List", generic("DictionaryEntry", named("Number"))),
            &[],
            None,
        );
        assert_eq!(
            Constraints::unique_type(&entries, &Term::Var(0)),
            Some(Type::Generic(
                "Dictionary".into(),
                Box::new(Type::Named("Number".into()))
            ))
        );
        let keys = super::dictionary_call_branches(
            &base,
            "keys",
            &[Term::Var(0)],
            &generic("List", named("String")),
            &[],
            None,
        );
        assert_eq!(Constraints::unique_type(&keys, &Term::Var(0)), None);
        assert!(
            !super::dictionary_call_branches(
                &base,
                "getValue",
                &[named("Dictionary"), named("String")],
                &named("Number"),
                &[],
                None,
            )
            .is_empty()
        );
        assert!(
            super::dictionary_call_branches(
                &base,
                "has",
                &[generic("Dictionary", named("Number")), named("Number")],
                &named("Bool"),
                &[],
                None,
            )
            .is_empty()
        );
    }

    #[test]
    fn nested_dictionary_get_value_path_uses_final_element_type() {
        let base = Constraints::default();
        let arguments = [
            generic("Dictionary", generic("Dictionary", named("Number"))),
            generic("List", named("String")),
        ];
        assert!(
            !super::dictionary_call_branches(
                &base,
                "getValue",
                &arguments,
                &named("Number"),
                &[],
                Some((2, Some("a")))
            )
            .is_empty()
        );
        assert!(
            super::dictionary_call_branches(
                &base,
                "getValue",
                &arguments,
                &named("Bool"),
                &[],
                Some((2, Some("a")))
            )
            .is_empty()
        );
    }

    #[test]
    fn named_record_get_value_literal_key_rejects_wrong_result() {
        let base = Constraints::default();
        let records = [crate::Record {
            name: "Order".into(),
            fields: vec![("total".into(), Type::Named("Number".into()))],
        }];
        let arguments = [named("Order"), named("String")];
        assert!(
            super::dictionary_call_branches(
                &base,
                "getValue",
                &arguments,
                &named("Bool"),
                &records,
                Some((1, Some("total")))
            )
            .is_empty()
        );
        assert!(
            !super::dictionary_call_branches(
                &base,
                "getValue",
                &arguments,
                &named("Number"),
                &records,
                Some((1, Some("total")))
            )
            .is_empty()
        );
    }

    #[test]
    fn scalar_date_field_does_not_enable_list_projection() {
        let base = Constraints::default();
        assert!(
            !super::field_branches(&base, "year", &named("Date"), &named("Number"), &[]).is_empty()
        );
        assert!(
            super::field_branches(
                &base,
                "year",
                &generic("List", named("Date")),
                &generic("List", named("Number")),
                &[]
            )
            .is_empty()
        );
    }

    #[test]
    fn bracket_dispatch_keeps_string_number_and_boolean_distinct() {
        let base = Constraints::default();
        for (key, result) in [
            (named("Number"), named("Number")),
            (named("Bool"), generic("List", named("Number"))),
        ] {
            assert!(
                !super::index_branches(
                    &base,
                    &generic("List", named("Number")),
                    &key,
                    &result,
                    &[],
                    None
                )
                .is_empty()
            );
        }
        assert!(
            super::index_branches(
                &base,
                &generic("List", named("Number")),
                &named("String"),
                &named("Number"),
                &[],
                None
            )
            .is_empty()
        );
        assert!(
            super::index_branches(
                &base,
                &generic("Dictionary", named("Number")),
                &named("String"),
                &named("Number"),
                &[],
                None
            )
            .len()
                == 1
        );
        let records = [crate::Record {
            name: "Order".into(),
            fields: vec![("active".into(), Type::Named("Bool".into()))],
        }];
        assert!(
            super::index_branches(
                &base,
                &named("Order"),
                &named("String"),
                &named("Bool"),
                &records,
                Some("active")
            )
            .len()
                == 1
        );
        assert!(
            super::index_branches(
                &base,
                &named("Order"),
                &named("String"),
                &named("Bool"),
                &records,
                Some("missing")
            )
            .is_empty()
        );
    }

    #[test]
    fn record_field_has_open_dictionary_alternative() {
        let base = Constraints::default();
        let records = [crate::Record {
            name: "Order".into(),
            fields: vec![("active".into(), Type::Named("Bool".into()))],
        }];
        let fields =
            super::field_branches(&base, "active", &Term::Var(0), &named("Bool"), &records);
        assert_eq!(Constraints::unique_type(&fields, &Term::Var(0)), None);
        assert!(
            fields
                .iter()
                .any(|branch| branch.concrete(&Term::Var(0)) == Some(Type::Named("Order".into())))
        );
        assert!(fields.iter().any(|branch| branch.concrete(&Term::Var(0))
            == Some(Type::Generic(
                "Dictionary".into(),
                Box::new(Type::Named("Bool".into()))
            ))));
        let projection = super::field_branches(
            &base,
            "active",
            &Term::Var(0),
            &generic("List", named("Bool")),
            &records,
        );
        assert!(
            projection
                .iter()
                .any(|branch| branch.concrete(&Term::Var(0))
                    == Some(Type::Generic(
                        "List".into(),
                        Box::new(Type::Named("Order".into()))
                    )))
        );
        assert!(
            super::field_branches(&base, "missing", &named("Order"), &named("Bool"), &records)
                .is_empty()
        );
        assert!(
            !super::field_branches(
                &base,
                "active",
                &named("Dictionary"),
                &named("Bool"),
                &records
            )
            .is_empty()
        );
        assert!(
            !super::field_branches(&base, "active", &named("Value"), &named("Bool"), &records)
                .is_empty()
        );
    }

    #[test]
    fn list_and_dictionary_literal_shapes_constrain_nested_elements() {
        let base = Constraints::default();
        let items = super::list_branches(
            &base,
            &[Term::Var(0), named("Number")],
            &generic("List", named("Number")),
        );
        assert_eq!(
            Constraints::unique_type(&items, &Term::Var(0)),
            Some(Type::Named("Number".into()))
        );
        assert!(
            super::list_branches(&base, &[named("String")], &generic("List", named("Number")))
                .is_empty()
        );
        assert_eq!(
            Constraints::unique_type(
                &super::list_branches(&base, &[], &Term::Var(0)),
                &Term::Var(0)
            ),
            None
        );
        let values = super::dictionary_literal_branches(
            &base,
            &[("a".into(), Term::Var(0)), ("b".into(), named("Number"))],
            &generic("Dictionary", named("Number")),
            &[],
        );
        assert_eq!(
            Constraints::unique_type(&values, &Term::Var(0)),
            Some(Type::Named("Number".into()))
        );
        assert!(
            super::dictionary_literal_branches(
                &base,
                &[("a".into(), named("Bool"))],
                &generic("Dictionary", named("Number")),
                &[]
            )
            .is_empty()
        );
    }

    #[test]
    fn scalar_fields_keep_temporal_and_entry_alternatives() {
        let base = Constraints::default();
        assert_eq!(
            Constraints::unique_type(
                &super::field_branches(&base, "year", &Term::Var(0), &named("Number"), &[]),
                &Term::Var(0),
            ),
            None
        ); // Date, DateTime, or Dictionary<Number>
        assert!(
            super::field_branches(
                &base,
                "totalSeconds",
                &named("DTDuration"),
                &named("Number"),
                &[]
            )
            .len()
                == 1
        );
        assert!(
            super::field_branches(&base, "year", &named("Time"), &named("Number"), &[]).is_empty()
        );
        let entry = super::field_branches(&base, "value", &Term::Var(0), &named("Number"), &[]);
        assert!(entry.iter().any(|branch| branch.concrete(&Term::Var(0))
            == Some(Type::Generic(
                "DictionaryEntry".into(),
                Box::new(Type::Named("Number".into()))
            ))));
    }

    #[test]
    fn nested_list_dictionary_unification_resolves_inner_variable() {
        let mut constraints = Constraints::default();
        let nested = generic("List", generic("Dictionary", Term::Var(0)));
        let concrete = generic("List", generic("Dictionary", named("Number")));
        assert!(constraints.unify(&nested, &concrete));
        assert_eq!(
            constraints.concrete(&nested),
            Some(Type::Generic(
                "List".into(),
                Box::new(Type::Generic(
                    "Dictionary".into(),
                    Box::new(Type::Named("Number".into()))
                ))
            ))
        );
    }

    #[test]
    fn incompatible_equation_preserves_prior_substitutions() {
        let mut constraints = Constraints::default();
        assert!(constraints.unify(&Term::Var(0), &named("Number")));
        assert!(constraints.unify(&Term::Var(1), &named("Number")));
        assert!(!constraints.unify(
            &generic("List", Term::Var(1)),
            &generic("List", named("String"))
        ));
        assert!(!constraints.unify(
            &generic("List", named("Number")),
            &generic("Dictionary", named("Number"))
        ));
        assert!(!constraints.unify(&Term::Var(0), &named("String")));
        assert_eq!(
            constraints.concrete(&Term::Var(0)),
            Some(Type::Named("Number".into()))
        );
        assert_eq!(
            constraints.concrete(&Term::Var(1)),
            Some(Type::Named("Number".into()))
        );
    }

    #[test]
    fn recursive_equation_is_rejected() {
        let mut constraints = Constraints::default();
        assert!(!constraints.unify(&Term::Var(0), &generic("List", Term::Var(0))));
        assert!(constraints.concrete(&Term::Var(0)).is_none());
        assert!(constraints.unify(&Term::Var(0), &Term::Var(1)));
        assert!(!constraints.unify(&Term::Var(1), &generic("Dictionary", Term::Var(0))));
    }

    #[test]
    fn alternatives_keep_independent_solutions_and_detect_distinct_types() {
        let base = Constraints::default();
        let number = base.branch(&[(Term::Var(0), named("Number"))]).unwrap();
        let same = base.branch(&[(Term::Var(0), named("Number"))]).unwrap();
        let string = base.branch(&[(Term::Var(0), named("String"))]).unwrap();
        assert_eq!(
            Constraints::unique_type(&[number.clone(), same], &Term::Var(0)),
            Some(Type::Named("Number".into()))
        );
        assert_eq!(
            Constraints::unique_type(&[number, string], &Term::Var(0)),
            None
        );
        assert!(base.concrete(&Term::Var(0)).is_none());
    }

    #[test]
    fn failed_alternative_does_not_discard_valid_branch() {
        let mut base = Constraints::default();
        assert!(base.unify(&Term::Var(0), &named("Number")));
        assert!(
            base.branch(&[
                (Term::Var(1), named("String")),
                (Term::Var(0), named("String")),
            ])
            .is_none()
        );
        assert!(base.concrete(&Term::Var(1)).is_none());
        let valid = base
            .branch(&[(Term::Var(1), generic("List", Term::Var(0)))])
            .unwrap();
        assert_eq!(
            Constraints::unique_type(&[valid], &Term::Var(1)),
            Some(Type::Generic(
                "List".into(),
                Box::new(Type::Named("Number".into()))
            ))
        );
    }

    #[test]
    fn expression_relation_constrains_literals_ranges_and_rejects_mixed_lists() {
        let env = HashMap::from([("x".into(), Term::Var(0))]);
        for (expression, result, target, expected) in [
            (
                "[x, 2]",
                generic("List", named("Number")),
                Term::Var(0),
                Some("Number"),
            ),
            (
                "[x..3]",
                generic("Range", named("Number")),
                Term::Var(0),
                Some("Number"),
            ),
            (
                "[date(\"2026-01-01\")..date(\"2026-01-02\")]",
                generic("Range", named("Date")),
                Term::Var(0),
                None,
            ),
            (
                "[x, true]",
                generic("List", named("Number")),
                Term::Var(0),
                None,
            ),
            (
                "[1, null]",
                generic("List", named("Number")),
                Term::Var(0),
                None,
            ),
        ] {
            let ast = crate::expr::expression(expression).unwrap();
            let branches = super::expression_branches(&ast, &result, &env, &[], &[], &[]).unwrap();
            assert_eq!(
                Constraints::unique_type(&branches, &target)
                    .as_ref()
                    .map(ToString::to_string)
                    .as_deref(),
                expected,
                "{expression}"
            );
        }
    }

    #[test]
    fn expression_relation_orders_later_sources_and_shadows_nested_bindings() {
        let env = HashMap::from([
            ("x".into(), named("String")),
            ("later".into(), Term::Var(0)),
        ]);
        for (expression, expected) in [
            ("for x in [1,2], y in [x, x + 1] return y", "List<Number>"),
            (
                "for x in [1] return (for x in [true] return x)",
                "List<List<Bool>>",
            ),
        ] {
            let ast = crate::expr::expression(expression).unwrap();
            let result = Term::Var(1);
            let branches = super::expression_branches(&ast, &result, &env, &[], &[], &[]).unwrap();
            assert_eq!(
                Constraints::unique_type(&branches, &result)
                    .unwrap()
                    .to_string(),
                expected
            );
        }
        for invalid in [
            "for x in [y], y in [1] return x",
            "for x in [1], y in [later] return y",
        ] {
            let ast = crate::expr::expression(invalid).unwrap();
            let branches = super::expression_branches(
                &ast,
                &generic("List", named("Number")),
                &env,
                &[],
                &[],
                &[],
            )
            .unwrap();
            if invalid.contains("[y]") {
                assert!(branches.is_empty(), "{invalid}");
            } else {
                assert_eq!(
                    Constraints::unique_type(&branches, &Term::Var(0)),
                    Some(Type::Named("Number".into()))
                );
            }
        }
    }

    #[test]
    fn expression_relation_constrains_empty_sources_quantifiers_and_ambiguity() {
        let env = HashMap::new();
        for (expression, result, binding, expected) in [
            (
                "for x in [1], y in [] return y",
                generic("List", named("Number")),
                "y",
                Some("Number"),
            ),
            (
                "every x in [] satisfies x > 0",
                named("Bool"),
                "x",
                Some("Number"),
            ),
            (
                "some x in [] satisfies odd(x)",
                named("Bool"),
                "x",
                Some("Number"),
            ),
            (
                "for x in [] return x == x",
                generic("List", named("Bool")),
                "x",
                None,
            ),
        ] {
            let ast = crate::expr::expression(expression).unwrap();
            let mut bindings = HashMap::new();
            let branches = super::expression_branches_with_bindings(
                &ast,
                &result,
                &env,
                &[],
                &[],
                &[],
                &mut bindings,
            )
            .unwrap();
            assert!(!branches.is_empty(), "{expression}");
            assert_eq!(
                Constraints::unique_type(&branches, iteration_binding(&ast, binding, &bindings))
                    .as_ref()
                    .map(ToString::to_string)
                    .as_deref(),
                expected,
                "{expression}"
            );
        }
    }

    #[test]
    fn expression_relation_reuses_dictionary_literal_and_index_constraints_in_bindings() {
        let env = HashMap::new();
        for (expression, result, binding, expected) in [
            (
                "for x in [] return values({n: x})",
                generic("List", generic("List", named("Number"))),
                "x",
                "Number",
            ),
            (
                "for x in [] return [1,2][x]",
                generic("List", named("Number")),
                "x",
                "Number",
            ),
            (
                "for x in [] return [1,2][.item == x]",
                generic("List", generic("List", named("Number"))),
                "x",
                "Number",
            ),
        ] {
            let ast = crate::expr::expression(expression).unwrap();
            let mut bindings = HashMap::new();
            let branches = super::expression_branches_with_bindings(
                &ast,
                &result,
                &env,
                &[],
                &[],
                &[],
                &mut bindings,
            )
            .unwrap();
            assert_eq!(
                Constraints::unique_type(&branches, iteration_binding(&ast, binding, &bindings))
                    .unwrap_or_else(|| panic!("not uniquely constrained: {expression}"))
                    .to_string(),
                expected,
                "{expression}"
            );
        }
        let ast = crate::expr::expression("for x in [] return {n: x}.n").unwrap();
        let mut bindings = HashMap::new();
        let branches = super::expression_branches_with_bindings(
            &ast,
            &generic("List", named("Number")),
            &env,
            &[],
            &[],
            &[],
            &mut bindings,
        )
        .unwrap();
        assert!(!branches.is_empty());
        assert_eq!(
            Constraints::unique_type(&branches, iteration_binding(&ast, "x", &bindings)),
            None
        );
    }

    #[test]
    fn expression_relation_reuses_calendar_and_dictionary_calls_across_sources() {
        let env = HashMap::new();
        for (expression, result, expected) in [
            (
                "for x in [] return entryName(x)",
                generic("List", named("String")),
                "CalendarEntry",
            ),
            (
                "for x in [] return count(x)",
                generic("List", named("Number")),
                "Calendar",
            ),
            (
                "for x in [] return size(x)",
                generic("List", named("Number")),
                "Dictionary<Number>",
            ),
        ] {
            let ast = crate::expr::expression(expression).unwrap();
            let mut bindings = HashMap::new();
            let branches = super::expression_branches_with_bindings(
                &ast,
                &result,
                &env,
                &[],
                &[],
                &[],
                &mut bindings,
            )
            .unwrap();
            assert!(!branches.is_empty(), "{expression}");
            if expression.contains("size") || expression.contains("count") {
                assert_eq!(
                    Constraints::unique_type(&branches, iteration_binding(&ast, "x", &bindings)),
                    None
                );
            } else {
                assert_eq!(
                    Constraints::unique_type(&branches, iteration_binding(&ast, "x", &bindings))
                        .unwrap()
                        .to_string(),
                    expected,
                    "{expression}"
                );
            }
        }
    }

    #[test]
    fn list_inspection_constrains_empty_iteration_bindings_without_choosing_a_collection_shape() {
        let env = HashMap::new();
        for (expression, result, expected) in [
            (
                "for x in [] return listContains(x, 2)",
                generic("List", named("Bool")),
                Some("List<Number>"),
            ),
            (
                "for x in [] return indexOf([1,2], x)",
                generic("List", generic("List", named("Number"))),
                Some("Number"),
            ),
            (
                "for x in [] return sublist(x, 1)",
                generic("List", generic("List", named("Number"))),
                Some("List<Number>"),
            ),
            (
                "for x in [] return isEmpty(x)",
                generic("List", named("Bool")),
                None,
            ),
            (
                "for x in [] return count(x)",
                generic("List", named("Number")),
                None,
            ),
        ] {
            let ast = crate::expr::expression(expression).unwrap();
            let mut bindings = HashMap::new();
            let branches = super::expression_branches_with_bindings(
                &ast,
                &result,
                &env,
                &[],
                &[],
                &[],
                &mut bindings,
            )
            .unwrap();
            assert!(!branches.is_empty(), "{expression}");
            assert_eq!(
                Constraints::unique_type(&branches, iteration_binding(&ast, "x", &bindings))
                    .map(|ty| ty.to_string())
                    .as_deref(),
                expected,
                "{expression}"
            );
        }
    }

    #[test]
    fn expression_relation_enum_variants_respect_binding_shadowing() {
        let enums = [crate::compiler::Enum {
            name: "Status".into(),
            variants: vec!["ready".into()],
        }];
        let env = HashMap::new();
        for (expression, result, valid) in [
            (
                "for x in [Status.ready] return x",
                generic("List", named("Status")),
                true,
            ),
            (
                "for x in [Status.missing] return x",
                generic("List", named("Status")),
                false,
            ),
            (
                "for Status in [{ready: true}] return Status.ready",
                generic("List", named("Bool")),
                true,
            ),
            (
                "for Status in [1] return Status.ready",
                generic("List", named("Bool")),
                false,
            ),
        ] {
            let ast = crate::expr::expression(expression).unwrap();
            let branches =
                super::expression_branches(&ast, &result, &env, &[], &[], &enums).unwrap();
            assert_eq!(!branches.is_empty(), valid, "{expression}");
        }
    }

    #[test]
    fn expression_relation_filter_discards_invalid_index_alternative() {
        let ast = crate::expr::expression("[1,2][.item > 0]").unwrap();
        let branches =
            super::expression_branches(&ast, &Term::Var(0), &HashMap::new(), &[], &[], &[])
                .expect("filter traversal is supported");
        assert_eq!(
            Constraints::unique_type(&branches, &Term::Var(0)),
            Some(Type::Generic(
                "List".into(),
                Box::new(Type::Named("Number".into()))
            ))
        );
    }

    #[test]
    fn expression_relation_knowledge_count_shadows_calendar_count() {
        let ast = crate::expr::expression("for x in [] return count(x)").unwrap();
        let knowledge = [crate::decision::Knowledge {
            name: "count".into(),
            params: vec![("n".into(), Type::Named("Number".into()))],
            output_name: "result".into(),
            output: Type::Named("Number".into()),
            body: crate::expr::Expr::Name("n".into()),
            braced: false,
        }];
        let mut bindings = HashMap::new();
        let branches = super::expression_branches_with_bindings(
            &ast,
            &generic("List", named("Number")),
            &HashMap::new(),
            &[],
            &knowledge,
            &[],
            &mut bindings,
        )
        .unwrap();
        assert_eq!(
            Constraints::unique_type(&branches, iteration_binding(&ast, "x", &bindings)),
            Some(Type::Named("Number".into()))
        );
    }

    #[test]
    fn expression_relation_nested_binding_names_retain_both_types() {
        let ast =
            crate::expr::expression("for x in [] return [x > 0, (for x in [true] return x)[1]]")
                .unwrap();
        let mut bindings = HashMap::new();
        let branches = super::expression_branches_with_bindings(
            &ast,
            &generic("List", generic("List", named("Bool"))),
            &HashMap::new(),
            &[],
            &[],
            &[],
            &mut bindings,
        )
        .unwrap();
        let crate::expr::Expr::Iteration { body, .. } = &ast else {
            panic!("expected outer iteration")
        };
        let crate::expr::Expr::List(items) = body.as_ref() else {
            panic!("expected list body")
        };
        let crate::expr::Expr::Index(inner, _) = &items[1] else {
            panic!("expected indexed inner iteration")
        };
        assert_eq!(bindings.len(), 2);
        assert_eq!(
            Constraints::unique_type(&branches, bindings.get(&std::ptr::from_ref(&ast)).unwrap()),
            Some(Type::Named("Number".into()))
        );
        assert_eq!(
            Constraints::unique_type(
                &branches,
                bindings.get(&std::ptr::from_ref(inner.as_ref())).unwrap()
            ),
            Some(Type::Named("Bool".into()))
        );
    }

    #[test]
    fn expression_relation_fresh_collection_variables_do_not_capture_outer_symbols() {
        let env = HashMap::from([("unrelated".into(), Term::Var(0))]);
        let ast = crate::expr::expression("for x in [1] return values({n: x})").unwrap();
        let branches = super::expression_branches(
            &ast,
            &generic("List", generic("List", named("Number"))),
            &env,
            &[],
            &[],
            &[],
        )
        .unwrap();
        assert!(!branches.is_empty());
        assert_eq!(Constraints::unique_type(&branches, &Term::Var(0)), None);
    }

    #[test]
    fn unbound_generic_families_are_not_unique_concrete_types() {
        let base = Constraints::default();
        let list = base
            .branch(&[(Term::Var(0), generic("List", Term::Var(1)))])
            .unwrap();
        let dictionary = base
            .branch(&[(Term::Var(0), generic("Dictionary", Term::Var(2)))])
            .unwrap();
        assert_eq!(
            Constraints::unique_type(&[list.clone()], &Term::Var(0)),
            None
        );
        assert_eq!(Constraints::unique_type(&[dictionary], &Term::Var(0)), None);
        let concrete = base.branch(&[(Term::Var(0), named("Number"))]).unwrap();
        assert_eq!(
            Constraints::unique_type(&[concrete, list], &Term::Var(0)),
            None
        );
        assert_eq!(Constraints::unique_type(&[], &Term::Var(0)), None);
    }
}
