//! Linux Qt PlatformCapabilities 实现
//!
//! 上报 Linux Qt 已真实接入的能力。能力定义必须与
//! core/writer_core/src/platform_interaction/capabilities.rs 的 linux_qt() 工厂方法一致。
//! 未真实接入的能力必须为 false，不允许吹牛。

use writer_core::platform_interaction::capabilities::PlatformCapabilities;

/// Linux Qt 平台能力适配器
///
/// 当前真实能力（与 Core linux_qt() 工厂方法对齐）：
/// - IME preedit: SujianEventFilter → FFI → EditorInputController 完整链路
/// - replacement commit: fcitx5 拼音修正走 sujian_ime_replace_and_commit
/// - text animation / smooth cursor / reflow animation: Core visual transaction → QML overlay
/// - clipboard: SujianEditorItem::clipboard_copy/paste via QClipboard
/// - context menu: QML context menu
///
/// 未真实接入：
/// - cursor_anchor: CursorAnchorAdapter 为空桩，IME query 直接读 QML property
pub struct LinuxQtCapabilitiesAdapter {
    capabilities: PlatformCapabilities,
}

impl LinuxQtCapabilitiesAdapter {
    pub fn new() -> Self {
        Self {
            capabilities: PlatformCapabilities::linux_qt(),
        }
    }

    pub fn capabilities(&self) -> &PlatformCapabilities {
        &self.capabilities
    }
}

impl Default for LinuxQtCapabilitiesAdapter {
    fn default() -> Self {
        Self::new()
    }
}
