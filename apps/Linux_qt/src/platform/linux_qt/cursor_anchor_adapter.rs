//! Linux Qt CursorAnchorAdapter 实现
//!
//! QInputMethod::update 和 InputMethodQuery 收敛到此。
//! SujianEditorItem 不直接调用 QInputMethod，只通过此适配器。
//!
//! 已完成迁移：
//! - notify_cursor_anchor_update() 缓存 CursorAnchorRequest 数据
//! - request_candidate_window_update() 触发 QInputMethod::update
//! - is_input_method_visible() 查询 QInputMethod::isVisible()
//!
//! 待完成迁移：
//! - qt_surface.rs handle_input_method_query() 仍直接读 QML property，
//!   需改为从此适配器 last_request() 读取数据

use cpp::cpp;
use std::cell::UnsafeCell;
use std::sync::Mutex;
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
    item_ptr: Mutex<*mut std::ffi::c_void>,
    last_request: Mutex<Option<CursorAnchorRequest>>,
}

impl LinuxQtCursorAnchorAdapter {
    pub fn new(item_ptr: *mut std::ffi::c_void) -> Self {
        Self {
            item_ptr: Mutex::new(item_ptr),
            last_request: Mutex::new(None),
        }
    }

    pub fn set_item_ptr(&self, ptr: *mut std::ffi::c_void) {
        if let Ok(mut guard) = self.item_ptr.lock() {
            *guard = ptr;
        }
    }

    pub fn last_request(&self) -> Option<CursorAnchorRequest> {
        self.last_request.lock().ok().and_then(|g| g.clone())
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
        if let Ok(mut guard) = self.last_request.lock() {
            *guard = Some(request.clone());
        }

        if let Ok(guard) = self.item_ptr.lock() {
            let item_ptr = *guard;
            if !item_ptr.is_null() {
                cpp!(unsafe [item_ptr as "QQuickItem*"] {
                    QInputMethod* im = QGuiApplication::inputMethod();
                    if (im) {
                        im->update(Qt::ImCursorRectangle | Qt::ImAnchorRectangle | Qt::ImSurroundingText | Qt::ImCursorPosition | Qt::ImCurrentSelection);
                    }
                });
            }
        }
    }

    fn request_candidate_window_update(&self, _cursor_rect: &NormalizedCursorRect) {
        if let Ok(guard) = self.item_ptr.lock() {
            let item_ptr = *guard;
            if !item_ptr.is_null() {
                cpp!(unsafe [item_ptr as "QQuickItem*"] {
                    QInputMethod* im = QGuiApplication::inputMethod();
                    if (im) {
                        im->update(Qt::ImCursorRectangle | Qt::ImAnchorRectangle);
                    }
                });
            }
        }
    }

    fn is_input_method_visible(&self) -> bool {
        cpp!(unsafe [] -> bool as "bool" {
            QInputMethod* im = QGuiApplication::inputMethod();
            return im ? im->isVisible() : false;
        })
    }
}
