//! # 设置 FFI 操作 — 本地设置与可同步设置的 C ABI 入口
//!
//! 所有函数遵循 FFI 契约：
//! - 输入：C 字符串指针（`*const c_char`），由调用方分配和释放
//! - 输出：Rust 分配的 C 字符串指针（`*mut c_char`），调用方必须调用对应的释放函数
//! - 返回值：JSON 字符串，`{"ok": true, "data": ...}` 或 `{"ok": false, "error": ...}`
//!
//! `save_*` 函数采用 load-then-patch 模式：先加载当前设置，再按 JSON 中
//! 提供的字段逐一覆盖，未提供的字段保持原值。这允许平台端部分更新设置。

use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_app_service};

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
pub unsafe extern "C" fn writer_core_save_local_settings(
    settings_json: *const c_char,
) -> *mut c_char {
    let json_str = match c_str_to_rust(settings_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid settings_json: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let mut settings = svc.load_local_settings().map_err(|e| format!("{}", e))?;
        let val: serde_json::Value =
            serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))?;
        // 字段名与 LocalSettingsDto 的 camelCase 序列化契约一致。
        if let Some(v) = val.get("editorFontSize").and_then(|v| v.as_f64()) {
            settings.editor_font_size = v as f32;
        }
        if let Some(v) = val
            .get("editorLineSpacingMultiplier")
            .and_then(|v| v.as_f64())
        {
            settings.editor_line_spacing_multiplier = v as f32;
        }
        if let Some(v) = val.get("autoSaveEnabled").and_then(|v| v.as_bool()) {
            settings.auto_save_enabled = v;
        }
        if let Some(v) = val.get("autoSaveDelayMs").and_then(|v| v.as_u64()) {
            settings.auto_save_delay_ms = v;
        }
        if let Some(v) = val.get("autoIndentEnabled").and_then(|v| v.as_bool()) {
            settings.auto_indent_enabled = v;
        }
        if let Some(v) = val.get("autoIndentWidth").and_then(|v| v.as_f64()) {
            settings.auto_indent_width = v as f32;
        }
        // themeMode 兼容别名：写入 appearanceMode。
        if let Some(v) = val.get("themeMode").and_then(|v| v.as_str()) {
            settings.appearance_mode = v.to_string();
        }
        if let Some(v) = val.get("appearanceMode").and_then(|v| v.as_str()) {
            settings.appearance_mode = v.to_string();
        }
        if let Some(v) = val.get("colorSource").and_then(|v| v.as_str()) {
            settings.color_source = v.to_string();
        }
        if let Some(v) = val.get("dynamicColorEnabled").and_then(|v| v.as_bool()) {
            settings.dynamic_color_enabled = v;
        }
        if let Some(v) = val.get("selectedBuiltinThemeId").and_then(|v| v.as_str()) {
            settings.selected_builtin_theme_id = v.to_string();
        }
        if let Some(v) = val.get("selectedPaletteId").and_then(|v| v.as_str()) {
            settings.selected_palette_id = v.to_string();
        }
        if let Some(v) = val.get("locale").and_then(|v| v.as_str()) {
            settings.locale = Some(v.to_string());
        }
        if let Some(v) = val.get("windowWidth").and_then(|v| v.as_f64()) {
            settings.window_width = v as f32;
        }
        if let Some(v) = val.get("windowHeight").and_then(|v| v.as_f64()) {
            settings.window_height = v as f32;
        }
        if let Some(v) = val.get("desktopSidebarWidth").and_then(|v| v.as_f64()) {
            settings.desktop_sidebar_width = v;
        }
        if let Some(v) = val.get("desktopEditorWidth").and_then(|v| v.as_f64()) {
            settings.desktop_editor_width = v;
        }
        if let Some(v) = val
            .get("editorTypingAnimationEnabled")
            .and_then(|v| v.as_bool())
        {
            settings.editor_typing_animation_enabled = v;
        }
        if let Some(v) = val
            .get("editorSmoothCursorEnabled")
            .and_then(|v| v.as_bool())
        {
            settings.editor_smooth_cursor_enabled = v;
        }
        if let Some(v) = val
            .get("editorTypingAnimationDurationMs")
            .and_then(|v| v.as_u64())
        {
            settings.editor_typing_animation_duration_ms = v;
        }
        if let Some(v) = val
            .get("editorSmoothCursorDurationMs")
            .and_then(|v| v.as_u64())
        {
            settings.editor_smooth_cursor_duration_ms = v;
        }
        if let Some(v) = val.get("aiEnabled").and_then(|v| v.as_bool()) {
            settings.ai_enabled = v;
        }
        if let Some(v) = val.get("statsDeviceId").and_then(|v| v.as_str()) {
            settings.stats_device_id = Some(v.to_string());
        }
        if let Some(v) = val
            .get("editorCoordinatedTextCursorAnimationEnabled")
            .and_then(|v| v.as_bool())
        {
            settings.editor_coordinated_text_cursor_animation_enabled = v;
        }
        if let Some(v) = val.get("diagnosticsEnabled").and_then(|v| v.as_bool()) {
            settings.diagnostics_enabled = v;
        }
        if let Some(v) = val.get("diagnosticsVerbose").and_then(|v| v.as_bool()) {
            settings.diagnostics_verbose = v;
        }
        svc.save_local_settings(settings)
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
#[allow(deprecated)]
pub unsafe extern "C" fn writer_core_save_syncable_settings(
    settings_json: *const c_char,
) -> *mut c_char {
    let json_str = match c_str_to_rust(settings_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid settings_json: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let mut settings = svc.load_syncable_settings().map_err(|e| format!("{}", e))?;
        let val: serde_json::Value =
            serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))?;
        // 字段名与 SyncableSettingsDto 的 camelCase 序列化契约一致。
        if let Some(v) = val.get("fontSize").and_then(|v| v.as_f64()) {
            settings.font_size = v;
        }
        if let Some(v) = val.get("themeMode").and_then(|v| v.as_str()) {
            settings.theme_mode = v.to_string();
        }
        #[allow(deprecated)]
        if let Some(v) = val.get("monetColor").and_then(|v| v.as_str()) {
            settings.monet_color = v.to_string();
        }
        if let Some(v) = val.get("themePaletteJson").and_then(|v| v.as_str()) {
            settings.theme_palette_json = v.to_string();
        }
        svc.save_syncable_settings(settings)
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
