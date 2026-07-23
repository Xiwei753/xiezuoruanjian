//! # 设置管理（Core 层）
//!
//! 管理两类设置：
//!
//! 1. **LocalSettings（本地设置）**：仅存储在本地，不同步
//!    - 窗口大小、自动保存开关、字号、行距、自动缩进、动画开关等
//!    - 文件路径：`app-meta/settings/settings.local.json`
//!
//! 2. **SyncableSettings（可同步设置）**：会随工作区同步到其他设备
//!    - 字号、主题模式、Monet 颜色
//!    - 文件路径：`app-meta/settings/settings.sync.json`
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
    #[serde(default)]
    pub theme_mode: Option<String>,
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
        default = "default_linux_qt_sidebar_width",
        alias = "desktop_sidebar_width",
        alias = "linux_sidebar_width"
    )]
    pub linux_qt_sidebar_width: f64,
    #[serde(
        default = "default_linux_qt_editor_width",
        alias = "desktop_editor_width"
    )]
    pub linux_qt_editor_width: f64,
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

fn default_linux_qt_sidebar_width() -> f64 {
    240.0
}

fn default_linux_qt_editor_width() -> f64 {
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
            theme_mode: Some("system".to_string()),
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
            editor_coordinated_text_cursor_animation_enabled: default_editor_coordinated_text_cursor_animation_enabled(),
            ai_enabled: false,
            stats_device_id: None,
            linux_qt_sidebar_width: default_linux_qt_sidebar_width(),
            linux_qt_editor_width: default_linux_qt_editor_width(),
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
}

/// Immutable theme palette record stored in the palette catalog.
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

pub fn load_local_settings(workspace_path: &Path) -> Result<LocalSettings> {
    let path = workspace_path.join("app-meta/settings/settings.local.json");
    if !path.exists() {
        return Ok(LocalSettings::default());
    }
    let content = fs::read_to_string(&path)?;
    let mut settings: LocalSettings = serde_json::from_str(&content)?;
    settings.validate();
    Ok(settings)
}

pub fn save_local_settings(workspace_path: &Path, settings: &LocalSettings) -> Result<()> {
    let path = workspace_path.join("app-meta/settings/settings.local.json");
    let content = serde_json::to_string_pretty(settings)?;
    crate::storage::atomic_write_string(&path, &content)
}

pub fn load_syncable_settings(workspace_path: &Path) -> Result<SyncableSettings> {
    let path = workspace_path.join("app-meta/settings/settings.sync.json");
    if !path.exists() {
        return Ok(SyncableSettings::default());
    }
    let content = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&content)?)
}

pub fn save_syncable_settings(workspace_path: &Path, settings: &SyncableSettings) -> Result<()> {
    let path = workspace_path.join("app-meta/settings/settings.sync.json");
    let content = serde_json::to_string_pretty(settings)?;
    crate::storage::atomic_write_string(&path, &content)
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

pub fn load_device_info(workspace_path: &Path) -> Result<DeviceInfo> {
    let path = workspace_path.join("app-meta/device/current_device.json");
    if !path.exists() {
        return Ok(DeviceInfo::default());
    }
    let content = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&content)?)
}

pub fn save_device_info(workspace_path: &Path, info: &DeviceInfo) -> Result<()> {
    let path = workspace_path.join("app-meta/device/current_device.json");
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
    workspace_path: &Path,
    platform: &str,
    device_class: &str,
    preferred_device_id: Option<&str>,
) -> Result<DeviceInfo> {
    let mut info = load_device_info(workspace_path).unwrap_or_default();
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
        save_device_info(workspace_path, &info)?;
    }
    Ok(info)
}

/// Returns the effective editor font size.
/// Primary source: SyncableSettings.font_size
/// Fallback: LocalSettings.editor_font_size (when syncable <= 0)
/// Final default: 16.0
pub fn get_effective_font_size(workspace_path: &Path) -> f64 {
    let syncable = load_syncable_settings(workspace_path);
    if let Ok(s) = syncable {
        if s.font_size > 0.0 {
            return s.font_size;
        }
    }
    let local = load_local_settings(workspace_path);
    if let Ok(s) = local {
        if s.editor_font_size > 0.0 {
            return f64::from(s.editor_font_size);
        }
    }
    16.0
}

/// Sets the editor font size in SyncableSettings.
/// Does NOT modify LocalSettings.editor_font_size (preserved for backward compatibility).
pub fn set_editor_font_size(workspace_path: &Path, font_size: f64) -> Result<()> {
    let mut syncable = load_syncable_settings(workspace_path).unwrap_or_default();
    syncable.font_size = font_size;
    save_syncable_settings(workspace_path, &syncable)
}

// ── Palette catalog operations ──

/// Base directory for palette catalog.
fn palettes_base_dir(workspace_path: &Path) -> std::path::PathBuf {
    workspace_path.join("app-meta/themes/palettes")
}

/// Compute a stable fingerprint for a pair of color schemes.
/// Uses SHA-256 on the normalized JSON of light + dark schemes.
pub fn compute_palette_fingerprint(light: &ThemeColorScheme, dark: &ThemeColorScheme) -> String {
    use sha2::Digest;
    use std::fmt::Write;
    let mut hasher = sha2::Sha256::new();
    sha2::Digest::update(&mut hasher, serde_json::to_string(light).unwrap_or_default().as_bytes());
    sha2::Digest::update(&mut hasher, serde_json::to_string(dark).unwrap_or_default().as_bytes());
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
pub fn save_palette_record(workspace_path: &Path, record: &ThemePaletteRecord) -> Result<()> {
    let dir = palettes_base_dir(workspace_path)
        .join(&record.source_device_id);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", record.palette_fingerprint));
    if path.exists() {
        return Ok(());
    }
    let content = serde_json::to_string_pretty(record)?;
    crate::storage::atomic_write_string(&path, &content)
}

/// Load a specific palette record by device_id and fingerprint.
pub fn load_palette_record(
    workspace_path: &Path,
    device_id: &str,
    fingerprint: &str,
) -> Result<ThemePaletteRecord> {
    let path = palettes_base_dir(workspace_path)
        .join(device_id)
        .join(format!("{}.json", fingerprint));
    let content = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&content)?)
}

/// List all palette records in the catalog.
/// Scans `app-meta/themes/palettes/<device_id>/<fingerprint>.json` recursively.
pub fn list_palette_records(workspace_path: &Path) -> Result<Vec<ThemePaletteRecord>> {
    let base = palettes_base_dir(workspace_path);
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
pub fn delete_palette_record(
    workspace_path: &Path,
    device_id: &str,
    fingerprint: &str,
) -> Result<()> {
    let path = palettes_base_dir(workspace_path)
        .join(device_id)
        .join(format!("{}.json", fingerprint));
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Convert legacy ThemePalette to ThemePaletteRecord for migration.
/// Legacy ThemePalette has flat light_/dark_ prefixed fields;
/// this converts them into the new ThemeColorScheme structure.
#[allow(deprecated)]
pub fn legacy_palette_to_record(palette: &ThemePalette) -> ThemePaletteRecord {
    let light = ThemeColorScheme {
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
    };
    let dark = ThemeColorScheme {
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
    };
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
pub fn migrate_legacy_theme_palette(workspace_path: &Path) -> Result<bool> {
    let syncable = load_syncable_settings(workspace_path)?;
    if syncable.theme_palette.source.is_empty() && syncable.theme_palette.light_primary.is_empty() {
        return Ok(false);
    }
    let record = legacy_palette_to_record(&syncable.theme_palette);
    save_palette_record(workspace_path, &record)?;
    let mut local = load_local_settings(workspace_path)?;
    let need_palette_update = local.selected_palette_id.is_empty();
    if need_palette_update {
        local.selected_palette_id = record.palette_id.clone();
        local.color_source = "saved_palette".to_string();
    }
    let need_appearance_update = local.appearance_mode == "system" && syncable.theme_mode != "system" && !syncable.theme_mode.is_empty();
    if need_appearance_update {
        local.appearance_mode = syncable.theme_mode.clone();
    }
    if need_palette_update || need_appearance_update {
        save_local_settings(workspace_path, &local)?;
    }
    Ok(true)
}

pub struct BuiltinTheme {
    pub theme_id: &'static str,
    pub name: &'static str,
    pub light_scheme: ThemeColorScheme,
    pub dark_scheme: ThemeColorScheme,
}

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
        },
    }]
}

#[cfg(test)]
mod tests {
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
        assert!(loaded.auto_indent_enabled, "auto_indent_enabled should persist as true");

        let mut settings2 = loaded;
        settings2.auto_indent_enabled = false;
        save_local_settings(temp_dir.path(), &settings2).unwrap();

        let loaded2 = load_local_settings(temp_dir.path()).unwrap();
        assert!(!loaded2.auto_indent_enabled, "auto_indent_enabled should persist as false after change");
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
        assert!(settings.diagnostics_enabled, "diagnostics_enabled should default to true (alpha)");
    }

    #[test]
    fn diagnostics_verbose_default_true() {
        let settings = LocalSettings::default();
        assert!(settings.diagnostics_verbose, "diagnostics_verbose should default to true (alpha)");
    }

    #[test]
    fn diagnostics_persist_and_load() {
        let temp_dir = tempdir().unwrap();
        let mut settings = LocalSettings::default();
        settings.diagnostics_enabled = true;
        settings.diagnostics_verbose = true;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert!(loaded.diagnostics_enabled, "diagnostics_enabled should persist as true");
        assert!(loaded.diagnostics_verbose, "diagnostics_verbose should persist as true");

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
        assert!(!loaded.diagnostics_enabled, "diagnostics_enabled=false should persist correctly");
    }

    #[test]
    fn test_diagnostics_verbose_false_persists() {
        let temp_dir = tempdir().unwrap();
        let mut settings = LocalSettings::default();
        settings.diagnostics_verbose = false;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert!(!loaded.diagnostics_verbose, "diagnostics_verbose=false should persist correctly");
    }

    #[test]
    fn test_typing_animation_toggle_persists() {
        let temp_dir = tempdir().unwrap();

        // Default is true, toggle to false
        let mut settings = LocalSettings::default();
        assert!(settings.editor_typing_animation_enabled, "default should be true");
        settings.editor_typing_animation_enabled = false;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert!(!loaded.editor_typing_animation_enabled, "typing animation should persist as false after toggle");

        // Toggle back to true
        let mut settings2 = loaded;
        settings2.editor_typing_animation_enabled = true;
        save_local_settings(temp_dir.path(), &settings2).unwrap();

        let loaded2 = load_local_settings(temp_dir.path()).unwrap();
        assert!(loaded2.editor_typing_animation_enabled, "typing animation should persist as true after toggle back");
    }

    #[test]
    fn test_smooth_cursor_toggle_persists() {
        let temp_dir = tempdir().unwrap();

        // Default is true, toggle to false
        let mut settings = LocalSettings::default();
        assert!(settings.editor_smooth_cursor_enabled, "default should be true");
        settings.editor_smooth_cursor_enabled = false;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert!(!loaded.editor_smooth_cursor_enabled, "smooth cursor should persist as false after toggle");

        // Toggle back to true
        let mut settings2 = loaded;
        settings2.editor_smooth_cursor_enabled = true;
        save_local_settings(temp_dir.path(), &settings2).unwrap();

        let loaded2 = load_local_settings(temp_dir.path()).unwrap();
        assert!(loaded2.editor_smooth_cursor_enabled, "smooth cursor should persist as true after toggle back");
    }

    #[test]
    fn test_font_size_change_persists() {
        let temp_dir = tempdir().unwrap();

        let mut settings = LocalSettings::default();
        settings.editor_font_size = 24.0;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(loaded.editor_font_size, 24.0, "font size should persist as 24.0");

        // Change again
        let mut settings2 = loaded;
        settings2.editor_font_size = 18.0;
        save_local_settings(temp_dir.path(), &settings2).unwrap();

        let loaded2 = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(loaded2.editor_font_size, 18.0, "font size should persist as 18.0 after change");
    }

    #[test]
    fn test_line_spacing_change_persists() {
        let temp_dir = tempdir().unwrap();

        let mut settings = LocalSettings::default();
        settings.editor_line_spacing_multiplier = 2.0;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(loaded.editor_line_spacing_multiplier, 2.0, "line spacing should persist as 2.0");

        // Change again
        let mut settings2 = loaded;
        settings2.editor_line_spacing_multiplier = 1.2;
        save_local_settings(temp_dir.path(), &settings2).unwrap();

        let loaded2 = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(loaded2.editor_line_spacing_multiplier, 1.2, "line spacing should persist as 1.2 after change");
    }

    #[test]
    fn test_indent_width_change_persists() {
        let temp_dir = tempdir().unwrap();

        let mut settings = LocalSettings::default();
        settings.auto_indent_width = 4.0;
        save_local_settings(temp_dir.path(), &settings).unwrap();

        let loaded = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(loaded.auto_indent_width, 4.0, "indent width should persist as 4.0");

        // Change again
        let mut settings2 = loaded;
        settings2.auto_indent_width = 0.0;
        save_local_settings(temp_dir.path(), &settings2).unwrap();

        let loaded2 = load_local_settings(temp_dir.path()).unwrap();
        assert_eq!(loaded2.auto_indent_width, 0.0, "indent width should persist as 0.0 after change");
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
        assert_eq!(info2.device_id, info.device_id, "device_id should not change");
        assert_eq!(info2.platform, "desktop", "platform should not change");
        assert_eq!(info2.device_class, "desktop", "device_class should not change");
    }

    #[test]
    fn test_ensure_device_info_uses_preferred_id() {
        let temp_dir = tempdir().unwrap();
        let info = ensure_device_info(temp_dir.path(), "desktop", "desktop", Some("platform-device-123")).unwrap();
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

        let path = temp_dir.path().join("app-meta/device/current_device.json");
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
        assert_ne!(fp1, fp2, "different schemes should have different fingerprints");
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
        let loaded = load_palette_record(temp_dir.path(), "test-device", "abcdef1234567890").unwrap();
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
        assert_eq!(records[0].palette_id, "dev1:fp1", "should be sorted by captured_at_ms desc");
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
        assert_eq!(record.source_device_id, "legacy", "empty device_id should become 'legacy'");
        assert_eq!(record.variant, "system_selected", "tonal_spot from android_dynamic_color should become system_selected");
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
        assert_eq!(record.variant, "custom", "non-tonal_spot variant should be preserved");
    }

    #[test]
    fn test_list_builtin_themes_has_default() {
        let themes = list_builtin_themes();
        assert!(!themes.is_empty(), "should have at least one built-in theme");
        assert_eq!(themes[0].theme_id, "sujian_default");
        assert!(!themes[0].light_scheme.primary.is_empty(), "light primary should not be empty");
        assert!(!themes[0].dark_scheme.primary.is_empty(), "dark primary should not be empty");
    }

    #[test]
    fn test_builtin_theme_complete_color_roles() {
        let themes = list_builtin_themes();
        let theme = &themes[0];
        assert!(!theme.light_scheme.error.is_empty(), "light error should be defined");
        assert!(!theme.light_scheme.on_error.is_empty(), "light on_error should be defined");
        assert!(!theme.light_scheme.inverse_surface.is_empty(), "light inverse_surface should be defined");
        assert!(!theme.light_scheme.surface_tint.is_empty(), "light surface_tint should be defined");
        assert!(!theme.light_scheme.scrim.is_empty(), "light scrim should be defined");
        assert!(!theme.dark_scheme.error.is_empty(), "dark error should be defined");
        assert!(!theme.dark_scheme.on_error.is_empty(), "dark on_error should be defined");
    }
}
