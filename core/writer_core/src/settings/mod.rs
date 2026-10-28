//! # 设置管理（Core 层）
//!
//! 管理两类设置：
//!
//! 1. **LocalSettings（本地设置）**：仅存储在本地，不同步
//!    - 自动保存开关、字号、行距、自动缩进、动画开关等
//!    - 文件路径：`<app_data_root>/settings.local.json`
//!
//! 2. **SyncableSettings（可同步设置）**：跨设备同步候选
//!    - 字号、旧版主题模式（仅兼容读取）
//!    - 文件路径：`<app_data_root>/settings.sync.json`（应用级，不随作品仓库同步）
//!    - Android Monet 设备配色只保存在 Android 本地，不进入此文件
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
            editor_typing_animation_enabled: default_editor_typing_animation_enabled(),
            editor_smooth_cursor_enabled: default_editor_smooth_cursor_enabled(),
            editor_typing_animation_duration_ms: default_editor_typing_animation_duration_ms(),
            editor_smooth_cursor_duration_ms: default_editor_smooth_cursor_duration_ms(),
            editor_coordinated_text_cursor_animation_enabled:
                default_editor_coordinated_text_cursor_animation_enabled(),
            ai_enabled: false,
            stats_device_id: None,
            diagnostics_enabled: default_diagnostics_enabled(),
            diagnostics_verbose: default_diagnostics_verbose(),
        }
    }
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
    // 旧版 settings.sync.json 中的 monetColor / themePalette 字段不再建模。
    // serde 默认忽略未知字段，读取不会报错；保存时自然不会再写出。
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

/// 旧版 `settings.sync.json` 中携带 Android 设备配色的字段。
///
/// Issue #1005：这些字段不再属于可同步设置。读取或同步旧文件时必须从实际
/// 落盘内容中移除，否则原始字节仍会被 Git/近场同步原样传播。
const LEGACY_DEVICE_COLOR_KEYS: &[&str] = &[
    "monetColor",
    "monet_color",
    "themePalette",
    "theme_palette",
    "themePaletteJson",
    "theme_palette_json",
];

/// 规范化可同步设置 JSON 内容。
///
/// 只有检测到旧设备配色字段（或旧 snake_case 字段别名）时才返回重写后的
/// canonical 字节；普通新格式文件返回 `None`，避免同步时产生无意义哈希变化。
pub fn normalize_syncable_settings_content(content: &[u8]) -> Result<Option<Vec<u8>>> {
    let mut value: serde_json::Value = serde_json::from_slice(content)?;
    let Some(object) = value.as_object_mut() else {
        return Ok(None);
    };

    let mut changed = false;
    for key in LEGACY_DEVICE_COLOR_KEYS {
        if object.remove(*key).is_some() {
            changed = true;
        }
    }
    if let Some(font_size) = object.remove("font_size") {
        object.entry("fontSize".to_string()).or_insert(font_size);
        changed = true;
    }
    if let Some(theme_mode) = object.remove("theme_mode") {
        object.entry("themeMode".to_string()).or_insert(theme_mode);
        changed = true;
    }

    if !changed {
        return Ok(None);
    }

    // 只校验当前已知字段；未知字段必须原样保留，避免旧客户端文件中的
    // 其他同步设置在新版本读取后被静默丢弃。
    let _: SyncableSettings = serde_json::from_value(value.clone())?;
    Ok(Some(serde_json::to_vec_pretty(&value)?))
}

/// 规范化 `settings.sync.json` 文件，必要时原子重写并保留原修改时间。
///
/// 保留 mtime 是为了避免“升级清理旧字段”被 LWW 误判成一次用户编辑，
/// 让旧文件在同步冲突中错误地压过其他设备的真实更新。
pub fn normalize_syncable_settings_file(path: &Path) -> Result<Option<Vec<u8>>> {
    let content = match fs::read(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let Some(normalized) = normalize_syncable_settings_content(&content)? else {
        return Ok(None);
    };

    let original_modified = fs::metadata(path).and_then(|m| m.modified()).ok();
    crate::storage::atomic_write_bytes(path, &normalized)?;
    if let Some(modified) = original_modified {
        match fs::File::options().write(true).open(path) {
            Ok(file) => {
                if let Err(err) = file.set_times(fs::FileTimes::new().set_modified(modified)) {
                    log::warn!(
                        "failed to restore mtime for normalized settings {}: {}",
                        path.display(),
                        err
                    );
                }
            }
            Err(err) => {
                log::warn!(
                    "failed to open normalized settings {} to restore mtime: {}",
                    path.display(),
                    err
                );
            }
        }
    }
    Ok(Some(normalized))
}

pub fn load_syncable_settings(config_dir: &Path) -> Result<SyncableSettings> {
    let path = config_dir.join("settings.sync.json");
    if !path.exists() {
        return Ok(SyncableSettings::default());
    }
    if let Some(normalized) = normalize_syncable_settings_file(&path)? {
        return Ok(serde_json::from_slice(&normalized)?);
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

/// 平台初始化确认运行平台后调用：保留已有 `device_id`，纠正旧版本误写的
/// `platform` 及由错误平台派生的 `device_class`。
///
/// 与 [`ensure_device_info`] 不同，本函数把平台注入视为权威事实：持久化平台
/// 与当前平台不一致时覆盖平台；平台变化时同步替换设备类别，避免早期写死的
/// `desktop` 继续污染打字、计时和同步证明事件。平台未变化时保留已有设备类别，
/// 让平台端上报的 phone/tablet 不被默认值覆盖。
pub fn reconcile_platform_identity(
    config_dir: &Path,
    platform: &str,
    device_class: &str,
    preferred_device_id: Option<&str>,
) -> Result<DeviceInfo> {
    if platform.is_empty() {
        return ensure_device_info(config_dir, platform, device_class, preferred_device_id);
    }
    let mut info = load_device_info(config_dir).unwrap_or_default();
    let mut changed = false;
    if info.device_id.is_empty() {
        info.device_id = preferred_device_id
            .filter(|id| !id.is_empty())
            .map(|id| id.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        changed = true;
    }
    if info.platform != platform {
        info.platform = platform.to_string();
        info.device_class = device_class.to_string();
        changed = true;
    } else if info.device_class.is_empty() && !device_class.is_empty() {
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
    fn test_reconcile_platform_identity_corrects_legacy_desktop() {
        let temp_dir = tempdir().unwrap();
        let legacy = DeviceInfo {
            device_id: "device-keep-me".to_string(),
            device_class: "desktop".to_string(),
            platform: "desktop".to_string(),
        };
        save_device_info(temp_dir.path(), &legacy).unwrap();

        let info = reconcile_platform_identity(temp_dir.path(), "harmony", "tablet", None).unwrap();
        assert_eq!(info.device_id, "device-keep-me");
        assert_eq!(info.platform, "harmony");
        assert_eq!(info.device_class, "tablet");

        let loaded = load_device_info(temp_dir.path()).unwrap();
        assert_eq!(loaded.device_id, "device-keep-me");
        assert_eq!(loaded.platform, "harmony");
        assert_eq!(loaded.device_class, "tablet");
    }

    #[test]
    fn test_reconcile_platform_identity_keeps_confirmed_device_class() {
        let temp_dir = tempdir().unwrap();
        let confirmed = DeviceInfo {
            device_id: "device-1".to_string(),
            device_class: "phone".to_string(),
            platform: "harmony".to_string(),
        };
        save_device_info(temp_dir.path(), &confirmed).unwrap();

        let info = reconcile_platform_identity(temp_dir.path(), "harmony", "tablet", None).unwrap();
        assert_eq!(info.device_id, "device-1");
        assert_eq!(info.platform, "harmony");
        assert_eq!(info.device_class, "phone");
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
        let written = std::fs::read_to_string(temp_dir.path().join("settings.local.json")).unwrap();
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
        let written = std::fs::read_to_string(temp_dir.path().join("settings.local.json")).unwrap();
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
}

#[cfg(test)]
mod tests;
