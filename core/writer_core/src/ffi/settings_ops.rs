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
pub unsafe extern "C" fn writer_core_save_local_settings(settings_json: *const c_char) -> *mut c_char {
    let json_str = match c_str_to_rust(settings_json) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid settings_json: error {}", e)),
    };
    match with_core(|core| {
        let mut settings = core.load_local_settings().map_err(|e| format!("{}", e))?;
        let val: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| format!("JSON parse error: {}", e))?;
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
        core.save_local_settings(&settings).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_INVALID", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_load_syncable_settings() -> *mut c_char {
    match with_core(|core| {
        let settings = core.load_syncable_settings().map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "fontSize": settings.font_size,
            "theme": settings.theme_mode,
            "monetColor": settings.monet_color
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_save_syncable_settings(settings_json: *const c_char) -> *mut c_char {
    let json_str = match c_str_to_rust(settings_json) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid settings_json: error {}", e)),
    };
    match with_core(|core| {
        let mut settings = core.load_syncable_settings().map_err(|e| format!("{}", e))?;
        let val: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| format!("JSON parse error: {}", e))?;
        if let Some(v) = val.get("fontSize").and_then(|v| v.as_f64()) {
            settings.font_size = v;
        }
        if let Some(v) = val.get("theme").and_then(|v| v.as_str()) {
            settings.theme_mode = v.to_string();
        }
        if let Some(v) = val.get("monetColor").and_then(|v| v.as_str()) {
            settings.monet_color = v.to_string();
        }
        core.save_syncable_settings(&settings).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_INVALID", &e),
    }
}