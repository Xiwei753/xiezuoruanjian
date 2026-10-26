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
fn test_legacy_device_color_fields_are_ignored_and_not_written_back() {
    let dir = tempdir().unwrap();
    let data_root = dir.path();
    let legacy = serde_json::json!({
        "fontSize": 18.0,
        "themeMode": "dark",
        "monetColor": "#FF5722",
        "themePalette": {
            "source": "android_dynamic_color",
            "deviceId": "android-device",
            "lightPrimary": "#006493"
        }
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

    save_syncable_settings(data_root, &loaded).unwrap();
    let rewritten: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(data_root.join("settings.sync.json")).unwrap(),
    )
    .unwrap();
    assert!(rewritten.get("monetColor").is_none());
    assert!(rewritten.get("themePalette").is_none());
    assert_eq!(rewritten["fontSize"], 18.0);
}
