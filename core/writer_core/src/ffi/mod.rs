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

/// 获取全局 `WriterCore` 单例的互斥锁并执行闭包。
///
/// ## 线程安全
///
/// `CORE` 是全局 `OnceLock<Mutex<Option<WriterCore>>>`。同一时刻只有一个线程可以访问 Core。
/// 调用方不得在闭包中再次调用 `with_core`（非递归锁，会死锁）。
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

/// 将成功数据包装为 JSON ResultEnvelope 并返回 C string。
///
/// ## 所有权
///
/// 返回的 `*mut c_char` 由 Rust 分配，调用方必须用 `writer_core_free_string` 释放。
pub(crate) fn ok_json<T: serde::Serialize>(data: T) -> *mut c_char {
    let envelope = serde_json::json!({
        "success": true,
        "data": data
    });
    let s = serde_json::to_string(&envelope)
        .unwrap_or_else(|_| r#"{"success":false,"errorCode":"SERDE_ERROR"}"#.to_string());
    CString::new(s).unwrap_or_default().into_raw()
}

/// 将错误信息包装为 JSON ResultEnvelope 并返回 C string。
///
/// ## 所有权
///
/// 返回的 `*mut c_char` 由 Rust 分配，调用方必须用 `writer_core_free_string` 释放。
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

/// 将 C string 转换为 Rust `String`。
///
/// ## 错误码
///
/// - `-1`：空指针
/// - `-2`：无效 UTF-8
pub(crate) fn c_str_to_rust(s: *const c_char) -> Result<String, i32> {
    if s.is_null() {
        return Err(-1);
    }
    // SAFETY: s is null-checked above; the C ABI caller guarantees a valid NUL-terminated UTF-8 string.
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
        // SAFETY: ptr is null-checked above; ptr was originally created by CString::into_raw() in rust_str_to_c; caller must ensure no double-free.
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
