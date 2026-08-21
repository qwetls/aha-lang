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

// --- ? operator: unwraps Ok ---

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

#[test]
fn err_propagation_compiles() {
    compile_only(r#"
        fn always_err() -> Result<int, string> {
            return err("fail")
        }
        fn main() -> int {
            let v = always_err()?
            v
        }
    "#).expect("Should compile");
}

#[test]
fn ok_and_err_constructors_compile() {
    compile_only(r#"
        fn make_ok() -> Result<int, string> {
            return ok(100)
        }
        fn make_err() -> Result<int, string> {
            return err("oops")
        }
    "#).expect("Should compile");
}
