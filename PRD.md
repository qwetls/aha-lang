# AHA! Lang — Product Requirements Document (PRD)

**Versi PRD:** 0.3.2
**Tanggal:** 2026-08-20
**Status:** Draf — living document, diperbarui seiring development
**Repo:** [qwetls/aha-lang](https://github.com/qwetls/aha-lang) · Docs: [aha-lang.is-a.dev](https://aha-lang.is-a.dev)

---

## 1. Ringkasan Eksekutif

AHA! Lang — **A**dvanced **H**ybrid **A**rchitecture — adalah bahasa
pemrograman yang dirancang untuk menjangkau **setiap lapisan komputasi**:
dari web backend, game engine, aplikasi desktop, hingga sistem onboard
kapal ruang angkasa. Satu bahasa, satu fondasi, tanpa kompromi.

Tiga karakter inti yang didefinisikan oleh AHA! sendiri — bukan tiruan
bahasa lain:

1. **Cepat.** Setiap program AHA! dikompilasi ke LLVM IR dan dieksekusi
   sebagai machine code (JIT sekarang, AOT menyusul). Tidak ada interpreter,
   tidak ada runtime yang memperlambat.
2. **Sederhana.** Sintaks ringkas dan ekspresif — "mudah dibaca seperti
   prosa". 13 keyword (target: 5-7), tanpa boilerplate, tanpa kurva belajar
   curam.
3. **Bebas memory leak tanpa garbage collector.** Keamanan memori dijamin di
   compile-time lewat model *ownership & lifetimes*. Pembebasan memori
   otomatis, deterministik, tanpa GC pause — kriteria mutlak untuk sistem
   safety-critical seperti aerospace.

> **"Hybrid"** bukan berarti "setengah-setengah" — tetapi satu bahasa yang
> mampu beroperasi di semua level: web yang butuh kecepatan develop, game
> engine yang butuh performa real-time, hingga flight computer yang butuh
> zero crash. Satu arsitektur, satu sintaks, dari frontend web ke luar
> angkasa.

---

## 2. Masalah & Motivasi

Bahasa pemrograman yang ada memaksa pilihan: cepat tapi rumit, sederhana
tapi lambat, aman tapi kaku. Tidak ada satu bahasa yang bisa dari web
sampai aerospace tanpa mengorbankan sesuatu.

**AHA! menolak pilihan itu.**

- **Web** → Python/Go/Rust — tapi Python lambat dengan GC, Rust berat.
- **Game engine** → C++/Lua — scripting Lua lambat, C++ manual management.
- **Desktop** → C/Rust — cepat tapi kurva curam.
- **Safety-critical / aerospace** → C/Ada/SPARK — aman tapi kuno, tooling
  minim, produktivitas rendah.

AHA! ingin menjadi bahasa yang mengisi semua itu: **satu fondasi, semua
lapisan.** Cepat seperti yang dibutuhkan aerospace, sederhana seperti yang
diinginkan web, aman tanpa GC seperti yang dituntut safety-critical.

---

## 3. Tujuan (Goals)

- **G1 — Performa native:** program AHA! dieksekusi sebagai machine code
  (LLVM) dengan overhead serendah mungkin pada workload komputasi & string.
- **G2 — Kesederhanaan:** waktu dari instal sampai "hello world" < 10 menit;
  kode AHA! terbaca tanpa komentar (self-documenting).
- **G3 — Aman memori tanpa GC:** setiap alokasi punya tepat satu *owner*;
  pembebasan memori otomatis dan deterministik saat *scope* berakhir —
  dijamin di compile-time, bukan andalan runtime. **Mutlak untuk aerospace.**
- **G4 — Tooling jujur & ramah pemula:** error message jelas, CLI minimal,
  dokumentasi dwibahasa (EN/ID), CI hijau di setiap commit.
- **G5 — Universal:** satu bahasa yang bisa dari web (REST API, backend)
  hingga game engine hingga flight computer. LLVM sebagai backend tunggal
  memungkinkan target yang berbeda tanpa perubahan sintaks.

---

## 4. Non-Tujuan (Non-Goals) — Agar Tidak Melenceng

- ❌ **Bukan** bahasa produksi enterprise untuk rilis 1.0 dalam waktu dekat.
  Fokus: fondasi benar, bukan fitur sebanyak-banyaknya.
- ❌ **Tidak ada GC** — komitmen permanen. Fitur apapun yang butuh GC (mis.
  siklus referensi tak terbatas) ditolak atau didesain ulang (arena/region).
- ❌ **Tidak mengejar** ekosistem package sebesar npm/pip dulu — module system
  tetap dibangun, tapi registry sederhana (`aha install`).
- ❌ **Bukan** bahasa untuk WASM/mobile di fase ini (bisa direvisi nanti
  di PRD v0.3+).
- ❌ **Tidak menjanjikan** fitur yang belum ada. README & docs wajib jujur
  soal status implementasi (prinsip anti-overclaim).
- ~~❌ **Resource lifetimes tidak disentuh** sebelum semua fondasi (F1-F4)
  benar-benar stabil.~~ — ✅ F1-F4 selesai & stabil (2026-08-20), F5
  sekarang aktif dengan fase 1 (compiler-inserted free, scope-based).

---

## 5. Target Pengguna (Persona)

1. **Developer yang ingin bahasa cepat tanpa kerumitan** — mereka yang
   menulis program yang butuh kecepatan, tapi tidak ingin tenggelam dalam
   detail manajemen memori.
2. **Developer yang ingin bahasa sederhana tanpa kompromi kecepatan** —
   sintaks ringkas, tapi tetap native.
3. **Pelajar & komunitas compiler** — AHA! terbuka sebagai bahasa untuk
   belajar kompilator (LLVM, Pratt parser, JIT).
4. **Kontributor awal** — proyek ini butuh komunitas untuk tumbuh.
5. **Insinyur aerospace & embedded** — target jangka panjang: bahasa yang
   bisa dipakai untuk sistem onboard, telemetri, dan kontrol penerbangan.

---

## 6. Visi Jangka Panjang: Dari Web ke Luar Angkasa

```
Layer              Contoh                 Bahasa sekarang       AHA! target
─────              ──────                 ───────────────       ──────────
Web                REST API, backend      Python, Go, Rust      ✅ AHA! → LLVM → native
Game engine        Logic, scripting       C++, Lua              ✅ Cepat + tanpa GC pause
Desktop            CLI, GUI, tools        C, Rust               ✅ LLVM native
Embedded           Sensor, kontrol        C, Ada                ✅ Tanpa GC, deterministik
Safety-critical    Flight computer        C, SPARK/Ada          ✅ Ownership = no leak, zero crash
```

AHA! tidak harus menjadi satu-satunya bahasa di setiap layer — tapi harus
**mampu** digunakan di semua layer itu. "Hybrid" berarti fleksibel, bukan
kompromi.

---

## 7. Strategi: Stabilisasi Dulu, Baru Melangkah

**F1-F3 stabil di `main`. F4 namespace sebagian. F5 Phase 2 selesai.**

| Fase | Fokus | Status |
|------|-------|--------|
| **F1** | Struct codegen, mutasi field, struct sebagai param/return | ✅ Selesai (v1.5.0) |
| **F2** | Type inference & annotations | ✅ Selesai |
| **F3** | Generics / parametric types | ✅ Selesai — fungsi generik + List<T> + Map<K,V> (581+ test) |
| **F4** | Module system — namespace & visibilitas | ⚠️ `use "file"` ✅ + `pub` keyword + `module::name` ✅ (v1.5.3); visibility filter belum |
| **F5** | Resource lifetimes (ownership) | 🔄 Phase 1 ✅ (scope-based auto-free); Phase 2 ✅ (last-use analysis); Phase 3 belum |
| **F6** | Actor-model concurrency | ⏳ Setelah F5 |
| **F7** | Self-hosting | ⏳ Setelah F6 |
| **F8** | Package manager (`aha install`) | ⏳ Setelah AOT binary + komunitas |

Setiap langkah: development → test → CI hijau → review → (jika mantap) merge
ke main. Tidak ada loncatan.

---

## 8. Kondisi Saat Ini (Status Jujur, per 2026-08-20)

### ✅ Sudah stabil (di `main` — 600+ test)
- Lexer, Pratt parser dengan error reporting penuh
- Tipe `Int` (i64), `Bool`, `String` (struct `{ptr, len}`)
- Operator aritmatika, perbandingan, `&&`/`||`, prefix, assignment
  (semua boolean → Int 0/1, bisa dikomposisi dengan aritmatika)
- `if`/`else`, `while`, `for a..b` dengan `break`/`continue`
- Fungsi: parameter, `return`, forward references, mutual recursion
- String: concat (malloc/memcpy), `==`/`!=` (strcmp), `len()` O(1)
- Array literal & indexing
- Builtin: `print`, `print_str`, `abs`, `min`, `max`, `len`
- JIT execution via LLVM (inkwell)
- CLI (`--file`, `--emit-ir`, `--version`), VS Code extension
- CI: `cargo check`, 571+ test, `cargo build --release`

### ✅ F1 — Struct (v1.5.0, di `main`)
- Struct codegen & field access at runtime
- Struct field type hints dihormati di runtime
- Mutasi field (`p.x = 5`) — lvalue field access
- Struct sebagai parameter & return value fungsi

### ✅ F2 — Type Inference & Annotations
- Inferensi tipe `let` tanpa anotasi (default Int, List/Map dari builtins)
- Inferensi tipe return fungsi dari body (String, Struct, if-branches)
- Anotasi tipe eksplisit `let x: int = 5`
- Return type annotation `fn f() -> int` + validation

### ✅ F3 — Generics / Parametric Types
- Fungsi generik `fn max<T>(a: T, b: T) -> T` — monomorphization per call site
- List<T> (F3e) — heap-allocated dynamic array + builtins + index read/write
- `fn first<T>(xs: List<T>) -> T` — type param T ter-bind dari hint `List<T>`
- Map<K,V> — deterministic hash table (open addressing, splitmix64/FNV-1a)
- Map 4 combos: `<Int,Int>`, `<String,Int>`, `<Int,String>`, `<String,String>`
- 21 Map tests, grow-on-load-factor + rehash + free old buffer

### ⚠️ F4 — Module System (partial, v1.5.3)
- `use "file"` — modularitas antar file (recursive import, AST merge, cycle detection) ✅
- `pub` keyword — di-lexer, di-parse, tersimpan di AST (`is_pub` pada FunctionLiteral & StructDefinition) ✅
- `::` token — `ColonColon` di-lexer ✅
- `module::name` — `ModuleAccess` expression, handle di parser & codegen ✅
- [ ] Visibility filter — `pub` disimpan tapi belum enforce (semua item dari import tetap accessible)

### 🔄 F5 — Resource Lifetimes (Phase 1 in progress)
**Pendekatan: Compiler-inserted free** — compiler secara otomatis menyisipkan
panggilan `free()` saat variabel keluar scope. Tidak ada borrow checker, tidak
ada GC, tidak ada reference counting.

| Phase | Strategi | Status |
|-------|----------|--------|
| Fase 1 | Scope-based free — auto free Map/List di akhir scope | ✅ |
| Fase 2 | Last-use analysis — free di titik usage terakhir | ✅ |
| Fase 3 | Escape analysis — handle alokasi yang di-return/passed | ⏳ |

Detail Fase 1 & 2 (di `development`):
- `VarInfo` extended: `freed: bool`, `is_param: bool`
- `mark_param()` — exclude function params dari auto-free
- `mark_freed()` — prevent double-free jika user manual call `list_free`/`map_free`
- `has_heap_locals()` — cek apakah scope punya variabel heap yang belum free
- `insert_cleanup_inline()` — insert free calls sebelum return terminator
- `find_last_uses()` — pre-scan AST untuk titik usage terakhir per variabel heap
- `insert_free_for_var()` — free variabel spesifik, skip jika sudah freed/param
- Fallback ke scope-end cleanup untuk variabel di branch (conservative)
- 19 ownership tests (12 Phase 1 + 7 Phase 2)
- String free belum diimplementasi (ponytail: `string_free` belum ada sebagai builtin)

### ❌ Belum ada
- AOT compile ke native binary (saat ini JIT-only)
- Self-hosting (compiler AHA! ditulis dalam AHA!)
- Namespace & visibilitas (F4 sisa)
- Package manager `aha install` (F8 — setelah AOT + komunitas)

---

## 9. Persyaratan Fungsional — Prioritas Stabilisasi

### F1. Struct codegen & field access — ✅ SELESAI
- [x] Literal struct, akses field, type hint field, type-check literal
- [x] Typed struct field layout (Int → i64, String → {i8*, i64})
- [x] Mutasi field (`p.x = 5`) — lvalue field access (7 tests)
- [x] Struct sebagai parameter & return value fungsi (6 tests)

### F2. Type inference & annotations — ✅ SELESAI
- [x] Field struct bertipe (slice pertama, selesai di `development`)
- [x] Inferensi tipe `let` tanpa anotasi (default Int, List/Map dari builtins)
- [x] Inferensi tipe return fungsi dari body (String, Struct, if-branches)
- [x] Anotasi tipe eksplisit `let x: int = 5` (20 tests)
- [x] Return type annotation `fn f() -> int` + validation (7 tests)

### F3. Generics / parametric types — ✅ SELESAI
- [x] Fungsi generik `fn max<T>(a: T, b: T) -> T` — monomorphization per call site
- [x] Monomorphization via LLVM (tanpa runtime cost)
- [x] List<T> (F3e) — heap-allocated dynamic array + builtins + index read/write
- [x] `fn first<T>(xs: List<T>) -> T` — type param T ter-bind dari hint `List<T>`
- [x] Map<K,V> — deterministic hash table (open addressing, splitmix64/FNV-1a, 4 combos, 21 tests)
- [x] Map grow-on-load-factor + rehash + free old buffer
- [x] Semua sub-fitur di `main` (571+ test)

### F4. Module system — ⚠️ PARTIAL (v1.5.3)
- [x] `use "file"` — modularitas antar file (recursive import resolution, AST merge, cycle detection)
- [x] `pub` keyword — lexer, parser, AST (is_pub flag on FunctionLiteral & StructDefinition)
- [x] `::` token — ColonColon in lexer
- [x] `module::name` — ModuleAccess expression, parser prefix, codegen (compile_expression, compile_call, scan_expr_for_calls, infer_expr_type_with_scope)
- [ ] Visibility filter — pub items only from imports (currently all items accessible)
- [ ] ~~`aha install` — registry sederhana~~ → dipindah ke F8 (setelah AOT binary + komunitas)

### F5. Resource lifetimes — 🔄 AKTIF (Phase 2 selesai)
**Approach: Compiler-inserted free** — compiler otomatis insert `free()` calls.
Tidak ada borrow checker, tidak ada GC, tidak ada reference counting.

- [x] Desain: scope-based → last-use → escape analysis (3 fase)
- [x] `VarInfo` extended: `freed`, `is_param` flags
- [x] `mark_param()` — exclude function params dari auto-free
- [x] `mark_freed()` — prevent double-free
- [x] `has_heap_locals()` — cek scope untuk variabel heap belum free
- [x] `insert_cleanup_inline()` — insert free calls sebelum return terminator
- [x] Phase 2: last-use analysis — `find_last_uses()`, `insert_free_for_var()`, 7 tests
- [ ] `string_free` builtin (belum — ponytail: add when string lifetime mgmt)
- [ ] Phase 3: escape analysis (returned/passed allocations)

### ⏳ F6. Actor-model concurrency
- [ ] Message passing antar actor
- [ ] `async`/`await` tanpa thread yang bisa di-share sembarangan
- [ ] Memory-safe: message transfer = ownership transfer (bukan shared)

### ⏳ F7. Self-hosting
- [ ] Compiler AHA! ditulis ulang dalam AHA! (bukti kedewasaan bahasa)

---

## 10. Persyaratan Non-Fungsional

- **Backend:** LLVM via inkwell — satu-satunya jalur codegen (JIT sekarang,
  AOT nanti). Tidak ada interpreter.
- **Model memori (target):** value semantics + ownership; string/array punya
  owner tunggal; free otomatis & deterministik di akhir scope; alokasi
  bertumpuk (stack) untuk nilai kecil.
- **Keamanan:** keamanan memori **compile-time** — tidak ada
  use-after-free, tidak ada double-free, tidak ada leak (bukan sekadar
  pengurangan risiko).
- **Determinisme:** tidak ada GC pause → latensi bisa diprediksi. **Mutlak
  untuk aerospace dan game engine.**
- **Kualitas:** CI menjalankan `cargo check` + seluruh test suite
  (`--test-threads=1`, LLVM JIT tidak thread-safe) + `cargo build --release`
  di setiap push.
- **Dokumentasi:** dwibahasa EN/ID (repo docs Fumadocs).

---

## 11. Metrik Keberhasilan (Fase Stabilisasi)

| Metrik | Target |
|--------|--------|
| Test suite | ≥ 500 test hijau di CI, 0 gagal |
| Memory leak di test suite (Valgrind/ASan) | **0** |
| Time-to-hello-world | < 10 menit (release binary/playground) |
| String & heap allocation leak-free | 0 leak (setelah F5 diimplementasi) |
| Komunitas (3 bulan setelah promosi) | ≥ 100 ⭐, ≥ 10 kontributor non-owner |

---

## 12. Keputusan Desain Terbuka (Butuh Diskusi — Setelah Stabilisasi)

1. **Ekspresi lifetime di sintaks:** implisit (scope-based — free otomatis
   saat keluar scope) atau anotasi eksplisit? Rekomendasi awal: implisit
   dulu — demi kesederhanaan.
2. **Parameter fungsi:** by-value atau by-reference? (menghindari copy struct
   besar; butuh definisi ownership untuk param).
3. **Mutasi field struct:** ~~kapan `p.x = 5` didukung?~~ — ✅ selesai di v1.5.0.
4. **String:** tetap immutable `{ptr, len}`? Atau ada tipe builder?
5. **AOT native binary:** kapan dirilis? (saat ini JIT-only; `--emit-ir` sudah
   ada, tinggal object code + linker).
6. **Alokasi:** kapan sebuah nilai boleh di-heap vs stack?
7. **Target aerospace:** apakah AHA! perlu dukungan hardware langsung (GPIO,
   MMIO, interrupt) atau cukup sebagai bahasa aplikasi di atas RTOS?

---

## 13. Prinsip "Tidak Melenceng" (Anti-Drift Guard)

1. Setiap fitur baru WAJIB berasal dari roadmap / PRD ini, atau disetujui
   eksplisit oleh owner.
2. Semua perubahan masuk ke `development` dulu; `main` hanya menerima lewat
   PR yang CI-nya hijau.
3. README & docs wajib jujur: fitur yang belum ada TIDAK diclaim.
4. **Tanpa GC adalah komitmen desain permanen** — setiap keputusan arsitektur
   diuji terhadap prinsip ini.
5. **F5 (resource lifetimes) aktif** sejak 2026-08-20 setelah F1-F4 stabil.
   Phase 1 (scope-based free) di `labs`; phase 2-3 menyusul bertahap.
6. PRD ini diperbarui saat keputusan besar diambil (bukan per commit kecil).

---

## 14. Riwayat Revisi

| Tanggal | Versi | Perubahan |
|---------|-------|-----------|
| 2026-08-16 | 0.1 | PRD awal: visi 3 pilar, status jujur, roadmap terpetakan, metrik |
| 2026-08-16 | 0.2 | Visi besar: web → aerospace; "Hybrid" dijelaskan; F5 di-freeze; strategi stabilisasi; target aerospace & embedded |
| 2026-08-20 | 0.3 | F1-F3 semua ✅ di `main`. F5 unfreeze — Phase 1 selesai. F4: `use "file"` ✅, namespace belum. `aha install` dipindah ke F8 (post-AOT). |
| 2026-08-20 | 0.3.1 | F5 Phase 1 merged (compiler-inserted free, 581+ test). `aha install` dipindah dari F4 ke F8 — terlalu dini tanpa binary release & komunitas. |
| 2026-08-20 | 0.3.2 | F4 namespace progress: `pub` keyword + `::` token + `module::name` expression implemented (lexer, AST, parser, codegen). Visibility filter deferred — pub stored in AST but all items still accessible from imports. |
| 2026-08-20 | 0.3.3 | F5 Phase 2 selesai: last-use analysis — `find_last_uses()` pre-scan AST, `insert_free_for_var()` per-variable free, fallback ke scope-end untuk branch. 7 tests baru (total 19 ownership tests). |
