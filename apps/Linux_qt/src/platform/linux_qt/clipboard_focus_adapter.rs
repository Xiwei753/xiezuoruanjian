//! Linux Qt ClipboardAndFocusAdapter 实现
//!
//! QClipboard / QMenu / forceActiveFocus / 软键盘收敛到此。
//!
//! 已完成迁移：
//! - execute_clipboard() 接入 QGuiApplication::clipboard()
//! - execute_focus() 接入 QQuickItem::forceActiveFocus / QInputMethod::show/hide
//! - focus_state() 跟踪焦点状态
//! - show_context_menu() 通过 QMetaObject::invokeMethod 触发 QML context_menu_requested 信号
//! - hide_context_menu() 通知 QML 侧关闭菜单

use cpp::cpp;
use qmetaobject::QString;
use std::sync::Mutex;
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
    item_ptr: Mutex<*mut std::ffi::c_void>,
    focus_state: Mutex<FocusState>,
}

impl LinuxQtClipboardFocusAdapter {
    pub fn new() -> Self {
        Self {
            item_ptr: Mutex::new(std::ptr::null_mut()),
            focus_state: Mutex::new(FocusState {
                has_focus: false,
                soft_input_visible: false,
            }),
        }
    }

    pub fn set_item_ptr(&self, ptr: *mut std::ffi::c_void) {
        if let Ok(mut guard) = self.item_ptr.lock() {
            *guard = ptr;
        }
    }
}

impl Default for LinuxQtClipboardFocusAdapter {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Send for LinuxQtClipboardFocusAdapter {}
unsafe impl Sync for LinuxQtClipboardFocusAdapter {}

impl ClipboardAndFocusAdapter for LinuxQtClipboardFocusAdapter {
    fn execute_clipboard(&mut self, request: ClipboardRequest) -> ClipboardResult {
        match request {
            ClipboardRequest::Copy { text } => {
                let text_utf16: Vec<u16> = text.encode_utf16().collect();
                let ptr = text_utf16.as_ptr();
                let len = text_utf16.len() as i32;
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
        if let Ok(guard) = self.item_ptr.lock() {
            let item_ptr = *guard;
            if item_ptr.is_null() {
                return;
            }
            match request {
                FocusRequest::RequestFocus | FocusRequest::RequestSoftInput => {
                    cpp!(unsafe [item_ptr as "QQuickItem*"] {
                        item_ptr->forceActiveFocus(Qt::MouseFocusReason);
                        QInputMethod* im = QGuiApplication::inputMethod();
                        if (im) {
                            im->update(Qt::ImEnabled | Qt::ImCursorRectangle | Qt::ImAnchorRectangle);
                            im->show();
                        }
                    });
                    if let Ok(mut fs) = self.focus_state.lock() {
                        fs.has_focus = true;
                        fs.soft_input_visible = true;
                    }
                }
                FocusRequest::ReleaseFocus | FocusRequest::HideSoftInput => {
                    cpp!(unsafe [item_ptr as "QQuickItem*"] {
                        QInputMethod* im = QGuiApplication::inputMethod();
                        if (im) {
                            im->hide();
                        }
                    });
                    if let Ok(mut fs) = self.focus_state.lock() {
                        fs.soft_input_visible = false;
                    }
                }
            }
        }
    }

    fn focus_state(&self) -> FocusState {
        self.focus_state.lock().map(|g| *g).unwrap_or(FocusState {
            has_focus: false,
            soft_input_visible: false,
        })
    }

    fn show_context_menu(&mut self, _request: ContextMenuRequest) {
        if let Ok(guard) = self.item_ptr.lock() {
            let item_ptr = *guard;
            if !item_ptr.is_null() {
                let x = _request.screen_x;
                let y = _request.screen_y;
                cpp!(unsafe [item_ptr as "QQuickItem*", x as "double", y as "double"] {
                    QMetaObject::invokeMethod(item_ptr, "context_menu_requested",
                        Q_ARG(QVariant, QVariant(x)),
                        Q_ARG(QVariant, QVariant(y)));
                });
            }
        }
    }

    fn hide_context_menu(&mut self) {
        if let Ok(guard) = self.item_ptr.lock() {
            let item_ptr = *guard;
            if !item_ptr.is_null() {
                cpp!(unsafe [item_ptr as "QQuickItem*"] {
                    QMetaObject::invokeMethod(item_ptr, "hide_context_menu_requested");
                });
            }
        }
    }
}
