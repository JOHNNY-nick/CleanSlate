//! C API of the CleanSlate engine, built as `cleanslate.dll`.
//!
//! Conventions (specifications § 4.2):
//! - inputs are UTF-8 JSON passed as `(*const u8, i32)`;
//! - every response is a [`ByteBuffer`] holding the JSON envelope
//!   `{ "ok": true, "data": … }` or `{ "ok": false, "error": { … } }`,
//!   allocated by Rust and released **only** by [`cs_buffer_free`];
//! - no panic ever crosses the boundary: each function runs inside
//!   [`std::panic::catch_unwind`], a panic becomes `INTERNAL_PANIC`.
//!
//! `build.rs` generates the C# declarations (`NativeMethods.g.cs`) from
//! this file with csbindgen.

use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

use cleanslate_core::{CoreError, CoreResult, Engine, EngineConfig};
use serde::Serialize;
use serde_json::json;

/// Bytes allocated by Rust and handed to C#.
///
/// C# reads `len` bytes at `ptr`, then gives the buffer back to
/// [`cs_buffer_free`]. `cap` is needed by Rust to free the memory.
#[repr(C)]
pub struct ByteBuffer {
    /// Start of the bytes.
    pub ptr: *mut u8,
    /// Number of valid bytes.
    pub len: usize,
    /// Allocated capacity.
    pub cap: usize,
}

impl ByteBuffer {
    /// Moves `bytes` to the heap and returns an owning raw pointer.
    fn into_raw(bytes: Vec<u8>) -> *mut ByteBuffer {
        // ManuallyDrop: Rust must not free the Vec here; cs_buffer_free will.
        let mut bytes = std::mem::ManuallyDrop::new(bytes);
        let buffer = ByteBuffer {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
            cap: bytes.capacity(),
        };
        Box::into_raw(Box::new(buffer))
    }
}

/// Opaque engine handle given to C#.
pub struct EngineHandle {
    engine: Engine,
}

/// Builds the success envelope `{ "ok": true, "data": … }`.
fn ok_envelope<T: Serialize>(data: &T) -> Vec<u8> {
    match serde_json::to_vec(&json!({ "ok": true, "data": data })) {
        Ok(bytes) => bytes,
        Err(err) => error_envelope("SERIALIZATION_ERROR", &err.to_string(), None),
    }
}

/// Builds the error envelope `{ "ok": false, "error": { code, message, path } }`.
fn error_envelope(code: &str, message: &str, path: Option<String>) -> Vec<u8> {
    let value = json!({
        "ok": false,
        "error": { "code": code, "message": message, "path": path }
    });
    serde_json::to_vec(&value).unwrap_or_else(|_| {
        br#"{"ok":false,"error":{"code":"SERIALIZATION_ERROR","message":"","path":null}}"#.to_vec()
    })
}

/// Extracts the message of a panic payload.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_owned()
    }
}

/// Runs `f`, catches any panic, and packs the outcome into a response buffer.
fn respond<T, F>(f: F) -> *mut ByteBuffer
where
    T: Serialize,
    F: FnOnce() -> CoreResult<T>,
{
    // AssertUnwindSafe: after a panic we only build an error message; no
    // state touched by `f` is reused.
    let bytes = match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(data)) => ok_envelope(&data),
        Ok(Err(err)) => error_envelope(err.code(), &err.to_string(), err.location()),
        Err(payload) => {
            let message = panic_message(payload.as_ref());
            tracing::error!(%message, "panic caught at the FFI boundary");
            error_envelope("INTERNAL_PANIC", &message, None)
        }
    };
    ByteBuffer::into_raw(bytes)
}

/// Borrows the input bytes `(ptr, len)` sent by C#.
///
/// # Safety
/// `ptr` must be null with `len == 0`, or point to `len` readable bytes that
/// stay valid and unchanged for the lifetime `'a`.
unsafe fn input_bytes<'a>(ptr: *const u8, len: i32) -> CoreResult<&'a [u8]> {
    let len = usize::try_from(len)
        .map_err(|_| CoreError::InvalidInput(format!("negative input length: {len}")))?;
    if len == 0 {
        return Ok(&[]);
    }
    if ptr.is_null() {
        return Err(CoreError::InvalidInput("null input pointer".into()));
    }
    // SAFETY: non-null, and the caller guarantees `len` readable bytes.
    Ok(unsafe { std::slice::from_raw_parts(ptr, len) })
}

/// Returns `{ "version": "0.1.0" }`.
#[no_mangle]
pub extern "C" fn cs_version() -> *mut ByteBuffer {
    respond(|| Ok(json!({ "version": cleanslate_core::VERSION })))
}

/// Creates an engine from an `EngineConfig` JSON (`len == 0` means defaults,
/// i.e. simulation mode); release it with [`cs_engine_free`].
///
/// On failure, returns null and stores in `*out_error` the error envelope
/// `{ "ok": false, "error": … }`, to be released with [`cs_buffer_free`].
/// On success, `*out_error` is set to null. `out_error` itself may be null
/// when the caller does not want the error details.
///
/// # Safety
/// `config` must point to `len` readable bytes (or be null with `len == 0`);
/// `out_error` must be null or point to writable storage for one pointer.
#[no_mangle]
pub unsafe extern "C" fn cs_engine_new(
    config: *const u8,
    len: i32,
    out_error: *mut *mut ByteBuffer,
) -> *mut EngineHandle {
    let created = catch_unwind(|| -> CoreResult<*mut EngineHandle> {
        // SAFETY: forwarded from this function's contract.
        let bytes = unsafe { input_bytes(config, len) }?;
        let config = if bytes.is_empty() {
            EngineConfig::default()
        } else {
            serde_json::from_slice(bytes)?
        };
        let handle = EngineHandle {
            engine: Engine::with_system(config),
        };
        Ok(Box::into_raw(Box::new(handle)))
    });
    let (handle, error) = match created {
        Ok(Ok(handle)) => (handle, ptr::null_mut()),
        Ok(Err(err)) => {
            tracing::error!(code = err.code(), %err, "cs_engine_new failed");
            let bytes = error_envelope(err.code(), &err.to_string(), err.location());
            (ptr::null_mut(), ByteBuffer::into_raw(bytes))
        }
        Err(payload) => {
            let message = panic_message(payload.as_ref());
            tracing::error!(%message, "panic in cs_engine_new");
            let bytes = error_envelope("INTERNAL_PANIC", &message, None);
            (ptr::null_mut(), ByteBuffer::into_raw(bytes))
        }
    };
    if out_error.is_null() {
        // Nobody will free the error buffer: release it here.
        // SAFETY: `error` is null or was just created by ByteBuffer::into_raw.
        unsafe { cs_buffer_free(error) };
    } else {
        // SAFETY: the caller guarantees `out_error` points to writable storage.
        unsafe { *out_error = error };
    }
    handle
}

/// Borrows the engine behind a handle.
///
/// # Safety
/// `engine` must be null or come from [`cs_engine_new`] and still be alive.
unsafe fn engine_ref<'a>(engine: *const EngineHandle) -> CoreResult<&'a Engine> {
    // SAFETY: null is checked by as_ref; otherwise the caller guarantees validity.
    unsafe { engine.as_ref() }
        .map(|handle| &handle.engine)
        .ok_or_else(|| CoreError::InvalidInput("null engine handle".into()))
}

/// Lists installed applications (EF-01): `data` is an `InstalledApp[]`.
///
/// # Safety
/// `engine` must be null or a live handle from [`cs_engine_new`].
#[no_mangle]
pub unsafe extern "C" fn cs_list_apps(engine: *const EngineHandle) -> *mut ByteBuffer {
    respond(|| {
        // SAFETY: forwarded from this function's contract.
        let engine = unsafe { engine_ref(engine) }?;
        engine.list_apps()
    })
}

/// Destroys an engine created by [`cs_engine_new`]. Null is ignored.
///
/// # Safety
/// `engine` must come from [`cs_engine_new`] and must not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn cs_engine_free(engine: *mut EngineHandle) {
    if engine.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the pointer comes from Box::into_raw in cs_engine_new and
        // ownership is given back exactly once by the caller.
        let handle = unsafe { Box::from_raw(engine) };
        tracing::debug!(version = handle.engine.version(), "engine freed");
        drop(handle);
    }));
}

/// Releases a buffer returned by any `cs_*` function. Null is ignored.
///
/// # Safety
/// `buffer` must come from this library and must not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn cs_buffer_free(buffer: *mut ByteBuffer) {
    if buffer.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `buffer` comes from Box::into_raw in ByteBuffer::into_raw,
        // and its fields describe a Vec<u8> whose ownership was leaked there.
        let buffer = unsafe { Box::from_raw(buffer) };
        drop(unsafe { Vec::from_raw_parts(buffer.ptr, buffer.len, buffer.cap) });
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// Reads a response buffer as JSON, then frees it like C# would.
    fn take(buffer: *mut ByteBuffer) -> Value {
        assert!(!buffer.is_null());
        let value = unsafe {
            let b = &*buffer;
            let bytes = std::slice::from_raw_parts(b.ptr, b.len);
            serde_json::from_slice(bytes).unwrap()
        };
        unsafe { cs_buffer_free(buffer) };
        value
    }

    #[test]
    fn version_returns_ok_envelope() {
        let value = take(cs_version());
        assert_eq!(value["ok"], true);
        assert_eq!(value["data"]["version"], "0.1.0");
    }

    #[test]
    fn core_error_becomes_error_envelope() {
        let value = take(respond(|| -> CoreResult<()> {
            Err(CoreError::AccessDenied {
                location: r"C:\X".into(),
            })
        }));
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["code"], "ACCESS_DENIED");
        assert_eq!(value["error"]["path"], r"C:\X");
    }

    #[test]
    fn panic_becomes_internal_panic() {
        let value = take(respond(|| -> CoreResult<()> { panic!("simulated panic") }));
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["code"], "INTERNAL_PANIC");
        assert_eq!(value["error"]["message"], "simulated panic");
    }

    #[test]
    fn formatted_panic_message_is_kept() {
        let n = 42;
        let value = take(respond(|| -> CoreResult<()> { panic!("bad value {n}") }));
        assert_eq!(value["error"]["message"], "bad value 42");
    }

    /// Calls cs_engine_new and returns the handle plus the decoded error, if any.
    fn engine_new(config: *const u8, len: i32) -> (*mut EngineHandle, Option<Value>) {
        // Sentinel value: checks that success really resets out_error to null.
        let mut error = ptr::NonNull::<ByteBuffer>::dangling().as_ptr();
        let engine = unsafe { cs_engine_new(config, len, &mut error) };
        let error = (!error.is_null()).then(|| take(error));
        (engine, error)
    }

    #[test]
    fn engine_new_and_free() {
        let config = br#"{"dry_run":true}"#;
        let (engine, error) = engine_new(config.as_ptr(), config.len() as i32);
        assert!(!engine.is_null());
        assert!(error.is_none());
        assert!(unsafe { &*engine }.engine.config().dry_run);
        unsafe { cs_engine_free(engine) };
    }

    #[test]
    fn engine_new_defaults_to_dry_run() {
        let (engine, error) = engine_new(ptr::null(), 0);
        assert!(!engine.is_null());
        assert!(error.is_none());
        assert!(unsafe { &*engine }.engine.config().dry_run);
        unsafe { cs_engine_free(engine) };
    }

    #[test]
    fn engine_new_reports_bad_input() {
        let bad = b"{not json";
        for (ptr, len) in [
            (bad.as_ptr(), bad.len() as i32),
            (bad.as_ptr(), -1),
            (ptr::null(), 5),
        ] {
            let (engine, error) = engine_new(ptr, len);
            assert!(engine.is_null());
            let error = error.expect("error envelope");
            assert_eq!(error["ok"], false);
            assert_eq!(error["error"]["code"], "INVALID_INPUT");
        }
    }

    #[test]
    fn engine_new_accepts_null_out_error() {
        let bad = b"{not json";
        let engine = unsafe { cs_engine_new(bad.as_ptr(), bad.len() as i32, ptr::null_mut()) };
        assert!(engine.is_null());
        let engine = unsafe { cs_engine_new(ptr::null(), 0, ptr::null_mut()) };
        assert!(!engine.is_null());
        unsafe { cs_engine_free(engine) };
    }

    #[test]
    fn list_apps_rejects_null_engine() {
        let value = take(unsafe { cs_list_apps(ptr::null()) });
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["code"], "INVALID_INPUT");
    }

    #[test]
    fn free_functions_ignore_null() {
        unsafe {
            cs_engine_free(ptr::null_mut());
            cs_buffer_free(ptr::null_mut());
        }
    }
}
