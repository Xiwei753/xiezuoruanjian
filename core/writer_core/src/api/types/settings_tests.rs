use super::*;
use crate::settings::{LocalSettings, DeviceInfo};

#[test]
fn test_local_settings_dto_conversion() {
    let settings = LocalSettings {
        theme_mode: Some("dark".to_string()),
        appearance_mode: "auto".to_string(),
        color_source: "dynamic".to_string(),
        dynamic_color_enabled: true,
        selected_builtin_theme_id: "default".to_string(),
        selected_palette_id: "palette1".to_string(),
        locale: Some("en-US".to_string()),
        auto_save_enabled: true,
        editor_font_size: 14.0,
        editor_line_spacing_multiplier: 1.5,
        window_width: 800.0,
        window_height: 600.0,
        auto_save_delay_ms: 1000,
        auto_indent_enabled: true,
        auto_indent_width: 4.0,
        editor_typing_animation_enabled: true,
        editor_smooth_cursor_enabled: true,
        editor_typing_animation_duration_ms: 100,
        editor_smooth_cursor_duration_ms: 100,
        ai_enabled: false,
        stats_device_id: Some("device123".to_string()),
        desktop_sidebar_width: 240.0,
        desktop_editor_width: 0.0,
        editor_coordinated_text_cursor_animation_enabled: true,
        diagnostics_enabled: false,
        diagnostics_verbose: false,
    };

    let dto: LocalSettingsDto = settings.clone().into();

    assert_eq!(dto.theme_mode, settings.theme_mode);
    assert_eq!(dto.desktop_sidebar_width, settings.desktop_sidebar_width);
    assert_eq!(dto.desktop_editor_width, settings.desktop_editor_width);

    let converted_back: LocalSettings = dto.into();
    // Compare field by field instead of requiring PartialEq on LocalSettings
    assert_eq!(converted_back.theme_mode, settings.theme_mode);
    assert_eq!(converted_back.appearance_mode, settings.appearance_mode);
    assert_eq!(converted_back.color_source, settings.color_source);
    assert_eq!(converted_back.dynamic_color_enabled, settings.dynamic_color_enabled);
    assert_eq!(converted_back.desktop_sidebar_width, settings.desktop_sidebar_width);
    assert_eq!(converted_back.desktop_editor_width, settings.desktop_editor_width);
}

#[test]
fn test_device_info_dto_conversion() {
    let device_info = DeviceInfo {
        device_id: "dev-123".to_string(),
        device_class: "desktop".to_string(),
        platform: "linux".to_string(),
    };

    let dto: DeviceInfoDto = device_info.clone().into();

    assert_eq!(dto.device_id, device_info.device_id);
    assert_eq!(dto.device_class, device_info.device_class);
    assert_eq!(dto.platform, device_info.platform);
}
