//! # 设置 FFI 操作 — 本地设置与可同步设置的 C ABI 入口
//!
//! 所有函数遵循 FFI 契约：
//! - 输入：C 字符串指针（`*const c_char`），由调用方分配和释放
//! - 输出：Rust 分配的 C 字符串指针（`*mut c_char`），调用方必须调用对应的释放函数
//! - 返回值：JSON 字符串，`{"ok": true, "data": ...}` 或 `{"ok": false, "error": ...}`
//!
//! `save_*` 采用 load-then-patch：入参里出现的顶层键覆盖到当前设置 DTO 上，
//! 未提供的键保持原值。字段名只由 DTO 的 serde 契约决定，FFI 不维护第二张映射表。

use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, patch_dto, with_app_service};

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_local_settings() -> *mut c_char {
    match with_app_service(|svc| {
        let settings = svc.load_local_settings().map_err(|e| format!("{}", e))?;
        Ok(settings)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_NOT_FOUND", &e),
    }
}

/// # Safety
/// `settings_json` must be a valid null-terminated UTF-8 C string containing valid JSON.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_local_settings(
    settings_json: *const c_char,
) -> *mut c_char {
    let json_str = match c_str_to_rust(settings_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid settings_json: error {e}"),
            )
        }
    };
    match with_app_service(|svc| {
        patch_dto(
            || svc.load_local_settings().map_err(|e| format!("{e}")),
            |next| svc.save_local_settings(next).map_err(|e| format!("{e}")),
            &json_str,
        )
    }) {
        Ok(()) => ok_json(true),
        Err(e) => err_json("SETTINGS_INVALID", &e),
    }
}

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
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
pub unsafe extern "C" fn writer_core_load_syncable_settings() -> *mut c_char {
    match with_app_service(|svc| {
        let settings = svc.load_syncable_settings().map_err(|e| format!("{}", e))?;
        Ok(settings)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_NOT_FOUND", &e),
    }
}

/// # Safety
/// `settings_json` must be a valid null-terminated UTF-8 C string containing valid JSON.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_syncable_settings(
    settings_json: *const c_char,
) -> *mut c_char {
    let json_str = match c_str_to_rust(settings_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid settings_json: error {e}"),
            )
        }
    };
    match with_app_service(|svc| {
        patch_dto(
            || svc.load_syncable_settings().map_err(|e| format!("{e}")),
            |next| svc.save_syncable_settings(next).map_err(|e| format!("{e}")),
            &json_str,
        )
    }) {
        Ok(()) => ok_json(true),
        Err(e) => err_json("SETTINGS_INVALID", &e),
    }
}

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_list_palette_records() -> *mut c_char {
    match with_app_service(|svc| {
        let records = svc.list_palette_records().map_err(|e| format!("{}", e))?;
        Ok(records)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("PALETTE_LIST_ERROR", &e),
    }
}

/// # Safety
/// `device_id` and `fingerprint` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_palette_record(
    device_id: *const c_char,
    fingerprint: *const c_char,
) -> *mut c_char {
    let device_id_str = match c_str_to_rust(device_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARG", &format!("device_id error: {}", e)),
    };
    let fingerprint_str = match c_str_to_rust(fingerprint) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARG", &format!("fingerprint error: {}", e)),
    };
    match with_app_service(|svc| {
        let record = svc
            .load_palette_record(device_id_str, fingerprint_str)
            .map_err(|e| format!("{}", e))?;
        Ok(record)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("PALETTE_LOAD_ERROR", &e),
    }
}

/// # Safety
/// `device_id` and `fingerprint` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_delete_palette_record(
    device_id: *const c_char,
    fingerprint: *const c_char,
) -> *mut c_char {
    let device_id_str = match c_str_to_rust(device_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARG", &format!("device_id error: {}", e)),
    };
    let fingerprint_str = match c_str_to_rust(fingerprint) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARG", &format!("fingerprint error: {}", e)),
    };
    match with_app_service(|svc| {
        svc.delete_palette_record(device_id_str, fingerprint_str)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("PALETTE_DELETE_ERROR", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// This function does not take any pointer arguments, so there are no additional
/// safety requirements beyond those inherent to FFI boundary calls.
pub unsafe extern "C" fn writer_core_list_builtin_themes() -> *mut c_char {
    match with_app_service(|svc| {
        let themes = svc.list_builtin_themes();
        Ok(themes)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("BUILTIN_THEMES_ERROR", &e),
    }
}
