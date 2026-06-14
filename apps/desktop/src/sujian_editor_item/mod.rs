// =============================================================================
// sujian_editor_item — Desktop self-rendered editor item
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
use buffer::{clamp_to_char_boundary, next_char_boundary, prev_char_boundary, byte_to_char_index, EditorBuffer, EditorSnapshot, normalize_plain_text};
use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::{
    QMouseEvent, QQuickItem, QRectF, QString,
};
use rendering::{InsertAnimation, DeleteAnimation, ScrollBuffer};
pub use rendering::AnimatedGlyph;
use std::cell::Cell;
use std::time::Instant;
use writer_core::editor::{EditorCursor, EditorEngine, EditorSelection, EditorTransactionCause, EditorAnimationEvent, EditorAnimationKind};

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
    scroll_y: qt_property!(f32; READ scroll_y WRITE set_scroll_y NOTIFY visual_settings_changed),
    viewport_height: qt_property!(f32; READ viewport_height WRITE set_viewport_height NOTIFY visual_settings_changed),
    is_scrolling: qt_property!(bool; READ is_scrolling WRITE set_is_scrolling NOTIFY visual_settings_changed),
    cursor_rect_x: qt_property!(f32; READ cursor_rect_x NOTIFY cursor_rect_changed),
    cursor_rect_y: qt_property!(f32; READ cursor_rect_y NOTIFY cursor_rect_changed),
    cursor_rect_width: qt_property!(f32; READ cursor_rect_width NOTIFY cursor_rect_changed),
    cursor_rect_height: qt_property!(f32; READ cursor_rect_height NOTIFY cursor_rect_changed),
    cursor_visible: qt_property!(bool; READ cursor_visible NOTIFY cursor_rect_changed),

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
    current_typing_animation_duration_ms: u32,
    current_scroll_y: f32,
    current_viewport_height: f32,
    current_is_scrolling: bool,
    last_summary: QString,
    last_event_count: u32,
    insert_animation: Option<InsertAnimation>,
    delete_animation: Option<DeleteAnimation>,
    preedit_text: String,
    preedit_cursor: usize,
    suppress_next_ime_commit: bool,
    editor_layout: EditorLayout,
    text_revision: u64,
    render_dirty: bool,
    scroll_buffer: Option<ScrollBuffer>,
    last_slow_paint_log: Option<Instant>,
    /// Isolated cursor visual state — target position, visual position,
    /// animation, IME pending flag.  All cursor-related visual logic
    /// lives in CursorController; SujianEditorItem only dispatches.
    cursor_ctrl: cursor_controller::CursorController,
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
            cursor_visible: Default::default(),
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
            current_typing_animation_duration_ms: 160,
            current_scroll_y: 0.0,
            current_viewport_height: 0.0,
            current_is_scrolling: false,
            last_summary: "".into(),
            last_event_count: 0,
            insert_animation: None,
            delete_animation: None,
            preedit_text: String::new(),
            preedit_cursor: 0,
            suppress_next_ime_commit: false,
            editor_layout: EditorLayout::default(),
            text_revision: 0,
            render_dirty: true,
            scroll_buffer: None,
            last_slow_paint_log: None,
            cursor_ctrl: cursor_controller::CursorController::new(),
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
        let clamped = value.max(30).min(500);
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
        self.cursor_ctrl.force_snap_next = true;
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
            self.cursor_ctrl.animation = None;
            self.insert_animation = None;
            self.delete_animation = None;
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

    fn cursor_rect_x(&self) -> f32 {
        self.cursor_ctrl.target_x as f32
    }

    fn cursor_rect_y(&self) -> f32 {
        // Viewport coordinates: cursor_ctrl.target_y is already viewport-relative
        self.cursor_ctrl.target_y as f32
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

    fn visual_changed(&mut self) {
        self.invalidate_layout_cache();
        self.recalculate_content_height_quiet();
        self.visual_settings_changed();
        self.request_static_repaint();
    }

    fn emit_content_changed(&mut self) {
        self.text_revision = self.text_revision.wrapping_add(1);
        self.invalidate_layout_cache();
        self.recalculate_content_height_quiet();
        self.plain_text_changed();
        self.text_changed();
        self.cursor_position_changed();
        self.selection_changed();
        self.request_static_repaint();
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
        let origin_cx = self.cursor_ctrl.target_x;
        let origin_cy = self.cursor_ctrl.target_y;
        let cursor_h = self.editor_layout.cursor_height(
            self.current_font_pixel_size as f64,
            &self.current_font_family.to_string(),
        );

        let old = self.buffer.snapshot();
        self.buffer.push_undo(old.clone());
        self.buffer.replace_selection_or_insert(&inserted);
        self.adjust_affinity_at_wrap_boundary();
        let cause = if inserted.chars().count() == 1 {
            EditorTransactionCause::Typing
        } else {
            EditorTransactionCause::ImeComposition
        };
        let new = self.buffer.snapshot();
        let events = self.record_transaction(old, new, cause, true);

        self.delete_animation = None;
        self.insert_animation = None;

        if self.current_typing_animation_enabled && !self.current_is_scrolling {
            for event in &events {
                if event.kind == EditorAnimationKind::Insert {
                    let anim = self.create_insert_animation(event, origin_cx, origin_cy, cursor_h);
                    self.insert_animation = Some(anim);
                    break;
                }
            }
        }

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
        let old_text = old.text.clone();
        self.buffer.push_undo(old.clone());
        self.adjust_affinity_at_wrap_boundary();
        let new = self.buffer.snapshot();
        let new_text = new.text.clone();

        self.insert_animation = None;
        self.delete_animation = None;

        let events = self.record_transaction(old, new, EditorTransactionCause::Delete, true);

        // 动画创建已禁用 — update_cursor_visual_position 在 GUI 线程入口点调用
        // if self.current_typing_animation_enabled && !self.current_is_scrolling {
        //     for event in &events {
        //         if event.kind == EditorAnimationKind::Delete {
        //             let anim = self.create_delete_animation(event, &old_text, &new_text);
        //             self.delete_animation = Some(anim);
        //             break;
        //         }
        //     }
        // }

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
        let old_text = old.text.clone();
        self.buffer.push_undo(old.clone());
        self.adjust_affinity_at_wrap_boundary();
        let new = self.buffer.snapshot();
        let new_text = new.text.clone();

        self.insert_animation = None;
        self.delete_animation = None;

        let events = self.record_transaction(old, new, EditorTransactionCause::Delete, true);

        // 动画创建已禁用 — update_cursor_visual_position 在 GUI 线程入口点调用
        // if self.current_typing_animation_enabled && !self.current_is_scrolling {
        //     for event in &events {
        //         if event.kind == EditorAnimationKind::Delete {
        //             let anim = self.create_delete_animation(event, &old_text, &new_text);
        //             self.delete_animation = Some(anim);
        //             break;
        //         }
        //     }
        // }

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
        let old_text = old.text.clone();
        self.buffer.push_undo(old.clone());
        self.adjust_affinity_at_wrap_boundary();
        let new = self.buffer.snapshot();
        let new_text = new.text.clone();

        self.insert_animation = None;
        self.delete_animation = None;

        let events = self.record_transaction(old, new, EditorTransactionCause::Delete, true);

        // 动画创建已禁用 — update_cursor_visual_position 在 GUI 线程入口点调用
        // if self.current_typing_animation_enabled && !self.current_is_scrolling {
        //     for event in &events {
        //         if event.kind == EditorAnimationKind::Delete {
        //             let anim = self.create_delete_animation(event, &old_text, &new_text);
        //             self.delete_animation = Some(anim);
        //             break;
        //         }
        //     }
        // }

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
        if extend {
            self.cursor_ctrl.force_snap_next = true;
        }
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
        input::insert_preedit_text(self, text.to_string());
    }

    fn commit_preedit(&mut self, text: QString) {
        input::commit_preedit_text(self, text.to_string());
    }

    fn cancel_preedit(&mut self) {
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
            (line.end, CaretAffinity::Upstream)
        } else {
            (line.start, CaretAffinity::Downstream)
        };
        self.cursor_ctrl.affinity = affinity;
        self.buffer.move_cursor(index, extend);
        self.cursor_position_changed();
        self.selection_changed();
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
        events
    }

    fn create_insert_animation(
        &mut self,
        event: &EditorAnimationEvent,
        origin_cx: f64,
        origin_cy: f64,
        cursor_h: f64,
    ) -> InsertAnimation {
        let width = self.bounding_width();
        let snapshot = self.layout_snapshot(width);
        let font_size = self.current_font_pixel_size as f64;
        let font_family = &self.current_font_family.to_string();

        let mut glyphs = Vec::new();
        let range_start = event.range_start;
        let range_end = range_start + event.range_len;

        for (line_idx, line) in snapshot.lines.iter().enumerate() {
            if line.end <= range_start || line.start >= range_end {
                continue;
            }
            if line.para_text.is_empty() {
                continue;
            }
            let seg_start = range_start.max(line.start);
            let seg_end = range_end.min(line.end);
            if seg_start >= seg_end {
                continue;
            }

            let baseline_y = self.editor_layout.text_baseline_y(line, font_size, font_family);
            let glyph_data = self.editor_layout.glyph_positions_on_line(
                line,
                seg_start,
                seg_end,
                font_size,
                font_family,
            );
            for (abs_byte, x_pos, ch_w) in glyph_data {
                if abs_byte >= self.buffer.text.len() {
                    continue;
                }
                let ch = self.buffer.text[abs_byte..].chars().next().unwrap();
                let ch_str = ch.to_string();
                glyphs.push(AnimatedGlyph {
                    byte_start: abs_byte,
                    byte_end: abs_byte + ch_str.len(),
                    text: ch_str,
                    rect: (line.x + x_pos, line.y, ch_w, line.height),
                    baseline_y,
                    line_index: line_idx,
                });
            }
        }

        let duration_ms = self.current_typing_animation_duration_ms.max(30) as u64;

        InsertAnimation {
            glyphs,
            origin_cursor_rect: (origin_cx, origin_cy, 2.0, cursor_h),
            start_time: Instant::now(),
            duration_ms,
        }
    }

    fn create_delete_animation(
        &mut self,
        event: &EditorAnimationEvent,
        old_text: &str,
        new_text: &str,
    ) -> DeleteAnimation {
        let width = self.bounding_width();
        let font_size = self.current_font_pixel_size as f64;
        let font_family = &self.current_font_family.to_string();

        // Old layout: glyph positions of the deleted text via Qt glyph runs
        let old_snapshot = self.layout_snapshot_for_text(old_text, width);
        let mut glyphs = Vec::new();
        let range_start = event.range_start;
        let range_end = range_start + event.range_len;

        for (line_idx, line) in old_snapshot.lines.iter().enumerate() {
            if line.end <= range_start || line.start >= range_end {
                continue;
            }
            if line.para_text.is_empty() {
                continue;
            }
            let seg_start = range_start.max(line.start);
            let seg_end = range_end.min(line.end);
            if seg_start >= seg_end {
                continue;
            }

            let baseline_y = self.editor_layout.text_baseline_y(line, font_size, font_family);
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
                let ch = old_text[abs_byte..].chars().next().unwrap();
                let ch_str = ch.to_string();
                glyphs.push(AnimatedGlyph {
                    byte_start: abs_byte,
                    byte_end: abs_byte + ch_str.len(),
                    text: ch_str,
                    rect: (line.x + x_pos, line.y, ch_w, line.height),
                    baseline_y,
                    line_index: line_idx,
                });
            }
        }

        // New layout: compute the cursor rect after deletion
        let new_snapshot = self.layout_snapshot_for_text(new_text, width);
        let new_cursor_byte = event.new_cursor.index;
        let new_cursor_rect = self.editor_layout.caret_rect(
            &new_snapshot,
            new_cursor_byte,
            CaretAffinity::Downstream,
            0.0,
            self.current_viewport_height.max(1.0) as f64,
        );

        let duration_ms = self.current_typing_animation_duration_ms.max(30) as u64;

        DeleteAnimation {
            glyphs,
            target_cursor_rect: (new_cursor_rect.x, new_cursor_rect.y, 2.0, new_cursor_rect.h),
            start_time: Instant::now(),
            duration_ms,
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
        &self.editor_layout.snapshot(&self.buffer.text, params, self.text_revision).lines
    }

    fn adjust_affinity_at_wrap_boundary(&mut self) {
        let width = self.bounding_width();
        let lines = self.ensure_layout_cached(width).clone();
        let cursor = self.buffer.cursor;

        let is_wrap_boundary = lines.iter().enumerate().any(|(idx, line)| {
            idx + 1 < lines.len() && line.end == cursor && lines[idx + 1].start == cursor
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
        self.recalculate_content_height_quiet();
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

        // 单层场景图：仅 Layer 0 静态正文 texture
        if !root_raw.is_null() && !item_ptr.is_null() {
            scene_graph::ensure_single_image_node(root_raw, item_ptr);
        }

        // ── Layer 0: Static text texture ──
        if self.render_dirty {
            match self.render_to_image() {
                Some((image, buf_scroll_y, _buf_h)) => {
                    let (src_y, src_h) = if let Some(ref buf) = self.scroll_buffer {
                        buf.clamp_source_rect(scroll_y, vp_h)
                    } else {
                        (scroll_y - buf_scroll_y, vp_h)
                    };
                    let logical_img_w = image.size().width as f64 / dpr;
                    final_root = scene_graph::update_texture_node(
                        root_raw, item_ptr, &image, 0.0, src_y, logical_img_w, src_h, 0.0, vp_h, dpr,
                    );
                    self.render_dirty = false;
                }
                None => {
                    if !root_raw.is_null() {
                        if let Some(ref buf) = self.scroll_buffer {
                            let (src_y, src_h) = buf.clamp_source_rect(scroll_y, vp_h);
                            let logical_img_w = buf.image.size().width as f64 / dpr;
                            scene_graph::update_source_rect(
                                root_raw, item_ptr, 0.0, src_y, logical_img_w, src_h, 0.0, vp_h, dpr,
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
                        root_raw, item_ptr, 0.0, src_y, logical_img_w, src_h, 0.0, vp_h, dpr,
                    );
                }
            }
            final_root = root_raw;
        }

        // 清理已完成的动画状态（不渲染 overlay）
        self.cleanup_finished_animations();

        let total_elapsed = frame_start.elapsed();
        if total_elapsed.as_millis() > 4 {
            eprintln!(
                "sujian_update_paint_node: total_ms={}, layer0_only=true",
                total_elapsed.as_millis(),
            );
        }

        unsafe { SGNode::<qmetaobject::scenegraph::ContainerNode>::from_raw(final_root) }
    }
}

