//! Linux_qt 自研写作区 — 唯一主路径
//!
//! 路线：SujianEditorItem(QQuickItem) + QTextLayout/QTextLine + QImage static texture
//!       + QSGImageNode + Rust Coordinator → immutable RenderPlan → Scene Graph renderer
//!
//! Qt 成熟路线原则（Issue #501）：
//!   排版一次，视觉快照一次，动画阶段不再理解文字。
//!
//!   - updatePaintNode() 只消费已准备好的视觉数据，不做排版/业务 diff/磁盘操作
//!   - QTextLayout/QTextLine 一次排版，QTextLine::draw() 立即生成行快照 (QImage)
//!   - 动画纹理从行快照 UV 裁剪提取，不再为每个 QGlyphRun 重新排版
//!   - TextAnimationGlyphInfo 只携带位置/尺寸/透明度/纹理引用，不携带 byte_range/para_text/font_id
//!   - QSGTransformNode 负责位移，QSGOpacityNode 负责淡入淡出，UV/sourceRect 负责裁剪
//!
//! 禁止旧路线：DocumentHandler / TextArea / QTextDocument / QQuickPaintedItem / QSG 三层 overlay
//!             EditorAnimationOverlay / EditorGlyphGhost / visual_transaction_json QML overlay

// =============================================================================
// sujian_editor_item - Linux_qt self-rendered editor item
// =============================================================================

pub(crate) mod animation_coordinator;
pub(crate) mod animation_mode;
pub(crate) mod buffer;
pub(crate) mod cursor_animation;
pub(crate) mod cursor_controller;
pub(crate) mod decoration_slice;
pub(crate) mod editing;
pub(crate) mod ime_visual;
pub(crate) mod input_host;
pub(crate) mod layout_ops;
pub(crate) mod layout_snapshot;
pub(crate) mod layout_revision;
pub(crate) mod line_snapshot;
pub(crate) mod line_snapshot_builder;
pub(crate) mod animated_slice;
pub(crate) mod pipeline;
pub(crate) mod static_line_patch;
pub(crate) mod text_visual_transaction;
pub(crate) mod properties;
pub(crate) mod qquickitem_impl;
pub(crate) mod render_plan;
pub(crate) mod rendering;
pub(crate) mod scene_graph_renderer;
pub(crate) mod snapshot_id;
pub(crate) mod texture_cache;
pub(crate) mod transaction;
pub(crate) mod transaction_key;
#[cfg(test)]
mod tests;

use crate::editor::input::{self, EditorInputHost};
use crate::editor::layout::{
    text_baseline_y, CaretAffinity, CursorLayoutRect, EditorLayout, LayoutParams, LayoutSnapshot,
    VisualLine,
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
use transaction_key::VisualTransactionKey;

use writer_core::editor::{
    AnimationMode as CoreAnimationMode, CompositionSession, CursorRect, EditorAnimationKind,
    EditorCursor, EditorSelection, EditorTransactionCause, EditorVisualTransaction,
    GlyphRect, PreeditVisualTransaction,
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
    if (0xFE00..=0xFE0F).contains(&cp) || (0xE0100..=0xE01EF).contains(&cp) {
        return true;
    }
    if (0x0300..=0x036F).contains(&cp) {
        return true;
    }
    if (0x1AB0..=0x1AFF).contains(&cp) {
        return true;
    }
    if (0x1DC0..=0x1DFF).contains(&cp) {
        return true;
    }
    if (0x20D0..=0x20FF).contains(&cp) {
        return true;
    }
    if (0xFE20..=0xFE2F).contains(&cp) {
        return true;
    }
    if (0x1F600..=0x1F64F).contains(&cp) {
        return true;
    }
    if (0x1F300..=0x1F5FF).contains(&cp) {
        return true;
    }
    if (0x1F680..=0x1F6FF).contains(&cp) {
        return true;
    }
    if (0x1F900..=0x1F9FF).contains(&cp) {
        return true;
    }
    false
}

#[derive(QObject)]
pub struct SujianEditorItem {
    #[allow(dead_code)]
    base: qt_base_class!(trait QQuickItem),

    #[allow(dead_code)]
    plain_text: qt_property!(QString; READ plain_text WRITE set_plain_text NOTIFY plain_text_changed),
    #[allow(dead_code)]
    content_height: qt_property!(f32; READ content_height NOTIFY content_height_changed),
    #[allow(dead_code)]
    cursor_position: qt_property!(u32; READ cursor_position NOTIFY cursor_position_changed),
    #[allow(dead_code)]
    has_selection: qt_property!(bool; READ has_selection NOTIFY selection_changed),
    #[allow(dead_code)]
    editor_enabled: qt_property!(bool; READ editor_enabled WRITE set_editor_enabled NOTIFY editor_enabled_changed),
    #[allow(dead_code)]
    font_pixel_size: qt_property!(f32; READ font_pixel_size WRITE set_font_pixel_size NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    font_family: qt_property!(QString; READ font_family WRITE set_font_family NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    line_spacing: qt_property!(f32; READ line_spacing WRITE set_line_spacing NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    text_indent: qt_property!(f32; READ text_indent WRITE set_text_indent NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    padding: qt_property!(f32; READ padding WRITE set_padding NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    text_color: qt_property!(QString; READ text_color WRITE set_text_color NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    selection_color: qt_property!(QString; READ selection_color WRITE set_selection_color NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    selected_text_color: qt_property!(QString; READ selected_text_color WRITE set_selected_text_color NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    cursor_color: qt_property!(QString; READ cursor_color WRITE set_cursor_color NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    smooth_cursor_enabled: qt_property!(bool; READ smooth_cursor_enabled WRITE set_smooth_cursor_enabled NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    cursor_animation_duration_ms: qt_property!(u32; READ cursor_animation_duration_ms WRITE set_cursor_animation_duration_ms NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    typing_animation_enabled: qt_property!(bool; READ typing_animation_enabled WRITE set_typing_animation_enabled NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    typing_animation_duration_ms: qt_property!(u32; READ typing_animation_duration_ms WRITE set_typing_animation_duration_ms NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    coordinated_text_cursor_animation_enabled: qt_property!(bool; READ coordinated_text_cursor_animation_enabled WRITE set_coordinated_text_cursor_animation_enabled NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    last_transaction_summary: qt_property!(QString; READ last_transaction_summary NOTIFY transaction_created),
    #[allow(dead_code)]
    last_animation_event_count: qt_property!(u32; READ last_animation_event_count NOTIFY transaction_created),
    #[allow(dead_code)]
    scroll_y: qt_property!(f32; READ scroll_y WRITE set_scroll_y NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    viewport_height: qt_property!(f32; READ viewport_height WRITE set_viewport_height NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    is_scrolling: qt_property!(bool; READ is_scrolling WRITE set_is_scrolling NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    is_loading: qt_property!(bool; READ is_loading WRITE set_is_loading NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    is_applying_format: qt_property!(bool; READ is_applying_format WRITE set_is_applying_format NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    is_applying_settings: qt_property!(bool; READ is_applying_settings WRITE set_is_applying_settings NOTIFY visual_settings_changed),
    #[allow(dead_code)]
    cursor_rect_x: qt_property!(f32; READ cursor_rect_x NOTIFY cursor_rect_changed),
    #[allow(dead_code)]
    cursor_rect_y: qt_property!(f32; READ cursor_rect_y NOTIFY cursor_rect_changed),
    #[allow(dead_code)]
    cursor_rect_width: qt_property!(f32; READ cursor_rect_width NOTIFY cursor_rect_changed),
    #[allow(dead_code)]
    cursor_rect_height: qt_property!(f32; READ cursor_rect_height NOTIFY cursor_rect_changed),
    #[allow(dead_code)]
    cursor_visible: qt_property!(bool; READ cursor_visible NOTIFY cursor_rect_changed),
    #[allow(dead_code)]
    cursor_blink_visible: qt_property!(bool; READ cursor_blink_visible NOTIFY cursor_rect_changed),
    #[allow(dead_code)]
    cursor_should_be_visible: qt_property!(bool; READ cursor_should_be_visible NOTIFY cursor_rect_changed),
    #[allow(dead_code)]
    cursor_blink_opacity: qt_property!(f32; READ cursor_blink_opacity NOTIFY cursor_rect_changed),
    #[allow(dead_code)]
    anchor_rect_x: qt_property!(f32; READ anchor_rect_x NOTIFY selection_changed),
    #[allow(dead_code)]
    anchor_rect_y: qt_property!(f32; READ anchor_rect_y NOTIFY selection_changed),
    #[allow(dead_code)]
    anchor_rect_width: qt_property!(f32; READ anchor_rect_width NOTIFY selection_changed),
    #[allow(dead_code)]
    anchor_rect_height: qt_property!(f32; READ anchor_rect_height NOTIFY selection_changed),
    #[allow(dead_code)]
    anchor_position: qt_property!(u32; READ anchor_position NOTIFY selection_changed),
    #[allow(dead_code)]
    current_selection_text: qt_property!(QString; READ current_selection_text NOTIFY selection_changed),

    #[allow(dead_code)]
    plain_text_changed: qt_signal!(),
    #[allow(dead_code)]
    text_changed: qt_signal!(),
    #[allow(dead_code)]
    content_height_changed: qt_signal!(),
    #[allow(dead_code)]
    cursor_position_changed: qt_signal!(),
    #[allow(dead_code)]
    selection_changed: qt_signal!(),
    #[allow(dead_code)]
    editor_enabled_changed: qt_signal!(),
    #[allow(dead_code)]
    visual_settings_changed: qt_signal!(),
    #[allow(dead_code)]
    transaction_created: qt_signal!(),
    #[allow(dead_code)]
    cursor_rect_changed: qt_signal!(),
    #[allow(dead_code)]
    explicit_clear_requested: qt_signal!(),
    #[allow(dead_code)]
    context_menu_requested: qt_signal!(x: f32, y: f32),
    #[allow(dead_code)]
    hide_context_menu_requested: qt_signal!(),

    #[allow(dead_code)]
    get_plain_text: qt_method!(fn(&self) -> QString),
    #[allow(dead_code)]
    set_plain_text: qt_method!(fn(&mut self, text: QString)),
    #[allow(dead_code)]
    reload_plain_text: qt_method!(fn(&mut self, text: QString)),
    #[allow(dead_code)]
    clear_undo_stack: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    insert_text: qt_method!(fn(&mut self, text: QString)),
    #[allow(dead_code)]
    delete_backward: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    delete_forward: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    delete_selection: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    select_all: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    selected_text: qt_method!(fn(&self) -> QString),
    #[allow(dead_code)]
    undo: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    redo: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    handle_key: qt_method!(fn(&mut self, key: i32, modifiers: i32) -> bool),
    #[allow(dead_code)]
    click_at: qt_method!(fn(&mut self, x: f32, y: f32, extend: bool)),
    #[allow(dead_code)]
    drag_select_at: qt_method!(fn(&mut self, x: f32, y: f32)),
    #[allow(dead_code)]
    clipboard_copy: qt_method!(fn(&mut self) -> bool),
    #[allow(dead_code)]
    clipboard_paste: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    insert_preedit: qt_method!(fn(&mut self, text: QString)),
    #[allow(dead_code)]
    commit_preedit: qt_method!(fn(&mut self, text: QString)),
    #[allow(dead_code)]
    cancel_preedit: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    flush_content_height: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    tick_cursor_animation: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    long_press_at: qt_method!(fn(&mut self, x: f32, y: f32)),
    #[allow(dead_code)]
    select_word_at: qt_method!(fn(&mut self, x: f32, y: f32)),
    #[allow(dead_code)]
    request_text_input_focus: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    snap_next_cursor_update: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    verify_animation_signal_meta_object: qt_method!(fn(&self) -> bool),

    pipeline: pipeline::LinuxEditorPipeline,
    buffer: EditorBuffer,
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
    editor_layout: EditorLayout,
    render_dirty: bool,
    scroll_buffer: Option<ScrollBuffer>,
    last_slow_paint_log: Option<Instant>,
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

            pipeline: pipeline::LinuxEditorPipeline::new(),
            buffer: EditorBuffer::default(),
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
            editor_layout: EditorLayout::default(),
            render_dirty: true,
            scroll_buffer: None,
            last_slow_paint_log: None,
            cursor_ctrl: cursor_controller::CursorController::new(),
        }
    }
}

impl SujianEditorItem {
    pub(crate) fn sync_buffer_from_pipeline(&mut self) {
        let mirror = self.pipeline.mirror();
        if self.buffer.text != mirror.text() {
            self.buffer.text = mirror.text().to_string();
        }
        self.buffer.cursor = mirror.cursor();
        self.buffer.selection_anchor = mirror.selection_anchor();
    }

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
        self.pipeline.bump_visual_revision();
    }

    pub(crate) fn clear_active_text_animations(&mut self) {
        if self.pipeline.animation_coordinator_mut().suppress_all() {
            self.pipeline.texture_cache_mut().clear();
            self.pipeline.set_current_layout_snapshot(None);
            self.pipeline.set_previous_layout_snapshot(None);
            self.pipeline.set_previous_canonical_snapshot(None);
            self.request_static_repaint();
            self.cursor_rect_changed();
        }
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
            let key = VisualTransactionKey {
                transaction_id: tid,
                generation: tid,
            };
            let removed = self.pipeline.animation_coordinator_mut().finish_by_key(key);
            if let Some(ids) = removed {
                editor_animation_debug_log(&format!(
                    "on_insert_animation_{}: tid={}, cleared, has_active_insert={}",
                    if skipped { "skipped" } else { "finished" },
                    tid,
                    self.pipeline.animation_coordinator_mut().has_active_insert()
                ));
                self.pipeline.texture_cache_mut().remove_for_transaction(&ids);
                self.request_static_repaint();
                self.cursor_rect_changed();
            }
        }
    }

    pub(crate) fn has_active_insert_animation(&self) -> bool {
        self.pipeline.animation_coordinator().has_active_insert()
    }

    pub(crate) fn active_insert_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.pipeline.animation_coordinator().insert_byte_ranges()
    }

    pub(crate) fn active_reflow_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.pipeline.animation_coordinator().reflow_byte_ranges()
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

    pub(crate) fn build_selection_preedit_plan(&mut self) -> animation_coordinator::SelectionPreeditPlan {
        use animation_coordinator::{SelectionRange, PreeditRange};

        let mut plan = animation_coordinator::SelectionPreeditPlan::default();

        let selection_color = self.current_selection_color.to_string();

        if self.buffer.has_selection() {
            plan.has_selection = true;
            let width = self.bounding_width();
            let _font_size = f64::from(self.current_font_pixel_size);
            let _font_family = &self.current_font_family.to_string();
            let scroll_y = f64::from(self.current_scroll_y);
            let viewport_h = f64::from(self.current_viewport_height.max(1.0));
            let snapshot = self.layout_snapshot(width);

            let anchor = self.buffer.selection_anchor.min(self.buffer.cursor);
            let head = self.buffer.selection_anchor.max(self.buffer.cursor);

            for line in &snapshot.lines {
                if line.para_text.is_empty() { continue; }
                if line.byte_end <= anchor || line.byte_start >= head { continue; }
                let line_top = line.y - scroll_y;
                let line_bottom = line_top + line.height;
                if line_bottom < 0.0 || line_top > viewport_h { continue; }

                let seg_start = anchor.max(line.byte_start);
                let seg_end = head.min(line.byte_end);
                if seg_start >= seg_end { continue; }

                let start_x = self.editor_layout.cursor_x_for_line(
                    &snapshot, line, seg_start, crate::editor::layout::CaretAffinity::Downstream,
                );
                let end_x = self.editor_layout.cursor_x_for_line(
                    &snapshot, line, seg_end, crate::editor::layout::CaretAffinity::Downstream,
                );
                let left_x = start_x.min(end_x);
                let sel_w = (end_x - start_x).abs();

                let sel_color = if selection_color.starts_with('#') && selection_color.len() >= 7 {
                    let r = u8::from_str_radix(&selection_color[1..3], 16).unwrap_or(0);
                    let g = u8::from_str_radix(&selection_color[3..5], 16).unwrap_or(0);
                    let b = u8::from_str_radix(&selection_color[5..7], 16).unwrap_or(0);
                    format!("#{:02X}{:02X}{:02X}{:02X}", r, g, b, 0x33)
                } else {
                    "#3381D1D1".to_string()
                };

                plan.selection_ranges.push(SelectionRange {
                    x: left_x,
                    y: line_top,
                    w: sel_w,
                    h: line.height,
                    color: sel_color,
                });
            }
        }

        if !self.pipeline.composition().preedit_text.is_empty() {
            plan.has_preedit = true;
            if let Some(ref _preedit_rect) = self.pipeline.composition().preedit_cursor_rect {
                let width = self.bounding_width();
                let font_size = f64::from(self.current_font_pixel_size);
                let font_family = &self.current_font_family.to_string();
                let scroll_y = f64::from(self.current_scroll_y);
                let snapshot = self.layout_snapshot(width);
                let cursor_byte = self.buffer.cursor;

                if let Some(line) = snapshot.lines.iter().find(|l| l.byte_end >= cursor_byte && l.byte_start <= cursor_byte) {
                    let start_x = self.editor_layout.cursor_x_for_line(
                        &snapshot, line, cursor_byte, crate::editor::layout::CaretAffinity::Downstream,
                    );
                    let preedit_w = self.editor_layout.text_width(&self.pipeline.composition().preedit_text, font_size, font_family);

                    let preedit_color = if selection_color.starts_with('#') && selection_color.len() >= 7 {
                        let r = u8::from_str_radix(&selection_color[1..3], 16).unwrap_or(0);
                        let g = u8::from_str_radix(&selection_color[3..5], 16).unwrap_or(0);
                        let b = u8::from_str_radix(&selection_color[5..7], 16).unwrap_or(0);
                        format!("#{:02X}{:02X}{:02X}{:02X}", r, g, b, 0x1A)
                    } else {
                        "#1A81D1D1".to_string()
                    };

                    plan.preedit_ranges.push(PreeditRange {
                        x: start_x,
                        y: line.y - scroll_y,
                        w: preedit_w,
                        h: line.height,
                        color: preedit_color,
                        underline: true,
                    });
                }
            }
        }

        plan
    }
}
