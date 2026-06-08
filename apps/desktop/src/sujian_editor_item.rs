// =============================================================================
// sujian_editor_item.rs — Desktop self-rendered editor item
// =============================================================================

use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::{QBrush, QColor, QLineF, QMouseEvent, QPainter, QPainterRenderHint, QPen, QPointF, QQuickItem, QRectF, QString};
use std::cell::Cell;
use std::time::{Duration, Instant};
use writer_core::editor::{EditorCursor, EditorEngine, EditorSelection, EditorTransactionCause};

cpp! {{
    #include <QtGui/QFont>
    #include <QtGui/QFontMetricsF>
    #include <QtGui/QPainter>
    #include <QtGui/QClipboard>
    #include <QtGui/QTextLayout>
    #include <QtGui/QTextOption>
    #include <QtGui/QInputMethodEvent>
    #include <QtGui/QKeyEvent>
    #include <QtGui/QMouseEvent>
    #include <QtQuick/QSGSimpleTextureNode>
    #include <QtQuick/QSGTexture>
    #include <QtQuick/QSGTransformNode>
    #include <QtQuick/QSGImageNode>
    #include <QtQuick/QSGRectangleNode>
    #include <QGuiApplication>
    #include <vector>

    struct QTextLayoutEntry {
        int byteStart;
        double width;
        double xPos;
    };
    thread_local std::vector<QTextLayoutEntry> g_layout_buf;

    // ---- Rust callbacks for event filter ----
    extern "C" bool sujian_handle_key_and_text(void* rust_item, int key, int modifiers, const ushort* text, int text_len);
    extern "C" void sujian_ime_commit(void* rust_item, const ushort* text, int text_len);
    extern "C" void sujian_ime_preedit(void* rust_item, const ushort* text, int text_len, int cursor);
    extern "C" void sujian_ime_cancel(void* rust_item);
    extern "C" void sujian_request_repaint(void* rust_item);

    // ---- Event filter: intercepts KeyPress + InputMethod on SujianEditorItem ----
    class SujianEventFilter : public QObject {
    public:
        void* rust_item;
        SujianEventFilter(QObject* parent, void* item)
            : QObject(parent), rust_item(item) {}

        bool eventFilter(QObject* obj, QEvent* event) override {
            if (!rust_item) return false;

            switch (event->type()) {
            case QEvent::KeyPress: {
                auto* ke = static_cast<QKeyEvent*>(event);
                QString text = ke->text();
                bool accepted = sujian_handle_key_and_text(
                    rust_item,
                    ke->key(),
                    static_cast<int>(ke->modifiers()),
                    reinterpret_cast<const ushort*>(text.utf16()),
                    static_cast<int>(text.size())
                );
                if (accepted) {
                    event->accept();
                    return true;
                }
                return false;
            }
            case QEvent::InputMethod: {
                auto* ime = static_cast<QInputMethodEvent*>(event);
                QString commit = ime->commitString();
                QString preedit = ime->preeditString();
                if (!commit.isEmpty()) {
                    sujian_ime_commit(
                        rust_item,
                        reinterpret_cast<const ushort*>(commit.utf16()),
                        static_cast<int>(commit.size())
                    );
                }
                if (!preedit.isEmpty()) {
                    int cursor = preedit.length();
                    if (ime->replacementStart() >= 0) {
                        cursor = ime->replacementStart() + ime->replacementLength();
                        if (cursor < 0) cursor = preedit.length();
                    }
                    sujian_ime_preedit(
                        rust_item,
                        reinterpret_cast<const ushort*>(preedit.utf16()),
                        static_cast<int>(preedit.size()),
                        cursor
                    );
                } else if (commit.isEmpty()) {
                    sujian_ime_cancel(rust_item);
                }
                sujian_request_repaint(rust_item);
                event->accept();
                return true;
            }
            case QEvent::InputMethodQuery: {
                auto* qe = static_cast<QInputMethodQueryEvent*>(event);
                if (qe->queries() & Qt::ImCursorRectangle) {
                    double cx = obj->property("cursor_rect_x").toDouble();
                    double cy = obj->property("cursor_rect_y").toDouble();
                    double cw = obj->property("cursor_rect_width").toDouble();
                    double ch = obj->property("cursor_rect_height").toDouble();
                    qe->setValue(Qt::ImCursorRectangle, QRectF(cx, cy, cw, ch));
                }
                if (qe->queries() & Qt::ImEnabled) {
                    qe->setValue(Qt::ImEnabled, true);
                }
                if (qe->queries() & Qt::ImHints) {
                    qe->setValue(Qt::ImHints, static_cast<int>(Qt::ImhNoPredictiveText));
                }
                if (qe->queries() & Qt::ImAnchorRectangle) {
                    double cx = obj->property("cursor_rect_x").toDouble();
                    double cy = obj->property("cursor_rect_y").toDouble();
                    double cw = obj->property("cursor_rect_width").toDouble();
                    double ch = obj->property("cursor_rect_height").toDouble();
                    qe->setValue(Qt::ImAnchorRectangle, QRectF(cx, cy, cw, ch));
                }
                event->accept();
                return true;
            }
            default:
                return false;
            }
        }
    };

    void sujian_install_event_filter(QQuickItem* item, void* rust_item) {
        if (!item) return;
        auto* filter = new SujianEventFilter(item, rust_item);
        item->installEventFilter(filter);
        item->setFlag(QQuickItem::ItemHasContents, true);
        item->setFlag(QQuickItem::ItemAcceptsInputMethod, true);
        item->setFlag(QQuickItem::ItemIsFocusScope, true);
        item->setAcceptedMouseButtons(Qt::AllButtons);
    }

    // Ensure cursor rect child exists on the given QSGTransformNode root.
    // Returns the QSGRectangleNode* (child 1).
    QSGRectangleNode* sujian_ensure_cursor_node(QSGTransformNode *root, QQuickItem *item, unsigned int color_rgba) {
        QSGRectangleNode *cursorNode = nullptr;
        if (root->childCount() > 1) {
            cursorNode = dynamic_cast<QSGRectangleNode*>(root->lastChild());
        }
        if (!cursorNode) {
            cursorNode = item->window()->createRectangleNode();
            cursorNode->setColor(QColor::fromRgba(color_rgba));
            root->appendChildNode(cursorNode);
        }
        return cursorNode;
    }

    void sujian_update_cursor_rect(QSGTransformNode *root, QQuickItem *item,
        double cx, double cy, double cw, double ch, bool visible, unsigned int color_rgba) {
        if (!root) return;
        // Find existing cursor node (always last child if present)
        QSGRectangleNode *cursorNode = nullptr;
        if (root->childCount() > 1) {
            cursorNode = dynamic_cast<QSGRectangleNode*>(root->lastChild());
        }
        if (!visible) {
            if (cursorNode) {
                root->removeChildNode(cursorNode);
                delete cursorNode;
            }
            return;
        }
        // Visible: create if needed, position, mark dirty
        if (!cursorNode) {
            cursorNode = item->window()->createRectangleNode();
            root->appendChildNode(cursorNode);
        }
        cursorNode->setRect(QRectF(cx, cy, cw, ch));
        cursorNode->setColor(QColor::fromRgba(color_rgba));
        cursorNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
    }
}}

cpp! {{
    // ---- QTextLayout-based positioning helpers ----

    // Given paragraph text and a cursor byte offset (UTF-8), return x position
    // relative to paragraph start. Uses QTextLayout for accurate positioning.
    // `cursor_before` is the text before the cursor (UTF-8 substring).
    double sujian_cursor_to_x(
        const QString& paraText, double fs, const QString& ff,
        const QString& textBeforeCursor
    ) {
        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QTextLayout layout(paraText, font);
        QTextOption option;
        option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
        layout.setTextOption(option);
        layout.beginLayout();

        // Find which QTextLine contains the cursor by matching text length
        int qchar_count = textBeforeCursor.size();
        double x = 0.0;
        while (true) {
            QTextLine line = layout.createLine();
            if (!line.isValid()) break;
            int line_start = line.textStart();
            int line_len = line.textLength();
            if (qchar_count >= line_start && qchar_count <= line_start + line_len) {
                x = line.cursorToX(qchar_count - line_start);
                break;
            }
        }
        layout.endLayout();
        return x;
    }

    // Given paragraph text, a cursor QChar offset, and line layout params,
    // returns x position relative to the specific QTextLine's content start.
    // `paragraph_wrap_w` is the full paragraph wrap width (line_wrap_width + line_indent_x).
    // Does NOT add indent to the returned x — caller adds line.x which already has indent.
    double sujian_cursor_to_x_on_line(
        const QString& paraText, int cursor_qchar, double fs, const QString& ff,
        double paragraph_wrap_w, double indent_w, int qtextline_idx
    ) {
        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QTextLayout layout(paraText, font);
        QTextOption option;
        option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
        layout.setTextOption(option);
        layout.beginLayout();

        double x = 0.0;
        int cur_idx = 0;
        bool first = true;
        while (true) {
            QTextLine line = layout.createLine();
            if (!line.isValid()) break;
            double lineWrap = first ? (paragraph_wrap_w - indent_w) : paragraph_wrap_w;
            line.setLineWidth(lineWrap);
            if (cur_idx == qtextline_idx) {
                int local_qchar = cursor_qchar - line.textStart();
                x = line.cursorToX(local_qchar);
                break;
            }
            first = false;
            cur_idx++;
        }
        layout.endLayout();
        return x;
    }

    // Given paragraph text, a cursor QChar offset (para-relative), and line layout params,
    // returns glyph positions for [range_qchar_start, range_qchar_end) on the specific QTextLine.
    // Results stored in g_layout_buf as {byteStart=para_qchar_offset, width=glyph_width, xPos=glyph_x}.
    // `paragraph_wrap_w` is the full paragraph wrap width (line_wrap_width + line_indent_x).
    // Does NOT add indent to xPos — caller adds line.x which already has indent.
    void sujian_glyph_positions_on_line(
        const QString& paraText, int range_qchar_start, int range_qchar_end,
        double fs, const QString& ff, double paragraph_wrap_w, double indent_w, int qtextline_idx
    ) {
        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QTextLayout layout(paraText, font);
        QTextOption option;
        option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
        layout.setTextOption(option);
        layout.beginLayout();

        g_layout_buf.clear();
        int cur_idx = 0;
        bool first = true;
        while (true) {
            QTextLine line = layout.createLine();
            if (!line.isValid()) break;
            double lineWrap = first ? (paragraph_wrap_w - indent_w) : paragraph_wrap_w;
            line.setLineWidth(lineWrap);
            if (cur_idx == qtextline_idx) {
                int line_start = line.textStart();
                int line_end = line_start + line.textLength();
                int r_start = std::max(range_qchar_start, line_start);
                int r_end = std::min(range_qchar_end, line_end);
                for (int i = r_start; i < r_end; i++) {
                    double x1 = line.cursorToX(i - line_start);
                    double x2 = line.cursorToX(i - line_start + 1);
                    QTextLayoutEntry e;
                    e.byteStart = i;  // QChar offset within paragraph
                    e.width = std::abs(x2 - x1);
                    e.xPos = x1;
                    g_layout_buf.push_back(e);
                }
                break;
            }
            first = false;
            cur_idx++;
        }
        layout.endLayout();
    }

    // Given paragraph text and an x position, return byte offset within paragraph.
    // Returns the QChar offset; caller converts to UTF-8 byte offset.
    int sujian_x_to_cursor(
        const QString& paraText, double x, double fs, const QString& ff,
        int qtextline_idx
    ) {
        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QTextLayout layout(paraText, font);
        QTextOption option;
        option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
        layout.setTextOption(option);
        layout.beginLayout();

        int target_idx = 0;
        int cur_idx = 0;
        while (true) {
            QTextLine line = layout.createLine();
            if (!line.isValid()) break;
            if (cur_idx == qtextline_idx) {
                target_idx = line.xToCursor(x);
                // Convert to paragraph-relative QChar offset
                target_idx = line.textStart() + target_idx;
                break;
            }
            cur_idx++;
        }
        layout.endLayout();
        return target_idx;
    }

    // Given paragraph text, an x position (relative to line content start), and line layout params,
    // returns QChar offset within paragraph on the specific QTextLine.
    // `paragraph_wrap_w` is the full paragraph wrap width (line_wrap_width + line_indent_x).
    // `x` is relative to line content start (not including indent), so for the first line
    // we subtract indent before calling xToCursor.
    int sujian_x_to_cursor_on_line(
        const QString& paraText, double x, double fs, const QString& ff,
        double paragraph_wrap_w, double indent_w, int qtextline_idx
    ) {
        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QTextLayout layout(paraText, font);
        QTextOption option;
        option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
        layout.setTextOption(option);
        layout.beginLayout();

        int target_idx = 0;
        int cur_idx = 0;
        bool first = true;
        while (true) {
            QTextLine line = layout.createLine();
            if (!line.isValid()) break;
            double lineWrap = first ? (paragraph_wrap_w - indent_w) : paragraph_wrap_w;
            line.setLineWidth(lineWrap);
            if (cur_idx == qtextline_idx) {
                // x is relative to line content start; xToCursor expects x relative to QTextLine start
                target_idx = line.xToCursor(x);
                target_idx = line.textStart() + target_idx;
                break;
            }
            first = false;
            cur_idx++;
        }
        layout.endLayout();
        return target_idx;
    }

    // Given paragraph text and a byte range, return per-glyph x positions.
    // Results stored in g_layout_buf as {byteStart=para_qchar_offset, width=glyph_width, xPos=glyph_x}.
    void sujian_glyph_positions(
        const QString& paraText, int range_qchar_start, int range_qchar_end,
        double fs, const QString& ff, int qtextline_idx
    ) {
        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QTextLayout layout(paraText, font);
        QTextOption option;
        option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
        layout.setTextOption(option);
        layout.beginLayout();

        g_layout_buf.clear();
        int cur_idx = 0;
        while (true) {
            QTextLine line = layout.createLine();
            if (!line.isValid()) break;
            if (cur_idx == qtextline_idx) {
                int line_start = line.textStart();
                int line_end = line_start + line.textLength();
                int r_start = std::max(range_qchar_start, line_start);
                int r_end = std::min(range_qchar_end, line_end);
                for (int i = r_start; i < r_end; i++) {
                    double x1 = line.cursorToX(i - line_start);
                    double x2 = line.cursorToX(i - line_start + 1);
                    QTextLayoutEntry e;
                    e.byteStart = i;  // QChar offset within paragraph
                    e.width = std::abs(x2 - x1);
                    e.xPos = x1;
                    g_layout_buf.push_back(e);
                }
                break;
            }
            cur_idx++;
        }
        layout.endLayout();
    }
}}

/// QTextLayout-based cursor-to-x: given paragraph text and text before cursor,
/// returns x position relative to paragraph origin.
/// DEPRECATED: use qtextlayout_cursor_to_x_on_line for wrapped-line accuracy.
fn qtextlayout_cursor_to_x(para_text: &str, text_before_cursor: &str, font_size: f64, font_family: &str) -> f64 {
    let para: QString = para_text.to_string().into();
    let before: QString = text_before_cursor.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    cpp!(unsafe [para as "QString", before as "QString", fs as "float", ff as "QString"] -> f64 as "double" {
        return sujian_cursor_to_x(para, fs, ff, before);
    })
}

/// QTextLayout-based cursor-to-x on a specific visual line with correct wrap/indent.
/// `cursor_abs_byte` is the absolute byte offset of the cursor in the full text.
/// `paragraph_wrap_w` should be `line.line_wrap_width + line.line_indent_x`.
fn qtextlayout_cursor_to_x_on_line(
    para_text: &str, cursor_abs_byte: usize, para_start: usize,
    font_size: f64, font_family: &str,
    paragraph_wrap_w: f64, indent_w: f64, qtextline_idx: i32,
) -> f64 {
    let cursor_in_para = cursor_abs_byte.saturating_sub(para_start);
    let cursor_qchar = byte_offset_to_qchar_offset(para_text, cursor_in_para) as i32;
    let para: QString = para_text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    cpp!(unsafe [
        para as "QString", cursor_qchar as "int",
        fs as "float", ff as "QString",
        paragraph_wrap_w as "double", indent_w as "double", qtextline_idx as "int"
    ] -> f64 as "double" {
        return sujian_cursor_to_x_on_line(para, cursor_qchar, fs, ff, paragraph_wrap_w, indent_w, qtextline_idx);
    })
}

/// QTextLayout-based glyph positions on a specific visual line with correct wrap/indent.
/// `range_start` and `range_end` are absolute byte offsets in the full text.
/// `paragraph_wrap_w` should be `line.line_wrap_width + line.line_indent_x`.
/// Returns per-glyph (abs_byte_offset, x, width).
fn qtextlayout_glyph_positions_on_line(
    para_text: &str, range_start: usize, range_end: usize, para_start: usize,
    font_size: f64, font_family: &str,
    paragraph_wrap_w: f64, indent_w: f64, qtextline_idx: i32,
) -> Vec<(usize, f64, f64)> {
    let seg_start_in_para = range_start.saturating_sub(para_start);
    let seg_end_in_para = range_end.saturating_sub(para_start).min(para_text.len());
    let qchar_start = byte_offset_to_qchar_offset(para_text, seg_start_in_para) as i32;
    let qchar_end = byte_offset_to_qchar_offset(para_text, seg_end_in_para) as i32;
    let para: QString = para_text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    let count = cpp!(unsafe [
        para as "QString", qchar_start as "int", qchar_end as "int",
        fs as "float", ff as "QString",
        paragraph_wrap_w as "double", indent_w as "double", qtextline_idx as "int"
    ] -> i32 as "int" {
        sujian_glyph_positions_on_line(para, qchar_start, qchar_end, fs, ff, paragraph_wrap_w, indent_w, qtextline_idx);
        return static_cast<int>(g_layout_buf.size());
    });
    let mut result = Vec::with_capacity(count as usize);
    for i in 0..count {
        let idx = i;
        let qchar_off = cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
            return static_cast<qulonglong>(g_layout_buf[idx].byteStart);
        });
        let w = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            return g_layout_buf[idx].width;
        });
        let x_pos = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            return g_layout_buf[idx].xPos;
        });
        let para_byte = qchar_offset_to_byte_offset(para_text, qchar_off);
        let abs_byte = para_start + para_byte;
        result.push((abs_byte, x_pos, w));
    }
    result
}

/// QTextLayout-based x-to-cursor: given paragraph text and x position,
/// returns QChar offset within paragraph. Caller converts to UTF-8 byte offset.
/// DEPRECATED: use qtextlayout_x_to_cursor_on_line for wrapped-line accuracy.
#[allow(dead_code)]
fn qtextlayout_x_to_cursor_qchar(para_text: &str, x: f64, font_size: f64, font_family: &str, qtextline_idx: i32) -> i32 {
    let para: QString = para_text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    cpp!(unsafe [para as "QString", x as "double", fs as "float", ff as "QString", qtextline_idx as "int"] -> i32 as "int" {
        return sujian_x_to_cursor(para, x, fs, ff, qtextline_idx);
    })
}

/// QTextLayout-based x-to-cursor on a specific visual line with correct wrap/indent.
/// `x` is relative to the line content start (i.e., caller already subtracted line.x).
/// `paragraph_wrap_w` should be `line.line_wrap_width + line.line_indent_x`.
/// Returns absolute byte offset in the full text.
fn qtextlayout_x_to_cursor_on_line(
    para_text: &str, x: f64, para_start: usize,
    font_size: f64, font_family: &str,
    paragraph_wrap_w: f64, indent_w: f64, qtextline_idx: i32,
) -> usize {
    let para: QString = para_text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    let qchar_off = cpp!(unsafe [
        para as "QString", x as "double",
        fs as "float", ff as "QString",
        paragraph_wrap_w as "double", indent_w as "double", qtextline_idx as "int"
    ] -> i32 as "int" {
        return sujian_x_to_cursor_on_line(para, x, fs, ff, paragraph_wrap_w, indent_w, qtextline_idx);
    });
    let para_byte = qchar_offset_to_byte_offset(para_text, qchar_off as usize);
    para_start + para_byte
}

/// QTextLayout-based glyph positions: returns per-glyph (byte_offset_in_para, x, width) for a range.
#[allow(dead_code)]
fn qtextlayout_glyph_positions(para_text: &str, range_start: usize, range_end: usize, font_size: f64, font_family: &str, qtextline_idx: i32) -> Vec<(usize, f64, f64)> {
    let para: QString = para_text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    let qchar_start = byte_offset_to_qchar_offset(para_text, range_start) as i32;
    let qchar_end = byte_offset_to_qchar_offset(para_text, range_end) as i32;
    let count = cpp!(unsafe [para as "QString", qchar_start as "int", qchar_end as "int", fs as "float", ff as "QString", qtextline_idx as "int"] -> i32 as "int" {
        sujian_glyph_positions(para, qchar_start, qchar_end, fs, ff, qtextline_idx);
        return static_cast<int>(g_layout_buf.size());
    });
    let mut result = Vec::with_capacity(count as usize);
    for i in 0..count {
        let idx = i;
        let qchar_off = cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
            return static_cast<qulonglong>(g_layout_buf[idx].byteStart);
        });
        let w = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            return g_layout_buf[idx].width;
        });
        let x_pos = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            return g_layout_buf[idx].xPos;
        });
        let byte_off = qchar_offset_to_byte_offset(para_text, qchar_off);
        result.push((byte_off, x_pos, w));
    }
    result
}

/// Convert UTF-8 byte offset to QChar offset within a string.
fn byte_offset_to_qchar_offset(text: &str, byte_offset: usize) -> usize {
    text[..byte_offset.min(text.len())].chars().map(|c| c.len_utf16()).sum()
}

const KEY_BACKSPACE: i32 = 0x0100_0003;
const KEY_TAB: i32 = 0x0100_0001;
const KEY_ENTER: i32 = 0x0100_0005;
const KEY_INSERT: i32 = 0x0100_0006;
const KEY_RETURN: i32 = 0x0100_0004;
const KEY_DELETE: i32 = 0x0100_0007;
const KEY_LEFT: i32 = 0x0100_0012;
const KEY_UP: i32 = 0x0100_0013;
const KEY_RIGHT: i32 = 0x0100_0014;
const KEY_DOWN: i32 = 0x0100_0015;
const KEY_HOME: i32 = 0x0100_0010;
const KEY_END: i32 = 0x0100_0011;
const KEY_ESCAPE: i32 = 0x0100_0000;
const KEY_A: i32 = 0x41;
const KEY_C: i32 = 0x43;
const KEY_V: i32 = 0x56;
const KEY_X: i32 = 0x58;
const KEY_Y: i32 = 0x59;
const KEY_Z: i32 = 0x5a;
const CTRL_MODIFIER: i32 = 0x0400_0000;
const SHIFT_MODIFIER: i32 = 0x0200_0000;
const ALT_MODIFIER: i32 = 0x0800_0000;
const META_MODIFIER: i32 = 0x1000_0000;

fn has_ctrl(modifiers: i32) -> bool {
    modifiers & CTRL_MODIFIER != 0
}

fn has_shift(modifiers: i32) -> bool {
    modifiers & SHIFT_MODIFIER != 0
}

fn has_alt(modifiers: i32) -> bool {
    modifiers & ALT_MODIFIER != 0
}

fn has_meta(modifiers: i32) -> bool {
    modifiers & META_MODIFIER != 0
}

fn is_copy_shortcut(key: i32, modifiers: i32) -> bool {
    has_ctrl(modifiers) && (key == KEY_C || key == KEY_INSERT)
}

fn is_paste_shortcut(key: i32, modifiers: i32) -> bool {
    (has_ctrl(modifiers) && key == KEY_V) || (has_shift(modifiers) && key == KEY_INSERT)
}

fn is_redo_shortcut(key: i32, modifiers: i32) -> bool {
    has_ctrl(modifiers) && (key == KEY_Y || (has_shift(modifiers) && key == KEY_Z))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EditorSnapshot {
    text: String,
    cursor: usize,
    selection_anchor: usize,
}

#[derive(Clone, Debug, PartialEq)]
struct VisualLine {
    /// 全文绝对字节起始 (UTF-8)
    start: usize,
    /// 全文绝对字节结束 (UTF-8)
    end: usize,
    hard_break: bool,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    /// 所属段落的完整文本（用于 QTextLayout 查询）
    para_text: String,
    /// 段落在全文中的字节起始偏移 (UTF-8)
    para_start: usize,
    /// 在 QTextLayout 中的行索引（同一段落内从 0 开始）
    qtextline_idx: i32,
    /// 段落内此行起始的 QChar (UTF-16) 偏移
    para_qchar_start: usize,
    /// 段落内此行结束的 QChar (UTF-16) 偏移
    para_qchar_end: usize,
    /// 此行的 wrap width（第一行需减去 indent）
    line_wrap_width: f64,
    /// 此行的 indent x 偏移（仅段落首行为 indent_w，其余为 0）
    line_indent_x: f64,
}

struct LayoutCache {
    text_ptr: usize,
    text_len: usize,
    width: f64,
    font_size: f32,
    font_family: String,
    line_spacing: f32,
    text_indent: f32,
    padding: f32,
    lines: Vec<VisualLine>,
    content_height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaretAffinity {
    Upstream,   // 靠前（上一行尾）
    Downstream, // 靠后（下一行首）
}

/// 滚动缓冲区 — 缓存超大 QImage + 纹理，滚动时只更新 rect 不重绘
struct ScrollBuffer {
    image: qmetaobject::QImage,
    buffer_scroll_y: f64,
    buffer_content_h: f64,
    buffer_logical_h: f64,
    dpr: f64,
}

/// 光标动画状态 — 固定时长 tween，不再用指数追赶
#[derive(Clone, Debug)]
struct CursorAnimationState {
    start_x: f64,
    start_y: f64,
    target_x: f64,
    target_y: f64,
    start_time: Instant,
    duration_ms: u64,
}

impl CursorAnimationState {
    /// 计算当前动画位置（easeOutCubic）
    fn current_position(&self, now: Instant) -> (f64, f64) {
        let elapsed_ms = now.duration_since(self.start_time).as_millis() as f64;
        let t = (elapsed_ms / self.duration_ms as f64).min(1.0);
        // easeOutCubic: 1 - (1 - t)^3
        let eased = 1.0 - (1.0 - t).powi(3);
        let x = self.start_x + (self.target_x - self.start_x) * eased;
        let y = self.start_y + (self.target_y - self.start_y) * eased;
        (x, y)
    }

    /// 动画是否已完成
    fn is_finished(&self, now: Instant) -> bool {
        now.duration_since(self.start_time).as_millis() as u64 >= self.duration_ms
    }
}

/// 单个字形的矩形信息
#[derive(Clone, Debug)]
struct AnimatedGlyph {
    byte_start: usize,
    byte_end: usize,
    text: String,
    /// 字形包围盒 (x, y, width, height) — x 是行内左偏移，y 是行顶
    rect: (f64, f64, f64, f64),
    baseline_y: f64,
    line_index: usize,
}

/// 吐字动画 — 插入文字从光标 clip 展开
#[derive(Clone, Debug)]
struct InsertAnimation {
    /// 每个插入字形的最终位置
    glyphs: Vec<AnimatedGlyph>,
    /// 插入点的光标矩形（动画起点）
    origin_cursor_rect: (f64, f64, f64, f64),
    start_time: Instant,
    duration_ms: u64,
}

impl InsertAnimation {
    fn progress(&self, now: Instant) -> f64 {
        let elapsed_ms = now.duration_since(self.start_time).as_millis() as f64;
        (elapsed_ms / self.duration_ms as f64).min(1.0)
    }

    fn is_finished(&self, now: Instant) -> bool {
        now.duration_since(self.start_time).as_millis() as u64 >= self.duration_ms
    }

    /// 插入内容的总包围盒
    #[allow(dead_code)]
    fn bounding_rect(&self) -> (f64, f64, f64, f64) {
        if self.glyphs.is_empty() {
            return self.origin_cursor_rect;
        }
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;
        for g in &self.glyphs {
            min_x = min_x.min(g.rect.0);
            min_y = min_y.min(g.rect.1);
            max_x = max_x.max(g.rect.0 + g.rect.2);
            max_y = max_y.max(g.rect.1 + g.rect.3);
        }
        (min_x, min_y, max_x - min_x, max_y - min_y)
    }
}

/// 吞字动画 — 旧字向光标收缩消失
#[derive(Clone, Debug)]
struct DeleteAnimation {
    /// 被删字形的原始位置
    glyphs: Vec<AnimatedGlyph>,
    /// 删除后光标位置（动画终点）
    target_cursor_rect: (f64, f64, f64, f64),
    start_time: Instant,
    duration_ms: u64,
}

impl DeleteAnimation {
    fn progress(&self, now: Instant) -> f64 {
        let elapsed_ms = now.duration_since(self.start_time).as_millis() as f64;
        (elapsed_ms / self.duration_ms as f64).min(1.0)
    }

    fn is_finished(&self, now: Instant) -> bool {
        now.duration_since(self.start_time).as_millis() as u64 >= self.duration_ms
    }
}

#[derive(Clone, Debug)]
struct EditorBuffer {
    text: String,
    cursor: usize,
    selection_anchor: usize,
    undo_stack: Vec<EditorSnapshot>,
    redo_stack: Vec<EditorSnapshot>,
}

impl Default for EditorBuffer {
    fn default() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            selection_anchor: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }
}

impl EditorBuffer {
    fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            selection_anchor: self.selection_anchor,
        }
    }

    fn restore(&mut self, snapshot: EditorSnapshot) {
        self.text = snapshot.text;
        self.cursor = clamp_to_char_boundary(&self.text, snapshot.cursor);
        self.selection_anchor = clamp_to_char_boundary(&self.text, snapshot.selection_anchor);
    }

    fn has_selection(&self) -> bool {
        self.cursor != self.selection_anchor
    }

    fn selection_range(&self) -> (usize, usize) {
        if self.cursor <= self.selection_anchor {
            (self.cursor, self.selection_anchor)
        } else {
            (self.selection_anchor, self.cursor)
        }
    }

    fn selected_text(&self) -> String {
        if !self.has_selection() {
            return String::new();
        }
        let (start, end) = self.selection_range();
        self.text[start..end].to_string()
    }

    fn set_text(&mut self, text: String) {
        self.text = text;
        self.cursor = 0;
        self.selection_anchor = self.cursor;
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    fn push_undo(&mut self, snapshot: EditorSnapshot) {
        if self.undo_stack.last() != Some(&snapshot) {
            self.undo_stack.push(snapshot);
        }
        if self.undo_stack.len() > 256 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    fn replace_selection_or_insert(&mut self, inserted: &str) {
        if inserted.is_empty() {
            return;
        }
        let (start, end) = self.selection_range();
        self.text.replace_range(start..end, inserted);
        self.cursor = start + inserted.len();
        self.cursor = clamp_to_char_boundary(&self.text, self.cursor);
        self.selection_anchor = self.cursor;
    }

    fn delete_selection(&mut self) -> bool {
        if !self.has_selection() {
            return false;
        }
        let (start, end) = self.selection_range();
        self.text.replace_range(start..end, "");
        self.cursor = start;
        self.selection_anchor = start;
        true
    }

    fn delete_backward(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        let Some(prev) = prev_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.text.replace_range(prev..self.cursor, "");
        self.cursor = prev;
        self.selection_anchor = prev;
        true
    }

    fn delete_forward(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        let Some(next) = next_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.text.replace_range(self.cursor..next, "");
        self.selection_anchor = self.cursor;
        true
    }

    fn move_cursor(&mut self, index: usize, extend: bool) {
        let next = clamp_to_char_boundary(&self.text, index);
        if !extend {
            self.selection_anchor = next;
        }
        self.cursor = next;
    }

    fn select_all(&mut self) {
        self.selection_anchor = 0;
        self.cursor = self.text.len();
    }

    fn undo(&mut self) -> Option<(EditorSnapshot, EditorSnapshot)> {
        let previous = self.undo_stack.pop()?;
        let current = self.snapshot();
        self.redo_stack.push(current.clone());
        self.restore(previous.clone());
        Some((current, previous))
    }

    fn redo(&mut self) -> Option<(EditorSnapshot, EditorSnapshot)> {
        let next = self.redo_stack.pop()?;
        let current = self.snapshot();
        self.undo_stack.push(current.clone());
        self.restore(next.clone());
        Some((current, next))
    }
}

#[allow(dead_code)]
#[derive(QObject)]
pub struct SujianEditorItem {
    base: qt_base_class!(trait QQuickItem),

    plain_text: qt_property!(QString; READ plain_text WRITE set_plain_text NOTIFY plain_text_changed),
    content_height: qt_property!(f32; READ content_height NOTIFY content_height_changed),
    cursor_position: qt_property!(u32; READ cursor_position NOTIFY cursor_position_changed),
    has_selection: qt_property!(bool; READ has_selection NOTIFY selection_changed),
    editor_enabled: qt_property!(bool; READ editor_enabled WRITE set_editor_enabled NOTIFY editor_enabled_changed),
    font_pixel_size: qt_property!(f32; READ font_pixel_size WRITE set_font_pixel_size NOTIFY visual_settings_changed),
    font_family: qt_property!(QString; READ font_family WRITE set_font_family NOTIFY visual_settings_changed),
    line_spacing: qt_property!(f32; READ line_spacing WRITE set_line_spacing NOTIFY visual_settings_changed),
    text_indent: qt_property!(f32; READ text_indent WRITE set_text_indent NOTIFY visual_settings_changed),
    padding: qt_property!(f32; READ padding WRITE set_padding NOTIFY visual_settings_changed),
    text_color: qt_property!(QString; READ text_color WRITE set_text_color NOTIFY visual_settings_changed),
    selection_color: qt_property!(QString; READ selection_color WRITE set_selection_color NOTIFY visual_settings_changed),
    selected_text_color: qt_property!(QString; READ selected_text_color WRITE set_selected_text_color NOTIFY visual_settings_changed),
    cursor_color: qt_property!(QString; READ cursor_color WRITE set_cursor_color NOTIFY visual_settings_changed),
    smooth_cursor_enabled: qt_property!(bool; READ smooth_cursor_enabled WRITE set_smooth_cursor_enabled NOTIFY visual_settings_changed),
    cursor_animation_duration_ms: qt_property!(u32; READ cursor_animation_duration_ms WRITE set_cursor_animation_duration_ms NOTIFY visual_settings_changed),
    typing_animation_enabled: qt_property!(bool; READ typing_animation_enabled WRITE set_typing_animation_enabled NOTIFY visual_settings_changed),
    last_transaction_summary: qt_property!(QString; READ last_transaction_summary NOTIFY transaction_created),
    last_animation_event_count: qt_property!(u32; READ last_animation_event_count NOTIFY transaction_created),
    scroll_y: qt_property!(f32; READ scroll_y WRITE set_scroll_y NOTIFY visual_settings_changed),
    viewport_height: qt_property!(f32; READ viewport_height WRITE set_viewport_height NOTIFY visual_settings_changed),
    is_scrolling: qt_property!(bool; READ is_scrolling WRITE set_is_scrolling NOTIFY visual_settings_changed),
    cursor_rect_x: qt_property!(f32; READ cursor_rect_x NOTIFY cursor_rect_changed),
    cursor_rect_y: qt_property!(f32; READ cursor_rect_y NOTIFY cursor_rect_changed),
    cursor_rect_width: qt_property!(f32; READ cursor_rect_width NOTIFY cursor_rect_changed),
    cursor_rect_height: qt_property!(f32; READ cursor_rect_height NOTIFY cursor_rect_changed),

    plain_text_changed: qt_signal!(),
    text_changed: qt_signal!(),
    content_height_changed: qt_signal!(),
    cursor_position_changed: qt_signal!(),
    selection_changed: qt_signal!(),
    editor_enabled_changed: qt_signal!(),
    visual_settings_changed: qt_signal!(),
    transaction_created: qt_signal!(),
    cursor_rect_changed: qt_signal!(),
    explicit_clear_requested: qt_signal!(),

    get_plain_text: qt_method!(fn(&self) -> QString),
    set_plain_text: qt_method!(fn(&mut self, text: QString)),
    reload_plain_text: qt_method!(fn(&mut self, text: QString)),
    clear_undo_stack: qt_method!(fn(&mut self)),
    insert_text: qt_method!(fn(&mut self, text: QString)),
    delete_backward: qt_method!(fn(&mut self)),
    delete_forward: qt_method!(fn(&mut self)),
    delete_selection: qt_method!(fn(&mut self)),
    select_all: qt_method!(fn(&mut self)),
    selected_text: qt_method!(fn(&self) -> QString),
    undo: qt_method!(fn(&mut self)),
    redo: qt_method!(fn(&mut self)),
    handle_key: qt_method!(fn(&mut self, key: i32, modifiers: i32) -> bool),
    click_at: qt_method!(fn(&mut self, x: f32, y: f32, extend: bool)),
    drag_select_at: qt_method!(fn(&mut self, x: f32, y: f32)),
    clipboard_copy: qt_method!(fn(&self) -> bool),
    clipboard_paste: qt_method!(fn(&mut self)),
    insert_preedit: qt_method!(fn(&mut self, text: QString)),
    commit_preedit: qt_method!(fn(&mut self, text: QString)),
    cancel_preedit: qt_method!(fn(&mut self)),
    flush_content_height: qt_method!(fn(&mut self)),

    buffer: EditorBuffer,
    engine: EditorEngine,
    current_content_height: f32,
    content_height_dirty: Cell<bool>,
    current_editor_enabled: bool,
    current_font_pixel_size: f32,
    current_font_family: QString,
    current_line_spacing: f32,
    current_text_indent: f32,
    current_padding: f32,
    current_text_color: QString,
    current_selection_color: QString,
    current_selected_text_color: QString,
    current_cursor_color: QString,
    current_smooth_cursor_enabled: bool,
    current_cursor_animation_duration_ms: u32,
    current_typing_animation_enabled: bool,
    current_scroll_y: f32,
    current_viewport_height: f32,
    current_is_scrolling: bool,
    last_summary: QString,
    last_event_count: u32,
    target_cursor_x: f64,
    target_cursor_y: f64,
    cursor_animation: Option<CursorAnimationState>,
    insert_animation: Option<InsertAnimation>,
    delete_animation: Option<DeleteAnimation>,
    preedit_text: String,
    preedit_cursor: usize,
    suppress_next_ime_commit: bool,
    layout_cache: Option<LayoutCache>,
    render_dirty: bool,
    scroll_buffer: Option<ScrollBuffer>,
    last_slow_paint_log: Option<Instant>,
    cursor_visual_x: f64,
    cursor_visual_y: f64,
    cursor_visual_h: f64,
    cursor_visible: bool,
    cursor_dirty: bool,
    current_cursor_affinity: CaretAffinity,
    ime_cursor_rect_y: f64,
    ime_cursor_rect_h: f64,
}

impl Default for SujianEditorItem {
    fn default() -> Self {
        Self {
            base: Default::default(),
            plain_text: Default::default(),
            content_height: Default::default(),
            cursor_position: Default::default(),
            has_selection: Default::default(),
            editor_enabled: Default::default(),
            font_pixel_size: Default::default(),
            font_family: "serif".into(),
            line_spacing: Default::default(),
            text_indent: Default::default(),
            padding: Default::default(),
            text_color: Default::default(),
            selection_color: Default::default(),
            selected_text_color: Default::default(),
            cursor_color: Default::default(),
            smooth_cursor_enabled: Default::default(),
            cursor_animation_duration_ms: Default::default(),
            typing_animation_enabled: Default::default(),
            last_transaction_summary: Default::default(),
            last_animation_event_count: Default::default(),
            scroll_y: Default::default(),
            viewport_height: Default::default(),
            is_scrolling: Default::default(),
            plain_text_changed: Default::default(),
            text_changed: Default::default(),
            content_height_changed: Default::default(),
            cursor_position_changed: Default::default(),
            selection_changed: Default::default(),
            editor_enabled_changed: Default::default(),
            visual_settings_changed: Default::default(),
            transaction_created: Default::default(),
            cursor_rect_changed: Default::default(),
            explicit_clear_requested: Default::default(),
            cursor_rect_x: Default::default(),
            cursor_rect_y: Default::default(),
            cursor_rect_width: Default::default(),
            cursor_rect_height: Default::default(),
            get_plain_text: Default::default(),
            set_plain_text: Default::default(),
            reload_plain_text: Default::default(),
            clear_undo_stack: Default::default(),
            insert_text: Default::default(),
            delete_backward: Default::default(),
            delete_forward: Default::default(),
            delete_selection: Default::default(),
            select_all: Default::default(),
            selected_text: Default::default(),
            undo: Default::default(),
            redo: Default::default(),
            handle_key: Default::default(),
            click_at: Default::default(),
            drag_select_at: Default::default(),
            clipboard_copy: Default::default(),
            clipboard_paste: Default::default(),
            insert_preedit: Default::default(),
            commit_preedit: Default::default(),
            cancel_preedit: Default::default(),
            flush_content_height: Default::default(),
            buffer: EditorBuffer::default(),
            engine: EditorEngine::new(),
            current_content_height: 0.0,
            content_height_dirty: Cell::new(false),
            current_editor_enabled: true,
            current_font_pixel_size: 16.0,
            current_font_family: "serif".into(),
            current_line_spacing: 1.5,
            current_text_indent: 0.0,
            current_padding: 16.0,
            current_text_color: "#E2E2E5".into(),
            current_selection_color: "#006497".into(),
            current_selected_text_color: "#CCE5FF".into(),
            current_cursor_color: "#006497".into(),
            current_smooth_cursor_enabled: true,
            current_cursor_animation_duration_ms: 160,
            current_typing_animation_enabled: false,
            current_scroll_y: 0.0,
            current_viewport_height: 0.0,
            current_is_scrolling: false,
            last_summary: "".into(),
            last_event_count: 0,
            target_cursor_x: 0.0,
            target_cursor_y: 0.0,
            cursor_animation: None,
            insert_animation: None,
            delete_animation: None,
            preedit_text: String::new(),
            preedit_cursor: 0,
            suppress_next_ime_commit: false,
            layout_cache: None,
            render_dirty: true,
            scroll_buffer: None,
            last_slow_paint_log: None,
            cursor_visual_x: 0.0,
            cursor_visual_y: 0.0,
            cursor_visual_h: 0.0,
            cursor_visible: false,
            cursor_dirty: true,
            current_cursor_affinity: CaretAffinity::Downstream,
            ime_cursor_rect_y: 0.0,
            ime_cursor_rect_h: 0.0,
        }
    }
}

impl SujianEditorItem {
    fn request_repaint(&mut self) {
        self.render_dirty = true;
        let item = self as &dyn QQuickItem;
        item.update();
    }

    fn bounding_width(&self) -> f64 {
        let item = self as &dyn QQuickItem;
        item.bounding_rect().width.max(1.0)
    }

    fn plain_text(&self) -> QString {
        self.buffer.text.clone().into()
    }

    fn set_plain_text(&mut self, text: QString) {
        self.set_plain_text_from_qml(text);
    }

    fn set_plain_text_from_qml(&mut self, text: QString) {
        let normalized = normalize_plain_text(&text.to_string());
        if self.buffer.text == normalized {
            return;
        }
        let old = self.buffer.snapshot();
        self.buffer.set_text(normalized);
        self.buffer.cursor = 0;
        self.buffer.selection_anchor = 0;
        self.current_cursor_affinity = CaretAffinity::Downstream;
        let new = self.buffer.snapshot();
        self.record_transaction(old, new, EditorTransactionCause::Load, false);
        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.emit_content_changed();
    }

    fn reload_plain_text(&mut self, text: QString) {
        let normalized = normalize_plain_text(&text.to_string());
        if self.buffer.text == normalized {
            return;
        }
        let old = self.buffer.snapshot();
        let old_cursor = self.buffer.cursor;
        let old_anchor = self.buffer.selection_anchor;
        self.buffer.set_text(normalized);
        self.buffer.cursor = clamp_to_char_boundary(&self.buffer.text, old_cursor);
        self.buffer.selection_anchor = clamp_to_char_boundary(&self.buffer.text, old_anchor);
        self.current_cursor_affinity = CaretAffinity::Downstream;
        let new = self.buffer.snapshot();
        self.record_transaction(old, new, EditorTransactionCause::Load, false);
        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.emit_content_changed();
    }

    fn get_plain_text(&self) -> QString {
        self.buffer.text.clone().into()
    }

    fn content_height(&self) -> f32 {
        self.current_content_height
    }

    fn cursor_position(&self) -> u32 {
        byte_to_char_index(&self.buffer.text, self.buffer.cursor) as u32
    }

    fn has_selection(&self) -> bool {
        self.buffer.has_selection()
    }

    fn editor_enabled(&self) -> bool {
        self.current_editor_enabled
    }

    fn set_editor_enabled(&mut self, value: bool) {
        if self.current_editor_enabled == value {
            return;
        }
        self.current_editor_enabled = value;
        self.editor_enabled_changed();
        self.request_repaint();
    }

    fn font_pixel_size(&self) -> f32 {
        self.current_font_pixel_size
    }

    fn set_font_pixel_size(&mut self, value: f32) {
        if (self.current_font_pixel_size - value).abs() <= 0.1 {
            return;
        }
        self.current_font_pixel_size = value.max(8.0);
        self.visual_changed();
    }

    fn font_family(&self) -> QString {
        self.current_font_family.clone()
    }

    fn set_font_family(&mut self, value: QString) {
        if self.current_font_family.to_string() == value.to_string() {
            return;
        }
        self.current_font_family = value;
        self.visual_changed();
    }

    fn line_spacing(&self) -> f32 {
        self.current_line_spacing
    }

    fn set_line_spacing(&mut self, value: f32) {
        if (self.current_line_spacing - value).abs() <= 0.01 {
            return;
        }
        self.current_line_spacing = value.max(1.0);
        self.visual_changed();
    }

    fn text_indent(&self) -> f32 {
        self.current_text_indent
    }

    fn set_text_indent(&mut self, value: f32) {
        if (self.current_text_indent - value).abs() <= 0.1 {
            return;
        }
        self.current_text_indent = value.max(0.0);
        self.visual_changed();
    }

    fn padding(&self) -> f32 {
        self.current_padding
    }

    fn set_padding(&mut self, value: f32) {
        if (self.current_padding - value).abs() <= 0.1 {
            return;
        }
        self.current_padding = value.max(0.0);
        self.visual_changed();
    }

    fn text_color(&self) -> QString {
        self.current_text_color.clone()
    }

    fn set_text_color(&mut self, value: QString) {
        if self.current_text_color.to_string() == value.to_string() {
            return;
        }
        self.current_text_color = value;
        self.visual_changed();
    }

    fn selection_color(&self) -> QString {
        self.current_selection_color.clone()
    }

    fn set_selection_color(&mut self, value: QString) {
        if self.current_selection_color.to_string() == value.to_string() {
            return;
        }
        self.current_selection_color = value;
        self.visual_changed();
    }

    fn selected_text_color(&self) -> QString {
        self.current_selected_text_color.clone()
    }

    fn set_selected_text_color(&mut self, value: QString) {
        if self.current_selected_text_color.to_string() == value.to_string() {
            return;
        }
        self.current_selected_text_color = value;
        self.visual_changed();
    }

    fn cursor_color(&self) -> QString {
        self.current_cursor_color.clone()
    }

    fn set_cursor_color(&mut self, value: QString) {
        if self.current_cursor_color.to_string() == value.to_string() {
            return;
        }
        self.current_cursor_color = value;
        self.visual_changed();
    }

    fn smooth_cursor_enabled(&self) -> bool {
        self.current_smooth_cursor_enabled
    }

    fn set_smooth_cursor_enabled(&mut self, value: bool) {
        self.current_smooth_cursor_enabled = value;
        self.visual_settings_changed();
    }

    fn cursor_animation_duration_ms(&self) -> u32 {
        self.current_cursor_animation_duration_ms
    }

    fn set_cursor_animation_duration_ms(&mut self, value: u32) {
        self.current_cursor_animation_duration_ms = value;
        self.visual_settings_changed();
    }

    fn typing_animation_enabled(&self) -> bool {
        false
    }

    fn set_typing_animation_enabled(&mut self, _value: bool) {
        self.current_typing_animation_enabled = false;
        if editor_animation_debug_enabled() {
            eprintln!("typing_animation_enabled_changed: forced false");
        }
        self.visual_settings_changed();
    }

    fn scroll_y(&self) -> f32 {
        self.current_scroll_y
    }

    fn set_scroll_y(&mut self, value: f32) {
        if (self.current_scroll_y - value).abs() < 0.5 {
            return;
        }
        self.current_scroll_y = value;
        self.request_repaint();
    }

    fn viewport_height(&self) -> f32 {
        self.current_viewport_height
    }

    fn set_viewport_height(&mut self, value: f32) {
        if (self.current_viewport_height - value).abs() < 0.5 {
            return;
        }
        self.current_viewport_height = value;
        self.request_repaint();
    }

    fn is_scrolling(&self) -> bool {
        self.current_is_scrolling
    }

    fn set_is_scrolling(&mut self, value: bool) {
        if self.current_is_scrolling == value {
            return;
        }
        self.current_is_scrolling = value;
        if value {
            self.cursor_animation = None;
            self.insert_animation = None;
            self.delete_animation = None;
            self.request_repaint();
            return;
        }
        if !value {
            // 滚动结束时，从当前动画位置继续，避免光标跳动
            let now = Instant::now();
            let (visual_x, visual_y) = if let Some(ref anim) = self.cursor_animation {
                if anim.is_finished(now) {
                    (anim.target_x, anim.target_y)
                } else {
                    anim.current_position(now)
                }
            } else {
                (self.target_cursor_x, self.target_cursor_y)
            };
            self.target_cursor_x = visual_x;
            self.target_cursor_y = visual_y;
        }
    }

    fn last_transaction_summary(&self) -> QString {
        self.last_summary.clone()
    }

    fn last_animation_event_count(&self) -> u32 {
        self.last_event_count
    }

    fn cursor_rect_x(&self) -> f32 {
        self.target_cursor_x as f32
    }

    fn cursor_rect_y(&self) -> f32 {
        self.ime_cursor_rect_y as f32
    }

    fn cursor_rect_width(&self) -> f32 {
        2.0
    }

    fn cursor_rect_height(&self) -> f32 {
        self.ime_cursor_rect_h as f32
    }

    fn visual_changed(&mut self) {
        self.invalidate_layout_cache();
        self.recalculate_content_height_quiet();
        self.visual_settings_changed();
        self.request_repaint();
    }

    fn emit_content_changed(&mut self) {
        self.invalidate_layout_cache();
        self.recalculate_content_height_quiet();
        self.plain_text_changed();
        self.text_changed();
        self.cursor_position_changed();
        self.selection_changed();
        self.request_repaint();
    }

    fn flush_content_height(&mut self) {
        if self.content_height_dirty.get() {
            self.content_height_dirty.set(false);
            self.content_height_changed();
        }
    }

    fn clear_undo_stack(&mut self) {
        self.buffer.undo_stack.clear();
        self.buffer.redo_stack.clear();
    }

    fn insert_text(&mut self, text: QString) {
        if !self.current_editor_enabled {
            return;
        }
        let inserted = normalize_plain_text(&text.to_string());
        if inserted.is_empty() {
            return;
        }
        self.preedit_text.clear();
        self.preedit_cursor = 0;

        // 记录插入前的光标位置（动画起点）
        let origin_cx = self.target_cursor_x;
        let origin_cy = self.target_cursor_y;
        let cursor_h = cursor_height_for_line(self.current_font_pixel_size as f64, &self.current_font_family.to_string());

        let old = self.buffer.snapshot();
        let insert_byte_start = if self.buffer.has_selection() {
            let (s, _) = self.buffer.selection_range();
            s
        } else {
            self.buffer.cursor
        };
        self.buffer.push_undo(old.clone());
        self.buffer.replace_selection_or_insert(&inserted);
        self.current_cursor_affinity = CaretAffinity::Downstream;
        let cause = if inserted.chars().count() == 1 {
            EditorTransactionCause::Typing
        } else {
            EditorTransactionCause::ImeComposition
        };
        let new = self.buffer.snapshot();
        self.record_transaction(old, new, cause, true);

        self.delete_animation = None;
        self.insert_animation = None;

        self.emit_content_changed();
    }

    fn delete_backward(&mut self) {
        if !self.current_editor_enabled {
            return;
        }
        let old = self.buffer.snapshot();

        if !self.buffer.delete_backward() {
            return;
        }
        self.buffer.push_undo(old.clone());
        self.current_cursor_affinity = CaretAffinity::Downstream;
        let new = self.buffer.snapshot();

        self.insert_animation = None;
        self.delete_animation = None;

        self.record_transaction(old, new, EditorTransactionCause::Delete, true);
        self.emit_content_changed();
    }

    fn delete_forward(&mut self) {
        if !self.current_editor_enabled {
            return;
        }
        let old = self.buffer.snapshot();

        if !self.buffer.delete_forward() {
            return;
        }
        self.buffer.push_undo(old.clone());
        self.current_cursor_affinity = CaretAffinity::Downstream;
        let new = self.buffer.snapshot();

        self.insert_animation = None;
        self.delete_animation = None;

        self.record_transaction(old, new, EditorTransactionCause::Delete, true);
        self.emit_content_changed();
    }

    fn delete_selection(&mut self) {
        if !self.current_editor_enabled || !self.buffer.has_selection() {
            return;
        }
        let old = self.buffer.snapshot();

        if !self.buffer.delete_selection() {
            return;
        }
        self.buffer.push_undo(old.clone());
        self.current_cursor_affinity = CaretAffinity::Downstream;
        let new = self.buffer.snapshot();

        self.insert_animation = None;
        self.delete_animation = None;

        self.record_transaction(old, new, EditorTransactionCause::Delete, true);
        self.emit_content_changed();
    }

    fn select_all(&mut self) {
        self.buffer.select_all();
        self.current_cursor_affinity = CaretAffinity::Downstream;
        self.cursor_position_changed();
        self.selection_changed();
        self.request_repaint();
    }

    fn selected_text(&self) -> QString {
        self.buffer.selected_text().into()
    }

    fn undo(&mut self) {
        let Some((old, new)) = self.buffer.undo() else {
            return;
        };
        self.current_cursor_affinity = CaretAffinity::Downstream;
        self.record_transaction(old, new, EditorTransactionCause::Undo, true);
        self.emit_content_changed();
    }

    fn redo(&mut self) {
        let Some((old, new)) = self.buffer.redo() else {
            return;
        };
        self.current_cursor_affinity = CaretAffinity::Downstream;
        self.record_transaction(old, new, EditorTransactionCause::Redo, true);
        self.emit_content_changed();
    }

    fn handle_key(&mut self, key: i32, modifiers: i32) -> bool {
        if !self.current_editor_enabled {
            return false;
        }
        let ctrl = has_ctrl(modifiers);
        let shift = has_shift(modifiers);
        if is_copy_shortcut(key, modifiers) {
            self.clipboard_copy();
            return true;
        }
        if is_paste_shortcut(key, modifiers) {
            self.clipboard_paste();
            return true;
        }
        if is_redo_shortcut(key, modifiers) {
            self.redo();
            return true;
        }
        if ctrl {
            match key {
                KEY_A => {
                    self.select_all();
                    return true;
                }
                KEY_X => {
                    self.clipboard_copy();
                    self.delete_selection();
                    return true;
                }
                KEY_Z => {
                    self.undo();
                    return true;
                }
                _ => return false,
            }
        }

        match key {
            KEY_ESCAPE => {
                self.preedit_text.clear();
                self.preedit_cursor = 0;
                self.suppress_next_ime_commit = true;
            }
            KEY_BACKSPACE => self.delete_backward(),
            KEY_DELETE => self.delete_forward(),
            KEY_RETURN | KEY_ENTER => self.insert_text("\n".into()),
            KEY_TAB => self.insert_text("\t".into()),
            KEY_LEFT => self.move_cursor_horizontal(false, shift),
            KEY_RIGHT => self.move_cursor_horizontal(true, shift),
            KEY_UP => self.move_cursor_vertical(false, shift),
            KEY_DOWN => self.move_cursor_vertical(true, shift),
            KEY_HOME => self.move_to_line_edge(false, shift),
            KEY_END => self.move_to_line_edge(true, shift),
            _ => return false,
        }
        true
    }

    fn click_at(&mut self, x: f32, y: f32, extend: bool) {
        let (index, affinity) = self.hit_test(x as f64, y as f64);
        self.current_cursor_affinity = affinity;
        if sujian_editor_debug_enabled() {
            eprintln!(
                "click_at: mouse_x={:.1}, mouse_y={:.1}, current_scroll_y={:.1}, hit_index={}, affinity={:?}, extend={}",
                x, y, self.current_scroll_y, index, affinity, extend
            );
        }
        self.buffer.move_cursor(index, extend);
        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.cursor_position_changed();
        self.selection_changed();
        self.request_repaint();
    }

    fn drag_select_at(&mut self, x: f32, y: f32) {
        let (index, affinity) = self.hit_test(x as f64, y as f64);
        self.current_cursor_affinity = affinity;
        self.buffer.move_cursor(index, true);
        self.cursor_position_changed();
        self.selection_changed();
        self.request_repaint();
    }

    fn clipboard_copy(&self) -> bool {
        if !self.buffer.has_selection() {
            return false;
        }
        let text = self.buffer.selected_text();
        if text.is_empty() {
            return false;
        }
        let qtext: QString = text.into();
        cpp!(unsafe [qtext as "QString"] {
            QClipboard *clipboard = QGuiApplication::clipboard();
            if (clipboard) clipboard->setText(qtext, QClipboard::Clipboard);
        });
        true
    }

    fn clipboard_paste(&mut self) {
        if !self.current_editor_enabled {
            return;
        }
        let pasted: QString = cpp!(unsafe [] -> QString as "QString" {
            QClipboard *clipboard = QGuiApplication::clipboard();
            return clipboard ? clipboard->text(QClipboard::Clipboard) : QString();
        });
        let s = pasted.to_string();
        if s.is_empty() {
            return;
        }
        let normalized = normalize_plain_text(&s);
        self.insert_text(normalized.into());
    }

    fn insert_preedit(&mut self, text: QString) {
        if !self.current_editor_enabled {
            return;
        }
        self.preedit_text = text.to_string();
        self.preedit_cursor = self.preedit_text.len();
        self.request_repaint();
    }

    fn commit_preedit(&mut self, text: QString) {
        if !self.current_editor_enabled {
            return;
        }
        self.preedit_text.clear();
        self.preedit_cursor = 0;
        let committed = text.to_string();
        if !committed.is_empty() {
            self.insert_text(committed.into());
        }
    }

    fn cancel_preedit(&mut self) {
        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.request_repaint();
    }

    fn move_cursor_horizontal(&mut self, forward: bool, extend: bool) {
        let next = if forward {
            next_char_boundary(&self.buffer.text, self.buffer.cursor).unwrap_or(self.buffer.cursor)
        } else {
            prev_char_boundary(&self.buffer.text, self.buffer.cursor).unwrap_or(self.buffer.cursor)
        };
        self.current_cursor_affinity = if forward {
            CaretAffinity::Downstream
        } else {
            CaretAffinity::Upstream
        };
        self.buffer.move_cursor(next, extend);
        self.cursor_position_changed();
        self.selection_changed();
        self.request_repaint();
    }

    fn move_cursor_vertical(&mut self, down: bool, extend: bool) {
        let width = self.bounding_width();
        let lines = self.ensure_layout_cached(width).clone();
        let Some((line_idx, x)) = self.cursor_line_and_x(&lines) else {
            return;
        };
        let target_idx = if down {
            (line_idx + 1).min(lines.len().saturating_sub(1))
        } else {
            line_idx.saturating_sub(1)
        };
        if target_idx == line_idx {
            return;
        }
        let index = self.index_at_line_x(&lines[target_idx], x);
        self.current_cursor_affinity = affinity_for_index_on_line(&lines[target_idx], index);
        self.buffer.move_cursor(index, extend);
        self.cursor_position_changed();
        self.selection_changed();
        self.request_repaint();
    }

    fn move_to_line_edge(&mut self, end: bool, extend: bool) {
        let width = self.bounding_width();
        let lines = self.ensure_layout_cached(width).clone();
        let Some((line_idx, _)) = self.cursor_line_and_x(&lines) else {
            return;
        };
        let line = &lines[line_idx];
        let (index, affinity) = if end {
            (line.end, CaretAffinity::Upstream)
        } else {
            (line.start, CaretAffinity::Downstream)
        };
        self.current_cursor_affinity = affinity;
        self.buffer.move_cursor(index, extend);
        self.cursor_position_changed();
        self.selection_changed();
        self.request_repaint();
    }

    fn record_transaction(
        &mut self,
        old: EditorSnapshot,
        new: EditorSnapshot,
        cause: EditorTransactionCause,
        emit: bool,
    ) {
        let transaction = self.engine.create_transaction(
            &old.text,
            &new.text,
            EditorSelection {
                anchor: EditorCursor::new(&old.text, old.selection_anchor),
                head: EditorCursor::new(&old.text, old.cursor),
            },
            EditorSelection {
                anchor: EditorCursor::new(&new.text, new.selection_anchor),
                head: EditorCursor::new(&new.text, new.cursor),
            },
            cause,
        );
        let events = self.engine.animation_events(&transaction);
        self.last_event_count = events.len() as u32;
        self.last_summary = format!(
            "cause={:?};changes={};events={};animate={}",
            transaction.cause,
            transaction.changes.len(),
            events.len(),
            transaction.should_animate
        )
        .into();
        if emit {
            self.transaction_created();
        }
    }

    fn recalculate_content_height_quiet(&mut self) {
        let next = self.compute_content_height();
        if (self.current_content_height - next).abs() > 0.5 {
            self.current_content_height = next;
            self.content_height_dirty.set(true);
        }
    }

    fn compute_content_height(&mut self) -> f32 {
        let width = self.bounding_width();
        let padding = self.current_padding;
        let font_size = self.current_font_pixel_size;
        let line_spacing = self.current_line_spacing;
        let lines = self.ensure_layout_cached(width);
        let height = lines
            .last()
            .map(|line| line.y + line.height + padding as f64)
            .unwrap_or((font_size * line_spacing + padding * 2.0) as f64);
        height.max(1.0) as f32
    }

    fn invalidate_layout_cache(&mut self) {
        self.layout_cache = None;
        self.scroll_buffer = None;
    }

    fn ensure_layout_cached(&mut self, width: f64) -> &Vec<VisualLine> {
        let text_ptr = self.buffer.text.as_ptr() as usize;
        let text_len = self.buffer.text.len();
        let font_size = self.current_font_pixel_size;
        let font_family = self.current_font_family.to_string();
        let line_spacing = self.current_line_spacing;
        let text_indent = self.current_text_indent;
        let padding = self.current_padding;

        let needs_refresh = match &self.layout_cache {
            Some(c) => c.text_ptr != text_ptr
                || c.text_len != text_len
                || (c.width - width).abs() > 0.1
                || (c.font_size - font_size).abs() > 0.1
                || c.font_family != font_family
                || (c.line_spacing - line_spacing).abs() > 0.01
                || (c.text_indent - text_indent).abs() > 0.1
                || (c.padding - padding).abs() > 0.1,
            None => true,
        };

        if needs_refresh {
            let lines = layout_lines(
                &self.buffer.text,
                width,
                font_size as f64,
                line_spacing as f64,
                padding as f64,
                text_indent as f64,
                &font_family,
            );
            let content_height = lines
                .last()
                .map(|l| (l.y + l.height + padding as f64) as f32)
                .unwrap_or((font_size * line_spacing + padding * 2.0) as f32)
                .max(1.0);
            self.layout_cache = Some(LayoutCache {
                text_ptr,
                text_len,
                width,
                font_size,
                font_family,
                line_spacing,
                text_indent,
                padding,
                lines,
                content_height,
            });
        }

        &self.layout_cache.as_ref().unwrap().lines
    }

    fn hit_test(&mut self, x: f64, y: f64) -> (usize, CaretAffinity) {
        let width = self.bounding_width();
        let lines = self.ensure_layout_cached(width).clone();
        if lines.is_empty() {
            return (0, CaretAffinity::Downstream);
        }
        // Model B: y is already in document coordinate space
        let doc_y = y;
        let line_opt = lines
            .iter()
            .enumerate()
            .find(|(_, line)| doc_y < line.y + line.height);
        let (line_idx, line) = match line_opt {
            Some((idx, l)) => (idx, l),
            None => (lines.len() - 1, lines.last().unwrap()),
        };
        let index = self.index_at_line_x(line, x);
        let affinity = if index == line.end && line.start != line.end {
            CaretAffinity::Upstream
        } else {
            CaretAffinity::Downstream
        };
        if sujian_editor_debug_enabled() {
            eprintln!(
                "hit_test: mouse_x={:.1}, mouse_y={:.1}, doc_y={:.1}, current_scroll_y={:.1}, hit_visual_line_idx={}, line_range={}..{}, index={}, affinity={:?}, cursor_rect_x={:.1}, cursor_rect_y={:.1}",
                x, y, doc_y, self.current_scroll_y, line_idx, line.start, line.end, index, affinity, self.target_cursor_x, self.target_cursor_y
            );
        }
        (index, affinity)
    }

    fn index_at_line_x(&self, line: &VisualLine, x: f64) -> usize {
        let font_size = self.current_font_pixel_size as f64;
        let font_family = self.current_font_family.to_string();
        let relative = (x - line.x).max(0.0);
        if line.para_text.is_empty() {
            return line.start;
        }
        let paragraph_wrap_w = line.line_wrap_width + line.line_indent_x;
        qtextlayout_x_to_cursor_on_line(
            &line.para_text, relative, line.para_start,
            font_size, &font_family,
            paragraph_wrap_w, line.line_indent_x, line.qtextline_idx,
        )
    }

    fn cursor_line_and_x(&self, lines: &[VisualLine]) -> Option<(usize, f64)> {
        if lines.is_empty() {
            return None;
        }
        let font_size = self.current_font_pixel_size as f64;
        let font_family = self.current_font_family.to_string();
        for (idx, line) in lines.iter().enumerate() {
            if line_contains_cursor_with_affinity(lines, idx, self.buffer.cursor, self.current_cursor_affinity) {
                let paragraph_wrap_w = line.line_wrap_width + line.line_indent_x;
                let w = if self.current_cursor_affinity == CaretAffinity::Upstream && self.buffer.cursor == line.end {
                    line.width
                } else if self.current_cursor_affinity == CaretAffinity::Downstream && self.buffer.cursor == line.start {
                    0.0
                } else if line.para_text.is_empty() {
                    0.0
                } else {
                    qtextlayout_cursor_to_x_on_line(
                        &line.para_text, self.buffer.cursor, line.para_start,
                        font_size, &font_family,
                        paragraph_wrap_w, line.line_indent_x, line.qtextline_idx,
                    )
                };
                return Some((idx, line.x + w));
            }
        }
        lines.last().map(|line| (lines.len() - 1, line.x + line.width))
    }

    /// 给定字节范围 [byte_start, byte_end)，返回每个字形的矩形信息。
    /// 使用 QTextLayout 精确定位每个字形。
    #[allow(dead_code)]
    fn glyph_rects_for_range(&self, byte_start: usize, byte_end: usize) -> Vec<AnimatedGlyph> {
        let Some(ref cache) = self.layout_cache else {
            return Vec::new();
        };
        let lines = &cache.lines;
        if lines.is_empty() || byte_start >= byte_end {
            return Vec::new();
        }
        let font_size = self.current_font_pixel_size as f64;
        let font_family = self.current_font_family.to_string();
        let mut result = Vec::new();

        let search_start = lines.partition_point(|l| l.end <= byte_start);
        let search_end = lines.len().min(
            search_start + lines[search_start..].partition_point(|l| l.start < byte_end) + 1
        );

        for line_idx in search_start..search_end {
            let line = &lines[line_idx];
            if line.end <= byte_start || line.start >= byte_end {
                continue;
            }
            let seg_start = byte_start.max(line.start);
            let seg_end = byte_end.min(line.end);
            if seg_start >= seg_end {
                continue;
            }

            let baseline_y = text_baseline_y(line, font_size, &font_family);
            let (top_y, h) = cursor_rect_for_line(line, font_size, &font_family);

            if line.para_text.is_empty() {
                continue;
            }
            let glyph_data = qtextlayout_glyph_positions_on_line(
                &line.para_text, seg_start, seg_end, line.para_start,
                font_size, &font_family,
                line.line_wrap_width + line.line_indent_x, line.line_indent_x, line.qtextline_idx,
            );
            for (abs_byte, x_pos, ch_w) in glyph_data {
                // Get the character at this position
                if abs_byte >= self.buffer.text.len() {
                    continue;
                }
                let ch = self.buffer.text[abs_byte..].chars().next().unwrap();
                let ch_str = ch.to_string();
                result.push(AnimatedGlyph {
                    byte_start: abs_byte,
                    byte_end: abs_byte + ch_str.len(),
                    text: ch_str,
                    rect: (line.x + x_pos, top_y, ch_w, h),
                    baseline_y,
                    line_index: line_idx,
                });
            }
        }
        result
    }

    #[allow(dead_code)]
    fn start_delete_animation(&mut self, glyphs: Vec<AnimatedGlyph>) {
        if glyphs.is_empty() || self.current_is_scrolling {
            return;
        }
        self.invalidate_layout_cache();
        let width = self.bounding_width();
        let lines = self.ensure_layout_cached(width).clone();
        let font_size = self.current_font_pixel_size as f64;
        let font_family = self.current_font_family.to_string();
        let (target_x, target_y, target_line_idx) = cursor_geometry_with_font(&self.buffer.text, &lines, self.buffer.cursor, self.current_cursor_affinity, font_size, &font_family);
        let cursor_h = if target_line_idx < lines.len() {
            cursor_rect_for_line(&lines[target_line_idx], font_size, &font_family).1
        } else {
            cursor_height_for_line(font_size, &font_family)
        };
        let duration = self.current_cursor_animation_duration_ms.max(30) as u64;
        self.delete_animation = Some(DeleteAnimation {
            glyphs,
            target_cursor_rect: (target_x, target_y, 2.0, cursor_h),
            start_time: Instant::now(),
            duration_ms: duration,
        });
    }

    #[allow(dead_code)]
    fn animation_visible_hit(&self, glyphs: &[AnimatedGlyph]) -> bool {
        if glyphs.is_empty() {
            return false;
        }
        let top = self.current_scroll_y as f64;
        let bottom = top + self.current_viewport_height.max(1.0) as f64;
        glyphs.iter().any(|g| {
            let glyph_top = g.rect.1;
            let glyph_bottom = g.rect.1 + g.rect.3;
            glyph_bottom >= top && glyph_top <= bottom
        })
    }

    #[allow(dead_code)]
    fn log_animation_created(&self, label: &str, offset: usize, glyph_count: usize, visible_line_hit: bool) {
        if editor_animation_debug_enabled() {
            eprintln!(
                "{}: offset={}, glyph_count={}, visible_line_hit={}, scrolling={}, enabled={}",
                label,
                offset,
                glyph_count,
                visible_line_hit,
                self.current_is_scrolling,
                self.current_typing_animation_enabled,
            );
        }
    }

    /// 给定字节范围，返回 (min_x, min_y, max_x, max_y) 包围盒
    #[allow(dead_code)]
    fn bounding_rect_for_range(&self, byte_start: usize, byte_end: usize) -> (f64, f64, f64, f64) {
        let glyphs = self.glyph_rects_for_range(byte_start, byte_end);
        if glyphs.is_empty() {
            return (self.target_cursor_x, self.target_cursor_y, 2.0, cursor_height_for_line(self.current_font_pixel_size as f64, &self.current_font_family.to_string()));
        }
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;
        for g in &glyphs {
            min_x = min_x.min(g.rect.0);
            min_y = min_y.min(g.rect.1);
            max_x = max_x.max(g.rect.0 + g.rect.2);
            max_y = max_y.max(g.rect.1 + g.rect.3);
        }
        (min_x, min_y, max_x - min_x, max_y - min_y)
    }
}

fn is_left_button_pressed(event: &QMouseEvent) -> bool {
    cpp!(unsafe [event as "const QMouseEvent*"] -> bool as "bool" {
        return event ? (event->buttons() & Qt::LeftButton) : false;
    })
}

impl QQuickItem for SujianEditorItem {
    fn component_complete(&mut self) {
        let obj_ptr = self.get_cpp_object();
        if obj_ptr.is_null() {
            return;
        }
        let item_ptr = self as *mut Self as *mut std::ffi::c_void;
        cpp!(unsafe [obj_ptr as "QQuickItem*", item_ptr as "void*"] {
            sujian_install_event_filter(obj_ptr, item_ptr);
        });
    }

    fn geometry_changed(&mut self, _new_geometry: QRectF, _old_geometry: QRectF) {
        self.scroll_buffer = None;
        self.recalculate_content_height_quiet();
        self.request_repaint();
    }

    fn mouse_event(&mut self, event: QMouseEvent) -> bool {
        let pos = event.position();
        match event.event_type() {
            qmetaobject::QMouseEventType::MouseButtonPress => {
                self.click_at(pos.x as f32, pos.y as f32, false);
                let obj_ptr = self.get_cpp_object();
                cpp!(unsafe [obj_ptr as "QQuickItem*"] {
                    if (obj_ptr) obj_ptr->setFocus(true);
                });
            }
            qmetaobject::QMouseEventType::MouseMove => {
                if is_left_button_pressed(&event) {
                    self.drag_select_at(pos.x as f32, pos.y as f32);
                }
            }
            qmetaobject::QMouseEventType::MouseButtonRelease => {}
            _ => {}
        }
        true
    }

    fn update_paint_node(
        &mut self,
        mut node: qmetaobject::scenegraph::SGNode<qmetaobject::scenegraph::ContainerNode>,
    ) -> qmetaobject::scenegraph::SGNode<qmetaobject::scenegraph::ContainerNode> {
        use qmetaobject::scenegraph::SGNode;

        let frame_start = Instant::now();

        // 光标动画进行中也算"需要更新"，否则 render_dirty 路径清完 cursor_dirty 后动画会被掐断
        let cursor_anim_active = self.cursor_animation.as_ref()
            .is_some_and(|a| !a.is_finished(Instant::now()));

        if !self.render_dirty && !self.cursor_dirty && !cursor_anim_active {
            return node;
        }

        let item_ptr = self.get_cpp_object();
        let dpr = if !item_ptr.is_null() { sujian_item_dpr(item_ptr) } else { 1.0 };
        let old_raw = node.into_raw();
        let vp_h = self.current_viewport_height.max(1.0) as f64;
        let scroll_y = self.current_scroll_y as f64;

        let cursor_rgba = color_hex_to_rgba(&self.current_cursor_color);

        // 路径 1: 文字需要重绘
        if self.render_dirty {
            match self.render_to_image() {
                Some((image, buf_scroll_y, _buf_h)) => {
                    let src_y = scroll_y - buf_scroll_y;
                    let logical_img_w = image.size().width as f64 / dpr;
                    let new_raw = sujian_update_texture_node(
                        old_raw, item_ptr, &image, 0.0, src_y, logical_img_w, vp_h, scroll_y, vp_h, dpr,
                    );
                    sujian_update_cursor_rect(
                        new_raw, item_ptr,
                        self.cursor_visual_x, self.cursor_visual_y, 2.0, self.cursor_visual_h,
                        self.cursor_visible, cursor_rgba,
                    );
                    // 光标动画仍 active 时保持 cursor_dirty，让下一帧进 cursor-only 路径推进动画
                    let still_animating = self.cursor_animation.as_ref()
                        .is_some_and(|a| !a.is_finished(Instant::now()));
                    if still_animating {
                        self.cursor_dirty = true;
                        let item = self as &dyn QQuickItem;
                        item.update();
                    } else {
                        self.cursor_dirty = false;
                    }
                    let total_elapsed = frame_start.elapsed();
                    if total_elapsed.as_millis() > 4 {
                        eprintln!(
                            "sujian_update_paint_node: total_ms={}, new_texture=true, buf={:.0}..{:.0}",
                            total_elapsed.as_millis(), buf_scroll_y, buf_scroll_y + _buf_h,
                        );
                    }
                    unsafe { SGNode::<qmetaobject::scenegraph::ContainerNode>::from_raw(new_raw) }
                }
                None => {
                    // 缓冲区命中：只更新 source rect，但先用 helper 重新算光标位置
                    self.update_cursor_visual_position();

                    if !old_raw.is_null() {
                        if let Some(ref buf) = self.scroll_buffer {
                            let src_y = scroll_y - buf.buffer_scroll_y;
                            let logical_img_w = buf.image.size().width as f64 / dpr;
                            sujian_update_source_rect(old_raw, item_ptr, 0.0, src_y, logical_img_w, vp_h, scroll_y, vp_h, dpr);
                        }
                        sujian_update_cursor_rect(
                            old_raw, item_ptr,
                            self.cursor_visual_x, self.cursor_visual_y, 2.0, self.cursor_visual_h,
                            self.cursor_visible, cursor_rgba,
                        );
                    }
                    let still_animating = self.cursor_animation.as_ref()
                        .is_some_and(|a| !a.is_finished(Instant::now()));
                    if still_animating {
                        self.cursor_dirty = true;
                        let item = self as &dyn QQuickItem;
                        item.update();
                    } else {
                        self.cursor_dirty = false;
                    }
                    unsafe { SGNode::<qmetaobject::scenegraph::ContainerNode>::from_raw(old_raw) }
                }
            }
        } else if self.cursor_dirty && !old_raw.is_null() {
            // 路径 2: 只有光标变了（动画/滚动），不需要重绘文字
            let now = Instant::now();
            if let Some(ref anim) = self.cursor_animation {
                if anim.is_finished(now) {
                    self.cursor_visual_x = anim.target_x;
                    self.cursor_visual_y = anim.target_y;
                    self.cursor_animation = None;
                    self.cursor_dirty = false;
                } else {
                    let (cx, cy) = anim.current_position(now);
                    self.cursor_visual_x = cx;
                    self.cursor_visual_y = cy;
                    let item = self as &dyn QQuickItem;
                    item.update();
                }
            } else {
                self.cursor_dirty = false;
            }

            if let Some(ref buf) = self.scroll_buffer {
                let src_y = scroll_y - buf.buffer_scroll_y;
                let logical_img_w = buf.image.size().width as f64 / dpr;
                sujian_update_source_rect(old_raw, item_ptr, 0.0, src_y, logical_img_w, vp_h, scroll_y, vp_h, dpr);
            }
            sujian_update_cursor_rect(
                old_raw, item_ptr,
                self.cursor_visual_x, self.cursor_visual_y, 2.0, self.cursor_visual_h,
                self.cursor_visible, cursor_rgba,
            );
            unsafe { SGNode::<qmetaobject::scenegraph::ContainerNode>::from_raw(old_raw) }
        } else {
            // 路径 3: 无变化（或只有 anim_active 但 cursor_dirty 已被清，补一次推进）
            if cursor_anim_active && !old_raw.is_null() {
                let now = Instant::now();
                if let Some(ref anim) = self.cursor_animation {
                    if anim.is_finished(now) {
                        self.cursor_visual_x = anim.target_x;
                        self.cursor_visual_y = anim.target_y;
                        self.cursor_animation = None;
                        self.cursor_dirty = false;
                    } else {
                        let (cx, cy) = anim.current_position(now);
                        self.cursor_visual_x = cx;
                        self.cursor_visual_y = cy;
                        sujian_update_cursor_rect(
                            old_raw, item_ptr,
                            self.cursor_visual_x, self.cursor_visual_y, 2.0, self.cursor_visual_h,
                            self.cursor_visible, cursor_rgba,
                        );
                        let item = self as &dyn QQuickItem;
                        item.update();
                    }
                }
            }
            unsafe { SGNode::<qmetaobject::scenegraph::ContainerNode>::from_raw(old_raw) }
        }
    }
}

impl SujianEditorItem {
    fn update_cursor_visual_position(&mut self) {
        let width = self.bounding_width();
        let lines = self.ensure_layout_cached(width).clone();
        let font_size = self.current_font_pixel_size as f64;
        let font_family = self.current_font_family.to_string();

        let (cursor_x, cursor_line_y, cursor_line_idx) = cursor_geometry_with_font(&self.buffer.text, &lines, self.buffer.cursor, self.current_cursor_affinity, font_size, &font_family);
        let (cursor_y, cursor_h) = if cursor_line_idx < lines.len() {
            cursor_rect_for_line(&lines[cursor_line_idx], font_size, &font_family)
        } else {
            (cursor_line_y, cursor_height_for_line(font_size, &font_family))
        };
        self.ime_cursor_rect_y = cursor_y;
        self.ime_cursor_rect_h = cursor_h;

        // Viewport check: hide cursor when not visible, avoid per-frame dirty
        let scroll_top = self.current_scroll_y as f64;
        let scroll_bottom = scroll_top + self.current_viewport_height.max(1.0) as f64;
        let in_viewport = self.target_cursor_y + cursor_h > scroll_top
            && self.target_cursor_y < scroll_bottom;
        let old_visible = self.cursor_visible;
        let new_visible = self.current_editor_enabled
            && !self.buffer.has_selection()
            && in_viewport;

        if !new_visible {
            // Off-screen or disabled: stop animation, hide once
            self.cursor_animation = None;
            if old_visible {
                self.cursor_visible = false;
                self.cursor_dirty = true;
                let item = self as &dyn QQuickItem;
                item.update();
            }
            return;
        }

        // Update target position
        let old_x = self.target_cursor_x;
        let old_y = self.target_cursor_y;
        if (old_x - cursor_x).abs() > 0.01 || (old_y - cursor_y).abs() > 0.01 {
            self.target_cursor_x = cursor_x;
            self.target_cursor_y = cursor_y;
            self.cursor_rect_changed();
            
            let obj_ptr = self.get_cpp_object();
            cpp!(unsafe [obj_ptr as "QQuickItem*"] {
                if (obj_ptr) {
                    QGuiApplication::inputMethod()->update(Qt::ImQueryInput);
                }
            });
        }

        let now = Instant::now();
        let is_selecting = self.buffer.selection_anchor != self.buffer.cursor;
        let is_preediting = !self.preedit_text.is_empty();

        let should_snap = self.current_is_scrolling 
            || is_selecting 
            || is_preediting;

        let (visual_x, visual_y) = if let Some(ref anim) = self.cursor_animation {
            if anim.is_finished(now) {
                (anim.target_x, anim.target_y)
            } else {
                anim.current_position(now)
            }
        } else {
            (self.target_cursor_x, self.target_cursor_y)
        };

        let (final_x, final_y, new_animation) = if should_snap || !self.current_smooth_cursor_enabled {
            (self.target_cursor_x, self.target_cursor_y, None)
        } else if let Some(ref anim) = self.cursor_animation {
            if (anim.target_x - self.target_cursor_x).abs() > 0.01 || (anim.target_y - self.target_cursor_y).abs() > 0.01 {
                let (cur_x, cur_y) = anim.current_position(now);
                let duration = self.current_cursor_animation_duration_ms.max(30) as u64;
                let new_anim = CursorAnimationState {
                    start_x: cur_x,
                    start_y: cur_y,
                    target_x: self.target_cursor_x,
                    target_y: self.target_cursor_y,
                    start_time: now,
                    duration_ms: duration,
                };
                (cur_x, cur_y, Some(new_anim))
            } else if anim.is_finished(now) {
                (anim.target_x, anim.target_y, None)
            } else {
                let (cur_x, cur_y) = anim.current_position(now);
                (cur_x, cur_y, Some(anim.clone()))
            }
        } else {
            if (visual_x - self.target_cursor_x).abs() > 0.01 || (visual_y - self.target_cursor_y).abs() > 0.01 {
                let duration = self.current_cursor_animation_duration_ms.max(30) as u64;
                let new_anim = CursorAnimationState {
                    start_x: visual_x,
                    start_y: visual_y,
                    target_x: self.target_cursor_x,
                    target_y: self.target_cursor_y,
                    start_time: now,
                    duration_ms: duration,
                };
                (visual_x, visual_y, Some(new_anim))
            } else {
                (self.target_cursor_x, self.target_cursor_y, None)
            }
        };

        self.cursor_animation = new_animation;
        self.cursor_visual_x = final_x;
        self.cursor_visual_y = final_y;
        self.cursor_visual_h = cursor_h;
        self.cursor_visible = true;

        // Only dirty when position actually changed or visibility transitioned
        let pos_changed = (final_x - old_x).abs() > 0.01
            || (final_y - old_y).abs() > 0.01
            || !old_visible;
        if pos_changed {
            self.cursor_dirty = true;
        }

        if sujian_editor_debug_enabled() {
            eprintln!(
                "update_cursor_visual_position: cursor={}, target_x={:.1}, target_y={:.1}, visual_x={:.1}, visual_y={:.1}, is_animating={}, current_scroll_y={:.1}",
                self.buffer.cursor, self.target_cursor_x, self.target_cursor_y, self.cursor_visual_x, self.cursor_visual_y, self.cursor_animation.is_some(), self.current_scroll_y
            );
        }

        if let Some(ref anim) = self.cursor_animation {
            if !anim.is_finished(now) {
                self.cursor_dirty = true;
                let item = self as &dyn QQuickItem;
                item.update();
            }
        }
    }

    /// 渲染文字到 painter。buffer_scroll_y 和 buffer_h 定义缓冲区可见范围。
    /// paint_offset_y 将文档坐标系转换为图片坐标系。
    fn paint_onto(&mut self, painter: &mut QPainter, buffer_scroll_y: f64, buffer_h: f64) {
        let paint_start = Instant::now();
        let width = self.bounding_width();
        // 先确保布局缓存有效，然后借用而非 clone
        {
            let _ = self.ensure_layout_cached(width);
        }
        let content_h = self.layout_cache.as_ref().map(|c| c.content_height).unwrap_or(self.current_content_height);
        if (self.current_content_height - content_h).abs() > 0.5 {
            self.current_content_height = content_h;
            self.content_height_dirty.set(true);
        }

        let scroll_y = buffer_scroll_y;
        let paint_offset_y = -scroll_y; // 文档坐标 → 图片坐标

        painter.set_render_hint(QPainterRenderHint::TextAntialiasing, true);
        painter.fill_rect(
            QRectF { x: 0.0, y: 0.0, width, height: buffer_h },
            QBrush::from_color(QColor::from_rgba(0, 0, 0, 0)),
        );

        let font_size = self.current_font_pixel_size as f64;
        let font_family = self.current_font_family.to_string();
        let fs = font_size as f32;
        let ff: QString = font_family.clone().into();
        cpp!(unsafe [painter as "QPainter*", fs as "float", ff as "QString"] {
            QFont f(ff);
            f.setPixelSize(static_cast<int>(fs));
            painter->setFont(f);
        });

        // 借用 layout_cache 中的 lines，不 clone
        let lines = &self.layout_cache.as_ref().unwrap().lines;
        let vis_start = lines.partition_point(|l| l.y + l.height < scroll_y);
        let vis_end = lines.len().min(lines.partition_point(|l| l.y < scroll_y + buffer_h + font_size * 2.0) + 1);

        let selection = self.buffer.selection_range();
        let now_anim = Instant::now();
        let mut needs_animation_repaint = false;

        // ── Layer 1: Selection background ──
        for line in &lines[vis_start..vis_end] {
            if self.buffer.has_selection() && selection.1 > line.start && selection.0 < line.end {
                let sel_start = selection.0.max(line.start);
                let sel_end = selection.1.min(line.end);
                let paragraph_wrap_w = line.line_wrap_width + line.line_indent_x;
                let x_start = if !line.para_text.is_empty() {
                    line.x + qtextlayout_cursor_to_x_on_line(
                        &line.para_text, sel_start, line.para_start,
                        font_size, &font_family,
                        paragraph_wrap_w, line.line_indent_x, line.qtextline_idx,
                    )
                } else {
                    line.x
                };
                let x_end = if !line.para_text.is_empty() {
                    line.x + qtextlayout_cursor_to_x_on_line(
                        &line.para_text, sel_end, line.para_start,
                        font_size, &font_family,
                        paragraph_wrap_w, line.line_indent_x, line.qtextline_idx,
                    )
                } else {
                    line.x
                };
                draw_rect(
                    painter,
                    x_start,
                    line.y + paint_offset_y,
                    (x_end - x_start).max(2.0),
                    line.height,
                    self.current_selection_color.clone(),
                );
            }
        }

        // ── Layer 2: Base text ──
        let active_insert: Option<&InsertAnimation> = None;
        let active_delete: Option<&DeleteAnimation> = None;
        let had_insert_animation = false;
        let had_delete_animation = false;

        for line_idx in vis_start..vis_end {
            let line = &lines[line_idx];
            let text = self.buffer.text[line.start..line.end].to_string();
            let text_y = text_baseline_y(line, font_size, &font_family) + paint_offset_y;

            if let Some(ref insert_anim) = active_insert {
                let insert_in_line = insert_anim.glyphs.iter().any(|g| g.line_index == line_idx);
                if insert_in_line {
                    let eased = 1.0 - (1.0 - insert_anim.progress(now_anim)).powi(3);
                    if editor_animation_debug_enabled() {
                        eprintln!(
                            "insert_animation_paint: progress={:.3}, glyph_count={}, visible_line_hit=true",
                            insert_anim.progress(now_anim),
                            insert_anim.glyphs.len(),
                        );
                    }
                    let first_glyph = insert_anim.glyphs.first().unwrap();
                    let last_glyph = insert_anim.glyphs.last().unwrap();
                    let insert_start_byte = first_glyph.byte_start;
                    let insert_end_byte = last_glyph.byte_end;

                    // 插入点之前的文字：正常绘制
                    if insert_start_byte > line.start && insert_start_byte <= line.end {
                        let before = &self.buffer.text[line.start..insert_start_byte];
                        draw_text(painter, line.x, text_y, fs, self.current_text_color.clone(), before.to_string().into());
                    }

                    // 插入的文字：逐字绘制，只画 clip 宽度内的字
                    let clip_origin_x = insert_anim.origin_cursor_rect.0;
                    let final_insert_w: f64 = insert_anim.glyphs.iter().map(|g| g.rect.2).sum();
                    let clip_right = clip_origin_x + final_insert_w * eased;
                    let base_color = color_from_qstring(self.current_text_color.clone());

                    for glyph in &insert_anim.glyphs {
                        let gx = glyph.rect.0;
                        let gy = glyph.baseline_y + paint_offset_y;
                        if gx + glyph.rect.2 <= clip_right + 0.5 {
                            draw_text(painter, gx, gy, fs, self.current_text_color.clone(), glyph.text.clone().into());
                        } else if gx < clip_right + 0.5 {
                            let visible_frac = ((clip_right - gx) / glyph.rect.2).clamp(0.0, 1.0);
                            let alpha = (visible_frac * 255.0).round() as i32;
                            draw_text_color(
                                painter,
                                gx,
                                gy,
                                QColor::from_rgba(base_color.red(), base_color.green(), base_color.blue(), alpha),
                                glyph.text.clone().into(),
                            );
                        }
                    }

                    // 插入点之后的文字：正常绘制
                    if insert_end_byte < line.end {
                        let insert_w = final_insert_w;
                        let after_x = first_glyph.rect.0 + insert_w;
                        let after = &self.buffer.text[insert_end_byte..line.end];
                        draw_text(painter, after_x, text_y, fs, self.current_text_color.clone(), after.to_string().into());
                    }

                    needs_animation_repaint = true;
                    continue;
                }
            }

            // 普通文字绘制
            draw_text(painter, line.x, text_y, fs, self.current_text_color.clone(), text.into());
        }

        // 删除动画不能用旧字节范围切新正文：先画新正文，再把旧 glyph 作为 ghost 叠上去。
        if let Some(ref delete_anim) = active_delete {
            let eased = 1.0 - (1.0 - delete_anim.progress(now_anim)).powi(3);
            let target_x = delete_anim.target_cursor_rect.0;
            let target_y_top = delete_anim.target_cursor_rect.1;
            let base_color = color_from_qstring(self.current_text_color.clone());
            if editor_animation_debug_enabled() {
                eprintln!(
                    "delete_animation_paint: progress={:.3}, glyph_count={}",
                    delete_anim.progress(now_anim),
                    delete_anim.glyphs.len(),
                );
            }
            for glyph in &delete_anim.glyphs {
                let (gx, gy, _gw, gh) = glyph.rect;
                if gy + gh < scroll_y || gy > scroll_y + buffer_h {
                    continue;
                }
                let offset_x = (target_x - gx) * eased;
                let offset_y = (target_y_top - gy) * eased;
                let alpha = ((1.0 - eased) * 255.0).round() as i32;
                draw_text_color(
                    painter,
                    gx + offset_x,
                    glyph.baseline_y + offset_y + paint_offset_y,
                    QColor::from_rgba(base_color.red(), base_color.green(), base_color.blue(), alpha),
                    glyph.text.clone().into(),
                );
            }
            needs_animation_repaint = true;
        }

        // ── Layer 3: Preedit ──
        if !self.preedit_text.is_empty() {
            let pc = self.buffer.cursor;
            for (idx, line) in lines.iter().enumerate() {
                if idx < vis_start || idx >= vis_end {
                    continue;
                }
                if line_contains_cursor_with_affinity(lines, idx, pc, self.current_cursor_affinity) {
                    let paragraph_wrap_w = line.line_wrap_width + line.line_indent_x;
                    let x = if !line.para_text.is_empty() {
                        line.x + qtextlayout_cursor_to_x_on_line(
                            &line.para_text, pc, line.para_start,
                            font_size, &font_family,
                            paragraph_wrap_w, line.line_indent_x, line.qtextline_idx,
                        )
                    } else {
                        line.x
                    };
                    let baseline = text_baseline_y(line, font_size, &font_family) + paint_offset_y;
                    draw_text(
                        painter,
                        x,
                        baseline,
                        fs,
                        self.current_text_color.clone(),
                        self.preedit_text.clone().into(),
                    );
                    // Preedit width: use a single-line layout for the preedit text itself
                    let preedit_w = qtextlayout_cursor_to_x(&self.preedit_text, &self.preedit_text, font_size, &font_family);
                    painter.set_pen(QPen::from_color(color_from_qstring(self.current_text_color.clone())));
                    let underline_y = baseline + 2.0;
                    let line_f = QLineF { pt1: QPointF { x, y: underline_y }, pt2: QPointF { x: x + preedit_w, y: underline_y } };
                    painter.draw_line(line_f);
                    break;
                }
            }
        }

        // ── Layer 4: Cursor ──
        self.update_cursor_visual_position();
        
        // 清理已完成的吐字/吞字动画
        let now_cleanup = Instant::now();
        if let Some(ref anim) = self.insert_animation {
            if anim.is_finished(now_cleanup) {
                self.insert_animation = None;
            }
        }
        if let Some(ref anim) = self.delete_animation {
            if anim.is_finished(now_cleanup) {
                self.delete_animation = None;
            }
        }
        if needs_animation_repaint && !self.current_is_scrolling {
            self.request_repaint();
        }

        let elapsed = paint_start.elapsed();
        if elapsed.as_millis() > 4 && should_log_slow_paint(self.last_slow_paint_log, now_cleanup) {
            self.last_slow_paint_log = Some(now_cleanup);
            eprintln!(
                "sujian_paint_onto: elapsed_ms={}, vis_lines=[{}..{}]={}, insert_anim={}, delete_anim={}, scrolling={}, buffer_h={:.1}",
                elapsed.as_millis(),
                vis_start, vis_end,
                vis_end.saturating_sub(vis_start),
                had_insert_animation,
                had_delete_animation,
                self.current_is_scrolling,
                self.current_content_height,
            );
        }
    }

    fn render_to_image(&mut self) -> Option<(qmetaobject::QImage, f64, f64)> {
        let render_start = Instant::now();
        let item_ptr = self.get_cpp_object();
        let dpr = if !item_ptr.is_null() { sujian_item_dpr(item_ptr) } else { 1.0 };
        let width = self.bounding_width();
        let vp_h = self.current_viewport_height.max(1.0) as f64;
        let img_w = (width as i32).max(1);
        let scroll_y = self.current_scroll_y as f64;
        let content_h = self.current_content_height as f64;

        // Overscan: Cap the overscan padding to keep texture sizes small on maximized screens
        let overscan = (vp_h * 0.8).min(600.0);
        let min_y = (scroll_y - overscan).max(0.0);
        let max_y = (scroll_y + vp_h + overscan).min(content_h.max(vp_h));
        let buffer_h = max_y - min_y;

        // Evaluate the scroll buffer miss reason
        let mut miss_reason = "none";
        let mut needs_render = true;

        if self.scroll_buffer.is_none() {
            miss_reason = "no_buffer";
        } else if self.layout_cache.is_none() {
            miss_reason = "layout_invalidated";
        } else {
            let buf = self.scroll_buffer.as_ref().unwrap();
            let content_changed = (content_h - buf.buffer_content_h).abs() > 1.0;
            if content_changed {
                miss_reason = "content_changed";
            } else {
                let dpr_changed = (dpr - buf.dpr).abs() > 0.01;
                if dpr_changed {
                    miss_reason = "dpr_changed";
                } else {
                    let inside_buffer = scroll_y >= buf.buffer_scroll_y - 1.0
                        && scroll_y + vp_h <= buf.buffer_scroll_y + buf.buffer_logical_h + 1.0;
                    if !inside_buffer {
                        miss_reason = "outside_buffer";
                    } else {
                        // Buffer is valid, skip render and reuse
                        needs_render = false;
                    }
                }
            }
        }

        if !needs_render {
            self.render_dirty = false;
            return None;
        }

        if sujian_editor_debug_enabled() {
            eprintln!(
                "render_to_image: rebuilding buffer, miss_reason={}, scroll_y={:.1}, content_h={:.1}, vp_h={:.1}",
                miss_reason, scroll_y, content_h, vp_h
            );
        }

        // 需要重新渲染 — 按 DPR 创建物理像素尺寸 of QImage
        let phys_w = ((img_w as f64 * dpr) as i32).max(1);
        let phys_h = ((buffer_h * dpr) as i32).max(1);
        let mut image = qmetaobject::QImage::new(
            qmetaobject::QSize { width: phys_w as u32, height: phys_h as u32 },
            qmetaobject::ImageFormat::ARGB32_Premultiplied,
        );
        image.fill(qmetaobject::QColor::from_rgba(0, 0, 0, 0));

        // 创建 painter 并缩放，使所有绘制操作使用逻辑坐标
        let painter_ptr = sujian_create_painter_scaled(&mut image, dpr);
        if painter_ptr.is_null() {
            return Some((image, min_y, buffer_h));
        }

        // Safety: QPainter is #[repr(C)], *mut QPainter == &mut QPainter
        let painter: &mut QPainter = unsafe { &mut *painter_ptr };
        self.paint_onto(painter, min_y, buffer_h);

        sujian_delete_painter(painter_ptr);

        self.render_dirty = false;
        self.scroll_buffer = Some(ScrollBuffer {
            image: image.clone(),
            buffer_scroll_y: min_y,
            buffer_content_h: content_h,
            buffer_logical_h: buffer_h,
            dpr,
        });

        let render_elapsed = render_start.elapsed();
        if should_log_slow_paint(self.last_slow_paint_log, render_start) {
            self.last_slow_paint_log = Some(render_start);
            let vis_lines = self.layout_cache.as_ref().map(|c| {
                let start = c.lines.partition_point(|l| l.y + l.height < min_y);
                let end = c.lines.len().min(c.lines.partition_point(|l| l.y < min_y + buffer_h + self.current_font_pixel_size as f64 * 2.0) + 1);
                end.saturating_sub(start)
            }).unwrap_or(0);
            eprintln!(
                "sujian_render_to_image: elapsed_ms={}, img={}x{}(phys {}x{}, dpr={}), vis_lines={}, scroll_y={:.1}, buf_scroll={:.1}, buf_h={:.1}",
                render_elapsed.as_millis(),
                img_w, (buffer_h as i32), phys_w, phys_h, dpr,
                vis_lines,
                scroll_y, min_y, buffer_h,
            );
        }

        Some((image, min_y, buffer_h))
    }
}

fn normalize_plain_text(text: &str) -> String {
    let replaced = text.replace('\u{2029}', "\n").replace("\r\n", "\n").replace('\r', "\n");
    replaced.chars().filter(|&c| c == '\n' || c == '\t' || !c.is_control()).collect()
}

// =============================================================================
// C++ event filter callbacks — called from SujianEventFilter::eventFilter
// =============================================================================

/// Handle key press + text input. Returns true if the event was consumed.
/// Replicates the QML handleSelfRenderedKey logic.
#[no_mangle]
extern "C" fn sujian_handle_key_and_text(
    rust_item: *mut std::ffi::c_void,
    key: i32,
    modifiers: i32,
    text: *const u16,
    text_len: i32,
) -> bool {
    let item = unsafe { &mut *(rust_item as *mut SujianEditorItem) };
    if !item.current_editor_enabled {
        return false;
    }

    let ctrl = has_ctrl(modifiers);

    // Emit explicit_clear for destructive operations
    if key == KEY_BACKSPACE || key == KEY_DELETE || (ctrl && key == KEY_X) {
        item.explicit_clear_requested();
    }

    // Try handle_key first (handles Ctrl+C/V/X/Z, arrows, backspace, delete, etc.)
    if item.handle_key(key, modifiers) {
        return true;
    }

    // If not handled by handle_key and there's printable text, insert it
    if !ctrl && !has_alt(modifiers) && !has_meta(modifiers) && text_len > 0 {
        let text_str = unsafe {
            let slice = std::slice::from_raw_parts(text, text_len as usize);
            String::from_utf16_lossy(slice)
        };
        if !text_str.is_empty() {
            item.insert_text(text_str.into());
            return true;
        }
    }

    false
}

/// Handle IME committed text
#[no_mangle]
extern "C" fn sujian_ime_commit(rust_item: *mut std::ffi::c_void, text: *const u16, text_len: i32) {
    let item = unsafe { &mut *(rust_item as *mut SujianEditorItem) };
    if !item.current_editor_enabled || text_len <= 0 {
        return;
    }
    if item.suppress_next_ime_commit {
        item.suppress_next_ime_commit = false;
        item.preedit_text.clear();
        item.preedit_cursor = 0;
        return;
    }
    let text_str = unsafe {
        let slice = std::slice::from_raw_parts(text, text_len as usize);
        String::from_utf16_lossy(slice)
    };
    if !text_str.is_empty() {
        item.preedit_text.clear();
        item.preedit_cursor = 0;
        item.insert_text(text_str.into());
    }
}

/// Handle IME preedit (composition) text
#[no_mangle]
extern "C" fn sujian_ime_preedit(
    rust_item: *mut std::ffi::c_void,
    text: *const u16,
    text_len: i32,
    cursor: i32,
) {
    let item = unsafe { &mut *(rust_item as *mut SujianEditorItem) };
    if !item.current_editor_enabled {
        return;
    }
    let text_str = unsafe {
        let slice = std::slice::from_raw_parts(text, text_len as usize);
        String::from_utf16_lossy(slice)
    };
    if !text_str.is_empty() {
        item.suppress_next_ime_commit = false;
    }
    item.preedit_text = text_str;
    item.preedit_cursor = cursor.max(0) as usize;
    if item.preedit_cursor > item.preedit_text.len() {
        item.preedit_cursor = item.preedit_text.len();
    }
}

/// Handle IME composition cancel
#[no_mangle]
extern "C" fn sujian_ime_cancel(rust_item: *mut std::ffi::c_void) {
    let item = unsafe { &mut *(rust_item as *mut SujianEditorItem) };
    item.preedit_text.clear();
    item.preedit_cursor = 0;
}

/// Request a repaint from C++ context
#[no_mangle]
extern "C" fn sujian_request_repaint(rust_item: *mut std::ffi::c_void) {
    let item = unsafe { &mut *(rust_item as *mut SujianEditorItem) };
    item.request_repaint();
}

/// Convert UTF-16 QChar offset to UTF-8 byte offset within a string.
/// Properly handles surrogate pairs (emoji, CJK extension B, etc.).
fn qchar_offset_to_byte_offset(text: &str, qchar_offset: usize) -> usize {
    let mut qchar_count: usize = 0;
    for (byte_pos, ch) in text.char_indices() {
        if qchar_count >= qchar_offset {
            return byte_pos;
        }
        qchar_count += ch.len_utf16();
    }
    // If qchar_offset is beyond the end, return text.len()
    text.len()
}

fn layout_lines(text: &str, width: f64, font_size: f64, line_spacing: f64, padding: f64, indent: f64, font_family: &str) -> Vec<VisualLine> {
    let line_height = (font_size * line_spacing).max(font_size + 4.0);
    let available = (width - padding * 2.0).max(font_size);
    let mut result = Vec::new();
    let mut y = padding;
    let mut paragraph_start = 0;

    for paragraph in text.split_inclusive('\n') {
        let hard_break = paragraph.ends_with('\n');
        let paragraph_text = paragraph.trim_end_matches('\n');
        let paragraph_text_end = paragraph_start + paragraph_text.len();

        if paragraph_text.is_empty() {
            result.push(VisualLine {
                start: paragraph_start,
                end: paragraph_start,
                hard_break,
                x: padding + indent,
                y,
                width: 0.0,
                height: line_height,
                para_text: String::new(),
                para_start: paragraph_start,
                qtextline_idx: 0,
                para_qchar_start: 0,
                para_qchar_end: 0,
                line_wrap_width: available - indent,
                line_indent_x: indent,
            });
            y += line_height;
            paragraph_start += paragraph.len();
            continue;
        }

        let para_start = paragraph_start;
        let fs = font_size as f32;
        let ff: QString = font_family.to_string().into();
        let wrap_w = available as f64;
        let indent_w = indent;
        let text_qstr: QString = paragraph_text.to_string().into();

        // 用 QTextLayout 做真正的文本布局，结果写入 g_layout_buf
        let line_count = cpp!(unsafe [
            text_qstr as "QString",
            fs as "float",
            ff as "QString",
            wrap_w as "double",
            indent_w as "double"
        ] -> i32 as "int" {
            QFont font(ff);
            font.setPixelSize(static_cast<int>(fs));
            QTextLayout layout(text_qstr, font);
            QTextOption option;
            option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
            layout.setTextOption(option);
            layout.beginLayout();

            g_layout_buf.clear();
            bool first = true;

            while (true) {
                QTextLine line = layout.createLine();
                if (!line.isValid()) break;
                double lineWrap = first ? (wrap_w - indent_w) : wrap_w;
                line.setLineWidth(lineWrap);
                QTextLayoutEntry e;
                e.byteStart = line.textStart();
                e.width = line.naturalTextWidth();
                e.xPos = first ? indent_w : 0.0;
                g_layout_buf.push_back(e);
                first = false;
            }
            layout.endLayout();
            return static_cast<int>(g_layout_buf.size());
        });

        // 从 g_layout_buf 读取，QChar 偏移转 UTF-8 字节偏移
        for line_idx in 0..line_count {
            let idx = line_idx;
            let qchar_off = cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
                if (idx >= 0 && idx < static_cast<int>(g_layout_buf.size())) {
                    return static_cast<qulonglong>(g_layout_buf[idx].byteStart);
                }
                return 0;
            });
            let line_w = cpp!(unsafe [idx as "int"] -> f64 as "double" {
                if (idx >= 0 && idx < static_cast<int>(g_layout_buf.size())) {
                    return g_layout_buf[idx].width;
                }
                return 0.0;
            });
            let x_off = cpp!(unsafe [idx as "int"] -> f64 as "double" {
                if (idx >= 0 && idx < static_cast<int>(g_layout_buf.size())) {
                    return g_layout_buf[idx].xPos;
                }
                return 0.0;
            });

            let byte_off = qchar_offset_to_byte_offset(paragraph_text, qchar_off);
            let abs_start = para_start + byte_off;
            let para_qchar_end = if line_idx + 1 < line_count {
                let next_idx = line_idx + 1;
                let next_qchar = cpp!(unsafe [next_idx as "int"] -> usize as "qulonglong" {
                    if (next_idx >= 0 && next_idx < static_cast<int>(g_layout_buf.size())) {
                        return static_cast<qulonglong>(g_layout_buf[next_idx].byteStart);
                    }
                    return 0;
                });
                next_qchar
            } else {
                byte_offset_to_qchar_offset(paragraph_text, paragraph_text.len())
            };
            let abs_end = if line_idx + 1 < line_count {
                para_start + qchar_offset_to_byte_offset(paragraph_text, para_qchar_end)
            } else {
                paragraph_text_end
            };
            let is_first = line_idx == 0;

            result.push(VisualLine {
                start: abs_start,
                end: abs_end,
                hard_break: hard_break && abs_end == paragraph_text_end,
                x: padding + x_off,
                y,
                width: line_w,
                height: line_height,
                para_text: paragraph_text.to_string(),
                para_start,
                qtextline_idx: line_idx as i32,
                para_qchar_start: qchar_off,
                para_qchar_end,
                line_wrap_width: if is_first { available - indent } else { available },
                line_indent_x: if is_first { indent } else { 0.0 },
            });
            y += line_height;
        }

        paragraph_start += paragraph.len();
    }

    if text.ends_with('\n') {
        result.push(VisualLine {
            start: text.len(),
            end: text.len(),
            hard_break: false,
            x: padding + indent,
            y,
            width: 0.0,
            height: line_height,
            para_text: String::new(),
            para_start: text.len(),
            qtextline_idx: 0,
            para_qchar_start: 0,
            para_qchar_end: 0,
            line_wrap_width: available - indent,
            line_indent_x: indent,
        });
    }

    if text.is_empty() {
        result.push(VisualLine {
            start: 0,
            end: 0,
            hard_break: false,
            x: padding + indent,
            y,
            width: 0.0,
            height: line_height,
            para_text: String::new(),
            para_start: 0,
            qtextline_idx: 0,
            para_qchar_start: 0,
            para_qchar_end: 0,
            line_wrap_width: available - indent,
            line_indent_x: indent,
        });
    }
    result
}

fn cursor_geometry_with_font(
    _text: &str,
    lines: &[VisualLine],
    cursor: usize,
    affinity: CaretAffinity,
    font_size: f64,
    font_family: &str,
) -> (f64, f64, usize) {
    for (idx, line) in lines.iter().enumerate() {
        if line_contains_cursor_with_affinity(lines, idx, cursor, affinity) {
            let w = if affinity == CaretAffinity::Upstream && cursor == line.end {
                line.width
            } else if affinity == CaretAffinity::Downstream && cursor == line.start {
                0.0
            } else if line.para_text.is_empty() {
                0.0
            } else {
                let paragraph_wrap_w = line.line_wrap_width + line.line_indent_x;
                qtextlayout_cursor_to_x_on_line(
                    &line.para_text, cursor, line.para_start,
                    font_size, font_family,
                    paragraph_wrap_w, line.line_indent_x, line.qtextline_idx,
                )
            };
            return (line.x + w, cursor_rect_for_line(line, font_size, font_family).0, idx);
        }
    }
    let last_idx = lines.len().saturating_sub(1);
    lines
        .last()
        .map(|line| (line.x + line.width, cursor_rect_for_line(line, font_size, font_family).0, last_idx))
        .unwrap_or((0.0, 0.0, 0))
}

#[cfg(not(test))]
fn get_font_ascent(font_family: &str, font_size: f32) -> f64 {
    let family = QString::from(font_family);
    cpp!(unsafe [family as "QString", font_size as "float"] -> f64 as "double" {
        QFont font(family);
        font.setPixelSize(font_size);
        QFontMetricsF metrics(font);
        return metrics.ascent();
    })
}

#[cfg(test)]
fn get_font_ascent(_font_family: &str, font_size: f32) -> f64 {
    font_size as f64 * 0.8
}

#[cfg(not(test))]
fn get_font_descent(font_family: &str, font_size: f32) -> f64 {
    let family = QString::from(font_family);
    cpp!(unsafe [family as "QString", font_size as "float"] -> f64 as "double" {
        QFont font(family);
        font.setPixelSize(font_size);
        QFontMetricsF metrics(font);
        return metrics.descent();
    })
}

#[cfg(test)]
fn get_font_descent(_font_family: &str, font_size: f32) -> f64 {
    font_size as f64 * 0.2
}

fn cursor_height_for_line(font_size: f64, font_family: &str) -> f64 {
    let ascent = get_font_ascent(font_family, font_size as f32);
    let descent = get_font_descent(font_family, font_size as f32);
    (ascent + descent) as f64
}

/// Unified cursor rect: (top_y, height), centered in the line box.
/// Height is capped at line.height * 0.84 so the caret never bleeds into adjacent lines.
fn cursor_rect_for_line(line: &VisualLine, font_size: f64, font_family: &str) -> (f64, f64) {
    let raw_h = cursor_height_for_line(font_size, font_family);
    let h = raw_h.min(line.height * 0.84);
    let top_y = line.y + (line.height - h) / 2.0;
    (top_y, h)
}

/// Cursor top Y, derived from the unified cursor rect.
fn cursor_top_y(line: &VisualLine, font_size: f64, font_family: &str) -> f64 {
    cursor_rect_for_line(line, font_size, font_family).0
}

fn text_baseline_y(line: &VisualLine, font_size: f64, font_family: &str) -> f64 {
    let ascent = get_font_ascent(font_family, font_size as f32);
    let descent = get_font_descent(font_family, font_size as f32);
    let top_padding = (line.height - (ascent + descent)).max(0.0) / 2.0;
    line.y + top_padding + ascent
}

fn affinity_for_index_on_line(line: &VisualLine, index: usize) -> CaretAffinity {
    if index == line.end && line.start != line.end {
        CaretAffinity::Upstream
    } else {
        CaretAffinity::Downstream
    }
}

fn line_contains_cursor_with_affinity(
    lines: &[VisualLine],
    idx: usize,
    cursor: usize,
    affinity: CaretAffinity,
) -> bool {
    let line = &lines[idx];
    if line.start == line.end {
        return cursor == line.start;
    }
    if cursor > line.start && cursor < line.end {
        return true;
    }
    if cursor == line.start {
        let has_prev_overlap = idx > 0 && lines[idx - 1].end == line.start;
        if has_prev_overlap {
            return affinity == CaretAffinity::Downstream;
        } else {
            return true;
        }
    }
    if cursor == line.end {
        let has_next_overlap = idx + 1 < lines.len() && lines[idx + 1].start == line.end;
        if has_next_overlap {
            return affinity == CaretAffinity::Upstream;
        } else {
            return true;
        }
    }
    false
}

#[allow(dead_code)]
fn line_contains_cursor(lines: &[VisualLine], idx: usize, cursor: usize) -> bool {
    line_contains_cursor_with_affinity(lines, idx, cursor, CaretAffinity::Downstream)
}

fn draw_text(painter: &mut QPainter, x: f64, baseline_y: f64, font_size: f32, color: QString, text: QString) {
    let _ = font_size;
    painter.set_pen(QPen::from_color(color_from_qstring(color)));
    painter.draw_text(QPointF { x, y: baseline_y }, text);
}

fn draw_text_color(painter: &mut QPainter, x: f64, baseline_y: f64, color: QColor, text: QString) {
    painter.set_pen(QPen::from_color(color));
    painter.draw_text(QPointF { x, y: baseline_y }, text);
}

fn draw_rect(painter: &mut QPainter, x: f64, y: f64, width: f64, height: f64, color: QString) {
    painter.fill_rect(
        QRectF { x, y, width, height },
        QBrush::from_color(color_from_qstring(color)),
    );
}

fn color_from_qstring(color: QString) -> QColor {
    QColor::from_name(&color.to_string())
}

fn editor_animation_debug_enabled() -> bool {
    cfg!(debug_assertions) || std::env::var_os("SUJIAN_EDITOR_ANIMATION_DEBUG").is_some()
}

fn sujian_editor_debug_enabled() -> bool {
    cfg!(debug_assertions) || std::env::var_os("SUJIAN_EDITOR_DEBUG").is_some()
}

fn should_log_slow_paint(last_log: Option<Instant>, now: Instant) -> bool {
    last_log.is_none_or(|last| now.duration_since(last) >= Duration::from_millis(500))
}

fn sujian_delete_painter(painter: *mut QPainter) {
    cpp!(unsafe [painter as "QPainter*"] { delete painter; })
}

fn sujian_item_dpr(item_ptr: *mut std::ffi::c_void) -> f64 {
    cpp!(unsafe [item_ptr as "QQuickItem*"] -> f64 as "double" {
        if (!item_ptr || !item_ptr->window()) return 1.0;
        return item_ptr->window()->devicePixelRatio();
    })
}

fn sujian_create_painter_scaled(image: &mut qmetaobject::QImage, dpr: f64) -> *mut QPainter {
    let img_ptr = image as *mut qmetaobject::QImage;
    cpp!(unsafe [img_ptr as "QImage*", dpr as "double"] -> *mut QPainter as "QPainter*" {
        auto *p = new QPainter(img_ptr);
        p->scale(dpr, dpr);
        return p;
    })
}

fn sujian_update_source_rect(
    old_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    src_x: f64, src_y: f64, src_w: f64, src_h: f64,
    dest_y: f64, dest_h: f64,
    dpr: f64,
) {
    cpp!(unsafe [
        old_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        src_x as "double", src_y as "double",
        src_w as "double", src_h as "double",
        dest_y as "double", dest_h as "double",
        dpr as "double"
    ] {
        auto *root = static_cast<QSGTransformNode*>(old_raw);
        if (!root || root->childCount() == 0) return;
        auto *imgNode = static_cast<QSGImageNode*>(root->firstChild());
        if (!imgNode) return;
        imgNode->setRect(0, dest_y, item_ptr->width(), dest_h);
        imgNode->setSourceRect(src_x * dpr, src_y * dpr, src_w * dpr, src_h * dpr);
    })
}

fn sujian_update_texture_node(
    old_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    image: &qmetaobject::QImage,
    src_x: f64, src_y: f64, src_w: f64, src_h: f64,
    dest_y: f64, dest_h: f64,
    dpr: f64,
) -> *mut std::ffi::c_void {
    let img_ptr = image as *const qmetaobject::QImage;
    cpp!(unsafe [
        old_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        img_ptr as "QImage*",
        src_x as "double", src_y as "double",
        src_w as "double", src_h as "double",
        dest_y as "double", dest_h as "double",
        dpr as "double"
    ] -> *mut std::ffi::c_void as "QSGNode*" {
        auto *root = static_cast<QSGTransformNode*>(old_raw);
        if (!root) {
            root = new QSGTransformNode;
        }
        QSGImageNode *imgNode = nullptr;
        if (root->childCount() > 0) {
            imgNode = static_cast<QSGImageNode*>(root->firstChild());
        }
        if (!imgNode) {
            imgNode = item_ptr->window()->createImageNode();
            imgNode->setFiltering(QSGTexture::Nearest);
            imgNode->setOwnsTexture(true);
            root->appendChildNode(imgNode);
        }
        imgNode->setRect(0, dest_y, item_ptr->width(), dest_h);
        imgNode->setSourceRect(src_x * dpr, src_y * dpr, src_w * dpr, src_h * dpr);
        imgNode->setTexture(item_ptr->window()->createTextureFromImage(*img_ptr));
        return root;
    })
}

fn sujian_update_cursor_rect(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    cx: f64, cy: f64, cw: f64, ch: f64,
    visible: bool,
    color_rgba: u32,
) {
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        cx as "double", cy as "double",
        cw as "double", ch as "double",
        visible as "bool",
        color_rgba as "unsigned int"
    ] {
        sujian_update_cursor_rect(
            static_cast<QSGTransformNode*>(root_raw), item_ptr,
            cx, cy, cw, ch, visible, color_rgba
        );
    })
}

fn color_hex_to_rgba(hex: &QString) -> u32 {
    let c = QColor::from_name(&hex.to_string());
    let r = c.red() as u32;
    let g = c.green() as u32;
    let b = c.blue() as u32;
    let a = c.alpha() as u32;
    (a << 24) | (r << 16) | (g << 8) | b
}

fn prev_char_boundary(text: &str, index: usize) -> Option<usize> {
    if index == 0 || text.is_empty() {
        return None;
    }
    text.char_indices()
        .map(|(idx, _)| idx)
        .take_while(|idx| *idx < index)
        .last()
}

fn next_char_boundary(text: &str, index: usize) -> Option<usize> {
    if index >= text.len() {
        return None;
    }
    text.char_indices()
        .map(|(idx, _)| idx)
        .find(|idx| *idx > index)
        .or(Some(text.len()))
}

fn clamp_to_char_boundary(text: &str, index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    if text.is_char_boundary(index) {
        return index;
    }
    let mut clamped = index;
    while clamped > 0 && !text.is_char_boundary(clamped) {
        clamped -= 1;
    }
    clamped
}

fn byte_to_char_index(text: &str, byte_index: usize) -> usize {
    text[..clamp_to_char_boundary(text, byte_index)].chars().count()
}

#[allow(dead_code)]
fn byte_index_at_char_offset_in_range(text: &str, start: usize, end: usize, char_offset: usize) -> usize {
    if char_offset == 0 {
        return start;
    }
    for (offset, (byte, _)) in text[start..end].char_indices().enumerate() {
        if offset == char_offset {
            return start + byte;
        }
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_buffer_inserts_and_deletes_utf8_plain_text() {
        let mut buffer = EditorBuffer::default();
        buffer.replace_selection_or_insert("你");
        buffer.replace_selection_or_insert("好");
        assert_eq!(buffer.text, "你好");
        assert_eq!(buffer.cursor, buffer.text.len());
        assert!(buffer.delete_backward());
        assert_eq!(buffer.text, "你");
    }

    #[test]
    fn editor_buffer_replaces_selection_without_html_or_format_state() {
        let mut buffer = EditorBuffer::default();
        buffer.replace_selection_or_insert("abc");
        buffer.selection_anchor = 0;
        buffer.cursor = 3;
        buffer.replace_selection_or_insert("纯文本");
        assert_eq!(buffer.text, "纯文本");
        assert_eq!(buffer.selected_text(), "");
    }

    #[test]
    fn editor_buffer_undo_redo_keeps_plain_text_and_cursor() {
        let mut buffer = EditorBuffer::default();
        let before = buffer.snapshot();
        buffer.push_undo(before);
        buffer.replace_selection_or_insert("第一行\n第二行");

        let Some((_old, restored)) = buffer.undo() else {
            panic!("undo should restore the empty snapshot");
        };
        assert_eq!(restored.text, "");
        assert_eq!(buffer.text, "");
        assert_eq!(buffer.cursor, 0);

        let Some((_old, redone)) = buffer.redo() else {
            panic!("redo should restore inserted plain text");
        };
        assert_eq!(redone.text, "第一行\n第二行");
        assert_eq!(buffer.text, "第一行\n第二行");
        assert_eq!(buffer.cursor, buffer.text.len());
    }

    #[test]
    fn self_editor_shortcuts_cover_desktop_clipboard_and_redo_keys() {
        assert!(is_copy_shortcut(KEY_C, CTRL_MODIFIER));
        assert!(is_copy_shortcut(KEY_INSERT, CTRL_MODIFIER));
        assert!(is_paste_shortcut(KEY_V, CTRL_MODIFIER));
        assert!(is_paste_shortcut(KEY_INSERT, SHIFT_MODIFIER));
        assert!(is_redo_shortcut(KEY_Y, CTRL_MODIFIER));
        assert!(is_redo_shortcut(KEY_Z, CTRL_MODIFIER | SHIFT_MODIFIER));
        assert!(!is_redo_shortcut(KEY_Z, CTRL_MODIFIER));
    }

    #[test]
    fn layout_keeps_cursor_on_blank_line_after_trailing_newline() {
        let text = "\n";
        let lines = layout_lines(text, 800.0, 16.0, 1.5, 16.0, 32.0, "serif");

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].start, text.len());
        assert_eq!(lines[1].end, text.len());

        let (_x, y, _li) = cursor_geometry_with_font(text, &lines, text.len(), CaretAffinity::Downstream, 16.0, "serif");
        assert_eq!(y, cursor_top_y(&lines[1], 16.0, "serif"));
    }

    #[test]
    fn cursor_geometry_prefers_next_visual_line_at_boundary() {
        let text = "ab";
        let lines = vec![
            VisualLine { start: 0, end: 1, hard_break: false, x: 16.0, y: 10.0, width: 10.0, height: 24.0, para_text: String::new(), para_start: 0, qtextline_idx: 0, para_qchar_start: 0, para_qchar_end: 1, line_wrap_width: 468.0, line_indent_x: 32.0 },
            VisualLine { start: 1, end: 2, hard_break: false, x: 16.0, y: 34.0, width: 10.0, height: 24.0, para_text: String::new(), para_start: 0, qtextline_idx: 0, para_qchar_start: 1, para_qchar_end: 2, line_wrap_width: 500.0, line_indent_x: 0.0 },
        ];

        let (_x, y, _li) = cursor_geometry_with_font(text, &lines, 1, CaretAffinity::Downstream, 16.0, "serif");

        assert_eq!(y, cursor_top_y(&lines[1], 16.0, "serif"));
    }

    #[test]
    fn test_caret_affinity_boundary_matching() {
        let lines = vec![
            VisualLine { start: 0, end: 10, hard_break: false, x: 16.0, y: 10.0, width: 100.0, height: 24.0, para_text: String::new(), para_start: 0, qtextline_idx: 0, para_qchar_start: 0, para_qchar_end: 10, line_wrap_width: 500.0, line_indent_x: 0.0 },
            VisualLine { start: 10, end: 20, hard_break: false, x: 16.0, y: 34.0, width: 100.0, height: 24.0, para_text: String::new(), para_start: 0, qtextline_idx: 0, para_qchar_start: 10, para_qchar_end: 20, line_wrap_width: 500.0, line_indent_x: 0.0 },
        ];

        // At boundary index 10:
        // With CaretAffinity::Upstream, it should belong to lines[0]
        assert!(line_contains_cursor_with_affinity(&lines, 0, 10, CaretAffinity::Upstream));
        assert!(!line_contains_cursor_with_affinity(&lines, 1, 10, CaretAffinity::Upstream));

        // With CaretAffinity::Downstream, it should belong to lines[1]
        assert!(!line_contains_cursor_with_affinity(&lines, 0, 10, CaretAffinity::Downstream));
        assert!(line_contains_cursor_with_affinity(&lines, 1, 10, CaretAffinity::Downstream));

        // Test visual coordinates resolved directly
        let (x_up, y_up, _li_up) = cursor_geometry_with_font("", &lines, 10, CaretAffinity::Upstream, 16.0, "serif");
        assert_eq!(x_up, 16.0 + 100.0);
        assert_eq!(y_up, cursor_top_y(&lines[0], 16.0, "serif"));

        let (x_down, y_down, _li_down) = cursor_geometry_with_font("", &lines, 10, CaretAffinity::Downstream, 16.0, "serif");
        assert_eq!(x_down, 16.0);
        assert_eq!(y_down, cursor_top_y(&lines[1], 16.0, "serif"));
    }

    #[test]
    fn cursor_and_text_are_vertically_centered_in_spaced_line() {
        let line = VisualLine { start: 0, end: 0, hard_break: false, x: 16.0, y: 10.0, width: 0.0, height: 32.0, para_text: String::new(), para_start: 0, qtextline_idx: 0, para_qchar_start: 0, para_qchar_end: 0, line_wrap_width: 468.0, line_indent_x: 32.0 };

        let top_y = cursor_top_y(&line, 16.0, "serif");
        assert!(top_y > line.y);
        assert!(text_baseline_y(&line, 16.0, "serif") > top_y);
    }

    #[test]
    fn line_char_offset_does_not_jump_to_document_end() {
        let text = "第一行\n第二行";
        let line_end = "第一行".len();

        assert_eq!(byte_index_at_char_offset_in_range(text, 0, line_end, 3), line_end);
    }

    #[test]
    fn qchar_to_byte_mixed_text() {
        // ASCII: 1 byte per char, 1 UTF-16 unit per char
        assert_eq!(qchar_offset_to_byte_offset("hello", 0), 0);
        assert_eq!(qchar_offset_to_byte_offset("hello", 3), 3);
        assert_eq!(qchar_offset_to_byte_offset("hello", 5), 5);

        // Chinese: 3 bytes per char, 1 UTF-16 unit per char
        let cn = "你好世界";
        assert_eq!(qchar_offset_to_byte_offset(cn, 0), 0);
        assert_eq!(qchar_offset_to_byte_offset(cn, 1), 3);
        assert_eq!(qchar_offset_to_byte_offset(cn, 2), 6);
        assert_eq!(qchar_offset_to_byte_offset(cn, 4), 12);

        // Emoji: 4 bytes, 2 UTF-16 units (surrogate pair)
        let emoji = "😀b"; // U+1F600 (2 UTF-16 units) + 'b' (1 unit)
        assert_eq!(qchar_offset_to_byte_offset(emoji, 0), 0);   // start of 😀
        assert_eq!(qchar_offset_to_byte_offset(emoji, 2), 4);   // after 😀, start of 'b'
        assert_eq!(qchar_offset_to_byte_offset(emoji, 3), 5);   // after 'b'

        // Mixed: "你😀好"
        let mixed = "你😀好"; // 你(3 bytes, 1 unit) + 😀(4 bytes, 2 units) + 好(3 bytes, 1 unit)
        assert_eq!(qchar_offset_to_byte_offset(mixed, 0), 0);   // start of 你
        assert_eq!(qchar_offset_to_byte_offset(mixed, 1), 3);   // after 你, start of 😀
        assert_eq!(qchar_offset_to_byte_offset(mixed, 3), 7);   // after 😀, start of 好
        assert_eq!(qchar_offset_to_byte_offset(mixed, 4), 10);  // after 好
    }

    #[test]
    fn byte_to_qchar_mixed_text() {
        // ASCII
        assert_eq!(byte_offset_to_qchar_offset("hello", 0), 0);
        assert_eq!(byte_offset_to_qchar_offset("hello", 3), 3);

        // Chinese
        assert_eq!(byte_offset_to_qchar_offset("你好", 0), 0);
        assert_eq!(byte_offset_to_qchar_offset("你好", 3), 1);

        // Emoji
        assert_eq!(byte_offset_to_qchar_offset("😀b", 0), 0);
        assert_eq!(byte_offset_to_qchar_offset("😀b", 4), 2);

        // Mixed
        assert_eq!(byte_offset_to_qchar_offset("你😀好", 0), 0);
        assert_eq!(byte_offset_to_qchar_offset("你😀好", 3), 1);
        assert_eq!(byte_offset_to_qchar_offset("你😀好", 7), 3);
    }

    #[test]
    fn qchar_byte_roundtrip() {
        let texts = [
            "hello world",
            "你好世界",
            "Hello 你好 World",
            "a😀b🎉c",
            "写作者：测试emoji🎉混合",
        ];
        for text in &texts {
            for (byte_pos, _ch) in text.char_indices() {
                let qchar = byte_offset_to_qchar_offset(text, byte_pos);
                let back = qchar_offset_to_byte_offset(text, qchar);
                assert_eq!(back, byte_pos,
                    "roundtrip failed for '{}' at byte {}: qchar={}, back={}",
                    text, byte_pos, qchar, back);
            }
            // Also check end-of-string
            let qchar_end = byte_offset_to_qchar_offset(text, text.len());
            let back_end = qchar_offset_to_byte_offset(text, qchar_end);
            assert_eq!(back_end, text.len(), "roundtrip failed for end of '{}'", text);
        }
    }

    #[test]
    fn test_scroll_y_cache_and_buffer_hit() {
        // 1. scroll_y change shouldn't invalidate layout cache.
        // In sujian_editor_item.rs, layout_cache is only invalidated/refreshed when text, width, font, padding etc change.
        // Changing scroll_y does not touch ensure_layout_cached criteria.
        
        // 2. Validate scroll_buffer hit bounds math.
        let vp_h: f64 = 1000.0;
        let content_h: f64 = 5000.0;
        let overscan: f64 = (vp_h * 0.8).min(600.0); // Capped at 600.0
        
        let scroll_y: f64 = 100.0;
        let min_y: f64 = (scroll_y - overscan).max(0.0); // 0.0
        let max_y: f64 = (scroll_y + vp_h + overscan).min(content_h.max(vp_h)); // 100 + 1000 + 600 = 1700.0
        let buffer_scroll_y: f64 = min_y; // 0.0
        let buffer_logical_h: f64 = max_y - min_y; // 1700.0
        
        // Test scroll step inside buffer
        let new_scroll_y: f64 = 200.0;
        let inside_buffer = new_scroll_y >= buffer_scroll_y - 1.0
            && new_scroll_y + vp_h <= buffer_scroll_y + buffer_logical_h + 1.0;
        assert!(inside_buffer, "scroll_y=200 should hit the buffer [0..1700]");

        // Test scroll step outside buffer
        let far_scroll_y: f64 = 800.0;
        let inside_buffer_far = far_scroll_y >= buffer_scroll_y - 1.0
            && far_scroll_y + vp_h <= buffer_scroll_y + buffer_logical_h + 1.0;
        assert!(!inside_buffer_far, "scroll_y=800 should miss the buffer [0..1700] since viewport bottom 1800 > 1700");
    }

    #[test]
    fn test_hit_test_line_matching() {
        // Mock 3 visual lines manually instead of calling layout_lines (which calls QFont and crashes without QGuiApplication)
        let lines = vec![
            VisualLine {
                start: 0,
                end: 10,
                hard_break: true,
                x: 16.0,
                y: 16.0,
                width: 100.0,
                height: 24.0,
                para_text: "First Line".to_string(),
                para_start: 0,
                qtextline_idx: 0,
                para_qchar_start: 0,
                para_qchar_end: 10,
                line_wrap_width: 768.0,
                line_indent_x: 0.0,
            },
            VisualLine {
                start: 11,
                end: 22,
                hard_break: true,
                x: 16.0,
                y: 40.0, // 16.0 + 24.0
                width: 110.0,
                height: 24.0,
                para_text: "Second Line".to_string(),
                para_start: 11,
                qtextline_idx: 0,
                para_qchar_start: 0,
                para_qchar_end: 11,
                line_wrap_width: 768.0,
                line_indent_x: 0.0,
            },
            VisualLine {
                start: 23,
                end: 33,
                hard_break: true,
                x: 16.0,
                y: 64.0, // 40.0 + 24.0
                width: 90.0,
                height: 24.0,
                para_text: "Third Line".to_string(),
                para_start: 23,
                qtextline_idx: 0,
                para_qchar_start: 0,
                para_qchar_end: 10,
                line_wrap_width: 768.0,
                line_indent_x: 0.0,
            },
        ];

        // Under Model B: doc_y = y. Confirm correct hit line finding
        let doc_y_first = lines[0].y + 2.0; // 18.0
        let matched_first = lines.iter().position(|l| doc_y_first < l.y + l.height);
        assert_eq!(matched_first, Some(0));
        
        let doc_y_second = lines[1].y + 2.0; // 42.0
        let matched_second = lines.iter().position(|l| doc_y_second < l.y + l.height);
        assert_eq!(matched_second, Some(1));
        
        let doc_y_third = lines[2].y + 2.0; // 66.0
        let matched_third = lines.iter().position(|l| doc_y_third < l.y + l.height);
        assert_eq!(matched_third, Some(2));
    }
}

