// tests/integration_tests.rs
//
// End-to-end integration tests: source code → lexer → parser → codegen → JIT execution
// These tests verify the entire compiler pipeline produces correct results.

use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser;
use aha_lang::codegen::CodeGenerator;
use inkwell::context::Context;

/// Helper: compile and JIT-execute AHA! source code, return i64 result
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

/// Helper: compile and expect a codegen error (type errors, etc.)
fn expect_compile_error(source: &str) -> String {
    let lexer = Lexer::new(source.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    codegen.compile(&program).unwrap_err()
}

// =====================================================================
// Integer Arithmetic
// =====================================================================

#[test]
fn test_integer_literal() {
    assert_eq!(run("42"), 42);
}

#[test]
fn test_addition() {
    assert_eq!(run("10 + 20"), 30);
}

#[test]
fn test_subtraction() {
    assert_eq!(run("50 - 30"), 20);
}

#[test]
fn test_multiplication() {
    assert_eq!(run("6 * 7"), 42);
}

#[test]
fn test_division() {
    assert_eq!(run("100 / 4"), 25);
}

#[test]
fn test_complex_arithmetic() {
    assert_eq!(run("2 + 3 * 4"), 14); // 2 + (3*4) = 14
}

// =====================================================================
// Boolean & Comparison
// =====================================================================

#[test]
fn test_true_value() {
    assert_eq!(run("true"), 1);
}

#[test]
fn test_false_value() {
    assert_eq!(run("false"), 0);
}

#[test]
fn test_equality() {
    assert_eq!(run("42 == 42"), 1);
    assert_eq!(run("42 == 99"), 0);
}

#[test]
fn test_not_equal() {
    assert_eq!(run("1 != 2"), 1);
    assert_eq!(run("1 != 1"), 0);
}

#[test]
fn test_less_than() {
    assert_eq!(run("1 < 2"), 1);
    assert_eq!(run("2 < 1"), 0);
}

#[test]
fn test_greater_than() {
    assert_eq!(run("5 > 3"), 1);
    assert_eq!(run("3 > 5"), 0);
}

#[test]
fn test_less_equal() {
    assert_eq!(run("3 <= 3"), 1);
    assert_eq!(run("3 <= 2"), 0);
}

#[test]
fn test_greater_equal() {
    assert_eq!(run("5 >= 5"), 1);
    assert_eq!(run("4 >= 5"), 0);
}

// =====================================================================
// Prefix Expressions
// =====================================================================

#[test]
fn test_negation() {
    assert_eq!(run("-42"), -42);
}

#[test]
fn test_logical_not() {
    assert_eq!(run("!false"), 1);
    assert_eq!(run("!true"), 0);
}

// =====================================================================
// Let Variables
// =====================================================================

#[test]
fn test_let_and_use() {
    assert_eq!(run("let x = 10;\nx"), 10);
}

#[test]
fn test_let_arithmetic() {
    assert_eq!(run("let a = 5;\nlet b = 10;\na + b"), 15);
}

// =====================================================================
// If/Else
// =====================================================================

#[test]
fn test_if_true_branch() {
    assert_eq!(run("if 1 > 0 { 42 } else { 99 }"), 42);
}

#[test]
fn test_if_false_branch() {
    assert_eq!(run("if 0 > 1 { 42 } else { 99 }"), 99);
}

#[test]
fn test_if_with_variables() {
    assert_eq!(run("let x = 10;\nlet y = 20;\nif x > y { x } else { y }"), 20);
}

// =====================================================================
// Functions
// =====================================================================

#[test]
fn test_function_call_simple() {
    assert_eq!(run("fn double(x) { x * 2 }\ndouble(21)"), 42);
}

#[test]
fn test_function_with_return() {
    assert_eq!(run("fn f(x) { return x + 1; }\nf(41)"), 42);
}

#[test]
fn test_function_two_params() {
    assert_eq!(run("fn add(a, b) { a + b }\nadd(20, 22)"), 42);
}

// =====================================================================
// While Loops
// =====================================================================

#[test]
fn test_while_loop_basic() {
    // Sum 1..5 using while
    let src = r#"
        let sum = 0;
        let i = 1;
        while i <= 5 {
            sum = sum + i;
            i = i + 1;
        }
        sum
    "#;
    assert_eq!(run(src), 15); // 1+2+3+4+5
}

// =====================================================================
// For Loops
// =====================================================================

#[test]
fn test_for_loop_sum() {
    let src = r#"
        let sum = 0;
        for i in 0..5 {
            sum = sum + i;
        }
        sum
    "#;
    assert_eq!(run(src), 10); // 0+1+2+3+4
}

// =====================================================================
// Variable Scoping (H-04)
// =====================================================================

#[test]
fn test_block_scoping() {
    // Outer variable should not be affected by inner block
    let src = r#"
        let x = 10;
        if true {
            let x = 99;
        }
        x
    "#;
    assert_eq!(run(src), 10); // outer x remains 10
}

// =====================================================================
// Stdlib Builtins
// =====================================================================

#[test]
fn test_abs() {
    assert_eq!(run("abs(-42)"), 42);
    assert_eq!(run("abs(42)"), 42);
}

#[test]
fn test_min() {
    assert_eq!(run("min(10, 20)"), 10);
    assert_eq!(run("min(20, 10)"), 10);
}

#[test]
fn test_max() {
    assert_eq!(run("max(10, 20)"), 20);
    assert_eq!(run("max(20, 10)"), 20);
}

// =====================================================================
// Type Error Detection (M-05)
// =====================================================================

#[test]
fn test_type_error_int_plus_string() {
    let err = expect_compile_error("let x = 1 + \"hello\"");
    assert!(err.contains("Cannot apply"), "Expected type error, got: {}", err);
}

#[test]
fn test_type_error_string_minus() {
    let err = expect_compile_error("let x = \"a\" - \"b\"");
    assert!(err.contains("Cannot apply"), "Expected type error, got: {}", err);
}

#[test]
fn test_type_error_negate_string() {
    let err = expect_compile_error("-\"hello\"");
    assert!(err.contains("Cannot apply"), "Expected type error, got: {}", err);
}
