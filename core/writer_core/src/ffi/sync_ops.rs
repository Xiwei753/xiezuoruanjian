use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_core};

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
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid platform: error {}", e),
            )
        }
    };
    let device_class_str = match c_str_to_rust(device_class) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid device_class: error {}", e),
            )
        }
    };
    match with_core(|core| {
        let info = core
            .ensure_device_info(&platform_str, &device_class_str)
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
