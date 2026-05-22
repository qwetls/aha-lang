# AHA! Lang

<div align="center">

<img src="assets/logo.png" alt="AHA! Lang Logo" width="200">

**A**dvanced **H**ybrid **A**rchitecture

A fast, expressive, and modern programming language designed for building anything from web backends to game engines.

[![CI/CD](https://github.com/ahalang-dev/aha-lang/actions/workflows/ci.yml/badge.svg)](https://github.com/ahalang-dev/aha-lang/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://www.rust-lang.org)

</div>

---

## ✨ Key Features

AHA! is designed from the ground up to deliver an outstanding developer experience:

- **🚀 High Performance:** Compiles to LLVM IR for maximum machine code optimization, on par with C++.
- **🧠 Smart Type System:** Static typing with automatic type inference. Safe from bugs, yet concise to write.
- **🔀 Safe Concurrency:** Built-in Actor model for writing parallel code free from race conditions.
- **🛠️ Full Resource Control:** Safe manual memory management with "Resource Lifetimes", without Garbage Collector overhead.
- **📦 Modern Ecosystem:** Built on Rust, leveraging the powerful `Cargo` toolchain.

---

## 🚀 Quick Start

### Prerequisites

- **Rust** (version 1.75 or later)
- **LLVM 14** and supporting libraries
- **Clang 14**

#### Installation on Ubuntu/Debian

```bash
# Update package list
sudo apt-get update

# Install LLVM, Clang, and development libraries
sudo apt-get install -y llvm-14-dev clang-14 libpolly-14-dev zlib1g-dev
```

### Building from Source

1.  **Clone the repository:**
    ```bash
    git clone https://github.com/ahalang-dev/aha-lang.git
    cd aha-lang
    ```

2.  **Run the Compiler:**
    ```bash
    cargo run -- --file <filename>.aha
    ```

### Code Example

Create a file called `example.aha`:

```aha
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
cargo run -- --file example.aha
```

**Expected Output:**
```
--- AHA! COMPILER v1.3 ---
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

---

## 🧠 Compiler Architecture

The AHA! compiler is built with a modern, modular architecture:

```
Source Code → Lexer → Parser → Code Generator → LLVM Backend → Native Binary
```

1.  **Lexer:** Breaks source code into tokens.
2.  **Parser:** Transforms tokens into an Abstract Syntax Tree (AST) using a Pratt Parser.
3.  **Code Generator:** Translates the AST into LLVM Intermediate Representation (IR).
4.  **LLVM Backend:** Optimizes and compiles IR into native machine code.

---

## 🛣️ Roadmap

AHA! is still in early development. Here is our plan:

- [x] **Milestone 1: Compiler Foundation**
    - [x] Lexer & Parser
    - [x] Integer data type
    - [x] Arithmetic & Comparison expressions
    - [x] Conditional statements `if/else`
- [x] **Milestone 2: Fundamental Features** ✅
    - [x] `Boolean` data type (codegen)
    - [x] Operators `<=`, `>=`, `!=` (fixed)
    - [x] `while` loop ✅
    - [x] `for` loop (parser + codegen) ✅
    - [x] Functions with parameters ✅
    - [x] `return` statement ✅
    - [x] Prefix expressions `-x`, `!x` ✅
    - [x] Assignment `x = value` ✅
    - [x] Break & Continue ✅
    - [x] Variable scoping (block-level) ✅
- [x] **Milestone 3: Advanced Data Structures** ✅
    - [x] String type (pointer-as-int, limited)
    - [x] Array (parser + codegen)
    - [x] Struct definitions (parser only)
    - [x] Field access (parser only)
- [x] **Milestone 4: Standard Library** ✅
    - [x] print(int) — print integers
    - [x] print_str(string) — print strings
    - [x] abs(x) — absolute value
    - [x] min(a, b) — minimum
    - [x] max(a, b) — maximum
- [x] **Milestone 5: Tooling & Ecosystem** ✅
    - [x] VS Code Extension (syntax highlighting)
    - [x] CLI improvements (--emit-ir, --version)
    - [x] Better error messages
    - [x] Multi-line comments `/* */` ✅

### 🚀 Phase 2: Advanced Features

- [ ] **Milestone 6: Type System**
    - [ ] Type inference
    - [ ] Type annotations
    - [ ] Generics / Parametric types

- [ ] **Milestone 7: Resource Lifetimes** ⭐
    - [ ] Ownership semantics
    - [ ] Borrow checking
    - [ ] Automatic resource cleanup

- [ ] **Milestone 8: Concurrency**
    - [ ] Actor model
    - [ ] Message passing
    - [ ] Async/await

- [ ] **Milestone 9: Package Ecosystem**
    - [ ] Package manager (`aha install`)
    - [ ] Module system
    - [ ] Dependency resolution

- [ ] **Milestone 10: Self-Hosting** 🏆
    - [ ] AHA! compiler written in AHA!
    - [ ] Bootstrap process
    - [ ] Production ready

---

## 🤝 Contributing

We welcome contributions of all kinds! Whether it's reporting bugs, suggesting new features, or contributing code.

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

---

## 📄 License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

---

## 💡 Why AHA!?

We believe programming should be closer to the way humans think. AHA! aims to eliminate unnecessary boilerplate and complexity, allowing you to focus on the logic and solutions you're building.

**Join us in creating the next generation of programming languages!**
