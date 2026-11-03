//! A small JSON reader/writer (objects keep their key order).

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsonError {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at line {}, column {}", self.message, self.line, self.col)
    }
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&Vec<Json>> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }
    pub fn obj() -> Json {
        Json::Obj(Vec::new())
    }
    /// Builder helper: append a key to an object.
    pub fn with(mut self, key: &str, v: impl Into<Json>) -> Json {
        if let Json::Obj(items) = &mut self {
            items.push((key.to_string(), v.into()));
        }
        self
    }

    pub fn to_pretty(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, 0, true);
        s.push('\n');
        s
    }

    pub fn to_compact(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, 0, false);
        s
    }

    fn is_simple(&self) -> bool {
        match self {
            Json::Arr(a) => a.len() <= 4 && a.iter().all(|v| matches!(v, Json::Num(_) | Json::Bool(_) | Json::Null)),
            Json::Obj(o) => o.is_empty(),
            _ => true,
        }
    }

    fn write(&self, out: &mut String, indent: usize, pretty: bool) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => {
                if !n.is_finite() {
                    out.push('0');
                } else if n.fract() == 0.0 && n.abs() < 1e15 {
                    let _ = write!(out, "{}", *n as i64);
                } else {
                    let _ = write!(out, "{}", n);
                }
            }
            Json::Str(s) => write_str(out, s),
            Json::Arr(a) => {
                if a.is_empty() {
                    out.push_str("[]");
                    return;
                }
                let inline = !pretty || self.is_simple();
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                        if inline && pretty {
                            out.push(' ');
                        }
                    }
                    if !inline {
                        out.push('\n');
                        out.push_str(&"  ".repeat(indent + 1));
                    }
                    v.write(out, indent + 1, pretty);
                }
                if !inline {
                    out.push('\n');
                    out.push_str(&"  ".repeat(indent));
                }
                out.push(']');
            }
            Json::Obj(o) => {
                if o.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (i, (k, v)) in o.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if pretty {
                        out.push('\n');
                        out.push_str(&"  ".repeat(indent + 1));
                    }
                    write_str(out, k);
                    out.push(':');
                    if pretty {
                        out.push(' ');
                    }
                    v.write(out, indent + 1, pretty);
                }
                if pretty {
                    out.push('\n');
                    out.push_str(&"  ".repeat(indent));
                }
                out.push('}');
            }
        }
    }
}

fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

impl From<f32> for Json {
    fn from(v: f32) -> Json {
        // Round-trip f32 values without long binary noise.
        let s = format!("{}", v);
        Json::Num(s.parse().unwrap_or(v as f64))
    }
}
impl From<f64> for Json {
    fn from(v: f64) -> Json {
        Json::Num(v)
    }
}
impl From<u64> for Json {
    fn from(v: u64) -> Json {
        Json::Num(v as f64)
    }
}
impl From<bool> for Json {
    fn from(v: bool) -> Json {
        Json::Bool(v)
    }
}
impl From<&str> for Json {
    fn from(v: &str) -> Json {
        Json::Str(v.to_string())
    }
}
impl From<String> for Json {
    fn from(v: String) -> Json {
        Json::Str(v)
    }
}
impl From<[f32; 3]> for Json {
    fn from(v: [f32; 3]) -> Json {
        Json::Arr(v.iter().map(|x| Json::from(*x)).collect())
    }
}
impl From<[f32; 4]> for Json {
    fn from(v: [f32; 4]) -> Json {
        Json::Arr(v.iter().map(|x| Json::from(*x)).collect())
    }
}
impl From<Vec<Json>> for Json {
    fn from(v: Vec<Json>) -> Json {
        Json::Arr(v)
    }
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> P<'a> {
    fn err(&self, msg: &str) -> JsonError {
        let before = &self.s[..self.i.min(self.s.len())];
        let line = before.iter().filter(|&&c| c == b'\n').count() + 1;
        let col = before.iter().rev().take_while(|&&c| c != b'\n').count() + 1;
        JsonError { message: msg.to_string(), line, col }
    }
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }
    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > 128 {
            return Err(self.err("JSON nested too deeply"));
        }
        self.ws();
        match self.s.get(self.i) {
            None => Err(self.err("unexpected end of input")),
            Some(b'{') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Json::Obj(items));
                }
                loop {
                    self.ws();
                    let k = match self.value(depth + 1)? {
                        Json::Str(s) => s,
                        _ => return Err(self.err("object keys must be strings")),
                    };
                    self.ws();
                    if self.s.get(self.i) != Some(&b':') {
                        return Err(self.err("expected ':'"));
                    }
                    self.i += 1;
                    let v = self.value(depth + 1)?;
                    items.push((k, v));
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Json::Obj(items));
                        }
                        _ => return Err(self.err("expected ',' or '}'")),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Json::Arr(items));
                        }
                        _ => return Err(self.err("expected ',' or ']'")),
                    }
                }
            }
            Some(b'"') => {
                self.i += 1;
                let mut out: Vec<u8> = Vec::new();
                loop {
                    let c = *self.s.get(self.i).ok_or_else(|| self.err("unterminated string"))?;
                    self.i += 1;
                    match c {
                        b'"' => break,
                        b'\\' => {
                            let e = *self.s.get(self.i).ok_or_else(|| self.err("bad escape"))?;
                            self.i += 1;
                            match e {
                                b'n' => out.push(b'\n'),
                                b't' => out.push(b'\t'),
                                b'r' => out.push(b'\r'),
                                b'b' => out.push(8),
                                b'f' => out.push(12),
                                b'/' => out.push(b'/'),
                                b'\\' => out.push(b'\\'),
                                b'"' => out.push(b'"'),
                                b'u' => {
                                    let hex = std::str::from_utf8(
                                        self.s.get(self.i..self.i + 4).ok_or_else(|| self.err("bad \\u escape"))?,
                                    )
                                    .map_err(|_| self.err("bad \\u escape"))?;
                                    let cp = u32::from_str_radix(hex, 16).map_err(|_| self.err("bad \\u escape"))?;
                                    self.i += 4;
                                    let ch = char::from_u32(cp).unwrap_or('\u{fffd}');
                                    let mut buf = [0u8; 4];
                                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                                }
                                _ => return Err(self.err("bad escape")),
                            }
                        }
                        c => out.push(c),
                    }
                }
                String::from_utf8(out).map(Json::Str).map_err(|_| self.err("invalid UTF-8"))
            }
            Some(b't') if self.s[self.i..].starts_with(b"true") => {
                self.i += 4;
                Ok(Json::Bool(true))
            }
            Some(b'f') if self.s[self.i..].starts_with(b"false") => {
                self.i += 5;
                Ok(Json::Bool(false))
            }
            Some(b'n') if self.s[self.i..].starts_with(b"null") => {
                self.i += 4;
                Ok(Json::Null)
            }
            Some(c) if *c == b'-' || c.is_ascii_digit() => {
                let start = self.i;
                self.i += 1;
                while self.i < self.s.len() && matches!(self.s[self.i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.s[start..self.i]).unwrap();
                t.parse::<f64>().map(Json::Num).map_err(|_| self.err("invalid number"))
            }
            Some(_) => Err(self.err("unexpected character")),
        }
    }
}

pub fn parse(src: &str) -> Result<Json, JsonError> {
    let mut p = P { s: src.as_bytes(), i: 0 };
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.err("trailing characters after JSON value"));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let src = r#"{"a": [1, 2.5, -3e2], "b": {"c": "x\"y\n", "d": null, "e": true}}"#;
        let v = parse(src).unwrap();
        let again = parse(&v.to_pretty()).unwrap();
        assert_eq!(v, again);
        assert_eq!(v.get("a").unwrap().as_array().unwrap()[2].as_f64(), Some(-300.0));
    }

    #[test]
    fn error_location() {
        let e = parse("{\n  \"a\": ,\n}").unwrap_err();
        assert_eq!(e.line, 2);
    }

    #[test]
    fn unicode() {
        let v = parse(r#""café ñ""#).unwrap();
        assert_eq!(v.as_str(), Some("café ñ"));
    }
}
