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
