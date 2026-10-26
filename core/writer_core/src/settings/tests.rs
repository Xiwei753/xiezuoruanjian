use crate::settings::{
    load_local_settings, load_syncable_settings, save_local_settings, save_syncable_settings,
};
use tempfile::tempdir;

#[test]
fn test_settings() {
    let dir = tempdir().unwrap();
    let data_root = dir.path();
    std::fs::create_dir_all(data_root.join("projects")).unwrap();

    let mut local = load_local_settings(data_root).unwrap();
    // Issue #705: LocalSettings 已删除 theme_mode 字段,运行时只认 appearance_mode。
    assert_eq!(local.appearance_mode.as_str(), "system");
    assert_eq!(local.editor_font_size, 16.0);
    assert_eq!(local.editor_line_spacing_multiplier, 1.5);
    assert!(local.auto_save_enabled);
    assert_eq!(local.auto_save_delay_ms, 1500);

    local.appearance_mode = "light".to_string();
    local.auto_save_delay_ms = 3000;
    save_local_settings(data_root, &local).unwrap();

    let loaded_local = load_local_settings(data_root).unwrap();
    assert_eq!(loaded_local.appearance_mode, "light");
    assert_eq!(loaded_local.auto_save_delay_ms, 3000);

    let mut syncable = load_syncable_settings(data_root).unwrap();
    syncable.font_size = 20.0;
    #[allow(deprecated)]
    {
        syncable.theme_mode = "system".to_string();
    }
    save_syncable_settings(data_root, &syncable).unwrap();

    let loaded_syncable = load_syncable_settings(data_root).unwrap();
    assert_eq!(loaded_syncable.font_size, 20.0);
    #[allow(deprecated)]
    {
        assert_eq!(loaded_syncable.theme_mode, "system");
    }
}

#[test]
fn test_legacy_device_color_fields_are_normalized_on_load() {
    let dir = tempdir().unwrap();
    let data_root = dir.path();
    let legacy = serde_json::json!({
        "fontSize": 18.0,
        "themeMode": "dark",
        "monetColor": "#FF5722",
        "monet_color": "#FF5722",
        "themePalette": {
            "source": "android_dynamic_color",
            "deviceId": "android-device",
            "lightPrimary": "#006493"
        },
        "theme_palette": {
            "source": "android_dynamic_color",
            "deviceId": "android-device"
        },
        "themePaletteJson": "{}",
        "theme_palette_json": "{}"
    });
    std::fs::write(
        data_root.join("settings.sync.json"),
        serde_json::to_string_pretty(&legacy).unwrap(),
    )
    .unwrap();

    let loaded = load_syncable_settings(data_root).unwrap();
    assert_eq!(loaded.font_size, 18.0);
    #[allow(deprecated)]
    {
        assert_eq!(loaded.theme_mode, "dark");
    }

    // 读取本身必须完成持久化 schema 迁移；不能等下一次显式保存。
    let rewritten: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(data_root.join("settings.sync.json")).unwrap(),
    )
    .unwrap();
    for key in [
        "monetColor",
        "monet_color",
        "themePalette",
        "theme_palette",
        "themePaletteJson",
        "theme_palette_json",
    ] {
        assert!(rewritten.get(key).is_none(), "legacy field {key} remains");
    }
    assert_eq!(rewritten["fontSize"], 18.0);
    assert_eq!(rewritten["themeMode"], "dark");
}

#[test]
fn test_snake_case_syncable_fields_are_normalized_on_load() {
    let dir = tempdir().unwrap();
    let data_root = dir.path();
    let legacy = serde_json::json!({
        "font_size": 19.0,
        "theme_mode": "light",
        "monet_color": "#FF5722",
        "theme_palette": {
            "source": "android_dynamic_color",
            "deviceId": "android-device"
        },
        "theme_palette_json": "{}"
    });
    std::fs::write(
        data_root.join("settings.sync.json"),
        serde_json::to_string_pretty(&legacy).unwrap(),
    )
    .unwrap();

    let loaded = load_syncable_settings(data_root).unwrap();
    assert_eq!(loaded.font_size, 19.0);
    #[allow(deprecated)]
    {
        assert_eq!(loaded.theme_mode, "light");
    }

    let rewritten: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(data_root.join("settings.sync.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(rewritten["fontSize"], 19.0);
    assert_eq!(rewritten["themeMode"], "light");
    assert!(rewritten.get("font_size").is_none());
    assert!(rewritten.get("theme_mode").is_none());
    assert!(rewritten.get("monet_color").is_none());
    assert!(rewritten.get("theme_palette").is_none());
    assert!(rewritten.get("theme_palette_json").is_none());
}

#[test]
fn test_canonical_syncable_settings_file_is_not_rewritten_on_load() {
    let dir = tempdir().unwrap();
    let data_root = dir.path();
    let settings = crate::settings::SyncableSettings {
        font_size: 20.0,
        #[allow(deprecated)]
        theme_mode: "system".to_string(),
    };
    save_syncable_settings(data_root, &settings).unwrap();
    let path = data_root.join("settings.sync.json");
    let before = std::fs::read(&path).unwrap();

    let loaded = load_syncable_settings(data_root).unwrap();
    assert_eq!(loaded.font_size, 20.0);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
