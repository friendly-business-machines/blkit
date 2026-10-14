use crate::{codegen, decision, expr, graph, semantic};
use std::collections::BTreeMap;

pub use blkit_core::{DeadlinePolicy, RetryPolicy};

pub fn transpile(source: &str) -> Result<String, String> {
    let program = parse(source)?;
    semantic::validate(&program)?;
    codegen::generate(&program)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Named(String),
    Generic(String, Box<Type>),
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Named(name) => write!(f, "{name}"),
            Self::Generic(name, inner) => write!(f, "{name}<{inner}>"),
        }
    }
}

#[derive(Debug)]
pub struct Record {
    pub name: String,
    pub fields: Vec<(String, Type)>,
}

#[derive(Debug)]
pub struct Enum {
    pub name: String,
    pub variants: Vec<String>,
}

#[derive(Debug)]
pub struct Process {
    pub name: String,
    pub input: String,
    pub input_type: Type,
    pub output: Type,
    pub body: Vec<expr::Stmt>,
    pub named_graph: Option<graph::NamedGraph>,
    pub source_graph: Option<graph::SourceGraph>,
    pub retry: Option<RetryPolicy>,
    pub deadline: Option<DeadlinePolicy>,
}

#[derive(Debug, Clone)]
pub struct ExternalTask {
    pub function: String,
    pub input: Type,
    pub output: Type,
}

#[derive(Debug)]
pub struct Program {
    pub namespace: String,
    pub version: String,
    pub records: Vec<Record>,
    pub enums: Vec<Enum>,
    pub processes: Vec<Process>,
    pub tasks: Vec<Process>,
    pub decisions: Vec<decision::DecisionModel>,
    pub peer_nodes: Vec<graph::Peer>,
    pub external_tasks: BTreeMap<String, ExternalTask>,
}

pub(crate) fn type_ref(text: &str) -> Result<Type, String> {
    if let Some((name, rest)) = text.split_once('<') {
        let inner = rest.strip_suffix('>').ok_or("unclosed type argument")?;
        if name.is_empty() || inner.is_empty() {
            return Err("invalid type argument".into());
        }
        Ok(Type::Generic(name.into(), Box::new(type_ref(inner)?)))
    } else if identifier(text) {
        Ok(Type::Named(text.into()))
    } else {
        Err(format!("invalid type {text}"))
    }
}

pub(crate) fn split_top_level(text: &str, separator: char) -> Result<Vec<&str>, String> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0_i32;
    let mut quoted = false;
    for (index, ch) in text.char_indices() {
        if ch == '"' {
            quoted = !quoted;
        }
        if !quoted {
            match ch {
                '{' | '[' | '(' => depth += 1,
                '}' | ']' | ')' => depth -= 1,
                _ => {}
            }
            if depth < 0 {
                return Err("unbalanced dictionary expression".into());
            }
            if ch == separator && depth == 0 {
                parts.push(&text[start..index]);
                start = index + ch.len_utf8();
            }
        }
    }
    if quoted || depth != 0 {
        return Err("unbalanced dictionary expression".into());
    }
    parts.push(&text[start..]);
    Ok(parts)
}

pub(crate) fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(crate) fn block_items(items: &[String], index: &mut usize) -> Result<Vec<String>, String> {
    if items.get(*index).map(String::as_str) != Some("{") {
        return Err("expected '{' after declaration".into());
    }
    *index += 1;
    let start = *index;
    let mut depth = 1;
    while *index < items.len() {
        match items[*index].as_str() {
            "{" => depth += 1,
            "}" => {
                depth -= 1;
                if depth == 0 {
                    let body = items[start..*index].to_vec();
                    *index += 1;
                    return Ok(body);
                }
            }
            _ => {}
        }
        *index += 1;
    }
    Err("unclosed declaration brace".into())
}

use crate::lexer::{self, Token};
use chumsky::{
    input::{Input as _, MappedInput},
    prelude::*,
};

type Tokens<'a> = MappedInput<'a, Token, SimpleSpan, &'a [(Token, SimpleSpan)]>;
type ParseError<'a> = extra::Err<Rich<'a, Token>>;

enum Declaration {
    Record(Record),
    Enum(Enum),
    Body(String, String, Vec<String>),
}

fn text(source: &str, span: SimpleSpan) -> &str {
    &source[span.into_range()]
}

fn name<'a>(source: &'a str) -> impl Parser<'a, Tokens<'a>, String, ParseError<'a>> + Clone {
    just(Token::Identifier).map_with(move |_, extra| text(source, extra.span()).into())
}

fn word<'a>(
    source: &'a str,
    expected: &'static str,
) -> impl Parser<'a, Tokens<'a>, (), ParseError<'a>> + Clone {
    just(Token::Identifier)
        .map_with(move |_, extra| text(source, extra.span()))
        .filter(move |value| *value == expected)
        .ignored()
}

fn type_parser<'a>(source: &'a str) -> impl Parser<'a, Tokens<'a>, Type, ParseError<'a>> + Clone {
    recursive(|ty| {
        name(source)
            .then(
                ty.delimited_by(just(Token::Less), just(Token::Greater))
                    .or_not(),
            )
            .map(|(name, inner)| match inner {
                Some(inner) => Type::Generic(name, Box::new(inner)),
                None => Type::Named(name),
            })
    })
}

// The not-yet-migrated graph/decision parsers accept statement strings. Preserve
// their delimiters and quoted text while dropping layout and full-line comments.
fn statement_text(source: &str, tokens: &[(Token, SimpleSpan)]) -> String {
    let mut result = String::new();
    let mut end = None;
    for (token, span) in tokens {
        if matches!(token, Token::Newline | Token::Indent) {
            continue;
        }
        let span = span.into_range();
        if let Some(previous) = end {
            let gap = &source[previous..span.start];
            if !gap.is_empty() {
                if gap.contains('#') || gap.contains('\n') || gap.contains('\r') {
                    result.push(' ');
                } else {
                    result.push_str(gap);
                }
            }
        }
        result.push_str(&source[span.clone()]);
        end = Some(span.end);
    }
    result
}

fn body_parser<'a>(
    source: &'a str,
) -> impl Parser<'a, Tokens<'a>, Vec<String>, ParseError<'a>> + Clone {
    recursive(|body| {
        let layout = choice((just(Token::Newline), just(Token::Indent)))
            .repeated()
            .ignored();
        let token = any().map_with(|token, extra| (token, extra.span()));
        let fragment =
            recursive::<Tokens<'a>, Vec<(Token, SimpleSpan)>, ParseError<'a>, _, _>(|fragment| {
                choice((
                    just(Token::OpenBrace)
                        .map_with(|token, extra| (token, extra.span()))
                        .then(fragment.repeated().collect::<Vec<_>>())
                        .then(
                            just(Token::CloseBrace).map_with(|token, extra| (token, extra.span())),
                        )
                        .map(|((open, middle), close)| {
                            let mut tokens = vec![open];
                            tokens.extend(middle.into_iter().flatten());
                            tokens.push(close);
                            tokens
                        }),
                    token
                        .filter(|(token, _)| {
                            !matches!(
                                token,
                                Token::OpenBrace | Token::CloseBrace | Token::Semicolon
                            )
                        })
                        .map(|token| vec![token]),
                ))
            });
        let statement = fragment
            .repeated()
            .at_least(1)
            .collect::<Vec<_>>()
            .then_ignore(just(Token::Semicolon))
            .map(move |parts| {
                statement_text(source, &parts.into_iter().flatten().collect::<Vec<_>>())
            })
            .filter(|value| !value.is_empty())
            .map(|value| vec![value]);
        let nested_kind = choice((
            word(source, "literal_expression").to("literal_expression"),
            word(source, "decision_table").to("decision_table"),
            word(source, "context").to("context"),
            word(source, "knowledge").to("knowledge"),
        ));
        let nested = nested_kind
            .then(name(source))
            .then_ignore(just(Token::OpenBrace))
            .then(body)
            .then_ignore(just(Token::CloseBrace))
            .map(|((kind, name), body)| {
                let mut items = vec![format!("{kind} {name}"), "{".into()];
                items.extend(body);
                items.push("}".into());
                items
            });
        layout
            .ignore_then(
                choice((nested, statement))
                    .padded_by(layout)
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .map(|items| items.into_iter().flatten().collect())
    })
}

fn program_parser<'a>(source: &'a str) -> impl Parser<'a, Tokens<'a>, Program, ParseError<'a>> {
    let newlines = just(Token::Newline).repeated().ignored();
    let outer_layout = choice((just(Token::Newline), just(Token::Indent)))
        .repeated()
        .ignored();
    let namespace = word(source, "namespace")
        .ignore_then(name(source))
        .then_ignore(just(Token::Semicolon));
    let version = word(source, "version")
        .ignore_then(just(Token::String).map_with(move |_, extra| {
            let value = text(source, extra.span());
            value[1..value.len() - 1].to_owned()
        }))
        .filter(|value| !value.is_empty())
        .then_ignore(just(Token::Semicolon));
    let key = choice((
        name(source),
        just(Token::String).map_with(move |_, extra| {
            let value = text(source, extra.span());
            value[1..value.len() - 1].into()
        }),
    ));
    let field_layout = choice((just(Token::Newline), just(Token::Indent)))
        .repeated()
        .ignored();
    let record = just(Token::Identifier)
        .map_with(move |_, extra| (text(source, extra.span()).to_owned(), extra.span()))
        .then(just(Token::Equal).map_with(|_, extra| extra.span()))
        .then(
            just(Token::OpenBrace)
                .map_with(|_, extra| extra.span())
                .then(
                    key.then_ignore(just(Token::Colon))
                        .then(type_parser(source).try_map_with(move |ty, extra| {
                            if text(source, extra.span()) == ty.to_string() {
                                Ok(ty)
                            } else {
                                Err(Rich::custom(extra.span(), "invalid type argument"))
                            }
                        }))
                        .padded_by(field_layout)
                        .separated_by(just(Token::Comma))
                        .allow_trailing()
                        .collect::<Vec<_>>(),
                )
                .then_ignore(just(Token::CloseBrace)),
        )
        .then_ignore(just(Token::Semicolon))
        .try_map_with(
            |(((name, name_span), equal_span), (brace_span, fields)), extra| {
                let before = &source[name_span.end..equal_span.start];
                let after = &source[equal_span.end..brace_span.start];
                if !matches!(before, " " | "\n" | "\r\n")
                    || !(after.starts_with(' ')
                        || after.starts_with('\n')
                        || after.starts_with('\r'))
                {
                    return Err(Rich::custom(extra.span(), "invalid record declaration"));
                }
                let mut used = std::collections::HashSet::new();
                for (key, _) in &fields {
                    if !used.insert(key) {
                        return Err(Rich::custom(
                            extra.span(),
                            format!("duplicate dictionary field: {key}"),
                        ));
                    }
                }
                Ok(Declaration::Record(Record { name, fields }))
            },
        );
    let variant = just(Token::Indent)
        .map_with(move |_, extra| text(source, extra.span()))
        .filter(|indent| *indent == "  ")
        .ignore_then(name(source))
        .then_ignore(just(Token::Semicolon));
    let enum_decl = word(source, "enum")
        .ignore_then(name(source))
        .then_ignore(just(Token::Colon))
        .then(
            newlines
                .ignore_then(variant)
                .repeated()
                .at_least(1)
                .collect::<Vec<_>>(),
        )
        .map(|(name, variants)| Declaration::Enum(Enum { name, variants }));
    let kind = choice((
        word(source, "start_event").to("start_event"),
        word(source, "end_event").to("end_event"),
        word(source, "decision_task").to("decision_task"),
        word(source, "process").to("process"),
        word(source, "xor_split").to("xor_split"),
        word(source, "xor_join").to("xor_join"),
        word(source, "and_split").to("and_split"),
        word(source, "and_join").to("and_join"),
        word(source, "or_split").to("or_split"),
        word(source, "or_join").to("or_join"),
        word(source, "error_event").to("error_event"),
        word(source, "cancel_event").to("cancel_event"),
        word(source, "terminate_event").to("terminate_event"),
        word(source, "pause_for").to("pause_for"),
        word(source, "pause_until").to("pause_until"),
        word(source, "subprocess").to("subprocess"),
    ));
    let braced = kind
        .then(name(source))
        .then_ignore(just(Token::OpenBrace))
        .then(body_parser(source))
        .then_ignore(just(Token::CloseBrace))
        .map(|((kind, name), body)| Declaration::Body(kind.into(), name, body));
    outer_layout
        .ignore_then(namespace)
        .then_ignore(outer_layout)
        .then(version)
        .then(
            outer_layout.ignore_then(
                choice((record, enum_decl, braced))
                    .padded_by(outer_layout)
                    .repeated()
                    .collect::<Vec<_>>(),
            ),
        )
        .then_ignore(end())
        .try_map_with(|((namespace, version), declarations), extra| {
            let mut program = Program {
                namespace,
                version,
                records: Vec::new(),
                enums: Vec::new(),
                processes: Vec::new(),
                tasks: Vec::new(),
                decisions: Vec::new(),
                peer_nodes: Vec::new(),
                external_tasks: BTreeMap::new(),
            };
            for declaration in declarations {
                match declaration {
                    Declaration::Record(record) => program.records.push(record),
                    Declaration::Enum(item) => program.enums.push(item),
                    Declaration::Body(kind, name, body) => match kind.as_str() {
                        "decision_task" => program.decisions.push(
                            decision::parse_braced(&name, &body)
                                .map_err(|message| Rich::custom(extra.span(), message))?,
                        ),
                        "process" => {
                            let (graph, retry, deadline) = graph::parse_source(&body)
                                .map_err(|message| Rich::custom(extra.span(), message))?;
                            program.processes.push(Process {
                                name,
                                input: String::new(),
                                input_type: Type::Named("Number".into()),
                                output: Type::Named("Number".into()),
                                body: Vec::new(),
                                named_graph: None,
                                source_graph: Some(graph),
                                retry,
                                deadline,
                            });
                        }
                        _ => program.peer_nodes.push(
                            graph::parse_peer(&kind, &name, &body)
                                .map_err(|message| Rich::custom(extra.span(), message))?,
                        ),
                    },
                }
            }
            Ok(program)
        })
}

pub fn parse(source: &str) -> Result<Program, String> {
    let lexed = lexer::lex_legacy_quotes(source);
    if let Some((_, span)) = lexed
        .tokens
        .iter()
        .find(|(token, _)| *token == Token::Invalid)
    {
        return Err(format!("invalid character at byte {}", span.start));
    }
    let tokens: Vec<_> = lexed
        .tokens
        .into_iter()
        .map(|(token, span)| (token, span.into()))
        .collect();
    program_parser(source)
        .parse(
            tokens
                .as_slice()
                .split_token_span((lexed.eof..lexed.eof).into()),
        )
        .into_result()
        .map_err(|errors| {
            let error = &errors[0];
            let span = error.span().into_range();
            if let Some([(_, kind_span), (_, name_span)]) =
                tokens.windows(2).find(|pair| pair[1].1.start == span.start)
                && text(source, *kind_span) == "type"
            {
                let name = text(source, *name_span);
                return format!("type {name}: is unsupported; use {name} = {{field: Type, ...}};");
            }
            let prefix = if !source.contains("namespace ") {
                "expected namespace declaration"
            } else if !source.contains("version ") {
                "expected version declaration"
            } else if span.start == source.len() {
                "unclosed brace or missing semicolon"
            } else if tokens.iter().any(|(token, location)| {
                *token == Token::CloseBrace && location.start == span.start
            }) {
                "missing semicolon"
            } else if tokens
                .iter()
                .any(|(token, location)| *token == Token::OpenParen && location.start == span.start)
            {
                "invalid declaration name"
            } else {
                "invalid declaration or statement"
            };
            format!("{prefix} at byte {}: {error:?}", span.start)
        })
}
