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

// ===========================================================================
// F12 JSON Parser/Serializer — native functions for JSON handling
// ===========================================================================

/// JSON value node in the parse tree.
enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<JsonValue>),
    Object(HashMap<String, JsonValue>),
}

/// Tokenizer for JSON parsing.
struct JsonTokenizer<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonTokenizer<'a> {
    fn new(input: &'a str) -> Self {
        Self { bytes: input.as_bytes(), pos: 0 }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_whitespace();
        self.bytes.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<u8> {
        self.skip_whitespace();
        if self.pos < self.bytes.len() {
            let ch = self.bytes[self.pos];
            self.pos += 1;
            Some(ch)
        } else {
            None
        }
    }

    fn expect(&mut self, ch: u8) -> Result<(), &'static str> {
        match self.next() {
            Some(c) if c == ch => Ok(()),
            _ => Err("unexpected character"),
        }
    }

    fn parse_string_raw(&mut self) -> Result<String, &'static str> {
        self.expect(b'"')?;
        let mut result = String::new();
        loop {
            match self.next() {
                None => return Err("unterminated string"),
                Some(b'"') => break,
                Some(b'\\') => match self.next() {
                    Some(b'"') => result.push('"'),
                    Some(b'\\') => result.push('\\'),
                    Some(b'/') => result.push('/'),
                    Some(b'n') => result.push('\n'),
                    Some(b'r') => result.push('\r'),
                    Some(b't') => result.push('\t'),
                    Some(b'b') => result.push('\u{0008}'),
                    Some(b'f') => result.push('\u{000C}'),
                    Some(b'u') => {
                        let mut hex = String::new();
                        for _ in 0..4 {
                            match self.next() {
                                Some(c) if c.is_ascii_hexdigit() => hex.push(c as char),
                                _ => return Err("invalid unicode escape"),
                            }
                        }
                        let code = u32::from_str_radix(&hex, 16)
                            .map_err(|_| "invalid unicode escape")?;
                        match char::from_u32(code) {
                            Some(ch) => result.push(ch),
                            None => return Err("invalid unicode codepoint"),
                        }
                    }
                    _ => return Err("invalid escape"),
                },
                Some(c) => result.push(c as char),
            }
        }
        Ok(result)
    }

    fn parse_number(&mut self) -> Result<f64, &'static str> {
        let start = self.pos;
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'-' {
            self.pos += 1;
        }
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'.' {
            self.pos += 1;
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        if self.pos < self.bytes.len()
            && (self.bytes[self.pos] == b'e' || self.bytes[self.pos] == b'E')
        {
            self.pos += 1;
            if self.pos < self.bytes.len()
                && (self.bytes[self.pos] == b'+' || self.bytes[self.pos] == b'-')
            {
                self.pos += 1;
            }
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        if self.pos == start {
            return Err("expected number");
        }
        let s = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap();
        s.parse::<f64>().map_err(|_| "invalid number")
    }
}

use std::fmt::Write as FmtWrite;

fn parse_json_value(tok: &mut JsonTokenizer) -> Result<JsonValue, &'static str> {
    match tok.peek() {
        None => Err("unexpected end of input"),
        Some(b'{') => parse_json_object(tok),
        Some(b'[') => parse_json_array(tok),
        Some(b'"') => Ok(JsonValue::Str(tok.parse_string_raw()?)),
        Some(b't') => {
            for &ch in b"true" { if tok.next() != Some(ch) { return Err("expected 'true'"); } }
            Ok(JsonValue::Bool(true))
        }
        Some(b'f') => {
            for &ch in b"false" { if tok.next() != Some(ch) { return Err("expected 'false'"); } }
            Ok(JsonValue::Bool(false))
        }
        Some(b'n') => {
            for &ch in b"null" { if tok.next() != Some(ch) { return Err("expected 'null'"); } }
            Ok(JsonValue::Null)
        }
        Some(c) if c.is_ascii_digit() || c == b'-' => Ok(JsonValue::Number(tok.parse_number()?)),
        _ => Err("unexpected character"),
    }
}

fn parse_json_object(tok: &mut JsonTokenizer) -> Result<JsonValue, &'static str> {
    tok.expect(b'{')?;
    let mut map = HashMap::new();
    if tok.peek() == Some(b'}') { tok.next(); return Ok(JsonValue::Object(map)); }
    loop {
        let key = tok.parse_string_raw()?;
        tok.expect(b':')?;
        let val = parse_json_value(tok)?;
        map.insert(key, val);
        match tok.next() {
            Some(b'}') => break,
            Some(b',') => continue,
            _ => return Err("expected ',' or '}'"),
        }
    }
    Ok(JsonValue::Object(map))
}

fn parse_json_array(tok: &mut JsonTokenizer) -> Result<JsonValue, &'static str> {
    tok.expect(b'[')?;
    let mut arr = Vec::new();
    if tok.peek() == Some(b']') { tok.next(); return Ok(JsonValue::Array(arr)); }
    loop {
        arr.push(parse_json_value(tok)?);
        match tok.next() {
            Some(b']') => break,
            Some(b',') => continue,
            _ => return Err("expected ',' or ']'"),
        }
    }
    Ok(JsonValue::Array(arr))
}

fn json_escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            c if c.is_control() => { let _ = write!(out, "\\u{:04x}", c as u32); }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_value_to_string(val: &JsonValue) -> String {
    match val {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(n) => {
            if n.is_finite() && n.fract() == 0.0 && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                format!("{}", *n as i64)
            } else {
                format!("{}", n)
            }
        }
        JsonValue::Str(s) => json_escape_str(s),
        JsonValue::Array(arr) => {
            let items: Vec<String> = arr.iter().map(json_value_to_string).collect();
            format!("[{}]", items.join(","))
        }
        JsonValue::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let pairs: Vec<String> = keys.iter()
                .map(|k| format!("{}:{}", json_escape_str(k), json_value_to_string(map.get(*k).unwrap())))
                .collect();
            format!("{{{}}}", pairs.join(","))
        }
    }
}

/// Navigate a JsonValue by dot-separated path ("user.name" or "items.0").
fn navigate_json<'a>(val: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    if path.is_empty() { return Some(val); }
    let mut current = val;
    for part in path.split('.') {
        match current {
            JsonValue::Object(map) => { current = map.get(part)?; }
            JsonValue::Array(arr) => {
                let idx: usize = part.parse().ok()?;
                current = arr.get(idx)?;
            }
            _ => return None,
        }
    }
    Some(current)
}

/// json_parse(json_ptr, json_len) -> handle (pointer to JsonValue tree)
/// # Safety
/// json_ptr must point to valid UTF-8 bytes, json_len must be correct.
#[no_mangle]
pub extern "C" fn aha_json_parse(json_ptr: i64, json_len: i64) -> i64 {
    let input = unsafe {
        let ptr = json_ptr as *const u8;
        let len = json_len as usize;
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len))
    };
    let mut tok = JsonTokenizer::new(input);
    match parse_json_value(&mut tok) {
        Ok(val) => Box::into_raw(Box::new(val)) as i64,
        Err(_) => 0,
    }
}

/// json_stringify(handle) -> String
/// # Safety
/// handle must be a valid pointer from json_parse, or 0 for null.
#[no_mangle]
pub extern "C" fn aha_json_stringify(handle: i64) -> i64 {
    if handle == 0 {
        let s = "null".to_string().into_boxed_str();
        return Box::into_raw(s) as *mut u8 as i64;
    }
    let val = unsafe { &*(handle as *const JsonValue) };
    let result = json_value_to_string(val);
    let boxed = result.into_boxed_str();
    Box::into_raw(boxed) as *mut u8 as i64
}

/// json_get(handle, path_ptr, path_len) -> String representation of value at path.
/// Path is dot-separated: "user.name", "items.0".
/// # Safety
/// handle must be a valid pointer from json_parse. path_ptr must point to valid UTF-8.
#[no_mangle]
pub extern "C" fn aha_json_get(handle: i64, path_ptr: i64, path_len: i64) -> i64 {
    if handle == 0 {
        let empty = "".to_string().into_boxed_str();
        return Box::into_raw(empty) as *mut u8 as i64;
    }
    let val = unsafe { &*(handle as *const JsonValue) };
    let path_str = unsafe {
        let ptr = path_ptr as *const u8;
        let len = path_len as usize;
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len))
    };
    match navigate_json(val, path_str) {
        Some(found) => {
            let result = match found {
                JsonValue::Str(s) => s.clone(),
                other => json_value_to_string(other),
            };
            let boxed = result.into_boxed_str();
            Box::into_raw(boxed) as *mut u8 as i64
        }
        None => {
            let empty = "".to_string().into_boxed_str();
            Box::into_raw(empty) as *mut u8 as i64
        }
    }
}

/// Recursively free a JsonValue tree.
/// # Safety
/// handle must be a valid pointer from json_parse, or 0 (no-op).
#[no_mangle]
pub extern "C" fn aha_json_free(handle: i64) -> i64 {
    if handle != 0 {
        unsafe { drop(Box::from_raw(handle as *mut JsonValue)); }
    }
    0
}

// ===========================================================================
// F13 — String Builtins
// ===========================================================================

struct SplitResult {
    parts: Vec<String>,
}

/// str_split(s_ptr, s_len, delim_ptr, delim_len) -> handle
/// Splits string by delimiter. Handle usable with str_split_count/str_split_get.
/// # Safety
/// All pointers must be valid UTF-8 of given lengths.
#[no_mangle]
pub extern "C" fn aha_str_split(s_ptr: i64, s_len: i64, delim_ptr: i64, delim_len: i64) -> i64 {
    let s = unsafe {
        let ptr = s_ptr as *const u8;
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, s_len as usize))
    };
    let delim = unsafe {
        let ptr = delim_ptr as *const u8;
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, delim_len as usize))
    };
    let parts: Vec<String> = s.split(delim).map(|p| p.to_string()).collect();
    let result = Box::new(SplitResult { parts });
    Box::into_raw(result) as i64
}

/// str_split_count(handle) -> number of parts
/// # Safety
/// handle must be valid from str_split.
#[no_mangle]
pub extern "C" fn aha_str_split_count(handle: i64) -> i64 {
    if handle == 0 { return 0; }
    let result = unsafe { &*(handle as *const SplitResult) };
    result.parts.len() as i64
}

/// str_split_get(handle, index) -> string pointer
/// Returns empty string if index out of bounds.
/// # Safety
/// handle must be valid from str_split.
#[no_mangle]
pub extern "C" fn aha_str_split_get(handle: i64, index: i64) -> i64 {
    if handle == 0 {
        let empty = "".to_string().into_boxed_str();
        return Box::into_raw(empty) as *mut u8 as i64;
    }
    let result = unsafe { &*(handle as *const SplitResult) };
    let idx = index as usize;
    let part = if idx < result.parts.len() { &result.parts[idx] } else { "" };
    let boxed = part.to_string().into_boxed_str();
    Box::into_raw(boxed) as *mut u8 as i64
}

/// str_split_free(handle) -> 0
/// Frees the SplitResult.
/// # Safety
/// handle must be valid from str_split, or 0 (no-op).
#[no_mangle]
pub extern "C" fn aha_str_split_free(handle: i64) -> i64 {
    if handle != 0 {
        unsafe { drop(Box::from_raw(handle as *mut SplitResult)); }
    }
    0
}

/// str_to_int(s_ptr, s_len) -> integer value, 0 on parse failure.
/// # Safety
/// s_ptr must be valid UTF-8 of given length.
#[no_mangle]
pub extern "C" fn aha_str_to_int(s_ptr: i64, s_len: i64) -> i64 {
    let s = unsafe {
        let ptr = s_ptr as *const u8;
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, s_len as usize))
    };
    s.trim().parse::<i64>().unwrap_or(0)
}

/// str_contains(s_ptr, s_len, sub_ptr, sub_len) -> 1 if contains, 0 otherwise.
/// # Safety
/// All pointers must be valid UTF-8 of given lengths.
#[no_mangle]
pub extern "C" fn aha_str_contains(s_ptr: i64, s_len: i64, sub_ptr: i64, sub_len: i64) -> i64 {
    let s = unsafe {
        let ptr = s_ptr as *const u8;
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, s_len as usize))
    };
    let sub = unsafe {
        let ptr = sub_ptr as *const u8;
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, sub_len as usize))
    };
    if s.contains(sub) { 1 } else { 0 }
}

/// str_substring(s_ptr, s_len, start, end) -> string pointer
/// Returns substring from start to end (exclusive). Clamps to bounds.
/// # Safety
/// s_ptr must be valid UTF-8 of given length.
#[no_mangle]
pub extern "C" fn aha_str_substring(s_ptr: i64, s_len: i64, start: i64, end: i64) -> i64 {
    let s = unsafe {
        let ptr = s_ptr as *const u8;
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, s_len as usize))
    };
    let len = s.chars().count();
    let start_idx = start.max(0) as usize;
    let end_idx = (end.max(0) as usize).min(len);
    if start_idx >= end_idx {
        let empty = "".to_string().into_boxed_str();
        return Box::into_raw(empty) as *mut u8 as i64;
    }
    let sub: String = s.chars().skip(start_idx).take(end_idx - start_idx).collect();
    let boxed = sub.into_boxed_str();
    Box::into_raw(boxed) as *mut u8 as i64
}
