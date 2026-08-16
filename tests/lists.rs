// tests/lists.rs
//
// BACKEND TESTS — F3e: List<T> generic data structure.
// `List<T>` is a heap-allocated dynamic array built on malloc/realloc/free.
// Lists are tracked in the type system as AhaType::List(inner); at the
// LLVM level a list is an i64 handle to a heap header struct
// {data: i8*, len: i64, cap: i64, elem_size: i64}.
//
// Builtins:
//   list_new()            -> List<Int>
//   list_new_string()     -> List<String>
//   list_push(list, val)  -> list
//   list_get(list, idx)   -> element
//   list_len(list)        -> Int
//   list_free(list)       -> Int (0)
//   list[i]               -> element (index read)
//   list[i] = v           -> element write

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

/// Helper: compile source and return the emitted LLVM IR as text.
fn emit_ir(source: &str) -> String {
    let lexer = Lexer::new(source.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    if !parser.errors.is_empty() {
        panic!("Parser errors: {:?}", parser.errors);
    }

    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    codegen.compile(&program).expect("Codegen failed");
    codegen.get_llvm_ir()
}

// =====================================================================
// List<Int> basics
// =====================================================================

#[test]
fn test_list_new_is_empty() {
    let result = run("let xs = list_new(); list_len(xs)");
    assert_eq!(result, 0);
}

#[test]
fn test_list_push_and_get_int() {
    let result = run("let xs = list_new(); list_push(xs, 42); list_get(xs, 0)");
    assert_eq!(result, 42);
}

#[test]
fn test_list_push_multiple() {
    let result = run(
        "let xs = list_new();
         list_push(xs, 10);
         list_push(xs, 20);
         list_push(xs, 30);
         list_get(xs, 0) + list_get(xs, 1) + list_get(xs, 2)"
    );
    assert_eq!(result, 60);
}

#[test]
fn test_list_len_after_pushes() {
    let result = run(
        "let xs = list_new();
         list_push(xs, 1);
         list_push(xs, 2);
         list_push(xs, 3);
         list_push(xs, 4);
         list_push(xs, 5);
         list_len(xs)"
    );
    assert_eq!(result, 5);
}

#[test]
fn test_list_grows_beyond_initial_capacity() {
    // Initial cap is 0 → first push allocates 4; pushing 8 forces realloc.
    let result = run(
        "let xs = list_new();
         list_push(xs, 0); list_push(xs, 1); list_push(xs, 2); list_push(xs, 3);
         list_push(xs, 4); list_push(xs, 5); list_push(xs, 6); list_push(xs, 7);
         list_get(xs, 7)"
    );
    assert_eq!(result, 7);
}

#[test]
fn test_list_index_read() {
    let result = run(
        "let xs = list_new();
         list_push(xs, 5);
         list_push(xs, 6);
         xs[1]"
    );
    assert_eq!(result, 6);
}

#[test]
fn test_list_index_write() {
    let result = run(
        "let xs = list_new();
         list_push(xs, 5);
         xs[0] = 99;
         xs[0]"
    );
    assert_eq!(result, 99);
}

#[test]
fn test_list_index_write_then_read_via_get() {
    let result = run(
        "let xs = list_new();
         list_push(xs, 1);
         list_push(xs, 2);
         xs[1] = 42;
         list_get(xs, 1)"
    );
    assert_eq!(result, 42);
}

#[test]
fn test_list_out_of_bounds_returns_zero() {
    let result = run("let xs = list_new(); list_get(xs, 0)");
    assert_eq!(result, 0);
}

#[test]
fn test_list_in_loop() {
    // Build 0..5 then sum via index reads.
    let result = run(
        "let xs = list_new();
         let i = 0;
         while i < 5 {
             list_push(xs, i);
             i = i + 1;
         }
         let sum = 0;
         let j = 0;
         while j < 5 {
             sum = sum + xs[j];
             j = j + 1;
         }
         sum"
    );
    assert_eq!(result, 10);
}

// =====================================================================
// List<String>
// =====================================================================

#[test]
fn test_list_string_push_get() {
    let result = run(
        "let xs = list_new_string();
         list_push(xs, \"hello\");
         len(list_get(xs, 0))"
    );
    assert_eq!(result, 5);
}

#[test]
fn test_list_string_multiple() {
    let result = run(
        "let xs = list_new_string();
         list_push(xs, \"ab\");
         list_push(xs, \"cdef\");
         len(list_get(xs, 0)) + len(list_get(xs, 1))"
    );
    assert_eq!(result, 6);
}

#[test]
fn test_list_string_index_read() {
    let result = run(
        "let xs = list_new_string();
         list_push(xs, \"abc\");
         xs[0]"
    );
    // JIT returns i64 — string index read can't be compared directly.
    // Instead use len().
    let _ = result;
    let len_result = run(
        "let xs = list_new_string();
         list_push(xs, \"abc\");
         len(xs[0])"
    );
    assert_eq!(len_result, 3);
}

#[test]
fn test_list_string_index_write() {
    let result = run(
        "let xs = list_new_string();
         list_push(xs, \"old\");
         xs[0] = \"new\";
         len(xs[0])"
    );
    assert_eq!(result, 3);
}

#[test]
fn test_list_string_growth() {
    let result = run(
        "let xs = list_new_string();
         list_push(xs, \"a\");
         list_push(xs, \"bb\");
         list_push(xs, \"ccc\");
         list_push(xs, \"dddd\");
         list_push(xs, \"eeeee\");
         len(list_get(xs, 4))"
    );
    assert_eq!(result, 5);
}

#[test]
fn test_list_string_concat_after_get() {
    let result = run(
        "let xs = list_new_string();
         list_push(xs, \"foo\");
         list_push(xs, \"bar\");
         len(list_get(xs, 0) + list_get(xs, 1))"
    );
    assert_eq!(result, 6);
}

// =====================================================================
// List<T> type annotations & inference
// =====================================================================

#[test]
fn test_list_type_annotation() {
    let result = run(
        "let xs: List<int> = list_new();
         list_push(xs, 7);
         list_get(xs, 0)"
    );
    assert_eq!(result, 7);
}

#[test]
fn test_list_in_function_param() {
    let result = run(
        "fn sum_list(xs) { list_get(xs, 0) + list_get(xs, 1) }
         let xs = list_new();
         list_push(xs, 40);
         list_push(xs, 2);
         sum_list(xs)"
    );
    assert_eq!(result, 42);
}

#[test]
fn test_list_in_function_return() {
    let result = run(
        "fn make() { list_new() }
         let xs = make();
         list_push(xs, 21);
         list_push(xs, 21);
         list_get(xs, 0) + list_get(xs, 1)"
    );
    assert_eq!(result, 42);
}

#[test]
fn test_list_free() {
    let result = run(
        "let xs = list_new();
         list_push(xs, 1);
         let r = list_free(xs);
         r"
    );
    assert_eq!(result, 0);
}

// =====================================================================
// Generic function over lists: fn first<T>(xs: List<T>) -> T
// =====================================================================

#[test]
fn test_generic_fn_over_list() {
    let result = run(
        "fn first<T>(xs: List<T>) -> T { xs[0] }
         let xs = list_new();
         list_push(xs, 99);
         first(xs)"
    );
    assert_eq!(result, 99);
}

#[test]
fn test_generic_fn_over_list_string() {
    let result = run(
        "fn first<T>(xs: List<T>) -> T { xs[0] }
         let xs = list_new_string();
         list_push(xs, \"abcd\");
         len(first(xs))"
    );
    assert_eq!(result, 4);
}

// =====================================================================
// IR shape: list builtins are LLVM functions with heap ops
// =====================================================================

#[test]
fn test_list_ir_contains_heap_ops() {
    let ir = emit_ir(
        "let xs = list_new();
         list_push(xs, 1);
         list_get(xs, 0)"
    );
    assert!(
        ir.contains("list_new") && ir.contains("list_push") && ir.contains("list_get"),
        "expected list builtins in IR, got:\n{}",
        ir
    );
    assert!(
        ir.contains("@malloc") && ir.contains("@realloc"),
        "expected malloc/realloc declarations in IR, got:\n{}",
        ir
    );
}
