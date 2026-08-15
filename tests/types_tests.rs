// tests/types_tests.rs
//
// Unit tests for the AHA! Type System module

use aha_lang::types::AhaType;

// =====================================================================
// Type Display
// =====================================================================

#[test]
fn test_type_display() {
    assert_eq!(format!("{}", AhaType::Int), "Int");
    assert_eq!(format!("{}", AhaType::Bool), "Bool");
    assert_eq!(format!("{}", AhaType::String), "String");
    assert_eq!(format!("{}", AhaType::Void), "Void");
    assert_eq!(format!("{}", AhaType::Array(Box::new(AhaType::Int))), "[Int]");
}

// =====================================================================
// Type Predicates
// =====================================================================

#[test]
fn test_type_predicates() {
    assert!(AhaType::Int.is_int());
    assert!(!AhaType::Int.is_string());
    assert!(AhaType::Bool.is_bool());
    assert!(AhaType::String.is_string());
    assert!(AhaType::Void.is_void());
    assert!(AhaType::Int.is_numeric());
    assert!(AhaType::Bool.is_numeric());
    assert!(!AhaType::String.is_numeric());
}

// =====================================================================
// Binary Operator Type Checking
// =====================================================================

#[test]
fn test_int_arithmetic_valid() {
    assert_eq!(AhaType::Int.check_binary_op("+", &AhaType::Int).unwrap(), AhaType::Int);
    assert_eq!(AhaType::Int.check_binary_op("-", &AhaType::Int).unwrap(), AhaType::Int);
    assert_eq!(AhaType::Int.check_binary_op("*", &AhaType::Int).unwrap(), AhaType::Int);
    assert_eq!(AhaType::Int.check_binary_op("/", &AhaType::Int).unwrap(), AhaType::Int);
}

#[test]
fn test_int_comparison_valid() {
    assert_eq!(AhaType::Int.check_binary_op("==", &AhaType::Int).unwrap(), AhaType::Int);
    assert_eq!(AhaType::Int.check_binary_op("!=", &AhaType::Int).unwrap(), AhaType::Int);
    assert_eq!(AhaType::Int.check_binary_op("<", &AhaType::Int).unwrap(), AhaType::Int);
    assert_eq!(AhaType::Int.check_binary_op(">", &AhaType::Int).unwrap(), AhaType::Int);
    assert_eq!(AhaType::Int.check_binary_op("<=", &AhaType::Int).unwrap(), AhaType::Int);
    assert_eq!(AhaType::Int.check_binary_op(">=", &AhaType::Int).unwrap(), AhaType::Int);
}

#[test]
fn test_string_concat_valid() {
    assert_eq!(AhaType::String.check_binary_op("+", &AhaType::String).unwrap(), AhaType::String);
}

#[test]
fn test_string_comparison_valid() {
    assert_eq!(AhaType::String.check_binary_op("==", &AhaType::String).unwrap(), AhaType::Int);
    assert_eq!(AhaType::String.check_binary_op("!=", &AhaType::String).unwrap(), AhaType::Int);
}

#[test]
fn test_bool_comparison_valid() {
    assert_eq!(AhaType::Bool.check_binary_op("==", &AhaType::Bool).unwrap(), AhaType::Int);
    assert_eq!(AhaType::Bool.check_binary_op("!=", &AhaType::Bool).unwrap(), AhaType::Int);
}

// =====================================================================
// Type Error Detection (THE WHOLE POINT)
// =====================================================================

#[test]
fn test_int_plus_string_error() {
    let result = AhaType::Int.check_binary_op("+", &AhaType::String);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Cannot apply"));
}

#[test]
fn test_string_minus_string_error() {
    let result = AhaType::String.check_binary_op("-", &AhaType::String);
    assert!(result.is_err());
}

#[test]
fn test_bool_plus_bool_error() {
    let result = AhaType::Bool.check_binary_op("+", &AhaType::Bool);
    assert!(result.is_err());
}

#[test]
fn test_string_less_than_string_error() {
    // String ordering is not supported
    let result = AhaType::String.check_binary_op("<", &AhaType::String);
    assert!(result.is_err());
}

#[test]
fn test_int_eq_string_error() {
    let result = AhaType::Int.check_binary_op("==", &AhaType::String);
    assert!(result.is_err());
}

// =====================================================================
// Prefix Operator Type Checking
// =====================================================================

#[test]
fn test_prefix_negate_int() {
    assert_eq!(AhaType::Int.check_prefix_op("-").unwrap(), AhaType::Int);
}

#[test]
fn test_prefix_not_bool() {
    assert_eq!(AhaType::Bool.check_prefix_op("!").unwrap(), AhaType::Bool);
}

#[test]
fn test_prefix_not_int() {
    // !0 = true, !nonzero = false — this is valid
    assert_eq!(AhaType::Int.check_prefix_op("!").unwrap(), AhaType::Bool);
}

#[test]
fn test_prefix_negate_string_error() {
    let result = AhaType::String.check_prefix_op("-");
    assert!(result.is_err());
}

#[test]
fn test_prefix_not_string_error() {
    let result = AhaType::String.check_prefix_op("!");
    assert!(result.is_err());
}

// =====================================================================
// Type Hint Parsing
// =====================================================================

#[test]
fn test_from_hint_valid() {
    assert_eq!(AhaType::from_hint("int"), Some(AhaType::Int));
    assert_eq!(AhaType::from_hint("i64"), Some(AhaType::Int));
    assert_eq!(AhaType::from_hint("bool"), Some(AhaType::Bool));
    assert_eq!(AhaType::from_hint("string"), Some(AhaType::String));
    assert_eq!(AhaType::from_hint("str"), Some(AhaType::String));
    assert_eq!(AhaType::from_hint("void"), Some(AhaType::Void));
}

#[test]
fn test_from_hint_unknown() {
    assert_eq!(AhaType::from_hint("float"), None);
    assert_eq!(AhaType::from_hint("char"), None);
}

// =====================================================================
// Type Equality
// =====================================================================

#[test]
fn test_type_equality() {
    assert_eq!(AhaType::Int, AhaType::Int);
    assert_ne!(AhaType::Int, AhaType::String);
    assert_eq!(
        AhaType::Array(Box::new(AhaType::Int)),
        AhaType::Array(Box::new(AhaType::Int))
    );
    assert_ne!(
        AhaType::Array(Box::new(AhaType::Int)),
        AhaType::Array(Box::new(AhaType::String))
    );
}
