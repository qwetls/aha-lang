// tests/maps.rs
//
// BACKEND TESTS — F3e: Map<K,V> generic data structure.
// `Map<K,V>` is a hash table built on malloc/free with open addressing
// and linear probing.  Deterministic FNV-1a (String keys) / splitmix64
// (Int keys) hashing — no randomness, no GC pauses (PRD: aerospace
// determinism).
//
// Builtins:
//   map_new()                          -> Map<Int, Int>
//   map_new_string_key()               -> Map<String, Int>
//   map_new_string_val()               -> Map<Int, String>
//   map_strings_new()                  -> Map<String, String>
//   map_set(map, key, val)             -> map
//   map_get(map, key)                  -> value (0 if missing)
//   map_contains(map, key)             -> Int (0 or 1)
//   map_remove(map, key)               -> map
//   map_len(map)                       -> Int
//   map_free(map)                      -> Int (0)

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
    let src = r#"
        let m = map_new()
        map_len(m)
    "#;
    assert_eq!(run(src), 0);
}

#[test]
fn map_set_get_int_int() {
    let src = r#"
        let m = map_new()
        let m2 = map_set(m, 1, 10)
        map_get(m2, 1)
    "#;
    assert_eq!(run(src), 10);
}

#[test]
fn map_get_missing_returns_zero() {
    let src = r#"
        let m = map_new()
        let m2 = map_set(m, 5, 42)
        map_get(m2, 99)
    "#;
    assert_eq!(run(src), 0);
}

#[test]
fn map_overwrite_value() {
    let src = r#"
        let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_set(m2, 1, 99)
        map_get(m3, 1)
    "#;
    assert_eq!(run(src), 99);
}

#[test]
fn map_len_after_sets() {
    let src = r#"
        let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_set(m2, 2, 20)
        let m4 = map_set(m3, 3, 30)
        map_len(m4)
    "#;
    assert_eq!(run(src), 3);
}

#[test]
fn map_len_overwrite_does_not_grow() {
    let src = r#"
        let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_set(m2, 1, 99)
        let m4 = map_set(m3, 1, 200)
        map_len(m4)
    "#;
    assert_eq!(run(src), 1);
}

#[test]
fn map_contains_present() {
    let src = r#"
        let m = map_new()
        let m2 = map_set(m, 7, 70)
        map_contains(m2, 7)
    "#;
    assert_eq!(run(src), 1);
}

#[test]
fn map_contains_absent() {
    let src = r#"
        let m = map_new()
        let m2 = map_set(m, 7, 70)
        map_contains(m2, 8)
    "#;
    assert_eq!(run(src), 0);
}

#[test]
fn map_remove_existing() {
    let src = r#"
        let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_remove(m2, 1)
        map_get(m3, 1)
    "#;
    assert_eq!(run(src), 0);
}

#[test]
fn map_remove_len_decrements() {
    let src = r#"
        let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_set(m2, 2, 20)
        let m4 = map_remove(m3, 1)
        map_len(m4)
    "#;
    assert_eq!(run(src), 1);
}

#[test]
fn map_remove_nonexistent() {
    let src = r#"
        let m = map_new()
        let m2 = map_set(m, 1, 10)
        let m3 = map_remove(m2, 99)
        map_len(m3)
    "#;
    assert_eq!(run(src), 1);
}

#[test]
fn map_multiple_keys() {
    let src = r#"
        let m = map_new()
        let m2 = map_set(m, 10, 100)
        let m3 = map_set(m2, 20, 200)
        let m4 = map_set(m3, 30, 300)
        let a = map_get(m4, 10)
        let b = map_get(m4, 20)
        let c = map_get(m4, 30)
        a + b + c
    "#;
    assert_eq!(run(src), 600);
}

// =====================================================================
// Map<Int, String> basics
// =====================================================================

#[test]
fn map_int_string_set_get() {
    let src = r#"
        let m = map_new_string_val()
        let m2 = map_string_val_set(m, 1, "hello")
        let s = map_string_val_get(m2, 1)
        length(s)
    "#;
    assert_eq!(run(src), 5);
}

#[test]
fn map_int_string_get_missing_returns_empty() {
    let src = r#"
        let m = map_new_string_val()
        let m2 = map_string_val_set(m, 1, "hello")
        let s = map_string_val_get(m2, 99)
        length(s)
    "#;
    assert_eq!(run(src), 0);
}

// =====================================================================
// Map<String, Int> basics
// =====================================================================

#[test]
fn map_string_int_set_get() {
    let src = r#"
        let m = map_new_string_key()
        let m2 = map_string_key_set(m, "x", 42)
        map_string_key_get(m2, "x")
    "#;
    assert_eq!(run(src), 42);
}

#[test]
fn map_string_int_multiple_keys() {
    let src = r#"
        let m = map_new_string_key()
        let m2 = map_string_key_set(m, "a", 1)
        let m3 = map_string_key_set(m2, "b", 2)
        let m4 = map_string_key_set(m3, "c", 3)
        let a = map_string_key_get(m4, "a")
        let b = map_string_key_get(m4, "b")
        let c = map_string_key_get(m4, "c")
        a + b + c
    "#;
    assert_eq!(run(src), 6);
}

#[test]
fn map_string_int_contains() {
    let src = r#"
        let m = map_new_string_key()
        let m2 = map_string_key_set(m, "hello", 100)
        map_string_key_contains(m2, "hello")
    "#;
    assert_eq!(run(src), 1);
}

// =====================================================================
// Map<String, String> basics
// =====================================================================

#[test]
fn map_strings_set_get() {
    let src = r#"
        let m = map_strings_new()
        let m2 = map_strings_set(m, "key", "value")
        let s = map_strings_get(m2, "key")
        length(s)
    "#;
    assert_eq!(run(src), 5);
}

#[test]
fn map_strings_len() {
    let src = r#"
        let m = map_strings_new()
        let m2 = map_strings_set(m, "a", "x")
        let m3 = map_strings_set(m2, "b", "y")
        map_strings_len(m3)
    "#;
    assert_eq!(run(src), 2);
}

// =====================================================================
// Edge cases
// =====================================================================

#[test]
fn map_free_returns_zero() {
    let src = r#"
        let m = map_new()
        map_free(m)
    "#;
    assert_eq!(run(src), 0);
}

#[test]
fn map_chained_set_get() {
    // Verify chaining returns the same handle for fluent style.
    let src = r#"
        let m = map_new()
        let m2 = map_set(map_set(map_set(m, 1, 10), 2, 20), 3, 30)
        let a = map_get(m2, 1)
        let b = map_get(m2, 2)
        let c = map_get(m2, 3)
        a + b + c
    "#;
    assert_eq!(run(src), 60);
}
