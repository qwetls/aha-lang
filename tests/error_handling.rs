// tests/error_handling.rs
//
// F9 — Error handling: Result<T, E> type, ok/err constructors, ? operator.

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

fn compile_only(source: &str) -> Result<(), String> {
    let lexer = Lexer::new(source.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    if !parser.errors.is_empty() {
        return Err(format!("Parser errors: {:?}", parser.errors));
    }
    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    codegen.compile(&program)
}

// --- Basic ok/err ---

#[test]
fn result_ok_returns_zero_tag() {
    // ok(42) → Result { tag=0, payload=42 }
    // We return the result struct as i64 (tag) via match
    let result = run(r#"
        fn get_value() -> Result<int, string> {
            return ok(42)
        }
        match get_value() {
            Ok(v) => v,
            Err(e) => 0,
        }
    "#);
    assert_eq!(result, 42);
}

#[test]
fn result_err_returns_one_tag() {
    let result = run(r#"
        fn get_value() -> Result<int, string> {
            return err("something went wrong")
        }
        match get_value() {
            Ok(v) => v,
            Err(e) => 99,
        }
    "#);
    assert_eq!(result, 99);
}

// --- ? operator ---

#[test]
fn question_mark_unwraps_ok() {
    let result = run(r#"
        fn double(x: int) -> Result<int, string> {
            return ok(x * 2)
        }
        fn main() -> int {
            let v = double(21)?
            v
        }
    "#);
    assert_eq!(result, 42);
}

#[test]
fn question_mark_propagates_err() {
    // If any called function returns Err, ? propagates it up
    let result = run(r#"
        fn always_err() -> Result<int, string> {
            return err("fail")
        }
        fn main() -> int {
            let v = always_err()?
            v
        }
    "#);
    // The ? propagates Err → function returns early with Err tag
    // Since main returns int (not Result), this tests the Err propagation path
    // The Err result struct is {1, ptr} — the tag is 1
    assert_eq!(result, 1);
}

// --- Chaining ? ---

#[test]
fn chain_question_marks() {
    let result = run(r#"
        fn add_one(x: int) -> Result<int, string> {
            return ok(x + 1)
        }
        fn main() -> int {
            let a = add_one(10)?
            let b = add_one(a)?
            b
        }
    "#);
    assert_eq!(result, 12);
}

// --- Compile-only tests (verify codegen doesn't crash) ---

#[test]
fn result_type_as_return_type() {
    compile_only(r#"
        fn divide(a: int, b: int) -> Result<int, string> {
            if b == 0 {
                return err("division by zero")
            }
            return ok(a / b)
        }
    "#).expect("Should compile");
}

#[test]
fn result_with_question_mark_compiles() {
    compile_only(r#"
        fn parse(s: int) -> Result<int, string> {
            return ok(s)
        }
        fn main() -> int {
            let v = parse(42)?
            v
        }
    "#).expect("Should compile");
}
