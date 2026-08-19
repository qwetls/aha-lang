// F2 — Type Inference & Annotations tests
// Covers explicit type annotations on `let` (`let x: int = 5`),
// type-checking of annotated values, and return type inference.

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
    codegen.compile(&program).expect("compile failed");
    codegen.run_jit().expect("JIT execution failed")
}

fn expect_error(source: &str) -> String {
    let lexer = Lexer::new(source.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    if !parser.errors.is_empty() {
        return format!("{:?}", parser.errors);
    }
    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    match codegen.compile(&program) {
        Ok(()) => panic!("Expected compile error, but compilation succeeded"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn test_int_annotation() {
    assert_eq!(run("let x: int = 5; x"), 5);
}

#[test]
fn test_int_annotation_arithmetic() {
    assert_eq!(run("let x: int = 5; let y: int = 3; x + y"), 8);
}

#[test]
fn test_string_annotation() {
    assert_eq!(run("let s: string = \"hi\"; len(s)"), 2);
}

#[test]
fn test_string_annotation_concat() {
    assert_eq!(run("let s: string = \"hello\" + \" world\"; len(s)"), 11);
}

#[test]
fn test_bool_annotation() {
    assert_eq!(run("let b: bool = true; if b { 1 } else { 0 }"), 1);
}

#[test]
fn test_struct_annotation() {
    assert_eq!(
        run("struct Point { x, y } let p: Point = Point { x: 3, y: 4 }; p.x + p.y"),
        7
    );
}

#[test]
fn test_struct_annotation_with_string_field() {
    assert_eq!(
        run("struct Person { name: string, age: int } let p: Person = Person { name: \"AHA\", age: 4 }; len(p.name) + p.age"),
        7
    );
}

#[test]
fn test_annotation_mismatch_int_string() {
    let err = expect_error("let x: int = \"hi\"; x");
    assert!(err.contains("Type mismatch"), "error was: {}", err);
}

#[test]
fn test_annotation_mismatch_string_int() {
    let err = expect_error("let s: string = 5; s");
    assert!(err.contains("Type mismatch"), "error was: {}", err);
}

#[test]
fn test_annotation_mismatch_struct() {
    let err = expect_error("struct Point { x, y } struct Other { a } let p: Point = Other { a: 1 }; p");
    assert!(err.contains("Type mismatch"), "error was: {}", err);
}

#[test]
fn test_inferred_type_still_works() {
    // Without annotation, inference still picks Int by default.
    assert_eq!(run("let x = 5; x + 1"), 6);
    // String inference from literal.
    assert_eq!(run("let s = \"abc\"; len(s)"), 3);
    // Struct inference from literal.
    assert_eq!(run("struct Point { x, y } let p = Point { x: 1, y: 2 }; p.x"), 1);
}

#[test]
fn test_return_type_inference_string() {
    // Function whose last expression is a string — return type String.
    assert_eq!(run("fn greet() { \"hello\" } let s = greet(); len(s)"), 5);
}

#[test]
fn test_return_type_inference_struct() {
    assert_eq!(
        run("struct Point { x, y } fn make() { Point { x: 40, y: 2 } } let p = make(); p.x + p.y"),
        42
    );
}

#[test]
fn test_return_type_inference_if_branches() {
    // If both branches produce strings, return type is String.
    assert_eq!(
        run("fn pick(a) { if a > 0 { \"pos\" } else { \"neg\" } } let s = pick(1); len(s)"),
        3
    );
}

#[test]
fn test_annotation_with_function_return() {
    // A function returning String assigned to a `string` annotated let.
    assert_eq!(
        run("fn hello() { \"world\" } let s: string = hello(); len(s)"),
        5
    );
}

#[test]
fn test_annotation_error_invalid_hint() {
    // Unknown type hint falls back to Int (lenient, like field hints).
    assert_eq!(run("let x: unknown_hint = 7; x"), 7);
}

#[test]
fn test_annotated_variable_in_struct_literal() {
    // Annotated variables can be used as struct field values.
    assert_eq!(
        run("struct Point { x, y } let a: int = 10; let b: int = 32; let p = Point { x: a, y: b }; p.x + p.y"),
        42
    );
}

#[test]
fn test_annotated_variable_in_function_call() {
    assert_eq!(
        run("fn add(a, b) { a + b } let x: int = 20; let y: int = 22; add(x, y)"),
        42
    );
}

#[test]
fn test_annotation_then_mutation() {
    assert_eq!(run("let x: int = 5; x = x + 1; x"), 6);
}

#[test]
fn test_annotation_string_then_mutation() {
    assert_eq!(run("let s: string = \"abc\"; s = s + \"d\"; len(s)"), 4);
}

// --- Function return type annotations (fn f() -> T) ---

#[test]
fn test_return_annotation_int() {
    assert_eq!(run("fn add(a: int, b: int) -> int { return a + b } add(3, 4)"), 7);
}

#[test]
fn test_return_annotation_string() {
    assert_eq!(run("fn greet() -> string { return \"hello\" } len(greet())"), 5);
}

#[test]
fn test_return_annotation_last_expr() {
    assert_eq!(run("fn double(n: int) -> int { n * 2 } double(21)"), 42);
}

#[test]
fn test_return_annotation_bool_as_int() {
    assert_eq!(run("fn is_pos(n: int) -> int { n > 0 } is_pos(5)"), 1);
}

#[test]
fn test_return_annotation_struct() {
    let src = "
struct Point { x: int, y: int }
fn make_point(x: int, y: int) -> Point { Point { x: x, y: y } }
fn main() { let p = make_point(3, 7); p.x + p.y }";
    assert_eq!(run(src), 10);
}

#[test]
fn test_return_annotation_mismatch_is_error() {
    let err = expect_error("fn bad() -> string { 42 } bad()");
    assert!(err.contains("does not match"), "Expected mismatch error, got: {}", err);
}

#[test]
fn test_return_annotation_unused_infers_from_body() {
    // No annotation — should still infer String from body
    assert_eq!(run("fn name() { \"aha\" } len(name())"), 3);
}
