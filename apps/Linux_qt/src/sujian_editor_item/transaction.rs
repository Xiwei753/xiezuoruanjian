use super::*;
use crate::sujian_editor_item::pipeline::VisualPrepareOutcome;
use crate::sujian_editor_item::text_motion::TextMotion;
use writer_core::editor::EditorEditResult;

impl SujianEditorItem {
    /// Issue #735: `record_transaction` 接收 `EditorEditResult` 而非从 `EditorEngine` 构造事务。
    ///
    /// Issue #904：单入口。编辑先由 Core 落地，再用 Qt 增量排版得到局部新旧几何，
    /// 最后只为受影响片段建立**一个** `text_motion::TextMotion`。
    /// 不再遍历整章建立 `TargetLayoutDescriptor`，也不再有整屏 ownership 状态机。
    pub(crate) fn record_transaction(
        &mut self,
        old: EditorSnapshot,
        new: EditorSnapshot,
        result: &EditorEditResult,
        emit: bool,
    ) -> VisualPrepareOutcome {
        // Issue #901 第五轮阻塞4：先用 Core new_revision 更新 text_revision，
        // 再排版，禁止 cache 用字节数命中旧布局。
        let core_new_revision = result.new_revision.value();
        self.pipeline.set_text_revision(core_new_revision);

        // Issue #904：编辑前的当前布局即"旧局部图片"的来源。
        let old_layout = self.pipeline.current_layout_snapshot().clone();

        // 从 Core display_patches 提取受影响字节范围（取旧/新坐标的并集仅用于
        // 定位受影响段落；动画侧仍分别使用旧/新坐标，不混用）。
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
        } else if affected_start == usize::MAX {
            None
        } else {
            Some((affected_start, affected_start + 1))
        };

        let width = self.bounding_width();

        // Issue #904：先增量排版得到新布局，再据此建立唯一的局部动画。
        let new_snapshot = match self.build_incremental_layout_snapshot(width, affected_range) {
            Ok(snapshot) => Some(snapshot),
            Err(_err) => self.build_editor_layout_snapshot(width, false, None).ok(),
        };

        let animations_requested = self.effective_animation().text;
        let suppressed =
            self.current_is_scrolling || self.current_is_loading || self.current_is_applying_format;

        // 旧侧局部范围（base 坐标）：被删除/被移位的旧字也要有行纹理，
        // 否则 required_textures 缺一张就会整体不接管、整屏静态重建。
        let mut old_start = usize::MAX;
        let mut old_end = 0usize;
        for patch in &result.display_patches {
            old_start = old_start.min(patch.replace_byte_range.start().value());
            old_end = old_end.max(patch.replace_byte_range.end().value());
        }
        let old_range = if old_start < old_end {
            Some((old_start, old_end))
        } else if old_start == usize::MAX {
            None
        } else {
            Some((old_start, old_start + 1))
        };

        let mut motion: Option<TextMotion> = None;
        if let Some(new_snap) = new_snapshot.as_ref() {
            // 只为局部变化行补齐行纹理，不再遍历整章。
            let tex_range = affected_range.unwrap_or((0, 1));
            self.pipeline
                .prepare_local_text_textures(new_snap, tex_range);
            if let Some(old_snap) = old_layout.as_ref() {
                if let Some(old_range) = old_range {
                    self.pipeline
                        .prepare_local_text_textures(old_snap, old_range);
                }
            }

            if animations_requested && !suppressed {
                let now_ms = Self::frame_now_ms(&self.last_frame_now);
                let session = self.document_session;
                let viewport = crate::sujian_editor_item::layout_snapshot::SourceRect {
                    x: 0.0,
                    y: f64::from(self.current_scroll_y),
                    w: self.bounding_width(),
                    h: f64::from(self.current_viewport_height.max(1.0)),
                };
                let duration_ms = self.current_typing_animation_duration_ms;
                // Issue #904 评论 3: 从"上一次真正画出的局部图片"重定向，
                // 不从计划建立时的静态起点重来。
                let last_drawn = self.last_drawn_local_frame.as_ref();
                motion = TextMotion::begin_or_retarget(
                    result,
                    old_layout.as_ref(),
                    new_snap,
                    last_drawn,
                    now_ms,
                    duration_ms,
                    session,
                    viewport,
                );
            }
        }

        if let Some(new_snap) = new_snapshot {
            self.pipeline.set_current_layout_snapshot(Some(new_snap));
        }
        *self.pipeline.text_motion_mut() = motion;
        self.request_scene_rebuild();

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
            VisualPrepareOutcome::Skipped(super::edit_flow::EditVisualSkipReason::LoadingSuppressed)
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
        let _ = (&old, &new);
        if emit {
            self.transaction_created();
        }
        outcome
    }

    /// Issue #904：把 `Instant` 转换为毫秒时间戳，供 `text_motion` 使用。
    /// 用从进程启动起的相对毫秒数——只关心时间差。
    pub(crate) fn frame_now_ms(last_frame_now: &Option<std::time::Instant>) -> f64 {
        use std::sync::OnceLock;
        static REFERENCE: OnceLock<std::time::Instant> = OnceLock::new();
        let reference = REFERENCE.get_or_init(std::time::Instant::now);
        let now = last_frame_now.unwrap_or_else(std::time::Instant::now);
        now.duration_since(*reference).as_millis() as f64
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
