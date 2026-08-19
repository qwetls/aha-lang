# Changelog

All notable changes to AHA! Lang are documented in this file.

## [1.5.0] — 2026-08-19

### Added

- **Module system (`use`):** `use "file"` imports all functions and structs from another `.aha` file into the current scope. Recursively resolves imports with cycle detection. Path resolution: `"math"` → `math.aha` in the same directory.
- **Compiler orchestrator** (`src/compiler.rs`): multi-file compilation that parses imported files, merges their ASTs, and compiles everything as a single LLVM module.
- **New keyword:** `use` — 14th keyword in the language.
- **Test suite:** 10 new module tests covering single import, multiple imports, mutual recursion across files, struct imports, string functions, chain imports, semicolon syntax, and empty imports.

## [1.4.9] — 2026-08-19

### Fixed

- **Map<K,V> grow-on-load-factor:** `_set` now grows the hash table when `len >= cap`, preventing silent data loss. Previously the map was fixed at 4 slots — entries were silently dropped once full.
  - `init_alloc`: cap==0 → allocate 4 slots, zero occupied flags.
  - `grow_rehash`: len>=cap → allocate 2×cap, rehash all occupied entries into new buffer, free old buffer, update header.
  - `probe_block`: unchanged linear probing for insert/overwrite.
- **Rehash string pointer type casts:** rehash path loaded string keys/values through `i8*` pointers, returning `i8` (byte) instead of `i8*` (pointer). Fixed 6 cast sites: `i8_ptr` → `i8_ptr.ptr_type(...)` so `build_load` returns the correct `i8*` value. Fixed inkwell panic: `Found IntValue but expected PointerValue variant`.

### Commits

- `ca602b0` — grow-on-load-factor with rehash + free old buffer
- `7702c74` / `71a33ec` — rehash string pointer casts i8* → i8**

## [1.4.8] — 2026-08-18

### Fixed

- **Map<K,V> SIGSEGV (signal 11):** 5 bugs fixed in `codegen.rs` emit_map_combo — all 21 map tests now pass on CI.
  - Occupied flag access used `build_int_to_ptr` producing absolute addresses → fixed with `build_gep` on `i8*` data ptr + `build_pointer_cast` to `i64*`.
  - `store_val`/`load_val` used `build_gep(i64*, [byte_offset])` which multiplies by sizeof(i64)=8 → heap corruption. Fixed with `build_int_to_ptr(ptr_to_int(slot_base) + byte_offset, i64*)`.
  - `_set` probe loop started at counter=1, skipping the initial slot → overwrites silently failed. Fixed by adding `init_check` block with `key_cmp` before overflow loop.
  - `key_cmp` for String keys compared raw pointer addresses, not content → `memcmp` used instead.
  - Test function `length(s)` renamed to `len(s)` to match AHA! builtin.

### Commits

- `d8c7d0a` — store_val/load_val byte addressing with correct i64* type
- `7f02337` — overwrite probe initial slot check + String key memcmp
- `ad9bf1e` — test function name length → len
- `a0e56b5` — memcmp i64 len type fix

## [1.4.7] — 2026-08-17

### File-by-File Change Summary

| File | Status | Lines | What Changed |
|------|--------|------:|-------------|
| `src/codegen.rs` | 🔧 ENHANCED | +1005 | Map<K,V>: map_header_type, emit_map_combo (open addressing), 4 prefix combos, compile_map_call dispatch |
| `src/parser.rs` | 🔧 ENHANCED | +22 | Parse `Map<K,V>` type hint with comma separator |
| `src/types.rs` | 🔧 ENHANCED | +51 | `AhaType::Map(Box<AhaType>, Box<AhaType>)`, from_hint, unify_with, Display |
| `tests/maps.rs` | ✨ NEW | ~220 | 21 integration tests for Map<Int,Int>/Map<String,Int>/Map<Int,String>/Map<String,String> |

### Added

- **Map<K,V> (F3e):** deterministic hash table — open addressing / linear probing, `splitmix64` (Int keys) / FNV-1a (String keys) hashing.
- **Header struct:** `{data: i8*, len: i64, cap: i64, key_size: i64, val_size: i64}` — 40 bytes on heap.
- **4 prefix combos:** `map_` (Int→Int), `map_string_key_` (String→Int), `map_string_val_` (Int→String), `map_strings_` (String→String).
- **Builtins:** `map_new`, `map_set`, `map_get`, `map_contains`, `map_remove`, `map_len`, `map_free` — each combo has its own prefixed LLVM function.
- **Parser:** `Map<K,V>` type hint parsed via comma separator in `parse_type_hint`.
- **Type system:** `AhaType::Map(K,V)` with `from_hint`, `unify_with`, `Display`.

### Rangkaian Pengujian

- 21 test Map<K,V> di `tests/maps.rs` — all 4 combos covered (Int→Int, String→Int, Int→String, String→String).

## [1.4.6] — 2026-08-17

### File-by-File Change Summary

| File | Status | Lines | What Changed |
|------|--------|------:|-------------|
| `src/codegen.rs` | 🔧 ENHANCED | +817 | List<T>: builtins heap (malloc/realloc/free), index read/write, monomorphization atas List, scan pass track list bindings, main return i64 untuk hasil non-int |
| `src/parser.rs` | 🔧 ENHANCED | +33 | Parsing `List<T>` type hint & list bindings |
| `src/types.rs` | 🔧 ENHANCED | +19 | `AhaType::List(Box<AhaType>)` |
| `tests/lists.rs` | ✨ NEW | 346 | 20+ test List<Int>/List<String>/List<T> generik/IR shape |

### Added

- **List<T> (F3e):** heap-allocated dynamic array — `list_new()`, `list_new_string()`, `list_push`, `list_get`, `list_get_string`, `list_len`, `list_free`, index read `xs[i]` dan write `xs[i] = v`.
- **List<String> penuh:** element struct `{i8*, i64}` — push/get/index/concat/len.
- **Fungsi generik atas List:** `fn first<T>(xs: List<T>) -> T { xs[0] }` — type param T ter-bind dari hint `List<T>`; monomorphization `first_Int`/`first_String`.

### Diubah

- Scan pass (`scan_call_sites`) kini men-track binding `let xs = list_new()` → `List<Int>` dan `list_new_string()` → `List<String>`, sehingga param fungsi (`fn sum_list(xs) { list_get(xs, 0) + ... }`) ter-infer sebagai list, bukan Int.
- Main entry point selalu return i64: jika last expression bertipe String/struct, main return 0 — memperbaiki verify abort `ret { i8*, i64 } %listidx / i64` (SIGABRT).

### Rangkaian Pengujian

- 440 tests passing di CI (`experimental/list`) — termasuk 20+ test `lists.rs`.

## [1.4.5] — 2026-08-16

### File-by-File Change Summary

| File | Status | Lines | What Changed |
|------|--------|------:|-------------|
| `src/ast.rs` | 🔧 ENHANCED | +8 | `TokenType::Arrow`; `FunctionLiteral` + `type_params`, `param_type_hints`, `return_type_hint` |
| `src/lexer.rs` | 🔧 ENHANCED | +7 | Lex `->` sebagai `Arrow` |
| `src/parser.rs` | 🔧 ENHANCED | +50 | Parse `<T, U>` type params, `a: T` param hints, `-> T` return hint |
| `src/codegen.rs` | 🔧 ENHANCED | +160 | Monomorphization: `generic_defs`, `type_param_map`, `resolve_hint_type`, `compile_generic_call`, `infer_generic_return_type` |
| `tests/generics.rs` | ✨ NEW | 170 | 13 test fungsi generik & monomorphization |

### Added

- **Generic functions (F3):** `fn pick<T>(a: T, b: T) -> T { if a > b { a } else { b } }` — fungsi dengan tipe parameter generik.
- **Monomorphization via LLVM:** Setiap kombinasi unik (nama generik, tipe konkret) menghasilkan fungsi LLVM terpisah (`pick_Int`, `pick_String`, ...), dikompilasi lazy di call site pertama dan di-cache. Tidak ada runtime cost — generics sepenuhnya resolve di compile time.
- **Type params:** `fn first<A, B>(a: A, b: B) -> A` — banyak tipe parameter; binding konkret di-infer dari argumen call site.
- **Return type annotation:** `-> T` dan `-> int` — return type generik atau konkret.
- **Nested monomorphization:** Fungsi generik bisa memanggil fungsi generik lain (`fn twice<U>(x: U) -> U { id(x) }`).
- **Rangkaian Pengujian:** 417 tests passing (sebelumnya 404; +13 test generics: identity int/string/bool/struct, pick, dua type params, nested call, IR shape).

### Diubah

- `predeclare_functions` menyimpan fungsi generik di `generic_defs` (bukan pre-declare langsung); body dikompilasi saat monomorphization.
- `compile_function` me-skip body fungsi generik di top-level (hanya dipanggil via instantiation).
- Fixpoint loop me-skip fungsi generik (return type hanya ada per instantiation).

### Security

- Generics tidak menambah jalur unsafe: semua tipe masih diverifikasi compile-time; monomorphization murni penyalinan codegen per tipe.

## [1.4.4] — 2026-08-16

### File-by-File Change Summary

| File | Status | Lines | What Changed |
|------|--------|------:|-------------|
| `src/ast.rs` | 🔧 ENHANCED | +3 | `LetStatement.type_annotation: Option<String>` (raw hint seperti `int`, `string`, nama struct) |
| `src/parser.rs` | 🔧 ENHANCED | +12 | Parsing `let x: int = 5` — `:` lalu identifier hint sebelum `=` |
| `src/codegen.rs` | 🔧 ENHANCED | +51 | Type-check annotation vs nilai; alloca pakai tipe annotation; inferensi return type (String/struct/if); phi node pakai tipe branch |
| `tests/type_inference.rs` | ✨ NEW | 170 | 20 test type annotation & inferensi |

### Added

- **Type annotations (F2):** `let x: int = 5` — deklarasi variabel bisa diberi anotasi tipe eksplisit (`int`, `string`, `bool`, atau nama struct). Nilai di-*type-check* saat kompilasi:
  - `let x: int = "hi"` → error `Type mismatch: variable 'x' annotated as 'int' but value has type 'String'`
  - `let p: Point = Other { ... }` → error bila nama struct berbeda
- **Type inference (F2):** Tipe variabel tanpa anotasi di-infer dari ekspresi (literal, panggilan fungsi, if-expression). Return type fungsi di-infer dari ekspresi terakhir (string, struct, cabang if).
- **Return type inference:** `fn greet() { "hello" }` ber-return type String — caller bisa `let s = greet(); len(s)`.
- **If-expression phi type:** Phi node kini memakai tipe LLVM dari cabang (String/struct), bukan selalu i64 — memperbaiki `fn pick(a) { if a > 0 { "pos" } else { "neg" } }`.
- **Rangkaian Pengujian:** 404 tests passing (sebelumnya 384; +20 test type inference/annotations).

### Diubah

- `infer_expr_type` & `infer_expr_type_with_scope` mendukung anotasi dan inferensi return type.
- Unknown type hint (`let x: unknown = 7`) bersifat lenient — fallback ke Int, konsisten dengan field hints.

### Security

- Anotasi tipe diverifikasi saat kompilasi: mencocokkan anotasi dengan tipe nilai menghasilkan error, bukan UB.

## [1.4.3] — 2026-08-16

### File-by-File Change Summary

| File | Status | Lines | What Changed |
|------|--------|------:|-------------|
| `src/types.rs` | 🔧 ENHANCED | +14 | `unify_with()` — merge param types from multiple call sites (String, struct names) |
| `src/codegen.rs` | 🔧 ENHANCED | +100 | `aha_type_to_llvm_type()` helper; `predeclare_functions` & `compile_function` support struct params/returns; `infer_expr_type` handles StructLiteral/FieldAccess; `scan_call_sites` tracks struct var bindings |
| `tests/struct_codegen.rs` | 🔧 ENHANCED | +80 | 11 test struct sebagai param & return value |

### Added

- **Struct sebagai parameter fungsi (F1):** Struct dapat dioper by-value ke fungsi. Parameter dialokasikan dengan tipe LLVM struct yang benar; field access di dalam fungsi bekerja normal.
- **Struct sebagai return value fungsi (F1):** Fungsi dapat mengembalikan struct literal. Caller menyimpan hasilnya ke variabel `let` dan membaca field-nya.
- **Struct literal langsung sebagai argumen:** `sum(Point { x: 1, y: 2 })` — tanpa variabel perantara.
- **Rantai fungsi struct:** `sum(make(20, 22))` — return value struct langsung dioper ke fungsi lain.
- **`unify_with()` pada AhaType:** Menyatukan tipe parameter dari beberapa call site (String, struct name meng-override Int default).
- **Rangkaian Pengujian:** 384 tests passing (sebelumnya 373; +11 test struct param/return).

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
