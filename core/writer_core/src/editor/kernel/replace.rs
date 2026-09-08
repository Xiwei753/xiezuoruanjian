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
        let new_revision = self.revision;

        // #624 评论10：一个 EditorEditResult 是一个原子 patch batch — 每条 delta 一条
        // 局部 DisplayPatch（base 文档坐标，inserted_text 只含本次替换文本）。
        // 不再合成覆盖 [outer_start, outer_end) 的单条外层 patch：旧实现的
        // `saturating_sub` 总长度差在替换变长时变成 0，从新 Rope 截出错误 retained
        // （aXbXc 做 X→YY 时 Android 变成 aYYbc）；相距很远的两个匹配还会复制中间
        // 整段保留正文。batch 内所有 patch 携带同一组 base/new revision（版本边界）。
        let display_patches: Vec<DisplayPatch> = edits
            .iter()
            .map(|d| DisplayPatch {
                base_revision,
                new_revision,
                replace_byte_range: d.old_range,
                inserted_text: d.inserted_text.clone(),
                resulting_selection_byte_range: new_selection,
            })
            .collect();

        self.undo_stack.push(UndoEntry {
            edits,
            old_selection,
            new_selection,
        });
        self.redo_stack.clear();

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
