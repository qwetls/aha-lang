# Changelog

All notable changes to AHA! Lang are documented in this file.

## [1.4.3] — 2026-08-16

### File-by-File Change Summary

| File | Status | Lines | What Changed |
|------|--------|------:|-------------|
| `src/types.rs` | 🔧 ENHANCED | +14 | `unify_with()` — merge param types from multiple call sites (String, struct names) |
| `src/codegen.rs` | 🔧 ENHANCED | +100 | `aha_type_to_llvm_type()` helper; `predeclare_functions` & `compile_function` support struct params/returns; `infer_expr_type` handles StructLiteral/FieldAccess; `scan_call_sites` tracks struct var bindings |
| `tests/struct_codegen.rs` | 🔧 ENHANCED | +90 | 12 test struct sebagai param & return value |

### Added

- **Struct sebagai parameter fungsi (F1):** Struct dapat dioper by-value ke fungsi. Parameter dialokasikan dengan tipe LLVM struct yang benar; field access di dalam fungsi bekerja normal.
- **Struct sebagai return value fungsi (F1):** Fungsi dapat mengembalikan struct literal. Caller menyimpan hasilnya ke variabel `let` dan membaca field-nya.
- **Struct literal langsung sebagai argumen:** `sum(Point { x: 1, y: 2 })` — tanpa variabel perantara.
- **Rantai fungsi struct:** `sum(make(20, 22))` — return value struct langsung dioper ke fungsi lain.
- **`unify_with()` pada AhaType:** Menyatukan tipe parameter dari beberapa call site (String, struct name meng-override Int default).
- **Rangkaian Pengujian:** 385 tests passing (sebelumnya 373; +12 test struct param/return).

### Diubah

- `infer_expr_type` & `infer_expr_type_with_scope` kini mengenali `StructLiteral` dan `FieldAccess` untuk inferensi tipe yang akurat saat pre-pass.
- `scan_call_sites` melacak binding variabel struct (`struct_var_types`) agar inferensi tipe bisa resolve struct variable sebagai argumen fungsi.

## [1.4.2] — 2026-08-16

### File-by-File Change Summary

| File | Status | Lines | What Changed |
|------|--------|------:|-------------|
| `src/ast.rs` | 🔧 ENHANCED | +3 | `AssignmentExpression.name` → `target: Box<Expression>` (generic) |
| `src/parser.rs` | 🔧 ENHANCED | +12 | `=` sebagai infix operator; `Assign` precedence; target bisa Identifier atau FieldAccess |
| `src/codegen.rs` | 🔧 ENHANCED | +60 | `compile_assignment` handle FieldAccess target (load → insertvalue → store); type-check; scan target |
| `tests/struct_codegen.rs` | 🔧 ENHANCED | +70 | 10 test mutasi field (int/string, loop, error paths, regresi) |

### Added

- **Field mutation (F1):** `p.x = value` — struct field assignment at runtime. `p.x` is now an lvalue: load the struct, `insertvalue` the new field, store back. Type-checked against the field's declared type (`string` field rejects int values and vice versa).
- **Generic assignment target:** The parser now treats `=` as an infix operator, so both `x = 5` (identifier) and `p.x = 5` (field access) parse to `AssignmentExpression` with a generic `target` expression.
- **Test suite:** 373 tests passing (was 363; +10 field-mutation tests, including mutation inside loops and string field reassignment).

### Changed

- `AssignmentExpression.name: Identifier` → `AssignmentExpression.target: Box<Expression>` — enables future lvalue forms (e.g. array element assignment `arr[0] = x`).

### Fixed

- `TokenType::Assign` was missing from `precedence()`, so `=` fell through to `parse_prefix` as an unexpected token. Added `Precedence::Assign`.

### Security

- Assigning a value of the wrong type to a typed field produces a compile-time error.

## [1.4.1] — 2026-08-16

### File-by-File Change Summary

| File | Status | Lines | What Changed |
|------|--------|------:|-------------|
| `src/types.rs` | 🔧 ENHANCED | +5 | `AhaType::Struct(name)` variant, `Display`, `TypedValue::struct_val()` |
| `src/parser.rs` | 🔧 ENHANCED | +60 | `struct_names` registry, `parse_struct_literal()`, `StructLiteral` import |
| `src/codegen.rs` | 🔧 ENHANCED | +130 | `struct_defs` registry, `struct_llvm_type()`, `field_index()`, `field_type()`, literal type-check, typed field access, `scan_expr_for_calls` descends into struct literals |
| `tests/struct_codegen.rs` | ✨ NEW | ~320 | 28 backend tests: JIT semantics, IR shape, typed fields, error paths |
| `PRD.md` | ✨ NEW | 225 | Product Requirements Document v0.2 |

### Added

- **Struct codegen (Roadmap Phase 2 #1):** `struct Point { x, y }` definitions now produce LLVM struct types. Literals `Point { x: 1, y: 2 }` build the aggregate via `insertvalue`; field access `p.x` reads via `extractvalue`.
- **Typed struct fields:** Field type hints (`name: string, age: int`) are honored at runtime — `string` fields use `{i8*, i64}` layout, `int` fields use `i64`. Literals are type-checked against declarations. Field access preserves the declared type, so `len(p.name)`, `p.name == "..."`, and `p.first + p.last` work correctly.
- **Struct literal parsing:** `TypeName { field: value, ... }` syntax, gated by a struct-name registry so ordinary block conditions `if x { ... }` are unaffected.
- **PRD v0.2:** Visi besar: web → aerospace; "Advanced Hybrid Architecture" dijelaskan; F5 (Resource lifetimes) di-freeze sampai F1-F4 stabil.
- **Test suite (struct):** 28 backend tests covering:
  - Single/multiple field read, field order independence, missing field defaults to zero
  - Field values from variables, expressions, arithmetic chains
  - Structs interacting with control flow (if conditions, loops)
  - Multiple instances and distinct struct types
  - Typed string fields (len, concat, equality, default empty string)
  - IR shape verification (insertvalue, extractvalue)
  - Error paths (unknown field, non-struct field access, wrong type)

### Changed

- **Branch workflow:** All development now happens on `development` branch; `main` only receives PRs with green CI. This is enforced as a project-wide rule.
- **PRD replaces ad-hoc planning:** All future features must originate from the roadmap in PRD.md.

### Security

- Struct field types are checked at compile time: assigning a string literal to an `int` field (or vice versa) produces a compile-time error instead of undefined behavior.

## [1.4.0] — 2026-05-17

### File-by-File Change Summary

| File | Status | Lines | What Changed |
|------|--------|------:|-------------|
| `src/types.rs` | ✨ NEW | 155 | Type system: AhaType, TypedValue, check_binary_op, check_prefix_op |
| `src/codegen.rs` | 🔨 REWRITE | 840 | VarInfo scope, TypedValue returns, string struct, C runtime, compile_infix, string ops, len builtin |
| `src/parser.rs` | 🔧 REFACTOR | 579 | `r#type`→`kind` (12 sites), removed "ERROR" identifiers, added call/fn parsing |
| `src/ast.rs` | 🔧 REFACTOR | 251 | `r#type`→`kind`, added Assignment/Break/Continue nodes |
| `src/lexer.rs` | 🔧 ENHANCED | 249 | Fixed != literal, added block comments, digit identifiers, escape sequences |
| `src/main.rs` | 📝 UPDATED | 81 | All messages to English |
| `src/lib.rs` | 📝 UPDATED | 14 | Added `pub mod types` + re-exports |
| `Cargo.toml` | 📝 UPDATED | 8 | Comment to English |
| `README.md` | 📝 UPDATED | 203 | Roadmap accuracy, expected output updated |
| `tests/lexer_tests.rs` | ✨ NEW | ~200 | 19 lexer tests |
| `tests/parser_tests.rs` | ✨ NEW | ~300 | 22 parser tests |
| `tests/types_tests.rs` | ✨ NEW | ~170 | 18 type system tests |
| `tests/integration_tests.rs` | ✨ NEW | ~230 | 25 end-to-end JIT tests |


### Added
- **Type System** (`src/types.rs`): `AhaType` enum (`Int`, `Bool`, `String`, `Void`, `Array`, `Function`) with compile-time type checking via `check_binary_op()` and `check_prefix_op()`
- **TypedValue**: All codegen expression methods now return `TypedValue<'ctx>` — a struct pairing LLVM `BasicValueEnum` with `AhaType` information
- **String Type**: Strings are now LLVM struct `{i8*, i64}` (pointer + length), replacing the unsafe pointer-to-int cast
- **String Concatenation**: `"hello" + " world"` allocates new buffer via `malloc`, copies via `memcpy`, null-terminates
- **String Comparison**: `==` and `!=` on strings use `strcmp` from C standard library
- **`len()` builtin**: Takes a string, returns its length in O(1) by reading the struct's length field
- **C Runtime Linkage**: External declarations for `malloc`, `memcpy`, `strlen`, `strcmp`
- **Block Comments**: `/* ... */` multi-line comment syntax via `skip_block_comment()` in lexer
- **String Escape Sequences**: `\n`, `\t`, `\\`, `\"`, `\r`, `\0` in string literals
- **Identifier Improvements**: Digits allowed after first character (`my_var2`), underscore prefix (`_private`)
- **Test Suite**: 84 tests across 4 modules:
  - `tests/lexer_tests.rs` — 19 tests (tokenization)
  - `tests/parser_tests.rs` — 22 tests (AST generation)
  - `tests/types_tests.rs` — 18 tests (type checking logic)
  - `tests/integration_tests.rs` — 25 tests (full compile → JIT pipeline)

### Changed
- `Token.r#type` renamed to `Token.kind` — eliminates raw identifier syntax, follows Rust convention
- Variable scope: flat `HashMap<String, PointerValue>` → stack `Vec<HashMap<String, VarInfo>>` with type tracking
- `compile_expression()` return type: `BasicValueEnum` → `TypedValue` throughout all 14 expression handlers
- `print_str()` builtin: now accepts string struct `{i8*, i64}` instead of `i64`
- All output messages in `main.rs` standardized to English
- All source code comments across all files standardized to English
- README.md roadmap updated to accurately reflect implementation status (honest about parser-only features)

### Fixed
- **C-01**: `!=` operator produced literal `"=="` instead of `"!="` (copy-paste error in lexer)
- **C-02**: `if` conditions not converted from `i64` to `i1` before `build_conditional_branch`
- **C-03**: Phi nodes referenced original basic blocks instead of actual end blocks after nested codegen
- **C-04**: Last expression in program body was compiled twice (once in loop, once for return)
- **C-05**: Functions emitted both implicit and explicit return, causing LLVM "multiple terminators" error
- **C-06**: Function compilation could leak scope/builder state on error (now uses `std::mem::replace` + closure safety pattern)
- **M-04**: Parser returned `Expression::Identifier("ERROR")` on parse failure, propagating invalid AST to codegen

### Security
- Type mismatches (`"hello" + 5`) now produce compile-time errors instead of undefined runtime behavior
- All `.unwrap()` calls replaced with `.expect("descriptive context")` for debuggable panics

## [1.3.0] - Previous Release
- Initial compiler with lexer, parser, codegen
- Integer, boolean, string (as pointer hack) types
- If/else, while, for loops
- Functions with parameters
- Basic stdlib: print, print_str, abs, min, max
