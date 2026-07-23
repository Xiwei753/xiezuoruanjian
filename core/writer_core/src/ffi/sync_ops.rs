//! 同步相关 FFI 函数 — 暴露同步配置、执行、诊断和冲突解决操作。
//!
//! ## 线程安全契约
//!
//! 所有函数通过 `with_core` 获取全局 `WriterCore` 单例的 `Mutex` 锁。
//! 调用方不得在回调中再次调用任何 FFI 函数（非递归锁，会死锁）。
//! NAPI 桥接层在主线程调用这些函数，同步引擎在后台线程通过命令队列通信，
//! 不直接调用 FFI 函数。
//!
//! ## JSON 传递语义
//!
//! 所有复杂数据通过 JSON C string 传递，格式为 `ResultEnvelope`：
//! - 成功：`{"success": true, "data": ...}`
//! - 失败：`{"success": false, "errorCode": "...", "userMessage": "..."}`
//!
//! 调用方必须用 `writer_core_free_string` 释放返回的 C string。
//! 输入的 C string 由调用方拥有，Rust 侧只读取不释放。

use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_core};

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_sync_config() -> *mut c_char {
    match with_core(|core| {
        let config = core.load_sync_config().map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "enabled": config.enabled,
            "provider": format!("{:?}", config.backend_type).to_lowercase(),
            "remoteUrl": config.remote_url,
            "branch": config.branch,
            "autoSync": config.auto_sync,
            "conflictStrategy": "manual"
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_NOT_FOUND", &e),
    }
}

/// # Safety
/// `config_json` must be a valid null-terminated UTF-8 C string containing valid JSON.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_sync_config(config_json: *const c_char) -> *mut c_char {
    let json_str = match c_str_to_rust(config_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid config_json: error {}", e),
            )
        }
    };
    match with_core(|core| {
        let mut config = core.load_sync_config().map_err(|e| format!("{}", e))?;
        let val: serde_json::Value =
            serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))?;
        if let Some(v) = val.get("enabled").and_then(|v| v.as_bool()) {
            config.enabled = v;
        }
        if let Some(v) = val.get("remoteUrl").and_then(|v| v.as_str()) {
            config.remote_url = v.to_string();
        }
        if let Some(v) = val.get("branch").and_then(|v| v.as_str()) {
            config.branch = v.to_string();
        }
        if let Some(v) = val.get("autoSync").and_then(|v| v.as_bool()) {
            config.auto_sync = v;
        }
        core.save_sync_config(&config)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_INVALID", &e),
    }
}

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_sync_dry_run() -> *mut c_char {
    match with_core(|core| {
        let config = core.load_sync_config().map_err(|e| format!("{}", e))?;
        let plan = core
            .perform_sync_dry_run(&config)
            .map_err(|e| format!("{}", e))?;
        Ok(serde_json::to_value(&plan).unwrap_or_default())
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_NETWORK_ERROR", &e),
    }
}

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_sync_diagnostics() -> *mut c_char {
    match with_core(|core| {
        let config = core.load_sync_config().map_err(|e| format!("{}", e))?;
        let diag = core
            .perform_sync_diagnostics(&config)
            .map_err(|e| format!("{}", e))?;
        Ok(serde_json::to_value(&diag).unwrap_or_default())
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_NETWORK_ERROR", &e),
    }
}

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_perform_sync() -> *mut c_char {
    match with_core(|core| {
        let config = core.load_sync_config().map_err(|e| format!("{}", e))?;
        let result = core.perform_sync(&config).map_err(|e| format!("{}", e))?;
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_NETWORK_ERROR", &e),
    }
}

/// # Safety
/// `platform` and `device_class` must be valid null-terminated UTF-8 C strings.
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_device_info() -> *mut c_char {
    match with_core(|core| {
        let info = core.load_device_info().map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "deviceId": info.device_id,
            "deviceClass": info.device_class,
            "platform": info.platform,
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("DEVICE_INFO_ERROR", &e),
    }
}

/// # Safety
/// `device_info_json` must be a valid null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_device_info(
    device_info_json: *const c_char,
) -> *mut c_char {
    let json_str = match c_str_to_rust(device_info_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid device_info_json: error {}", e),
            )
        }
    };
    match with_core(|core| {
        let val: serde_json::Value =
            serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))?;
        let mut info = core.load_device_info().map_err(|e| format!("{}", e))?;
        if let Some(v) = val.get("deviceId").and_then(|v| v.as_str()) {
            info.device_id = v.to_string();
        }
        if let Some(v) = val.get("deviceClass").and_then(|v| v.as_str()) {
            info.device_class = v.to_string();
        }
        if let Some(v) = val.get("platform").and_then(|v| v.as_str()) {
            info.platform = v.to_string();
        }
        core.save_device_info(&info)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("DEVICE_INFO_ERROR", &e),
    }
}

/// # Safety
/// `platform` and `device_class` must be valid null-terminated UTF-8 C strings.
#[no_mangle]
pub unsafe extern "C" fn writer_core_ensure_device_info(
    platform: *const c_char,
    device_class: *const c_char,
) -> *mut c_char {
    let platform_str = match c_str_to_rust(platform) {
        Ok(s) => {
            if s.len() > 64
                || !s
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return err_json("INVALID_ARGUMENT", "Invalid platform format");
            }
            s
        }
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid platform: error {}", e),
            )
        }
    };
    let device_class_str = match c_str_to_rust(device_class) {
        Ok(s) => {
            if s.len() > 64
                || !s
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return err_json("INVALID_ARGUMENT", "Invalid device_class format");
            }
            s
        }
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid device_class: error {}", e),
            )
        }
    };
    match with_core(|core| {
        let info = core
            .ensure_device_info(&platform_str, &device_class_str, None)
            .map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "deviceId": info.device_id,
            "deviceClass": info.device_class,
            "platform": info.platform,
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("DEVICE_INFO_ERROR", &e),
    }
}

/// 校验平台/设备类别标识符的合法性。
///
/// 仅允许 ASCII 字母数字、下划线和连字符，最长 64 字符。
/// 此限制确保标识符可安全嵌入文件路径和 Git 分支名，无需额外转义。
fn validate_platform_identifier(s: &str) -> bool {
    s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_platform_identifier_valid() {
        assert!(validate_platform_identifier("android"));
        assert!(validate_platform_identifier("linux_qt"));
        assert!(validate_platform_identifier("windows-x86"));
        assert!(validate_platform_identifier("a"));
    }

    #[test]
    fn test_validate_platform_identifier_invalid_chars() {
        assert!(!validate_platform_identifier("android!"));
        assert!(!validate_platform_identifier("linux qt"));
        assert!(!validate_platform_identifier("win.dows"));
    }

    #[test]
    fn test_validate_platform_identifier_too_long() {
        let long = "a".repeat(65);
        assert!(!validate_platform_identifier(&long));
        let ok = "a".repeat(64);
        assert!(validate_platform_identifier(&ok));
    }

    #[test]
    fn test_validate_platform_identifier_empty() {
        assert!(validate_platform_identifier(""));
    }
}
