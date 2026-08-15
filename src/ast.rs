// src/ast.rs

// --- Token & TokenType ---
#[derive(Debug, Clone, PartialEq, Copy)]
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
    While,
    For,
    In,
    Break,
    Continue,
    Struct,
    // Operators
    Assign,       // =
    Plus,         // +
    Minus,        // -
    Asterisk,     // *
    Slash,        // /
    Percent,      // %
    Eq,           // ==
    NotEq,        // !=
    LT,           // <
    GT,           // >
    LtEq,        // <=
    GtEq,        // >=
    Bang,         // !
    And,          // &&
    Or,           // ||
    // Delimiters
    Comma,        // ,
    Semicolon,    // ;
    Colon,        // :
    LeftParen,    // (
    RightParen,   // )
    LeftBrace,    // {
    RightBrace,   // }
    LeftBracket,  // [
    RightBracket, // ]
    DotDot,       // ..
    Dot,          // .
    // Special
    Eof,          // End of file
    Illegal,      // Unrecognized character
}

// L-01: Renamed `r#type` to `kind` for idiomatic Rust
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenType,
    pub literal: String,
    pub line: usize,
    pub column: usize,
}

impl Token {
    pub fn new(kind: TokenType, literal: String, line: usize, column: usize) -> Self {
        Token {
            kind,
            literal,
            line,
            column,
        }
    }
}

// --- AST Nodes ---

// --- Expression Nodes ---
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Identifier(Identifier),
    Integer(IntegerLiteral),
    Boolean(BooleanLiteral),
    String(StringLiteral),
    Prefix(PrefixExpression),
    Infix(InfixExpression),
    If(IfExpression),
    While(WhileExpression),
    For(ForExpression),
    Function(FunctionLiteral),
    Call(CallExpression),
    Array(ArrayLiteral),
    Index(IndexExpression),
    Range(RangeExpression),
    StructLiteral(StructLiteral),
    FieldAccess(FieldAccess),
    Assignment(AssignmentExpression),
    Break,
    Continue,
}

// Assignment expression: name = value
#[derive(Debug, Clone, PartialEq)]
pub struct AssignmentExpression {
    pub name: Identifier,
    pub value: Box<Expression>,
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
pub struct StringLiteral {
    pub value: String,
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
pub struct WhileExpression {
    pub condition: Box<Expression>,
    pub body: BlockStatement,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForExpression {
    pub variable: Identifier,
    pub iterable: Box<Expression>,
    pub body: BlockStatement,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RangeExpression {
    pub start: Box<Expression>,
    pub end: Box<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayLiteral {
    pub elements: Vec<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndexExpression {
    pub left: Box<Expression>,
    pub index: Box<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionLiteral {
    pub name: Option<Identifier>,
    pub parameters: Vec<Identifier>,
    pub body: BlockStatement,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallExpression {
    pub function: Box<Expression>,
    pub arguments: Vec<Expression>,
}

// --- Statement Nodes ---
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Let(LetStatement),
    Return(ReturnStatement),
    Expression(ExpressionStatement),
    Struct(StructDefinition),
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

// --- Root Node ---
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Statement>,
}

// --- Struct-related Nodes ---
#[derive(Debug, Clone, PartialEq)]
pub struct StructDefinition {
    pub name: Identifier,
    pub fields: Vec<StructField>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub name: Identifier,
    pub type_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructLiteral {
    pub name: Identifier,
    pub fields: Vec<(Identifier, Expression)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldAccess {
    pub object: Box<Expression>,
    pub field: Identifier,
}