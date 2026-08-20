// src/runtime.rs
//
// F6 Phase 1 — Actor runtime (synchronous, state-only).
// The runtime only stores/retrieves actor state.
// Handler dispatch is done in JIT code (codegen emits direct calls).
//
// actor_spawn(init_state) -> handle  — stores state, returns handle
// actor_send(handle, msg)            — no-op (fire-and-forget, Phase 2)
// actor_call(handle, msg) -> i64     — returns pending msg (Phase 2 logic)

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

struct ActorEntry {
    state: i64,
}

static ACTORS: OnceLock<Mutex<HashMap<i64, ActorEntry>>> = OnceLock::new();
static NEXT_HANDLE: OnceLock<Mutex<i64>> = OnceLock::new();

fn actors() -> &'static Mutex<HashMap<i64, ActorEntry>> {
    ACTORS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_handle() -> i64 {
    let counter = NEXT_HANDLE.get_or_init(|| Mutex::new(1i64));
    let mut c = counter.lock().unwrap();
    let h = *c;
    *c += 1;
    h
}

// ---------------------------------------------------------------------------
// Native functions (called from JIT)
// ---------------------------------------------------------------------------

/// actor_spawn(init_state) -> handle
#[no_mangle]
pub extern "C" fn actor_spawn(init_state: i64) -> i64 {
    let handle = next_handle();
    actors().lock().unwrap().insert(handle, ActorEntry { state: init_state });
    handle
}

/// actor_send(handle, msg) — fire-and-forget (no-op in Phase 1)
#[no_mangle]
pub extern "C" fn actor_send(handle: i64, msg: i64) {
    // Phase 1: no-op. Phase 2 will queue messages.
    let _ = (handle, msg);
}

/// actor_call(handle, msg) -> i64
/// Returns the actor's stored state (msg is available for Phase 2 dispatch)
#[no_mangle]
pub extern "C" fn actor_call(handle: i64, msg: i64) -> i64 {
    let actors = actors().lock().unwrap();
    match actors.get(&handle) {
        Some(entry) => entry.state,
        None => 0,
    }
}

/// actor_get_state(handle) -> i64
/// Returns the actor's stored state (used by codegen for handler dispatch)
#[no_mangle]
pub extern "C" fn actor_get_state(handle: i64) -> i64 {
    let actors = actors().lock().unwrap();
    match actors.get(&handle) {
        Some(entry) => entry.state,
        None => 0,
    }
}
