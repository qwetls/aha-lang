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
    While,    // NEW: while loops
    For,      // NEW: for loops
    In,       // NEW: for x in range
    Break,    // NEW: break from loop
    Continue, // NEW: continue loop
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
    LeftBracket,  // [ NEW for arrays
    RightBracket, // ] NEW for arrays
    DotDot,       // .. NEW for ranges
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
    While(WhileExpression),       // NEW
    For(ForExpression),           // NEW
    Function(FunctionLiteral),
    Call(CallExpression),
    Array(ArrayLiteral),          // NEW
    Index(IndexExpression),       // NEW
    Range(RangeExpression),       // NEW
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