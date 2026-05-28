use qmetaobject::prelude::*;
use qmetaobject::QString;
use cpp::cpp;

cpp!{{
    #include <QtCore/QVariant>
    #include <QtQuick/QQuickTextDocument>
    #include <QtGui/QTextDocument>
    #include <QtGui/QTextCursor>
    #include <QtGui/QTextBlockFormat>
}}

#[allow(dead_code)]
#[derive(QObject, Default)]
pub struct DocumentHandler {
    base: qt_base_class!(trait QObject),

    document: qt_property!(QVariant; READ document WRITE set_document NOTIFY document_changed),
    line_spacing: qt_property!(f32; READ line_spacing WRITE set_line_spacing NOTIFY line_spacing_changed),
    text_indent: qt_property!(f32; READ text_indent WRITE set_text_indent NOTIFY text_indent_changed),

    document_changed: qt_signal!(),
    line_spacing_changed: qt_signal!(),
    text_indent_changed: qt_signal!(),

    apply_format: qt_method!(fn(&self)),
    set_plain_text: qt_method!(fn(&self, text: QString)),
    clear_undo_stack: qt_method!(fn(&self)),

    current_doc: QVariant,
    current_line_spacing: f32,
    current_text_indent: f32,
}

impl DocumentHandler {
    fn document(&self) -> QVariant { self.current_doc.clone() }
    fn set_document(&mut self, val: QVariant) {
        self.current_doc = val;
        self.document_changed();
        self.apply_format();
    }

    fn line_spacing(&self) -> f32 { self.current_line_spacing }
    fn set_line_spacing(&mut self, val: f32) {
        if (self.current_line_spacing - val).abs() > 0.001 {
            self.current_line_spacing = val;
            self.line_spacing_changed();
            self.apply_format();
        }
    }

    fn text_indent(&self) -> f32 { self.current_text_indent }
    fn set_text_indent(&mut self, val: f32) {
        if (self.current_text_indent - val).abs() > 0.001 {
            self.current_text_indent = val;
            self.text_indent_changed();
            self.apply_format();
        }
    }

    fn apply_format(&self) {
        let doc_variant = self.current_doc.clone();
        let line_spacing = self.current_line_spacing;
        let indent = self.current_text_indent;

        cpp!(unsafe [doc_variant as "QVariant", line_spacing as "float", indent as "float"] {
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

            cursor.endEditBlock();
        });
    }

    fn set_plain_text(&self, text: QString) {
        let doc_variant = self.current_doc.clone();
        let line_spacing = self.current_line_spacing;
        let indent = self.current_text_indent;

        cpp!(unsafe [doc_variant as "QVariant", line_spacing as "float", indent as "float", text as "QString"] {
            QObject* obj = doc_variant.value<QObject*>();
            if (!obj) return;
            QQuickTextDocument* qquick_doc = qobject_cast<QQuickTextDocument*>(obj);
            if (!qquick_doc) return;
            QTextDocument* doc = qquick_doc->textDocument();
            if (!doc) return;

            QTextCursor cursor(doc);
            cursor.beginEditBlock();

            cursor.select(QTextCursor::Document);
            cursor.removeSelectedText();

            QTextBlockFormat blockFormat;
            blockFormat.setLineHeight(line_spacing * 100, QTextBlockFormat::ProportionalHeight);
            blockFormat.setTextIndent(indent);

            QStringList lines = text.split("\n");
            for (int i = 0; i < lines.size(); ++i) {
                cursor.setBlockFormat(blockFormat);
                cursor.insertText(lines[i]);
                if (i < lines.size() - 1) {
                    cursor.insertBlock();
                }
            }

            cursor.endEditBlock();
        });
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
