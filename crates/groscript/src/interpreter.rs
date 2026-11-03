//! The tree-walking interpreter.

use crate::ast::*;
use crate::error::{ErrorKind, GroError, GroResult, Span};
use crate::value::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Non-local control flow while executing statements.
pub(crate) enum Ctrl {
    Error(GroError),
    Return(Value),
    Break,
    Next,
}

impl From<GroError> for Ctrl {
    fn from(e: GroError) -> Self {
        Ctrl::Error(e)
    }
}

pub(crate) type EResult<T> = Result<T, Ctrl>;

pub(crate) fn rt<T>(kind: ErrorKind, msg: impl Into<String>) -> EResult<T> {
    Err(Ctrl::Error(GroError::new(kind, msg)))
}

struct Frame {
    locals: HashMap<Rc<str>, Value>,
    self_val: Value,
    module: Option<Rc<Module>>,
    class: Option<Rc<Class>>,
    func_name: Rc<str>,
    file: Rc<str>,
    source: Rc<str>,
    /// When true, constant assignments are allowed (top level of a file).
    top_level: bool,
}

pub struct Interpreter {
    pub globals: HashMap<Rc<str>, Value>,
    pub gvars: HashMap<Rc<str>, Value>,
    frames: Vec<Frame>,
    output: Box<dyn FnMut(&str)>,
    steps: u64,
    pub step_limit: u64,
    pub max_depth: usize,
    /// Directory that `import` paths are resolved against.
    pub import_root: Option<PathBuf>,
    loaded: HashSet<PathBuf>,
    loading: Vec<PathBuf>,
    /// Arbitrary host data, e.g. the engine world.
    pub user_data: Option<Rc<dyn std::any::Any>>,
    rng_state: u64,
    repl_locals: Option<HashMap<Rc<str>, Value>>,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

const MAX_DEPTH: usize = 200;

impl Interpreter {
    pub fn new() -> Self {
        let mut it = Interpreter {
            globals: HashMap::new(),
            gvars: HashMap::new(),
            frames: Vec::new(),
            output: Box::new(|s| println!("{}", s)),
            steps: 0,
            step_limit: 50_000_000,
            max_depth: MAX_DEPTH,
            import_root: None,
            loaded: HashSet::new(),
            loading: Vec::new(),
            user_data: None,
            rng_state: 0x2545_F491_4F6C_DD1D,
            repl_locals: None,
        };
        crate::builtins::install(&mut it);
        it
    }

    /// Redirect `print` output. The callback receives one line (without newline).
    pub fn set_output(&mut self, f: impl FnMut(&str) + 'static) {
        self.output = Box::new(f);
    }

    /// Capture output into a shared buffer (convenient for tests).
    pub fn capture_output(&mut self) -> Rc<RefCell<String>> {
        let buf = Rc::new(RefCell::new(String::new()));
        let b2 = buf.clone();
        self.set_output(move |s| {
            let mut b = b2.borrow_mut();
            b.push_str(s);
            b.push('\n');
        });
        buf
    }

    pub fn print_line(&mut self, s: &str) {
        (self.output)(s);
    }

    pub fn set_global(&mut self, name: &str, v: Value) {
        self.globals.insert(name.into(), v);
    }

    pub fn get_global(&self, name: &str) -> Option<Value> {
        self.globals.get(name).cloned()
    }

    /// Install a native module (e.g. `Input`) as a global.
    pub fn add_module(&mut self, m: Rc<Module>) {
        self.globals.insert(m.name.clone(), Value::Module(m));
    }

    /// Define a native class that scripts can inherit from (e.g. `Entity`).
    pub fn define_native_class(&mut self, name: &str) -> Rc<Class> {
        let c = Rc::new(Class {
            name: name.into(),
            parent: None,
            methods: RefCell::new(HashMap::new()),
            constants: RefCell::new(HashMap::new()),
            native: true,
        });
        self.globals.insert(name.into(), Value::Class(c.clone()));
        c
    }

    /// Reset the execution budget (call before each engine callback).
    pub fn reset_budget(&mut self) {
        self.steps = 0;
    }

    pub fn next_random(&mut self) -> f64 {
        // xorshift64*
        let mut x = self.rng_state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng_state = x;
        let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (r >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn seed_random(&mut self, seed: u64) {
        self.rng_state = seed.max(1);
    }

    // ------------------------------------------------------------ entry points

    /// Parse and run a source file. Returns the value of the last expression statement.
    pub fn run_source(&mut self, file: &str, src: &str) -> GroResult<Value> {
        let file: Rc<str> = file.into();
        let source: Rc<str> = src.into();
        let stmts = match crate::parser::parse(src) {
            Ok(s) => s,
            Err(mut errs) => {
                let n = errs.len();
                let mut e = errs.remove(0).in_file(&file, &source);
                if n > 1 {
                    e.message.push_str(&format!(" (and {} more error{})", n - 1, if n > 2 { "s" } else { "" }));
                }
                return Err(e);
            }
        };
        self.run_program(&stmts, file, source)
    }

    /// Load and run a file from disk.
    pub fn run_file(&mut self, path: &Path) -> GroResult<Value> {
        let src = std::fs::read_to_string(path)
            .map_err(|e| GroError::new(ErrorKind::Import, format!("cannot read '{}': {}", path.display(), e)))?;
        if self.import_root.is_none() {
            self.import_root = path.parent().map(|p| p.to_path_buf());
        }
        if let Ok(c) = path.canonicalize() {
            self.loaded.insert(c);
        }
        self.run_source(&path.display().to_string(), &src)
    }

    /// Like [`Interpreter::run_source`], but top-level variables survive between calls (for a REPL).
    pub fn eval_repl(&mut self, src: &str) -> GroResult<Value> {
        if self.repl_locals.is_none() {
            self.repl_locals = Some(HashMap::new());
        }
        self.steps = 0;
        self.run_source("<repl>", src)
    }

    fn run_program(&mut self, stmts: &[Stmt], file: Rc<str>, source: Rc<str>) -> GroResult<Value> {
        let use_repl = &*file == "<repl>" && self.repl_locals.is_some();
        let locals = if use_repl { self.repl_locals.take().unwrap() } else { HashMap::new() };
        self.frames.push(Frame {
            locals,
            self_val: Value::Nil,
            module: None,
            class: None,
            func_name: "<main>".into(),
            file: file.clone(),
            source: source.clone(),
            top_level: true,
        });
        let mut last = Value::Nil;
        let mut result = Ok(());
        for s in stmts {
            match &s.kind {
                StmtKind::Expr(e) => match self.eval(e) {
                    Ok(v) => last = v,
                    Err(c) => {
                        result = Err(c);
                        break;
                    }
                },
                _ => {
                    last = Value::Nil;
                    if let Err(c) = self.exec(s) {
                        result = Err(c);
                        break;
                    }
                }
            }
        }
        let frame = self.frames.pop();
        if use_repl {
            self.repl_locals = frame.map(|f| f.locals);
        }
        match result {
            Ok(()) => Ok(last),
            Err(Ctrl::Error(mut e)) => {
                e = e.in_file(&file, &source);
                e.stack.push(format!("<main> ({})", file));
                Err(e)
            }
            Err(Ctrl::Return(_)) | Err(Ctrl::Break) | Err(Ctrl::Next) => Ok(last),
        }
    }

    /// Call any callable value (function, native function) with arguments.
    pub fn call_value(&mut self, f: &Value, args: Vec<Value>) -> GroResult<Value> {
        self.call_callable(f, args, Span::default()).map_err(|c| match c {
            Ctrl::Error(e) => e,
            _ => GroError::new(ErrorKind::Runtime, "unexpected control flow"),
        })
    }

    /// Call a method on an object if it exists. Returns `Ok(None)` when the method
    /// is not defined (used for optional lifecycle callbacks).
    pub fn call_method_if_exists(&mut self, recv: &Value, name: &str, args: Vec<Value>) -> GroResult<Option<Value>> {
        let method = match recv {
            Value::Object(o) => o.class.find_method(name),
            _ => None,
        };
        match method {
            Some(m) => self.call_function(&m, recv.clone(), args, Span::default()).map(Some).map_err(|c| match c {
                Ctrl::Error(e) => e,
                _ => GroError::new(ErrorKind::Runtime, "unexpected control flow"),
            }),
            None => Ok(None),
        }
    }

    /// Call a method by name (errors if it does not exist).
    pub fn call_method(&mut self, recv: &Value, name: &str, args: Vec<Value>) -> GroResult<Value> {
        self.invoke_method(recv, name, args, Span::default()).map_err(|c| match c {
            Ctrl::Error(e) => e,
            _ => GroError::new(ErrorKind::Runtime, "unexpected control flow"),
        })
    }

    /// Create an instance of a class without calling `initialize`, attaching a host object.
    pub fn new_object(&mut self, class: &Rc<Class>, host: Option<Rc<dyn HostObject>>) -> Value {
        Value::Object(Rc::new(Object {
            class: class.clone(),
            fields: RefCell::new(HashMap::new()),
            host: RefCell::new(host),
        }))
    }

    // ------------------------------------------------------------ statements

    fn frame(&self) -> &Frame {
        self.frames.last().expect("no frame")
    }

    fn frame_mut(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("no frame")
    }

    fn tick(&mut self, span: Span) -> EResult<()> {
        self.steps += 1;
        if self.steps > self.step_limit {
            return Err(GroError::new(ErrorKind::Runtime, "execution budget exceeded (is there an infinite loop?)")
                .at(span)
                .into());
        }
        Ok(())
    }

    fn exec_block(&mut self, stmts: &[Stmt]) -> EResult<()> {
        for s in stmts {
            self.exec(s)?;
        }
        Ok(())
    }

    fn exec(&mut self, stmt: &Stmt) -> EResult<()> {
        self.tick(stmt.span)?;
        match &stmt.kind {
            StmtKind::Expr(e) => {
                self.eval(e)?;
            }
            StmtKind::If { branches, else_body } => {
                for (cond, body) in branches {
                    if self.eval(cond)?.truthy() {
                        return self.exec_block(body);
                    }
                }
                if let Some(b) = else_body {
                    self.exec_block(b)?;
                }
            }
            StmtKind::While(cond, body) => {
                while self.eval(cond)?.truthy() {
                    self.tick(stmt.span)?;
                    match self.exec_block(body) {
                        Ok(()) | Err(Ctrl::Next) => {}
                        Err(Ctrl::Break) => break,
                        Err(e) => return Err(e),
                    }
                }
            }
            StmtKind::For(var, iter, body) => {
                let it = self.eval(iter)?;
                let items: Vec<Value> = match &it {
                    Value::Array(a) => a.borrow().clone(),
                    Value::Hash(h) => h.borrow().keys.iter().map(|k| k.to_value()).collect(),
                    Value::Str(s) => s.chars().map(|c| Value::Str(c.to_string().into())).collect(),
                    other => {
                        return Err(GroError::new(
                            ErrorKind::Type,
                            format!(
                                "cannot loop over a {} (use an Array, Hash, String or range(a, b))",
                                other.type_name()
                            ),
                        )
                        .at(iter.span)
                        .into())
                    }
                };
                for v in items {
                    self.tick(stmt.span)?;
                    self.frame_mut().locals.insert(var.clone(), v);
                    match self.exec_block(body) {
                        Ok(()) | Err(Ctrl::Next) => {}
                        Err(Ctrl::Break) => break,
                        Err(e) => return Err(e),
                    }
                }
            }
            StmtKind::Repeat(count, body) => {
                let n = match self.eval(count)? {
                    Value::Number(n) if n >= 0.0 => n.floor() as u64,
                    other => {
                        return Err(GroError::new(
                            ErrorKind::Type,
                            format!("repeat needs a non-negative Number, got {}", other.inspect()),
                        )
                        .at(count.span)
                        .into())
                    }
                };
                for _ in 0..n {
                    self.tick(stmt.span)?;
                    match self.exec_block(body) {
                        Ok(()) | Err(Ctrl::Next) => {}
                        Err(Ctrl::Break) => break,
                        Err(e) => return Err(e),
                    }
                }
            }
            StmtKind::Return(v) => {
                let v = match v {
                    Some(e) => self.eval(e)?,
                    None => Value::Nil,
                };
                return Err(Ctrl::Return(v));
            }
            StmtKind::Break => return Err(Ctrl::Break),
            StmtKind::Next => return Err(Ctrl::Next),
            StmtKind::Function(decl) => {
                let f = Rc::new(Function {
                    decl: decl.clone(),
                    module: self.frame().module.clone(),
                    class: None,
                    file: self.frame().file.clone(),
                    source: self.frame().source.clone(),
                });
                self.globals.insert(decl.name.clone(), Value::Function(f));
            }
            StmtKind::Class { name, parent, body } => self.declare_class(name, parent, body, stmt.span)?,
            StmtKind::Module { name, body } => self.declare_module(name, body)?,
            StmtKind::Import(path) => self.import(path, stmt.span)?,
        }
        Ok(())
    }

    fn declare_class(&mut self, name: &Rc<str>, parent: &Option<Rc<str>>, body: &[Stmt], span: Span) -> EResult<()> {
        let parent_class = match parent {
            None => None,
            Some(p) => match self.globals.get(p) {
                Some(Value::Class(c)) => Some(c.clone()),
                Some(other) => {
                    return Err(GroError::new(
                        ErrorKind::Type,
                        format!("'{}' is a {}, not a class", p, other.type_name()),
                    )
                    .at(span)
                    .into())
                }
                None => {
                    return Err(GroError::new(
                        ErrorKind::Name,
                        format!("unknown parent class '{}' (declare or import it first)", p),
                    )
                    .at(span)
                    .into())
                }
            },
        };
        let class = Rc::new(Class {
            name: name.clone(),
            parent: parent_class,
            methods: RefCell::new(HashMap::new()),
            constants: RefCell::new(HashMap::new()),
            native: false,
        });
        self.globals.insert(name.clone(), Value::Class(class.clone()));
        for s in body {
            match &s.kind {
                StmtKind::Function(decl) => {
                    let f = Rc::new(Function {
                        decl: decl.clone(),
                        module: None,
                        class: Some(class.clone()),
                        file: self.frame().file.clone(),
                        source: self.frame().source.clone(),
                    });
                    class.methods.borrow_mut().insert(decl.name.clone(), f);
                }
                StmtKind::Expr(Expr { kind: ExprKind::Assign(target, None, value), .. }) => {
                    if let ExprKind::Ident(cname) = &target.kind {
                        let v = self.eval(value)?;
                        class.constants.borrow_mut().insert(cname.clone(), v);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn declare_module(&mut self, name: &Rc<str>, body: &[Stmt]) -> EResult<()> {
        let module = Module::new(name);
        self.globals.insert(name.clone(), Value::Module(module.clone()));
        for s in body {
            match &s.kind {
                StmtKind::Function(decl) => {
                    let f = Rc::new(Function {
                        decl: decl.clone(),
                        module: Some(module.clone()),
                        class: None,
                        file: self.frame().file.clone(),
                        source: self.frame().source.clone(),
                    });
                    module.set(&decl.name, Value::Function(f));
                }
                StmtKind::Expr(Expr { kind: ExprKind::Assign(target, None, value), .. }) => {
                    if let ExprKind::Ident(cname) = &target.kind {
                        let v = self.eval(value)?;
                        module.set(cname, v);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn import(&mut self, path: &str, span: Span) -> EResult<()> {
        let root = match &self.import_root {
            Some(r) => r.clone(),
            None => std::env::current_dir().unwrap_or_default(),
        };
        let mut rel = PathBuf::from(path);
        if rel.extension().is_none() {
            rel.set_extension("gro");
        }
        if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(GroError::new(
                ErrorKind::Import,
                format!("import path '{}' must stay inside the scripts folder (no '..' or absolute paths)", path),
            )
            .at(span)
            .into());
        }
        let full = root.join(&rel);
        let canon = full.canonicalize().map_err(|_| {
            GroError::new(ErrorKind::Import, format!("module '{}' not found (looked for {})", path, full.display()))
                .at(span)
        })?;
        if self.loading.contains(&canon) {
            let mut chain: Vec<String> =
                self.loading.iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string()).collect();
            chain.push(canon.file_name().unwrap_or_default().to_string_lossy().to_string());
            return Err(GroError::new(ErrorKind::Import, format!("circular import: {}", chain.join(" -> ")))
                .at(span)
                .into());
        }
        if self.loaded.contains(&canon) {
            return Ok(());
        }
        let src = std::fs::read_to_string(&canon)
            .map_err(|e| GroError::new(ErrorKind::Import, format!("cannot read module '{}': {}", path, e)).at(span))?;
        self.loading.push(canon.clone());
        let display = rel.display().to_string();
        let r = self.run_source(&display, &src);
        self.loading.pop();
        self.loaded.insert(canon);
        r.map(|_| ()).map_err(Ctrl::Error)
    }

    // ------------------------------------------------------------ expressions

    pub(crate) fn eval(&mut self, e: &Expr) -> EResult<Value> {
        let r = self.eval_inner(e);
        match r {
            Err(Ctrl::Error(err)) => Err(Ctrl::Error(err.at(e.span))),
            other => other,
        }
    }

    fn eval_inner(&mut self, e: &Expr) -> EResult<Value> {
        match &e.kind {
            ExprKind::Number(n) => Ok(Value::Number(*n)),
            ExprKind::Str(segs) => {
                if let [StrSeg::Lit(s)] = segs.as_slice() {
                    return Ok(Value::Str(s.as_str().into()));
                }
                let mut out = String::new();
                for s in segs {
                    match s {
                        StrSeg::Lit(l) => out.push_str(l),
                        StrSeg::Expr(x) => {
                            let v = self.eval(x)?;
                            out.push_str(&v.to_display());
                        }
                    }
                }
                Ok(Value::Str(out.into()))
            }
            ExprKind::Symbol(s) => Ok(Value::Symbol(s.clone())),
            ExprKind::Bool(b) => Ok(Value::Bool(*b)),
            ExprKind::Nil => Ok(Value::Nil),
            ExprKind::SelfRef => Ok(self.frame().self_val.clone()),
            ExprKind::Ident(name) => self.lookup(name),
            ExprKind::IVar(name) => self.get_ivar(name),
            ExprKind::GVar(name) => Ok(self.gvars.get(name).cloned().unwrap_or(Value::Nil)),
            ExprKind::Array(items) => {
                let mut v = Vec::with_capacity(items.len());
                for i in items {
                    v.push(self.eval(i)?);
                }
                Ok(Value::array(v))
            }
            ExprKind::Hash(pairs) => {
                let mut h = GroHash::default();
                for (k, v) in pairs {
                    let kv = self.eval(k)?;
                    let key = kv.hash_key().ok_or_else(|| {
                        GroError::new(ErrorKind::Type, format!("a {} cannot be a hash key", kv.type_name()))
                    })?;
                    let vv = self.eval(v)?;
                    h.insert(key, vv);
                }
                Ok(Value::hash(h))
            }
            ExprKind::Unary(op, inner) => {
                let v = self.eval(inner)?;
                match op {
                    UnOp::Not => Ok(Value::Bool(!v.truthy())),
                    UnOp::Neg => match v {
                        Value::Number(n) => Ok(Value::Number(-n)),
                        Value::Vec2(a) => Ok(Value::Vec2(V2 { x: -a.x, y: -a.y })),
                        Value::Vec3(a) => Ok(Value::vec3(-a.x, -a.y, -a.z)),
                        other => rt(ErrorKind::Type, format!("cannot negate a {}", other.type_name())),
                    },
                }
            }
            ExprKind::Binary(op, l, r) => {
                let a = self.eval(l)?;
                let b = self.eval(r)?;
                crate::builtins::binary_op(*op, &a, &b).map_err(Ctrl::Error)
            }
            ExprKind::And(l, r) => {
                let a = self.eval(l)?;
                if !a.truthy() {
                    return Ok(a);
                }
                self.eval(r)
            }
            ExprKind::Or(l, r) => {
                let a = self.eval(l)?;
                if a.truthy() {
                    return Ok(a);
                }
                self.eval(r)
            }
            ExprKind::CallName(name, args) => {
                let args = self.eval_args(args)?;
                self.call_by_name(name, args, e.span)
            }
            ExprKind::CallMethod(obj, name, args) => {
                let recv = self.eval(obj)?;
                let args = self.eval_args(args)?;
                self.invoke_method(&recv, name, args, e.span)
            }
            ExprKind::CallValue(callee, args) => {
                let f = self.eval(callee)?;
                let args = self.eval_args(args)?;
                self.call_callable(&f, args, e.span)
            }
            ExprKind::Super(args) => {
                let args = self.eval_args(args)?;
                let (class, name, self_val) = {
                    let f = self.frame();
                    (f.class.clone(), f.func_name.clone(), f.self_val.clone())
                };
                let class = match class {
                    Some(c) => c,
                    None => return rt(ErrorKind::Name, "'super' can only be used inside a class method"),
                };
                let method_name: Rc<str> = name.rsplit('.').next().unwrap_or(&name).into();
                let parent_method = class.parent.as_ref().and_then(|p| p.find_method(&method_name));
                match parent_method {
                    Some(m) => self.call_function(&m, self_val, args, e.span),
                    None => {
                        if let Some(h) = self_val.host() {
                            if let Some(r) = h.call_method(self, &method_name, &args) {
                                return r.map_err(Ctrl::Error);
                            }
                        }
                        rt(
                            ErrorKind::Method,
                            format!("no parent method '{}' for super() in class {}", method_name, class.name),
                        )
                    }
                }
            }
            ExprKind::Member(obj, name) => {
                let o = self.eval(obj)?;
                self.get_member(&o, name)
            }
            ExprKind::Index(obj, idx) => {
                let o = self.eval(obj)?;
                let i = self.eval(idx)?;
                crate::builtins::index_get(&o, &i).map_err(Ctrl::Error)
            }
            ExprKind::Assign(target, op, value) => self.assign(target, *op, value),
        }
    }

    fn eval_args(&mut self, args: &[Expr]) -> EResult<Vec<Value>> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            out.push(self.eval(a)?);
        }
        Ok(out)
    }

    fn lookup(&mut self, name: &Rc<str>) -> EResult<Value> {
        if let Some(v) = self.frame().locals.get(name) {
            return Ok(v.clone());
        }
        let is_const = name.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false);
        if is_const {
            // Constants resolve through the receiver's class first, so a subclass
            // can override a constant used by an inherited method.
            if let Value::Object(o) = &self.frame().self_val {
                if let Some(v) = o.class.find_constant(name) {
                    return Ok(v);
                }
            }
            if let Some(c) = &self.frame().class {
                if let Some(v) = c.find_constant(name) {
                    return Ok(v);
                }
            }
        }
        if let Some(m) = &self.frame().module {
            if let Some(v) = m.get(name) {
                return Ok(v);
            }
        }
        if let Some(v) = self.globals.get(name) {
            return Ok(v.clone());
        }
        let mut msg =
            if is_const { format!("undefined constant '{}'", name) } else { format!("undefined variable '{}'", name) };
        let self_val = self.frame().self_val.clone();
        if let Value::Object(o) = &self_val {
            if o.class.find_method(name).is_some() {
                msg.push_str(&format!(" (to call the method, write {}())", name));
            } else if o.fields.borrow().contains_key(name) {
                msg.push_str(&format!(" (did you mean @{}?)", name));
            } else if self_val.host().and_then(|h| h.get_property(name)).is_some() {
                msg.push_str(&format!(" (did you mean self.{}?)", name));
            }
        }
        rt(ErrorKind::Name, msg)
    }

    fn get_ivar(&mut self, name: &Rc<str>) -> EResult<Value> {
        match &self.frame().self_val {
            Value::Object(o) => Ok(o.fields.borrow().get(name).cloned().unwrap_or(Value::Nil)),
            _ => rt(ErrorKind::Name, format!("@{} can only be used inside a class method", name)),
        }
    }

    fn assign(&mut self, target: &Expr, op: Option<BinOp>, value: &Expr) -> EResult<Value> {
        // Compute new value (for compound assignment read the old value first).
        let compute = |this: &mut Self, old: Option<Value>| -> EResult<Value> {
            let rhs = this.eval(value)?;
            match (op, old) {
                (None, _) => Ok(rhs),
                (Some(op), Some(old)) => crate::builtins::binary_op(op, &old, &rhs).map_err(Ctrl::Error),
                (Some(_), None) => unreachable!(),
            }
        };
        match &target.kind {
            ExprKind::Ident(name) => {
                let is_const = name.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false);
                if is_const {
                    if op.is_some() || self.globals.contains_key(name) && self.frame().top_level {
                        return rt(ErrorKind::Name, format!("cannot reassign constant '{}'", name));
                    }
                    if !self.frame().top_level {
                        return rt(
                            ErrorKind::Name,
                            format!(
                                "constants like '{}' can only be defined at the top level or in a class/module body",
                                name
                            ),
                        );
                    }
                    let v = compute(self, None)?;
                    self.globals.insert(name.clone(), v.clone());
                    return Ok(v);
                }
                let old = if op.is_some() { Some(self.lookup(name)?) } else { None };
                let v = compute(self, old)?;
                self.frame_mut().locals.insert(name.clone(), v.clone());
                Ok(v)
            }
            ExprKind::IVar(name) => {
                let obj = match &self.frame().self_val {
                    Value::Object(o) => o.clone(),
                    _ => return rt(ErrorKind::Name, format!("@{} can only be used inside a class method", name)),
                };
                let old = if op.is_some() {
                    Some(obj.fields.borrow().get(name).cloned().unwrap_or(Value::Nil))
                } else {
                    None
                };
                let v = compute(self, old)?;
                obj.fields.borrow_mut().insert(name.clone(), v.clone());
                Ok(v)
            }
            ExprKind::GVar(name) => {
                let old = if op.is_some() { Some(self.gvars.get(name).cloned().unwrap_or(Value::Nil)) } else { None };
                let v = compute(self, old)?;
                self.gvars.insert(name.clone(), v.clone());
                Ok(v)
            }
            ExprKind::Member(obj, name) => {
                let o = self.eval(obj)?;
                let old = if op.is_some() { Some(self.get_member(&o, name)?) } else { None };
                let v = compute(self, old)?;
                self.set_member(&o, name, v.clone())?;
                Ok(v)
            }
            ExprKind::Index(obj, idx) => {
                let o = self.eval(obj)?;
                let i = self.eval(idx)?;
                let old =
                    if op.is_some() { Some(crate::builtins::index_get(&o, &i).map_err(Ctrl::Error)?) } else { None };
                let v = compute(self, old)?;
                crate::builtins::index_set(&o, &i, v.clone()).map_err(Ctrl::Error)?;
                Ok(v)
            }
            _ => rt(ErrorKind::Syntax, "invalid assignment target"),
        }
    }

    pub(crate) fn get_member(&mut self, o: &Value, name: &str) -> EResult<Value> {
        match o {
            Value::Module(m) => match m.get(name) {
                Some(v) => Ok(v),
                None => rt(ErrorKind::Property, format!("module {} has no member '{}'", m.name, name)),
            },
            Value::Class(c) => match c.find_constant(name) {
                Some(v) => Ok(v),
                None => {
                    if name == "name" {
                        return Ok(Value::Str(c.name.clone()));
                    }
                    rt(ErrorKind::Property, format!("class {} has no constant '{}'", c.name, name))
                }
            },
            Value::Object(obj) => {
                if let Some(h) = obj.host.borrow().clone() {
                    if let Some(r) = h.get_property(name) {
                        return r.map_err(Ctrl::Error);
                    }
                }
                if let Some(v) = obj.fields.borrow().get(name) {
                    return Ok(v.clone());
                }
                if obj.class.find_method(name).is_some() {
                    return rt(
                        ErrorKind::Property,
                        format!("'{}' is a method on {}; call it with {}()", name, obj.class.name, name),
                    );
                }
                rt(ErrorKind::Property, format!("unknown property '{}' on {}", name, obj.class.name))
            }
            Value::Host(h) => {
                if let Some(r) = h.get_property(name) {
                    return r.map_err(Ctrl::Error);
                }
                if let Some(Value::Object(obj)) = h.script_object() {
                    if let Some(v) = obj.fields.borrow().get(name) {
                        return Ok(v.clone());
                    }
                    if obj.class.find_method(name).is_some() {
                        return rt(ErrorKind::Property, format!("'{}' is a method; call it with {}()", name, name));
                    }
                }
                rt(ErrorKind::Property, format!("unknown property '{}' on {}", name, h.type_name()))
            }
            _ => crate::builtins::value_property(o, name).map_err(Ctrl::Error),
        }
    }

    fn set_member(&mut self, o: &Value, name: &Rc<str>, v: Value) -> EResult<()> {
        match o {
            Value::Object(obj) => {
                if let Some(h) = obj.host.borrow().clone() {
                    if let Some(r) = h.set_property(name, v.clone()) {
                        return r.map_err(Ctrl::Error);
                    }
                }
                obj.fields.borrow_mut().insert(name.clone(), v);
                Ok(())
            }
            Value::Host(h) => {
                if let Some(r) = h.set_property(name, v.clone()) {
                    return r.map_err(Ctrl::Error);
                }
                if let Some(Value::Object(obj)) = h.script_object() {
                    obj.fields.borrow_mut().insert(name.clone(), v);
                    return Ok(());
                }
                rt(ErrorKind::Property, format!("cannot set unknown property '{}' on {}", name, h.type_name()))
            }
            Value::Vec2(_) | Value::Vec3(_) | Value::Color(_) => rt(
                ErrorKind::Property,
                format!(
                    "{} values are immutable; build a new one instead (e.g. v = v.with_{}(...))",
                    o.type_name(),
                    name
                ),
            ),
            Value::Module(m) => rt(ErrorKind::Property, format!("cannot assign to module member {}.{}", m.name, name)),
            Value::Hash(_) => rt(ErrorKind::Property, format!("use h[:{}] = value to set a hash entry", name)),
            other => rt(ErrorKind::Property, format!("cannot set property '{}' on a {}", name, other.type_name())),
        }
    }

    fn call_by_name(&mut self, name: &Rc<str>, args: Vec<Value>, span: Span) -> EResult<Value> {
        // 1. local variable holding a function
        if let Some(v) = self.frame().locals.get(name).cloned() {
            if v.is_callable() {
                return self.call_callable(&v, args, span);
            }
            return rt(ErrorKind::Type, format!("'{}' is a {}, not a function", name, v.type_name()));
        }
        // 2. method on self
        let self_val = self.frame().self_val.clone();
        if let Value::Object(o) = &self_val {
            if let Some(m) = o.class.find_method(name) {
                return self.call_function(&m, self_val.clone(), args, span);
            }
            if let Some(h) = o.host.borrow().clone() {
                if let Some(r) = h.call_method(self, name, &args) {
                    return r.map_err(Ctrl::Error);
                }
            }
        }
        // 3. module member, 4. global
        let f = self.frame().module.as_ref().and_then(|m| m.get(name)).or_else(|| self.globals.get(name).cloned());
        match f {
            Some(f @ (Value::Function(_) | Value::Native(_))) => self.call_callable(&f, args, span),
            Some(Value::Class(c)) => rt(ErrorKind::Type, format!("to create a {} write {}.new(...)", c.name, c.name)),
            Some(other) => rt(ErrorKind::Type, format!("'{}' is a {}, not a function", name, other.type_name())),
            None => rt(ErrorKind::Name, format!("undefined function '{}'", name)),
        }
    }

    pub(crate) fn call_callable(&mut self, f: &Value, args: Vec<Value>, span: Span) -> EResult<Value> {
        match f {
            Value::Function(func) => {
                let self_val = Value::Nil;
                self.call_function(func, self_val, args, span)
            }
            Value::Native(n) => self.call_native(n, args),
            other => rt(ErrorKind::Type, format!("cannot call a {} like a function", other.type_name())),
        }
    }

    pub(crate) fn call_native(&mut self, n: &Rc<NativeFunction>, args: Vec<Value>) -> EResult<Value> {
        if args.len() < n.min_args || n.max_args.map(|m| args.len() > m).unwrap_or(false) {
            let expected = match n.max_args {
                Some(m) if m == n.min_args => format!("{}", m),
                Some(m) => format!("{} to {}", n.min_args, m),
                None => format!("at least {}", n.min_args),
            };
            return rt(
                ErrorKind::Type,
                format!(
                    "{}() expects {} argument{}, got {}",
                    n.name,
                    expected,
                    if expected == "1" { "" } else { "s" },
                    args.len()
                ),
            );
        }
        (n.func)(self, &args).map_err(Ctrl::Error)
    }

    pub(crate) fn call_function(
        &mut self,
        func: &Rc<Function>,
        self_val: Value,
        mut args: Vec<Value>,
        span: Span,
    ) -> EResult<Value> {
        let decl = &func.decl;
        let required = decl.params.iter().filter(|p| p.default.is_none()).count();
        if args.len() < required || args.len() > decl.params.len() {
            let expected = if required == decl.params.len() {
                format!("{}", required)
            } else {
                format!("{} to {}", required, decl.params.len())
            };
            return Err(GroError::new(
                ErrorKind::Type,
                format!(
                    "{}() expects {} argument{}, got {}",
                    func.qualified_name(),
                    expected,
                    if expected == "1" { "" } else { "s" },
                    args.len()
                ),
            )
            .at(span)
            .into());
        }
        if self.frames.len() >= self.max_depth {
            return rt(
                ErrorKind::Runtime,
                format!("stack overflow: too much recursion in {}()", func.qualified_name()),
            );
        }
        let qname: Rc<str> = func.qualified_name().into();
        let mut frame = Frame {
            locals: HashMap::new(),
            self_val,
            module: func.module.clone(),
            class: func.class.clone(),
            func_name: qname.clone(),
            file: func.file.clone(),
            source: func.source.clone(),
            top_level: false,
        };
        let extra: Vec<Value> = std::mem::take(&mut args);
        let mut given = extra.into_iter();
        let mut pending_defaults = Vec::new();
        for p in &decl.params {
            match given.next() {
                Some(v) => {
                    frame.locals.insert(p.name.clone(), v);
                }
                None => pending_defaults.push(p),
            }
        }
        self.frames.push(frame);
        // Defaults are evaluated inside the callee frame (can see earlier params).
        for p in pending_defaults {
            let v = match self.eval(p.default.as_ref().unwrap()) {
                Ok(v) => v,
                Err(e) => {
                    self.frames.pop();
                    return Err(e);
                }
            };
            self.frame_mut().locals.insert(p.name.clone(), v);
        }
        let result = self.exec_block(&decl.body);
        self.frames.pop();
        match result {
            Ok(()) => Ok(Value::Nil),
            Err(Ctrl::Return(v)) => Ok(v),
            Err(Ctrl::Error(mut e)) => {
                if e.file.is_none() {
                    e = e.in_file(&func.file, &func.source);
                }
                e.stack.push(qname.to_string());
                Err(Ctrl::Error(e))
            }
            Err(Ctrl::Break) | Err(Ctrl::Next) => Ok(Value::Nil),
        }
    }

    pub(crate) fn invoke_method(&mut self, recv: &Value, name: &str, args: Vec<Value>, span: Span) -> EResult<Value> {
        match recv {
            Value::Module(m) => match m.get(name) {
                Some(f @ (Value::Function(_) | Value::Native(_))) => self.call_callable(&f, args, span),
                Some(Value::Class(c)) => {
                    rt(ErrorKind::Type, format!("to create a {} write {}.{}.new(...)", c.name, m.name, c.name))
                }
                Some(other) => rt(
                    ErrorKind::Type,
                    format!("{}.{} is a {}, not a function (remove the parentheses)", m.name, name, other.type_name()),
                ),
                None => rt(ErrorKind::Method, format!("module {} has no function '{}'", m.name, name)),
            },
            Value::Class(c) => {
                if name == "new" {
                    return self.instantiate(c, args, span);
                }
                rt(
                    ErrorKind::Method,
                    format!("class {} has no class method '{}' (only new() is available)", c.name, name),
                )
            }
            Value::Object(o) => {
                if let Some(m) = o.class.find_method(name) {
                    return self.call_function(&m, recv.clone(), args, span);
                }
                if let Some(h) = o.host.borrow().clone() {
                    if let Some(r) = h.call_method(self, name, &args) {
                        return r.map_err(Ctrl::Error);
                    }
                }
                if let Some(r) = self.common_method(recv, name, &args) {
                    return r;
                }
                if o.fields.borrow().contains_key(name) {
                    return rt(
                        ErrorKind::Method,
                        format!("'{}' is a property of {}, not a method; remove the parentheses", name, o.class.name),
                    );
                }
                rt(ErrorKind::Method, format!("Method '{}' does not exist on {}.", name, o.class.name))
            }
            Value::Host(h) => {
                if let Some(r) = h.call_method(self, name, &args) {
                    return r.map_err(Ctrl::Error);
                }
                if let Some(obj) = h.script_object() {
                    if let Value::Object(o) = &obj {
                        if let Some(m) = o.class.find_method(name) {
                            return self.call_function(&m, obj.clone(), args, span);
                        }
                    }
                }
                if let Some(r) = self.common_method(recv, name, &args) {
                    return r;
                }
                rt(ErrorKind::Method, format!("Method '{}' does not exist on {}.", name, h.type_name()))
            }
            Value::Function(_) | Value::Native(_) if name == "call" => self.call_callable(recv, args, span),
            _ => {
                if let Some(r) = self.common_method(recv, name, &args) {
                    return r;
                }
                crate::builtins::value_method(self, recv, name, &args).map_err(Ctrl::Error)
            }
        }
    }

    /// Methods every value understands.
    fn common_method(&mut self, recv: &Value, name: &str, args: &[Value]) -> Option<EResult<Value>> {
        match (name, args.len()) {
            ("is_a?", 1) => Some(Ok(Value::Bool(match (&args[0], recv) {
                (Value::Class(c), Value::Object(o)) => o.class.is_subclass_of(&c.name),
                (Value::Class(c), other) => other.type_name() == *c.name,
                (Value::Str(s), other) => other.type_name() == **s,
                _ => false,
            }))),
            ("nil?", 0) => Some(Ok(Value::Bool(matches!(recv, Value::Nil)))),
            ("to_s", 0) => Some(Ok(Value::Str(recv.to_display().into()))),
            ("inspect", 0) => Some(Ok(Value::Str(recv.inspect().into()))),
            _ => None,
        }
    }

    fn instantiate(&mut self, c: &Rc<Class>, args: Vec<Value>, span: Span) -> EResult<Value> {
        let obj = self.new_object(c, None);
        match c.find_method("initialize") {
            Some(init) => {
                self.call_function(&init, obj.clone(), args, span)?;
            }
            None => {
                if !args.is_empty() {
                    return rt(
                        ErrorKind::Type,
                        format!(
                            "{}.new() got {} argument(s) but {} has no initialize() method",
                            c.name,
                            args.len(),
                            c.name
                        ),
                    );
                }
            }
        }
        Ok(obj)
    }

    /// The current `self` (useful for natives like Timer that capture a receiver).
    pub fn current_self(&self) -> Value {
        self.frames.last().map(|f| f.self_val.clone()).unwrap_or(Value::Nil)
    }
}
