//! PlatformCapabilities — 平台能力报告
//!
//! 启动时报告平台能力，设置页和前端按钮按 capabilities 显示/禁用。
//! 鸿蒙未实现动画就禁用动画，Android 不支持 replacement commit 就不暴露。

use serde::{Deserialize, Serialize};

/// 平台能力集合 — 启动时由平台适配层一次性报告
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    /// 是否支持 IME preedit（组合输入）
    pub supports_ime_preedit: bool,
    /// 是否支持光标锚点更新（候选框跟随光标）
    pub supports_cursor_anchor: bool,
    /// 是否支持 replacement commit（fcitx5 拼音修正）
    pub supports_replacement_commit: bool,
    /// 是否支持文本动画（吞吐/删除/光标动画）
    pub supports_text_animation: bool,
    /// 是否支持平滑光标移动动画
    pub supports_smooth_cursor: bool,
    /// 是否支持 reflow 动画（行级位移）
    pub supports_reflow_animation: bool,
    /// 是否支持系统剪贴板
    pub supports_clipboard: bool,
    /// 是否支持上下文菜单（右键菜单）
    pub supports_context_menu: bool,
}

/// 预定义的平台能力配置
impl PlatformCapabilities {
    /// Linux Qt (fcitx5/ibus/Wayland) 完整能力
    pub fn linux_qt() -> Self {
        Self {
            supports_ime_preedit: true,
            supports_cursor_anchor: true,
            supports_replacement_commit: true,
            supports_text_animation: true,
            supports_smooth_cursor: true,
            supports_reflow_animation: true,
            supports_clipboard: true,
            supports_context_menu: true,
        }
    }

    /// Android 完整能力
    pub fn android() -> Self {
        Self {
            supports_ime_preedit: true,
            supports_cursor_anchor: true,
            supports_replacement_commit: false,
            supports_text_animation: true,
            supports_smooth_cursor: true,
            supports_reflow_animation: true,
            supports_clipboard: true,
            supports_context_menu: true,
        }
    }

    /// Windows 完整能力
    pub fn windows() -> Self {
        Self {
            supports_ime_preedit: true,
            supports_cursor_anchor: true,
            supports_replacement_commit: false,
            supports_text_animation: true,
            supports_smooth_cursor: true,
            supports_reflow_animation: true,
            supports_clipboard: true,
            supports_context_menu: true,
        }
    }

    /// Harmony — 动画未实现，能力受限
    pub fn harmony() -> Self {
        Self {
            supports_ime_preedit: false,
            supports_cursor_anchor: false,
            supports_replacement_commit: false,
            supports_text_animation: false,
            supports_smooth_cursor: false,
            supports_reflow_animation: false,
            supports_clipboard: true,
            supports_context_menu: true,
        }
    }

    /// 未知/最小能力集
    pub fn minimal() -> Self {
        Self {
            supports_ime_preedit: false,
            supports_cursor_anchor: false,
            supports_replacement_commit: false,
            supports_text_animation: false,
            supports_smooth_cursor: false,
            supports_reflow_animation: false,
            supports_clipboard: false,
            supports_context_menu: false,
        }
    }

    /// 检查是否支持任何动画能力
    pub fn has_any_animation_support(&self) -> bool {
        self.supports_text_animation
            || self.supports_smooth_cursor
            || self.supports_reflow_animation
    }

    /// 检查是否支持任何 IME 能力
    pub fn has_any_ime_support(&self) -> bool {
        self.supports_ime_preedit
            || self.supports_cursor_anchor
            || self.supports_replacement_commit
    }
}

/// 平台标识 — 用于选择能力配置
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlatformKind {
    LinuxQt,
    Android,
    Windows,
    Harmony,
    Unknown,
}

impl PlatformKind {
    /// 获取该平台的默认能力配置
    pub fn default_capabilities(&self) -> PlatformCapabilities {
        match self {
            Self::LinuxQt => PlatformCapabilities::linux_qt(),
            Self::Android => PlatformCapabilities::android(),
            Self::Windows => PlatformCapabilities::windows(),
            Self::Harmony => PlatformCapabilities::harmony(),
            Self::Unknown => PlatformCapabilities::minimal(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_qt_has_full_capabilities() {
        let caps = PlatformCapabilities::linux_qt();
        assert!(caps.supports_ime_preedit);
        assert!(caps.supports_cursor_anchor);
        assert!(caps.supports_replacement_commit);
        assert!(caps.supports_text_animation);
        assert!(caps.supports_smooth_cursor);
        assert!(caps.supports_reflow_animation);
        assert!(caps.supports_clipboard);
        assert!(caps.supports_context_menu);
    }

    #[test]
    fn harmony_has_no_animation() {
        let caps = PlatformCapabilities::harmony();
        assert!(!caps.supports_text_animation);
        assert!(!caps.supports_smooth_cursor);
        assert!(!caps.supports_reflow_animation);
        assert!(!caps.supports_ime_preedit);
        assert!(caps.supports_clipboard);
        assert!(caps.supports_context_menu);
    }

    #[test]
    fn android_no_replacement_commit() {
        let caps = PlatformCapabilities::android();
        assert!(!caps.supports_replacement_commit);
        assert!(caps.supports_ime_preedit);
    }

    #[test]
    fn platform_kind_default_capabilities() {
        assert!(PlatformKind::LinuxQt.default_capabilities().supports_replacement_commit);
        assert!(!PlatformKind::Android.default_capabilities().supports_replacement_commit);
        assert!(!PlatformKind::Harmony.default_capabilities().supports_text_animation);
        assert!(!PlatformKind::Unknown.default_capabilities().has_any_animation_support());
    }

    #[test]
    fn has_any_animation_support() {
        assert!(PlatformCapabilities::linux_qt().has_any_animation_support());
        assert!(!PlatformCapabilities::harmony().has_any_animation_support());
        assert!(!PlatformCapabilities::minimal().has_any_animation_support());
    }

    #[test]
    fn capabilities_serialize_camel_case() {
        let caps = PlatformCapabilities::linux_qt();
        let json = serde_json::to_string(&caps).unwrap();
        assert!(json.contains("\"supportsImePreedit\":"));
        assert!(json.contains("\"supportsCursorAnchor\":"));
        assert!(json.contains("\"supportsReplacementCommit\":"));
        assert!(json.contains("\"supportsTextAnimation\":"));
        assert!(json.contains("\"supportsSmoothCursor\":"));
        assert!(json.contains("\"supportsReflowAnimation\":"));
        assert!(json.contains("\"supportsClipboard\":"));
        assert!(json.contains("\"supportsContextMenu\":"));
    }
}
