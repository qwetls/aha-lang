// src/runtime.rs
//
// F6 Phase 2 — Actor runtime (threaded).
// Provides actor_spawn, actor_send, actor_call as native functions
// linked to the LLVM JIT via add_global_mapping.
//
// Each actor has:
//   - A thread running a message loop
//   - A mailbox (mpsc channel) for incoming messages
//   - A shared result slot (Mutex + Condvar) for request-response
//
// actor_spawn(fn_ptr, init_state) -> handle
// actor_send(handle, msg)           — fire-and-forget
// actor_call(handle, msg) -> i64    — blocking request-response

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;

struct ActorEntry {
    mailbox_tx: std::sync::mpsc::Sender<i64>,
    result: Arc<(Mutex<Option<i64>>, Condvar)>,
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
// Native functions (linked to LLVM JIT via add_global_mapping)
// ---------------------------------------------------------------------------

/// actor_spawn(fn_ptr, init_state) -> handle
///
/// Spawns an actor thread. fn_ptr is a JIT function with signature
/// `fn(state: i64, msg: i64) -> i64` that handles one message.
/// The actor loops: receive msg → call fn(state, msg) → store result.
///
/// # Safety
/// fn_ptr must be a valid JIT function pointer with the above signature.
#[no_mangle]
pub extern "C" fn actor_spawn(fn_ptr: i64, init_state: i64) -> i64 {
    let handle = next_handle();
    let (tx, rx) = std::sync::mpsc::channel::<i64>();
    let result_slot = Arc::new((Mutex::<Option<i64>>::new(None), Condvar::new()));

    let func: extern "C" fn(i64, i64) -> i64 = unsafe { std::mem::transmute(fn_ptr) };
    let slot_clone = result_slot.clone();

    thread::spawn(move || loop {
        match rx.recv() {
            Ok(msg) => {
                let ret = func(init_state, msg);
                let (lock, cvar) = &*slot_clone;
                let mut guard = lock.lock().unwrap();
                *guard = Some(ret);
                cvar.notify_all();
            }
            Err(_) => break,
        }
    });

    actors().lock().unwrap().insert(
        handle,
        ActorEntry {
            mailbox_tx: tx,
            result: result_slot,
        },
    );
    handle
}

/// actor_send(handle, msg) — fire-and-forget message.
#[no_mangle]
pub extern "C" fn actor_send(handle: i64, msg: i64) {
    let actors = actors().lock().unwrap();
    if let Some(entry) = actors.get(&handle) {
        let _ = entry.mailbox_tx.send(msg);
    }
}

/// actor_call(handle, msg) -> result — blocking request-response.
/// Sends msg, waits for handler to process it, returns the result.
#[no_mangle]
pub extern "C" fn actor_call(handle: i64, msg: i64) -> i64 {
    let actors_guard = actors().lock().unwrap();
    let entry = match actors_guard.get(&handle) {
        Some(e) => e,
        None => return 0,
    };

    // Clear previous result, send message.
    {
        let (lock, _) = &*entry.result;
        let mut guard = lock.lock().unwrap();
        *guard = None;
    }
    let _ = entry.mailbox_tx.send(msg);

    // Wait for result.
    let (lock, cvar) = &*entry.result;
    let mut guard = lock.lock().unwrap();
    while guard.is_none() {
        guard = cvar.wait(guard).unwrap();
    }
    guard.unwrap_or(0)
}

// ===========================================================================
// F11 HTTP Parser — native functions for HTTP request/response handling
// ===========================================================================

/// Parse HTTP method from request string. Returns pointer to static buffer.
/// # Safety
/// req must be a valid null-terminated UTF-8 string from AHA! String.
#[no_mangle]
pub extern "C" fn aha_http_request_method(req: i64) -> i64 {
    let req_str = unsafe {
        let ptr = req as *const u8;
        let mut len = 0usize;
        while *ptr.add(len) != 0 { len += 1; }
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len))
    };
    let method = match req_str.split_once(' ') {
        Some((m, _)) => m,
        None => return 0,
    };
    // Leak a Rust String so the pointer stays valid until next call
    let boxed = method.to_string().into_boxed_str();
    let ptr = Box::into_raw(boxed) as *mut u8 as i64;
    ptr
}

/// Parse HTTP path from request string. Returns pointer to static buffer.
/// # Safety
/// req must be a valid null-terminated UTF-8 string from AHA! String.
#[no_mangle]
pub extern "C" fn aha_http_request_path(req: i64) -> i64 {
    let req_str = unsafe {
        let ptr = req as *const u8;
        let mut len = 0usize;
        while *ptr.add(len) != 0 { len += 1; }
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len))
    };
    let path = match req_str.split_once(' ') {
        Some((_, rest)) => match rest.split_once(' ') {
            Some((p, _)) => p,
            None => return 0,
        },
        None => return 0,
    };
    let boxed = path.to_string().into_boxed_str();
    Box::into_raw(boxed) as *mut u8 as i64
}

/// Parse HTTP body from request string (everything after \r\n\r\n).
/// # Safety
/// req must be a valid null-terminated UTF-8 string.
#[no_mangle]
pub extern "C" fn aha_http_request_body(req: i64) -> i64 {
    let req_str = unsafe {
        let ptr = req as *const u8;
        let mut len = 0usize;
        while *ptr.add(len) != 0 { len += 1; }
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len))
    };
    let body = match req_str.split_once("\r\n\r\n") {
        Some((_, b)) => b,
        None => "",
    };
    let boxed = body.to_string().into_boxed_str();
    Box::into_raw(boxed) as *mut u8 as i64
}

/// Find header value by name (case-insensitive).
/// # Safety
/// req and name must be valid null-terminated UTF-8 strings.
#[no_mangle]
pub extern "C" fn aha_http_request_header(req: i64, name: i64) -> i64 {
    let req_str = unsafe {
        let ptr = req as *const u8;
        let mut len = 0usize;
        while *ptr.add(len) != 0 { len += 1; }
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len))
    };
    let name_str = unsafe {
        let ptr = name as *const u8;
        let mut len = 0usize;
        while *ptr.add(len) != 0 { len += 1; }
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len))
    };
    let name_lower = name_str.to_lowercase();
    for line in req_str.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().to_lowercase() == name_lower {
                let val = v.trim();
                let boxed = val.to_string().into_boxed_str();
                return Box::into_raw(boxed) as *mut u8 as i64;
            }
        }
    }
    let empty = "".to_string().into_boxed_str();
    Box::into_raw(empty) as *mut u8 as i64
}

/// Build HTTP response string from status code and body.
/// # Safety
/// body must be a valid null-terminated UTF-8 string.
#[no_mangle]
pub extern "C" fn aha_http_response(status: i64, body: i64) -> i64 {
    let body_str = unsafe {
        let ptr = body as *const u8;
        let mut len = 0usize;
        while *ptr.add(len) != 0 { len += 1; }
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len))
    };
    let status_text = match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let resp = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status, status_text, body_str.len(), body_str
    );
    let boxed = resp.into_boxed_str();
    Box::into_raw(boxed) as *mut u8 as i64
}
