//! Linux Qt CursorAnchorAdapter 实现
//!
//! QInputMethod::update 和 InputMethodQuery 收敛到此。
//! SujianEditorItem 不直接调用 QInputMethod，只通过此适配器。

use writer_core::platform_interaction::cursor_anchor::{
    CursorAnchorAdapter, CursorAnchorRequest, CursorAnchorUpdateReason,
    NormalizedCursorRect,
};

/// Linux Qt CursorAnchorAdapter 实现
///
/// 实际的 QInputMethod::update 调用通过 FFI 回调到 C++ 侧执行。
/// Rust 侧只负责准备请求参数和触发回调。
pub struct LinuxQtCursorAnchorAdapter {
    item_ptr: *mut std::ffi::c_void,
}

impl LinuxQtCursorAnchorAdapter {
    pub fn new(item_ptr: *mut std::ffi::c_void) -> Self {
        Self { item_ptr }
    }

    pub fn set_item_ptr(&mut self, ptr: *mut std::ffi::c_void) {
        self.item_ptr = ptr;
    }
}

unsafe impl Send for LinuxQtCursorAnchorAdapter {}
unsafe impl Sync for LinuxQtCursorAnchorAdapter {}

impl CursorAnchorAdapter for LinuxQtCursorAnchorAdapter {
    fn notify_cursor_anchor_update(
        &self,
        _request: &CursorAnchorRequest,
        _reason: CursorAnchorUpdateReason,
    ) {
        if self.item_ptr.is_null() {
            return;
        }
        // 实际的 QInputMethod::update 调用通过 C++ FFI 执行
        // cpp! 宏调用在 SujianEditorItem 的 Rust 代码中完成
        // 此处只提供接口边界，具体实现委托给 SujianEditorItem
    }

    fn request_candidate_window_update(&self, _cursor_rect: &NormalizedCursorRect) {
        if self.item_ptr.is_null() {
            return;
        }
        // 委托给 QInputMethod::update(Qt::ImCursorRectangle)
    }

    fn is_input_method_visible(&self) -> bool {
        // QInputMethod::isVisible() 不可靠，不使用
        false
    }
}
