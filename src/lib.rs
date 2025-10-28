// src/lib.rs

// Publikasikan semua modul utama kompiler kita agar bisa diakses dari luar (oleh main.rs)
pub mod ast;
pub mod lexer;
pub mod parser;
pub mod codegen;

// Opsional: Buat pintasan agar import di main.rs lebih bersih
// Dengan ini, kita bisa menulis `use aha_lang::Lexer;` instead of `use aha_lang::lexer::Lexer;`
pub use lexer::Lexer;
pub use parser::Parser;
pub use codegen::CodeGenerator;
