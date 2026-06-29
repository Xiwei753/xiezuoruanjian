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
    #[serde(default)]
    pub ai_enabled: bool,
    #[serde(default)]
    pub stats_device_id: Option<String>,
    #[serde(
        default = "default_desktop_sidebar_width",
        alias = "linux_sidebar_width"
    )]
    pub desktop_sidebar_width: f64,
    #[serde(default = "default_desktop_editor_width")]
    pub desktop_editor_width: f64,
    #[serde(default = "default_diagnostics_enabled")]
    pub diagnostics_enabled: bool,
    #[serde(default = "default_diagnostics_verbose")]
    pub diagnostics_verbose: bool,
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
            ai_enabled: false,
            stats_device_id: None,
            desktop_sidebar_width: default_desktop_sidebar_width(),
            desktop_editor_width: default_desktop_editor_width(),
            diagnostics_enabled: default_diagnostics_enabled(),
            diagnostics_verbose: default_diagnostics_verbose(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncableSettings {
    #[serde(default)]
    pub font_size: f64,
    #[serde(default)]
    pub theme_mode: String,
    #[serde(default)]
    pub monet_color: String,
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
            return s.editor_font_size as f64;
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
}
