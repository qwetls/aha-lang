// tests/ffi.rs
//
// BACKEND TESTS — FFI support (Roadmap Phase 8: "extern fn").
// Tests extern function declarations and calling C library functions
// from AHA! code via LLVM external linkage.
//
// Current tests focus on PARSING and COMPILATION of extern fn declarations.
// Calling C functions via JIT requires dlsym resolution which may not work
// in all CI environments. Full end-to-end calling tests will be added once
// the JIT symbol resolution is verified.

use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser;
use aha_lang::codegen::CodeGenerator;
use inkwell::context::Context;

/// Helper: parse only, returning errors.
fn parse_only(source: &str) -> Vec<String> {
    let lexer = Lexer::new(source.to_string());
    let mut parser = Parser::new(lexer);
    let _ = parser.parse_program();
    parser.errors
}

/// Helper: compile only (no JIT), returning codegen errors.
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

/// Helper: expect a parser error matching the given substring.
fn expect_parse_error(source: &str, expected: &str) {
    let errors = parse_only(source);
    assert!(!errors.is_empty(),
        "Expected parser error containing '{}', but no errors occurred", expected);
    let all_errors = errors.join("; ");
    assert!(all_errors.contains(expected),
        "Expected error containing '{}', got: {}", expected, all_errors);
}

// --- Parsing tests ---

#[test]
fn extern_fn_parse_basic() {
    let errors = parse_only(r#"
        extern fn atoi(s: int) -> int;
        42
    "#);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
}

#[test]
fn extern_fn_parse_pointer_param() {
    let errors = parse_only(r#"
        extern fn atol(s: *void) -> int;
        42
    "#);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
}

#[test]
fn extern_fn_parse_multiple() {
    let errors = parse_only(r#"
        extern fn atoi(s: int) -> int;
        extern fn atol(s: *void) -> int;
        42
    "#);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
}

#[test]
fn extern_fn_parse_no_return_type() {
    let errors = parse_only(r#"
        extern fn puts(s: *void);
        42
    "#);
    assert!(errors.is_empty(), "Parse errors: {:?}", errors);
}

// --- Compilation tests (no JIT) ---

#[test]
fn extern_fn_compile_basic() {
    compile_only(r#"
        extern fn atoi(s: int) -> int;
        42
    "#).expect("Compilation should succeed");
}

#[test]
fn extern_fn_compile_pointer_param() {
    compile_only(r#"
        extern fn atol(s: *void) -> int;
        42
    "#).expect("Compilation should succeed");
}

#[test]
fn extern_fn_compile_redeclare_builtin() {
    // strlen is already declared by C runtime — re-declaring should be a no-op
    compile_only(r#"
        extern fn strlen(s: *void) -> int;
        99
    "#).expect("Compilation should succeed");
}

// --- JIT tests (only safe ones — no pointer args) ---

#[test]
fn extern_fn_jit_declared_not_called() {
    let result = run(r#"
        extern fn atoi(s: int) -> int;
        42
    "#);
    assert_eq!(result, 42);
}

#[test]
fn extern_fn_jit_pointer_param_compile_only() {
    // atol takes *void — verify it compiles, but don't call it
    let result = run(r#"
        extern fn atol(s: *void) -> int;
        77
    "#);
    assert_eq!(result, 77);
}

// --- Error cases ---

#[test]
fn extern_fn_missing_fn_keyword() {
    expect_parse_error(
        r#"extern atoi(s: int) -> int;"#,
        "Expected 'fn' after 'extern'",
    );
}

#[test]
fn extern_fn_missing_semicolon() {
    expect_parse_error(
        r#"extern fn atoi(s: int) -> int
        42"#,
        "Expected ';' after extern fn declaration",
    );
}
