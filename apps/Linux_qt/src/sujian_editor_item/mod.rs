//! Linux_qt 自研写作区 — 唯一主路径
//!
//! 路线：SujianEditorItem(QQuickItem) + QTextLayout/QTextLine + QImage static texture
//!       + QSGImageNode + QML Rectangle cursor + QML EditorAnimationOverlay
//!
//! 禁止旧路线：DocumentHandler / TextArea / QTextDocument / QQuickPaintedItem / QSG 三层 overlay

// =============================================================================
// sujian_editor_item - Linux_qt self-rendered editor item
// =============================================================================

pub(crate) mod animation_coordinator;
pub(crate) mod buffer;
pub(crate) mod cursor_controller;
pub(crate) mod editing;
pub(crate) mod ime_visual;
pub(crate) mod input_host;
pub(crate) mod layout_ops;
pub(crate) mod properties;
pub(crate) mod qquickitem_impl;
pub(crate) mod rendering;
pub(crate) mod transaction;
#[cfg(test)]
mod tests;

use crate::backend::diagnostics;
use crate::editor::input::{self, EditorInputHost};
use crate::editor::layout::{
    text_baseline_y, CaretAffinity, CursorLayoutRect, EditorLayout, LayoutParams, LayoutSnapshot,
    VisualLine,
};
use crate::editor::renderer;
use crate::editor::scene_graph;
use crate::platform::linux_qt::LinuxQtClipboardFocusAdapter;
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
use animation_coordinator::LinuxEditorAnimationCoordinator;

use writer_core::editor::{
    AnimationMode as CoreAnimationMode, CursorRect, EditorAnimationKind, EditorCursor,
    EditorEngine, EditorSelection, EditorTransactionCause, EditorVisualTransaction, GlyphRect, PreeditVisualTransaction,
    ReflowGlyphRect,
};

use animation_coordinator::AnimationMode;

#[derive(Clone, Debug)]
pub(crate) struct PreeditAttribute {
    pub start: usize,
    pub length: usize,
    pub kind: PreeditAttributeKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PreeditAttributeKind {
    Underline,
    Cursor,
    TextColor { color: String },
    BackgroundColor { color: String },
    FontUnderline,
}

cpp! {{
    #include <QtGui/QFont>
    #include <QtGui/QPainter>
    #include <QByteArray>
    #include <QGuiApplication>
    #include <QMetaMethod>
    #include <QMetaObject>
    #include <QStringList>
}}

mod editor_color_fallback {
    pub const TEXT_COLOR: &str = "#E2E2E5";
    pub const SELECTION_COLOR: &str = "#006497";
    pub const SELECTED_TEXT_COLOR: &str = "#CCE5FF";
    pub const CURSOR_COLOR: &str = "#006497";
}

pub(crate) fn editor_debug_log(msg: &str) {
    if std::env::var("SUJIAN_EDITOR_DEBUG").is_ok() || std::env::var("WRITER_DEBUG").is_ok() {
        eprintln!("{}", msg);
    }
}

pub(crate) fn editor_animation_debug_log(msg: &str) {
    if std::env::var("SUJIAN_EDITOR_ANIMATION_DEBUG").is_ok() 
        || std::env::var("SUJIAN_EDITOR_DEBUG").is_ok() 
        || std::env::var("WRITER_DEBUG").is_ok() {
        eprintln!("{}", msg);
    }
}

pub(crate) fn is_complex_grapheme(ch: char) -> bool {
    let cp = ch as u32;
    if cp > 0xFFFF {
        return true;
    }
    if cp == 0x200D {
        return true;
    }
    if (cp >= 0xFE00 && cp <= 0xFE0F) || (cp >= 0xE0100 && cp <= 0xE01EF) {
        return true;
    }
    if cp >= 0x0300 && cp <= 0x036F {
        return true;
    }
    if cp >= 0x1AB0 && cp <= 0x1AFF {
        return true;
    }
    if cp >= 0x1DC0 && cp <= 0x1DFF {
        return true;
    }
    if cp >= 0x20D0 && cp <= 0x20FF {
        return true;
    }
    if cp >= 0xFE20 && cp <= 0xFE2F {
        return true;
    }
    if cp >= 0x1F600 && cp <= 0x1F64F {
        return true;
    }
    if cp >= 0x1F300 && cp <= 0x1F5FF {
        return true;
    }
    if cp >= 0x1F680 && cp <= 0x1F6FF {
        return true;
    }
    if cp >= 0x1F900 && cp <= 0x1F9FF {
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
    preedit_visual_transaction_json: qt_property!(QString; READ preedit_visual_transaction_json NOTIFY preedit_visual_transaction_changed),
    scroll_y: qt_property!(f32; READ scroll_y WRITE set_scroll_y NOTIFY visual_settings_changed),
    viewport_height: qt_property!(f32; READ viewport_height WRITE set_viewport_height NOTIFY visual_settings_changed),
    is_scrolling: qt_property!(bool; READ is_scrolling WRITE set_is_scrolling NOTIFY visual_settings_changed),
    is_loading: qt_property!(bool; READ is_loading WRITE set_is_loading NOTIFY visual_settings_changed),
    is_applying_format: qt_property!(bool; READ is_applying_format WRITE set_is_applying_format NOTIFY visual_settings_changed),
    is_applying_settings: qt_property!(bool; READ is_applying_settings WRITE set_is_applying_settings NOTIFY visual_settings_changed),
    cursor_rect_x: qt_property!(f32; READ cursor_rect_x NOTIFY cursor_rect_changed),
    cursor_rect_y: qt_property!(f32; READ cursor_rect_y NOTIFY cursor_rect_changed),
    cursor_rect_width: qt_property!(f32; READ cursor_rect_width NOTIFY cursor_rect_changed),
    cursor_rect_height: qt_property!(f32; READ cursor_rect_height NOTIFY cursor_rect_changed),
    cursor_visible: qt_property!(bool; READ cursor_visible NOTIFY cursor_rect_changed),
    cursor_blink_visible: qt_property!(bool; READ cursor_blink_visible NOTIFY cursor_rect_changed),
    cursor_should_be_visible: qt_property!(bool; READ cursor_should_be_visible NOTIFY cursor_rect_changed),
    cursor_blink_opacity: qt_property!(f32; READ cursor_blink_opacity NOTIFY cursor_rect_changed),
    anchor_rect_x: qt_property!(f32; READ anchor_rect_x NOTIFY selection_changed),
    anchor_rect_y: qt_property!(f32; READ anchor_rect_y NOTIFY selection_changed),
    anchor_rect_width: qt_property!(f32; READ anchor_rect_width NOTIFY selection_changed),
    anchor_rect_height: qt_property!(f32; READ anchor_rect_height NOTIFY selection_changed),
    anchor_position: qt_property!(u32; READ anchor_position NOTIFY selection_changed),
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
    preedit_visual_transaction_changed: qt_signal!(),
    context_menu_requested: qt_signal!(x: f32, y: f32),
    hide_context_menu_requested: qt_signal!(),

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
    clipboard_copy: qt_method!(fn(&mut self) -> bool),
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
    verify_animation_signal_meta_object: qt_method!(fn(&self) -> bool),
    on_insert_animation_finished_by_id: qt_method!(fn(&mut self, transaction_id: QString, range_id: QString, byte_start: i32, byte_end: i32)),
    on_insert_animation_skipped_by_id: qt_method!(fn(&mut self, transaction_id: QString, range_id: QString, byte_start: i32, byte_end: i32)),

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
    current_is_loading: bool,
    current_is_applying_format: bool,
    current_is_applying_settings: bool,
    last_summary: QString,
    last_event_count: u32,
    last_visual_transaction_json: QString,
    last_preedit_visual_transaction_json: QString,
    preedit_text: String,
    preedit_cursor: usize,
    preedit_attributes: Vec<PreeditAttribute>,
    preedit_old_text: String,
    preedit_visual_transaction: Option<PreeditVisualTransaction>,
    preedit_cursor_rect: Option<CursorRect>,
    pending_preedit_cursor_rect: Option<CursorRect>,
    suppress_next_ime_commit: bool,
    editor_layout: EditorLayout,
    text_revision: u64,
    visual_revision: u64,
    render_dirty: bool,
    scroll_buffer: Option<ScrollBuffer>,
    last_slow_paint_log: Option<Instant>,
    cursor_ctrl: cursor_controller::CursorController,
    animation_coordinator: LinuxEditorAnimationCoordinator,
    cached_animation_textures: std::collections::HashMap<animation_coordinator::VisualTransactionKey, Vec<qmetaobject::QImage>>,
    clipboard_adapter: LinuxQtClipboardFocusAdapter,
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
            font_family: Default::default(),
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
            preedit_visual_transaction_json: Default::default(),
            scroll_y: Default::default(),
            viewport_height: Default::default(),
            is_scrolling: Default::default(),
            is_loading: Default::default(),
            is_applying_format: Default::default(),
            is_applying_settings: Default::default(),
            cursor_rect_x: Default::default(),
            cursor_rect_y: Default::default(),
            cursor_rect_width: Default::default(),
            cursor_rect_height: Default::default(),
            cursor_visible: Default::default(),
            cursor_blink_visible: Default::default(),
            cursor_should_be_visible: Default::default(),
            cursor_blink_opacity: Default::default(),
            anchor_rect_x: Default::default(),
            anchor_rect_y: Default::default(),
            anchor_rect_width: Default::default(),
            anchor_rect_height: Default::default(),
            anchor_position: Default::default(),
            current_selection_text: Default::default(),

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
            preedit_visual_transaction_changed: Default::default(),
            context_menu_requested: Default::default(),
            hide_context_menu_requested: Default::default(),

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
            verify_animation_signal_meta_object: Default::default(),
            on_insert_animation_finished_by_id: Default::default(),
            on_insert_animation_skipped_by_id: Default::default(),

            buffer: EditorBuffer::default(),
            engine: EditorEngine::new(),
            current_content_height: 0.0,
            content_height_dirty: Cell::new(false),
            current_editor_enabled: true,
            current_font_pixel_size: 22.0,
            current_font_family: QString::from("Noto Sans CJK SC"),
            current_line_spacing: 1.5,
            current_text_indent: 0.0,
            current_padding: 16.0,
            current_text_color: editor_color_fallback::TEXT_COLOR.into(),
            current_selection_color: editor_color_fallback::SELECTION_COLOR.into(),
            current_selected_text_color: editor_color_fallback::SELECTED_TEXT_COLOR.into(),
            current_cursor_color: editor_color_fallback::CURSOR_COLOR.into(),
            current_smooth_cursor_enabled: true,
            current_cursor_animation_duration_ms: 120,
            current_typing_animation_enabled: true,
            current_typing_animation_duration_ms: 160,
            current_coordinated_text_cursor_animation_enabled: true,
            current_scroll_y: 0.0,
            current_viewport_height: 0.0,
            current_is_scrolling: false,
            current_is_loading: false,
            current_is_applying_format: false,
            current_is_applying_settings: false,
            last_summary: Default::default(),
            last_event_count: 0,
            last_visual_transaction_json: "{}".into(),
            last_preedit_visual_transaction_json: "".into(),
            preedit_text: String::new(),
            preedit_cursor: 0,
            preedit_attributes: Vec::new(),
            preedit_old_text: String::new(),
            preedit_visual_transaction: None,
            preedit_cursor_rect: None,
            pending_preedit_cursor_rect: None,
            suppress_next_ime_commit: false,
            editor_layout: EditorLayout::default(),
            text_revision: 0,
            visual_revision: 0,
            render_dirty: true,
            scroll_buffer: None,
            last_slow_paint_log: None,
            cursor_ctrl: cursor_controller::CursorController::new(),
            animation_coordinator: LinuxEditorAnimationCoordinator::new(),
            cached_animation_textures: std::collections::HashMap::new(),
            clipboard_adapter: LinuxQtClipboardFocusAdapter::new(),
        }
    }
}

impl SujianEditorItem {
    pub(crate) fn request_static_repaint(&mut self) {
        self.render_dirty = true;
        let item = self as &dyn QQuickItem;
        item.update();
    }

    pub(crate) fn request_frame_update(&mut self) {
        let item = self as &dyn QQuickItem;
        item.update();
    }

    pub(crate) fn bump_visual_revision(&mut self) {
        self.visual_revision = self.visual_revision.wrapping_add(1);
    }

    pub(crate) fn clear_active_text_animations(&mut self) {
        if self.animation_coordinator.suppress_all() {
            self.request_static_repaint();
            self.cursor_rect_changed();
        }
    }

    pub(crate) fn on_insert_animation_finished_by_id(
        &mut self,
        transaction_id: QString,
        range_id: QString,
        byte_start: i32,
        byte_end: i32,
    ) {
        self.finish_insert_animation(
            Self::positive_id_string_to_u64(&transaction_id.to_string()),
            Self::positive_id_string_to_u64(&range_id.to_string()),
            byte_start,
            byte_end,
            false,
        );
    }

    pub(crate) fn on_insert_animation_skipped_by_id(
        &mut self,
        transaction_id: QString,
        range_id: QString,
        byte_start: i32,
        byte_end: i32,
    ) {
        self.finish_insert_animation(
            Self::positive_id_string_to_u64(&transaction_id.to_string()),
            Self::positive_id_string_to_u64(&range_id.to_string()),
            byte_start,
            byte_end,
            true,
        );
    }

    pub(crate) fn positive_id_string_to_u64(value: &str) -> Option<u64> {
        value.parse::<u64>().ok().filter(|id| *id > 0)
    }

    pub(crate) fn animation_mode_from_core(mode: CoreAnimationMode) -> AnimationMode {
        match mode {
            CoreAnimationMode::GlyphAnimation => AnimationMode::GlyphAnimation,
            CoreAnimationMode::ClusterAnimation => AnimationMode::ClusterAnimation,
            CoreAnimationMode::RunAnimation => AnimationMode::RunAnimation,
            CoreAnimationMode::LineReflowAnimation => AnimationMode::LineReflowAnimation,
            CoreAnimationMode::SnapshotAnimation | CoreAnimationMode::SystemSuppressed => {
                AnimationMode::SystemSuppressed
            }
        }
    }

    pub(crate) fn finish_insert_animation(
        &mut self,
        transaction_id: Option<u64>,
        range_id: Option<u64>,
        byte_start: i32,
        byte_end: i32,
        skipped: bool,
    ) {
        let _ = (range_id, byte_start, byte_end);
        if let Some(tid) = transaction_id {
            let key = animation_coordinator::VisualTransactionKey {
                transaction_id: tid,
                generation: tid,
            };
            let removed = self.animation_coordinator.finish_by_key(key);
            if removed {
                editor_animation_debug_log(&format!(
                    "on_insert_animation_{}: tid={}, cleared hidden range, has_active_insert={}",
                    if skipped { "skipped" } else { "finished" },
                    tid,
                    self.animation_coordinator.has_active_insert()
                ));
                self.cached_animation_textures.remove(&key);
                self.request_static_repaint();
                self.cursor_rect_changed();
            }
        }
    }

    pub(crate) fn has_active_insert_animation(&self) -> bool {
        self.animation_coordinator.has_active_insert()
    }

    pub(crate) fn active_insert_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.animation_coordinator.insert_byte_ranges()
    }

    pub(crate) fn active_reflow_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.animation_coordinator.reflow_byte_ranges()
    }

    pub(crate) fn ime_query_text_before_cursor(&self, max_chars: usize) -> String {
        let text = &self.buffer.text;
        let cursor_char = byte_to_char_index(text, self.buffer.cursor);
        let before_char_len = cursor_char.min(max_chars);
        text.chars()
            .skip(cursor_char - before_char_len)
            .take(before_char_len)
            .collect()
    }

    pub(crate) fn ime_query_text_after_cursor(&self, max_chars: usize) -> String {
        let text = &self.buffer.text;
        let cursor_char = byte_to_char_index(text, self.buffer.cursor);
        let total_chars = text.chars().count();
        let after_char_len = total_chars.saturating_sub(cursor_char).min(max_chars);
        text.chars()
            .skip(cursor_char)
            .take(after_char_len)
            .collect()
    }

    pub(crate) fn ime_query_selected_text(&self) -> String {
        self.buffer.selected_text()
    }
}
