# AHA! Lang

<div align="center">

<img src="assets/logo.png" alt="AHA! Lang Logo" width="200">

**A**dvanced **H**ybrid **A**rchitecture

**Easy to read. Powerful to wield.**

A modern programming language with an LLVM backend — designed to be understood at a glance, yet strong enough to build real software.

[![CI/CD](https://github.com/qwetls/aha-lang/actions/workflows/ci.yml/badge.svg)](https://github.com/qwetls/aha-lang/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org)
[![Tests](https://img.shields.io/badge/tests-336%20passing-brightgreen.svg)](https://github.com/qwetls/aha-lang/actions)

</div>

---

## ✨ Key Features

AHA! is built on a simple belief: a language should feel **obvious** when you read it, and **effortless** when you run it. No magic, no surprises — just tools that work the way you expect.

- **⚡ LLVM-Powered:** Compiles source to LLVM IR and executes it through a built-in JIT — native-level performance from day one.
- **🧠 Expressive Type Discipline:** First-class `Int`, `Bool`, and `String` types with a real type-checking pass. Type errors are caught at compile time, not at runtime.
- **🔢 Boolean Algebra That Composes:** All boolean-producing operators (`==`, `!=`, `<`, `>`, `<=`, `>=`, `&&`, `||`) return `Int` `0`/`1` — so logic results flow straight into arithmetic: `is_even(n) * 100` just works.
- **📦 Strings Done Right:** Strings are a real `{pointer, length}` struct — safe concatenation, `==`/`!=` comparison, and an O(1) `len()` builtin.
- **🔁 Modern Control Flow:** `if`/`else`, `while`, and `for` loops with `break`/`continue`, functions with parameters, `return`, forward references, and mutual recursion.
- **🔗 Module System:** `use "file"` imports functions and structs from another `.aha` file — recursive resolution, cycle detection, zero config.
- **🛠️ Honest Tooling:** A clean CLI (`--file`, `--emit-ir`, `--version`), a VS Code syntax-highlighting extension, and a CI pipeline that runs 336+ tests on every commit.

---

## 🚀 Quick Start

### Prerequisites

- **Rust** (stable toolchain)
- **LLVM 14** with Clang 14 and Polly

#### Ubuntu / Debian

```bash
sudo apt-get update
sudo apt-get install -y llvm-14-dev clang-14 libpolly-14-dev
```

### Building from Source

```bash
git clone https://github.com/qwetls/aha-lang.git
cd aha-lang
cargo build --release
```

### Running the Compiler

```bash
cargo run --release -- --file example.aha
```

**CLI options:**

| Option | Description |
|--------|-------------|
| `--file <path>` | Source file to compile and execute |
| `--dir <path>` | Directory for module resolution (default: `.`) |
| `--emit-ir <path>` | Save the generated LLVM IR to a file |
| `--version` | Print the compiler version |
| `--help` | Show usage information |

---

## 🧪 Code Example

Create `example.aha`:

```aha
// AHA! is expression-oriented — the last expression is the result
let x = 10;
let y = 20;

if x > y {
    x
} else {
    y
}
```

Run it:

```bash
cargo run --release -- --file example.aha
```

**Expected output:**

```
--- AHA! COMPILER ---
Reading file: example.aha

[1] LEXING...
[2] PARSING...
Parsing successful!

[3] CODE GENERATION...
LLVM IR generated successfully!

--- LLVM IR OUTPUT ---
; ModuleID = 'aha_module'
...
----------------------

[4] EXECUTION (JIT)...
Program executed successfully. Result: 20
```

### More Examples

**Functions & mutual recursion:**

```aha
fn is_even(n) {
    n % 2 == 0
}

fn is_odd(n) {
    if is_even(n) { 0 } else { 1 }
}

let count = 0;
for i 0..10 {
    if is_odd(i) {
        count = count + 1;
    }
}
count  // 5
```

**Strings:**

```aha
let name = "world";
let greeting = "Hello, " + name;
print_str(greeting);   // Hello, world
print(len(name));      // 5
```

---

## 🧠 Compiler Architecture

```
Source Code → Lexer → Parser (Pratt) → AST → Code Generator → LLVM IR → JIT Execution
```

| Stage | Module | What it does |
|-------|--------|--------------|
| **Lexer** | `src/lexer.rs` | Tokenizes source: identifiers, integers, strings (with escapes), operators, line & block comments |
| **Parser** | `src/parser.rs` | Pratt parser producing the AST — expression-oriented, with correct operator precedence |
| **Type System** | `src/types.rs` | `AhaType` + `TypedValue`; compile-time checks for binary/prefix operators |
| **Codegen** | `src/codegen.rs` | LLVM IR generation via `inkwell`: functions (with return-type inference), loops, strings, arrays, C-runtime linkage (`malloc`, `memcpy`, `strcmp`) |
| **Driver** | `src/main.rs` | CLI: lex → parse → codegen → print IR → JIT execute |

---

## 🌍 Language Tour

### Types

| Type | Notes |
|------|-------|
| `Int` | 64-bit integer — the universal numeric type |
| `Bool` | `true` / `false` literals; produced by `!` |
| `String` | `"..."` with escape sequences (`\n`, `\t`, `\\`, `\"`, `\r`, `\0`) |

### Operators

| Category | Operators |
|----------|-----------|
| Arithmetic | `+` `-` `*` `/` `%` |
| Comparison | `==` `!=` `<` `>` `<=` `>=` (→ `Int` 0/1) |
| Logical | `&&` `\|\|` (→ `Int` 0/1) |
| Prefix | `-x`, `!x` |
| Assignment | `x = value` |

### Control Flow

- `if cond { ... } else { ... }` — an *expression*; the last expression of each branch is the value
- `while cond { ... }`
- `for x a..b { ... }` — range loop with `break` / `continue`

### Builtins

| Builtin | Description |
|---------|-------------|
| `print(int)` | Print an integer |
| `print_str(string)` | Print a string |
| `len(string)` | Length in O(1) |
| `abs(x)`, `min(a, b)`, `max(a, b)` | Numeric helpers |

### Modules (v1.5.0)

Import functions and structs from another `.aha` file:

```aha
use "math"
use "utils"

let result = add(2, 3);
```

- `use "math"` resolves to `math.aha` in the same directory
- Imports are recursive — if `math.aha` uses `"helper"`, that file is resolved too
- Cycle detection prevents infinite loops
- CLI: `aha run main.aha --dir ./src`

---

## 🗺️ Roadmap

### ✅ Implemented (v1.x)

- [x] Lexer & Pratt parser with full error reporting
- [x] `Int`, `Bool`, `String` types
- [x] Arithmetic, comparison, `&&`/`||`, prefix, assignment
- [x] `if`/`else`, `while`, `for` (with `break`/`continue`)
- [x] Functions: parameters, `return`, forward references, mutual recursion, string params & returns
- [x] String struct, concatenation, comparison, `len()`
- [x] Array literals & indexing (codegen)
- [x] Block comments, string escapes, `!=` fix, type-checking pass
- [x] Builtins: `print`, `print_str`, `abs`, `min`, `max`, `len`
- [x] JIT execution via LLVM
- [x] CLI (`--file`, `--emit-ir`, `--version`)
- [x] VS Code syntax-highlighting extension (`editors/vscode`)
- [x] Module system: `use "file"` for multi-file compilation (v1.5.0)
- [x] CI: `cargo check`, 336+ tests, `cargo build --release`

### 🚧 Planned (Phase 2)

- [x] Struct codegen & field access at runtime
- [x] Type inference & annotations
- [x] Generics / parametric types
- [x] Module system (`use "file"` imports, multi-file compilation) — v1.5.0
- [ ] Package manager (`aha install`)
- [ ] Resource lifetimes — safe manual memory management, no GC overhead
- [ ] Actor-model concurrency (message passing, async/await)
- [ ] Self-hosting — the AHA! compiler written in AHA!

---

## 🤝 Contributing

We welcome contributions of all kinds — bug reports, feature ideas, or code.

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

---

## 📄 License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

---

## 💡 Why AHA!?

Great tools don't add complexity — they remove it. AHA! was built on one simple principle: **a language easy enough to read like prose, powerful enough to write like a system.** No ceremony, no boilerplate — just clear code that runs at native speed.

**Join us in writing the next chapter of computing.**