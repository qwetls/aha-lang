// tests/parser_tests.rs
//
// Unit tests for the AHA! Parser module

use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser;
use aha_lang::ast::*;

/// Helper: parse source code and return the program (panics on error)
fn parse(input: &str) -> Program {
    let lexer = Lexer::new(input.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    if !parser.errors.is_empty() {
        panic!("Parser errors: {:?}", parser.errors);
    }
    program
}

/// Helper: parse and expect errors
#[allow(dead_code)]
fn parse_with_errors(input: &str) -> Vec<String> {
    let lexer = Lexer::new(input.to_string());
    let mut parser = Parser::new(lexer);
    parser.parse_program();
    parser.errors
}

// =====================================================================
// Let Statements
// =====================================================================

#[test]
fn test_let_integer() {
    let program = parse("let x = 42;");
    assert_eq!(program.statements.len(), 1);
    if let Statement::Let(let_stmt) = &program.statements[0] {
        assert_eq!(let_stmt.name.value, "x");
        if let Expression::Integer(int_lit) = &let_stmt.value {
            assert_eq!(int_lit.value, 42);
        } else {
            panic!("Expected Integer expression, got {:?}", let_stmt.value);
        }
    } else {
        panic!("Expected Let statement");
    }
}

#[test]
fn test_let_string() {
    let program = parse("let name = \"hello\";");
    assert_eq!(program.statements.len(), 1);
    if let Statement::Let(let_stmt) = &program.statements[0] {
        assert_eq!(let_stmt.name.value, "name");
        if let Expression::String(str_lit) = &let_stmt.value {
            assert_eq!(str_lit.value, "hello");
        } else {
            panic!("Expected String expression");
        }
    } else {
        panic!("Expected Let statement");
    }
}

#[test]
fn test_let_boolean() {
    let program = parse("let flag = true;");
    if let Statement::Let(let_stmt) = &program.statements[0] {
        if let Expression::Boolean(b) = &let_stmt.value {
            assert_eq!(b.value, true);
        } else {
            panic!("Expected Boolean expression");
        }
    } else {
        panic!("Expected Let statement");
    }
}

// =====================================================================
// Return Statements
// =====================================================================

#[test]
fn test_return_expression() {
    let program = parse("return 42;");
    if let Statement::Return(ret) = &program.statements[0] {
        if let Expression::Integer(i) = &ret.return_value {
            assert_eq!(i.value, 42);
        } else {
            panic!("Expected Integer in return");
        }
    } else {
        panic!("Expected Return statement");
    }
}

// =====================================================================
// Arithmetic Expressions
// =====================================================================

#[test]
fn test_infix_addition() {
    let program = parse("1 + 2");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Infix(infix) = &expr_stmt.expression {
            assert_eq!(infix.operator, "+");
        } else {
            panic!("Expected Infix expression");
        }
    } else {
        panic!("Expected Expression statement");
    }
}

#[test]
fn test_operator_precedence() {
    // 2 + 3 * 4 should parse as 2 + (3 * 4)
    let program = parse("2 + 3 * 4");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Infix(infix) = &expr_stmt.expression {
            assert_eq!(infix.operator, "+");
            // Right side should be 3 * 4
            if let Expression::Infix(right_infix) = infix.right.as_ref() {
                assert_eq!(right_infix.operator, "*");
            } else {
                panic!("Right side should be infix 3*4");
            }
        } else {
            panic!("Expected Infix expression");
        }
    }
}

// =====================================================================
// Comparison Operators
// =====================================================================

#[test]
fn test_comparison_operators() {
    let ops = vec!["==", "!=", "<", ">", "<=", ">="];
    for op in ops {
        let program = parse(&format!("1 {} 2", op));
        if let Statement::Expression(expr_stmt) = &program.statements[0] {
            if let Expression::Infix(infix) = &expr_stmt.expression {
                assert_eq!(infix.operator, op, "Operator mismatch for {}", op);
            } else {
                panic!("Expected Infix for {}", op);
            }
        }
    }
}

// =====================================================================
// Prefix Expressions
// =====================================================================

#[test]
fn test_prefix_negation() {
    let program = parse("-5");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Prefix(prefix) = &expr_stmt.expression {
            assert_eq!(prefix.operator, "-");
            if let Expression::Integer(i) = prefix.right.as_ref() {
                assert_eq!(i.value, 5);
            }
        } else {
            panic!("Expected Prefix expression");
        }
    }
}

#[test]
fn test_prefix_not() {
    let program = parse("!true");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Prefix(prefix) = &expr_stmt.expression {
            assert_eq!(prefix.operator, "!");
        }
    }
}

// =====================================================================
// If/Else Expressions
// =====================================================================

#[test]
fn test_if_expression() {
    let program = parse("if x > 5 { 10 }");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::If(if_expr) = &expr_stmt.expression {
            assert!(if_expr.alternative.is_none());
            assert_eq!(if_expr.consequence.statements.len(), 1);
        } else {
            panic!("Expected If expression");
        }
    }
}

#[test]
fn test_if_else_expression() {
    let program = parse("if x > 5 { 10 } else { 20 }");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::If(if_expr) = &expr_stmt.expression {
            assert!(if_expr.alternative.is_some());
        } else {
            panic!("Expected If expression");
        }
    }
}

// =====================================================================
// Function Literals (H-01)
// =====================================================================

#[test]
fn test_function_definition() {
    let program = parse("fn add(a, b) { a + b }");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Function(func) = &expr_stmt.expression {
            assert_eq!(func.name.as_ref().map(|n| n.value.as_str()), Some("add"));
            assert_eq!(func.parameters.len(), 2);
            assert_eq!(func.parameters[0].value, "a");
            assert_eq!(func.parameters[1].value, "b");
        } else {
            panic!("Expected Function expression");
        }
    }
}

#[test]
fn test_function_no_params() {
    let program = parse("fn hello() { 42 }");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Function(func) = &expr_stmt.expression {
            assert_eq!(func.parameters.len(), 0);
        }
    }
}

// =====================================================================
// While Loop
// =====================================================================

#[test]
fn test_while_loop() {
    let program = parse("while x > 0 { x }");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::While(while_expr) = &expr_stmt.expression {
            assert_eq!(while_expr.body.statements.len(), 1);
        } else {
            panic!("Expected While expression");
        }
    }
}

// =====================================================================
// For Loop
// =====================================================================

#[test]
fn test_for_loop() {
    let program = parse("for i in 0..10 { i }");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::For(for_expr) = &expr_stmt.expression {
            assert_eq!(for_expr.variable.value, "i");
        } else {
            panic!("Expected For expression");
        }
    }
}

// =====================================================================
// Break / Continue (H-05)
// =====================================================================

#[test]
fn test_break_expression() {
    let program = parse("break");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        assert!(matches!(expr_stmt.expression, Expression::Break));
    }
}

#[test]
fn test_continue_expression() {
    let program = parse("continue");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        assert!(matches!(expr_stmt.expression, Expression::Continue));
    }
}

// =====================================================================
// Assignment (H-06)
// =====================================================================

#[test]
fn test_assignment() {
    let program = parse("x = 42");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Assignment(assign) = &expr_stmt.expression {
            if let Expression::Identifier(id) = &*assign.target {
                assert_eq!(id.value, "x");
            } else {
                panic!("Expected Identifier as assignment target");
            }
            if let Expression::Integer(i) = &*assign.value {
                assert_eq!(i.value, 42);
            }
        } else {
            panic!("Expected Assignment expression");
        }
    }
}

// =====================================================================
// Array & Index
// =====================================================================

#[test]
fn test_array_literal() {
    let program = parse("[1, 2, 3]");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Array(arr) = &expr_stmt.expression {
            assert_eq!(arr.elements.len(), 3);
        } else {
            panic!("Expected Array expression");
        }
    }
}

#[test]
fn test_index_expression() {
    let program = parse("arr[0]");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        assert!(matches!(expr_stmt.expression, Expression::Index(_)));
    }
}

// =====================================================================
// Multiple Statements
// =====================================================================

#[test]
fn test_multiple_statements() {
    let program = parse("let x = 10;\nlet y = 20;\nx + y");
    assert_eq!(program.statements.len(), 3);
}

// =====================================================================
// Edge Cases
// =====================================================================

#[test]
fn test_nested_if() {
    let program = parse("if a > 0 { if b > 0 { 1 } else { 2 } } else { 3 }");
    // Should parse without errors
    assert_eq!(program.statements.len(), 1);
}

#[test]
fn test_function_call() {
    let program = parse("add(1, 2)");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Call(call) = &expr_stmt.expression {
            assert_eq!(call.arguments.len(), 2);
        } else {
            panic!("Expected Call expression");
        }
    }
}

// =====================================================================
// Grouped / Parenthesized Expressions
// =====================================================================

#[test]
fn test_grouped_expression() {
    // (1 + 2) * 3 should parse as (* (+ 1 2) 3)
    let program = parse("(1 + 2) * 3");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Infix(infix) = &expr_stmt.expression {
            assert_eq!(infix.operator, "*");
            // Left side should be the grouped (1 + 2)
            if let Expression::Infix(left) = infix.left.as_ref() {
                assert_eq!(left.operator, "+");
            } else {
                panic!("Left side should be infix (1 + 2)");
            }
        } else {
            panic!("Expected Infix expression");
        }
    }
}

#[test]
fn test_nested_grouped_expression() {
    let program = parse("((1 + 2))");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Infix(infix) = &expr_stmt.expression {
            assert_eq!(infix.operator, "+");
        } else {
            panic!("Expected Infix expression inside nested parens");
        }
    }
}

// =====================================================================
// Range Expressions
// =====================================================================

#[test]
fn test_range_expression() {
    let program = parse("0..10");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Range(range) = &expr_stmt.expression {
            if let Expression::Integer(start) = range.start.as_ref() {
                assert_eq!(start.value, 0);
            } else {
                panic!("Expected Integer start");
            }
            if let Expression::Integer(end) = range.end.as_ref() {
                assert_eq!(end.value, 10);
            } else {
                panic!("Expected Integer end");
            }
        } else {
            panic!("Expected Range expression");
        }
    }
}

// =====================================================================
// Chained Arithmetic
// =====================================================================

#[test]
fn test_chained_arithmetic() {
    // 1 + 2 + 3 should left-associate: ((1 + 2) + 3)
    let program = parse("1 + 2 + 3");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Infix(outer) = &expr_stmt.expression {
            assert_eq!(outer.operator, "+");
            // Left should be (1 + 2)
            if let Expression::Infix(inner) = outer.left.as_ref() {
                assert_eq!(inner.operator, "+");
            } else {
                panic!("Expected left-associative chaining");
            }
            // Right should be 3
            if let Expression::Integer(r) = outer.right.as_ref() {
                assert_eq!(r.value, 3);
            } else {
                panic!("Expected Integer on right");
            }
        } else {
            panic!("Expected Infix expression");
        }
    }
}

#[test]
fn test_mixed_arithmetic_precedence() {
    // 1 * 2 + 3 should parse as ((1 * 2) + 3)
    let program = parse("1 * 2 + 3");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Infix(outer) = &expr_stmt.expression {
            assert_eq!(outer.operator, "+");
            if let Expression::Infix(left) = outer.left.as_ref() {
                assert_eq!(left.operator, "*");
            } else {
                panic!("Expected multiplication on left");
            }
        } else {
            panic!("Expected Infix expression");
        }
    }
}

// =====================================================================
// Nested Function Calls
// =====================================================================

#[test]
fn test_nested_function_call() {
    let program = parse("add(mul(1, 2), 3)");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Call(call) = &expr_stmt.expression {
            assert_eq!(call.arguments.len(), 2);
            // First arg should be a Call(mul)
            assert!(matches!(&call.arguments[0], Expression::Call(_)));
        } else {
            panic!("Expected Call expression");
        }
    }
}

#[test]
fn test_function_call_no_args() {
    let program = parse("foo()");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Call(call) = &expr_stmt.expression {
            assert_eq!(call.arguments.len(), 0);
        } else {
            panic!("Expected Call expression");
        }
    }
}

// =====================================================================
// Empty Array
// =====================================================================

#[test]
fn test_empty_array() {
    let program = parse("[]");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Array(arr) = &expr_stmt.expression {
            assert_eq!(arr.elements.len(), 0);
        } else {
            panic!("Expected empty Array expression");
        }
    }
}

// =====================================================================
// Index with Complex Expressions
// =====================================================================

#[test]
fn test_index_with_arithmetic() {
    let program = parse("arr[1 + 2]");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Index(idx) = &expr_stmt.expression {
            // left should be identifier "arr"
            assert!(matches!(idx.left.as_ref(), Expression::Identifier(_)));
            // index should be infix (1 + 2)
            assert!(matches!(idx.index.as_ref(), Expression::Infix(_)));
        } else {
            panic!("Expected Index expression");
        }
    }
}

// =====================================================================
// String in Expressions
// =====================================================================

#[test]
fn test_string_expression_standalone() {
    let program = parse("\"hello world\"");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::String(s) = &expr_stmt.expression {
            assert_eq!(s.value, "hello world");
        } else {
            panic!("Expected String expression");
        }
    }
}

#[test]
fn test_string_concatenation_parse() {
    let program = parse("\"hello\" + \"world\"");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Infix(infix) = &expr_stmt.expression {
            assert_eq!(infix.operator, "+");
            assert!(matches!(infix.left.as_ref(), Expression::String(_)));
            assert!(matches!(infix.right.as_ref(), Expression::String(_)));
        } else {
            panic!("Expected Infix expression for string concat");
        }
    }
}

// =====================================================================
// Complex Nested Expressions
// =====================================================================

#[test]
fn test_deeply_nested_if_else() {
    let program = parse("if a > 0 { if b > 0 { if c > 0 { 1 } else { 2 } } else { 3 } } else { 4 }");
    assert_eq!(program.statements.len(), 1);
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        assert!(matches!(&expr_stmt.expression, Expression::If(_)));
    } else {
        panic!("Expected expression statement");
    }
}

#[test]
fn test_while_with_complex_condition() {
    let program = parse("while x > 0 { x }");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::While(w) = &expr_stmt.expression {
            // Condition should be an infix (x > 0)
            assert!(matches!(w.condition.as_ref(), Expression::Infix(_)));
        } else {
            panic!("Expected While expression");
        }
    }
}

#[test]
fn test_prefix_chain() {
    // !!true should parse as !(!(true))
    let program = parse("!!true");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Prefix(outer) = &expr_stmt.expression {
            assert_eq!(outer.operator, "!");
            if let Expression::Prefix(inner) = outer.right.as_ref() {
                assert_eq!(inner.operator, "!");
                assert!(matches!(inner.right.as_ref(), Expression::Boolean(_)));
            } else {
                panic!("Expected nested Prefix");
            }
        } else {
            panic!("Expected Prefix expression");
        }
    }
}

#[test]
fn test_negative_in_arithmetic() {
    // -5 + 3 should parse as ((-5) + 3)
    let program = parse("-5 + 3");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Infix(infix) = &expr_stmt.expression {
            assert_eq!(infix.operator, "+");
            assert!(matches!(infix.left.as_ref(), Expression::Prefix(_)));
        } else {
            panic!("Expected Infix expression");
        }
    }
}

// =====================================================================
// Parser Error Cases
// =====================================================================

#[test]
fn test_error_missing_closing_paren() {
    let errors = parse_with_errors("(1 + 2");
    assert!(!errors.is_empty(), "Expected parser errors for missing ')'");
}

#[test]
fn test_error_missing_let_identifier() {
    let errors = parse_with_errors("let = 42;");
    assert!(!errors.is_empty(), "Expected parser errors for missing identifier");
}

#[test]
fn test_error_missing_assign_in_let() {
    let errors = parse_with_errors("let x 42;");
    assert!(!errors.is_empty(), "Expected parser errors for missing '='");
}
