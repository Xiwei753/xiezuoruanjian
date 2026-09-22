use super::result::{make_selection, EditorContentDelta, EditorEditOutcome, EditorEditResult};
use super::types::EditorOperationKind;
use super::EditorKernel;

use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset};
use crate::editor::transaction::EditorTransactionCause;

impl EditorKernel {
    pub(crate) fn apply_set_selection(
        &mut self,
        anchor_byte_offset: usize,
        head_byte_offset: usize,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection_anchor: usize,
        old_selection_head: usize,
    ) -> EditorEditOutcome {
        let anchor = anchor_byte_offset;
        let head = head_byte_offset;
        if anchor > self.text.byte_len() || head > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if !self.text.is_char_boundary(anchor) || !self.text.is_char_boundary(head) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        // Issue #683：只有 anchor/head 与当前状态完全相同时才返回 NoChange。
        // 只要 anchor 或 head 真变了，就返回 Applied——Applied 表示命令改变了
        // 编辑器状态，不等于"正文一定发生变化"。平台端据此更新 mirror cursor，
        // 不再把"正文没变"误解成"什么都没变"。
        let state_unchanged = anchor == old_selection_anchor && head == old_selection_head;

        self.selection_anchor = Utf8ByteOffset::unchecked(anchor);
        self.cursor = Utf8ByteOffset::unchecked(head);

        let new_selection = make_selection(anchor, head);

        let result = EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision: self.revision,
            display_patches: vec![],
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
            cause: EditorTransactionCause::Programmatic,
            operation_kind: EditorOperationKind::CursorOnly,
            // 选区操作不变更正文，无 offset_map
            offset_map: None,
            content_delta: EditorContentDelta::default(),
        };

        if state_unchanged {
            EditorEditOutcome::NoChange(result)
        } else {
            EditorEditOutcome::Applied(result)
        }
    }
}
