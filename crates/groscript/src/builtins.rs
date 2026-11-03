//! Built-in functions, operators and methods for core value types.

use crate::ast::BinOp;
use crate::error::{err, ErrorKind, GroError, GroResult};
use crate::interpreter::Interpreter;
use crate::value::*;
use std::rc::Rc;

fn type_err<T>(msg: impl Into<String>) -> GroResult<T> {
    err(ErrorKind::Type, msg)
}

pub fn num_arg(args: &[Value], i: usize, fname: &str) -> GroResult<f64> {
    match args.get(i) {
        Some(Value::Number(n)) => Ok(*n),
        Some(other) => type_err(format!("{}() argument {} must be a Number, got {}", fname, i + 1, other.type_name())),
        None => type_err(format!("{}() is missing argument {}", fname, i + 1)),
    }
}

pub fn str_arg(args: &[Value], i: usize, fname: &str) -> GroResult<Rc<str>> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s.clone()),
        Some(Value::Symbol(s)) => Ok(s.clone()),
        Some(other) => type_err(format!("{}() argument {} must be a String, got {}", fname, i + 1, other.type_name())),
        None => type_err(format!("{}() is missing argument {}", fname, i + 1)),
    }
}

pub fn vec3_arg(args: &[Value], i: usize, fname: &str) -> GroResult<V3> {
    match args.get(i) {
        Some(Value::Vec3(v)) => Ok(*v),
        Some(other) => type_err(format!("{}() argument {} must be a Vector3, got {}", fname, i + 1, other.type_name())),
        None => type_err(format!("{}() is missing argument {}", fname, i + 1)),
    }
}

pub fn install(it: &mut Interpreter) {
    it.set_global(
        "print",
        Value::native("print", 0, None, |it, args| {
            let line: Vec<String> = args.iter().map(|v| v.to_display()).collect();
            it.print_line(&line.join(" "));
            Ok(Value::Nil)
        }),
    );
    it.set_global("inspect", Value::native("inspect", 1, Some(1), |_, args| Ok(Value::Str(args[0].inspect().into()))));
    it.set_global(
        "assert",
        Value::native("assert", 1, Some(2), |_, args| {
            if args[0].truthy() {
                Ok(Value::Nil)
            } else {
                let msg = match args.get(1) {
                    Some(m) => m.to_display(),
                    None => "assertion failed".into(),
                };
                err(ErrorKind::Assertion, msg)
            }
        }),
    );
    it.set_global("type", Value::native("type", 1, Some(1), |_, args| Ok(Value::Str(args[0].type_name().into()))));
    it.set_global("str", Value::native("str", 1, Some(1), |_, args| Ok(Value::Str(args[0].to_display().into()))));
    it.set_global(
        "num",
        Value::native("num", 1, Some(1), |_, args| match &args[0] {
            Value::Number(n) => Ok(Value::Number(*n)),
            Value::Str(s) => Ok(s.trim().parse::<f64>().map(Value::Number).unwrap_or(Value::Nil)),
            Value::Bool(b) => Ok(Value::Number(if *b { 1.0 } else { 0.0 })),
            other => type_err(format!("num() cannot convert a {}", other.type_name())),
        }),
    );
    it.set_global(
        "range",
        Value::native("range", 1, Some(3), |_, args| {
            let (start, stop) = if args.len() == 1 {
                (0.0, num_arg(args, 0, "range")?)
            } else {
                (num_arg(args, 0, "range")?, num_arg(args, 1, "range")?)
            };
            let step = if args.len() == 3 { num_arg(args, 2, "range")? } else { 1.0 };
            if step == 0.0 {
                return err(ErrorKind::Runtime, "range() step cannot be 0");
            }
            let mut out = Vec::new();
            let mut x = start;
            while (step > 0.0 && x < stop) || (step < 0.0 && x > stop) {
                out.push(Value::Number(x));
                x += step;
                if out.len() > 10_000_000 {
                    return err(ErrorKind::Runtime, "range() is too large");
                }
            }
            Ok(Value::array(out))
        }),
    );

    // ---------------------------------------------------------------- Math
    let math = Module::new("Math");
    math.set("PI", Value::Number(std::f64::consts::PI));
    math.set("INFINITY", Value::Number(f64::INFINITY));
    fn unary(m: &Module, name: &'static str, f: fn(f64) -> f64) {
        m.native(name, 1, Some(1), move |_, a| Ok(Value::Number(f(num_arg(a, 0, name)?))));
    }
    unary(&math, "abs", f64::abs);
    unary(&math, "floor", f64::floor);
    unary(&math, "ceil", f64::ceil);
    unary(&math, "round", f64::round);
    unary(&math, "sin", f64::sin);
    unary(&math, "cos", f64::cos);
    unary(&math, "tan", f64::tan);
    unary(&math, "sign", |x| {
        if x > 0.0 {
            1.0
        } else if x < 0.0 {
            -1.0
        } else {
            0.0
        }
    });
    unary(&math, "deg_to_rad", f64::to_radians);
    unary(&math, "rad_to_deg", f64::to_degrees);
    math.native("sqrt", 1, Some(1), |_, a| {
        let x = num_arg(a, 0, "Math.sqrt")?;
        if x < 0.0 {
            return err(ErrorKind::Runtime, format!("Math.sqrt() of a negative number ({})", fmt_num(x)));
        }
        Ok(Value::Number(x.sqrt()))
    });
    math.native("atan2", 2, Some(2), |_, a| {
        Ok(Value::Number(num_arg(a, 0, "Math.atan2")?.atan2(num_arg(a, 1, "Math.atan2")?)))
    });
    math.native("min", 1, None, |_, a| {
        let mut m = f64::INFINITY;
        for i in 0..a.len() {
            m = m.min(num_arg(a, i, "Math.min")?);
        }
        Ok(Value::Number(m))
    });
    math.native("max", 1, None, |_, a| {
        let mut m = f64::NEG_INFINITY;
        for i in 0..a.len() {
            m = m.max(num_arg(a, i, "Math.max")?);
        }
        Ok(Value::Number(m))
    });
    math.native("clamp", 3, Some(3), |_, a| {
        let (v, lo, hi) = (num_arg(a, 0, "Math.clamp")?, num_arg(a, 1, "Math.clamp")?, num_arg(a, 2, "Math.clamp")?);
        Ok(Value::Number(v.max(lo).min(hi)))
    });
    math.native("lerp", 3, Some(3), |_, a| {
        let (x, y, t) = (num_arg(a, 0, "Math.lerp")?, num_arg(a, 1, "Math.lerp")?, num_arg(a, 2, "Math.lerp")?);
        Ok(Value::Number(x + (y - x) * t))
    });
    math.native("random", 0, Some(0), |it, _| Ok(Value::Number(it.next_random())));
    math.native("random_range", 2, Some(2), |it, a| {
        let (lo, hi) = (num_arg(a, 0, "Math.random_range")?, num_arg(a, 1, "Math.random_range")?);
        Ok(Value::Number(lo + (hi - lo) * it.next_random()))
    });
    math.native("random_int", 2, Some(2), |it, a| {
        let (lo, hi) = (num_arg(a, 0, "Math.random_int")?, num_arg(a, 1, "Math.random_int")?);
        Ok(Value::Number((lo + ((hi - lo + 1.0) * it.next_random()).floor()).min(hi)))
    });
    math.native("seed", 1, Some(1), |it, a| {
        it.seed_random(num_arg(a, 0, "Math.seed")? as u64);
        Ok(Value::Nil)
    });
    it.add_module(math);

    // ---------------------------------------------------------------- Vector types
    let v3 = Module::new("Vector3");
    v3.native("new", 0, Some(3), |_, a| {
        let g = |i| -> GroResult<f64> {
            if a.len() > i {
                num_arg(a, i, "Vector3.new")
            } else {
                Ok(0.0)
            }
        };
        Ok(Value::vec3(g(0)?, g(1)?, g(2)?))
    });
    v3.set("ZERO", Value::vec3(0.0, 0.0, 0.0));
    v3.set("ONE", Value::vec3(1.0, 1.0, 1.0));
    v3.set("UP", Value::vec3(0.0, 1.0, 0.0));
    v3.set("DOWN", Value::vec3(0.0, -1.0, 0.0));
    v3.set("LEFT", Value::vec3(-1.0, 0.0, 0.0));
    v3.set("RIGHT", Value::vec3(1.0, 0.0, 0.0));
    v3.set("FORWARD", Value::vec3(0.0, 0.0, -1.0));
    v3.set("BACK", Value::vec3(0.0, 0.0, 1.0));
    it.add_module(v3);

    let v2 = Module::new("Vector2");
    v2.native("new", 0, Some(2), |_, a| {
        let g = |i| -> GroResult<f64> {
            if a.len() > i {
                num_arg(a, i, "Vector2.new")
            } else {
                Ok(0.0)
            }
        };
        Ok(Value::Vec2(V2 { x: g(0)?, y: g(1)? }))
    });
    v2.set("ZERO", Value::Vec2(V2 { x: 0.0, y: 0.0 }));
    it.add_module(v2);

    let color = Module::new("Color");
    color.native("new", 3, Some(4), |_, a| {
        let alpha = if a.len() > 3 { num_arg(a, 3, "Color.new")? } else { 1.0 };
        Ok(Value::Color(Rgba {
            r: num_arg(a, 0, "Color.new")?,
            g: num_arg(a, 1, "Color.new")?,
            b: num_arg(a, 2, "Color.new")?,
            a: alpha,
        }))
    });
    color.native("hex", 1, Some(1), |_, a| {
        let s = str_arg(a, 0, "Color.hex")?;
        parse_hex_color(&s).map(Value::Color)
    });
    for (name, r, g, b) in [
        ("WHITE", 1.0, 1.0, 1.0),
        ("BLACK", 0.0, 0.0, 0.0),
        ("RED", 0.9, 0.2, 0.2),
        ("GREEN", 0.2, 0.8, 0.3),
        ("BLUE", 0.2, 0.4, 0.95),
        ("YELLOW", 1.0, 0.85, 0.2),
        ("ORANGE", 1.0, 0.55, 0.1),
        ("PURPLE", 0.6, 0.3, 0.9),
        ("GRAY", 0.5, 0.5, 0.5),
    ] {
        color.set(name, Value::Color(Rgba { r, g, b, a: 1.0 }));
    }
    it.add_module(color);
}

pub fn parse_hex_color(s: &str) -> GroResult<Rgba> {
    let h = s.trim_start_matches('#');
    let p = |i: usize| -> Option<f64> { u8::from_str_radix(h.get(i..i + 2)?, 16).ok().map(|v| v as f64 / 255.0) };
    let bad = || GroError::new(ErrorKind::Runtime, format!("invalid hex colour '{}' (use \"#rrggbb\")", s));
    if h.len() != 6 && h.len() != 8 {
        return Err(bad());
    }
    Ok(Rgba {
        r: p(0).ok_or_else(bad)?,
        g: p(2).ok_or_else(bad)?,
        b: p(4).ok_or_else(bad)?,
        a: if h.len() == 8 { p(6).ok_or_else(bad)? } else { 1.0 },
    })
}

// ------------------------------------------------------------------ operators

pub fn binary_op(op: BinOp, a: &Value, b: &Value) -> GroResult<Value> {
    use Value::*;
    match op {
        BinOp::Eq => return Ok(Bool(values_equal(a, b))),
        BinOp::Ne => return Ok(Bool(!values_equal(a, b))),
        _ => {}
    }
    match (op, a, b) {
        (_, Number(x), Number(y)) => {
            let (x, y) = (*x, *y);
            Ok(match op {
                BinOp::Add => Number(x + y),
                BinOp::Sub => Number(x - y),
                BinOp::Mul => Number(x * y),
                BinOp::Div => {
                    if y == 0.0 {
                        return err(ErrorKind::Runtime, "division by zero");
                    }
                    Number(x / y)
                }
                BinOp::Mod => {
                    if y == 0.0 {
                        return err(ErrorKind::Runtime, "modulo by zero");
                    }
                    Number(x - y * (x / y).floor())
                }
                BinOp::Pow => Number(x.powf(y)),
                BinOp::Lt => Bool(x < y),
                BinOp::Gt => Bool(x > y),
                BinOp::Le => Bool(x <= y),
                BinOp::Ge => Bool(x >= y),
                BinOp::Eq | BinOp::Ne => unreachable!(),
            })
        }
        (BinOp::Add, Str(x), Str(y)) => {
            let mut s = String::with_capacity(x.len() + y.len());
            s.push_str(x);
            s.push_str(y);
            Ok(Str(s.into()))
        }
        (BinOp::Mul, Str(x), Number(n)) => {
            if *n < 0.0 || *n > 1e6 {
                return err(ErrorKind::Runtime, "string repeat count must be between 0 and 1000000");
            }
            Ok(Str(x.repeat(*n as usize).into()))
        }
        (BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge, Str(x), Str(y)) => Ok(Bool(match op {
            BinOp::Lt => x < y,
            BinOp::Gt => x > y,
            BinOp::Le => x <= y,
            _ => x >= y,
        })),
        (BinOp::Add, Array(x), Array(y)) => {
            let mut v = x.borrow().clone();
            v.extend(y.borrow().iter().cloned());
            Ok(Value::array(v))
        }
        (_, Vec3(x), Vec3(y)) => match op {
            BinOp::Add => Ok(Value::vec3(x.x + y.x, x.y + y.y, x.z + y.z)),
            BinOp::Sub => Ok(Value::vec3(x.x - y.x, x.y - y.y, x.z - y.z)),
            BinOp::Mul => Ok(Value::vec3(x.x * y.x, x.y * y.y, x.z * y.z)),
            _ => op_error(op, a, b),
        },
        (BinOp::Mul, Vec3(v), Number(n)) | (BinOp::Mul, Number(n), Vec3(v)) => {
            Ok(Value::vec3(v.x * n, v.y * n, v.z * n))
        }
        (BinOp::Div, Vec3(v), Number(n)) => {
            if *n == 0.0 {
                return err(ErrorKind::Runtime, "division by zero");
            }
            Ok(Value::vec3(v.x / n, v.y / n, v.z / n))
        }
        (_, Vec2(x), Vec2(y)) => match op {
            BinOp::Add => Ok(Vec2(V2 { x: x.x + y.x, y: x.y + y.y })),
            BinOp::Sub => Ok(Vec2(V2 { x: x.x - y.x, y: x.y - y.y })),
            _ => op_error(op, a, b),
        },
        (BinOp::Mul, Vec2(v), Number(n)) | (BinOp::Mul, Number(n), Vec2(v)) => Ok(Vec2(V2 { x: v.x * n, y: v.y * n })),
        (BinOp::Div, Vec2(v), Number(n)) => {
            if *n == 0.0 {
                return err(ErrorKind::Runtime, "division by zero");
            }
            Ok(Vec2(V2 { x: v.x / n, y: v.y / n }))
        }
        (BinOp::Mul, Color(c), Number(n)) => Ok(Color(Rgba { r: c.r * n, g: c.g * n, b: c.b * n, a: c.a })),
        _ => op_error(op, a, b),
    }
}

fn op_error(op: BinOp, a: &Value, b: &Value) -> GroResult<Value> {
    let hint = match (op, a, b) {
        (BinOp::Add, Value::Str(_), _) | (BinOp::Add, _, Value::Str(_)) => {
            " (use interpolation like \"score: #{score}\" or str(value))"
        }
        _ => "",
    };
    type_err(format!("cannot use '{}' with {} and {}{}", op.symbol(), a.type_name(), b.type_name(), hint))
}

// ------------------------------------------------------------------ indexing

fn array_index(len: usize, i: &Value) -> GroResult<Option<usize>> {
    let n = match i {
        Value::Number(n) => *n,
        other => return type_err(format!("array index must be a Number, got {}", other.type_name())),
    };
    if n.fract() != 0.0 {
        return type_err(format!("array index must be a whole number, got {}", fmt_num(n)));
    }
    let idx = if n < 0.0 { len as f64 + n } else { n };
    if idx < 0.0 || idx >= len as f64 {
        return Ok(None);
    }
    Ok(Some(idx as usize))
}

pub fn index_get(o: &Value, i: &Value) -> GroResult<Value> {
    match o {
        Value::Array(a) => {
            let a = a.borrow();
            Ok(array_index(a.len(), i)?.map(|ix| a[ix].clone()).unwrap_or(Value::Nil))
        }
        Value::Hash(h) => {
            let key = i
                .hash_key()
                .ok_or_else(|| GroError::new(ErrorKind::Type, format!("a {} cannot be a hash key", i.type_name())))?;
            Ok(h.borrow().get(&key).cloned().unwrap_or(Value::Nil))
        }
        Value::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            Ok(array_index(chars.len(), i)?.map(|ix| Value::Str(chars[ix].to_string().into())).unwrap_or(Value::Nil))
        }
        other => type_err(format!("cannot index into a {}", other.type_name())),
    }
}

pub fn index_set(o: &Value, i: &Value, v: Value) -> GroResult<()> {
    match o {
        Value::Array(a) => {
            let mut a = a.borrow_mut();
            let len = a.len();
            if let Value::Number(n) = i {
                if *n == len as f64 {
                    a.push(v);
                    return Ok(());
                }
            }
            match array_index(len, i)? {
                Some(ix) => {
                    a[ix] = v;
                    Ok(())
                }
                None => {
                    err(ErrorKind::Runtime, format!("array index {} is out of range (length {})", i.inspect(), len))
                }
            }
        }
        Value::Hash(h) => {
            let key = i
                .hash_key()
                .ok_or_else(|| GroError::new(ErrorKind::Type, format!("a {} cannot be a hash key", i.type_name())))?;
            h.borrow_mut().insert(key, v);
            Ok(())
        }
        Value::Str(_) => type_err("strings are immutable; build a new string instead"),
        other => type_err(format!("cannot index into a {}", other.type_name())),
    }
}

// ------------------------------------------------------------------ properties

pub fn value_property(o: &Value, name: &str) -> GroResult<Value> {
    match (o, name) {
        (Value::Vec3(v), "x") => Ok(Value::Number(v.x)),
        (Value::Vec3(v), "y") => Ok(Value::Number(v.y)),
        (Value::Vec3(v), "z") => Ok(Value::Number(v.z)),
        (Value::Vec2(v), "x") => Ok(Value::Number(v.x)),
        (Value::Vec2(v), "y") => Ok(Value::Number(v.y)),
        (Value::Color(c), "r") => Ok(Value::Number(c.r)),
        (Value::Color(c), "g") => Ok(Value::Number(c.g)),
        (Value::Color(c), "b") => Ok(Value::Number(c.b)),
        (Value::Color(c), "a") => Ok(Value::Number(c.a)),
        (Value::Array(_) | Value::Str(_) | Value::Hash(_), _) if has_method(o, name) => {
            type_err(format!("'{}' is a method; call it with {}()", name, name))
        }
        (Value::Hash(_), _) => {
            err(ErrorKind::Property, format!("hashes have no properties; to read a key write h[:{}]", name))
        }
        (Value::Vec3(_) | Value::Vec2(_), _) if has_method(o, name) => {
            type_err(format!("'{}' is a method; call it with {}()", name, name))
        }
        _ => err(ErrorKind::Property, format!("unknown property '{}' on {}", name, o.type_name())),
    }
}

fn has_method(o: &Value, name: &str) -> bool {
    let list: &[&str] = match o {
        Value::Str(_) => &STRING_METHODS,
        Value::Array(_) => &ARRAY_METHODS,
        Value::Hash(_) => &HASH_METHODS,
        Value::Vec3(_) | Value::Vec2(_) => &VECTOR_METHODS,
        _ => &[],
    };
    list.contains(&name)
}

const STRING_METHODS: [&str; 14] = [
    "length",
    "upcase",
    "downcase",
    "contains?",
    "starts_with?",
    "ends_with?",
    "replace",
    "split",
    "trim",
    "empty?",
    "reverse",
    "chars",
    "to_n",
    "capitalize",
];
const ARRAY_METHODS: [&str; 18] = [
    "push",
    "pop",
    "remove",
    "remove_at",
    "contains?",
    "clear",
    "length",
    "empty?",
    "first",
    "last",
    "reverse",
    "sort",
    "join",
    "index_of",
    "insert",
    "shuffle",
    "sum",
    "copy",
];
const HASH_METHODS: [&str; 8] = ["keys", "values", "contains?", "remove", "clear", "length", "empty?", "get"];
const VECTOR_METHODS: [&str; 10] =
    ["length", "normalized", "dot", "cross", "distance_to", "lerp", "with_x", "with_y", "with_z", "abs"];

// ------------------------------------------------------------------ methods

fn arity(name: &str, args: &[Value], n: usize) -> GroResult<()> {
    if args.len() != n {
        return type_err(format!(
            "{}() expects {} argument{}, got {}",
            name,
            n,
            if n == 1 { "" } else { "s" },
            args.len()
        ));
    }
    Ok(())
}

fn no_method<T>(o: &Value, name: &str) -> GroResult<T> {
    let mut msg = format!("Method '{}' does not exist on {}.", name, o.type_name());
    if matches!(o, Value::Vec3(_) | Value::Vec2(_) | Value::Color(_)) && name.len() == 1 {
        msg.push_str(&format!(" ('{}' is a property; remove the parentheses)", name));
    }
    err(ErrorKind::Method, msg)
}

fn compare_for_sort(a: &Value, b: &Value) -> GroResult<std::cmp::Ordering> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => Ok(x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)),
        (Value::Str(x), Value::Str(y)) => Ok(x.cmp(y)),
        _ => type_err(format!("sort() cannot compare {} with {}", a.type_name(), b.type_name())),
    }
}

pub fn value_method(it: &mut Interpreter, o: &Value, name: &str, args: &[Value]) -> GroResult<Value> {
    match o {
        Value::Str(s) => string_method(s, name, args).unwrap_or_else(|| no_method(o, name)),
        Value::Array(a) => array_method(it, o, a, name, args).unwrap_or_else(|| no_method(o, name)),
        Value::Hash(h) => hash_method(h, name, args).unwrap_or_else(|| no_method(o, name)),
        Value::Vec3(v) => vec3_method(*v, name, args).unwrap_or_else(|| no_method(o, name)),
        Value::Vec2(v) => vec2_method(*v, name, args).unwrap_or_else(|| no_method(o, name)),
        Value::Color(c) => match name {
            "lerp" => Some((|| {
                arity("lerp", args, 2)?;
                let t = num_arg(args, 1, "lerp")?;
                match &args[0] {
                    Value::Color(d) => Ok(Value::Color(Rgba {
                        r: c.r + (d.r - c.r) * t,
                        g: c.g + (d.g - c.g) * t,
                        b: c.b + (d.b - c.b) * t,
                        a: c.a + (d.a - c.a) * t,
                    })),
                    other => type_err(format!("lerp() needs a Color, got {}", other.type_name())),
                }
            })()),
            _ => None,
        }
        .unwrap_or_else(|| no_method(o, name)),
        Value::Number(n) => match name {
            "floor" => Ok(Value::Number(n.floor())),
            "round" => Ok(Value::Number(n.round())),
            "abs" => Ok(Value::Number(n.abs())),
            _ => no_method(o, name),
        },
        _ => no_method(o, name),
    }
}

fn string_method(s: &Rc<str>, name: &str, args: &[Value]) -> Option<GroResult<Value>> {
    let r = (|| -> GroResult<Value> {
        Ok(match name {
            "length" => {
                arity(name, args, 0)?;
                Value::Number(s.chars().count() as f64)
            }
            "upcase" => Value::str(&s.to_uppercase()),
            "downcase" => Value::str(&s.to_lowercase()),
            "capitalize" => {
                let mut c = s.chars();
                match c.next() {
                    Some(f) => Value::str(&(f.to_uppercase().collect::<String>() + c.as_str())),
                    None => Value::str(""),
                }
            }
            "trim" => Value::str(s.trim()),
            "empty?" => Value::Bool(s.is_empty()),
            "reverse" => Value::str(&s.chars().rev().collect::<String>()),
            "chars" => Value::array(s.chars().map(|c| Value::str(&c.to_string())).collect()),
            "to_n" => s.trim().parse::<f64>().map(Value::Number).unwrap_or(Value::Nil),
            "contains?" => {
                arity(name, args, 1)?;
                Value::Bool(s.contains(&*str_arg(args, 0, name)?))
            }
            "starts_with?" => {
                arity(name, args, 1)?;
                Value::Bool(s.starts_with(&*str_arg(args, 0, name)?))
            }
            "ends_with?" => {
                arity(name, args, 1)?;
                Value::Bool(s.ends_with(&*str_arg(args, 0, name)?))
            }
            "replace" => {
                arity(name, args, 2)?;
                Value::str(&s.replace(&*str_arg(args, 0, name)?, &str_arg(args, 1, name)?))
            }
            "split" => {
                let parts: Vec<Value> = if args.is_empty() {
                    s.split_whitespace().map(Value::str).collect()
                } else {
                    let sep = str_arg(args, 0, name)?;
                    if sep.is_empty() {
                        s.chars().map(|c| Value::str(&c.to_string())).collect()
                    } else {
                        s.split(&*sep).map(Value::str).collect()
                    }
                };
                Value::array(parts)
            }
            _ => return Err(GroError::new(ErrorKind::Method, "")),
        })
    })();
    match r {
        Err(e) if e.message.is_empty() => None,
        other => Some(other),
    }
}

fn array_method(
    _it: &mut Interpreter,
    this: &Value,
    a: &Rc<std::cell::RefCell<Vec<Value>>>,
    name: &str,
    args: &[Value],
) -> Option<GroResult<Value>> {
    let r = (|| -> GroResult<Value> {
        Ok(match name {
            "push" => {
                if args.is_empty() {
                    return type_err("push() needs at least one value");
                }
                a.borrow_mut().extend(args.iter().cloned());
                this.clone()
            }
            "pop" => {
                arity(name, args, 0)?;
                a.borrow_mut().pop().unwrap_or(Value::Nil)
            }
            "remove" => {
                arity(name, args, 1)?;
                let mut v = a.borrow_mut();
                match v.iter().position(|x| values_equal(x, &args[0])) {
                    Some(i) => v.remove(i),
                    None => Value::Nil,
                }
            }
            "remove_at" => {
                arity(name, args, 1)?;
                let len = a.borrow().len();
                match array_index(len, &args[0])? {
                    Some(i) => a.borrow_mut().remove(i),
                    None => {
                        return err(
                            ErrorKind::Runtime,
                            format!("remove_at({}) is out of range (length {})", args[0].inspect(), len),
                        )
                    }
                }
            }
            "insert" => {
                arity(name, args, 2)?;
                let len = a.borrow().len();
                let i = num_arg(args, 0, name)?;
                if i < 0.0 || i > len as f64 || i.fract() != 0.0 {
                    return err(ErrorKind::Runtime, format!("insert() index {} is out of range", fmt_num(i)));
                }
                a.borrow_mut().insert(i as usize, args[1].clone());
                this.clone()
            }
            "contains?" => {
                arity(name, args, 1)?;
                Value::Bool(a.borrow().iter().any(|x| values_equal(x, &args[0])))
            }
            "index_of" => {
                arity(name, args, 1)?;
                match a.borrow().iter().position(|x| values_equal(x, &args[0])) {
                    Some(i) => Value::Number(i as f64),
                    None => Value::Nil,
                }
            }
            "clear" => {
                arity(name, args, 0)?;
                a.borrow_mut().clear();
                this.clone()
            }
            "length" => {
                arity(name, args, 0)?;
                Value::Number(a.borrow().len() as f64)
            }
            "empty?" => Value::Bool(a.borrow().is_empty()),
            "first" => a.borrow().first().cloned().unwrap_or(Value::Nil),
            "last" => a.borrow().last().cloned().unwrap_or(Value::Nil),
            "reverse" => {
                let mut v = a.borrow().clone();
                v.reverse();
                Value::array(v)
            }
            "copy" => Value::array(a.borrow().clone()),
            "sort" => {
                let mut v = a.borrow().clone();
                let mut error = None;
                v.sort_by(|x, y| match compare_for_sort(x, y) {
                    Ok(o) => o,
                    Err(e) => {
                        error.get_or_insert(e);
                        std::cmp::Ordering::Equal
                    }
                });
                if let Some(e) = error {
                    return Err(e);
                }
                Value::array(v)
            }
            "sum" => {
                let mut t = 0.0;
                for (i, x) in a.borrow().iter().enumerate() {
                    match x {
                        Value::Number(n) => t += n,
                        other => {
                            return type_err(format!(
                                "sum() needs an array of Numbers; item {} is a {}",
                                i,
                                other.type_name()
                            ))
                        }
                    }
                }
                Value::Number(t)
            }
            "join" => {
                let sep = if args.is_empty() { "".into() } else { str_arg(args, 0, name)? };
                let parts: Vec<String> = a.borrow().iter().map(|v| v.to_display()).collect();
                Value::str(&parts.join(&sep))
            }
            "shuffle" => {
                let mut v = a.borrow().clone();
                for i in (1..v.len()).rev() {
                    let j = (_it.next_random() * (i + 1) as f64) as usize;
                    v.swap(i, j.min(i));
                }
                Value::array(v)
            }
            _ => return Err(GroError::new(ErrorKind::Method, "")),
        })
    })();
    match r {
        Err(e) if e.message.is_empty() => None,
        other => Some(other),
    }
}

fn hash_method(h: &Rc<std::cell::RefCell<GroHash>>, name: &str, args: &[Value]) -> Option<GroResult<Value>> {
    let key_of = |v: &Value| -> GroResult<HashKey> {
        v.hash_key().ok_or_else(|| GroError::new(ErrorKind::Type, format!("a {} cannot be a hash key", v.type_name())))
    };
    let r = (|| -> GroResult<Value> {
        Ok(match name {
            "keys" => Value::array(h.borrow().keys.iter().map(|k| k.to_value()).collect()),
            "values" => {
                let h = h.borrow();
                Value::array(h.keys.iter().map(|k| h.map[k].clone()).collect())
            }
            "contains?" => {
                arity(name, args, 1)?;
                Value::Bool(h.borrow().get(&key_of(&args[0])?).is_some())
            }
            "get" => {
                if args.is_empty() || args.len() > 2 {
                    return type_err("get() expects 1 or 2 arguments (key, default)");
                }
                let d = args.get(1).cloned().unwrap_or(Value::Nil);
                h.borrow().get(&key_of(&args[0])?).cloned().unwrap_or(d)
            }
            "remove" => {
                arity(name, args, 1)?;
                h.borrow_mut().remove(&key_of(&args[0])?).unwrap_or(Value::Nil)
            }
            "clear" => {
                h.borrow_mut().clear();
                Value::Hash(h.clone())
            }
            "length" => Value::Number(h.borrow().len() as f64),
            "empty?" => Value::Bool(h.borrow().is_empty()),
            _ => return Err(GroError::new(ErrorKind::Method, "")),
        })
    })();
    match r {
        Err(e) if e.message.is_empty() => None,
        other => Some(other),
    }
}

fn v3_len(v: V3) -> f64 {
    (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
}

fn vec3_method(v: V3, name: &str, args: &[Value]) -> Option<GroResult<Value>> {
    let r = (|| -> GroResult<Value> {
        Ok(match name {
            "length" => Value::Number(v3_len(v)),
            "normalized" => {
                let l = v3_len(v);
                if l == 0.0 {
                    Value::vec3(0.0, 0.0, 0.0)
                } else {
                    Value::vec3(v.x / l, v.y / l, v.z / l)
                }
            }
            "abs" => Value::vec3(v.x.abs(), v.y.abs(), v.z.abs()),
            "dot" => {
                let o = vec3_arg(args, 0, name)?;
                Value::Number(v.x * o.x + v.y * o.y + v.z * o.z)
            }
            "cross" => {
                let o = vec3_arg(args, 0, name)?;
                Value::vec3(v.y * o.z - v.z * o.y, v.z * o.x - v.x * o.z, v.x * o.y - v.y * o.x)
            }
            "distance_to" => {
                let o = vec3_arg(args, 0, name)?;
                Value::Number(v3_len(V3::new(v.x - o.x, v.y - o.y, v.z - o.z)))
            }
            "lerp" => {
                let o = vec3_arg(args, 0, name)?;
                let t = num_arg(args, 1, name)?;
                Value::vec3(v.x + (o.x - v.x) * t, v.y + (o.y - v.y) * t, v.z + (o.z - v.z) * t)
            }
            "with_x" => Value::vec3(num_arg(args, 0, name)?, v.y, v.z),
            "with_y" => Value::vec3(v.x, num_arg(args, 0, name)?, v.z),
            "with_z" => Value::vec3(v.x, v.y, num_arg(args, 0, name)?),
            _ => return Err(GroError::new(ErrorKind::Method, "")),
        })
    })();
    match r {
        Err(e) if e.message.is_empty() => None,
        other => Some(other),
    }
}

fn vec2_method(v: V2, name: &str, args: &[Value]) -> Option<GroResult<Value>> {
    let v2arg = |i: usize| -> GroResult<V2> {
        match args.get(i) {
            Some(Value::Vec2(o)) => Ok(*o),
            _ => type_err(format!("{}() needs a Vector2", name)),
        }
    };
    let r = (|| -> GroResult<Value> {
        let l = (v.x * v.x + v.y * v.y).sqrt();
        Ok(match name {
            "length" => Value::Number(l),
            "normalized" => {
                if l == 0.0 {
                    Value::Vec2(V2::default())
                } else {
                    Value::Vec2(V2 { x: v.x / l, y: v.y / l })
                }
            }
            "dot" => {
                let o = v2arg(0)?;
                Value::Number(v.x * o.x + v.y * o.y)
            }
            "distance_to" => {
                let o = v2arg(0)?;
                Value::Number(((v.x - o.x).powi(2) + (v.y - o.y).powi(2)).sqrt())
            }
            "lerp" => {
                let o = v2arg(0)?;
                let t = num_arg(args, 1, name)?;
                Value::Vec2(V2 { x: v.x + (o.x - v.x) * t, y: v.y + (o.y - v.y) * t })
            }
            "with_x" => Value::Vec2(V2 { x: num_arg(args, 0, name)?, y: v.y }),
            "with_y" => Value::Vec2(V2 { x: v.x, y: num_arg(args, 0, name)? }),
            _ => return Err(GroError::new(ErrorKind::Method, "")),
        })
    })();
    match r {
        Err(e) if e.message.is_empty() => None,
        other => Some(other),
    }
}
