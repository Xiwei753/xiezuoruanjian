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

        // Issue #901 问题4：从 Core display_patches 提取受影响字节范围，
        // 传给 EditorLayout 增量排版路径，只重排受影响段落。
        // replace_byte_range 是旧坐标，inserted_byte_range 是新坐标，
        // 取两者并集用于定位受影响段落（近似足够准确）。
        let mut affected_start = usize::MAX;
        let mut affected_end = 0usize;
        for patch in &result.display_patches {
            let rep_start = patch.replace_byte_range.start().value();
            let rep_end = patch.replace_byte_range.end().value();
            let ins_start = patch.inserted_byte_range.start().value();
            let ins_end = patch.inserted_byte_range.end().value();
            affected_start = affected_start.min(rep_start).min(ins_start);
            affected_end = affected_end.max(rep_end).max(ins_end);
        }
        let affected_range = if affected_start < affected_end {
            Some((affected_start, affected_end))
        } else {
            None
        };

        // Issue #901 第五轮阻塞4：在建立增量排版目标**之前**，先用 Core 的
        // new_revision 更新 pipeline 的 text_revision，使 EditorLayout::snapshot
        // 收到的 revision 一定和 cache 中的不同（cache 中是旧 revision）。
        // 禁止等字节长度替换靠 text_ptr + text_len 判断命中 cache 直接返回旧布局，
        // 导致中段同宽替换/IME 替换的光标、字形和正文版本脱节。
        // 同一次编辑 Core result → EditorLayoutSnapshot → TargetLayoutDescriptor
        // → PreparedEditorFrame 必须共享同一个已确定的 revision；
        // emit_content_changed 只发 UI 通知，不再负责事后才给正文排版增加版本。
        let core_new_revision = result.new_revision.value();
        self.pipeline.set_text_revision(core_new_revision);

        // Issue #901 第五轮阻塞5：不 clone 整份 current_layout_snapshot，
        // 只比较必要的版本/参数元数据。needs_new_layout 判断改为正常版本判断：
        // - current_layout_snapshot 为 None → 需要排版（首次/重置后）
        // - current_layout_snapshot 为 Some 但 line_snapshots 为空 → 需要重排
        // - 其他 → 走增量路径（EditorLayout::snapshot 内部判断 revision 变化）
        // 增量排版路径（build_incremental_layout_snapshot）内部会判断
        // paragraph_store 是否为空，为空时走全篇重排，非空时走增量重排。
        let width = self.bounding_width();
        let needs_new_layout = {
            match self.pipeline.current_layout_snapshot() {
                None => true,
                Some(snap) => snap.line_snapshots.is_empty(),
            }
        };

        // Issue #901 问题4：走增量排版路径。
        // build_incremental_layout_snapshot 内部调 EditorLayout::snapshot()，
        // 只重排受影响段落，不调用全篇 prepare_document_visual_snapshot_scoped。
        // 同时为受影响行提取 QImage 纹理，闭环"普通输入也产生纹理"。
        // Issue #901 第五轮阻塞5：去掉无意义的 `needs_new_layout || is_some()`
        // 条件（原条件恒为 true），改为显式判断 needs_new_layout。
        // needs_new_layout 为 true 时仍走增量路径——其内部 paragraph_store
        // 为空会自动 fallback 到全篇重排，不需要外部分支。
        let _ = needs_new_layout; // 保留版本判断语义供诊断/未来扩展
        match self.build_incremental_layout_snapshot(width, affected_range) {
            Ok(new_snap) => {
                // Issue #901 问题4：把新 snapshot 通过 prepare_visual_edit_textures
                // 注入 texture_cache，使渲染层能找到受影响行的纹理。
                self.prepare_visual_edit_textures(&new_snap);
                self.pipeline
                    .set_current_layout_snapshot(Some(new_snap));
            }
            Err(_err) => {
                // 增量排版失败时 fallback 到全篇排版。
                // build_editor_layout_snapshot 内部已记诊断，
                // current_layout_snapshot 保持旧值——不伪装成功。
                if let Ok(new_snap) = self.build_editor_layout_snapshot(width, false, None) {
                    self.prepare_visual_edit_textures(&new_snap);
                    self.pipeline
                        .set_current_layout_snapshot(Some(new_snap));
                }
            }
        }

        // Issue #901 阻塞 1：从当前 Qt LayoutSnapshot 产生 TargetLayoutDescriptor
        // 并调用 apply_target_layout，让 CanvasController 的 target_layout 不再为 None。
        // 这样 SceneTransition::build_draw_plan 才能从 presented + target 生成绘制计划。
        // Issue #901 问题2：此时 current_layout_snapshot 已是本次 Core edit 后的新布局。
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

        // Issue #901 问题4：基于 cluster 分割而非整行一个 TargetGlyphRun。
        // 每个 cluster 产生一个 TargetGlyphRun，携带完整 LineSnapshotId。
        // document_rect 的 width 从 cluster 的真实字形范围计算（source_rect.w / dpr），
        // height 是行高（visual_line_bottom - visual_line_top）。
        // byte_range 使用 cluster 的字节范围，而非整行的 (byte_start, byte_end)。
        // 这样 SceneTransition 能精确判断每个 cluster 的存活/插入/删除身份。
        let glyph_runs: Vec<TargetGlyphRun> = snapshot
            .line_snapshots
            .iter()
            .flat_map(|line| {
                let line_height = (line.visual_line_bottom - line.visual_line_top).max(0.0);
                // source_rect 是纹理局部坐标（已乘 DPR），转回文档逻辑坐标需除以 dpr。
                let dpr = if line.dpr > 0.0 { line.dpr } else { 1.0 };

                if line.clusters.is_empty() {
                    // cluster 信息不足时 fallback：整行一个 run。
                    // width=0 表示无可见字形范围（不产生绘制命令），但 byte_range
                    // 仍正确，find_matching_target_run 可匹配存活判断。
                    vec![TargetGlyphRun {
                        snapshot_id: line.id,
                        source_uv: CanvasRect::zero(),
                        document_rect: CanvasRect::new(
                            line.visual_x,
                            line.visual_line_top,
                            0.0,
                            line_height,
                        ),
                        byte_range: (line.byte_start, line.byte_end),
                    }]
                } else {
                    // 按 cluster 分割：每个 cluster 一个 TargetGlyphRun。
                    line.clusters
                        .iter()
                        .map(|cluster| {
                            let doc_x = line.visual_x + cluster.source_rect.x / dpr;
                            let doc_w = (cluster.source_rect.w / dpr).max(0.0);
                            TargetGlyphRun {
                                // Issue #901 问题2：携带完整 LineSnapshotId，不转 u64 hash。
                                snapshot_id: line.id,
                                source_uv: CanvasRect::new(
                                    cluster.source_rect.x,
                                    cluster.source_rect.y,
                                    cluster.source_rect.w,
                                    cluster.source_rect.h,
                                ),
                                document_rect: CanvasRect::new(
                                    doc_x,
                                    line.visual_line_top,
                                    doc_w,
                                    line_height,
                                ),
                                byte_range: (cluster.byte_start, cluster.byte_end),
                            }
                        })
                        .collect()
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
