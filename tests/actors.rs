// tests/actors.rs
//
// F6 Phase 1 — Actor-model concurrency tests.

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

// =====================================================================
// F6 Phase 1: Actor spawn + call
// =====================================================================

#[test]
fn actor_basic_call() {
    // spawn an actor, call it with a message, get the result.
    // The handler function (fn handle) processes the message.
    let src = r#"
actor Echo {
    id: int
}

fn handle(state, msg) -> int {
    msg
}

let a = spawn Echo { id: 1 }
call(a, 42)
"#;
    assert_eq!(run(src), 42);
}

#[test]
fn actor_multiple_calls() {
    // Multiple call invocations on the same actor.
    let src = r#"
actor Counter {
    count: int
}

fn handle(state, msg) -> int {
    msg
}

let c = spawn Counter { count: 0 }
let r1 = call(c, 10)
let r2 = call(c, 20)
r1 + r2
"#;
    assert_eq!(run(src), 30);
}

#[test]
fn actor_send_then_call() {
    // send (fire-and-forget) followed by call.
    let src = r#"
actor Worker {
    id: int
}

fn handle(state, msg) -> int {
    msg
}

let w = spawn Worker { id: 5 }
send(w, 100)
call(w, 200)
"#;
    assert_eq!(run(src), 200);
}

#[test]
fn actor_state_ignored_by_handler() {
    // Handler uses msg only (state is a pointer, not the field value).
    let src = r#"
actor Node {
    value: int
}

fn handle(state, msg) -> int {
    msg * 2
}

let n = spawn Node { value: 7 }
call(n, 5)
"#;
    assert_eq!(run(src), 10);
}
