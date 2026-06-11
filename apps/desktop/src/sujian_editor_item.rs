// =============================================================================
// sujian_editor_item.rs — Desktop self-rendered editor item
// =============================================================================

use cpp::cpp;
use crate::editor::layout::{
    affinity_for_index_on_line, calculate_cursor_x_for_line, cursor_height_for_line, cursor_rect_for_line,
    cursor_top_y, line_contains_cursor_with_affinity, qtextlayout_cursor_to_x,
    qtextlayout_glyph_positions_on_line, text_baseline_y, CaretAffinity, CursorLayoutRect,
    EditorLayout, LayoutParams, LayoutSnapshot, VisualLine,
};
use qmetaobject::prelude::*;
use qmetaobject::{QBrush, QColor, QLineF, QMouseEvent, QPainter, QPainterRenderHint, QPen, QPointF, QQuickItem, QRectF, QString};
use std::cell::Cell;
use std::time::{Duration, Instant};
use writer_core::editor::{EditorCursor, EditorEngine, EditorSelection, EditorTransactionCause};

cpp! {{
    #include <QtGui/QFont>
    #include <QtGui/QPainter>
    #include <QtGui/QClipboard>
    #include <QtGui/QInputMethodEvent>
    #include <QtGui/QKeyEvent>
    #include <QtGui/QMouseEvent>
    #include <QtQuick/QSGSimpleTextureNode>
    #include <QtQuick/QSGTexture>
    #include <QtQuick/QSGTransformNode>
    #include <QtQuick/QSGImageNode>
    #include <QtQuick/QSGRectangleNode>
    #include <QGuiApplication>

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

    void sujian_clean_cursor_nodes(QSGNode *root) {
        if (!root) return;
        auto *transformNode = static_cast<QSGTransformNode*>(root);
        while (transformNode->childCount() > 1) {
            QSGNode *child = transformNode->lastChild();
            transformNode->removeChildNode(child);
            delete child;
        }
    }

    void sujian_update_cursor_rect(QSGTransformNode *root, QQuickItem *item,
        double cx, double cy, double cw, double ch, bool visible, unsigned int color_rgba) {
        if (!root) return;
        // Find existing cursor node (always last child if present)
        QSGRectangleNode *cursorNode = nullptr;
        if (root->childCount() > 1) {
            cursorNode = dynamic_cast<QSGRectangleNode*>(root->lastChild());
        }
        // Create if needed
        if (!cursorNode) {
            cursorNode = item->window()->createRectangleNode();
            root->appendChildNode(cursorNode);
        }
        if (!visible) {
            // Move offscreen instead of deleting
            cursorNode->setRect(QRectF(-100000, -100000, 0, 0));
            cursorNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
            return;
        }
        // Visible: position and mark dirty
        cursorNode->setRect(QRectF(cx, cy, cw, ch));
        cursorNode->setColor(QColor::fromRgba(color_rgba));
        cursorNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
    }
}}

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

/// 滚动缓冲区 — 缓存超大 QImage + 纹理，滚动时只更新 rect 不重绘
struct ScrollBuffer {
    image: qmetaobject::QImage,
    buffer_scroll_y: f64,
    buffer_content_h: f64,
    buffer_logical_h: f64,
    dpr: f64,
}

impl ScrollBuffer {
    fn contains_viewport(&self, scroll_y: f64, vp_h: f64) -> bool {
        // Low watermark threshold: if the viewport is within 0.5 * vp_h of the buffer edges,
        // we consider it as not containing the viewport (triggering pre-render).
        // However, we must also consider the document boundaries. If the buffer already covers
        // the top of the document (buffer_scroll_y == 0.0) or the bottom of the document
        // (buffer_scroll_y + buffer_logical_h >= buffer_content_h), then we shouldn't trigger rebuild
        // just because we are close to those boundaries, since we cannot scroll further anyway.
        let threshold = vp_h * 0.5;
        
        let near_top = scroll_y < self.buffer_scroll_y + threshold;
        let near_bottom = scroll_y + vp_h > self.buffer_scroll_y + self.buffer_logical_h - threshold;
        
        if near_top && self.buffer_scroll_y > 0.1 {
            return false;
        }
        if near_bottom && (self.buffer_scroll_y + self.buffer_logical_h) < self.buffer_content_h - 0.1 {
            return false;
        }
        
        // Strict boundary check (outside buffer entirely)
        if scroll_y < self.buffer_scroll_y || scroll_y + vp_h > self.buffer_scroll_y + self.buffer_logical_h {
            return false;
        }
        
        true
    }

    fn clamp_source_rect(&self, scroll_y: f64, vp_h: f64) -> (f64, f64) {
        let mut src_y = scroll_y - self.buffer_scroll_y;
        let mut src_h = vp_h;

        if src_y < 0.0 {
            src_y = 0.0;
        }
        
        if src_y + src_h > self.buffer_logical_h {
            if src_h > self.buffer_logical_h {
                src_h = self.buffer_logical_h;
            }
            if src_y + src_h > self.buffer_logical_h {
                src_y = self.buffer_logical_h - src_h;
            }
        }
        
        // DPR check to make sure physical coordinates do not exceed QImage physical size
        let phys_h = (self.image.size().height as f64).max(1.0);
        let dpr = self.dpr;
        
        let mut phys_src_y = src_y * dpr;
        let mut phys_src_h = src_h * dpr;
        
        if phys_src_y < 0.0 {
            phys_src_y = 0.0;
        }
        if phys_src_y + phys_src_h > phys_h {
            if phys_src_h > phys_h {
                phys_src_h = phys_h;
            }
            if phys_src_y + phys_src_h > phys_h {
                phys_src_y = phys_h - phys_src_h;
            }
        }
        
        // Convert back to logical
        src_y = phys_src_y / dpr;
        src_h = phys_src_h / dpr;

        (src_y, src_h)
    }
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
    editor_layout: EditorLayout,
    render_dirty: bool,
    scroll_buffer: Option<ScrollBuffer>,
    last_slow_paint_log: Option<Instant>,
    cursor_visual_x: f64,
    cursor_visual_y: f64,
    cursor_visual_h: f64,
    cursor_visible: bool,
    cursor_dirty: bool,
    current_cursor_affinity: CaretAffinity,
    current_visual_line_id: Option<usize>,
    last_cursor_scroll_y: f64,
    ime_cursor_rect_h: f64,
    force_snap_next_cursor: bool,
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
            editor_layout: EditorLayout::default(),
            render_dirty: true,
            scroll_buffer: None,
            last_slow_paint_log: None,
            cursor_visual_x: 0.0,
            cursor_visual_y: 0.0,
            cursor_visual_h: 0.0,
            cursor_visible: false,
            cursor_dirty: true,
            current_cursor_affinity: CaretAffinity::Downstream,
            current_visual_line_id: None,
            last_cursor_scroll_y: 0.0,
            ime_cursor_rect_h: 0.0,
            force_snap_next_cursor: false,
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
        let obj = self.get_cpp_object();
        if obj.is_null() {
            return 800.0;
        }
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
        self.adjust_affinity_at_wrap_boundary();
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
        self.adjust_affinity_at_wrap_boundary();
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
        self.force_snap_next_cursor = true;
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
            self.force_snap_next_cursor = true;
            self.request_repaint();
            return;
        }
        if !value {
            self.force_snap_next_cursor = true;
            self.update_cursor_visual_position();
            self.request_repaint();
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
        // Viewport coordinates: target_cursor_y is already viewport-relative
        self.target_cursor_y as f32
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
        self.adjust_affinity_at_wrap_boundary();
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
        self.adjust_affinity_at_wrap_boundary();
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
        self.adjust_affinity_at_wrap_boundary();
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
        self.adjust_affinity_at_wrap_boundary();
        let new = self.buffer.snapshot();

        self.insert_animation = None;
        self.delete_animation = None;

        self.record_transaction(old, new, EditorTransactionCause::Delete, true);
        self.emit_content_changed();
    }

    fn select_all(&mut self) {
        self.buffer.select_all();
        self.adjust_affinity_at_wrap_boundary();
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
        self.adjust_affinity_at_wrap_boundary();
        self.record_transaction(old, new, EditorTransactionCause::Undo, true);
        self.emit_content_changed();
    }

    fn redo(&mut self) {
        let Some((old, new)) = self.buffer.redo() else {
            return;
        };
        self.adjust_affinity_at_wrap_boundary();
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
        self.force_snap_next_cursor = true;
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
        self.cursor_dirty = true;
        self.request_repaint();
    }

    fn drag_select_at(&mut self, x: f32, y: f32) {
        let (index, affinity) = self.hit_test(x as f64, y as f64);
        self.current_cursor_affinity = affinity;
        self.force_snap_next_cursor = true;
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
        self.editor_layout.invalidate();
        self.scroll_buffer = None;
    }

    fn layout_params(&self, width: f64) -> LayoutParams {
        LayoutParams {
            width,
            font_size: self.current_font_pixel_size,
            font_family: self.current_font_family.to_string(),
            line_spacing: self.current_line_spacing,
            text_indent: self.current_text_indent,
            padding: self.current_padding,
        }
    }

    fn layout_snapshot(&mut self, width: f64) -> LayoutSnapshot {
        let params = self.layout_params(width);
        self.editor_layout.snapshot(&self.buffer.text, params).clone()
    }

    fn ensure_layout_cached(&mut self, width: f64) -> &Vec<VisualLine> {
        let params = self.layout_params(width);
        &self.editor_layout.snapshot(&self.buffer.text, params).lines
    }

    fn adjust_affinity_at_wrap_boundary(&mut self) {
        let width = self.bounding_width();
        let lines = self.ensure_layout_cached(width).clone();
        let cursor = self.buffer.cursor;

        let is_wrap_boundary = lines.iter().enumerate().any(|(idx, line)| {
            idx + 1 < lines.len() && line.end == cursor && lines[idx + 1].start == cursor
        });

        if is_wrap_boundary {
            self.current_cursor_affinity = CaretAffinity::Upstream;
        } else {
            self.current_cursor_affinity = CaretAffinity::Downstream;
        }
    }

    fn editor_layout_cursor_rect(
        &mut self,
        cursor_byte: usize,
        affinity: CaretAffinity,
        scroll_y: f64,
    ) -> CursorLayoutRect {
        let width = self.bounding_width();
        let snapshot = self.layout_snapshot(width);
        self.editor_layout.caret_rect(
            &snapshot,
            cursor_byte,
            affinity,
            scroll_y,
            self.current_viewport_height.max(1.0) as f64,
        )
    }

    fn hit_test(&mut self, x: f64, y: f64) -> (usize, CaretAffinity) {
        let width = self.bounding_width();
        let snapshot = self.layout_snapshot(width);
        let scroll_y = self.current_scroll_y as f64;
        let (index, affinity) = self.editor_layout.hit_test(&snapshot, x, y, scroll_y);
        if sujian_editor_debug_enabled() {
            eprintln!(
                "hit_test: mouse_x={:.1}, mouse_y={:.1}, current_scroll_y={:.1}, clamped_index={}, affinity={:?}",
                x, y, self.current_scroll_y, index, affinity
            );
        }
        (index, affinity)
    }

    fn index_at_line_x(&self, line: &VisualLine, x: f64) -> usize {
        let snapshot = self
            .editor_layout
            .cache()
            .expect("index_at_line_x requires an existing layout snapshot");
        self.editor_layout.index_at_line_x(snapshot, line, x)
    }

    fn cursor_line_and_x(&self, lines: &[VisualLine]) -> Option<(usize, f64)> {
        let snapshot = self
            .editor_layout
            .cache()
            .expect("cursor_line_and_x requires an existing layout snapshot");
        debug_assert_eq!(lines.len(), snapshot.lines.len());
        self.editor_layout
            .cursor_line_and_x(snapshot, self.buffer.cursor, self.current_cursor_affinity)
    }

    /// 给定字节范围 [byte_start, byte_end)，返回每个字形的矩形信息。
    /// 使用 QTextLayout 精确定位每个字形。
    #[allow(dead_code)]
    fn glyph_rects_for_range(&self, byte_start: usize, byte_end: usize) -> Vec<AnimatedGlyph> {
        let Some(cache) = self.editor_layout.cache() else {
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
        let snapshot = self.layout_snapshot(width);
        let font_size = snapshot.font_size as f64;
        let (target_x, target_y, target_line_id) =
            self.editor_layout
                .cursor_geometry(&snapshot, self.buffer.cursor, self.current_cursor_affinity);
        let cursor_h = if let Some(line) = snapshot.lines.iter().find(|line| line.id == target_line_id) {
            cursor_rect_for_line(line, font_size, &snapshot.font_family).1
        } else {
            cursor_height_for_line(font_size, &snapshot.font_family)
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
        node: qmetaobject::scenegraph::SGNode<qmetaobject::scenegraph::ContainerNode>,
    ) -> qmetaobject::scenegraph::SGNode<qmetaobject::scenegraph::ContainerNode> {
        use qmetaobject::scenegraph::SGNode;

        let frame_start = Instant::now();

        // 1. 每次进入 update_paint_node()，先调用一次 update_cursor_visual_position() 算出最新的 cursor_visual_x/y
        self.update_cursor_visual_position();

        let item_ptr = self.get_cpp_object();
        let dpr = if !item_ptr.is_null() { sujian_item_dpr(item_ptr) } else { 1.0 };
        let root_raw = node.into_raw();

        let vp_h = self.current_viewport_height.max(1.0) as f64;
        let scroll_y = self.current_scroll_y as f64;
        let content_h = self.current_content_height as f64;

        // Check if scroll buffer or text needs rebuilding
        let mut force_rebuild = false;
        if let Some(ref buf) = self.scroll_buffer {
            let relative_src_y = scroll_y - buf.buffer_scroll_y;
            if relative_src_y < -0.1 || relative_src_y + vp_h > buf.buffer_logical_h + 0.1 {
                force_rebuild = true;
            } else {
                let content_changed = (content_h - buf.buffer_content_h).abs() > 1.0;
                let dpr_changed = (dpr - buf.dpr).abs() > 0.01;
                if content_changed || dpr_changed || !buf.contains_viewport(scroll_y, vp_h) {
                    force_rebuild = true;
                }
            }
        } else {
            force_rebuild = true;
        }

        if force_rebuild {
            self.render_dirty = true;
        }

        let mut final_root = root_raw;

        if self.render_dirty {
            match self.render_to_image() {
                Some((image, buf_scroll_y, _buf_h)) => {
                    let (src_y, src_h) = if let Some(ref buf) = self.scroll_buffer {
                        buf.clamp_source_rect(scroll_y, vp_h)
                    } else {
                        (scroll_y - buf_scroll_y, vp_h)
                    };
                    let logical_img_w = image.size().width as f64 / dpr;
                    final_root = sujian_update_texture_node(
                        root_raw, item_ptr, &image, 0.0, src_y, logical_img_w, src_h, 0.0, vp_h, dpr,
                    );
                    self.render_dirty = false;
                    let total_elapsed = frame_start.elapsed();
                    if total_elapsed.as_millis() > 4 {
                        eprintln!(
                            "sujian_update_paint_node: total_ms={}, new_texture=true, buf={:.0}..{:.0}",
                            total_elapsed.as_millis(), buf_scroll_y, buf_scroll_y + _buf_h,
                        );
                    }
                }
                None => {
                    if !root_raw.is_null() {
                        if let Some(ref buf) = self.scroll_buffer {
                            let (src_y, src_h) = buf.clamp_source_rect(scroll_y, vp_h);
                            let logical_img_w = buf.image.size().width as f64 / dpr;
                            sujian_update_source_rect(root_raw, item_ptr, 0.0, src_y, logical_img_w, src_h, 0.0, vp_h, dpr);
                        }
                    }
                    final_root = root_raw;
                }
            }
        } else {
            if !root_raw.is_null() {
                if let Some(ref buf) = self.scroll_buffer {
                    let (src_y, src_h) = buf.clamp_source_rect(scroll_y, vp_h);
                    let logical_img_w = buf.image.size().width as f64 / dpr;
                    sujian_update_source_rect(root_raw, item_ptr, 0.0, src_y, logical_img_w, src_h, 0.0, vp_h, dpr);
                }
            }
            final_root = root_raw;
        }

        // 2. 更新光标动画位置
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

        // 3. 更新独立的 cursor node overlay
        let is_selecting = self.buffer.has_selection();
        let show_cursor = self.cursor_visible && !self.current_is_scrolling && !is_selecting;
        let cursor_color_rgba = color_hex_to_rgba(&self.current_cursor_color);

        if !final_root.is_null() && !item_ptr.is_null() {
            sujian_update_cursor_rect(
                final_root,
                item_ptr,
                self.cursor_visual_x,
                self.cursor_visual_y,
                2.0,
                self.cursor_visual_h,
                show_cursor,
                cursor_color_rgba,
            );
        }

        unsafe { SGNode::<qmetaobject::scenegraph::ContainerNode>::from_raw(final_root) }
    }
}

impl SujianEditorItem {
    fn update_cursor_visual_position(&mut self) {
        let scroll_y = self.current_scroll_y as f64;
        let layout_res = self.editor_layout_cursor_rect(self.buffer.cursor, self.current_cursor_affinity, scroll_y);

        let cursor_x = layout_res.x;
        let cursor_y = layout_res.y;
        let cursor_h = layout_res.h;
        let visual_line_id = layout_res.visual_line_id;

        // Store the visual line id for future lookups
        let old_visual_line_id = self.current_visual_line_id;
        self.current_visual_line_id = Some(visual_line_id);

        let scroll_changed = (self.last_cursor_scroll_y - scroll_y).abs() > 0.01;
        self.last_cursor_scroll_y = scroll_y;

        // Always save new target position and IME rect — regardless of visibility
        let old_x = self.target_cursor_x;
        let old_y = self.target_cursor_y;
        let old_visible = self.cursor_visible;

        self.target_cursor_x = cursor_x;
        self.target_cursor_y = cursor_y;
        self.ime_cursor_rect_h = cursor_h;
        self.cursor_visual_h = cursor_h;

        if (old_x - cursor_x).abs() > 0.01 || (old_y - cursor_y).abs() > 0.01 {
            self.cursor_rect_changed();
            let obj_ptr = self.get_cpp_object();
            cpp!(unsafe [obj_ptr as "QQuickItem*"] {
                if (obj_ptr) {
                    QGuiApplication::inputMethod()->update(Qt::ImQueryInput);
                }
            });
        }

        // Viewport check: cursor is in viewport coordinates, just check bounds
        let vp_h = self.current_viewport_height.max(1.0) as f64;
        let in_viewport = cursor_y + cursor_h > 0.0 && cursor_y < vp_h;
        let new_visible = self.current_editor_enabled
            && !self.buffer.has_selection()
            && in_viewport
            && !self.current_is_scrolling;
        self.cursor_visible = new_visible;

        // Debug assert: cursor target_y must be in viewport coordinates to be visible.
        if self.cursor_visible {
            debug_assert!(
                self.target_cursor_y + cursor_h > 0.0 && self.target_cursor_y < vp_h,
                "Debug assert failed: cursor is visible but target_cursor_y ({:.2}) is outside viewport [0, {:.2}]",
                self.target_cursor_y, vp_h
            );
        }

        if !new_visible {
            // Not in viewport: stop animation, snap visual to target, hide node
            self.cursor_animation = None;
            self.cursor_visual_x = cursor_x;
            self.cursor_visual_y = cursor_y;
            if old_visible {
                self.cursor_dirty = true;
                let item = self as &dyn QQuickItem;
                item.update();
            }
            return;
        }

        let now = Instant::now();
        let is_selecting = self.buffer.selection_anchor != self.buffer.cursor;
        let is_preediting = !self.preedit_text.is_empty();

        // Check if line changed, target_y shifted by more than half line, or x shifted by a large amount
        let line_changed = old_visual_line_id.is_none() || old_visual_line_id != Some(visual_line_id);
        let half_line = cursor_h * 0.5;
        let target_y_changed_more_than_half_line = (old_y - cursor_y).abs() > half_line;
        let x_diff = (old_x - cursor_x).abs();
        let is_small_x_change = x_diff <= 150.0;

        // Click/drag/scroll should snap immediately (no animation)
        let should_snap = self.current_is_scrolling 
            || is_selecting 
            || is_preediting
            || !old_visible  // snap when becoming visible again
            || self.force_snap_next_cursor
            || scroll_changed
            || line_changed
            || target_y_changed_more_than_half_line
            || !is_small_x_change;

        let (visual_x, visual_y) = if let Some(ref anim) = self.cursor_animation {
            if anim.is_finished(now) {
                (anim.target_x, anim.target_y)
            } else {
                anim.current_position(now)
            }
        } else {
            (old_x, old_y)
        };

        let (final_x, final_y, new_animation) = if should_snap || !self.current_smooth_cursor_enabled {
            (cursor_x, cursor_y, None)
        } else if let Some(ref anim) = self.cursor_animation {
            if (anim.target_x - cursor_x).abs() > 0.01 || (anim.target_y - cursor_y).abs() > 0.01 {
                let (cur_x, cur_y) = anim.current_position(now);
                let duration = self.current_cursor_animation_duration_ms.max(30) as u64;
                let new_anim = CursorAnimationState {
                    start_x: cur_x,
                    start_y: cur_y,
                    target_x: cursor_x,
                    target_y: cursor_y,
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
            if (visual_x - cursor_x).abs() > 0.01 || (visual_y - cursor_y).abs() > 0.01 {
                let duration = self.current_cursor_animation_duration_ms.max(30) as u64;
                let new_anim = CursorAnimationState {
                    start_x: visual_x,
                    start_y: visual_y,
                    target_x: cursor_x,
                    target_y: cursor_y,
                    start_time: now,
                    duration_ms: duration,
                };
                (visual_x, visual_y, Some(new_anim))
            } else {
                (cursor_x, cursor_y, None)
            }
        };

        self.cursor_animation = new_animation;
        self.cursor_visual_x = final_x;
        self.cursor_visual_y = final_y;
        self.force_snap_next_cursor = false;

        // Dirty when position changed or visibility transitioned
        let pos_changed = (final_x - old_x).abs() > 0.01
            || (final_y - old_y).abs() > 0.01
            || !old_visible;
        if pos_changed {
            self.cursor_dirty = true;
        }

        if sujian_editor_debug_enabled() {
            eprintln!(
                "update_cursor_visual_position: cursor={}, target_x={:.1}, target_y={:.1}, visual_x={:.1}, visual_y={:.1}, is_animating={}, scroll_y={:.1}",
                self.buffer.cursor, self.target_cursor_x, self.target_cursor_y, self.cursor_visual_x, self.cursor_visual_y, self.cursor_animation.is_some(), scroll_y
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
        let snapshot = self.layout_snapshot(width);
        let content_h = snapshot.content_height;
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

        let lines = &snapshot.lines;
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
                let x_start = calculate_cursor_x_for_line(line, sel_start, CaretAffinity::Downstream, &snapshot);
                let x_end = calculate_cursor_x_for_line(line, sel_end, CaretAffinity::Upstream, &snapshot);
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
                    let x = calculate_cursor_x_for_line(line, pc, self.current_cursor_affinity, &snapshot);
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

        // Overscan: Use 2.5 * viewport_height as padding (at least viewport*2 or viewport*3)
        let overscan = vp_h * 2.5;
        let min_y = (scroll_y - overscan).max(0.0);
        let max_y = (scroll_y + vp_h + overscan).min(content_h.max(vp_h));
        let buffer_h = max_y - min_y;

        // Evaluate the scroll buffer miss reason
        let mut miss_reason = "none";
        let mut needs_render = true;

        if self.scroll_buffer.is_none() {
            miss_reason = "no_buffer";
        } else if self.editor_layout.cache().is_none() {
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
                    let inside_buffer = buf.contains_viewport(scroll_y, vp_h);
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
            let vis_lines = self.editor_layout.cache().map(|c| {
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
        
        double final_src_x = src_x * dpr;
        double final_src_y = src_y * dpr;
        double final_src_w = src_w * dpr;
        double final_src_h = src_h * dpr;
        
        if (imgNode->texture()) {
            QSize texSize = imgNode->texture()->textureSize();
            double tex_w = texSize.width();
            double tex_h = texSize.height();
            
            if (final_src_y < 0.0) final_src_y = 0.0;
            if (final_src_y + final_src_h > tex_h) {
                if (final_src_h > tex_h) {
                    final_src_h = tex_h;
                }
                if (final_src_y + final_src_h > tex_h) {
                    final_src_y = tex_h - final_src_h;
                }
            }
            if (final_src_x < 0.0) final_src_x = 0.0;
            if (final_src_x + final_src_w > tex_w) {
                if (final_src_w > tex_w) {
                    final_src_w = tex_w;
                }
                if (final_src_x + final_src_w > tex_w) {
                    final_src_x = tex_w - final_src_w;
                }
            }
        }
        
        imgNode->setSourceRect(final_src_x, final_src_y, final_src_w, final_src_h);
        imgNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
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
        
        double tex_w = img_ptr->width();
        double tex_h = img_ptr->height();
        double final_src_x = src_x * dpr;
        double final_src_y = src_y * dpr;
        double final_src_w = src_w * dpr;
        double final_src_h = src_h * dpr;
        
        if (final_src_y < 0.0) final_src_y = 0.0;
        if (final_src_y + final_src_h > tex_h) {
            if (final_src_h > tex_h) {
                final_src_h = tex_h;
            }
            if (final_src_y + final_src_h > tex_h) {
                final_src_y = tex_h - final_src_h;
            }
        }
        if (final_src_x < 0.0) final_src_x = 0.0;
        if (final_src_x + final_src_w > tex_w) {
            if (final_src_w > tex_w) {
                final_src_w = tex_w;
            }
            if (final_src_x + final_src_w > tex_w) {
                final_src_x = tex_w - final_src_w;
            }
        }
        
        imgNode->setSourceRect(final_src_x, final_src_y, final_src_w, final_src_h);
        imgNode->setTexture(item_ptr->window()->createTextureFromImage(*img_ptr));
        imgNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
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
    fn line_char_offset_does_not_jump_to_document_end() {
        let text = "第一行\n第二行";
        let line_end = "第一行".len();

        assert_eq!(byte_index_at_char_offset_in_range(text, 0, line_end, 3), line_end);
    }

    #[test]
    fn test_scroll_y_cache_and_buffer_hit() {
        // 1. scroll_y change shouldn't invalidate editor layout cache.
        // The layout cache is only invalidated/refreshed when text, width, font, padding etc change.
        // Changing scroll_y does not touch ensure_layout_cached criteria.
        
        // 2. Validate ScrollBuffer helper functions: contains_viewport and clamp_source_rect.
        let vp_h: f64 = 1000.0;
        let content_h: f64 = 5000.0;
        
        let scroll_buffer = ScrollBuffer {
            image: qmetaobject::QImage::new(
                qmetaobject::QSize { width: 10, height: 3500 },
                qmetaobject::ImageFormat::ARGB32_Premultiplied,
            ),
            buffer_scroll_y: 0.0,
            buffer_content_h: content_h,
            buffer_logical_h: 3500.0,
            dpr: 1.0,
        };

        // Strict boundary check and watermark check:
        // Threshold is 0.5 * vp_h = 500.0.
        // At scroll_y = 100.0, it is near the top, but since buffer_scroll_y is 0.0 (top of document),
        // contains_viewport should return true because we can't scroll up anyway.
        assert!(scroll_buffer.contains_viewport(100.0, vp_h), "Should contain viewport at document top");

        // Scroll to 2000.0 (viewport is [2000, 3000]).
        // This is within the buffer [0, 3500].
        // Watermark check:
        // near_top = 2000 < 500 (false)
        // near_bottom = 3000 > 3500 - 500 (false, equal, but not strictly greater)
        // So contains_viewport should return true.
        assert!(scroll_buffer.contains_viewport(2000.0, vp_h), "Should contain viewport in the middle of buffer");

        // Scroll to 3100.0 (viewport is [3100, 4100]).
        // This is strictly outside the buffer [0, 3500], so contains_viewport must return false.
        assert!(!scroll_buffer.contains_viewport(3100.0, vp_h), "Should not contain viewport if outside buffer");

        // Test clamp_source_rect.
        // At scroll_y = 2000.0, src_y = 2000.0, src_h = 1000.0.
        let (src_y, src_h) = scroll_buffer.clamp_source_rect(2000.0, vp_h);
        assert_eq!(src_y, 2000.0);
        assert_eq!(src_h, 1000.0);

        // At scroll_y = -100.0, src_y should clamp to 0.0.
        let (src_y_neg, src_h_neg) = scroll_buffer.clamp_source_rect(-100.0, vp_h);
        assert_eq!(src_y_neg, 0.0);
        assert!(src_h_neg > 0.0);

        // At scroll_y = 3000.0 (viewport [3000, 4000]), src_y + src_h = 3000 + 1000 = 4000,
        // which exceeds buffer_logical_h (3500).
        // It should clamp so that src_y + src_h <= 3500.
        let (src_y_clamp, src_h_clamp) = scroll_buffer.clamp_source_rect(3000.0, vp_h);
        assert!(src_y_clamp + src_h_clamp <= 3500.0);
    }

}
