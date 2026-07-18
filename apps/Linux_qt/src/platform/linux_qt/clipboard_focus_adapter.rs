//! Linux Qt ClipboardAndFocusAdapter 实现
//!
//! QClipboard / QMenu / forceActiveFocus / 软键盘收敛到此。
//!
//! 安全约束：
//! - 本适配器持有 QQuickItem* 裸指针，仅限 GUI 线程使用。
//! - 使用 Rc<Cell<>> 而非 Mutex，因为 GUI 单线程不需要跨线程同步；
//!   Rc 不是 Send/Sync，编译器会阻止跨线程传播。
//!
//! 已完成迁移：
//! - execute_clipboard() 接入 QGuiApplication::clipboard()
//! - execute_focus() 接入 QQuickItem::forceActiveFocus / QInputMethod::show/hide
//! - focus_state() 跟踪焦点状态
//! - show_context_menu() 通过 QMetaObject::invokeMethod 触发 QML context_menu_requested 信号
//! - hide_context_menu() 通知 QML 侧关闭菜单

use cpp::cpp;
use qmetaobject::QString;
use std::cell::Cell;
use std::rc::Rc;
use writer_core::platform_interaction::clipboard_focus::{
    ClipboardAndFocusAdapter, ClipboardRequest, ClipboardResult,
    ContextMenuRequest, FocusRequest, FocusState,
};

cpp! {{
    #include <QtGui/QClipboard>
    #include <QGuiApplication>
    #include <QtQuick/QQuickItem>
    #include <QtGui/QInputMethod>
}}

pub struct LinuxQtClipboardFocusAdapter {
    item_ptr: Rc<Cell<*mut std::ffi::c_void>>,
    focus_state: FocusState,
}

impl LinuxQtClipboardFocusAdapter {
    pub fn new() -> Self {
        Self {
            item_ptr: Rc::new(Cell::new(std::ptr::null_mut())),
            focus_state: FocusState {
                has_focus: false,
                soft_input_visible: false,
            },
        }
    }

    pub fn set_item_ptr(&self, ptr: *mut std::ffi::c_void) {
        self.item_ptr.set(ptr);
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
            ClipboardRequest::Copy { text } => {
                let text_utf16: Vec<u16> = text.encode_utf16().collect();
                let ptr = text_utf16.as_ptr();
                let len = text_utf16.len() as i32;
                // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                cpp!(unsafe [ptr as "const ushort*", len as "int"] {
                    QClipboard* cb = QGuiApplication::clipboard();
                    if (cb) {
                        QString qs = QString::fromUtf16(ptr, len);
                        cb->setText(qs);
                    }
                });
                ClipboardResult::Copied
            }
            ClipboardRequest::Paste => {
                let qtext: QString = cpp!(unsafe [] -> QString as "QString" {
                    QClipboard* cb = QGuiApplication::clipboard();
                    return cb ? cb->text(QClipboard::Clipboard) : QString();
                });
                let text = qtext.to_string();
                if text.is_empty() {
                    return ClipboardResult::Unavailable;
                }
                ClipboardResult::Pasted { text }
            }
            ClipboardRequest::Cut { text } => {
                let text_utf16: Vec<u16> = text.encode_utf16().collect();
                let ptr = text_utf16.as_ptr();
                let len = text_utf16.len() as i32;
                // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                cpp!(unsafe [ptr as "const ushort*", len as "int"] {
                    QClipboard* cb = QGuiApplication::clipboard();
                    if (cb) {
                        QString qs = QString::fromUtf16(ptr, len);
                        cb->setText(qs);
                    }
                });
                ClipboardResult::Cut
            }
            ClipboardRequest::HasText => {
                let has = cpp!(unsafe [] -> bool as "bool" {
                    QClipboard* cb = QGuiApplication::clipboard();
                    return cb ? !cb->text().isEmpty() : false;
                });
                ClipboardResult::HasText { has_text: has }
            }
        }
    }

    fn execute_focus(&mut self, request: FocusRequest) {
        let item_ptr = self.item_ptr.get();
        if item_ptr.is_null() {
            return;
        }
        match request {
            FocusRequest::RequestFocus | FocusRequest::RequestSoftInput => {
                // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                cpp!(unsafe [item_ptr as "QQuickItem*"] {
                    item_ptr->forceActiveFocus(Qt::MouseFocusReason);
                    QInputMethod* im = QGuiApplication::inputMethod();
                    if (im) {
                        im->update(Qt::ImEnabled | Qt::ImCursorRectangle | Qt::ImAnchorRectangle);
                        im->show();
                    }
                });
                self.focus_state.has_focus = true;
                self.focus_state.soft_input_visible = true;
            }
            FocusRequest::ReleaseFocus | FocusRequest::HideSoftInput => {
                // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                cpp!(unsafe [item_ptr as "QQuickItem*"] {
                    QInputMethod* im = QGuiApplication::inputMethod();
                    if (im) {
                        im->hide();
                    }
                });
                self.focus_state.soft_input_visible = false;
            }
        }
    }

    fn focus_state(&self) -> FocusState {
        self.focus_state
    }

    fn show_context_menu(&mut self, _request: ContextMenuRequest) {
        let item_ptr = self.item_ptr.get();
        if !item_ptr.is_null() {
            let x = _request.screen_x;
            let y = _request.screen_y;
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            cpp!(unsafe [item_ptr as "QQuickItem*", x as "double", y as "double"] {
                QMetaObject::invokeMethod(item_ptr, "context_menu_requested",
                    Q_ARG(QVariant, QVariant(x)),
                    Q_ARG(QVariant, QVariant(y)));
            });
        }
    }

    fn hide_context_menu(&mut self) {
        let item_ptr = self.item_ptr.get();
        if !item_ptr.is_null() {
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            cpp!(unsafe [item_ptr as "QQuickItem*"] {
                QMetaObject::invokeMethod(item_ptr, "hide_context_menu_requested");
            });
        }
    }
}
