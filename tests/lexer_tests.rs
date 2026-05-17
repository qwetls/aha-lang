// tests/lexer_tests.rs
//
// Unit tests for the AHA! Lexer module

use aha_lang::lexer::Lexer;
use aha_lang::ast::TokenType;

/// Helper: collect all tokens from input until EOF
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

// =====================================================================
// Basic Tokens
// =====================================================================

#[test]
fn test_single_char_operators() {
    let tokens = tokenize("+ - * / = < > !");
    assert_eq!(tokens[0], (TokenType::Plus, "+".to_string()));
    assert_eq!(tokens[1], (TokenType::Minus, "-".to_string()));
    assert_eq!(tokens[2], (TokenType::Asterisk, "*".to_string()));
    assert_eq!(tokens[3], (TokenType::Slash, "/".to_string()));
    assert_eq!(tokens[4], (TokenType::Assign, "=".to_string()));
    assert_eq!(tokens[5], (TokenType::LT, "<".to_string()));
    assert_eq!(tokens[6], (TokenType::GT, ">".to_string()));
    assert_eq!(tokens[7], (TokenType::Bang, "!".to_string()));
}

#[test]
fn test_two_char_operators() {
    let tokens = tokenize("== != <= >= ..");
    assert_eq!(tokens[0], (TokenType::Eq, "==".to_string()));
    assert_eq!(tokens[1], (TokenType::NotEq, "!=".to_string()));
    assert_eq!(tokens[2], (TokenType::LtEq, "<=".to_string()));
    assert_eq!(tokens[3], (TokenType::GtEq, ">=".to_string()));
    assert_eq!(tokens[4], (TokenType::DotDot, "..".to_string()));
}

#[test]
fn test_not_eq_c01_fix() {
    // C-01: != must produce "!=" literal, not "=="
    let tokens = tokenize("!=");
    assert_eq!(tokens[0].0, TokenType::NotEq);
    assert_eq!(tokens[0].1, "!=");
}

#[test]
fn test_delimiters() {
    let tokens = tokenize("( ) { } [ ] , ; :");
    assert_eq!(tokens[0].0, TokenType::LeftParen);
    assert_eq!(tokens[1].0, TokenType::RightParen);
    assert_eq!(tokens[2].0, TokenType::LeftBrace);
    assert_eq!(tokens[3].0, TokenType::RightBrace);
    assert_eq!(tokens[4].0, TokenType::LeftBracket);
    assert_eq!(tokens[5].0, TokenType::RightBracket);
    assert_eq!(tokens[6].0, TokenType::Comma);
    assert_eq!(tokens[7].0, TokenType::Semicolon);
    assert_eq!(tokens[8].0, TokenType::Colon);
}

// =====================================================================
// Keywords
// =====================================================================

#[test]
fn test_keywords() {
    let tokens = tokenize("let fn if else return while for in true false break continue struct");
    let expected = vec![
        TokenType::Let, TokenType::Fn, TokenType::If, TokenType::Else,
        TokenType::Return, TokenType::While, TokenType::For, TokenType::In,
        TokenType::True, TokenType::False, TokenType::Break, TokenType::Continue,
        TokenType::Struct,
    ];
    for (i, expected_type) in expected.iter().enumerate() {
        assert_eq!(tokens[i].0, *expected_type, "Keyword mismatch at index {}", i);
    }
}

// =====================================================================
// Identifiers (M-01: digits in identifiers)
// =====================================================================

#[test]
fn test_identifier_simple() {
    let tokens = tokenize("foo bar baz");
    assert_eq!(tokens[0], (TokenType::Identifier, "foo".to_string()));
    assert_eq!(tokens[1], (TokenType::Identifier, "bar".to_string()));
    assert_eq!(tokens[2], (TokenType::Identifier, "baz".to_string()));
}

#[test]
fn test_identifier_with_digits() {
    // M-01: Identifiers can contain digits (not at start)
    let tokens = tokenize("my_var2 x1 count99");
    assert_eq!(tokens[0], (TokenType::Identifier, "my_var2".to_string()));
    assert_eq!(tokens[1], (TokenType::Identifier, "x1".to_string()));
    assert_eq!(tokens[2], (TokenType::Identifier, "count99".to_string()));
}

#[test]
fn test_identifier_with_underscore_prefix() {
    let tokens = tokenize("_private _x _123");
    assert_eq!(tokens[0], (TokenType::Identifier, "_private".to_string()));
    assert_eq!(tokens[1], (TokenType::Identifier, "_x".to_string()));
    assert_eq!(tokens[2], (TokenType::Identifier, "_123".to_string()));
}

// =====================================================================
// Integer Literals
// =====================================================================

#[test]
fn test_integers() {
    let tokens = tokenize("0 42 12345");
    assert_eq!(tokens[0], (TokenType::Integer, "0".to_string()));
    assert_eq!(tokens[1], (TokenType::Integer, "42".to_string()));
    assert_eq!(tokens[2], (TokenType::Integer, "12345".to_string()));
}

// =====================================================================
// String Literals (M-02: escape sequences)
// =====================================================================

#[test]
fn test_string_simple() {
    let tokens = tokenize("\"hello world\"");
    assert_eq!(tokens[0].0, TokenType::String);
    assert_eq!(tokens[0].1, "hello world");
}

#[test]
fn test_string_escape_sequences() {
    // M-02: escape sequences \n, \t, \\, \"
    let tokens = tokenize(r#""line1\nline2""#);
    assert_eq!(tokens[0].1, "line1\nline2");

    let tokens = tokenize(r#""tab\there""#);
    assert_eq!(tokens[0].1, "tab\there");

    let tokens = tokenize(r#""back\\slash""#);
    assert_eq!(tokens[0].1, "back\\slash");

    let tokens = tokenize(r#""say \"hi\"""#);
    assert_eq!(tokens[0].1, "say \"hi\"");
}

// =====================================================================
// Comments
// =====================================================================

#[test]
fn test_single_line_comment() {
    let tokens = tokenize("42 // this is a comment\n99");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0], (TokenType::Integer, "42".to_string()));
    assert_eq!(tokens[1], (TokenType::Integer, "99".to_string()));
}

#[test]
fn test_multi_line_comment() {
    // M-03: Block comments /* ... */
    let tokens = tokenize("42 /* this is\na block\ncomment */ 99");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0], (TokenType::Integer, "42".to_string()));
    assert_eq!(tokens[1], (TokenType::Integer, "99".to_string()));
}

// =====================================================================
// Complete Expression Tokenization
// =====================================================================

#[test]
fn test_let_statement_tokens() {
    let tokens = tokenize("let x = 42;");
    assert_eq!(tokens[0].0, TokenType::Let);
    assert_eq!(tokens[1], (TokenType::Identifier, "x".to_string()));
    assert_eq!(tokens[2].0, TokenType::Assign);
    assert_eq!(tokens[3], (TokenType::Integer, "42".to_string()));
    assert_eq!(tokens[4].0, TokenType::Semicolon);
}

#[test]
fn test_function_tokens() {
    let tokens = tokenize("fn add(a, b) { a + b }");
    assert_eq!(tokens[0].0, TokenType::Fn);
    assert_eq!(tokens[1], (TokenType::Identifier, "add".to_string()));
    assert_eq!(tokens[2].0, TokenType::LeftParen);
    assert_eq!(tokens[3], (TokenType::Identifier, "a".to_string()));
    assert_eq!(tokens[4].0, TokenType::Comma);
    assert_eq!(tokens[5], (TokenType::Identifier, "b".to_string()));
    assert_eq!(tokens[6].0, TokenType::RightParen);
}

#[test]
fn test_for_loop_tokens() {
    let tokens = tokenize("for i in 0..10 { }");
    assert_eq!(tokens[0].0, TokenType::For);
    assert_eq!(tokens[1], (TokenType::Identifier, "i".to_string()));
    assert_eq!(tokens[2].0, TokenType::In);
    assert_eq!(tokens[3], (TokenType::Integer, "0".to_string()));
    assert_eq!(tokens[4].0, TokenType::DotDot);
    assert_eq!(tokens[5], (TokenType::Integer, "10".to_string()));
}

#[test]
fn test_empty_input() {
    let tokens = tokenize("");
    assert_eq!(tokens.len(), 0);
}

#[test]
fn test_eof_token() {
    let mut lexer = Lexer::new("42".to_string());
    let tok1 = lexer.next_token();
    assert_eq!(tok1.kind, TokenType::Integer);
    let tok2 = lexer.next_token();
    assert_eq!(tok2.kind, TokenType::Eof);
}
