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

// --- Basic extern fn call ---

#[test]
fn extern_fn_abs_basic() {
    let result = run(r#"
        extern fn abs(x: int) -> int;
        abs(-42)
    "#);
    assert_eq!(result, 42);
}

#[test]
fn extern_fn_abs_positive() {
    let result = run(r#"
        extern fn abs(x: int) -> int;
        abs(10)
    "#);
    assert_eq!(result, 10);
}

#[test]
fn extern_fn_abs_zero() {
    let result = run(r#"
        extern fn abs(x: int) -> int;
        abs(0)
    "#);
    assert_eq!(result, 0);
}

// --- Multiple extern declarations ---

#[test]
fn extern_fn_multiple_declarations() {
    let result = run(r#"
        extern fn abs(x: int) -> int;
        extern fn abs(y: int) -> int;
        abs(-5) + abs(3)
    "#);
    assert_eq!(result, 8);
}

// --- Extern fn in user function body ---

#[test]
fn extern_fn_called_from_user_function() {
    let result = run(r#"
        extern fn abs(x: int) -> int;

        fn double_abs(n) {
            abs(n) * 2
        }

        double_abs(-7)
    "#);
    assert_eq!(result, 14);
}

// --- Extern fn with if/else ---

#[test]
fn extern_fn_in_control_flow() {
    let result = run(r#"
        extern fn abs(x: int) -> int;

        fn abs_if_neg(n) {
            if n < 0 {
                abs(n)
            } else {
                n
            }
        }

        abs_if_neg(-99) + abs_if_neg(50)
    "#);
    assert_eq!(result, 149);
}

// --- Extern fn return value used in arithmetic ---

#[test]
fn extern_fn_in_arithmetic() {
    let result = run(r#"
        extern fn abs(x: int) -> int;
        abs(-3) + abs(-4) + abs(5)
    "#);
    assert_eq!(result, 12);
}

// --- Extern fn in loop ---

#[test]
fn extern_fn_in_loop() {
    let result = run(r#"
        extern fn abs(x: int) -> int;
        let sum = 0;
        for i in 0..5 {
            sum = sum + abs(i - 2);
        }
        sum
    "#);
    assert_eq!(result, 6);
}

// --- Error cases ---

#[test]
fn extern_fn_missing_fn_keyword() {
    expect_compile_error(
        r#"extern abs(x: int) -> int;"#,
        "Expected 'fn' after 'extern'",
    );
}

#[test]
fn extern_fn_missing_semicolon() {
    expect_compile_error(
        r#"extern fn abs(x: int) -> int
        abs(5)"#,
        "Expected ';' after extern fn declaration",
    );
}
