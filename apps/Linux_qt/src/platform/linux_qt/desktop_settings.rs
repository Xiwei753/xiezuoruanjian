//! Linux Qt 平台本地桌面布局设置。
//!
//! 背景（Issue #920）：窗口几何与侧栏 / 编辑器宽度属于**平台 UI 布局状态**，
//! Core 不再承载它们。旧 Core 业务设置里的 `window_width` / `window_height` /
//! `desktop_sidebar_width` / `desktop_editor_width` 已删除，改为由本模块
//! 在 Qt 平台层持久化，与 `writer_platform_linux` 的 XDG 目录约定一致。
//!
//! 迁移策略：新文件 `linux_qt_desktop.json` 不存在时，读取**用户工作区数据根目录**
//! 下旧 Core 的 `settings.local.json` 原始 JSON（不经过已删除字段的 Core DTO），
//! 把其中的窗口 / 侧栏 / 编辑器宽度迁移过来并立刻写入新文件；缺失的字段才使用
//! [`DesktopSettings::default`]。迁移只发生一次，此后始终以本平台文件为准
//! （Core 重写 `settings.local.json` 丢弃未知字段也不会影响 Qt UI 状态）。
//!
//! 注意：旧 Core `settings.local.json` 位于 `app_data_root`（用户选择的数据根目录，
//! 见 `facade/settings_ops.rs` 与 `workspace_backend.rs::internal_open_data_root`），
//! 不一定等于 `xdg_config_dir()`。因此迁移入口必须显式传入真实数据根目录，
//! 否则自定义数据目录的用户无法被迁移。
//! 其他设置（字号、行距、主题、同步、动画偏好等）仍由 Core 负责，不在此列。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 平台本地桌面设置文件名，落在 `xdg_config_dir()`（`~/.config/writer`）下。
const DESKTOP_SETTINGS_FILE: &str = "linux_qt_desktop.json";

/// 旧 Core 本地设置文件名。位于用户工作区数据根目录（`app_data_root`）下，
/// **不是** XDG 配置目录。
const LEGACY_CORE_SETTINGS_FILE: &str = "settings.local.json";

/// Qt 桌面端本地布局设置。所有长度单位都是 Qt 设备无关逻辑像素（vp）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DesktopSettings {
    /// 上次窗口宽度（vp）。0 表示未记录，由 QML 按屏幕可用几何决定。
    pub window_width: f64,
    /// 上次窗口高度（vp）。0 表示未记录。
    pub window_height: f64,
    /// 章节导航 / 列表侧栏宽度（vp）。
    pub desktop_sidebar_width: f64,
    /// 编辑器正文列宽度（vp）。0 表示不额外限宽。
    pub desktop_editor_width: f64,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            // 与 main.qml 的初始窗口尺寸默认值一致。
            window_width: 1100.0,
            window_height: 768.0,
            // 与旧 Core LayoutMetrics.list_pane_width_dp 一致的合理默认。
            desktop_sidebar_width: 240.0,
            desktop_editor_width: 0.0,
        }
    }
}

/// 进程内缓存。首次访问从磁盘加载，加载失败或文件缺失时用默认值。
static CURRENT: Mutex<Option<DesktopSettings>> = Mutex::new(None);

fn settings_path() -> PathBuf {
    writer_platform_linux::xdg_config_dir().join(DESKTOP_SETTINGS_FILE)
}

fn legacy_settings_path(data_root: &Path) -> PathBuf {
    data_root.join(LEGACY_CORE_SETTINGS_FILE)
}

/// 从旧 Core `settings.local.json` 的原始 JSON 迁移布局字段。
///
/// 取值顺序按旧字段的 serde 主键与 alias：`camelCase` 主键优先，其后是历史
/// snake_case / `linux_qt_*` / `linux_*` 兼容键。缺失或非数值的字段沿用
/// [`DesktopSettings::default`]，不因为个别键缺失而整份回退。
fn desktop_settings_from_legacy(value: &Value) -> DesktopSettings {
    let defaults = DesktopSettings::default();
    DesktopSettings {
        window_width: legacy_number(value, &["windowWidth", "window_width"])
            .unwrap_or(defaults.window_width),
        window_height: legacy_number(value, &["windowHeight", "window_height"])
            .unwrap_or(defaults.window_height),
        desktop_sidebar_width: legacy_number(
            value,
            &[
                "desktopSidebarWidth",
                "linux_qt_sidebar_width",
                "desktop_sidebar_width",
                "linux_sidebar_width",
            ],
        )
        .unwrap_or(defaults.desktop_sidebar_width),
        desktop_editor_width: legacy_number(
            value,
            &[
                "desktopEditorWidth",
                "linux_qt_editor_width",
                "desktop_editor_width",
            ],
        )
        .unwrap_or(defaults.desktop_editor_width),
    }
}

/// 依次尝试多个键，返回第一个存在的数值。
fn legacy_number(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_f64))
}

/// 新文件不存在（或不可解析）时，从 `data_root` 下旧 Core 设置迁移一次并写盘。
///
/// `settings_path` 显式传入，便于测试用隔离的临时目录验证「数据根目录 ≠ 平台配置目录」。
fn migrate_from_legacy_settings_at(
    settings_path: &Path,
    data_root: &Path,
) -> Option<DesktopSettings> {
    let bytes = std::fs::read(legacy_settings_path(data_root)).ok()?;
    let legacy: Value = serde_json::from_slice(&bytes).ok()?;
    let migrated = desktop_settings_from_legacy(&legacy);
    // 迁移后立刻落盘：此后即使 Core 重写 settings.local.json 丢弃这些未知字段，
    // Qt 的窗口 / 栏宽也不会丢失。
    let _ = write_to_disk_at(settings_path, &migrated);
    Some(migrated)
}

fn read_from_disk_at(settings_path: &Path, data_root: &Path) -> Option<DesktopSettings> {
    if let Ok(bytes) = std::fs::read(settings_path) {
        if let Ok(settings) = serde_json::from_slice(&bytes) {
            return Some(settings);
        }
    }
    migrate_from_legacy_settings_at(settings_path, data_root)
}

fn read_from_disk(data_root: &Path) -> Option<DesktopSettings> {
    read_from_disk_at(&settings_path(), data_root)
}

fn write_to_disk(settings: &DesktopSettings) -> Result<(), String> {
    write_to_disk_at(&settings_path(), settings)
}

fn write_to_disk_at(path: &Path, settings: &DesktopSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    // 先写临时文件再原子重命名，避免中途失败留下半个 JSON。
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

/// 读取当前平台本地桌面设置。
///
/// `data_root` 是用户工作区数据根目录，仅用于首次迁移旧 Core `settings.local.json`
/// （见模块文档）。首次成功读取后缓存；若既没有新文件也没有可迁移的旧文件，则返回
/// 默认值但**不缓存**，以便之后拿到真实数据根时仍能完成迁移。
pub fn load(data_root: &Path) -> DesktopSettings {
    let mut guard = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cached) = guard.as_ref() {
        return cached.clone();
    }
    match read_from_disk(data_root) {
        Some(settings) => {
            *guard = Some(settings.clone());
            settings
        }
        None => DesktopSettings::default(),
    }
}

/// 就地把当前设置交给 `f` 修改，然后持久化到磁盘。
///
/// 失败只返回错误信息，不影响进程内缓存（下次仍可重试写盘）。
pub fn update(data_root: &Path, f: impl FnOnce(&mut DesktopSettings)) -> Result<(), String> {
    let updated = {
        let mut guard = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            if let Some(settings) = read_from_disk(data_root) {
                *guard = Some(settings);
            }
        }
        let settings = guard.get_or_insert_with(DesktopSettings::default);
        f(settings);
        settings.clone()
    };
    write_to_disk(&updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_desktop_settings_are_sane() {
        let d = DesktopSettings::default();
        assert_eq!(d.window_width, 1100.0);
        assert_eq!(d.window_height, 768.0);
        assert_eq!(d.desktop_sidebar_width, 240.0);
        assert_eq!(d.desktop_editor_width, 0.0);
    }

    #[test]
    fn desktop_settings_roundtrip_camel_case() {
        let d = DesktopSettings {
            window_width: 1280.0,
            window_height: 800.0,
            desktop_sidebar_width: 320.0,
            desktop_editor_width: 640.0,
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"windowWidth\""));
        assert!(json.contains("\"desktopSidebarWidth\""));
        let back: DesktopSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn legacy_camel_case_keys_migrate() {
        let legacy = serde_json::json!({
            "windowWidth": 1280.0,
            "windowHeight": 800.0,
            "desktopSidebarWidth": 320.0,
            "desktopEditorWidth": 640.0,
        });
        assert_eq!(
            desktop_settings_from_legacy(&legacy),
            DesktopSettings {
                window_width: 1280.0,
                window_height: 800.0,
                desktop_sidebar_width: 320.0,
                desktop_editor_width: 640.0,
            }
        );
    }

    #[test]
    fn legacy_old_alias_keys_migrate() {
        let legacy = serde_json::json!({
            "linux_qt_sidebar_width": 300.0,
            "linux_qt_editor_width": 520.0,
        });
        let migrated = desktop_settings_from_legacy(&legacy);
        assert_eq!(migrated.desktop_sidebar_width, 300.0);
        assert_eq!(migrated.desktop_editor_width, 520.0);
        // 窗口尺寸缺失时回退默认值。
        assert_eq!(
            migrated.window_width,
            DesktopSettings::default().window_width
        );
        assert_eq!(
            migrated.window_height,
            DesktopSettings::default().window_height
        );
    }

    #[test]
    fn legacy_snake_case_keys_migrate() {
        let legacy = serde_json::json!({
            "window_width": 1440.0,
            "window_height": 900.0,
            "desktop_sidebar_width": 260.0,
            "desktop_editor_width": 700.0,
        });
        assert_eq!(
            desktop_settings_from_legacy(&legacy),
            DesktopSettings {
                window_width: 1440.0,
                window_height: 900.0,
                desktop_sidebar_width: 260.0,
                desktop_editor_width: 700.0,
            }
        );
    }

    #[test]
    fn legacy_camel_case_takes_precedence_over_aliases() {
        let legacy = serde_json::json!({
            "desktopSidebarWidth": 320.0,
            "linux_qt_sidebar_width": 200.0,
        });
        assert_eq!(
            desktop_settings_from_legacy(&legacy).desktop_sidebar_width,
            320.0
        );
    }

    #[test]
    fn legacy_missing_or_invalid_fields_fall_back_to_defaults() {
        let defaults = DesktopSettings::default();
        assert_eq!(
            desktop_settings_from_legacy(&serde_json::json!({})),
            defaults
        );
        let invalid = serde_json::json!({ "windowWidth": "wide", "desktopSidebarWidth": null });
        assert_eq!(desktop_settings_from_legacy(&invalid), defaults);
    }

    fn unique_temp_dir(suffix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "sujian-desktop-settings-{}-{}-{suffix}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    /// 真实文件迁移：旧 `settings.local.json` 只存在于用户数据根目录，
    /// 与新平台配置目录（`linux_qt_desktop.json`）不同，仍应按值迁移并落盘。
    #[test]
    fn migrates_legacy_file_from_data_root_into_platform_config_dir() {
        let base = unique_temp_dir("migrate");
        let config_dir = base.join("xdg-config-writer");
        let data_root = base.join("user-chosen-workspace");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_root).unwrap();
        let new_path = config_dir.join(DESKTOP_SETTINGS_FILE);

        std::fs::write(
            legacy_settings_path(&data_root),
            br#"{"windowWidth":1280.0,"windowHeight":800.0,"desktopSidebarWidth":360.0,"linux_qt_editor_width":520.0}"#,
        )
        .unwrap();

        assert!(!new_path.exists());
        let migrated = read_from_disk_at(&new_path, &data_root).expect("legacy migration");
        assert_eq!(migrated.window_width, 1280.0);
        assert_eq!(migrated.window_height, 800.0);
        assert_eq!(migrated.desktop_sidebar_width, 360.0);
        assert_eq!(migrated.desktop_editor_width, 520.0);
        // 迁移后立刻写入平台配置目录。
        assert!(new_path.exists());

        // 此后以平台文件为准：旧文件被删除也不影响。
        std::fs::remove_file(legacy_settings_path(&data_root)).unwrap();
        let reread = read_from_disk_at(&new_path, &data_root).expect("platform file");
        assert_eq!(reread, migrated);

        let _ = std::fs::remove_dir_all(&base);
    }

    /// 平台文件已存在时，数据根目录下的旧文件不再覆盖它。
    #[test]
    fn platform_file_wins_over_legacy_data_root_file() {
        let base = unique_temp_dir("precedence");
        let config_dir = base.join("xdg-config-writer");
        let data_root = base.join("workspace");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_root).unwrap();
        let new_path = config_dir.join(DESKTOP_SETTINGS_FILE);

        let existing = DesktopSettings {
            desktop_sidebar_width: 300.0,
            ..DesktopSettings::default()
        };
        write_to_disk_at(&new_path, &existing).unwrap();
        std::fs::write(
            legacy_settings_path(&data_root),
            br#"{"desktopSidebarWidth":999.0}"#,
        )
        .unwrap();

        let loaded = read_from_disk_at(&new_path, &data_root).unwrap();
        assert_eq!(loaded.desktop_sidebar_width, 300.0);

        let _ = std::fs::remove_dir_all(&base);
    }
}
