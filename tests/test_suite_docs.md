# AHA! Lang — Test Suite Documentation

## How to Run

```bash
# Run ALL tests
cargo test

# Run specific test suite
cargo test --test lexer_tests
cargo test --test parser_tests
cargo test --test types_tests
cargo test --test integration_tests

# Run single test
cargo test --test lexer_tests test_not_eq_c01_fix

# Verbose output (see pass/fail for each)
cargo test -- --nocapture
```

## Test File Structure

```
tests/
├── lexer_tests.rs        — 19 tests (tokenization)
├── parser_tests.rs        — 22 tests (AST generation)
├── types_tests.rs         — 18 tests (type checking logic)
└── integration_tests.rs   — 25 tests (full pipeline: source → JIT result)
```

## Test Coverage Matrix

### `lexer_tests.rs` — 19 Tests

| Test | What it Verifies | Audit Fix |
|------|-----------------|-----------|
| `test_single_char_operators` | `+ - * / = < > !` | — |
| `test_two_char_operators` | `== != <= >= ..` | M-06 |
| `test_not_eq_c01_fix` | `!=` literal is `"!="` not `"=="` | **C-01** |
| `test_delimiters` | `( ) { } [ ] , ; :` | — |
| `test_keywords` | All 13 keywords recognized | — |
| `test_identifier_simple` | Basic identifiers | — |
| `test_identifier_with_digits` | `my_var2`, `x1` | **M-01** |
| `test_identifier_with_underscore_prefix` | `_private` | Bonus |
| `test_integers` | `0`, `42`, `12345` | — |
| `test_string_simple` | `"hello world"` | — |
| `test_string_escape_sequences` | `\n`, `\t`, `\\`, `\"` | **M-02** |
| `test_single_line_comment` | `// comment` | — |
| `test_multi_line_comment` | `/* ... */` | **M-03** |
| `test_let_statement_tokens` | `let x = 42;` | — |
| `test_function_tokens` | `fn add(a, b) { ... }` | — |
| `test_for_loop_tokens` | `for i in 0..10` | — |
| `test_empty_input` | Empty string → no tokens | — |
| `test_eof_token` | EOF after last token | — |

### `parser_tests.rs` — 22 Tests

| Test | What it Verifies | Audit Fix |
|------|-----------------|-----------|
| `test_let_integer` | `let x = 42;` → LetStatement | — |
| `test_let_string` | `let name = "hello";` | — |
| `test_let_boolean` | `let flag = true;` | — |
| `test_return_expression` | `return 42;` | — |
| `test_infix_addition` | `1 + 2` → InfixExpression | — |
| `test_operator_precedence` | `2+3*4` = `2+(3*4)` | — |
| `test_comparison_operators` | All 6 comparison ops | — |
| `test_prefix_negation` | `-5` | — |
| `test_prefix_not` | `!true` | — |
| `test_if_expression` | `if x > 5 { 10 }` | — |
| `test_if_else_expression` | `if/else` with alternative | — |
| `test_function_definition` | `fn add(a, b) { ... }` | **H-01** |
| `test_function_no_params` | `fn hello() { 42 }` | H-01 |
| `test_while_loop` | `while x > 0 { x }` | — |
| `test_for_loop` | `for i in 0..10 { i }` | — |
| `test_break_expression` | `break` | **H-05** |
| `test_continue_expression` | `continue` | **H-05** |
| `test_assignment` | `x = 42` | **H-06** |
| `test_array_literal` | `[1, 2, 3]` | — |
| `test_index_expression` | `arr[0]` | — |
| `test_multiple_statements` | 3 statements parsed | — |
| `test_nested_if` | Nested if/else | — |
| `test_function_call` | `add(1, 2)` | — |

### `types_tests.rs` — 18 Tests

| Test | What it Verifies | Audit Fix |
|------|-----------------|-----------|
| `test_type_display` | Format `Int`, `Bool`, `[Int]` | **M-05** |
| `test_type_predicates` | `is_int()`, `is_string()`, etc. | M-05 |
| `test_int_arithmetic_valid` | `Int + - * / Int = Int` | M-05 |
| `test_int_comparison_valid` | `Int == != < > Int = Bool` | M-05 |
| `test_string_concat_valid` | `String + String = String` | **H-07** |
| `test_string_comparison_valid` | `String == != String = Bool` | H-07 |
| `test_bool_comparison_valid` | `Bool == != Bool = Bool` | M-05 |
| `test_int_plus_string_error` | `Int + String → ERROR` | **M-05** |
| `test_string_minus_string_error` | `String - String → ERROR` | M-05 |
| `test_bool_plus_bool_error` | `Bool + Bool → ERROR` | M-05 |
| `test_string_less_than_error` | `String < String → ERROR` | M-05 |
| `test_int_eq_string_error` | `Int == String → ERROR` | M-05 |
| `test_prefix_negate_int` | `-Int = Int` | M-05 |
| `test_prefix_not_bool` | `!Bool = Bool` | M-05 |
| `test_prefix_not_int` | `!Int = Bool` | M-05 |
| `test_prefix_negate_string_error` | `-String → ERROR` | M-05 |
| `test_from_hint_valid` | `"int"` → `AhaType::Int` | M-05 |
| `test_type_equality` | `Int == Int`, `Int != String` | M-05 |

### `integration_tests.rs` — 25 Tests

| Test | What it Verifies | Audit Fix |
|------|-----------------|-----------|
| `test_integer_literal` | `42` → 42 | — |
| `test_addition` | `10 + 20` → 30 | — |
| `test_subtraction` | `50 - 30` → 20 | — |
| `test_multiplication` | `6 * 7` → 42 | — |
| `test_division` | `100 / 4` → 25 | — |
| `test_complex_arithmetic` | `2 + 3 * 4` → 14 | — |
| `test_true/false_value` | Booleans | — |
| `test_equality/not_equal` | `==`, `!=` | **C-01** |
| `test_less/greater` | `<`, `>`, `<=`, `>=` | — |
| `test_negation` | `-42` → -42 | **H-03** |
| `test_logical_not` | `!false` → 1 | H-03 |
| `test_let_and_use` | Variables | — |
| `test_if_true/false_branch` | Conditional | **C-02, C-03** |
| `test_function_call_simple` | `fn double(x)` | **H-01** |
| `test_function_with_return` | `return x + 1` | **C-05** |
| `test_while_loop_basic` | While sum 1..5 | — |
| `test_for_loop_sum` | For sum 0..5 | **H-02** |
| `test_block_scoping` | Inner `let x` doesn't leak | **H-04** |
| `test_abs/min/max` | Stdlib builtins | — |
| `test_type_error_*` (3) | Type errors at compile time | **M-05** |

## Total: 84 Tests across 4 suites
