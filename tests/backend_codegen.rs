// tests/backend_codegen.rs
//
// BACKEND TESTS — focused on the code-generation backend itself:
// LLVM IR emission (via get_llvm_ir) and JIT execution semantics
// (via run_jit). Unlike the integration suites that mostly assert
// end-to-end results, these tests inspect the *generated IR* and
// exercise backend-specific behavior: signed division/remainder,
// builtin emission, module structure, and instruction selection.

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

// =====================================================================
// LLVM Module Structure
// =====================================================================

#[test]
fn test_ir_is_not_empty() {
    let ir = emit_ir("42");
    assert!(!ir.trim().is_empty(), "IR should not be empty");
}

#[test]
fn test_ir_has_module_id() {
    let ir = emit_ir("1");
    assert!(ir.contains("aha_module"), "IR should carry the module id, got:\n{}", ir);
}

#[test]
fn test_ir_defines_main() {
    let ir = emit_ir("1 + 1");
    assert!(ir.contains("@main"), "IR should define @main, got:\n{}", ir);
    assert!(ir.contains("define"), "IR should contain a function definition, got:\n{}", ir);
}

#[test]
fn test_ir_declares_printf() {
    // printf is always declared for the print builtins.
    let ir = emit_ir("1");
    assert!(ir.contains("@printf"), "IR should declare printf, got:\n{}", ir);
}

// =====================================================================
// Builtins are always emitted (declare_printf runs unconditionally)
// =====================================================================

#[test]
fn test_ir_emits_all_builtins() {
    // Even a program that uses none of them still emits every builtin,
    // because compile() calls declare_printf() unconditionally.
    let ir = emit_ir("0");
    for builtin in ["@print", "@print_str", "@abs", "@min", "@max", "@len"] {
        assert!(
            ir.contains(builtin),
            "IR should emit builtin {}, got:\n{}",
            builtin,
            ir
        );
    }
}

// =====================================================================
// Instruction Selection — arithmetic maps to the right LLVM opcodes.
// Operands are loaded from variables so the IR builder cannot
// constant-fold them away into a literal.
// =====================================================================

#[test]
fn test_ir_add_instruction() {
    let ir = emit_ir("let a = 7; let b = 5; a + b");
    assert!(ir.contains("add"), "expected an add instruction, got:\n{}", ir);
}

#[test]
fn test_ir_sub_instruction() {
    let ir = emit_ir("let a = 7; let b = 5; a - b");
    assert!(ir.contains("sub"), "expected a sub instruction, got:\n{}", ir);
}

#[test]
fn test_ir_mul_instruction() {
    let ir = emit_ir("let a = 7; let b = 5; a * b");
    assert!(ir.contains("mul"), "expected a mul instruction, got:\n{}", ir);
}

#[test]
fn test_ir_signed_div_instruction() {
    let ir = emit_ir("let a = 20; let b = 4; a / b");
    assert!(ir.contains("sdiv"), "expected a signed-div instruction, got:\n{}", ir);
}

#[test]
fn test_ir_signed_rem_instruction() {
    let ir = emit_ir("let a = 20; let b = 6; a % b");
    assert!(ir.contains("srem"), "expected a signed-rem instruction, got:\n{}", ir);
}

#[test]
fn test_ir_comparison_emits_icmp() {
    let ir = emit_ir("let a = 5; let b = 3; a > b");
    assert!(ir.contains("icmp"), "expected an icmp instruction, got:\n{}", ir);
}

#[test]
fn test_ir_all_arithmetic_opcodes_present() {
    // One program exercising every arithmetic opcode with variables.
    let ir = emit_ir("let a = 20; let b = 6; a + b - a * b / (a % b)");
    for opcode in ["add", "sub", "mul", "sdiv", "srem"] {
        assert!(
            ir.contains(opcode),
            "expected opcode {} in IR, got:\n{}",
            opcode,
            ir
        );
    }
}

// =====================================================================
// JIT Backend Semantics — signed integer division & remainder
// (LLVM sdiv/srem truncate toward zero).
// =====================================================================

#[test]
fn test_jit_division_truncates_toward_zero() {
    assert_eq!(run("let a = 7; let b = 2; a / b"), 3);
}

#[test]
fn test_jit_negative_division_truncates_toward_zero() {
    // -7 sdiv 2 = -3 (truncated toward zero, not floored to -4)
    assert_eq!(run("let a = 0 - 7; let b = 2; a / b"), -3);
}

#[test]
fn test_jit_remainder_follows_dividend_sign() {
    // -7 srem 3 = -1 (remainder takes the sign of the dividend)
    assert_eq!(run("let a = 0 - 7; let b = 3; a % b"), -1);
}

#[test]
fn test_jit_positive_remainder() {
    assert_eq!(run("let a = 17; let b = 5; a % b"), 2);
}

#[test]
fn test_jit_division_exact() {
    assert_eq!(run("let a = 100; let b = 5; a / b"), 20);
}

// =====================================================================
// JIT Backend Semantics — large 64-bit arithmetic (no overflow).
// =====================================================================

#[test]
fn test_jit_large_multiplication_fits_i64() {
    // 1_000_000 * 1_000_000 = 1_000_000_000_000, well within i64.
    assert_eq!(run("let a = 1000000; let b = 1000000; a * b"), 1000000000000);
}

#[test]
fn test_jit_large_addition() {
    assert_eq!(run("let a = 2000000000; let b = 2000000000; a + b"), 4000000000);
}

// =====================================================================
// JIT Backend Semantics — boolean-producing operators lower to Int 0/1.
// =====================================================================

#[test]
fn test_jit_comparison_lowers_to_one() {
    assert_eq!(run("let a = 10; let b = 3; a > b"), 1);
}

#[test]
fn test_jit_comparison_lowers_to_zero() {
    assert_eq!(run("let a = 3; let b = 10; a > b"), 0);
}

#[test]
fn test_jit_boolean_result_flows_into_arithmetic() {
    // (a == a) is 1, times 100 = 100; plus (a != a) which is 0.
    assert_eq!(run("let a = 5; (a == a) * 100 + (a != a)"), 100);
}

// =====================================================================
// JIT Backend Semantics — control-flow codegen (branches & phi-free
// loops) produce correct results end to end.
// =====================================================================

#[test]
fn test_jit_while_accumulator_codegen() {
    let src = r#"
        let sum = 0;
        let i = 0;
        while i < 100 {
            sum = sum + i;
            i = i + 1;
        }
        sum
    "#;
    assert_eq!(run(src), 4950);
}

#[test]
fn test_jit_for_range_codegen() {
    let src = r#"
        let total = 0;
        for i in 1..11 {
            total = total + i;
        }
        total
    "#;
    assert_eq!(run(src), 55);
}

#[test]
fn test_jit_if_else_branch_codegen() {
    let src = r#"
        let x = 42;
        if x > 40 { 111 } else { 222 }
    "#;
    assert_eq!(run(src), 111);
}

// =====================================================================
// JIT Backend Semantics — function call codegen & recursion.
// =====================================================================

#[test]
fn test_jit_recursive_call_codegen() {
    let src = r#"
        fn fact(n) {
            if n <= 1 { return 1; }
            return n * fact(n - 1);
        }
        fact(6)
    "#;
    assert_eq!(run(src), 720);
}

#[test]
fn test_jit_nested_call_codegen() {
    let src = r#"
        fn inc(x) { x + 1 }
        fn twice(x) { inc(inc(x)) }
        twice(40)
    "#;
    assert_eq!(run(src), 42);
}

// =====================================================================
// JIT Backend Semantics — array literal & index codegen.
// =====================================================================

#[test]
fn test_jit_array_index_codegen() {
    let src = r#"
        let arr = [10, 20, 30, 40];
        arr[0] + arr[3]
    "#;
    assert_eq!(run(src), 50);
}

#[test]
fn test_jit_array_computed_index_codegen() {
    let src = r#"
        let arr = [5, 15, 25, 35];
        arr[1 + 1]
    "#;
    assert_eq!(run(src), 25);
}

// =====================================================================
// JIT Backend Semantics — string struct codegen (len is O(1) from the
// {ptr, len} struct's length field).
// =====================================================================

#[test]
fn test_jit_string_len_codegen() {
    assert_eq!(run(r#"len("backend")"#), 7);
}

#[test]
fn test_jit_string_concat_len_codegen() {
    assert_eq!(run(r#"len("aha" + "lang")"#), 7);
}
