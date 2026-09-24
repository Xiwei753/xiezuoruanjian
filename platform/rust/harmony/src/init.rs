//! HarmonyOS 平台初始化构造与诊断后端接入。
//!
//! Harmony 平台通过 NAPI 桥接层把目录信息传进来，构造 `PlatformInit`
//! 并启动统一日志后端。

use std::path::PathBuf;

use writer_platform_api::{PlatformInit, PlatformKind};

/// 从 Harmony 目录信息构造平台初始化结构。
pub fn create_platform_init(
    files_dir: PathBuf,
    cache_dir: PathBuf,
    device_id: String,
    app_version: String,
    locale: String,
    timezone: String,
) -> PlatformInit {
    let log_dir = cache_dir.join("log");
    PlatformInit {
        platform: PlatformKind::Harmony,
        app_data_dir: files_dir,
        cache_dir,
        log_dir,
        no_backup_dir: None,
        device_id,
        app_version,
        locale,
        timezone,
    }
}

/// 把完整 `PlatformInit` 交给 `writer_diagnostics::init`，启动统一日志后端。
///
/// 幂等：重复调用无副作用。
pub fn init_diagnostics(
    init: &PlatformInit,
    build_key: String,
    session_id: String,
    enabled: bool,
    verbose: bool,
) {
    let config = writer_diagnostics::DiagnosticsConfig {
        log_dir: init.log_dir.clone(),
        platform_name: init.platform.to_string(),
        device_id: init.device_id.clone(),
        app_version: init.app_version.clone(),
        build_key,
        locale: init.locale.clone(),
        timezone: init.timezone.clone(),
        session_id,
        enabled,
        verbose,
    };
    writer_diagnostics::init(config);
}

// ── C-ABI diagnostics init 入口 ──

use std::ffi::CStr;
use std::os::raw::c_char;

/// C-ABI diagnostics init 入口 — Issue #670 评论 5652119660 修复 2a / #760 诊断导出。
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

/// 将 C string 转换为 Rust `String`（init 模块私有辅助）。
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
