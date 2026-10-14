use crate::lexer::{self, Token};
use chumsky::{
    input::{Input as _, MappedInput},
    pratt::*,
    prelude::*,
};

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
    List(Vec<Expr>),
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

#[derive(Debug)]
pub enum Stmt {
    Return(Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
}

pub fn body(lines: &[String]) -> Result<Vec<Stmt>, String> {
    let mut index = 0;
    let result = block(lines, &mut index, 2)?;
    if index != lines.len() {
        return Err(format!("unexpected indentation: {}", lines[index]));
    }
    Ok(result)
}

fn block(lines: &[String], index: &mut usize, indent: usize) -> Result<Vec<Stmt>, String> {
    let mut result = Vec::new();
    while *index < lines.len() {
        let line = &lines[*index];
        let spaces = line.len() - line.trim_start_matches(' ').len();
        if spaces < indent {
            break;
        }
        if spaces != indent {
            return Err(format!("unexpected indentation: {line}"));
        }
        let statement = &line[indent..];
        if let Some(text) = statement.strip_prefix("return ") {
            result.push(Stmt::Return(expression(text)?));
            *index += 1;
        } else if let Some(text) = statement.strip_prefix("if ") {
            let condition = expression(text.strip_suffix(':').ok_or("missing colon after if")?)?;
            *index += 1;
            let yes = block(lines, index, indent + 2)?;
            if yes.is_empty() {
                return Err("empty if body".into());
            }
            let mut no = Vec::new();
            if *index < lines.len() && lines[*index] == format!("{}else:", " ".repeat(indent)) {
                *index += 1;
                no = block(lines, index, indent + 2)?;
                if no.is_empty() {
                    return Err("empty else body".into());
                }
            }
            result.push(Stmt::If(condition, yes, no));
        } else {
            return Err(format!("invalid statement: {statement}"));
        }
    }
    Ok(result)
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

fn expression_parser<'a>(
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
            .filter(|values| !matches!(values.first(), Some(Expr::Name(name)) if name == "null"))
            .delimited_by(just(Token::OpenBracket), just(Token::CloseBracket))
            .map(Expr::List)
            .boxed();
        let grouped = expr
            .clone()
            .filter(|value| !matches!(value, Expr::Name(name) if name == "null"))
            .delimited_by(just(Token::OpenParen), just(Token::CloseParen))
            .boxed();
        let key = choice((quoted_text(source), ident.clone()));
        let dictionary =
            key.then_ignore(just(Token::Colon))
                .then(expr.clone())
                .separated_by(just(Token::Comma))
                .collect::<Vec<_>>()
                .delimited_by(just(Token::OpenBrace), just(Token::CloseBrace))
                .try_map_with(|entries, extra| {
                    let mut args = Vec::new();
                    for (key, value) in entries {
                        if args.iter().any(
                            |entry| matches!(entry, Expr::String(existing) if existing == &key),
                        ) {
                            return Err(Rich::custom(
                                extra.span(),
                                format!("duplicate dictionary key: {key}"),
                            ));
                        }
                        args.push(Expr::String(key));
                        args.push(value);
                    }
                    Ok(Expr::Call("__bl_dictionary".into(), args))
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
            for (index, (binding, source)) in bindings.into_iter().rev().enumerate() {
                body = Expr::Iteration {
                    every: kind == "every",
                    flatten: kind != "every" && index > 0,
                    binding,
                    source: Box::new(source),
                    body: Box::new(body),
                };
            }
            Ok(if kind == "some" {
                Expr::Call("__bl_some".into(), vec![body])
            } else {
                body
            })
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
            Predicate(String, Expr),
        }
        let argument = choice((
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
        let index = expr
            .clone()
            .delimited_by(just(Token::OpenBracket), just(Token::CloseBracket))
            .map(|key| move |left| Expr::Call("__bl_index".into(), vec![left, key]));
        // Calls validate their target and argument positions before they become AST nodes.
        let postfix = atom.map(Ok).foldl(
            choice((
                call.map(|f| Box::new(move |left: Expr| {
                    let (target, args) = f(left);
                    let Expr::Name(name) = target else { return Err("call requires a function name".to_owned()) };
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
                            Arg::Named(option, value) => {
                                if !matches!(name.as_str(), "calendarDrop" | "calendarKeep" | "calendarMerge") {
                                    return Err("named arguments are only supported by calendar transformations".into());
                                }
                                if named.contains(&option) { return Err(format!("duplicate named argument: {option}")); }
                                named.push(option.clone());
                                result.push(Expr::Call(format!("__bl_named_{option}"), vec![value]));
                            }
                            Arg::Predicate(binding, body) => {
                                if !matches!(name.as_str(), "remove" | "listReplace") || result.len() != 1 || !named.is_empty() {
                                    return Err("inline predicate requires remove or listReplace match argument".into());
                                }
                                result.push(Expr::InlinePredicate { binding, body: Box::new(body) });
                            }
                        }
                    }
                    Ok(Expr::Call(name, result))
                }) as Box<dyn FnOnce(Expr) -> Result<Expr, String>>),
                field.map(|f| Box::new(move |left| Ok(f(left))) as Box<dyn FnOnce(Expr) -> Result<Expr, String>>),
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
            just(Token::NotEqual).to("!="),
            just(Token::GreaterEqual).to(">="),
            just(Token::LessEqual).to("<="),
            just(Token::Greater).to(">"),
            just(Token::Less).to("<"),
            word(source, "in").to("in"),
        ));
        let comparison = compare_op.then(math.clone()).map(|(op, right)| {
            Box::new(move |left: Expr| Ok(Expr::Binary(Box::new(left), op.into(), Box::new(right))))
                as Box<dyn FnOnce(Expr) -> Result<Expr, String>>
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
                    Expr::Call("__bl_sequence".into(), vec![a, b])
                } else {
                    Expr::Binary(Box::new(a), op.into(), Box::new(b))
                }
            },
        )
    })
}

pub fn expression(text: &str) -> Result<Expr, String> {
    parse_expression(text, &[])
}

pub fn table_condition(text: &str, columns: &[String]) -> Result<Expr, String> {
    parse_expression(text, columns)
}

fn parse_expression(text: &str, columns: &[String]) -> Result<Expr, String> {
    let lexed = lexer::lex(text);
    if lexed.has_full_line_comment {
        return Err("invalid character in expression: #".into());
    }
    let tokens: Vec<_> = lexed
        .tokens
        .into_iter()
        .filter(|(token, _)| !matches!(token, Token::Newline | Token::Indent))
        .map(|(token, span)| (token, span.into()))
        .collect();
    expression_parser(text, columns)
        .then_ignore(end())
        .parse(
            tokens
                .as_slice()
                .split_token_span((lexed.eof..lexed.eof).into()),
        )
        .into_result()
        .map_err(|errors| format!("expression: {:?}", errors[0]))
}
