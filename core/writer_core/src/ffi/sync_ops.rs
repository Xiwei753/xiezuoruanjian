//! 同步相关 FFI 函数 — 全量同步统一入口。
//!
//! 一个全局 `SyncConfig` + 一份全局凭据。旧的"作品同步 + 应用数据同步"两套
//! C ABI 已删除，新增 `writer_core_perform_full_sync` 等全量同步入口。
//!
//! ## 线程安全契约
//!
//! 所有函数通过 `with_app_service` 获取全局 `WriterAppService` 单例的 `Mutex` 锁。
//! 调用方不得在回调中再次调用任何 FFI 函数（非递归锁，会死锁）。
//!
//! ## JSON 传递语义
//!
//! 所有复杂数据通过 JSON C string 传递，格式为 `ResultEnvelope`。
//! 调用方必须用 `writer_core_free_string` 释放返回的 C string。

use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, patch_dto, with_app_service};

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_sync_config() -> *mut c_char {
    match with_app_service(|svc| {
        let dto: crate::api::SyncConfigDto =
            svc.load_sync_config().map_err(|e| format!("{}", e))?;
        Ok(dto)
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
    match with_app_service(|svc| {
        patch_dto(
            || svc.load_sync_config().map_err(|e| format!("{e}")),
            |next| svc.save_sync_config(next).map_err(|e| format!("{e}")),
            &json_str,
        )
    }) {
        Ok(()) => ok_json(true),
        Err(e) => err_json("SETTINGS_INVALID", &e),
    }
}

/// 全量同步 dry-run C ABI。
///
///   改走 `with_app_service` 唯一 pipeline，
/// 经 `WriterAppService::perform_full_sync_dry_run` →
/// `WriterCoreApi::perform_full_sync_dry_run` →
/// `WriterCore::perform_full_sync_dry_run`。旧 facade `with_core` 路径
/// 不加载 pending deleted targets，已删除作品的远端前缀不会被清理。
///
/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_full_sync_dry_run() -> *mut c_char {
    match with_app_service(|svc| {
        let config = svc.load_sync_config().map_err(|e| format!("{}", e))?;
        let plan = svc
            .perform_full_sync_dry_run(config)
            .map_err(|e| format!("{}", e))?;
        Ok(plan)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_NETWORK_ERROR", &e),
    }
}

/// 全量同步诊断 C ABI— 只测一次仓库、分支、token。
///
///   改走 `with_app_service` 唯一 pipeline。
///
/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_full_sync_diagnostics() -> *mut c_char {
    match with_app_service(|svc| {
        let config = svc.load_sync_config().map_err(|e| format!("{}", e))?;
        let diag = svc
            .perform_full_sync_diagnostics(config)
            .map_err(|e| format!("{}", e))?;
        Ok(diag)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_NETWORK_ERROR", &e),
    }
}

/// 全量同步 C ABI— 先 App target，再所有 Project target。
///
///   改走 `with_app_service` 唯一 pipeline，
/// 经 `WriterAppService::perform_full_sync` →
/// `WriterCoreApi::perform_full_sync`（Prepare → Seed → Transfer → Commit）。
/// 旧 facade `with_app_service(|svc| svc.perform_full_sync(...))` 不加载
/// pending deleted targets，已删除作品的远端前缀不会被清理；且不走
/// 三段式 staging + workspace history，是第二套并行 pipeline。删除。
///
/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_perform_full_sync() -> *mut c_char {
    match with_app_service(|svc| {
        let config = svc.load_sync_config().map_err(|e| format!("{}", e))?;
        let result = svc
            .perform_full_sync(config, false)
            .map_err(|e| format!("{}", e))?;
        Ok(result)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_NETWORK_ERROR", &e),
    }
}

/// 加载 App target 同步状态。返回 JSON 形式的 `SyncState`。
///
/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_app_sync_state() -> *mut c_char {
    match with_app_service(|svc| {
        let state = svc.load_app_sync_state().map_err(|e| format!("{}", e))?;
        Ok(state)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_STATE_ERROR", &e),
    }
}

/// 保存 App target 同步状态。`state_json` 为 JSON 形式的 `SyncState`。
///
/// # Safety
/// `state_json` must be a valid null-terminated UTF-8 C string containing valid JSON.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_app_sync_state(state_json: *const c_char) -> *mut c_char {
    let json_str = match c_str_to_rust(state_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid state_json: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let state: crate::api::SyncStateDto =
            serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))?;
        svc.save_app_sync_state(state)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_STATE_ERROR", &e),
    }
}

/// # Safety
/// `platform` and `device_class` must be valid null-terminated UTF-8 C strings.
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_device_info() -> *mut c_char {
    match with_app_service(|svc| {
        let info = svc.load_device_info().map_err(|e| format!("{}", e))?;
        Ok(info)
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
    match with_app_service(|svc| {
        patch_dto(
            || svc.load_device_info().map_err(|e| format!("{e}")),
            |next| {
                svc.save_device_info_raw(&next)
                    .map(|()| true)
                    .map_err(|e| format!("{e}"))
            },
            &json_str,
        )
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
    match with_app_service(|svc| {
        svc.ensure_device_info(platform_str, device_class_str)
            .map_err(|e| format!("{}", e))?;
        let info = svc.load_device_info().map_err(|e| format!("{}", e))?;
        Ok(info)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("DEVICE_INFO_ERROR", &e),
    }
}
