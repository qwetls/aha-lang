// src/lib.rs

// Public modules for the AHA! compiler
pub mod ast;
pub mod types;
pub mod lexer;
pub mod parser;
pub mod codegen;
pub mod compiler;

// Re-exports for convenient access
pub use lexer::Lexer;
pub use parser::Parser;
pub use codegen::CodeGenerator;
pub use compiler::Compiler;
pub use types::{AhaType, TypedValue};
