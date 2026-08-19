// tests/ownership.rs
//
// BACKEND TESTS — F5 Phase 1: Compiler-inserted free (scope-based).
// Verifies that heap-allocated locals (Map, List, String) are automatically
// freed when leaving scope, preventing memory leaks.

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

// =====================================================================
// Map auto-free at function return
// =====================================================================

#[test]
fn map_auto_free_in_function() {
    // Create a map, use it, return — auto-free should not crash.
    let src = r#"
fn use_map() {
    let m = map_new()
    map_set(m, 1, 100)
    map_set(m, 2, 200)
    map_get(m, 1)
}
use_map()
"#;
    assert_eq!(run(src), 100);
}

#[test]
fn map_auto_free_with_explicit_free() {
    // Manual free + auto-free should not double-free (mark_freed prevents it).
    let src = r#"
fn use_map() {
    let m = map_new()
    map_set(m, 1, 42)
    let v = map_get(m, 1)
    map_free(m)
    v
}
use_map()
"#;
    assert_eq!(run(src), 42);
}

#[test]
fn map_auto_free_params_not_freed() {
    // Function parameters should NOT be auto-freed (owned by caller).
    let src = r#"
fn get_val(m, k) {
    map_get(m, k)
}
let m = map_new()
map_set(m, 5, 99)
let result = get_val(m, 5)
map_free(m)
result
"#;
    assert_eq!(run(src), 99);
}

// =====================================================================
// List auto-free at function return
// =====================================================================

#[test]
fn list_auto_free_in_function() {
    let src = r#"
fn use_list() {
    let xs = list_new()
    list_push(xs, 10)
    list_push(xs, 20)
    list_get(xs, 1)
}
use_list()
"#;
    assert_eq!(run(src), 20);
}

#[test]
fn list_auto_free_with_explicit_free() {
    let src = r#"
fn use_list() {
    let xs = list_new()
    list_push(xs, 7)
    let v = list_get(xs, 0)
    list_free(xs)
    v
}
use_list()
"#;
    assert_eq!(run(src), 7);
}

// =====================================================================
// Multiple heap locals in same scope
// =====================================================================

#[test]
fn multiple_heap_locals_auto_free() {
    // Two maps in the same scope — both should be auto-freed.
    let src = r#"
fn two_maps() {
    let a = map_new()
    let b = map_new()
    map_set(a, 1, 10)
    map_set(b, 1, 20)
    map_get(a, 1) + map_get(b, 1)
}
two_maps()
"#;
    assert_eq!(run(src), 30);
}

#[test]
fn map_and_list_auto_free() {
    // Mix of Map and List in same scope.
    let src = r#"
fn mixed() {
    let m = map_new()
    let xs = list_new()
    map_set(m, 1, 50)
    list_push(xs, 100)
    map_get(m, 1) + list_get(xs, 0)
}
mixed()
"#;
    assert_eq!(run(src), 150);
}

// =====================================================================
// String auto-free (when string_free builtin exists)
// Currently String is NOT auto-freed (ponytail: add when string_free exists).
// These tests verify String still works correctly with the new VarInfo fields.
// =====================================================================

#[test]
fn string_still_works_with_freed_field() {
    // Sanity: String concat + len still works with the new VarInfo.freed field.
    let src = r#"
fn greet() {
    let s = "hello"
    let t = " world"
    let full = s + t
    len(full)
}
greet()
"#;
    assert_eq!(run(src), 11);
}

// =====================================================================
// Recursive functions with heap locals
// =====================================================================

#[test]
fn recursive_with_map_auto_free() {
    let src = r#"
fn sum_map(m, n) {
    if n == 0 {
        0
    } else {
        map_set(m, n, n * 10)
        sum_map(m, n - 1) + map_get(m, n)
    }
}
let m = map_new()
sum_map(m, 3)
"#;
    // sum_map(3): set+get 3→30, recurse 2: set+get 2→20, recurse 1: set+get 1→10, recurse 0: 0
    // Each recursive call has its own scope with the same map param (not freed).
    assert_eq!(run(src), 60);
}
