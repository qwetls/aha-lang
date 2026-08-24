// tests/json.rs
//
// F12 JSON Parser/Serializer — compile-only tests for JSON builtins.

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

// --- json_parse ---

#[test]
fn json_parse_compiles() {
    compile(r#"
        let json = json_parse("{\"key\": \"value\"}")
        json
    "#);
}

#[test]
fn json_parse_nested_compiles() {
    compile(r#"
        let json = json_parse("{\"user\": {\"name\": \"AHA\"}}")
        json
    "#);
}

// --- json_stringify ---

#[test]
fn json_stringify_compiles() {
    compile(r#"
        let json = json_parse("{\"key\": 42}")
        let str = json_stringify(json)
        str
    "#);
}

// --- json_get ---

#[test]
fn json_get_compiles() {
    compile(r#"
        let json = json_parse("{\"user\": {\"name\": \"AHA\", \"age\": 1}}")
        let name = json_get(json, "user.name")
        name
    "#);
}

#[test]
fn json_get_array_compiles() {
    compile(r#"
        let json = json_parse("{\"items\": [1, 2, 3]}")
        let first = json_get(json, "items.0")
        first
    "#);
}

// --- json_free ---

#[test]
fn json_free_compiles() {
    compile(r#"
        let json = json_parse("{\"key\": \"value\"}")
        let val = json_get(json, "key")
        json_free(json)
    "#);
}

// --- Full pattern ---

#[test]
fn json_full_pattern_compiles() {
    compile(r#"
        let json = json_parse("{\"status\": \"ok\", \"data\": {\"count\": 42}}")
        let status = json_get(json, "status")
        let count = json_get(json, "data.count")
        let output = json_stringify(json)
        json_free(json)
        output
    "#);
}

#[test]
fn json_http_pattern_compiles() {
    compile(r#"
        let server = http_listen(8080)
        let client = http_accept(server)
        let request = http_recv(client)
        let body = http_request_body(request)
        let json = json_parse(body)
        let name = json_get(json, "name")
        let response = http_response(200, name)
        json_free(json)
        http_send(client, response)
    "#);
}
