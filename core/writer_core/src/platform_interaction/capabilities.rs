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
    /// Linux Qt 当前能力
    ///
    /// 已真实接入的能力：
    /// - IME preedit: SujianEventFilter → FFI → EditorInputController 完整链路
    /// - replacement commit: fcitx5 拼音修正走 sujian_ime_replace_and_commit
    /// - cursor anchor: IME query 已迁移到 Rust FFI 数据源（sujian_get_ime_query_data）
    /// - text animation: Core visual transaction → QML EditorAnimationOverlay
    /// - smooth cursor: QML Rectangle cursor + cursor_rect_changed signal
    /// - reflow animation: Core reflow visual transaction → overlay
    /// - clipboard: LinuxQtClipboardFocusAdapter → QClipboard
    /// - context menu: LinuxQtClipboardFocusAdapter → QML context_menu_requested signal
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

    /// Android 当前能力
    ///
    /// 已真实接入的能力：
    /// - IME preedit: SujianInputConnection → EditorInputController
    /// - cursor anchor: CursorAnchorInfo → EditorView
    /// - text animation: Core visual transaction → SujianEditorView animation layer
    /// - smooth cursor: SujianEditorView cursor layer
    /// - reflow animation: Core reflow visual transaction → animation layer
    /// - clipboard: Android ClipboardManager
    /// - context menu: Android context menu
    pub fn android() -> Self {
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

    /// Windows 当前能力
    ///
    /// 已真实接入的能力：
    /// - IME preedit: CoreTextEditContext composition/commit
    /// - cursor anchor: CoreTextEditContext + candidate window anchoring
    /// - clipboard: Windows.ApplicationModel.DataTransfer.Clipboard
    ///
    /// 未真实接入 / 使用本地独立实现：
    /// - replacement commit: CoreTextEditContext 未实现 replacement range commit
    /// - text animation: SujianAnimationOverlay 未接入 Core visual transaction
    /// - smooth cursor: cursor blink 仅有闪烁，无平滑移动动画
    /// - reflow animation: 未接入 Core reflow visual transaction
    /// - context menu: WinUI 3 context menu 未通过适配器接入
    /// - IEditorTransactionBoundary: LocalStandaloneTransactionBoundary（UsesCoreEngine == false）
    pub fn windows() -> Self {
        Self {
            supports_ime_preedit: true,
            supports_cursor_anchor: true,
            supports_replacement_commit: false,
            supports_text_animation: false,
            supports_smooth_cursor: false,
            supports_reflow_animation: false,
            supports_clipboard: true,
            supports_context_menu: false,
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
    fn linux_qt_capabilities_reflect_real_ime() {
        let caps = PlatformCapabilities::linux_qt();
        assert!(caps.supports_ime_preedit);
        assert!(caps.supports_replacement_commit);
        assert!(caps.supports_text_animation);
        assert!(caps.supports_smooth_cursor);
        assert!(caps.supports_reflow_animation);
        assert!(caps.supports_clipboard);
        assert!(caps.supports_context_menu);
        assert!(caps.supports_cursor_anchor);
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
    fn android_capabilities_reflect_real_ime() {
        let caps = PlatformCapabilities::android();
        assert!(caps.supports_ime_preedit);
        assert!(caps.supports_cursor_anchor);
        assert!(caps.supports_replacement_commit);
        assert!(caps.has_any_animation_support());
    }

    #[test]
    fn windows_capabilities_reflect_real_ime() {
        let caps = PlatformCapabilities::windows();
        assert!(caps.supports_ime_preedit);
        assert!(caps.supports_cursor_anchor);
        assert!(!caps.supports_replacement_commit);
        assert!(!caps.has_any_animation_support());
        assert!(!caps.supports_context_menu);
    }

    #[test]
    fn platform_kind_default_capabilities() {
        assert!(PlatformKind::LinuxQt.default_capabilities().supports_cursor_anchor);
        assert!(PlatformKind::Android.default_capabilities().supports_replacement_commit);
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
