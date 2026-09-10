// tests/rest_api_tutorial.rs
//
// Regression tests for the 2026-09-09 systemic null-termination bug (v1.7.1).
//
// Root cause: runtime functions returned Box<str> (NOT null-terminated) while
// codegen measured returned strings with strlen — so http_request_method/path/
// body/header, http_response, json_stringify and json_get read past the
// allocation, producing garbage lengths in the GET/POST pipeline.
//
// Two layers of coverage:
//   1. Compile-check every examples/*.aha shipped with the REST API tutorial.
//   2. JIT-execute the parsing pipeline and assert exact len() of each
//      returned string — a non-null-terminated buffer yields a wrong length.

use aha_lang::codegen::CodeGenerator;
use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser;
use inkwell::context::Context;

fn compile(source: &str) {
    let lexer = Lexer::new(source.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    if !parser.errors.is_empty() {
        panic!("Parser errors: {:?}", parser.errors);
    }
    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    codegen.compile(&program).expect("Codegen failed");
}

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

// --- 1. Tutorial examples must compile ---

#[test]
fn example_hello_compiles() {
    compile(include_str!("../examples/hello.aha"));
}

#[test]
fn example_rest_api_compiles() {
    compile(include_str!("../examples/rest_api.aha"));
}

#[test]
fn example_todo_api_compiles() {
    compile(include_str!("../examples/todo_api.aha"));
}

// --- 2. Parsing pipeline returns correctly-sized (null-terminated) strings ---

#[test]
fn request_method_len_accurate() {
    // "GET" -> exactly 3; strlen past a non-terminated Box<str> gave garbage.
    let result = run(r#"
        let request = "GET /api/todos/42 HTTP/1.1\r\nHost: localhost\r\n\r\n{\"id\":1}"
        let method = http_request_method(request)
        len(method)
    "#);
    assert_eq!(result, 3);
}

#[test]
fn request_path_len_accurate() {
    // "/api/todos/42" -> exactly 13.
    let result = run(r#"
        let request = "GET /api/todos/42 HTTP/1.1\r\nHost: localhost\r\n\r\n"
        let path = http_request_path(request)
        len(path)
    "#);
    assert_eq!(result, 13);
}

#[test]
fn request_body_len_accurate() {
    // `{"id":1}` -> exactly 8 bytes after the \r\n\r\n boundary.
    let result = run(r#"
        let request = "POST /api/todos HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{\"id\":1}"
        let body = http_request_body(request)
        len(body)
    "#);
    assert_eq!(result, 8);
}

#[test]
fn request_header_len_accurate() {
    // "application/json" -> exactly 16.
    let result = run(r#"
        let request = "POST /api/todos HTTP/1.1\r\nContent-Type: application/json\r\n\r\n"
        let ct = http_request_header(request, "Content-Type")
        len(ct)
    "#);
    assert_eq!(result, 16);
}

#[test]
fn parsed_method_matches_route_string() {
    // Equality against a route literal is the real routing check an AHA server
    // performs; it fails if the parsed method isn't exactly "POST".
    let result = run(r#"
        let request = "POST /api/todos HTTP/1.1\r\nHost: localhost\r\n\r\n{\"title\":\"Learn AHA!\"}"
        let method = http_request_method(request)
        if method == "POST" {
            1
        } else {
            0
        }
    "#);
    assert_eq!(result, 1);
}

#[test]
fn string_compare_composes_with_int_builtins() {
    // Regression: string == was tagged Bool, so `method == "GET" &&
    // str_contains(...)` (Bool && Int) was rejected by codegen. Comparisons
    // must return Int (0/1) so they compose with && / || / arithmetic.
    let result = run(r#"
        let request = "GET /api/users/7 HTTP/1.1\r\nHost: localhost\r\n\r\n"
        let method = http_request_method(request)
        let path = http_request_path(request)
        (method == "GET") + str_contains(path, "/api/users/")
    "#);
    assert_eq!(result, 2);
}

#[test]
fn http_response_len_accurate() {
    // http_response builds "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n
    // Content-Length: 5\r\nConnection: close\r\n\r\nHello" — Content-Length must
    // be the real body length, and the whole buffer must be null-terminated.
    let result = run(r#"
        let resp = http_response(200, "Hello")
        let body = http_request_body(resp)
        len(body)
    "#);
    // Body "Hello" -> 5; a non-terminated response buffer would mis-measure it.
    assert_eq!(result, 5);
}
