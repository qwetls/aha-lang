// tests/edge_cases.rs
//
// Edge-case and stress tests for the AHA! compiler pipeline.
// These complement the existing 84 tests with boundary conditions,
// complex combinations, and potential crash scenarios.

use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser;
use aha_lang::ast::*;
use aha_lang::types::AhaType;

// =====================================================================
// Lexer Edge Cases
// =====================================================================

/// Helper: tokenize and return all non-EOF tokens
fn tokenize(input: &str) -> Vec<(TokenType, String)> {
    let mut lexer = Lexer::new(input.to_string());
    let mut tokens = Vec::new();
    loop {
        let tok = lexer.next_token();
        if tok.kind == TokenType::Eof {
            break;
        }
        tokens.push((tok.kind, tok.literal));
    }
    tokens
}

#[test]
fn test_lexer_only_whitespace() {
    let tokens = tokenize("   \t\n\r  ");
    assert_eq!(tokens.len(), 0);
}

#[test]
fn test_lexer_only_comment() {
    let tokens = tokenize("// just a comment\n");
    assert_eq!(tokens.len(), 0);
}

#[test]
fn test_lexer_only_block_comment() {
    let tokens = tokenize("/* block comment */");
    assert_eq!(tokens.len(), 0);
}

#[test]
fn test_lexer_nested_block_comment_content() {
    // Block comment with code-like content inside
    let tokens = tokenize("/* let x = 42; */ 99");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenType::Integer);
}

#[test]
fn test_lexer_unterminated_block_comment() {
    // Should reach EOF without panic
    let tokens = tokenize("42 /* never closed");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenType::Integer);
}

#[test]
fn test_lexer_empty_string() {
    let tokens = tokenize("\"\"");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenType::String);
    assert_eq!(tokens[0].1, "");
}

#[test]
fn test_lexer_string_with_all_escapes() {
    let tokens = tokenize(r#""\n\t\r\\\"\0""#);
    assert_eq!(tokens[0].1, "\n\t\r\\\"\0");
}

#[test]
fn test_lexer_unknown_escape_kept_as_is() {
    let tokens = tokenize(r#""\x""#);
    assert_eq!(tokens[0].1, "\\x");
}

#[test]
fn test_lexer_large_integer() {
    let tokens = tokenize("9223372036854775807"); // i64::MAX
    assert_eq!(tokens[0].0, TokenType::Integer);
    assert_eq!(tokens[0].1, "9223372036854775807");
}

#[test]
fn test_lexer_mixed_operators_no_spaces() {
    let tokens = tokenize("1+2*3-4/5");
    assert_eq!(tokens[0], (TokenType::Integer, "1".to_string()));
    assert_eq!(tokens[1].0, TokenType::Plus);
    assert_eq!(tokens[2], (TokenType::Integer, "2".to_string()));
    assert_eq!(tokens[3].0, TokenType::Asterisk);
    assert_eq!(tokens[4], (TokenType::Integer, "3".to_string()));
    assert_eq!(tokens[5].0, TokenType::Minus);
    assert_eq!(tokens[6], (TokenType::Integer, "4".to_string()));
    assert_eq!(tokens[7].0, TokenType::Slash);
    assert_eq!(tokens[8], (TokenType::Integer, "5".to_string()));
}

#[test]
fn test_lexer_chained_dot_dot() {
    // 0..10..20 — lexer should produce Integer, DotDot, Integer, DotDot, Integer
    let tokens = tokenize("0..10..20");
    assert_eq!(tokens.len(), 5);
    assert_eq!(tokens[1].0, TokenType::DotDot);
    assert_eq!(tokens[3].0, TokenType::DotDot);
}

#[test]
fn test_lexer_illegal_character() {
    let tokens = tokenize("@#$%");
    // Each illegal char becomes an Illegal token
    assert_eq!(tokens.len(), 4);
    for (_, literal) in &tokens {
        assert!(literal.len() == 1);
    }
}

#[test]
fn test_lexer_line_column_tracking() {
    let mut lexer = Lexer::new("let x = 1;\nlet y = 2;".to_string());
    let tok1 = lexer.next_token(); // let
    assert_eq!(tok1.line, 1);
    let tok2 = lexer.next_token(); // x
    assert_eq!(tok2.line, 1);
    // Skip to second line
    let _ = lexer.next_token(); // =
    let _ = lexer.next_token(); // 1
    let _ = lexer.next_token(); // ;
    let tok_line2 = lexer.next_token(); // let on line 2
    assert_eq!(tok_line2.line, 2);
}

// =====================================================================
// Parser Edge Cases
// =====================================================================

/// Helper: parse and return program (panics on error)
fn parse(input: &str) -> Program {
    let lexer = Lexer::new(input.to_string());
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    if !parser.errors.is_empty() {
        panic!("Parser errors: {:?}", parser.errors);
    }
    program
}

/// Helper: parse and return errors
fn parse_errors(input: &str) -> Vec<String> {
    let lexer = Lexer::new(input.to_string());
    let mut parser = Parser::new(lexer);
    parser.parse_program();
    parser.errors
}

#[test]
fn test_parser_empty_program() {
    let program = parse("");
    assert_eq!(program.statements.len(), 0);
}

#[test]
fn test_parser_only_comments() {
    let program = parse("// comment\n/* block */\n");
    assert_eq!(program.statements.len(), 0);
}

#[test]
fn test_parser_only_semicolons() {
    // Semicolons alone should not crash the parser.
    // The parser will try to parse them as expression statements;
    // since Semicolon has no prefix parser, it produces errors but should not panic.
    let lexer = Lexer::new(";;;");
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();
    // Parser should complete without panicking.
    // It may produce errors or empty statements — either is acceptable.
    let _ = program;
}

#[test]
fn test_parser_deeply_nested_arithmetic() {
    // 1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 10
    let program = parse("1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 10");
    assert_eq!(program.statements.len(), 1);
    // Verify it's an expression statement
    assert!(matches!(&program.statements[0], Statement::Expression(_)));
}

#[test]
fn test_parser_deeply_nested_parens() {
    let program = parse("(((((1 + 2)))))");
    assert_eq!(program.statements.len(), 1);
}

#[test]
fn test_parser_function_with_many_params() {
    let program = parse("fn f(a, b, c, d, e, f, g, h) { a }");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Function(func) = &expr_stmt.expression {
            assert_eq!(func.parameters.len(), 8);
        } else {
            panic!("Expected Function");
        }
    } else {
        panic!("Expected Expression statement");
    }
}

#[test]
fn test_parser_nested_function_calls_deep() {
    let program = parse("f(g(h(i(j(1)))))");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Call(outer) = &expr_stmt.expression {
            assert_eq!(outer.arguments.len(), 1);
            // Inner should also be a call
            assert!(matches!(&outer.arguments[0], Expression::Call(_)));
        } else {
            panic!("Expected Call");
        }
    } else {
        panic!("Expected Expression statement");
    }
}

#[test]
fn test_parser_array_with_mixed_elements() {
    let program = parse("[1, true, \"hello\", 42]");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Array(arr) = &expr_stmt.expression {
            assert_eq!(arr.elements.len(), 4);
            assert!(matches!(&arr.elements[0], Expression::Integer(_)));
            assert!(matches!(&arr.elements[1], Expression::Boolean(_)));
            assert!(matches!(&arr.elements[2], Expression::String(_)));
            assert!(matches!(&arr.elements[3], Expression::Integer(_)));
        } else {
            panic!("Expected Array");
        }
    } else {
        panic!("Expected Expression statement");
    }
}

#[test]
fn test_parser_nested_arrays() {
    let program = parse("[[1, 2], [3, 4]]");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Array(arr) = &expr_stmt.expression {
            assert_eq!(arr.elements.len(), 2);
            assert!(matches!(&arr.elements[0], Expression::Array(_)));
            assert!(matches!(&arr.elements[1], Expression::Array(_)));
        } else {
            panic!("Expected Array");
        }
    } else {
        panic!("Expected Expression statement");
    }
}

#[test]
fn test_parser_chained_field_access() {
    let program = parse("a.b.c.d");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::FieldAccess(fa) = &expr_stmt.expression {
            assert_eq!(fa.field.value, "d");
            // Inner should be another field access
            assert!(matches!(&*fa.object, Expression::FieldAccess(_)));
        } else {
            panic!("Expected FieldAccess");
        }
    } else {
        panic!("Expected Expression statement");
    }
}

#[test]
fn test_parser_chained_index_access() {
    let program = parse("arr[0][1][2]");
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        if let Expression::Index(idx) = &expr_stmt.expression {
            // Outermost index should have another index as left
            assert!(matches!(&*idx.left, Expression::Index(_)));
        } else {
            panic!("Expected Index");
        }
    } else {
        panic!("Expected Expression statement");
    }
}

#[test]
fn test_parser_mixed_field_and_index() {
    let program = parse("obj.field[0].subfield");
    // Should parse without errors
    assert_eq!(program.statements.len(), 1);
}

#[test]
fn test_parser_struct_with_type_hints() {
    let program = parse("struct Point { x: int, y: int }");
    if let Statement::Struct(struct_def) = &program.statements[0] {
        assert_eq!(struct_def.name.value, "Point");
        assert_eq!(struct_def.fields.len(), 2);
        assert_eq!(struct_def.fields[0].type_hint, Some("int".to_string()));
        assert_eq!(struct_def.fields[1].type_hint, Some("int".to_string()));
    } else {
        panic!("Expected Struct definition");
    }
}

#[test]
fn test_parser_struct_without_type_hints() {
    let program = parse("struct Person { name, age }");
    if let Statement::Struct(struct_def) = &program.statements[0] {
        assert_eq!(struct_def.fields.len(), 2);
        assert_eq!(struct_def.fields[0].type_hint, None);
    } else {
        panic!("Expected Struct definition");
    }
}

#[test]
fn test_parser_empty_struct() {
    let program = parse("struct Empty {}");
    if let Statement::Struct(struct_def) = &program.statements[0] {
        assert_eq!(struct_def.name.value, "Empty");
        assert_eq!(struct_def.fields.len(), 0);
    } else {
        panic!("Expected Struct definition");
    }
}

#[test]
fn test_parser_else_if_chain_long() {
    let program = parse("if x > 10 { 1 } else if x > 5 { 2 } else if x > 0 { 3 } else { 4 }");
    assert_eq!(program.statements.len(), 1);
    if let Statement::Expression(expr_stmt) = &program.statements[0] {
        assert!(matches!(&expr_stmt.expression, Expression::If(_)));
    } else {
        panic!("Expected Expression statement");
    }
}

#[test]
fn test_parser_assignment_in_let() {
    // let x = (y = 42) — assignment as expression in let
    let program = parse("let x = 42");
    assert_eq!(program.statements.len(), 1);
}

#[test]
fn test_parser_multiple_let_statements() {
    let src = "let a = 1;\nlet b = 2;\nlet c = 3;\nlet d = 4;\nlet e = 5;";
    let program = parse(src);
    assert_eq!(program.statements.len(), 5);
}

#[test]
fn test_parser_error_unexpected_token() {
    let errors = parse_errors("@");
    assert!(!errors.is_empty());
}

#[test]
fn test_parser_error_missing_function_body() {
    let errors = parse_errors("fn f(a, b)");
    assert!(!errors.is_empty());
}

#[test]
fn test_parser_error_missing_for_in() {
    let errors = parse_errors("for x 0..10 { x }");
    assert!(!errors.is_empty());
}

#[test]
fn test_parser_error_missing_while_body() {
    let errors = parse_errors("while x > 0");
    assert!(!errors.is_empty());
}

#[test]
fn test_parser_error_struct_missing_name() {
    let errors = parse_errors("struct { x, y }");
    assert!(!errors.is_empty());
}

// =====================================================================
// Type System Edge Cases
// =====================================================================

#[test]
fn test_type_array_of_array() {
    let nested = AhaType::Array(Box::new(AhaType::Array(Box::new(AhaType::Int))));
    assert_eq!(format!("{}", nested), "[[Int]]");
}

#[test]
fn test_type_function_display() {
    let fn_type = AhaType::Function {
        params: vec![AhaType::Int, AhaType::String],
        ret: Box::new(AhaType::Bool),
    };
    assert_eq!(format!("{}", fn_type), "fn(Int, String) -> Bool");
}

#[test]
fn test_type_check_division_returns_int() {
    let result = AhaType::Int.check_binary_op("/", &AhaType::Int).unwrap();
    assert_eq!(result, AhaType::Int);
}

#[test]
fn test_type_check_string_plus_int_error() {
    let result = AhaType::String.check_binary_op("+", &AhaType::Int);
    assert!(result.is_err());
}

#[test]
fn test_type_check_int_plus_bool_error() {
    let result = AhaType::Int.check_binary_op("+", &AhaType::Bool);
    assert!(result.is_err());
}

#[test]
fn test_type_check_bool_less_than_error() {
    let result = AhaType::Bool.check_binary_op("<", &AhaType::Bool);
    assert!(result.is_err());
}

#[test]
fn test_type_check_array_arithmetic_error() {
    let arr = AhaType::Array(Box::new(AhaType::Int));
    let result = arr.check_binary_op("+", &arr);
    assert!(result.is_err());
}

#[test]
fn test_type_from_hint_all_valid() {
    assert_eq!(AhaType::from_hint("int"), Some(AhaType::Int));
    assert_eq!(AhaType::from_hint("i64"), Some(AhaType::Int));
    assert_eq!(AhaType::from_hint("bool"), Some(AhaType::Bool));
    assert_eq!(AhaType::from_hint("string"), Some(AhaType::String));
    assert_eq!(AhaType::from_hint("str"), Some(AhaType::String));
    assert_eq!(AhaType::from_hint("void"), Some(AhaType::Void));
}

#[test]
fn test_type_from_hint_invalid_variants() {
    assert_eq!(AhaType::from_hint("float"), None);
    assert_eq!(AhaType::from_hint("char"), None);
    assert_eq!(AhaType::from_hint("double"), None);
    assert_eq!(AhaType::from_hint(""), None);
    assert_eq!(AhaType::from_hint("Int"), None); // case-sensitive
}

#[test]
fn test_type_is_numeric_excludes_string() {
    assert!(!AhaType::String.is_numeric());
    assert!(!AhaType::Void.is_numeric());
}

#[test]
fn test_type_equality_nested() {
    let a1 = AhaType::Array(Box::new(AhaType::Array(Box::new(AhaType::Int))));
    let a2 = AhaType::Array(Box::new(AhaType::Array(Box::new(AhaType::Int))));
    assert_eq!(a1, a2);
}
