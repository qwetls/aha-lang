// tests/ownership.rs
//
// BACKEND TESTS — F5: Compiler-inserted auto-free.
// Phase 1: scope-based free at end of block.
// Phase 2: last-use analysis — free at last reference, not scope end.

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

// =====================================================================
// F5 Phase 2: Last-use analysis tests
// =====================================================================

#[test]
fn map_freed_at_last_use_not_scope_end() {
    // m is last used on statement 2 (map_get), statement 3 (map_free) uses
    // explicit free already — test that auto-free doesn't double-free.
    // Actually: with last-use, m is freed after map_get, then map_free is a no-op.
    let src = r#"
fn test() {
    let m = map_new()
    map_set(m, 1, 42)
    let v = map_get(m, 1)
    map_free(m)
    v
}
test()
"#;
    assert_eq!(run(src), 42);
}

#[test]
fn map_used_early_freed_early() {
    // m is used in statements 1 and 2, not in 3 or 4.
    // With last-use analysis, m should be freed after statement 2,
    // NOT at scope end (after statement 4).
    let src = r#"
fn test() {
    let m = map_new()
    map_set(m, 1, 10)
    let v = map_get(m, 1)
    let a = 1 + 2
    let b = a + 3
    v
}
test()
"#;
    assert_eq!(run(src), 10);
}

#[test]
fn two_maps_different_last_use_points() {
    // a is last used at statement 2, b is last used at statement 3.
    // Each should be freed at its own last-use point.
    let src = r#"
fn test() {
    let a = map_new()
    let b = map_new()
    map_set(a, 1, 10)
    map_set(b, 1, 20)
    let va = map_get(a, 1)
    let vb = map_get(b, 1)
    va + vb
}
test()
"#;
    assert_eq!(run(src), 30);
}

#[test]
fn list_freed_at_last_use() {
    // xs last used at map_get(xs, 1), then more statements follow.
    let src = r#"
fn test() {
    let xs = list_new()
    list_push(xs, 10)
    list_push(xs, 20)
    let v = list_get(xs, 1)
    let extra = 5 + 5
    v
}
test()
"#;
    assert_eq!(run(src), 20);
}

#[test]
fn last_use_with_explicit_free_no_double_free() {
    // Explicit free + last-use: explicit free marks as freed, so
    // last-use auto-free is skipped (no double-free).
    let src = r#"
fn test() {
    let m = map_new()
    map_set(m, 1, 99)
    map_free(m)
    let a = 1 + 1
    a
}
test()
"#;
    assert_eq!(run(src), 2);
}

#[test]
fn heap_var_in_return_statement() {
    // Variable used in return — last-use is the return statement itself.
    let src = r#"
fn test() {
    let m = map_new()
    map_set(m, 1, 77)
    map_get(m, 1)
}
test()
"#;
    assert_eq!(run(src), 77);
}

#[test]
fn heap_var_used_in_if_condition() {
    // Variable used in if condition — last-use index is the if statement.
    let src = r#"
fn test() {
    let m = map_new()
    map_set(m, 1, 1)
    if map_get(m, 1) == 1 {
        42
    } else {
        0
    }
}
test()
"#;
    assert_eq!(run(src), 42);
}

// =====================================================================
// F5 Phase 3: Escape analysis tests
// =====================================================================

#[test]
fn returned_map_not_freed() {
    // Critical: returning a map from a function must NOT free it.
    // Caller receives a valid pointer and can use it.
    let src = r#"
fn create_map() {
    let m = map_new()
    map_set(m, 1, 42)
    m
}
let m = create_map()
map_get(m, 1)
"#;
    assert_eq!(run(src), 42);
}

#[test]
fn returned_list_not_freed() {
    let src = r#"
fn create_list() {
    let xs = list_new()
    list_push(xs, 99)
    xs
}
let xs = create_list()
list_get(xs, 0)
"#;
    assert_eq!(run(src), 99);
}

#[test]
fn return_one_free_others() {
    // Return one map, but another map in the same scope should still be freed.
    let src = r#"
fn test() {
    let a = map_new()
    let b = map_new()
    map_set(a, 1, 10)
    map_set(b, 1, 20)
    a
}
let m = test()
map_get(m, 1)
"#;
    assert_eq!(run(src), 10);
}

#[test]
fn returned_map_escaped_not_in_last_use() {
    // m is used in set, get, and return — last-use should be the return,
    // not the get. So m is not freed before return.
    let src = r#"
fn test() {
    let m = map_new()
    map_set(m, 1, 55)
    let v = map_get(m, 1)
    m
}
let m = test()
map_get(m, 1)
"#;
    assert_eq!(run(src), 55);
}

#[test]
fn no_escape_still_freed() {
    // If a map is NOT returned, it should still be freed (no regression).
    let src = r#"
fn test() {
    let m = map_new()
    map_set(m, 1, 77)
    let v = map_get(m, 1)
    v
}
test()
"#;
    assert_eq!(run(src), 77);
}

#[test]
fn explicit_free_then_return_other() {
    // Free one map explicitly, return another.
    let src = r#"
fn test() {
    let a = map_new()
    let b = map_new()
    map_set(a, 1, 11)
    map_set(b, 1, 22)
    map_free(a)
    b
}
let m = test()
map_get(m, 1)
"#;
    assert_eq!(run(src), 22);
}
