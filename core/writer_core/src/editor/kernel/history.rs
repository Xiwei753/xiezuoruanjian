use super::result::{EditorContentDelta, EditorEditOutcome, EditorEditResult};
use super::types::{CoordinatedCursor, DisplayPatch, EditorOperationKind, EditorVisualIntent};
use super::EditorKernel;

use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset, Utf8ByteRange};
use crate::editor::transaction::{AnimationMode, EditorTransactionCause, OffsetMap};

impl EditorKernel {
    /// #624 评论8：Undo 通过 inverse delta 局部应用。
    ///
    /// 当前正文是编辑后的 new 文本；对 entry.edits 按 new_range 逆序应用
    /// inverse delta（把 new_range 处的内容替换回 deleted_text），光标/选区恢复
    /// 为 old_selection。DisplayPatch、VisualIntent、OffsetMap、content delta
    /// 全部从 delta 生成，不再 clone 全文、不再 diff_plain_text、不再全文 build。
    pub(crate) fn apply_undo(
        &mut self,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        let entry = match self.undo_stack.pop() {
            Some(e) => e,
            None => {
                return EditorEditOutcome::NoChange(self.noop_result(
                    base_revision,
                    old_cursor,
                    old_selection,
                ))
            }
        };

        let old_len_before_undo = self.text.byte_len();

        // 逆序应用 inverse delta：new 文本 → old 文本。
        for delta in entry.edits.iter().rev() {
            self.text.replace(
                delta.new_range.start().value()..delta.new_range.end().value(),
                &delta.deleted_text,
            );
        }
        self.cursor = Utf8ByteOffset::unchecked(entry.old_selection.end().value());
        self.selection_anchor = Utf8ByteOffset::unchecked(entry.old_selection.start().value());
        self.revision = self.revision.next();
        self.composition_session = None;

        let new_cursor_val = self.cursor;
        let new_revision = self.revision;
        let new_selection = entry.old_selection;

        // DisplayPatch 从 inverse delta 生成：undo 前文本（new 坐标）中被替换区域。
        let mut patches: Vec<DisplayPatch> = entry
            .edits
            .iter()
            .map(|d| DisplayPatch {
                base_revision,
                new_revision,
                replace_byte_range: d.new_range,
                inserted_text: d.deleted_text.clone(),
                resulting_selection_byte_range: new_selection,
            })
            .collect();
        patches.sort_by_key(|p| p.replace_byte_range.start().value());

        // OffsetMap：old=undo 前文本 → new=undo 后文本，从 inverse delta 构造。
        let inverse_pairs: Vec<(usize, usize, usize, usize)> = entry
            .edits
            .iter()
            .map(|d| {
                (
                    d.new_range.start().value(),
                    d.new_range.end().value(),
                    d.old_range.start().value(),
                    d.old_range.end().value(),
                )
            })
            .collect();
        let offset_map = OffsetMap::from_edits(old_len_before_undo, &inverse_pairs);

        let old_affected: Vec<Utf8ByteRange> = entry.edits.iter().map(|d| d.new_range).collect();
        let new_affected: Vec<Utf8ByteRange> = entry.edits.iter().map(|d| d.old_range).collect();

        let mut content_delta = EditorContentDelta::default();
        for d in &entry.edits {
            // inverse：undo 时插入的是原 deleted_text，删除的是原 inserted_text。
            content_delta.accumulate(&EditorContentDelta::from_texts(
                &d.deleted_text,
                &d.inserted_text,
            ));
        }

        let animation_mode = if !self.animation_enabled {
            AnimationMode::SystemSuppressed
        } else {
            AnimationMode::SnapshotAnimation
        };

        let visual_intent = EditorVisualIntent {
            cause: EditorTransactionCause::Undo,
            operation_kind: EditorOperationKind::Replace,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: new_cursor_val,
                should_animate: self.animation_enabled && old_cursor != new_cursor_val,
            },
            offset_map: Some(offset_map),
        };

        self.redo_stack.push(entry);

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches: patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
            content_delta,
        })
    }

    /// #624 评论8：Redo 通过 forward delta 局部应用。
    ///
    /// 当前正文是 undo 后的 old 文本；对 entry.edits 按 old_range 正序应用
    /// forward delta（把 old_range 处的内容替换回 inserted_text），光标/选区恢复
    /// 为 new_selection。DisplayPatch、VisualIntent、OffsetMap、content delta
    /// 全部从 delta 生成。
    pub(crate) fn apply_redo(
        &mut self,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        let entry = match self.redo_stack.pop() {
            Some(e) => e,
            None => {
                return EditorEditOutcome::NoChange(self.noop_result(
                    base_revision,
                    old_cursor,
                    old_selection,
                ))
            }
        };

        let old_len_before_redo = self.text.byte_len();

        // #624 评论8：按 old_range 从后往前应用 forward delta — 所有 old_range 都基于
        // 编辑前文本坐标，从右往左应用时先前的替换不会使后面 delta 的坐标漂移。
        // （edits 记录顺序 = 执行顺序，可能是升序或降序，不能直接依赖 iter/rev。）
        let mut forward: Vec<&super::TextEditDelta> = entry.edits.iter().collect();
        forward.sort_by_key(|d| std::cmp::Reverse(d.old_range.start().value()));
        for delta in forward {
            self.text.replace(
                delta.old_range.start().value()..delta.old_range.end().value(),
                &delta.inserted_text,
            );
        }
        self.cursor = Utf8ByteOffset::unchecked(entry.new_selection.end().value());
        self.selection_anchor = Utf8ByteOffset::unchecked(entry.new_selection.start().value());
        self.revision = self.revision.next();
        self.composition_session = None;

        let new_cursor_val = self.cursor;
        let new_revision = self.revision;
        let new_selection = entry.new_selection;

        // DisplayPatch 从 forward delta 生成：redo 前文本（old 坐标）中被替换区域。
        let mut patches: Vec<DisplayPatch> = entry
            .edits
            .iter()
            .map(|d| DisplayPatch {
                base_revision,
                new_revision,
                replace_byte_range: d.old_range,
                inserted_text: d.inserted_text.clone(),
                resulting_selection_byte_range: new_selection,
            })
            .collect();
        patches.sort_by_key(|p| p.replace_byte_range.start().value());

        // OffsetMap：old=redo 前文本 → new=redo 后文本，从 forward delta 构造。
        let forward_pairs: Vec<(usize, usize, usize, usize)> = entry
            .edits
            .iter()
            .map(|d| {
                (
                    d.old_range.start().value(),
                    d.old_range.end().value(),
                    d.new_range.start().value(),
                    d.new_range.end().value(),
                )
            })
            .collect();
        let offset_map = OffsetMap::from_edits(old_len_before_redo, &forward_pairs);

        let old_affected: Vec<Utf8ByteRange> = entry.edits.iter().map(|d| d.old_range).collect();
        let new_affected: Vec<Utf8ByteRange> = entry.edits.iter().map(|d| d.new_range).collect();

        let mut content_delta = EditorContentDelta::default();
        for d in &entry.edits {
            content_delta.accumulate(&EditorContentDelta::from_texts(
                &d.inserted_text,
                &d.deleted_text,
            ));
        }

        let animation_mode = if !self.animation_enabled {
            AnimationMode::SystemSuppressed
        } else {
            AnimationMode::SnapshotAnimation
        };

        let visual_intent = EditorVisualIntent {
            cause: EditorTransactionCause::Redo,
            operation_kind: EditorOperationKind::Replace,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: new_cursor_val,
                should_animate: self.animation_enabled && old_cursor != new_cursor_val,
            },
            offset_map: Some(offset_map),
        };

        self.undo_stack.push(entry);

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches: patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
            content_delta,
        })
    }
}
