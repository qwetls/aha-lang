// src/types.rs
//
// AHA! Type System — Defines the type representation used throughout
// the compiler for type tracking and type checking.

use std::fmt;

/// Represents all types that AHA! language understands.
/// This is the compiler's internal representation, not the LLVM type.
#[derive(Debug, Clone, PartialEq)]
pub enum AhaType {
    /// 64-bit signed integer
    Int,
    /// Boolean (true/false)
    Bool,
    /// String — represented as {i8*, i64} (pointer + length) in LLVM
    String,
    /// Void/Unit type — result of statements that don't produce a value
    Void,
    /// Homogeneous array of elements
    Array(Box<AhaType>),
    /// Function type with parameter types and return type
    Function {
        params: Vec<AhaType>,
        ret: Box<AhaType>,
    },
}

impl AhaType {
    pub fn is_int(&self) -> bool {
        matches!(self, AhaType::Int)
    }

    pub fn is_bool(&self) -> bool {
        matches!(self, AhaType::Bool)
    }

    pub fn is_string(&self) -> bool {
        matches!(self, AhaType::String)
    }

    pub fn is_void(&self) -> bool {
        matches!(self, AhaType::Void)
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, AhaType::Int | AhaType::Bool)
    }

    /// Check if two types are compatible for an operator
    pub fn check_binary_op(&self, op: &str, other: &AhaType) -> Result<AhaType, String> {
        match (self, op, other) {
            // Arithmetic: int op int → int
            (AhaType::Int, "+" | "-" | "*" | "/" | "%", AhaType::Int) => Ok(AhaType::Int),

            // String concatenation: string + string → string
            (AhaType::String, "+", AhaType::String) => Ok(AhaType::String),

            // Comparison: int op int → bool
            (AhaType::Int, "==" | "!=" | "<" | ">" | "<=" | ">=", AhaType::Int) => Ok(AhaType::Bool),

            // String comparison: string == string → bool
            (AhaType::String, "==" | "!=", AhaType::String) => Ok(AhaType::Bool),

            // Bool comparison: bool == bool → bool
            (AhaType::Bool, "==" | "!=", AhaType::Bool) => Ok(AhaType::Bool),

            // Logical AND/OR: int/bool op int/bool → int (0 or 1)
            // Returns Int (not Bool) so logical results compose with
            // arithmetic like in C: (a && b) * weight.
            (AhaType::Int, "&&" | "||", AhaType::Int) => Ok(AhaType::Int),
            (AhaType::Bool, "&&" | "||", AhaType::Bool) => Ok(AhaType::Int),

            // Everything else is a type error
            _ => Err(format!(
                "Cannot apply operator '{}' to types {} and {}",
                op, self, other
            )),
        }
    }

    /// Check if a prefix operator is valid for this type
    pub fn check_prefix_op(&self, op: &str) -> Result<AhaType, String> {
        match (op, self) {
            ("-", AhaType::Int) => Ok(AhaType::Int),
            ("!", AhaType::Bool) => Ok(AhaType::Bool),
            ("!", AhaType::Int) => Ok(AhaType::Bool), // !0 = true, !nonzero = false
            _ => Err(format!(
                "Cannot apply prefix operator '{}' to type {}",
                op, self
            )),
        }
    }

    /// Parse a type hint string into an AhaType
    pub fn from_hint(hint: &str) -> Option<AhaType> {
        match hint {
            "int" | "i64" => Some(AhaType::Int),
            "bool" => Some(AhaType::Bool),
            "string" | "str" => Some(AhaType::String),
            "void" => Some(AhaType::Void),
            _ => None,
        }
    }
}

impl fmt::Display for AhaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AhaType::Int => write!(f, "Int"),
            AhaType::Bool => write!(f, "Bool"),
            AhaType::String => write!(f, "String"),
            AhaType::Void => write!(f, "Void"),
            AhaType::Array(inner) => write!(f, "[{}]", inner),
            AhaType::Function { params, ret } => {
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", p)?;
                }
                write!(f, ") -> {}", ret)
            }
        }
    }
}

/// A typed value — combines an LLVM value with its AHA! type information.
/// This is the core unit passed around during code generation.
///
/// The `value` field holds the LLVM IR value, and `aha_type` tracks
/// what AHA! type it represents, enabling type checking at compile time.
use inkwell::values::BasicValueEnum;

#[derive(Debug, Clone)]
pub struct TypedValue<'ctx> {
    pub value: BasicValueEnum<'ctx>,
    pub aha_type: AhaType,
}

impl<'ctx> TypedValue<'ctx> {
    pub fn new(value: BasicValueEnum<'ctx>, aha_type: AhaType) -> Self {
        TypedValue { value, aha_type }
    }

    pub fn int(value: BasicValueEnum<'ctx>) -> Self {
        TypedValue { value, aha_type: AhaType::Int }
    }

    pub fn bool_val(value: BasicValueEnum<'ctx>) -> Self {
        TypedValue { value, aha_type: AhaType::Bool }
    }

    pub fn string(value: BasicValueEnum<'ctx>) -> Self {
        TypedValue { value, aha_type: AhaType::String }
    }

    pub fn void(value: BasicValueEnum<'ctx>) -> Self {
        TypedValue { value, aha_type: AhaType::Void }
    }
}
