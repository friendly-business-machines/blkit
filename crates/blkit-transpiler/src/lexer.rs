use logos::Logos;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Logos)]
pub(crate) enum Token {
    #[regex(r"[a-zA-Z_][a-zA-Z_0-9]*")]
    Identifier,
    #[regex(r"[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?")]
    Number,
    #[regex(r#""([^"\\\r\n]|\\[^\r\n])*""#)]
    String,
    #[token("{")]
    OpenBrace,
    #[token("}")]
    CloseBrace,
    #[token("(")]
    OpenParen,
    #[token(")")]
    CloseParen,
    #[token("[")]
    OpenBracket,
    #[token("]")]
    CloseBracket,
    #[token(":")]
    Colon,
    #[token(";")]
    Semicolon,
    #[token(",")]
    Comma,
    #[token(".")]
    Dot,
    #[token("..")]
    Range,
    #[token("->")]
    Arrow,
    #[token("==")]
    EqualEqual,
    #[token("!=")]
    NotEqual,
    #[token("<=")]
    LessEqual,
    #[token(">=")]
    GreaterEqual,
    #[token("**")]
    Power,
    #[token("=")]
    Equal,
    #[token("<")]
    Less,
    #[token(">")]
    Greater,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[regex(r"\r\n|\n|\r")]
    Newline,
    #[regex(r"[^\S\r\n]+")]
    Space,
    #[regex(r"#[^\r\n]*", allow_greedy = true)]
    Comment,
    Indent,
    Invalid,
}

pub(crate) struct Lexed {
    pub(crate) tokens: Vec<(Token, Range<usize>)>,
    pub(crate) eof: usize,
    pub(crate) has_full_line_comment: bool,
}

pub(crate) fn lex(source: &str) -> Lexed {
    let mut tokens = Vec::new();
    let mut at_line_start = true;
    let mut indent = None;
    let mut has_full_line_comment = false;
    let mut lexer = Token::lexer(source);
    while let Some(token) = lexer.next() {
        let span = lexer.span();
        match token.unwrap_or(Token::Invalid) {
            Token::Newline => {
                indent = None;
                at_line_start = true;
                tokens.push((Token::Newline, span));
            }
            Token::Space => {
                if at_line_start {
                    indent = Some(span);
                }
            }
            Token::Comment if at_line_start => {
                has_full_line_comment = true;
                indent = None;
            }
            token => {
                if let Some(indent) = indent.take() {
                    tokens.push((Token::Indent, indent));
                }
                tokens.push((
                    if token == Token::Comment {
                        Token::Invalid
                    } else {
                        token
                    },
                    span,
                ));
                at_line_start = false;
            }
        }
    }
    Lexed {
        tokens,
        eof: source.len(),
        has_full_line_comment,
    }
}

// Match the pre-existing source grammar: a backslash does not escape its next quote.
// Relex the remaining original source so both file and expression parsers see the same spans.
pub(crate) fn lex_legacy_quotes(source: &str) -> Lexed {
    let mut tokens = Vec::new();
    let mut offset = 0;
    let mut has_full_line_comment = false;
    loop {
        let lexed = lex(&source[offset..]);
        has_full_line_comment |= lexed.has_full_line_comment;
        let mut split = false;
        for (token, span) in lexed.tokens {
            let span = offset + span.start..offset + span.end;
            if matches!(token, Token::Invalid | Token::String)
                && source[span.clone()].starts_with('"')
                && let Some(close) = source[span.start + 1..span.end]
                    .find('"')
                    .map(|index| span.start + 1 + index)
                    .filter(|&close| {
                        source.as_bytes()[close - 1] == b'\\'
                            && (token == Token::Invalid || close + 1 < span.end)
                    })
            {
                tokens.push((Token::String, span.start..close + 1));
                offset = close + 1;
                split = true;
                break;
            }
            tokens.push((token, span));
        }
        if !split {
            return Lexed {
                tokens,
                eof: source.len(),
                has_full_line_comment,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Token, lex};

    #[test]
    fn lex_preserves_enum_indent_newlines_and_skips_full_line_comments() {
        let source = "  enum State:\n    ready;\n  # comment\n\n    done;\n";
        let lexed = lex(source);
        assert_eq!(
            lexed.tokens,
            vec![
                (Token::Indent, 0..2),
                (Token::Identifier, 2..6),
                (Token::Identifier, 7..12),
                (Token::Colon, 12..13),
                (Token::Newline, 13..14),
                (Token::Indent, 14..18),
                (Token::Identifier, 18..23),
                (Token::Semicolon, 23..24),
                (Token::Newline, 24..25),
                (Token::Newline, 36..37),
                (Token::Newline, 37..38),
                (Token::Indent, 38..42),
                (Token::Identifier, 42..46),
                (Token::Semicolon, 46..47),
                (Token::Newline, 47..48),
            ]
        );
        assert_eq!(lexed.eof, 48);
    }

    #[test]
    fn lex_skips_nbsp_prefixed_full_line_comments_without_losing_byte_offsets() {
        let source = " \u{a0}# comment\nx\u{a0}y";
        assert!(crate::parse("namespace demo; version \"1.0\";\n\u{a0}# comment\n").is_ok());
        assert_eq!(
            lex(source).tokens,
            vec![
                (Token::Newline, 12..13),
                (Token::Identifier, 13..14),
                (Token::Identifier, 16..17),
            ]
        );
    }

    #[test]
    fn lex_keeps_quoted_escapes_exponents_and_long_operators_intact() {
        let source = "\"a\\\"#b\" 42.1e-3 != <= >= == ** -> ..";
        let lexed = lex(source);
        assert_eq!(
            lexed.tokens,
            vec![
                (Token::String, 0..7),
                (Token::Number, 8..15),
                (Token::NotEqual, 16..18),
                (Token::LessEqual, 19..21),
                (Token::GreaterEqual, 22..24),
                (Token::EqualEqual, 25..27),
                (Token::Power, 28..30),
                (Token::Arrow, 31..33),
                (Token::Range, 34..36),
            ]
        );
        assert_eq!(&source[lexed.tokens[0].1.clone()], "\"a\\\"#b\"");
    }

    #[test]
    fn lex_keeps_crlf_and_trailing_eof_positions() {
        let lexed = lex("enum E:\r\n\tready;  ");
        assert_eq!(
            lexed.tokens,
            vec![
                (Token::Identifier, 0..4),
                (Token::Identifier, 5..6),
                (Token::Colon, 6..7),
                (Token::Newline, 7..9),
                (Token::Indent, 9..10),
                (Token::Identifier, 10..15),
                (Token::Semicolon, 15..16),
            ]
        );
        assert_eq!(lexed.eof, 18);
        assert_eq!(lex("").eof, 0);
    }

    #[test]
    fn lex_reports_invalid_characters_unclosed_strings_and_original_byte_eof() {
        let source = "x #oops\n # skipped\ny ☃\n\"unfinished";
        let lexed = lex(source);
        assert!(lexed.tokens.contains(&(Token::Invalid, 2..7)));
        assert!(lexed.tokens.contains(&(Token::Invalid, 21..24)));
        assert!(lexed.tokens.contains(&(Token::Invalid, 25..36)));
        assert_eq!(lexed.eof, source.len());
        assert_eq!(lexed.eof, 36);
    }
}
