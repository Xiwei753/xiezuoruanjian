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

        // #624 评论10 第4项补漏：inverse delta 按 new_range.start 降序应用（先恢复右侧），
        // 保证左侧 delta 的 new_range（最终文本坐标）在应用时仍然有效。旧实现固定
        // iter().rev() 隐含「edits 列表按 new_range 升序」——replace-all 满足，但
        // deleteSurrounding 的 edits 是 [after, before]，rev() 变成升序，before 先恢复
        // 会把 after 点坐标推偏（顺序应用碰巧对，但 DisplayPatch/OffsetMap 坐标是错的）。
        // 同起点（before/after 紧邻，均退化为 point(bs)）时按 old_range.start 降序决胜：
        // after（右侧）先插入，before 后插入，文本顺序才是先 before 后 after。
        let mut undo_order: Vec<&super::TextEditDelta> = entry.edits.iter().collect();
        undo_order.sort_by(|a, b| {
            b.new_range
                .start()
                .value()
                .cmp(&a.new_range.start().value())
                .then_with(|| {
                    b.old_range
                        .start()
                        .value()
                        .cmp(&a.old_range.start().value())
                })
        });
        for delta in &undo_order {
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
        // 列表顺序 = undo_order（new_range 降序）— Android 稳定降序排序后同一批顺序，
        // 紧邻同起点时 after（右侧）先应用，与顺序应用语义一致。
        let patches: Vec<DisplayPatch> = undo_order
            .iter()
            .map(|d| DisplayPatch {
                base_revision,
                new_revision,
                replace_byte_range: d.new_range,
                inserted_text: d.deleted_text.clone(),
                resulting_selection_byte_range: new_selection,
            })
            .collect();

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
