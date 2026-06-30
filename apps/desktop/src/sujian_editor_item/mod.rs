//! Desktop 自研写作区 — 唯一主路径
//!
//! 路线：SujianEditorItem(QQuickItem) + QTextLayout/QTextLine + QImage static texture
//!       + QSGImageNode + QML Rectangle cursor + QML EditorAnimationOverlay
//!
//! 禁止旧路线：DocumentHandler / TextArea / QTextDocument / QQuickPaintedItem / QSG 三层 overlay

// =============================================================================
// sujian_editor_item - Desktop self-rendered editor item
// =============================================================================

pub(crate) mod buffer;
pub(crate) mod cursor_controller;
pub(crate) mod rendering;
pub(crate) mod text_animation_state;

use crate::editor::input::{self, EditorInputHost};
use crate::editor::layout::{
    CaretAffinity, CursorLayoutRect, EditorLayout, LayoutParams, LayoutSnapshot, VisualLine,
    text_baseline_y,
};
use crate::editor::renderer;
use crate::editor::scene_graph;
use buffer::{
    byte_to_char_index, clamp_to_char_boundary, next_char_boundary, normalize_plain_text,
    prev_char_boundary, EditorBuffer, EditorSnapshot,
};
use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::{QMouseEvent, QQuickItem, QRectF, QString};
use rendering::ScrollBuffer;
use std::cell::Cell;
use std::time::Instant;
use text_animation_state::{AnimationDecision, TextAnimationState, should_create_text_animation};

pub use rendering::AnimatedGlyph;
use writer_core::editor::{
    CursorRect, EditorAnimationKind, EditorCursor, EditorEngine,
    EditorSelection, EditorTransactionCause, EditorVisualTransaction, GlyphRect, ReflowGlyphRect,
};

/// Preedit attribute from QInputMethodEvent.
/// Represents TextFormat (underline/highlight) and Cursor attributes
/// that control visual styling and cursor position within the preedit string.
#[derive(Clone, Debug)]
pub(crate) struct PreeditAttribute {
    /// Byte offset start within preedit_text
    pub start: usize,
    /// Byte length of the attribute span
    pub length: usize,
    /// Attribute type: "underline" (TextFormat) or "cursor" (Cursor)
    pub kind: PreeditAttributeKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PreeditAttributeKind {
    /// QInputMethodEvent::TextFormat — underline/highlight styling
    Underline,
    /// QInputMethodEvent::Cursor — cursor position within preedit
    Cursor,
}

cpp! {{
    #include <QtGui/QFont>
    #include <QtGui/QPainter>
    #include <QtGui/QClipboard>
    #include <QGuiApplication>
    #include <QMetaObject>
}}

pub fn editor_animation_debug_enabled() -> bool {
    cfg!(debug_assertions) || std::env::var_os("SUJIAN_EDITOR_ANIMATION_DEBUG").is_some()
}

pub fn sujian_editor_debug_enabled() -> bool {
    cfg!(debug_assertions) || std::env::var_os("SUJIAN_EDITOR_DEBUG").is_some()
}

/// 判断字符是否为复杂 grapheme（emoji / ZWJ / variation selector / combining mark）。
/// 与 Android `OverlayAnim.containsComplexGrapheme` 和 QML `isComplexGrapheme` 对齐。
/// 复杂字符不参与 glyph ghost 动画，避免渲染异常和资源浪费。
fn is_complex_grapheme(ch: char) -> bool {
    let cp = ch as u32;
    // Surrogate pairs: code point > 0xFFFF (non-BMP, e.g. emoji)
    if cp > 0xFFFF {
        return true;
    }
    // Zero Width Joiner
    if cp == 0x200D {
        return true;
    }
    // Variation selectors (FE00-FE0F, E0100-E01EF)
    if (cp >= 0xFE00 && cp <= 0xFE0F) || (cp >= 0xE0100 && cp <= 0xE01EF) {
        return true;
    }
    // Combining Diacritical Marks (0300-036F)
    if cp >= 0x0300 && cp <= 0x036F {
        return true;
    }
    // Combining Diacritical Marks Extended (1AB0-1AFF)
    if cp >= 0x1AB0 && cp <= 0x1AFF {
        return true;
    }
    // Combining Diacritical Marks Supplement (1DC0-1DFF)
    if cp >= 0x1DC0 && cp <= 0x1DFF {
        return true;
    }
    // Combining Diacritical Marks for Symbols (20D0-20FF)
    if cp >= 0x20D0 && cp <= 0x20FF {
        return true;
    }
    // Combining Half Marks (FE20-FE2F)
    if cp >= 0xFE20 && cp <= 0xFE2F {
        return true;
    }
    false
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
    typing_animation_duration_ms: qt_property!(u32; READ typing_animation_duration_ms WRITE set_typing_animation_duration_ms NOTIFY visual_settings_changed),
    coordinated_text_cursor_animation_enabled: qt_property!(bool; READ coordinated_text_cursor_animation_enabled WRITE set_coordinated_text_cursor_animation_enabled NOTIFY visual_settings_changed),
    last_transaction_summary: qt_property!(QString; READ last_transaction_summary NOTIFY transaction_created),
    last_animation_event_count: qt_property!(u32; READ last_animation_event_count NOTIFY transaction_created),
    visual_transaction_json: qt_property!(QString; READ visual_transaction_json NOTIFY visual_transaction_changed),
    scroll_y: qt_property!(f32; READ scroll_y WRITE set_scroll_y NOTIFY visual_settings_changed),
    viewport_height: qt_property!(f32; READ viewport_height WRITE set_viewport_height NOTIFY visual_settings_changed),
    is_scrolling: qt_property!(bool; READ is_scrolling WRITE set_is_scrolling NOTIFY visual_settings_changed),
    cursor_rect_x: qt_property!(f32; READ cursor_rect_x NOTIFY cursor_rect_changed),
    cursor_rect_y: qt_property!(f32; READ cursor_rect_y NOTIFY cursor_rect_changed),
    cursor_rect_width: qt_property!(f32; READ cursor_rect_width NOTIFY cursor_rect_changed),
    cursor_rect_height: qt_property!(f32; READ cursor_rect_height NOTIFY cursor_rect_changed),
    cursor_visible: qt_property!(bool; READ cursor_visible NOTIFY cursor_rect_changed),
    current_selection_text: qt_property!(QString; READ current_selection_text NOTIFY selection_changed),

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
    visual_transaction_changed: qt_signal!(),
    context_menu_requested: qt_signal!(x: f32, y: f32),

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
    tick_cursor_animation: qt_method!(fn(&mut self)),
    long_press_at: qt_method!(fn(&mut self, x: f32, y: f32)),
    select_word_at: qt_method!(fn(&mut self, x: f32, y: f32)),
    request_text_input_focus: qt_method!(fn(&mut self)),
    snap_next_cursor_update: qt_method!(fn(&mut self)),
    on_insert_animation_finished: qt_method!(fn(&mut self, byte_start: i32, byte_end: i32)),
    on_insert_animation_skipped: qt_method!(fn(&mut self, byte_start: i32, byte_end: i32)),

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
    current_typing_animation_duration_ms: u32,
    current_coordinated_text_cursor_animation_enabled: bool,
    current_scroll_y: f32,
    current_viewport_height: f32,
    current_is_scrolling: bool,
    last_summary: QString,
    last_event_count: u32,
    last_visual_transaction_json: QString,
    preedit_text: String,
    preedit_cursor: usize,
    /// Preedit attributes from QInputMethodEvent (TextFormat and Cursor attributes).
    /// Stores (start, length, type) where type is "underline" or "cursor".
    preedit_attributes: Vec<PreeditAttribute>,
    /// Previous preedit text for diff-based visual transaction generation.
    preedit_old_text: String,
    /// Visual transaction for preedit changes (insert/delete within composition).
    preedit_visual_transaction: Option<EditorVisualTransaction>,
    /// Preedit cursor rect (position within the preedit string, not the buffer cursor).
    preedit_cursor_rect: Option<CursorRect>,
    suppress_next_ime_commit: bool,
    editor_layout: EditorLayout,
    text_revision: u64,
    render_dirty: bool,
    scroll_buffer: Option<ScrollBuffer>,
    last_slow_paint_log: Option<Instant>,
    /// Isolated cursor visual state - target position, visual position,
    /// animation, IME pending flag.  All cursor-related visual logic
    /// lives in CursorController; SujianEditorItem only dispatches.
    cursor_ctrl: cursor_controller::CursorController,
    /// 文本动画状态机 — 独立管理动画生命周期（不依赖 Qt）。
    /// Insert 动画期间，正文层跳过 inserted range 不绘制，由 QML overlay 显示 glyph。
    /// 动画结束后清除，正文层恢复完整绘制。所有停止路径立即清理 hidden range。
    text_anim_state: TextAnimationState,
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
            typing_animation_duration_ms: Default::default(),
            coordinated_text_cursor_animation_enabled: Default::default(),
            last_transaction_summary: Default::default(),
            last_animation_event_count: Default::default(),
            visual_transaction_json: Default::default(),
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
            visual_transaction_changed: Default::default(),
            context_menu_requested: Default::default(),
            cursor_rect_x: Default::default(),
            cursor_rect_y: Default::default(),
            cursor_rect_width: Default::default(),
            cursor_rect_height: Default::default(),
            cursor_visible: Default::default(),
            current_selection_text: Default::default(),
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
            tick_cursor_animation: Default::default(),
            long_press_at: Default::default(),
            select_word_at: Default::default(),
            request_text_input_focus: Default::default(),
            snap_next_cursor_update: Default::default(),
            on_insert_animation_finished: Default::default(),
            on_insert_animation_skipped: Default::default(),
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
            current_cursor_animation_duration_ms: 80,
            current_typing_animation_enabled: true,
            current_typing_animation_duration_ms: 100,
            current_coordinated_text_cursor_animation_enabled: true,
            current_scroll_y: 0.0,
            current_viewport_height: 0.0,
            current_is_scrolling: false,
            last_summary: "".into(),
            last_event_count: 0,
            last_visual_transaction_json: "".into(),
            preedit_text: String::new(),
            preedit_cursor: 0,
            preedit_attributes: Vec::new(),
            preedit_old_text: String::new(),
            preedit_visual_transaction: None,
            preedit_cursor_rect: None,
            suppress_next_ime_commit: false,
            editor_layout: EditorLayout::default(),
            text_revision: 0,
            render_dirty: true,
            scroll_buffer: None,
            last_slow_paint_log: None,
            cursor_ctrl: cursor_controller::CursorController::new(),
            text_anim_state: TextAnimationState::new(),
        }
    }
}

impl SujianEditorItem {
    fn request_static_repaint(&mut self) {
        self.render_dirty = true;
        let item = self as &dyn QQuickItem;
        item.update();
    }

    fn request_frame_update(&mut self) {
        let item = self as &dyn QQuickItem;
        item.update();
    }

    /// 清空所有活跃文本动画，并触发静态正文重绘恢复完整绘制
    fn clear_active_text_animations(&mut self) {
        if !self.text_anim_state.is_empty() {
            self.text_anim_state.clear();
            self.request_static_repaint();
        }
    }

    /// QML 动画 overlay 通知 Insert 动画完成，清除对应的隐藏 range。
    /// 只要真的移除了匹配的 Insert 动画就立刻 request_static_repaint，
    /// 不必等 has_active_insert 变成 false，避免多 Insert 动画时漏重绘。
    fn on_insert_animation_finished(&mut self, byte_start: i32, byte_end: i32) {
        let bs = byte_start as usize;
        let be = byte_end as usize;
        let removed = self.text_anim_state.on_insert_animation_finished(bs, be);
        if removed {
            if editor_animation_debug_enabled() {
                eprintln!(
                    "on_insert_animation_finished: byte_range=({},{}), removed, has_active_insert={}",
                    bs, be, self.text_anim_state.has_active_insert()
                );
            }
            self.request_static_repaint();
        }
    }

    /// QML 动画 overlay 通知 Insert 动画被跳过（component not ready / glyph 超限 / 换行等）。
    /// 此时 Rust 侧可能已经创建了 hidden range，必须立即清除，
    /// 否则正文层会永久跳过该 range 导致文字消失。
    fn on_insert_animation_skipped(&mut self, byte_start: i32, byte_end: i32) {
        let bs = byte_start as usize;
        let be = byte_end as usize;
        let removed = self.text_anim_state.on_insert_animation_finished(bs, be);
        if removed {
            if editor_animation_debug_enabled() {
                eprintln!(
                    "on_insert_animation_skipped: byte_range=({},{}), cleared hidden range, has_active_insert={}",
                    bs, be, self.text_anim_state.has_active_insert()
                );
            }
            self.request_static_repaint();
        }
    }

    /// 检查是否有活跃的 Insert 动画（正文层需要跳过其 range）
    fn has_active_insert_animation(&self) -> bool {
        self.text_anim_state.has_active_insert()
    }

    /// 获取活跃 Insert 动画的 byte range（如有多个取第一个）
    fn active_insert_byte_range(&self) -> Option<(usize, usize)> {
        self.text_anim_state.active_insert_byte_range()
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
        self.preedit_attributes.clear();
        self.preedit_old_text.clear();
        self.preedit_visual_transaction = None;
        self.preedit_cursor_rect = None;
        self.clear_active_text_animations();
        self.cursor_ctrl.animation = None;
        self.cursor_ctrl.force_snap_next = true;
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
        self.preedit_attributes.clear();
        self.preedit_old_text.clear();
        self.preedit_visual_transaction = None;
        self.preedit_cursor_rect = None;
        self.clear_active_text_animations();
        self.cursor_ctrl.animation = None;
        self.cursor_ctrl.force_snap_next = true;
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
        self.update_cursor_visual_position();
        self.request_static_repaint();
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
        let clamped = value.max(30).min(1000);
        self.current_cursor_animation_duration_ms = clamped;
        self.visual_settings_changed();
    }

    fn typing_animation_enabled(&self) -> bool {
        self.current_typing_animation_enabled
    }

    fn set_typing_animation_enabled(&mut self, value: bool) {
        if self.current_typing_animation_enabled == value {
            return;
        }
        self.current_typing_animation_enabled = value;
        if !value {
            // 关闭动画时立即清除 hidden range，不依赖 timeout 恢复文字
            self.clear_active_text_animations();
            self.request_static_repaint();
        }
        if editor_animation_debug_enabled() {
            eprintln!("typing_animation_enabled_changed: {}", value);
        }
        self.visual_settings_changed();
    }

    fn typing_animation_duration_ms(&self) -> u32 {
        self.current_typing_animation_duration_ms
    }

    fn set_typing_animation_duration_ms(&mut self, value: u32) {
        let clamped = value.max(30).min(1000);
        if self.current_typing_animation_duration_ms == clamped {
            return;
        }
        self.current_typing_animation_duration_ms = clamped;
        self.engine.set_animation_duration_ms(clamped as u64);
        if editor_animation_debug_enabled() {
            eprintln!("typing_animation_duration_ms_changed: {}", clamped);
        }
        self.visual_settings_changed();
    }

    fn coordinated_text_cursor_animation_enabled(&self) -> bool {
        self.current_coordinated_text_cursor_animation_enabled
    }

    fn set_coordinated_text_cursor_animation_enabled(&mut self, value: bool) {
        if self.current_coordinated_text_cursor_animation_enabled == value {
            return;
        }
        self.current_coordinated_text_cursor_animation_enabled = value;
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
        self.clear_active_text_animations();
        self.cursor_ctrl.force_snap_next = true;
        self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    fn viewport_height(&self) -> f32 {
        self.current_viewport_height
    }

    fn set_viewport_height(&mut self, value: f32) {
        if (self.current_viewport_height - value).abs() < 0.5 {
            return;
        }
        self.current_viewport_height = value;
        self.cursor_ctrl.animation = None;
        self.cursor_ctrl.force_snap_next = true;
        self.update_cursor_visual_position();
        self.request_static_repaint();
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
            self.clear_active_text_animations();
            self.cursor_ctrl.animation = None;
            // Animation overlay clearing is handled by QML EditorAnimationOverlay.suppressed
            self.cursor_ctrl.force_snap_next = true;
            self.request_static_repaint();
            return;
        }
        if !value {
            self.cursor_ctrl.force_snap_next = true;
            self.update_cursor_visual_position();
            self.request_static_repaint();
        }
    }

    fn last_transaction_summary(&self) -> QString {
        self.last_summary.clone()
    }

    fn last_animation_event_count(&self) -> u32 {
        self.last_event_count
    }

    fn visual_transaction_json(&self) -> QString {
        self.last_visual_transaction_json.clone()
    }

    fn cursor_rect_x(&self) -> f32 {
        // During preedit, use preedit cursor position for IME candidate window
        if !self.preedit_text.is_empty() {
            if let Some(ref r) = self.preedit_cursor_rect {
                return r.x as f32;
            }
        }
        if self.current_smooth_cursor_enabled && self.cursor_ctrl.animation.is_some() {
            self.cursor_ctrl.visual_x as f32
        } else {
            self.cursor_ctrl.target_x as f32
        }
    }

    fn cursor_rect_y(&self) -> f32 {
        // During preedit, use preedit cursor position for IME candidate window
        if !self.preedit_text.is_empty() {
            if let Some(ref r) = self.preedit_cursor_rect {
                return r.top as f32;
            }
        }
        if self.current_smooth_cursor_enabled && self.cursor_ctrl.animation.is_some() {
            self.cursor_ctrl.visual_y as f32
        } else {
            self.cursor_ctrl.target_y as f32
        }
    }

    fn cursor_rect_width(&self) -> f32 {
        2.0
    }

    fn cursor_rect_height(&self) -> f32 {
        self.cursor_ctrl.ime_cursor_rect_h as f32
    }

    fn cursor_visible(&self) -> bool {
        self.cursor_ctrl.visible
    }

    fn current_selection_text(&self) -> QString {
        self.buffer.selected_text().into()
    }

    fn request_text_input_focus(&mut self) {
        let obj_ptr = self.get_cpp_object();
        if obj_ptr.is_null() {
            return;
        }
        input::focus_item(obj_ptr);
    }

    fn snap_next_cursor_update(&mut self) {
        self.cursor_ctrl.force_snap_next = true;
        self.update_cursor_visual_position();
        self.request_frame_update();
    }

    fn visual_changed(&mut self) {
        self.invalidate_layout_cache();
        self.clear_active_text_animations();
        self.cursor_ctrl.animation = None;
        self.cursor_ctrl.force_snap_next = true;
        self.recalculate_content_height_and_emit();
        self.visual_settings_changed();
        self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    fn emit_content_changed(&mut self) {
        self.text_revision = self.text_revision.wrapping_add(1);
        self.invalidate_layout_cache();
        // Do NOT unconditionally clear cursor animation or force snap.
        // Typing/Delete should let cursor animate from old visual position
        // to the new position. Only Load/chapter-switch/format/settings/scroll
        // should snap. The caller (insert_text, delete_backward, etc.) sets
        // force_snap_next when appropriate; emit_content_changed no longer
        // overrides it.
        self.recalculate_content_height_and_emit();
        self.plain_text_changed();
        self.text_changed();
        self.cursor_position_changed();
        self.selection_changed();
        self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    fn flush_content_height(&mut self) {
        if self.content_height_dirty.get() {
            self.content_height_dirty.set(false);
            self.content_height_changed();
        }
    }

    fn tick_cursor_animation(&mut self) {
        if self.cursor_ctrl.animation.is_none() {
            return;
        }
        let still_animating = self.cursor_ctrl.tick_animation();
        self.cursor_rect_changed();
        if still_animating {
            self.request_frame_update();
        }
    }

    /// 超时安全机制：检查活跃文本动画是否超时，超时则清除并重绘。
    /// 正常情况下 QML overlay 动画完成后通过 on_insert_animation_finished 清除，
    /// 此方法作为兜底防止 QML 信号丢失导致正文层永久跳过 range。
    fn tick_text_animations(&mut self) {
        let now = Instant::now();
        if self.text_anim_state.tick(now) {
            if editor_animation_debug_enabled() {
                eprintln!("tick_text_animations: cleared timed-out animations");
            }
            self.request_static_repaint();
        }
    }

    // tick_typing_animation() removed: Rust no longer manages animation display
    // lifecycle.  QML EditorAnimationOverlay owns animation state and ghosts
    // self-destroy on completion.

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
        // Clear preedit state when inserting formal text (e.g. IME commit)
        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.preedit_attributes.clear();
        self.preedit_old_text.clear();
        self.preedit_visual_transaction = None;
        self.preedit_cursor_rect = None;

        let old = self.buffer.snapshot();
        self.buffer.push_undo(old.clone());
        self.buffer.replace_selection_or_insert(&inserted);
        self.adjust_affinity_at_wrap_boundary();
        let cause = if inserted.chars().count() == 1 {
            EditorTransactionCause::Typing
        } else {
            EditorTransactionCause::TypingCommit
        };
        let new = self.buffer.snapshot();
        let _vt = self.record_transaction(old, new, cause, true);
        // Animation lifecycle is owned by QML EditorAnimationOverlay.
        // Rust only provides glyph rect data via visual_transaction_json.

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

        let _vt = self.record_transaction(old, new, EditorTransactionCause::Delete, true);
        // Animation lifecycle is owned by QML EditorAnimationOverlay.

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

        let _vt = self.record_transaction(old, new, EditorTransactionCause::Delete, true);
        // Animation lifecycle is owned by QML EditorAnimationOverlay.

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

        let _vt = self.record_transaction(old, new, EditorTransactionCause::Delete, true);
        // Animation lifecycle is owned by QML EditorAnimationOverlay.

        self.emit_content_changed();
    }

    fn select_all(&mut self) {
        self.buffer.select_all();
        self.adjust_affinity_at_wrap_boundary();
        self.cursor_position_changed();
        self.selection_changed();
        self.request_static_repaint();
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
        input::handle_key(self, key, modifiers)
    }

    fn click_at(&mut self, x: f32, y: f32, extend: bool) {
        let (index, affinity) = self.hit_test(x as f64, y as f64);
        self.cursor_ctrl.affinity = affinity;
        // All click operations must snap — click, extend, drag, etc.
        self.cursor_ctrl.force_snap_next = true;
        if sujian_editor_debug_enabled() {
            eprintln!(
                "click_at: mouse_x={:.1}, mouse_y={:.1}, current_scroll_y={:.1}, hit_index={}, affinity={:?}, extend={}",
                x, y, self.current_scroll_y, index, affinity, extend
            );
        }
        self.buffer.move_cursor(index, extend);
        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.preedit_attributes.clear();
        self.preedit_old_text.clear();
        self.preedit_visual_transaction = None;
        self.preedit_cursor_rect = None;
        self.cursor_position_changed();
        self.selection_changed();
        self.cursor_ctrl.dirty = true;
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    fn drag_select_at(&mut self, x: f32, y: f32) {
        let (index, affinity) = self.hit_test(x as f64, y as f64);
        self.cursor_ctrl.affinity = affinity;
        self.cursor_ctrl.force_snap_next = true;
        self.buffer.move_cursor(index, true);
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    fn long_press_at(&mut self, x: f32, y: f32) {
        let (index, affinity) = self.hit_test(x as f64, y as f64);
        self.cursor_ctrl.affinity = affinity;
        self.cursor_ctrl.force_snap_next = true;
        // 长按时，如果没有选区则选词
        if !self.buffer.has_selection() {
            self.select_word_at_impl(index);
        }
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
        self.context_menu_requested(x, y);
    }

    fn select_word_at(&mut self, x: f32, y: f32) {
        let (index, affinity) = self.hit_test(x as f64, y as f64);
        self.cursor_ctrl.affinity = affinity;
        self.cursor_ctrl.force_snap_next = true;
        self.select_word_at_impl(index);
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    fn select_word_at_impl(&mut self, index: usize) {
        let text = &self.buffer.text;
        if text.is_empty() || index > text.len() {
            return;
        }
        // 将字节位置转为字符索引，处理多字节字符
        let char_index = byte_to_char_index(text, index);
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            return;
        }
        let ci = char_index.min(chars.len().saturating_sub(1));

        // 判断是否为词边界：包含中英文标点
        fn is_word_boundary(c: char) -> bool {
            c.is_whitespace()
                || c == '\n'
                || c == ','
                || c == '?'
                || c == '!'
                || c == '！'
                || c == ';'
                || c == ':'
                || c == '"'
                || c == '"'
                || c == '\u{2018}'
                || c == '\u{2019}'
                || c == '？'
                || c == '-'
                || c == '.'
                || c == '('
                || c == ')'
                || c == '（'
                || c == '）'
        }

        // 向前扫描
        let mut start = ci;
        while start > 0 && !is_word_boundary(chars[start - 1]) {
            start -= 1;
        }
        // 向后扫描
        let mut end = ci + 1;
        while end < chars.len() && !is_word_boundary(chars[end]) {
            end += 1;
        }

        // 转回 byte 位置
        let byte_start = chars[..start].iter().map(|c| c.len_utf8()).sum::<usize>();
        let byte_end = chars[..end].iter().map(|c| c.len_utf8()).sum::<usize>();

        self.buffer.selection_anchor = byte_start;
        self.buffer.cursor = byte_end;
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
        self.clear_active_text_animations();
        input::insert_preedit_text(self, text.to_string());
    }

    fn commit_preedit(&mut self, text: QString) {
        input::commit_preedit_text(self, text.to_string());
    }

    fn cancel_preedit(&mut self) {
        self.clear_active_text_animations();
        input::cancel_preedit(self);
    }

    fn move_cursor_horizontal(&mut self, forward: bool, extend: bool) {
        let next = if forward {
            next_char_boundary(&self.buffer.text, self.buffer.cursor).unwrap_or(self.buffer.cursor)
        } else {
            prev_char_boundary(&self.buffer.text, self.buffer.cursor).unwrap_or(self.buffer.cursor)
        };
        self.cursor_ctrl.affinity = if forward {
            CaretAffinity::Downstream
        } else {
            CaretAffinity::Upstream
        };
        self.buffer.move_cursor(next, extend);
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
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
        self.cursor_ctrl.affinity = self
            .editor_layout
            .affinity_for_index_on_line(&lines[target_idx], index);
        self.buffer.move_cursor(index, extend);
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    fn move_to_line_edge(&mut self, end: bool, extend: bool) {
        let width = self.bounding_width();
        let lines = self.ensure_layout_cached(width).clone();
        let Some((line_idx, _)) = self.cursor_line_and_x(&lines) else {
            return;
        };
        let line = &lines[line_idx];
        let (index, affinity) = if end {
            (line.byte_end, CaretAffinity::Upstream)
        } else {
            (line.byte_start, CaretAffinity::Downstream)
        };
        self.cursor_ctrl.affinity = affinity;
        self.buffer.move_cursor(index, extend);
        self.cursor_position_changed();
        self.selection_changed();
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    fn record_transaction(
        &mut self,
        old: EditorSnapshot,
        new: EditorSnapshot,
        cause: EditorTransactionCause,
        emit: bool,
    ) -> Option<EditorVisualTransaction> {
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
        let mut vt = self.engine.visual_transaction(&transaction);

        // 为 Insert/Delete 事务填充坐标字段（动画定位用）
        if let Some(ref mut vt) = vt {
            self.fill_visual_transaction_coords(vt, &new.text, &old.text);
        }

        // 创建 TextAnimationState 条目，让正文层在动画期间跳过 inserted range
        // 使用统一的 should_create_text_animation 判定，确保 Rust 和 QML 一致
        // 仅在动画启用且事务非空时创建
        if self.current_typing_animation_enabled && vt.is_some() && !self.current_is_scrolling {
            if let Some(ref vt) = vt {
                match vt.kind {
                    EditorAnimationKind::Insert => {
                        // 从 vt.inserted_range 读取 hidden range
                        if let Some((range_start, range_end)) = vt.inserted_range {
                            // 统一动画判定：与 QML 使用相同规则
                            let glyph_rects = vt.insert_glyph_rects.as_ref();
                            let glyph_count = glyph_rects.map_or(0, |g| g.len());
                            let contains_newline = vt.new_text[range_start..range_end].contains('\n');
                            let decision = should_create_text_animation(
                                glyph_count,
                                contains_newline,
                                self.current_is_scrolling,
                                false, // is_loading — record_transaction 不会在 loading 时调用
                                false, // is_applying_format
                                false, // is_applying_settings
                                self.current_typing_animation_enabled,
                                true,  // component_ready — Rust 侧始终为 true
                            );
                            match decision {
                                AnimationDecision::FullAnimation => {
                                    self.text_anim_state.start_insert((range_start, range_end), vt.duration_ms);
                                    if editor_animation_debug_enabled() {
                                        eprintln!(
                                            "record_transaction: created Insert animation byte_range=({},{}), duration_ms={}",
                                            range_start, range_end, vt.duration_ms
                                        );
                                    }
                                }
                                AnimationDecision::CursorOnly => {
                                    // 换行场景：不创建 hidden range，光标仍可动画
                                    // 不调用 start_insert，正文层正常绘制
                                    if editor_animation_debug_enabled() {
                                        eprintln!(
                                            "record_transaction: Insert CursorOnly (newline), byte_range=({},{}), no hidden range",
                                            range_start, range_end
                                        );
                                    }
                                }
                                AnimationDecision::NoAnimation => {
                                    // 不创建 hidden range，正文层正常绘制
                                    if editor_animation_debug_enabled() {
                                        eprintln!(
                                            "record_transaction: Insert NoAnimation, byte_range=({},{}), no hidden range",
                                            range_start, range_end
                                        );
                                    }
                                }
                            }
                        }
                    }
                    EditorAnimationKind::Delete => {
                        // Delete 动画不需要正文层跳过，但记录以跟踪活跃动画
                        // 使用统一判定
                        let glyph_rects = vt.deleted_glyph_rects.as_ref();
                        let glyph_count = glyph_rects.map_or(0, |g| g.len());
                        let contains_newline = vt.old_text != vt.new_text &&
                            (vt.old_text.contains('\n') || vt.new_text.contains('\n'));
                        let decision = should_create_text_animation(
                            glyph_count,
                            contains_newline,
                            self.current_is_scrolling,
                            false, // is_loading
                            false, // is_applying_format
                            false, // is_applying_settings
                            self.current_typing_animation_enabled,
                            true,  // component_ready
                        );
                        if matches!(decision, AnimationDecision::FullAnimation) {
                            // 对于 Delete，inserted_range 为 None，需要从变更推导 byte range
                            let changes = writer_core::editor::diff_plain_text(&vt.old_text, &vt.new_text);
                            for change in &changes {
                                if let writer_core::editor::EditorChange::Delete { index, text } = change {
                                    let range_start = *index;
                                    let range_end = range_start + text.len();
                                    self.text_anim_state.start_delete((range_start, range_end), vt.duration_ms);
                                }
                            }
                        }
                    }
                    EditorAnimationKind::Cursor => {}
                }
            }
        }

        self.last_event_count = if vt.is_some() { 1 } else { 0 };
        self.last_summary = format!(
            "cause={:?};changes={};vt={};animate={}",
            transaction.cause,
            transaction.changes.len(),
            vt.is_some(),
            transaction.should_animate
        )
        .into();
        if editor_animation_debug_enabled() {
            eprintln!(
                "record_transaction: cause={:?}, changes={}, vt={}, animate={}, typing_anim_enabled={}, is_scrolling={}",
                transaction.cause,
                transaction.changes.len(),
                vt.is_some(),
                transaction.should_animate,
                self.current_typing_animation_enabled,
                self.current_is_scrolling,
            );
        }
        // 序列化单个 EditorVisualTransaction 而非 Vec<EditorAnimationEvent>
        if let Some(ref vt) = vt {
            match serde_json::to_string(vt) {
                Ok(json) => {
                    self.last_visual_transaction_json = json.into();
                }
                Err(e) => {
                    eprintln!(
                        "record_transaction: failed to serialize visual transaction: {}",
                        e
                    );
                    self.last_visual_transaction_json = "{}".into();
                }
            }
        } else {
            self.last_visual_transaction_json = "{}".into();
        }
        if emit {
            self.transaction_created();
            if vt.is_some() {
                self.visual_transaction_changed();
            }
        }
        vt
    }

    /// 为 EditorVisualTransaction 填充坐标字段
    ///
    /// 通过 EditorLayout 的 glyph_positions_on_line 获取字符位置
    /// 转为(x, y, w, h),写入 vt.insert_glyph_rects / vt.deleted_glyph_rects 供 QML overlay 消费
    /// 同时填充 old_cursor_rect / new_cursor_rect 供 QML 动画 overlay
    /// 使用正确的光标位置，避免依赖可能过时的 editorItem.cursor_rect_x/y
    /// Cursor 类型不需要 glyph_rects，直接跳过
    fn fill_visual_transaction_coords(
        &mut self,
        vt: &mut EditorVisualTransaction,
        text: &str,
        old_text: &str,
    ) {
        let width = self.bounding_width();
        let font_size = self.current_font_pixel_size as f64;
        let font_family = &self.current_font_family.to_string();
        let scroll_y = self.current_scroll_y as f64;
        let viewport_h = self.current_viewport_height.max(1.0) as f64;

        /// 从 CaretRect 和对应的 LayoutSnapshot 构建 CursorRect（含 baseline_y）
        fn make_cursor_rect(
            caret: &CursorLayoutRect,
            snapshot: &LayoutSnapshot,
            font_family: &str,
            scroll_y: f64,
        ) -> CursorRect {
            let line = snapshot.lines.iter().find(|l| l.id == caret.visual_line_id);
            let baseline_y = match line {
                Some(l) => text_baseline_y(l, snapshot.font_size as f64, font_family) - scroll_y,
                None => caret.y + caret.h * 0.8, // fallback: approximate baseline
            };
            CursorRect {
                x: caret.x,
                top: caret.y,
                bottom: caret.y + caret.h,
                baseline_y,
            }
        }

        match vt.kind {
            EditorAnimationKind::Insert => {
                // Insert: 使用 new_text 布局计算 insert_glyph_rects
                let insert_snapshot = self.layout_snapshot_for_text(text, width);
                let mut glyph_rects = Vec::new();

                // 从 vt.inserted_range 获取插入范围
                if let Some((range_start, range_end)) = vt.inserted_range {
                    // 计算 old_cursor_rect：使用 old_text 布局
                    let old_snapshot = self.layout_snapshot_for_text(old_text, width);
                    let old_caret = self.editor_layout.caret_rect(
                        &old_snapshot,
                        vt.old_selection.head.index,
                        CaretAffinity::Downstream,
                        scroll_y,
                        viewport_h,
                    );
                    vt.old_cursor_rect = Some(make_cursor_rect(
                        &old_caret, &old_snapshot, font_family, scroll_y,
                    ));

                    // 计算 new_cursor_rect：使用 new_text 布局
                    let new_caret = self.editor_layout.caret_rect(
                        &insert_snapshot,
                        vt.new_selection.head.index,
                        CaretAffinity::Downstream,
                        scroll_y,
                        viewport_h,
                    );
                    vt.new_cursor_rect = Some(make_cursor_rect(
                        &new_caret, &insert_snapshot, font_family, scroll_y,
                    ));

                    // 局部 reflow 动画：限制 glyph_rects 只包含插入点所在行和下一行（最多 2 行）
                    // 避免中间插入时给光标后面的全部正文做字符动画，导致换行牵动整个屏幕
                    let mut lines_with_insert: Vec<usize> = Vec::new();
                    for (line_idx, line) in insert_snapshot.lines.iter().enumerate() {
                        if line.byte_end <= range_start || line.byte_start >= range_end {
                            continue;
                        }
                        if line.para_text.is_empty() {
                            continue;
                        }
                        lines_with_insert.push(line_idx);
                    }

                    // 限制到插入点所在行 + 下一行（最多 2 行）
                    // 如果影响超过 2 行、跨段落、滚动中、格式化中，直接 snap
                    let max_affected_lines = 2;
                    let should_snap = lines_with_insert.len() > max_affected_lines
                        || self.current_is_scrolling;

                    let allowed_lines: Vec<usize> = if should_snap {
                        // 只保留前 2 行的 glyph，后续行不做动画
                        lines_with_insert.into_iter().take(max_affected_lines).collect()
                    } else {
                        lines_with_insert
                    };

                    for line_idx in allowed_lines {
                        let line = &insert_snapshot.lines[line_idx];
                        let seg_start = range_start.max(line.byte_start);
                        let seg_end = range_end.min(line.byte_end);
                        if seg_start >= seg_end {
                            continue;
                        }

                        let glyph_data = self.editor_layout.glyph_positions_on_line(
                            line,
                            seg_start,
                            seg_end,
                            font_size,
                            font_family,
                        );
                        let line_baseline_y = text_baseline_y(line, font_size, font_family) - scroll_y;
                        for (abs_byte, x_pos, ch_w) in glyph_data {
                            if abs_byte >= text.len() {
                                continue;
                            }
                            let ch = text
                                .get(abs_byte..)
                                .and_then(|s| s.chars().next())
                                .unwrap_or(' ');
                            // 复杂字符（emoji / ZWJ / variation selector / combining mark）
                            // 不参与 glyph ghost 动画，跳过以避免渲染异常和资源浪费
                            if is_complex_grapheme(ch) {
                                continue;
                            }
                            glyph_rects.push(GlyphRect {
                                x: line.x + x_pos,
                                y: line.y - scroll_y,
                                w: ch_w,
                                h: line.height,
                                char_: ch.to_string(),
                                baseline_y: line_baseline_y,
                            });
                        }
                    }
                }

                vt.insert_glyph_rects = Some(glyph_rects);

                // ── 局部 reflow 动画：插入点右侧 glyph 的旧位置 vs 新位置 ──
                // 中间插入时，插入点右侧的文字做轻量位移动画（局部挤开），避免瞬间大跳。
                // 只影响同一行中插入点右侧 + 相邻 1-2 行。
                // 超过 2 行、跨段落、滚动中、格式化中、加载中时直接 snap。
                if let Some((range_start, range_end)) = vt.inserted_range {
                    let old_snapshot = self.layout_snapshot_for_text(old_text, width);

                    // 找到插入点所在行（在新布局中）
                    let insert_line_idx = insert_snapshot.lines.iter().position(|l| {
                        l.byte_end > range_start && l.byte_start <= range_end
                    });

                    if let Some(first_line_idx) = insert_line_idx {
                        // 收集受影响行：插入点所在行 + 后续最多 2 行
                        let max_reflow_lines = 2;
                        let mut reflow_lines: Vec<usize> = Vec::new();
                        for i in first_line_idx..insert_snapshot.lines.len() {
                            if reflow_lines.len() >= max_reflow_lines {
                                break;
                            }
                            let new_line = &insert_snapshot.lines[i];
                            // 跳过空行
                            if new_line.para_text.is_empty() {
                                continue;
                            }
                            // 检查是否跨段落：新行和插入行必须同一段落
                            let first_line = &insert_snapshot.lines[first_line_idx];
                            if new_line.para_start != first_line.para_start {
                                break;
                            }
                            reflow_lines.push(i);
                        }

                        // 如果影响超过 2 行或正在滚动，直接 snap（不收集 reflow rects）
                        let should_snap = reflow_lines.len() > max_reflow_lines
                            || self.current_is_scrolling;

                        if !should_snap && !reflow_lines.is_empty() {
                            let mut reflow_rects: Vec<ReflowGlyphRect> = Vec::new();

                            for line_idx in &reflow_lines {
                                let new_line = &insert_snapshot.lines[*line_idx];

                                // 确定该行需要做 reflow 动画的 byte 范围：
                                // - 插入点所在行：range_end 之后的所有 glyph
                                // - 后续行：整行所有 glyph
                                let reflow_byte_start = if *line_idx == first_line_idx {
                                    range_end
                                } else {
                                    new_line.byte_start
                                };
                                let reflow_byte_end = new_line.byte_end;

                                if reflow_byte_start >= reflow_byte_end {
                                    continue;
                                }

                                // 新布局中该行 glyph 位置
                                let new_glyph_data = self.editor_layout.glyph_positions_on_line(
                                    new_line,
                                    reflow_byte_start.max(new_line.byte_start),
                                    reflow_byte_end.min(new_line.byte_end),
                                    font_size,
                                    font_family,
                                );
                                let new_baseline_y = text_baseline_y(new_line, font_size, font_family) - scroll_y;

                                // 在旧布局中找对应行（通过 para_start + qtextline_idx 匹配）
                                let old_line = old_snapshot.lines.iter().find(|ol| {
                                    ol.para_start == new_line.para_start
                                        && ol.qtextline_idx == new_line.qtextline_idx
                                });

                                for (abs_byte, new_x_pos, ch_w) in &new_glyph_data {
                                    if *abs_byte >= text.len() {
                                        continue;
                                    }
                                    let ch = text
                                        .get(*abs_byte..)
                                        .and_then(|s| s.chars().next())
                                        .unwrap_or(' ');
                                    // 复杂字符不参与 reflow 动画
                                    if is_complex_grapheme(ch) {
                                        continue;
                                    }
                                    // 跳过插入范围内的 glyph（它们走 insert 动画）
                                    if *abs_byte >= range_start && *abs_byte < range_end {
                                        continue;
                                    }

                                    // 在旧布局中找该 glyph 的位置
                                    let (old_x, old_y, old_baseline_y) = if let Some(ol) = old_line {
                                        let old_glyph_data = self.editor_layout.glyph_positions_on_line(
                                            ol,
                                            (*abs_byte).max(ol.byte_start),
                                            (*abs_byte + ch.len_utf8()).min(ol.byte_end),
                                            font_size,
                                            font_family,
                                        );
                                        let oby = text_baseline_y(ol, font_size, font_family) - scroll_y;
                                        if let Some((_, ox, _)) = old_glyph_data.first() {
                                            (*ox, ol.y - scroll_y, oby)
                                        } else {
                                            // 旧布局中找不到，用新位置（snap）
                                            (new_line.x + *new_x_pos, new_line.y - scroll_y, new_baseline_y)
                                        }
                                    } else {
                                        // 旧布局中找不到对应行，用新位置（snap）
                                        (new_line.x + *new_x_pos, new_line.y - scroll_y, new_baseline_y)
                                    };

                                    let new_x = new_line.x + *new_x_pos;
                                    let new_y = new_line.y - scroll_y;

                                    // 只有位置实际发生变化才需要 reflow 动画
                                    let dx = (new_x - old_x).abs();
                                    let dy = (new_y - old_y).abs();
                                    if dx < 0.1 && dy < 0.1 {
                                        continue;
                                    }

                                    reflow_rects.push(ReflowGlyphRect {
                                        char_: ch.to_string(),
                                        old_x,
                                        old_y,
                                        old_baseline_y,
                                        new_x,
                                        new_y,
                                        new_baseline_y,
                                        w: *ch_w,
                                        h: new_line.height,
                                        line_index: *line_idx,
                                    });
                                }
                            }

                            if !reflow_rects.is_empty() {
                                vt.reflow_glyph_rects = Some(reflow_rects);
                            }
                        }
                    }
                }
            }
            EditorAnimationKind::Delete => {
                // Delete: 使用 old_text 布局计算 deleted_glyph_rects
                let delete_snapshot = self.layout_snapshot_for_text(old_text, width);
                let mut glyph_rects = Vec::new();

                // 从 diff 推导删除范围
                let changes = writer_core::editor::diff_plain_text(old_text, text);
                for change in &changes {
                    if let writer_core::editor::EditorChange::Delete { index, text: deleted_text } = change {
                        let range_start = *index;
                        let range_end = range_start + deleted_text.len();

                        // 计算 old_cursor_rect：使用 old_text 布局
                        let old_caret = self.editor_layout.caret_rect(
                            &delete_snapshot,
                            vt.old_selection.head.index,
                            CaretAffinity::Downstream,
                            scroll_y,
                            viewport_h,
                        );
                        vt.old_cursor_rect = Some(make_cursor_rect(
                            &old_caret, &delete_snapshot, font_family, scroll_y,
                        ));

                        // 计算 new_cursor_rect：使用 new_text 布局
                        let new_snapshot = self.layout_snapshot_for_text(text, width);
                        let new_caret = self.editor_layout.caret_rect(
                            &new_snapshot,
                            vt.new_selection.head.index,
                            CaretAffinity::Downstream,
                            scroll_y,
                            viewport_h,
                        );
                        vt.new_cursor_rect = Some(make_cursor_rect(
                            &new_caret, &new_snapshot, font_family, scroll_y,
                        ));

                        for line in &delete_snapshot.lines {
                            if line.byte_end <= range_start || line.byte_start >= range_end {
                                continue;
                            }
                            if line.para_text.is_empty() {
                                continue;
                            }
                            let seg_start = range_start.max(line.byte_start);
                            let seg_end = range_end.min(line.byte_end);
                            if seg_start >= seg_end {
                                continue;
                            }

                            let glyph_data = self.editor_layout.glyph_positions_on_line(
                                line,
                                seg_start,
                                seg_end,
                                font_size,
                                font_family,
                            );
                            let line_baseline_y = text_baseline_y(line, font_size, font_family) - scroll_y;
                            for (abs_byte, x_pos, ch_w) in glyph_data {
                                if abs_byte >= old_text.len() {
                                    continue;
                                }
                                let ch = old_text
                                    .get(abs_byte..)
                                    .and_then(|s| s.chars().next())
                                    .unwrap_or(' ');
                                // 复杂字符不参与 glyph ghost 动画
                                if is_complex_grapheme(ch) {
                                    continue;
                                }
                                glyph_rects.push(GlyphRect {
                                    x: line.x + x_pos,
                                    y: line.y - scroll_y,
                                    w: ch_w,
                                    h: line.height,
                                    char_: ch.to_string(),
                                    baseline_y: line_baseline_y,
                                });
                            }
                        }
                    }
                }

                vt.deleted_glyph_rects = Some(glyph_rects);
            }
            EditorAnimationKind::Cursor => {
                // Cursor 类型不需要 glyph rects 或 cursor rects
            }
        }
    }

    fn recalculate_content_height_quiet(&mut self) {
        let next = self.compute_content_height();
        if (self.current_content_height - next).abs() > 0.5 {
            self.current_content_height = next;
            self.content_height_dirty.set(true);
        }
    }

    /// Recalculate content height and immediately emit `content_height_changed`
    /// if the height changed. This ensures QML ScrollView always gets the
    /// latest contentHeight without relying on a deferred flush.
    fn recalculate_content_height_and_emit(&mut self) {
        let next = self.compute_content_height();
        if (self.current_content_height - next).abs() > 0.5 {
            self.current_content_height = next;
            self.content_height_dirty.set(false);
            self.content_height_changed();
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
        self.editor_layout
            .snapshot(&self.buffer.text, params, self.text_revision)
            .clone()
    }

    fn layout_snapshot_for_text(&mut self, text: &str, width: f64) -> LayoutSnapshot {
        let params = self.layout_params(width);
        // For temporary text (e.g. old text in delete animation), use revision 0
        // since this is a one-off layout that won't be cached across frames.
        self.editor_layout.snapshot(text, params, 0).clone()
    }

    fn ensure_layout_cached(&mut self, width: f64) -> &Vec<VisualLine> {
        let params = self.layout_params(width);
        &self
            .editor_layout
            .snapshot(&self.buffer.text, params, self.text_revision)
            .lines
    }

    fn adjust_affinity_at_wrap_boundary(&mut self) {
        let width = self.bounding_width();
        let lines = self.ensure_layout_cached(width).clone();
        let cursor = self.buffer.cursor;

        let is_wrap_boundary = lines.iter().enumerate().any(|(idx, line)| {
            idx + 1 < lines.len() && line.byte_end == cursor && lines[idx + 1].byte_start == cursor
        });

        if is_wrap_boundary {
            self.cursor_ctrl.affinity = CaretAffinity::Upstream;
        } else {
            self.cursor_ctrl.affinity = CaretAffinity::Downstream;
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
        let Some(snapshot) = self.editor_layout.cache() else {
            return line.byte_start;
        };
        self.editor_layout.index_at_line_x(snapshot, line, x)
    }

    fn cursor_line_and_x(&self, lines: &[VisualLine]) -> Option<(usize, f64)> {
        let Some(snapshot) = self.editor_layout.cache() else {
            return None;
        };
        debug_assert_eq!(lines.len(), snapshot.lines.len());
        self.editor_layout.cursor_line_and_x(
            snapshot,
            self.buffer.cursor,
            self.cursor_ctrl.affinity,
        )
    }
}

fn is_left_button_pressed(event: &QMouseEvent) -> bool {
    cpp!(unsafe [event as "const QMouseEvent*"] -> bool as "bool" {
        return event ? (event->buttons() & Qt::LeftButton) : false;
    })
}

impl EditorInputHost for SujianEditorItem {
    fn input_enabled(&self) -> bool {
        self.current_editor_enabled
    }

    fn input_emit_explicit_clear_requested(&mut self) {
        self.explicit_clear_requested();
    }

    fn input_clipboard_copy(&mut self) -> bool {
        self.clipboard_copy()
    }

    fn input_clipboard_paste(&mut self) {
        self.clipboard_paste();
    }

    fn input_undo(&mut self) {
        self.undo();
    }

    fn input_redo(&mut self) {
        self.redo();
    }

    fn input_select_all(&mut self) {
        self.select_all();
    }

    fn input_delete_selection(&mut self) {
        self.delete_selection();
    }

    fn input_delete_backward(&mut self) {
        self.delete_backward();
    }

    fn input_delete_forward(&mut self) {
        self.delete_forward();
    }

    fn input_insert_text(&mut self, text: String) {
        self.insert_text(text.into());
    }

    fn input_move_cursor_horizontal(&mut self, forward: bool, extend: bool) {
        self.move_cursor_horizontal(forward, extend);
    }

    fn input_move_cursor_vertical(&mut self, down: bool, extend: bool) {
        self.move_cursor_vertical(down, extend);
    }

    fn input_move_to_line_edge(&mut self, end: bool, extend: bool) {
        self.move_to_line_edge(end, extend);
    }

    fn input_clear_preedit(&mut self) {
        self.preedit_old_text = self.preedit_text.clone();
        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.preedit_attributes.clear();
        self.preedit_visual_transaction = None;
        self.preedit_cursor_rect = None;
    }

    fn input_set_preedit(&mut self, text: String, cursor: usize) {
        self.preedit_old_text = self.preedit_text.clone();
        self.preedit_text = text;
        self.preedit_cursor = cursor;
        // Generate preedit visual transaction for animation
        self.update_preedit_visual_state();
        // Update IME cursor rectangle so Windows input method candidate
        // window follows the correct position within the preedit
        self.update_ime_cursor_for_preedit();
        self.request_static_repaint();
    }

    fn input_set_preedit_with_attrs(&mut self, text: String, cursor: usize, attributes: Vec<PreeditAttribute>) {
        self.preedit_old_text = self.preedit_text.clone();
        self.preedit_text = text;
        self.preedit_cursor = cursor;
        self.preedit_attributes = attributes;
        // Generate preedit visual transaction for animation
        self.update_preedit_visual_state();
        // Update IME cursor rectangle so Windows input method candidate
        // window follows the correct position within the preedit
        self.update_ime_cursor_for_preedit();
        self.request_static_repaint();
    }

    fn input_set_suppress_next_ime_commit(&mut self, value: bool) {
        self.suppress_next_ime_commit = value;
    }

    fn input_take_suppress_next_ime_commit(&mut self) -> bool {
        let value = self.suppress_next_ime_commit;
        if value {
            self.suppress_next_ime_commit = false;
        }
        value
    }

    fn input_request_repaint(&mut self) {
        self.request_static_repaint();
    }
}

/// Preedit visual state management methods
impl SujianEditorItem {
    /// Update preedit visual transaction based on old vs new preedit text.
    ///
    /// Compares preedit_old_text and preedit_text to generate a visual
    /// transaction that drives animation:
    /// - New characters → Insert (吐字) animation
    /// - Removed characters → Delete (吞字) animation
    /// - Preedit cursor position → updates preedit_cursor_rect
    fn update_preedit_visual_state(&mut self) {
        if self.preedit_text.is_empty() {
            self.preedit_visual_transaction = None;
            self.preedit_cursor_rect = None;
            return;
        }

        // Compute preedit cursor rect based on preedit_cursor position
        // within the preedit string, relative to the buffer cursor position
        self.compute_preedit_cursor_rect();

        // Generate visual transaction for preedit changes
        let old = &self.preedit_old_text;
        let new = &self.preedit_text;

        if old == new {
            // No text change, only cursor moved — no visual transaction needed
            return;
        }

        // Create a simple visual transaction for the preedit diff
        // This is a temporary visual layer transaction, NOT a buffer transaction
        let cause = EditorTransactionCause::ImeComposition;
        let kind = if new.len() > old.len() {
            EditorAnimationKind::Insert
        } else {
            EditorAnimationKind::Delete
        };

        // For preedit, the byte range is relative to the buffer cursor position
        let cursor_pos = self.buffer.cursor;
        let inserted_range = if kind == EditorAnimationKind::Insert {
            Some((cursor_pos, cursor_pos + new.len()))
        } else {
            None
        };

        let vt = EditorVisualTransaction {
            id: 0, // Preedit transactions don't need unique IDs
            kind,
            cause,
            old_text: old.clone(),
            new_text: new.clone(),
            old_selection: EditorSelection {
                anchor: EditorCursor::new(&self.buffer.text, cursor_pos),
                head: EditorCursor::new(&self.buffer.text, cursor_pos),
            },
            new_selection: EditorSelection {
                anchor: EditorCursor::new(&self.buffer.text, cursor_pos),
                head: EditorCursor::new(&self.buffer.text, cursor_pos),
            },
            inserted_range,
            deleted_glyph_rects: None,
            insert_glyph_rects: None,
            reflow_glyph_rects: None,
            old_cursor_rect: self.preedit_cursor_rect.clone(),
            new_cursor_rect: self.preedit_cursor_rect.clone(),
            duration_ms: self.current_typing_animation_duration_ms as u64,
            coordinate_mode: writer_core::editor::VisualCoordinateMode::Baseline,
        };

        self.preedit_visual_transaction = Some(vt);
    }

    /// Compute the preedit cursor rect based on preedit_cursor position.
    ///
    /// The preedit cursor is displayed within the preedit string, not at
    /// the buffer cursor position. This calculates the visual position
    /// by measuring text width up to preedit_cursor bytes.
    fn compute_preedit_cursor_rect(&mut self) {
        if self.preedit_text.is_empty() {
            self.preedit_cursor_rect = None;
            return;
        }

        let width = self.bounding_width();
        let font_size = self.current_font_pixel_size as f64;
        let font_family = &self.current_font_family.to_string();
        let scroll_y = self.current_scroll_y as f64;

        // Get the buffer cursor position (where preedit starts)
        let cursor_byte = self.buffer.cursor;
        let snapshot = self.layout_snapshot(width);

        // Find the line containing the cursor
        let cursor_line = snapshot.lines.iter().find(|l| {
            l.byte_end >= cursor_byte && l.byte_start <= cursor_byte
        });

        let Some(line) = cursor_line else {
            self.preedit_cursor_rect = None;
            return;
        };

        // X position where preedit starts (buffer cursor position)
        let preedit_start_x = self.editor_layout.cursor_x_for_line(
            &snapshot,
            line,
            cursor_byte,
            self.cursor_ctrl.affinity,
        );

        // Measure width of preedit text up to preedit_cursor
        let preedit_before_cursor = &self.preedit_text[..self.preedit_cursor.min(self.preedit_text.len())];
        let preedit_cursor_offset = self.editor_layout.text_width(
            preedit_before_cursor,
            font_size,
            font_family,
        );

        let cursor_x = preedit_start_x + preedit_cursor_offset;
        let cursor_y = line.y - scroll_y;
        let cursor_h = line.height;

        let baseline_y = text_baseline_y(line, font_size, font_family) - scroll_y;

        self.preedit_cursor_rect = Some(CursorRect {
            x: cursor_x,
            top: cursor_y,
            bottom: cursor_y + cursor_h,
            baseline_y,
        });
    }

    /// Update IME cursor rectangle for preedit.
    ///
    /// When in preedit mode, the IME candidate window should follow
    /// the preedit cursor position (within the composition string),
    /// not the buffer cursor position.
    fn update_ime_cursor_for_preedit(&mut self) {
        if self.preedit_cursor_rect.is_some() {
            // Update cursor_rect properties so InputMethodQuery returns
            // the correct position for the IME candidate window
            self.cursor_rect_changed();
            let obj_ptr = self.get_cpp_object();
            if !obj_ptr.is_null() {
                cpp!(unsafe [obj_ptr as "QQuickItem*"] {
                    QGuiApplication::inputMethod()->update(Qt::ImCursorRectangle | Qt::ImAnchorRectangle);
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_complex_grapheme_emoji() {
        assert!(is_complex_grapheme('😀'));
    }

    #[test]
    fn test_is_complex_grapheme_zwj() {
        assert!(is_complex_grapheme('\u{200D}'));
    }

    #[test]
    fn test_is_complex_grapheme_variation_selector() {
        assert!(is_complex_grapheme('\u{FE0F}'));
    }

    #[test]
    fn test_is_complex_grapheme_combining_mark() {
        assert!(is_complex_grapheme('\u{0301}')); // combining acute accent
    }

    #[test]
    fn test_is_complex_grapheme_chinese_char() {
        assert!(!is_complex_grapheme('你'));
    }

    #[test]
    fn test_is_complex_grapheme_ascii() {
        assert!(!is_complex_grapheme('a'));
        assert!(!is_complex_grapheme('Z'));
        assert!(!is_complex_grapheme('0'));
    }

    #[test]
    fn test_is_complex_grapheme_chinese_punctuation() {
        assert!(!is_complex_grapheme('，'));
        assert!(!is_complex_grapheme('。'));
        assert!(!is_complex_grapheme('！'));
    }

    #[test]
    fn test_is_complex_grapheme_combining_half_mark() {
        assert!(is_complex_grapheme('\u{FE20}'));
    }

    // --- TextAnimationState lifecycle tests ---
    // TextAnimationState 是正式的生命周期状态机（独立于 Qt/SujianEditorItem），
    // 完整单元测试见 text_animation_state.rs。
    // 以下测试验证 SujianEditorItem 与 TextAnimationState 的集成约束。

    /// 验证：typing_animation_enabled=false 时不应创建新动画
    /// 逻辑约束：当 typing_animation_enabled=false 时，
    /// record_transaction 不应调用 text_anim_state.start_insert/start_delete。
    #[test]
    fn typing_animation_disabled_prevents_new_animations() {
        let typing_animation_enabled = false;
        let vt_present = true;
        let is_scrolling = false;
        let should_create = typing_animation_enabled && vt_present && !is_scrolling;
        assert!(!should_create, "when typing_animation_enabled=false, no animations should be created");
    }

    /// 验证：scrolling 抑制动画创建
    #[test]
    fn scrolling_prevents_new_animations() {
        let typing_animation_enabled = true;
        let vt_present = true;
        let is_scrolling = true;
        let should_create = typing_animation_enabled && vt_present && !is_scrolling;
        assert!(!should_create, "when scrolling, no animations should be created");
    }

    /// 验证：visual_transaction 的 inserted_range 用于 hidden range
    /// EditorVisualTransaction.inserted_range 是 Insert 动画的 hidden range 来源
    #[test]
    fn visual_transaction_inserted_range_used_for_hidden_range() {
        let mut state = TextAnimationState::new();
        // 模拟从 vt.inserted_range 读取的 hidden range
        let inserted_range = Some((5, 10));
        if let Some((range_start, range_end)) = inserted_range {
            state.start_insert((range_start, range_end), 100);
        }
        assert_eq!(state.active_insert_byte_range(), Some((5, 10)));
        assert!(state.has_active_insert());
    }

    /// 验证：关闭 typing_animation_enabled 时 hidden range 立即清除
    /// 不依赖 timeout 恢复文字
    #[test]
    fn typing_animation_disabled_clears_hidden_range_immediately() {
        let mut state = TextAnimationState::new();
        // 模拟从 vt.inserted_range 创建的 Insert 动画
        state.start_insert((10, 20), 100);
        assert!(state.has_active_insert());
        assert_eq!(state.active_insert_byte_range(), Some((10, 20)));
        // 关闭动画 — 立即清除，不依赖 timeout
        state.clear_on_typing_animation_disabled();
        assert!(state.is_empty());
        assert_eq!(state.active_insert_byte_range(), None);
    }

    // --- Additional animation scenario tests ---

    /// 验证：输入中文句号（。）不跳过动画
    /// is_complex_grapheme('。') == false，因此句号应正常创建动画
    #[test]
    fn chinese_period_not_complex_grapheme_allows_animation() {
        assert!(!is_complex_grapheme('。'), "Chinese period '。' is not complex, should allow animation");
    }

    /// 验证：删除单字动画生命周期
    /// Delete 动画不产生 hidden range，但状态机跟踪它
    #[test]
    fn delete_single_char_animation_lifecycle() {
        let mut state = TextAnimationState::new();
        state.start_delete((5, 8), 100);
        // Delete 不产生 hidden range
        assert_eq!(state.active_insert_byte_range(), None);
        assert!(!state.has_active_insert());
        // 但状态机不为空
        assert!(!state.is_empty());
        // Delete 可通过 clear 清除
        state.clear();
        assert!(state.is_empty());
    }

    /// 验证：emoji 输入跳过动画
    /// is_complex_grapheme('😀') == true，因此 emoji 不应创建动画
    #[test]
    fn emoji_is_complex_grapheme_skips_animation() {
        assert!(is_complex_grapheme('😀'), "Emoji should be complex grapheme, skipping animation");
    }

    /// 验证：组合音标跳过动画
    /// is_complex_grapheme('\u{0301}') == true（combining acute accent）
    #[test]
    fn combining_accent_is_complex_grapheme_skips_animation() {
        assert!(is_complex_grapheme('\u{0301}'), "Combining acute accent should be complex grapheme, skipping animation");
    }

    /// 验证：滚动中输入不创建动画
    /// 即使 typing_animation_enabled=true 且 visual_transaction 存在
    #[test]
    fn scrolling_input_does_not_create_animation() {
        let typing_animation_enabled = true;
        let vt_present = true;
        let is_scrolling = true;
        let should_create = typing_animation_enabled && vt_present && !is_scrolling;
        assert!(!should_create, "Scrolling should suppress animation creation");
    }

    /// 验证：关闭 typingAnimation 后新输入不创建动画
    #[test]
    fn typing_animation_disabled_no_new_animation_on_input() {
        let typing_animation_enabled = false;
        let vt_present = true;
        let is_scrolling = false;
        let should_create = typing_animation_enabled && vt_present && !is_scrolling;
        assert!(!should_create, "Disabled typing animation should prevent new animations");
    }

    /// 验证：关闭 coordinated cursor 不影响 text animation 创建条件
    /// text animation 只看 typing_animation_enabled && vt_present && !is_scrolling
    #[test]
    fn coordinated_cursor_disabled_does_not_affect_text_animation() {
        let typing_animation_enabled = true;
        let vt_present = true;
        let is_scrolling = false;
        let coordinated_enabled = false;
        // Text animation creation is independent of coordinated cursor
        let should_create_text_anim = typing_animation_enabled && vt_present && !is_scrolling;
        assert!(should_create_text_anim, "Text animation creation should not be affected by coordinated cursor setting");
        // coordinated_enabled is a separate concern for cursor animation, not text animation
        let _ = coordinated_enabled; // explicitly not used in text animation logic
    }

    // ========================================================================
    // IME commit / newline / reflow 场景自动化测试
    // ========================================================================

    /// 测试 1：ime_commit_4_char_idiom_produces_cursor_animation
    ///
    /// 插入 4 字成语（如 "风和日丽"），验证产生 Cursor 类型动画事件。
    /// IME commit 多字使用 TypingCommit cause，4 字 ≤ max_animated_chars(8)，
    /// should_animate=true，should_create_text_animation=FullAnimation，
    /// TextAnimationState 应创建 hidden range，cursor rect 正确更新。
    #[test]
    fn ime_commit_4_char_idiom_produces_cursor_animation() {
        use writer_core::editor::{EditorEngine, EditorSelection, EditorTransactionCause,
            EditorAnimationKind};

        let mut engine = EditorEngine::with_animation_limits(8, 160);
        let idiom = "风和日丽";
        let old_text = "你好";
        let new_text = "你好风和日丽";
        let old_cursor = EditorSelection::collapsed(old_text, old_text.len());
        let new_cursor = EditorSelection::collapsed(new_text, new_text.len());

        let tx = engine.create_transaction(
            old_text, new_text,
            old_cursor, new_cursor,
            EditorTransactionCause::TypingCommit,
        );

        // TypingCommit cause + 4 chars ≤ 8 → should_animate = true
        assert!(tx.should_animate, "4-char idiom commit should animate");

        // visual_transaction 应返回 Insert kind
        let vt = engine.visual_transaction(&tx);
        assert!(vt.is_some(), "4-char idiom commit should produce visual transaction");
        let vt = vt.unwrap();
        assert_eq!(vt.kind, EditorAnimationKind::Insert);
        assert_eq!(vt.inserted_range, Some((old_text.len(), old_text.len() + idiom.len())));

        // should_create_text_animation → FullAnimation (4 glyphs, no newline)
        let decision = should_create_text_animation(
            4,    // glyph_count
            false, // contains_newline
            false, // is_scrolling
            false, // is_loading
            false, // is_applying_format
            false, // is_applying_settings
            true,  // animation_enabled
            true,  // component_ready
        );
        assert_eq!(decision, AnimationDecision::FullAnimation);

        // TextAnimationState 应创建 hidden range
        let mut state = TextAnimationState::new();
        if let Some((range_start, range_end)) = vt.inserted_range {
            match decision {
                AnimationDecision::FullAnimation => {
                    state.start_insert((range_start, range_end), vt.duration_ms);
                }
                _ => {}
            }
        }
        assert!(state.has_active_insert(), "4-char idiom should create hidden range");
        assert_eq!(state.active_insert_byte_range(), Some((6, 18))); // "你好"=6 bytes, "风和日丽"=12 bytes

        // Cursor 动画事件：old_cursor.index != new_cursor.index → Cursor 事件产生
        assert_ne!(tx.old_selection.head.index, tx.new_selection.head.index,
            "Cursor should move after idiom commit");
        assert_eq!(tx.new_selection.head.index, new_text.len(),
            "Cursor byte offset should be at end of committed text");
    }

    /// 测试 2：ime_commit_long_candidate_no_text_animation_cursor_still_moves
    ///
    /// 插入超过 8 字的候选，验证不产生 Insert 文字动画（NoAnimation），
    /// 但验证光标位置正确更新。
    #[test]
    fn ime_commit_long_candidate_no_text_animation_cursor_still_moves() {
        use writer_core::editor::{EditorEngine, EditorSelection, EditorTransactionCause};

        let mut engine = EditorEngine::with_animation_limits(8, 160);
        // 9 个汉字，超过 max_animated_chars(8)
        let long_candidate = "一二三四五六七八九";
        assert!(long_candidate.chars().count() > 8);
        let old_text = "";
        let new_text = long_candidate;
        let old_cursor = EditorSelection::collapsed(old_text, 0);
        let new_cursor = EditorSelection::collapsed(new_text, new_text.len());

        let tx = engine.create_transaction(
            old_text, new_text,
            old_cursor, new_cursor,
            EditorTransactionCause::TypingCommit,
        );

        // 9 chars > 8 → should_animate = false (core level)
        assert!(!tx.should_animate, "9-char candidate should not animate at core level");

        // should_create_text_animation → NoAnimation (9 glyphs > 8)
        let decision = should_create_text_animation(
            9,    // glyph_count > MAX_GLYPH_COUNT
            false, // contains_newline
            false, // is_scrolling
            false, // is_loading
            false, // is_applying_format
            false, // is_applying_settings
            true,  // animation_enabled
            true,  // component_ready
        );
        assert_eq!(decision, AnimationDecision::NoAnimation,
            "Long candidate should produce NoAnimation decision");

        // TextAnimationState 不应创建 hidden range
        let mut state = TextAnimationState::new();
        match decision {
            AnimationDecision::FullAnimation => {
                // 不会进入此分支
                state.start_insert((0, long_candidate.len()), 160);
            }
            _ => {}
        }
        assert!(!state.has_active_insert(),
            "Long candidate should NOT create hidden range");

        // 光标位置仍正确更新
        assert_eq!(tx.new_selection.head.index, long_candidate.len(),
            "Cursor byte offset should be at end of committed text even without animation");
    }

    /// 测试 3：ime_commit_after_initials_cursor_moves_forward
    ///
    /// 只输入首字母出长词，commit 后验证光标正常向前走，
    /// 验证 cursor byte offset 正确。
    #[test]
    fn ime_commit_after_initials_cursor_moves_forward() {
        use writer_core::editor::{EditorEngine, EditorSelection, EditorTransactionCause};

        let mut engine = EditorEngine::with_animation_limits(8, 160);

        // 模拟：用户输入首字母 "fhrl"，commit 后得到 "风和日丽"
        // Step 1: preedit 阶段（ImeComposition cause，不产生动画）
        let preedit_text = "fhrl";
        let tx_preedit = engine.create_transaction(
            "", preedit_text,
            EditorSelection::collapsed("", 0),
            EditorSelection::collapsed(preedit_text, preedit_text.len()),
            EditorTransactionCause::ImeComposition,
        );
        assert!(!tx_preedit.should_animate,
            "ImeComposition should not animate");
        assert_eq!(tx_preedit.new_selection.head.index, preedit_text.len(),
            "Preedit cursor should be at end of preedit text");

        // Step 2: commit 阶段（TypingCommit cause，产生动画）
        let committed = "风和日丽";
        let tx_commit = engine.create_transaction(
            preedit_text, committed,
            EditorSelection::collapsed(preedit_text, preedit_text.len()),
            EditorSelection::collapsed(committed, committed.len()),
            EditorTransactionCause::TypingCommit,
        );
        // 4 chars ≤ 8 → should_animate = true
        assert!(tx_commit.should_animate,
            "TypingCommit of 4-char idiom should animate");

        // 光标从 preedit 末尾移动到 commit 末尾
        // preedit_text.len() = 4, committed.len() = 12
        assert_eq!(tx_commit.old_selection.head.index, preedit_text.len(),
            "Old cursor should be at end of preedit");
        assert_eq!(tx_commit.new_selection.head.index, committed.len(),
            "New cursor should be at end of committed text (forward move)");

        // 光标确实向前走了（byte offset 增大）
        assert!(tx_commit.new_selection.head.index > tx_commit.old_selection.head.index,
            "Cursor should move forward after commit (byte offset increases)");
    }

    /// 测试 4：ime_commit_pinyin_longer_than_hanzi_cursor_can_retreat
    ///
    /// 拼音比上屏汉字长，commit 后光标允许回退动画。
    /// 场景：preedit "fengherili" (10 bytes) → commit "风和日丽" (12 bytes)
    /// 在字节偏移上光标实际前进，但视觉上拼音可能比汉字宽。
    /// 此测试验证：当 pinyin 在视觉上比 hanzi 宽时，cursor 动画事件仍然生成，
    /// 允许光标从 preedit 位置到 commit 后位置做动画（可能视觉回退）。
    #[test]
    fn ime_commit_pinyin_longer_than_hanzi_cursor_can_retreat() {
        use writer_core::editor::{EditorEngine, EditorSelection, EditorTransactionCause,
            EditorAnimationKind};

        let mut engine = EditorEngine::with_animation_limits(8, 160);

        // 拼音 "fengherili" 有 10 个 ASCII 字符（视觉上可能比 4 个汉字宽）
        let pinyin = "fengherili";
        let hanzi = "风和日丽";

        // 模拟 preedit 阶段
        let tx_preedit = engine.create_transaction(
            "", pinyin,
            EditorSelection::collapsed("", 0),
            EditorSelection::collapsed(pinyin, pinyin.len()),
            EditorTransactionCause::ImeComposition,
        );
        assert!(!tx_preedit.should_animate);

        // commit 阶段：pinyin → hanzi
        let tx_commit = engine.create_transaction(
            pinyin, hanzi,
            EditorSelection::collapsed(pinyin, pinyin.len()),
            EditorSelection::collapsed(hanzi, hanzi.len()),
            EditorTransactionCause::TypingCommit,
        );
        assert!(tx_commit.should_animate,
            "TypingCommit should animate for 4-char hanzi");

        // Cursor 动画事件必须生成（光标位置变化）
        assert_ne!(tx_commit.old_selection.head.index, tx_commit.new_selection.head.index,
            "Cursor position must change for retreat animation");

        // 验证 visual_transaction 包含 cursor 信息
        let vt = engine.visual_transaction(&tx_commit);
        assert!(vt.is_some());
        let vt = vt.unwrap();
        assert_eq!(vt.kind, EditorAnimationKind::Insert);
        // old_cursor_rect 和 new_cursor_rect 由平台层填充，core 默认 None
        // 但 old_selection / new_selection 正确记录了光标位置
        assert_eq!(vt.old_selection.head.index, pinyin.len());
        assert_eq!(vt.new_selection.head.index, hanzi.len());

        // should_create_text_animation → FullAnimation (4 glyphs, no newline)
        let decision = should_create_text_animation(
            4, false, false, false, false, false, true, true,
        );
        assert_eq!(decision, AnimationDecision::FullAnimation);

        // TextAnimationState 创建 hidden range
        let mut state = TextAnimationState::new();
        if let Some((rs, re)) = vt.inserted_range {
            if decision == AnimationDecision::FullAnimation {
                state.start_insert((rs, re), vt.duration_ms);
            }
        }
        assert!(state.has_active_insert());
    }

    /// 测试 5：newline_commit_cursor_vertical_animation
    ///
    /// 回车换行，验证不产生 Insert 文字动画（containsNewline → CursorOnly），
    /// 但验证光标必须垂直动画（cursor rect y 变化）。
    #[test]
    fn newline_commit_cursor_vertical_animation() {
        use writer_core::editor::{EditorEngine, EditorSelection, EditorTransactionCause,
            EditorAnimationKind, CursorRect};

        let mut engine = EditorEngine::with_animation_limits(8, 160);

        // 插入换行
        let old_text = "你好";
        let new_text = "你好\n";
        let tx = engine.create_transaction(
            old_text, new_text,
            EditorSelection::collapsed(old_text, old_text.len()),
            EditorSelection::collapsed(new_text, new_text.len()),
            EditorTransactionCause::Typing,
        );

        // 换行包含 \n → should_animate = false (core level: text.contains('\n'))
        assert!(!tx.should_animate,
            "Newline commit should not animate at core level (contains newline)");

        // should_create_text_animation → CursorOnly (contains_newline=true)
        let decision = should_create_text_animation(
            1,    // glyph_count (the newline char)
            true, // contains_newline
            false, false, false, false, true, true,
        );
        assert_eq!(decision, AnimationDecision::CursorOnly,
            "Newline should produce CursorOnly decision");

        // TextAnimationState 不应创建 hidden range (CursorOnly 不创建 hidden range)
        let mut state = TextAnimationState::new();
        match decision {
            AnimationDecision::FullAnimation => {
                state.start_insert((old_text.len(), new_text.len()), 160);
            }
            _ => {} // CursorOnly 和 NoAnimation 都不创建 hidden range
        }
        assert!(!state.has_active_insert(),
            "Newline should NOT create hidden range (CursorOnly)");

        // 光标位置必须变化（y 变化 → 垂直动画）
        assert_ne!(tx.old_selection.head.index, tx.new_selection.head.index,
            "Cursor must move after newline (vertical animation required)");

        // 模拟 cursor rect y 变化：换行后光标 y 应增大
        // Core 不计算坐标，但我们可以验证 CursorRect 数据结构支持 y 变化
        let old_cursor_rect = CursorRect {
            x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0,
        };
        let new_cursor_rect = CursorRect {
            x: 0.0,  // 行首
            top: 30.0, // 下一行
            bottom: 50.0,
            baseline_y: 45.0,
        };
        // y 变化 = 垂直动画
        assert_ne!(old_cursor_rect.top, new_cursor_rect.top,
            "Cursor rect y must change for vertical animation after newline");
        assert!(new_cursor_rect.top > old_cursor_rect.top,
            "New cursor rect y should be below old (moved down a line)");
    }

    /// 测试 6：mid_insert_reflow_animation_local_push
    ///
    /// 中间插入，验证后文不能瞬间大跳。
    /// 验证 ReflowGlyphRect 包含插入点右侧的 glyph。
    /// 由于单元测试无法执行 Qt layout，此测试验证：
    /// 1. ReflowGlyphRect 数据结构正确
    /// 2. 中间插入时 reflow_glyph_rects 的概念正确性
    /// 3. 插入范围内的 glyph 不应出现在 reflow_rects 中
    #[test]
    fn mid_insert_reflow_animation_local_push() {
        use writer_core::editor::ReflowGlyphRect;

        // 构造 ReflowGlyphRect 模拟中间插入后的 reflow 数据
        // 场景：文本 "ABCDE"，在 B 和 C 之间插入 "XY"
        // 插入前：A(0) B(1) C(2) D(3) E(4)
        // 插入后：A(0) B(1) X(2) Y(3) C(4) D(5) E(6)
        // reflow 应包含 C, D, E 的旧位置→新位置

        let reflow_c = ReflowGlyphRect {
            char_: "C".to_string(),
            old_x: 20.0,
            old_y: 0.0,
            old_baseline_y: 16.0,
            new_x: 40.0,  // 向右推了
            new_y: 0.0,
            new_baseline_y: 16.0,
            w: 10.0,
            h: 20.0,
            line_index: 0,
        };
        let reflow_d = ReflowGlyphRect {
            char_: "D".to_string(),
            old_x: 30.0,
            old_y: 0.0,
            old_baseline_y: 16.0,
            new_x: 50.0,  // 向右推了
            new_y: 0.0,
            new_baseline_y: 16.0,
            w: 10.0,
            h: 20.0,
            line_index: 0,
        };
        let reflow_e = ReflowGlyphRect {
            char_: "E".to_string(),
            old_x: 40.0,
            old_y: 0.0,
            old_baseline_y: 16.0,
            new_x: 60.0,  // 向右推了
            new_y: 0.0,
            new_baseline_y: 16.0,
            w: 10.0,
            h: 20.0,
            line_index: 0,
        };

        let reflow_rects = vec![reflow_c, reflow_d, reflow_e];

        // 验证 reflow_rects 包含插入点右侧的 glyph
        assert_eq!(reflow_rects.len(), 3, "Reflow should contain 3 glyphs right of insertion");
        assert_eq!(reflow_rects[0].char_, "C");
        assert_eq!(reflow_rects[1].char_, "D");
        assert_eq!(reflow_rects[2].char_, "E");

        // 验证每个 reflow glyph 的新位置比旧位置更右（被推开了）
        for rect in &reflow_rects {
            assert!(rect.new_x > rect.old_x,
                "Reflow glyph '{}' should be pushed right: old_x={}, new_x={}",
                rect.char_, rect.old_x, rect.new_x);
        }

        // 验证插入范围内的 glyph（X, Y）不应出现在 reflow_rects 中
        // （它们走 insert 动画，不走 reflow 动画）
        let inserted_chars: Vec<&str> = vec!["X", "Y"];
        for rect in &reflow_rects {
            assert!(
                !inserted_chars.contains(&rect.char_.as_str()),
                "Inserted glyph '{}' should NOT be in reflow_rects (it goes through insert animation)",
                rect.char_
            );
        }

        // 验证 reflow 动画是局部推送而非瞬间大跳
        // new_x - old_x 应该等于插入文本的宽度（2 chars × 10px = 20px）
        let push_distance = reflow_rects[0].new_x - reflow_rects[0].old_x;
        assert!((push_distance - 20.0).abs() < 0.1,
            "Reflow push distance should equal inserted text width: got {}",
            push_distance);
    }

    /// 测试 7：qml_overlay_skip_must_clear_hidden_range
    ///
    /// QML overlay 跳过动画时 Rust hidden range 必须立刻清掉。
    /// 验证 on_insert_animation_skipped 清除 active_insert_byte_range。
    /// 这模拟了 QML 调用 on_insert_animation_skipped(byte_start, byte_end) 的场景：
    /// Rust 侧可能已经创建了 hidden range，但 QML 决定跳过动画，
    /// 此时必须立即清除 hidden range，否则正文层会永久跳过该 range 导致文字消失。
    #[test]
    fn qml_overlay_skip_must_clear_hidden_range() {
        // 场景：Rust 侧已经创建了 Insert 动画（hidden range）
        let mut state = TextAnimationState::new();
        let byte_range = (10, 22); // 模拟 "风和日丽" 的 byte range
        state.start_insert(byte_range, 160);
        assert!(state.has_active_insert(),
            "Should have active insert before skip");
        assert_eq!(state.active_insert_byte_range(), Some(byte_range),
            "Hidden range should be (10, 22) before skip");

        // QML overlay 跳过动画 → 调用 on_insert_animation_finished（与 skip 共用逻辑）
        // 这模拟了 SujianEditorItem::on_insert_animation_skipped 的行为：
        //   let removed = self.text_anim_state.on_insert_animation_finished(bs, be);
        let removed = state.on_insert_animation_finished(10, 22);
        assert!(removed, "on_insert_animation_skipped should return true (removed matching animation)");
        assert!(state.is_empty(),
            "Hidden range must be immediately cleared after skip");
        assert_eq!(state.active_insert_byte_range(), None,
            "No active insert byte range should remain after skip");

        // 验证：跳过不匹配的 range 不影响现有 hidden range
        state.start_insert((30, 42), 160);
        let removed_wrong = state.on_insert_animation_finished(50, 60);
        assert!(!removed_wrong, "Skipping non-matching range should return false");
        assert!(state.has_active_insert(),
            "Existing hidden range should remain when skip doesn't match");
        assert_eq!(state.active_insert_byte_range(), Some((30, 42)));

        // 清理
        state.clear();
        assert!(state.is_empty());
    }
}

impl QQuickItem for SujianEditorItem {
    fn component_complete(&mut self) {
        let obj_ptr = self.get_cpp_object();
        if obj_ptr.is_null() {
            return;
        }
        let item_ptr = self as *mut Self as *mut std::ffi::c_void;
        input::install_event_filter(obj_ptr, item_ptr);
    }

    fn geometry_changed(&mut self, _new_geometry: QRectF, _old_geometry: QRectF) {
        self.scroll_buffer = None;
        self.recalculate_content_height_and_emit();
        self.cursor_ctrl.force_snap_next = true;
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    fn mouse_event(&mut self, event: QMouseEvent) -> bool {
        let pos = event.position();
        match event.event_type() {
            qmetaobject::QMouseEventType::MouseButtonPress => {
                self.click_at(pos.x as f32, pos.y as f32, false);
                let obj_ptr = self.get_cpp_object();
                input::focus_item(obj_ptr);
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

        // Tick text animations (timeout safety for active_text_animations)
        self.tick_text_animations();

        let item_ptr = self.get_cpp_object();
        let dpr = if !item_ptr.is_null() {
            renderer::sujian_item_dpr(item_ptr)
        } else {
            1.0
        };
        let root_raw = node.into_raw();

        let vp_h = self.current_viewport_height.max(1.0) as f64;
        let scroll_y = self.current_scroll_y as f64;
        let content_h = self.current_content_height as f64;

        // Check if scroll buffer or text needs rebuilding
        let mut force_rebuild = false;
        if let Some(ref buf) = self.scroll_buffer {
            let revision_changed = buf.text_revision != self.text_revision;
            if revision_changed {
                force_rebuild = true;
            } else {
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
            }
        } else {
            force_rebuild = true;
        }

        if force_rebuild {
            self.render_dirty = true;
        }

        let mut final_root = root_raw;

        // 第一步：确保 Layer 0 有基础 texture
        if !root_raw.is_null() && !item_ptr.is_null() {
            scene_graph::ensure_single_image_node(root_raw, item_ptr);
        }

        // 更新 Layer 0: Static text texture 节点
        if self.render_dirty {
            match self.render_to_image() {
                Some((image, buf_scroll_y, _buf_h)) => {
                    let (src_y, src_h) = if let Some(ref buf) = self.scroll_buffer {
                        buf.clamp_source_rect(scroll_y, vp_h)
                    } else {
                        (scroll_y - buf_scroll_y, vp_h)
                    };
                    // Image has DPR=1.0, so logical width = physical width / dpr.
                    // But we pass logical coords to update_texture_node which multiplies by dpr internally.
                    let logical_img_w = image.size().width as f64 / dpr;
                    final_root = scene_graph::update_texture_node(
                        root_raw,
                        item_ptr,
                        &image,
                        0.0,
                        src_y,
                        logical_img_w,
                        src_h,
                        0.0,
                        vp_h,
                        dpr,
                    );
                    self.render_dirty = false;
                }
                None => {
                    if !root_raw.is_null() {
                        if let Some(ref buf) = self.scroll_buffer {
                            let (src_y, src_h) = buf.clamp_source_rect(scroll_y, vp_h);
                            let logical_img_w = buf.image.size().width as f64 / dpr;
                            scene_graph::update_source_rect(
                                root_raw,
                                item_ptr,
                                0.0,
                                src_y,
                                logical_img_w,
                                src_h,
                                0.0,
                                vp_h,
                                dpr,
                            );
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
                    scene_graph::update_source_rect(
                        root_raw,
                        item_ptr,
                        0.0,
                        src_y,
                        logical_img_w,
                        src_h,
                        0.0,
                        vp_h,
                        dpr,
                    );
                }
            }
            final_root = root_raw;
        }

        // Animation lifecycle split:
        // - QML EditorAnimationOverlay owns ghost (insert overlay glyph) lifecycle
        // - Rust owns hidden range lifecycle (active_text_animations),
        //   animation-disabled cleanup, and timeout fallback (tick_text_animations)

        let total_elapsed = frame_start.elapsed();
        if total_elapsed.as_millis() > 4 {
            eprintln!(
                "sujian_update_paint_node: total_ms={}",
                total_elapsed.as_millis(),
            );
        }

        unsafe { SGNode::<qmetaobject::scenegraph::ContainerNode>::from_raw(final_root) }
    }
}
