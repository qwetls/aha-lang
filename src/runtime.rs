// src/runtime.rs
//
// F6 Phase 1 — Actor runtime.
//
// Phase 1: actors are pure JIT — no runtime functions needed.
// The spawn expression allocates a struct on the heap and returns the pointer as i64 (handle).
// call(a, msg) compiles to a direct call to handle(a, msg) in JIT code.
// send(a, msg) is a no-op in Phase 1.
//
// Phase 2 will add threading, message queues, and proper actor dispatch via native runtime.
