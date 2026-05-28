//! # Qt C++ 互操作测试（Linux UI 层 - 测试）
//!
//! 测试 Rust 与 Qt C++ 的互操作能力，验证 QTextDocument 的格式化功能。
//!
//! ## 架构定位
//! - 测试用例，验证 cpp! 宏与 Qt API 的集成
//! - EditorFormatter QObject 用于测试 QTextBlockFormat 的应用
//!
//! ## 使用场景
//! - 开发阶段验证 Qt C++ 绑定的正确性
//! - 不在生产环境使用

use qmetaobject::prelude::*;
use cpp::cpp;

cpp!{{
    #include <QVariant>
    #include <QQuickTextDocument>
    #include <QTextDocument>
    #include <QTextCursor>
    #include <QTextBlockFormat>
    #include <QDebug>
}}

#[derive(QObject, Default)]
pub struct EditorFormatter {
    base: qt_base_class!(trait QObject),
    format_document: qt_method!(fn(&self, doc_variant: QVariant, font_size: f32, line_spacing: f32, indent: f32) {
        cpp!(unsafe [doc_variant as "QVariant", font_size as "float", line_spacing as "float", indent as "float"] {
            QObject* obj = doc_variant.value<QObject*>();
            if (!obj) {
                qDebug() << "Doc variant is null";
                return;
            }
            QQuickTextDocument* qquick_doc = qobject_cast<QQuickTextDocument*>(obj);
            if (!qquick_doc) {
                qDebug() << "Cannot cast to QQuickTextDocument";
                return;
            }
            QTextDocument* doc = qquick_doc->textDocument();
            if (!doc) return;

            QTextCursor cursor(doc);
            cursor.select(QTextCursor::Document);
            QTextBlockFormat blockFormat;
            blockFormat.setLineHeight(line_spacing * 100, QTextBlockFormat::ProportionalHeight);
            blockFormat.setTextIndent(indent);
            cursor.mergeBlockFormat(blockFormat);
        });
    }),
}
