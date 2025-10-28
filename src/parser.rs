// src/parser.rs


use crate::ast::{Program, Statement, LetStatement, ReturnStatement, ExpressionStatement, Expression, Identifier, IntegerLiteral, BooleanLiteral, PrefixExpression, InfixExpression, Token};
use crate::lexer::Lexer;
use crate::ast::TokenType;
use inkwell::values::BasicValueEnum;
use inkwell::values::PointerValue;

pub struct Parser {
    lexer: Lexer,
    current_token: Token,
    peek_token: Token,
    errors: Vec<String>,
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
        // Saat fungsi ini dipanggil, current_token adalah 'Let'.
        // Kita perlu memajukan token untuk mendapatkan identifier.
        self.next_token();

        // Sekarang current_token seharusnya adalah identifier (misal: 'x')
        if !self.current_token_is(TokenType::Identifier) {
            self.errors.push(format!("expected next token to be Identifier, got {:?} instead", self.current_token.r#type));
            return None;
        }
        let name = Identifier { value: self.current_token.literal.clone() };

        // Sekarang kita perlu memastikan token berikutnya adalah '='
        if !self.expect_peek(TokenType::Assign) {
            return None;
        }

        // Lewati token '=' untuk menuju ke awal ekspresi
        self.next_token();

        // Parse ekspresi di sebelah kanan tanda '='
        let value = self.parse_expression(Precedence::Lowest);

        // Lewati semicolon jika ada
        if self.peek_token_is(TokenType::Semicolon) {
            self.next_token();
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

    pub fn parse_expression(&mut self, precedence: Precedence) -> Expression {
        let mut left = self.parse_prefix();

        // Loop selama kita melihat operator dengan presedensi lebih tinggi
        while !self.peek_token_is(TokenType::Semicolon) && precedence < self.peek_precedence() {
            // Ambil operator
            self.next_token();
            let operator = self.current_token.literal.clone();
            
            // Tentukan presedensi operator
            let right_precedence = self.current_precedence();
            
            // Pindah ke ekspresi di sebelah kanan
            self.next_token();
            let right = Box::new(self.parse_expression(right_precedence));

            // Bangun ekspresi Infix baru dan jadikan sebagai 'left' untuk iterasi selanjutnya
            left = Expression::Infix(InfixExpression {
                left: Box::new(left),
                operator,
                right,
            });
        }

        left
    } 
    
    fn parse_prefix(&mut self) -> Expression {
        match self.current_token.r#type.clone() {
            TokenType::Identifier => Expression::Identifier(Identifier { value: self.current_token.literal.clone() }),
            TokenType::Integer => Expression::Integer(IntegerLiteral { value: self.current_token.literal.parse().unwrap() }),
            TokenType::True => Expression::Boolean(BooleanLiteral { value: true }),
            TokenType::False => Expression::Boolean(BooleanLiteral { value: false }),
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
                    // TODO: Handle error better, maybe return a dummy expression
                    Expression::Identifier(Identifier{ value: "ERROR".to_string() })
                } else {
                    exp
                }
            }
            _ => {
                self.no_prefix_parse_fn_error(self.current_token.r#type.clone());
                // Return a dummy expression to avoid crashing
                Expression::Identifier(Identifier{ value: "ERROR".to_string() })
            }
        }
    }

    fn parse_infix(&mut self, left: Expression) -> Option<Expression> {
        if !self.is_infix_operator(&self.peek_token.r#type) {
            return None;
        }
        
        self.next_token(); // Pindah ke operator
        let operator = self.current_token.literal.clone();
        let precedence = self.current_precedence();
        self.next_token(); // Pindah ke ekspresi di sebelah kanan
        let right = Box::new(self.parse_expression(precedence));

        Some(Expression::Infix(InfixExpression {
            left: Box::new(left),
            operator,
            right,
        }))
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
    
    fn is_infix_operator(&self, t: &TokenType) -> bool {
        matches!(t, TokenType::Plus | TokenType::Minus | TokenType::Slash | TokenType::Asterisk | TokenType::Eq | TokenType::NotEq | TokenType::LT | TokenType::GT)
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