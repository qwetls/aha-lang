// src/parser.rs

use crate::Lexer;
use crate::ast;
use crate::ast::{
    Program, Statement, Expression, Identifier, IntegerLiteral, BooleanLiteral,
    StringLiteral, PrefixExpression, InfixExpression, LetStatement, ReturnStatement,
    ExpressionStatement, BlockStatement, WhileExpression, ForExpression, ArrayLiteral,
    IndexExpression, StructDefinition, StructField, StructLiteral, FieldAccess,
    AssignmentExpression, FunctionLiteral, ImportStatement, ModuleAccess,
    ActorDefinition, SpawnExpression,
    EnumDefinition, EnumVariant, MatchExpression, MatchArm, Pattern,
    ExternFnDecl,
};
use crate::ast::Token;
use crate::ast::TokenType;

pub struct Parser {
    lexer: Lexer,
    current_token: Token,
    peek_token: Token,
    pub errors: Vec<String>,
    /// Names of structs declared so far, so `Point { ... }` is parsed as a
    /// struct literal instead of an identifier followed by a block.
    struct_names: std::collections::HashSet<String>,
}

// Operator precedence levels (lowest to highest)
#[derive(Debug, PartialEq, PartialOrd)]
pub enum Precedence {
    Lowest,
    Logical,     // && or ||
    Assign,      // =
    Equals,      // == or !=
    LessGreater, // > or < or <= or >=
    Range,       // ..
    Sum,         // +
    Product,     // *
    Prefix,      // -X or !X
    Call,        // myFunction(X)
    Index,       // arr[i]
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
            struct_names: std::collections::HashSet::new(),
        }
    }

    /// Create a parser with pre-known struct names (from imported files).
    pub fn with_structs(mut lexer: Lexer, known_structs: std::collections::HashSet<String>) -> Self {
        let current_token = lexer.next_token();
        let peek_token = lexer.next_token();

        Parser {
            lexer,
            current_token,
            peek_token,
            errors: Vec::new(),
            struct_names: known_structs,
        }
    }

    /// Get the struct names discovered during parsing.
    pub fn get_struct_names(&self) -> &std::collections::HashSet<String> {
        &self.struct_names
    }

    // Parse the entire program into an AST
    pub fn parse_program(&mut self) -> Program {
        let mut program = Program { statements: Vec::new() };

        while self.current_token.kind != TokenType::Eof {
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

    /// Parse a type hint, supporting compound types like `List<int>` and
    /// `Map<K,V>` (two comma-separated inner types).
    /// Caller has already consumed the leading identifier (current_token is
    /// the first identifier of the hint). Consumes the full hint and returns
    /// the canonical hint string ("List<int>", "Map<string,int>", ...).
    fn parse_type_hint(&mut self) -> Option<String> {
        // *T — raw pointer prefix
        if self.current_token_is(TokenType::Asterisk) {
            self.next_token(); // skip '*'
            let inner = self.parse_type_hint()?;
            return Some(format!("*{}", inner));
        }
        if !self.current_token_is(TokenType::Identifier) {
            return None;
        }
        let hint = self.current_token.literal.clone();
        // Compound hint: identifier followed by '<'.
        if self.peek_token_is(TokenType::LT) {
            self.next_token(); // current = '<', peek = first token of inner
            self.next_token(); // current = inner hint start, peek = '>' or '<'
            let first_hint = self.parse_type_hint()?;
            if self.peek_token_is(TokenType::Comma) {
                // Map<K, V>: after the key hint comes a comma, then the value hint.
                self.next_token(); // current = ','
                self.next_token(); // current = value hint start
                let second_hint = self.parse_type_hint()?;
                if !self.expect_peek(TokenType::GT) {
                    self.errors.push("Expected '>' to close Map<K,V> type hint".to_string());
                    return None;
                }
                return Some(format!("Map<{}, {}>", first_hint, second_hint));
            }
            if !self.expect_peek(TokenType::GT) {
                self.errors.push("Expected '>' to close List<T> type hint".to_string());
                return None;
            }
            return Some(format!("List<{}>", first_hint));
        }
        Some(hint)
    }
    
    fn current_token_is(&self, t: TokenType) -> bool {
        self.current_token.kind == t
    }

    fn peek_token_is(&self, t: TokenType) -> bool {
        self.peek_token.kind == t
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

    // --- Statement Parsing ---

    fn parse_statement(&mut self) -> Option<Statement> {
        match self.current_token.kind {
            TokenType::Let => self.parse_let_statement(),
            TokenType::Return => self.parse_return_statement(),
            TokenType::Struct => self.parse_struct_definition(false),
            TokenType::Actor => self.parse_actor_definition(false),
            TokenType::Enum => self.parse_enum_definition(false),
            TokenType::Use => self.parse_use_statement(),
            TokenType::Pub => self.parse_pub_statement(),
            TokenType::Extern => self.parse_extern_function(),
            _ => self.parse_expression_statement(),
        }
    }

    fn parse_pub_statement(&mut self) -> Option<Statement> {
        self.next_token(); // Skip 'pub'
        match self.current_token.kind {
            TokenType::Fn => self.parse_function_statement(true),
            TokenType::Struct => self.parse_struct_definition(true),
            TokenType::Actor => self.parse_actor_definition(true),
            TokenType::Enum => self.parse_enum_definition(true),
            _ => {
                self.errors.push("Expected 'fn', 'struct', 'enum', or 'actor' after 'pub'".to_string());
                None
            }
        }
    }

    fn parse_function_statement(&mut self, is_pub: bool) -> Option<Statement> {
        let expr = self.parse_function_literal_with_pub(is_pub);
        Some(Statement::Expression(ExpressionStatement { expression: expr }))
    }

    /// Parse `extern fn name(param: Type, ...) -> RetType;`
    /// Called when current_token is Extern. peek_token is the next token (fn).
    fn parse_extern_function(&mut self) -> Option<Statement> {
        // peek_token should be 'fn' — advance to verify
        if !self.expect_peek(TokenType::Fn) {
            self.errors.push("Expected 'fn' after 'extern'".to_string());
            return None;
        }

        // Now current_token = Fn, peek_token = Identifier(name)
        if !self.expect_peek(TokenType::Identifier) {
            self.errors.push("Expected function name after 'extern fn'".to_string());
            return None;
        }

        let name = Identifier { value: self.current_token.literal.clone() };

        if !self.expect_peek(TokenType::LeftParen) {
            self.errors.push("Expected '(' after extern function name".to_string());
            return None;
        }

        let (parameters, param_type_hints) = self.parse_function_parameters();
        eprintln!("DEBUG parse_extern_function: after params current={:?} peek={:?}", self.current_token.kind, self.peek_token.kind);

        // Optional return type: -> T
        let return_type_hint = if self.peek_token_is(TokenType::Arrow) {
            self.next_token(); // skip '->'
            self.parse_type_hint()
        } else {
            None
        };

        // Expect semicolon to close the declaration
        if !self.expect_peek(TokenType::Semicolon) {
            self.errors.push("Expected ';' after extern fn declaration".to_string());
            return None;
        }

        Some(Statement::ExternFn(ExternFnDecl {
            name,
            parameters,
            param_type_hints,
            return_type_hint,
        }))
    }

    fn parse_struct_definition(&mut self, is_pub: bool) -> Option<Statement> {
        self.next_token(); // Skip 'struct'
        
        if !self.current_token_is(TokenType::Identifier) {
            self.errors.push("Expected struct name".to_string());
            return None;
        }
        let name = Identifier { value: self.current_token.literal.clone() };
        self.struct_names.insert(name.value.clone());

        if !self.expect_peek(TokenType::LeftBrace) {
            return None;
        }
        
        let mut fields = Vec::new();
        self.next_token(); // Skip '{'
        
        while !self.current_token_is(TokenType::RightBrace) {
            if !self.current_token_is(TokenType::Identifier) {
                break;
            }
            let field_name = Identifier { value: self.current_token.literal.clone() };
            
            // Optional type hint after colon
            let type_hint = if self.peek_token_is(TokenType::Colon) {
                self.next_token(); // Skip field name
                self.next_token(); // Skip ':'
                self.parse_type_hint()
            } else {
                None
            };
            
            fields.push(StructField { name: field_name, type_hint });
            
            if self.peek_token_is(TokenType::Comma) {
                self.next_token(); // Skip current
                self.next_token(); // Skip ','
            } else {
                self.next_token();
            }
        }
        
        Some(Statement::Struct(StructDefinition { name, is_pub, fields }))
    }

    fn parse_actor_definition(&mut self, is_pub: bool) -> Option<Statement> {
        self.next_token(); // Skip 'actor'

        if !self.current_token_is(TokenType::Identifier) {
            self.errors.push("Expected actor name".to_string());
            return None;
        }
        let name = Identifier { value: self.current_token.literal.clone() };
        self.struct_names.insert(name.value.clone());

        if !self.expect_peek(TokenType::LeftBrace) {
            return None;
        }

        let mut fields = Vec::new();
        self.next_token(); // Skip '{'

        while !self.current_token_is(TokenType::RightBrace) {
            if !self.current_token_is(TokenType::Identifier) {
                break;
            }
            let field_name = Identifier { value: self.current_token.literal.clone() };

            let type_hint = if self.peek_token_is(TokenType::Colon) {
                self.next_token(); // Skip field name
                self.next_token(); // Skip ':'
                self.parse_type_hint()
            } else {
                None
            };

            fields.push(StructField { name: field_name, type_hint });

            if self.peek_token_is(TokenType::Comma) {
                self.next_token();
                self.next_token();
            } else {
                self.next_token();
            }
        }

        // current_token is '}' from the last else-branch next_token();
        // Don't consume it here — parse_program's loop calls next_token().

        Some(Statement::Actor(ActorDefinition { name, is_pub, fields }))
    }

    /// Parse: enum Name { Variant, Variant(Type, ...), ... }
    fn parse_enum_definition(&mut self, is_pub: bool) -> Option<Statement> {
        self.next_token(); // Skip 'enum'

        if !self.current_token_is(TokenType::Identifier) {
            self.errors.push("Expected enum name".to_string());
            return None;
        }
        let name = Identifier { value: self.current_token.literal.clone() };

        if !self.expect_peek(TokenType::LeftBrace) {
            return None;
        }
        self.next_token(); // Skip '{'

        let mut variants = Vec::new();

        while !self.current_token_is(TokenType::RightBrace) && !self.current_token_is(TokenType::Eof) {
            if !self.current_token_is(TokenType::Identifier) {
                self.errors.push(format!(
                    "Expected variant name, got {:?}",
                    self.current_token.kind
                ));
                break;
            }
            let variant_name = Identifier { value: self.current_token.literal.clone() };

            // Optional tuple payload: Variant(Type, Type, ...)
            let payload_types = if self.peek_token_is(TokenType::LeftParen) {
                self.next_token(); // '('
                let mut types = Vec::new();
                self.next_token(); // first token inside parens
                while !self.current_token_is(TokenType::RightParen) && !self.current_token_is(TokenType::Eof) {
                    if self.current_token_is(TokenType::Identifier) {
                        types.push(self.current_token.literal.clone());
                    }
                    self.next_token();
                }
                self.next_token(); // skip ')'
                types
            } else {
                Vec::new()
            };

            // For tuple variants, current is already on comma after ')'.
            // For unit variants, current is still on variant name — advance first.
            let is_tuple = !payload_types.is_empty();
            variants.push(EnumVariant { name: variant_name, payload_types });

            if is_tuple {
                // Tuple: current = comma, check directly
                if self.current_token_is(TokenType::Comma) {
                    self.next_token(); // skip ','
                    if self.current_token_is(TokenType::RightBrace) {
                        break;
                    }
                } else if self.current_token_is(TokenType::RightBrace) {
                    break;
                }
            } else {
                // Unit: peek to check what's next
                if self.peek_token_is(TokenType::Comma) {
                    self.next_token(); // skip variant name
                    self.next_token(); // skip ','
                    if self.current_token_is(TokenType::RightBrace) {
                        break;
                    }
                } else if self.peek_token_is(TokenType::RightBrace) {
                    self.next_token(); // advance to '}'
                    break; // last variant
                } else {
                    self.next_token(); // advance to next variant
                }
            }
        }
        // ponytail: current stays on '}' — parse_program advances past it

        Some(Statement::Enum(EnumDefinition { name, is_pub, variants }))
    }

    /// Parse: spawn ActorName { field: value, ... }
    /// Current token is `spawn` (already consumed by caller).
    fn parse_spawn_expression(&mut self) -> Expression {
        self.next_token(); // Skip 'spawn'

        if !self.current_token_is(TokenType::Identifier) {
            self.errors.push("Expected actor name after 'spawn'".to_string());
            return Expression::Integer(IntegerLiteral { value: 0 });
        }
        let actor_name = Identifier { value: self.current_token.literal.clone() };

        if !self.expect_peek(TokenType::LeftBrace) {
            return Expression::Integer(IntegerLiteral { value: 0 });
        }

        let mut fields = Vec::new();
        self.next_token(); // Skip '{'

        while !self.current_token_is(TokenType::RightBrace)
            && !self.current_token_is(TokenType::Eof)
        {
            if !self.current_token_is(TokenType::Identifier) {
                break;
            }
            let field_name = Identifier { value: self.current_token.literal.clone() };

            if !self.expect_peek(TokenType::Colon) {
                break;
            }
            self.next_token(); // Skip ':'
            let value = self.parse_expression(Precedence::Lowest);
            fields.push((field_name, value));

            if self.peek_token_is(TokenType::Comma) {
                self.next_token(); // Skip value
                self.next_token(); // Skip ','
            } else {
                self.next_token();
            }
        }

        // Don't consume '}' — parse_expression's while loop exits on it.
        if !self.current_token_is(TokenType::RightBrace) {
            self.errors.push("Expected '}' to close spawn expression".to_string());
        }

        Expression::Spawn(SpawnExpression { actor_name, fields })
    }

    /// Parse a struct literal: TypeName { field: value, field2: value2 }
    fn parse_struct_literal(&mut self, name: Identifier) -> Expression {
        // current token is the type name; peek is '{'
        self.next_token(); // move to '{'

        let mut fields: Vec<(Identifier, Expression)> = Vec::new();
        self.next_token(); // move past '{'

        while !self.current_token_is(TokenType::RightBrace)
            && !self.current_token_is(TokenType::Eof)
        {
            if !self.current_token_is(TokenType::Identifier) {
                self.errors.push("Expected field name in struct literal".to_string());
                break;
            }
            let field_name = Identifier { value: self.current_token.literal.clone() };

            if !self.expect_peek(TokenType::Colon) {
                self.errors.push("Expected ':' after field name in struct literal".to_string());
                break;
            }
            self.next_token(); // move to value expression

            let value = self.parse_expression(Precedence::Lowest);
            fields.push((field_name, value));

            if self.peek_token_is(TokenType::Comma) {
                self.next_token(); // move to ','
                self.next_token(); // move to next field name
            } else {
                self.next_token(); // move to '}' (or whatever ends the literal)
            }
        }

        // Don't consume '}' — parse_expression's while loop will see it via peek
        // and exit. parse_program's next_token() advances past it.
        if !self.current_token_is(TokenType::RightBrace) {
            self.errors.push("Expected '}' to close struct literal".to_string());
        }

        Expression::StructLiteral(StructLiteral { name, fields })
    }

    /// Parse a `use "file"` statement.
    /// Syntax: `use "path/to/file"` — imports all functions and structs from the file.
    fn parse_use_statement(&mut self) -> Option<Statement> {
        self.next_token(); // Skip 'use'

        if !self.current_token_is(TokenType::String) {
            self.errors.push(format!(
                "Expected file path string after 'use', got '{}'",
                self.current_token.literal
            ));
            return None;
        }

        let path = self.current_token.literal.clone();

        // Optional semicolon — consume if present, but do NOT advance past
        // the string token. The parse_program loop calls next_token() after
        // each statement, which handles the advance.
        if self.peek_token_is(TokenType::Semicolon) {
            self.next_token();
        }

        Some(Statement::Import(ImportStatement { path }))
    }

    fn parse_let_statement(&mut self) -> Option<Statement> {
        self.next_token(); // Skip 'let'

        if !self.current_token_is(TokenType::Identifier) {
            self.errors.push(format!(
                "Expected identifier after 'let', got {:?} instead",
                self.current_token.kind
            ));
            return None;
        }
        let name = Identifier { value: self.current_token.literal.clone() };

        // Optional type annotation: `let x: int = 5`
        let mut type_annotation: Option<String> = None;
        if self.peek_token_is(TokenType::Colon) {
            self.next_token(); // Skip ':'
            if !self.expect_peek(TokenType::Identifier) {
                return None;
            }
            type_annotation = self.parse_type_hint();
        }

        if !self.expect_peek(TokenType::Assign) {
            return None;
        }

        self.next_token(); // Skip '='
        let value = self.parse_expression(Precedence::Lowest);

        if self.peek_token_is(TokenType::Semicolon) {
            self.next_token(); // Skip ';'
        }

        Some(Statement::Let(LetStatement { name, value, type_annotation }))
    }

    fn parse_return_statement(&mut self) -> Option<Statement> {
        self.next_token(); // Skip 'return'
        let return_value = self.parse_expression(Precedence::Lowest);

        if self.peek_token_is(TokenType::Semicolon) {
            self.next_token(); // Skip ';'
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

    // --- Expression Parsing (Pratt Parser) ---

    pub fn parse_expression(&mut self, precedence: Precedence) -> Expression {
        let mut left = self.parse_prefix();

        while !self.peek_token_is(TokenType::Semicolon) && precedence < self.peek_precedence() {
            // Handle index expression: arr[i]
            if self.peek_token_is(TokenType::LeftBracket) {
                self.next_token(); // consume '['
                self.next_token(); // move to index expression
                let index = self.parse_expression(Precedence::Lowest);
                if !self.expect_peek(TokenType::RightBracket) {
                    return Expression::Integer(IntegerLiteral { value: 0 });
                }
                left = Expression::Index(IndexExpression {
                    left: Box::new(left),
                    index: Box::new(index),
                });
                continue;
            }
            
            // Handle field access: person.name
            if self.peek_token_is(TokenType::Dot) {
                self.next_token(); // consume '.'
                self.next_token(); // move to field name
                if !self.current_token_is(TokenType::Identifier) {
                    return Expression::Integer(IntegerLiteral { value: 0 });
                }
                let field = Identifier { value: self.current_token.literal.clone() };
                left = Expression::FieldAccess(FieldAccess {
                    object: Box::new(left),
                    field,
                });
                continue;
            }

            // Handle assignment: left = expr
            // Left can be an identifier (x = 5) or a field access (p.x = 5)
            if self.peek_token_is(TokenType::Assign) {
                self.next_token(); // consume '='
                self.next_token(); // consume '='
                let value = self.parse_expression(Precedence::Lowest);
                left = Expression::Assignment(AssignmentExpression {
                    target: Box::new(left),
                    value: Box::new(value),
                });
                continue;
            }
            
            // Handle function call: expr(args)
            if self.peek_token_is(TokenType::LeftParen) {
                self.next_token(); // consume '('
                let arguments = self.parse_call_arguments();
                left = Expression::Call(ast::CallExpression {
                    function: Box::new(left),
                    arguments,
                });
                continue;
            }

            // Handle range expression: 0..10
            if self.peek_token_is(TokenType::DotDot) {
                self.next_token(); // consume '..'
                self.next_token(); // move to end expression
                let end = Box::new(self.parse_expression(Precedence::Range));
                left = Expression::Range(ast::RangeExpression {
                    start: Box::new(left),
                    end,
                });
                continue;
            }

            // Generic infix operator
            self.next_token(); // consume operator
            let operator = self.current_token.literal.clone();
            let right_precedence = self.current_precedence();
            self.next_token(); // move to right-hand expression
            let right = Box::new(self.parse_expression(right_precedence));

            left = Expression::Infix(InfixExpression {
                left: Box::new(left),
                operator,
                right,
            });
        }

        left
    }
    
    // Parse prefix expressions (literals, identifiers, unary operators, etc.)
    fn parse_prefix(&mut self) -> Expression {
        match self.current_token.kind {
            TokenType::Identifier => {
                let ident = Identifier { value: self.current_token.literal.clone() };
                // Struct literal: TypeName { field: value, ... }
                if self.peek_token_is(TokenType::LeftBrace)
                    && self.struct_names.contains(&ident.value)
                {
                    return self.parse_struct_literal(ident);
                }
                // Module access: module::name
                if self.peek_token_is(TokenType::ColonColon) {
                    self.next_token(); // skip '::'
                    self.next_token(); // skip to name
                    let name = self.current_token.literal.clone();
                    // Qualified struct literal: module::Name { field: value, ... }
                    if self.peek_token_is(TokenType::LeftBrace)
                        && self.struct_names.contains(&name)
                    {
                        let name_ident = Identifier { value: name };
                        return self.parse_struct_literal(name_ident);
                    }
                    return Expression::ModuleAccess(ModuleAccess {
                        module: ident.value,
                        name,
                    });
                }
                Expression::Identifier(ident)
            },
            TokenType::Integer => {
                match self.current_token.literal.parse() {
                    Ok(v) => Expression::Integer(IntegerLiteral { value: v }),
                    Err(_) => {
                        self.errors.push(format!(
                            "Could not parse '{}' as integer",
                            self.current_token.literal
                        ));
                        Expression::Integer(IntegerLiteral { value: 0 })
                    }
                }
            },
            TokenType::True => Expression::Boolean(BooleanLiteral { value: true }),
            TokenType::False => Expression::Boolean(BooleanLiteral { value: false }),
            TokenType::String => Expression::String(StringLiteral {
                value: self.current_token.literal.clone(),
            }),
            TokenType::If => self.parse_if_expression(),
            TokenType::While => self.parse_while_expression(),
            TokenType::For => self.parse_for_expression(),
            TokenType::Fn => self.parse_function_literal_with_pub(false),
            TokenType::Break => Expression::Break,
            TokenType::Continue => Expression::Continue,
            TokenType::Spawn => self.parse_spawn_expression(),
            TokenType::Match => self.parse_match_expression(),
            TokenType::LeftBracket => self.parse_array_literal(),
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
                    self.errors.push("Expected closing parenthesis ')'".to_string());
                    return exp;
                }
                exp
            }
            _ => {
                self.no_prefix_parse_fn_error(self.current_token.kind);
                Expression::Integer(IntegerLiteral { value: 0 })
            }
        }
    }

    // Parse function literal: fn name(params) { body }
    fn parse_function_literal_with_pub(&mut self, is_pub: bool) -> Expression {
        let name = if self.peek_token_is(TokenType::Identifier) {
            self.next_token();
            Some(Identifier { value: self.current_token.literal.clone() })
        } else {
            None
        };

        // Generic type parameters: fn max<T, U>(...) 
        let mut type_params = Vec::new();
        if self.peek_token_is(TokenType::LT) {
            self.next_token(); // skip '<'
            while !self.current_token_is(TokenType::GT) && !self.current_token_is(TokenType::Eof) {
                if self.current_token_is(TokenType::Identifier) {
                    type_params.push(self.current_token.literal.clone());
                }
                self.next_token(); // skip ',' or type name
            }
            if !self.expect_peek(TokenType::LeftParen) {
                self.errors.push("Expected '(' after generic type params".to_string());
                return Expression::Integer(IntegerLiteral { value: 0 });
            }
        } else if !self.expect_peek(TokenType::LeftParen) {
            self.errors.push("Expected '(' after function name".to_string());
            return Expression::Integer(IntegerLiteral { value: 0 });
        }

        let (parameters, param_type_hints) = self.parse_function_parameters();

        // Optional return type annotation: fn f(...) -> T
        let return_type_hint = if self.peek_token_is(TokenType::Arrow) {
            self.next_token(); // skip '->'
            if !self.expect_peek(TokenType::Identifier) {
                self.errors.push("Expected type after '->' in function return".to_string());
            }
            self.parse_type_hint()
        } else {
            None
        };

        if !self.expect_peek(TokenType::LeftBrace) {
            self.errors.push("Expected '{' for function body".to_string());
            return Expression::Integer(IntegerLiteral { value: 0 });
        }

        let body = self.parse_block_statement();

        Expression::Function(FunctionLiteral { name, parameters, is_pub, type_params, param_type_hints, return_type_hint, body })
    }

    // Parse function parameters: (a, b, c) or (a: T, b: int)
    fn parse_function_parameters(&mut self) -> (Vec<Identifier>, Vec<Option<String>>) {
        let mut params = Vec::new();
        let mut hints = Vec::new();

        if self.peek_token_is(TokenType::RightParen) {
            self.next_token();
            return (params, hints);
        }

        self.next_token(); // Skip '('
        params.push(Identifier { value: self.current_token.literal.clone() });

        // Optional per-param type hint: name: Type
        let hint = if self.peek_token_is(TokenType::Colon) {
            self.next_token(); // skip ':'
            self.next_token(); // advance to type token
            let is_ptr = self.current_token_is(TokenType::Asterisk);
            let h = self.parse_type_hint();
            if is_ptr { self.next_token(); } // advance past base type for pointers
            h
        } else {
            None
        };
        hints.push(hint);

        while self.peek_token_is(TokenType::Comma) {
            self.next_token(); // Skip current param
            self.next_token(); // Skip ','
            params.push(Identifier { value: self.current_token.literal.clone() });
            let hint = if self.peek_token_is(TokenType::Colon) {
                self.next_token(); // skip ':'
                self.next_token(); // advance to type token
                let is_ptr = self.current_token_is(TokenType::Asterisk);
                let h = self.parse_type_hint();
                if is_ptr { self.next_token(); } // advance past base type for pointers
                h
            } else {
                None
            };
            hints.push(hint);
        }

        if !self.expect_peek(TokenType::RightParen) {
            self.errors.push("Expected ')' after function parameters".to_string());
        }
        eprintln!("DEBUG parse_function_parameters: current={:?} peek={:?}", self.current_token.kind, self.peek_token.kind);

        (params, hints)
    }

    // Parse function call arguments: (expr, expr, ...)
    fn parse_call_arguments(&mut self) -> Vec<Expression> {
        let mut args = Vec::new();

        if self.peek_token_is(TokenType::RightParen) {
            self.next_token();
            return args;
        }

        self.next_token(); // Skip '('
        args.push(self.parse_expression(Precedence::Lowest));

        while self.peek_token_is(TokenType::Comma) {
            self.next_token(); // Skip current arg
            self.next_token(); // Skip ','
            args.push(self.parse_expression(Precedence::Lowest));
        }

        if !self.expect_peek(TokenType::RightParen) {
            self.errors.push("Expected ')' after function arguments".to_string());
        }

        args
    }

    // Parse while expression: while condition { body }
    fn parse_while_expression(&mut self) -> Expression {
        self.next_token(); // Skip 'while'
        
        let condition = self.parse_expression(Precedence::Lowest);
        
        if !self.expect_peek(TokenType::LeftBrace) {
            return Expression::Integer(IntegerLiteral { value: 0 });
        }
        
        let body = self.parse_block_statement();
        
        Expression::While(WhileExpression {
            condition: Box::new(condition),
            body,
        })
    }

    // Parse for expression: for variable in iterable { body }
    fn parse_for_expression(&mut self) -> Expression {
        self.next_token(); // Skip 'for'
        
        if !self.current_token_is(TokenType::Identifier) {
            self.errors.push("Expected identifier in for loop".to_string());
            return Expression::Integer(IntegerLiteral { value: 0 });
        }
        let variable = Identifier { value: self.current_token.literal.clone() };
        
        if !self.expect_peek(TokenType::In) {
            return Expression::Integer(IntegerLiteral { value: 0 });
        }
        
        self.next_token(); // Skip 'in'
        
        let iterable = self.parse_expression(Precedence::Lowest);
        
        if !self.expect_peek(TokenType::LeftBrace) {
            return Expression::Integer(IntegerLiteral { value: 0 });
        }
        
        let body = self.parse_block_statement();
        
        Expression::For(ForExpression {
            variable,
            iterable: Box::new(iterable),
            body,
        })
    }

    /// Parse: match expr { pattern => body, ... }
    fn parse_match_expression(&mut self) -> Expression {
        self.next_token(); // Skip 'match'

        let value = self.parse_expression(Precedence::Lowest);

        if !self.expect_peek(TokenType::LeftBrace) {
            return Expression::Integer(IntegerLiteral { value: 0 });
        }
        self.next_token(); // Skip '{'

        let mut arms = Vec::new();
        while !self.current_token_is(TokenType::RightBrace) && !self.current_token_is(TokenType::Eof) {
            let pattern = self.parse_pattern();

            // parse_pattern leaves current_token on ',' (unit) or '=>' (tuple).
            // For unit patterns, advance past comma to reach '=>'.
            if self.current_token_is(TokenType::Comma) {
                self.next_token(); // ',' → '=>'
            }
            if !self.current_token_is(TokenType::FatArrow) {
                self.errors.push(format!(
                    "Expected => after pattern, got {:?}",
                    self.current_token.kind
                ));
                self.next_token();
                continue;
            }
            self.next_token(); // Skip '=>'

            let body = self.parse_expression(Precedence::Lowest);
            arms.push(MatchArm { pattern, body });

            // After parse_expression, current_token is on body's last token,
            // peek_token is on ',' or '}'. Skip comma if present.
            if self.peek_token_is(TokenType::Comma) {
                self.next_token(); // advance to ','
                self.next_token(); // advance past ',' to next arm
            } else if self.peek_token_is(TokenType::RightBrace) {
                self.next_token(); // advance to '}'
                break; // last arm, exit loop
            }
        }

        Expression::Match(MatchExpression {
            value: Box::new(value),
            arms,
        })
    }

    /// Parse a match pattern: `_`, `Variant`, or `Variant(a, b, ...)`
    /// Advances current_token to `=>` (FatArrow) in all cases.
    fn parse_pattern(&mut self) -> Pattern {
        if self.current_token_is(TokenType::Identifier) && self.current_token.literal == "_" {
            self.next_token(); // Skip '_'
            return Pattern::Wildcard;
        }

        if !self.current_token_is(TokenType::Identifier) {
            self.errors.push(format!(
                "Expected pattern, got {:?}",
                self.current_token.kind
            ));
            self.next_token();
            return Pattern::Wildcard;
        }

        let name = self.current_token.literal.clone();

        // Check for tuple pattern: Variant(a, b, ...)
        if self.peek_token_is(TokenType::LeftParen) {
            self.next_token(); // Skip variant name
            self.next_token(); // Skip '('
            let mut bindings = Vec::new();
            while !self.current_token_is(TokenType::RightParen) && !self.current_token_is(TokenType::Eof) {
                if self.current_token_is(TokenType::Identifier) {
                    bindings.push(self.current_token.literal.clone());
                }
                self.next_token();
            }
            self.next_token(); // Skip ')'
            Pattern::EnumTuple(name, bindings)
        } else {
            self.next_token(); // Skip variant name → ','
            Pattern::EnumUnit(name)
        }
    }

    // Parse array literal: [elem1, elem2, ...]
    fn parse_array_literal(&mut self) -> Expression {
        let mut elements = Vec::new();
        
        if self.peek_token_is(TokenType::RightBracket) {
            self.next_token();
            return Expression::Array(ArrayLiteral { elements });
        }
        
        self.next_token(); // Skip '['
        elements.push(self.parse_expression(Precedence::Lowest));
        
        while self.peek_token_is(TokenType::Comma) {
            self.next_token(); // Skip current element
            self.next_token(); // Skip ','
            elements.push(self.parse_expression(Precedence::Lowest));
        }
        
        if !self.expect_peek(TokenType::RightBracket) {
            return Expression::Integer(IntegerLiteral { value: 0 });
        }
        
        Expression::Array(ArrayLiteral { elements })
    }

    // Parse if expression: if condition { consequence } else { alternative }
    fn parse_if_expression(&mut self) -> Expression {
        self.next_token(); // Skip 'if'

        let condition = self.parse_expression(Precedence::Lowest);

        if !self.expect_peek(TokenType::LeftBrace) {
            return Expression::Integer(IntegerLiteral { value: 0 });
        }

        let consequence = self.parse_block_statement();

        // Check for 'else' branch
        let alternative = if self.peek_token_is(TokenType::Else) {
            self.next_token(); // Skip 'else'
            
            // Check for 'else if' chain
            if self.peek_token_is(TokenType::If) {
                self.next_token(); // Skip to 'if'
                // Recursive: wrap else-if as a block containing an if expression
                Some(BlockStatement {
                    statements: vec![Statement::Expression(ExpressionStatement {
                        expression: self.parse_if_expression(),
                    })],
                })
            } else if self.expect_peek(TokenType::LeftBrace) {
                Some(self.parse_block_statement())
            } else {
                return Expression::Integer(IntegerLiteral { value: 0 });
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

    // Parse a block statement: { stmt1; stmt2; ... }
    fn parse_block_statement(&mut self) -> BlockStatement {
        self.next_token(); // Skip '{'

        let mut statements = Vec::new();

        while !self.current_token_is(TokenType::RightBrace) && !self.current_token_is(TokenType::Eof) {
            if let Some(stmt) = self.parse_statement() {
                statements.push(stmt);
            }
            self.next_token();
        }
        
        BlockStatement { statements }
    }

    // --- Precedence Helpers ---

    fn peek_precedence(&self) -> Precedence {
        self.precedence(&self.peek_token.kind)
    }

    fn current_precedence(&self) -> Precedence {
        self.precedence(&self.current_token.kind)
    }

    fn precedence(&self, t: &TokenType) -> Precedence {
        match t {
            TokenType::Assign => Precedence::Assign,
            TokenType::Eq => Precedence::Equals,
            TokenType::NotEq => Precedence::Equals,
            TokenType::And => Precedence::Logical,
            TokenType::Or => Precedence::Logical,
            TokenType::LT => Precedence::LessGreater,
            TokenType::GT => Precedence::LessGreater,
            TokenType::LtEq => Precedence::LessGreater,
            TokenType::GtEq => Precedence::LessGreater,
            TokenType::DotDot => Precedence::Range,
            TokenType::Plus => Precedence::Sum,
            TokenType::Minus => Precedence::Sum,
            TokenType::Slash => Precedence::Product,
            TokenType::Asterisk => Precedence::Product,
            TokenType::Percent => Precedence::Product,
            TokenType::LeftParen => Precedence::Call,
            TokenType::LeftBracket => Precedence::Index,
            TokenType::Dot => Precedence::Index,
            _ => Precedence::Lowest,
        }
    }
    
    // --- Error Handling ---

    fn peek_error(&mut self, t: TokenType) {
        let msg = format!(
            "Expected next token to be {:?}, got {:?} instead",
            t, self.peek_token.kind
        );
        self.errors.push(msg);
    }
    
    fn no_prefix_parse_fn_error(&mut self, t: TokenType) {
        let msg = format!("No prefix parse function for {:?} found", t);
        self.errors.push(msg);
    }
}