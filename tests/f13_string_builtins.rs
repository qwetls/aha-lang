// tests/string_builtins.rs
//
// Tests for F13 string builtins: str_split, str_to_int, str_contains, str_substring

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

// --- str_split ---

#[test]
fn str_split_basic() {
    let result = run(r#"
        let parts = str_split("a,b,c", ",")
        let count = str_split_count(parts)
        str_split_free(parts)
        count
    "#);
    assert_eq!(result, 3);
}

#[test]
fn str_split_get_first() {
    let result = run(r#"
        let parts = str_split("hello world", " ")
        let first = str_split_get(parts, 0)
        let l = len(first)
        str_split_free(parts)
        l
    "#);
    assert_eq!(result, 5);
}

#[test]
fn str_split_single() {
    let result = run(r#"
        let parts = str_split("no_delimiter", ",")
        str_split_count(parts)
    "#);
    assert_eq!(result, 1);
}

// --- str_to_int ---

#[test]
fn str_to_int_basic() {
    let result = run(r#"
        str_to_int("42")
    "#);
    assert_eq!(result, 42);
}

#[test]
fn str_to_int_negative() {
    let result = run(r#"
        str_to_int("-7")
    "#);
    assert_eq!(result, -7);
}

#[test]
fn str_to_int_invalid() {
    let result = run(r#"
        str_to_int("abc")
    "#);
    assert_eq!(result, 0);
}

// --- str_contains ---

#[test]
fn str_contains_yes() {
    let result = run(r#"
        str_contains("hello world", "world")
    "#);
    assert_eq!(result, 1);
}

#[test]
fn str_contains_no() {
    let result = run(r#"
        str_contains("hello world", "xyz")
    "#);
    assert_eq!(result, 0);
}

// --- str_substring ---

#[test]
fn str_substring_basic() {
    let result = run(r#"
        let sub = str_substring("hello", 1, 4)
        len(sub)
    "#);
    assert_eq!(result, 3);
}

#[test]
fn str_substring_full() {
    let result = run(r#"
        let sub = str_substring("test", 0, 4)
        len(sub)
    "#);
    assert_eq!(result, 4);
}

// --- Combined: routing pattern ---

#[test]
fn routing_pattern() {
    let result = run(r#"
        let path = "/api/users/42"
        let parts = str_split(path, "/")
        let count = str_split_count(parts)
        let id_str = str_split_get(parts, 3)
        let id = str_to_int(id_str)
        str_split_free(parts)
        id
    "#);
    assert_eq!(result, 42);
}
