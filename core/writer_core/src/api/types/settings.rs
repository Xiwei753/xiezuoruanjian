#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct LocalSettingsDto {
    pub theme_mode: Option<String>,
    pub appearance_mode: String,
    pub color_source: String,
    pub dynamic_color_enabled: bool,
    pub selected_builtin_theme_id: String,
    pub selected_palette_id: String,
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

#[allow(clippy::cast_possible_truncation)]
impl From<crate::settings::LocalSettings> for LocalSettingsDto {
    fn from(s: crate::settings::LocalSettings) -> Self {
        Self {
            theme_mode: s.theme_mode,
            appearance_mode: s.appearance_mode,
            color_source: s.color_source,
            dynamic_color_enabled: s.dynamic_color_enabled,
            selected_builtin_theme_id: s.selected_builtin_theme_id,
            selected_palette_id: s.selected_palette_id,
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
            editor_coordinated_text_cursor_animation_enabled: s
                .editor_coordinated_text_cursor_animation_enabled,
            diagnostics_enabled: s.diagnostics_enabled,
            diagnostics_verbose: s.diagnostics_verbose,
        }
    }
}

impl From<LocalSettingsDto> for crate::settings::LocalSettings {
    fn from(s: LocalSettingsDto) -> Self {
        crate::settings::LocalSettings {
            theme_mode: s.theme_mode,
            appearance_mode: s.appearance_mode,
            color_source: s.color_source,
            dynamic_color_enabled: s.dynamic_color_enabled,
            selected_builtin_theme_id: s.selected_builtin_theme_id,
            selected_palette_id: s.selected_palette_id,
            locale: s.locale,
            auto_save_enabled: s.auto_save_enabled,
            editor_font_size: s.editor_font_size,
            editor_line_spacing_multiplier: s.editor_line_spacing_multiplier,
            window_width: f64::from(s.window_width),
            window_height: f64::from(s.window_height),
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
            editor_coordinated_text_cursor_animation_enabled: s
                .editor_coordinated_text_cursor_animation_enabled,
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
    pub light_surface_container_lowest: String,
    pub light_surface_container_low: String,
    pub light_surface_container: String,
    pub light_surface_container_high: String,
    pub light_surface_container_highest: String,
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
    pub dark_surface_container_lowest: String,
    pub dark_surface_container_low: String,
    pub dark_surface_container: String,
    pub dark_surface_container_high: String,
    pub dark_surface_container_highest: String,
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
            light_surface_container_lowest: p.light_surface_container_lowest,
            light_surface_container_low: p.light_surface_container_low,
            light_surface_container: p.light_surface_container,
            light_surface_container_high: p.light_surface_container_high,
            light_surface_container_highest: p.light_surface_container_highest,
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
            dark_surface_container_lowest: p.dark_surface_container_lowest,
            dark_surface_container_low: p.dark_surface_container_low,
            dark_surface_container: p.dark_surface_container,
            dark_surface_container_high: p.dark_surface_container_high,
            dark_surface_container_highest: p.dark_surface_container_highest,
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
            light_surface_container_lowest: p.light_surface_container_lowest,
            light_surface_container_low: p.light_surface_container_low,
            light_surface_container: p.light_surface_container,
            light_surface_container_high: p.light_surface_container_high,
            light_surface_container_highest: p.light_surface_container_highest,
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
            dark_surface_container_lowest: p.dark_surface_container_lowest,
            dark_surface_container_low: p.dark_surface_container_low,
            dark_surface_container: p.dark_surface_container,
            dark_surface_container_high: p.dark_surface_container_high,
            dark_surface_container_highest: p.dark_surface_container_highest,
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
        let palette: ThemePaletteDto =
            serde_json::from_str(&s.theme_palette_json).unwrap_or_default();
        crate::settings::SyncableSettings {
            font_size: s.font_size,
            theme_mode: s.theme_mode,
            monet_color: s.monet_color,
            theme_palette: palette.into(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub struct ThemeColorSchemeDto {
    pub primary: String,
    pub on_primary: String,
    pub primary_container: String,
    pub on_primary_container: String,
    pub inverse_primary: String,
    pub secondary: String,
    pub on_secondary: String,
    pub secondary_container: String,
    pub on_secondary_container: String,
    pub tertiary: String,
    pub on_tertiary: String,
    pub tertiary_container: String,
    pub on_tertiary_container: String,
    pub background: String,
    pub on_background: String,
    pub surface: String,
    pub on_surface: String,
    pub surface_variant: String,
    pub on_surface_variant: String,
    pub surface_tint: String,
    pub surface_dim: String,
    pub surface_bright: String,
    pub surface_container_lowest: String,
    pub surface_container_low: String,
    pub surface_container: String,
    pub surface_container_high: String,
    pub surface_container_highest: String,
    pub inverse_surface: String,
    pub inverse_on_surface: String,
    pub error: String,
    pub on_error: String,
    pub error_container: String,
    pub on_error_container: String,
    pub outline: String,
    pub outline_variant: String,
    pub scrim: String,
    pub primary_fixed: String,
    pub primary_fixed_dim: String,
    pub on_primary_fixed: String,
    pub on_primary_fixed_variant: String,
    pub secondary_fixed: String,
    pub secondary_fixed_dim: String,
    pub on_secondary_fixed: String,
    pub on_secondary_fixed_variant: String,
    pub tertiary_fixed: String,
    pub tertiary_fixed_dim: String,
    pub on_tertiary_fixed: String,
    pub on_tertiary_fixed_variant: String,
}

impl From<crate::settings::ThemeColorScheme> for ThemeColorSchemeDto {
    fn from(s: crate::settings::ThemeColorScheme) -> Self {
        Self {
            primary: s.primary,
            on_primary: s.on_primary,
            primary_container: s.primary_container,
            on_primary_container: s.on_primary_container,
            inverse_primary: s.inverse_primary,
            secondary: s.secondary,
            on_secondary: s.on_secondary,
            secondary_container: s.secondary_container,
            on_secondary_container: s.on_secondary_container,
            tertiary: s.tertiary,
            on_tertiary: s.on_tertiary,
            tertiary_container: s.tertiary_container,
            on_tertiary_container: s.on_tertiary_container,
            background: s.background,
            on_background: s.on_background,
            surface: s.surface,
            on_surface: s.on_surface,
            surface_variant: s.surface_variant,
            on_surface_variant: s.on_surface_variant,
            surface_tint: s.surface_tint,
            surface_dim: s.surface_dim,
            surface_bright: s.surface_bright,
            surface_container_lowest: s.surface_container_lowest,
            surface_container_low: s.surface_container_low,
            surface_container: s.surface_container,
            surface_container_high: s.surface_container_high,
            surface_container_highest: s.surface_container_highest,
            inverse_surface: s.inverse_surface,
            inverse_on_surface: s.inverse_on_surface,
            error: s.error,
            on_error: s.on_error,
            error_container: s.error_container,
            on_error_container: s.on_error_container,
            outline: s.outline,
            outline_variant: s.outline_variant,
            scrim: s.scrim,
            primary_fixed: s.primary_fixed,
            primary_fixed_dim: s.primary_fixed_dim,
            on_primary_fixed: s.on_primary_fixed,
            on_primary_fixed_variant: s.on_primary_fixed_variant,
            secondary_fixed: s.secondary_fixed,
            secondary_fixed_dim: s.secondary_fixed_dim,
            on_secondary_fixed: s.on_secondary_fixed,
            on_secondary_fixed_variant: s.on_secondary_fixed_variant,
            tertiary_fixed: s.tertiary_fixed,
            tertiary_fixed_dim: s.tertiary_fixed_dim,
            on_tertiary_fixed: s.on_tertiary_fixed,
            on_tertiary_fixed_variant: s.on_tertiary_fixed_variant,
        }
    }
}

impl From<ThemeColorSchemeDto> for crate::settings::ThemeColorScheme {
    fn from(s: ThemeColorSchemeDto) -> Self {
        Self {
            primary: s.primary,
            on_primary: s.on_primary,
            primary_container: s.primary_container,
            on_primary_container: s.on_primary_container,
            inverse_primary: s.inverse_primary,
            secondary: s.secondary,
            on_secondary: s.on_secondary,
            secondary_container: s.secondary_container,
            on_secondary_container: s.on_secondary_container,
            tertiary: s.tertiary,
            on_tertiary: s.on_tertiary,
            tertiary_container: s.tertiary_container,
            on_tertiary_container: s.on_tertiary_container,
            background: s.background,
            on_background: s.on_background,
            surface: s.surface,
            on_surface: s.on_surface,
            surface_variant: s.surface_variant,
            on_surface_variant: s.on_surface_variant,
            surface_tint: s.surface_tint,
            surface_dim: s.surface_dim,
            surface_bright: s.surface_bright,
            surface_container_lowest: s.surface_container_lowest,
            surface_container_low: s.surface_container_low,
            surface_container: s.surface_container,
            surface_container_high: s.surface_container_high,
            surface_container_highest: s.surface_container_highest,
            inverse_surface: s.inverse_surface,
            inverse_on_surface: s.inverse_on_surface,
            error: s.error,
            on_error: s.on_error,
            error_container: s.error_container,
            on_error_container: s.on_error_container,
            outline: s.outline,
            outline_variant: s.outline_variant,
            scrim: s.scrim,
            primary_fixed: s.primary_fixed,
            primary_fixed_dim: s.primary_fixed_dim,
            on_primary_fixed: s.on_primary_fixed,
            on_primary_fixed_variant: s.on_primary_fixed_variant,
            secondary_fixed: s.secondary_fixed,
            secondary_fixed_dim: s.secondary_fixed_dim,
            on_secondary_fixed: s.on_secondary_fixed,
            on_secondary_fixed_variant: s.on_secondary_fixed_variant,
            tertiary_fixed: s.tertiary_fixed,
            tertiary_fixed_dim: s.tertiary_fixed_dim,
            on_tertiary_fixed: s.on_tertiary_fixed,
            on_tertiary_fixed_variant: s.on_tertiary_fixed_variant,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub struct ThemePaletteRecordDto {
    pub schema_version: u32,
    pub palette_id: String,
    pub palette_fingerprint: String,
    pub source: String,
    pub source_platform: String,
    pub source_device_id: String,
    pub source_device_class: String,
    pub captured_at_ms: i64,
    pub variant: String,
    pub light_scheme: ThemeColorSchemeDto,
    pub dark_scheme: ThemeColorSchemeDto,
}

impl From<crate::settings::ThemePaletteRecord> for ThemePaletteRecordDto {
    fn from(r: crate::settings::ThemePaletteRecord) -> Self {
        Self {
            schema_version: r.schema_version,
            palette_id: r.palette_id,
            palette_fingerprint: r.palette_fingerprint,
            source: r.source,
            source_platform: r.source_platform,
            source_device_id: r.source_device_id,
            source_device_class: r.source_device_class,
            captured_at_ms: r.captured_at_ms,
            variant: r.variant,
            light_scheme: r.light_scheme.into(),
            dark_scheme: r.dark_scheme.into(),
        }
    }
}

impl From<ThemePaletteRecordDto> for crate::settings::ThemePaletteRecord {
    fn from(r: ThemePaletteRecordDto) -> Self {
        Self {
            schema_version: r.schema_version,
            palette_id: r.palette_id,
            palette_fingerprint: r.palette_fingerprint,
            source: r.source,
            source_platform: r.source_platform,
            source_device_id: r.source_device_id,
            source_device_class: r.source_device_class,
            captured_at_ms: r.captured_at_ms,
            variant: r.variant,
            light_scheme: r.light_scheme.into(),
            dark_scheme: r.dark_scheme.into(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct BuiltinThemeDto {
    pub theme_id: String,
    pub name: String,
    pub light_scheme: ThemeColorSchemeDto,
    pub dark_scheme: ThemeColorSchemeDto,
}

impl From<crate::settings::BuiltinTheme> for BuiltinThemeDto {
    fn from(t: crate::settings::BuiltinTheme) -> Self {
        Self {
            theme_id: t.theme_id.to_string(),
            name: t.name.to_string(),
            light_scheme: t.light_scheme.into(),
            dark_scheme: t.dark_scheme.into(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct DeviceInfoDto {
    pub device_id: String,
    pub device_class: String,
    pub platform: String,
}

impl From<crate::settings::DeviceInfo> for DeviceInfoDto {
    fn from(d: crate::settings::DeviceInfo) -> Self {
        Self {
            device_id: d.device_id,
            device_class: d.device_class,
            platform: d.platform,
        }
    }
}
