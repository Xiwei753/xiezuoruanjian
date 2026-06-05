// =============================================================================
// document_handler.rs — QTextDocument 视觉排版操作器
// =============================================================================
//
// 引用了什么：
// - qmetaobject：提供 QObject 宏定义，在 Rust 侧实现供 QML 直接调用的方法与属性。
// - cpp：内联 C++ 头文件与类，操纵 Qt 原生 C++ 类（QQuickTextDocument、QTextDocument、QTextCursor、QTextBlockFormat 等）。
//
// 干什么的：
// - 接管 QML TextArea 关联的 QTextDocument 实例。
// - 负责纯文本的视觉排版调整，例如行高比例、段首首行缩进、主题正文前景色和临时隐藏范围。
// - 实现纯文本的安全提取（doc->toPlainText()），杜绝 HTML 字符串或富文本内容物理污染磁盘正文。
// - 提供在切换章节时一键清空撤销栈（clearUndoRedoStacks()）的底层实现。
//
// 被什么引用：
// - 被 apps/desktop/src/main.rs 注册为 QML 类 "DocumentHandler" (在 "Sujian" 命名空间下)。
// - 被 apps/desktop/qml/EditorController.qml 实例化并绑定至 TextArea。
// =============================================================================

//! # QTextDocument 排版操作（Desktop UI 层 - Backend Adapter）
//!
//! 负责纯文本的视觉排版（行距、首行缩进、主题正文前景色），不改变正文内容。
//!
//! ## 架构定位
//!
//! ```text
//! QML EditorPage → DocumentHandler (QObject) → QTextDocument (Qt)
//! ```
//!
//! ## 职责边界
//!
//! - **做**：应用行距、首行缩进、主题正文前景色、临时隐藏输入范围、获取纯文本、清空撤销栈
//! - **不做**：正文内容管理（由 WriterCore 负责）
//! - **不做**：业务逻辑（只做视觉排版）
//!
//! ## 设计原则
//!
//! - 只做视觉排版，不改变正文文件内容
//! - 字号、行距、首行缩进和正文前景色只影响显示，不改变正文文件
//! - 正文文件永远是纯文本
//!
//! ## 关键方法
//!
//! - `apply_format()`：将行距、首行缩进和主题正文前景色应用到 QTextDocument
//! - `get_plain_text()`：获取纯文本（替换 `\u2029` 为 `\n`）
//! - `hide_text_range()` / `show_text_range()`：临时隐藏/恢复字符显示，不改变纯文本内容
//! - `clear_undo_stack()`：清空撤销栈（章节切换时调用）

use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::QString;

cpp! {{
    #include <QtCore/QVariant>
    #include <QtQuick/QQuickTextDocument>
    #include <QtGui/QTextDocument>
    #include <QtGui/QTextCursor>
    #include <QtGui/QTextBlockFormat>
    #include <QtGui/QTextCharFormat>
    #include <QtGui/QColor>
    #include <QtCore/QDebug>
    #include <algorithm>
}}

#[allow(dead_code)]
#[derive(QObject, Default)]
pub struct DocumentHandler {
    base: qt_base_class!(trait QObject),

    document: qt_property!(QVariant; READ document WRITE set_document NOTIFY document_changed),
    line_spacing: qt_property!(f32; READ line_spacing WRITE set_line_spacing NOTIFY line_spacing_changed),
    text_indent: qt_property!(f32; READ text_indent WRITE set_text_indent NOTIFY text_indent_changed),
    text_color: qt_property!(QString; READ text_color WRITE set_text_color NOTIFY text_color_changed),

    document_changed: qt_signal!(),
    line_spacing_changed: qt_signal!(),
    text_indent_changed: qt_signal!(),
    text_color_changed: qt_signal!(),

    apply_format: qt_method!(fn(&self)),
    get_plain_text: qt_method!(fn(&self) -> QString),
    hide_text_range: qt_method!(fn(&self, start: i32, length: i32)),
    show_text_range: qt_method!(fn(&self, start: i32, length: i32)),
    clear_hidden_text_ranges: qt_method!(fn(&self)),
    clear_undo_stack: qt_method!(fn(&self)),

    current_doc: QVariant,
    current_line_spacing: f32,
    current_text_indent: f32,
    current_text_color: QString,
}

impl DocumentHandler {
    fn document(&self) -> QVariant {
        self.current_doc.clone()
    }
    fn set_document(&mut self, val: QVariant) {
        self.current_doc = val;
        self.document_changed();
        self.apply_format();
    }

    fn line_spacing(&self) -> f32 {
        self.current_line_spacing
    }
    fn set_line_spacing(&mut self, val: f32) {
        if (self.current_line_spacing - val).abs() > 0.001 {
            self.current_line_spacing = val;
            self.line_spacing_changed();
            self.apply_format();
        }
    }

    fn text_indent(&self) -> f32 {
        self.current_text_indent
    }
    fn set_text_indent(&mut self, val: f32) {
        if (self.current_text_indent - val).abs() > 0.001 {
            self.current_text_indent = val;
            self.text_indent_changed();
            self.apply_format();
        }
    }

    fn text_color(&self) -> QString {
        self.current_text_color.clone()
    }
    fn set_text_color(&mut self, val: QString) {
        if self.current_text_color.to_string() != val.to_string() {
            self.current_text_color = val;
            self.text_color_changed();
            self.apply_format();
        }
    }

    fn apply_format(&self) {
        let doc_variant = self.current_doc.clone();
        let line_spacing = self.current_line_spacing;
        let indent = self.current_text_indent;
        let text_color = self.current_text_color.clone();

        cpp!(unsafe [doc_variant as "QVariant", line_spacing as "float", indent as "float", text_color as "QString"] {
            QObject* obj = doc_variant.value<QObject*>();
            if (!obj) return;
            QQuickTextDocument* qquick_doc = qobject_cast<QQuickTextDocument*>(obj);
            if (!qquick_doc) return;
            QTextDocument* doc = qquick_doc->textDocument();
            if (!doc) return;

            QTextCursor cursor(doc);
            cursor.beginEditBlock();
            cursor.select(QTextCursor::Document);

            QTextBlockFormat blockFormat;
            blockFormat.setLineHeight(line_spacing * 100, QTextBlockFormat::ProportionalHeight);
            blockFormat.setTextIndent(indent);
            cursor.mergeBlockFormat(blockFormat);

            QColor color(text_color);
            qWarning().noquote()
                << "[SujianThemeDiagnostics] DocumentHandler.apply_format"
                << "text_color=" << text_color
                << "valid=" << color.isValid()
                << "line_spacing=" << line_spacing
                << "indent=" << indent;
            if (color.isValid()) {
                QTextCharFormat charFormat;
                charFormat.setForeground(color);
                cursor.setCharFormat(charFormat);
            }

            cursor.endEditBlock();
        });
    }

    fn get_plain_text(&self) -> QString {
        let doc_variant = self.current_doc.clone();

        cpp!(unsafe [doc_variant as "QVariant"] -> QString as "QString" {
            QObject* obj = doc_variant.value<QObject*>();
            if (!obj) return QString();
            QQuickTextDocument* qquick_doc = qobject_cast<QQuickTextDocument*>(obj);
            if (!qquick_doc) return QString();
            QTextDocument* doc = qquick_doc->textDocument();
            if (!doc) return QString();
            return doc->toPlainText();
        })
    }

    fn hide_text_range(&self, start: i32, length: i32) {
        let doc_variant = self.current_doc.clone();

        cpp!(unsafe [doc_variant as "QVariant", start as "int", length as "int"] {
            QObject* obj = doc_variant.value<QObject*>();
            if (!obj) return;
            QQuickTextDocument* qquick_doc = qobject_cast<QQuickTextDocument*>(obj);
            if (!qquick_doc) return;
            QTextDocument* doc = qquick_doc->textDocument();
            if (!doc || length <= 0) return;

            const int doc_len = std::max(0, doc->characterCount() - 1);
            const int safe_start = std::max(0, std::min(start, doc_len));
            const int safe_end = std::max(safe_start, std::min(start + length, doc_len));
            if (safe_end <= safe_start) return;

            QTextCharFormat hidden_format;
            hidden_format.setForeground(QColor(0, 0, 0, 0));

            QTextCursor cursor(doc);
            cursor.setPosition(safe_start);
            cursor.setPosition(safe_end, QTextCursor::KeepAnchor);
            cursor.mergeCharFormat(hidden_format);
        });
    }

    fn show_text_range(&self, start: i32, length: i32) {
        let doc_variant = self.current_doc.clone();
        let text_color = self.current_text_color.clone();

        cpp!(unsafe [doc_variant as "QVariant", start as "int", length as "int", text_color as "QString"] {
            QObject* obj = doc_variant.value<QObject*>();
            if (!obj) return;
            QQuickTextDocument* qquick_doc = qobject_cast<QQuickTextDocument*>(obj);
            if (!qquick_doc) return;
            QTextDocument* doc = qquick_doc->textDocument();
            if (!doc || length <= 0) return;

            const int doc_len = std::max(0, doc->characterCount() - 1);
            const int safe_start = std::max(0, std::min(start, doc_len));
            const int safe_end = std::max(safe_start, std::min(start + length, doc_len));
            if (safe_end <= safe_start) return;

            QColor color(text_color);
            if (!color.isValid()) return;

            QTextCharFormat visible_format;
            visible_format.setForeground(color);

            QTextCursor cursor(doc);
            cursor.setPosition(safe_start);
            cursor.setPosition(safe_end, QTextCursor::KeepAnchor);
            cursor.mergeCharFormat(visible_format);
        });
    }

    fn clear_hidden_text_ranges(&self) {
        self.apply_format();
    }

    fn clear_undo_stack(&self) {
        let doc_variant = self.current_doc.clone();
        cpp!(unsafe [doc_variant as "QVariant"] {
            QObject* obj = doc_variant.value<QObject*>();
            if (!obj) return;
            QQuickTextDocument* qquick_doc = qobject_cast<QQuickTextDocument*>(obj);
            if (!qquick_doc) return;
            QTextDocument* doc = qquick_doc->textDocument();
            if (!doc) return;
            doc->clearUndoRedoStacks();
        });
    }
}
