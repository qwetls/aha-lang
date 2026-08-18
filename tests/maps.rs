// tests/maps.rs
//
// BACKEND TESTS — F3e: Map<K,V> generic data structure.

use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser;
use aha_lang::codegen::CodeGenerator;
use inkwell::context::Context;

/// Helper: compile and JIT-execute AHA! source, returning the i64 result.
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

// =====================================================================
// Map<Int, Int> basics
// =====================================================================

#[test]
fn map_new_empty_len() {
    assert_eq!(run(r#"let m = map_new()
        map_len(m)"#), 0);
}

#[test]
fn map_set_get_int_int() {
    assert_eq!(run(r#"let m = map_new()
        let m2 = map_set(m, 1, 10)
        map_get(m2, 1)"#), 10);
}

#[test]
fn map_get_missing_returns_zero() {
    assert_eq!(run(r#"let m = map_new()
        let m2 = map_set(m, 5, 42)
        map_get(m2, 99)"#), 0);
}

#[test]
fn map_overwrite_value() {
    assert_eq!(run(r#"let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_set(m2, 1, 99)
        map_get(m3, 1)"#), 99);
}

#[test]
fn map_len_after_sets() {
    assert_eq!(run(r#"let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_set(m2, 2, 20)
        let m4 = map_set(m3, 3, 30)
        map_len(m4)"#), 3);
}

#[test]
fn map_len_overwrite_does_not_grow() {
    assert_eq!(run(r#"let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_set(m2, 1, 99)
        let m4 = map_set(m3, 1, 200)
        map_len(m4)"#), 1);
}

#[test]
fn map_contains_present() {
    assert_eq!(run(r#"let m = map_new()
        let m2 = map_set(m, 7, 70)
        map_contains(m2, 7)"#), 1);
}

#[test]
fn map_contains_absent() {
    assert_eq!(run(r#"let m = map_new()
        let m2 = map_set(m, 7, 70)
        map_contains(m2, 8)"#), 0);
}

#[test]
fn map_remove_existing() {
    assert_eq!(run(r#"let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_remove(m2, 1)
        map_get(m3, 1)"#), 0);
}

#[test]
fn map_remove_len_decrements() {
    assert_eq!(run(r#"let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_set(m2, 2, 20)
        let m4 = map_remove(m3, 1)
        map_len(m4)"#), 1);
}

#[test]
fn map_remove_nonexistent() {
    assert_eq!(run(r#"let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_remove(m2, 99)
        map_len(m3)"#), 1);
}

#[test]
fn map_multiple_keys() {
    assert_eq!(run(r#"let m = map_new()
        let m2 = map_set(m, 10, 100)
        let m3 = map_set(m2, 20, 200)
        let m4 = map_set(m3, 30, 300)
        let a = map_get(m4, 10)
        let b = map_get(m4, 20)
        let c = map_get(m4, 30)
        a + b + c"#), 600);
}

// =====================================================================
// Map<Int, String> basics
// =====================================================================

#[test]
fn map_int_string_set_get() {
    assert_eq!(run(r#"let m = map_string_val_new()
        let m2 = map_string_val_set(m, 1, "hello")
        let s = map_string_val_get(m2, 1)
        len(s)"#), 5);
}

#[test]
fn map_int_string_get_missing_returns_empty() {
    assert_eq!(run(r#"let m = map_string_val_new()
        let m2 = map_string_val_set(m, 1, "hello")
        let s = map_string_val_get(m2, 99)
        len(s)"#), 0);
}

// =====================================================================
// Map<String, Int> basics
// =====================================================================

#[test]
fn map_string_int_set_get() {
    assert_eq!(run(r#"let m = map_string_key_new()
        let m2 = map_string_key_set(m, "x", 42)
        map_string_key_get(m2, "x")"#), 42);
}

#[test]
fn map_string_int_multiple_keys() {
    assert_eq!(run(r#"let m = map_string_key_new()
        let m2 = map_string_key_set(m, "a", 1)
        let m3 = map_string_key_set(m2, "b", 2)
        let m4 = map_string_key_set(m3, "c", 3)
        let a = map_string_key_get(m4, "a")
        let b = map_string_key_get(m4, "b")
        let c = map_string_key_get(m4, "c")
        a + b + c"#), 6);
}

#[test]
fn map_string_int_contains() {
    assert_eq!(run(r#"let m = map_string_key_new()
        let m2 = map_string_key_set(m, "hello", 100)
        map_string_key_contains(m2, "hello")"#), 1);
}

// =====================================================================
// Map<String, String> basics
// =====================================================================

#[test]
fn map_strings_set_get() {
    assert_eq!(run(r#"let m = map_strings_new()
        let m2 = map_strings_set(m, "key", "value")
        let s = map_strings_get(m2, "key")
        len(s)"#), 5);
}

#[test]
fn map_strings_len() {
    assert_eq!(run(r#"let m = map_strings_new()
        let m2 = map_strings_set(m, "a", "x")
        let m3 = map_strings_set(m2, "b", "y")
        map_strings_len(m3)"#), 2);
}

// =====================================================================
// Edge cases
// =====================================================================

#[test]
fn map_free_returns_zero() {
    assert_eq!(run(r#"let m = map_new()
        map_free(m)"#), 0);
}

#[test]
fn map_chained_set_get() {
    assert_eq!(run(r#"let m = map_new()
        let m2 = map_set(map_set(map_set(m, 1, 10), 2, 20), 3, 30)
        let a = map_get(m2, 1)
        let b = map_get(m2, 2)
        let c = map_get(m2, 3)
        a + b + c"#), 60);
}
