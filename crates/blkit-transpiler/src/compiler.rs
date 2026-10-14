use crate::{codegen, decision, expr, graph, semantic};
use std::{collections::BTreeMap, ops::Range};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompilerError {
    pub(crate) message: String,
    pub(crate) source_name: String,
    pub(crate) span: Option<Range<usize>>,
}

impl CompilerError {
    pub(crate) fn located(message: impl Into<String>, span: Range<usize>) -> Self {
        Self {
            message: message.into(),
            source_name: String::new(),
            span: Some(span),
        }
    }

    pub(crate) fn context(mut self, context: &str) -> Self {
        self.message = format!("{context}: {}", self.message);
        self
    }
}

impl From<String> for CompilerError {
    fn from(message: String) -> Self {
        Self {
            message,
            source_name: String::new(),
            span: None,
        }
    }
}

impl From<&str> for CompilerError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

impl std::fmt::Display for CompilerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.source_name.is_empty() {
            write!(f, "{}: ", self.source_name)?;
        }
        write!(f, "{}", self.message)?;
        if let Some(span) = &self.span {
            write!(f, " at byte {}", span.start)?;
        }
        Ok(())
    }
}

// Original input stays alongside the AST for later file-aware validation.
#[allow(dead_code)]
pub(crate) struct ParsedSource<'a> {
    pub(crate) program: Program,
    pub(crate) source_name: &'a str,
    pub(crate) source: &'a str,
}

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

pub(crate) type Tokens<'a> = MappedInput<'a, Token, SimpleSpan, &'a [(Token, SimpleSpan)]>;
pub(crate) type ParseError<'a> = extra::Err<Rich<'a, Token>>;

enum Declaration {
    Record(Record),
    Enum(Enum),
    Body(String, String, Vec<BodyItem>),
}

fn text(source: &str, span: SimpleSpan) -> &str {
    &source[span.into_range()]
}

pub(crate) fn name<'a>(
    source: &'a str,
) -> impl Parser<'a, Tokens<'a>, String, ParseError<'a>> + Clone {
    just(Token::Identifier).map_with(move |_, extra| text(source, extra.span()).into())
}

pub(crate) fn word<'a>(
    source: &'a str,
    expected: &'static str,
) -> impl Parser<'a, Tokens<'a>, (), ParseError<'a>> + Clone {
    just(Token::Identifier)
        .map_with(move |_, extra| text(source, extra.span()))
        .filter(move |value| *value == expected)
        .ignored()
}

pub(crate) fn type_parser<'a>(
    source: &'a str,
) -> impl Parser<'a, Tokens<'a>, Type, ParseError<'a>> + Clone {
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

#[derive(Debug)]
pub(crate) enum BodyItem {
    Statement(Vec<(Token, SimpleSpan)>, SimpleSpan),
    Block(String, String, Vec<BodyItem>, SimpleSpan),
}

pub(crate) fn assignment<'a, O, P, A, B>(
    source: &'a str,
    left: A,
    right: B,
) -> impl Parser<'a, Tokens<'a>, (O, P), ParseError<'a>> + Clone
where
    A: Parser<'a, Tokens<'a>, O, ParseError<'a>> + Clone,
    B: Parser<'a, Tokens<'a>, P, ParseError<'a>> + Clone,
{
    left.map_with(|value, extra| (value, extra.span()))
        .then(just(Token::Equal).map_with(|_, extra| extra.span()))
        .then(right.map_with(|value, extra| (value, extra.span())))
        .try_map_with(move |(((left, a), equal), (right, b)), extra| {
            if &source[a.end..equal.start] == " " && &source[equal.end..b.start] == " " {
                Ok((left, right))
            } else {
                Err(Rich::custom(extra.span(), "invalid assignment spacing"))
            }
        })
}

pub(crate) fn typed_port<'a>(
    source: &'a str,
) -> impl Parser<'a, Tokens<'a>, (String, Type), ParseError<'a>> + Clone {
    name(source)
        .map_with(|name, extra| (name, extra.span()))
        .then(just(Token::Colon).map_with(|_, extra| extra.span()))
        .then(type_parser(source).map_with(|ty, extra| (ty, extra.span())))
        .try_map_with(move |(((name, name_span), colon), (ty, ty_span)), extra| {
            if name_span.end != colon.start
                || &source[colon.end..ty_span.start] != " "
                || text(source, ty_span) != ty.to_string()
            {
                Err(Rich::custom(extra.span(), "invalid typed port"))
            } else {
                Ok((name, ty))
            }
        })
}

pub(crate) fn number<'a>(
    source: &'a str,
) -> impl Parser<'a, Tokens<'a>, String, ParseError<'a>> + Clone {
    just(Token::Number).map_with(move |_, extra| text(source, extra.span()).to_owned())
}

pub(crate) fn quoted<'a>(
    source: &'a str,
) -> impl Parser<'a, Tokens<'a>, String, ParseError<'a>> + Clone {
    just(Token::String).map_with(move |_, extra| text(source, extra.span()).to_owned())
}

pub(crate) fn statement<'a, O, P>(
    tokens: &'a [(Token, SimpleSpan)],
    terminator: SimpleSpan,
    parser: P,
) -> Result<O, CompilerError>
where
    P: Parser<'a, Tokens<'a>, O, ParseError<'a>>,
{
    let eof = terminator.start;
    parser
        .then_ignore(end())
        .parse(tokens.split_token_span((eof..eof).into()))
        .into_result()
        .map_err(|errors| {
            CompilerError::located(
                format!("statement: {:?}", errors[0]),
                errors[0].span().into_range(),
            )
        })
}

fn body_parser<'a>(
    source: &'a str,
) -> impl Parser<'a, Tokens<'a>, Vec<BodyItem>, ParseError<'a>> + Clone {
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
            .then(just(Token::Semicolon).map_with(|_, extra| extra.span()))
            .map(|(parts, terminator)| {
                BodyItem::Statement(
                    parts
                        .into_iter()
                        .flatten()
                        .filter(|(token, _)| !matches!(token, Token::Newline | Token::Indent))
                        .collect(),
                    terminator,
                )
            });
        let nested_kind = choice((
            word(source, "literal_expression").to("literal_expression"),
            word(source, "decision_table").to("decision_table"),
            word(source, "context").to("context"),
            word(source, "knowledge").to("knowledge"),
        ));
        let nested = nested_kind
            .map_with(|kind, extra| (kind, extra.span()))
            .then(name(source))
            .then_ignore(just(Token::OpenBrace))
            .then(body)
            .then_ignore(just(Token::CloseBrace))
            .map(|(((kind, kind_span), name), body)| {
                BodyItem::Block(kind.into(), name, body, kind_span)
            });
        layout.ignore_then(
            choice((nested, statement))
                .padded_by(layout)
                .repeated()
                .collect::<Vec<_>>(),
        )
    })
}

fn program_parser<'a>(
    source: &'a str,
) -> impl Parser<'a, Tokens<'a>, Result<Program, CompilerError>, ParseError<'a>> {
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
        .map(|((namespace, version), declarations)| {
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
                        "decision_task" => program
                            .decisions
                            .push(decision::parse_tokens(source, &name, &body)?),
                        "process" => {
                            let (graph, retry, deadline) =
                                graph::parse_source_tokens(source, &body)?;
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
                        _ => program
                            .peer_nodes
                            .push(graph::parse_peer_tokens(source, &kind, &name, &body)?),
                    },
                }
            }
            Ok(program)
        })
}

pub fn parse(source: &str) -> Result<Program, String> {
    parse_source(source, "<input>")
        .map(|parsed| parsed.program)
        .map_err(|error| error.to_string())
}

pub(crate) fn parse_source<'a>(
    source: &'a str,
    source_name: &'a str,
) -> Result<ParsedSource<'a>, CompilerError> {
    parse_source_inner(source)
        .map(|program| ParsedSource {
            program,
            source_name,
            source,
        })
        .map_err(|mut error| {
            error.source_name = source_name.to_owned();
            error
        })
}

fn parse_source_inner(source: &str) -> Result<Program, CompilerError> {
    let lexed = lexer::lex_legacy_quotes(source);
    if let Some((_, span)) = lexed
        .tokens
        .iter()
        .find(|(token, _)| *token == Token::Invalid)
    {
        return Err(CompilerError::located("invalid character", span.clone()));
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
                return CompilerError::located(
                    format!("type {name}: is unsupported; use {name} = {{field: Type, ...}};"),
                    span,
                );
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
            CompilerError::located(format!("{prefix}: {error:?}"), span)
        })?
}

#[cfg(test)]
mod diagnostic_tests {
    use super::parse_source;

    #[test]
    fn nested_expression_error_preserves_source_identity_and_terminator_byte() {
        let source = "namespace demo; version \"1.0\"; decision_task check { input amount: Number; output result: Number = value; literal_expression value { output result: Number; expression amount + ; } }";
        let error = parse_source(source, "test.bl").err().unwrap();
        assert_eq!(error.source_name, "test.bl");
        let semicolon = source.find("+ ;").unwrap() + 2;
        assert_eq!(error.span, Some(semicolon..semicolon));
        assert!(error.message.contains("expression"), "{error}");
    }

    #[test]
    fn lex_error_and_unicode_eof_use_original_source_bytes() {
        let source = "namespace demo; version \"1.0\";\n# café\nstart_event start { ☃ }";
        let error = parse_source(source, "unicode.bl").err().unwrap();
        let snowman = source.find('☃').unwrap();
        assert_eq!(error.span, Some(snowman..snowman + '☃'.len_utf8()));
        assert_eq!(error.source_name, "unicode.bl");
        let source = "namespace demo; version \"1.0\";\n# café\nstart_event start {";
        let error = parse_source(source, "unicode.bl").err().unwrap();
        assert_eq!(error.span, Some(source.len()..source.len()));
    }

    #[test]
    fn graph_rejects_nested_block_at_original_kind_span_without_emitting_rust() {
        let source = "namespace demo; version \"1.0\";\n# café\nprocess example { literal_expression wrong {} }";
        let kind = source.find("literal_expression").unwrap();
        let error = parse_source(source, "graph.bl").err().unwrap();
        assert_eq!(error.source_name, "graph.bl");
        assert_eq!(error.span, Some(kind..kind + "literal_expression".len()));
        assert!(error.message.contains("invalid graph block"), "{error}");
        assert!(super::transpile(source).is_err());
    }

    #[test]
    fn decision_rejects_nested_block_at_original_kind_span_without_emitting_rust() {
        let source = "namespace demo; version \"1.0\";\n# café\ndecision_task check { literal_expression value { context nested {} } }";
        let kind = source.find("context nested").unwrap();
        let error = parse_source(source, "decision.bl").err().unwrap();
        assert_eq!(error.source_name, "decision.bl");
        assert_eq!(error.span, Some(kind..kind + "context".len()));
        assert!(
            error.message.contains("unexpected nested decision block"),
            "{error}"
        );
        assert!(super::transpile(source).is_err());
    }

    #[test]
    fn parsed_source_keeps_original_text_and_identity_without_changing_program() {
        let source = "namespace demo; version \"1.0\";";
        let parsed = parse_source(source, "demo.bl").unwrap();
        assert_eq!(parsed.program.namespace, "demo");
        assert_eq!(parsed.source_name, "demo.bl");
        assert_eq!(parsed.source, source);
    }
}
