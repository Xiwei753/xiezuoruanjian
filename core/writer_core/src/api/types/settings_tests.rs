use super::settings::*;
use crate::settings::LocalSettings;

#[test]
fn test_local_settings_dto_json_key_contract() {
    let dto = LocalSettingsDto {
        theme_mode: Some("dark".to_string()),
        appearance_mode: "follow_system".to_string(),
        color_source: "dynamic".to_string(),
        dynamic_color_enabled: true,
        selected_builtin_theme_id: "default".to_string(),
        selected_palette_id: "palette_1".to_string(),
        locale: Some("zh-CN".to_string()),
        auto_save_enabled: true,
        editor_font_size: 16.0,
        editor_line_spacing_multiplier: 1.5,
        window_width: 800.0,
        window_height: 600.0,
        auto_save_delay_ms: 1000,
        auto_indent_enabled: true,
        auto_indent_width: 4.0,
        editor_typing_animation_enabled: true,
        editor_smooth_cursor_enabled: true,
        editor_typing_animation_duration_ms: 100,
        editor_smooth_cursor_duration_ms: 80,
        ai_enabled: false,
        stats_device_id: None,
        desktop_sidebar_width: 240.0,
        desktop_editor_width: 0.0,
        editor_coordinated_text_cursor_animation_enabled: false,
        diagnostics_enabled: false,
        diagnostics_verbose: false,
    };

    let json_val = serde_json::to_value(&dto).unwrap();
    // Verify that without `#[serde(rename_all = "camelCase")]`, fields use snake_case
    assert_eq!(json_val["theme_mode"], "dark");
    assert_eq!(json_val["appearance_mode"], "follow_system");
    assert_eq!(json_val["desktop_sidebar_width"], 240.0);
    assert_eq!(json_val["desktop_editor_width"], 0.0);
}

#[test]
fn test_local_settings_dto_roundtrip() {
    let internal = LocalSettings {
        theme_mode: Some("dark".to_string()),
        appearance_mode: "follow_system".to_string(),
        color_source: "dynamic".to_string(),
        dynamic_color_enabled: true,
        selected_builtin_theme_id: "default".to_string(),
        selected_palette_id: "palette_1".to_string(),
        locale: Some("zh-CN".to_string()),
        auto_save_enabled: true,
        editor_font_size: 16.0,
        editor_line_spacing_multiplier: 1.5,
        window_width: 800.0,
        window_height: 600.0,
        auto_save_delay_ms: 1000,
        auto_indent_enabled: true,
        auto_indent_width: 4.0,
        editor_typing_animation_enabled: true,
        editor_smooth_cursor_enabled: true,
        editor_typing_animation_duration_ms: 100,
        editor_smooth_cursor_duration_ms: 80,
        ai_enabled: false,
        stats_device_id: None,
        desktop_sidebar_width: 240.0,
        desktop_editor_width: 0.0,
        editor_coordinated_text_cursor_animation_enabled: false,
        diagnostics_enabled: false,
        diagnostics_verbose: false,
    };

    let dto: LocalSettingsDto = internal.clone().into();
    let back: LocalSettings = dto.into();

    assert_eq!(internal.theme_mode, back.theme_mode);
    assert_eq!(internal.appearance_mode, back.appearance_mode);
    assert_eq!(internal.desktop_sidebar_width, back.desktop_sidebar_width);
    assert_eq!(internal.desktop_editor_width, back.desktop_editor_width);
}
