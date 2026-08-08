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
    assert_eq!(local.theme_mode.as_deref(), Some("system"));
    assert_eq!(local.editor_font_size, 16.0);
    assert_eq!(local.editor_line_spacing_multiplier, 1.5);
    assert!(local.auto_save_enabled);
    assert_eq!(local.auto_save_delay_ms, 1500);

    local.window_width = 800.0;
    local.theme_mode = Some("light".to_string());
    local.auto_save_delay_ms = 3000;
    save_local_settings(data_root, &local).unwrap();

    let loaded_local = load_local_settings(data_root).unwrap();
    assert_eq!(loaded_local.window_width, 800.0);
    assert_eq!(loaded_local.theme_mode.unwrap(), "light");
    assert_eq!(loaded_local.auto_save_delay_ms, 3000);

    let mut syncable = load_syncable_settings(data_root).unwrap();
    syncable.font_size = 20.0;
    syncable.theme_mode = "system".to_string();
    save_syncable_settings(data_root, &syncable).unwrap();

    let loaded_syncable = load_syncable_settings(data_root).unwrap();
    assert_eq!(loaded_syncable.font_size, 20.0);
    assert_eq!(loaded_syncable.theme_mode, "system");
}
