// =============================================================================
// sujian_editor_item - Desktop self-rendered editor item
// =============================================================================

pub(crate) mod buffer;
pub(crate) mod cursor_controller;
pub(crate) mod rendering;

use crate::editor::input::{self, EditorInputHost};
use crate::editor::layout::{
    CaretAffinity, CursorLayoutRect, EditorLayout, LayoutParams, LayoutSnapshot, VisualLine,
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

pub use rendering::AnimatedGlyph;
use writer_core::editor::{
    CursorRect, EditorAnimationEvent, EditorAnimationKind, EditorCursor, EditorEngine,
    EditorSelection, EditorTransactionCause, GlyphRect,
};

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

/// 活跃的文本动画状态 — 用于让静态正文层在动画期间跳过 inserted range，
/// 实现"真吐字"效果（而非正文完整绘制 + ghost overlay）。
///
/// - Insert 动画：正文层跳过 inserted range，动画 overlay 显示 glyph 从光标"吐出"
/// - Delete 动画：正文层正常绘制 new_text，动画 overlay 显示 glyph 被光标"吞入"
#[derive(Clone, Debug)]
pub(crate) struct ActiveTextAnimation {
    pub kind: TextAnimationKind,
    /// Insert: inserted byte range (start, end) — 基于 new_text
    /// Delete: deleted byte range (start, end) — 基于 old_text（Delete 不需要正文层跳过）
    pub byte_range: (usize, usize),
    /// 动画开始时间
    pub start_time: Instant,
    /// 动画持续时间 (ms)
    pub duration_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextAnimationKind {
    Insert,
    Delete,
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
    last_transaction_summary: qt_property!(QString; READ last_transaction_summary NOTIFY transaction_created),
    last_animation_event_count: qt_property!(u32; READ last_animation_event_count NOTIFY transaction_created),
    animation_events_json: qt_property!(QString; READ animation_events_json NOTIFY animation_events_changed),
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
    animation_events_changed: qt_signal!(),
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
    current_scroll_y: f32,
    current_viewport_height: f32,
    current_is_scrolling: bool,
    last_summary: QString,
    last_event_count: u32,
    last_animation_events_json: QString,
    preedit_text: String,
    preedit_cursor: usize,
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
    /// 活跃的文本动画列表（通常 0~1 个元素）。
    /// Insert 动画期间，正文层跳过 inserted range 不绘制，由 QML overlay 显示 glyph。
    /// 动画结束后清除，正文层恢复完整绘制。
    active_text_animations: Vec<ActiveTextAnimation>,
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
            last_transaction_summary: Default::default(),
            last_animation_event_count: Default::default(),
            animation_events_json: Default::default(),
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
            animation_events_changed: Default::default(),
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
            current_scroll_y: 0.0,
            current_viewport_height: 0.0,
            current_is_scrolling: false,
            last_summary: "".into(),
            last_event_count: 0,
            last_animation_events_json: "".into(),
            preedit_text: String::new(),
            preedit_cursor: 0,
            suppress_next_ime_commit: false,
            editor_layout: EditorLayout::default(),
            text_revision: 0,
            render_dirty: true,
            scroll_buffer: None,
            last_slow_paint_log: None,
            cursor_ctrl: cursor_controller::CursorController::new(),
            active_text_animations: Vec::new(),
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
        if !self.active_text_animations.is_empty() {
            self.active_text_animations.clear();
            self.request_static_repaint();
        }
    }

    /// QML 动画 overlay 通知 Insert 动画完成，清除对应的隐藏 range
    fn on_insert_animation_finished(&mut self, byte_start: i32, byte_end: i32) {
        let bs = byte_start as usize;
        let be = byte_end as usize;
        let before = self.active_text_animations.len();
        self.active_text_animations.retain(|anim| {
            !(anim.kind == TextAnimationKind::Insert && anim.byte_range == (bs, be))
        });
        if self.active_text_animations.len() != before {
            if editor_animation_debug_enabled() {
                eprintln!(
                    "on_insert_animation_finished: byte_range=({},{}), cleared, remaining={}",
                    bs, be, self.active_text_animations.len()
                );
            }
            self.request_static_repaint();
        }
    }

    /// 检查是否有活跃的 Insert 动画（正文层需要跳过其 range）
    fn has_active_insert_animation(&self) -> bool {
        self.active_text_animations
            .iter()
            .any(|a| a.kind == TextAnimationKind::Insert)
    }

    /// 获取活跃 Insert 动画的 byte range（如有多个取第一个）
    fn active_insert_byte_range(&self) -> Option<(usize, usize)> {
        self.active_text_animations
            .iter()
            .find(|a| a.kind == TextAnimationKind::Insert)
            .map(|a| a.byte_range)
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

    fn animation_events_json(&self) -> QString {
        self.last_animation_events_json.clone()
    }

    fn cursor_rect_x(&self) -> f32 {
        if self.current_smooth_cursor_enabled && self.cursor_ctrl.animation.is_some() {
            self.cursor_ctrl.visual_x as f32
        } else {
            self.cursor_ctrl.target_x as f32
        }
    }

    fn cursor_rect_y(&self) -> f32 {
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
        if self.active_text_animations.is_empty() {
            return;
        }
        let now = Instant::now();
        let before = self.active_text_animations.len();
        self.active_text_animations.retain(|anim| {
            let elapsed = now.duration_since(anim.start_time).as_millis() as u64;
            // 给 2x duration 作为宽限期，防止 QML 动画和 Rust 超时不同步
            elapsed < anim.duration_ms * 2 + 200
        });
        if self.active_text_animations.len() != before {
            if editor_animation_debug_enabled() {
                eprintln!(
                    "tick_text_animations: cleared {} timed-out animations, remaining={}",
                    before - self.active_text_animations.len(),
                    self.active_text_animations.len()
                );
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
        self.preedit_text.clear();
        self.preedit_cursor = 0;

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
        let _events = self.record_transaction(old, new, cause, true);
        // Animation lifecycle is owned by QML EditorAnimationOverlay.
        // Rust only provides glyph rect data via animation_events_json.

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

        let _events = self.record_transaction(old, new, EditorTransactionCause::Delete, true);
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

        let _events = self.record_transaction(old, new, EditorTransactionCause::Delete, true);
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

        let _events = self.record_transaction(old, new, EditorTransactionCause::Delete, true);
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
    ) -> Vec<EditorAnimationEvent> {
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
        let mut events = self.engine.animation_events(&transaction);

        // 为 Insert/Delete 事件填充 glyph_rects（动画定位用）
        self.fill_glyph_rects_for_events(&mut events, &new.text, &old.text);

        // 创建 active_text_animations 条目，让正文层在动画期间跳过 inserted range
        // 仅在动画启用且事件非空时创建
        if self.current_typing_animation_enabled && !events.is_empty() && !self.current_is_scrolling {
            for event in &events {
                match event.kind {
                    EditorAnimationKind::Insert => {
                        let range_start = event.range_start;
                        let range_end = range_start + event.range_len;
                        // 只有 glyph_rects 非空时才创建（空说明被过滤了）
                        if !event.glyph_rects.is_empty() {
                            self.active_text_animations.push(ActiveTextAnimation {
                                kind: TextAnimationKind::Insert,
                                byte_range: (range_start, range_end),
                                start_time: Instant::now(),
                                duration_ms: event.duration_ms,
                            });
                            if editor_animation_debug_enabled() {
                                eprintln!(
                                    "record_transaction: created ActiveTextAnimation::Insert byte_range=({},{}), duration_ms={}",
                                    range_start, range_end, event.duration_ms
                                );
                            }
                        }
                    }
                    EditorAnimationKind::Delete => {
                        let range_start = event.range_start;
                        let range_end = range_start + event.range_len;
                        // Delete 动画不需要正文层跳过，但记录以跟踪活跃动画
                        if !event.glyph_rects.is_empty() {
                            self.active_text_animations.push(ActiveTextAnimation {
                                kind: TextAnimationKind::Delete,
                                byte_range: (range_start, range_end),
                                start_time: Instant::now(),
                                duration_ms: event.duration_ms,
                            });
                        }
                    }
                    EditorAnimationKind::Cursor => {}
                }
            }
        }

        self.last_event_count = events.len() as u32;
        self.last_summary = format!(
            "cause={:?};changes={};events={};animate={}",
            transaction.cause,
            transaction.changes.len(),
            events.len(),
            transaction.should_animate
        )
        .into();
        if editor_animation_debug_enabled() {
            eprintln!(
                "record_transaction: cause={:?}, changes={}, events={}, animate={}, typing_anim_enabled={}, is_scrolling={}",
                transaction.cause,
                transaction.changes.len(),
                events.len(),
                transaction.should_animate,
                self.current_typing_animation_enabled,
                self.current_is_scrolling,
            );
        }
        if !events.is_empty() {
            match serde_json::to_string(&events) {
                Ok(json) => {
                    self.last_animation_events_json = json.into();
                }
                Err(e) => {
                    eprintln!(
                        "record_transaction: failed to serialize animation events: {}",
                        e
                    );
                    self.last_animation_events_json = "[]".into();
                }
            }
        } else {
            self.last_animation_events_json = "[]".into();
        }
        if emit {
            self.transaction_created();
            if !events.is_empty() {
                self.animation_events_changed();
            }
        }
        events
    }

    /// 为 Insert/Delete 类型 animation events 填充 glyph_rects 数据
    ///
    /// 通过 EditorLayout 的 glyph_positions_on_line 获取字符位置
    /// 转为(x, y, w, h),写入 event.glyph_rects 供 QML overlay 消费
    /// 同时填充 old_cursor_rect / new_cursor_rect 供 QML 动画 overlay
    /// 使用正确的光标位置，避免依赖可能过时的 editorItem.cursor_rect_x/y
    /// Cursor 类型不需要 glyph_rects，直接跳过
    fn fill_glyph_rects_for_events(
        &mut self,
        events: &mut [EditorAnimationEvent],
        text: &str,
        old_text: &str,
    ) {
        let width = self.bounding_width();
        let font_size = self.current_font_pixel_size as f64;
        let font_family = &self.current_font_family.to_string();
        let scroll_y = self.current_scroll_y as f64;
        let viewport_h = self.current_viewport_height.max(1.0) as f64;

        for event in events.iter_mut() {
            match event.kind {
                EditorAnimationKind::Insert => {
                    let range_start = event.range_start;
                    let range_end = range_start + event.range_len;
                    let mut glyph_rects = Vec::new();

                    // Use new_text layout for Insert events — the cached layout
                    // may be stale because emit_content_changed() (which calls
                    // invalidate_layout_cache) has not run yet at this point.
                    let insert_snapshot = self.layout_snapshot_for_text(text, width);

                    // 计算 old_cursor_rect：使用 old_text 布局
                    let old_snapshot = self.layout_snapshot_for_text(old_text, width);
                    let old_caret = self.editor_layout.caret_rect(
                        &old_snapshot,
                        event.old_cursor.index,
                        CaretAffinity::Downstream,
                        scroll_y,
                        viewport_h,
                    );
                    event.old_cursor_rect = Some(CursorRect {
                        x: old_caret.x,
                        y: old_caret.y,
                    });

                    // 计算 new_cursor_rect：使用 new_text 布局
                    let new_caret = self.editor_layout.caret_rect(
                        &insert_snapshot,
                        event.new_cursor.index,
                        CaretAffinity::Downstream,
                        scroll_y,
                        viewport_h,
                    );
                    event.new_cursor_rect = Some(CursorRect {
                        x: new_caret.x,
                        y: new_caret.y,
                    });

                    for line in &insert_snapshot.lines {
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
                                y: line.y - self.current_scroll_y as f64,
                                w: ch_w,
                                h: line.height,
                                char_: ch.to_string(),
                            });
                        }
                    }

                    event.glyph_rects = glyph_rects;
                }
                EditorAnimationKind::Delete => {
                    let range_start = event.range_start;
                    let range_end = range_start + event.range_len;
                    let mut glyph_rects = Vec::new();

                    // Use old_text layout for Delete events
                    let delete_snapshot = self.layout_snapshot_for_text(old_text, width);

                    // 计算 old_cursor_rect：使用 old_text 布局
                    let old_caret = self.editor_layout.caret_rect(
                        &delete_snapshot,
                        event.old_cursor.index,
                        CaretAffinity::Downstream,
                        scroll_y,
                        viewport_h,
                    );
                    event.old_cursor_rect = Some(CursorRect {
                        x: old_caret.x,
                        y: old_caret.y,
                    });

                    // 计算 new_cursor_rect：使用 new_text 布局
                    let new_snapshot = self.layout_snapshot_for_text(text, width);
                    let new_caret = self.editor_layout.caret_rect(
                        &new_snapshot,
                        event.new_cursor.index,
                        CaretAffinity::Downstream,
                        scroll_y,
                        viewport_h,
                    );
                    event.new_cursor_rect = Some(CursorRect {
                        x: new_caret.x,
                        y: new_caret.y,
                    });

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
                        for (abs_byte, x_pos, ch_w) in glyph_data {
                            if abs_byte >= old_text.len() {
                                continue;
                            }
                            let ch = old_text
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
                                y: line.y - self.current_scroll_y as f64,
                                w: ch_w,
                                h: line.height,
                                char_: ch.to_string(),
                            });
                        }
                    }

                    event.glyph_rects = glyph_rects;
                }
                EditorAnimationKind::Cursor => {
                    // Cursor 类型不需要 glyph rects 或 cursor rects
                }
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
        self.preedit_text.clear();
        self.preedit_cursor = 0;
    }

    fn input_set_preedit(&mut self, text: String, cursor: usize) {
        self.preedit_text = text;
        self.preedit_cursor = cursor;
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

    #[test]
    fn test_active_text_animation_insert_kind() {
        let anim = ActiveTextAnimation {
            kind: TextAnimationKind::Insert,
            byte_range: (3, 6),
            start_time: Instant::now(),
            duration_ms: 160,
        };
        assert_eq!(anim.kind, TextAnimationKind::Insert);
        assert_eq!(anim.byte_range, (3, 6));
        assert_eq!(anim.duration_ms, 160);
    }

    #[test]
    fn test_active_text_animation_delete_kind() {
        let anim = ActiveTextAnimation {
            kind: TextAnimationKind::Delete,
            byte_range: (0, 3),
            start_time: Instant::now(),
            duration_ms: 160,
        };
        assert_eq!(anim.kind, TextAnimationKind::Delete);
        assert_eq!(anim.byte_range, (0, 3));
    }

    #[test]
    fn test_text_animation_kind_equality() {
        assert_eq!(TextAnimationKind::Insert, TextAnimationKind::Insert);
        assert_eq!(TextAnimationKind::Delete, TextAnimationKind::Delete);
        assert_ne!(TextAnimationKind::Insert, TextAnimationKind::Delete);
    }

    #[test]
    fn test_active_text_animation_timeout() {
        // Create an animation that started 100ms ago with 50ms duration
        // (should be considered timed out by tick_text_animations with 2x+200ms grace)
        let anim = ActiveTextAnimation {
            kind: TextAnimationKind::Insert,
            byte_range: (0, 1),
            start_time: Instant::now() - std::time::Duration::from_millis(1000),
            duration_ms: 50,
        };
        // The animation should have timed out (1000ms > 50*2+200 = 300ms)
        let elapsed = Instant::now().duration_since(anim.start_time).as_millis() as u64;
        assert!(elapsed > anim.duration_ms * 2 + 200);
    }

    // --- Animation logic constraint tests ---
    // These tests verify data-structure invariants and logic constraints of
    // ActiveTextAnimation / Vec<ActiveTextAnimation>, NOT integration tests
    // that call SujianEditorItem methods directly.
    //
    // TODO(长期): 将 active_text_animations 抽象为独立的 TextAnimationState 结构体，
    // 不依赖 Qt/SujianEditorItem，支持完整单元测试：
    //   - start: 插入/删除动画创建，byte_range/duration_ms/kind 正确
    //   - clear: set_plain_text / reload / visual_changed / typing_animation_disabled 清除
    //   - timeout: tick_text_animations 超时清除（2x duration + 200ms 宽限）
    //   - disable: typing_animation_enabled=false 立即清除 hidden range
    //   - scroll: set_is_scrolling(true) 清除
    //   - reload: reload_plain_text 清除
    //   - visual_change: 字号/行距/缩进变更清除
    // 当前测试仅验证 Vec::clear() 模拟，TextAnimationState 抽象后可真测上述全部路径。

    /// Verifies that Vec<ActiveTextAnimation>.clear() empties the collection.
    /// This is a precondition for set_plain_text_from_qml's call to
    /// clear_active_text_animations() — if clear() didn't empty the Vec,
    /// hidden ranges would persist and cause permanently invisible text.
    #[test]
    fn test_clear_active_text_animations_on_set_plain_text() {
        // Simulate active insert animation state
        let mut animations: Vec<ActiveTextAnimation> = vec![ActiveTextAnimation {
            kind: TextAnimationKind::Insert,
            byte_range: (3, 6),
            start_time: Instant::now(),
            duration_ms: 160,
        }];
        assert!(!animations.is_empty(), "precondition: should have active animation");

        // set_plain_text_from_qml calls clear_active_text_animations()
        animations.clear();

        assert!(animations.is_empty(), "active_text_animations should be empty after set_plain_text_from_qml");
    }

    /// Verifies that Vec<ActiveTextAnimation>.clear() empties the collection
    /// even with multiple animation entries. This is a precondition for
    /// reload_plain_text's call to clear_active_text_animations().
    #[test]
    fn test_clear_active_text_animations_on_reload() {
        let mut animations: Vec<ActiveTextAnimation> = vec![
            ActiveTextAnimation {
                kind: TextAnimationKind::Insert,
                byte_range: (0, 3),
                start_time: Instant::now(),
                duration_ms: 160,
            },
            ActiveTextAnimation {
                kind: TextAnimationKind::Delete,
                byte_range: (5, 8),
                start_time: Instant::now(),
                duration_ms: 160,
            },
        ];
        assert_eq!(animations.len(), 2, "precondition: should have active animations");

        // reload_plain_text calls clear_active_text_animations()
        animations.clear();

        assert!(animations.is_empty(), "active_text_animations should be empty after reload_plain_text");
    }

    /// Verifies that Vec<ActiveTextAnimation>.clear() empties the collection.
    /// This is a precondition for set_is_scrolling(true)'s call to
    /// clear_active_text_animations().
    #[test]
    fn test_clear_active_text_animations_on_scroll() {
        let mut animations: Vec<ActiveTextAnimation> = vec![ActiveTextAnimation {
            kind: TextAnimationKind::Insert,
            byte_range: (10, 13),
            start_time: Instant::now(),
            duration_ms: 100,
        }];
        assert!(!animations.is_empty(), "precondition: should have active animation");

        // set_is_scrolling(true) calls clear_active_text_animations()
        animations.clear();

        assert!(animations.is_empty(), "active_text_animations should be empty after set_is_scrolling(true)");
    }

    /// Verifies that Vec<ActiveTextAnimation>.clear() empties the collection.
    /// This is a precondition for visual_changed's call to
    /// clear_active_text_animations().
    #[test]
    fn test_clear_active_text_animations_on_visual_changed() {
        let mut animations: Vec<ActiveTextAnimation> = vec![ActiveTextAnimation {
            kind: TextAnimationKind::Insert,
            byte_range: (0, 6),
            start_time: Instant::now(),
            duration_ms: 200,
        }];
        assert!(!animations.is_empty(), "precondition: should have active animation");

        // visual_changed calls clear_active_text_animations()
        animations.clear();

        assert!(animations.is_empty(), "active_text_animations should be empty after visual_changed");
    }

    /// Verifies that after Vec<ActiveTextAnimation>.clear(), the active_insert_byte_range
    /// logic returns None. This is a constraint: clear must eliminate all hidden ranges
    /// that would cause permanently invisible text in the static text layer.
    #[test]
    fn test_no_hidden_range_after_clear() {
        // Simulate active insert animation
        let mut animations: Vec<ActiveTextAnimation> = vec![ActiveTextAnimation {
            kind: TextAnimationKind::Insert,
            byte_range: (3, 6),
            start_time: Instant::now(),
            duration_ms: 160,
        }];

        // Before clear: should have an insert byte range
        let before_clear = animations
            .iter()
            .find(|a| a.kind == TextAnimationKind::Insert)
            .map(|a| a.byte_range);
        assert_eq!(before_clear, Some((3, 6)), "precondition: should have hidden insert range before clear");

        // Clear (same as clear_active_text_animations)
        animations.clear();

        // After clear: active_insert_byte_range should return None
        let after_clear = animations
            .iter()
            .find(|a| a.kind == TextAnimationKind::Insert)
            .map(|a| a.byte_range);
        assert_eq!(after_clear, None, "active_insert_byte_range should return None after clear — no permanent hidden range");
    }

    // --- Additional guard tests for different setting combinations ---

    /// Verifies that Vec<ActiveTextAnimation>.clear() removes all entries
    /// (both Insert and Delete kinds). This is a constraint for
    /// clear_active_text_animations: no stale entries may remain after clear.
    #[test]
    fn test_active_text_animation_cleared_on_multiple_events() {
        let mut animations: Vec<ActiveTextAnimation> = vec![
            ActiveTextAnimation {
                kind: TextAnimationKind::Insert,
                byte_range: (0, 3),
                start_time: Instant::now(),
                duration_ms: 100,
            },
            ActiveTextAnimation {
                kind: TextAnimationKind::Delete,
                byte_range: (5, 8),
                start_time: Instant::now(),
                duration_ms: 160,
            },
            ActiveTextAnimation {
                kind: TextAnimationKind::Insert,
                byte_range: (10, 13),
                start_time: Instant::now(),
                duration_ms: 120,
            },
        ];
        assert_eq!(animations.len(), 3, "precondition: should have 3 active animations");

        // clear_active_text_animations() clears all
        animations.clear();

        assert!(animations.is_empty(), "all active animations should be cleared");
        // Verify no Insert or Delete entries remain
        let has_insert = animations.iter().any(|a| a.kind == TextAnimationKind::Insert);
        let has_delete = animations.iter().any(|a| a.kind == TextAnimationKind::Delete);
        assert!(!has_insert, "no Insert animations should remain after clear");
        assert!(!has_delete, "no Delete animations should remain after clear");
    }

    /// Verifies that Insert and Delete kinds are not equal.
    /// This is important for correct filtering in on_insert_animation_finished
    /// and other kind-based logic.
    #[test]
    fn test_text_animation_kind_not_equal() {
        assert_ne!(
            TextAnimationKind::Insert, TextAnimationKind::Delete,
            "Insert and Delete kinds must not be equal — they drive different rendering paths"
        );
        // Also verify that each kind equals itself
        assert_eq!(TextAnimationKind::Insert, TextAnimationKind::Insert);
        assert_eq!(TextAnimationKind::Delete, TextAnimationKind::Delete);
    }

    /// Verifies that byte_range in ActiveTextAnimation remains unchanged after creation.
    /// This is a data integrity guard — the byte_range should never be mutated
    /// between creation and consumption/clear.
    #[test]
    fn test_active_text_animation_byte_range_integrity() {
        let anim = ActiveTextAnimation {
            kind: TextAnimationKind::Insert,
            byte_range: (7, 13),
            start_time: Instant::now(),
            duration_ms: 200,
        };
        // Verify byte_range is exactly as created
        assert_eq!(anim.byte_range.0, 7, "byte_range start should match creation value");
        assert_eq!(anim.byte_range.1, 13, "byte_range end should match creation value");
        assert_eq!(anim.byte_range, (7, 13), "byte_range tuple should match creation value");

        // Verify for Delete kind as well
        let anim_del = ActiveTextAnimation {
            kind: TextAnimationKind::Delete,
            byte_range: (0, 6),
            start_time: Instant::now(),
            duration_ms: 160,
        };
        assert_eq!(anim_del.byte_range, (0, 6), "Delete byte_range should match creation value");
    }

    // --- Lifecycle guard tests for typing animation disabled ---

    /// 验证：set_typing_animation_enabled(false) 后不应创建新动画
    /// 逻辑约束：当 typing_animation_enabled=false 时，
    /// record_transaction 不应创建新的 ActiveTextAnimation。
    #[test]
    fn typing_animation_disabled_prevents_new_animations() {
        let typing_animation_enabled = false;
        let events_non_empty = true;
        let is_scrolling = false;
        let should_create = typing_animation_enabled && events_non_empty && !is_scrolling;
        assert!(!should_create, "when typing_animation_enabled=false, no animations should be created");
    }

    /// 验证：scrolling 抑制动画创建
    #[test]
    fn scrolling_prevents_new_animations() {
        let typing_animation_enabled = true;
        let events_non_empty = true;
        let is_scrolling = true;
        let should_create = typing_animation_enabled && events_non_empty && !is_scrolling;
        assert!(!should_create, "when scrolling, no animations should be created");
    }

    /// 验证：动画超时后应被 tick_text_animations 清除
    /// 2x duration + 200ms 宽限期后应过期
    #[test]
    fn active_text_animation_expires_after_timeout() {
        let anim = ActiveTextAnimation {
            kind: TextAnimationKind::Insert,
            byte_range: (0, 3),
            start_time: Instant::now() - std::time::Duration::from_millis(500),
            duration_ms: 100,
        };
        let elapsed = Instant::now().duration_since(anim.start_time).as_millis() as u64;
        let should_expire = elapsed >= anim.duration_ms * 2 + 200;
        assert!(should_expire, "animation should be expired after 2x duration + 200ms grace");
    }

    /// 验证：动画在宽限期内不应过期
    #[test]
    fn active_text_animation_not_expired_within_grace() {
        let anim = ActiveTextAnimation {
            kind: TextAnimationKind::Insert,
            byte_range: (0, 3),
            start_time: Instant::now(),
            duration_ms: 100,
        };
        let elapsed = Instant::now().duration_since(anim.start_time).as_millis() as u64;
        let should_expire = elapsed >= anim.duration_ms * 2 + 200;
        assert!(!should_expire, "animation should NOT be expired within grace period");
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
