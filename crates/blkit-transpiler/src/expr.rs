use crate::lexer::{self, Token};
use chumsky::{
    input::{Input as _, MappedInput},
    pratt::*,
    prelude::*,
};

/// A parsed `.bl` expression. Source constructs are distinct from authored calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Number(String),
    String(String),
    Bool(bool),
    Name(String),
    FilterItem,
    InlinePredicate {
        binding: String,
        body: Box<Expr>,
    },
    Field(Box<Expr>, String),
    Call(String, Vec<Expr>),
    NamedArgument(String, Box<Expr>),
    Some {
        bindings: Vec<(String, Expr)>,
        predicate: Box<Expr>,
    },
    List(Vec<Expr>),
    /// Source-ordered key/value pairs; keys contain the unquoted source text.
    Dictionary(Vec<(String, Expr)>),
    Index(Box<Expr>, Box<Expr>),
    TableSelect {
        base: Box<Expr>,
        rows: Option<Box<Expr>>,
        columns: Option<Box<Expr>>,
    },
    Column(String),
    OuterVar(String),
    TableRange(Box<Expr>, Box<Expr>),
    TableMethod {
        base: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
    TableJoin(Vec<Expr>),
    TableSortKey {
        column: String,
        order: TableSortOrder,
    },
    Rename(String, String),
    Output(String, Box<Expr>),
    TableEqual(Box<Expr>, Box<Expr>),
    Sequence(Box<Expr>, Box<Expr>),
    Iteration {
        every: bool,
        flatten: bool,
        binding: String,
        source: Box<Expr>,
        body: Box<Expr>,
    },
    Range(Option<Box<Expr>>, Option<Box<Expr>>, bool, bool),
    Not(Box<Expr>),
    Binary(Box<Expr>, String, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableSortOrder {
    Asc,
    Desc,
    InOrder(Box<Expr>),
}

#[derive(Debug)]
pub enum Stmt {
    Return(Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
}

type Tokens<'a> = MappedInput<'a, Token, SimpleSpan, &'a [(Token, SimpleSpan)]>;
type ParseError<'a> = extra::Err<Rich<'a, Token>>;

fn slice<'a>(
    source: &'a str,
    extra: &mut chumsky::input::MapExtra<'a, '_, Tokens<'a>, ParseError<'a>>,
) -> &'a str {
    &source[extra.span().into_range()]
}

fn word<'a>(
    source: &'a str,
    value: &'static str,
) -> impl Parser<'a, Tokens<'a>, (), ParseError<'a>> + Clone {
    just(Token::Identifier)
        .map_with(move |_, extra| slice(source, extra))
        .filter(move |name| *name == value)
        .ignored()
}

fn name<'a>(source: &'a str) -> impl Parser<'a, Tokens<'a>, String, ParseError<'a>> + Clone {
    just(Token::Identifier).map_with(move |_, extra| slice(source, extra).to_owned())
}

fn quoted_text<'a>(source: &'a str) -> impl Parser<'a, Tokens<'a>, String, ParseError<'a>> + Clone {
    just(Token::String).try_map_with(move |_, extra| {
        let value = slice(source, extra);
        let value = &value[1..value.len() - 1];
        if value.contains("\\\"") {
            Err(Rich::custom(
                extra.span(),
                "escaped quotes are not supported in expressions",
            ))
        } else {
            Ok(value.to_owned())
        }
    })
}

pub(crate) fn expression_parser<'a>(
    source: &'a str,
    columns: &'a [String],
) -> impl Parser<'a, Tokens<'a>, Expr, ParseError<'a>> {
    recursive(|expr| {
        let ident = name(source);
        let range_bound = choice((word(source, "null").to(None), expr.clone().map(Some)));
        let range = choice((
            just(Token::OpenBracket).to(true),
            just(Token::OpenParen).to(false),
        ))
        .then(range_bound.clone().or_not())
        .then_ignore(just(Token::Range))
        .then(range_bound.clone())
        .then(choice((
            just(Token::CloseBracket).to(true),
            just(Token::CloseParen).to(false),
        )))
        .map(|(((lower_closed, lower), upper), upper_closed)| {
            Expr::Range(
                lower.flatten().map(Box::new),
                upper.map(Box::new),
                lower_closed,
                upper_closed,
            )
        })
        .boxed();
        let null_list = word(source, "null")
            .delimited_by(just(Token::OpenBracket), just(Token::CloseBracket))
            .to(Expr::List(vec![]));
        let list = expr
            .clone()
            .separated_by(just(Token::Comma))
            .collect::<Vec<_>>()
            .delimited_by(
                just(Token::OpenBracket).then_ignore(word(source, "null").not()),
                just(Token::CloseBracket),
            )
            .map(Expr::List)
            .boxed();
        let grouped = expr
            .clone()
            .delimited_by(
                just(Token::OpenParen).then_ignore(word(source, "null").not()),
                just(Token::CloseParen),
            )
            .boxed();
        let key = choice((quoted_text(source), ident.clone()));
        let dictionary = key
            .then_ignore(just(Token::Colon))
            .then(expr.clone())
            .separated_by(just(Token::Comma))
            .collect::<Vec<_>>()
            .delimited_by(just(Token::OpenBrace), just(Token::CloseBrace))
            .try_map_with(|entries, extra| {
                let mut seen = std::collections::HashSet::new();
                for (key, _) in &entries {
                    if !seen.insert(key) {
                        return Err(Rich::custom(
                            extra.span(),
                            format!("duplicate dictionary key: {key}"),
                        ));
                    }
                }
                Ok(Expr::Dictionary(entries))
            })
            .boxed();
        let binding = ident
            .clone()
            .filter(|name| !matches!(name.as_str(), "in" | "return" | "satisfies"))
            .then_ignore(word(source, "in"))
            .then(expr.clone());
        let iteration = choice((
            word(source, "every").to("every"),
            word(source, "some").to("some"),
            word(source, "for").to("for"),
        ))
        .then(
            binding
                .separated_by(just(Token::Comma))
                .at_least(1)
                .collect::<Vec<_>>(),
        )
        .then(choice((
            word(source, "return").to("return"),
            word(source, "satisfies").to("satisfies"),
        )))
        .then(expr.clone())
        .try_map_with(|(((kind, bindings), keyword), mut body), extra| {
            if (kind == "for") != (keyword == "return") {
                return Err(Rich::custom(extra.span(), "invalid iteration body keyword"));
            }
            if kind == "some" {
                return Ok(Expr::Some {
                    bindings,
                    predicate: Box::new(body),
                });
            }
            for (index, (binding, source)) in bindings.into_iter().rev().enumerate() {
                body = Expr::Iteration {
                    every: kind == "every",
                    flatten: kind != "every" && index > 0,
                    binding,
                    source: Box::new(source),
                    body: Box::new(body),
                };
            }
            Ok(body)
        })
        .boxed();
        let atom = choice((
            range,
            grouped,
            null_list,
            list,
            dictionary,
            iteration,
            just(Token::Dot)
                .ignore_then(word(source, "item"))
                .to(Expr::FilterItem),
            word(source, "true").to(Expr::Bool(true)),
            word(source, "false").to(Expr::Bool(false)),
            just(Token::Number).map_with(move |_, extra| Expr::Number(slice(source, extra).into())),
            quoted_text(source).map(Expr::String),
            ident.map(Expr::Name),
        ))
        .boxed();

        enum Arg {
            Positional(Expr),
            Named(String, Expr),
            Rename(String, String),
            Predicate(String, Expr),
        }
        let argument = choice((
            name(source)
                .then_ignore(just(Token::Arrow))
                .then(name(source))
                .map(|(from, to)| Arg::Rename(from, to)),
            name(source)
                .then_ignore(just(Token::Equal))
                .then(expr.clone())
                .map(|(name, expr)| Arg::Named(name, expr)),
            word(source, "function")
                .ignore_then(
                    name(source)
                        .filter(|name| {
                            !matches!(name.as_str(), "in" | "return" | "satisfies" | "function")
                        })
                        .delimited_by(just(Token::OpenParen), just(Token::CloseParen)),
                )
                .then(expr.clone())
                .map(|(binding, body)| Arg::Predicate(binding, body)),
            expr.clone().map(Arg::Positional),
        ));
        let call = argument
            .separated_by(just(Token::Comma))
            .collect::<Vec<_>>()
            .delimited_by(just(Token::OpenParen), just(Token::CloseParen))
            .map(|args| move |left: Expr| (left, args));
        let field = just(Token::Dot)
            .ignore_then(name(source))
            .map(|field| move |left| Expr::Field(Box::new(left), field));
        let table_index = expr
            .clone()
            .or_not()
            .then_ignore(just(Token::Comma))
            .then(expr.clone().or_not())
            .delimited_by(just(Token::OpenBracket), just(Token::CloseBracket))
            .map(|(rows, columns)| {
                move |base| {
                    if rows.is_none() && columns.is_none() {
                        return Err("table selector requires a row or column".into());
                    }
                    let columns = columns.map(table_columns).transpose()?;
                    Ok(Expr::TableSelect {
                        base: Box::new(base),
                        rows: rows.map(table_row).transpose()?.map(Box::new),
                        columns: columns.map(Box::new),
                    })
                }
            });
        let index = expr
            .clone()
            .delimited_by(just(Token::OpenBracket), just(Token::CloseBracket))
            .map(|key| move |left| Expr::Index(Box::new(left), Box::new(key)));
        // Calls validate their target and argument positions before they become AST nodes.
        let postfix = atom.map(Ok).foldl(
            choice((
                call.map(|f| Box::new(move |left: Expr| {
                    let (target, args) = f(left);
                    let (name, base) = match target {
                        Expr::Name(name) => (name, None),
                        Expr::Field(base, name) if is_table_method(&name) => (name, Some(base)),
                        _ => return Err("call requires a function name".to_owned()),
                    };
                    let mut result = Vec::new();
                    let mut named = Vec::new();
                    for arg in args {
                        match arg {
                            Arg::Positional(Expr::Call(function, args)) if function == "function" &&
                                matches!(args.as_slice(), [Expr::Name(_)]) &&
                                matches!(name.as_str(), "remove" | "listReplace") && result.len() == 1 => {
                                return Err("inline predicate requires a body".into());
                            }
                            Arg::Positional(value) if named.is_empty() => result.push(value),
                            Arg::Positional(_) => return Err("positional argument after named argument".into()),
                            Arg::Rename(from, to) => {
                                if name != "rename" || base.is_none() {
                                    return Err("rename arrow requires a table rename method".into());
                                }
                                result.push(Expr::Rename(from, to));
                            }
                            Arg::Named(option, value) if base.is_some() && matches!(name.as_str(), "withColumn" | "agg") => {
                                result.push(Expr::Output(option, Box::new(table_row(value)?)));
                            }
                            Arg::Named(option, value) => {
                                if !matches!(name.as_str(), "calendarDrop" | "calendarKeep" | "calendarMerge") {
                                    return Err("named arguments are only supported by calendar transformations".into());
                                }
                                if named.contains(&option) { return Err(format!("duplicate named argument: {option}")); }
                                named.push(option.clone());
                                result.push(Expr::NamedArgument(option, Box::new(value)));
                            }
                            Arg::Predicate(binding, body) => {
                                if !matches!(name.as_str(), "remove" | "listReplace") || result.len() != 1 || !named.is_empty() {
                                    return Err("inline predicate requires remove or listReplace match argument".into());
                                }
                                result.push(Expr::InlinePredicate { binding, body: Box::new(body) });
                            }
                        }
                    }
                    if let Some(base) = base {
                        let args = table_method_arguments(&name, result)?;
                        Ok(Expr::TableMethod { base, method: name, args })
                    } else if name == "join" {
                        if !matches!(result.len(), 3 | 4) {
                            return Err("join requires a table, another table, and a key".into());
                        }
                        let on = table_columns(result.remove(2))?;
                        result.insert(2, on);
                        Ok(Expr::TableJoin(result))
                    } else {
                        Ok(Expr::Call(name, result))
                    }
                }) as Box<dyn FnOnce(Expr) -> Result<Expr, String>>),
                field.map(|f| Box::new(move |left| Ok(f(left))) as Box<dyn FnOnce(Expr) -> Result<Expr, String>>),
                table_index.map(|f| Box::new(f) as Box<dyn FnOnce(Expr) -> Result<Expr, String>>),
                index.map(|f| Box::new(move |left| Ok(f(left))) as Box<dyn FnOnce(Expr) -> Result<Expr, String>>),
            )).repeated(),
            |left: Result<Expr, String>, f| left.and_then(f),
        ).try_map_with(|result, extra| result.map_err(|message| Rich::custom(extra.span(), message))).boxed();
        let math = postfix
            .pratt((
                prefix(6, just(Token::Minus), |_, value, _| {
                    Expr::Binary(
                        Box::new(Expr::Number("0".into())),
                        "-".into(),
                        Box::new(value),
                    )
                }),
                prefix(6, just(Token::Plus), |_, value, _| value),
                prefix(4, word(source, "not"), |_, value, _| {
                    Expr::Not(Box::new(value))
                }),
                infix(right(7), just(Token::Power), |a, _, b, _| {
                    Expr::Binary(Box::new(a), "**".into(), Box::new(b))
                }),
                infix(
                    left(5),
                    choice((
                        just(Token::Star).to("*"),
                        just(Token::Slash).to("/"),
                        word(source, "mod").to("mod"),
                    )),
                    |a, op: &str, b, _| Expr::Binary(Box::new(a), op.into(), Box::new(b)),
                ),
                infix(
                    left(4),
                    choice((just(Token::Plus).to("+"), just(Token::Minus).to("-"))),
                    |a, op: &str, b, _| Expr::Binary(Box::new(a), op.into(), Box::new(b)),
                ),
            ))
            .boxed();
        let compare_op = choice((
            just(Token::EqualEqual).to("=="),
            just(Token::Equal).to("="),
            just(Token::NotEqual).to("!="),
            just(Token::GreaterEqual).to(">="),
            just(Token::LessEqual).to("<="),
            just(Token::Greater).to(">"),
            just(Token::Less).to("<"),
            word(source, "in").to("in"),
        ));
        let comparison = compare_op.then(math.clone()).map(|(op, right)| {
            Box::new(move |left: Expr| {
                Ok(if op == "=" {
                    Expr::TableEqual(Box::new(left), Box::new(right))
                } else {
                    Expr::Binary(Box::new(left), op.into(), Box::new(right))
                })
            }) as Box<dyn FnOnce(Expr) -> Result<Expr, String>>
        });
        let between = word(source, "between")
            .ignore_then(math.clone())
            .then_ignore(word(source, "and"))
            .then(math.clone())
            .map(|(lower, upper)| {
                Box::new(move |left: Expr| {
                    Ok(Expr::Binary(
                        Box::new(left),
                        "in".into(),
                        Box::new(Expr::Range(
                            Some(Box::new(lower)),
                            Some(Box::new(upper)),
                            true,
                            true,
                        )),
                    ))
                }) as Box<dyn FnOnce(Expr) -> Result<Expr, String>>
            });
        let unary_test = choice((
            choice((
                just(Token::Less).to("<"),
                just(Token::LessEqual).to("<="),
                just(Token::Greater).to(">"),
                just(Token::GreaterEqual).to(">="),
                just(Token::EqualEqual).to("=="),
                just(Token::NotEqual).to("!="),
            ))
            .then(expr.clone())
            .map(|(op, value)| (Some(op), value)),
            range_bound
                .clone()
                .filter(|value| matches!(value, Some(Expr::Range(..))))
                .map(|value| (None, value.unwrap())),
        ));
        let matches = word(source, "matches")
            .ignore_then(
                unary_test
                    .separated_by(just(Token::Comma))
                    .at_least(1)
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::OpenParen), just(Token::CloseParen)),
            )
            .map(move |tests| {
                Box::new(move |left: Expr| {
                    let Expr::Name(name) = &left else {
                        return Err("matches requires a table input column".into());
                    };
                    if !columns.contains(name) {
                        return Err(format!("unknown table input column: {name}"));
                    }
                    Ok(tests
                        .into_iter()
                        .map(|(op, value)| {
                            Expr::Binary(
                                Box::new(left.clone()),
                                op.unwrap_or("in").into(),
                                Box::new(value),
                            )
                        })
                        .reduce(|a, b| Expr::Binary(Box::new(a), "or".into(), Box::new(b)))
                        .unwrap())
                }) as Box<dyn FnOnce(Expr) -> Result<Expr, String>>
            });
        let compared = math
            .map(Ok)
            .foldl(
                choice((comparison, between, matches)).repeated(),
                |left: Result<Expr, String>, op| left.and_then(op),
            )
            .try_map_with(|result, extra| {
                result.map_err(|message| Rich::custom(extra.span(), message))
            })
            .boxed();
        let conjunction = compared
            .clone()
            .foldl(
                word(source, "and").ignore_then(compared).repeated(),
                |a, b| Expr::Binary(Box::new(a), "and".into(), Box::new(b)),
            )
            .boxed();
        conjunction.clone().foldl(
            choice((word(source, "or").to("or"), just(Token::Colon).to(":")))
                .then(conjunction)
                .repeated(),
            |a, (op, b)| {
                if op == ":" {
                    Expr::Sequence(Box::new(a), Box::new(b))
                } else {
                    Expr::Binary(Box::new(a), op.into(), Box::new(b))
                }
            },
        )
    })
}

fn is_table_method(name: &str) -> bool {
    matches!(
        name,
        "filter"
            | "filterOut"
            | "select"
            | "rename"
            | "slice"
            | "distinct"
            | "withColumn"
            | "sort"
            | "union"
            | "join"
            | "groupBy"
            | "agg"
            | "toList"
            | "toDict"
            | "toValue"
    )
}

fn table_columns(value: Expr) -> Result<Expr, String> {
    match value {
        Expr::Name(name) => Ok(Expr::Column(name)),
        Expr::List(items) => items
            .into_iter()
            .map(|item| match item {
                Expr::Name(name) => Ok(Expr::Column(name)),
                _ => Err("table columns require unquoted names".into()),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Expr::List),
        _ => Err("table columns require unquoted names".into()),
    }
}

pub(crate) fn table_row(value: Expr) -> Result<Expr, String> {
    Ok(match value {
        Expr::Name(name) => Expr::Column(name),
        Expr::Sequence(start, end) => {
            Expr::TableRange(Box::new(table_row(*start)?), Box::new(table_row(*end)?))
        }
        Expr::Call(name, args) if name == "var" => {
            let [Expr::Name(binding)] = args.as_slice() else {
                return Err("var requires one unquoted variable name".into());
            };
            Expr::OuterVar(binding.clone())
        }
        Expr::Call(name, args) => Expr::Call(
            name,
            args.into_iter().map(table_row).collect::<Result<_, _>>()?,
        ),
        Expr::List(items) => {
            Expr::List(items.into_iter().map(table_row).collect::<Result<_, _>>()?)
        }
        Expr::Dictionary(entries) => Expr::Dictionary(
            entries
                .into_iter()
                .map(|(key, value)| Ok((key, table_row(value)?)))
                .collect::<Result<_, String>>()?,
        ),
        Expr::Range(lower, upper, lower_closed, upper_closed) => Expr::Range(
            lower
                .map(|bound| table_row(*bound))
                .transpose()?
                .map(Box::new),
            upper
                .map(|bound| table_row(*bound))
                .transpose()?
                .map(Box::new),
            lower_closed,
            upper_closed,
        ),
        Expr::Binary(left, op, right) => Expr::Binary(
            Box::new(table_row(*left)?),
            op,
            Box::new(table_row(*right)?),
        ),
        Expr::TableEqual(left, right) => {
            Expr::TableEqual(Box::new(table_row(*left)?), Box::new(table_row(*right)?))
        }
        Expr::Field(base, field) => Expr::Field(Box::new(table_row(*base)?), field),
        Expr::Index(base, key) => {
            Expr::Index(Box::new(table_row(*base)?), Box::new(table_row(*key)?))
        }
        Expr::Not(inner) => Expr::Not(Box::new(table_row(*inner)?)),
        other => other,
    })
}

fn table_sort_key(value: Expr) -> Result<Expr, String> {
    match value {
        Expr::Call(name, args) if matches!(name.as_str(), "asc" | "desc" | "inOrder") => {
            let mut args = args.into_iter();
            let Some(Expr::Name(column)) = args.next() else {
                return Err(format!("{name} requires an unquoted column name"));
            };
            let order = match name.as_str() {
                "asc" | "desc" if args.next().is_none() => {
                    if name == "asc" {
                        TableSortOrder::Asc
                    } else {
                        TableSortOrder::Desc
                    }
                }
                "inOrder" => {
                    let order = args.next().ok_or("inOrder requires an order expression")?;
                    if args.next().is_some() {
                        return Err("inOrder requires one column and one order expression".into());
                    }
                    TableSortOrder::InOrder(Box::new(table_row(order)?))
                }
                _ => return Err(format!("{name} requires one column")),
            };
            Ok(Expr::TableSortKey { column, order })
        }
        other => table_row(other),
    }
}

fn table_method_arguments(name: &str, args: Vec<Expr>) -> Result<Vec<Expr>, String> {
    args.into_iter()
        .enumerate()
        .map(|(index, arg)| match name {
            "select" | "groupBy" => table_columns(arg),
            "join" if index == 1 => table_columns(arg),
            "sort" => table_sort_key(arg),
            "filter" | "filterOut" | "slice" => table_row(arg),
            "rename" if matches!(arg, Expr::Rename(..)) => Ok(arg),
            "agg" | "withColumn" if matches!(arg, Expr::Output(..)) => Ok(arg),
            "rename" | "agg" | "withColumn" => {
                Err(format!("{name} requires table-specific arguments"))
            }
            _ => Ok(arg),
        })
        .collect()
}

pub fn expression(text: &str) -> Result<Expr, String> {
    parse_expression(text, &[])
}

pub fn table_condition(text: &str, columns: &[String]) -> Result<Expr, String> {
    parse_expression(text, columns)
}

pub(crate) fn expression_list(text: &str) -> Result<Vec<Expr>, String> {
    let tokens = expression_tokens(text)?;
    expression_parser(text, &[])
        .separated_by(just(Token::Comma))
        .at_least(1)
        .collect::<Vec<_>>()
        .then_ignore(end())
        .parse(
            tokens
                .as_slice()
                .split_token_span((text.len()..text.len()).into()),
        )
        .into_result()
        .map_err(|errors| format!("expression: {:?}", errors[0]))
}

fn expression_tokens(text: &str) -> Result<Vec<(Token, SimpleSpan)>, String> {
    let lexed = lexer::lex_legacy_quotes(text);
    if lexed.has_full_line_comment {
        return Err("invalid character in expression: #".into());
    }
    Ok(lexed
        .tokens
        .into_iter()
        .filter(|(token, _)| !matches!(token, Token::Newline | Token::Indent))
        .map(|(token, span)| (token, span.into()))
        .collect())
}

fn parse_expression(text: &str, columns: &[String]) -> Result<Expr, String> {
    let tokens = expression_tokens(text)?;
    expression_parser(text, columns)
        .then_ignore(end())
        .parse(
            tokens
                .as_slice()
                .split_token_span((text.len()..text.len()).into()),
        )
        .into_result()
        .map_err(|errors| format!("expression: {:?}", errors[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_two_slot_selectors_keep_omissions_and_scoped_columns() {
        for (source, expected) in [
            (
                "t[, rate]",
                "TableSelect { base: Name(\"t\"), rows: None, columns: Some(Column(\"rate\")) }",
            ),
            (
                "t[1, rate]",
                "TableSelect { base: Name(\"t\"), rows: Some(Number(\"1\")), columns: Some(Column(\"rate\")) }",
            ),
            (
                "t[rate > var(minimum), [region, rate]]",
                "TableSelect { base: Name(\"t\"), rows: Some(Binary(Column(\"rate\"), \">\", OuterVar(\"minimum\"))), columns: Some(List([Column(\"region\"), Column(\"rate\")])) }",
            ),
            (
                "t[[3, 1, 3:4], region]",
                "TableSelect { base: Name(\"t\"), rows: Some(List([Number(\"3\"), Number(\"1\"), TableRange(Number(\"3\"), Number(\"4\"))])), columns: Some(Column(\"region\")) }",
            ),
            (
                "t[1,]",
                "TableSelect { base: Name(\"t\"), rows: Some(Number(\"1\")), columns: None }",
            ),
        ] {
            assert_eq!(
                format!("{:?}", expression(source).unwrap()),
                expected,
                "{source}"
            );
        }
        for invalid in ["t[]", "t[1, 2, rate]", "t[1, 2, 3]"] {
            assert!(expression(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn table_methods_keep_columns_renames_outputs_and_chain_order() {
        let ast = expression("t.select(region, rate).rename(rate -> price).withColumn(with_tax = price * 1.2).sort(region, desc(with_tax))").unwrap();
        assert_eq!(
            format!("{ast:?}"),
            "TableMethod { base: TableMethod { base: TableMethod { base: TableMethod { base: Name(\"t\"), method: \"select\", args: [Column(\"region\"), Column(\"rate\")] }, method: \"rename\", args: [Rename(\"rate\", \"price\")] }, method: \"withColumn\", args: [Output(\"with_tax\", Binary(Column(\"price\"), \"*\", Number(\"1.2\")))] }, method: \"sort\", args: [Column(\"region\"), TableSortKey { column: \"with_tax\", order: Desc }] }"
        );
        assert_eq!(
            format!(
                "{:?}",
                expression("t.groupBy(region).agg(total = sum(rate), n = count())").unwrap()
            ),
            "TableMethod { base: TableMethod { base: Name(\"t\"), method: \"groupBy\", args: [Column(\"region\")] }, method: \"agg\", args: [Output(\"total\", Call(\"sum\", [Column(\"rate\")])), Output(\"n\", Call(\"count\", []))] }"
        );
        assert_eq!(
            format!(
                "{:?}",
                expression("t.filter(rate > var(rate)).filterOut(rate < 0)").unwrap()
            ),
            "TableMethod { base: TableMethod { base: Name(\"t\"), method: \"filter\", args: [Binary(Column(\"rate\"), \">\", OuterVar(\"rate\"))] }, method: \"filterOut\", args: [Binary(Column(\"rate\"), \"<\", Number(\"0\"))] }"
        );
    }

    #[test]
    fn table_equal_is_distinct_from_ordinary_equality_and_named_arguments() {
        assert_eq!(
            format!("{:?}", expression("t = other").unwrap()),
            "TableEqual(Name(\"t\"), Name(\"other\"))"
        );
        assert_eq!(
            expression("t == other").unwrap(),
            Expr::Binary(
                Box::new(Expr::Name("t".into())),
                "==".into(),
                Box::new(Expr::Name("other".into()))
            )
        );
        assert_eq!(
            format!("{:?}", expression("some r in t satisfies r = row").unwrap()),
            "Some { bindings: [(\"r\", Name(\"t\"))], predicate: TableEqual(Name(\"r\"), Name(\"row\")) }"
        );
        assert!(matches!(
            expression("calendarKeep(c, tiebreak=x)").unwrap(),
            Expr::Call(_, _)
        ));
    }

    #[test]
    fn table_global_join_and_sort_keys_keep_column_roles() {
        assert_eq!(
            format!(
                "{:?}",
                expression("join(t, other, [region, tier], \"left\")").unwrap()
            ),
            "TableJoin([Name(\"t\"), Name(\"other\"), List([Column(\"region\"), Column(\"tier\")]), String(\"left\")])"
        );
        assert_eq!(
            format!(
                "{:?}",
                expression("t.sort(inOrder(region, [\"eu\", \"us\"]), desc(rate))").unwrap()
            ),
            "TableMethod { base: Name(\"t\"), method: \"sort\", args: [TableSortKey { column: \"region\", order: InOrder(List([String(\"eu\"), String(\"us\")])) }, TableSortKey { column: \"rate\", order: Desc }] }"
        );
        assert_eq!(
            format!("{:?}", expression("t[1:4, region]").unwrap()),
            "TableSelect { base: Name(\"t\"), rows: Some(TableRange(Number(\"1\"), Number(\"4\"))), columns: Some(Column(\"region\")) }"
        );
        assert_eq!(
            format!(
                "{:?}",
                expression("t.sort(inOrder(region, var(order)))").unwrap()
            ),
            "TableMethod { base: Name(\"t\"), method: \"sort\", args: [TableSortKey { column: \"region\", order: InOrder(OuterVar(\"order\")) }] }"
        );
        for invalid in [
            "join(t, other)",
            "t[,]",
            "t.select(\"rate\")",
            "t.rename(rate, price)",
            "t.agg(total)",
            "t.filter(rate > var(rate + 1))",
        ] {
            assert!(expression(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn table_column_selector_rejects_nested_lists() {
        for invalid in [
            "t[, [[rate]]]",
            "t.select([[rate]])",
            "join(t, other, [[rate]])",
        ] {
            assert!(expression(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn table_row_scope_reaches_range_bounds_and_dictionary_values() {
        assert_eq!(
            format!(
                "{:?}",
                expression("t[rate in [var(minimum)..var(maximum)], region]").unwrap()
            ),
            "TableSelect { base: Name(\"t\"), rows: Some(Binary(Column(\"rate\"), \"in\", Range(Some(OuterVar(\"minimum\")), Some(OuterVar(\"maximum\")), true, true))), columns: Some(Column(\"region\")) }"
        );
        assert_eq!(
            format!(
                "{:?}",
                expression(
                    "t.withColumn(result = {amount: rate, bounds: [var(minimum)..var(maximum)]})"
                )
                .unwrap()
            ),
            "TableMethod { base: Name(\"t\"), method: \"withColumn\", args: [Output(\"result\", Dictionary([(\"amount\", Column(\"rate\")), (\"bounds\", Range(Some(OuterVar(\"minimum\")), Some(OuterVar(\"maximum\")), true, true))]))] }"
        );
        assert!(expression("t.filter(rate in [var(rate + 1)..5])").is_err());
    }

    #[test]
    fn table_sort_constructors_have_distinct_ast_only_within_sort() {
        assert_eq!(
            format!(
                "{:?}",
                expression("t.sort(asc(region), desc(rate), inOrder(region, var(order)))").unwrap()
            ),
            "TableMethod { base: Name(\"t\"), method: \"sort\", args: [TableSortKey { column: \"region\", order: Asc }, TableSortKey { column: \"rate\", order: Desc }, TableSortKey { column: \"region\", order: InOrder(OuterVar(\"order\")) }] }"
        );
        assert_eq!(
            format!("{:?}", expression("asc(region)").unwrap()),
            "Call(\"asc\", [Name(\"region\")])"
        );
        assert_eq!(
            format!("{:?}", expression("inOrder(region, order)").unwrap()),
            "Call(\"inOrder\", [Name(\"region\"), Name(\"order\")])"
        );
        assert!(expression("t.sort(desc(\"rate\"))").is_err());
        assert!(expression("t.sort(inOrder(region))").is_err());
    }

    #[test]
    fn table_syntax_does_not_change_legacy_index_sequence_range_or_calls() {
        assert_eq!(
            format!("{:?}", expression("items[1]").unwrap()),
            "Index(Name(\"items\"), Number(\"1\"))"
        );
        assert_eq!(
            format!("{:?}", expression("items[1:4]").unwrap()),
            "Index(Name(\"items\"), Sequence(Number(\"1\"), Number(\"4\")))"
        );
        assert_eq!(
            format!("{:?}", expression("[1, 2:3]").unwrap()),
            "List([Number(\"1\"), Sequence(Number(\"2\"), Number(\"3\"))])"
        );
        assert_eq!(
            format!("{:?}", expression("[1..4]").unwrap()),
            "Range(Some(Number(\"1\")), Some(Number(\"4\")), true, true)"
        );
        assert_eq!(
            format!("{:?}", expression("{a: 1}[\"a\"]").unwrap()),
            "Index(Dictionary([(\"a\", Number(\"1\"))]), String(\"a\"))"
        );
        assert!(expression("t.rate[1]").is_ok());
        assert!(expression("union([1], [2])").is_ok());
        assert_eq!(
            format!("{:?}", expression("[[rate]]").unwrap()),
            "List([List([Name(\"rate\")])])"
        );
        assert!(expression("t[1, rate, region]").is_err());
    }
}
