use super::settings::*;
use crate::settings::{SyncableSettings, ThemeColorScheme, ThemePalette};

#[test]
fn test_syncable_settings_dto_roundtrip() {
    let original = SyncableSettings {
        font_size: 14.5,
        theme_mode: "dark".to_string(),
        monet_color: "blue".to_string(),
        theme_palette: ThemePalette::default(),
    };

    let dto = SyncableSettingsDto::from(original.clone());
    let restored = SyncableSettings::from(dto);

    assert_eq!(original.font_size, restored.font_size);
    assert_eq!(original.theme_mode, restored.theme_mode);
    assert_eq!(original.monet_color, restored.monet_color);
}

#[test]
fn test_syncable_settings_dto_serialization() {
    let original = SyncableSettings {
        font_size: 16.0,
        theme_mode: "light".to_string(),
        monet_color: "red".to_string(),
        theme_palette: ThemePalette::default(),
    };

    let dto = SyncableSettingsDto::from(original.clone());
    let json = serde_json::to_string(&dto).unwrap();
    let deserialized_dto: SyncableSettingsDto = serde_json::from_str(&json).unwrap();
    let restored = SyncableSettings::from(deserialized_dto);

    assert_eq!(original.font_size, restored.font_size);
    assert_eq!(original.theme_mode, restored.theme_mode);
    assert_eq!(original.monet_color, restored.monet_color);
}

#[test]
fn test_theme_color_scheme_dto_roundtrip() {
    let mut original = ThemeColorScheme::default();
    original.primary = "red".to_string();
    original.background = "black".to_string();

    let dto = ThemeColorSchemeDto::from(original.clone());
    let restored = ThemeColorScheme::from(dto);

    assert_eq!(original.primary, restored.primary);
    assert_eq!(original.background, restored.background);
}

use crate::settings::LocalSettings;

#[test]
fn test_local_settings_dto_roundtrip() {
    let mut original = LocalSettings::default();
    original.editor_font_size = 18.0;
    original.appearance_mode = "system".to_string();
    original.desktop_sidebar_width = 300.0;

    let dto = LocalSettingsDto::from(original.clone());
    let restored = LocalSettings::from(dto);

    assert_eq!(original.editor_font_size, restored.editor_font_size);
    assert_eq!(original.appearance_mode, restored.appearance_mode);
    assert_eq!(
        original.desktop_sidebar_width,
        restored.desktop_sidebar_width
    );
}
