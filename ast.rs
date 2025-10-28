// src/ast.rs

// --- Token & TokenType ---
#[derive(Debug, Clone, PartialEq, Copy)] // TAMBAHKAN Copy, Clone
pub enum TokenType {
    // Literals
    Integer,
    String,
    Boolean,
    // Identifiers
    Identifier,
    // Keywords
    Let,
    Fn,
    True,
    False,
    If,
    Else,
    Return,
    // Operators
    Assign,       // =
    Plus,         // +
    Minus,        // -
    Asterisk,     // *
    Slash,        // /
    Eq,           // ==
    NotEq,        // !=
    LT,           // <
    GT,           // >
    Bang,         // !
    // Delimiters
    Comma,        // ,
    Semicolon,    // ;
    LeftParen,    // (
    RightParen,   // )
    LeftBrace,    // {
    RightBrace,   // }
    // Special
    Eof,          // End of File
    Illegal,      // Karakter tidak dikenal
}

#[derive(Debug, Clone)]
pub struct Token {
    pub r#type: TokenType, // PERBAIKI: gunakan r#type
    pub literal: String,
    pub line: usize,
    pub column: usize,
}

impl Token {
    pub fn new(token_type: TokenType, literal: String, line: usize, column: usize) -> Self {
        Token {
            r#type: token_type, // PERBAIKI: gunakan r#type
            literal,
            line,
            column,
        }
    }
}

// --- AST Nodes ---

// --- Node Ekspresi ---
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Identifier(Identifier),
    Integer(IntegerLiteral),
    Boolean(BooleanLiteral),
    Prefix(PrefixExpression),
    Infix(InfixExpression),
    If(IfExpression),
    Function(FunctionLiteral),
    Call(CallExpression),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Identifier {
    pub value: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IntegerLiteral {
    pub value: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BooleanLiteral {
    pub value: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrefixExpression {
    pub operator: String,
    pub right: Box<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InfixExpression {
    pub left: Box<Expression>,
    pub operator: String,
    pub right: Box<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IfExpression {
    pub condition: Box<Expression>,
    pub consequence: BlockStatement,
    pub alternative: Option<BlockStatement>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionLiteral {
    pub parameters: Vec<Identifier>,
    pub body: BlockStatement,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallExpression {
    pub function: Box<Expression>,
    pub arguments: Vec<Expression>,
}

// --- Node Pernyataan ---
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Let(LetStatement),
    Return(ReturnStatement),
    Expression(ExpressionStatement),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LetStatement {
    pub name: Identifier,
    pub value: Expression,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReturnStatement {
    pub return_value: Expression,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExpressionStatement {
    pub expression: Expression,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlockStatement {
    pub statements: Vec<Statement>,
}

// --- Node Akar ---
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Statement>,
}