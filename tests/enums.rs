// tests/enums.rs
//
// BACKEND TESTS — enum + pattern matching (Roadmap Phase 7: "Enum keyword
// + pattern matching"). Tests unit enums, tuple enums, match expressions,
// destructuring, and wildcard patterns.

use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser;
use aha_lang::codegen::CodeGenerator;
use inkwell::context::Context;

/// Helper: parse only, returning errors and statement count.
fn parse_only(source: &str) -> (Vec<String>, usize) {
    let lexer = Lexer::new(source.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    let count = program.statements.len();
    eprintln!("[TEST DBG] parse_only: {} stmts, {} errors", count, parser.errors.len());
    for (i, e) in parser.errors.iter().enumerate() {
        eprintln!("[TEST DBG]   error[{}]: {}", i, e);
    }
    (parser.errors, count)
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

// --- Diagnostic: does tuple enum parse at all? ---

#[test]
fn enum_tuple_parse_diagnostic() {
    let (errors, stmts) = parse_only(r#"
        enum Op { Add(int, int), Sub(int, int) }
        fn main() -> int { 42 }
    "#);
    eprintln!("[TEST DBG] enum_tuple_parse_diagnostic: stmts={}, errors={:?}", stmts, errors);
    assert_eq!(stmts, 2, "Expected 2 statements (enum + fn), got {}", stmts);
    assert!(errors.is_empty(), "Tuple enum parse errors: {:?}", errors);
}

#[test]
fn enum_unit_parse_diagnostic() {
    let (errors, stmts) = parse_only(r#"
        enum Color { Red, Green, Blue }
        fn main() -> int { 1 }
    "#);
    eprintln!("[TEST DBG] enum_unit_parse_diagnostic: stmts={}, errors={:?}", stmts, errors);
    assert_eq!(stmts, 2, "Expected 2 statements (enum + fn), got {}", stmts);
    assert!(errors.is_empty(), "Unit enum parse errors: {:?}", errors);
}

#[test]
fn enum_tuple_minimal_diagnostic() {
    // Absolute minimum: just an enum, no following function
    let (errors, stmts) = parse_only("enum Op { Add(int, int) }");
    eprintln!("[TEST DBG] enum_tuple_minimal_diagnostic: stmts={}, errors={:?}", stmts, errors);
    assert_eq!(stmts, 1, "Expected 1 statement, got {}", stmts);
    assert!(errors.is_empty(), "Minimal tuple enum errors: {:?}", errors);
}

// --- Unit enum tests ---

#[test]
fn enum_unit_basic() {
    let result = run(r#"
        enum Color { Red, Green, Blue }

        fn main() -> int {
            let c = Red()
            match c {
                Red => 1,
                Green => 2,
                Blue => 3,
                _ => 0,
            }
        }
    "#);
    assert_eq!(result, 1);
}

#[test]
fn enum_unit_second_variant() {
    let result = run(r#"
        enum Color { Red, Green, Blue }

        fn main() -> int {
            let c = Green()
            match c {
                Red => 10,
                Green => 20,
                Blue => 30,
                _ => 0,
            }
        }
    "#);
    assert_eq!(result, 20);
}

#[test]
fn enum_unit_third_variant() {
    let result = run(r#"
        enum Color { Red, Green, Blue }

        fn main() -> int {
            let c = Blue()
            match c {
                Red => 100,
                Green => 200,
                Blue => 300,
                _ => 0,
            }
        }
    "#);
    assert_eq!(result, 300);
}

#[test]
fn enum_unit_wildcard() {
    let result = run(r#"
        enum Shape { Circle, Square }

        fn main() -> int {
            let s = Square()
            match s {
                Circle => 1,
                _ => 99,
            }
        }
    "#);
    assert_eq!(result, 99);
}

// --- Tuple enum tests ---

#[test]
fn enum_tuple_one_field() {
    let result = run(r#"
        enum Option { Some(int), None }

        fn main() -> int {
            let x = Some(42)
            match x {
                Some(v) => v,
                None => 0,
            }
        }
    "#);
    assert_eq!(result, 42);
}

#[test]
fn enum_tuple_two_fields() {
    let result = run(r#"
        enum Pair { Make(int, int) }

        fn main() -> int {
            let p = Make(10, 20)
            match p {
                Make(a, b) => a + b,
                _ => 0,
            }
        }
    "#);
    assert_eq!(result, 30);
}

#[test]
fn enum_tuple_destructure_math() {
    let result = run(r#"
        enum Point { Pt(int, int) }

        fn main() -> int {
            let p = Pt(3, 7)
            match p {
                Pt(x, y) => x * y + x,
                _ => 0,
            }
        }
    "#);
    assert_eq!(result, 24); // 3*7 + 3 = 24
}

// --- Mixed unit + tuple enum ---

#[test]
fn enum_mixed_unit_and_tuple() {
    let result = run(r#"
        enum Result { Ok(int), Err }

        fn main() -> int {
            let r = Ok(500)
            match r {
                Ok(v) => v,
                Err => -1,
            }
        }
    "#);
    assert_eq!(result, 500);
}

#[test]
fn enum_mixed_unit_and_tuple_err() {
    let result = run(r#"
        enum Result { Ok(int), Err }

        fn main() -> int {
            let r = Err()
            match r {
                Ok(v) => v,
                Err => -1,
            }
        }
    "#);
    assert_eq!(result, -1);
}

// --- Match in function ---

#[test]
fn enum_match_in_function() {
    let result = run(r#"
        enum Day { Mon, Tue, Wed, Thu, Fri, Sat, Sun }

        fn is_weekend(d: Day) -> int {
            match d {
                Sat => 1,
                Sun => 1,
                _ => 0,
            }
        }

        fn main() -> int {
            let d = Sat()
            is_weekend(d)
        }
    "#);
    assert_eq!(result, 1);
}

// --- Match with arithmetic in arms ---

#[test]
fn enum_match_arithmetic() {
    let result = run(r#"
        enum Op { Add(int, int), Sub(int, int) }

        fn main() -> int {
            let op = Sub(100, 37)
            match op {
                Add(a, b) => a + b,
                Sub(a, b) => a - b,
                _ => 0,
            }
        }
    "#);
    assert_eq!(result, 63);
}

// --- Nested match ---

#[test]
fn enum_nested_match() {
    let result = run(r#"
        enum Inner { A(int), B }
        enum Outer { X(Inner), Y }

        fn main() -> int {
            let o = X(A(7))
            match o {
                X(inner) => match inner {
                    A(v) => v * 3,
                    B => 0,
                },
                Y => -1,
            }
        }
    "#);
    assert_eq!(result, 21);
}

// --- Enum with two variants, both tuple ---

#[test]
fn enum_two_tuple_variants() {
    let result = run(r#"
        enum Either { Left(int), Right(int) }

        fn main() -> int {
            let e = Right(99)
            match e {
                Left(v) => v,
                Right(v) => v + 1,
            }
        }
    "#);
    assert_eq!(result, 100);
}

// --- Single variant enum ---

#[test]
fn enum_single_variant() {
    let result = run(r#"
        enum Wrapper { Only(int) }

        fn main() -> int {
            let w = Only(55)
            match w {
                Only(v) => v,
                _ => 0,
            }
        }
    "#);
    assert_eq!(result, 55);
}
