// tests/ffi.rs
//
// BACKEND TESTS — FFI support (Roadmap Phase 8: "extern fn").
// Tests extern function declarations and calling C library functions
// from AHA! code via LLVM external linkage.

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

/// Helper: expect a compile error matching the given substring.
fn expect_compile_error(source: &str, expected: &str) {
    let lexer = Lexer::new(source.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    match codegen.compile(&program) {
        Ok(()) => panic!("Expected compile error containing '{}', but compilation succeeded", expected),
        Err(e) => {
            assert!(e.contains(expected),
                "Expected error containing '{}', got: {}", expected, e);
        }
    }
}

// --- Basic extern fn call (no params, returns int) ---

#[test]
fn extern_fn_getpid() {
    // getpid() returns the current process ID — always > 0
    let result = run(r#"
        extern fn getpid() -> int;
        let pid = getpid();
        if pid > 0 { 1 } else { 0 }
    "#);
    assert_eq!(result, 1);
}

// --- Extern fn with int param (using atoi-like pattern) ---

#[test]
fn extern_fn_atol() {
    // atol("42") returns 42 — atol takes *void, returns long (i64)
    // We can't pass AHA! strings to *void yet, so test the parse+codegen path
    // by just calling it with a dummy pointer (0 = NULL, which atol handles
    // by returning 0).
    let result = run(r#"
        extern fn atol(s: *void) -> int;
        atol(0)
    "#);
    assert_eq!(result, 0);
}

// --- Extern fn declared, not called (should not crash) ---

#[test]
fn extern_fn_declared_not_called() {
    let result = run(r#"
        extern fn getpid() -> int;
        42
    "#);
    assert_eq!(result, 42);
}

// --- Multiple extern declarations ---

#[test]
fn extern_fn_multiple_declarations() {
    let result = run(r#"
        extern fn getpid() -> int;
        extern fn atol(s: *void) -> int;
        let a = getpid();
        let b = atol(0);
        if a > 0 { 1 } else { 0 }
    "#);
    assert_eq!(result, 1);
}

// --- Extern fn called from user function ---

#[test]
fn extern_fn_called_from_user_function() {
    let result = run(r#"
        extern fn getpid() -> int;

        fn check_pid() {
            let p = getpid();
            if p > 0 { 1 } else { 0 }
        }

        check_pid()
    "#);
    assert_eq!(result, 1);
}

// --- Extern fn in control flow ---

#[test]
fn extern_fn_in_control_flow() {
    let result = run(r#"
        extern fn getpid() -> int;
        extern fn atol(s: *void) -> int;

        let x = getpid();
        if x > 0 {
            atol(0) + 10
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
        extern fn atol(s: *void) -> int;
        let sum = 0;
        for i in 0..5 {
            sum = sum + atol(0) + 1;
        }
        sum
    "#);
    assert_eq!(result, 5);
}

// --- Extern fn with redeclaration (skip if already declared) ---

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
    expect_compile_error(
        r#"extern getpid() -> int;"#,
        "Expected 'fn' after 'extern'",
    );
}

#[test]
fn extern_fn_missing_semicolon() {
    expect_compile_error(
        r#"extern fn getpid() -> int
        getpid()"#,
        "Expected ';' after extern fn declaration",
    );
}
