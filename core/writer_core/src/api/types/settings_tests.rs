use super::settings::*;
use crate::settings::{LocalSettings, SyncableSettings, ThemeColorScheme};

#[test]
fn test_local_settings_dto_roundtrip_and_json() {
    let internal = LocalSettings {
        theme_mode: Some("dark".to_string()),
        appearance_mode: "auto".to_string(),
        color_source: "system".to_string(),
        dynamic_color_enabled: true,
        selected_builtin_theme_id: "default".to_string(),
        selected_palette_id: "none".to_string(),
        locale: Some("en-US".to_string()),
        auto_save_enabled: false,
        editor_font_size: 14.0,
        editor_line_spacing_multiplier: 1.2,
        window_width: 1024.0,
        window_height: 768.0,
        auto_save_delay_ms: 1000,
        auto_indent_enabled: true,
        auto_indent_width: 2.0,
        editor_typing_animation_enabled: false,
        editor_smooth_cursor_enabled: true,
        editor_typing_animation_duration_ms: 200,
        editor_smooth_cursor_duration_ms: 100,
        ai_enabled: true,
        stats_device_id: Some("device_123".to_string()),
        desktop_sidebar_width: 240.0,
        desktop_editor_width: 0.0,
        editor_coordinated_text_cursor_animation_enabled: false,
        diagnostics_enabled: true,
        diagnostics_verbose: false,
    };

    let dto: LocalSettingsDto = internal.clone().into();
    let back: LocalSettings = dto.clone().into();

    assert_eq!(internal.theme_mode, back.theme_mode);
    assert_eq!(internal.window_width, back.window_width);
    assert_eq!(internal.desktop_sidebar_width, back.desktop_sidebar_width);
    assert_eq!(internal.auto_save_delay_ms, back.auto_save_delay_ms);

    let json_val = serde_json::to_value(&dto).unwrap();
    assert_eq!(json_val["theme_mode"], "dark");
    assert_eq!(json_val["appearance_mode"], "auto");
    assert_eq!(json_val["desktop_sidebar_width"], 240.0);
    assert_eq!(json_val["window_width"], 1024.0);
}

#[test]
fn test_syncable_settings_dto_roundtrip_and_json() {
    let internal = SyncableSettings {
        font_size: 18.0,
        theme_mode: "light".to_string(),
        monet_color: "#FFFFFF".to_string(),
        theme_palette: crate::settings::ThemePalette::default(),
    };

    let dto: SyncableSettingsDto = internal.clone().into();
    let back: SyncableSettings = dto.clone().into();

    assert_eq!(internal.font_size, back.font_size);
    assert_eq!(internal.theme_mode, back.theme_mode);
    assert_eq!(internal.monet_color, back.monet_color);

    let json_val = serde_json::to_value(&dto).unwrap();
    assert_eq!(json_val["font_size"], 18.0);
    assert_eq!(json_val["theme_mode"], "light");
    assert_eq!(json_val["monet_color"], "#FFFFFF");
}

#[test]
fn test_theme_color_scheme_dto_roundtrip_and_json() {
    let mut internal = ThemeColorScheme::default();
    internal.primary = "#FF0000".to_string();
    internal.on_primary = "#FFFFFF".to_string();

    let dto: ThemeColorSchemeDto = internal.clone().into();
    let back: ThemeColorScheme = dto.clone().into();

    assert_eq!(internal.primary, back.primary);
    assert_eq!(internal.on_primary, back.on_primary);

    let json_val = serde_json::to_value(&dto).unwrap();
    assert_eq!(json_val["primary"], "#FF0000");
    assert_eq!(json_val["on_primary"], "#FFFFFF");
}
