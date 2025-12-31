# AHA! Lang - Walkthrough

Panduan lengkap untuk memahami dan menggunakan AHA! Lang.

---

## 🚀 Quick Start

```bash
# Clone repository
git clone https://github.com/ahalang-dev/aha-lang.git
cd aha-lang

# Build
cargo build --release

# Run a program
cargo run -- --file examples/hello.aha
```

---

## 📖 Syntax Guide

### Variables

```aha
let x = 10;
let name = "AHA";
let active = true;
```

### Arithmetic

```aha
let a = 5 + 3;      // 8
let b = 10 - 4;     // 6  
let c = 3 * 4;      // 12
let d = 20 / 5;     // 4
```

### Comparisons

```aha
let eq = 5 == 5;    // true
let neq = 3 != 4;   // true
let lt = 3 < 5;     // true
let gt = 7 > 2;     // true
let lte = 5 <= 5;   // true
let gte = 6 >= 4;   // true
```

### Conditionals

```aha
let x = 10;

if x > 5 {
    100
} else {
    0
}
```

### Loops

```aha
// While loop
let i = 0;
while i < 10 {
    i = i + 1
}

// For loop (parser ready)
for x in 0..10 {
    x
}
```

### Functions

```aha
fn add(a, b) {
    return a + b
}

fn factorial(n) {
    if n <= 1 {
        return 1
    }
    return n * factorial(n - 1)
}

// Call functions
add(5, 3)           // 8
factorial(5)        // 120
```

### Arrays

```aha
let arr = [10, 20, 30, 40, 50];

let first = arr[0];    // 10
let third = arr[2];    // 30
let last = arr[4];     // 50
```

### Strings

```aha
let greeting = "Hello, World!";
let name = "AHA! Lang";
let empty = "";
```

### Structs

```aha
// Define a struct
struct Person {
    name,
    age
}

// With type hints (optional)
struct Point {
    x: int,
    y: int
}

// Field access
person.name
point.x
```

---

## 🏗️ Architecture

```
Source Code (.aha)
       ↓
    [Lexer] ──────────→ Tokens
       ↓
    [Parser] ─────────→ AST (Pratt Parser)
       ↓
    [CodeGen] ────────→ LLVM IR
       ↓
    [LLVM JIT] ───────→ Native Execution
```

### Components

| File | Purpose |
|------|---------|
| `src/lexer.rs` | Tokenizes source code |
| `src/parser.rs` | Builds AST using Pratt parser |
| `src/ast.rs` | Defines tokens and AST nodes |
| `src/codegen.rs` | Generates LLVM IR |
| `src/main.rs` | CLI entry point |

---

## 📋 Feature Matrix

| Feature | Lexer | Parser | Codegen |
|---------|:-----:|:------:|:-------:|
| Integers | ✅ | ✅ | ✅ |
| Booleans | ✅ | ✅ | ✅ |
| Strings | ✅ | ✅ | ✅ |
| Arrays | ✅ | ✅ | ✅ |
| Arithmetic | ✅ | ✅ | ✅ |
| Comparisons | ✅ | ✅ | ✅ |
| If/Else | ✅ | ✅ | ✅ |
| While | ✅ | ✅ | ✅ |
| For | ✅ | ✅ | ⏳ |
| Functions | ✅ | ✅ | ✅ |
| Structs | ✅ | ✅ | ⏳ |

---

## 🔜 Coming Soon

- **Resource Lifetimes**: Memory safety without GC
- **Actor Model**: Safe concurrency
- **Package Manager**: Dependency management

---

## 📚 Standard Library

### I/O Functions
```aha
print(42)           // Print integer: 42
print_str("hello")  // Print string: hello
```

### Math Functions
```aha
abs(-10)            // 10
min(5, 3)           // 3  
max(5, 3)           // 5
```

---

## 👥 Team

- **Xeyyzu** - Visionary Architect
- **GLM 4.5** - Chief Architect
- **Antigravity** - Development Partner
