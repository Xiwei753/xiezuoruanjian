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

// pub：供 writer-platform-harmony cdylib re-export 并导出 `writer_core_*` 符号。
pub mod app_state_ops;
pub mod editor_session_ops;
pub mod layout_ops;
pub mod project_ops;
pub mod screen_policy_ops;
pub mod search_ops;
pub mod settings_ops;
pub mod starmap_ops;
pub mod sync_ops;
pub mod writing_stats_ops;

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::{Arc, Mutex, OnceLock};

use crate::app_service::WriterAppService;

/// 全局 `WriterAppService` 单例，由 `writer_core_init` 初始化。
///
///   FFI 写操作统一改走 `with_app_service`，
/// `WriterAppService` 由 bootstrap 流程初始化（`ensure_workspace_git` +
/// `recover_storage_transactions`），持有 `GitRepoLayout`，写操作能记 history。
///
/// 持有 `text_edit_session_*` 系列方法所需的多目标会话注册表，
/// 会话跨 FFI 调用持久化，不随单次调用重建。
///
/// ## 线程安全
///
/// 只用 `OnceLock` 保证只初始化一次，**不再用一把全局 `Mutex` 把所有 FFI 串行化**
/// （Issue #829 评论 #5996577737 第 1 项）。
///
/// 之前这里是 `OnceLock<Mutex<Arc<WriterAppService>>>`，`with_app_service` 要先拿锁
/// 再执行整个业务闭包、闭包返回后才释放。于是任何一次耗时调用都会把所有其他 FFI
/// 调用一起堵住：写作统计上报已经放进 Node-API async work 的线程池，但那条链最终
/// 还是 `with_app_service()`，于是 `record_event -> aggregate_single_event ->
/// read/write/rename` 全程持着这把外层锁；而编辑热路径
/// （`writer_core_editor_session_insert/delete/commit_text`）仍是 ArkUI 线程上的同步
/// NAPI，下一次按键就会在这把锁上排队 —— 磁盘 I/O 只是从主线程挪到了线程池，
/// 并没有真正离开输入链路。
///
/// 并发安全由内部锁负责，职责分层如下：
/// - `WriterAppService` 自身：`session_registry: Mutex<_>`、`network_state: Mutex<_>`
/// - `WriterCoreApi` 内部：`RwLock<WriterCore>`（写操作走写锁）
///
/// 即业务并发控制下沉到真正共享的那份状态上，全局 holder 只负责「初始化一次 +
/// 共享所有权」。
static APP_SERVICE: OnceLock<Arc<WriterAppService>> = OnceLock::new();

/// 全局最近一次错误信息，供 `writer_core_get_last_error` 读取。
static LAST_ERROR: OnceLock<Mutex<String>> = OnceLock::new();

/// 记录最近一次错误信息到全局 `LAST_ERROR`。
fn set_last_error(msg: &str) {
    if let Some(m) = LAST_ERROR.get() {
        if let Ok(mut guard) = m.lock() {
            *guard = msg.to_string();
        }
    }
}

/// 在全局 `WriterAppService` 单例上执行闭包。
///
/// ## 线程安全
///
/// `APP_SERVICE` 是全局 `OnceLock<Arc<WriterAppService>>`，**这里不加全局锁**
/// （Issue #829 评论 #5996577737 第 1 项）：并发由 `WriterAppService` 内部
/// （`session_registry` / `network_state` 的 `Mutex`）与 `WriterCoreApi` 的
/// `RwLock<WriterCore>` 负责，闭包只拿不可变引用，可以多线程并发跑。
///
/// 代价：闭包里如果自己再取一个内部 `Mutex` 锁并阻塞很久（比如同步做磁盘 I/O），
/// 那些要拿同一把锁的调用方会被拖住。所以耗时操作应该放线程池的 async work 里
/// （NAPI 侧已经这样做了），不要在持有内部锁时做慢 I/O。
///
/// ## 所有权
///
/// 闭包只获得 `&WriterAppService` 不可变引用。所有修改操作通过内部可变性实现，
/// 不违反只读约束。
pub(crate) fn with_app_service<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&WriterAppService) -> Result<R, String>,
{
    let service = APP_SERVICE.get().ok_or("app service not initialized")?;
    f(service.as_ref())
}

/// load-then-patch：把入参里出现的顶层键覆盖到当前 DTO 上，再反序列化回同一个
/// DTO 落盘。字段名只由 DTO 的 serde 契约决定，FFI 不维护第二张读写映射表
/// （Issue #753 评论 5809590165 第 2、7 条）。
pub(crate) fn patch_dto<T, L, S>(load: L, save: S, payload_json: &str) -> Result<(), String>
where
    T: serde::de::DeserializeOwned + serde::Serialize,
    L: FnOnce() -> Result<T, String>,
    S: FnOnce(T) -> Result<bool, String>,
{
    let patch: serde_json::Value =
        serde_json::from_str(payload_json).map_err(|e| format!("JSON parse error: {e}"))?;
    let patch_obj = patch
        .as_object()
        .ok_or_else(|| "payload must be a JSON object".to_string())?;
    let mut merged = serde_json::to_value(load()?).map_err(|e| format!("{e}"))?;
    let target = merged
        .as_object_mut()
        .ok_or_else(|| "current DTO is not a JSON object".to_string())?;
    for (key, value) in patch_obj {
        target.insert(key.clone(), value.clone());
    }
    let next: T =
        serde_json::from_value(merged).map_err(|e| format!("wire contract error: {e}"))?;
    save(next).map(|_| ())
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
///  -4  = bootstrap failed
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
    let projects_root = std::path::Path::new(&c_str).join("projects");
    std::fs::create_dir_all(&projects_root).ok();
    let app_data_root_str = c_str.clone();
    let projects_root_str = projects_root.to_string_lossy().to_string();
    //   FFI writer_core_init 复用 bootstrap 流程，
    // 让 WriterAppService 持有 GitRepoLayout，写操作能记 workspace history。
    // bootstrap 流程：ensure_workspace_git → recover_storage_transactions →
    // 注入 layout → WriterAppService。与 api::bootstrap::open_app_service 一致。
    let app_service =
        match crate::api::bootstrap::open_app_service(app_data_root_str, projects_root_str) {
            Ok(svc) => svc,
            Err(e) => {
                set_last_error(&format!("bootstrap failed: {}", e));
                return -4;
            }
        };
    APP_SERVICE.get_or_init(|| app_service);
    0
}

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
/// Thread-safe: only touches the dedicated `LAST_ERROR` mutex.
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
    let status = match with_app_service(|_| Ok::<_, String>("native_loaded".to_string())) {
        Ok(s) => s,
        Err(e) => e,
    };
    CString::new(status).unwrap_or_default().into_raw()
}

/// # Safety
/// `text` must be a valid null-terminated UTF-8 C string.
/// Thread-safe: reads the shared `WriterAppService`; no global lock is taken.
/// Returns word count on success, -2 on invalid UTF-8, -3 when the service is
/// not initialized.
#[no_mangle]
// TODO(#597): 既有代码可读性技术债，待后续重构拆分
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    deprecated
)]
pub unsafe extern "C" fn writer_core_calculate_word_count(text: *const c_char) -> i32 {
    let text_str = match c_str_to_rust(text) {
        Ok(s) => s,
        Err(e) => return e,
    };
    with_app_service(|svc| Ok(svc.calculate_word_count(text_str) as i32)).unwrap_or(-3)
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

/// # Safety
/// Thread-safe: reads the shared `WriterAppService`; no global lock is taken.
/// Returns 1 if AI is available, 0 if unavailable or on error.
#[no_mangle]
// TODO(#597): 既有代码可读性技术债，待后续重构拆分
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    deprecated
)]
pub unsafe extern "C" fn writer_core_is_ai_available() -> i32 {
    with_app_service(|svc| Ok(svc.ai_available() as i32)).unwrap_or_default()
}

#[cfg(test)]
mod concurrency_tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;
    use tempfile::{tempdir, TempDir};

    /// 临时目录要活到进程结束：全局 Core 仍引用它，提前删会让后续 FFI 测试读到空目录。
    static TEST_TEMP_DIR: OnceLock<Mutex<Option<TempDir>>> = OnceLock::new();

    fn ensure_core_init() {
        TEST_TEMP_DIR.get_or_init(|| Mutex::new(None));
        if APP_SERVICE.get().is_some() {
            return;
        }
        let dir = tempdir().expect("无法创建临时目录");
        let path = CString::new(dir.path().to_str().unwrap()).unwrap();
        // SAFETY: path 是有效的 NUL-terminated UTF-8 C string。
        let rc = unsafe { writer_core_init(path.as_ptr()) };
        assert_eq!(rc, 0, "writer_core_init 失败");
        if let Ok(mut slot) = TEST_TEMP_DIR.get().unwrap().lock() {
            *slot = Some(dir);
        }
    }

    /// Issue #829 评论 #5996577737 第 1 项的回归测试。
    ///
    /// 之前 `APP_SERVICE` 是 `OnceLock<Mutex<Arc<WriterAppService>>>`，`with_app_service`
    /// 要先拿全局锁再执行整个业务闭包。于是写作统计上报那条慢链（`record_event` 要读
    /// 当日统计 + write + rename）会一路持锁，把 ArkUI 线程上的编辑热路径
    /// （`writer_core_editor_session_insert` 等同步 NAPI）一起堵住 —— 磁盘 I/O 只是从
    /// 主线程挪到了线程池，并没有真正离开输入链路。
    ///
    /// 这里钉住「一个慢闭包不阻塞另一个闭包」：慢的那个在闭包里 sleep（模拟慢 I/O），
    /// 快的那个必须能在它没结束前就返回。若哪天又加回全局锁，这个测试会超时失败。
    #[test]
    fn slow_call_does_not_block_other_app_service_calls() {
        ensure_core_init();
        let (tx_slow_done, rx_slow_done) = mpsc::channel::<()>();
        let (tx_fast_done, rx_fast_done) = mpsc::channel::<bool>();

        let slow = std::thread::spawn(move || {
            with_app_service(|_| {
                std::thread::sleep(Duration::from_millis(400));
                Ok::<_, String>(())
            })
            .expect("慢闭包本身应成功");
            tx_slow_done.send(()).expect("通知慢闭包结束");
        });

        let fast = std::thread::spawn(move || {
            let v =
                with_app_service(|svc| Ok::<_, String>(svc.ai_available())).expect("快闭包应成功");
            tx_fast_done.send(v).expect("通知快闭包结束");
        });

        // 快的那次必须在慢的还在 sleep 时就返回。
        rx_fast_done.recv_timeout(Duration::from_millis(200)).unwrap_or_else(|_| {
            panic!(
                "快闭包被慢闭包阻塞：说明 with_app_service 又变回了全局串行（Issue #829 评论 #5996577737 第 1 项）"
            )
        });

        // 慢闭包最终也会正常结束（没有死锁）。
        rx_slow_done
            .recv_timeout(Duration::from_secs(5))
            .expect("慢闭包应完成，不应死锁");
        fast.join().expect("快线程应正常结束");
        slow.join().expect("慢线程应正常结束");
    }

    /// 去掉全局 Mutex 后，多个 FFI 调用能并发跑进同一个 `WriterAppService`，
    /// 并发安全由内部锁（`session_registry` / `RwLock<WriterCore>`）保证。
    /// 这个测试确认并发调用不会 panic（不会撞上 Mutex 中毒 / 内部状态撕裂）。
    #[test]
    fn concurrent_app_service_calls_do_not_poison() {
        ensure_core_init();
        let handles: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..8 {
                        let _ = with_app_service(|svc| Ok::<_, String>(svc.ai_available()))
                            .expect("并发调用不应报错");
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("并发线程应正常结束，不应 panic");
        }
    }
}
