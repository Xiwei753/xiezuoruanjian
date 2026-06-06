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
    #[serde(default = "default_desktop_sidebar_width", alias = "linux_sidebar_width")]
    pub desktop_sidebar_width: f64,
    #[serde(default = "default_desktop_editor_width")]
    pub desktop_editor_width: f64,
}

fn default_desktop_sidebar_width() -> f64 {
    240.0
}

fn default_desktop_editor_width() -> f64 {
    0.0
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
    false
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
    Ok(serde_json::from_str(&content)?)
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
}
