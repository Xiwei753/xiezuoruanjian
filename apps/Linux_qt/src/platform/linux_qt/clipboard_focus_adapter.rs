//! Linux Qt ClipboardAndFocusAdapter 实现
//!
//! QClipboard / QMenu / forceActiveFocus / 软键盘收敛到此。
//!
//! TODO(平台交互收口): 当前剪贴板操作在 SujianEditorItem::clipboard_copy/paste() 中
//! 通过 cpp! 宏直接调用 QGuiApplication::clipboard()，不走此适配器。
//! 焦点操作在 input::focus_item() 中通过 cpp! 宏直接调用 forceActiveFocus()。
//! 迁移计划：将 cpp! 剪贴板/焦点调用收敛到此适配器，SujianEditorItem 只通过适配器操作。

use writer_core::platform_interaction::clipboard_focus::{
    ClipboardAndFocusAdapter, ClipboardRequest, ClipboardResult,
    ContextMenuRequest, FocusRequest, FocusState,
};

/// Linux Qt ClipboardAndFocusAdapter 实现
///
/// 当前状态：所有操作返回 Unavailable。实际剪贴板/焦点操作绕过此适配器，
/// 直接在 SujianEditorItem 和 input 模块中通过 cpp! 宏完成。
pub struct LinuxQtClipboardFocusAdapter {
    focus_state: FocusState,
}

impl LinuxQtClipboardFocusAdapter {
    pub fn new() -> Self {
        Self {
            focus_state: FocusState {
                has_focus: false,
                soft_input_visible: false,
            },
        }
    }
}

impl Default for LinuxQtClipboardFocusAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardAndFocusAdapter for LinuxQtClipboardFocusAdapter {
    fn execute_clipboard(&mut self, request: ClipboardRequest) -> ClipboardResult {
        // TODO(平台交互收口): 接入 QGuiApplication::clipboard()
        match request {
            ClipboardRequest::Copy { text: _ } => ClipboardResult::Unavailable,
            ClipboardRequest::Paste => ClipboardResult::Unavailable,
            ClipboardRequest::Cut { text: _ } => ClipboardResult::Unavailable,
            ClipboardRequest::HasText => ClipboardResult::HasText { has_text: false },
        }
    }

    fn execute_focus(&mut self, _request: FocusRequest) {
        // TODO(平台交互收口): 接入 QQuickItem::forceActiveFocus / QInputMethod::show/hide
    }

    fn focus_state(&self) -> FocusState {
        self.focus_state
    }

    fn show_context_menu(&mut self, _request: ContextMenuRequest) {
        // TODO(平台交互收口): 接入 QML context menu
    }

    fn hide_context_menu(&mut self) {
        // TODO(平台交互收口): 接入 QML context menu
    }
}
