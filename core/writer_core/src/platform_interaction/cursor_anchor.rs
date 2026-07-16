//! CursorAnchorAdapter — 光标/锚点/候选框定位
//!
//! 编辑器只通过此接口请求/报告光标和选区信息，不直接调用
//! QInputMethod::update、IMM.updateSelection、WinUI/TSF 等。

use serde::{Deserialize, Serialize};

/// 光标/锚点信息请求 — 编辑器 → 平台适配层
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorAnchorRequest {
    /// 光标位置（UTF-8 byte offset）
    pub cursor_index: usize,
    /// 选区锚点（UTF-8 byte offset），无选区时等于 cursor_index
    pub anchor_index: usize,
    /// 光标矩形（文档坐标系）
    pub cursor_rect: Option<NormalizedCursorRect>,
    /// 选区范围（UTF-8 byte offset）
    pub selection_start: usize,
    pub selection_end: usize,
    /// 光标前文本
    pub text_before_cursor: String,
    /// 光标后文本
    pub text_after_cursor: String,
}

/// 归一化光标矩形 — 跨平台统一坐标
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedCursorRect {
    pub x: f64,
    pub top: f64,
    pub bottom: f64,
    pub baseline_y: f64,
}

/// 光标锚点更新原因
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CursorAnchorUpdateReason {
    CursorMoved,
    SelectionChanged,
    PreeditChanged,
    ScrollChanged,
    LayoutChanged,
}

/// CursorAnchorAdapter trait — 平台适配层实现
///
/// 平台端实现此 trait，将编辑器的光标/选区信息通知系统输入法服务。
/// - Linux Qt: QInputMethod::update + InputMethodQuery
/// - Android: IMM.updateSelection/updateCursorAnchorInfo
/// - Windows: WinUI/TSF/Composition 候选框定位
/// - Harmony: 暂不实现
pub trait CursorAnchorAdapter {
    /// 通知系统输入法光标/选区已更新
    fn notify_cursor_anchor_update(
        &self,
        request: &CursorAnchorRequest,
        reason: CursorAnchorUpdateReason,
    );

    /// 请求系统更新候选框位置
    fn request_candidate_window_update(&self, cursor_rect: &NormalizedCursorRect);

    /// 查询系统输入法是否可见（用于判断是否接受按键）
    fn is_input_method_visible(&self) -> bool {
        false
    }

    /// 通知系统滚动已发生，需要更新光标位置
    fn notify_scroll_changed(&self, request: &CursorAnchorRequest) {
        self.notify_cursor_anchor_update(request, CursorAnchorUpdateReason::ScrollChanged);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_anchor_request_serializes_camel_case() {
        let req = CursorAnchorRequest {
            cursor_index: 6,
            anchor_index: 6,
            cursor_rect: Some(NormalizedCursorRect {
                x: 100.0,
                top: 50.0,
                bottom: 70.0,
                baseline_y: 65.0,
            }),
            selection_start: 6,
            selection_end: 6,
            text_before_cursor: "你好".to_string(),
            text_after_cursor: "世界".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"cursorIndex\":"));
        assert!(json.contains("\"anchorIndex\":"));
        assert!(json.contains("\"cursorRect\":"));
        assert!(json.contains("\"textBeforeCursor\":"));
        assert!(json.contains("\"textAfterCursor\":"));
    }

    #[test]
    fn update_reason_serializes_camel_case() {
        let reason = CursorAnchorUpdateReason::PreeditChanged;
        let json = serde_json::to_string(&reason).unwrap();
        assert!(json.contains("\"preeditChanged\""));
    }
}
