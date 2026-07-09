//! Linux Qt CursorAnchorAdapter 实现
//!
//! QInputMethod::update 和 InputMethodQuery 收敛到此。
//! SujianEditorItem 不直接调用 QInputMethod，只通过此适配器。
//!
//! TODO(平台交互收口): 当前 IME query (handle_input_method_query) 在 qt_surface.rs C++ 侧
//! 直接读取 QML property (cursor_rect_x/y, plain_text, cursor_position 等)。
//! 最终目标：IME query 所需的 cursor rect、surrounding text、selection、anchor
//! 全部由 CursorAnchorAdapter 数据生成，C++ 侧只负责 QInputMethodQueryEvent 协议翻译。
//! 迁移路径：qt_surface.rs handle_input_method_query → 调用此适配器 → 读取 CursorAnchorRequest

use writer_core::platform_interaction::cursor_anchor::{
    CursorAnchorAdapter, CursorAnchorRequest, CursorAnchorUpdateReason,
    NormalizedCursorRect,
};

/// Linux Qt CursorAnchorAdapter 实现
///
/// 当前状态：空桩。实际的 QInputMethod::update 调用在 ime_visual.rs 的
/// update_ime_cursor_for_preedit() 中通过 cpp! 宏直接完成。
/// IME query 响应在 qt_surface.rs 的 handle_input_method_query() 中直接读 QML property。
///
/// 迁移计划：
/// 1. SujianEditorItem 在光标/选区/preedit 变化时调用 notify_cursor_anchor_update()
/// 2. 此适配器缓存 CursorAnchorRequest 数据
/// 3. qt_surface.rs handle_input_method_query() 从此适配器读取数据而非 QML property
/// 4. ime_visual.rs update_ime_cursor_for_preedit() 通过此适配器触发 QInputMethod::update()
pub struct LinuxQtCursorAnchorAdapter {
    item_ptr: *mut std::ffi::c_void,
    last_request: Option<CursorAnchorRequest>,
}

impl LinuxQtCursorAnchorAdapter {
    pub fn new(item_ptr: *mut std::ffi::c_void) -> Self {
        Self {
            item_ptr,
            last_request: None,
        }
    }

    pub fn set_item_ptr(&mut self, ptr: *mut std::ffi::c_void) {
        self.item_ptr = ptr;
    }

    pub fn last_request(&self) -> Option<&CursorAnchorRequest> {
        self.last_request.as_ref()
    }
}

unsafe impl Send for LinuxQtCursorAnchorAdapter {}
unsafe impl Sync for LinuxQtCursorAnchorAdapter {}

impl CursorAnchorAdapter for LinuxQtCursorAnchorAdapter {
    fn notify_cursor_anchor_update(
        &self,
        request: &CursorAnchorRequest,
        _reason: CursorAnchorUpdateReason,
    ) {
        // TODO(平台交互收口): 缓存 request 数据，供未来 IME query 使用
        let _ = request;
    }

    fn request_candidate_window_update(&self, _cursor_rect: &NormalizedCursorRect) {
        // TODO(平台交互收口): 触发 QInputMethod::update(Qt::ImCursorRectangle)
    }

    fn is_input_method_visible(&self) -> bool {
        // TODO(平台交互收口): 查询 QInputMethod::isVisible()
        false
    }
}
