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
//! 7. 明确返回 `Created(key)` / `Skipped(reason)` / `AnimationDisabled`
//! 8. 发 content/cursor/selection changed（由调用方决定）
//!
//! 硬约束：Core Applied + animations_requested 时不允许返回裸 `None`——
//! 必须有明确的 `Created` 或 `Skipped`。

use super::*;
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;

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
/// 两条路径最终都创建同一种 `TextVisualTransaction`（放入 `prepared_queue`）。
/// Issue #819: 协同 InsertReveal/DeleteConceal 的空间边界直接来自同一笔 cursor track
/// 的当前帧。非协同时才是独立文字 timeline + 独立 smooth cursor。
pub(crate) struct CompositionCommitParams {
    pub pending_preedit_cursor_rect: Option<CursorRect>,
    pub preedit_byte_start: usize,
    pub preedit_byte_end: usize,
    pub saved_virtual_text: String,
    pub candidate_byte_start: usize,
    pub candidate_byte_end: usize,
    pub committed_replace_start: usize,
    pub committed_replace_end: usize,
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
    /// `prepare_edit_motion` 内部检测到 stale canonical / canonical invariant。
    StaleCanonical,
    /// caret 几何缺失，无法构造动画坐标。
    CaretGeometryMissing,
    /// 协同模式拿不到 cursor track。
    CursorTrackMissing,
    /// builder 跳过了事务创建（空事务 / 无可视单元）。
    BuilderEmptyTransaction,
    /// composition commit 路径的 old/new snapshot 构造失败或不可用。
    CompositionCommitSnapshotUnavailable,
    /// composition commit 路径创建了事务但 key 为 None（builder 跳过）。
    CompositionCommitBuilderSkipped,
}

/// Issue #819 评论 5956495850 第 1 节：视觉事务的明确结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EditVisualOutcome {
    /// 视觉事务已创建，`key` 是真正创建的事务 key。
    Created(VisualTransactionKey),
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
    /// 7. 明确返回 `Created(key)` / `Skipped(reason)` / `AnimationDisabled`。
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
        let edit_result: Option<writer_core::editor::EditorEditResult> = match op {
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

        let applied = edit_result.is_some();
        if !applied {
            // Core edit 未应用（如空删除范围），无编辑无动画。
            return EditApplyOutcome {
                applied: false,
                visual: EditVisualOutcome::AnimationDisabled,
            };
        }

        // 4. 记录 editor.edit.applied
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
            // 内部自己处理动画开关和跳过事件，返回 Option<VisualTransactionKey>。
            let key = self.record_composition_commit_transaction(
                &old,
                &new,
                visual_cause,
                params.pending_preedit_cursor_rect,
                params.preedit_byte_start,
                params.preedit_byte_end,
                &params.saved_virtual_text,
                params.candidate_byte_start,
                params.candidate_byte_end,
                params.committed_replace_start,
                params.committed_replace_end,
                params.cancel_reason,
                params.summary_tag,
            );
            match key {
                Some(k) => EditVisualOutcome::Created(k),
                None => {
                    if !animations_requested {
                        EditVisualOutcome::AnimationDisabled
                    } else {
                        EditVisualOutcome::Skipped(
                            EditVisualSkipReason::CompositionCommitBuilderSkipped,
                        )
                    }
                }
            }
        } else {
            // 普通路径 — record_transaction 总是被调用（做 summary/log/transaction_created）。
            let result = self.record_transaction(
                old,
                new,
                edit_result
                    .as_ref()
                    .expect("edit_result is Some when applied is true"),
                true,
            );
            if !animations_requested {
                EditVisualOutcome::AnimationDisabled
            } else {
                // 硬约束：animations_requested 时不允许返回裸 None——必须有明确的 Created 或 Skipped。
                match result {
                    Some((_, Some(key))) => EditVisualOutcome::Created(key),
                    Some((_, None)) => {
                        // builder 跳过了事务创建（空事务 / 无可视单元）。
                        // prepare_edit_motion 内部已记过正式跳过事件。
                        EditVisualOutcome::Skipped(EditVisualSkipReason::BuilderEmptyTransaction)
                    }
                    None => {
                        // animations_requested 但 motion 为 None。
                        // 区分滚动抑制和其他跳过原因。
                        if self.current_is_scrolling {
                            EditVisualOutcome::Skipped(EditVisualSkipReason::ScrollingSuppressed)
                        } else {
                            EditVisualOutcome::Skipped(EditVisualSkipReason::BuilderEmptyTransaction)
                        }
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
