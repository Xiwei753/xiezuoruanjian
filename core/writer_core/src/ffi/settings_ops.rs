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

use super::{c_str_to_rust, err_json, ok_json, with_core};

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_local_settings() -> *mut c_char {
    match with_core(|core| {
        let settings = core.load_local_settings().map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "fontSize": settings.editor_font_size,
            "lineHeight": settings.editor_line_spacing_multiplier,
            "fontFamily": "HarmonyOS Sans",
            "theme": settings.appearance_mode.as_str(),
            "appearanceMode": settings.appearance_mode,
            "colorSource": settings.color_source,
            "dynamicColorEnabled": settings.dynamic_color_enabled,
            "selectedBuiltinThemeId": settings.selected_builtin_theme_id,
            "selectedPaletteId": settings.selected_palette_id,
            "autoSave": settings.auto_save_enabled,
            "autoSaveInterval": settings.auto_save_delay_ms as f64 / 1000.0,
            "autoIndent": settings.auto_indent_enabled,
            "showWordCount": true,
            "showLineNumbers": false,
            "wordWrap": true,
            "spellCheck": false
        }))
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
    match with_core(|core| {
        let mut settings = core.load_local_settings().map_err(|e| format!("{}", e))?;
        let val: serde_json::Value =
            serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))?;
        if let Some(v) = val.get("fontSize").and_then(|v| v.as_f64()) {
            settings.editor_font_size = v as f32;
        }
        if let Some(v) = val.get("lineHeight").and_then(|v| v.as_f64()) {
            settings.editor_line_spacing_multiplier = v as f32;
        }
        if let Some(v) = val.get("autoSave").and_then(|v| v.as_bool()) {
            settings.auto_save_enabled = v;
        }
        if let Some(v) = val.get("autoSaveInterval").and_then(|v| v.as_f64()) {
            settings.auto_save_delay_ms = (v * 1000.0) as u64;
        }
        if let Some(v) = val.get("autoIndent").and_then(|v| v.as_bool()) {
            settings.auto_indent_enabled = v;
        }
        if let Some(v) = val.get("theme").and_then(|v| v.as_str()) {
            settings.appearance_mode = v.to_string();
            settings.theme_mode = Some(v.to_string());
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
        core.save_local_settings(&settings)
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
    match with_core(|core| {
        let settings = core
            .load_syncable_settings()
            .map_err(|e| format!("{}", e))?;
        // theme_palette 字段被标记为 deprecated，但 FFI 仍需读取它序列化给平台端。
        // 用 struct 的 Serialize 实现作为唯一事实来源，避免手写巨型 JSON 宏
        // 与 struct 字段漂移，同时规避 serde_json::json! 嵌套过深触发递归限制。
        #[allow(deprecated)]
        let palette_value = serde_json::to_value(&settings.theme_palette)
            .map_err(|e| format!("palette serialize error: {}", e))?;
        #[allow(deprecated)]
        let monet_color = settings.monet_color.clone();
        Ok(serde_json::json!({
            "fontSize": settings.font_size,
            "theme": settings.theme_mode,
            "monetColor": monet_color,
            "themePalette": palette_value,
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_NOT_FOUND", &e),
    }
}

/// # Safety
/// `settings_json` must be a valid null-terminated UTF-8 C string containing valid JSON.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
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
    match with_core(|core| {
        let mut settings = core
            .load_syncable_settings()
            .map_err(|e| format!("{}", e))?;
        let val: serde_json::Value =
            serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))?;
        if let Some(v) = val.get("fontSize").and_then(|v| v.as_f64()) {
            settings.font_size = v;
        }
        if let Some(v) = val.get("theme").and_then(|v| v.as_str()) {
            settings.theme_mode = v.to_string();
        }
        #[allow(deprecated)]
        if let Some(v) = val.get("monetColor").and_then(|v| v.as_str()) {
            settings.monet_color = v.to_string();
        }
        // Parse themePalette object
        if let Some(tp) = val.get("themePalette") {
            let palette = &mut settings.theme_palette;
            if let Some(v) = tp.get("source").and_then(|v| v.as_str()) {
                palette.source = v.to_string();
            }
            if let Some(v) = tp.get("updatedAtMs").and_then(|v| v.as_i64()) {
                palette.updated_at_ms = v;
            }
            if let Some(v) = tp.get("deviceId").and_then(|v| v.as_str()) {
                palette.device_id = v.to_string();
            }
            if let Some(v) = tp.get("variant").and_then(|v| v.as_str()) {
                palette.variant = v.to_string();
            }
            if let Some(v) = tp.get("lightPrimary").and_then(|v| v.as_str()) {
                palette.light_primary = v.to_string();
            }
            if let Some(v) = tp.get("lightOnPrimary").and_then(|v| v.as_str()) {
                palette.light_on_primary = v.to_string();
            }
            if let Some(v) = tp.get("lightPrimaryContainer").and_then(|v| v.as_str()) {
                palette.light_primary_container = v.to_string();
            }
            if let Some(v) = tp.get("lightOnPrimaryContainer").and_then(|v| v.as_str()) {
                palette.light_on_primary_container = v.to_string();
            }
            if let Some(v) = tp.get("lightSecondary").and_then(|v| v.as_str()) {
                palette.light_secondary = v.to_string();
            }
            if let Some(v) = tp.get("lightOnSecondary").and_then(|v| v.as_str()) {
                palette.light_on_secondary = v.to_string();
            }
            if let Some(v) = tp.get("lightSecondaryContainer").and_then(|v| v.as_str()) {
                palette.light_secondary_container = v.to_string();
            }
            if let Some(v) = tp.get("lightOnSecondaryContainer").and_then(|v| v.as_str()) {
                palette.light_on_secondary_container = v.to_string();
            }
            if let Some(v) = tp.get("lightTertiary").and_then(|v| v.as_str()) {
                palette.light_tertiary = v.to_string();
            }
            if let Some(v) = tp.get("lightOnTertiary").and_then(|v| v.as_str()) {
                palette.light_on_tertiary = v.to_string();
            }
            if let Some(v) = tp.get("lightTertiaryContainer").and_then(|v| v.as_str()) {
                palette.light_tertiary_container = v.to_string();
            }
            if let Some(v) = tp.get("lightOnTertiaryContainer").and_then(|v| v.as_str()) {
                palette.light_on_tertiary_container = v.to_string();
            }
            if let Some(v) = tp.get("lightBackground").and_then(|v| v.as_str()) {
                palette.light_background = v.to_string();
            }
            if let Some(v) = tp.get("lightOnBackground").and_then(|v| v.as_str()) {
                palette.light_on_background = v.to_string();
            }
            if let Some(v) = tp.get("lightSurface").and_then(|v| v.as_str()) {
                palette.light_surface = v.to_string();
            }
            if let Some(v) = tp.get("lightOnSurface").and_then(|v| v.as_str()) {
                palette.light_on_surface = v.to_string();
            }
            if let Some(v) = tp.get("lightSurfaceVariant").and_then(|v| v.as_str()) {
                palette.light_surface_variant = v.to_string();
            }
            if let Some(v) = tp.get("lightOnSurfaceVariant").and_then(|v| v.as_str()) {
                palette.light_on_surface_variant = v.to_string();
            }
            if let Some(v) = tp
                .get("lightSurfaceContainerLowest")
                .and_then(|v| v.as_str())
            {
                palette.light_surface_container_lowest = v.to_string();
            }
            if let Some(v) = tp.get("lightSurfaceContainerLow").and_then(|v| v.as_str()) {
                palette.light_surface_container_low = v.to_string();
            }
            if let Some(v) = tp.get("lightSurfaceContainer").and_then(|v| v.as_str()) {
                palette.light_surface_container = v.to_string();
            }
            if let Some(v) = tp.get("lightSurfaceContainerHigh").and_then(|v| v.as_str()) {
                palette.light_surface_container_high = v.to_string();
            }
            if let Some(v) = tp
                .get("lightSurfaceContainerHighest")
                .and_then(|v| v.as_str())
            {
                palette.light_surface_container_highest = v.to_string();
            }
            if let Some(v) = tp.get("lightOutline").and_then(|v| v.as_str()) {
                palette.light_outline = v.to_string();
            }
            if let Some(v) = tp.get("lightOutlineVariant").and_then(|v| v.as_str()) {
                palette.light_outline_variant = v.to_string();
            }
            if let Some(v) = tp.get("darkPrimary").and_then(|v| v.as_str()) {
                palette.dark_primary = v.to_string();
            }
            if let Some(v) = tp.get("darkOnPrimary").and_then(|v| v.as_str()) {
                palette.dark_on_primary = v.to_string();
            }
            if let Some(v) = tp.get("darkPrimaryContainer").and_then(|v| v.as_str()) {
                palette.dark_primary_container = v.to_string();
            }
            if let Some(v) = tp.get("darkOnPrimaryContainer").and_then(|v| v.as_str()) {
                palette.dark_on_primary_container = v.to_string();
            }
            if let Some(v) = tp.get("darkSecondary").and_then(|v| v.as_str()) {
                palette.dark_secondary = v.to_string();
            }
            if let Some(v) = tp.get("darkOnSecondary").and_then(|v| v.as_str()) {
                palette.dark_on_secondary = v.to_string();
            }
            if let Some(v) = tp.get("darkSecondaryContainer").and_then(|v| v.as_str()) {
                palette.dark_secondary_container = v.to_string();
            }
            if let Some(v) = tp.get("darkOnSecondaryContainer").and_then(|v| v.as_str()) {
                palette.dark_on_secondary_container = v.to_string();
            }
            if let Some(v) = tp.get("darkTertiary").and_then(|v| v.as_str()) {
                palette.dark_tertiary = v.to_string();
            }
            if let Some(v) = tp.get("darkOnTertiary").and_then(|v| v.as_str()) {
                palette.dark_on_tertiary = v.to_string();
            }
            if let Some(v) = tp.get("darkTertiaryContainer").and_then(|v| v.as_str()) {
                palette.dark_tertiary_container = v.to_string();
            }
            if let Some(v) = tp.get("darkOnTertiaryContainer").and_then(|v| v.as_str()) {
                palette.dark_on_tertiary_container = v.to_string();
            }
            if let Some(v) = tp.get("darkBackground").and_then(|v| v.as_str()) {
                palette.dark_background = v.to_string();
            }
            if let Some(v) = tp.get("darkOnBackground").and_then(|v| v.as_str()) {
                palette.dark_on_background = v.to_string();
            }
            if let Some(v) = tp.get("darkSurface").and_then(|v| v.as_str()) {
                palette.dark_surface = v.to_string();
            }
            if let Some(v) = tp.get("darkOnSurface").and_then(|v| v.as_str()) {
                palette.dark_on_surface = v.to_string();
            }
            if let Some(v) = tp.get("darkSurfaceVariant").and_then(|v| v.as_str()) {
                palette.dark_surface_variant = v.to_string();
            }
            if let Some(v) = tp.get("darkOnSurfaceVariant").and_then(|v| v.as_str()) {
                palette.dark_on_surface_variant = v.to_string();
            }
            if let Some(v) = tp
                .get("darkSurfaceContainerLowest")
                .and_then(|v| v.as_str())
            {
                palette.dark_surface_container_lowest = v.to_string();
            }
            if let Some(v) = tp.get("darkSurfaceContainerLow").and_then(|v| v.as_str()) {
                palette.dark_surface_container_low = v.to_string();
            }
            if let Some(v) = tp.get("darkSurfaceContainer").and_then(|v| v.as_str()) {
                palette.dark_surface_container = v.to_string();
            }
            if let Some(v) = tp.get("darkSurfaceContainerHigh").and_then(|v| v.as_str()) {
                palette.dark_surface_container_high = v.to_string();
            }
            if let Some(v) = tp
                .get("darkSurfaceContainerHighest")
                .and_then(|v| v.as_str())
            {
                palette.dark_surface_container_highest = v.to_string();
            }
            if let Some(v) = tp.get("darkOutline").and_then(|v| v.as_str()) {
                palette.dark_outline = v.to_string();
            }
            if let Some(v) = tp.get("darkOutlineVariant").and_then(|v| v.as_str()) {
                palette.dark_outline_variant = v.to_string();
            }
        }
        core.save_syncable_settings(&settings)
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
    match with_core(|core| {
        let records = core.list_palette_records().map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!(records))
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
    match with_core(|core| {
        let record = core
            .load_palette_record(&device_id_str, &fingerprint_str)
            .map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!(record))
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
    match with_core(|core| {
        core.delete_palette_record(&device_id_str, &fingerprint_str)
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
    match with_core(|core| {
        let themes = core.list_builtin_themes();
        Ok(serde_json::json!(themes))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("BUILTIN_THEMES_ERROR", &e),
    }
}
