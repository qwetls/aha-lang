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
    While,    // while loops
    For,      // for loops
    In,       // for x in range
    Break,    // break from loop
    Continue, // continue loop
    Struct,   // NEW: struct definition
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
    LtEq,         // <= NEW
    GtEq,         // >= NEW
    Bang,         // !
    // Delimiters
    Comma,        // ,
    Semicolon,    // ;
    Colon,        // : NEW for type hints
    LeftParen,    // (
    RightParen,   // )
    LeftBrace,    // {
    RightBrace,   // }
    LeftBracket,  // [ for arrays
    RightBracket, // ] for arrays
    DotDot,       // .. for ranges
    Dot,          // . NEW: field access
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
    StructLiteral(StructLiteral),  // NEW: Person { name: "x", age: 25 }
    FieldAccess(FieldAccess),      // NEW: person.name
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

// NEW: String literal
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

// NEW: While loop expression
#[derive(Debug, Clone, PartialEq)]
pub struct WhileExpression {
    pub condition: Box<Expression>,
    pub body: BlockStatement,
}

// NEW: For loop expression
#[derive(Debug, Clone, PartialEq)]
pub struct ForExpression {
    pub variable: Identifier,
    pub iterable: Box<Expression>,
    pub body: BlockStatement,
}

// NEW: Range expression (0..10)
#[derive(Debug, Clone, PartialEq)]
pub struct RangeExpression {
    pub start: Box<Expression>,
    pub end: Box<Expression>,
}

// NEW: Array literal [1, 2, 3]
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayLiteral {
    pub elements: Vec<Expression>,
}

// NEW: Index expression arr[0]
#[derive(Debug, Clone, PartialEq)]
pub struct IndexExpression {
    pub left: Box<Expression>,
    pub index: Box<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionLiteral {
    pub name: Option<Identifier>,  // NEW: optional function name
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
    Struct(StructDefinition),  // NEW: struct definitions
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

// NEW: Struct-related nodes
#[derive(Debug, Clone, PartialEq)]
pub struct StructDefinition {
    pub name: Identifier,
    pub fields: Vec<StructField>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub name: Identifier,
    pub type_hint: Option<String>,  // Optional type annotation
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructLiteral {
    pub name: Identifier,
    pub fields: Vec<(Identifier, Expression)>,  // field: value pairs
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldAccess {
    pub object: Box<Expression>,
    pub field: Identifier,
}