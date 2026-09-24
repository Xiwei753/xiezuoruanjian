//! Linux_qt 自研写作区 — 唯一主路径
//!
//! 路线：SujianEditorItem(QQuickItem) + QTextLayout/QTextLine + QSGTextNode (Qt 6.7+ public API)
//!       + Rust Coordinator → immutable RenderPlan → Scene Graph renderer
//!
//! Qt 成熟路线原则（Issue #501 / #658）：
//!   排版一次，视觉快照一次，动画阶段不再理解文字。
//!
//!   - updatePaintNode() 只消费已准备好的视觉数据，不做排版/业务 diff/磁盘操作
//!   - QTextLayout/QTextLine 一次排版，QSGTextNode::addTextLayout() 把已排好的 layout 交给 Qt 文本节点
//!   - 动画纹理从行快照 UV 裁剪提取，不再为每个 QGlyphRun 重新排版
//!   - TextAnimationGlyphInfo 只携带位置/尺寸/透明度/纹理引用，不携带 byte_range/para_text/font_id
//!   - QSGTransformNode 负责位移，QSGOpacityNode 负责淡入淡出，UV/sourceRect 负责裁剪
//!
//! 禁止旧路线：DocumentHandler / TextArea / QTextDocument / QQuickPaintedItem / QSG 三层 overlay
//!             EditorAnimationOverlay / EditorGlyphGhost / visual_transaction_json QML overlay
//!             整块 QImage -> QSGImageNode 栅格化（Issue #658 已删除）

// =============================================================================
// sujian_editor_item - Linux_qt self-rendered editor item
// =============================================================================

pub(crate) mod animated_slice;
pub(crate) mod animation;
pub(crate) mod animation_mode;
pub(crate) mod cursor_animation;
/// Issue #707 评论 5723616999: 改 `pub` 让集成测试能访问 `CursorController`。
pub mod cursor_controller;
pub(crate) mod edit_motion;
pub(crate) mod edit_snapshot;
pub(crate) mod editing;
pub(crate) mod ime_visual;
pub(crate) mod input_host;
pub(crate) mod layout_ops;
pub(crate) mod layout_revision;
pub(crate) mod layout_snapshot;
pub(crate) mod line_snapshot;
pub(crate) mod line_snapshot_builder;
pub(crate) mod pipeline;
#[allow(clippy::misnamed_getters)]
pub(crate) mod properties;
pub(crate) mod qquickitem_impl;
pub(crate) mod qt_text_node;
/// Issue #707 评论 5723616999: 改 `pub` 让集成测试能访问 `RenderPlan`。
pub mod render_plan;
pub(crate) mod rendering;
/// Issue #707 评论 5724685300: 内部状态测试 — 在模块内部直接访问 pub(crate) 字段。
#[cfg(test)]
mod runtime_tests;
pub(crate) mod scene_graph_renderer;
pub(crate) mod snapshot_id;
pub(crate) mod text_utils;
pub(crate) mod texture_cache;
pub(crate) mod transaction;
pub(crate) mod transaction_key;

use crate::editor::input::{self, EditorInputHost};
use crate::editor::layout::{
    text_baseline_y, CaretAffinity, CursorLayoutRect, EditorLayout, LayoutParams, LayoutSnapshot,
    VisualLine,
};
use crate::editor::renderer;
use crate::editor::scene_graph;
use cpp::cpp;
use edit_snapshot::EditorSnapshot;
use qmetaobject::prelude::*;
use qmetaobject::{QMouseEvent, QQuickItem, QRectF, QString};
use std::cell::Cell;
use text_utils::{
    byte_to_char_index, clamp_to_char_boundary, next_char_boundary, normalize_plain_text,
    prev_char_boundary,
};
use transaction_key::VisualTransactionKey;

use writer_core::editor::EditorTransactionCause;

// Issue #735: 重新导出 Linux 私有视觉类型，供 `use super::*` 的子模块使用。
pub(crate) use edit_motion::{CompositionSession, CursorRect};

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
        || std::env::var("WRITER_DEBUG").is_ok()
    {
        eprintln!("{}", msg);
    }
}

/// 当前 Unix 毫秒时间戳，用于动画诊断事件的"首帧时间"字段（与 `record_event`
/// 内部 `timestamp_ms` 同一时基，便于诊断包内关联）。
pub(crate) fn diagnostic_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Issue #690 评论 5675007226 步骤 5: 把每笔动画的紧凑事件写入正式诊断包
/// （`writer_diagnostics`），不再只走 env 控制的 `editor_animation_debug_log` eprintln。
///
/// 只在四点各触发一次：事务创建（create）、被新编辑 retarget（rebase）、
/// 未被本次编辑覆盖而继续自播（keep）、播放完成（complete）。
/// 不逐帧刷日志。diagnostics 在桌面端默认开启，普通实测包（不带 debug 环境变量）即可在
/// zip 中看到 transaction key / operation kind / old-new caret / visual unit kinds /
/// 首帧时间 / 完成或 retarget 原因，便于排查"文字领先光标"是不是同一个 frame sample、
/// 哪个 unit 被重启。
///
/// `record_event` 经由 `writer_diagnostics` 后台 writer 落盘；未初始化/禁用时直接丢弃，
/// 因此在本模块单测中调用也安全、不会 panic。
pub(crate) fn editor_animation_diagnostic_event(
    event: &str,
    key: &VisualTransactionKey,
    operation_kind: &str,
    old_caret: Option<(f64, f64)>,
    new_caret: Option<(f64, f64)>,
    unit_kinds: &str,
    first_frame_ms: Option<i64>,
    reason: &str,
) {
    use std::collections::BTreeMap;
    let mut fields = BTreeMap::new();
    fields.insert(
        "transaction_id".to_string(),
        serde_json::json!(key.transaction_id),
    );
    fields.insert("generation".to_string(), serde_json::json!(key.generation));
    fields.insert(
        "operation_kind".to_string(),
        serde_json::Value::String(operation_kind.to_string()),
    );
    if let Some((x, y)) = old_caret {
        fields.insert("old_caret_x".to_string(), serde_json::json!(x));
        fields.insert("old_caret_y".to_string(), serde_json::json!(y));
    }
    if let Some((x, y)) = new_caret {
        fields.insert("new_caret_x".to_string(), serde_json::json!(x));
        fields.insert("new_caret_y".to_string(), serde_json::json!(y));
    }
    fields.insert(
        "unit_kinds".to_string(),
        serde_json::Value::String(unit_kinds.to_string()),
    );
    if let Some(t) = first_frame_ms {
        fields.insert("first_frame_ms".to_string(), serde_json::json!(t));
    }
    fields.insert(
        "reason".to_string(),
        serde_json::Value::String(reason.to_string()),
    );

    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        // sequence / session_id 由 writer_diagnostics::record_event 统一补全
        sequence: 0,
        session_id: String::new(),
        level: writer_diagnostics::DiagnosticLevel::Info,
        origin: writer_diagnostics::DiagnosticOrigin::App,
        event: event.to_string(),
        target: "editor.anim".to_string(),
        message: None,
        fields,
    });
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
    cursor_rect_x: qt_property!(f32; READ cursor_rect_x NOTIFY cursor_rect_changed),
    #[allow(dead_code)]
    cursor_rect_y: qt_property!(f32; READ cursor_rect_y NOTIFY cursor_rect_changed),
    #[allow(dead_code)]
    cursor_rect_width: qt_property!(f32; READ cursor_rect_width NOTIFY cursor_rect_changed),
    #[allow(dead_code)]
    cursor_rect_height: qt_property!(f32; READ cursor_rect_height NOTIFY cursor_rect_changed),
    // Issue #727 评论 5757225958 问题2: 删除 visual_cursor_rect_y / visual_cursor_rect_height
    // qt_property，这套只为 QML auto-follow anchor 服务的旧接口已无消费者。
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
    current_content_height: f32,
    content_height_dirty: Cell<bool>,
    current_editor_enabled: bool,
    /// Issue #714: 鼠标拖选状态标志。MouseButtonPress 时置 true，
    /// MouseButtonRelease 时置 false。用于区分拖选和普通点击。
    pointer_drag_selecting: bool,
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
    current_is_loading: bool,
    current_is_applying_format: bool,
    last_summary: QString,
    last_event_count: u32,
    editor_layout: EditorLayout,
    /// 正文/字体/宽度变化时为 true，GUI 线程预计算 snapshot 并置为 false。
    layout_dirty: bool,
    /// 动画裁剪开始/结束时为 true，仅重建 Scene Graph，不重新排版。
    scene_dirty: bool,
    /// Issue #677 评论 5653944889: GUI 线程一次性准备好的不可变帧数据。
    /// 包含同一次排版得到的 `LayoutSnapshot` 和从该 snapshot 派生的选区/preedit 几何。
    /// 不变性：
    /// - 只在 GUI 线程上由 `prepare_editor_frame()` 构造并一次性替换。
    /// - render thread（`update_paint_node()`）只读，不调用任何排版方法。
    /// - `None` 表示需要 GUI 侧重新准备，render thread 跳过静态正文渲染。
    prepared_frame: Option<render_plan::PreparedEditorFrame>,
    cursor_ctrl: cursor_controller::CursorController,
    /// Issue #690 评论 5675007226 步骤 1: render thread 上次采样的帧时间。
    /// 供 `tick_cursor_animation()` 在 GUI 线程使用，确保光标和文字 progress
    /// 来自同一个 `frame_now`，不再各自 `Instant::now()`。
    last_frame_now: Option<std::time::Instant>,
    /// Issue #710 评论 5732160521 问题 2: 上一帧 `cursor_blink_suppressed` 状态，
    /// 用于检测 blink 抑制的边沿变化（false->true / true->false），
    /// 在边沿处重置 blink 状态，避免输入/光标动画时光标消失。
    /// - false->true（suppressed 开始）：立即 blink_visible = true（光标从可见状态开始）
    /// - true->false（suppressed 结束）：重置 blink_last_toggle = now / blink_visible = true，
    ///   不继承 suppressed 开始前碰巧为 false 的旧相位
    /// 之前只跟踪 has_active_insert（只认 Insert），CursorOnly Tween 的起止不触发边沿重置。
    /// 现在跟踪 current_cursor_blink_mode() == Suppressed，覆盖 CursorOnly Tween。
    prev_cursor_blink_suppressed: bool,
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
            last_transaction_summary: Default::default(),
            last_animation_event_count: Default::default(),
            scroll_y: Default::default(),
            viewport_height: Default::default(),
            is_scrolling: Default::default(),
            is_loading: Default::default(),
            is_applying_format: Default::default(),
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
            current_content_height: 0.0,
            content_height_dirty: Cell::new(false),
            current_editor_enabled: true,
            pointer_drag_selecting: false,
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
            current_scroll_y: 0.0,
            current_viewport_height: 0.0,
            current_is_scrolling: false,
            current_is_loading: false,
            current_is_applying_format: false,
            last_summary: Default::default(),
            last_event_count: 0,
            editor_layout: EditorLayout::default(),
            layout_dirty: true,
            scene_dirty: true,
            prepared_frame: None,
            cursor_ctrl: cursor_controller::CursorController::new(),
            last_frame_now: None,
            prev_cursor_blink_suppressed: false,
        }
    }
}

impl SujianEditorItem {
    /// GUI 线程上准备不可变静态正文快照，然后请求 Scene Graph 更新。
    ///
    /// Issue #658: 在 GUI/input/layout 阶段先准备好 snapshot，
    /// 再请求 QSG 更新，确保 update_paint_node()（render thread）只消费缓存。
    ///
    /// Issue #677 评论 5653944889: 改为一次性准备 `PreparedEditorFrame`，
    /// 包含同一次排版得到的 `LayoutSnapshot` 和从该 snapshot 派生的选区/preedit 几何。
    /// render thread 不再调用 `build_selection_preedit_plan()` /
    /// `layout_snapshot()`，避免进入排版生命周期。
    pub(crate) fn request_static_repaint(&mut self) {
        self.layout_dirty = true;
        self.scene_dirty = true;
        // Issue #658: 先清除旧 frame，确保后续 prepare 一定重新排版。
        // 不清 frame 时 prepare_editor_frame 会跳过重建，
        // 导致 emit_content_changed / visual_changed 后仍显示旧正文。
        self.prepared_frame = None;
        self.prepare_editor_frame();
        let item = self as &dyn QQuickItem;
        item.update();
    }

    /// GUI 线程上仅请求 Scene Graph 重建（动画裁剪变化等）。
    ///
    /// 不重新排版，只通知 update_paint_node() 重建 QSGNode。
    pub(crate) fn request_scene_rebuild(&mut self) {
        self.scene_dirty = true;
        let item = self as &dyn QQuickItem;
        item.update();
    }

    /// 在 GUI 线程上一次性准备不可变帧数据 `PreparedEditorFrame`。
    ///
    /// Issue #677 评论 5653944889: 这是 render thread 与排版生命周期的唯一边界。
    /// 调用时机：`request_static_repaint()`、`geometry_changed()` 等 GUI 线程路径。
    ///
    /// 流程：
    /// 1. 只排版一次：调用 `self.editor_layout.snapshot(...)` 得到 `LayoutSnapshot`。
    /// 2. 从同一个 snapshot 派生 selection/preedit 几何（不再次排版）。
    /// 3. 构造 `PreparedEditorFrame { layout_snapshot, selection_preedit }`。
    /// 4. 一次性替换 `self.prepared_frame = Some(frame)`。
    /// 5. 不调用 `update()`（由调用方决定）。
    ///
    /// 不变性：此方法只在 GUI 线程调用；render thread（`update_paint_node()`）
    /// 只读 `self.prepared_frame`，不再进入排版生命周期。
    fn prepare_editor_frame(&mut self) {
        if self.prepared_frame.is_some() {
            return;
        }
        let width = self.bounding_width();
        let params = self.layout_params(width);
        let snapshot = self
            .editor_layout
            .snapshot(
                self.pipeline.committed_text(),
                params,
                self.pipeline.text_revision(),
            )
            .clone();
        let selection_preedit = self.build_selection_preedit_plan_from_snapshot(&snapshot);
        self.prepared_frame = Some(render_plan::PreparedEditorFrame {
            layout_snapshot: snapshot,
            selection_preedit,
        });
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
            self.pipeline.set_current_canonical_snapshot(None);
            self.request_scene_rebuild();
            self.cursor_rect_changed();
        }
    }

    /// Issue #715: 章节 Load 的完整视觉状态边界。
    ///
    /// 与 `clear_active_text_animations()` 分开——后者只在动画队列非空时才执行
    /// （`suppress_all()` 返回 true），章节替换时队列已空则整个跳过，仍会遗留
    /// 上一章的 visual snapshot/cache/layout generation。
    ///
    /// 此方法无条件执行，确保切章后下一次真实输入从新章节的 layout/snapshot 起算：
    /// - suppress_all()：停止所有活动动画
    /// - 清 texture_cache：丢弃旧章行纹理
    /// - 清 current/previous layout snapshot：丢弃旧章排版快照
    /// - 清 current_canonical_snapshot：丢弃旧章 canonical 快照
    /// - 清 pending_promoted_layout：丢弃未消费的 promoted layout
    /// - 清 prepared_frame：丢弃旧帧数据
    /// - invalidate editor_layout：清旧排版 generation
    /// - request_scene_rebuild()：触发 Scene Graph 重建
    pub(crate) fn reset_document_visual_state(&mut self) {
        self.pipeline.animation_coordinator_mut().suppress_all();
        self.pipeline.texture_cache_mut().clear();
        self.pipeline.set_current_layout_snapshot(None);
        self.pipeline.set_previous_layout_snapshot(None);
        self.pipeline.set_current_canonical_snapshot(None);
        let _ = self.pipeline.take_pending_promoted_layout();
        self.prepared_frame = None;
        self.editor_layout.invalidate();
        self.request_scene_rebuild();
    }

    pub(crate) fn ime_query_text_before_cursor(&self, max_chars: usize) -> String {
        let text = self.pipeline.committed_text();
        let cursor_char = byte_to_char_index(text, self.pipeline.cursor());
        let before_char_len = cursor_char.min(max_chars);
        text.chars()
            .skip(cursor_char - before_char_len)
            .take(before_char_len)
            .collect()
    }

    pub(crate) fn ime_query_text_after_cursor(&self, max_chars: usize) -> String {
        let text = self.pipeline.committed_text();
        let cursor_char = byte_to_char_index(text, self.pipeline.cursor());
        let total_chars = text.chars().count();
        let after_char_len = total_chars.saturating_sub(cursor_char).min(max_chars);
        text.chars()
            .skip(cursor_char)
            .take(after_char_len)
            .collect()
    }

    pub(crate) fn ime_query_selected_text(&self) -> String {
        self.pipeline.selected_text()
    }

    /// Issue #677 评论 5653944889: 从已有的 `LayoutSnapshot` 派生选区/preedit 几何。
    ///
    /// **关键约束**：此方法不再调用 `self.layout_snapshot(width)`，避免进入排版生命周期
    /// （`EditorLayout::snapshot()` / `begin_layout_generation()` /
    /// `clear_layout_generation()`）。snapshot 由调用方（`prepare_editor_frame()`）
    /// 在 GUI 线程上一次性准备好后传入。
    ///
    /// 只读方法 `self.editor_layout.cursor_x_for_line()` 和
    /// `self.editor_layout.text_width()` 不进入排版生命周期，可以保留。
    ///
    /// Issue #677 评论 5654686856: prepare 阶段不再做视口裁剪。selection/preedit 几何
    /// 只按 `anchor/head` 与 `snapshot.lines` 的 byte range 相交关系生成，所有 `y`
    /// 保持 `line.y` 文档坐标，完整文档坐标 selection geometry 都写入 frame。视口裁剪
    /// 下沉到 renderer 每帧轻量状态（`render_selection_preedit_layer`），这样滚动后
    /// 新进入视口的选区行依然存在于 `selection_ranges` 中，renderer 能正确画出。
    pub(crate) fn build_selection_preedit_plan_from_snapshot(
        &self,
        snapshot: &LayoutSnapshot,
    ) -> render_plan::SelectionPreeditPlan {
        use render_plan::{PreeditRange, SelectionRange};

        let mut plan = render_plan::SelectionPreeditPlan::default();

        if self.pipeline.has_selection() {
            plan.has_selection = true;

            let anchor = self.pipeline.selection_anchor().min(self.pipeline.cursor());
            let head = self.pipeline.selection_anchor().max(self.pipeline.cursor());

            for line in &snapshot.lines {
                if line.para_text.is_empty() {
                    continue;
                }
                if line.byte_end <= anchor || line.byte_start >= head {
                    continue;
                }

                let seg_start = anchor.max(line.byte_start);
                let seg_end = head.min(line.byte_end);
                if seg_start >= seg_end {
                    continue;
                }

                let start_x = self.editor_layout.cursor_x_for_line(
                    snapshot,
                    line,
                    seg_start,
                    crate::editor::layout::CaretAffinity::Downstream,
                );
                let end_x = self.editor_layout.cursor_x_for_line(
                    snapshot,
                    line,
                    seg_end,
                    crate::editor::layout::CaretAffinity::Downstream,
                );
                let left_x = start_x.min(end_x);
                let sel_w = (end_x - start_x).abs();

                plan.selection_ranges.push(SelectionRange {
                    x: left_x,
                    y: line.y,
                    w: sel_w,
                    h: line.height,
                });
            }
        }

        if !self.pipeline.composition().preedit_text.is_empty() {
            plan.has_preedit = true;
            if let Some(ref _preedit_rect) = self.pipeline.composition().preedit_cursor_rect {
                let font_size = f64::from(self.current_font_pixel_size);
                let font_family = &self.current_font_family.to_string();
                let cursor_byte = self.pipeline.cursor();

                if let Some(line) = snapshot
                    .lines
                    .iter()
                    .find(|l| l.byte_end >= cursor_byte && l.byte_start <= cursor_byte)
                {
                    let start_x = self.editor_layout.cursor_x_for_line(
                        snapshot,
                        line,
                        cursor_byte,
                        crate::editor::layout::CaretAffinity::Downstream,
                    );
                    let preedit_w = self.editor_layout.text_width(
                        &self.pipeline.composition().preedit_text,
                        font_size,
                        font_family,
                    );

                    // Issue #677 评论 5654174714: PreeditRange.y 保存文档坐标 line.y，
                    // scroll_y 换算和带透明度颜色都由 renderer 在绘制时计算。
                    plan.preedit_ranges.push(PreeditRange {
                        x: start_x,
                        y: line.y,
                        w: preedit_w,
                        h: line.height,
                        underline: true,
                    });
                }
            }
        }

        plan
    }
}
