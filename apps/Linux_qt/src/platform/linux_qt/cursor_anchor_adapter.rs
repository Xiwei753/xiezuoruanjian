//! Linux Qt CursorAnchorAdapter 实现
//!
//! QInputMethod::update 和 InputMethodQuery 收敛到此。
//! SujianEditorItem 不直接调用 QInputMethod，只通过此适配器。
//!
//! 安全约束：
//! - 本适配器持有 QQuickItem* 裸指针，仅限 GUI 线程使用。
//! - 使用 Rc<Cell<>> 而非 Mutex，因为 GUI 单线程不需要跨线程同步；
//!   Rc 不是 Send/Sync，编译器会阻止跨线程传播。

use cpp::cpp;
use std::cell::Cell;
use std::rc::Rc;
use writer_core::platform_interaction::cursor_anchor::{
    CursorAnchorAdapter, CursorAnchorRequest, CursorAnchorUpdateReason,
    NormalizedCursorRect,
};

cpp! {{
    #include <QtGui/QInputMethod>
    #include <QGuiApplication>
    #include <QtQuick/QQuickItem>
}}

pub struct LinuxQtCursorAnchorAdapter {
    item_ptr: Rc<Cell<*mut std::ffi::c_void>>,
    last_request: Rc<Cell<Option<CursorAnchorRequest>>>,
}

impl LinuxQtCursorAnchorAdapter {
    pub fn new(item_ptr: *mut std::ffi::c_void) -> Self {
        Self {
            item_ptr: Rc::new(Cell::new(item_ptr)),
            last_request: Rc::new(Cell::new(None)),
        }
    }

    pub fn set_item_ptr(&self, ptr: *mut std::ffi::c_void) {
        self.item_ptr.set(ptr);
    }

    pub fn last_request(&self) -> Option<CursorAnchorRequest> {
        self.last_request.take()
    }
}

impl CursorAnchorAdapter for LinuxQtCursorAnchorAdapter {
    fn notify_cursor_anchor_update(
        &self,
        request: &CursorAnchorRequest,
        _reason: CursorAnchorUpdateReason,
    ) {
        self.last_request.set(Some(request.clone()));

        let item_ptr = self.item_ptr.get();
        if !item_ptr.is_null() {
            cpp!(unsafe [item_ptr as "QQuickItem*"] {
                QInputMethod* im = QGuiApplication::inputMethod();
                if (im) {
                    im->update(Qt::ImCursorRectangle | Qt::ImAnchorRectangle | Qt::ImSurroundingText | Qt::ImCursorPosition | Qt::ImCurrentSelection);
                }
            });
        }
    }

    fn request_candidate_window_update(&self, _cursor_rect: &NormalizedCursorRect) {
        let item_ptr = self.item_ptr.get();
        if !item_ptr.is_null() {
            cpp!(unsafe [item_ptr as "QQuickItem*"] {
                QInputMethod* im = QGuiApplication::inputMethod();
                if (im) {
                    im->update(Qt::ImCursorRectangle | Qt::ImAnchorRectangle);
                }
            });
        }
    }

    fn is_input_method_visible(&self) -> bool {
        cpp!(unsafe [] -> bool as "bool" {
            QInputMethod* im = QGuiApplication::inputMethod();
            return im ? im->isVisible() : false;
        })
    }
}
