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
    Use,
    Pub,
    Actor,
    Spawn,
    Enum,
    Match,
    Extern,
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
    QuestionMark, // ?
    And,          // &&
    Or,           // ||
    // Delimiters
    Comma,        // ,
    Semicolon,    // ;
    Colon,        // :
    ColonColon,   // ::
    LeftParen,    // (
    RightParen,   // )
    LeftBrace,    // {
    RightBrace,   // }
    LeftBracket,  // [
    RightBracket, // ]
    DotDot,       // ..
    Dot,          // .
    Arrow,        // ->
    FatArrow,     // =>
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
    ModuleAccess(ModuleAccess),
    Spawn(SpawnExpression),
    Assignment(AssignmentExpression),
    Match(MatchExpression),
    Postfix(PostfixExpression),
    Break,
    Continue,
}

// Assignment expression: name = value
#[derive(Debug, Clone, PartialEq)]
pub struct AssignmentExpression {
    pub target: Box<Expression>,
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
pub struct PostfixExpression {
    pub operator: String,
    pub operand: Box<Expression>,
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
    pub is_pub: bool,
    /// Generic type parameters: `fn max<T>(...)` → ["T"]
    pub type_params: Vec<String>,
    /// Per-parameter type hints: `fn f(a: T, b: int)` → [Some("T"), Some("int")]
    pub param_type_hints: Vec<Option<String>>,
    /// Optional return type annotation: `fn f(...) -> T`
    pub return_type_hint: Option<String>,
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
    Actor(ActorDefinition),
    Enum(EnumDefinition),
    Import(ImportStatement),
    ExternFn(ExternFnDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LetStatement {
    pub name: Identifier,
    pub value: Expression,
    /// Optional explicit type annotation: `let x: int = 5`.
    /// Stored as the raw hint string ("int", "string", "bool", struct name).
    pub type_annotation: Option<String>,
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

// --- Import Statement ---
#[derive(Debug, Clone, PartialEq)]
pub struct ImportStatement {
    /// The file path string literal, e.g. "math" or "utils/helper"
    pub path: String,
}

// --- Extern Function Declaration ---
/// `extern fn name(param: Type, ...) -> RetType;`
/// No body — the linker (AOT) or runtime (JIT) resolves the symbol.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternFnDecl {
    pub name: Identifier,
    pub parameters: Vec<Identifier>,
    pub param_type_hints: Vec<Option<String>>,
    pub return_type_hint: Option<String>,
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
    pub is_pub: bool,
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

// --- Actor-related Nodes ---

/// `actor Name { field: type, ... }` — defines an actor type.
#[derive(Debug, Clone, PartialEq)]
pub struct ActorDefinition {
    pub name: Identifier,
    pub is_pub: bool,
    pub fields: Vec<StructField>,
}

/// `spawn Name { field: expr, ... }` — creates an actor instance.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnExpression {
    pub actor_name: Identifier,
    pub fields: Vec<(Identifier, Expression)>,
}

// --- Module Access ---

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleAccess {
    pub module: String,
    pub name: String,
}

// --- Enum Definition ---

/// `enum Name { Variant, Variant(Type, ...), ... }`
#[derive(Debug, Clone, PartialEq)]
pub struct EnumDefinition {
    pub name: Identifier,
    pub is_pub: bool,
    pub variants: Vec<EnumVariant>,
}

/// A single enum variant: `Name` or `Name(Type, Type, ...)`
#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: Identifier,
    pub payload_types: Vec<String>, // empty = unit variant, e.g. `Red`
}

// --- Match Expression ---

/// `match expr { pattern => body, ... }`
#[derive(Debug, Clone, PartialEq)]
pub struct MatchExpression {
    pub value: Box<Expression>,
    pub arms: Vec<MatchArm>,
}

/// A single match arm: `Pattern => body`
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expression,
}

/// Patterns: `EnumVariant`, `EnumVariant(a, b, ...)`, or `_` (wildcard)
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// `_` wildcard — matches anything
    Wildcard,
    /// `Variant` — unit enum variant
    EnumUnit(String),
    /// `Variant(a, b, ...)` — enum variant with destructured bindings
    EnumTuple(String, Vec<String>),
}