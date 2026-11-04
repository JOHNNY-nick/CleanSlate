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
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

use cleanslate_core::model::{
    AppLeftoversRequest, CleanPlan, CleanProgress, PurgeRequest, RestoreRequest, ScanOptions,
    ScanProgress, UninstallProgress, UninstallRequest,
};
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

/// Progress callback: receives `ctx` and a UTF-8 JSON `ScanProgress`
/// (`ptr`, `len`) valid only during the call. It may be called from a
/// worker thread, never after the scan function has returned.
pub type ProgressCallback =
    Option<unsafe extern "C" fn(ctx: *mut c_void, json: *const u8, len: i32)>;

/// Caller context handed back to the callback; only passed through.
struct CallbackContext(*mut c_void);

// SAFETY: the pointer is never dereferenced by Rust, only handed back to the
// caller's callback, which the caller made thread-safe (documented above).
unsafe impl Send for CallbackContext {}
// SAFETY: same reason: Rust only copies the pointer value.
unsafe impl Sync for CallbackContext {}

impl CallbackContext {
    /// Returns the pointer. A method (not `.0`) makes closures capture the
    /// whole `Sync` wrapper rather than the raw field (Rust 2021 captures).
    fn get(&self) -> *mut c_void {
        self.0
    }
}

/// Scans for orphan leftovers, read-only (EF-05 to EF-11): `data` is a
/// `ScanReport`. `options` is a `ScanOptions` JSON (`len == 0`: defaults).
/// Returns `BUSY` if another long operation runs on this engine.
///
/// # Safety
/// `engine` must be a live handle; `options` must point to `len` readable
/// bytes (or be null with `len == 0`); `callback` may be null.
#[no_mangle]
pub unsafe extern "C" fn cs_scan_orphans(
    engine: *const EngineHandle,
    options: *const u8,
    len: i32,
    callback: ProgressCallback,
    ctx: *mut c_void,
) -> *mut ByteBuffer {
    let context = CallbackContext(ctx);
    respond(|| {
        // SAFETY: forwarded from this function's contract.
        let engine = unsafe { engine_ref(engine) }?;
        // SAFETY: forwarded from this function's contract.
        let bytes = unsafe { input_bytes(options, len) }?;
        let options: ScanOptions = if bytes.is_empty() {
            ScanOptions::default()
        } else {
            serde_json::from_slice(bytes)?
        };
        let progress = |event: &ScanProgress| {
            let (Some(callback), Ok(json)) = (callback, serde_json::to_vec(event)) else {
                return;
            };
            let Ok(json_len) = i32::try_from(json.len()) else {
                return;
            };
            // SAFETY: the caller guarantees the callback is valid for the
            // whole call; `json` outlives this synchronous invocation.
            unsafe { callback(context.get(), json.as_ptr(), json_len) };
        };
        engine.scan_orphans(&options, &progress)
    })
}

/// Parses a required JSON input `(ptr, len)`.
///
/// # Safety
/// Same contract as [`input_bytes`].
unsafe fn parse_input<T: serde::de::DeserializeOwned>(ptr: *const u8, len: i32) -> CoreResult<T> {
    // SAFETY: forwarded from this function's contract.
    let bytes = unsafe { input_bytes(ptr, len) }?;
    Ok(serde_json::from_slice(bytes)?)
}

/// Moves the selected items of the last scan to quarantine (EF-17): `data`
/// is a `Session` (`simulated: true` when the engine or the plan is in
/// simulation, the default). `callback` (may be null) receives a
/// `CleanProgress` JSON before each item and once at the end.
///
/// # Safety
/// `engine` must be a live handle; `plan` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn cs_clean(
    engine: *const EngineHandle,
    plan: *const u8,
    len: i32,
    callback: ProgressCallback,
    ctx: *mut c_void,
) -> *mut ByteBuffer {
    let context = CallbackContext(ctx);
    respond(|| {
        // SAFETY: forwarded from this function's contract.
        let engine = unsafe { engine_ref(engine) }?;
        // SAFETY: forwarded from this function's contract.
        let plan: CleanPlan = unsafe { parse_input(plan, len) }?;
        let progress = |event: &CleanProgress| {
            let (Some(callback), Ok(json)) = (callback, serde_json::to_vec(event)) else {
                return;
            };
            let Ok(json_len) = i32::try_from(json.len()) else {
                return;
            };
            // SAFETY: the caller guarantees the callback is valid for the
            // whole call; `json` outlives this synchronous invocation.
            unsafe { callback(context.get(), json.as_ptr(), json_len) };
        };
        engine.clean_with_progress(&plan, &progress)
    })
}

/// Restores a session, entirely or partly (EF-18): `data` is the `Session`
/// whose `errors` lists the items that could not be restored.
///
/// # Safety
/// `engine` must be a live handle; `request` must point to `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn cs_restore(
    engine: *const EngineHandle,
    request: *const u8,
    len: i32,
) -> *mut ByteBuffer {
    respond(|| {
        // SAFETY: forwarded from this function's contract.
        let engine = unsafe { engine_ref(engine) }?;
        // SAFETY: forwarded from this function's contract.
        let request: RestoreRequest = unsafe { parse_input(request, len) }?;
        engine.restore(&request)
    })
}

/// Sessions of the history, most recent first (EF-20): `data` is a `Session[]`.
///
/// # Safety
/// `engine` must be null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn cs_list_sessions(engine: *const EngineHandle) -> *mut ByteBuffer {
    respond(|| {
        // SAFETY: forwarded from this function's contract.
        let engine = unsafe { engine_ref(engine) }?;
        engine.list_sessions()
    })
}

/// Deletes for good the sessions older than `older_than_days` (EF-19):
/// `data` is `{ freed_bytes, simulated }`.
///
/// # Safety
/// `engine` must be a live handle; `request` must point to `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn cs_purge_quarantine(
    engine: *const EngineHandle,
    request: *const u8,
    len: i32,
) -> *mut ByteBuffer {
    respond(|| {
        // SAFETY: forwarded from this function's contract.
        let engine = unsafe { engine_ref(engine) }?;
        // SAFETY: forwarded from this function's contract.
        let request: PurgeRequest = unsafe { parse_input(request, len) }?;
        engine.purge(&request)
    })
}

/// Runs the official uninstaller of an application and waits for its whole
/// process tree (EF-12 to EF-15): `data` is an `UninstallResult`
/// (`simulated: true` by default). `cs_cancel` stops waiting for an `.exe`
/// uninstaller (never kills it) and asks Windows Installer to roll back an
/// MSI uninstall. `callback` receives `ctx` and a UTF-8 JSON
/// `UninstallProgress` at each step and at most every 250 ms (EF-21); it
/// may be null.
///
/// # Safety
/// `engine` must be a live handle; `request` must point to `len` bytes;
/// `callback`, if not null, must stay valid for the whole call.
#[no_mangle]
pub unsafe extern "C" fn cs_uninstall(
    engine: *const EngineHandle,
    request: *const u8,
    len: i32,
    callback: ProgressCallback,
    ctx: *mut c_void,
) -> *mut ByteBuffer {
    let context = CallbackContext(ctx);
    respond(|| {
        // SAFETY: forwarded from this function's contract.
        let engine = unsafe { engine_ref(engine) }?;
        // SAFETY: forwarded from this function's contract.
        let request: UninstallRequest = unsafe { parse_input(request, len) }?;
        let progress = |event: &UninstallProgress| {
            let (Some(callback), Ok(json)) = (callback, serde_json::to_vec(event)) else {
                return;
            };
            let Ok(json_len) = i32::try_from(json.len()) else {
                return;
            };
            // SAFETY: the caller guarantees the callback is valid for the
            // whole call; `json` outlives this synchronous invocation.
            unsafe { callback(context.get(), json.as_ptr(), json_len) };
        };
        engine.uninstall_with_progress(&request, &progress)
    })
}

/// Leftovers of an application just uninstalled (EF-13): `data` is a
/// `ScanReport` (empty if the application is still installed). Same
/// progress callback as [`cs_scan_orphans`].
///
/// # Safety
/// `engine` must be a live handle; `request` must point to `len` bytes;
/// `callback` may be null.
#[no_mangle]
pub unsafe extern "C" fn cs_scan_app_leftovers(
    engine: *const EngineHandle,
    request: *const u8,
    len: i32,
    callback: ProgressCallback,
    ctx: *mut c_void,
) -> *mut ByteBuffer {
    let context = CallbackContext(ctx);
    respond(|| {
        // SAFETY: forwarded from this function's contract.
        let engine = unsafe { engine_ref(engine) }?;
        // SAFETY: forwarded from this function's contract.
        let request: AppLeftoversRequest = unsafe { parse_input(request, len) }?;
        let progress = |event: &ScanProgress| {
            let (Some(callback), Ok(json)) = (callback, serde_json::to_vec(event)) else {
                return;
            };
            let Ok(json_len) = i32::try_from(json.len()) else {
                return;
            };
            // SAFETY: the caller guarantees the callback is valid for the
            // whole call; `json` outlives this synchronous invocation.
            unsafe { callback(context.get(), json.as_ptr(), json_len) };
        };
        engine.scan_app_leftovers(&request, &progress)
    })
}

/// Storage map of a drive, read-only (D73): `data` is a `DiskMap`. `request` is a
/// `DiskMapRequest` JSON; progress events are `DiskMapProgress` JSON; `cs_cancel` stops it.
/// Returns `BUSY` if another long operation runs on this engine.
///
/// # Safety
/// `engine` must be a live handle; `request` must point to `len` readable bytes;
/// `callback` may be null.
#[no_mangle]
pub unsafe extern "C" fn cs_disk_map(
    engine: *const EngineHandle,
    request: *const u8,
    len: i32,
    callback: ProgressCallback,
    ctx: *mut c_void,
) -> *mut ByteBuffer {
    let context = CallbackContext(ctx);
    respond(|| {
        // SAFETY: forwarded from this function's contract.
        let engine = unsafe { engine_ref(engine) }?;
        // SAFETY: forwarded from this function's contract.
        let bytes = unsafe { input_bytes(request, len) }?;
        let request: cleanslate_core::disk_map::DiskMapRequest = serde_json::from_slice(bytes)?;
        let progress = |event: &cleanslate_core::disk_map::DiskMapProgress| {
            let (Some(callback), Ok(json)) = (callback, serde_json::to_vec(event)) else {
                return;
            };
            let Ok(json_len) = i32::try_from(json.len()) else {
                return;
            };
            // SAFETY: the caller guarantees the callback is valid for the
            // whole call; `json` outlives this synchronous invocation.
            unsafe { callback(context.get(), json.as_ptr(), json_len) };
        };
        engine.disk_map(&request, &progress)
    })
}

/// One request of the Gestionnaire (D79): `request` is a `ManagerRequest` JSON
/// (`{"op": "analyze", …}`); `data` is the answer of that operation. Every action is
/// simulated unless the request says `"dry_run": false` on a real engine; progress
/// events are `ManagerProgress` JSON; `cs_cancel` stops a walk.
///
/// # Safety
/// `engine` must be a live handle; `request` must point to `len` readable bytes;
/// `callback` may be null.
#[no_mangle]
pub unsafe extern "C" fn cs_manager(
    engine: *const EngineHandle,
    request: *const u8,
    len: i32,
    callback: ProgressCallback,
    ctx: *mut c_void,
) -> *mut ByteBuffer {
    let context = CallbackContext(ctx);
    respond(|| {
        // SAFETY: forwarded from this function's contract.
        let engine = unsafe { engine_ref(engine) }?;
        // SAFETY: forwarded from this function's contract.
        let request: cleanslate_core::manager::ManagerRequest =
            unsafe { parse_input(request, len) }?;
        let progress = |event: &cleanslate_core::manager::ManagerProgress| {
            let (Some(callback), Ok(json)) = (callback, serde_json::to_vec(event)) else {
                return;
            };
            let Ok(json_len) = i32::try_from(json.len()) else {
                return;
            };
            // SAFETY: the caller guarantees the callback is valid for the
            // whole call; `json` outlives this synchronous invocation.
            unsafe { callback(context.get(), json.as_ptr(), json_len) };
        };
        engine.manager(&request, &progress)
    })
}

/// Asks the running long operation of `engine` to stop (EF-11). Safe to
/// call from any thread while the operation runs. Null is ignored.
///
/// # Safety
/// `engine` must be null or a live handle from [`cs_engine_new`].
#[no_mangle]
pub unsafe extern "C" fn cs_cancel(engine: *const EngineHandle) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: forwarded from this function's contract.
        if let Ok(engine) = unsafe { engine_ref(engine) } {
            engine.cancel();
        }
    }));
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

    /// Engine on an in-memory disk and an empty, never-written test registry.
    fn fake_engine() -> *mut EngineHandle {
        use cleanslate_core::fs::FakeFileSystem;
        use cleanslate_core::registry::WinRegistry;
        let mut fs = FakeFileSystem::new();
        fs.add_file(
            r"C:\Users\Ana\AppData\Roaming\OldTool\settings.ini",
            10,
            None,
        );
        let registry =
            WinRegistry::sandboxed(r"Software\CleanSlateTest\ffi-tests-never-created").unwrap();
        let engine = Engine::new(EngineConfig::default(), Box::new(fs), Box::new(registry));
        Box::into_raw(Box::new(EngineHandle { engine }))
    }

    unsafe extern "C" fn count_events(ctx: *mut c_void, json: *const u8, len: i32) {
        // SAFETY: test context is an AtomicUsize; json/len come from Rust.
        let counter = unsafe { &*(ctx as *const std::sync::atomic::AtomicUsize) };
        let bytes = unsafe { std::slice::from_raw_parts(json, len as usize) };
        let event: Value = serde_json::from_slice(bytes).unwrap();
        assert!(event["phase"].is_string());
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    #[test]
    fn scan_orphans_with_progress() {
        let engine = fake_engine();
        let counter = std::sync::atomic::AtomicUsize::new(0);
        let options = br#"{"dry_run":true}"#;
        let value = take(unsafe {
            cs_scan_orphans(
                engine,
                options.as_ptr(),
                options.len() as i32,
                Some(count_events),
                &counter as *const _ as *mut c_void,
            )
        });
        unsafe { cs_engine_free(engine) };
        assert_eq!(value["ok"], true, "{value}");
        let groups = value["data"]["groups"].as_array().unwrap();
        assert_eq!(groups[0]["label"], "OldTool");
        assert_eq!(value["data"]["cancelled"], false);
        assert!(counter.load(std::sync::atomic::Ordering::Relaxed) >= 2);
    }

    #[test]
    fn scan_orphans_rejects_bad_input() {
        let engine = fake_engine();
        let bad = b"{oops";
        let value = take(unsafe {
            cs_scan_orphans(
                engine,
                bad.as_ptr(),
                bad.len() as i32,
                None,
                ptr::null_mut(),
            )
        });
        assert_eq!(value["error"]["code"], "INVALID_INPUT");
        let value =
            take(unsafe { cs_scan_orphans(ptr::null(), ptr::null(), 0, None, ptr::null_mut()) });
        assert_eq!(value["error"]["code"], "INVALID_INPUT");
        unsafe {
            cs_cancel(engine);
            cs_cancel(ptr::null());
            cs_engine_free(engine);
        }
    }

    #[test]
    fn clean_is_simulated_by_default_and_history_calls_work() {
        let engine = fake_engine();
        let scan = take(unsafe { cs_scan_orphans(engine, ptr::null(), 0, None, ptr::null_mut()) });
        let id = scan["data"]["groups"][0]["items"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();

        let plan = format!(r#"{{"item_ids":["{id}"],"origin":{{"kind":"orphans"}}}}"#);
        let session = take(unsafe {
            cs_clean(
                engine,
                plan.as_ptr(),
                plan.len() as i32,
                None,
                ptr::null_mut(),
            )
        });
        assert_eq!(session["ok"], true, "{session}");
        assert_eq!(session["data"]["simulated"], true);

        let sessions = take(unsafe { cs_list_sessions(engine) });
        assert_eq!(sessions["data"].as_array().unwrap().len(), 0);

        let purge = br#"{"older_than_days":30}"#;
        let purged =
            take(unsafe { cs_purge_quarantine(engine, purge.as_ptr(), purge.len() as i32) });
        assert_eq!(purged["data"]["simulated"], true);

        let restore = br#"{"session_id":"00000000-0000-0000-0000-000000000000"}"#;
        let restored = take(unsafe { cs_restore(engine, restore.as_ptr(), restore.len() as i32) });
        assert_eq!(restored["ok"], false, "no data folder on a test engine");

        let bad = take(unsafe { cs_clean(engine, ptr::null(), 0, None, ptr::null_mut()) });
        assert_eq!(bad["error"]["code"], "INVALID_INPUT");
        unsafe { cs_engine_free(engine) };
    }

    #[test]
    fn uninstall_and_app_leftovers_report_errors_safely() {
        let engine = fake_engine();
        let unknown = br#"{"app_id":"nope"}"#;
        let value = take(unsafe {
            cs_uninstall(
                engine,
                unknown.as_ptr(),
                unknown.len() as i32,
                None,
                ptr::null_mut(),
            )
        });
        assert_eq!(value["error"]["code"], "NOT_FOUND");
        let value = take(unsafe {
            cs_scan_app_leftovers(
                engine,
                unknown.as_ptr(),
                unknown.len() as i32,
                None,
                ptr::null_mut(),
            )
        });
        assert_eq!(value["error"]["code"], "NOT_FOUND");
        let value = take(unsafe { cs_uninstall(engine, ptr::null(), 0, None, ptr::null_mut()) });
        assert_eq!(value["error"]["code"], "INVALID_INPUT");
        unsafe { cs_engine_free(engine) };
    }

    /// D73: the storage map answers through the envelope, malformed input included.
    #[test]
    fn disk_map_reports_errors_safely() {
        let engine = fake_engine();
        let value = take(unsafe { cs_disk_map(engine, ptr::null(), 0, None, ptr::null_mut()) });
        assert_eq!(value["error"]["code"], "INVALID_INPUT");
        let bad = br#"{"max_depth":3}"#;
        let value = take(unsafe {
            cs_disk_map(
                engine,
                bad.as_ptr(),
                bad.len() as i32,
                None,
                ptr::null_mut(),
            )
        });
        assert_eq!(value["error"]["code"], "INVALID_INPUT");
        let value = take(unsafe {
            cs_disk_map(
                ptr::null(),
                bad.as_ptr(),
                bad.len() as i32,
                None,
                ptr::null_mut(),
            )
        });
        assert_eq!(value["ok"], false);
        unsafe { cs_engine_free(engine) };
    }

    /// D79: the Gestionnaire through the FFI. Analyses answer; a deletion is simulated
    /// by default; a sensitive path is refused outside expert mode; the test engine
    /// never touches the Recycle Bin nor Windows.
    #[test]
    fn manager_analyses_and_simulates_through_the_ffi() {
        let engine = fake_engine();
        let call = |json: &[u8]| {
            take(unsafe {
                cs_manager(
                    engine,
                    json.as_ptr(),
                    json.len() as i32,
                    None,
                    ptr::null_mut(),
                )
            })
        };
        let analysis = call(br#"{"op":"analyze","root":"C:\\Users"}"#);
        assert_eq!(analysis["ok"], true, "{analysis}");
        assert_eq!(analysis["data"]["files"], 1);

        let simulated = call(br#"{"op":"delete","paths":["C:\\Users\\Ana\\AppData\\Roaming\\OldTool","C:\\Windows\\System32"]}"#);
        assert_eq!(simulated["data"]["simulated"], true);
        assert_eq!(simulated["data"]["items"][0]["done"], "simulated");
        assert_eq!(
            simulated["data"]["items"][1]["error"]["code"],
            "EXPERT_REQUIRED"
        );

        let zone = call(br#"{"op":"zone","path":"C:\\Program Files"}"#);
        assert_eq!(zone["data"], "sensitive");
        assert_eq!(
            call(br#"{"op":"system","task":"hibernation","dry_run":false}"#)["data"]["simulated"],
            true
        );
        assert_eq!(
            call(br#"{"op":"recycle_bins"}"#)["error"]["code"],
            "INVALID_INPUT"
        );
        assert_eq!(
            call(br#"{"op":"nonsense"}"#)["error"]["code"],
            "INVALID_INPUT"
        );
        unsafe { cs_engine_free(engine) };
    }

    #[test]
    fn free_functions_ignore_null() {
        unsafe {
            cs_engine_free(ptr::null_mut());
            cs_buffer_free(ptr::null_mut());
        }
    }
}
