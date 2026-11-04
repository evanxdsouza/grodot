//! Abstract syntax tree.

use crate::error::Span;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

impl BinOp {
    pub fn symbol(&self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Pow => "**",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::Le => "<=",
            BinOp::Ge => ">=",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub enum StrSeg {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Number(f64),
    Str(Vec<StrSeg>),
    Symbol(Rc<str>),
    Bool(bool),
    Nil,
    SelfRef,
    Ident(Rc<str>),
    IVar(Rc<str>),
    GVar(Rc<str>),
    Array(Vec<Expr>),
    Hash(Vec<(Expr, Expr)>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    /// `name(args)` — a call by bare name.
    CallName(Rc<str>, Vec<Expr>),
    /// `obj.name(args)`
    CallMethod(Box<Expr>, Rc<str>, Vec<Expr>),
    /// `callee(args)` where callee is an arbitrary expression (e.g. `fns[0](1)`).
    CallValue(Box<Expr>, Vec<Expr>),
    /// `super(args)`
    Super(Vec<Expr>),
    /// `obj.name`
    Member(Box<Expr>, Rc<str>),
    /// `obj[index]`
    Index(Box<Expr>, Box<Expr>),
    /// `target op= value`
    Assign(Box<Expr>, Option<BinOp>, Box<Expr>),
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: Rc<str>,
    pub default: Option<Expr>,
}

#[derive(Debug)]
pub struct FunctionDecl {
    pub name: Rc<str>,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Expr(Expr),
    If {
        branches: Vec<(Expr, Vec<Stmt>)>,
        else_body: Option<Vec<Stmt>>,
    },
    While(Expr, Vec<Stmt>),
    For(Rc<str>, Expr, Vec<Stmt>),
    Repeat(Expr, Vec<Stmt>),
    Return(Option<Expr>),
    Break,
    Next,
    Function(Rc<FunctionDecl>),
    Class {
        name: Rc<str>,
        parent: Option<Rc<str>>,
        body: Vec<Stmt>,
    },
    Module {
        name: Rc<str>,
        body: Vec<Stmt>,
    },
    /// `enum Name ... end`: members are (name, optional explicit value).
    Enum {
        name: Rc<str>,
        members: Vec<(Rc<str>, Option<Expr>)>,
    },
    /// `match subject when a, b ... else ... end`
    Match {
        subject: Expr,
        arms: Vec<MatchArm>,
        else_body: Option<Vec<Stmt>>,
    },
    Import(String),
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub patterns: Vec<Expr>,
    pub body: Vec<Stmt>,
}
