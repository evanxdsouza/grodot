//! Runtime values.

use crate::ast::FunctionDecl;
use crate::error::GroResult;
use crate::interpreter::Interpreter;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;

/// A 2D vector value.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct V2 {
    pub x: f64,
    pub y: f64,
}

/// A 3D vector value.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl V3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        V3 { x, y, z }
    }
}

/// An RGBA colour, components normalised to 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

/// Objects provided by the host engine (entities, components, resources).
///
/// Groscript never sees engine internals: it only talks to this trait.
pub trait HostObject {
    fn type_name(&self) -> String;
    /// Read a property. `None` means "no such property".
    fn get_property(&self, name: &str) -> Option<GroResult<Value>>;
    /// Write a property. `None` means "no such property".
    fn set_property(&self, name: &str, value: Value) -> Option<GroResult<()>>;
    /// Call a method. `None` means "no such method".
    fn call_method(&self, interp: &mut Interpreter, name: &str, args: &[Value]) -> Option<GroResult<Value>>;
    /// Stable identity used for equality.
    fn identity(&self) -> u64;
    /// A script object attached to this host object (for entities with scripts).
    fn script_object(&self) -> Option<Value> {
        None
    }
    /// Short description for `inspect`.
    fn describe(&self) -> String {
        format!("#<{}>", self.type_name())
    }
    fn as_any(&self) -> &dyn Any;
}

pub type NativeFn = dyn Fn(&mut Interpreter, &[Value]) -> GroResult<Value>;

pub struct NativeFunction {
    pub name: Rc<str>,
    /// Minimum and maximum argument counts (`None` = unlimited).
    pub min_args: usize,
    pub max_args: Option<usize>,
    pub func: Box<NativeFn>,
}

impl std::fmt::Debug for NativeFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<native {}>", self.name)
    }
}

#[derive(Debug)]
pub struct Function {
    pub decl: Rc<FunctionDecl>,
    pub module: Option<Rc<Module>>,
    pub class: Option<Rc<Class>>,
    pub file: Rc<str>,
    pub source: Rc<str>,
}

impl Function {
    pub fn qualified_name(&self) -> String {
        if let Some(c) = &self.class {
            format!("{}.{}", c.name, self.decl.name)
        } else if let Some(m) = &self.module {
            format!("{}.{}", m.name, self.decl.name)
        } else {
            self.decl.name.to_string()
        }
    }
}

#[derive(Debug)]
pub struct Class {
    pub name: Rc<str>,
    pub parent: Option<Rc<Class>>,
    pub methods: RefCell<HashMap<Rc<str>, Rc<Function>>>,
    pub constants: RefCell<HashMap<Rc<str>, Value>>,
    /// Native classes (like `Entity`) are provided by the host.
    pub native: bool,
}

impl Class {
    pub fn find_method(&self, name: &str) -> Option<Rc<Function>> {
        if let Some(m) = self.methods.borrow().get(name) {
            return Some(m.clone());
        }
        self.parent.as_ref().and_then(|p| p.find_method(name))
    }

    pub fn find_constant(&self, name: &str) -> Option<Value> {
        if let Some(v) = self.constants.borrow().get(name) {
            return Some(v.clone());
        }
        self.parent.as_ref().and_then(|p| p.find_constant(name))
    }

    pub fn is_subclass_of(&self, name: &str) -> bool {
        if &*self.name == name {
            return true;
        }
        self.parent.as_ref().map(|p| p.is_subclass_of(name)).unwrap_or(false)
    }
}

#[derive(Debug)]
pub struct Module {
    pub name: Rc<str>,
    pub members: RefCell<HashMap<Rc<str>, Value>>,
}

impl Module {
    pub fn new(name: &str) -> Rc<Module> {
        Rc::new(Module { name: name.into(), members: RefCell::new(HashMap::new()) })
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        self.members.borrow().get(name).cloned()
    }

    pub fn set(&self, name: &str, v: Value) {
        self.members.borrow_mut().insert(name.into(), v);
    }

    /// Register a native function member.
    pub fn native<F>(&self, name: &str, min: usize, max: Option<usize>, f: F)
    where
        F: Fn(&mut Interpreter, &[Value]) -> GroResult<Value> + 'static,
    {
        let full = format!("{}.{}", self.name, name);
        self.set(name, Value::native(&full, min, max, f));
    }
}

pub struct Object {
    pub class: Rc<Class>,
    pub fields: RefCell<HashMap<Rc<str>, Value>>,
    pub host: RefCell<Option<Rc<dyn HostObject>>>,
}

impl std::fmt::Debug for Object {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#<{}>", self.class.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HashKey {
    Nil,
    Bool(bool),
    Num(u64),
    Str(Rc<str>),
    Sym(Rc<str>),
}

impl HashKey {
    pub fn to_value(&self) -> Value {
        match self {
            HashKey::Nil => Value::Nil,
            HashKey::Bool(b) => Value::Bool(*b),
            HashKey::Num(bits) => Value::Number(f64::from_bits(*bits)),
            HashKey::Str(s) => Value::Str(s.clone()),
            HashKey::Sym(s) => Value::Symbol(s.clone()),
        }
    }
}

/// An insertion-ordered hash map.
#[derive(Debug, Default, Clone)]
pub struct GroHash {
    pub keys: Vec<HashKey>,
    pub map: HashMap<HashKey, Value>,
}

impl GroHash {
    pub fn get(&self, k: &HashKey) -> Option<&Value> {
        self.map.get(k)
    }
    pub fn insert(&mut self, k: HashKey, v: Value) {
        if self.map.insert(k.clone(), v).is_none() {
            self.keys.push(k);
        }
    }
    pub fn remove(&mut self, k: &HashKey) -> Option<Value> {
        let v = self.map.remove(k)?;
        self.keys.retain(|x| x != k);
        Some(v)
    }
    pub fn len(&self) -> usize {
        self.keys.len()
    }
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
    pub fn clear(&mut self) {
        self.keys.clear();
        self.map.clear();
    }
}

#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Number(f64),
    Str(Rc<str>),
    Symbol(Rc<str>),
    Array(Rc<RefCell<Vec<Value>>>),
    Hash(Rc<RefCell<GroHash>>),
    Function(Rc<Function>),
    Native(Rc<NativeFunction>),
    Class(Rc<Class>),
    Object(Rc<Object>),
    Module(Rc<Module>),
    Vec2(V2),
    Vec3(V3),
    Color(Rgba),
    Host(Rc<dyn HostObject>),
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inspect())
    }
}

/// Format a vector/colour component: rounded to 4 decimals so f32 noise does not show.
pub fn fmt_component(n: f64) -> String {
    fmt_num((n * 10000.0).round() / 10000.0 + 0.0)
}

pub fn fmt_num(n: f64) -> String {
    if n.is_nan() {
        "nan".into()
    } else if n.is_infinite() {
        if n > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        }
    } else if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        let s = format!("{}", n);
        s
    }
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(s.into())
    }

    pub fn sym(s: &str) -> Value {
        Value::Symbol(s.into())
    }

    pub fn array(items: Vec<Value>) -> Value {
        Value::Array(Rc::new(RefCell::new(items)))
    }

    pub fn hash(h: GroHash) -> Value {
        Value::Hash(Rc::new(RefCell::new(h)))
    }

    pub fn vec3(x: f64, y: f64, z: f64) -> Value {
        Value::Vec3(V3 { x, y, z })
    }

    pub fn native<F>(name: &str, min: usize, max: Option<usize>, f: F) -> Value
    where
        F: Fn(&mut Interpreter, &[Value]) -> GroResult<Value> + 'static,
    {
        Value::Native(Rc::new(NativeFunction { name: name.into(), min_args: min, max_args: max, func: Box::new(f) }))
    }

    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false))
    }

    pub fn type_name(&self) -> String {
        match self {
            Value::Nil => "Nil".into(),
            Value::Bool(_) => "Boolean".into(),
            Value::Number(_) => "Number".into(),
            Value::Str(_) => "String".into(),
            Value::Symbol(_) => "Symbol".into(),
            Value::Array(_) => "Array".into(),
            Value::Hash(_) => "Hash".into(),
            Value::Function(_) | Value::Native(_) => "Function".into(),
            Value::Class(_) => "Class".into(),
            Value::Object(o) => o.class.name.to_string(),
            Value::Module(_) => "Module".into(),
            Value::Vec2(_) => "Vector2".into(),
            Value::Vec3(_) => "Vector3".into(),
            Value::Color(_) => "Color".into(),
            Value::Host(h) => h.type_name(),
        }
    }

    pub fn is_callable(&self) -> bool {
        matches!(self, Value::Function(_) | Value::Native(_))
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn hash_key(&self) -> Option<HashKey> {
        Some(match self {
            Value::Nil => HashKey::Nil,
            Value::Bool(b) => HashKey::Bool(*b),
            Value::Number(n) => {
                let n = if *n == 0.0 { 0.0 } else { *n };
                HashKey::Num(n.to_bits())
            }
            Value::Str(s) => HashKey::Str(s.clone()),
            Value::Symbol(s) => HashKey::Sym(s.clone()),
            _ => return None,
        })
    }

    /// The text used by `print` and string interpolation.
    pub fn to_display(&self) -> String {
        match self {
            Value::Str(s) => s.to_string(),
            Value::Symbol(s) => s.to_string(),
            _ => self.inspect(),
        }
    }

    /// A developer-oriented representation (strings are quoted).
    pub fn inspect(&self) -> String {
        let mut out = String::new();
        self.inspect_into(&mut out, 0);
        out
    }

    fn inspect_into(&self, out: &mut String, depth: usize) {
        if depth > 16 {
            out.push_str("...");
            return;
        }
        match self {
            Value::Nil => out.push_str("nil"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Number(n) => out.push_str(&fmt_num(*n)),
            Value::Str(s) => {
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\t' => out.push_str("\\t"),
                        c => out.push(c),
                    }
                }
                out.push('"');
            }
            Value::Symbol(s) => {
                out.push(':');
                out.push_str(s);
            }
            Value::Array(a) => {
                out.push('[');
                for (i, v) in a.borrow().iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    v.inspect_into(out, depth + 1);
                }
                out.push(']');
            }
            Value::Hash(h) => {
                let h = h.borrow();
                if h.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (i, k) in h.keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push(' ');
                    match k {
                        HashKey::Sym(s) => {
                            out.push_str(s);
                        }
                        other => other.to_value().inspect_into(out, depth + 1),
                    }
                    out.push_str(": ");
                    h.map[k].inspect_into(out, depth + 1);
                }
                out.push_str(" }");
            }
            Value::Function(f) => {
                let _ = write!(out, "#<function {}>", f.qualified_name());
            }
            Value::Native(f) => {
                let _ = write!(out, "#<function {}>", f.name);
            }
            Value::Class(c) => out.push_str(&c.name),
            Value::Module(m) => out.push_str(&m.name),
            Value::Object(o) => {
                if let Some(h) = o.host.borrow().as_ref() {
                    let _ = write!(out, "#<{} {}>", o.class.name, h.describe());
                    return;
                }
                let _ = write!(out, "#<{}", o.class.name);
                let fields = o.fields.borrow();
                let mut names: Vec<_> = fields.keys().cloned().collect();
                names.sort();
                for (i, k) in names.iter().enumerate() {
                    out.push_str(if i == 0 { " " } else { ", " });
                    let _ = write!(out, "@{}=", k);
                    fields[k].inspect_into(out, depth + 1);
                }
                out.push('>');
            }
            Value::Vec2(v) => {
                let _ = write!(out, "Vector2({}, {})", fmt_component(v.x), fmt_component(v.y));
            }
            Value::Vec3(v) => {
                let _ = write!(out, "Vector3({}, {}, {})", fmt_component(v.x), fmt_component(v.y), fmt_component(v.z));
            }
            Value::Color(c) => {
                let _ = write!(
                    out,
                    "Color({}, {}, {}, {})",
                    fmt_component(c.r),
                    fmt_component(c.g),
                    fmt_component(c.b),
                    fmt_component(c.a)
                );
            }
            Value::Host(h) => out.push_str(&h.describe()),
        }
    }

    /// The host object behind this value, if any (directly or via a script object).
    pub fn host(&self) -> Option<Rc<dyn HostObject>> {
        match self {
            Value::Host(h) => Some(h.clone()),
            Value::Object(o) => o.host.borrow().clone(),
            _ => None,
        }
    }
}

/// Structural equality as defined by the language spec.
pub fn values_equal(a: &Value, b: &Value) -> bool {
    eq_depth(a, b, 0)
}

fn eq_depth(a: &Value, b: &Value, depth: usize) -> bool {
    if depth > 64 {
        return false;
    }
    match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Symbol(x), Value::Symbol(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => {
            if Rc::ptr_eq(x, y) {
                return true;
            }
            let (x, y) = (x.borrow(), y.borrow());
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| eq_depth(a, b, depth + 1))
        }
        (Value::Hash(x), Value::Hash(y)) => {
            if Rc::ptr_eq(x, y) {
                return true;
            }
            let (x, y) = (x.borrow(), y.borrow());
            x.len() == y.len()
                && x.keys.iter().all(|k| match y.get(k) {
                    Some(v) => eq_depth(&x.map[k], v, depth + 1),
                    None => false,
                })
        }
        (Value::Function(x), Value::Function(y)) => Rc::ptr_eq(x, y),
        (Value::Native(x), Value::Native(y)) => Rc::ptr_eq(x, y),
        (Value::Class(x), Value::Class(y)) => Rc::ptr_eq(x, y),
        (Value::Module(x), Value::Module(y)) => Rc::ptr_eq(x, y),
        (Value::Vec2(x), Value::Vec2(y)) => x == y,
        (Value::Vec3(x), Value::Vec3(y)) => x == y,
        (Value::Color(x), Value::Color(y)) => x == y,
        (Value::Object(x), Value::Object(y)) if Rc::ptr_eq(x, y) => true,
        _ => match (a.host(), b.host()) {
            (Some(x), Some(y)) => x.identity() == y.identity() && x.type_name() == y.type_name(),
            _ => false,
        },
    }
}
