//! The Groscript lexer: turns source text into tokens with line/column spans.

use crate::error::{ErrorKind, GroError, GroResult, Span};
use crate::token::{keyword, StrPart, Tok, Token};

pub struct Lexer<'a> {
    chars: Vec<char>,
    pos: usize,
    line: u32,
    col: u32,
    depth: i32,
    tokens: Vec<Token>,
    _src: &'a str,
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self::with_offset(src, 1, 1)
    }

    pub fn with_offset(src: &'a str, line: u32, col: u32) -> Self {
        Lexer {
            chars: src.chars().collect(),
            pos: 0,
            line,
            col,
            depth: 0,
            tokens: Vec::new(),
            _src: src,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn push(&mut self, tok: Tok, line: u32, col: u32, len: u32) {
        self.tokens.push(Token {
            tok,
            span: Span::new(line, col, len),
        });
    }

    fn error(&self, msg: impl Into<String>, line: u32, col: u32) -> GroError {
        GroError::new(ErrorKind::Lexical, msg).at(Span::new(line, col, 1))
    }

    pub fn tokenize(mut self) -> GroResult<Vec<Token>> {
        while let Some(c) = self.peek() {
            let (line, col) = (self.line, self.col);
            match c {
                ' ' | '\t' | '\r' => {
                    self.bump();
                }
                '\\' if self.peek_at(1) == Some('\n') => {
                    // explicit line continuation
                    self.bump();
                    self.bump();
                }
                '\n' => {
                    self.bump();
                    if self.depth <= 0 {
                        let last_is_nl = matches!(
                            self.tokens.last().map(|t| &t.tok),
                            Some(Tok::Newline) | None
                        );
                        if !last_is_nl {
                            self.push(Tok::Newline, line, col, 1);
                        }
                    }
                }
                '#' => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                '"' => self.string()?,
                '0'..='9' => self.number()?,
                '@' | '$' => {
                    self.bump();
                    let name = self.ident_text();
                    if name.is_empty() {
                        return Err(self.error(
                            format!("expected a name after '{}'", c),
                            line,
                            col,
                        ));
                    }
                    let len = name.chars().count() as u32 + 1;
                    let tok = if c == '@' {
                        Tok::IVar(name)
                    } else {
                        Tok::GVar(name)
                    };
                    self.push(tok, line, col, len);
                }
                ':' => {
                    let prev_is_word = self.pos > 0
                        && matches!(self.chars.get(self.pos - 1), Some(p) if is_ident_char(*p) || *p == '"' || *p == '?' || *p == ')' || *p == ']');
                    if !prev_is_word && self.peek_at(1).map(is_ident_start).unwrap_or(false) {
                        self.bump();
                        let name = self.ident_text();
                        let len = name.chars().count() as u32 + 1;
                        self.push(Tok::Symbol(name), line, col, len);
                    } else {
                        self.bump();
                        self.push(Tok::Colon, line, col, 1);
                    }
                }
                c if is_ident_start(c) => {
                    let name = self.ident_text();
                    let len = name.chars().count() as u32;
                    let tok = if let Some(k) = keyword(&name) {
                        k
                    } else if name.chars().next().unwrap().is_ascii_uppercase() {
                        Tok::Const(name)
                    } else {
                        Tok::Ident(name)
                    };
                    self.push(tok, line, col, len);
                }
                _ => self.operator()?,
            }
        }
        if !matches!(self.tokens.last().map(|t| &t.tok), Some(Tok::Newline) | None) {
            self.push(Tok::Newline, self.line, self.col, 1);
        }
        self.push(Tok::Eof, self.line, self.col, 1);
        Ok(self.tokens)
    }

    fn ident_text(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if is_ident_char(c) {
                s.push(c);
                self.bump();
            } else {
                break;
            }
        }
        if !s.is_empty() && self.peek() == Some('?') && self.peek_at(1) != Some('=') {
            s.push('?');
            self.bump();
        }
        s
    }

    fn number(&mut self) -> GroResult<()> {
        let (line, col) = (self.line, self.col);
        let mut s = String::new();
        let mut len = 0;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
            } else if c == '_' {
            } else {
                break;
            }
            len += 1;
            self.bump();
        }
        if self.peek() == Some('.') && self.peek_at(1).map(|c| c.is_ascii_digit()).unwrap_or(false) {
            s.push('.');
            self.bump();
            len += 1;
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    s.push(c);
                } else if c == '_' {
                } else {
                    break;
                }
                len += 1;
                self.bump();
            }
        }
        if let Some(c) = self.peek() {
            if is_ident_start(c) {
                return Err(self.error(
                    format!("unexpected '{}' after number", c),
                    self.line,
                    self.col,
                ));
            }
        }
        let n: f64 = s
            .parse()
            .map_err(|_| self.error(format!("invalid number '{}'", s), line, col))?;
        self.push(Tok::Number(n), line, col, len);
        Ok(())
    }

    fn string(&mut self) -> GroResult<()> {
        let (line, col) = (self.line, self.col);
        self.bump(); // opening quote
        let mut parts = Vec::new();
        let mut cur = String::new();
        loop {
            let c = match self.peek() {
                Some(c) => c,
                None => return Err(self.error("unterminated string", line, col)),
            };
            match c {
                '"' => {
                    self.bump();
                    break;
                }
                '\n' => return Err(self.error("unterminated string (strings cannot span lines; use \\n)", line, col)),
                '\\' => {
                    self.bump();
                    let e = self
                        .bump()
                        .ok_or_else(|| self.error("unterminated string", line, col))?;
                    cur.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '0' => '\0',
                        '"' => '"',
                        '\\' => '\\',
                        '#' => '#',
                        other => {
                            return Err(self.error(
                                format!("unknown escape sequence '\\{}'", other),
                                self.line,
                                self.col - 2,
                            ))
                        }
                    });
                }
                '#' if self.peek_at(1) == Some('{') => {
                    self.bump();
                    self.bump();
                    if !cur.is_empty() {
                        parts.push(StrPart::Lit(std::mem::take(&mut cur)));
                    }
                    let span = Span::new(self.line, self.col, 1);
                    let mut code = String::new();
                    let mut depth = 1;
                    let mut in_str = false;
                    loop {
                        let c = self.bump().ok_or_else(|| {
                            self.error("unterminated interpolation '#{'", span.line, span.col)
                        })?;
                        if c == '\n' {
                            return Err(self.error("unterminated interpolation '#{'", span.line, span.col));
                        }
                        if in_str {
                            if c == '\\' {
                                code.push(c);
                                if let Some(n) = self.bump() {
                                    code.push(n);
                                }
                                continue;
                            }
                            if c == '"' {
                                in_str = false;
                            }
                        } else if c == '"' {
                            in_str = true;
                        } else if c == '{' {
                            depth += 1;
                        } else if c == '}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        code.push(c);
                    }
                    if code.trim().is_empty() {
                        return Err(self.error("empty interpolation '#{}'", span.line, span.col));
                    }
                    parts.push(StrPart::Code(code, span));
                }
                _ => {
                    cur.push(c);
                    self.bump();
                }
            }
        }
        if !cur.is_empty() || parts.is_empty() {
            parts.push(StrPart::Lit(cur));
        }
        let len = if self.line == line { self.col - col } else { 1 };
        self.push(Tok::Str(parts), line, col, len);
        Ok(())
    }

    fn operator(&mut self) -> GroResult<()> {
        let (line, col) = (self.line, self.col);
        let c = self.peek().unwrap();
        let n = self.peek_at(1);
        let (tok, len) = match (c, n) {
            ('*', Some('*')) => (Tok::StarStar, 2),
            ('+', Some('=')) => (Tok::PlusEq, 2),
            ('-', Some('=')) => (Tok::MinusEq, 2),
            ('*', Some('=')) => (Tok::StarEq, 2),
            ('/', Some('=')) => (Tok::SlashEq, 2),
            ('=', Some('=')) => (Tok::EqEq, 2),
            ('!', Some('=')) => (Tok::BangEq, 2),
            ('<', Some('=')) => (Tok::LtEq, 2),
            ('>', Some('=')) => (Tok::GtEq, 2),
            ('&', Some('&')) => (Tok::AmpAmp, 2),
            ('|', Some('|')) => (Tok::PipePipe, 2),
            ('(', _) => (Tok::LParen, 1),
            (')', _) => (Tok::RParen, 1),
            ('[', _) => (Tok::LBracket, 1),
            (']', _) => (Tok::RBracket, 1),
            ('{', _) => (Tok::LBrace, 1),
            ('}', _) => (Tok::RBrace, 1),
            (',', _) => (Tok::Comma, 1),
            ('.', _) => (Tok::Dot, 1),
            ('+', _) => (Tok::Plus, 1),
            ('-', _) => (Tok::Minus, 1),
            ('*', _) => (Tok::Star, 1),
            ('/', _) => (Tok::Slash, 1),
            ('%', _) => (Tok::Percent, 1),
            ('=', _) => (Tok::Eq, 1),
            ('<', _) => (Tok::Lt, 1),
            ('>', _) => (Tok::Gt, 1),
            ('!', _) => (Tok::Bang, 1),
            (';', _) => {
                return Err(self.error(
                    "semicolons are not used in Groscript; put each statement on its own line",
                    line,
                    col,
                ))
            }
            (other, _) => {
                return Err(self.error(format!("unexpected character '{}'", other), line, col))
            }
        };
        for _ in 0..len {
            self.bump();
        }
        match tok {
            Tok::LParen | Tok::LBracket | Tok::LBrace => self.depth += 1,
            Tok::RParen | Tok::RBracket | Tok::RBrace => self.depth -= 1,
            _ => {}
        }
        self.push(tok, line, col, len);
        Ok(())
    }
}

pub fn tokenize(src: &str) -> GroResult<Vec<Token>> {
    Lexer::new(src).tokenize()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        tokenize(src).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn keywords_and_identifiers() {
        assert_eq!(
            toks("if empty? x\nend"),
            vec![
                Tok::If,
                Tok::Ident("empty?".into()),
                Tok::Ident("x".into()),
                Tok::Newline,
                Tok::End,
                Tok::Newline,
                Tok::Eof
            ]
        );
    }

    #[test]
    fn variables_and_constants() {
        assert_eq!(
            toks("@speed $score Player"),
            vec![
                Tok::IVar("speed".into()),
                Tok::GVar("score".into()),
                Tok::Const("Player".into()),
                Tok::Newline,
                Tok::Eof
            ]
        );
    }

    #[test]
    fn numbers() {
        assert_eq!(
            toks("1 2.5 1_000"),
            vec![
                Tok::Number(1.0),
                Tok::Number(2.5),
                Tok::Number(1000.0),
                Tok::Newline,
                Tok::Eof
            ]
        );
    }

    #[test]
    fn operators_longest_match() {
        assert_eq!(
            toks("<= != ** += && || ="),
            vec![
                Tok::LtEq,
                Tok::BangEq,
                Tok::StarStar,
                Tok::PlusEq,
                Tok::AmpAmp,
                Tok::PipePipe,
                Tok::Eq,
                Tok::Newline,
                Tok::Eof
            ]
        );
    }

    #[test]
    fn comments_and_blank_lines() {
        assert_eq!(
            toks("x # hi\n\n\n# more\ny"),
            vec![
                Tok::Ident("x".into()),
                Tok::Newline,
                Tok::Ident("y".into()),
                Tok::Newline,
                Tok::Eof
            ]
        );
    }

    #[test]
    fn newlines_ignored_inside_brackets() {
        assert_eq!(
            toks("[1,\n2]"),
            vec![
                Tok::LBracket,
                Tok::Number(1.0),
                Tok::Comma,
                Tok::Number(2.0),
                Tok::RBracket,
                Tok::Newline,
                Tok::Eof
            ]
        );
    }

    #[test]
    fn symbols_vs_hash_colons() {
        assert_eq!(
            toks("{a: :b}"),
            vec![
                Tok::LBrace,
                Tok::Ident("a".into()),
                Tok::Colon,
                Tok::Symbol("b".into()),
                Tok::RBrace,
                Tok::Newline,
                Tok::Eof
            ]
        );
    }

    #[test]
    fn string_interpolation() {
        let t = toks("\"hp: #{hp + 1}!\"");
        assert_eq!(
            t[0],
            Tok::Str(vec![
                StrPart::Lit("hp: ".into()),
                StrPart::Code("hp + 1".into(), Span::new(1, 8, 1)),
                StrPart::Lit("!".into()),
            ])
        );
    }

    #[test]
    fn string_escapes() {
        assert_eq!(
            toks(r#""a\n\"b\#{""#)[0],
            Tok::Str(vec![StrPart::Lit("a\n\"b#{".into())])
        );
    }

    #[test]
    fn spans() {
        let t = tokenize("x = 1\n  foo").unwrap();
        assert_eq!(t[0].span, Span::new(1, 1, 1));
        assert_eq!(t[2].span, Span::new(1, 5, 1));
        assert_eq!(t[4].span, Span::new(2, 3, 3));
    }

    #[test]
    fn unterminated_string_is_error() {
        let e = tokenize("\"abc").unwrap_err();
        assert_eq!(e.kind, ErrorKind::Lexical);
        assert!(e.message.contains("unterminated"));
    }

    #[test]
    fn invalid_character() {
        let e = tokenize("x = ~").unwrap_err();
        assert_eq!(e.span.unwrap().col, 5);
    }
}
