#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
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
    pub auto_save_delay_ms: u64,
    pub auto_indent_enabled: bool,
    pub auto_indent_width: f32,
    pub editor_typing_animation_enabled: bool,
    pub editor_smooth_cursor_enabled: bool,
    pub editor_typing_animation_duration_ms: u64,
    pub editor_smooth_cursor_duration_ms: u64,
    pub ai_enabled: bool,
    pub stats_device_id: Option<String>,
    pub editor_coordinated_text_cursor_animation_enabled: bool,
    pub diagnostics_enabled: bool,
    pub diagnostics_verbose: bool,
}

#[allow(clippy::cast_possible_truncation)]
impl From<crate::settings::LocalSettings> for LocalSettingsDto {
    fn from(s: crate::settings::LocalSettings) -> Self {
        Self {
            // Issue #705: LocalSettings 已删除运行时 theme_mode 字段。
            // DTO 保留 theme_mode 供 Android 端兼容,但 Linux_Qt 不再读取。
            theme_mode: None,
            appearance_mode: s.appearance_mode,
            color_source: s.color_source,
            dynamic_color_enabled: s.dynamic_color_enabled,
            selected_builtin_theme_id: s.selected_builtin_theme_id,
            selected_palette_id: s.selected_palette_id,
            locale: s.locale,
            auto_save_enabled: s.auto_save_enabled,
            editor_font_size: s.editor_font_size,
            editor_line_spacing_multiplier: s.editor_line_spacing_multiplier,
            auto_save_delay_ms: s.auto_save_delay_ms,
            auto_indent_enabled: s.auto_indent_enabled,
            auto_indent_width: s.auto_indent_width,
            editor_typing_animation_enabled: s.editor_typing_animation_enabled,
            editor_smooth_cursor_enabled: s.editor_smooth_cursor_enabled,
            editor_typing_animation_duration_ms: s.editor_typing_animation_duration_ms,
            editor_smooth_cursor_duration_ms: s.editor_smooth_cursor_duration_ms,
            ai_enabled: s.ai_enabled,
            stats_device_id: s.stats_device_id,
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
            // Issue #705: LocalSettings 已删除运行时 theme_mode 字段。
            // DTO 的 theme_mode 不再写回 LocalSettings(运行时只认 appearance_mode)。
            appearance_mode: s.appearance_mode,
            color_source: s.color_source,
            dynamic_color_enabled: s.dynamic_color_enabled,
            selected_builtin_theme_id: s.selected_builtin_theme_id,
            selected_palette_id: s.selected_palette_id,
            locale: s.locale,
            auto_save_enabled: s.auto_save_enabled,
            editor_font_size: s.editor_font_size,
            editor_line_spacing_multiplier: s.editor_line_spacing_multiplier,
            auto_save_delay_ms: s.auto_save_delay_ms,
            auto_indent_enabled: s.auto_indent_enabled,
            auto_indent_width: s.auto_indent_width,
            editor_typing_animation_enabled: s.editor_typing_animation_enabled,
            editor_smooth_cursor_enabled: s.editor_smooth_cursor_enabled,
            editor_typing_animation_duration_ms: s.editor_typing_animation_duration_ms,
            editor_smooth_cursor_duration_ms: s.editor_smooth_cursor_duration_ms,
            ai_enabled: s.ai_enabled,
            stats_device_id: s.stats_device_id,
            editor_coordinated_text_cursor_animation_enabled: s
                .editor_coordinated_text_cursor_animation_enabled,
            diagnostics_enabled: s.diagnostics_enabled,
            diagnostics_verbose: s.diagnostics_verbose,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncableSettingsDto {
    pub font_size: f64,
    pub theme_mode: String,
}

#[allow(deprecated)]
impl From<crate::settings::SyncableSettings> for SyncableSettingsDto {
    fn from(s: crate::settings::SyncableSettings) -> Self {
        Self {
            font_size: s.font_size,
            theme_mode: s.theme_mode,
        }
    }
}

#[allow(deprecated)]
impl From<SyncableSettingsDto> for crate::settings::SyncableSettings {
    fn from(s: SyncableSettingsDto) -> Self {
        crate::settings::SyncableSettings {
            font_size: s.font_size,
            theme_mode: s.theme_mode,
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
#[serde(rename_all = "camelCase")]
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
