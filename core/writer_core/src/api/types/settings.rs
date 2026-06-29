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
    pub editor_coordinated_text_cursor_animation_enabled: bool,
    pub diagnostics_enabled: bool,
    pub diagnostics_verbose: bool,
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
            editor_coordinated_text_cursor_animation_enabled: s.editor_coordinated_text_cursor_animation_enabled,
            diagnostics_enabled: s.diagnostics_enabled,
            diagnostics_verbose: s.diagnostics_verbose,
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
            editor_coordinated_text_cursor_animation_enabled: s.editor_coordinated_text_cursor_animation_enabled,
            diagnostics_enabled: s.diagnostics_enabled,
            diagnostics_verbose: s.diagnostics_verbose,
        }
    }
}

/// Cross-platform theme palette DTO.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub struct ThemePaletteDto {
    pub source: String,
    pub updated_at_ms: i64,
    pub device_id: String,
    pub variant: String,
    pub light_primary: String,
    pub light_on_primary: String,
    pub light_primary_container: String,
    pub light_on_primary_container: String,
    pub light_secondary: String,
    pub light_on_secondary: String,
    pub light_secondary_container: String,
    pub light_on_secondary_container: String,
    pub light_tertiary: String,
    pub light_on_tertiary: String,
    pub light_tertiary_container: String,
    pub light_on_tertiary_container: String,
    pub light_background: String,
    pub light_on_background: String,
    pub light_surface: String,
    pub light_on_surface: String,
    pub light_surface_variant: String,
    pub light_on_surface_variant: String,
    pub light_surface_container: String,
    pub light_surface_container_high: String,
    pub light_outline: String,
    pub light_outline_variant: String,
    pub dark_primary: String,
    pub dark_on_primary: String,
    pub dark_primary_container: String,
    pub dark_on_primary_container: String,
    pub dark_secondary: String,
    pub dark_on_secondary: String,
    pub dark_secondary_container: String,
    pub dark_on_secondary_container: String,
    pub dark_tertiary: String,
    pub dark_on_tertiary: String,
    pub dark_tertiary_container: String,
    pub dark_on_tertiary_container: String,
    pub dark_background: String,
    pub dark_on_background: String,
    pub dark_surface: String,
    pub dark_on_surface: String,
    pub dark_surface_variant: String,
    pub dark_on_surface_variant: String,
    pub dark_surface_container: String,
    pub dark_surface_container_high: String,
    pub dark_outline: String,
    pub dark_outline_variant: String,
}

impl From<crate::settings::ThemePalette> for ThemePaletteDto {
    fn from(p: crate::settings::ThemePalette) -> Self {
        Self {
            source: p.source,
            updated_at_ms: p.updated_at_ms,
            device_id: p.device_id,
            variant: p.variant,
            light_primary: p.light_primary,
            light_on_primary: p.light_on_primary,
            light_primary_container: p.light_primary_container,
            light_on_primary_container: p.light_on_primary_container,
            light_secondary: p.light_secondary,
            light_on_secondary: p.light_on_secondary,
            light_secondary_container: p.light_secondary_container,
            light_on_secondary_container: p.light_on_secondary_container,
            light_tertiary: p.light_tertiary,
            light_on_tertiary: p.light_on_tertiary,
            light_tertiary_container: p.light_tertiary_container,
            light_on_tertiary_container: p.light_on_tertiary_container,
            light_background: p.light_background,
            light_on_background: p.light_on_background,
            light_surface: p.light_surface,
            light_on_surface: p.light_on_surface,
            light_surface_variant: p.light_surface_variant,
            light_on_surface_variant: p.light_on_surface_variant,
            light_surface_container: p.light_surface_container,
            light_surface_container_high: p.light_surface_container_high,
            light_outline: p.light_outline,
            light_outline_variant: p.light_outline_variant,
            dark_primary: p.dark_primary,
            dark_on_primary: p.dark_on_primary,
            dark_primary_container: p.dark_primary_container,
            dark_on_primary_container: p.dark_on_primary_container,
            dark_secondary: p.dark_secondary,
            dark_on_secondary: p.dark_on_secondary,
            dark_secondary_container: p.dark_secondary_container,
            dark_on_secondary_container: p.dark_on_secondary_container,
            dark_tertiary: p.dark_tertiary,
            dark_on_tertiary: p.dark_on_tertiary,
            dark_tertiary_container: p.dark_tertiary_container,
            dark_on_tertiary_container: p.dark_on_tertiary_container,
            dark_background: p.dark_background,
            dark_on_background: p.dark_on_background,
            dark_surface: p.dark_surface,
            dark_on_surface: p.dark_on_surface,
            dark_surface_variant: p.dark_surface_variant,
            dark_on_surface_variant: p.dark_on_surface_variant,
            dark_surface_container: p.dark_surface_container,
            dark_surface_container_high: p.dark_surface_container_high,
            dark_outline: p.dark_outline,
            dark_outline_variant: p.dark_outline_variant,
        }
    }
}

impl From<ThemePaletteDto> for crate::settings::ThemePalette {
    fn from(p: ThemePaletteDto) -> Self {
        Self {
            source: p.source,
            updated_at_ms: p.updated_at_ms,
            device_id: p.device_id,
            variant: p.variant,
            light_primary: p.light_primary,
            light_on_primary: p.light_on_primary,
            light_primary_container: p.light_primary_container,
            light_on_primary_container: p.light_on_primary_container,
            light_secondary: p.light_secondary,
            light_on_secondary: p.light_on_secondary,
            light_secondary_container: p.light_secondary_container,
            light_on_secondary_container: p.light_on_secondary_container,
            light_tertiary: p.light_tertiary,
            light_on_tertiary: p.light_on_tertiary,
            light_tertiary_container: p.light_tertiary_container,
            light_on_tertiary_container: p.light_on_tertiary_container,
            light_background: p.light_background,
            light_on_background: p.light_on_background,
            light_surface: p.light_surface,
            light_on_surface: p.light_on_surface,
            light_surface_variant: p.light_surface_variant,
            light_on_surface_variant: p.light_on_surface_variant,
            light_surface_container: p.light_surface_container,
            light_surface_container_high: p.light_surface_container_high,
            light_outline: p.light_outline,
            light_outline_variant: p.light_outline_variant,
            dark_primary: p.dark_primary,
            dark_on_primary: p.dark_on_primary,
            dark_primary_container: p.dark_primary_container,
            dark_on_primary_container: p.dark_on_primary_container,
            dark_secondary: p.dark_secondary,
            dark_on_secondary: p.dark_on_secondary,
            dark_secondary_container: p.dark_secondary_container,
            dark_on_secondary_container: p.dark_on_secondary_container,
            dark_tertiary: p.dark_tertiary,
            dark_on_tertiary: p.dark_on_tertiary,
            dark_tertiary_container: p.dark_tertiary_container,
            dark_on_tertiary_container: p.dark_on_tertiary_container,
            dark_background: p.dark_background,
            dark_on_background: p.dark_on_background,
            dark_surface: p.dark_surface,
            dark_on_surface: p.dark_on_surface,
            dark_surface_variant: p.dark_surface_variant,
            dark_on_surface_variant: p.dark_on_surface_variant,
            dark_surface_container: p.dark_surface_container,
            dark_surface_container_high: p.dark_surface_container_high,
            dark_outline: p.dark_outline,
            dark_outline_variant: p.dark_outline_variant,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct SyncableSettingsDto {
    pub font_size: f64,
    pub theme_mode: String,
    pub monet_color: String,
    /// JSON string of ThemePaletteDto, for UniFFI compatibility.
    /// Kotlin side parses this JSON string.
    pub theme_palette_json: String,
}

#[allow(deprecated)]
impl From<crate::settings::SyncableSettings> for SyncableSettingsDto {
    fn from(s: crate::settings::SyncableSettings) -> Self {
        let palette_dto = ThemePaletteDto::from(s.theme_palette);
        let palette_json = serde_json::to_string(&palette_dto).unwrap_or_else(|_| "{}".to_string());
        Self {
            font_size: s.font_size,
            theme_mode: s.theme_mode,
            monet_color: s.monet_color,
            theme_palette_json: palette_json,
        }
    }
}

#[allow(deprecated)]
impl From<SyncableSettingsDto> for crate::settings::SyncableSettings {
    fn from(s: SyncableSettingsDto) -> Self {
        let palette: ThemePaletteDto = serde_json::from_str(&s.theme_palette_json)
            .unwrap_or_default();
        crate::settings::SyncableSettings {
            font_size: s.font_size,
            theme_mode: s.theme_mode,
            monet_color: s.monet_color,
            theme_palette: palette.into(),
        }
    }
}
