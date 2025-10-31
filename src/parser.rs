// src/parser.rs

use crate::Lexer;
use crate::ast;
use crate::ast::{Program, Statement, Expression, Identifier, IntegerLiteral, BooleanLiteral, PrefixExpression, InfixExpression, LetStatement, ReturnStatement, ExpressionStatement, BlockStatement, FunctionLiteral, CallExpression};
use crate::ast::Token;
use crate::ast::TokenType;

pub struct Parser {
    lexer: Lexer,
    current_token: Token,
    peek_token: Token,
    pub errors: Vec<String>, // PASTIKAN INI PUBLIK
}

// Definisi presedensi operator
#[derive(Debug, PartialEq, PartialOrd)]
pub enum Precedence {
    Lowest,
    Equals,      // == or !=
    LessGreater, // > or <
    Sum,         // +
    Product,     // *
    Prefix,      // -X or !X
    Call,        // myFunction(X)
}

impl Parser {
    pub fn new(mut lexer: Lexer) -> Self {
        let current_token = lexer.next_token();
        let peek_token = lexer.next_token();

        Parser {
            lexer,
            current_token,
            peek_token,
            errors: Vec::new(),
        }
    }

    // Fungsi utama untuk mem-parse seluruh program
    pub fn parse_program(&mut self) -> Program {
        let mut program = Program { statements: Vec::new() };

        while self.current_token.r#type != TokenType::Eof {
            if let Some(stmt) = self.parse_statement() {
                program.statements.push(stmt);
            }
            self.next_token();
        }

        program
    }

    // --- Helper Functions ---
    fn next_token(&mut self) {
        self.current_token = self.peek_token.clone();
        self.peek_token = self.lexer.next_token();
    }
    
    fn current_token_is(&self, t: TokenType) -> bool {
        self.current_token.r#type == t
    }

    fn peek_token_is(&self, t: TokenType) -> bool {
        self.peek_token.r#type == t
    }

    fn expect_peek(&mut self, t: TokenType) -> bool {
        if self.peek_token_is(t) {
            self.next_token();
            true
        } else {
            self.peek_error(t);
            false
        }
    }

    // --- Parsing Functions ---
    fn parse_statement(&mut self) -> Option<Statement> {
        match self.current_token.r#type.clone() {
            TokenType::Let => self.parse_let_statement(),
            TokenType::Return => self.parse_return_statement(),
            _ => self.parse_expression_statement(),
        }
    }

    fn parse_let_statement(&mut self) -> Option<Statement> {
        self.next_token(); // Lewati 'let'

        if !self.current_token_is(TokenType::Identifier) {
            self.errors.push(format!("expected next token to be Identifier, got {:?} instead", self.current_token.r#type));
            return None;
        }
        let name = Identifier { value: self.current_token.literal.clone() };

        if !self.expect_peek(TokenType::Assign) {
            return None;
        }

        self.next_token(); // Lewati '='
        let value = self.parse_expression(Precedence::Lowest);

        if self.peek_token_is(TokenType::Semicolon) {
            self.next_token(); // Lewati ';'
        }

        Some(Statement::Let(LetStatement { name, value }))
    }

    fn parse_return_statement(&mut self) -> Option<Statement> {
        self.next_token(); // Lewati 'return'
        let return_value = self.parse_expression(Precedence::Lowest);

        if self.peek_token_is(TokenType::Semicolon) {
            self.next_token(); // Lewati ';'
        }

        Some(Statement::Return(ReturnStatement { return_value }))
    }

    fn parse_expression_statement(&mut self) -> Option<Statement> {
        let expression = self.parse_expression(Precedence::Lowest);
        
        if self.peek_token_is(TokenType::Semicolon) {
            self.next_token();
        }

        Some(Statement::Expression(ExpressionStatement { expression }))
    }

    // --- Parsing Expressions (Versi Diperbaiki) ---
    pub fn parse_expression(&mut self, precedence: Precedence) -> Expression {
        let mut left = self.parse_prefix();

        while !self.peek_token_is(TokenType::Semicolon) && precedence < self.peek_precedence() {
            if self.peek_token_is(TokenType::LeftParen) {
                left = self.parse_call_expression(left);
            } else {
                self.next_token(); // Ambil operator
                let operator = self.current_token.literal.clone();
                let right_precedence = self.current_precedence();
                self.next_token(); // Pindah ke ekspresi di sebelah kanan
                let right = Box::new(self.parse_expression(right_precedence));

                left = Expression::Infix(InfixExpression {
                    left: Box::new(left),
                    operator,
                    right,
                });
            }
        }

        left
    }
    
    fn parse_prefix(&mut self) -> Expression {
        match self.current_token.r#type.clone() {
            TokenType::Identifier => Expression::Identifier(Identifier { value: self.current_token.literal.clone() }),
            TokenType::Integer => Expression::Integer(IntegerLiteral { value: self.current_token.literal.parse().unwrap() }),
            TokenType::True => Expression::Boolean(BooleanLiteral { value: true }),
            TokenType::False => Expression::Boolean(BooleanLiteral { value: false }),
            TokenType::If => self.parse_if_expression(),
            TokenType::Bang | TokenType::Minus => {
                let operator = self.current_token.literal.clone();
                self.next_token();
                let right = Box::new(self.parse_expression(Precedence::Prefix));
                Expression::Prefix(PrefixExpression { operator, right })
            },
            TokenType::LeftParen => {
                self.next_token();
                let exp = self.parse_expression(Precedence::Lowest);
                if !self.expect_peek(TokenType::RightParen) {
                    return self.error("expected ')'");
                }
                exp
            }
            TokenType::Fn => self.parse_function_literal(),
            _ => {
                self.no_prefix_parse_fn_error(self.current_token.r#type.clone());
                self.error("no prefix parse function")
            }
        }
    }

    // Fungsi baru untuk parsing definisi fungsi
    fn parse_function_literal(&mut self) -> Expression {
        self.next_token(); // Lewati 'fn'

        // Parse parameter
        if !self.expect_peek(TokenType::LeftParen) {
            return self.error("expected '(' after 'fn'");
        }

        // Parse parameter
        let mut parameters = Vec::new();
        self.next_token(); // Lewati '('
        while !self.peek_token_is(TokenType::RightParen) {
            if !self.current_token_is(TokenType::Identifier) {
                return self.error("expected parameter name");
            }
            parameters.push(Identifier { value: self.current_token.literal.clone() });
            self.next_token();
            if self.peek_token_is(TokenType::Comma) {
                self.next_token(); // Lewati ','
            }
        }
        self.next_token(); // Lewati ')'

        // TODO: Parse return type di sini untuk masa depan
        // if self.peek_token_is(TokenType::Arrow) { ... }

        if !self.expect_peek(TokenType::LeftBrace) {
            return self.error("expected '{' before function body");
        }
        let body = self.parse_block_statement();

        Expression::Function(FunctionLiteral {
            parameters,
            body,
        })
    }

    // Fungsi baru untuk parsing pemanggilan fungsi
    fn parse_call_expression(&mut self, function: Expression) -> Expression {
        self.next_token(); // Lewati '('
        let mut arguments = Vec::new();
        while !self.peek_token_is(TokenType::RightParen) {
            arguments.push(self.parse_expression(Precedence::Lowest));
            if self.peek_token_is(TokenType::Comma) {
                self.next_token(); // Lewati ','
            }
        }
        self.next_token(); // Lewati ')'
        
        Expression::Call(CallExpression {
            function: Box::new(function),
            arguments,
        })
    }

    // Fungsi baru untuk parsing if expression
    fn parse_if_expression(&mut self) -> Expression {
        self.next_token(); // Lewati 'if'

        // Parse kondisi
        let condition = self.parse_expression(Precedence::Lowest);

        if !self.expect_peek(TokenType::LeftBrace) {
            return self.error("expected '{'");
        }

        // Parse blok consequence
        let consequence = self.parse_block_statement();

        // Cek apakah ada 'else'
        let alternative = if self.peek_token_is(TokenType::Else) {
            self.next_token(); // Lewati 'else'
            
            // Cek apakah 'else' diikuti oleh 'if' (untuk if-else if)
            if self.peek_token_is(TokenType::If) {
                self.next_token(); // Lewati 'if'
                // Rekursif untuk if-else if
                Some(BlockStatement { statements: vec![Statement::Expression(ExpressionStatement{ expression: self.parse_if_expression() })] })
            } else if self.expect_peek(TokenType::LeftBrace) {
                // Parse blok else
                Some(self.parse_block_statement())
            } else {
                return self.error("expected '{' or 'if' after 'else'");
            }
        } else {
            None
        };

        Expression::If(ast::IfExpression {
            condition: Box::new(condition),
            consequence,
            alternative,
        })
    }

    // Fungsi baru untuk parsing blok statement { ... }
    fn parse_block_statement(&mut self) -> BlockStatement {
        self.next_token(); // Lewati '{'

        let mut statements = Vec::new();

        while !self.current_token_is(TokenType::RightBrace) && !self.current_token_is(TokenType::Eof) {
            if let Some(stmt) = self.parse_statement() {
                statements.push(stmt);
            }
            self.next_token();
        }
        
        BlockStatement { statements }
    }

    // Helper function untuk error
    fn error(&mut self, message: &str) -> Expression {
        self.errors.push(message.to_string());
        Expression::Identifier(Identifier{ value: "ERROR".to_string() })
    }

    // --- Presedence Helper ---
    fn peek_precedence(&self) -> Precedence {
        self.precedence(&self.peek_token.r#type)
    }

    fn current_precedence(&self) -> Precedence {
        self.precedence(&self.current_token.r#type)
    }

    fn precedence(&self, t: &TokenType) -> Precedence {
        match t {
            TokenType::Eq => Precedence::Equals,
            TokenType::NotEq => Precedence::Equals,
            TokenType::LT => Precedence::LessGreater,
            TokenType::GT => Precedence::LessGreater,
            TokenType::Plus => Precedence::Sum,
            TokenType::Minus => Precedence::Sum,
            TokenType::Slash => Precedence::Product,
            TokenType::Asterisk => Precedence::Product,
            TokenType::LeftParen => Precedence::Call,
            _ => Precedence::Lowest,
        }
    }
    
    // --- Error Handling ---
    fn peek_error(&mut self, t: TokenType) {
        let msg = format!("expected next token to be {:?}, got {:?} instead", t, self.peek_token.r#type);
        self.errors.push(msg);
    }
    
    fn no_prefix_parse_fn_error(&mut self, t: TokenType) {
        let msg = format!("no prefix parse function for {:?} found", t);
        self.errors.push(msg);
    }
}