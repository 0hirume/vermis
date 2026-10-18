//! Lossless Luau parsing with indexed syntax and diagnostics.

/// Typed Luau output.
pub mod emitter;

/// Byte-oriented tokenization.
pub mod lexer;

/// Parsing and syntax construction.
pub mod parser;

/// Tokens and source ranges.
pub mod token;

/// Indexed syntax storage.
pub mod tree;

pub use self::parser::parse;
