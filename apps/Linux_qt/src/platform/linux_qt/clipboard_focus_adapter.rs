//! Linux Qt ClipboardAndFocusAdapter 实现
//!
//! QClipboard / QMenu / forceActiveFocus / 软键盘收敛到此。

use writer_core::platform_interaction::clipboard_focus::{
    ClipboardAndFocusAdapter, ClipboardRequest, ClipboardResult,
    ContextMenuRequest, FocusRequest, FocusState,
};

/// Linux Qt ClipboardAndFocusAdapter 实现
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
        match request {
            ClipboardRequest::Copy { text: _ } => ClipboardResult::Unavailable,
            ClipboardRequest::Paste => ClipboardResult::Unavailable,
            ClipboardRequest::Cut { text: _ } => ClipboardResult::Unavailable,
            ClipboardRequest::HasText => ClipboardResult::HasText { has_text: false },
        }
    }

    fn execute_focus(&mut self, _request: FocusRequest) {
        // 未真实接入 QQuickItem::forceActiveFocus，暂不执行
    }

    fn focus_state(&self) -> FocusState {
        self.focus_state
    }

    fn show_context_menu(&mut self, _request: ContextMenuRequest) {
        // 未真实接入 QMenu，暂不执行
    }

    fn hide_context_menu(&mut self) {
        // 未真实接入 QMenu，暂不执行
    }
}
