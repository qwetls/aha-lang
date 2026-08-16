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
    /// Heap-allocated dynamic list: List<T> — handle is an i64 pointer
    /// to a header struct {data: i8*, len: i64, cap: i64, elem_size: i64}.
    List(Box<AhaType>),
    /// Hash table: Map<K,V> — handle is an i64 pointer to a header struct
    /// {data: i8*, len: i64, cap: i64, key_size: i64, val_size: i64}.
    /// Open addressing / linear probing, deterministic FNV-1a/splitmix64
    /// hashing (PRD: determinisme mutlak untuk aerospace).
    Map(Box<AhaType>, Box<AhaType>),
    /// Named struct — carries the struct's declared name so codegen can
    /// look up its field layout and LLVM struct type.
    Struct(String),
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

            // Comparison: int op int → int (0 or 1)
            // Returns Int (not Bool) so comparison results compose with
            // arithmetic like in C: (a == b) * weight.
            (AhaType::Int, "==" | "!=" | "<" | ">" | "<=" | ">=", AhaType::Int) => Ok(AhaType::Int),

            // String comparison: string == string → int (0 or 1)
            (AhaType::String, "==" | "!=", AhaType::String) => Ok(AhaType::Int),

            // Bool comparison: bool == bool → int (0 or 1)
            (AhaType::Bool, "==" | "!=", AhaType::Bool) => Ok(AhaType::Int),

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
            _ => {
                // List<T> — parse the inner type.
                if let Some(inner) = hint.strip_prefix("List<").and_then(|s| s.strip_suffix('>')) {
                    let inner_type = match inner {
                        "int" | "i64" => AhaType::Int,
                        "bool" => AhaType::Bool,
                        "string" | "str" => AhaType::String,
                        // Nested List<U> inside List<T>.
                        _ => Self::from_hint(inner)?,
                    };
                    return Some(AhaType::List(Box::new(inner_type)));
                }
                // Map<K,V> — parse two comma-separated inner types.
                if let Some(inner) = hint.strip_prefix("Map<").and_then(|s| s.strip_suffix('>')) {
                    let (key, value) = inner.split_once(',')?;
                    let key_type = match key.trim() {
                        "int" | "i64" => AhaType::Int,
                        "bool" => AhaType::Bool,
                        "string" | "str" => AhaType::String,
                        _ => Self::from_hint(key.trim())?,
                    };
                    let value_type = match value.trim() {
                        "int" | "i64" => AhaType::Int,
                        "bool" => AhaType::Bool,
                        "string" | "str" => AhaType::String,
                        _ => Self::from_hint(value.trim())?,
                    };
                    return Some(AhaType::Map(Box::new(key_type), Box::new(value_type)));
                }
                None
            }
        }
    }

    /// Merge a newly inferred type into an existing one when multiple
    /// call sites disagree. Used by scan_expr_for_calls to narrow a
    /// param's type: String and named structs are kept when observed,
    /// Int stays as the default. Two different struct names never meet
    /// (each call site's arg type wins for its position).
    pub fn unify_with(&self, other: &AhaType) -> AhaType {
        match (self, other) {
            (AhaType::Int, t) => t.clone(),
            // Map<K,V> unifies key/value independently: Int defaults upgrade
            // to observed types (String, ...), matching List<T> semantics.
            (AhaType::Map(k1, v1), AhaType::Map(k2, v2)) => AhaType::Map(
                Box::new(k1.unify_with(k2)),
                Box::new(v1.unify_with(v2)),
            ),
            (_, _) => self.clone(),
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
            AhaType::List(inner) => write!(f, "List<{}>", inner),
            AhaType::Map(key, value) => write!(f, "Map<{}, {}>", key, value),
            AhaType::Struct(name) => write!(f, "{}", name),
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

    pub fn struct_val(value: BasicValueEnum<'ctx>, name: String) -> Self {
        TypedValue { value, aha_type: AhaType::Struct(name) }
    }

    pub fn void(value: BasicValueEnum<'ctx>) -> Self {
        TypedValue { value, aha_type: AhaType::Void }
    }
}
