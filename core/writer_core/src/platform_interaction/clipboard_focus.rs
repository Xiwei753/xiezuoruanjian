//! ClipboardAndFocusAdapter — 剪贴板、右键菜单、焦点、软键盘统一入口
//!
//! 编辑器只通过此接口访问剪贴板、请求焦点、控制软键盘，
//! 不直接调用 QClipboard、ClipboardManager、InputMethodManager 等。

use serde::{Deserialize, Serialize};

/// 剪贴板操作请求
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipboardRequest {
    Copy { text: String },
    Paste,
    Cut { text: String },
    HasText,
}

/// 剪贴板操作结果
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipboardResult {
    Copied,
    Pasted { text: String },
    Cut,
    HasText { has_text: bool },
    Unavailable,
    Error { message: String },
}

/// 焦点请求
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FocusRequest {
    RequestFocus,
    ReleaseFocus,
    RequestSoftInput,
    HideSoftInput,
}

/// 焦点状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusState {
    pub has_focus: bool,
    pub soft_input_visible: bool,
}

/// 上下文菜单请求
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextMenuRequest {
    pub screen_x: f64,
    pub screen_y: f64,
    pub has_selection: bool,
    pub can_paste: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

/// ClipboardAndFocusAdapter trait — 平台适配层实现
///
/// 平台端实现此 trait，统一处理：
/// - 剪贴板读写（QClipboard / ClipboardManager / WinRT DataPackage）
/// - 焦点管理（QQuickItem::forceActiveFocus / View.requestFocus / UIElement.Focus）
/// - 软键盘控制（InputMethodManager.showSoftInput / hideSoftInput）
/// - 右键菜单（QMenu / PopupMenu / MenuFlyout）
pub trait ClipboardAndFocusAdapter {
    /// 执行剪贴板操作
    fn execute_clipboard(&mut self, request: ClipboardRequest) -> ClipboardResult;

    /// 执行焦点请求
    fn execute_focus(&mut self, request: FocusRequest);

    /// 获取当前焦点状态
    fn focus_state(&self) -> FocusState;

    /// 显示上下文菜单
    fn show_context_menu(&mut self, request: ContextMenuRequest);

    /// 隐藏上下文菜单
    fn hide_context_menu(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_request_serializes_camel_case() {
        let req = ClipboardRequest::Copy {
            text: "你好".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"copy\":"));
        assert!(json.contains("\"text\":\"你好\""));
    }

    #[test]
    fn clipboard_result_pasted() {
        let result = ClipboardResult::Pasted {
            text: "世界".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"pasted\":"));
        assert!(json.contains("\"世界\""));
    }

    #[test]
    fn test_clipboard_result_serialization_exhaustive() {
        let res_err = ClipboardResult::Error { message: "err".into() };
        assert!(serde_json::to_string(&res_err).unwrap().contains("\"error\""));

        let res_unavail = ClipboardResult::Unavailable;
        assert_eq!(serde_json::to_string(&res_unavail).unwrap(), "\"unavailable\"");

        let res_cut = ClipboardResult::Cut;
        assert_eq!(serde_json::to_string(&res_cut).unwrap(), "\"cut\"");

        let res_has = ClipboardResult::HasText { has_text: true };
        assert!(serde_json::to_string(&res_has).unwrap().contains("\"hasText\""));
    }

    #[test]
    fn test_focus_request_serialization() {
        assert_eq!(serde_json::to_string(&FocusRequest::RequestFocus).unwrap(), "\"requestFocus\"");
        assert_eq!(serde_json::to_string(&FocusRequest::ReleaseFocus).unwrap(), "\"releaseFocus\"");
        assert_eq!(serde_json::to_string(&FocusRequest::RequestSoftInput).unwrap(), "\"requestSoftInput\"");
        assert_eq!(serde_json::to_string(&FocusRequest::HideSoftInput).unwrap(), "\"hideSoftInput\"");
    }

    #[test]
    fn focus_state_default() {
        let state = FocusState {
            has_focus: false,
            soft_input_visible: false,
        };
        assert!(!state.has_focus);
        assert!(!state.soft_input_visible);
    }

    #[test]
    fn context_menu_request_roundtrip() {
        let req = ContextMenuRequest {
            screen_x: 100.0,
            screen_y: 200.0,
            has_selection: true,
            can_paste: true,
            can_undo: false,
            can_redo: false,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: ContextMenuRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, parsed);
    }
}
