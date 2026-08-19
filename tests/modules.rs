// tests/modules.rs
//
// BACKEND TESTS — F4: Module system (`use "file"`).

use aha_lang::compiler::Compiler;
use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser;
use aha_lang::codegen::CodeGenerator;
use inkwell::context::Context;
use std::fs;
use std::path::PathBuf;

/// Helper: create a temp directory with .aha files, compile the main file,
/// JIT-execute, and return the i64 result.
fn run_with_files(main_content: &str, files: &[(&str, &str)]) -> i64 {
    let tmp = std::env::temp_dir().join(format!("aha_test_{}", std::process::id()));
    fs::create_dir_all(&tmp).expect("Failed to create temp dir");

    // Write main file
    fs::write(tmp.join("main.aha"), main_content).expect("Failed to write main.aha");

    // Write imported files
    for (name, content) in files {
        fs::write(tmp.join(format!("{}.aha", name)), content)
            .expect(&format!("Failed to write {}.aha", name));
    }

    let main_path = tmp.join("main.aha").to_string_lossy().to_string();
    let search_dir = Compiler::parent_dir(&main_path);
    let compiler = Compiler::new(vec![search_dir]);

    let program = compiler.compile(&main_path)
        .unwrap_or_else(|errors| panic!("Compilation failed: {:?}", errors));

    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    codegen.compile(&program).expect("Codegen failed");
    let result = codegen.run_jit().expect("JIT execution failed");

    // Cleanup
    let _ = fs::remove_dir_all(&tmp);

    result
}

// =====================================================================
// Basic import: single function
// =====================================================================

#[test]
fn import_single_function() {
    let math = r#"
pub fn add(a, b) {
    a + b
}
"#;
    let main = r#"
use "math"
add(3, 4)
"#;
    assert_eq!(run_with_files(main, &[("math", math)]), 7);
}

#[test]
fn import_multiple_functions() {
    let utils = r#"
pub fn double(x) {
    x * 2
}
pub fn triple(x) {
    x * 3
}
"#;
    let main = r#"
use "utils"
double(5) + triple(5)
"#;
    assert_eq!(run_with_files(main, &[("utils", utils)]), 25);
}

// =====================================================================
// Import: functions calling each other
// =====================================================================

#[test]
fn import_mutual_recursion() {
    let math = r#"
pub fn is_even(n) {
    if n == 0 {
        1
    } else {
        is_odd(n - 1)
    }
}
pub fn is_odd(n) {
    if n == 0 {
        0
    } else {
        is_even(n - 1)
    }
}
"#;
    let main = r#"
use "math"
is_even(4)
"#;
    assert_eq!(run_with_files(main, &[("math", math)]), 1);
}

// =====================================================================
// Import: struct definitions
// =====================================================================

#[test]
fn import_struct() {
    let geo = r#"
pub struct Point {
    x: int
    y: int
}
pub fn make_point(px, py) {
    Point { x: px, y: py }
}
"#;
    let main = r#"
use "geo"
let p = make_point(3, 4)
p.x + p.y
"#;
    assert_eq!(run_with_files(main, &[("geo", geo)]), 7);
}

// =====================================================================
// Import: string functions
// =====================================================================

#[test]
fn import_string_function() {
    let greet = r#"
pub fn greet(name) {
    "hello " + name
}
"#;
    let main = r#"
use "greet"
let s = greet("world")
len(s)
"#;
    assert_eq!(run_with_files(main, &[("greet", greet)]), 11);
}

// =====================================================================
// Multiple imports
// =====================================================================

#[test]
fn import_multiple_files() {
    let math = r#"
pub fn add(a, b) {
    a + b
}
"#;
    let strings = r#"
pub fn shout(s) {
    s + "!!!"
}
"#;
    let main = r#"
use "math"
use "strings"
let x = add(10, 20)
let s = shout("wow")
x + len(s)
"#;
    assert_eq!(run_with_files(main, &[("math", math), ("strings", strings)]), 36);
}

// =====================================================================
// Import: chain (A uses B)
// =====================================================================

#[test]
fn import_chain() {
    let base = r#"
pub fn inc(x) {
    x + 1
}
"#;
    let mid = r#"
use "base"
pub fn double_inc(x) {
    inc(inc(x))
}
"#;
    let main = r#"
use "mid"
double_inc(5)
"#;
    assert_eq!(run_with_files(main, &[("base", base), ("mid", mid)]), 7);
}

// =====================================================================
// Edge cases
// =====================================================================

#[test]
fn import_with_semicolon() {
    let math = r#"
pub fn add(a, b) {
    a + b
}
"#;
    let main = r#"
use "math";
add(1, 2)
"#;
    assert_eq!(run_with_files(main, &[("math", math)]), 3);
}

#[test]
fn import_no_functions_still_works() {
    // Importing an empty file should be a no-op
    let main = r#"
use "empty"
42
"#;
    assert_eq!(run_with_files(main, &[("empty", "")]), 42);
}

// =====================================================================
// Parser: use keyword tokenization
// =====================================================================

#[test]
fn parser_use_statement() {
    use aha_lang::ast::Statement;
    let source = r#"use "math""#;
    let lexer = Lexer::new(source.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    assert!(parser.errors.is_empty(), "Parser errors: {:?}", parser.errors);
    assert_eq!(program.statements.len(), 1);
    match &program.statements[0] {
        Statement::Import(import) => {
            assert_eq!(import.path, "math");
        }
        other => panic!("Expected Import statement, got {:?}", other),
    }
}
