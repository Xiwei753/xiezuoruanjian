// =============================================================================
// sujian_editor_item.rs — Desktop self-rendered editor item
// =============================================================================

use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::{QBrush, QColor, QLineF, QMouseEvent, QPainter, QPainterRenderHint, QPen, QPointF, QQuickItem, QQuickPaintedItem, QRectF, QString};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Instant;
use writer_core::editor::{EditorCursor, EditorEngine, EditorSelection, EditorTransactionCause};

cpp! {{
    #include <QtGui/QFont>
    #include <QtGui/QFontMetricsF>
    #include <QtGui/QPainter>
    #include <QtGui/QClipboard>
    #include <QGuiApplication>
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
const KEY_A: i32 = 0x41;
const KEY_C: i32 = 0x43;
const KEY_V: i32 = 0x56;
const KEY_X: i32 = 0x58;
const KEY_Y: i32 = 0x59;
const KEY_Z: i32 = 0x5a;
const CTRL_MODIFIER: i32 = 0x0400_0000;
const SHIFT_MODIFIER: i32 = 0x0200_0000;

fn has_ctrl(modifiers: i32) -> bool {
    modifiers & CTRL_MODIFIER != 0
}

fn has_shift(modifiers: i32) -> bool {
    modifiers & SHIFT_MODIFIER != 0
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
    start: usize,
    end: usize,
    hard_break: bool,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
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
    base: qt_base_class!(trait QQuickPaintedItem),

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
    current_is_scrolling: bool,
    last_summary: QString,
    last_event_count: u32,
    target_cursor_x: f64,
    target_cursor_y: f64,
    cursor_animation: Option<CursorAnimationState>,
    preedit_text: String,
    preedit_cursor: usize,
    layout_cache: Option<LayoutCache>,
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
            current_is_scrolling: false,
            last_summary: "".into(),
            last_event_count: 0,
            target_cursor_x: 0.0,
            target_cursor_y: 0.0,
            cursor_animation: None,
            preedit_text: String::new(),
            preedit_cursor: 0,
            layout_cache: None,
        }
    }
}

impl SujianEditorItem {
    fn request_repaint(&self) {
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
        self.current_typing_animation_enabled
    }

    fn set_typing_animation_enabled(&mut self, value: bool) {
        self.current_typing_animation_enabled = value;
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

    fn is_scrolling(&self) -> bool {
        self.current_is_scrolling
    }

    fn set_is_scrolling(&mut self, value: bool) {
        if self.current_is_scrolling == value {
            return;
        }
        self.current_is_scrolling = value;
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
        self.target_cursor_y as f32
    }

    fn cursor_rect_width(&self) -> f32 {
        2.0
    }

    fn cursor_rect_height(&self) -> f32 {
        cursor_height_for_line(self.current_font_pixel_size as f64, &self.current_font_family.to_string()) as f32
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
        let old = self.buffer.snapshot();
        self.buffer.push_undo(old.clone());
        self.buffer.replace_selection_or_insert(&inserted);
        let cause = if inserted.chars().count() == 1 {
            EditorTransactionCause::Typing
        } else {
            EditorTransactionCause::ImeComposition
        };
        let new = self.buffer.snapshot();
        self.record_transaction(old, new, cause, true);
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
        let new = self.buffer.snapshot();
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
        let new = self.buffer.snapshot();
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
        let new = self.buffer.snapshot();
        self.record_transaction(old, new, EditorTransactionCause::Delete, true);
        self.emit_content_changed();
    }

    fn select_all(&mut self) {
        self.buffer.select_all();
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
        self.record_transaction(old, new, EditorTransactionCause::Undo, true);
        self.emit_content_changed();
    }

    fn redo(&mut self) {
        let Some((old, new)) = self.buffer.redo() else {
            return;
        };
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
        let index = self.hit_test(x as f64, y as f64);
        self.buffer.move_cursor(index, extend);
        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.cursor_position_changed();
        self.selection_changed();
        self.request_repaint();
    }

    fn drag_select_at(&mut self, x: f32, y: f32) {
        let index = self.hit_test(x as f64, y as f64);
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
        self.buffer.move_cursor(if end { line.end } else { line.start }, extend);
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
        clear_text_width_cache();
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

    fn hit_test(&mut self, x: f64, y: f64) -> usize {
        let width = self.bounding_width();
        let lines = self.ensure_layout_cached(width).clone();
        if lines.is_empty() {
            return 0;
        }
        let line = lines
            .iter()
            .find(|line| y < line.y + line.height)
            .unwrap_or_else(|| lines.last().unwrap());
        self.index_at_line_x(line, x)
    }

    fn index_at_line_x(&self, line: &VisualLine, x: f64) -> usize {
        let segment = &self.buffer.text[line.start..line.end];
        let font_size = self.current_font_pixel_size as f64;
        let font_family = self.current_font_family.to_string();
        let relative = (x - line.x).max(0.0);
        let chars: Vec<char> = segment.chars().collect();
        let mut best_col = 0usize;
        let mut best_dist = f64::MAX;
        for i in 0..=chars.len() {
            let prefix: String = chars[..i].iter().collect();
            let w = measure_text_width(&prefix, font_size, &font_family);
            let dist = (w - relative).abs();
            if dist < best_dist {
                best_dist = dist;
                best_col = i;
            } else {
                break;
            }
        }
        byte_index_at_char_offset_in_range(&self.buffer.text, line.start, line.end, best_col)
    }

    fn cursor_line_and_x(&self, lines: &[VisualLine]) -> Option<(usize, f64)> {
        if lines.is_empty() {
            return None;
        }
        let font_size = self.current_font_pixel_size as f64;
        let font_family = self.current_font_family.to_string();
        for (idx, line) in lines.iter().enumerate() {
            if line_contains_cursor(lines, idx, self.buffer.cursor) {
                let segment = &self.buffer.text[line.start..self.buffer.cursor];
                let w = measure_text_width(segment, font_size, &font_family);
                return Some((idx, line.x + w));
            }
        }
        lines.last().map(|line| (lines.len() - 1, line.x + line.width))
    }
}

impl QQuickItem for SujianEditorItem {
    fn geometry_changed(&mut self, _new_geometry: QRectF, _old_geometry: QRectF) {
        self.recalculate_content_height_quiet();
        self.request_repaint();
    }

    fn mouse_event(&mut self, event: QMouseEvent) -> bool {
        let pos = event.position();
        match event.event_type() {
            qmetaobject::QMouseEventType::MouseButtonPress => self.click_at(pos.x as f32, pos.y as f32, false),
            qmetaobject::QMouseEventType::MouseMove => self.drag_select_at(pos.x as f32, pos.y as f32),
            qmetaobject::QMouseEventType::MouseButtonRelease => {}
            _ => {}
        }
        true
    }
}

impl QQuickPaintedItem for SujianEditorItem {
    fn paint(&mut self, painter: &mut QPainter) {
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

        let item = self as &dyn QQuickItem;
        let viewport_height = item.bounding_rect().height.max(content_h as f64);
        painter.set_render_hint(QPainterRenderHint::TextAntialiasing, true);
        painter.fill_rect(
            QRectF { x: 0.0, y: 0.0, width, height: viewport_height },
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

        let scroll_y = self.current_scroll_y as f64;
        // 借用 layout_cache 中的 lines，不 clone
        let lines = &self.layout_cache.as_ref().unwrap().lines;
        let vis_start = lines.partition_point(|l| l.y + l.height < scroll_y);
        let vis_end = lines.len().min(lines.partition_point(|l| l.y < scroll_y + viewport_height) + 1);

        let selection = self.buffer.selection_range();
        for line in &lines[vis_start..vis_end] {
            if self.buffer.has_selection() && selection.1 > line.start && selection.0 < line.end {
                let sel_start = selection.0.max(line.start);
                let sel_end = selection.1.min(line.end);
                let prefix_start = &self.buffer.text[line.start..sel_start];
                let prefix_end = &self.buffer.text[line.start..sel_end];
                let x_start = line.x + measure_text_width(prefix_start, font_size, &font_family);
                let x_end = line.x + measure_text_width(prefix_end, font_size, &font_family);
                draw_rect(
                    painter,
                    x_start,
                    line.y,
                    (x_end - x_start).max(2.0),
                    line.height,
                    self.current_selection_color.clone(),
                );
            }
            let text = self.buffer.text[line.start..line.end].to_string();
            draw_text(
                painter,
                line.x,
                text_baseline_y(line, font_size, &font_family),
                fs,
                self.current_text_color.clone(),
                text.into(),
            );
        }

        if !self.preedit_text.is_empty() {
            let pc = self.buffer.cursor;
            for line in &lines[vis_start..vis_end] {
                if pc >= line.start && pc <= line.end {
                    let prefix = &self.buffer.text[line.start..pc];
                    let x = line.x + measure_text_width(prefix, font_size, &font_family);
                    draw_text(
                        painter,
                        x,
                        text_baseline_y(line, font_size, &font_family),
                        fs,
                        self.current_text_color.clone(),
                        self.preedit_text.clone().into(),
                    );
                    let preedit_w = measure_text_width(&self.preedit_text, font_size, &font_family);
                    painter.set_pen(QPen::from_color(color_from_qstring(self.current_text_color.clone())));
                    let underline_y = text_baseline_y(line, font_size, &font_family) + 2.0;
                    let line_f = QLineF { pt1: QPointF { x, y: underline_y }, pt2: QPointF { x: x + preedit_w, y: underline_y } };
                    painter.draw_line(line_f);
                    break;
                }
            }
        }

        let (cursor_x, cursor_y) = cursor_geometry_with_font(&self.buffer.text, &lines, self.buffer.cursor, font_size, &font_family);
        let cursor_h = cursor_height_for_line(font_size, &font_family);

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

        // 光标动画：固定时长 tween，不再用指数追赶
        let now = Instant::now();
        let is_selecting = self.buffer.selection_anchor != self.buffer.cursor;
        let is_preediting = !self.preedit_text.is_empty();

        // 判断是否应该 snap（直接到位）
        let should_snap = self.current_is_scrolling 
            || is_selecting 
            || is_preediting;

        // 获取当前光标视觉位置
        let (visual_x, visual_y) = if let Some(ref anim) = self.cursor_animation {
            if anim.is_finished(now) {
                (anim.target_x, anim.target_y)
            } else {
                anim.current_position(now)
            }
        } else {
            (self.target_cursor_x, self.target_cursor_y)
        };

        // 判断是否跨行或远距离
        let same_line = (self.target_cursor_y - visual_y).abs() < 2.0;
        let dist = ((self.target_cursor_x - visual_x).powi(2) + (self.target_cursor_y - visual_y).powi(2)).sqrt();
        let small_move = dist < font_size * 3.0;

        // 决定最终光标位置
        let (final_x, final_y, new_animation) = if should_snap || !same_line || !small_move || !self.current_smooth_cursor_enabled {
            // snap: 直接到位
            (self.target_cursor_x, self.target_cursor_y, None)
        } else if let Some(ref anim) = self.cursor_animation {
            // 动画中，目标点变了 → 从当前视觉位置继续动画到新目标
            if (anim.target_x - self.target_cursor_x).abs() > 0.01 || (anim.target_y - self.target_cursor_y).abs() > 0.01 {
                // 目标变了，创建新动画，从当前视觉位置开始
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
                // 动画完成
                (anim.target_x, anim.target_y, None)
            } else {
                // 动画进行中
                let (cur_x, cur_y) = anim.current_position(now);
                (cur_x, cur_y, Some(anim.clone()))
            }
        } else {
            // 没有动画，目标点变了 → 创建新动画
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
                // 已经到位
                (self.target_cursor_x, self.target_cursor_y, None)
            }
        };

        // 更新动画状态
        self.cursor_animation = new_animation;

        // 如果动画还在进行中，请求重绘
        if let Some(ref anim) = self.cursor_animation {
            if !anim.is_finished(now) {
                self.request_repaint();
            }
        }

        if self.current_editor_enabled && !self.buffer.has_selection() {
            draw_rect(
                painter,
                final_x,
                final_y,
                2.0,
                cursor_h,
                self.current_cursor_color.clone(),
            );
        }
    }
}

fn normalize_plain_text(text: &str) -> String {
    text.replace('\u{2029}', "\n").replace("\r\n", "\n").replace('\r', "\n")
}

thread_local! {
    static TEXT_WIDTH_CACHE: RefCell<HashMap<(String, u32, String), f64>> = RefCell::new(HashMap::new());
}

fn clear_text_width_cache() {
    TEXT_WIDTH_CACHE.with(|cache| cache.borrow_mut().clear());
}

#[cfg(not(test))]
fn measure_text_width(text: &str, font_size: f64, font_family: &str) -> f64 {
    if text.is_empty() {
        return 0.0;
    }
    let fs_key = (font_size * 100.0).round() as u32;
    let key = (text.to_string(), fs_key, font_family.to_string());
    if let Some(cached) = TEXT_WIDTH_CACHE.with(|c| c.borrow().get(&key).copied()) {
        return cached;
    }
    let qtext: QString = text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    let result = cpp!(unsafe [qtext as "QString", fs as "float", ff as "QString"] -> f64 as "double" {
        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QFontMetricsF fm(font);
        return fm.horizontalAdvance(qtext);
    });
    TEXT_WIDTH_CACHE.with(|c| c.borrow_mut().insert(key, result));
    result
}

#[cfg(test)]
fn measure_text_width(text: &str, font_size: f64, _font_family: &str) -> f64 {
    text.chars().count() as f64 * (font_size * 0.6)
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
        let mut line_start = paragraph_start;
        if line_start == paragraph_text_end {
            result.push(VisualLine {
                start: line_start,
                end: line_start,
                hard_break,
                x: padding + indent,
                y,
                width: 0.0,
                height: line_height,
            });
            y += line_height;
            paragraph_start += paragraph.len();
            continue;
        }

        let mut first_line = true;
        while line_start < paragraph_text_end {
            let x = padding + if first_line { indent } else { 0.0 };
            let line_available = available - if first_line { indent } else { 0.0 };
            let segment = &text[line_start..paragraph_text_end];
            let chars: Vec<char> = segment.chars().collect();
            let mut end_col = 0usize;
            for i in 1..=chars.len() {
                let prefix: String = chars[..i].iter().collect();
                let w = measure_text_width(&prefix, font_size, font_family);
                if w > line_available {
                    break;
                }
                end_col = i;
            }
            if end_col == 0 {
                end_col = 1;
            }
            let line_end = byte_index_at_char_offset(text, line_start, end_col).min(paragraph_text_end);
            let line_text = &text[line_start..line_end];
            let line_width = measure_text_width(line_text, font_size, font_family);
            result.push(VisualLine {
                start: line_start,
                end: line_end,
                hard_break: hard_break && line_end == paragraph_text_end,
                x,
                y,
                width: line_width,
                height: line_height,
            });
            y += line_height;
            first_line = false;
            line_start = line_end;
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
        });
    }
    result
}

fn cursor_geometry_with_font(text: &str, lines: &[VisualLine], cursor: usize, font_size: f64, font_family: &str) -> (f64, f64) {
    for (idx, line) in lines.iter().enumerate() {
        if line_contains_cursor(lines, idx, cursor) {
            let segment = &text[line.start..cursor];
            let w = measure_text_width(segment, font_size, font_family);
            return (line.x + w, cursor_top_y(line, font_size, font_family));
        }
    }
    lines
        .last()
        .map(|line| (line.x + line.width, cursor_top_y(line, font_size, font_family)))
        .unwrap_or((0.0, 0.0))
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

fn cursor_top_y(line: &VisualLine, font_size: f64, font_family: &str) -> f64 {
    let ascent = get_font_ascent(font_family, font_size as f32);
    text_baseline_y(line, font_size, font_family) - (ascent as f64)
}

fn text_baseline_y(line: &VisualLine, font_size: f64, font_family: &str) -> f64 {
    let ascent = get_font_ascent(font_family, font_size as f32);
    let descent = get_font_descent(font_family, font_size as f32);
    let top_padding = (line.height - (ascent + descent)).max(0.0) / 2.0;
    line.y + top_padding + ascent
}

fn line_contains_cursor(lines: &[VisualLine], idx: usize, cursor: usize) -> bool {
    let line = &lines[idx];
    if cursor == line.start {
        return true;
    }
    if cursor > line.start && cursor < line.end {
        return true;
    }
    cursor == line.end && lines.get(idx + 1).is_none_or(|next| next.start != cursor)
}

fn draw_text(painter: &mut QPainter, x: f64, baseline_y: f64, font_size: f32, color: QString, text: QString) {
    let _ = font_size;
    painter.set_pen(QPen::from_color(color_from_qstring(color)));
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

fn byte_index_at_char_offset(text: &str, start: usize, char_offset: usize) -> usize {
    for (offset, (byte, _)) in text[start..].char_indices().enumerate() {
        if offset == char_offset {
            return start + byte;
        }
    }
    if char_offset == 0 {
        start
    } else {
        text.len()
    }
}

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

        let (_x, y) = cursor_geometry_with_font(text, &lines, text.len(), 16.0, "serif");
        assert_eq!(y, cursor_top_y(&lines[1], 16.0, "serif"));
    }

    #[test]
    fn cursor_geometry_prefers_next_visual_line_at_boundary() {
        let text = "ab";
        let lines = vec![
            VisualLine { start: 0, end: 1, hard_break: false, x: 16.0, y: 10.0, width: 10.0, height: 24.0 },
            VisualLine { start: 1, end: 2, hard_break: false, x: 16.0, y: 34.0, width: 10.0, height: 24.0 },
        ];

        let (_x, y) = cursor_geometry_with_font(text, &lines, 1, 16.0, "serif");

        assert_eq!(y, cursor_top_y(&lines[1], 16.0, "serif"));
    }

    #[test]
    fn cursor_and_text_are_vertically_centered_in_spaced_line() {
        let line = VisualLine { start: 0, end: 0, hard_break: false, x: 16.0, y: 10.0, width: 0.0, height: 32.0 };

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
}
