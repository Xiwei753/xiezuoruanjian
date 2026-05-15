pub mod models;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

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
    #[serde(default = "default_editor_fullscreen_portrait_enabled")]
    pub editor_fullscreen_portrait_enabled: bool,
    #[serde(default)]
    pub window_width: f64,
    #[serde(default)]
    pub window_height: f64,
}

fn default_editor_fullscreen_portrait_enabled() -> bool {
    false
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
            editor_fullscreen_portrait_enabled: default_editor_fullscreen_portrait_enabled(),
            window_width: 800.0,
            window_height: 600.0,
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
