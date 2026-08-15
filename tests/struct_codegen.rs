// tests/struct_codegen.rs
//
// BACKEND TESTS — struct codegen (Roadmap Phase 2: "Struct codegen &
// field access at runtime"). These exercise the newly-implemented
// struct-literal construction and field-access read path through the
// LLVM backend, both as emitted IR and as JIT-executed semantics.
//
// Structs are laid out as an LLVM struct where every field is i64,
// in declaration order. A struct literal builds the aggregate with
// `insertvalue`; field access reads it with `extractvalue`.

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

/// Helper: compile AHA! source and return the emitted LLVM IR as text.
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

/// Helper: compile source expecting a codegen error, returning it.
fn expect_codegen_error(source: &str) -> String {
    let lexer = Lexer::new(source.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    if !parser.errors.is_empty() {
        panic!("Parser errors (expected codegen error, not parse error): {:?}", parser.errors);
    }

    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    codegen.compile(&program).expect_err("expected a codegen error")
}

// =====================================================================
// Construction + single-field read
// =====================================================================

#[test]
fn test_struct_single_field_read() {
    let result = run("struct Point { x, y } let p = Point { x: 7, y: 3 }; p.x");
    assert_eq!(result, 7);
}

#[test]
fn test_struct_second_field_read() {
    let result = run("struct Point { x, y } let p = Point { x: 7, y: 3 }; p.y");
    assert_eq!(result, 3);
}

#[test]
fn test_struct_field_sum() {
    let result = run("struct Point { x, y } let p = Point { x: 10, y: 20 }; p.x + p.y");
    assert_eq!(result, 30);
}

#[test]
fn test_struct_field_difference() {
    let result = run("struct Point { x, y } let p = Point { x: 50, y: 8 }; p.x - p.y");
    assert_eq!(result, 42);
}

// =====================================================================
// Field ordering and layout
// =====================================================================

#[test]
fn test_struct_three_fields_each_slot() {
    // Read every slot independently to prove the layout maps each field
    // name to the correct index in declaration order.
    assert_eq!(run("struct T { a, b, c } let t = T { a: 1, b: 2, c: 3 }; t.a"), 1);
    assert_eq!(run("struct T { a, b, c } let t = T { a: 1, b: 2, c: 3 }; t.b"), 2);
    assert_eq!(run("struct T { a, b, c } let t = T { a: 1, b: 2, c: 3 }; t.c"), 3);
}

#[test]
fn test_struct_literal_field_order_independent() {
    // Fields may be given in any order in the literal; each still lands
    // in the slot named by its field, not by position.
    let result = run("struct Point { x, y } let p = Point { y: 99, x: 1 }; p.x - p.y");
    assert_eq!(result, 1 - 99);
}

#[test]
fn test_struct_missing_field_defaults_to_zero() {
    // A field omitted from the literal defaults to 0 (const_zero base).
    let result = run("struct Point { x, y } let p = Point { x: 5 }; p.y");
    assert_eq!(result, 0);
}

// =====================================================================
// Field values from expressions and variables
// =====================================================================

#[test]
fn test_struct_field_from_variable() {
    let result = run("struct Box { w } let n = 12; let b = Box { w: n }; b.w");
    assert_eq!(result, 12);
}

#[test]
fn test_struct_field_from_expression() {
    let result = run("struct Box { w } let b = Box { w: 6 * 7 }; b.w");
    assert_eq!(result, 42);
}

#[test]
fn test_struct_field_in_arithmetic_chain() {
    let result = run(
        "struct Rect { w, h } let r = Rect { w: 4, h: 5 }; r.w * r.h + 2"
    );
    assert_eq!(result, 22);
}

// =====================================================================
// Structs interacting with control flow
// =====================================================================

#[test]
fn test_struct_field_in_if_condition() {
    let result = run(
        "struct Flag { on } let f = Flag { on: 1 }; if f.on { 100 } else { 200 }"
    );
    assert_eq!(result, 100);
}

#[test]
fn test_struct_field_in_if_condition_false() {
    let result = run(
        "struct Flag { on } let f = Flag { on: 0 }; if f.on { 100 } else { 200 }"
    );
    assert_eq!(result, 200);
}

#[test]
fn test_struct_field_accumulated_in_loop() {
    let result = run(
        "struct Step { by } let s = Step { by: 3 }; let total = 0; for i in 0..4 { total = total + s.by; } total"
    );
    assert_eq!(result, 12);
}

// =====================================================================
// Multiple struct instances and multiple struct types
// =====================================================================

#[test]
fn test_two_instances_same_struct() {
    let result = run(
        "struct P { x } let a = P { x: 3 }; let b = P { x: 4 }; a.x + b.x"
    );
    assert_eq!(result, 7);
}

#[test]
fn test_two_distinct_struct_types() {
    let result = run(
        "struct A { v } struct B { v } let a = A { v: 10 }; let b = B { v: 5 }; a.v - b.v"
    );
    assert_eq!(result, 5);
}

// =====================================================================
// Emitted IR shape
// =====================================================================

#[test]
fn test_struct_literal_emits_insertvalue() {
    // Field values come from variables so the IR builder cannot
    // constant-fold the aggregate into a constant (which would elide
    // the insertvalue instructions).
    let ir = emit_ir("struct Point { x, y } let a = 1; let b = 2; let p = Point { x: a, y: b }; p.x");
    assert!(
        ir.contains("insertvalue"),
        "struct literal should emit insertvalue, got:\n{}",
        ir
    );
}

#[test]
fn test_field_access_emits_extractvalue() {
    let ir = emit_ir("struct Point { x, y } let p = Point { x: 1, y: 2 }; p.x");
    assert!(
        ir.contains("extractvalue"),
        "field access should emit extractvalue, got:\n{}",
        ir
    );
}

// =====================================================================
// Typed struct fields (type hints honored at runtime)
// =====================================================================

#[test]
fn test_typed_string_field_len() {
    // A field declared `string` stores a real {i8*, i64} struct, so
    // len() works on it.
    let result = run("struct Person { name: string, age: int } let p = Person { name: \"AHA\", age: 3 }; len(p.name)");
    assert_eq!(result, 3);
}

#[test]
fn test_typed_int_field_arithmetic() {
    let result = run("struct Person { name: string, age: int } let p = Person { name: \"x\", age: 30 }; p.age * 2");
    assert_eq!(result, 60);
}

#[test]
fn test_typed_string_field_string_concat() {
    // The field carries a real string, so + on it concatenates.
    let result = run("struct P { first: string, last: string } let p = P { first: \"A\", last: \"B\" }; len(p.first + p.last)");
    assert_eq!(result, 2);
}

#[test]
fn test_typed_string_field_equality() {
    let result = run("struct P { name: string } let p = P { name: \"hello\" }; p.name == \"hello\"");
    assert_eq!(result, 1);
}

#[test]
fn test_missing_typed_string_field_defaults() {
    // An omitted string field still zeroes out; reading it back yields
    // an empty string (len 0).
    let result = run("struct P { name: string, age: int } let p = P { age: 5 }; len(p.name)");
    assert_eq!(result, 0);
}

#[test]
fn test_typed_string_field_from_string_var() {
    let result = run("struct P { name: string } let n = \"world\"; let p = P { name: n }; len(p.name)");
    assert_eq!(result, 5);
}

// =====================================================================
// Error paths
// =====================================================================

#[test]
fn test_wrong_type_string_field_is_error() {
    let err = expect_codegen_error(
        "struct Person { name: string } let p = Person { name: 123 }; p.name"
    );
    assert!(
        err.contains("expects a string"),
        "expected a string-typed field error, got: {}",
        err
    );
}

#[test]
fn test_wrong_type_int_field_is_error() {
    let err = expect_codegen_error(
        "struct Person { name: string, age: int } let p = Person { name: \"x\", age: \"old\" }; p.age"
    );
    assert!(
        err.contains("expects") && err.contains("got string"),
        "expected an int-typed field error, got: {}",
        err
    );
}

#[test]
fn test_unknown_field_is_error() {
    let err = expect_codegen_error(
        "struct Point { x, y } let p = Point { x: 1, y: 2 }; p.z"
    );
    assert!(
        err.contains("no field") || err.contains("field 'z'"),
        "expected an unknown-field error, got: {}",
        err
    );
}

#[test]
fn test_field_access_on_non_struct_is_error() {
    let err = expect_codegen_error("let n = 5; n.x");
    assert!(
        err.contains("non-struct") || err.contains("Field access"),
        "expected a non-struct field-access error, got: {}",
        err
    );
}

// =====================================================================
// Field mutation (p.x = value)
// =====================================================================

#[test]
fn test_mutate_int_field() {
    let result = run("struct P { x, y } let p = P { x: 1, y: 2 }; p.x = 99; p.x");
    assert_eq!(result, 99);
}

#[test]
fn test_mutate_int_field_and_read_other() {
    let result = run("struct P { x, y } let p = P { x: 1, y: 2 }; p.x = 99; p.y");
    assert_eq!(result, 2);
}

#[test]
fn test_mutate_then_arithmetic() {
    let result = run("struct P { x, y } let p = P { x: 10, y: 20 }; p.x = 30; p.x + p.y");
    assert_eq!(result, 50);
}

#[test]
fn test_double_mutation() {
    let result = run("struct P { x, y } let p = P { x: 1, y: 2 }; p.x = 5; p.y = 6; p.x * p.y");
    assert_eq!(result, 30);
}

#[test]
fn test_mutate_string_field() {
    let result = run("struct P { name: string } let p = P { name: \"hello\" }; p.name = \"world\"; len(p.name)");
    assert_eq!(result, 5);
}

#[test]
fn test_mutate_string_field_equality() {
    let result = run("struct P { name: string } let p = P { name: \"hello\" }; p.name = \"world\"; p.name == \"world\"");
    assert_eq!(result, 1);
}

#[test]
fn test_mutate_field_in_loop() {
    let result = run(
        "struct P { x } let p = P { x: 0 }; for i in 0..3 { p.x = p.x + 1; } p.x"
    );
    assert_eq!(result, 3);
}

#[test]
fn test_mutate_field_wrong_type_is_error() {
    let err = expect_codegen_error(
        "struct P { name: string } let p = P { name: \"x\" }; p.name = 42; p.name"
    );
    assert!(
        err.contains("expects a string"),
        "expected a string-typed field error, got: {}",
        err
    );
}

#[test]
fn test_plain_variable_assignment_still_works() {
    // Ensure the change to generic assignment target didn't break
    // plain x = value assignments.
    let result = run("let x = 5; x = 10; x");
    assert_eq!(result, 10);
}

#[test]
fn test_plain_variable_mutate_in_loop() {
    let result = run("let total = 0; for i in 0..5 { total = total + i; } total");
    assert_eq!(result, 10);
}
