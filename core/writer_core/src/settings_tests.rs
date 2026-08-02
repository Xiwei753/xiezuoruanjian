#[cfg(test)]
mod tests {
    use crate::settings::{
        load_local_settings, load_syncable_settings, save_local_settings, save_syncable_settings,
    };
    use crate::workspace::create_workspace;
    use tempfile::tempdir;

    #[test]
    fn test_settings() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let mut local = load_local_settings(workspace_path).unwrap();
        assert_eq!(local.theme_mode.as_deref(), Some("system"));
        assert_eq!(local.editor_font_size, 16.0);
        assert_eq!(local.editor_line_spacing_multiplier, 1.5);
        assert!(local.auto_save_enabled);
        assert_eq!(local.auto_save_delay_ms, 1500);

        local.window_width = 800.0;
        local.theme_mode = Some("light".to_string());
        local.auto_save_delay_ms = 3000;
        save_local_settings(workspace_path, &local).unwrap();

        let loaded_local = load_local_settings(workspace_path).unwrap();
        assert_eq!(loaded_local.window_width, 800.0);
        assert_eq!(loaded_local.theme_mode.unwrap(), "light");
        assert_eq!(loaded_local.auto_save_delay_ms, 3000);

        let mut syncable = load_syncable_settings(workspace_path).unwrap();
        syncable.font_size = 20.0;
        syncable.theme_mode = "system".to_string();
        save_syncable_settings(workspace_path, &syncable).unwrap();

        let loaded_syncable = load_syncable_settings(workspace_path).unwrap();
        assert_eq!(loaded_syncable.font_size, 20.0);
        assert_eq!(loaded_syncable.theme_mode, "system");
    }

    #[test]
    fn test_local_settings_validation_clamps_values() {
        use crate::settings::{LocalSettings, ranges};
        let mut settings = LocalSettings::default();

        settings.editor_font_size = ranges::FONT_SIZE_MIN - 1.0;
        settings.editor_line_spacing_multiplier = ranges::LINE_SPACING_MIN - 1.0;
        settings.auto_indent_width = ranges::INDENT_WIDTH_MIN - 1.0;
        settings.editor_typing_animation_duration_ms = ranges::ANIMATION_DURATION_MIN_MS - 1;
        settings.editor_smooth_cursor_duration_ms = ranges::ANIMATION_DURATION_MIN_MS - 1;
        settings.auto_save_delay_ms = ranges::AUTO_SAVE_DELAY_MIN_MS - 1;

        settings.validate();

        assert_eq!(settings.editor_font_size, ranges::FONT_SIZE_MIN);
        assert_eq!(settings.editor_line_spacing_multiplier, ranges::LINE_SPACING_MIN);
        assert_eq!(settings.auto_indent_width, ranges::INDENT_WIDTH_MIN);
        assert_eq!(settings.editor_typing_animation_duration_ms, ranges::ANIMATION_DURATION_MIN_MS);
        assert_eq!(settings.editor_smooth_cursor_duration_ms, ranges::ANIMATION_DURATION_MIN_MS);
        assert_eq!(settings.auto_save_delay_ms, ranges::AUTO_SAVE_DELAY_MIN_MS);

        settings.editor_font_size = ranges::FONT_SIZE_MAX + 1.0;
        settings.editor_line_spacing_multiplier = ranges::LINE_SPACING_MAX + 1.0;
        settings.auto_indent_width = ranges::INDENT_WIDTH_MAX + 1.0;
        settings.editor_typing_animation_duration_ms = ranges::ANIMATION_DURATION_MAX_MS + 1;
        settings.editor_smooth_cursor_duration_ms = ranges::ANIMATION_DURATION_MAX_MS + 1;
        settings.auto_save_delay_ms = ranges::AUTO_SAVE_DELAY_MAX_MS + 1;

        settings.validate();

        assert_eq!(settings.editor_font_size, ranges::FONT_SIZE_MAX);
        assert_eq!(settings.editor_line_spacing_multiplier, ranges::LINE_SPACING_MAX);
        assert_eq!(settings.auto_indent_width, ranges::INDENT_WIDTH_MAX);
        assert_eq!(settings.editor_typing_animation_duration_ms, ranges::ANIMATION_DURATION_MAX_MS);
        assert_eq!(settings.editor_smooth_cursor_duration_ms, ranges::ANIMATION_DURATION_MAX_MS);
        assert_eq!(settings.auto_save_delay_ms, ranges::AUTO_SAVE_DELAY_MAX_MS);
    }
    #[test]
    fn test_ranges_are_valid_and_sane() {
        use crate::settings::ranges::*;

        // Ensure MIN is strictly less than MAX
        assert!(FONT_SIZE_MIN < FONT_SIZE_MAX);
        assert!(LINE_SPACING_MIN < LINE_SPACING_MAX);
        assert!(INDENT_WIDTH_MIN < INDENT_WIDTH_MAX);
        assert!(ANIMATION_DURATION_MIN_MS < ANIMATION_DURATION_MAX_MS);
        assert!(AUTO_SAVE_DELAY_MIN_MS < AUTO_SAVE_DELAY_MAX_MS);

        // Ensure reasonable positive boundaries
        assert!(FONT_SIZE_MIN > 0.0);
        assert!(FONT_SIZE_MAX <= 200.0);

        assert!(LINE_SPACING_MIN > 0.0);
        assert!(LINE_SPACING_MAX <= 10.0);

        assert!(INDENT_WIDTH_MIN >= 0.0);
        assert!(INDENT_WIDTH_MAX <= 16.0);

        assert!(ANIMATION_DURATION_MIN_MS > 0);
        assert!(ANIMATION_DURATION_MAX_MS <= 10000);

        assert!(AUTO_SAVE_DELAY_MIN_MS > 0);
        assert!(AUTO_SAVE_DELAY_MAX_MS <= 3600000); // Max 1 hour
    }
}
