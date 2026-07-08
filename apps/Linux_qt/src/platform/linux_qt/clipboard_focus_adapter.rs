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
        // 实际的 QClipboard 操作通过 FFI 回调到 C++ 侧执行
        match request {
            ClipboardRequest::Copy { text: _ } => ClipboardResult::Copied,
            ClipboardRequest::Paste => ClipboardResult::Pasted {
                text: String::new(),
            },
            ClipboardRequest::Cut { text: _ } => ClipboardResult::Cut,
            ClipboardRequest::HasText => ClipboardResult::HasText { has_text: false },
        }
    }

    fn execute_focus(&mut self, request: FocusRequest) {
        match request {
            FocusRequest::RequestFocus => {
                self.focus_state.has_focus = true;
            }
            FocusRequest::ReleaseFocus => {
                self.focus_state.has_focus = false;
            }
            FocusRequest::RequestSoftInput | FocusRequest::HideSoftInput => {}
        }
    }

    fn focus_state(&self) -> FocusState {
        self.focus_state
    }

    fn show_context_menu(&mut self, _request: ContextMenuRequest) {
        // QMenu 显示通过 FFI 回调
    }

    fn hide_context_menu(&mut self) {
        // QMenu 隐藏通过 FFI 回调
    }
}
