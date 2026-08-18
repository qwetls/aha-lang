// tests/maps.rs
//
// BACKEND TESTS — F3e: Map<K,V> generic data structure.

use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser;
use aha_lang::codegen::CodeGenerator;
use inkwell::context::Context;

/// Helper: compile and JIT-execute AHA! source, returning the i64 result.
fn run(label: &str, source: &str) -> i64 {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "[maps] RUN: {}", label);
    let lexer = Lexer::new(source.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    if !parser.errors.is_empty() {
        panic!("Parser errors: {:?}", parser.errors);
    }
    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    codegen.compile(&program).expect("Codegen failed");
    // Dump IR for debugging
    let ir = codegen.get_llvm_ir();
    let _ = writeln!(std::io::stderr(), "[maps] === IR for {} ===", label);
    let _ = writeln!(std::io::stderr(), "{}", ir);
    let _ = writeln!(std::io::stderr(), "[maps] === END IR ===");
    let _ = writeln!(std::io::stderr(), "[maps] compiled, JIT...");
    let result = codegen.run_jit().expect("JIT failed");
    let _ = writeln!(std::io::stderr(), "[maps] OK result={}", result);
    result
}

#[test]
fn map_new_empty_len() {
    assert_eq!(run("new_empty_len", r#"let m = map_new()
        map_len(m)"#), 0);
}

#[test]
fn map_set_get_int_int() {
    assert_eq!(run("set_get_ii", r#"let m = map_new()
        let m2 = map_set(m, 1, 10)
        map_get(m2, 1)"#), 10);
}

#[test]
fn map_get_missing_returns_zero() {
    assert_eq!(run("get_missing", r#"let m = map_new()
        let m2 = map_set(m, 5, 42)
        map_get(m2, 99)"#), 0);
}

#[test]
fn map_overwrite_value() {
    assert_eq!(run("overwrite", r#"let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_set(m2, 1, 99)
        map_get(m3, 1)"#), 99);
}

#[test]
fn map_len_after_sets() {
    assert_eq!(run("len_after_sets", r#"let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_set(m2, 2, 20)
        let m4 = map_set(m3, 3, 30)
        map_len(m4)"#), 3);
}

#[test]
fn map_len_overwrite_does_not_grow() {
    assert_eq!(run("len_ow_nogrow", r#"let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_set(m2, 1, 99)
        let m4 = map_set(m3, 1, 200)
        map_len(m4)"#), 1);
}

#[test]
fn map_contains_present() {
    assert_eq!(run("contains_yes", r#"let m = map_new()
        let m2 = map_set(m, 7, 70)
        map_contains(m2, 7)"#), 1);
}

#[test]
fn map_contains_absent() {
    assert_eq!(run("contains_no", r#"let m = map_new()
        let m2 = map_set(m, 7, 70)
        map_contains(m2, 8)"#), 0);
}

#[test]
fn map_remove_existing() {
    assert_eq!(run("remove_exists", r#"let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_remove(m2, 1)
        map_get(m3, 1)"#), 0);
}

#[test]
fn map_remove_len_decrements() {
    assert_eq!(run("remove_len", r#"let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_set(m2, 2, 20)
        let m4 = map_remove(m3, 1)
        map_len(m4)"#), 1);
}

#[test]
fn map_remove_nonexistent() {
    assert_eq!(run("remove_none", r#"let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_remove(m2, 99)
        map_len(m3)"#), 1);
}

#[test]
fn map_multiple_keys() {
    assert_eq!(run("multi_keys", r#"let m = map_new()
        let m2 = map_set(m, 10, 100)
        let m3 = map_set(m2, 20, 200)
        let m4 = map_set(m3, 30, 300)
        let a = map_get(m4, 10)
        let b = map_get(m4, 20)
        let c = map_get(m4, 30)
        a + b + c"#), 600);
}

#[test]
fn map_int_string_set_get() {
    assert_eq!(run("int_str_setget", r#"let m = map_new_string_val()
        let m2 = map_string_val_set(m, 1, "hello")
        let s = map_string_val_get(m2, 1)
        length(s)"#), 5);
}

#[test]
fn map_int_string_get_missing_returns_empty() {
    assert_eq!(run("int_str_missing", r#"let m = map_new_string_val()
        let m2 = map_string_val_set(m, 1, "hello")
        let s = map_string_val_get(m2, 99)
        length(s)"#), 0);
}

#[test]
fn map_string_int_set_get() {
    assert_eq!(run("str_int_setget", r#"let m = map_new_string_key()
        let m2 = map_string_key_set(m, "x", 42)
        map_string_key_get(m2, "x")"#), 42);
}

#[test]
fn map_string_int_multiple_keys() {
    assert_eq!(run("str_int_multi", r#"let m = map_new_string_key()
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
    assert_eq!(run("str_int_contains", r#"let m = map_new_string_key()
        let m2 = map_string_key_set(m, "hello", 100)
        map_string_key_contains(m2, "hello")"#), 1);
}

#[test]
fn map_strings_set_get() {
    assert_eq!(run("str_str_setget", r#"let m = map_strings_new()
        let m2 = map_strings_set(m, "key", "value")
        let s = map_strings_get(m2, "key")
        length(s)"#), 5);
}

#[test]
fn map_strings_len() {
    assert_eq!(run("str_str_len", r#"let m = map_strings_new()
        let m2 = map_strings_set(m, "a", "x")
        let m3 = map_strings_set(m2, "b", "y")
        map_strings_len(m3)"#), 2);
}

#[test]
fn map_free_returns_zero() {
    assert_eq!(run("free_zero", r#"let m = map_new()
        map_free(m)"#), 0);
}

#[test]
fn map_chained_set_get() {
    assert_eq!(run("chained", r#"let m = map_new()
        let m2 = map_set(map_set(map_set(m, 1, 10), 2, 20), 3, 30)
        let a = map_get(m2, 1)
        let b = map_get(m2, 2)
        let c = map_get(m2, 3)
        a + b + c"#), 60);
}
