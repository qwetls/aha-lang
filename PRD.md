# AHA! Lang — Product Requirements Document (PRD)

**Versi PRD:** 0.1
**Tanggal:** 2026-08-16
**Status:** Draf — living document, diperbarui seiring development
**Repo:** [qwetls/aha-lang](https://github.com/qwetls/aha-lang) · Docs: [aha-lang.is-a.dev](https://aha-lang.is-a.dev)

---

## 1. Ringkasan Eksekutif

AHA! Lang adalah bahasa pemrograman dengan tiga karakter inti yang
didefinisikan oleh dirinya sendiri — bukan tiruan bahasa lain:

1. **Cepat.** Setiap program AHA! dikompilasi ke LLVM IR dan dieksekusi
   sebagai machine code (JIT sekarang, AOT menyusul). Tidak ada interpreter,
   tidak ada runtime yang memperlambat.
2. **Sederhana.** Sintaks ringkas dan ekspresif — "mudah dibaca seperti
   prosa". Sedikit kata kunci, tanpa boilerplate, tanpa kurva belajar curam.
3. **Bebas memory leak tanpa garbage collector.** Keamanan memori dijamin di
   compile-time lewat model *ownership & lifetimes* (komitmen roadmap:
   "Resource lifetimes — safe manual memory management, no GC overhead").
   Pembebasan memori otomatis, deterministik, tanpa GC pause.

> **Posisi:** AHA! tidak mengikuti aturan bahasa lain dan tidak berusaha
> menjadi "versi lebih baik dari X". AHA! punya keputusan desainnya sendiri,
> dan tiga karakter di atas adalah komitmennya — bukan pembanding.

---

## 2. Masalah & Motivasi

Bahasa pemrograman yang ada umumnya memaksa pilihan: cepat tapi rumit,
sederhana tapi lambat, atau aman tapi kaku. AHA! menolak pilihan itu — ketiga
karakter inti (cepat, sederhana, aman memori tanpa GC) adalah hak AHA! untuk
menentukan caranya sendiri, bukan meniru solusi yang sudah ada.

Target pengguna: siapa pun yang menginginkan kecepatan native dan keamanan
memori tanpa harus menerima kompleksitas berlebihan.

---

## 3. Tujuan (Goals)

- **G1 — Performa native:** program AHA! dieksekusi sebagai machine code
  (LLVM) dengan overhead serendah mungkin pada workload komputasi & string.
- **G2 — Kesederhanaan:** waktu dari instal sampai "hello world" < 10 menit;
  kode AHA! terbaca tanpa komentar (self-documenting).
- **G3 — Aman memori tanpa GC:** setiap alokasi punya tepat satu *owner*;
  pembebasan memori otomatis dan deterministik saat *scope* berakhir —
  dijamin di compile-time, bukan andalan runtime.
- **G4 — Tooling jujur & ramah pemula:** error message jelas, CLI minimal,
  dokumentasi dwibahasa (EN/ID), CI hijau di setiap commit.

---

## 4. Non-Tujuan (Non-Goals) — Agar Tidak Melenceng

- ❌ **Bukan** bahasa produksi enterprise untuk rilis 1.0 dalam waktu dekat.
  Fokus: fondasi benar, bukan fitur sebanyak-banyaknya.
- ❌ **Tidak ada GC** — komitmen permanen. Fitur apapun yang butuh GC (mis.
  siklus referensi tak terbatas) ditolak atau didesain ulang (arena/region).
- ❌ **Tidak mengejar** ekosistem package sebesar npm/pip dulu — module system
  tetap dibangun, tapi registry sederhana (`aha install`).
- ❌ **Bukan** bahasa untuk web/WASM/mobile di fase ini (bisa direvisi nanti
  di PRD v0.2+).
- ❌ **Tidak menjanjikan** fitur yang belum ada. README & docs wajib jujur
  soal status implementasi (prinsip anti-overclaim).

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

---

## 6. Kondisi Saat Ini (Status Jujur, per 2026-08-16)

### ✅ Sudah berjalan (di `main`, v1.x — 336 test)
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
- CI: `cargo check`, 363 test, `cargo build --release`

### 🆕 Baru di branch `development` (belum di-merge ke `main`)
- Struct codegen & field access at runtime (Roadmap Phase 2 #1)
- Struct field type hints dihormati di runtime (`name: string` → layout
  `{i8*, i64}`; type-check literal; akses field bertipe benar)

### ❌ Belum ada (target Phase 2+)
- Mutasi field struct (`p.x = 5`)
- Type inference penuh (variabel & fungsi) + anotasi eksplisit
- Generics / parametric types
- Module system & package manager (`aha install`)
- **Resource lifetimes (ownership) — inti visi "no leak, no GC"**
- Actor-model concurrency (message passing, async/await)
- AOT compile ke native binary (saat ini JIT-only)
- Self-hosting (compiler AHA! ditulis dalam AHA!)

---

## 7. Persyaratan Fungsional (Terpetakan ke Roadmap)

Prioritas mengikuti urutan roadmap Phase 2. Setiap item wajib: sesuai roadmap
→ ditulis + diuji → di-merge ke `development` → CI hijau → baru `main`.

### F1. Struct codegen & field access — ✅ SELESAI
- [x] Literal struct, akses field, type hint field, type-check literal
- [ ] Mutasi field (`p.x = 5`) — lvalue field access (kandidat lanjutan)
- [ ] Struct sebagai parameter & return value fungsi

### F2. Type inference & annotations
- [x] Field struct bertipe (slice pertama, selesai di `development`)
- [ ] Inferensi tipe `let` tanpa anotasi (default Int saat ini)
- [ ] Inferensi tipe return fungsi (sudah parsial untuk String)
- [ ] Anotasi tipe eksplisit `let x: int = 5`

### F3. Generics / parametric types
- [ ] Fungsi generik `fn max<T>(a: T, b: T) -> T`
- [ ] Struktur data generik (List<T>, Map<K,V>)
- [ ] Monomorphization via LLVM (tanpa runtime cost)

### F4. Module system & package manager
- [ ] `import "file.aha"` — modularitas antar file
- [ ] Namespace & visibilitas
- [ ] `aha install` — registry sederhana

### F5. Resource lifetimes (NO LEAK, NO GC) — ⭐ INTI VISI
- [ ] Ownership model: setiap nilai punya satu owner; drop otomatis saat
      scope keluar (RAII-style, dijamin compile-time)
- [ ] Tidak ada `free` manual, tidak ada GC runtime
- [ ] Verifikasi memori di CI: jalankan suite di bawah Valgrind/ASan → **0 leak**
- [ ] String & array tidak lagi bergantung pada alokasi yang bisa bocor

### F6. Actor-model concurrency
- [ ] Message passing antar actor
- [ ] `async`/`await` tanpa thread yang bisa di-share sembarangan
- [ ] Memory-safe: message transfer = ownership transfer (bukan shared)

### F7. Self-hosting
- [ ] Compiler AHA! ditulis ulang dalam AHA! (bukti kedewasaan bahasa)

---

## 8. Persyaratan Non-Fungsional

- **Backend:** LLVM via inkwell — satu-satunya jalur codegen (JIT sekarang,
  AOT nanti). Tidak ada interpreter.
- **Model memori (target):** value semantics + ownership; string/array punya
  owner tunggal; free otomatis & deterministik di akhir scope; alokasi
  bertumpuk (stack) untuk nilai kecil.
- **Keamanan:** keamanan memori **compile-time** — tidak ada
  use-after-free, tidak ada double-free, tidak ada leak (bukan sekadar
  pengurangan risiko).
- **Determinisme:** tidak ada GC pause → latensi bisa diprediksi.
- **Kualitas:** CI menjalankan `cargo check` + seluruh test suite
  (`--test-threads=1`, LLVM JIT tidak thread-safe) + `cargo build --release`
  di setiap push.
- **Dokumentasi:** dwibahasa EN/ID (repo docs Fumadocs).

---

## 9. Metrik Keberhasilan

| Metrik | Target |
|--------|--------|
| Performa komputasi (benchmark AHA! sendiri) | Overhead serendah mungkin; tidak ada interpreter |
| Memory leak di test suite (Valgrind/ASan) | **0** |
| Time-to-hello-world | < 10 menit (release binary/playground) |
| Test suite | ≥ 500 test hijau di CI, 0 gagal |
| Komunitas (3 bulan setelah promosi) | ≥ 100 ⭐, ≥ 10 kontributor non-owner |

---

## 10. Keputusan Desain Terbuka (Butuh Diskusi)

1. **Ekspresi lifetime di sintaks:** implisit (scope-based — free otomatis
   saat keluar scope) atau anotasi eksplisit? Rekomendasi awal: implisit
   dulu — demi kesederhanaan.
2. **Parameter fungsi:** by-value atau by-reference? (menghindari copy struct
   besar; butuh definisi ownership untuk param).
3. **Mutasi struct:** kapan `p.x = 5` didukung? (butuh lvalue field access di
   codegen).
4. **String:** tetap immutable `{ptr, len}`? Atau ada tipe builder?
5. **AOT native binary:** kapan dirilis? (saat ini JIT-only; `--emit-ir` sudah
   ada, tinggal object code + linker).
6. **Alokasi:** kapan sebuah nilai boleh di-heap vs stack?

---

## 11. Prinsip "Tidak Melenceng" (Anti-Drift Guard)

1. Setiap fitur baru WAJIB berasal dari roadmap / PRD ini, atau disetujui
   eksplisit oleh owner.
2. Semua perubahan masuk ke `development` dulu; `main` hanya menerima lewat
   PR yang CI-nya hijau.
3. README & docs wajib jujur: fitur yang belum ada TIDAK diclaim.
4. **Tanpa GC adalah komitmen desain permanen** — setiap keputusan arsitektur
   diuji terhadap prinsip ini.
5. PRD ini diperbarui saat keputusan besar diambil (bukan per commit kecil).

---

## 12. Riwayat Revisi

| Tanggal | Versi | Perubahan |
|---------|-------|-----------|
| 2026-08-16 | 0.1 | PRD awal: visi 3 pilar, status jujur, roadmap terpetakan, metrik |
