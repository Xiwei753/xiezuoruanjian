use super::result::{EditorContentDelta, EditorEditOutcome, EditorEditResult};
use super::types::{CoordinatedCursor, DisplayPatch, EditorOperationKind, EditorVisualIntent};
use super::{EditorKernel, TextEditDelta, UndoEntry};

use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset, Utf8ByteRange};
use crate::editor::transaction::{AnimationMode, EditorTransactionCause, OffsetMap};

impl EditorKernel {
    /// #624 评论8：replace-all 是冷路径（明确需要全文的边界），允许 materialize
    /// 旧文本一次做全文匹配；但正文修改与 UndoEntry 都只保存局部 delta：
    /// 按旧文本坐标从后往前对 Rope 做局部 replace，UndoEntry 记录每个匹配位置的
    /// old/new range 与局部文本，不再保存两份全文。
    // TODO(#597): 既有代码可读性技术债，待后续重构拆分
    #[allow(clippy::too_many_lines)]
    pub(crate) fn apply_replace_all(
        &mut self,
        search: &str,
        replacement: &str,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        if search.is_empty() {
            return EditorEditOutcome::NoChange(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }

        // 冷路径：全文匹配一次。
        let old_text = self.snapshot_text();
        let mut match_starts: Vec<usize> = Vec::new();
        let mut from = 0usize;
        while let Some(pos) = old_text[from..].find(search) {
            let absolute = from + pos;
            match_starts.push(absolute);
            from = absolute + search.len();
        }

        if match_starts.is_empty() {
            return EditorEditOutcome::NoChange(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }

        // 构造 delta（基于旧文本坐标；new_range 由前面替换的累计长度差决定）。
        let search_len = search.len();
        let replacement_len = replacement.len();
        let mut edits: Vec<TextEditDelta> = Vec::with_capacity(match_starts.len());
        // #624 评论8：长度差可能为负（替换变短），用 i128 累积避免 usize 溢出。
        // 第 i 个替换的 new_start = search 起点 + 前 i 次长度差；数学上恒 >= 0
        // （search 起点按 search_len 递增，长度差下界为 -（search_len - replacement_len）），
        // 因此 i128 → usize 转换不可能符号丢失，也不会截断（正文受 usize 约束）。
        let mut cumulative_diff: i128 = 0;
        for &start in &match_starts {
            edits.push(TextEditDelta {
                old_range: Utf8ByteRange::from_start_len(start, search_len),
                #[allow(clippy::cast_sign_loss)]
                new_range: Utf8ByteRange::from_start_len(
                    (start as i128 + cumulative_diff) as usize,
                    replacement_len,
                ),
                deleted_text: search.to_string(),
                inserted_text: replacement.to_string(),
            });
            cumulative_diff += replacement_len as i128 - search_len as i128;
        }

        // 从后往前对 Rope 局部 replace（前面的坐标不受后面替换影响）。
        for &start in match_starts.iter().rev() {
            self.text.replace(start..start + search_len, replacement);
        }

        self.revision = self.revision.next();
        let new_cursor_val = Self::clamp_to_char_boundary(&self.text, self.cursor.value());
        self.cursor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.selection_anchor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.composition_session = None;

        let new_selection = Utf8ByteRange::point(new_cursor_val);
        // #624 评论8：content delta / offset map / affected ranges 从 delta 构造，
        // 计算完成后才把 edits 移入 Undo 栈。
        let mut content_delta = EditorContentDelta::default();
        let mut offset_pairs: Vec<(usize, usize, usize, usize)> = Vec::with_capacity(edits.len());
        for delta in &edits {
            content_delta.accumulate(&EditorContentDelta::from_texts(
                &delta.inserted_text,
                &delta.deleted_text,
            ));
            offset_pairs.push((
                delta.old_range.start().value(),
                delta.old_range.end().value(),
                delta.new_range.start().value(),
                delta.new_range.end().value(),
            ));
        }
        let old_affected: Vec<Utf8ByteRange> = edits.iter().map(|e| e.old_range).collect();
        let new_affected: Vec<Utf8ByteRange> = edits.iter().map(|e| e.new_range).collect();

        self.undo_stack.push(UndoEntry {
            edits,
            old_selection,
            new_selection,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;

        // 单条最终 DisplayPatch：最外层范围（旧文本坐标）+ 中间保留文本的局部拼接。
        // #624 评论8：retained 长度必须扣除总长度差（替换变短时 outer_end 超出新文本）。
        let outer_start = match_starts[0];
        let outer_end = match_starts.last().map_or(0, |s| s + search_len);
        let total_diff = search_len
            .saturating_sub(replacement_len)
            .saturating_mul(match_starts.len());
        let retained_len = (outer_end - outer_start).saturating_sub(total_diff);
        let retained = self
            .text
            .byte_slice(outer_start..outer_start + retained_len)
            .to_string();
        let display_patches = if outer_start < outer_end || !retained.is_empty() {
            vec![DisplayPatch {
                base_revision,
                new_revision,
                replace_byte_range: Utf8ByteRange::from_ordered(outer_start, outer_end),
                inserted_text: retained,
                resulting_selection_byte_range: new_selection,
            }]
        } else {
            vec![]
        };

        let visual_intent = EditorVisualIntent {
            cause: EditorTransactionCause::Format,
            operation_kind: EditorOperationKind::Format,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode: AnimationMode::SystemSuppressed,
            duration_ms: 0,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(new_cursor_val),
                should_animate: false,
            },
            // #624 评论8：从 delta 直接构造 offset map，不再全文 diff。
            offset_map: Some(OffsetMap::from_edits(old_text.len(), &offset_pairs)),
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
            content_delta,
        })
    }
}
