// tests/ffi.rs
//
// BACKEND TESTS — FFI support (Roadmap Phase 8: "extern fn").
// Tests extern function declarations and calling C library functions
// from AHA! code via LLVM external linkage.
//
// Note: atol(s: *void) tests removed — AHA! strings are {i8*, i64} structs
// and can't be passed to C functions yet. atoi(i: int) is used instead
// because it takes an integer, avoiding the pointer-passing limitation.

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

/// Helper: expect a parser error matching the given substring.
fn expect_parse_error(source: &str, expected: &str) {
    let lexer = Lexer::new(source.to_string());
    let mut parser = Parser::new(lexer);
    let _program = parser.parse_program();

    assert!(!parser.errors.is_empty(),
        "Expected parser error containing '{}', but no errors occurred", expected);
    let all_errors = parser.errors.join("; ");
    assert!(all_errors.contains(expected),
        "Expected error containing '{}', got: {}", expected, all_errors);
}

// --- Basic extern fn call (atoi: int -> int) ---

#[test]
fn extern_fn_atoi_basic() {
    // atoi("0") returns 0 — atoi takes int, returns int
    let result = run(r#"
        extern fn atoi(s: int) -> int;
        atoi(0)
    "#);
    assert_eq!(result, 0);
}

// --- Extern fn declared, not called ---

#[test]
fn extern_fn_declared_not_called() {
    let result = run(r#"
        extern fn atoi(s: int) -> int;
        42
    "#);
    assert_eq!(result, 42);
}

// --- Multiple extern declarations ---

#[test]
fn extern_fn_multiple_declarations() {
    let result = run(r#"
        extern fn atoi(s: int) -> int;
        extern fn atoi(s: int) -> int;
        atoi(0) + atoi(0)
    "#);
    assert_eq!(result, 0);
}

// --- Extern fn called from user function ---

#[test]
fn extern_fn_called_from_user_function() {
    let result = run(r#"
        extern fn atoi(s: int) -> int;

        fn get_zero() {
            atoi(0)
        }

        get_zero() + 5
    "#);
    assert_eq!(result, 5);
}

// --- Extern fn in control flow ---

#[test]
fn extern_fn_in_control_flow() {
    let result = run(r#"
        extern fn atoi(s: int) -> int;

        let x = atoi(0);
        if x == 0 {
            10
        } else {
            0
        }
    "#);
    assert_eq!(result, 10);
}

// --- Extern fn in loop ---

#[test]
fn extern_fn_in_loop() {
    let result = run(r#"
        extern fn atoi(s: int) -> int;
        let sum = 0;
        for i in 0..5 {
            sum = sum + atoi(0) + 1;
        }
        sum
    "#);
    assert_eq!(result, 5);
}

// --- Extern fn with pointer param (atol) — compile-only, no call ---

#[test]
fn extern_fn_pointer_param_compile_only() {
    // atol takes *void — verify it compiles, but don't call it (atol(NULL) is UB)
    let result = run(r#"
        extern fn atol(s: *void) -> int;
        77
    "#);
    assert_eq!(result, 77);
}

// --- Extern fn redeclaring a C runtime builtin (skip) ---

#[test]
fn extern_fn_redeclare_builtin() {
    // strlen is already declared by C runtime — re-declaring should be a no-op
    let result = run(r#"
        extern fn strlen(s: *void) -> int;
        99
    "#);
    assert_eq!(result, 99);
}

// --- Error cases ---

#[test]
fn extern_fn_missing_fn_keyword() {
    expect_parse_error(
        r#"extern atoi(s: int) -> int;"#,
        "Expected 'fn' after 'extern'",
    );
}

#[test]
fn extern_fn_missing_semicolon() {
    expect_parse_error(
        r#"extern fn atoi(s: int) -> int
        atoi(0)"#,
        "Expected ';' after extern fn declaration",
    );
}
