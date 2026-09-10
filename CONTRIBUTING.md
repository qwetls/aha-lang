# Berkontribusi ke AHA! Lang

Terima kasih atas minat Anda untuk berkontribusi ke AHA! Lang! Ini adalah panduan singkat untuk membantu Anda memulai.

## Cara Berkontribusi

### Melaporkan Bug
Jika Anda menemukan bug, silakan buat "Issue" baru di GitHub dengan label `bug`. Sertakan:
- Deskripsi singkat masalahnya.
- Langkah-langkah untuk mereproduksi bug.
- Log error (jika ada).
- Informasi lingkungan (OS, versi Rust, versi LLVM).

### Menyarankan Fitur
Jika Anda memiliki ide untuk fitur baru, silakan buat "Issue" baru dengan label `enhancement`. Jelaskan:
- Apa fitur tersebut dan mengapa itu berguna.
- Bagaimana Anda membayangkan penggunaannya.

### Berkontribusi Kode
1.  **Fork** repositori ini.
2.  **Buat branch baru** untuk fitur atau perbaikan Anda (`git checkout -b fitur-baru-saya`). Fitur besar sebaiknya diprototipe di branch `experimental/<nama-fitur>` terlebih dahulu.
3.  **Lakukan perubahan** Anda. Pastikan kode mengikuti gaya yang sudah ada dan semua tes berjalan.
4.  **Commit** perubahan Anda (`git commit -m 'Menambahkan fitur X'`).
5.  **Push** ke branch Anda (`git push origin fitur-baru-saya`).
6.  **Buat Pull Request** ke branch **`development`** repositori asli (bukan `main` — `main` hanya menerima rilis stabil yang sudah terverifikasi CI).

### Checklist Sebelum Membuka Pull Request
- CI hijau pada commit terbaru (`gh run list` atau tab Actions).
- `CHANGELOG.md` diperbarui di bawah heading versi berikutnya.
- Perubahan pada fitur/behavior wajib disertai test (utamakan test behavioral JIT, bukan hanya compile-only).
- Perubahan syntax/builtin yang user-facing: docs site (aha-lang-docs) juga diperbarui.

## Pengaturan Lingkungan Pengembangan

Pastikan Anda telah mengikuti langkah-langkah di `README.md` untuk menginstall semua prasyarat (Rust, LLVM, dll).

Untuk memastikan semuanya berjalan dengan baik, jalankan:
```bash
cargo check
cargo test
```

## Pedoman Komunitas

- [Code of Conduct](CODE_OF_CONDUCT.md) — standar perilaku dalam semua ruang komunitas AHA!.
- [Security Policy](SECURITY.md) — cara melaporkan kerentanan secara privat (jangan lewat issue publik).