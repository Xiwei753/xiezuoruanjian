use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Mutex;

use once_cell::sync::OnceCell;

use crate::facade::WriterCore;

static CORE: OnceCell<Mutex<Option<WriterCore>>> = OnceCell::new();

fn with_core<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&WriterCore) -> Result<R, String>,
{
    let guard = CORE
        .get()
        .and_then(|m| m.lock().ok())
        .ok_or("core not initialized")?;
    let core = guard.as_ref().ok_or("core not initialized")?;
    f(core)
}

/// # Safety
/// `path` must be a valid null-terminated UTF-8 C string.
///
/// Return codes:
///   0  = success
///  -1  = null pointer
///  -2  = invalid UTF-8
///  -3  = mutex poisoned
///  -4  = create_workspace failed
#[no_mangle]
pub unsafe extern "C" fn writer_core_init(path: *const c_char) -> i32 {
    if path.is_null() {
        return -1;
    }
    let c_str = match unsafe { CStr::from_ptr(path) }.to_str() {
        Ok(s) => s,
        Err(_) => return -2,
    };
    let core = WriterCore::new(c_str);
    if let Err(_) = core.create_workspace() {
        return -4;
    }
    let m = CORE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = m.lock() {
        *guard = Some(core);
        0
    } else {
        -3
    }
}

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_load_status() -> *mut c_char {
    let status = match with_core(|_| Ok::<_, String>("native_loaded".to_string())) {
        Ok(s) => s,
        Err(e) => e,
    };
    CString::new(status).unwrap_or_default().into_raw()
}

/// # Safety
/// `text` must be a valid null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn writer_core_calculate_word_count(text: *const c_char) -> i32 {
    if text.is_null() {
        return -1;
    }
    let text_str = match unsafe { CStr::from_ptr(text) }.to_str() {
        Ok(s) => s,
        Err(_) => return -2,
    };
    match with_core(|core| Ok(core.calculate_word_count(text_str) as i32)) {
        Ok(count) => count,
        Err(_) => -3,
    }
}

/// # Safety
/// `ptr` must have been returned by a `writer_core_*` function that returns `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe { drop(CString::from_raw(ptr)) };
    }
}
