//! 同步相关 FFI 函数 — 全量同步统一入口（Issue #630）。
//!
//! 一个全局 `SyncConfig` + 一份全局凭据。旧的"作品同步 + 应用数据同步"两套
//! C ABI 已删除，新增 `writer_core_perform_full_sync` 等全量同步入口。
//!
//! ## 线程安全契约
//!
//! 所有函数通过 `with_core` 获取全局 `WriterCore` 单例的 `Mutex` 锁。
//! 调用方不得在回调中再次调用任何 FFI 函数（非递归锁，会死锁）。
//!
//! ## JSON 传递语义
//!
//! 所有复杂数据通过 JSON C string 传递，格式为 `ResultEnvelope`。
//! 调用方必须用 `writer_core_free_string` 释放返回的 C string。

use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_app_service, with_core};

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_sync_config() -> *mut c_char {
    match with_core(|core| {
        let config = core.load_sync_config().map_err(|e| format!("{}", e))?;
        // Issue #645 评论第 2 点：FFI 暴露的旧字段从 provider_config 读取，
        // 保持 C ABI 兼容（旧调用方仍读 remoteUrl/branch/provider）。
        let (remote_url, branch, provider) = match &config.provider_config {
            #[cfg(feature = "github-api")]
            Some(crate::sync::provider::ProviderConfig::GitHub(gh)) => (
                gh.remote_url.clone(),
                gh.branch.clone(),
                "github_api".to_string(),
            ),
            _ => (
                String::new(),
                "main".to_string(),
                config.active_provider.clone(),
            ),
        };
        Ok(serde_json::json!({
            "enabled": config.enabled,
            "provider": provider,
            "remoteUrl": remote_url,
            "branch": branch,
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
        // Issue #645 评论第 2 点：FFI 仍接受旧字段 remoteUrl/branch，
        // 写入 provider_config: ProviderConfig::GitHub。
        // provider::github 模块仅在 github-api feature 下编译，整段逻辑需门控；
        // 无 github-api 时该 block 不编译，FFI 仍保存 enabled/autoSync 等通用字段。
        #[cfg(feature = "github-api")]
        {
            let remote_url = val
                .get("remoteUrl")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let branch = val
                .get("branch")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if remote_url.is_some() || branch.is_some() {
                let existing_gh = match &config.provider_config {
                    Some(crate::sync::provider::ProviderConfig::GitHub(gh)) => Some(gh.clone()),
                    _ => None,
                };
                let defaults =
                    crate::sync::provider::github::config::GitHubProviderConfig::defaults();
                let prev_remote = existing_gh.as_ref().map(|g| g.remote_url.clone());
                let prev_branch = existing_gh.as_ref().map(|g| g.branch.clone());
                let prev_username = existing_gh.as_ref().map(|g| g.username.clone());
                let prev_transport = existing_gh.as_ref().map(|g| g.transport.clone());
                let gh = crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: remote_url.or(prev_remote).unwrap_or(defaults.remote_url),
                    branch: branch.or(prev_branch).unwrap_or(defaults.branch),
                    username: prev_username.unwrap_or(defaults.username),
                    transport: prev_transport.unwrap_or(defaults.transport),
                };
                config.provider_config = Some(crate::sync::provider::ProviderConfig::GitHub(gh));
            }
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

/// 全量同步 dry-run C ABI（Issue #630）。
///
/// #645 评论 5504296097 问题2：改走 `with_app_service` 唯一 pipeline，
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
        let config = svc.load_sync_config_core().map_err(|e| format!("{}", e))?;
        let dto: crate::api::SyncConfigDto = config.into();
        let plan = svc
            .perform_full_sync_dry_run(dto)
            .map_err(|e| format!("{}", e))?;
        Ok(serde_json::to_value(&plan).unwrap_or_default())
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_NETWORK_ERROR", &e),
    }
}

/// 全量同步诊断 C ABI（Issue #630）— 只测一次仓库、分支、token。
///
/// #645 评论 5504296097 问题2：改走 `with_app_service` 唯一 pipeline。
///
/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_full_sync_diagnostics() -> *mut c_char {
    match with_app_service(|svc| {
        let config = svc.load_sync_config_core().map_err(|e| format!("{}", e))?;
        let dto: crate::api::SyncConfigDto = config.into();
        let diag = svc
            .perform_full_sync_diagnostics(dto)
            .map_err(|e| format!("{}", e))?;
        Ok(serde_json::to_value(&diag).unwrap_or_default())
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_NETWORK_ERROR", &e),
    }
}

/// 全量同步 C ABI（Issue #630）— 先 App target，再所有 Project target。
///
/// #645 评论 5504296097 问题2：改走 `with_app_service` 唯一 pipeline，
/// 经 `WriterAppService::perform_full_sync` →
/// `WriterCoreApi::perform_full_sync`（Prepare → Seed → Transfer → Commit）。
/// 旧 facade `with_core(|core| core.perform_full_sync(...))` 不加载
/// pending deleted targets，已删除作品的远端前缀不会被清理；且不走
/// 三段式 staging + workspace history，是第二套并行 pipeline。删除。
///
/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_perform_full_sync() -> *mut c_char {
    match with_app_service(|svc| {
        let config = svc.load_sync_config_core().map_err(|e| format!("{}", e))?;
        let dto: crate::api::SyncConfigDto = config.into();
        let result = svc
            .perform_full_sync(dto, false)
            .map_err(|e| format!("{}", e))?;
        Ok(serde_json::to_value(&result).unwrap_or_default())
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
    match with_core(|core| {
        let state = core.load_app_sync_state().map_err(|e| format!("{}", e))?;
        Ok(serde_json::to_value(&state).unwrap_or_default())
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
    match with_core(|core| {
        let state: crate::sync::SyncState =
            serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))?;
        core.save_app_sync_state(&state)
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
        core.save_device_info(&info).map_err(|e| format!("{}", e))?;
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
