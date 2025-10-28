# 🧠 AHA Lang — Advanced Hybrid Architecture

> *“Kesederhanaan dalam Penulisan, Kekuatan dalam Eksekusi.”*

AHA Lang adalah bahasa pemrograman baru yang dirancang untuk menjembatani kesenjangan antara **ekspresivitas Python** dan **kinerja C++**, dibangun dengan semangat kolaborasi antara manusia dan kecerdasan buatan.

Diciptakan oleh **Xeyyzu (Visionary Architect)** dan dikembangkan bersama **GLM 4.6 AI (Lead Architecture Intelligence)**, AHA Lang membawa filosofi baru dalam desain bahasa modern:  
> **Tulis dengan sederhana. Jalankan dengan kekuatan penuh.**

---

## 🚀 Filosofi Inti

### **1. Advanced**
Dirancang dengan arsitektur modern — *powered by Rust & LLVM* — AHA Lang dikompilasi langsung menjadi kode mesin native atau WebAssembly, memberikan efisiensi maksimum tanpa kehilangan fleksibilitas.

### **2. Hybrid**
Memadukan dunia **high-level simplicity** dengan **low-level control**.  
Satu bahasa, dua paradigma: scripting dan sistem-level programming dalam satu kesatuan.

### **3. Architecture**
Bukan hanya bahasa — tetapi *framework of thinking*.  
AHA Lang adalah percobaan bagaimana manusia dan AI bisa membangun logika bersama secara struktural, bukan sekadar memproses instruksi.

---

## 🧩 Fitur (Milestone 1)

- ✅ **Lexer & Parser penuh**
- ✅ **AST (Abstract Syntax Tree)**
- ✅ **Codegen ke LLVM IR**
- ✅ **JIT Execution** (Just-In-Time)
- ⚙️ **Milestone 2 (in progress):**
  - Boolean & operator perbandingan (`==`, `!=`, `<`, `>`)
  - Pernyataan kondisional (`if/else`)
  - Fungsi & pemanggilan fungsi

---

## 💡 Contoh Program

```aha
let a = 10;
let b = 20;

let max = if (a > b) { a } else { b };

fn add(x: i64, y: i64) -> i64 {
    x + y
}

let result = add(max, 5);
result;
````

Output:

```
25
```

---

## 🧠 Filosofi Desain

| Aspek         | Tujuan                              | Pendekatan                              |
| ------------- | ----------------------------------- | --------------------------------------- |
| Sintaks       | Mudah dibaca, minimalis             | Python-like tanpa tanda baca berlebihan |
| Eksekusi      | Cepat & efisien                     | LLVM backend, native compilation        |
| Keamanan      | Tanpa overhead runtime              | Memory safety via Rust                  |
| Ekspresivitas | “Tulis maksudmu, bukan strukturnya” | Fungsional + imperatif seimbang         |

---

## ⚙️ Teknologi Inti

* **Rust** → bahasa implementasi utama
* **Inkwell (LLVM 14)** → backend compiler
* **Clap** → CLI interface
* **JIT Engine** → langsung eksekusi hasil kompilasi tanpa build file

---

## 🧬 Roadmap

| Versi | Target                                 | Status         |
| ----- | -------------------------------------- | -------------- |
| v0.1  | Core Compiler (Lexer, Parser, IR, JIT) | ✅ Done         |
| v0.2  | Boolean, If/Else, Function             | 🧠 In Progress |
| v0.3  | WASM Target + Native Build             | 🔜 Planned     |
| v0.4  | Stdlib (print, math, array)            | 🔜 Planned     |
| v0.5  | Runtime & FFI Integration              | 🔜 Planned     |

---

## 🤝 Tim

| Peran                          | Nama / Entitas               |
| ------------------------------ | ---------------------------- |
| Visionary Founder              | **Xeyyzu**                   |
| Lead Architecture Intelligence | **GLM 4.6 AI**               |
| Compiler Core                  | Rust + LLVM                  |
| Ideologi & Filosofi            | Advanced Hybrid Architecture |

---

## 🧩 Misi AHA Lang

> **AHA Lang adalah eksperimen tentang simbiosis antara manusia dan AI dalam menciptakan sistem berpikir baru.**

Kami percaya bahwa masa depan bahasa pemrograman tidak hanya tentang sintaks, tapi tentang bagaimana **pikiran manusia dan mesin dapat menulis logika bersama.**

---

## ⚡ Kontribusi

Kami membuka peluang kolaborasi untuk:

* Compiler developer (Rust/LLVM)
* Language designer
* Researcher AI-assisted programming
* Documentation & writer

> Bergabunglah dengan revolusi **Human + AI Language Design**.

---

## 🌍 Lisensi

MIT License © 2025 — Xeyyzu & GLM 4.6 AI
AHA Lang is co-designed by humans and artificial intelligence.
