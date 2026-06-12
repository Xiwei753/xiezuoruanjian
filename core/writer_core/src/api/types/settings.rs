#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct LocalSettingsDto {
    pub theme_mode: Option<String>,
    pub locale: Option<String>,
    pub auto_save_enabled: bool,
    pub editor_font_size: f32,
    pub editor_line_spacing_multiplier: f32,
    pub window_width: f32,
    pub window_height: f32,
    pub auto_save_delay_ms: u64,
    pub auto_indent_enabled: bool,
    pub auto_indent_width: f32,
    pub editor_typing_animation_enabled: bool,
    pub editor_smooth_cursor_enabled: bool,
    pub editor_typing_animation_duration_ms: u64,
    pub editor_smooth_cursor_duration_ms: u64,
    pub ai_enabled: bool,
    pub stats_device_id: Option<String>,
    pub desktop_sidebar_width: f64,
    pub desktop_editor_width: f64,
}

impl From<crate::settings::LocalSettings> for LocalSettingsDto {
    fn from(s: crate::settings::LocalSettings) -> Self {
        Self {
            theme_mode: s.theme_mode,
            locale: s.locale,
            auto_save_enabled: s.auto_save_enabled,
            editor_font_size: s.editor_font_size,
            editor_line_spacing_multiplier: s.editor_line_spacing_multiplier,
            window_width: s.window_width as f32,
            window_height: s.window_height as f32,
            auto_save_delay_ms: s.auto_save_delay_ms,
            auto_indent_enabled: s.auto_indent_enabled,
            auto_indent_width: s.auto_indent_width,
            editor_typing_animation_enabled: s.editor_typing_animation_enabled,
            editor_smooth_cursor_enabled: s.editor_smooth_cursor_enabled,
            editor_typing_animation_duration_ms: s.editor_typing_animation_duration_ms,
            editor_smooth_cursor_duration_ms: s.editor_smooth_cursor_duration_ms,
            ai_enabled: s.ai_enabled,
            stats_device_id: s.stats_device_id,
            desktop_sidebar_width: s.desktop_sidebar_width,
            desktop_editor_width: s.desktop_editor_width,
        }
    }
}

impl From<LocalSettingsDto> for crate::settings::LocalSettings {
    fn from(s: LocalSettingsDto) -> Self {
        crate::settings::LocalSettings {
            theme_mode: s.theme_mode,
            locale: s.locale,
            auto_save_enabled: s.auto_save_enabled,
            editor_font_size: s.editor_font_size,
            editor_line_spacing_multiplier: s.editor_line_spacing_multiplier,
            window_width: s.window_width as f64,
            window_height: s.window_height as f64,
            auto_save_delay_ms: s.auto_save_delay_ms,
            auto_indent_enabled: s.auto_indent_enabled,
            auto_indent_width: s.auto_indent_width,
            editor_typing_animation_enabled: s.editor_typing_animation_enabled,
            editor_smooth_cursor_enabled: s.editor_smooth_cursor_enabled,
            editor_typing_animation_duration_ms: s.editor_typing_animation_duration_ms,
            editor_smooth_cursor_duration_ms: s.editor_smooth_cursor_duration_ms,
            ai_enabled: s.ai_enabled,
            stats_device_id: s.stats_device_id,
            desktop_sidebar_width: s.desktop_sidebar_width,
            desktop_editor_width: s.desktop_editor_width,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct SyncableSettingsDto {
    pub font_size: f64,
    pub theme_mode: String,
    pub monet_color: String,
}

impl From<crate::settings::SyncableSettings> for SyncableSettingsDto {
    fn from(s: crate::settings::SyncableSettings) -> Self {
        Self {
            font_size: s.font_size,
            theme_mode: s.theme_mode,
            monet_color: s.monet_color,
        }
    }
}

impl From<SyncableSettingsDto> for crate::settings::SyncableSettings {
    fn from(s: SyncableSettingsDto) -> Self {
        crate::settings::SyncableSettings {
            font_size: s.font_size,
            theme_mode: s.theme_mode,
            monet_color: s.monet_color,
        }
    }
}
