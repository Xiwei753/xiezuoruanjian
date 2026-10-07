//! Issue #819 评论 5956495850 第 1 节：正文修改主链统一入口。
//!
//! 把正文修改主链从 `editing.rs` / `transaction.rs` 抽出来。所有普通输入、删除、
//! IME commit/replace 都收口到 [`SujianEditorItem::apply_edit_with_visuals`] 这一个入口。
//!
//! 顺序固定：
//! 1. 保存 old snapshot
//! 2. `ensure_current_canonical_before_edit`
//! 3. 调 Core edit command（由 [`EditOp`] 描述）
//! 4. 记录 `editor.edit.applied`
//! 5. 保存 new snapshot
//! 6. 动画开启时调视觉流水线
//! 7. 明确返回 `Created` / `Skipped(reason)` / `AnimationDisabled`
//! 8. 发 content/cursor/selection changed（由调用方决定）
//!
//! 硬约束：Core Applied + animations_requested 时不允许返回裸 `None`——
//! 必须有明确的 `Created` 或 `Skipped`。

use super::*;

/// Issue #701 评论 5699573227 第三阶段: 统一编辑操作描述。
///
/// `apply_edit_with_visuals` 内部根据此枚举执行一次 pipeline edit command。
/// 所有普通输入、删除、IME commit/replace 都收口到这同一个入口，
/// 不再各自直接调 `pipeline.insert_text` / `pipeline.replace_range` /
/// `pipeline.delete_range`。
///
/// `pipeline_cause` 是传给 Core pipeline 的事务分类（用于 undo/redo 栈语义），
/// 与 `apply_edit_with_visuals` 的 `visual_cause`（用于视觉事务分类）分离。
/// 多数场景两者相同，但 `clipboard_paste` 走 `insert_text_with_cause` 时
/// `pipeline_cause` 仍是 `Typing`/`TypingCommit`，`visual_cause` 是 `Paste`。
pub(crate) enum EditOp {
    Insert {
        cursor: usize,
        text: String,
        pipeline_cause: EditorTransactionCause,
    },
    Replace {
        start: usize,
        end: usize,
        text: String,
        pipeline_cause: EditorTransactionCause,
    },
    Delete {
        start: usize,
        end: usize,
        pipeline_cause: EditorTransactionCause,
    },
    /// Issue #701 评论 5702675971: IME commit 的 Qt 两步语义。
    ///
    /// 调用一次 Core `ImeCommit` 原子命令（三段语义），在 Core 内部顺序执行两步
    /// 正文修改，只产生一个 Core revision 推进和一个 UndoEntry：
    /// 1. 第一步：删 selection（在 committed text 上），
    ///    `selection_byte_range` 为 `None` 或零长度时跳过（传 (0, 0)）；
    /// 2. 第二步：在删 selection 后的文本（base_text）上做 replacement/insert，
    ///    `replacement_byte_range` 是 base_text 坐标。
    ImeCommit {
        selection_byte_range: Option<(usize, usize)>,
        replacement_byte_range: (usize, usize),
        inserted_text: String,
        pipeline_cause: EditorTransactionCause,
    },
}

/// Issue #701 评论 5699573227 第三阶段: IME composition commit 参数。
///
/// 仅在 `apply_edit_with_visuals` 处理 composition commit/replace 时提供。
/// 普通输入/删除传 `None`，走 `record_transaction` 路径。
/// 带 `Some` 时走 `record_composition_commit_transaction` 路径，处理 preedit
/// 区间收进、candidate 揭示、committed replace range、pending preedit cursor rect
/// 作为 old caret 起点等 composition 专属语义。
///
/// Composition 与普通编辑最终都使用同一个 VisualEditState；preedit 自身不进入正文过渡。
pub(crate) struct CompositionCommitParams {
    pub preedit_byte_start: usize,
    pub preedit_byte_end: usize,
    pub saved_virtual_text: String,
    pub candidate_byte_start: usize,
    pub candidate_byte_end: usize,
    pub cancel_reason: &'static str,
    pub summary_tag: &'static str,
}

/// Issue #819 评论 5956495850 第 1 节：视觉事务跳过的具体原因。
///
/// `apply_edit_with_visuals` 在 Core edit 已应用但视觉事务未创建时，
/// 必须返回明确的跳过原因，不再让调用方拿裸 `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EditVisualSkipReason {
    /// 滚动期间抑制动画（`animations_requested && is_scrolling`）。
    ScrollingSuppressed,
    /// 加载期间抑制动画（`animations_requested && is_loading`）。
    LoadingSuppressed,
    /// 套用格式期间抑制动画（`animations_requested && is_applying_format`）。
    FormatApplyingSuppressed,
    /// `prepare_edit_motion` 内部检测到 stale canonical / canonical invariant。
    StaleCanonical,
    /// builder 跳过了事务创建（空事务 / 无可视单元）。
    BuilderEmptyTransaction,
    /// composition commit 路径的 old/new snapshot 构造失败或不可用。
    CompositionCommitSnapshotUnavailable,
}

/// Issue #819 评论 5956495850 第 1 节：视觉事务的明确结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EditVisualOutcome {
    /// 正文由单一 VisualEditState 创建了视觉过渡。
    ///
    /// 新模型没有 prepared transaction 队列，所以不再携带事务 key。
    Created,
    /// Core edit 已应用但视觉事务未创建，附带具体跳过原因。
    Skipped(EditVisualSkipReason),
    /// 动画未请求（三个开关全关），Core edit 已应用但无视觉事务。
    AnimationDisabled,
}

/// Issue #819 评论 5956495850 第 1 节：`apply_edit_with_visuals` 的完整返回值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditApplyOutcome {
    /// Core edit 是否已应用。`false` 表示 pipeline edit 未应用（如空删除范围）。
    pub applied: bool,
    /// 视觉事务结果。`applied == false` 时为 `AnimationDisabled`（无编辑无动画）。
    pub visual: EditVisualOutcome,
}

impl SujianEditorItem {
    /// Issue #819 评论 5956495850 第 1 节：正文修改主链唯一入口。
    ///
    /// `insert_text_with_cause` / `delete_backward` / `delete_forward` /
    /// `ime_replace_and_insert` / `delete_selection` 全部收口到这一个 helper。
    /// 固定顺序：
    /// 1. 保存 old text/selection/caret（`self.pipeline.snapshot()`）；
    /// 2. `ensure_current_canonical_before_edit`；
    /// 3. 调一次 pipeline edit command（由 `op` 描述）；
    /// 4. 记录 `editor.edit.applied`；
    /// 5. 读取 new text/selection/caret；
    /// 6. 动画开启时调视觉流水线；
    /// 7. 明确返回 `Created` / `Skipped(reason)` / `AnimationDisabled`。
    ///
    /// - `composition` 不带 composition commit 参数（`None`）时走
    ///   `record_transaction`，由 `pipeline.prepare_edit_motion` 内部
    ///   排版 old/new 并 `process_transaction`。
    /// - 带 `Some` 时走 `record_composition_commit_transaction`，
    ///   处理 preedit 区间收进、candidate 提示、committed replace range 等
    ///   composition 专属语义。
    ///
    /// 硬约束：Core Applied + animations_requested 时不允许返回裸 `None`——
    /// 必须有明确的 `Created` 或 `Skipped`。
    pub(crate) fn apply_edit_with_visuals(
        &mut self,
        op: EditOp,
        visual_cause: EditorTransactionCause,
        composition: Option<CompositionCommitParams>,
    ) -> EditApplyOutcome {
        // 1. 保存 old snapshot
        let old = self.pipeline.snapshot();

        // 2. 在 Core edit command 之前保证 old/current canonical 已建立且属于当前 text revision。
        self.ensure_current_canonical_before_edit();

        // 3. 调 Core edit command
        // Issue #819 评论 5967250411 问题 3：pipeline edit 方法现在返回 PipelineEditOutcome，
        // 保留 Core EditorEditOutcome 的完整分类。只有 Applied / AppliedWithAdjustedSelection
        // 才算真正应用了编辑，NoChange/StaleRevision/InvalidOffset/InvalidRange 都是 NotApplied。
        // 不再用 `is_some()` 判 Applied——那会把 NoChange/StaleRevision 误判成 Applied。

        // 先从 &op 提取诊断标签（op 后面会被 match 消费）。
        // Issue #819 评论 5968240881 问题 3：不再从 &op 提取 inserted_range/deleted_range，
        // 改成从 EditorEditResult.display_patches 提取（见下方 Applied 路径）。
        let op_kind_label = match &op {
            EditOp::Insert { .. } => "Insert",
            EditOp::Replace { .. } => "Replace",
            EditOp::Delete { .. } => "Delete",
            EditOp::ImeCommit { .. } => "ImeCommit",
        };

        let edit_outcome: super::pipeline::PipelineEditOutcome = match op {
            EditOp::Insert {
                cursor,
                text,
                pipeline_cause,
            } => self.pipeline.insert_text(cursor, &text, pipeline_cause),
            EditOp::Replace {
                start,
                end,
                text,
                pipeline_cause,
            } => self
                .pipeline
                .replace_range(start, end, &text, pipeline_cause),
            EditOp::Delete {
                start,
                end,
                pipeline_cause,
            } => self.pipeline.delete_range(start, end, pipeline_cause),
            EditOp::ImeCommit {
                selection_byte_range,
                replacement_byte_range,
                inserted_text,
                pipeline_cause,
            } => {
                let (sel_start, sel_end) = selection_byte_range.unwrap_or((0, 0));
                let (rep_start, rep_end) = replacement_byte_range;
                self.pipeline.ime_commit(
                    sel_start,
                    sel_end,
                    rep_start,
                    rep_end,
                    &inserted_text,
                    pipeline_cause,
                )
            }
        };

        // Issue #819 评论 5967250411 问题 3：只有 PipelineEditOutcome::Applied 才进视觉流水线。
        // NotApplied 时不进视觉流水线，返回 applied: false。
        let (applied, edit_result): (bool, Option<&writer_core::editor::EditorEditResult>) =
            match &edit_outcome {
                super::pipeline::PipelineEditOutcome::Applied(result) => (true, Some(result)),
                super::pipeline::PipelineEditOutcome::NotApplied { kind, result } => {
                    // Issue #819 评论 5968240881 问题 3：NotApplied 单独记 editor.edit.not_applied 事件，
                    // 不再混进 editor.edit.applied。old_revision 用 Core base_revision 而非平台
                    // text_revision()——后者在 NotApplied 时没有推进，拿它当 old_revision 会和
                    // Core 的 base_revision 不一致。
                    let kind_label = match kind {
                        super::pipeline::PipelineEditOutcomeKind::NoChange => "NoChange",
                        super::pipeline::PipelineEditOutcomeKind::StaleRevision => "StaleRevision",
                        super::pipeline::PipelineEditOutcomeKind::InvalidOffset => "InvalidOffset",
                        super::pipeline::PipelineEditOutcomeKind::InvalidRange => "InvalidRange",
                    };
                    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
                        timestamp_ms: chrono::Utc::now().timestamp_millis(),
                        sequence: 0,
                        session_id: String::new(),
                        level: writer_diagnostics::DiagnosticLevel::Info,
                        origin: writer_diagnostics::DiagnosticOrigin::App,
                        event: "editor.edit.not_applied".to_string(),
                        target: "editor.edit".to_string(),
                        message: None,
                        fields: {
                            let mut f = std::collections::BTreeMap::new();
                            f.insert(
                                "not_applied_kind".to_string(),
                                serde_json::json!(kind_label),
                            );
                            f.insert(
                                "operation_kind".to_string(),
                                serde_json::json!(op_kind_label),
                            );
                            f.insert(
                                "visual_cause".to_string(),
                                serde_json::json!(format!("{:?}", visual_cause)),
                            );
                            f.insert(
                                "base_revision".to_string(),
                                serde_json::json!(result.base_revision.value()),
                            );
                            f.insert(
                                "new_revision".to_string(),
                                serde_json::json!(result.new_revision.value()),
                            );
                            f
                        },
                    });
                    (false, None)
                }
            };
        if !applied {
            // Core edit 未应用（NoChange/StaleRevision/InvalidOffset/InvalidRange），无编辑无动画。
            return EditApplyOutcome {
                applied: false,
                visual: EditVisualOutcome::AnimationDisabled,
            };
        }

        // 4. 记录 editor.edit.applied 诊断事件（Applied 路径）。
        // Issue #819 评论 5968240881 问题 3：诊断字段全部来自 Core EditorEditResult，
        // 不再从平台 EditOp 猜 inserted_range/deleted_range。
        // - old_revision 用 Core base_revision（编辑前的 revision），而非平台 text_revision()
        //   （后者在 Core edit 之后已经推进到 new_revision，拿它当 old_revision 是错的）。
        // - inserted_range / deleted_range 从 display_patches 提取：
        //   每个 DisplayPatch 的 replace_byte_range 是被替换（删除）的旧文本范围，
        //   inserted_byte_range 是新插入文本在 **final new 坐标**里的准确范围。
        //   Issue #826 评论 10 阻塞 4.5：不要再用
        //   replace_byte_range.start + inserted_text.len() 推算 inserted 位置，
        //   一笔 batch 里前一处变长后后一处已右移，推算会把中间正文算成 inserted。
        // - IME/Replace 多 patch 时记 ranges 数组，单 patch 时记单个 range。
        {
            let result = edit_result
                .as_ref()
                .expect("edit_result is Some when applied is true");
            // Issue #819 评论 5968931455 问题 3: 诊断字段全部来自 Core EditorEditResult，
            // 不再从平台 EditOp 猜 operation_kind/cause。
            // - operation_kind 用 Core 的 result.operation_kind（经 editor_operation_kind_label 转短名）。
            // - cause 用 Core 的 result.cause（Debug 格式）。
            // - 另加 visual_cause 字段记录平台视觉原因。
            let core_op_kind_label =
                super::transaction::editor_operation_kind_label(result.operation_kind);
            // Issue #819 评论 5968931455 问题 3（多 patch）: 诊断不伪造 final new range，
            // 直接记录 Core 已经明确给出的 patch 事实：old_replace_range + inserted_bytes。
            // deleted_ranges 仍用 old 坐标（正确）。
            let mut deleted_ranges: Vec<[usize; 2]> =
                Vec::with_capacity(result.display_patches.len());
            let mut display_patches: Vec<serde_json::Value> =
                Vec::with_capacity(result.display_patches.len());
            for patch in &result.display_patches {
                let del_start = patch.replace_byte_range.start().value();
                let del_end = patch.replace_byte_range.end().value();
                deleted_ranges.push([del_start, del_end]);
                display_patches.push(serde_json::json!({
                    "old_replace_range": [del_start, del_end],
                    "inserted_bytes": patch.inserted_text.len(),
                }));
            }
            writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
                timestamp_ms: chrono::Utc::now().timestamp_millis(),
                sequence: 0,
                session_id: String::new(),
                level: writer_diagnostics::DiagnosticLevel::Info,
                origin: writer_diagnostics::DiagnosticOrigin::App,
                event: "editor.edit.applied".to_string(),
                target: "editor.edit".to_string(),
                message: None,
                fields: {
                    let mut f = std::collections::BTreeMap::new();
                    f.insert("applied".to_string(), serde_json::json!(true));
                    f.insert(
                        "transaction_id".to_string(),
                        serde_json::json!(result.transaction_id),
                    );
                    // Issue #819 评论 5968931455 问题 3: operation_kind 来自 Core 真相。
                    f.insert(
                        "operation_kind".to_string(),
                        serde_json::json!(core_op_kind_label),
                    );
                    // Issue #819 评论 5968931455 问题 3: cause 来自 Core 真相，
                    // 另加 visual_cause 记录平台视觉原因。
                    f.insert(
                        "cause".to_string(),
                        serde_json::json!(format!("{:?}", result.cause)),
                    );
                    f.insert(
                        "visual_cause".to_string(),
                        serde_json::json!(format!("{:?}", visual_cause)),
                    );
                    f.insert(
                        "old_revision".to_string(),
                        serde_json::json!(result.base_revision.value()),
                    );
                    f.insert(
                        "new_revision".to_string(),
                        serde_json::json!(result.new_revision.value()),
                    );
                    // content_delta：Core 统计的字符增量（inserted/deleted chars）。
                    f.insert(
                        "inserted_chars".to_string(),
                        serde_json::json!(result.content_delta.inserted_chars),
                    );
                    f.insert(
                        "deleted_chars".to_string(),
                        serde_json::json!(result.content_delta.deleted_chars),
                    );
                    // 多 patch 时记数组，单 patch 时也记数组（统一格式，便于消费方解析）。
                    f.insert(
                        "deleted_ranges".to_string(),
                        serde_json::json!(deleted_ranges),
                    );
                    // Issue #819 评论 5968931455 问题 3: display_patches 记录 Core 给出的
                    // patch 事实（old_replace_range + inserted_bytes），不伪造 final new range。
                    f.insert(
                        "display_patches".to_string(),
                        serde_json::json!(display_patches),
                    );
                    f
                },
            });
        }
        editor_animation_debug_log(&format!(
            "apply_edit_with_visuals: core edit applied, visual_cause={:?}",
            visual_cause
        ));

        // 5. 保存 new snapshot
        let new = self.pipeline.snapshot();

        // 6. 算 animations_requested
        let animations_requested = self.current_coordinated_animation_enabled
            || self.current_typing_animation_enabled
            || self.current_smooth_cursor_enabled;

        // 7. 调视觉流水线，构造明确的 visual outcome
        let visual = if let Some(params) = composition {
            // composition commit 路径 — record_composition_commit_transaction
            // 内部自己处理动画开关和跳过事件，返回 VisualPrepareOutcome。
            // Issue #826: 直接透传 outcome，不再猜。
            let outcome = self.record_composition_commit_transaction(
                &old,
                &new,
                edit_result.expect("edit_result is Some when applied is true"),
                visual_cause,
                params.preedit_byte_start,
                params.preedit_byte_end,
                &params.saved_virtual_text,
                params.candidate_byte_start,
                params.candidate_byte_end,
                params.cancel_reason,
                params.summary_tag,
            );
            if !animations_requested {
                EditVisualOutcome::AnimationDisabled
            } else {
                match outcome {
                    super::pipeline::VisualPrepareOutcome::Created => EditVisualOutcome::Created,
                    super::pipeline::VisualPrepareOutcome::Skipped(reason) => {
                        EditVisualOutcome::Skipped(reason)
                    }
                    super::pipeline::VisualPrepareOutcome::AnimationDisabled => {
                        EditVisualOutcome::AnimationDisabled
                    }
                }
            }
        } else {
            // 普通路径 — record_transaction 总是被调用（做 summary/log/transaction_created）。
            // Issue #819 评论 5968240881 问题 2：record_transaction 返回 VisualPrepareOutcome，
            // 直接透传，不再用 self.current_is_scrolling 猜 ScrollingSuppressed、
            // 不再把 None 猜成 BuilderEmptyTransaction。
            let outcome = self.record_transaction(
                old,
                new,
                edit_result.expect("edit_result is Some when applied is true"),
                true,
            );
            if !animations_requested {
                EditVisualOutcome::AnimationDisabled
            } else {
                match outcome {
                    super::pipeline::VisualPrepareOutcome::Created => EditVisualOutcome::Created,
                    super::pipeline::VisualPrepareOutcome::Skipped(reason) => {
                        EditVisualOutcome::Skipped(reason)
                    }
                    super::pipeline::VisualPrepareOutcome::AnimationDisabled => {
                        // prepare_edit_motion 返回 AnimationDisabled 但 animations_requested
                        // 为 true——这只发生在三个开关全关但 animations_requested 计算为 true
                        // 的矛盾状态（不应该发生）。保守返回 BuilderEmptyTransaction。
                        EditVisualOutcome::Skipped(EditVisualSkipReason::BuilderEmptyTransaction)
                    }
                }
            }
        };

        EditApplyOutcome {
            applied: true,
            visual,
        }
    }
}
