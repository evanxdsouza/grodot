//! # Groscript
//!
//! A small, dynamically typed, Ruby-inspired scripting language for the Grodot
//! engine. Groscript is a tree-walking interpreter with no external
//! dependencies. It knows nothing about rendering or physics: the engine
//! exposes its features through [`HostObject`] values and native modules.
//!
//! ```
//! let mut it = groscript::Interpreter::new();
//! let out = it.capture_output();
//! it.run_source("demo.gro", "x = 2\nprint(\"x squared is #{x ** 2}\")").unwrap();
//! assert_eq!(out.borrow().as_str(), "x squared is 4\n");
//! ```

pub mod ast;
pub mod builtins;
pub mod error;
pub mod interpreter;
pub mod lexer;
pub mod parser;
pub mod token;
pub mod value;

pub use error::{ErrorKind, GroError, GroResult, Span};
pub use interpreter::Interpreter;
pub use value::{
    fmt_num, values_equal, Class, Function, GroHash, HashKey, HostObject, Module, NativeFunction, Object, Rgba,
    Value, V2, V3,
};

/// Check a source file for lexical and syntax errors without running it.
pub fn check(src: &str) -> Vec<GroError> {
    match parser::parse(src) {
        Ok(_) => Vec::new(),
        Err(e) => e,
    }
}
