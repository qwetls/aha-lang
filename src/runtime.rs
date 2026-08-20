// src/runtime.rs
//
// F6 Phase 1 — Actor runtime (synchronous).
// Provides actor_spawn, actor_send, actor_call as native functions
// linked to the LLVM JIT via add_global_mapping.
//
// Phase 1: synchronous actor model (no threads).
// actor_spawn(fn_ptr, init_state) -> handle — stores handler + state
// actor_send(handle, msg)           — queued for next actor_call
// actor_call(handle, msg) -> i64    — calls handler synchronously

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

struct ActorEntry {
    handler_fn: i64,    // JIT function pointer
    state: i64,         // initial state (struct pointer as i64)
    pending_msg: Option<i64>,  // queued message from actor_send
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
// Native functions (linked to LLVM JIT)
// ---------------------------------------------------------------------------

/// actor_spawn(fn_ptr, init_state) -> handle
/// Stores the handler function and initial state for later calls.
#[no_mangle]
pub extern "C" fn actor_spawn(fn_ptr: i64, init_state: i64) -> i64 {
    let handle = next_handle();
    actors().lock().unwrap().insert(
        handle,
        ActorEntry {
            handler_fn: fn_ptr,
            state: init_state,
            pending_msg: None,
        },
    );
    handle
}

/// actor_send(handle, msg) — queues a message for next actor_call.
#[no_mangle]
pub extern "C" fn actor_send(handle: i64, msg: i64) {
    let mut actors = actors().lock().unwrap();
    if let Some(entry) = actors.get_mut(&handle) {
        entry.pending_msg = Some(msg);
    }
}

/// actor_call(handle, msg) -> result — calls handler synchronously.
/// If there's a pending message from actor_send, the handler is called
/// with the pending message first, then with msg.
#[no_mangle]
pub extern "C" fn actor_call(handle: i64, msg: i64) -> i64 {
    // Extract the pending message and handler info.
    let (handler_fn, state, pending) = {
        let mut actors = actors().lock().unwrap();
        match actors.get_mut(&handle) {
            Some(entry) => {
                let pending = entry.pending_msg.take();
                (entry.handler_fn, entry.state, pending)
            }
            None => return 0,
        }
    };

    // Cast function pointer and call handler.
    let func: extern "C" fn(i64, i64) -> i64 = unsafe {
        std::mem::transmute(handler_fn)
    };

    // Process pending message first (if any).
    let mut result = if let Some(p) = pending {
        func(state, p)
    } else {
        0
    };

    // Process the current message.
    result = func(state, msg);
    result
}
