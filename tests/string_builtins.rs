// tests/string_builtins.rs
//
// Tests for string builtins: int_to_string, string_to_int, string_sub, char_at

use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser;
use aha_lang::codegen::CodeGenerator;
use inkwell::context::Context;

fn run(source: &str) -> i64 {
    let lexer = Lexer::new(source.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    if !parser.errors.is_empty() {
        panic!("Parser errors: {:?}", parser.errors);
    }
    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    codegen.compile(&program).expect("Codegen failed");
    codegen.run_jit().expect("JIT execution failed")
}

// --- int_to_string ---

#[test]
fn int_to_string_basic() {
    let result = run(r#"
        let s = int_to_string(42)
        len(s)
    "#);
    assert_eq!(result, 2);
}

#[test]
fn int_to_string_zero() {
    let result = run(r#"
        let s = int_to_string(0)
        len(s)
    "#);
    assert_eq!(result, 1);
}

#[test]
fn int_to_string_negative() {
    let result = run(r#"
        let s = int_to_string(-7)
        len(s)
    "#);
    assert_eq!(result, 2);
}

// --- string_to_int ---

#[test]
fn string_to_int_basic() {
    let result = run(r#"
        let s = int_to_string(123)
        string_to_int(s)
    "#);
    assert_eq!(result, 123);
}

#[test]
fn string_to_int_roundtrip() {
    let result = run(r#"
        let s = int_to_string(999)
        let n = string_to_int(s)
        n
    "#);
    assert_eq!(result, 999);
}

// --- string_sub ---

#[test]
fn string_sub_basic() {
    let result = run(r#"
        let s = int_to_string(12345)
        let sub = string_sub(s, 1, 3)
        len(sub)
    "#);
    assert_eq!(result, 3);
}

#[test]
fn string_sub_from_start() {
    let result = run(r#"
        let s = int_to_string(12345)
        let sub = string_sub(s, 0, 2)
        len(sub)
    "#);
    assert_eq!(result, 2);
}

// --- char_at ---

#[test]
fn char_at_first() {
    let result = run(r#"
        let s = int_to_string(65)
        char_at(s, 0)
    "#);
    // 65 as string is "65", char_at(0) = '6' = 54
    assert_eq!(result, 54);
}

#[test]
fn char_at_second() {
    let result = run(r#"
        let s = int_to_string(65)
        char_at(s, 1)
    "#);
    // "65" -> char_at(1) = '5' = 53
    assert_eq!(result, 53);
}
