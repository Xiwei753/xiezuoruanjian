//! # HarmonyOS 平台适配层
//!
//! 组装 HarmonyOS / OHOS 端最终 `cdylib`：包含通用核心与 C-ABI FFI 层，
//! 供 NAPI 桥接层（`apps/harmony/entry/src/main/cpp`）在链接期解析
//! `writer_core_*` 符号。
//!
//! ## 依赖方向
//!
//! ```text
//! ArkTS → NAPI (C++) → writer-platform-harmony (cdylib) → writer_core::ffi
//! ```
//!
//! 构建入口见 `tools/build_harmony.sh`；产物复制为
//! `apps/harmony/entry/src/main/prebuilt/arm64-v8a/libwriter_core_ffi.so`。

// 逐模块 re-export C-ABI 入口：既是符号引用（把各目标文件拉进 cdylib 并导出），
// 也是 NAPI 桥接层可调用的 `writer_core_*` 函数清单。
#[allow(unused_imports)]
pub use writer_core::ffi::{
    app_state_ops::*, editor_session_ops::*, layout_ops::*, project_ops::*, screen_policy_ops::*,
    search_ops::*, settings_ops::*, starmap_ops::*, sync_ops::*, writing_stats_ops::*,
};
#[allow(unused_imports)]
pub use writer_core::ffi::{
    writer_core_free_string, writer_core_get_last_error, writer_core_get_load_status,
    writer_core_init,
};

mod init;

pub use init::*;

use std::ffi::CStr;
use std::os::raw::c_char;

/// C-ABI diagnostics init 入口 — Issue #670 评论 5652119660 修复 2a。
///
/// 供 NAPI 桥接层（`napi_init.cpp` 的 `NativeInitDiagnostics`）调用，
/// 把目录/设备/版本/locale/timezone 等信息从 ArkTS 传进 Rust，
/// 构造 `DiagnosticsConfig` 并启动统一日志后端。
///
/// ## 参数
///
/// 所有 `*const c_char` 参数均为 NUL-terminated UTF-8 C 字符串。任一为 null 时
/// 返回 `-1`（无效参数），不构造 config、不调用 `writer_diagnostics::init`。
///
/// ## 返回码
///
/// - `0`  成功
/// - `-1` 任一参数为 null
/// - `-2` 任一参数含无效 UTF-8
///
/// ## Safety
///
/// 调用方必须保证非 null 参数指向有效的 NUL-terminated UTF-8 C 字符串。
#[no_mangle]
pub unsafe extern "C" fn writer_core_init_diagnostics(
    log_dir: *const c_char,
    device_id: *const c_char,
    app_version: *const c_char,
    build_key: *const c_char,
    locale: *const c_char,
    timezone: *const c_char,
) -> i32 {
    // SAFETY: 各参数 null 检查后再 CStr::from_ptr；调用方保证非 null 指针指向有效 NUL-terminated UTF-8。
    let log_dir_str = match c_str_to_rust(log_dir) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let device_id_str = match c_str_to_rust(device_id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let app_version_str = match c_str_to_rust(app_version) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let build_key_str = match c_str_to_rust(build_key) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let locale_str = match c_str_to_rust(locale) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let timezone_str = match c_str_to_rust(timezone) {
        Ok(s) => s,
        Err(e) => return e,
    };

    // session_id 在 Rust 里生成 — 每次启动唯一。
    let session_id = uuid::Uuid::new_v4().to_string();

    let config = writer_diagnostics::DiagnosticsConfig {
        log_dir: std::path::PathBuf::from(log_dir_str),
        platform_name: "harmony".to_string(),
        device_id: device_id_str,
        app_version: app_version_str,
        build_key: build_key_str,
        locale: locale_str,
        timezone: timezone_str,
        session_id,
        enabled: true,
        verbose: true,
    };
    writer_diagnostics::init(config);
    0
}

/// 将 C string 转换为 Rust `String`。
///
/// ## 错误码
///
/// - `-1`：空指针
/// - `-2`：无效 UTF-8
fn c_str_to_rust(s: *const c_char) -> Result<String, i32> {
    if s.is_null() {
        return Err(-1);
    }
    // SAFETY: s is null-checked above; the C ABI caller guarantees a valid NUL-terminated UTF-8 string.
    match unsafe { CStr::from_ptr(s) }.to_str() {
        Ok(s) => Ok(s.to_string()),
        Err(_) => Err(-2),
    }
}
