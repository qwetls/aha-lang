# AHA! Lang

<div align="center">

![AHA! Lang Logo](https://via.placeholder.com/150x150/000000/FFFFFF?text=AHA!)

**A**dvanced **H**ybrid **A**rchitecture

A fast, expressive, and modern programming language designed for building anything from web backends to game engines.

[![CI/CD](https://github.com/ahalang-dev/aha-lang/actions/workflows/ci.yml/badge.svg)](https://github.com/ahalang-dev/aha-lang/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://www.rust-lang.org)

</div>

---

## ✨ Fitur Utama

AHA! dirancang dari awal untuk memberikan pengalaman pengembangan yang luar biasa:

- **🚀 Performa Tinggi:** Dikompilasi ke LLVM IR untuk optimasi kode mesin yang maksimal, setara dengan C++.
- **🧠 Sistem Tipe Cerdas:** Statis dengan inferensi tipe otomatis. Aman dari bug, namun tetap ringkas untuk ditulis.
- **🔀 Konkurensi Aman:** Model Aktor bawaan untuk menulis kode paralel yang bebas dari race condition.
- **🛠️ Manajemen Sumber Daya Kontrol Penuh:** Kontrol memori manual yang aman dengan "Resource Lifetimes", tanpa overhead Garbage Collector.
- **📦 Ekosistem Modern:** Dibangun di atas Rust, memanfaatkan toolchain `Cargo` yang kuat.

---

## 🚀 Mulai Cepat

### Prasyarat

- **Rust** (versi 1.75 atau lebih baru)
- **LLVM 14** dan library pendukung
- **Clang 14**

#### Instalasi di Ubuntu/Debian

```bash
# Update package list
sudo apt-get update

# Install LLVM, Clang, and development libraries
sudo apt-get install -y llvm-14-dev clang-14 libpolly-14-dev zlib1g-dev
```

### Mengompilasi dari Sumber

1.  **Clone repositori:**
    ```bash
    git clone https://github.com/ahalang-dev/aha-lang.git
    cd aha-lang
    ```

2.  **Jalankan Kompilator:**
    ```bash
    cargo run -- --file <nama_file>.aha
    ```

### Contoh Kode

Buat file bernama `contoh.aha`:

```aha
let x = 10;
let y = 20;

if x > y {
    x
} else {
    y
}
```

Jalankan:
```bash
cargo run -- --file contoh.aha
```

**Output yang Diharapkan:**
```
--- KOMPILER AHA! ---
Membaca file: contoh.aha

[1] LEXING...
[2] PARSING...
Parsing berhasil!

[3] CODE GENERATION...
Kode LLVM IR berhasil dihasilkan!

--- LLVM IR OUTPUT ---
; ModuleID = 'aha_module'
...
----------------------

[4] EKSEKUSI (JIT)...
Program berhasil dijalankan. Hasil: 20
```

---

## 🧠 Arsitektur Kompiler

Kompiler AHA! dibangun dengan arsitektur modern dan modular:

1.  **Lexer:** Memecah kode sumber menjadi token-token.
2.  **Parser:** Mengurai token menjadi Abstract Syntax Tree (AST) menggunakan Pratt Parser.
3.  **Code Generator:** Menerjemahkan AST menjadi LLVM Intermediate Representation (IR).
4.  **LLVM Backend:** Mengoptimalkan dan mengompilasi IR menjadi kode mesin asli.

![Architecture Diagram](https://via.placeholder.com/600x300/CCCCCC/000000?text=Lexer+->+Parser+->+Codegen+->+LLVM)

---

## 🛣️ Peta Jalan (Roadmap)

AHA! masih dalam pengembangan awal. Ini adalah rencana kami:

- [x] **Milestone 1: Fondasi Kompiler**
    - [x] Lexer & Parser
    - [x] Tipe data Integer
    - [x] Ekspresi Aritmatika & Perbandingan
    - [x] Pernyataan Kondisional `if/else`
- [x] **Milestone 2: Fitur Fundamental** *(In Progress)*
    - [x] Tipe data `Boolean` (codegen)
    - [x] Operator `<=`, `>=`
    - [x] Pernyataan Perulangan `while` ✅
    - [x] Pernyataan Perulangan `for` (parser) 
    - [ ] Fungsi dengan parameter
    - [ ] Statement `return`
- [ ] **Milestone 3: Struktur Data Tingkat Lanjut**
    - [x] Array (parser)
    - [ ] Array (codegen)
    - [ ] String
    - [ ] Struct
- [ ] **Milestone 4: Pustaka Standar**
    - [ ] Modul I/O File
    - [ ] Modul Jaringan
    - [ ] Modul Web (HTTP Server)
- [ ] **Milestone 5: Tooling & Ekosistem**
    - [ ] AHA! Language Server (untuk VS Code, dll.)
    - [ ] Package Manager
    - [ ] Dokumentasi Interaktif

---

## 🤝 Berkontribusi

Kami sangat terbuka untuk kontribusi! Baik itu melaporkan bug, menyarankan fitur baru, atau berkontribusi kode.

Lihat [CONTRIBUTING.md](CONTRIBUTING.md) untuk panduan lebih lanjut.

---

## 📄 Lisensi

Proyek ini dilisensikan di bawah Lisensi MIT. Lihat file [LICENSE](LICENSE) untuk detailnya.

---

## 💡 Mengapa AHA!?

Kami percaya bahwa pemrograman harus lebih dekat dengan cara berpikir manusia. AHA! bertujuan untuk menghilangkan boilerplate dan kompleksitas yang tidak perlu, memungkinkan Anda untuk fokus pada logika dan solusi yang Anda bangun.

**Bergabunglah dengan kami dalam menciptakan generasi berikutnya dari bahasa pemrograman!**
