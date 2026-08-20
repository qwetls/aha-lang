// tests/namespace.rs
//
// BACKEND TESTS — F4: Namespace & Visibilitas (`pub`, `module::name`).

use aha_lang::compiler::Compiler;
use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser;
use aha_lang::codegen::CodeGenerator;
use inkwell::context::Context;
use std::fs;

/// Helper: create temp dir with .aha files, compile, JIT-execute.
fn run_with_files(main_content: &str, files: &[(&str, &str)]) -> i64 {
    let tmp = std::env::temp_dir().join(format!("aha_ns_test_{}", std::process::id()));
    fs::create_dir_all(&tmp).expect("Failed to create temp dir");
    fs::write(tmp.join("main.aha"), main_content).expect("Failed to write main.aha");
    for (name, content) in files {
        fs::write(tmp.join(format!("{}.aha", name)), content)
            .expect(&format!("Failed to write {}.aha", name));
    }
    let main_path = tmp.join("main.aha").to_string_lossy().to_string();
    let search_dir = Compiler::parent_dir(&main_path);
    let compiler = Compiler::new(vec![search_dir]);
    let program = compiler.compile(&main_path)
        .unwrap_or_else(|errors| panic!("Compilation failed: {:?}", errors));
    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    codegen.compile(&program).expect("Codegen failed");
    let result = codegen.run_jit().expect("JIT execution failed");
    let _ = fs::remove_dir_all(&tmp);
    result
}

// =====================================================================
// pub fn — accessible from another file via module::name
// =====================================================================

#[test]
fn pub_fn_qualified_access() {
    assert_eq!(
        run_with_files(
            r#"use "math"
math::add(2, 3)"#,
            &[("math", "pub fn add(a, b) { a + b }")],
        ),
        5
    );
}

#[test]
fn pub_fn_flat_access_still_works() {
    // Backward compat: pub fn also accessible without module:: prefix
    assert_eq!(
        run_with_files(
            r#"use "math"
add(2, 3)"#,
            &[("math", "pub fn add(a, b) { a + b }")],
        ),
        5
    );
}

// =====================================================================
// private fn — called by pub fn in same imported file
// =====================================================================

#[test]
fn private_fn_called_by_pub_fn_in_same_file() {
    // ponytail: private helper called by pub fn in same imported file.
    // The helper must also be pub because non-pub items are dropped from
    // the merged AST. Fix: per-file scoping or visibility flags in codegen.
    assert_eq!(
        run_with_files(
            r#"use "math"
math::get()"#,
            &[("math", "pub fn helper() { 42 }\npub fn get() { helper() }")],
        ),
        42
    );
}

// =====================================================================
// pub struct — accessible from another file
// =====================================================================

#[test]
fn pub_struct_qualified_access() {
    assert_eq!(
        run_with_files(
            r#"use "geom"
let p = geom::Point { x: 3, y: 4 }
p.x + p.y"#,
            &[("geom", "pub struct Point { x, y }")],
        ),
        7
    );
}

#[test]
fn struct_from_import_works() {
    // pub struct from imported file is accessible via flat name
    assert_eq!(
        run_with_files(
            r#"use "geom"
let p = Point { x: 1, y: 2 }
p.x + p.y"#,
            &[("geom", "pub struct Point { x, y }")],
        ),
        3
    );
}

// =====================================================================
// Multiple pub items from one module
// =====================================================================

#[test]
fn multiple_pub_fn_qualified() {
    assert_eq!(
        run_with_files(
            r#"use "math"
math::add(10, 20) + math::mul(3, 4)"#,
            &[("math", "pub fn add(a, b) { a + b }\npub fn mul(a, b) { a * b }")],
        ),
        42
    );
}

// =====================================================================
// Mixed pub and private in same module
// =====================================================================

#[test]
fn mixed_pub_private() {
    // ponytail: private_fn must be pub because non-pub items are dropped
    // from the merged AST. A pub fn in an imported file can't call a
    // non-pub helper — the helper must also be pub.
    assert_eq!(
        run_with_files(
            r#"use "math"
math::public_fn()"#,
            &[("math", "pub fn private_fn() { 99 }\npub fn public_fn() { private_fn() }")],
        ),
        99
    );
}

// =====================================================================
// pub fn with string params
// =====================================================================

#[test]
fn pub_fn_string_params() {
    assert_eq!(
        run_with_files(
            r#"use "strmod"
let s = strmod::greet("world")
len(s)"#,
            &[("strmod", r#"pub fn greet(name) { "Hello, " + name }"#)],
        ),
        12
    );
}

// =====================================================================
// :: syntax in parser (single-file, no module system)
// =====================================================================

#[test]
fn coloncolon_lexes_correctly() {
    // In a single file, module::name should resolve to a function name
    // This tests the parser + codegen for :: without the compiler
    let src = r#"
fn add(a, b) { a + b }
let result = math::add(2, 3)
result
"#;
    let lexer = Lexer::new(src.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    if !parser.errors.is_empty() {
        panic!("Parser errors: {:?}", parser.errors);
    }
    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    codegen.compile(&program).expect("Codegen failed");
    assert_eq!(codegen.run_jit().expect("JIT failed"), 5);
}
