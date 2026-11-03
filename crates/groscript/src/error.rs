//! Error types and pretty error presentation for Groscript.

use std::fmt;
use std::rc::Rc;

/// A location in a source file. Lines and columns are 1-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub line: u32,
    pub col: u32,
    pub len: u32,
}

impl Span {
    pub fn new(line: u32, col: u32, len: u32) -> Self {
        Span { line, col, len }
    }
}

/// Error categories from the language specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Lexical,
    Syntax,
    Name,
    Type,
    Property,
    Method,
    Import,
    Engine,
    Runtime,
    Assertion,
}

impl ErrorKind {
    pub fn title(&self) -> &'static str {
        match self {
            ErrorKind::Lexical => "LexicalError",
            ErrorKind::Syntax => "SyntaxError",
            ErrorKind::Name => "NameError",
            ErrorKind::Type => "TypeError",
            ErrorKind::Property => "PropertyError",
            ErrorKind::Method => "MethodError",
            ErrorKind::Import => "ImportError",
            ErrorKind::Engine => "EngineError",
            ErrorKind::Runtime => "RuntimeError",
            ErrorKind::Assertion => "AssertionError",
        }
    }
}

/// A Groscript error with source location and call stack.
#[derive(Debug, Clone)]
pub struct GroError {
    pub kind: ErrorKind,
    pub message: String,
    pub file: Option<Rc<str>>,
    pub span: Option<Span>,
    /// Innermost frame first.
    pub stack: Vec<String>,
    /// The source text of the file the error occurred in (used for the snippet).
    pub source: Option<Rc<str>>,
}

impl GroError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        GroError {
            kind,
            message: message.into(),
            file: None,
            span: None,
            stack: Vec::new(),
            source: None,
        }
    }

    pub fn at(mut self, span: Span) -> Self {
        if self.span.is_none() {
            self.span = Some(span);
        }
        self
    }

    pub fn in_file(mut self, file: &Rc<str>, source: &Rc<str>) -> Self {
        if self.file.is_none() {
            self.file = Some(file.clone());
            self.source = Some(source.clone());
        }
        self
    }

    /// One-line summary, e.g. `scripts/player.gro:24:12: NameError: ...`.
    pub fn short(&self) -> String {
        let mut s = String::new();
        if let Some(f) = &self.file {
            s.push_str(f);
            if let Some(sp) = self.span {
                s.push_str(&format!(":{}:{}", sp.line, sp.col));
            }
            s.push_str(": ");
        } else if let Some(sp) = self.span {
            s.push_str(&format!("line {}:{}: ", sp.line, sp.col));
        }
        s.push_str(self.kind.title());
        s.push_str(": ");
        s.push_str(&self.message);
        s
    }

    /// Multi-line report in the format described by the spec.
    pub fn report(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Groscript Error: {}\n", self.kind.title()));
        if let Some(f) = &self.file {
            out.push_str(&format!("File: {}\n", f));
        }
        if let Some(sp) = self.span {
            out.push_str(&format!("Line: {}\nColumn: {}\n", sp.line, sp.col));
        }
        out.push('\n');
        out.push_str(&self.message);
        out.push('\n');
        if let (Some(sp), Some(src)) = (self.span, &self.source) {
            if let Some(line) = src.lines().nth(sp.line.saturating_sub(1) as usize) {
                let num = sp.line.to_string();
                let pad = " ".repeat(num.len());
                out.push_str(&format!("\n  {} | {}\n", num, line));
                let col = sp.col.saturating_sub(1) as usize;
                let prefix: String = line
                    .chars()
                    .take(col)
                    .map(|c| if c == '\t' { '\t' } else { ' ' })
                    .collect();
                out.push_str(&format!(
                    "  {} | {}{}\n",
                    pad,
                    prefix,
                    "^".repeat(sp.len.max(1) as usize)
                ));
            }
        }
        if !self.stack.is_empty() {
            out.push_str("\nCall stack:\n");
            let mut i = 0;
            while i < self.stack.len() {
                let mut n = 1;
                while i + n < self.stack.len() && self.stack[i + n] == self.stack[i] {
                    n += 1;
                }
                if n > 1 {
                    out.push_str(&format!("  {} (repeated {} times)\n", self.stack[i], n));
                } else {
                    out.push_str(&format!("  {}\n", self.stack[i]));
                }
                i += n;
            }
        }
        out
    }
}

impl fmt::Display for GroError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.short())
    }
}

impl std::error::Error for GroError {}

pub type GroResult<T> = Result<T, GroError>;

/// Shorthand constructors.
pub fn err<T>(kind: ErrorKind, msg: impl Into<String>) -> GroResult<T> {
    Err(GroError::new(kind, msg))
}
