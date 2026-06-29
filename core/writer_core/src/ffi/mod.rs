//! # C-ABI FFI 层（HarmonyOS / OHOS）
//!
//! 通过 C ABI 暴露 WriterCore 操作，供 NAPI 桥接层调用。
//! 所有复杂数据通过 JSON 字符串传递：Rust 序列化 → C string → NAPI → ArkTS JSON.parse。
//!
//! ## 设计原则
//!
//! - 简单标量直接返回 i32
//! - 复杂数据（struct/vec）返回 JSON C string，调用方须用 `writer_core_free_string` 释放
//! - 错误通过负数返回码或 JSON ResultEnvelope 传递
//! - 所有函数要求先调用 `writer_core_init` 初始化全局单例

mod layout_ops;
mod project_ops;
mod screen_policy_ops;
mod settings_ops;
mod starmap_ops;
mod sync_ops;
mod workspace_ops;
mod writing_stats_ops;

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Mutex;

use once_cell::sync::OnceCell;

use crate::editor::{EditorEngine, EditorSelection, EditorTransactionCause};
use crate::facade::WriterCore;

pub(crate) static CORE: OnceCell<Mutex<Option<WriterCore>>> = OnceCell::new();
static LAST_ERROR: OnceCell<Mutex<String>> = OnceCell::new();

pub(crate) fn set_last_error(msg: &str) {
    if let Some(m) = LAST_ERROR.get() {
        if let Ok(mut guard) = m.lock() {
            *guard = msg.to_string();
        }
    }
}

pub(crate) fn with_core<F, R>(f: F) -> Result<R, String>
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

pub(crate) fn ok_json<T: serde::Serialize>(data: T) -> *mut c_char {
    let envelope = serde_json::json!({
        "success": true,
        "data": data
    });
    let s = serde_json::to_string(&envelope)
        .unwrap_or_else(|_| r#"{"success":false,"errorCode":"SERDE_ERROR"}"#.to_string());
    CString::new(s).unwrap_or_default().into_raw()
}

pub(crate) fn err_json(code: &str, msg: &str) -> *mut c_char {
    let envelope = serde_json::json!({
        "success": false,
        "errorCode": code,
        "userMessage": msg
    });
    let s = serde_json::to_string(&envelope).unwrap_or_else(|_| {
        format!(
            r#"{{"success":false,"errorCode":"{}","userMessage":"{}"}}"#,
            code, msg
        )
    });
    CString::new(s).unwrap_or_default().into_raw()
}

pub(crate) fn c_str_to_rust(s: *const c_char) -> Result<String, i32> {
    if s.is_null() {
        return Err(-1);
    }
    match unsafe { CStr::from_ptr(s) }.to_str() {
        Ok(s) => Ok(s.to_string()),
        Err(_) => Err(-2),
    }
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
    let _ = LAST_ERROR.get_or_init(|| Mutex::new(String::new()));
    let c_str = match c_str_to_rust(path) {
        Ok(s) => s,
        Err(e) => {
            set_last_error("path is null or invalid UTF-8");
            return e;
        }
    };
    let core = WriterCore::new(&c_str);
    if let Err(e) = core.create_workspace() {
        let msg = format!("create_workspace failed: {}", e);
        set_last_error(&msg);
        return -4;
    }
    let m = CORE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = m.lock() {
        *guard = Some(core);
        0
    } else {
        set_last_error("mutex poisoned");
        -3
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_get_last_error() -> *mut c_char {
    let msg = LAST_ERROR
        .get()
        .and_then(|m| m.lock().ok())
        .map(|g| g.clone())
        .unwrap_or_default();
    CString::new(msg).unwrap_or_default().into_raw()
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
    let text_str = match c_str_to_rust(text) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match with_core(|core| Ok(core.calculate_word_count(&text_str) as i32)) {
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

#[no_mangle]
pub unsafe extern "C" fn writer_core_is_ai_available() -> i32 {
    match with_core(|core| Ok(core.ai_available() as i32)) {
        Ok(v) => v,
        Err(_) => 0,
    }
}

/// # Safety
/// `old_text`, `new_text`, `cause` must be valid null-terminated UTF-8 C strings.
/// `old_cursor_index` and `new_cursor_index` are UTF-8 byte offsets.
/// Returns a caller-owned JSON C string. Free with `writer_core_free_string`.
///
/// This is a **stateless** function that does not require `writer_core_init`.
/// It computes animation events purely from the input parameters.
#[no_mangle]
pub unsafe extern "C" fn writer_core_editor_animation_events(
    old_text: *const c_char,
    new_text: *const c_char,
    old_cursor_index: u32,
    new_cursor_index: u32,
    cause: *const c_char,
    max_animated_chars: u32,
    animation_duration_ms: u32,
) -> *mut c_char {
    let old = match c_str_to_rust(old_text) {
        Ok(s) => s,
        Err(_) => return err_json("INVALID_ARG", "old_text is null or invalid UTF-8"),
    };
    let new = match c_str_to_rust(new_text) {
        Ok(s) => s,
        Err(_) => return err_json("INVALID_ARG", "new_text is null or invalid UTF-8"),
    };
    let cause_str = match c_str_to_rust(cause) {
        Ok(s) => s,
        Err(_) => return err_json("INVALID_ARG", "cause is null or invalid UTF-8"),
    };

    let core_cause = match cause_str.as_str() {
        "Typing" => EditorTransactionCause::Typing,
        "Delete" => EditorTransactionCause::Delete,
        "ImeComposition" => EditorTransactionCause::ImeComposition,
        "TypingCommit" => EditorTransactionCause::TypingCommit,
        "Paste" => EditorTransactionCause::Paste,
        "Undo" => EditorTransactionCause::Undo,
        "Redo" => EditorTransactionCause::Redo,
        "Load" => EditorTransactionCause::Load,
        "Format" => EditorTransactionCause::Format,
        "Programmatic" => EditorTransactionCause::Programmatic,
        _ => EditorTransactionCause::Typing,
    };

    let old_sel = EditorSelection::collapsed(&old, old_cursor_index as usize);
    let new_sel = EditorSelection::collapsed(&new, new_cursor_index as usize);

    let mut engine = EditorEngine::with_animation_limits(
        max_animated_chars as usize,
        animation_duration_ms as u64,
    );
    let transaction = engine.create_transaction(old, new, old_sel, new_sel, core_cause);
    #[allow(deprecated)]
    let events = engine.animation_events(&transaction);

    ok_json(events)
}

/// # Safety
/// `old_text`, `new_text`, `cause` must be valid null-terminated UTF-8 C strings.
/// `old_cursor_index` and `new_cursor_index` are UTF-8 byte offsets.
/// Returns a caller-owned JSON C string (null when no animation is warranted).
/// Free with `writer_core_free_string`.
///
/// This is a **stateless** function that does not require `writer_core_init`.
/// It computes a visual transaction purely from the input parameters.
/// When the change should not be animated (e.g., Paste, Load, multi-change),
/// returns a JSON string with `data: null`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_editor_visual_transaction(
    old_text: *const c_char,
    new_text: *const c_char,
    old_cursor_index: u32,
    new_cursor_index: u32,
    cause: *const c_char,
    max_animated_chars: u32,
    animation_duration_ms: u32,
) -> *mut c_char {
    let old = match c_str_to_rust(old_text) {
        Ok(s) => s,
        Err(_) => return err_json("INVALID_ARG", "old_text is null or invalid UTF-8"),
    };
    let new = match c_str_to_rust(new_text) {
        Ok(s) => s,
        Err(_) => return err_json("INVALID_ARG", "new_text is null or invalid UTF-8"),
    };
    let cause_str = match c_str_to_rust(cause) {
        Ok(s) => s,
        Err(_) => return err_json("INVALID_ARG", "cause is null or invalid UTF-8"),
    };

    let core_cause = match cause_str.as_str() {
        "Typing" => EditorTransactionCause::Typing,
        "Delete" => EditorTransactionCause::Delete,
        "ImeComposition" => EditorTransactionCause::ImeComposition,
        "TypingCommit" => EditorTransactionCause::TypingCommit,
        "Paste" => EditorTransactionCause::Paste,
        "Undo" => EditorTransactionCause::Undo,
        "Redo" => EditorTransactionCause::Redo,
        "Load" => EditorTransactionCause::Load,
        "Format" => EditorTransactionCause::Format,
        "Programmatic" => EditorTransactionCause::Programmatic,
        _ => EditorTransactionCause::Typing,
    };

    let old_sel = EditorSelection::collapsed(&old, old_cursor_index as usize);
    let new_sel = EditorSelection::collapsed(&new, new_cursor_index as usize);

    let mut engine = EditorEngine::with_animation_limits(
        max_animated_chars as usize,
        animation_duration_ms as u64,
    );
    let transaction = engine.create_transaction(&old, &new, old_sel, new_sel, core_cause);
    let vt = engine.visual_transaction(&transaction);

    match vt {
        Some(vt_data) => ok_json(vt_data),
        None => {
            // Return JSON with data: null to indicate no animation warranted
            let envelope = serde_json::json!({
                "success": true,
                "data": null
            });
            let s = serde_json::to_string(&envelope)
                .unwrap_or_else(|_| r#"{"success":true,"data":null}"#.to_string());
            CString::new(s).unwrap_or_default().into_raw()
        }
    }
}
