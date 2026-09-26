#[derive(Debug, Clone)]
pub enum Expr {
    Number(String),
    String(String),
    Bool(bool),
    Name(String),
    Field(Box<Expr>, String),
    List(Vec<Expr>),
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

fn lex(text: &str) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }
        let mut token = ch.to_string();
        if ch == '"' {
            let mut closed = false;
            for next in chars.by_ref() {
                token.push(next);
                if next == '"' {
                    closed = true;
                    break;
                }
            }
            if !closed {
                return Err("unterminated string expression".into());
            }
        } else if ch.is_ascii_alphanumeric() || ch == '_' {
            while chars.peek().is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_') {
                token.push(chars.next().unwrap());
            }
            if ch.is_ascii_digit() && chars.peek() == Some(&'.') {
                chars.next();
                if !chars.peek().is_some_and(char::is_ascii_digit) {
                    return Err("invalid number expression".into());
                }
                token.push('.');
                while chars.peek().is_some_and(char::is_ascii_digit) {
                    token.push(chars.next().unwrap());
                }
            }
        } else if "!=<>".contains(ch) && chars.peek() == Some(&'=') {
            token.push(chars.next().unwrap());
        } else if !".[](),<>".contains(ch) {
            return Err(format!("invalid character in expression: {ch}"));
        }
        result.push(token);
    }
    Ok(result)
}

struct Parser {
    tokens: Vec<String>,
    index: usize,
}

impl Parser {
    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.index).map(String::as_str)
    }

    fn take(&mut self) -> Option<String> {
        let token = self.peek()?.to_owned();
        self.index += 1;
        Some(token)
    }

    fn expect(&mut self, token: &str) -> Result<(), String> {
        if self.peek() == Some(token) {
            self.take();
            Ok(())
        } else {
            Err(format!("expected {token} in expression"))
        }
    }

    fn parse(&mut self, minimum: u8) -> Result<Expr, String> {
        let first = self.take().ok_or("expected expression")?;
        let mut left = match first.as_str() {
            "true" => Expr::Bool(true),
            "false" => Expr::Bool(false),
            "not" => Expr::Not(Box::new(self.parse(4)?)),
            "(" => {
                let expr = self.parse(0)?;
                self.expect(")")?;
                expr
            }
            "[" => {
                let mut elements = Vec::new();
                if self.peek() != Some("]") {
                    loop {
                        elements.push(self.parse(0)?);
                        if self.peek() != Some(",") {
                            break;
                        }
                        self.take();
                    }
                }
                self.expect("]")?;
                Expr::List(elements)
            }
            _ if first.starts_with('"') && first.ends_with('"') && first.len() >= 2 => {
                Expr::String(first[1..first.len() - 1].into())
            }
            _ if first.chars().next().is_some_and(|c| c.is_ascii_digit())
                && first.chars().all(|c| c.is_ascii_digit() || c == '.') => Expr::Number(first),
            _ if super::identifier(&first) => Expr::Name(first),
            _ => return Err(format!("invalid expression token: {first}")),
        };
        loop {
            if self.peek() == Some(".") {
                self.take();
                let field = self.take().ok_or("expected field in expression")?;
                if !super::identifier(&field) {
                    return Err("expected field in expression".into());
                }
                left = Expr::Field(Box::new(left), field);
                continue;
            }
            let priority = match self.peek() {
                Some("or") => 1,
                Some("and") => 2,
                Some("==" | "!=" | ">" | ">=" | "<" | "<=") => 3,
                _ => break,
            };
            if priority < minimum {
                break;
            }
            let op = self.take().unwrap();
            let right = self.parse(priority + 1)?;
            left = Expr::Binary(Box::new(left), op, Box::new(right));
        }
        Ok(left)
    }
}

pub fn expression(text: &str) -> Result<Expr, String> {
    let mut parser = Parser { tokens: lex(text)?, index: 0 };
    let result = parser.parse(0)?;
    if parser.peek().is_some() {
        return Err(format!("unexpected token in expression: {}", parser.peek().unwrap()));
    }
    Ok(result)
}
