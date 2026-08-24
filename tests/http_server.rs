// tests/http_server.rs
//
// F11 HTTP Server — compile-only tests for HTTP builtins.

use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser;
use aha_lang::codegen::CodeGenerator;
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

// --- http_listen ---

#[test]
fn http_listen_compiles() {
    compile(r#"
        let server = http_listen(8080)
        server
    "#);
}

// --- http_accept ---

#[test]
fn http_accept_compiles() {
    compile(r#"
        let server = http_listen(8080)
        let client = http_accept(server)
        client
    "#);
}

// --- http_recv ---

#[test]
fn http_recv_compiles() {
    compile(r#"
        let server = http_listen(8080)
        let client = http_accept(server)
        let request = http_recv(client)
        request
    "#);
}

// --- http_send ---

#[test]
fn http_send_compiles() {
    compile(r#"
        let server = http_listen(8080)
        let client = http_accept(server)
        let request = http_recv(client)
        let response = http_response(200, "Hello World")
        let sent = http_send(client, response)
        sent
    "#);
}

// --- http_request_method ---

#[test]
fn http_request_method_compiles() {
    compile(r#"
        let server = http_listen(8080)
        let client = http_accept(server)
        let request = http_recv(client)
        let method = http_request_method(request)
        method
    "#);
}

// --- http_request_path ---

#[test]
fn http_request_path_compiles() {
    compile(r#"
        let server = http_listen(8080)
        let client = http_accept(server)
        let request = http_recv(client)
        let path = http_request_path(request)
        path
    "#);
}

// --- http_request_body ---

#[test]
fn http_request_body_compiles() {
    compile(r#"
        let server = http_listen(8080)
        let client = http_accept(server)
        let request = http_recv(client)
        let body = http_request_body(request)
        body
    "#);
}

// --- http_request_header ---

#[test]
fn http_request_header_compiles() {
    compile(r#"
        let server = http_listen(8080)
        let client = http_accept(server)
        let request = http_recv(client)
        let ct = http_request_header(request, "Content-Type")
        ct
    "#);
}

// --- http_response ---

#[test]
fn http_response_compiles() {
    compile(r#"
        let response = http_response(200, "OK")
        response
    "#);
}

// --- Full server pattern ---

#[test]
fn http_server_pattern_compiles() {
    compile(r#"
        let server = http_listen(8080)
        let client = http_accept(server)
        let request = http_recv(client)
        let method = http_request_method(request)
        let path = http_request_path(request)
        let body = http_request_body(request)
        let ct = http_request_header(request, "Content-Type")
        let response = http_response(200, "Hello from AHA!")
        let sent = http_send(client, response)
        sent
    "#);
}
