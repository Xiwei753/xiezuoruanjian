//! Linux Qt 平台本地桌面布局设置。
//!
//! 背景（Issue #920）：窗口几何与侧栏 / 编辑器宽度属于**平台 UI 布局状态**，
//! Core 不再承载它们。旧 Core 业务设置里的 `window_width` / `window_height` /
//! `desktop_sidebar_width` / `desktop_editor_width` 已删除，改为由本模块
//! 在 Qt 平台层持久化，与 `writer_platform_linux` 的 XDG 目录约定一致。
//!
//! 迁移策略：旧值已无法通过（已删除的）Core API 读回，因此首次升级时
//! 使用 [`DesktopSettings::default`] 的合理默认值；之后由本模块独立读写。
//! 其他设置（字号、行距、主题、同步、动画偏好等）仍由 Core 负责，不在此列。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

/// 平台本地桌面设置文件名，落在 `xdg_config_dir()`（`~/.config/writer`）下。
const DESKTOP_SETTINGS_FILE: &str = "linux_qt_desktop.json";

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

fn read_from_disk() -> Option<DesktopSettings> {
    let bytes = std::fs::read(settings_path()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_to_disk(settings: &DesktopSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    // 先写临时文件再原子重命名，避免中途失败留下半个 JSON。
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(())
}

/// 读取当前平台本地桌面设置。
///
/// 首次调用从磁盘加载并缓存；此后直接返回缓存。缺省或解析失败时返回默认值。
pub fn load() -> DesktopSettings {
    let mut guard = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .get_or_insert_with(|| read_from_disk().unwrap_or_default())
        .clone()
}

/// 就地把当前设置交给 `f` 修改，然后持久化到磁盘。
///
/// 失败只返回错误信息，不影响进程内缓存（下次仍可重试写盘）。
pub fn update(f: impl FnOnce(&mut DesktopSettings)) -> Result<(), String> {
    let updated = {
        let mut guard = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
        let settings = guard.get_or_insert_with(|| read_from_disk().unwrap_or_default());
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
}
