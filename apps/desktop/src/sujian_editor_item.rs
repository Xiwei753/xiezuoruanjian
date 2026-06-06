// =============================================================================
// sujian_editor_item.rs — Desktop self-rendered editor item
// =============================================================================

use qmetaobject::prelude::*;
use qmetaobject::{QBrush, QColor, QMouseEvent, QPainter, QPainterRenderHint, QPen, QPointF, QQuickItem, QQuickPaintedItem, QRectF, QString};
use writer_core::editor::{EditorCursor, EditorEngine, EditorSelection, EditorTransactionCause};

const KEY_BACKSPACE: i32 = 0x0100_0003;
const KEY_TAB: i32 = 0x0100_0001;
const KEY_ENTER: i32 = 0x0100_0005;
const KEY_RETURN: i32 = 0x0100_0004;
const KEY_DELETE: i32 = 0x0100_0007;
const KEY_LEFT: i32 = 0x0100_0012;
const KEY_UP: i32 = 0x0100_0013;
const KEY_RIGHT: i32 = 0x0100_0014;
const KEY_DOWN: i32 = 0x0100_0015;
const KEY_HOME: i32 = 0x0100_0010;
const KEY_END: i32 = 0x0100_0011;
const KEY_A: i32 = 0x41;
const KEY_X: i32 = 0x58;
const KEY_Y: i32 = 0x59;
const KEY_Z: i32 = 0x5a;
const CTRL_MODIFIER: i32 = 0x0400_0000;
const SHIFT_MODIFIER: i32 = 0x0200_0000;

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
        self.cursor = self.text.len();
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

    plain_text_changed: qt_signal!(),
    text_changed: qt_signal!(),
    content_height_changed: qt_signal!(),
    cursor_position_changed: qt_signal!(),
    selection_changed: qt_signal!(),
    editor_enabled_changed: qt_signal!(),
    visual_settings_changed: qt_signal!(),
    transaction_created: qt_signal!(),

    get_plain_text: qt_method!(fn(&self) -> QString),
    set_plain_text: qt_method!(fn(&mut self, text: QString)),
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

    buffer: EditorBuffer,
    engine: EditorEngine,
    current_content_height: f32,
    current_editor_enabled: bool,
    current_font_pixel_size: f32,
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
    last_summary: QString,
    last_event_count: u32,
    last_cursor_x: f64,
    last_cursor_y: f64,
    animated_cursor_x: f64,
    animated_cursor_y: f64,
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
            plain_text_changed: Default::default(),
            text_changed: Default::default(),
            content_height_changed: Default::default(),
            cursor_position_changed: Default::default(),
            selection_changed: Default::default(),
            editor_enabled_changed: Default::default(),
            visual_settings_changed: Default::default(),
            transaction_created: Default::default(),
            get_plain_text: Default::default(),
            set_plain_text: Default::default(),
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
            buffer: EditorBuffer::default(),
            engine: EditorEngine::new(),
            current_content_height: 0.0,
            current_editor_enabled: true,
            current_font_pixel_size: 16.0,
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
            last_summary: "".into(),
            last_event_count: 0,
            last_cursor_x: 0.0,
            last_cursor_y: 0.0,
            animated_cursor_x: 0.0,
            animated_cursor_y: 0.0,
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
        let old = self.buffer.snapshot();
        self.buffer.set_text(normalized);
        let new = self.buffer.snapshot();
        self.record_transaction(old, new, EditorTransactionCause::Load, false);
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
        // Kept as a future event-driven hook; text reveal animation stays off by default.
        self.current_typing_animation_enabled = value;
        self.visual_settings_changed();
    }

    fn last_transaction_summary(&self) -> QString {
        self.last_summary.clone()
    }

    fn last_animation_event_count(&self) -> u32 {
        self.last_event_count
    }

    fn visual_changed(&mut self) {
        self.recalculate_content_height();
        self.visual_settings_changed();
        self.request_repaint();
    }

    fn emit_content_changed(&mut self) {
        self.recalculate_content_height();
        self.plain_text_changed();
        self.text_changed();
        self.cursor_position_changed();
        self.selection_changed();
        self.request_repaint();
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
        let ctrl = modifiers & CTRL_MODIFIER != 0;
        let shift = modifiers & SHIFT_MODIFIER != 0;
        if ctrl {
            match key {
                KEY_A => {
                    self.select_all();
                    return true;
                }
                KEY_X => {
                    self.delete_selection();
                    return true;
                }
                KEY_Z => {
                    self.undo();
                    return true;
                }
                KEY_Y => {
                    self.redo();
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
        let lines = self.layout_lines(self.bounding_width());
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
        let lines = self.layout_lines(self.bounding_width());
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

    fn recalculate_content_height(&mut self) {
        let lines = self.layout_lines(self.bounding_width());
        let height = lines
            .last()
            .map(|line| line.y + line.height + self.current_padding as f64)
            .unwrap_or((self.current_font_pixel_size * self.current_line_spacing + self.current_padding * 2.0) as f64);
        let next = height.max(1.0) as f32;
        if (self.current_content_height - next).abs() > 0.5 {
            self.current_content_height = next;
            self.content_height_changed();
        }
    }

    fn layout_lines(&self, width: f64) -> Vec<VisualLine> {
        layout_lines(
            &self.buffer.text,
            width,
            self.current_font_pixel_size as f64,
            self.current_line_spacing as f64,
            self.current_padding as f64,
            self.current_text_indent as f64,
        )
    }

    fn hit_test(&self, x: f64, y: f64) -> usize {
        let lines = self.layout_lines(self.bounding_width());
        if lines.is_empty() {
            return 0;
        }
        let line = lines
            .iter()
            .find(|line| y <= line.y + line.height)
            .unwrap_or_else(|| lines.last().unwrap());
        self.index_at_line_x(line, x)
    }

    fn index_at_line_x(&self, line: &VisualLine, x: f64) -> usize {
        let char_width = average_char_width(self.current_font_pixel_size as f64);
        let relative = (x - line.x).max(0.0);
        let mut col = (relative / char_width).round() as usize;
        let max_col = self.buffer.text[line.start..line.end].chars().count();
        col = col.min(max_col);
        byte_index_at_char_offset(&self.buffer.text, line.start, col)
    }

    fn cursor_line_and_x(&self, lines: &[VisualLine]) -> Option<(usize, f64)> {
        if lines.is_empty() {
            return None;
        }
        for (idx, line) in lines.iter().enumerate() {
            if self.buffer.cursor >= line.start && self.buffer.cursor <= line.end {
                let col = self.buffer.text[line.start..self.buffer.cursor].chars().count();
                return Some((idx, line.x + col as f64 * average_char_width(self.current_font_pixel_size as f64)));
            }
        }
        lines.last().map(|line| (lines.len() - 1, line.x + line.width))
    }
}

impl QQuickItem for SujianEditorItem {
    fn geometry_changed(&mut self, _new_geometry: QRectF, _old_geometry: QRectF) {
        self.recalculate_content_height();
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
        self.recalculate_content_height();
        let width = self.bounding_width();
        let item = self as &dyn QQuickItem;
        let height = item.bounding_rect().height.max(self.current_content_height as f64);
        painter.set_render_hint(QPainterRenderHint::TextAntialiasing, true);
        painter.fill_rect(
            QRectF { x: 0.0, y: 0.0, width, height },
            QBrush::from_color(QColor::from_rgba(0, 0, 0, 0)),
        );

        let lines = self.layout_lines(width);
        let selection = self.buffer.selection_range();
        for line in &lines {
            if self.buffer.has_selection() && selection.1 > line.start && selection.0 < line.end {
                let sel_start = selection.0.max(line.start);
                let sel_end = selection.1.min(line.end);
                let start_col = self.buffer.text[line.start..sel_start].chars().count() as f64;
                let end_col = self.buffer.text[line.start..sel_end].chars().count() as f64;
                let char_width = average_char_width(self.current_font_pixel_size as f64);
                draw_rect(
                    painter,
                    line.x + start_col * char_width,
                    line.y,
                    (end_col - start_col).max(0.5) * char_width,
                    line.height,
                    self.current_selection_color.clone(),
                );
            }
            let text = self.buffer.text[line.start..line.end].to_string();
            draw_text(
                painter,
                line.x,
                line.y + self.current_font_pixel_size as f64,
                self.current_font_pixel_size,
                self.current_text_color.clone(),
                text.into(),
            );
        }

        let (cursor_x, cursor_y) = cursor_geometry(&self.buffer.text, &lines, self.buffer.cursor, self.current_font_pixel_size as f64);
        let cursor_h = (self.current_font_pixel_size as f64 * self.current_line_spacing as f64).max(16.0);
        let same_line = (cursor_y - self.last_cursor_y).abs() < 2.0;
        let small_move = (cursor_x - self.last_cursor_x).abs() <= 160.0;
        if self.current_smooth_cursor_enabled && same_line && small_move {
            self.animated_cursor_x = self.animated_cursor_x * 0.45 + cursor_x * 0.55;
            self.animated_cursor_y = cursor_y;
        } else {
            self.animated_cursor_x = cursor_x;
            self.animated_cursor_y = cursor_y;
        }
        self.last_cursor_x = cursor_x;
        self.last_cursor_y = cursor_y;
        if self.current_editor_enabled && !self.buffer.has_selection() {
            draw_rect(
                painter,
                self.animated_cursor_x,
                self.animated_cursor_y,
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

fn average_char_width(font_size: f64) -> f64 {
    (font_size * 0.58).max(6.0)
}

fn layout_lines(text: &str, width: f64, font_size: f64, line_spacing: f64, padding: f64, indent: f64) -> Vec<VisualLine> {
    let char_width = average_char_width(font_size);
    let line_height = (font_size * line_spacing).max(font_size + 4.0);
    let available = (width - padding * 2.0).max(char_width);
    let mut result = Vec::new();
    let mut y = padding;
    let mut paragraph_start = 0;

    for paragraph in text.split_inclusive('\n') {
        let hard_break = paragraph.ends_with('\n');
        let paragraph_text_end = paragraph_start + paragraph.trim_end_matches('\n').len();
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
            let max_chars = ((available - if first_line { indent } else { 0.0 }).max(char_width) / char_width).floor().max(1.0) as usize;
            let line_end = byte_index_at_char_offset(text, line_start, max_chars).min(paragraph_text_end);
            let end = if line_end == line_start {
                next_char_boundary(text, line_start).unwrap_or(paragraph_text_end)
            } else {
                line_end
            };
            let count = text[line_start..end].chars().count() as f64;
            result.push(VisualLine {
                start: line_start,
                end,
                hard_break: hard_break && end == paragraph_text_end,
                x,
                y,
                width: count * char_width,
                height: line_height,
            });
            y += line_height;
            first_line = false;
            line_start = end;
        }
        paragraph_start += paragraph.len();
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

fn cursor_geometry(text: &str, lines: &[VisualLine], cursor: usize, font_size: f64) -> (f64, f64) {
    let char_width = average_char_width(font_size);
    for line in lines {
        if cursor >= line.start && cursor <= line.end {
            let col = text[line.start..cursor].chars().count() as f64;
            return (line.x + col * char_width, line.y);
        }
    }
    lines
        .last()
        .map(|line| (line.x + line.width, line.y))
        .unwrap_or((0.0, 0.0))
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
    fn editor_layout_wraps_and_maps_cursor_on_char_boundaries() {
        let lines = layout_lines("你好hello", 30.0, 16.0, 1.5, 0.0, 0.0);
        assert!(lines.len() >= 2);
        for line in lines {
            assert!("你好hello".is_char_boundary(line.start));
            assert!("你好hello".is_char_boundary(line.end));
        }
    }
}
