use super::*;
use crate::sujian_editor_item::pipeline::VisualPrepareOutcome;
use crate::sujian_editor_item::visual_canvas::{
    CanvasRect, CanvasVec2, TargetCaret, TargetGlyphRun, TargetLayoutDescriptor, TargetScroll,
};
use writer_core::editor::EditorEditResult;

impl SujianEditorItem {
    /// Issue #735: `record_transaction` 接收 `EditorEditResult` 而非从 `EditorEngine` 构造事务。
    ///
    /// Core 已删除 `EditorEngine`、`EditorVisualTransaction`。平台端从
    /// `EditorEditResult`（含 `cause`、`operation_kind`、`offset_map`、`content_delta`）
    /// 直接派生动画策略，不再经过 Core 的视觉事务工厂。
    ///
    /// Issue #901：编辑结果统一交给 `visual_canvas::VisualCanvas::apply_edit_result`，
    /// 旧 `prepare_edit_motion` / `animation_coordinator` 已删除。
    ///
    /// Issue #901 阻塞 1：除了调用 `apply_edit_result`，还从当前 Qt LayoutSnapshot
    /// 产生 `TargetLayoutDescriptor` 并调用 `visual_canvas.apply_target_layout`，
    /// 让 `CanvasController` 的 `target_layout` 不再始终为 `None`。
    pub(crate) fn record_transaction(
        &mut self,
        old: EditorSnapshot,
        new: EditorSnapshot,
        result: &EditorEditResult,
        emit: bool,
    ) -> VisualPrepareOutcome {
        // Issue #901: 编辑结果收口到 visual_canvas，由 RevisionChain 精确消费 offset map。
        self.pipeline.visual_canvas_mut().apply_edit_result(result);

        // Issue #901 阻塞 1：从当前 Qt LayoutSnapshot 产生 TargetLayoutDescriptor
        // 并调用 apply_target_layout，让 CanvasController 的 target_layout 不再为 None。
        // 这样 SceneTransition::build_draw_plan 才能从 presented + target 生成绘制计划。
        let now_ms = Self::frame_now_ms(&self.last_frame_now);
        if let Some(target) = self.build_target_layout_descriptor() {
            self.pipeline
                .visual_canvas_mut()
                .apply_target_layout(target, now_ms);
        }

        let animations_requested = super::animation::any_animation_enabled(
            self.current_typing_animation_enabled,
            self.current_smooth_cursor_enabled,
            self.current_coordinated_animation_enabled,
        );

        // Issue #815 评论 6042062633 修改 8: 滚动/加载/套用格式期间抑制动画是显式规则，
        // 但必须留下正式事件，否则诊断包里"编辑发生了却没有动画"没有任何线索。
        let outcome = if !animations_requested {
            VisualPrepareOutcome::AnimationDisabled
        } else if self.current_is_scrolling {
            editor_animation_transaction_skipped_event(&AnimationSkipFields {
                cause: "suppressed_by_scrolling",
                operation_kind: editor_operation_kind_label(result.operation_kind),
                typing_animation_enabled: self.current_typing_animation_enabled,
                smooth_cursor_enabled: self.current_smooth_cursor_enabled,
                coordinated_animation_enabled: self.current_coordinated_animation_enabled,
                old_caret_present: false,
                new_caret_present: false,
                inserted_range: None,
                unit_kinds: "",
                cursor_track_present: false,
                is_scrolling: true,
                is_loading: self.current_is_loading,
                is_applying_format: self.current_is_applying_format,
                transaction_id: None,
                generation: 0,
            });
            VisualPrepareOutcome::Skipped(
                super::edit_flow::EditVisualSkipReason::ScrollingSuppressed,
            )
        } else if self.current_is_loading {
            VisualPrepareOutcome::Skipped(
                super::edit_flow::EditVisualSkipReason::LoadingSuppressed,
            )
        } else if self.current_is_applying_format {
            VisualPrepareOutcome::Skipped(
                super::edit_flow::EditVisualSkipReason::FormatApplyingSuppressed,
            )
        } else {
            VisualPrepareOutcome::Created
        };

        let has_motion = matches!(outcome, VisualPrepareOutcome::Created);
        self.last_event_count = if has_motion { 1 } else { 0 };
        self.last_summary = format!(
            "cause={:?};op={:?};motion={};delta_inserted={};delta_deleted={}",
            result.cause,
            result.operation_kind,
            has_motion,
            result.content_delta.inserted_chars,
            result.content_delta.deleted_chars,
        )
        .into();
        editor_animation_debug_log(&format!(
            "record_transaction: cause={:?}, op={:?}, motion={}, typing_anim_enabled={}, is_scrolling={}",
            result.cause,
            result.operation_kind,
            has_motion,
            self.current_typing_animation_enabled,
            self.current_is_scrolling,
        ));
        // old/new 在 Issue #901 后不再用于构建 motion，但保留比较以避免 unused warning。
        let _ = (&old, &new);
        if emit {
            self.transaction_created();
        }
        outcome
    }

    /// Issue #901 阻塞 1：从当前 Qt LayoutSnapshot 构造 `TargetLayoutDescriptor`。
    ///
    /// 从 `EditorLayoutSnapshot` 的 `line_snapshots` 转换为 `TargetGlyphRun` 列表
    ///（每段连续文字一个 run，包含 texture_id、source_uv、document_rect、byte_range），
    /// 从 `caret_rect_doc` 构造 `TargetCaret`，从当前 scroll_y 构造 `TargetScroll`，
    /// 从当前 viewport 构造 `CanvasRect` clip。
    ///
    /// 如果当前没有 layout snapshot，返回 `None`——此时不更新 target_layout，
    /// `CanvasController` 保持上一份 target。
    pub(crate) fn build_target_layout_descriptor(&self) -> Option<TargetLayoutDescriptor> {
        let snapshot = self.pipeline.current_layout_snapshot().as_ref()?;

        // 从 line_snapshots 转换为 TargetGlyphRun 列表。
        // 每个 PreparedLineSnapshot 对应一个 glyph run。
        let glyph_runs: Vec<TargetGlyphRun> = snapshot
            .line_snapshots
            .iter()
            .map(|line| {
                let texture_id = line.id.to_cache_key();
                // source_uv：行视觉资源内的局部裁剪区域。
                // 从 clusters 的 source_rect 合并出整行的 UV。
                let source_uv = if line.clusters.is_empty() {
                    CanvasRect::new(0.0, 0.0, 0.0, 0.0)
                } else {
                    let min_x = line
                        .clusters
                        .iter()
                        .map(|c| c.source_rect.x)
                        .fold(f64::MAX, f64::min);
                    let min_y = line
                        .clusters
                        .iter()
                        .map(|c| c.source_rect.y)
                        .fold(f64::MAX, f64::min);
                    let max_right = line
                        .clusters
                        .iter()
                        .map(|c| c.source_rect.x + c.source_rect.w)
                        .fold(f64::MIN, f64::max);
                    let max_bottom = line
                        .clusters
                        .iter()
                        .map(|c| c.source_rect.y + c.source_rect.h)
                        .fold(f64::MIN, f64::max);
                    CanvasRect::new(
                        if min_x.is_finite() { min_x } else { 0.0 },
                        if min_y.is_finite() { min_y } else { 0.0 },
                        if max_right.is_finite() {
                            (max_right - min_x).max(0.0)
                        } else {
                            0.0
                        },
                        if max_bottom.is_finite() {
                            (max_bottom - min_y).max(0.0)
                        } else {
                            0.0
                        },
                    )
                };
                // document_rect：行在文档坐标系中的矩形。
                let document_rect = CanvasRect::new(
                    line.visual_x,
                    line.visual_line_top,
                    (line.visual_line_bottom - line.visual_line_top).max(0.0),
                    (line.visual_line_bottom - line.visual_line_top).max(0.0),
                );
                TargetGlyphRun {
                    texture_id,
                    source_uv,
                    document_rect,
                    byte_range: (line.byte_start, line.byte_end),
                }
            })
            .collect();

        // 从 caret_rect_doc 构造 TargetCaret。
        let caret = snapshot.caret_rect_doc.as_ref().map(|c| TargetCaret {
            position: CanvasVec2::new(c.x, c.y),
            visible: c.visible,
            width: 2.0,
            height: c.h,
        });

        // 从当前 scroll_y 构造 TargetScroll。
        let scroll = TargetScroll {
            x: 0.0,
            y: f64::from(self.current_scroll_y),
        };

        // 从当前 viewport 构造 CanvasRect clip。
        let clip = CanvasRect::new(
            0.0,
            0.0,
            self.bounding_width(),
            f64::from(self.current_viewport_height.max(1.0)),
        );

        Some(TargetLayoutDescriptor {
            layout_revision: snapshot.revision,
            glyph_runs,
            caret,
            scroll,
            clip,
        })
    }

    /// Issue #901 阻塞 1：把 `Instant` 转换为毫秒时间戳，供 `visual_canvas` 使用。
    /// 如果没有上一帧时间，用当前 `Instant::now()`。
    /// 用从进程启动起的相对毫秒数——`visual_canvas` 只关心时间差，不关心绝对值。
    pub(crate) fn frame_now_ms(last_frame_now: &Option<std::time::Instant>) -> f64 {
        use std::sync::OnceLock;
        static REFERENCE: OnceLock<std::time::Instant> = OnceLock::new();
        let reference = REFERENCE.get_or_init(std::time::Instant::now);
        let now = last_frame_now.unwrap_or_else(std::time::Instant::now);
        now.duration_since(*reference).as_millis() as f64
    }

    /// 为当前编辑准备行纹理，并请求 Scene Graph 处理 ownership 更新。
    pub(crate) fn prepare_visual_edit_textures(
        &mut self,
        base_snapshot: &super::layout_snapshot::EditorLayoutSnapshot,
    ) {
        self.pipeline.prepare_visual_edit_textures(base_snapshot);
        self.request_scene_rebuild();
    }
}

/// Issue #815 评论 6042062633 修改 8: Core 编辑操作类型 -> 动画诊断里的 operation_kind 短名。
/// Issue #819 评论 5968931455 问题 3: 改成 pub(crate) 供 edit_flow.rs 复用。
pub(crate) fn editor_operation_kind_label(
    kind: writer_core::editor::EditorOperationKind,
) -> &'static str {
    match kind {
        writer_core::editor::EditorOperationKind::Insert => "Insert",
        writer_core::editor::EditorOperationKind::Delete => "Delete",
        writer_core::editor::EditorOperationKind::Replace => "Replace",
        writer_core::editor::EditorOperationKind::CursorOnly => "CursorOnly",
        writer_core::editor::EditorOperationKind::CompositionUpdate => "CompositionUpdate",
        writer_core::editor::EditorOperationKind::CompositionCommit => "CompositionCommit",
        writer_core::editor::EditorOperationKind::CompositionCancel => "CompositionCancel",
        writer_core::editor::EditorOperationKind::Load => "Load",
        writer_core::editor::EditorOperationKind::Format => "Format",
    }
}
