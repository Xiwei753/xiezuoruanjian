use super::*;
use crate::sujian_editor_item::pipeline::LocalTextTexturePrep;
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

        // Issue #904 评论 4：先建立唯一的局部动画计划，再按计划真正引用的源快照 id
        // 准备纹理；不再用编辑字节范围预判几行，也不在计划之前无差别补图。
        let mut motion: Option<TextMotion> = None;
        if let Some(new_snap) = new_snapshot.as_ref() {
            let session = self.document_session;
            // Issue #904 评论 3: 有上次真正画出的局部图片（同一 session）才算重定向。
            let retargeting = self
                .last_drawn_local_frame
                .as_ref()
                .map_or(false, |frame| frame.session == session);

            if animations_requested && !suppressed {
                let now_ms = Self::frame_now_ms(&self.last_frame_now);
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

            if let Some(m) = motion.as_ref() {
                // 计划的源纹理：只用 TextMotion 真正引用的旧/新源行 id，新旧两侧
                // 都尝试准备（函数会忽略本快照里不存在的 id）。缺任一纹理必须显式
                // 记入 missing，不能静默降级让 renderer 误以为"这次没有动画"。
                let required = m.required_textures.clone();
                let new_prep = self.prepare_local_text_textures(new_snap, &required);
                let old_prep = old_layout
                    .as_ref()
                    .map(|old_snap| self.prepare_local_text_textures(old_snap, &required));
                let prep = match old_prep.as_ref() {
                    Some(op) => LocalTextTexturePrep::merge_sides(&required, &[&new_prep, op]),
                    None => LocalTextTexturePrep::merge_sides(&required, &[&new_prep]),
                };
                let missing_ids = if prep.missing.is_empty() {
                    String::new()
                } else {
                    format!(
                        ", missing_ids=[{}]",
                        prep.missing
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                };
                editor_animation_debug_log(&format!(
                    "record_transaction: local_animation kind={}, pieces={}, required={}, prepared={}, missing={}, ready={}{}",
                    if retargeting { "retarget" } else { "create" },
                    m.pieces.len(),
                    required.len(),
                    prep.prepared.len(),
                    prep.missing.len(),
                    prep.ready(),
                    missing_ids,
                ));
            }
        }

        let has_motion = motion.is_some();
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
        } else if has_motion {
            VisualPrepareOutcome::Created
        } else {
            // Issue #904 复核: 没有真正建立局部动画（begin_or_retarget 返回 None）
            // 不能报 Created，否则诊断会把"根本没有动画"记成成功。
            VisualPrepareOutcome::Skipped(
                super::edit_flow::EditVisualSkipReason::BuilderEmptyTransaction,
            )
        };

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
