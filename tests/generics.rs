// tests/generics.rs
//
// BACKEND TESTS — F3: generic functions with monomorphization.
// `fn pick<T>(a: T, b: T) -> T` compiles to a separate LLVM function
// per concrete type at each call site (pick_Int, pick_String, ...).

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
// Identity: fn id<T>(x: T) -> T
// =====================================================================

#[test]
fn test_generic_identity_int() {
    let result = run("fn id<T>(x: T) -> T { x } id(42)");
    assert_eq!(result, 42);
}

#[test]
fn test_generic_identity_string() {
    let result = run("fn id<T>(x: T) -> T { x } len(id(\"hello\"))");
    assert_eq!(result, 5);
}

#[test]
fn test_generic_identity_bool() {
    let result = run("fn id<T>(x: T) -> T { x } if id(true) { 1 } else { 0 }");
    assert_eq!(result, 1);
}

#[test]
fn test_generic_identity_struct() {
    let result = run(
        "struct Point { x, y } fn id<T>(x: T) -> T { x } let p = id(Point { x: 3, y: 4 }); p.x + p.y"
    );
    assert_eq!(result, 7);
}

// =====================================================================
// pick (max-like): fn pick<T>(a: T, b: T) -> T
// =====================================================================

#[test]
fn test_generic_pick_int() {
    let result = run("fn pick<T>(a: T, b: T) -> T { if a > b { a } else { b } } pick(3, 7)");
    assert_eq!(result, 7);
}

#[test]
fn test_generic_pick_int_reverse() {
    let result = run("fn pick<T>(a: T, b: T) -> T { if a > b { a } else { b } } pick(10, 2)");
    assert_eq!(result, 10);
}

#[test]
fn test_generic_pick_multiple_calls_same_type() {
    // Same instantiation (pick_Int) used at two call sites — cache path.
    let result = run("fn pick<T>(a: T, b: T) -> T { if a > b { a } else { b } } pick(3, 7) + pick(10, 2)");
    assert_eq!(result, 17);
}

// =====================================================================
// Two type params: fn first<A, B>(a: A, b: B) -> A
// =====================================================================

#[test]
fn test_generic_two_type_params_int_then_string() {
    let result = run("fn first<A, B>(a: A, b: B) -> A { a } first(1, \"x\")");
    assert_eq!(result, 1);
}

#[test]
fn test_generic_two_type_params_string_then_int() {
    // Same function, instantiated with A=String, B=Int — separate mangled fn.
    let result = run("fn first<A, B>(a: A, b: B) -> A { a } len(first(\"hello\", 2))");
    assert_eq!(result, 5);
}

// =====================================================================
// Nested monomorphization: generic body calls another generic
// =====================================================================

#[test]
fn test_generic_nested_call() {
    let result = run(
        "fn id<T>(x: T) -> T { x } fn twice<U>(x: U) -> U { id(x) } len(twice(\"ab\"))"
    );
    assert_eq!(result, 2);
}

#[test]
fn test_generic_in_arithmetic() {
    let result = run("fn id<T>(x: T) -> T { x } id(5) + id(6)");
    assert_eq!(result, 11);
}

#[test]
fn test_generic_with_let_binding() {
    let result = run("fn id<T>(x: T) -> T { x } let y: int = id(9); y * 2");
    assert_eq!(result, 18);
}

// =====================================================================
// IR shape: each concrete instantiation gets its own LLVM function
// =====================================================================

#[test]
fn test_generic_monomorphizes_separate_functions() {
    let ir = emit_ir("fn id<T>(x: T) -> T { x } id(1); len(id(\"a\"))");
    assert!(
        ir.contains("id_Int") && ir.contains("id_String"),
        "expected separate id_Int and id_String functions, got:\n{}",
        ir
    );
}
