use crate::{codegen, decision, expr, graph, semantic};
use std::{collections::BTreeMap, ops::Range, sync::Arc};

// Keep a useful prefix of malformed-input diagnostics without retaining an unbounded error list.
const MAX_PARSE_DIAGNOSTICS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerError {
    pub message: String,
    pub source_name: String,
    pub span: Option<Range<usize>>,
    pub source: Option<Arc<str>>,
    pub related: Vec<Self>,
}

impl CompilerError {
    pub(crate) fn located(message: impl Into<String>, span: Range<usize>) -> Self {
        Self {
            message: message.into(),
            source_name: String::new(),
            span: Some(span),
            source: None,
            related: Vec::new(),
        }
    }

    pub(crate) fn context(mut self, context: &str) -> Self {
        self.message = format!("{context}: {}", self.message);
        self
    }

    pub(crate) fn append(&mut self, other: Self) {
        let (located, unlocated) = self.diagnostic_counts();
        let mut remaining = (
            MAX_PARSE_DIAGNOSTICS.saturating_sub(located),
            MAX_PARSE_DIAGNOSTICS.saturating_sub(unlocated),
        );
        let mut pending = vec![other];
        while let Some(mut error) = pending.pop() {
            pending.extend(std::mem::take(&mut error.related).into_iter().rev());
            let budget = if error.span.is_some() {
                &mut remaining.0
            } else {
                &mut remaining.1
            };
            if *budget > 0 {
                *budget -= 1;
                self.related.push(error);
            }
        }
    }

    fn diagnostic_counts(&self) -> (usize, usize) {
        let mut counts = if self.span.is_some() { (1, 0) } else { (0, 1) };
        for error in &self.related {
            let (located, unlocated) = error.diagnostic_counts();
            counts.0 += located;
            counts.1 += unlocated;
        }
        counts
    }

    fn limit_diagnostics(&mut self, remaining: &mut usize) {
        *remaining -= 1;
        let mut retained = 0;
        for error in &mut self.related {
            if *remaining == 0 {
                break;
            }
            error.limit_diagnostics(remaining);
            retained += 1;
        }
        self.related.truncate(retained);
    }

    fn set_source(&mut self, source_name: &str, source: &str) {
        self.set_shared_source(source_name, Arc::from(source));
    }

    fn set_shared_source(&mut self, source_name: &str, source: Arc<str>) {
        self.source_name = source_name.to_owned();
        self.source = Some(Arc::clone(&source));
        for error in &mut self.related {
            error.set_shared_source(source_name, Arc::clone(&source));
        }
    }
}

impl From<String> for CompilerError {
    fn from(message: String) -> Self {
        Self {
            message,
            source_name: String::new(),
            span: None,
            source: None,
            related: Vec::new(),
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
        for error in &self.related {
            write!(f, "\n{error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for CompilerError {}

// Original input stays alongside the AST for later file-aware validation.
pub(crate) struct ParsedSource<'a> {
    pub(crate) program: Program,
    pub(crate) source_name: &'a str,
    pub(crate) source: &'a str,
}

pub use blkit_core::{DeadlinePolicy, RetryPolicy};

pub fn transpile(source: &str) -> Result<String, String> {
    transpile_diagnostic(source, "<input>").map_err(|error| error.to_string())
}

pub fn transpile_diagnostic(source: &str, source_name: &str) -> Result<String, CompilerError> {
    let parsed = parse_source(source, source_name)?;
    semantic::validate(&parsed.program).map_err(|error| {
        validation_error(
            error,
            &parsed.program,
            [(parsed.source_name, parsed.source)],
        )
    })?;
    codegen::generate(&parsed.program).map_err(CompilerError::from)
}

pub(crate) fn validation_error<'a>(
    message: String,
    program: &Program,
    sources: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> CompilerError {
    let sources: Vec<_> = sources.into_iter().collect();
    let mut matches = Vec::new();
    for &(name, source) in &sources {
        let tokens = lexer::lex_legacy_quotes(source).tokens;
        let tokens: Vec<_> = tokens
            .iter()
            .filter(|(token, _)| !matches!(token, Token::Newline | Token::Indent))
            .collect();
        let part = |index: usize| -> &str { &source[tokens[index].1.clone()] };
        if let Some(unknown) = message.strip_prefix("unknown type: ") {
            for index in 1..tokens.len() {
                if tokens[index].0 == Token::Identifier
                    && part(index) == unknown
                    && matches!(tokens[index - 1].0, Token::Colon | Token::Less)
                {
                    matches.push((name, tokens[index].1.clone()));
                }
            }
        } else {
            let nodes = message.strip_prefix("binding node outside process: ");
            let port = message
                .strip_prefix("unknown binding output: ")
                .or_else(|| message.strip_prefix("unknown binding input: "));
            if nodes.is_none() && port.is_none() {
                continue;
            }
            for index in 0..tokens.len().saturating_sub(8) {
                if part(index) == "bind"
                    && tokens[index + 2].0 == Token::Dot
                    && tokens[index + 4].0 == Token::Arrow
                    && tokens[index + 6].0 == Token::Dot
                    && tokens[index + 8].0 == Token::Semicolon
                {
                    if let Some(nodes) = nodes {
                        if format!("{} -> {}", part(index + 1), part(index + 5)) == nodes {
                            let graphs: Vec<_> = program
                                .processes
                                .iter()
                                .filter_map(|process| process.source_graph.as_ref())
                                .filter(|graph| {
                                    graph.bindings.iter().any(|binding| {
                                        binding.source == part(index + 1)
                                            && binding.target == part(index + 5)
                                    })
                                })
                                .collect();
                            let span = if let [graph] = graphs.as_slice() {
                                let in_process = |node: &str| {
                                    graph
                                        .flows
                                        .iter()
                                        .any(|(source, target)| source == node || target == node)
                                };
                                match (in_process(part(index + 1)), in_process(part(index + 5))) {
                                    (false, true) => tokens[index + 1].1.clone(),
                                    (true, false) => tokens[index + 5].1.clone(),
                                    _ => tokens[index].1.start..tokens[index + 8].1.end,
                                }
                            } else {
                                tokens[index].1.start..tokens[index + 8].1.end
                            };
                            matches.push((name, span));
                        }
                    } else if let Some(port) = port {
                        let (start, end) = if message.starts_with("unknown binding output:") {
                            (index + 1, index + 3)
                        } else {
                            (index + 5, index + 7)
                        };
                        if format!("{}.{}", part(start), part(end)) == port {
                            matches.push((name, tokens[start].1.start..tokens[end].1.end));
                        }
                    }
                }
            }
        }
    }
    // Repeated matching constructs cannot be assigned a source span with certainty.
    if let [(name, span)] = matches.as_slice() {
        let mut error = CompilerError::located(message, span.clone());
        error.set_source(
            name,
            sources.iter().find(|(path, _)| path == name).unwrap().1,
        );
        error
    } else {
        let mut error = CompilerError::from(message);
        error.source_name = sources
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ");
        error
    }
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
            Self::Generic(name, inner) if name == "Nullable" => write!(f, "{inner}?"),
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
    if let Some(inner) = text.strip_suffix('?') {
        return Ok(Type::Generic("Nullable".into(), Box::new(type_ref(inner)?)));
    }
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
type ProgramParseError<'a> =
    extra::Full<Rich<'a, Token>, chumsky::inspector::RollbackState<usize>, ()>;

enum Declaration {
    Record(Record),
    Enum(Enum),
    Body(String, String, Vec<BodyItem>),
}

fn text(source: &str, span: SimpleSpan) -> &str {
    &source[span.into_range()]
}

pub(crate) fn name<'a, S: chumsky::inspector::Inspector<'a, Tokens<'a>>>(
    source: &'a str,
) -> impl Parser<'a, Tokens<'a>, String, extra::Full<Rich<'a, Token>, S, ()>> + Clone {
    just(Token::Identifier).map_with(move |_, extra| text(source, extra.span()).into())
}

pub(crate) fn word<'a, S: chumsky::inspector::Inspector<'a, Tokens<'a>>>(
    source: &'a str,
    expected: &'static str,
) -> impl Parser<'a, Tokens<'a>, (), extra::Full<Rich<'a, Token>, S, ()>> + Clone {
    just(Token::Identifier)
        .map_with(move |_, extra| text(source, extra.span()))
        .filter(move |value| *value == expected)
        .ignored()
}

pub(crate) fn type_parser<'a, S: chumsky::inspector::Inspector<'a, Tokens<'a>> + 'a>(
    source: &'a str,
) -> impl Parser<'a, Tokens<'a>, Type, extra::Full<Rich<'a, Token>, S, ()>> + Clone {
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
            .then(just(Token::Question).or_not())
            .map(|(ty, nullable)| match nullable {
                Some(_) => Type::Generic("Nullable".into(), Box::new(ty)),
                None => ty,
            })
    })
}

#[derive(Debug)]
pub(crate) enum BodyItem {
    Statement(Vec<(Token, SimpleSpan)>, SimpleSpan),
    Block(String, String, Vec<BodyItem>, SimpleSpan),
    Invalid,
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

type BodyContext = (String, String, SimpleSpan);
type BodyParseError<'a> =
    extra::Full<Rich<'a, Token>, chumsky::inspector::RollbackState<usize>, BodyContext>;

fn body_parser<'a>(
    source: &'a str,
    truncated: bool,
) -> impl Parser<'a, Tokens<'a>, Vec<BodyItem>, BodyParseError<'a>> + Clone {
    recursive(|body| {
        let layout = choice((just(Token::Newline), just(Token::Indent)))
            .repeated()
            .ignored();
        let token = any().map_with(|token, extra| (token, extra.span()));
        let fragment = recursive::<Tokens<'a>, Vec<(Token, SimpleSpan)>, BodyParseError<'a>, _, _>(
            |fragment| {
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
            },
        );
        let statement = fragment
            .repeated()
            .at_least(1)
            .collect::<Vec<_>>()
            .then(just(Token::Semicolon).map_with(|_, extra| extra.span()))
            .map(|(parts, terminator)| {
                let tokens: Vec<_> = parts
                    .into_iter()
                    .flatten()
                    .filter(|(token, _)| !matches!(token, Token::Newline | Token::Indent))
                    .collect();
                // Lexing reports this statement; retain a marker to avoid derived missing fields.
                if tokens.iter().any(|(token, _)| *token == Token::Invalid) {
                    Some(BodyItem::Invalid)
                } else {
                    Some(BodyItem::Statement(tokens, terminator))
                }
            })
            .recover_with(chumsky::recovery::via_parser(
                any()
                    .and_is(choice((just(Token::Semicolon), just(Token::CloseBrace))).not())
                    .repeated()
                    .at_least(1)
                    .then_ignore(choice((
                        just(Token::Semicolon).ignored(),
                        just(Token::CloseBrace).rewind().ignored(),
                    )))
                    .map(|_| None),
            ));
        let nested_kind = just(Token::Identifier)
            .map_with(move |_, extra| (text(source, extra.span()).to_owned(), extra.span()))
            .filter(|(kind, _)| {
                matches!(
                    kind.as_str(),
                    "literal_expression" | "decision_table" | "context" | "knowledge"
                )
            });
        let nested = nested_kind
            .then(
                just(Token::Identifier)
                    .map_with(move |_, extra| text(source, extra.span()).to_owned()),
            )
            .then_ignore(just(Token::OpenBrace))
            .map(|((kind, span), name)| (kind, name, span))
            .then_with_ctx(body)
            .then_ignore(
                just(Token::CloseBrace)
                    .ignored()
                    .or(end().filter(move |_| truncated)),
            )
            .map(|((kind, name, span), body)| Some(BodyItem::Block(kind, name, body, span)));
        layout.ignore_then(
            empty()
                .map(|()| (Vec::new(), Vec::new()))
                .foldl_with(
                    choice((nested, statement)).padded_by(layout).repeated(),
                    move |(mut items, mut columns): (Vec<BodyItem>, Vec<String>), item, extra| {
                        if let Some(item) = item {
                            if extra.state().0 >= MAX_PARSE_DIAGNOSTICS {
                                items.push(BodyItem::Invalid);
                                return (items, columns);
                            }
                            let kind = &extra.ctx().0;
                            let checked = match &item {
                                BodyItem::Statement(tokens, terminator)
                                    if matches!(
                                        kind.as_str(),
                                        "decision_task"
                                            | "literal_expression"
                                            | "decision_table"
                                            | "context"
                                            | "knowledge"
                                    ) =>
                                {
                                    decision::check_statement(
                                        source,
                                        kind,
                                        tokens,
                                        *terminator,
                                        &columns,
                                    )
                                }
                                BodyItem::Statement(tokens, terminator) => {
                                    graph::check_statement(source, tokens, *terminator)
                                        .map_err(|error| {
                                            if kind == "process" {
                                                error.context("invalid process statement")
                                            } else {
                                                error
                                            }
                                        })
                                        .map(|()| None)
                                }
                                _ => Ok(None),
                            };
                            match checked {
                                Ok(column) => {
                                    if let Some(column) = column {
                                        columns.push(column);
                                    }
                                    items.push(item);
                                }
                                Err(error) => {
                                    extra.emit(Rich::custom(
                                        error.span.clone().unwrap().into(),
                                        error.message,
                                    ));
                                    extra.state().0 += 1;
                                    items.push(BodyItem::Invalid);
                                }
                            }
                        }
                        (items, columns)
                    },
                )
                .map(|(items, _)| items),
        )
    })
}

fn program_parser<'a>(
    source: &'a str,
    truncated: bool,
) -> impl Parser<'a, Tokens<'a>, Result<Program, CompilerError>, ProgramParseError<'a>> {
    let newlines = just(Token::Newline).repeated().ignored();
    let outer_layout = choice((just(Token::Newline), just(Token::Indent)))
        .repeated()
        .ignored();
    let namespace = word(source, "namespace")
        .map_with(|_, extra| extra.span())
        .then(name(source).map_with(|value, extra| (value, extra.span())))
        .try_map_with(move |(keyword, (value, span)), extra| {
            let gap = &source[keyword.end..span.start];
            if gap == " " {
                Ok(value)
            } else {
                Err(Rich::custom(extra.span(), "invalid namespace spacing"))
            }
        })
        .then_ignore(just(Token::Semicolon));
    let version = word(source, "version")
        .map_with(|_, extra| extra.span())
        .then(just(Token::String).map_with(move |_, extra| {
            let span = extra.span();
            let value = text(source, span);
            (value[1..value.len() - 1].to_owned(), span)
        }))
        .try_map_with(move |(keyword, (value, span)), extra| {
            let gap = &source[keyword.end..span.start];
            if gap == " " {
                Ok(value)
            } else {
                Err(Rich::custom(extra.span(), "invalid version spacing"))
            }
        })
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
        .ignore_then(
            name(source)
                .then_ignore(just(Token::Semicolon))
                .map(Some)
                .recover_with(chumsky::recovery::via_parser(
                    any()
                        .and_is(choice((just(Token::Semicolon), just(Token::Newline))).not())
                        .repeated()
                        .then_ignore(choice((just(Token::Semicolon), just(Token::Newline))))
                        .to(None),
                )),
        );
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
        .map(|(name, variants)| {
            Declaration::Enum(Enum {
                name,
                variants: variants.into_iter().flatten().collect(),
            })
        });
    let kind = choice((
        word(source, "start_event").to("start_event"),
        word(source, "end_event").to("end_event"),
        word(source, "decision_task").to("decision_task"),
        word(source, "input_request").to("input_request"),
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
        .map_with(|(kind, name), extra| (kind.to_owned(), name, extra.span()))
        .then_with_ctx(body_parser(source, truncated))
        .then_ignore(
            just(Token::CloseBrace)
                .ignored()
                .or(end().filter(move |_| truncated)),
        )
        .map(|((kind, name, _), body)| Declaration::Body(kind, name, body));
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
            let mut errors: Option<CompilerError> = None;
            // Mapping runs after parsing; keep located and unlocated prefixes separately
            // so later source spans can still outrank earlier unlocated failures.
            for declaration in declarations {
                if errors
                    .as_ref()
                    .is_some_and(|error| error.diagnostic_counts().0 >= MAX_PARSE_DIAGNOSTICS)
                {
                    break;
                }
                match declaration {
                    Declaration::Record(record) => program.records.push(record),
                    Declaration::Enum(item) => program.enums.push(item),
                    Declaration::Body(kind, name, body) => match kind.as_str() {
                        "decision_task" => match decision::parse_tokens(source, &name, &body) {
                            Ok(Some(item)) => program.decisions.push(item),
                            Ok(None) => {}
                            Err(error) => {
                                if let Some(errors) = &mut errors {
                                    errors.append(error);
                                } else {
                                    errors = Some(error);
                                }
                            }
                        },
                        "process" => match graph::parse_source_tokens(source, &body) {
                            Ok(Some((graph, retry, deadline))) => program.processes.push(Process {
                                name,
                                input: String::new(),
                                input_type: Type::Named("Number".into()),
                                output: Type::Named("Number".into()),
                                body: Vec::new(),
                                named_graph: None,
                                source_graph: Some(graph),
                                retry,
                                deadline,
                            }),
                            Ok(None) => {}
                            Err(error) => {
                                if let Some(errors) = &mut errors {
                                    errors.append(error);
                                } else {
                                    errors = Some(error);
                                }
                            }
                        },
                        _ => match graph::parse_peer_tokens(source, &kind, &name, &body) {
                            Ok(Some(item)) => program.peer_nodes.push(item),
                            Ok(None) => {}
                            Err(error) => {
                                let error = if kind == "input_request" {
                                    error.context(&format!("input_request {name}"))
                                } else {
                                    error
                                };
                                if let Some(errors) = &mut errors {
                                    errors.append(error);
                                } else {
                                    errors = Some(error);
                                }
                            }
                        },
                    },
                }
            }
            errors.map_or(Ok(program), Err)
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
            let mut remaining = MAX_PARSE_DIAGNOSTICS;
            error.limit_diagnostics(&mut remaining);
            error.set_source(source_name, source);
            error
        })
}

fn parse_source_inner(source: &str) -> Result<Program, CompilerError> {
    let lexed = lexer::lex_legacy_quotes(source);
    let mut errors: Vec<_> = lexed
        .tokens
        .iter()
        .filter(|(token, _)| *token == Token::Invalid)
        .take(MAX_PARSE_DIAGNOSTICS)
        .map(|(_, span)| CompilerError::located("invalid character", span.clone()))
        .collect();
    // The 32nd lexical error is retained, but its token cannot complete a statement.
    let eof = if errors.len() == MAX_PARSE_DIAGNOSTICS {
        errors
            .last()
            .and_then(|error| error.span.as_ref())
            .expect("lexical errors have spans")
            .start
    } else {
        lexed.eof
    };
    let tokens: Vec<_> = lexed
        .tokens
        .into_iter()
        .take_while(|(_, span)| span.start < eof)
        .map(|(token, span)| (token, span.into()))
        .collect();
    let (output, syntax_errors) = program_parser(source, eof != source.len())
        .parse(tokens.as_slice().split_token_span((eof..eof).into()))
        .into_output_errors();
    let lexical_spans: Vec<_> = errors
        .iter()
        .filter_map(|error| error.span.clone())
        .collect();
    errors.extend(
        syntax_errors
            .iter()
            .filter(|error| {
                let at = error.span().start;
                if eof != source.len() && at >= eof {
                    return false; // A truncated prefix has no genuine EOF diagnostic.
                }
                // A boundary error after invalid text is a cascade until a previous boundary intervenes.
                let boundary = tokens.iter().any(|(token, span)| {
                    span.start == at
                        && matches!(token, Token::Semicolon | Token::CloseBrace | Token::Newline)
                });
                !lexical_spans.iter().any(|span| {
                    (span.start <= at && at < span.end)
                        || (boundary
                            && span.end <= at
                            && !tokens.iter().any(|(token, location)| {
                                location.start >= span.end
                                    && location.start < at
                                    && matches!(
                                        token,
                                        Token::Semicolon | Token::CloseBrace | Token::Newline
                                    )
                            }))
                })
            })
            .take(MAX_PARSE_DIAGNOSTICS)
            .map(|error| {
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
                } else if tokens.iter().any(|(token, location)| {
                    *token == Token::OpenParen && location.start == span.start
                }) {
                    "invalid declaration name"
                } else {
                    "invalid declaration or statement"
                };
                CompilerError::located(format!("{prefix}: {error:?}"), span)
            }),
    );
    if let Some(Err(error)) = output.as_ref() {
        let mut mapped = error.clone();
        let related = std::mem::take(&mut mapped.related);
        if mapped
            .span
            .as_ref()
            .is_none_or(|span| span.start < eof || eof == source.len())
        {
            errors.push(mapped);
        }
        for error in related {
            if error
                .span
                .as_ref()
                .is_none_or(|span| span.start < eof || eof == source.len())
            {
                errors.push(error);
            }
        }
    }
    if !errors.is_empty() {
        errors.sort_by_key(|error| error.span.as_ref().map_or(usize::MAX, |span| span.start));
        errors.truncate(MAX_PARSE_DIAGNOSTICS);
        let mut first = errors.remove(0);
        for error in errors {
            first.append(error);
        }
        return Err(first);
    }
    output.expect("Chumsky returns either a program or parse errors")
}

#[cfg(test)]
mod diagnostic_tests {
    use super::parse_source;
    use chumsky::{Parser as _, input::Input as _};

    #[test]
    fn early_syntax_error_precedes_32_later_invalid_tokens_without_emitting_rust() {
        let source = format!(
            "namespace demo; version \"1.0\"; start_event start {{ output first: ; }}{}",
            "☃;".repeat(32)
        );
        let error = parse_source(&source, "mixed.bl").err().unwrap();
        let first = source.find("first: ;").unwrap() + "first: ".len();
        let snowman = source.find('☃').unwrap();
        let diagnostics: Vec<_> = std::iter::once(&error).chain(&error.related).collect();
        assert_eq!(diagnostics.len(), 32, "{error}");
        assert_eq!(error.span, Some(first..first), "{error}");
        assert!(error.message.contains("semicolon"), "{error}");
        assert_eq!(diagnostics[1].span, Some(snowman..snowman + '☃'.len_utf8()));
        assert!(
            diagnostics
                .iter()
                .all(|error| error.source_name == "mixed.bl")
        );
        assert!(super::transpile(&source).is_err());
    }

    #[test]
    fn malformed_statement_before_first_invalid_in_same_declaration_survives_lexical_cap() {
        let source = format!(
            "namespace demo; version \"1.0\"; start_event start {{ output first: ; output broken: ☃; }}{}",
            "☃;".repeat(31)
        );
        let error = parse_source(&source, "mixed.bl").err().unwrap();
        let first = source.find("first: ;").unwrap() + "first: ".len();
        let snowman = source.find('☃').unwrap();
        let diagnostics: Vec<_> = std::iter::once(&error).chain(&error.related).collect();
        assert_eq!(diagnostics.len(), 32, "{error}");
        assert_eq!(error.span, Some(first..first), "{error}");
        assert!(error.message.contains("semicolon"), "{error}");
        assert_eq!(diagnostics[1].span, Some(snowman..snowman + '☃'.len_utf8()));
        assert!(
            diagnostics
                .iter()
                .all(|error| error.source_name == "mixed.bl")
        );
        assert!(
            !diagnostics
                .iter()
                .any(|error| error.message.contains("unclosed brace"))
        );
        assert!(super::transpile(&source).is_err());
    }

    #[test]
    fn malformed_statement_survives_32nd_invalid_inside_declaration() {
        let source = format!(
            "namespace demo; version \"1\"; start_event start {{ output first: ; {} }}",
            "☃;".repeat(32)
        );
        let error = parse_source(&source, "mixed.bl").err().unwrap();
        let diagnostics: Vec<_> = std::iter::once(&error).chain(&error.related).collect();
        let first = source.find("first: ;").unwrap() + "first: ".len();
        assert_eq!(diagnostics.len(), 32, "{error}");
        assert_eq!(error.span, Some(first..first), "{error}");
        assert!(error.message.contains("semicolon"), "{error}");
        assert_eq!(diagnostics[1].span, source.find('☃').map(|at| at..at + 3));
        assert!(
            !diagnostics
                .iter()
                .any(|item| item.message.contains("unclosed brace"))
        );
        assert!(super::transpile(&source).is_err());
    }

    #[test]
    fn malformed_decision_table_rule_survives_32nd_invalid_inside_block() {
        let source = format!(
            "namespace demo; version \"1\"; decision_task check {{ input amount: Number; output result: Number = table; decision_table table {{ output value: Number; policy FIRST; input amount: Number = amount; rule amount -> ; {} }} }}",
            "☃;".repeat(32)
        );
        let error = parse_source(&source, "decision.bl").err().unwrap();
        let diagnostics: Vec<_> = std::iter::once(&error).chain(&error.related).collect();
        let first = source.find("rule amount -> ;").unwrap() + "rule amount -> ".len();
        assert_eq!(diagnostics.len(), 32, "{error}");
        assert_eq!(error.span, Some(first..first), "{error}");
        assert!(error.message.contains("statement"), "{error}");
        assert_eq!(diagnostics[1].span, source.find('☃').map(|at| at..at + 3));
        assert!(super::transpile(&source).is_err());
    }

    #[test]
    fn truncated_lexical_prefix_does_not_invent_an_eof_error() {
        let source = format!(
            "namespace demo; version \"1\"; start_event start {{ output amount: {} ; }}",
            "☃".repeat(10_000)
        );
        let error = parse_source(&source, "prefix.bl").err().unwrap();
        let diagnostics: Vec<_> = std::iter::once(&error).chain(&error.related).collect();
        assert_eq!(diagnostics.len(), 32, "{error}");
        assert!(
            diagnostics
                .iter()
                .all(|error| error.message == "invalid character"),
            "{error}"
        );
        assert!(super::transpile(&source).is_err());
    }

    #[test]
    fn many_invalid_graph_statements_keep_at_most_32_diagnostics() {
        let source = format!(
            "namespace demo; version \"1\"; {}",
            (0..80)
                .map(|index| format!("pause_for wait{index} {{ duration \"bogus\"; }}"))
                .collect::<String>()
        );
        let error = parse_source(&source, "graph.bl").err().unwrap();
        fn count(error: &super::CompilerError) -> usize {
            1 + error.related.iter().map(count).sum::<usize>()
        }
        assert_eq!(count(&error), 32, "{error}");
        assert!(super::transpile(&source).is_err());
    }

    #[test]
    fn unlocated_declarations_keep_authored_order_and_source_without_rust() {
        let source = "namespace demo; version \"1\"; pause_for wait { } pause_until later { }";
        let error = parse_source(source, "ordered.bl").err().unwrap();
        assert_eq!(error.message, "pause_for requires duration");
        assert_eq!(error.related.len(), 1);
        assert_eq!(
            error.related[0].message,
            "pause_until requires one DateTime input"
        );
        for diagnostic in std::iter::once(&error).chain(&error.related) {
            assert_eq!(diagnostic.source_name, "ordered.bl");
            assert_eq!(diagnostic.span, None);
            assert_eq!(diagnostic.source.as_deref(), Some(source));
        }
        assert!(super::transpile(source).is_err());
    }

    #[test]
    fn later_located_declaration_error_survives_many_unlocated_errors() {
        let source = format!(
            "namespace demo; version \"1\"; {}pause_for late {{ duration \"bogus\"; }}",
            (0..1000)
                .map(|index| format!("pause_for wait{index} {{ }} "))
                .collect::<String>()
        );
        let quoted = source.find("\"bogus\"").unwrap();
        let lexed = crate::lexer::lex_legacy_quotes(&source);
        let tokens: Vec<_> = lexed
            .tokens
            .into_iter()
            .map(|(token, span)| (token, span.into()))
            .collect();
        let (output, _) = super::program_parser(&source, false)
            .parse(
                tokens
                    .as_slice()
                    .split_token_span((lexed.eof..lexed.eof).into()),
            )
            .into_output_errors();
        let mapped = output.unwrap().err().unwrap();
        assert_eq!(mapped.diagnostic_counts(), (1, 32));
        let error = parse_source(&source, "late.bl").err().unwrap();
        let diagnostics: Vec<_> = std::iter::once(&error).chain(&error.related).collect();
        assert_eq!(diagnostics.len(), 32);
        assert_eq!(error.span, Some(quoted..quoted + "\"bogus\"".len()));
        assert!(error.message.contains("invalid retry duration"), "{error}");
        assert!(diagnostics.iter().all(|item| item.source_name == "late.bl"));
        assert!(diagnostics[1..].iter().all(|item| item.span.is_none()));
        assert!(super::transpile(&source).is_err());
    }

    #[test]
    fn located_error_in_one_declaration_survives_unlocated_inner_mapping_errors() {
        let source = format!(
            "namespace demo; version \"1\"; decision_task check {{ {} literal_expression node {{ context nested {{ }} }} }}",
            "output result: Number;".repeat(40)
        );
        let nested = source.find("context nested").unwrap();
        let error = parse_source(&source, "decision.bl").err().unwrap();
        assert_eq!(
            error.span,
            Some(nested..nested + "context".len()),
            "{error}"
        );
        assert_eq!(error.related.len(), 31);
        assert!(error.related.iter().all(|item| item.span.is_none()));
        assert!(super::transpile(&source).is_err());
    }

    #[test]
    fn many_invalid_declarations_bound_chumsky_errors_before_mapping() {
        let source = format!(
            "namespace demo; version \"1\"; {}",
            (0..100_000)
                .map(|index| format!("pause_for wait{index} {{ duration \"bogus\"; }}"))
                .collect::<String>()
        );
        let lexed = crate::lexer::lex_legacy_quotes(&source);
        assert!(
            !lexed
                .tokens
                .iter()
                .any(|(token, _)| *token == crate::lexer::Token::Invalid)
        );
        let tokens: Vec<_> = lexed
            .tokens
            .into_iter()
            .map(|(token, span)| (token, span.into()))
            .collect();
        let (output, emitted) = super::program_parser(&source, false)
            .parse(
                tokens
                    .as_slice()
                    .split_token_span((lexed.eof..lexed.eof).into()),
            )
            .into_output_errors();
        assert!(
            emitted.len() <= 32,
            "emitted {} diagnostics before mapping",
            emitted.len()
        );
        let mapped = output.unwrap().err().unwrap();
        assert_eq!(
            mapped.related.len(),
            31,
            "mapped diagnostics before truncation"
        );
        let error = parse_source(&source, "graph.bl").err().unwrap();
        let diagnostics: Vec<_> = std::iter::once(&error).chain(&error.related).collect();
        assert_eq!(diagnostics.len(), 32);
        for (index, diagnostic) in diagnostics.iter().enumerate() {
            let byte = source.find(&format!("wait{index}")).unwrap()
                + format!("wait{index} {{ duration ").len();
            assert_eq!(diagnostic.span.as_ref().map(|span| span.start), Some(byte));
        }
        assert!(super::transpile(&source).is_err());
    }

    #[test]
    fn many_invalid_body_statements_bound_chumsky_emission() {
        let source = format!(
            "namespace demo; version \"1\"; start_event start {{ {} }}",
            "output bad: ;".repeat(1000)
        );
        let lexed = crate::lexer::lex_legacy_quotes(&source);
        let tokens: Vec<_> = lexed
            .tokens
            .into_iter()
            .map(|(token, span)| (token, span.into()))
            .collect();
        let (_, emitted) = super::program_parser(&source, false)
            .parse(
                tokens
                    .as_slice()
                    .split_token_span((lexed.eof..lexed.eof).into()),
            )
            .into_output_errors();
        assert_eq!(emitted.len(), 32, "emitted {} diagnostics", emitted.len());
        let error = parse_source(&source, "body.bl").err().unwrap();
        assert_eq!(error.related.len(), 31);
        assert_eq!(
            error.span.as_ref().map(|span| span.start),
            source.find("bad: ;").map(|at| at + 5)
        );
        assert!(super::transpile(&source).is_err());
    }

    #[test]
    fn earlier_declaration_errors_survive_later_full_body_budget() {
        let source = format!(
            "namespace demo; version \"1\"; {} start_event start {{ {} }}",
            (0..20)
                .map(|index| format!("pause_for wait{index} {{ duration \"bogus\"; }}"))
                .collect::<String>(),
            "output bad: ;".repeat(40)
        );
        let error = parse_source(&source, "ordered.bl").err().unwrap();
        let diagnostics: Vec<_> = std::iter::once(&error).chain(&error.related).collect();
        assert_eq!(diagnostics.len(), 32);
        for (index, diagnostic) in diagnostics.iter().enumerate().take(20) {
            let byte = source.find(&format!("wait{index}")).unwrap()
                + format!("wait{index} {{ duration ").len();
            assert_eq!(diagnostic.span.as_ref().map(|span| span.start), Some(byte));
        }
        let first_body = source.find("bad: ;").unwrap() + "bad: ".len();
        assert_eq!(
            diagnostics[20].span.as_ref().map(|span| span.start),
            Some(first_body)
        );
        assert!(
            diagnostics
                .iter()
                .all(|item| item.source_name == "ordered.bl")
        );
        assert!(super::transpile(&source).is_err());
    }

    #[test]
    fn many_nested_blocks_bound_declaration_mapping_errors() {
        let source = format!(
            "namespace demo; version \"1\"; start_event start {{ {} }}",
            (0..1000)
                .map(|index| format!("literal_expression block{index} {{ }}"))
                .collect::<String>()
        );
        let lexed = crate::lexer::lex_legacy_quotes(&source);
        let tokens: Vec<_> = lexed
            .tokens
            .into_iter()
            .map(|(token, span)| (token, span.into()))
            .collect();
        let (output, emitted) = super::program_parser(&source, false)
            .parse(
                tokens
                    .as_slice()
                    .split_token_span((lexed.eof..lexed.eof).into()),
            )
            .into_output_errors();
        assert!(emitted.is_empty());
        let mapped = output.unwrap().err().unwrap();
        assert_eq!(mapped.related.len(), 31);
        let error = parse_source(&source, "blocks.bl").err().unwrap();
        assert_eq!(error.related.len(), 31);
        assert_eq!(
            error.span.as_ref().map(|span| span.start),
            source.find("literal_expression")
        );
        assert!(super::transpile(&source).is_err());
    }

    #[test]
    fn mixed_lexical_body_and_declaration_errors_keep_earliest_original_bytes() {
        let source = "namespace demo; version \"1\"; start_event start { output bad: ; } pause_for wait { duration ☃; duration \"bogus\"; } start_event end { output other: ; }";
        let error = parse_source(&source, "mixed.bl").err().unwrap();
        let diagnostics: Vec<_> = std::iter::once(&error).chain(&error.related).collect();
        let bad = source.find("bad: ;").unwrap() + "bad: ".len();
        let invalid = source.find('☃').unwrap();
        let duration = source.find("\"bogus\"").unwrap();
        let other = source.find("other: ;").unwrap() + "other: ".len();
        assert_eq!(
            diagnostics
                .iter()
                .map(|item| item.span.as_ref().unwrap().start)
                .collect::<Vec<_>>(),
            vec![bad, invalid, duration, other]
        );
        assert!(
            diagnostics
                .iter()
                .all(|item| item.source_name == "mixed.bl")
        );
        assert!(super::transpile(&source).is_err());
    }

    #[test]
    fn structured_compiler_error_propagates_as_standard_error() {
        fn boxed() -> Result<(), Box<dyn std::error::Error>> {
            super::transpile_diagnostic("☃", "invalid.bl")?;
            Ok(())
        }
        assert!(boxed().is_err());
    }

    #[test]
    fn many_invalid_tokens_keep_first_32_bytes_and_share_source_without_emitting_rust() {
        let source = format!(
            "namespace demo; version \"1\";\n# café\n{}",
            "☃;".repeat(300)
        );
        let error = parse_source(&source, "many.bl").err().unwrap();
        let errors = std::iter::once(&error).chain(error.related.iter());
        let first = source.find('☃').unwrap();
        let source_ptr = error.source.as_ref().unwrap().as_ptr();
        assert_eq!(error.related.len(), 31);
        for (index, error) in errors.enumerate() {
            let start = first + index * "☃;".len();
            assert_eq!(error.span, Some(start..start + '☃'.len_utf8()));
            assert_eq!(error.source_name, "many.bl");
            assert_eq!(error.source.as_ref().unwrap().as_ptr(), source_ptr);
        }
        assert!(super::transpile(&source).is_err());
    }

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
    fn independent_malformed_statements_report_once_each_and_emit_no_rust() {
        let source = "namespace demo; version \"1.0\";\n# café\nstart_event start { output first: ; output second: ; }";
        let error = parse_source(source, "twice.bl").err().unwrap().to_string();
        for name in ["first", "second"] {
            let at = source.find(&format!("{name}: ;")).unwrap() + name.len() + 2;
            assert_eq!(error.matches(&format!("byte {at}")).count(), 1, "{error}");
        }
        assert!(error.contains("twice.bl"), "{error}");
        assert!(super::transpile(source).is_err());
    }

    #[test]
    fn recovery_stops_at_brace_and_enum_variant_boundaries() {
        let source = "namespace demo; version \"1.0\";\nenum State:\n  123;\n  ready;\nstart_event start { output amount: Number; }\nstart_event other { output missing: ; }";
        let error = parse_source(source, "boundaries.bl")
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains(&format!("byte {}", source.find("123").unwrap())),
            "{error}"
        );
        assert!(
            error.contains(&format!(
                "byte {}",
                source.find("missing: ;").unwrap() + "missing: ".len()
            )),
            "{error}"
        );
        assert!(super::transpile(source).is_err());
    }

    #[test]
    fn enum_variant_missing_semicolon_recovers_at_newline() {
        let source = "namespace demo; version \"1.0\";\nenum State:\n  broken\n  ready;\nstart_event start { output missing: ; }";
        let error = parse_source(source, "variants.bl")
            .err()
            .unwrap()
            .to_string();
        assert_eq!(
            error
                .matches(&format!("byte {}", source.find("\n  ready").unwrap()))
                .count(),
            1,
            "{error}"
        );
        assert_eq!(
            error
                .matches(&format!(
                    "byte {}",
                    source.find("missing: ;").unwrap() + "missing: ".len()
                ))
                .count(),
            1,
            "{error}"
        );
        assert!(super::transpile(source).is_err());
    }

    #[test]
    fn malformed_decision_statements_report_both_original_bytes() {
        let source = "namespace demo; version \"1.0\"; decision_task check { input value: Number; output result: Number = node; literal_expression node { output result: Number; expression value + ; expression value * ; } }";
        let error = parse_source(source, "decision.bl")
            .err()
            .unwrap()
            .to_string();
        for part in ["+ ;", "* ;"] {
            let at = source.find(part).unwrap() + 2;
            assert_eq!(error.matches(&format!("byte {at}")).count(), 1, "{error}");
        }
        assert!(super::transpile(source).is_err());
    }

    #[test]
    fn missing_semicolon_recovers_at_brace_without_hiding_next_declaration() {
        let source = "namespace demo; version \"1.0\";\nstart_event start { output amount: Number }\nstart_event other { output missing: ; }";
        let error = parse_source(source, "braces.bl").err().unwrap().to_string();
        assert!(
            error.contains(&format!("byte {}", source.find("}\nstart_event").unwrap())),
            "{error}"
        );
        assert!(
            error.contains(&format!(
                "byte {}",
                source.find("missing: ;").unwrap() + "missing: ".len()
            )),
            "{error}"
        );
        assert!(super::transpile(source).is_err());
    }

    #[test]
    fn lexical_error_recovers_at_semicolon_without_duplicate_syntax_error() {
        let source = "namespace demo; version \"1.0\";\n# café\nstart_event start { output first: ☃; output second: ; }";
        let error = parse_source(source, "mixed.bl").err().unwrap();
        let snowman = source.find('☃').unwrap();
        let second = source.find("second: ;").unwrap() + "second: ".len();
        assert_eq!(error.span, Some(snowman..snowman + '☃'.len_utf8()));
        let lines: Vec<_> = error.to_string().lines().map(str::to_owned).collect();
        assert_eq!(lines.len(), 2, "{error}");
        assert!(
            lines[0].contains(&format!("invalid character at byte {snowman}")),
            "{error}"
        );
        assert!(lines[1].contains(&format!("at byte {second}")), "{error}");
        assert_eq!(error.related[0].source_name, "mixed.bl");
        assert_eq!(error.related[0].span, Some(second..second));
        assert!(super::transpile(source).is_err());
    }

    #[test]
    fn invalid_required_decision_expression_has_no_missing_field_cascade_but_keeps_later_error() {
        let source = "namespace demo; version \"1.0\";\n# café\ndecision_task check { input value: Number; output result: Number = node; literal_expression node { output result: Number; expression ☃; expression value + ; } }";
        let error = parse_source(source, "decision.bl").err().unwrap();
        let invalid = source.find('☃').unwrap();
        let later = source.find("+ ;").unwrap() + 2;
        assert_eq!(error.span, Some(invalid..invalid + '☃'.len_utf8()));
        let lines: Vec<_> = error.to_string().lines().map(str::to_owned).collect();
        assert_eq!(lines.len(), 2, "{error}");
        assert!(
            lines[0].contains(&format!("invalid character at byte {invalid}")),
            "{error}"
        );
        assert!(lines[1].contains(&format!("expression")), "{error}");
        assert!(lines[1].ends_with(&format!("at byte {later}")), "{error}");
        assert!(super::transpile(source).is_err());
    }

    #[test]
    fn invalid_required_decision_expression_has_only_lexical_diagnostic() {
        let source = "namespace demo; version \"1.0\"; decision_task check { input value: Number; output result: Number = node; literal_expression node { output result: Number; expression ☃; } }";
        let error = parse_source(source, "decision.bl").err().unwrap();
        let invalid = source.find('☃').unwrap();
        assert_eq!(error.span, Some(invalid..invalid + '☃'.len_utf8()));
        assert_eq!(error.to_string().lines().count(), 1, "{error}");
        assert!(super::transpile(source).is_err());
    }

    #[test]
    fn invalid_required_graph_statement_has_only_lexical_diagnostic() {
        let source = "namespace demo; version \"1.0\"; pause_for wait { duration ☃; }";
        let error = parse_source(source, "graph.bl").err().unwrap();
        let invalid = source.find('☃').unwrap();
        assert_eq!(error.span, Some(invalid..invalid + '☃'.len_utf8()));
        assert_eq!(error.to_string().lines().count(), 1, "{error}");
        assert!(super::transpile(source).is_err());
    }

    #[test]
    fn invalid_graph_duration_after_lexical_error_reports_both_without_missing_duration() {
        let source = "namespace demo; version \"1.0\";\n# café\npause_for wait { duration ☃; duration \"bogus\"; }";
        let error = parse_source(source, "graph.bl").err().unwrap();
        let invalid = source.find('☃').unwrap();
        let duration = source.find("\"bogus\"").unwrap();
        assert_eq!(error.span, Some(invalid..invalid + '☃'.len_utf8()));
        let lines: Vec<_> = error.to_string().lines().map(str::to_owned).collect();
        assert_eq!(lines.len(), 2, "{error}");
        assert!(
            lines[0].ends_with(&format!("invalid character at byte {invalid}")),
            "{error}"
        );
        assert!(lines[1].contains("invalid retry duration"), "{error}");
        assert!(
            lines[1].ends_with(&format!("at byte {duration}")),
            "{error}"
        );
        assert_eq!(
            error.related[0].span,
            Some(duration..duration + "\"bogus\"".len())
        );
        assert_eq!(error.related[0].source_name, "graph.bl");
        assert!(super::transpile(source).is_err());
    }

    #[test]
    fn invalid_process_deadline_after_lexical_retry_reports_both_original_bytes() {
        let source = "namespace demo; version \"1.0\";\n# café\nprocess route { retry max_retries ☃ retry_for \"1s\" retry_delay \"1s\" backoff exponential; deadline queued \"bogus\"; }";
        let error = parse_source(source, "graph.bl").err().unwrap();
        let invalid = source.find('☃').unwrap();
        let deadline = source.find("\"bogus\"").unwrap();
        assert_eq!(error.span, Some(invalid..invalid + '☃'.len_utf8()));
        let lines: Vec<_> = error.to_string().lines().map(str::to_owned).collect();
        assert_eq!(lines.len(), 2, "{error}");
        assert!(
            lines[0].ends_with(&format!("invalid character at byte {invalid}")),
            "{error}"
        );
        assert!(lines[1].contains("invalid retry duration"), "{error}");
        assert!(
            lines[1].ends_with(&format!("at byte {deadline}")),
            "{error}"
        );
        assert_eq!(
            error.related[0].span,
            Some(deadline..deadline + "\"bogus\"".len())
        );
        assert_eq!(error.related[0].source_name, "graph.bl");
        assert!(super::transpile(source).is_err());
    }

    #[test]
    fn invalid_token_before_brace_has_no_second_syntax_diagnostic() {
        let source = "namespace demo; version \"1.0\"; start_event start { output first: ☃ }";
        let error = parse_source(source, "single.bl").err().unwrap();
        assert_eq!(
            error.span,
            Some(source.find('☃').unwrap()..source.find('☃').unwrap() + '☃'.len_utf8())
        );
        assert_eq!(error.to_string().lines().count(), 1, "{error}");
        assert!(super::transpile(source).is_err());
    }

    #[test]
    fn aggregated_diagnostics_keep_each_message_with_its_own_span() {
        let source = "namespace demo; version \"1.0\"; start_event start { output first: ; output second: ; }";
        let error = parse_source(source, "pairs.bl").err().unwrap();
        let first = source.find("first: ;").unwrap() + "first: ".len();
        let second = source.find("second: ;").unwrap() + "second: ".len();
        let lines: Vec<_> = error.to_string().lines().map(str::to_owned).collect();
        assert_eq!(lines.len(), 2, "{error}");
        assert!(lines[0].ends_with(&format!("at byte {first}")), "{error}");
        assert!(lines[1].ends_with(&format!("at byte {second}")), "{error}");
        assert_eq!(error.span, Some(first..first));
        assert_eq!(error.source_name, "pairs.bl");
        assert_eq!(error.related.len(), 1);
        assert_eq!(error.related[0].source_name, "pairs.bl");
        assert_eq!(error.related[0].span, Some(second..second));
    }

    #[test]
    fn direct_unknown_type_identifies_original_utf8_type_byte_without_rust() {
        let source = "namespace demo; version \"1\";\n# café\nstart_event start { output amount: List<Missing>; }";
        let error = super::transpile(source).unwrap_err();
        let byte = source.find("Missing").unwrap();
        assert!(error.contains("unknown type: Missing"), "{error}");
        assert!(error.ends_with(&format!("at byte {byte}")), "{error}");
    }

    #[test]
    fn direct_invalid_binding_identifies_original_byte_without_rust() {
        let source = "namespace demo; version \"1\"; start_event start { output amount: Number; } end_event done { input result: Number; } process route { flow start -> done; bind start.missing -> done.result; }";
        let error = super::transpile(source).unwrap_err();
        let byte = source.find("start.missing").unwrap();
        assert!(
            error.contains("unknown binding output: start.missing"),
            "{error}"
        );
        assert!(error.ends_with(&format!("at byte {byte}")), "{error}");
    }

    #[test]
    fn direct_binding_outside_process_locates_original_node_reference_without_rust() {
        let source = "namespace demo; version \"1\";\n# café\nstart_event start { output amount: Number; } end_event done { input result: Number; } end_event spare { input result: Number; } process route { flow start -> done; bind start.amount -> spare.result; }";
        let error = super::transpile(source).unwrap_err();
        let byte = source.find("spare.result").unwrap();
        assert!(
            error.contains("binding node outside process: start -> spare"),
            "{error}"
        );
        assert!(error.ends_with(&format!("at byte {byte}")), "{error}");
    }

    #[test]
    fn direct_binding_outside_process_locates_source_node_without_rust() {
        let source = "namespace demo; version \"1\";\n# café\nstart_event start { output amount: Number; } end_event done { input result: Number; } end_event spare { input result: Number; } process route { flow start -> done; bind spare.result -> done.result; }";
        let error = super::transpile(source).unwrap_err();
        let byte = source.find("spare.result").unwrap();
        assert!(
            error.contains("binding node outside process: spare -> done"),
            "{error}"
        );
        assert!(error.ends_with(&format!("at byte {byte}")), "{error}");
    }

    #[test]
    fn binding_with_both_nodes_outside_process_locates_the_bind_statement() {
        let source = "namespace demo; version \"1\"; start_event start { output amount: Number; } end_event done { input result: Number; } end_event spare { input result: Number; } end_event other { input result: Number; } process route { flow start -> done; bind spare.result -> other.result; }";
        let error = super::transpile(source).unwrap_err();
        assert!(
            error.contains("binding node outside process: spare -> other"),
            "{error}"
        );
        assert!(
            error.ends_with(&format!("at byte {}", source.find("bind spare").unwrap())),
            "{error}"
        );
    }

    #[test]
    fn repeated_binding_node_pair_does_not_guess_a_byte() {
        let source = "namespace demo; version \"1\"; start_event start { output amount: Number; } end_event done { input result: Number; } end_event spare { input result: Number; } process route { flow start -> done; bind start.amount -> spare.result; bind start.other -> spare.result; }";
        let error = super::transpile(source).unwrap_err();
        assert!(
            error.contains("binding node outside process: start -> spare"),
            "{error}"
        );
        assert!(!error.contains("at byte"), "{error}");
    }

    #[test]
    fn repeated_unknown_type_does_not_guess_a_byte() {
        let source = "namespace demo; version \"1\"; start_event start { output one: Missing; output two: Missing; }";
        let error = super::transpile(source).unwrap_err();
        assert!(error.contains("unknown type: Missing"), "{error}");
        assert!(!error.contains("at byte"), "{error}");
    }

    #[test]
    fn global_validation_failure_has_no_fabricated_byte() {
        let source = "namespace demo; version \"1\"; process route { }";
        let error = super::transpile(source).unwrap_err();
        assert!(error.contains("start"), "{error}");
        assert!(!error.contains("at byte"), "{error}");
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
