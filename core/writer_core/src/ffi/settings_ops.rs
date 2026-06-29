use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_core};

#[no_mangle]
pub unsafe extern "C" fn writer_core_load_local_settings() -> *mut c_char {
    match with_core(|core| {
        let settings = core.load_local_settings().map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "fontSize": settings.editor_font_size,
            "lineHeight": settings.editor_line_spacing_multiplier,
            "fontFamily": "HarmonyOS Sans",
            "theme": settings.theme_mode.as_deref().unwrap_or("system"),
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

#[no_mangle]
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
            settings.theme_mode = Some(v.to_string());
        }
        core.save_local_settings(&settings)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_INVALID", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_load_syncable_settings() -> *mut c_char {
    match with_core(|core| {
        let settings = core
            .load_syncable_settings()
            .map_err(|e| format!("{}", e))?;
        #[allow(deprecated)]
        let palette = &settings.theme_palette;
        Ok(serde_json::json!({
            "fontSize": settings.font_size,
            "theme": settings.theme_mode,
            "monetColor": settings.monet_color,
            "themePalette": {
                "source": palette.source,
                "updatedAtMs": palette.updated_at_ms,
                "deviceId": palette.device_id,
                "variant": palette.variant,
                "lightPrimary": palette.light_primary,
                "lightOnPrimary": palette.light_on_primary,
                "lightPrimaryContainer": palette.light_primary_container,
                "lightOnPrimaryContainer": palette.light_on_primary_container,
                "lightSecondary": palette.light_secondary,
                "lightOnSecondary": palette.light_on_secondary,
                "lightSecondaryContainer": palette.light_secondary_container,
                "lightOnSecondaryContainer": palette.light_on_secondary_container,
                "lightTertiary": palette.light_tertiary,
                "lightOnTertiary": palette.light_on_tertiary,
                "lightTertiaryContainer": palette.light_tertiary_container,
                "lightOnTertiaryContainer": palette.light_on_tertiary_container,
                "lightBackground": palette.light_background,
                "lightOnBackground": palette.light_on_background,
                "lightSurface": palette.light_surface,
                "lightOnSurface": palette.light_on_surface,
                "lightSurfaceVariant": palette.light_surface_variant,
                "lightOnSurfaceVariant": palette.light_on_surface_variant,
                "lightSurfaceContainer": palette.light_surface_container,
                "lightSurfaceContainerHigh": palette.light_surface_container_high,
                "lightOutline": palette.light_outline,
                "lightOutlineVariant": palette.light_outline_variant,
                "darkPrimary": palette.dark_primary,
                "darkOnPrimary": palette.dark_on_primary,
                "darkPrimaryContainer": palette.dark_primary_container,
                "darkOnPrimaryContainer": palette.dark_on_primary_container,
                "darkSecondary": palette.dark_secondary,
                "darkOnSecondary": palette.dark_on_secondary,
                "darkSecondaryContainer": palette.dark_secondary_container,
                "darkOnSecondaryContainer": palette.dark_on_secondary_container,
                "darkTertiary": palette.dark_tertiary,
                "darkOnTertiary": palette.dark_on_tertiary,
                "darkTertiaryContainer": palette.dark_tertiary_container,
                "darkOnTertiaryContainer": palette.dark_on_tertiary_container,
                "darkBackground": palette.dark_background,
                "darkOnBackground": palette.dark_on_background,
                "darkSurface": palette.dark_surface,
                "darkOnSurface": palette.dark_on_surface,
                "darkSurfaceVariant": palette.dark_surface_variant,
                "darkOnSurfaceVariant": palette.dark_on_surface_variant,
                "darkSurfaceContainer": palette.dark_surface_container,
                "darkSurfaceContainerHigh": palette.dark_surface_container_high,
                "darkOutline": palette.dark_outline,
                "darkOutlineVariant": palette.dark_outline_variant,
            }
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_NOT_FOUND", &e),
    }
}

#[no_mangle]
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
            if let Some(v) = tp.get("lightSurfaceContainer").and_then(|v| v.as_str()) {
                palette.light_surface_container = v.to_string();
            }
            if let Some(v) = tp.get("lightSurfaceContainerHigh").and_then(|v| v.as_str()) {
                palette.light_surface_container_high = v.to_string();
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
            if let Some(v) = tp.get("darkSurfaceContainer").and_then(|v| v.as_str()) {
                palette.dark_surface_container = v.to_string();
            }
            if let Some(v) = tp.get("darkSurfaceContainerHigh").and_then(|v| v.as_str()) {
                palette.dark_surface_container_high = v.to_string();
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
