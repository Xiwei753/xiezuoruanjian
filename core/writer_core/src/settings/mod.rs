//! # 设置管理（Core 层）
//!
//! 管理两类设置：
//!
//! 1. **LocalSettings（本地设置）**：仅存储在本地，不同步
//!    - 窗口大小、自动保存开关、字号、行距、自动缩进、动画开关等
//!    - 文件路径：`<app_data_root>/settings.local.json`
//!
//! 2. **SyncableSettings（可同步设置）**：跨设备同步候选
//!    - 字号、主题模式、Monet 颜色
//!    - 文件路径：`<app_data_root>/settings.sync.json`（应用级，不随作品仓库同步）
//!
//! ## 职责边界
//!
//! - **做**：设置的加载/保存/默认值/有效字号计算
//! - **不做**：设置 UI 展示（由客户端负责）
//! - **修改设置后**：客户端需要监听设置变更事件并刷新 UI

pub mod models;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// 跨端设置安全范围常量。
///
/// 各端 UI 应使用这些常量作为滑块/输入的 min/max，
/// 以确保同步后设置值在所有平台上有效。
pub mod ranges {
    pub const FONT_SIZE_MIN: f32 = 12.0;
    pub const FONT_SIZE_MAX: f32 = 72.0;
    pub const LINE_SPACING_MIN: f32 = 1.0;
    pub const LINE_SPACING_MAX: f32 = 3.0;
    pub const INDENT_WIDTH_MIN: f32 = 0.0;
    pub const INDENT_WIDTH_MAX: f32 = 8.0;
    pub const ANIMATION_DURATION_MIN_MS: u64 = 30;
    pub const ANIMATION_DURATION_MAX_MS: u64 = 1000;
    pub const AUTO_SAVE_DELAY_MIN_MS: u64 = 1000;
    pub const AUTO_SAVE_DELAY_MAX_MS: u64 = 10000;
}

/// 本地设置（不同步到其他设备）。
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LocalSettings {
    // Issue #705 评论 5716410988: 运行时只认 appearance_mode(值仅 system/light/dark)。
    // 旧版本 LocalSettings 同时有 theme_mode 和 appearance_mode 字段,rename_all=camelCase
    // 后旧文件会同时存在 themeMode 和 appearanceMode 两个 key。serde alias 会把两者都
    // 映射到 appearance_mode,导致 serde_json::from_str::<LocalSettings>() 报 duplicate
    // field 错误。因此这里不再用 alias,改在 load_local_settings() 里用原始 JSON 迁移:
    // 取值顺序 appearanceMode > themeMode > system,读完后写回只保留 appearanceMode。
    #[serde(default = "default_appearance_mode")]
    pub appearance_mode: String,
    #[serde(default = "default_color_source")]
    pub color_source: String,
    #[serde(default)]
    pub dynamic_color_enabled: bool,
    #[serde(default)]
    pub selected_builtin_theme_id: String,
    #[serde(default)]
    pub selected_palette_id: String,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default = "default_editor_font_size")]
    pub editor_font_size: f32,
    #[serde(default = "default_editor_line_spacing_multiplier")]
    pub editor_line_spacing_multiplier: f32,
    #[serde(default = "default_auto_save_enabled")]
    pub auto_save_enabled: bool,
    #[serde(default = "default_auto_save_delay_ms")]
    pub auto_save_delay_ms: u64,
    #[serde(default = "default_auto_indent_enabled")]
    pub auto_indent_enabled: bool,
    #[serde(default = "default_auto_indent_width")]
    pub auto_indent_width: f32,
    #[serde(default)]
    pub window_width: f64,
    #[serde(default)]
    pub window_height: f64,
    #[serde(default = "default_editor_typing_animation_enabled")]
    pub editor_typing_animation_enabled: bool,
    #[serde(default = "default_editor_smooth_cursor_enabled")]
    pub editor_smooth_cursor_enabled: bool,
    #[serde(default = "default_editor_typing_animation_duration_ms")]
    pub editor_typing_animation_duration_ms: u64,
    #[serde(default = "default_editor_smooth_cursor_duration_ms")]
    pub editor_smooth_cursor_duration_ms: u64,
    #[serde(default = "default_editor_coordinated_text_cursor_animation_enabled")]
    pub editor_coordinated_text_cursor_animation_enabled: bool,
    #[serde(default)]
    pub ai_enabled: bool,
    #[serde(default)]
    pub stats_device_id: Option<String>,
    #[serde(
        default = "default_desktop_sidebar_width",
        alias = "linux_qt_sidebar_width",
        alias = "desktop_sidebar_width",
        alias = "linux_sidebar_width"
    )]
    pub desktop_sidebar_width: f64,
    #[serde(
        default = "default_desktop_editor_width",
        alias = "linux_qt_editor_width",
        alias = "desktop_editor_width"
    )]
    pub desktop_editor_width: f64,
    #[serde(default = "default_diagnostics_enabled")]
    pub diagnostics_enabled: bool,
    #[serde(default = "default_diagnostics_verbose")]
    pub diagnostics_verbose: bool,
}

fn default_appearance_mode() -> String {
    "system".to_string()
}

fn default_color_source() -> String {
    "built_in".to_string()
}

fn default_desktop_sidebar_width() -> f64 {
    240.0
}

fn default_desktop_editor_width() -> f64 {
    0.0
}

/// alpha/内测阶段 diagnostics_enabled 默认 true（crash/error 永远开启）
fn default_diagnostics_enabled() -> bool {
    true
}

/// alpha/内测阶段 diagnostics_verbose 默认 true
/// 稳定版应改为 false
fn default_diagnostics_verbose() -> bool {
    true
}

fn default_editor_font_size() -> f32 {
    16.0
}
fn default_editor_line_spacing_multiplier() -> f32 {
    1.5
}
fn default_auto_save_enabled() -> bool {
    true
}
fn default_auto_save_delay_ms() -> u64 {
    1500
}

fn default_auto_indent_enabled() -> bool {
    true
}
fn default_auto_indent_width() -> f32 {
    2.0
}
fn default_editor_typing_animation_enabled() -> bool {
    true
}
fn default_editor_smooth_cursor_enabled() -> bool {
    true
}
fn default_editor_typing_animation_duration_ms() -> u64 {
    100
}
fn default_editor_smooth_cursor_duration_ms() -> u64 {
    80
}
fn default_editor_coordinated_text_cursor_animation_enabled() -> bool {
    true
}

impl LocalSettings {
    /// 将所有设置项 clamp 到安全范围内。
    pub fn validate(&mut self) {
        // Issue #705 评论 5716919024: appearance_mode 运行时只允许 system/light/dark,
        // 非法值归一成 system,避免下游(如 LinuxThemeController)把未知值默默当 system。
        if !is_valid_appearance_mode(&self.appearance_mode) {
            self.appearance_mode = default_appearance_mode();
        }
        self.editor_font_size = self
            .editor_font_size
            .clamp(ranges::FONT_SIZE_MIN, ranges::FONT_SIZE_MAX);
        self.editor_line_spacing_multiplier = self
            .editor_line_spacing_multiplier
            .clamp(ranges::LINE_SPACING_MIN, ranges::LINE_SPACING_MAX);
        self.auto_indent_width = self
            .auto_indent_width
            .clamp(ranges::INDENT_WIDTH_MIN, ranges::INDENT_WIDTH_MAX);
        self.editor_typing_animation_duration_ms = self.editor_typing_animation_duration_ms.clamp(
            ranges::ANIMATION_DURATION_MIN_MS,
            ranges::ANIMATION_DURATION_MAX_MS,
        );
        self.editor_smooth_cursor_duration_ms = self.editor_smooth_cursor_duration_ms.clamp(
            ranges::ANIMATION_DURATION_MIN_MS,
            ranges::ANIMATION_DURATION_MAX_MS,
        );
        self.auto_save_delay_ms = self.auto_save_delay_ms.clamp(
            ranges::AUTO_SAVE_DELAY_MIN_MS,
            ranges::AUTO_SAVE_DELAY_MAX_MS,
        );
    }
}

impl Default for LocalSettings {
    fn default() -> Self {
        Self {
            appearance_mode: default_appearance_mode(),
            color_source: default_color_source(),
            dynamic_color_enabled: false,
            selected_builtin_theme_id: String::new(),
            selected_palette_id: String::new(),
            locale: None,
            editor_font_size: default_editor_font_size(),
            editor_line_spacing_multiplier: default_editor_line_spacing_multiplier(),
            auto_save_enabled: default_auto_save_enabled(),
            auto_save_delay_ms: default_auto_save_delay_ms(),
            auto_indent_enabled: default_auto_indent_enabled(),
            auto_indent_width: default_auto_indent_width(),
            window_width: 800.0,
            window_height: 600.0,
            editor_typing_animation_enabled: default_editor_typing_animation_enabled(),
            editor_smooth_cursor_enabled: default_editor_smooth_cursor_enabled(),
            editor_typing_animation_duration_ms: default_editor_typing_animation_duration_ms(),
            editor_smooth_cursor_duration_ms: default_editor_smooth_cursor_duration_ms(),
            editor_coordinated_text_cursor_animation_enabled:
                default_editor_coordinated_text_cursor_animation_enabled(),
            ai_enabled: false,
            stats_device_id: None,
            desktop_sidebar_width: default_desktop_sidebar_width(),
            desktop_editor_width: default_desktop_editor_width(),
            diagnostics_enabled: default_diagnostics_enabled(),
            diagnostics_verbose: default_diagnostics_verbose(),
        }
    }
}

/// Cross-platform theme palette synced from Android Dynamic Color.
/// Non-Android clients only consume this; they never produce it.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ThemePalette {
    /// Source identifier, e.g. "android_dynamic_color"
    #[serde(default)]
    pub source: String,
    /// Epoch millis when palette was last updated
    #[serde(default)]
    pub updated_at_ms: i64,
    /// Device that produced this palette
    #[serde(default)]
    pub device_id: String,
    /// Variant name, e.g. "tonal_spot"
    #[serde(default)]
    pub variant: String,

    // Light palette
    #[serde(default)]
    pub light_primary: String,
    #[serde(default)]
    pub light_on_primary: String,
    #[serde(default)]
    pub light_primary_container: String,
    #[serde(default)]
    pub light_on_primary_container: String,
    #[serde(default)]
    pub light_secondary: String,
    #[serde(default)]
    pub light_on_secondary: String,
    #[serde(default)]
    pub light_secondary_container: String,
    #[serde(default)]
    pub light_on_secondary_container: String,
    #[serde(default)]
    pub light_tertiary: String,
    #[serde(default)]
    pub light_on_tertiary: String,
    #[serde(default)]
    pub light_tertiary_container: String,
    #[serde(default)]
    pub light_on_tertiary_container: String,
    #[serde(default)]
    pub light_background: String,
    #[serde(default)]
    pub light_on_background: String,
    #[serde(default)]
    pub light_surface: String,
    #[serde(default)]
    pub light_on_surface: String,
    #[serde(default)]
    pub light_surface_variant: String,
    #[serde(default)]
    pub light_on_surface_variant: String,
    #[serde(default)]
    pub light_surface_container_lowest: String,
    #[serde(default)]
    pub light_surface_container_low: String,
    #[serde(default)]
    pub light_surface_container: String,
    #[serde(default)]
    pub light_surface_container_high: String,
    #[serde(default)]
    pub light_surface_container_highest: String,
    #[serde(default)]
    pub light_outline: String,
    #[serde(default)]
    pub light_outline_variant: String,

    // Dark palette
    #[serde(default)]
    pub dark_primary: String,
    #[serde(default)]
    pub dark_on_primary: String,
    #[serde(default)]
    pub dark_primary_container: String,
    #[serde(default)]
    pub dark_on_primary_container: String,
    #[serde(default)]
    pub dark_secondary: String,
    #[serde(default)]
    pub dark_on_secondary: String,
    #[serde(default)]
    pub dark_secondary_container: String,
    #[serde(default)]
    pub dark_on_secondary_container: String,
    #[serde(default)]
    pub dark_tertiary: String,
    #[serde(default)]
    pub dark_on_tertiary: String,
    #[serde(default)]
    pub dark_tertiary_container: String,
    #[serde(default)]
    pub dark_on_tertiary_container: String,
    #[serde(default)]
    pub dark_background: String,
    #[serde(default)]
    pub dark_on_background: String,
    #[serde(default)]
    pub dark_surface: String,
    #[serde(default)]
    pub dark_on_surface: String,
    #[serde(default)]
    pub dark_surface_variant: String,
    #[serde(default)]
    pub dark_on_surface_variant: String,
    #[serde(default)]
    pub dark_surface_container_lowest: String,
    #[serde(default)]
    pub dark_surface_container_low: String,
    #[serde(default)]
    pub dark_surface_container: String,
    #[serde(default)]
    pub dark_surface_container_high: String,
    #[serde(default)]
    pub dark_surface_container_highest: String,
    #[serde(default)]
    pub dark_outline: String,
    #[serde(default)]
    pub dark_outline_variant: String,
}

/// Complete Material 3 ColorScheme for a single light or dark theme.
/// Covers all semantic roles defined by Material 3.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThemeColorScheme {
    #[serde(default)]
    pub primary: String,
    #[serde(default)]
    pub on_primary: String,
    #[serde(default)]
    pub primary_container: String,
    #[serde(default)]
    pub on_primary_container: String,
    #[serde(default)]
    pub inverse_primary: String,
    #[serde(default)]
    pub secondary: String,
    #[serde(default)]
    pub on_secondary: String,
    #[serde(default)]
    pub secondary_container: String,
    #[serde(default)]
    pub on_secondary_container: String,
    #[serde(default)]
    pub tertiary: String,
    #[serde(default)]
    pub on_tertiary: String,
    #[serde(default)]
    pub tertiary_container: String,
    #[serde(default)]
    pub on_tertiary_container: String,
    #[serde(default)]
    pub background: String,
    #[serde(default)]
    pub on_background: String,
    #[serde(default)]
    pub surface: String,
    #[serde(default)]
    pub on_surface: String,
    #[serde(default)]
    pub surface_variant: String,
    #[serde(default)]
    pub on_surface_variant: String,
    #[serde(default)]
    pub surface_tint: String,
    #[serde(default)]
    pub surface_dim: String,
    #[serde(default)]
    pub surface_bright: String,
    #[serde(default)]
    pub surface_container_lowest: String,
    #[serde(default)]
    pub surface_container_low: String,
    #[serde(default)]
    pub surface_container: String,
    #[serde(default)]
    pub surface_container_high: String,
    #[serde(default)]
    pub surface_container_highest: String,
    #[serde(default)]
    pub inverse_surface: String,
    #[serde(default)]
    pub inverse_on_surface: String,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub on_error: String,
    #[serde(default)]
    pub error_container: String,
    #[serde(default)]
    pub on_error_container: String,
    #[serde(default)]
    pub outline: String,
    #[serde(default)]
    pub outline_variant: String,
    #[serde(default)]
    pub scrim: String,
    #[serde(default)]
    pub primary_fixed: String,
    #[serde(default)]
    pub primary_fixed_dim: String,
    #[serde(default)]
    pub on_primary_fixed: String,
    #[serde(default)]
    pub on_primary_fixed_variant: String,
    #[serde(default)]
    pub secondary_fixed: String,
    #[serde(default)]
    pub secondary_fixed_dim: String,
    #[serde(default)]
    pub on_secondary_fixed: String,
    #[serde(default)]
    pub on_secondary_fixed_variant: String,
    #[serde(default)]
    pub tertiary_fixed: String,
    #[serde(default)]
    pub tertiary_fixed_dim: String,
    #[serde(default)]
    pub on_tertiary_fixed: String,
    #[serde(default)]
    pub on_tertiary_fixed_variant: String,
}
/// Each record is a complete snapshot of a Material 3 theme from one device.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThemePaletteRecord {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub palette_id: String,
    #[serde(default)]
    pub palette_fingerprint: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub source_platform: String,
    #[serde(default)]
    pub source_device_id: String,
    #[serde(default)]
    pub source_device_class: String,
    #[serde(default)]
    pub captured_at_ms: i64,
    #[serde(default)]
    pub variant: String,
    #[serde(default)]
    pub light_scheme: ThemeColorScheme,
    #[serde(default)]
    pub dark_scheme: ThemeColorScheme,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncableSettings {
    #[serde(default)]
    pub font_size: f64,
    /// Deprecated: appearance_mode is now per-device in LocalSettings.
    /// Retained for backward-compatible reading and migration.
    #[serde(default)]
    #[deprecated(note = "use LocalSettings.appearance_mode instead")]
    pub theme_mode: String,
    /// Deprecated: use theme_palette instead. Retained for backward-compatible reading.
    #[serde(default)]
    #[deprecated(note = "use theme_palette instead")]
    pub monet_color: String,
    /// Deprecated: use palette catalog (app-meta/themes/palettes/) instead.
    /// Retained for backward-compatible reading and migration.
    #[serde(default)]
    #[deprecated(note = "use palette catalog instead")]
    pub theme_palette: ThemePalette,
}

pub fn load_local_settings(config_dir: &Path) -> Result<LocalSettings> {
    let path = config_dir.join("settings.local.json");
    if !path.exists() {
        return Ok(LocalSettings::default());
    }
    let content = fs::read_to_string(&path)?;
    // Issue #705 评论 5716410988: 旧版本 LocalSettings 同时有 theme_mode 和 appearance_mode,
    // rename_all=camelCase 后旧文件同时存在 themeMode 和 appearanceMode。
    // 不能用 serde alias(会报 duplicate field),改为原始 JSON 迁移:
    // 取值顺序:有效的 appearanceMode > 旧 themeMode > system 默认。
    // 读完后写回当前格式,只保留 appearanceMode。
    let mut root: serde_json::Value = serde_json::from_str(&content)?;
    let resolved_appearance = resolve_appearance_mode(&root);
    // Issue #705 评论 5716919024: 只要旧文件存在 themeMode/theme_mode 旧字段,
    // 读取成功后就要写回一次,确保磁盘上不再保留第二套旧字段。
    // 之前用字符串 contains 判断"有 themeMode 且没有 appearanceMode",
    // 旧文件同时含两个 key 时不会写回,磁盘上 themeMode 会一直留着。
    let had_legacy_theme_mode = root
        .as_object()
        .map(|obj| obj.contains_key("themeMode") || obj.contains_key("theme_mode"))
        .unwrap_or(false);
    // 确保只保留 appearanceMode,删除旧 themeMode key(如果存在)
    if let Some(obj) = root.as_object_mut() {
        obj.remove("themeMode");
        obj.remove("theme_mode");
        obj.insert(
            "appearanceMode".to_string(),
            serde_json::Value::String(resolved_appearance.clone()),
        );
    }
    let mut settings: LocalSettings = serde_json::from_value(root)?;
    settings.validate();
    if had_legacy_theme_mode {
        // 静默写回,只保留 appearanceMode。写回失败不应阻止读取。
        let _ = save_local_settings(config_dir, &settings);
    }
    Ok(settings)
}

/// Issue #705 评论 5716919024: appearance_mode 运行时只允许这三个值。
/// resolve_appearance_mode 取值和 validate 归一都依赖此判定。
fn is_valid_appearance_mode(s: &str) -> bool {
    matches!(s, "system" | "light" | "dark")
}

/// Issue #705 评论 5716410988: 解析旧/新配置的 appearance mode。
/// 取值顺序:有效的 appearanceMode > 旧 themeMode > system。
/// Issue #705 评论 5716919024: 只接受 system/light/dark,非法值继续回退下一来源,最终回 system。
fn resolve_appearance_mode(root: &serde_json::Value) -> String {
    let default = default_appearance_mode();
    let obj = match root.as_object() {
        Some(o) => o,
        None => return default,
    };
    // 优先 appearanceMode
    if let Some(v) = obj.get("appearanceMode") {
        if let Some(s) = v.as_str() {
            if is_valid_appearance_mode(s) {
                return s.to_string();
            }
        }
    }
    // 回退到旧 themeMode
    if let Some(v) = obj.get("themeMode") {
        if let Some(s) = v.as_str() {
            if is_valid_appearance_mode(s) {
                return s.to_string();
            }
        }
    }
    // 也检查 snake_case key(以防 rename_all 不生效的极端情况)
    if let Some(v) = obj.get("appearance_mode") {
        if let Some(s) = v.as_str() {
            if is_valid_appearance_mode(s) {
                return s.to_string();
            }
        }
    }
    if let Some(v) = obj.get("theme_mode") {
        if let Some(s) = v.as_str() {
            if is_valid_appearance_mode(s) {
                return s.to_string();
            }
        }
    }
    default
}

pub fn save_local_settings(config_dir: &Path, settings: &LocalSettings) -> Result<()> {
    let path = config_dir.join("settings.local.json");
    let content = serde_json::to_string_pretty(settings)?;
    crate::storage::atomic_write_string(&path, &content)
}

pub fn load_syncable_settings(config_dir: &Path) -> Result<SyncableSettings> {
    let path = config_dir.join("settings.sync.json");
    if !path.exists() {
        return Ok(SyncableSettings::default());
    }
    let content = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&content)?)
}

pub fn save_syncable_settings(config_dir: &Path, settings: &SyncableSettings) -> Result<()> {
    let path = config_dir.join("settings.sync.json");
    let content = serde_json::to_string_pretty(settings)?;
    crate::storage::atomic_write_string(&path, &content)
}

// ──   settings 的 *_with_changes 入口 ──
//
// settings 文件直接写在 `config_dir`（= `app_data_root`）下，
// 所以 workspace-relative 路径就是文件名本身。
// palette 文件写在 `config_dir/themes/palettes/<device_id>/<fingerprint>.json`，
// workspace-relative 是 `themes/palettes/<device_id>/<fingerprint>.json`。
// 这些路径在 `classify_workspace_path_str` 中归为 UserSetting/UserContent，
// `is_workspace_history_path` 返回 true，会进入本地 Git history。

///   save_local_settings 的变更集版本。
///
/// 返回 `WorkspaceChangeSet`，变更集包含 `Upsert(settings.local.json)`。
pub fn save_local_settings_with_changes(
    config_dir: &Path,
    settings: &LocalSettings,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    save_local_settings(config_dir, settings)?;
    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_upsert(std::path::PathBuf::from("settings.local.json"));
    Ok(change_set)
}

///   save_syncable_settings 的变更集版本。
///
/// 返回 `WorkspaceChangeSet`，变更集包含 `Upsert(settings.sync.json)`。
pub fn save_syncable_settings_with_changes(
    config_dir: &Path,
    settings: &SyncableSettings,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    save_syncable_settings(config_dir, settings)?;
    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_upsert(std::path::PathBuf::from("settings.sync.json"));
    Ok(change_set)
}

/// 粗粒度设备信息，用于同步和统计。
/// 不包含详细硬件型号、序列号、用户名、系统账户路径等隐私信息。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// 本地持久化随机 UUID
    #[serde(default)]
    pub device_id: String,
    /// phone / tablet / desktop
    #[serde(default)]
    pub device_class: String,
    /// android / harmony / desktop
    #[serde(default)]
    pub platform: String,
}

pub fn load_device_info(config_dir: &Path) -> Result<DeviceInfo> {
    let path = config_dir.join("device/current_device.json");
    if !path.exists() {
        return Ok(DeviceInfo::default());
    }
    let content = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&content)?)
}

pub fn save_device_info(config_dir: &Path, info: &DeviceInfo) -> Result<()> {
    let path = config_dir.join("device/current_device.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(info)?;
    crate::storage::atomic_write_string(&path, &content)
}

/// 确保设备信息存在，如果不存在则创建并持久化。
/// 仅在字段为空时填充，已有值不会被覆盖。
/// 当 `preferred_device_id` 为 `Some` 时优先使用平台注入值，避免随机生成。
pub fn ensure_device_info(
    config_dir: &Path,
    platform: &str,
    device_class: &str,
    preferred_device_id: Option<&str>,
) -> Result<DeviceInfo> {
    let mut info = load_device_info(config_dir).unwrap_or_default();
    let mut changed = false;
    if info.device_id.is_empty() {
        info.device_id = preferred_device_id
            .filter(|id| !id.is_empty())
            .map(|id| id.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        changed = true;
    }
    if info.platform.is_empty() {
        info.platform = platform.to_string();
        changed = true;
    }
    if info.device_class.is_empty() {
        info.device_class = device_class.to_string();
        changed = true;
    }
    if changed {
        save_device_info(config_dir, &info)?;
    }
    Ok(info)
}

/// Returns the effective editor font size.
/// Primary source: SyncableSettings.font_size
/// Fallback: LocalSettings.editor_font_size (when syncable <= 0)
/// Final default: 16.0
pub fn get_effective_font_size(config_dir: &Path) -> f64 {
    let syncable = load_syncable_settings(config_dir);
    if let Ok(s) = syncable {
        if s.font_size > 0.0 {
            return s.font_size;
        }
    }
    let local = load_local_settings(config_dir);
    if let Ok(s) = local {
        if s.editor_font_size > 0.0 {
            return f64::from(s.editor_font_size);
        }
    }
    16.0
}

/// Sets the editor font size in SyncableSettings.
/// Does NOT modify LocalSettings.editor_font_size (preserved for backward compatibility).
pub fn set_editor_font_size(config_dir: &Path, font_size: f64) -> Result<()> {
    let mut syncable = load_syncable_settings(config_dir).unwrap_or_default();
    syncable.font_size = font_size;
    save_syncable_settings(config_dir, &syncable)
}

// ── Palette catalog operations ──

/// Base directory for palette catalog.
fn palettes_base_dir(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join("themes/palettes")
}

/// Issue #709 评论 5728916561: 判断 hex 颜色是否为深色（亮度低于阈值）。
///
/// 解析 `#RRGGBB` 格式的 R/G/B 分量，计算相对亮度
/// `(0.299*R + 0.587*G + 0.114*B) / 255`，低于 0.5 视为深色。
/// 空字符串或无效格式返回 `false`（不确定时不触发修正）。
fn is_dark_color(hex: &str) -> bool {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 {
        return false;
    }
    let r = u32::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u32::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u32::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    let luminance = (0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b)) / 255.0;
    luminance < 0.5
}

/// Issue #709 评论 5728916561: 验证并修正 color scheme 中明显不合理的颜色组合。
///
/// **深色 scheme（`is_dark_scheme == true`）**：
/// - `on_surface` 为 `#000000` 或空且 `surface` 是深色 → 修正为 `#DFE3E7`
/// - `on_surface_variant` 为 `#000000` 或空且 `surface_variant` 是深色 → 修正为 `#C1C7CE`
/// - `on_background` 为 `#000000` 或空且 `background` 是深色 → 修正为 `#DFE3E7`
///
/// **浅色 scheme（`is_dark_scheme == false`）**：
/// - `on_surface` 为 `#FFFFFF` 或空且 `surface` 是浅色 → 修正为 `#171C1F`
/// - `on_surface_variant` 为 `#FFFFFF` 或空且 `surface_variant` 是浅色 → 修正为 `#41484D`
/// - `on_background` 为 `#FFFFFF` 或空且 `background` 是浅色 → 修正为 `#171C1F`
///
/// 只修正明显不合理的组合（黑字配深色背景、白字配浅色背景），有效数据不受影响。
fn sanitize_color_scheme(scheme: &mut ThemeColorScheme, is_dark_scheme: bool) {
    if is_dark_scheme {
        // 深色 scheme：前景色不应是黑色或空（配深色背景时不可见）
        if (scheme.on_surface == "#000000" || scheme.on_surface.is_empty())
            && is_dark_color(&scheme.surface)
        {
            scheme.on_surface = "#DFE3E7".to_string();
        }
        if (scheme.on_surface_variant == "#000000" || scheme.on_surface_variant.is_empty())
            && is_dark_color(&scheme.surface_variant)
        {
            scheme.on_surface_variant = "#C1C7CE".to_string();
        }
        if (scheme.on_background == "#000000" || scheme.on_background.is_empty())
            && is_dark_color(&scheme.background)
        {
            scheme.on_background = "#DFE3E7".to_string();
        }
    } else {
        // 浅色 scheme：前景色不应是白色或空（配浅色背景时不可见）
        if (scheme.on_surface == "#FFFFFF" || scheme.on_surface.is_empty())
            && !is_dark_color(&scheme.surface)
        {
            scheme.on_surface = "#171C1F".to_string();
        }
        if (scheme.on_surface_variant == "#FFFFFF" || scheme.on_surface_variant.is_empty())
            && !is_dark_color(&scheme.surface_variant)
        {
            scheme.on_surface_variant = "#41484D".to_string();
        }
        if (scheme.on_background == "#FFFFFF" || scheme.on_background.is_empty())
            && !is_dark_color(&scheme.background)
        {
            scheme.on_background = "#171C1F".to_string();
        }
    }
}

/// Compute a stable fingerprint for a pair of color schemes.
/// Uses SHA-256 on the normalized JSON of light + dark schemes.
pub fn compute_palette_fingerprint(light: &ThemeColorScheme, dark: &ThemeColorScheme) -> String {
    use sha2::Digest;
    use std::fmt::Write;
    let mut hasher = sha2::Sha256::new();
    sha2::Digest::update(
        &mut hasher,
        serde_json::to_string(light).unwrap_or_default().as_bytes(),
    );
    sha2::Digest::update(
        &mut hasher,
        serde_json::to_string(dark).unwrap_or_default().as_bytes(),
    );
    let hash = sha2::Digest::finalize(hasher);
    let mut hex = String::with_capacity(16);
    for byte in &hash[..8] {
        #[allow(clippy::unwrap_used)]
        write!(&mut hex, "{:02x}", byte).unwrap();
    }
    hex
}

/// Save a palette record to the catalog.
/// Path: `app-meta/themes/palettes/<device_id>/<fingerprint>.json`
/// If the file already exists, it is not overwritten (immutable).
pub fn save_palette_record(config_dir: &Path, record: &ThemePaletteRecord) -> Result<()> {
    let dir = palettes_base_dir(config_dir).join(&record.source_device_id);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", record.palette_fingerprint));
    if path.exists() {
        return Ok(());
    }
    let content = serde_json::to_string_pretty(record)?;
    crate::storage::atomic_write_string(&path, &content)
}

///   save_palette_record 的变更集版本。
///
/// 返回 `WorkspaceChangeSet`，变更集包含
/// `Upsert(themes/palettes/<device_id>/<fingerprint>.json)`。
/// palette 属于用户设置（UserSetting/UserContent），应进入本地 history。
pub fn save_palette_record_with_changes(
    config_dir: &Path,
    record: &ThemePaletteRecord,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    save_palette_record(config_dir, record)?;
    let rel = std::path::PathBuf::from("themes")
        .join("palettes")
        .join(&record.source_device_id)
        .join(format!("{}.json", record.palette_fingerprint));
    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new().add_upsert(rel);
    Ok(change_set)
}

/// Load a specific palette record by device_id and fingerprint.
pub fn load_palette_record(
    config_dir: &Path,
    device_id: &str,
    fingerprint: &str,
) -> Result<ThemePaletteRecord> {
    let path = palettes_base_dir(config_dir)
        .join(device_id)
        .join(format!("{}.json", fingerprint));
    let content = fs::read_to_string(&path)?;
    let mut record: ThemePaletteRecord = serde_json::from_str(&content)?;
    // Issue #709 评论 5728916561: 验证并修正 palette record 的颜色值。
    // 如果 dark_scheme 的前景色是 #000000 或空字符串配深色 surface，
    // 说明 palette 数据有问题（来自 Android 动态颜色系统的错误数据或
    // legacy migration 缺失字段），在此修正源头数据而不是在 QML 反色。
    sanitize_color_scheme(&mut record.light_scheme, false);
    sanitize_color_scheme(&mut record.dark_scheme, true);
    Ok(record)
}

/// List all palette records in the catalog.
/// Scans `app-meta/themes/palettes/<device_id>/<fingerprint>.json` recursively.
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub fn list_palette_records(config_dir: &Path) -> Result<Vec<ThemePaletteRecord>> {
    let base = palettes_base_dir(config_dir);
    if !base.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for device_dir in fs::read_dir(&base)? {
        let device_dir = device_dir?;
        if !device_dir.file_type()?.is_dir() {
            continue;
        }
        for file_entry in fs::read_dir(device_dir.path())? {
            let file_entry = file_entry?;
            let path = file_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(record) = serde_json::from_str::<ThemePaletteRecord>(&content) {
                    records.push(record);
                }
            }
        }
    }
    records.sort_by_key(|b| std::cmp::Reverse(b.captured_at_ms));
    Ok(records)
}

/// Delete a specific palette record.
pub fn delete_palette_record(config_dir: &Path, device_id: &str, fingerprint: &str) -> Result<()> {
    let path = palettes_base_dir(config_dir)
        .join(device_id)
        .join(format!("{}.json", fingerprint));
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

///   delete_palette_record 的变更集版本。
///
/// 返回 `WorkspaceChangeSet`，变更集包含
/// `Delete(themes/palettes/<device_id>/<fingerprint>.json)`。
pub fn delete_palette_record_with_changes(
    config_dir: &Path,
    device_id: &str,
    fingerprint: &str,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    delete_palette_record(config_dir, device_id, fingerprint)?;
    let rel = std::path::PathBuf::from("themes")
        .join("palettes")
        .join(device_id)
        .join(format!("{}.json", fingerprint));
    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new().add_delete(rel);
    Ok(change_set)
}

/// Convert legacy ThemePalette to ThemePaletteRecord for migration.
/// Legacy ThemePalette has flat light_/dark_ prefixed fields;
/// this converts them into the new ThemeColorScheme structure.
#[allow(deprecated)]
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub fn legacy_palette_to_record(palette: &ThemePalette) -> ThemePaletteRecord {
    let mut light = ThemeColorScheme {
        primary: palette.light_primary.clone(),
        on_primary: palette.light_on_primary.clone(),
        primary_container: palette.light_primary_container.clone(),
        on_primary_container: palette.light_on_primary_container.clone(),
        inverse_primary: String::new(),
        secondary: palette.light_secondary.clone(),
        on_secondary: palette.light_on_secondary.clone(),
        secondary_container: palette.light_secondary_container.clone(),
        on_secondary_container: palette.light_on_secondary_container.clone(),
        tertiary: palette.light_tertiary.clone(),
        on_tertiary: palette.light_on_tertiary.clone(),
        tertiary_container: palette.light_tertiary_container.clone(),
        on_tertiary_container: palette.light_on_tertiary_container.clone(),
        background: palette.light_background.clone(),
        on_background: palette.light_on_background.clone(),
        surface: palette.light_surface.clone(),
        on_surface: palette.light_on_surface.clone(),
        surface_variant: palette.light_surface_variant.clone(),
        on_surface_variant: palette.light_on_surface_variant.clone(),
        surface_tint: String::new(),
        surface_dim: String::new(),
        surface_bright: String::new(),
        surface_container_lowest: palette.light_surface_container_lowest.clone(),
        surface_container_low: palette.light_surface_container_low.clone(),
        surface_container: palette.light_surface_container.clone(),
        surface_container_high: palette.light_surface_container_high.clone(),
        surface_container_highest: palette.light_surface_container_highest.clone(),
        inverse_surface: String::new(),
        inverse_on_surface: String::new(),
        error: String::new(),
        on_error: String::new(),
        error_container: String::new(),
        on_error_container: String::new(),
        outline: palette.light_outline.clone(),
        outline_variant: palette.light_outline_variant.clone(),
        scrim: String::new(),
        primary_fixed: String::new(),
        primary_fixed_dim: String::new(),
        on_primary_fixed: String::new(),
        on_primary_fixed_variant: String::new(),
        secondary_fixed: String::new(),
        secondary_fixed_dim: String::new(),
        on_secondary_fixed: String::new(),
        on_secondary_fixed_variant: String::new(),
        tertiary_fixed: String::new(),
        tertiary_fixed_dim: String::new(),
        on_tertiary_fixed: String::new(),
        on_tertiary_fixed_variant: String::new(),
    };
    let mut dark = ThemeColorScheme {
        primary: palette.dark_primary.clone(),
        on_primary: palette.dark_on_primary.clone(),
        primary_container: palette.dark_primary_container.clone(),
        on_primary_container: palette.dark_on_primary_container.clone(),
        inverse_primary: String::new(),
        secondary: palette.dark_secondary.clone(),
        on_secondary: palette.dark_on_secondary.clone(),
        secondary_container: palette.dark_secondary_container.clone(),
        on_secondary_container: palette.dark_on_secondary_container.clone(),
        tertiary: palette.dark_tertiary.clone(),
        on_tertiary: palette.dark_on_tertiary.clone(),
        tertiary_container: palette.dark_tertiary_container.clone(),
        on_tertiary_container: palette.dark_on_tertiary_container.clone(),
        background: palette.dark_background.clone(),
        on_background: palette.dark_on_background.clone(),
        surface: palette.dark_surface.clone(),
        on_surface: palette.dark_on_surface.clone(),
        surface_variant: palette.dark_surface_variant.clone(),
        on_surface_variant: palette.dark_on_surface_variant.clone(),
        surface_tint: String::new(),
        surface_dim: String::new(),
        surface_bright: String::new(),
        surface_container_lowest: palette.dark_surface_container_lowest.clone(),
        surface_container_low: palette.dark_surface_container_low.clone(),
        surface_container: palette.dark_surface_container.clone(),
        surface_container_high: palette.dark_surface_container_high.clone(),
        surface_container_highest: palette.dark_surface_container_highest.clone(),
        inverse_surface: String::new(),
        inverse_on_surface: String::new(),
        error: String::new(),
        on_error: String::new(),
        error_container: String::new(),
        on_error_container: String::new(),
        outline: palette.dark_outline.clone(),
        outline_variant: palette.dark_outline_variant.clone(),
        scrim: String::new(),
        primary_fixed: String::new(),
        primary_fixed_dim: String::new(),
        on_primary_fixed: String::new(),
        on_primary_fixed_variant: String::new(),
        secondary_fixed: String::new(),
        secondary_fixed_dim: String::new(),
        on_secondary_fixed: String::new(),
        on_secondary_fixed_variant: String::new(),
        tertiary_fixed: String::new(),
        tertiary_fixed_dim: String::new(),
        on_tertiary_fixed: String::new(),
        on_tertiary_fixed_variant: String::new(),
    };
    // Issue #709 评论 5728916561: legacy palette 的 dark_* 字段可能为空或 #000000，
    // 迁移时验证并修正颜色值。
    sanitize_color_scheme(&mut light, false);
    sanitize_color_scheme(&mut dark, true);
    let fingerprint = compute_palette_fingerprint(&light, &dark);
    let device_id = if palette.device_id.is_empty() {
        "legacy".to_string()
    } else {
        palette.device_id.clone()
    };
    let palette_id = format!("{}:{}", device_id, fingerprint);
    let variant = if palette.variant == "tonal_spot" && palette.source == "android_dynamic_color" {
        "system_selected".to_string()
    } else {
        palette.variant.clone()
    };
    ThemePaletteRecord {
        schema_version: 1,
        palette_id,
        palette_fingerprint: fingerprint,
        source: palette.source.clone(),
        source_platform: String::new(),
        source_device_id: device_id,
        source_device_class: String::new(),
        captured_at_ms: palette.updated_at_ms,
        variant,
        light_scheme: light,
        dark_scheme: dark,
    }
}

/// Migrate legacy ThemePalette from SyncableSettings to palette catalog.
/// Does nothing if the legacy palette is empty/default.
/// Returns true if migration was performed.
#[allow(deprecated)]
pub fn migrate_legacy_theme_palette(config_dir: &Path) -> Result<bool> {
    let syncable = load_syncable_settings(config_dir)?;
    if syncable.theme_palette.source.is_empty() && syncable.theme_palette.light_primary.is_empty() {
        return Ok(false);
    }
    let record = legacy_palette_to_record(&syncable.theme_palette);
    save_palette_record(config_dir, &record)?;
    let mut local = load_local_settings(config_dir)?;
    let need_palette_update = local.selected_palette_id.is_empty();
    if need_palette_update {
        local.selected_palette_id = record.palette_id.clone();
        local.color_source = "saved_palette".to_string();
    }
    let need_appearance_update = local.appearance_mode == "system"
        && syncable.theme_mode != "system"
        && !syncable.theme_mode.is_empty();
    if need_appearance_update {
        local.appearance_mode = syncable.theme_mode.clone();
    }
    if need_palette_update || need_appearance_update {
        save_local_settings(config_dir, &local)?;
    }
    Ok(true)
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinTheme {
    pub theme_id: &'static str,
    pub name: &'static str,
    pub light_scheme: ThemeColorScheme,
    pub dark_scheme: ThemeColorScheme,
}

#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub fn list_builtin_themes() -> Vec<BuiltinTheme> {
    vec![BuiltinTheme {
        theme_id: "sujian_default",
        name: "素笺默认",
        light_scheme: ThemeColorScheme {
            primary: "#006493".to_string(),
            on_primary: "#FFFFFF".to_string(),
            primary_container: "#C9E6FF".to_string(),
            on_primary_container: "#001E2F".to_string(),
            inverse_primary: "#87CEFF".to_string(),
            secondary: "#50606E".to_string(),
            on_secondary: "#FFFFFF".to_string(),
            secondary_container: "#D3E5F5".to_string(),
            on_secondary_container: "#0C1D29".to_string(),
            tertiary: "#65587B".to_string(),
            on_tertiary: "#FFFFFF".to_string(),
            tertiary_container: "#EBDDFF".to_string(),
            on_tertiary_container: "#201634".to_string(),
            background: "#F6FAFE".to_string(),
            on_background: "#171C1F".to_string(),
            surface: "#F6FAFE".to_string(),
            on_surface: "#171C1F".to_string(),
            surface_variant: "#DDE3EA".to_string(),
            on_surface_variant: "#41484D".to_string(),
            surface_tint: "#006493".to_string(),
            surface_dim: "#D7DADE".to_string(),
            surface_bright: "#F6FAFE".to_string(),
            surface_container_lowest: "#FFFFFF".to_string(),
            surface_container_low: "#F0F4F8".to_string(),
            surface_container: "#EBEEF2".to_string(),
            surface_container_high: "#E5E8EC".to_string(),
            surface_container_highest: "#DFE3E7".to_string(),
            inverse_surface: "#2C3134".to_string(),
            inverse_on_surface: "#ECF0F4".to_string(),
            error: "#BA1A1A".to_string(),
            on_error: "#FFFFFF".to_string(),
            error_container: "#FFDAD6".to_string(),
            on_error_container: "#410002".to_string(),
            outline: "#71787D".to_string(),
            outline_variant: "#C1C7CE".to_string(),
            scrim: "#000000".to_string(),
            primary_fixed: "#C9E6FF".to_string(),
            primary_fixed_dim: "#A5CCF0".to_string(),
            on_primary_fixed: "#001E2F".to_string(),
            on_primary_fixed_variant: "#004B6E".to_string(),
            secondary_fixed: "#D3E5F5".to_string(),
            secondary_fixed_dim: "#B7C9D8".to_string(),
            on_secondary_fixed: "#0C1D29".to_string(),
            on_secondary_fixed_variant: "#384956".to_string(),
            tertiary_fixed: "#EBDDFF".to_string(),
            tertiary_fixed_dim: "#CFC0E7".to_string(),
            on_tertiary_fixed: "#201634".to_string(),
            on_tertiary_fixed_variant: "#4D4462".to_string(),
        },
        dark_scheme: ThemeColorScheme {
            primary: "#87CEFF".to_string(),
            on_primary: "#00344D".to_string(),
            primary_container: "#004B6E".to_string(),
            on_primary_container: "#C9E6FF".to_string(),
            inverse_primary: "#006493".to_string(),
            secondary: "#B7C9D8".to_string(),
            on_secondary: "#22323F".to_string(),
            secondary_container: "#384956".to_string(),
            on_secondary_container: "#D3E5F5".to_string(),
            tertiary: "#CFC0E7".to_string(),
            on_tertiary: "#362E4A".to_string(),
            tertiary_container: "#4D4462".to_string(),
            on_tertiary_container: "#EBDDFF".to_string(),
            background: "#0F1417".to_string(),
            on_background: "#DFE3E7".to_string(),
            surface: "#0F1417".to_string(),
            on_surface: "#DFE3E7".to_string(),
            surface_variant: "#41484D".to_string(),
            on_surface_variant: "#C1C7CE".to_string(),
            surface_tint: "#87CEFF".to_string(),
            surface_dim: "#0F1417".to_string(),
            surface_bright: "#353A3D".to_string(),
            surface_container_lowest: "#0A0F12".to_string(),
            surface_container_low: "#171C1F".to_string(),
            surface_container: "#1C2023".to_string(),
            surface_container_high: "#262B2E".to_string(),
            surface_container_highest: "#313539".to_string(),
            inverse_surface: "#DFE3E7".to_string(),
            inverse_on_surface: "#2C3134".to_string(),
            error: "#FFB4AB".to_string(),
            on_error: "#690005".to_string(),
            error_container: "#93000A".to_string(),
            on_error_container: "#FFDAD6".to_string(),
            outline: "#8B9198".to_string(),
            outline_variant: "#41484D".to_string(),
            scrim: "#000000".to_string(),
            primary_fixed: "#C9E6FF".to_string(),
            primary_fixed_dim: "#87CEFF".to_string(),
            on_primary_fixed: "#001E2F".to_string(),
            on_primary_fixed_variant: "#004B6E".to_string(),
            secondary_fixed: "#D3E5F5".to_string(),
            secondary_fixed_dim: "#B7C9D8".to_string(),
            on_secondary_fixed: "#0C1D29".to_string(),
            on_secondary_fixed_variant: "#384956".to_string(),
            tertiary_fixed: "#EBDDFF".to_string(),
            tertiary_fixed_dim: "#CFC0E7".to_string(),
            on_tertiary_fixed: "#201634".to_string(),
            on_tertiary_fixed_variant: "#4D4462".to_string(),
        },
    }]
}

#[cfg(test)]
mod inline_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_get_effective_font_size_syncable_primary() {
        let temp_dir = tempdir().unwrap();
        let mut syncable = load_syncable_settings(temp_dir.path()).unwrap_or_default();
        syncable.font_size = 20.0;
        save_syncable_settings(temp_dir.path(), &syncable).unwrap();

        let size = get_effective_font_size(temp_dir.path());
        assert_eq!(size, 20.0);
    }

    #[test]
    fn test_auto_indent_enabled_persists() {
        let temp_dir = tempdir().unwrap();

        let mut settings = LocalSettings::default();
        settings.auto_indent_enabled = true;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert!(
            loaded.auto_indent_enabled,
            "auto_indent_enabled should persist as true"
        );

        let mut settings2 = loaded;
        settings2.auto_indent_enabled = false;
        save_local_settings(temp_dir.path(), &settings2).unwrap();

        let loaded2 = load_local_settings(temp_dir.path()).unwrap();
        assert!(
            !loaded2.auto_indent_enabled,
            "auto_indent_enabled should persist as false after change"
        );
    }

    #[test]
    fn test_get_effective_font_size_fallback_to_local() {
        let temp_dir = tempdir().unwrap();
        let mut local = LocalSettings::default();
        local.editor_font_size = 18.0;
        save_local_settings(temp_dir.path(), &local).unwrap();

        let size = get_effective_font_size(temp_dir.path());
        assert_eq!(size, 18.0);
    }

    #[test]
    fn test_get_effective_font_size_default() {
        let temp_dir = tempdir().unwrap();
        let size = get_effective_font_size(temp_dir.path());
        assert_eq!(size, 16.0);
    }

    #[test]
    fn test_get_effective_font_size_syncable_zero_uses_local() {
        let temp_dir = tempdir().unwrap();
        let mut syncable = SyncableSettings::default();
        syncable.font_size = 0.0;
        save_syncable_settings(temp_dir.path(), &syncable).unwrap();

        let mut local = LocalSettings::default();
        local.editor_font_size = 22.0;
        save_local_settings(temp_dir.path(), &local).unwrap();

        let size = get_effective_font_size(temp_dir.path());
        assert_eq!(size, 22.0);
    }

    #[test]
    fn test_set_editor_font_size_writes_syncable() {
        let temp_dir = tempdir().unwrap();
        set_editor_font_size(temp_dir.path(), 24.0).unwrap();

        let syncable = load_syncable_settings(temp_dir.path()).unwrap();
        assert_eq!(syncable.font_size, 24.0);
    }

    #[test]
    fn test_set_editor_font_size_does_not_modify_local() {
        let temp_dir = tempdir().unwrap();
        let mut local = LocalSettings::default();
        local.editor_font_size = 14.0;
        save_local_settings(temp_dir.path(), &local).unwrap();

        set_editor_font_size(temp_dir.path(), 30.0).unwrap();

        let local_after = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(local_after.editor_font_size, 14.0);
    }

    #[test]
    fn test_ranges_constants_exist() {
        // 验证常量存在且值合理
        assert_eq!(ranges::FONT_SIZE_MIN, 12.0);
        assert_eq!(ranges::FONT_SIZE_MAX, 72.0);
        assert_eq!(ranges::LINE_SPACING_MIN, 1.0);
        assert_eq!(ranges::LINE_SPACING_MAX, 3.0);
        assert_eq!(ranges::INDENT_WIDTH_MIN, 0.0);
        assert_eq!(ranges::INDENT_WIDTH_MAX, 8.0);
        assert_eq!(ranges::ANIMATION_DURATION_MIN_MS, 30);
        assert_eq!(ranges::ANIMATION_DURATION_MAX_MS, 1000);
        assert_eq!(ranges::AUTO_SAVE_DELAY_MIN_MS, 1000);
        assert_eq!(ranges::AUTO_SAVE_DELAY_MAX_MS, 10000);
    }

    #[test]
    fn test_validate_clamps_values() {
        let mut settings = LocalSettings {
            editor_font_size: 999.0,
            editor_line_spacing_multiplier: 10.0,
            auto_indent_width: 100.0,
            editor_typing_animation_duration_ms: 5000,
            editor_smooth_cursor_duration_ms: 0,
            auto_save_delay_ms: 50,
            ..LocalSettings::default()
        };
        settings.validate();
        assert_eq!(settings.editor_font_size, ranges::FONT_SIZE_MAX);
        assert_eq!(
            settings.editor_line_spacing_multiplier,
            ranges::LINE_SPACING_MAX
        );
        assert_eq!(settings.auto_indent_width, ranges::INDENT_WIDTH_MAX);
        assert_eq!(
            settings.editor_typing_animation_duration_ms,
            ranges::ANIMATION_DURATION_MAX_MS
        );
        assert_eq!(
            settings.editor_smooth_cursor_duration_ms,
            ranges::ANIMATION_DURATION_MIN_MS
        );
        assert_eq!(settings.auto_save_delay_ms, ranges::AUTO_SAVE_DELAY_MIN_MS);

        // 验证低于下限也被 clamp
        let mut settings_low = LocalSettings {
            editor_font_size: 1.0,
            editor_line_spacing_multiplier: 0.2,
            auto_indent_width: -5.0,
            auto_save_delay_ms: 100,
            ..LocalSettings::default()
        };
        settings_low.validate();
        assert_eq!(settings_low.editor_font_size, ranges::FONT_SIZE_MIN);
        assert_eq!(
            settings_low.editor_line_spacing_multiplier,
            ranges::LINE_SPACING_MIN
        );
        assert_eq!(settings_low.auto_indent_width, ranges::INDENT_WIDTH_MIN);
        assert_eq!(
            settings_low.auto_save_delay_ms,
            ranges::AUTO_SAVE_DELAY_MIN_MS
        );
    }

    #[test]
    fn diagnostics_enabled_default_true() {
        let settings = LocalSettings::default();
        assert!(
            settings.diagnostics_enabled,
            "diagnostics_enabled should default to true (alpha)"
        );
    }

    #[test]
    fn diagnostics_verbose_default_true() {
        let settings = LocalSettings::default();
        assert!(
            settings.diagnostics_verbose,
            "diagnostics_verbose should default to true (alpha)"
        );
    }

    #[test]
    fn diagnostics_persist_and_load() {
        let temp_dir = tempdir().unwrap();
        let mut settings = LocalSettings::default();
        settings.diagnostics_enabled = true;
        settings.diagnostics_verbose = true;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert!(
            loaded.diagnostics_enabled,
            "diagnostics_enabled should persist as true"
        );
        assert!(
            loaded.diagnostics_verbose,
            "diagnostics_verbose should persist as true"
        );

        // Test round-trip with false
        settings.diagnostics_enabled = false;
        settings.diagnostics_verbose = false;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert!(!loaded.diagnostics_enabled);
        assert!(!loaded.diagnostics_verbose);
    }

    // --- Guard tests for different setting combinations ---

    #[test]
    fn test_diagnostics_enabled_false_persists() {
        let temp_dir = tempdir().unwrap();
        let mut settings = LocalSettings::default();
        settings.diagnostics_enabled = false;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert!(
            !loaded.diagnostics_enabled,
            "diagnostics_enabled=false should persist correctly"
        );
    }

    #[test]
    fn test_diagnostics_verbose_false_persists() {
        let temp_dir = tempdir().unwrap();
        let mut settings = LocalSettings::default();
        settings.diagnostics_verbose = false;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert!(
            !loaded.diagnostics_verbose,
            "diagnostics_verbose=false should persist correctly"
        );
    }

    #[test]
    fn test_typing_animation_toggle_persists() {
        let temp_dir = tempdir().unwrap();

        // Default is true, toggle to false
        let mut settings = LocalSettings::default();
        assert!(
            settings.editor_typing_animation_enabled,
            "default should be true"
        );
        settings.editor_typing_animation_enabled = false;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert!(
            !loaded.editor_typing_animation_enabled,
            "typing animation should persist as false after toggle"
        );

        // Toggle back to true
        let mut settings2 = loaded;
        settings2.editor_typing_animation_enabled = true;
        save_local_settings(temp_dir.path(), &settings2).unwrap();

        let loaded2 = load_local_settings(temp_dir.path()).unwrap();
        assert!(
            loaded2.editor_typing_animation_enabled,
            "typing animation should persist as true after toggle back"
        );
    }

    #[test]
    fn test_smooth_cursor_toggle_persists() {
        let temp_dir = tempdir().unwrap();

        // Default is true, toggle to false
        let mut settings = LocalSettings::default();
        assert!(
            settings.editor_smooth_cursor_enabled,
            "default should be true"
        );
        settings.editor_smooth_cursor_enabled = false;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert!(
            !loaded.editor_smooth_cursor_enabled,
            "smooth cursor should persist as false after toggle"
        );

        // Toggle back to true
        let mut settings2 = loaded;
        settings2.editor_smooth_cursor_enabled = true;
        save_local_settings(temp_dir.path(), &settings2).unwrap();

        let loaded2 = load_local_settings(temp_dir.path()).unwrap();
        assert!(
            loaded2.editor_smooth_cursor_enabled,
            "smooth cursor should persist as true after toggle back"
        );
    }

    #[test]
    fn test_font_size_change_persists() {
        let temp_dir = tempdir().unwrap();

        let mut settings = LocalSettings::default();
        settings.editor_font_size = 24.0;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(
            loaded.editor_font_size, 24.0,
            "font size should persist as 24.0"
        );

        // Change again
        let mut settings2 = loaded;
        settings2.editor_font_size = 18.0;
        save_local_settings(temp_dir.path(), &settings2).unwrap();

        let loaded2 = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(
            loaded2.editor_font_size, 18.0,
            "font size should persist as 18.0 after change"
        );
    }

    #[test]
    fn test_line_spacing_change_persists() {
        let temp_dir = tempdir().unwrap();

        let mut settings = LocalSettings::default();
        settings.editor_line_spacing_multiplier = 2.0;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(
            loaded.editor_line_spacing_multiplier, 2.0,
            "line spacing should persist as 2.0"
        );

        // Change again
        let mut settings2 = loaded;
        settings2.editor_line_spacing_multiplier = 1.2;
        save_local_settings(temp_dir.path(), &settings2).unwrap();

        let loaded2 = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(
            loaded2.editor_line_spacing_multiplier, 1.2,
            "line spacing should persist as 1.2 after change"
        );
    }

    #[test]
    fn test_indent_width_change_persists() {
        let temp_dir = tempdir().unwrap();

        let mut settings = LocalSettings::default();
        settings.auto_indent_width = 4.0;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(
            loaded.auto_indent_width, 4.0,
            "indent width should persist as 4.0"
        );

        // Change again
        let mut settings2 = loaded;
        settings2.auto_indent_width = 0.0;
        save_local_settings(temp_dir.path(), &settings2).unwrap();

        let loaded2 = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(
            loaded2.auto_indent_width, 0.0,
            "indent width should persist as 0.0 after change"
        );
    }

    #[test]
    fn test_device_info_round_trip() {
        let temp_dir = tempdir().unwrap();
        let info = DeviceInfo {
            device_id: "test-uuid-123".to_string(),
            device_class: "desktop".to_string(),
            platform: "desktop".to_string(),
        };
        save_device_info(temp_dir.path(), &info).unwrap();
        let loaded = load_device_info(temp_dir.path()).unwrap();
        assert_eq!(loaded.device_id, "test-uuid-123");
        assert_eq!(loaded.device_class, "desktop");
        assert_eq!(loaded.platform, "desktop");
    }

    #[test]
    fn test_device_info_default_empty() {
        let temp_dir = tempdir().unwrap();
        let loaded = load_device_info(temp_dir.path()).unwrap();
        assert!(loaded.device_id.is_empty());
        assert!(loaded.device_class.is_empty());
        assert!(loaded.platform.is_empty());
    }

    #[test]
    fn test_ensure_device_info_creates_new() {
        let temp_dir = tempdir().unwrap();
        let info = ensure_device_info(temp_dir.path(), "desktop", "desktop", None).unwrap();
        assert!(!info.device_id.is_empty());
        assert_eq!(info.platform, "desktop");
        assert_eq!(info.device_class, "desktop");

        let info2 = ensure_device_info(temp_dir.path(), "android", "phone", None).unwrap();
        assert_eq!(
            info2.device_id, info.device_id,
            "device_id should not change"
        );
        assert_eq!(info2.platform, "desktop", "platform should not change");
        assert_eq!(
            info2.device_class, "desktop",
            "device_class should not change"
        );
    }

    #[test]
    fn test_ensure_device_info_uses_preferred_id() {
        let temp_dir = tempdir().unwrap();
        let info = ensure_device_info(
            temp_dir.path(),
            "desktop",
            "desktop",
            Some("platform-device-123"),
        )
        .unwrap();
        assert_eq!(info.device_id, "platform-device-123");
    }

    #[test]
    fn test_device_info_camel_case_serialization() {
        let temp_dir = tempdir().unwrap();
        let info = DeviceInfo {
            device_id: "uuid-456".to_string(),
            device_class: "phone".to_string(),
            platform: "android".to_string(),
        };
        save_device_info(temp_dir.path(), &info).unwrap();

        let path = temp_dir.path().join("device/current_device.json");
        let content = fs::read_to_string(&path).unwrap();
        let loaded: DeviceInfo = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.device_id, "uuid-456");
        assert_eq!(loaded.device_class, "phone");
        assert_eq!(loaded.platform, "android");
    }

    #[test]
    fn test_compute_palette_fingerprint_deterministic() {
        let light = ThemeColorScheme {
            primary: "#006493".to_string(),
            on_primary: "#FFFFFF".to_string(),
            ..ThemeColorScheme::default()
        };
        let dark = ThemeColorScheme {
            primary: "#87CEFF".to_string(),
            on_primary: "#00344D".to_string(),
            ..ThemeColorScheme::default()
        };
        let fp1 = compute_palette_fingerprint(&light, &dark);
        let fp2 = compute_palette_fingerprint(&light, &dark);
        assert_eq!(fp1, fp2, "fingerprint should be deterministic");
        assert_eq!(fp1.len(), 16, "fingerprint should be 16 hex chars");
    }

    #[test]
    fn test_compute_palette_fingerprint_differs_for_different_schemes() {
        let light1 = ThemeColorScheme {
            primary: "#006493".to_string(),
            ..ThemeColorScheme::default()
        };
        let light2 = ThemeColorScheme {
            primary: "#FF0000".to_string(),
            ..ThemeColorScheme::default()
        };
        let dark = ThemeColorScheme::default();
        let fp1 = compute_palette_fingerprint(&light1, &dark);
        let fp2 = compute_palette_fingerprint(&light2, &dark);
        assert_ne!(
            fp1, fp2,
            "different schemes should have different fingerprints"
        );
    }

    #[test]
    fn test_save_and_load_palette_record() {
        let temp_dir = tempdir().unwrap();
        let record = ThemePaletteRecord {
            schema_version: 1,
            palette_id: "test-device:abcdef1234567890".to_string(),
            palette_fingerprint: "abcdef1234567890".to_string(),
            source: "android_dynamic_color".to_string(),
            source_platform: "android".to_string(),
            source_device_id: "test-device".to_string(),
            source_device_class: "phone".to_string(),
            captured_at_ms: 1000000,
            variant: "system_selected".to_string(),
            light_scheme: ThemeColorScheme {
                primary: "#006493".to_string(),
                ..ThemeColorScheme::default()
            },
            dark_scheme: ThemeColorScheme {
                primary: "#87CEFF".to_string(),
                ..ThemeColorScheme::default()
            },
        };
        save_palette_record(temp_dir.path(), &record).unwrap();
        let loaded =
            load_palette_record(temp_dir.path(), "test-device", "abcdef1234567890").unwrap();
        assert_eq!(loaded.palette_id, record.palette_id);
        assert_eq!(loaded.light_scheme.primary, "#006493");
        assert_eq!(loaded.dark_scheme.primary, "#87CEFF");
    }

    #[test]
    fn test_save_palette_record_idempotent() {
        let temp_dir = tempdir().unwrap();
        let record = ThemePaletteRecord {
            schema_version: 1,
            palette_id: "dev:fp1".to_string(),
            palette_fingerprint: "fp1".to_string(),
            source_device_id: "dev".to_string(),
            captured_at_ms: 1000,
            ..ThemePaletteRecord::default()
        };
        save_palette_record(temp_dir.path(), &record).unwrap();
        save_palette_record(temp_dir.path(), &record).unwrap();
    }

    #[test]
    fn test_list_palette_records() {
        let temp_dir = tempdir().unwrap();
        let r1 = ThemePaletteRecord {
            schema_version: 1,
            palette_id: "dev1:fp1".to_string(),
            palette_fingerprint: "fp1".to_string(),
            source_device_id: "dev1".to_string(),
            captured_at_ms: 2000,
            ..ThemePaletteRecord::default()
        };
        let r2 = ThemePaletteRecord {
            schema_version: 1,
            palette_id: "dev2:fp2".to_string(),
            palette_fingerprint: "fp2".to_string(),
            source_device_id: "dev2".to_string(),
            captured_at_ms: 1000,
            ..ThemePaletteRecord::default()
        };
        save_palette_record(temp_dir.path(), &r1).unwrap();
        save_palette_record(temp_dir.path(), &r2).unwrap();
        let records = list_palette_records(temp_dir.path()).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[0].palette_id, "dev1:fp1",
            "should be sorted by captured_at_ms desc"
        );
        assert_eq!(records[1].palette_id, "dev2:fp2");
    }

    #[test]
    fn test_delete_palette_record() {
        let temp_dir = tempdir().unwrap();
        let record = ThemePaletteRecord {
            schema_version: 1,
            palette_id: "dev:fp1".to_string(),
            palette_fingerprint: "fp1".to_string(),
            source_device_id: "dev".to_string(),
            captured_at_ms: 1000,
            ..ThemePaletteRecord::default()
        };
        save_palette_record(temp_dir.path(), &record).unwrap();
        delete_palette_record(temp_dir.path(), "dev", "fp1").unwrap();
        let records = list_palette_records(temp_dir.path()).unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn test_legacy_palette_to_record_empty_device_id() {
        let palette = ThemePalette {
            source: "android_dynamic_color".to_string(),
            variant: "tonal_spot".to_string(),
            device_id: String::new(),
            light_primary: "#006493".to_string(),
            ..ThemePalette::default()
        };
        let record = legacy_palette_to_record(&palette);
        assert_eq!(
            record.source_device_id, "legacy",
            "empty device_id should become 'legacy'"
        );
        assert_eq!(
            record.variant, "system_selected",
            "tonal_spot from android_dynamic_color should become system_selected"
        );
    }

    #[test]
    fn test_legacy_palette_to_record_with_device_id() {
        let palette = ThemePalette {
            source: "android_dynamic_color".to_string(),
            variant: "custom".to_string(),
            device_id: "real-device-uuid".to_string(),
            light_primary: "#006493".to_string(),
            ..ThemePalette::default()
        };
        let record = legacy_palette_to_record(&palette);
        assert_eq!(record.source_device_id, "real-device-uuid");
        assert_eq!(
            record.variant, "custom",
            "non-tonal_spot variant should be preserved"
        );
    }

    #[test]
    fn test_list_builtin_themes_has_default() {
        let themes = list_builtin_themes();
        assert!(
            !themes.is_empty(),
            "should have at least one built-in theme"
        );
        assert_eq!(themes[0].theme_id, "sujian_default");
        assert!(
            !themes[0].light_scheme.primary.is_empty(),
            "light primary should not be empty"
        );
        assert!(
            !themes[0].dark_scheme.primary.is_empty(),
            "dark primary should not be empty"
        );
    }

    #[test]
    fn test_builtin_theme_complete_color_roles() {
        let themes = list_builtin_themes();
        let theme = &themes[0];
        assert!(
            !theme.light_scheme.error.is_empty(),
            "light error should be defined"
        );
        assert!(
            !theme.light_scheme.on_error.is_empty(),
            "light on_error should be defined"
        );
        assert!(
            !theme.light_scheme.inverse_surface.is_empty(),
            "light inverse_surface should be defined"
        );
        assert!(
            !theme.light_scheme.surface_tint.is_empty(),
            "light surface_tint should be defined"
        );
        assert!(
            !theme.light_scheme.scrim.is_empty(),
            "light scrim should be defined"
        );
        assert!(
            !theme.dark_scheme.error.is_empty(),
            "dark error should be defined"
        );
        assert!(
            !theme.dark_scheme.on_error.is_empty(),
            "dark on_error should be defined"
        );
    }

    #[test]
    fn test_load_local_settings_both_theme_mode_and_appearance_mode() {
        // Issue #705 评论 5716410988: 旧版本同时写 themeMode 和 appearanceMode,
        // 不能报 duplicate field,appearanceMode 优先。
        let temp_dir = tempdir().unwrap();
        let content = r#"{
            "themeMode": "dark",
            "appearanceMode": "light",
            "editorFontSize": 18.0
        }"#;
        std::fs::write(temp_dir.path().join("settings.local.json"), content).unwrap();
        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(
            loaded.appearance_mode, "light",
            "appearanceMode should win over themeMode"
        );
        assert_eq!(loaded.editor_font_size, 18.0);
        // Issue #705 评论 5716919024: 读取后重新打开文件,确认 themeMode 已被写回清除。
        // 之前 needs_writeback 只在"有 themeMode 且没有 appearanceMode"时才写回,
        // 旧文件同时含两个 key 时磁盘上 themeMode 会一直留着。
        let written =
            std::fs::read_to_string(temp_dir.path().join("settings.local.json")).unwrap();
        assert!(
            !written.contains("themeMode"),
            "themeMode + appearanceMode 同时存在时,读取后应写回清除 themeMode"
        );
        assert!(
            written.contains("appearanceMode"),
            "写回后应保留 appearanceMode"
        );
    }

    #[test]
    fn test_load_local_settings_legacy_theme_mode_only() {
        // Issue #705 评论 5716410988: 旧文件只有 themeMode,迁移到 appearance_mode。
        let temp_dir = tempdir().unwrap();
        let content = r#"{
            "themeMode": "dark",
            "editorFontSize": 20.0
        }"#;
        std::fs::write(temp_dir.path().join("settings.local.json"), content).unwrap();
        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(
            loaded.appearance_mode, "dark",
            "legacy themeMode should migrate to appearance_mode"
        );
        assert_eq!(loaded.editor_font_size, 20.0);
        // 写回后应该只有 appearanceMode
        let written =
            std::fs::read_to_string(temp_dir.path().join("settings.local.json")).unwrap();
        assert!(
            !written.contains("themeMode"),
            "writeback should not contain themeMode"
        );
        assert!(written.contains("appearanceMode"));
    }

    #[test]
    fn test_load_local_settings_neither_key() {
        // Issue #705: 两个 key 都没有,用默认 system。
        let temp_dir = tempdir().unwrap();
        let content = r#"{"editorFontSize": 14.0}"#;
        std::fs::write(temp_dir.path().join("settings.local.json"), content).unwrap();
        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(loaded.appearance_mode, "system");
    }

    #[test]
    fn test_resolve_appearance_mode_rejects_invalid_values() {
        // Issue #705 评论 5716919024: 非法 appearanceMode 应回退到下一来源或 system。
        // 之前 resolve_appearance_mode 只判断字符串非空,"Dark"/"foo" 等都会被直接采用。
        let temp_dir = tempdir().unwrap();
        let content = r#"{
            "appearanceMode": "Dark",
            "editorFontSize": 16.0
        }"#;
        std::fs::write(temp_dir.path().join("settings.local.json"), content).unwrap();
        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(
            loaded.appearance_mode, "system",
            "非法 appearanceMode 'Dark' 应回退到 system"
        );
    }

    #[test]
    fn test_resolve_appearance_mode_invalid_falls_back_to_theme_mode() {
        // Issue #705 评论 5716919024: 非法 appearanceMode 应回退到旧 themeMode(如果 themeMode 有效)。
        let temp_dir = tempdir().unwrap();
        let content = r#"{
            "appearanceMode": "foo",
            "themeMode": "dark",
            "editorFontSize": 16.0
        }"#;
        std::fs::write(temp_dir.path().join("settings.local.json"), content).unwrap();
        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(
            loaded.appearance_mode, "dark",
            "非法 appearanceMode 'foo' 应回退到有效 themeMode 'dark'"
        );
    }

    #[test]
    fn test_validate_normalizes_invalid_appearance_mode() {
        // Issue #705 评论 5716919024: validate 应把非法 appearance_mode 归一成 system。
        let mut settings = LocalSettings::default();
        settings.appearance_mode = "foo".to_string();
        settings.validate();
        assert_eq!(
            settings.appearance_mode, "system",
            "validate 应把非法 appearance_mode 归一成 system"
        );
        // 合法值不受影响
        settings.appearance_mode = "dark".to_string();
        settings.validate();
        assert_eq!(settings.appearance_mode, "dark");
    }

    // --- Issue #709 评论 5728916561: palette 颜色值验证和修正测试 ---

    #[test]
    fn test_sanitize_dark_scheme_black_on_surface_with_dark_surface() {
        let mut scheme = ThemeColorScheme {
            surface: "#0F1417".to_string(),
            on_surface: "#000000".to_string(),
            surface_variant: "#41484D".to_string(),
            on_surface_variant: "#000000".to_string(),
            background: "#0F1417".to_string(),
            on_background: "#000000".to_string(),
            ..Default::default()
        };
        sanitize_color_scheme(&mut scheme, true);
        assert_ne!(
            scheme.on_surface, "#000000",
            "dark on_surface=#000000 应被修正"
        );
        assert_ne!(
            scheme.on_surface, "",
            "dark on_surface 不应为空"
        );
        assert_ne!(
            scheme.on_surface_variant, "#000000",
            "dark on_surface_variant=#000000 应被修正"
        );
        assert_ne!(
            scheme.on_background, "#000000",
            "dark on_background=#000000 应被修正"
        );
    }

    #[test]
    fn test_sanitize_dark_scheme_empty_on_surface_with_dark_surface() {
        let mut scheme = ThemeColorScheme {
            surface: "#0F1417".to_string(),
            on_surface: String::new(),
            surface_variant: "#41484D".to_string(),
            on_surface_variant: String::new(),
            background: "#0F1417".to_string(),
            on_background: String::new(),
            ..Default::default()
        };
        sanitize_color_scheme(&mut scheme, true);
        assert!(
            !scheme.on_surface.is_empty(),
            "dark 空 on_surface 应被修正"
        );
        assert!(
            !scheme.on_surface_variant.is_empty(),
            "dark 空 on_surface_variant 应被修正"
        );
        assert!(
            !scheme.on_background.is_empty(),
            "dark 空 on_background 应被修正"
        );
    }

    #[test]
    fn test_sanitize_dark_scheme_valid_colors_not_modified() {
        let mut scheme = ThemeColorScheme {
            surface: "#0F1417".to_string(),
            on_surface: "#DFE3E7".to_string(),
            surface_variant: "#41484D".to_string(),
            on_surface_variant: "#C1C7CE".to_string(),
            background: "#0F1417".to_string(),
            on_background: "#DFE3E7".to_string(),
            ..Default::default()
        };
        let original = scheme.clone();
        sanitize_color_scheme(&mut scheme, true);
        assert_eq!(
            scheme.on_surface, original.on_surface,
            "有效的 dark on_surface 不应被修改"
        );
        assert_eq!(
            scheme.on_surface_variant, original.on_surface_variant,
            "有效的 dark on_surface_variant 不应被修改"
        );
        assert_eq!(
            scheme.on_background, original.on_background,
            "有效的 dark on_background 不应被修改"
        );
    }

    #[test]
    fn test_sanitize_light_scheme_white_on_surface_with_light_surface() {
        let mut scheme = ThemeColorScheme {
            surface: "#F6FAFE".to_string(),
            on_surface: "#FFFFFF".to_string(),
            surface_variant: "#DDE3EA".to_string(),
            on_surface_variant: "#FFFFFF".to_string(),
            background: "#F6FAFE".to_string(),
            on_background: "#FFFFFF".to_string(),
            ..Default::default()
        };
        sanitize_color_scheme(&mut scheme, false);
        assert_ne!(
            scheme.on_surface, "#FFFFFF",
            "light on_surface=#FFFFFF 配浅色 surface 应被修正"
        );
        assert_ne!(
            scheme.on_surface_variant, "#FFFFFF",
            "light on_surface_variant=#FFFFFF 应被修正"
        );
        assert_ne!(
            scheme.on_background, "#FFFFFF",
            "light on_background=#FFFFFF 应被修正"
        );
    }

    #[test]
    fn test_sanitize_light_scheme_valid_colors_not_modified() {
        let mut scheme = ThemeColorScheme {
            surface: "#F6FAFE".to_string(),
            on_surface: "#171C1F".to_string(),
            surface_variant: "#DDE3EA".to_string(),
            on_surface_variant: "#41484D".to_string(),
            background: "#F6FAFE".to_string(),
            on_background: "#171C1F".to_string(),
            ..Default::default()
        };
        let original = scheme.clone();
        sanitize_color_scheme(&mut scheme, false);
        assert_eq!(
            scheme.on_surface, original.on_surface,
            "有效的 light on_surface 不应被修改"
        );
    }

    #[test]
    fn test_load_palette_record_sanitizes_dark_scheme() {
        let temp_dir = tempdir().unwrap();
        let base = palettes_base_dir(temp_dir.path());
        std::fs::create_dir_all(base.join("device1")).unwrap();
        let record = ThemePaletteRecord {
            schema_version: 1,
            palette_id: "device1:abc".to_string(),
            palette_fingerprint: "abc".to_string(),
            source: "android_dynamic_color".to_string(),
            source_platform: String::new(),
            source_device_id: "device1".to_string(),
            source_device_class: String::new(),
            captured_at_ms: 0,
            variant: String::new(),
            light_scheme: ThemeColorScheme {
                surface: "#F6FAFE".to_string(),
                on_surface: "#171C1F".to_string(),
                ..Default::default()
            },
            dark_scheme: ThemeColorScheme {
                surface: "#0F1417".to_string(),
                on_surface: "#000000".to_string(), // 问题数据：黑字配深色背景
                ..Default::default()
            },
        };
        let content = serde_json::to_string_pretty(&record).unwrap();
        std::fs::write(base.join("device1").join("abc.json"), content).unwrap();
        let loaded = load_palette_record(temp_dir.path(), "device1", "abc").unwrap();
        assert_ne!(
            loaded.dark_scheme.on_surface, "#000000",
            "load_palette_record 应修正 dark on_surface=#000000"
        );
        assert!(
            !loaded.dark_scheme.on_surface.is_empty(),
            "修正后的 on_surface 不应为空"
        );
    }

    #[test]
    fn test_legacy_palette_to_record_sanitizes_empty_dark_fields() {
        let palette = ThemePalette {
            source: "android_dynamic_color".to_string(),
            device_id: "device1".to_string(),
            light_primary: "#006493".to_string(),
            light_surface: "#F6FAFE".to_string(),
            light_on_surface: "#171C1F".to_string(),
            dark_primary: "#87CEFF".to_string(),
            dark_surface: "#0F1417".to_string(),
            // dark_on_surface 故意留空 — legacy data 常见情况
            ..ThemePalette::default()
        };
        let record = legacy_palette_to_record(&palette);
        assert!(
            !record.dark_scheme.on_surface.is_empty(),
            "legacy migration 应修正空的 dark on_surface"
        );
        assert_ne!(
            record.dark_scheme.on_surface, "#000000",
            "legacy migration 不应产生 #000000"
        );
    }
}

#[cfg(test)]
mod tests;
