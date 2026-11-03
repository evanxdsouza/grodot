//! Recursive-descent parser with precedence climbing for expressions.
//!
//! The parser recovers at statement boundaries so that one syntax error does
//! not hide all later errors.

use crate::ast::*;
use crate::error::{ErrorKind, GroError, Span};
use crate::lexer::Lexer;
use crate::token::{StrPart, Tok, Token};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctx {
    TopLevel,
    Class,
    Module,
    Function,
}

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
    errors: Vec<GroError>,
    ctx: Vec<Ctx>,
    loop_depth: u32,
}

type PResult<T> = Result<T, GroError>;

fn syntax(msg: impl Into<String>, span: Span) -> GroError {
    GroError::new(ErrorKind::Syntax, msg).at(span)
}

impl Parser {
    pub fn new(toks: Vec<Token>) -> Self {
        Parser {
            toks,
            pos: 0,
            errors: Vec::new(),
            ctx: vec![Ctx::TopLevel],
            loop_depth: 0,
        }
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn advance(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn check(&self, t: &Tok) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(t)
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.check(t) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: &Tok, what: &str) -> PResult<Token> {
        if self.check(t) {
            Ok(self.advance())
        } else {
            Err(syntax(
                format!("expected {} but found {}", what, self.peek().describe()),
                self.span(),
            ))
        }
    }

    fn skip_newlines(&mut self) {
        while self.check(&Tok::Newline) {
            self.advance();
        }
    }

    fn expect_line_end(&mut self) -> PResult<()> {
        match self.peek() {
            Tok::Newline => {
                self.advance();
                Ok(())
            }
            Tok::Eof => Ok(()),
            other => Err(syntax(
                format!(
                    "expected end of line but found {} (put each statement on its own line)",
                    other.describe()
                ),
                self.span(),
            )),
        }
    }

    /// Skip tokens until the next statement boundary.
    fn recover(&mut self) {
        while !matches!(self.peek(), Tok::Newline | Tok::Eof) {
            self.advance();
        }
        self.skip_newlines();
    }

    pub fn parse_program(mut self) -> Result<Vec<Stmt>, Vec<GroError>> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while !self.check(&Tok::Eof) {
            match self.statement() {
                Ok(s) => stmts.push(s),
                Err(e) => {
                    self.errors.push(e);
                    self.recover();
                }
            }
            self.skip_newlines();
            if matches!(self.peek(), Tok::End | Tok::Elsif | Tok::Else) {
                let sp = self.span();
                self.errors.push(syntax(
                    format!("unexpected {} with no open block", self.peek().describe()),
                    sp,
                ));
                self.advance();
                self.skip_newlines();
            }
        }
        if self.errors.is_empty() {
            Ok(stmts)
        } else {
            Err(self.errors)
        }
    }

    /// Parse statements until one of the block terminators.
    fn block(&mut self, opener: &str, open_span: Span) -> PResult<Vec<Stmt>> {
        let mut body = Vec::new();
        self.skip_newlines();
        loop {
            match self.peek() {
                Tok::End | Tok::Elsif | Tok::Else => break,
                Tok::Eof => {
                    return Err(syntax(
                        format!(
                            "missing 'end' for '{}' opened at line {}",
                            opener, open_span.line
                        ),
                        open_span,
                    ))
                }
                _ => {}
            }
            match self.statement() {
                Ok(s) => body.push(s),
                Err(e) => {
                    if e.message.starts_with("missing 'end'") {
                        return Err(e);
                    }
                    self.errors.push(e);
                    self.recover();
                }
            }
            self.skip_newlines();
        }
        Ok(body)
    }

    fn expect_end(&mut self, opener: &str, open_span: Span) -> PResult<()> {
        if self.eat(&Tok::End) {
            self.expect_line_end()
        } else {
            Err(syntax(
                format!(
                    "expected 'end' to close '{}' opened at line {} but found {}",
                    opener,
                    open_span.line,
                    self.peek().describe()
                ),
                self.span(),
            ))
        }
    }

    fn in_ctx(&self) -> Ctx {
        *self.ctx.last().unwrap()
    }

    fn statement(&mut self) -> PResult<Stmt> {
        let span = self.span();
        let ctx = self.in_ctx();
        match self.peek().clone() {
            Tok::Function => self.function_decl(),
            Tok::Class => {
                if ctx != Ctx::TopLevel {
                    return Err(syntax("classes can only be declared at the top level of a file", span));
                }
                self.class_decl()
            }
            Tok::Module => {
                if ctx != Ctx::TopLevel {
                    return Err(syntax("modules can only be declared at the top level of a file", span));
                }
                self.module_decl()
            }
            _ if matches!(ctx, Ctx::Class | Ctx::Module) => {
                // Only constant assignments are allowed besides functions.
                if let Tok::Const(_) = self.peek() {
                    if matches!(self.peek_at(1), Tok::Eq) {
                        let e = self.expression()?;
                        self.expect_line_end()?;
                        return Ok(Stmt { kind: StmtKind::Expr(e), span });
                    }
                }
                Err(syntax(
                    format!(
                        "a {} body may only contain functions and constants (like SPEED = 5)",
                        if ctx == Ctx::Class { "class" } else { "module" }
                    ),
                    span,
                ))
            }
            Tok::Import => {
                self.advance();
                if ctx != Ctx::TopLevel {
                    return Err(syntax("import can only be used at the top level of a file", span));
                }
                let t = self.advance();
                let path = match t.tok {
                    Tok::Str(parts) => match parts.as_slice() {
                        [StrPart::Lit(s)] => s.clone(),
                        _ => return Err(syntax("import path must be a plain string", t.span)),
                    },
                    other => {
                        return Err(syntax(
                            format!("expected a path string after import but found {}", other.describe()),
                            t.span,
                        ))
                    }
                };
                self.expect_line_end()?;
                Ok(Stmt { kind: StmtKind::Import(path), span })
            }
            Tok::If => self.if_stmt(),
            Tok::While => {
                self.advance();
                let cond = self.expression()?;
                self.expect_line_end()?;
                self.loop_depth += 1;
                let body = self.block("while", span);
                self.loop_depth -= 1;
                let body = body?;
                self.expect_end("while", span)?;
                Ok(Stmt { kind: StmtKind::While(cond, body), span })
            }
            Tok::For => {
                self.advance();
                let name = match self.advance() {
                    Token { tok: Tok::Ident(n), .. } => n,
                    t => {
                        return Err(syntax(
                            format!("expected a variable name after 'for' but found {}", t.tok.describe()),
                            t.span,
                        ))
                    }
                };
                self.expect(&Tok::In, "'in'")?;
                let iter = self.expression()?;
                self.expect_line_end()?;
                self.loop_depth += 1;
                let body = self.block("for", span);
                self.loop_depth -= 1;
                let body = body?;
                self.expect_end("for", span)?;
                Ok(Stmt { kind: StmtKind::For(name.into(), iter, body), span })
            }
            Tok::Repeat => {
                self.advance();
                let count = self.expression()?;
                self.expect(&Tok::Times, "'times' (as in: repeat 3 times)")?;
                self.expect_line_end()?;
                self.loop_depth += 1;
                let body = self.block("repeat", span);
                self.loop_depth -= 1;
                let body = body?;
                self.expect_end("repeat", span)?;
                Ok(Stmt { kind: StmtKind::Repeat(count, body), span })
            }
            Tok::Return => {
                self.advance();
                if !self.ctx.contains(&Ctx::Function) {
                    return Err(syntax("'return' can only be used inside a function", span));
                }
                let value = if matches!(self.peek(), Tok::Newline | Tok::Eof | Tok::End) {
                    None
                } else {
                    Some(self.expression()?)
                };
                self.expect_line_end()?;
                Ok(Stmt { kind: StmtKind::Return(value), span })
            }
            Tok::Break | Tok::Next => {
                let t = self.advance();
                let word = t.tok.text();
                if self.loop_depth == 0 {
                    return Err(syntax(
                        format!("'{}' can only be used inside a loop (while, for, repeat)", word),
                        span,
                    ));
                }
                self.expect_line_end()?;
                Ok(Stmt {
                    kind: if t.tok == Tok::Break { StmtKind::Break } else { StmtKind::Next },
                    span,
                })
            }
            _ => {
                let e = self.expression()?;
                self.expect_line_end()?;
                Ok(Stmt { kind: StmtKind::Expr(e), span })
            }
        }
    }

    fn if_stmt(&mut self) -> PResult<Stmt> {
        let span = self.advance().span;
        let mut branches = Vec::new();
        let cond = self.expression()?;
        self.expect_line_end()?;
        let body = self.block("if", span)?;
        branches.push((cond, body));
        let mut else_body = None;
        loop {
            if self.eat(&Tok::Elsif) {
                let cond = self.expression()?;
                self.expect_line_end()?;
                let body = self.block("if", span)?;
                branches.push((cond, body));
            } else if self.check(&Tok::Else) {
                let es = self.advance().span;
                if self.check(&Tok::If) {
                    return Err(syntax("use 'elsif' instead of 'else if'", es));
                }
                self.expect_line_end()?;
                else_body = Some(self.block("if", span)?);
                if self.check(&Tok::Elsif) || self.check(&Tok::Else) {
                    return Err(syntax("'else' must be the last branch of an 'if'", self.span()));
                }
            } else {
                break;
            }
        }
        self.expect_end("if", span)?;
        Ok(Stmt { kind: StmtKind::If { branches, else_body }, span })
    }

    fn function_decl(&mut self) -> PResult<Stmt> {
        let span = self.advance().span;
        if self.in_ctx() == Ctx::Function {
            return Err(syntax(
                "functions cannot be declared inside other functions",
                span,
            ));
        }
        let nt = self.advance();
        let name: Rc<str> = match nt.tok {
            Tok::Ident(n) => n.into(),
            other => {
                return Err(syntax(
                    format!("expected a function name but found {}", other.describe()),
                    nt.span,
                ))
            }
        };
        self.expect(&Tok::LParen, "'(' after the function name (use () for no parameters)")?;
        let mut params: Vec<Param> = Vec::new();
        if !self.check(&Tok::RParen) {
            loop {
                let pt = self.advance();
                let pname: Rc<str> = match pt.tok {
                    Tok::Ident(n) => n.into(),
                    other => {
                        return Err(syntax(
                            format!("expected a parameter name but found {}", other.describe()),
                            pt.span,
                        ))
                    }
                };
                if params.iter().any(|p| p.name == pname) {
                    return Err(syntax(format!("duplicate parameter '{}'", pname), pt.span));
                }
                let default = if self.eat(&Tok::Eq) {
                    Some(self.expression()?)
                } else {
                    if params.last().map(|p| p.default.is_some()).unwrap_or(false) {
                        return Err(syntax(
                            "parameters without defaults must come before parameters with defaults",
                            pt.span,
                        ));
                    }
                    None
                };
                params.push(Param { name: pname, default });
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(&Tok::RParen, "')' to close the parameter list")?;
        self.expect_line_end()?;
        self.ctx.push(Ctx::Function);
        let saved_loop = self.loop_depth;
        self.loop_depth = 0;
        let body = self.block("function", span);
        self.loop_depth = saved_loop;
        self.ctx.pop();
        let body = body?;
        self.expect_end("function", span)?;
        Ok(Stmt {
            kind: StmtKind::Function(Rc::new(FunctionDecl { name, params, body, span })),
            span,
        })
    }

    fn const_name(&mut self, what: &str) -> PResult<Rc<str>> {
        let t = self.advance();
        match t.tok {
            Tok::Const(n) => Ok(n.into()),
            Tok::Ident(n) => Err(syntax(
                format!("{} names must start with an uppercase letter (try '{}')", what, capitalize(&n)),
                t.span,
            )),
            other => Err(syntax(
                format!("expected a {} name but found {}", what, other.describe()),
                t.span,
            )),
        }
    }

    fn class_decl(&mut self) -> PResult<Stmt> {
        let span = self.advance().span;
        let name = self.const_name("class")?;
        let parent = if self.eat(&Tok::Lt) {
            Some(self.const_name("class")?)
        } else {
            None
        };
        self.expect_line_end()?;
        self.ctx.push(Ctx::Class);
        let body = self.block("class", span);
        self.ctx.pop();
        let body = body?;
        self.expect_end("class", span)?;
        Ok(Stmt { kind: StmtKind::Class { name, parent, body }, span })
    }

    fn module_decl(&mut self) -> PResult<Stmt> {
        let span = self.advance().span;
        let name = self.const_name("module")?;
        self.expect_line_end()?;
        self.ctx.push(Ctx::Module);
        let body = self.block("module", span);
        self.ctx.pop();
        let body = body?;
        self.expect_end("module", span)?;
        Ok(Stmt { kind: StmtKind::Module { name, body }, span })
    }

    // ---------------------------------------------------------------- expressions

    pub fn expression(&mut self) -> PResult<Expr> {
        self.assignment()
    }

    fn assignment(&mut self) -> PResult<Expr> {
        let lhs = self.logical_or()?;
        let op = match self.peek() {
            Tok::Eq => None,
            Tok::PlusEq => Some(BinOp::Add),
            Tok::MinusEq => Some(BinOp::Sub),
            Tok::StarEq => Some(BinOp::Mul),
            Tok::SlashEq => Some(BinOp::Div),
            _ => return Ok(lhs),
        };
        let op_span = self.advance().span;
        match &lhs.kind {
            ExprKind::Ident(_)
            | ExprKind::IVar(_)
            | ExprKind::GVar(_)
            | ExprKind::Member(..)
            | ExprKind::Index(..) => {}
            ExprKind::CallName(..) | ExprKind::CallMethod(..) => {
                return Err(syntax("cannot assign to the result of a call", op_span))
            }
            _ => return Err(syntax("invalid assignment target", lhs.span)),
        }
        let rhs = self.assignment()?;
        let span = lhs.span;
        Ok(Expr {
            kind: ExprKind::Assign(Box::new(lhs), op, Box::new(rhs)),
            span,
        })
    }

    fn logical_or(&mut self) -> PResult<Expr> {
        let mut e = self.logical_and()?;
        while matches!(self.peek(), Tok::Or | Tok::PipePipe) {
            self.advance();
            self.skip_newlines();
            let r = self.logical_and()?;
            let span = e.span;
            e = Expr { kind: ExprKind::Or(Box::new(e), Box::new(r)), span };
        }
        Ok(e)
    }

    fn logical_and(&mut self) -> PResult<Expr> {
        let mut e = self.equality()?;
        while matches!(self.peek(), Tok::And | Tok::AmpAmp) {
            self.advance();
            self.skip_newlines();
            let r = self.equality()?;
            let span = e.span;
            e = Expr { kind: ExprKind::And(Box::new(e), Box::new(r)), span };
        }
        Ok(e)
    }

    fn equality(&mut self) -> PResult<Expr> {
        let e = self.comparison()?;
        let op = match self.peek() {
            Tok::EqEq => BinOp::Eq,
            Tok::BangEq => BinOp::Ne,
            _ => return Ok(e),
        };
        let op_span = self.advance().span;
        self.skip_newlines();
        let r = self.comparison()?;
        if matches!(self.peek(), Tok::EqEq | Tok::BangEq) {
            return Err(syntax("chained equality like a == b == c is not allowed; use 'and'", op_span));
        }
        let span = e.span;
        Ok(Expr { kind: ExprKind::Binary(op, Box::new(e), Box::new(r)), span })
    }

    fn comparison(&mut self) -> PResult<Expr> {
        let e = self.addition()?;
        let op = match self.peek() {
            Tok::Lt => BinOp::Lt,
            Tok::Gt => BinOp::Gt,
            Tok::LtEq => BinOp::Le,
            Tok::GtEq => BinOp::Ge,
            _ => return Ok(e),
        };
        self.advance();
        self.skip_newlines();
        let r = self.addition()?;
        if matches!(self.peek(), Tok::Lt | Tok::Gt | Tok::LtEq | Tok::GtEq) {
            return Err(syntax(
                "chained comparisons like a < b < c are not allowed; write a < b and b < c",
                self.span(),
            ));
        }
        let span = e.span;
        Ok(Expr { kind: ExprKind::Binary(op, Box::new(e), Box::new(r)), span })
    }

    fn addition(&mut self) -> PResult<Expr> {
        let mut e = self.multiplication()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => return Ok(e),
            };
            self.advance();
            self.skip_newlines();
            let r = self.multiplication()?;
            let span = e.span;
            e = Expr { kind: ExprKind::Binary(op, Box::new(e), Box::new(r)), span };
        }
    }

    fn multiplication(&mut self) -> PResult<Expr> {
        let mut e = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => return Ok(e),
            };
            self.advance();
            self.skip_newlines();
            let r = self.unary()?;
            let span = e.span;
            e = Expr { kind: ExprKind::Binary(op, Box::new(e), Box::new(r)), span };
        }
    }

    fn unary(&mut self) -> PResult<Expr> {
        let span = self.span();
        let op = match self.peek() {
            Tok::Minus => UnOp::Neg,
            Tok::Bang | Tok::Not => UnOp::Not,
            Tok::Plus => {
                return Err(syntax("unary '+' is not supported; just write the number", span))
            }
            _ => return self.power(),
        };
        self.advance();
        let inner = self.unary()?;
        // Fold negative number literals.
        if op == UnOp::Neg {
            if let ExprKind::Number(n) = inner.kind {
                return Ok(Expr { kind: ExprKind::Number(-n), span });
            }
        }
        Ok(Expr { kind: ExprKind::Unary(op, Box::new(inner)), span })
    }

    fn power(&mut self) -> PResult<Expr> {
        let base = self.postfix()?;
        if self.eat(&Tok::StarStar) {
            let exp = self.unary()?; // right-associative
            let span = base.span;
            return Ok(Expr { kind: ExprKind::Binary(BinOp::Pow, Box::new(base), Box::new(exp)), span });
        }
        Ok(base)
    }

    fn arguments(&mut self) -> PResult<Vec<Expr>> {
        // '(' already consumed
        let mut args = Vec::new();
        if !self.check(&Tok::RParen) {
            loop {
                args.push(self.expression()?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
                if self.check(&Tok::RParen) {
                    break; // trailing comma
                }
            }
        }
        self.expect(&Tok::RParen, "')' to close the argument list")?;
        Ok(args)
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            match self.peek() {
                Tok::LParen => {
                    let sp = self.advance().span;
                    let args = self.arguments()?;
                    let span = e.span;
                    e = match e.kind {
                        ExprKind::Ident(name) => Expr { kind: ExprKind::CallName(name, args), span },
                        ExprKind::Member(obj, name) => Expr {
                            kind: ExprKind::CallMethod(obj, name, args),
                            span: sp,
                        },
                        _ => Expr { kind: ExprKind::CallValue(Box::new(e), args), span },
                    };
                }
                Tok::Dot => {
                    self.advance();
                    self.skip_newlines();
                    let t = self.advance();
                    let name: Rc<str> = match t.tok {
                        Tok::Ident(n) | Tok::Const(n) => n.into(),
                        Tok::Class => "class".into(),
                        other => {
                            return Err(syntax(
                                format!("expected a name after '.' but found {}", other.describe()),
                                t.span,
                            ))
                        }
                    };
                    let span = Span::new(t.span.line, t.span.col, t.span.len);
                    if self.check(&Tok::LParen) {
                        self.advance();
                        let args = self.arguments()?;
                        e = Expr { kind: ExprKind::CallMethod(Box::new(e), name, args), span };
                    } else {
                        e = Expr { kind: ExprKind::Member(Box::new(e), name), span };
                    }
                }
                Tok::LBracket => {
                    self.advance();
                    let idx = self.expression()?;
                    self.expect(&Tok::RBracket, "']'")?;
                    let span = e.span;
                    e = Expr { kind: ExprKind::Index(Box::new(e), Box::new(idx)), span };
                }
                _ => return Ok(e),
            }
        }
    }

    fn primary(&mut self) -> PResult<Expr> {
        let t = self.advance();
        let span = t.span;
        let kind = match t.tok {
            Tok::Number(n) => ExprKind::Number(n),
            Tok::Str(parts) => ExprKind::Str(self.string_parts(parts)?),
            Tok::Symbol(s) => ExprKind::Symbol(s.into()),
            Tok::True => ExprKind::Bool(true),
            Tok::False => ExprKind::Bool(false),
            Tok::Nil => ExprKind::Nil,
            Tok::SelfKw => ExprKind::SelfRef,
            Tok::Ident(n) | Tok::Const(n) => ExprKind::Ident(n.into()),
            Tok::IVar(n) => ExprKind::IVar(n.into()),
            Tok::GVar(n) => ExprKind::GVar(n.into()),
            Tok::Super => {
                self.expect(&Tok::LParen, "'(' after super")?;
                ExprKind::Super(self.arguments()?)
            }
            Tok::LParen => {
                let e = self.expression()?;
                self.expect(&Tok::RParen, "')'")?;
                return Ok(e);
            }
            Tok::LBracket => {
                let mut items = Vec::new();
                if !self.check(&Tok::RBracket) {
                    loop {
                        items.push(self.expression()?);
                        if !self.eat(&Tok::Comma) || self.check(&Tok::RBracket) {
                            break;
                        }
                    }
                }
                self.expect(&Tok::RBracket, "']' to close the array")?;
                ExprKind::Array(items)
            }
            Tok::LBrace => {
                let mut pairs = Vec::new();
                if !self.check(&Tok::RBrace) {
                    loop {
                        let kt = self.advance();
                        let key = match kt.tok {
                            Tok::Ident(n) | Tok::Const(n) | Tok::Symbol(n) => {
                                Expr { kind: ExprKind::Symbol(n.into()), span: kt.span }
                            }
                            Tok::Str(parts) => Expr { kind: ExprKind::Str(self.string_parts(parts)?), span: kt.span },
                            Tok::Number(n) => Expr { kind: ExprKind::Number(n), span: kt.span },
                            Tok::True => Expr { kind: ExprKind::Bool(true), span: kt.span },
                            Tok::False => Expr { kind: ExprKind::Bool(false), span: kt.span },
                            other => {
                                return Err(syntax(
                                    format!("expected a hash key (name, string or number) but found {}", other.describe()),
                                    kt.span,
                                ))
                            }
                        };
                        self.expect(&Tok::Colon, "':' after the hash key (as in {name: value})")?;
                        let v = self.expression()?;
                        pairs.push((key, v));
                        if !self.eat(&Tok::Comma) || self.check(&Tok::RBrace) {
                            break;
                        }
                    }
                }
                self.expect(&Tok::RBrace, "'}' to close the hash")?;
                ExprKind::Hash(pairs)
            }
            Tok::Function => {
                return Err(syntax(
                    "anonymous functions are not supported; declare a named function and pass its name",
                    span,
                ))
            }
            other => {
                return Err(syntax(format!("unexpected {}", other.describe()), span));
            }
        };
        Ok(Expr { kind, span })
    }

    fn string_parts(&mut self, parts: Vec<StrPart>) -> PResult<Vec<StrSeg>> {
        let mut out = Vec::new();
        for p in parts {
            match p {
                StrPart::Lit(s) => out.push(StrSeg::Lit(s)),
                StrPart::Code(code, sp) => {
                    let toks = Lexer::with_offset(&code, sp.line, sp.col).tokenize()?;
                    let mut sub = Parser::new(toks);
                    let e = sub.expression()?;
                    sub.skip_newlines();
                    if !sub.check(&Tok::Eof) {
                        return Err(syntax("interpolation must contain a single expression", sp));
                    }
                    out.push(StrSeg::Expr(e));
                }
            }
        }
        Ok(out)
    }
}

fn capitalize(s: &str) -> String {
    let mut out = String::new();
    let mut up = true;
    for c in s.chars() {
        if c == '_' {
            up = true;
            continue;
        }
        if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse a whole program. Returns every syntax error found.
pub fn parse(src: &str) -> Result<Vec<Stmt>, Vec<GroError>> {
    let toks = Lexer::new(src).tokenize().map_err(|e| vec![e])?;
    Parser::new(toks).parse_program()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(src: &str) -> Vec<Stmt> {
        match parse(src) {
            Ok(s) => s,
            Err(e) => panic!("parse failed: {:?}", e.iter().map(|e| e.short()).collect::<Vec<_>>()),
        }
    }

    fn first_error(src: &str) -> GroError {
        parse(src).unwrap_err().remove(0)
    }

    fn expr_of(src: &str) -> ExprKind {
        match &parse_ok(src)[0].kind {
            StmtKind::Expr(e) => e.kind.clone(),
            _ => panic!("not an expression"),
        }
    }

    #[test]
    fn precedence_mul_over_add() {
        match expr_of("1 + 2 * 3") {
            ExprKind::Binary(BinOp::Add, _, r) => {
                assert!(matches!(r.kind, ExprKind::Binary(BinOp::Mul, _, _)))
            }
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn power_is_right_associative() {
        match expr_of("2 ** 3 ** 2") {
            ExprKind::Binary(BinOp::Pow, l, r) => {
                assert!(matches!(l.kind, ExprKind::Number(_)));
                assert!(matches!(r.kind, ExprKind::Binary(BinOp::Pow, _, _)));
            }
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn assignment_is_right_associative() {
        match expr_of("a = b = 1") {
            ExprKind::Assign(_, None, r) => assert!(matches!(r.kind, ExprKind::Assign(..))),
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn calls_and_members() {
        assert!(matches!(expr_of("foo(1, 2)"), ExprKind::CallName(_, a) if a.len() == 2));
        assert!(matches!(expr_of("a.b(1)"), ExprKind::CallMethod(_, _, _)));
        assert!(matches!(expr_of("a.b"), ExprKind::Member(_, _)));
        assert!(matches!(expr_of("a[0]"), ExprKind::Index(_, _)));
        assert!(matches!(expr_of("a.b = 1"), ExprKind::Assign(..)));
        assert!(matches!(expr_of("a[0] += 1"), ExprKind::Assign(_, Some(BinOp::Add), _)));
    }

    #[test]
    fn blocks() {
        let s = parse_ok("if x\n  y()\nelsif z\n  w()\nelse\n  v()\nend\n");
        match &s[0].kind {
            StmtKind::If { branches, else_body } => {
                assert_eq!(branches.len(), 2);
                assert!(else_body.is_some());
            }
            _ => panic!(),
        }
        parse_ok("while x < 3\n  x += 1\nend");
        parse_ok("for i in [1,2]\n  print(i)\nend");
        parse_ok("repeat 3 times\n  print(1)\nend");
    }

    #[test]
    fn classes_and_modules() {
        let s = parse_ok(
            "class Player < Entity\n  SPEED = 3\n  function ready()\n    @hp = 1\n  end\nend\nmodule M\n  function f(a, b = 2)\n    return a + b\n  end\nend",
        );
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn hash_and_array_literals() {
        assert!(matches!(expr_of("{a: 1, \"b\": 2}"), ExprKind::Hash(p) if p.len() == 2));
        assert!(matches!(expr_of("[1, [2, 3], {}]"), ExprKind::Array(p) if p.len() == 3));
    }

    #[test]
    fn missing_end_reports_opening_line() {
        let e = first_error("function f()\n  x = 1\n");
        assert_eq!(e.kind, ErrorKind::Syntax);
        assert!(e.message.contains("opened at line 1"), "{}", e.message);
    }

    #[test]
    fn chained_comparison_rejected() {
        let e = first_error("a < b < c");
        assert!(e.message.contains("chained"));
    }

    #[test]
    fn break_outside_loop_rejected() {
        let e = first_error("break");
        assert!(e.message.contains("inside a loop"));
    }

    #[test]
    fn return_outside_function_rejected() {
        let e = first_error("return 1");
        assert!(e.message.contains("inside a function"));
    }

    #[test]
    fn recovery_reports_multiple_errors() {
        let errs = parse("x = )\ny = 2\nz = (\nw = 3").unwrap_err();
        assert!(errs.len() >= 2, "{:?}", errs);
    }

    #[test]
    fn malformed_argument_list() {
        let e = first_error("foo(1 2)");
        assert!(e.message.contains("')'"), "{}", e.message);
    }

    #[test]
    fn interpolation_parses_expression() {
        match expr_of("\"a#{1 + 2}b\"") {
            ExprKind::Str(segs) => assert_eq!(segs.len(), 3),
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn lowercase_class_name_hint() {
        let e = first_error("class player\nend");
        assert!(e.message.contains("Player"));
    }
}
