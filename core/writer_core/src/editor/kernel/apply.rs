use super::result::{make_selection, EditorContentDelta, EditorEditOutcome, EditorEditResult};
use super::types::EditorCommand;
use super::types::{DisplayPatch, EditorOperationKind};
use super::{EditorKernel, TextEditDelta, UndoEntry};

use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset, Utf8ByteRange};
use crate::editor::transaction::{EditorTransactionCause, OffsetMap};

impl EditorKernel {
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn apply(&mut self, command: EditorCommand) -> EditorEditOutcome {
        let base_revision = self.revision;

        match &command {
            EditorCommand::Insert {
                expected_revision, ..
            }
            | EditorCommand::Delete {
                expected_revision, ..
            }
            | EditorCommand::Replace {
                expected_revision, ..
            }
            | EditorCommand::SetSelection {
                expected_revision, ..
            }
            | EditorCommand::ReplaceAll {
                expected_revision, ..
            }
            | EditorCommand::InsertLineBreak {
                expected_revision, ..
            }
            | EditorCommand::Undo { expected_revision }
            | EditorCommand::Redo { expected_revision }
            | EditorCommand::CommitText {
                expected_revision, ..
            }
            | EditorCommand::DeleteSurrounding {
                expected_revision, ..
            }
            | EditorCommand::ImeCommit {
                expected_revision, ..
            }
            | EditorCommand::BeginComposition {
                expected_revision, ..
            }
            | EditorCommand::UpdateComposition {
                expected_revision, ..
            }
            | EditorCommand::FinishComposition {
                expected_revision, ..
            }
            | EditorCommand::CancelComposition {
                expected_revision, ..
            }
            | EditorCommand::CompositionMoveGraphemeLeft {
                expected_revision, ..
            }
            | EditorCommand::CompositionMoveGraphemeRight {
                expected_revision, ..
            }
            | EditorCommand::CompositionDeleteGraphemeBackward {
                expected_revision, ..
            }
            | EditorCommand::CompositionDeleteGraphemeForward {
                expected_revision, ..
            } => {
                if *expected_revision != base_revision {
                    return EditorEditOutcome::StaleRevision(self.stale_session_result());
                }
            }
        }

        let old_cursor = self.cursor;
        let old_selection_anchor = self.selection_anchor.value();
        let old_selection_head = self.cursor.value();

        match command {
            EditorCommand::Insert {
                byte_offset,
                text,
                cause,
                ..
            } => self.apply_insert(
                byte_offset.value(),
                &text,
                cause,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::Delete {
                byte_range,
                deleted_text: _,
                cause,
                ..
            } => self.apply_delete(
                byte_range.start().value(),
                byte_range.end().value(),
                cause,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::Replace {
                byte_range,
                replacement_text,
                original_text: _,
                cause,
                ..
            } => self.apply_replace(
                byte_range.start().value(),
                byte_range.end().value(),
                &replacement_text,
                cause,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::SetSelection { anchor, head, .. } => self.apply_set_selection(
                anchor.value(),
                head.value(),
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::Undo { .. } => self.apply_undo(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::Redo { .. } => self.apply_redo(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::ReplaceAll {
                search,
                replacement,
                ..
            } => self.apply_replace_all(
                &search,
                &replacement,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::InsertLineBreak {
                byte_offset,
                auto_indent_enabled,
                cause,
                ..
            } => self.apply_insert_line_break(
                byte_offset.value(),
                auto_indent_enabled,
                cause,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::CommitText {
                byte_range,
                replacement_text,
                resulting_selection_anchor,
                resulting_selection_head,
                composition_session_id,
                composition_base_revision,
                composition_generation,
                cause,
                ..
            } => self.apply_commit_text(
                byte_range.start().value(),
                byte_range.end().value(),
                &replacement_text,
                resulting_selection_anchor.value(),
                resulting_selection_head.value(),
                composition_session_id.value(),
                composition_base_revision.value(),
                composition_generation.value(),
                cause,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::DeleteSurrounding {
                before_byte_range,
                after_byte_range,
                cause,
                ..
            } => self.apply_delete_surrounding(
                before_byte_range.start().value(),
                before_byte_range.end().value(),
                after_byte_range.start().value(),
                after_byte_range.end().value(),
                cause,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::ImeCommit {
                selection_byte_range,
                replacement_byte_range_after_selection,
                inserted_text,
                cause,
                ..
            } => self.apply_ime_commit(
                selection_byte_range.start().value(),
                selection_byte_range.end().value(),
                replacement_byte_range_after_selection.start().value(),
                replacement_byte_range_after_selection.end().value(),
                &inserted_text,
                cause,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::BeginComposition { replace_range, .. } => self.apply_begin_composition(
                replace_range.start().value(),
                replace_range.end().value(),
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::UpdateComposition {
                composition_session_id,
                composition_generation,
                new_preedit_text,
                new_preedit_cursor_utf16,
                ..
            } => self.apply_update_composition(
                composition_session_id.value(),
                composition_generation.value(),
                &new_preedit_text,
                new_preedit_cursor_utf16,
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::FinishComposition {
                composition_session_id,
                composition_generation,
                ..
            } => self.apply_finish_composition(
                composition_session_id.value(),
                composition_generation.value(),
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::CancelComposition {
                composition_session_id,
                composition_generation,
                ..
            } => self.apply_cancel_composition(
                composition_session_id.value(),
                composition_generation.value(),
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            //  R8: composition 专用 grapheme 语义操作
            EditorCommand::CompositionMoveGraphemeLeft {
                composition_session_id,
                composition_generation,
                ..
            } => self.apply_composition_move_grapheme_left(
                composition_session_id.value(),
                composition_generation.value(),
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::CompositionMoveGraphemeRight {
                composition_session_id,
                composition_generation,
                ..
            } => self.apply_composition_move_grapheme_right(
                composition_session_id.value(),
                composition_generation.value(),
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::CompositionDeleteGraphemeBackward {
                composition_session_id,
                composition_generation,
                ..
            } => self.apply_composition_delete_grapheme_backward(
                composition_session_id.value(),
                composition_generation.value(),
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
            EditorCommand::CompositionDeleteGraphemeForward {
                composition_session_id,
                composition_generation,
                ..
            } => self.apply_composition_delete_grapheme_forward(
                composition_session_id.value(),
                composition_generation.value(),
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ),
        }
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn apply_insert(
        &mut self,
        byte_offset: usize,
        text: &str,
        cause: EditorTransactionCause,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection_anchor: usize,
        old_selection_head: usize,
    ) -> EditorEditOutcome {
        if byte_offset > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if !self.text.is_char_boundary(byte_offset) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        self.composition_session = None;

        // 局部 Rope edit，不 clone 全文。
        self.text.insert(byte_offset, text);
        self.revision = self.revision.next();
        let new_cursor_val = byte_offset + text.len();
        self.cursor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.selection_anchor = Utf8ByteOffset::unchecked(new_cursor_val);

        let new_selection = make_selection(new_cursor_val, new_cursor_val);
        let delta = TextEditDelta {
            old_range: Utf8ByteRange::point(byte_offset),
            new_range: Utf8ByteRange::from_start_len(byte_offset, text.len()),
            deleted_text: String::new(),
            inserted_text: text.to_string(),
        };
        self.undo_stack.push(UndoEntry {
            edits: vec![delta],
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::point(byte_offset),
            inserted_text: text.to_string(),
            resulting_selection_byte_range: EditorEditResult::selection_byte_range(new_selection),
        }];

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
            cause,
            operation_kind: EditorOperationKind::Insert,
            // 单次编辑从 delta 直接构造 offset map，不再扫全文。
            offset_map: Some(OffsetMap::from_single_edit(
                self.text.byte_len() - text.len(),
                (byte_offset, byte_offset),
                text.len(),
            )),
            content_delta: EditorContentDelta::from_inserted_text(text),
        })
    }

    // TODO(#597): 既有代码可读性技术债，待后续重构拆分
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    fn apply_delete(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        cause: EditorTransactionCause,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection_anchor: usize,
        old_selection_head: usize,
    ) -> EditorEditOutcome {
        let (byte_start, byte_end_exclusive) =
            Self::normalize_range(byte_start, byte_end_exclusive);
        if byte_start > self.text.byte_len() || byte_end_exclusive > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if !self.text.is_char_boundary(byte_start)
            || !self.text.is_char_boundary(byte_end_exclusive)
        {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if byte_start >= byte_end_exclusive {
            return EditorEditOutcome::InvalidRange(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        // 先取局部删除文本，再局部 Rope delete，不 clone 全文。
        let deleted_text = self
            .text
            .byte_slice(byte_start..byte_end_exclusive)
            .to_string();

        self.composition_session = None;

        self.text.delete(byte_start..byte_end_exclusive);
        self.revision = self.revision.next();
        self.cursor = Utf8ByteOffset::unchecked(byte_start);
        self.selection_anchor = Utf8ByteOffset::unchecked(byte_start);

        let new_selection = make_selection(byte_start, byte_start);
        let delta = TextEditDelta {
            old_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            new_range: Utf8ByteRange::point(byte_start),
            deleted_text: deleted_text.clone(),
            inserted_text: String::new(),
        };
        self.undo_stack.push(UndoEntry {
            edits: vec![delta],
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            inserted_text: String::new(),
            resulting_selection_byte_range: EditorEditResult::selection_byte_range(new_selection),
        }];

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
            cause,
            operation_kind: EditorOperationKind::Delete,
            // 单次删除从 delta 直接构造 offset map。
            offset_map: Some(OffsetMap::from_single_edit(
                self.text.byte_len() + (byte_end_exclusive - byte_start),
                (byte_start, byte_end_exclusive),
                0,
            )),
            content_delta: EditorContentDelta::from_deleted_text(&deleted_text),
        })
    }

    #[allow(clippy::too_many_arguments)]
    // TODO(#597): 既有代码可读性技术债，待后续重构拆分
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting
    )]
    fn apply_replace(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        replacement_text: &str,
        cause: EditorTransactionCause,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection_anchor: usize,
        old_selection_head: usize,
    ) -> EditorEditOutcome {
        let (byte_start, byte_end_exclusive) =
            Self::normalize_range(byte_start, byte_end_exclusive);
        if byte_start > self.text.byte_len() || byte_end_exclusive > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if !self.text.is_char_boundary(byte_start)
            || !self.text.is_char_boundary(byte_end_exclusive)
        {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        // 先取局部删除文本，再局部 Rope replace，不 clone 全文。
        let deleted_text = self
            .text
            .byte_slice(byte_start..byte_end_exclusive)
            .to_string();

        self.composition_session = None;

        self.text
            .replace(byte_start..byte_end_exclusive, replacement_text);
        self.revision = self.revision.next();
        let new_cursor_val = byte_start + replacement_text.len();
        self.cursor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.selection_anchor = Utf8ByteOffset::unchecked(new_cursor_val);

        let new_selection = make_selection(new_cursor_val, new_cursor_val);
        let delta = TextEditDelta {
            old_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            new_range: Utf8ByteRange::from_start_len(byte_start, replacement_text.len()),
            deleted_text: deleted_text.clone(),
            inserted_text: replacement_text.to_string(),
        };
        self.undo_stack.push(UndoEntry {
            edits: vec![delta],
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let new_selection = make_selection(new_cursor_val, new_cursor_val);

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            inserted_text: replacement_text.to_string(),
            resulting_selection_byte_range: EditorEditResult::selection_byte_range(new_selection),
        }];

        let operation_kind = if byte_start == byte_end_exclusive {
            EditorOperationKind::Insert
        } else if replacement_text.is_empty() {
            EditorOperationKind::Delete
        } else {
            EditorOperationKind::Replace
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
            cause,
            operation_kind,
            // 单次替换从 delta 直接构造 offset map。
            offset_map: Some(OffsetMap::from_single_edit(
                self.text.byte_len() - replacement_text.len() + (byte_end_exclusive - byte_start),
                (byte_start, byte_end_exclusive),
                replacement_text.len(),
            )),
            content_delta: EditorContentDelta::from_texts(replacement_text, &deleted_text),
        })
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn apply_insert_line_break(
        &mut self,
        byte_offset: usize,
        auto_indent_enabled: bool,
        cause: EditorTransactionCause,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection_anchor: usize,
        old_selection_head: usize,
    ) -> EditorEditOutcome {
        if byte_offset > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if !self.text.is_char_boundary(byte_offset) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        // #606: Core 端 auto-indent — 从正文按 UTF-8 安全边界找到当前逻辑行开头，
        // 读取已有前导空白（空格/Tab），构造插入文本为 \n + prefix。
        // auto_indent_enabled 为 false 时只插入 \n。
        // 行首定位与前导空白读取都基于光标附近 RopeSlice，不 materialize 全文。
        let text = if auto_indent_enabled {
            let prefix = Self::compute_auto_indent_prefix(&self.text, byte_offset);
            format!("\n{}", prefix)
        } else {
            "\n".to_string()
        };

        self.composition_session = None;

        // 局部 Rope insert，不 clone 全文。
        self.text.insert(byte_offset, &text);
        self.revision = self.revision.next();
        let new_cursor_val = byte_offset + text.len();
        self.cursor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.selection_anchor = Utf8ByteOffset::unchecked(new_cursor_val);

        let new_selection = make_selection(new_cursor_val, new_cursor_val);
        let delta = TextEditDelta {
            old_range: Utf8ByteRange::point(byte_offset),
            new_range: Utf8ByteRange::from_start_len(byte_offset, text.len()),
            deleted_text: String::new(),
            inserted_text: text.clone(),
        };
        self.undo_stack.push(UndoEntry {
            edits: vec![delta],
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let new_selection = make_selection(new_cursor_val, new_cursor_val);

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::point(byte_offset),
            inserted_text: text.clone(),
            resulting_selection_byte_range: EditorEditResult::selection_byte_range(new_selection),
        }];

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
            cause,
            operation_kind: EditorOperationKind::Insert,
            // 单次换行插入从 delta 直接构造 offset map。
            offset_map: Some(OffsetMap::from_single_edit(
                self.text.byte_len() - text.len(),
                (byte_offset, byte_offset),
                text.len(),
            )),
            content_delta: EditorContentDelta::from_inserted_text(&text),
        })
    }

    /// #606: Core 端 auto-indent 前导空白计算。
    ///
    /// 从正文按 UTF-8 安全边界找到  所在逻辑行的开头，
    /// 读取该行已有的前导空白（空格和 Tab），返回前导空白字符串。
    ///
    /// 规则：
    /// - 找到  之前最后一个换行符的位置，下一字节即为行首
    /// - 从行首开始逐字节检查，只收集连续的空格和 Tab
    /// - 遇到其他字符（包括多字节字符的首字节）立即停止
    /// - UTF-8 安全：空格和 Tab 都是单字节 ASCII，不会出现在多字节字符的续字节中
    ///
    /// 返回的前导空白会被追加到新行之后，实现自动缩进。
    /// Rope 局部版本 — 只在光标前 `[0, byte_offset)` slice 上迭代，
    /// 不 materialize 全文。`bytes().rev()` 从光标向前找行首，再从行首收集前导空白。
    fn compute_auto_indent_prefix(rope: &crop::Rope, byte_offset: usize) -> String {
        // 找到 byte_offset 所在行的行首
        let prefix_slice = rope.byte_slice(0..byte_offset);
        let line_start = prefix_slice
            .bytes()
            .rev()
            .position(|b| b == b'\n')
            .map_or(0, |from_end| byte_offset - from_end);

        // 从行首开始收集前导空白（空格和 Tab）
        let line = rope.byte_slice(line_start..byte_offset);
        let mut prefix = String::new();
        for byte in line.bytes() {
            if byte == b' ' || byte == b'\t' {
                prefix.push(byte as char);
            } else {
                break;
            }
        }
        prefix
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    fn apply_commit_text(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        replacement_text: &str,
        resulting_selection_anchor: usize,
        resulting_selection_head: usize,
        composition_session_id: u64,
        composition_base_revision: u64,
        composition_generation: u64,
        cause: EditorTransactionCause,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection_anchor: usize,
        old_selection_head: usize,
    ) -> EditorEditOutcome {
        if let Some(ref session) = self.composition_session {
            if session.session_id.value() != composition_session_id
                || session.base_revision.value() != composition_base_revision
                || session.generation.value() != composition_generation
            {
                return EditorEditOutcome::StaleRevision(self.stale_session_result());
            }
        } else if composition_session_id != 0 {
            return EditorEditOutcome::StaleRevision(self.stale_session_result());
        }

        let (byte_start, byte_end_exclusive) =
            Self::normalize_range(byte_start, byte_end_exclusive);

        if let Some(ref session) = self.composition_session {
            if byte_start != session.replace_start.value()
                || byte_end_exclusive != session.replace_end_exclusive.value()
            {
                return EditorEditOutcome::InvalidRange(self.noop_result(
                    base_revision,
                    old_cursor,
                    old_selection_anchor,
                    old_selection_head,
                ));
            }
        }

        if byte_start == byte_end_exclusive
            && replacement_text.is_empty()
            && self.composition_session.is_none()
        {
            return EditorEditOutcome::NoChange(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if byte_start > self.text.byte_len() || byte_end_exclusive > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if !self.text.is_char_boundary(byte_start)
            || !self.text.is_char_boundary(byte_end_exclusive)
        {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        // 先取局部删除文本，再局部 Rope replace，不 clone 全文。
        let deleted_text = self
            .text
            .byte_slice(byte_start..byte_end_exclusive)
            .to_string();

        self.text
            .replace(byte_start..byte_end_exclusive, replacement_text);
        self.revision = self.revision.next();

        let sel_anchor = Self::clamp_to_char_boundary(&self.text, resulting_selection_anchor);
        let sel_head = Self::clamp_to_char_boundary(&self.text, resulting_selection_head);
        let selection_was_adjusted =
            sel_anchor != resulting_selection_anchor || sel_head != resulting_selection_head;
        self.selection_anchor = Utf8ByteOffset::unchecked(sel_anchor);
        self.cursor = Utf8ByteOffset::unchecked(sel_head);

        let new_selection = make_selection(sel_anchor, sel_head);
        let delta = TextEditDelta {
            old_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            new_range: Utf8ByteRange::from_start_len(byte_start, replacement_text.len()),
            deleted_text: deleted_text.clone(),
            inserted_text: replacement_text.to_string(),
        };
        self.undo_stack.push(UndoEntry {
            edits: vec![delta],
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
        });
        self.redo_stack.clear();
        let is_composition_commit = self.composition_session.is_some();
        self.composition_session = None;

        let new_revision = self.revision;

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            inserted_text: replacement_text.to_string(),
            resulting_selection_byte_range: EditorEditResult::selection_byte_range(new_selection),
        }];

        let operation_kind = if is_composition_commit {
            EditorOperationKind::CompositionCommit
        } else if byte_start == byte_end_exclusive {
            EditorOperationKind::Insert
        } else if replacement_text.is_empty() {
            EditorOperationKind::Delete
        } else {
            EditorOperationKind::Replace
        };

        let edit_result = EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
            cause,
            operation_kind,
            // 单次 commit 从 delta 直接构造 offset map。
            offset_map: Some(OffsetMap::from_single_edit(
                self.text.byte_len() - replacement_text.len() + (byte_end_exclusive - byte_start),
                (byte_start, byte_end_exclusive),
                replacement_text.len(),
            )),
            content_delta: EditorContentDelta::from_texts(replacement_text, &deleted_text),
        };

        if selection_was_adjusted {
            EditorEditOutcome::AppliedWithAdjustedSelection(edit_result)
        } else {
            EditorEditOutcome::Applied(edit_result)
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    fn apply_delete_surrounding(
        &mut self,
        before_byte_start: usize,
        before_byte_end_exclusive: usize,
        after_byte_start: usize,
        after_byte_end_exclusive: usize,
        cause: EditorTransactionCause,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection_anchor: usize,
        old_selection_head: usize,
    ) -> EditorEditOutcome {
        let sel_anchor = self.selection_anchor.value();
        let sel_head = self.cursor.value();
        let (sel_min, sel_max) = if sel_anchor <= sel_head {
            (sel_anchor, sel_head)
        } else {
            (sel_head, sel_anchor)
        };

        let mut edits: Vec<TextEditDelta> = Vec::new();
        let old_len = self.text.byte_len();

        let after_range = if after_byte_start < after_byte_end_exclusive {
            Some((after_byte_start, after_byte_end_exclusive))
        } else {
            None
        };
        let before_range = if before_byte_start < before_byte_end_exclusive {
            Some((before_byte_start, before_byte_end_exclusive))
        } else {
            None
        };
        // before 删除长度用于计算 after delta 的最终文本坐标。
        // 纯几何计算，不依赖正文状态，可提前求值。
        let before_deleted_len: usize = before_range.map_or(0, |(bs, be)| be.saturating_sub(bs));

        if let Some((as_, ae)) = after_range {
            if as_ > self.text.byte_len()
                || ae > self.text.byte_len()
                || !self.text.is_char_boundary(as_)
                || !self.text.is_char_boundary(ae)
            {
                return EditorEditOutcome::InvalidOffset(self.noop_result(
                    base_revision,
                    old_cursor,
                    old_selection_anchor,
                    old_selection_head,
                ));
            }
            if as_ >= ae || as_ < sel_max {
                return EditorEditOutcome::InvalidRange(self.noop_result(
                    base_revision,
                    old_cursor,
                    old_selection_anchor,
                    old_selection_head,
                ));
            }
            // 局部 Rope delete 记录 delta。
            let deleted = self.text.byte_slice(as_..ae).to_string();
            self.text.delete(as_..ae);
            edits.push(TextEditDelta {
                old_range: Utf8ByteRange::from_ordered(as_, ae),
                // new_range 必须使用两次删除都完成后的最终文本坐标。
                // after 先于 before 删除，删除 after 瞬间正文仍含 before
                // 区间，point(as_) 是「仅删除 after」时的坐标；随后 before 删除会把该点
                // 左移 before_deleted_len。若这里保留 point(as_)，undo 的 DisplayPatch
                // 以 base 坐标降序应用时 after patch 会插到错误位置（"abXYcd" undo 后
                // Android 得到 "abXYdc"），Core/Android mirror 分裂。as_ >= be 保证
                // as_ - before_deleted_len >= bs >= 0，不会下溢。
                new_range: Utf8ByteRange::point(as_.saturating_sub(before_deleted_len)),
                deleted_text: deleted,
                inserted_text: String::new(),
            });
        }

        if let Some((bs, be)) = before_range {
            if bs > self.text.byte_len()
                || be > self.text.byte_len()
                || !self.text.is_char_boundary(bs)
                || !self.text.is_char_boundary(be)
            {
                return EditorEditOutcome::InvalidOffset(self.noop_result(
                    base_revision,
                    old_cursor,
                    old_selection_anchor,
                    old_selection_head,
                ));
            }
            if bs >= be || be > sel_min {
                return EditorEditOutcome::InvalidRange(self.noop_result(
                    base_revision,
                    old_cursor,
                    old_selection_anchor,
                    old_selection_head,
                ));
            }
            // 局部 Rope delete 记录 delta。
            let deleted = self.text.byte_slice(bs..be).to_string();
            self.text.delete(bs..be);
            edits.push(TextEditDelta {
                old_range: Utf8ByteRange::from_ordered(bs, be),
                new_range: Utf8ByteRange::point(bs),
                deleted_text: deleted,
                inserted_text: String::new(),
            });
        }

        if edits.is_empty() {
            return EditorEditOutcome::NoChange(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        self.revision = self.revision.next();
        self.composition_session = None;

        let new_sel_anchor = if sel_anchor == sel_min {
            sel_min.saturating_sub(before_deleted_len)
        } else {
            sel_max.saturating_sub(before_deleted_len)
        };
        let new_sel_head = if sel_head == sel_min {
            sel_min.saturating_sub(before_deleted_len)
        } else {
            sel_max.saturating_sub(before_deleted_len)
        };
        self.selection_anchor = Utf8ByteOffset::unchecked(new_sel_anchor);
        self.cursor = Utf8ByteOffset::unchecked(new_sel_head);

        let new_selection = make_selection(new_sel_anchor, new_sel_head);

        // content delta / offset map / affected ranges 全部从 delta 构造，
        // 计算完成后才把 edits 积入 Undo 栈。
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
        let new_revision = self.revision;

        // 原子 patch batch — 每条 delta 一条局部 DisplayPatch
        // （base 文档坐标，删除的 inserted_text 为空）。不再合成最外层单条 patch
        // （把 before/after 之间的保留段 middle 重新拼接进 inserted_text）：
        // 两个相距很远的删除会复制中间整段正文，且与 batch 协议不一致。
        let display_patches: Vec<DisplayPatch> = edits
            .iter()
            .map(|d| DisplayPatch {
                base_revision,
                new_revision,
                replace_byte_range: d.old_range,
                inserted_text: d.inserted_text.clone(),
                resulting_selection_byte_range: EditorEditResult::selection_byte_range(
                    new_selection,
                ),
            })
            .collect();

        self.undo_stack.push(UndoEntry {
            edits,
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
        });
        self.redo_stack.clear();

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
            cause,
            operation_kind: EditorOperationKind::Delete,
            offset_map: Some(OffsetMap::from_edits(old_len, &offset_pairs)),
            content_delta,
        })
    }

    /// 原子 IME commit：Qt `QInputMethodEvent` 两步语义的原子执行。
    ///
    /// Qt 对 `QInputMethodEvent` 的定义：先删除当前 selection，再做
    /// replacement/commit，整个 operation 加入 undo stack。
    /// 本方法在一个 `apply()` 调用内完成两步正文修改：
    /// - Step 1: 删除 selection `[sel_min, sel_max)`（原始 text 坐标）；
    /// - Step 2: 在删完 selection 后的文本（base_text）上删除
    ///   `[rep_min, rep_max)` 并在 `rep_min` 插入 `inserted_text`
    ///   （rep range 是 base_text 坐标）。
    /// - revision 只推进一次；
    /// - 只 push 一个 `UndoEntry`（可包含两条 `TextEditDelta`）；
    /// - selection/cursor 一次算完。
    ///
    /// 多段 affected range / offset map 按现有 `DeleteSurrounding` 的
    /// 多编辑事务写法保留，不退化成把中间没改的正文当成一个大 replace。
    #[allow(
        clippy::too_many_arguments,
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting
    )]
    fn apply_ime_commit(
        &mut self,
        sel_start: usize,
        sel_end: usize,
        rep_start: usize,
        rep_end: usize,
        inserted_text: &str,
        cause: EditorTransactionCause,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection_anchor: usize,
        old_selection_head: usize,
    ) -> EditorEditOutcome {
        let (sel_min, sel_max) = if sel_start <= sel_end {
            (sel_start, sel_end)
        } else {
            (sel_end, sel_start)
        };
        let (rep_min, rep_max) = if rep_start <= rep_end {
            (rep_start, rep_end)
        } else {
            (rep_end, rep_start)
        };

        let old_len = self.text.byte_len();
        let sel_len = sel_max - sel_min;
        let rep_len = rep_max - rep_min;
        let ins_len = inserted_text.len();
        let has_sel = sel_min < sel_max;
        let has_rep = rep_min < rep_max;
        let has_ins = ins_len > 0;

        // Issue #701 评论 5704688994 问题 2: 所有检查前置到任何 delete/insert 之前。
        // 旧实现先执行 self.text.delete(sel_min..sel_max) 再校验 rep char boundary，
        // 若 rep 非法则 return InvalidOffset 但正文已被删、revision 未推进，Core 状态被污染。

        // 边界检查 sel（针对原始 text）。
        if sel_min > old_len || sel_max > old_len {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }
        if !self.text.is_char_boundary(sel_min) || !self.text.is_char_boundary(sel_max) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        // 用 base_text 长度检查 rep 越界（base_text_len = old_len - sel_len）。
        let base_text_len = old_len - sel_len;
        if rep_min > base_text_len || rep_max > base_text_len {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        // 把 base offset 映射回原始 rope 的边界位置，用原始 rope 的 is_char_boundary
        // 校验 rep 在 base_text 上是否 char boundary。
        // 映射规则：b <= sel_min → o = b；b > sel_min → o = b + sel_len。
        // sel_min 与 sel_max 均已校验为 char boundary，b == sel_min 时映射到 sel_min
        // 与映射到 sel_max 在 char boundary 上等价（两者都是 selection 边界）。
        let rep_min_orig_for_boundary = if rep_min <= sel_min {
            rep_min
        } else {
            rep_min + sel_len
        };
        let rep_max_orig_for_boundary = if rep_max <= sel_min {
            rep_max
        } else {
            rep_max + sel_len
        };
        if !self.text.is_char_boundary(rep_min_orig_for_boundary)
            || !self.text.is_char_boundary(rep_max_orig_for_boundary)
        {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        // 全部检查通过。先从原始 rope 提取需要的 deleted_text 切片，再修改 text。
        let deleted_selection_text = if has_sel {
            self.text.byte_slice(sel_min..sel_max).to_string()
        } else {
            String::new()
        };

        // Issue #701 评论 5704688994 问题 3: base range 映回原始坐标的算法修正。
        // 横跨 gap（has_sel && rep_min < sel_min && rep_max > sel_min）时 replacement
        // 在 base_text 上跨越了 selection 删除产生的"缺口"，需要拆成左右两段不重叠 delta。
        let crosses_gap = has_sel && rep_min < sel_min && rep_max > sel_min;

        let deleted_replacement_left_text: String;
        let deleted_replacement_right_text: String;
        let _deleted_replacement_text: String;
        if has_rep {
            if crosses_gap {
                // 左段：base_text[rep_min..sel_min) 对应原始 [rep_min, sel_min)。
                deleted_replacement_left_text = self.text.byte_slice(rep_min..sel_min).to_string();
                // 右段：base_text[sel_min..rep_max) 对应原始 [sel_max, rep_max + sel_len)。
                let right_orig_start = sel_max;
                let right_orig_end = rep_max + sel_len;
                deleted_replacement_right_text = self
                    .text
                    .byte_slice(right_orig_start..right_orig_end)
                    .to_string();
                let mut combined = String::with_capacity(
                    deleted_replacement_left_text.len() + deleted_replacement_right_text.len(),
                );
                combined.push_str(&deleted_replacement_left_text);
                combined.push_str(&deleted_replacement_right_text);
                _deleted_replacement_text = combined;
            } else {
                // 单段：不横跨 gap，整个 replacement range 在 selection 同侧。
                let (rep_min_orig, rep_max_orig) = if rep_max <= sel_min {
                    (rep_min, rep_max)
                } else {
                    // rep_min >= sel_min（在 selection 之后或从 gap 开始）。
                    (rep_min + sel_len, rep_max + sel_len)
                };
                deleted_replacement_left_text = String::new();
                deleted_replacement_right_text =
                    self.text.byte_slice(rep_min_orig..rep_max_orig).to_string();
                _deleted_replacement_text = deleted_replacement_right_text.clone();
            }
        } else {
            deleted_replacement_left_text = String::new();
            deleted_replacement_right_text = String::new();
            _deleted_replacement_text = String::new();
        }

        // 一次性修改 self.text：先 delete selection，再 delete replacement，再 insert。
        // Step 1: 删除 selection [sel_min, sel_max)，得到 base_text。
        if has_sel {
            self.text.delete(sel_min..sel_max);
        }
        // 此时 self.text 是 base_text。
        // Step 2: 在 base_text 上删除 [rep_min, rep_max)。
        if has_rep {
            self.text.delete(rep_min..rep_max);
        }
        // Step 3: 在 rep_min 插入 inserted_text。
        if has_ins {
            self.text.insert(rep_min, inserted_text);
        }
        // 此时 self.text 是 final_text。

        self.composition_session = None;
        self.revision = self.revision.next();

        // cursor 在 final_text 中的位置 = rep_min + inserted_text.len()。
        let new_cursor_val = rep_min + ins_len;
        self.cursor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.selection_anchor = Utf8ByteOffset::unchecked(new_cursor_val);

        let new_selection = make_selection(new_cursor_val, new_cursor_val);

        // base_text 坐标 b → final_text 坐标 f(b)。
        // final_text[0..rep_min] = base_text[0..rep_min]
        // final_text[rep_min..rep_min+ins_len] = inserted_text
        // final_text[rep_min+ins_len..] = base_text[rep_max..]
        let base_to_final = |b: usize| -> usize {
            if has_rep {
                if b <= rep_min {
                    b
                } else if b >= rep_max {
                    b - rep_len + ins_len
                } else {
                    // rep_min < b < rep_max：replacement 内部，收缩到插入点。
                    rep_min
                }
            } else {
                // 纯插入（无 replacement 删除）。
                if b < rep_min {
                    b
                } else {
                    b + ins_len
                }
            }
        };

        // 构造 delta 列表（按原始坐标 → final_text 坐标）。
        // 顺序：selection delta 先 push，replacement/插入 delta 后 push。
        // 这保证 DisplayPatch 按 old_range.start 降序 stable 排序时，同起点处
        // selection（非零长度删除）先于纯插入（零长度）应用。
        let mut edits: Vec<TextEditDelta> = Vec::with_capacity(4);

        // selection 删除 delta。
        if has_sel {
            edits.push(TextEditDelta {
                old_range: Utf8ByteRange::from_ordered(sel_min, sel_max),
                new_range: Utf8ByteRange::point(base_to_final(sel_min)),
                deleted_text: deleted_selection_text.clone(),
                inserted_text: String::new(),
            });
        }

        // replacement delta(s) + 插入。
        if has_rep {
            if crosses_gap {
                // 左段：old_range = [rep_min, sel_min)，收缩到 rep_min。
                edits.push(TextEditDelta {
                    old_range: Utf8ByteRange::from_ordered(rep_min, sel_min),
                    new_range: Utf8ByteRange::point(rep_min),
                    deleted_text: deleted_replacement_left_text.clone(),
                    inserted_text: String::new(),
                });
                // 右段：old_range = [sel_max, rep_max + sel_len)，
                // 合并插入：new_range = [rep_min, rep_min + ins_len)。
                let new_range = if has_ins {
                    Utf8ByteRange::from_start_len(rep_min, ins_len)
                } else {
                    Utf8ByteRange::point(rep_min)
                };
                edits.push(TextEditDelta {
                    old_range: Utf8ByteRange::from_ordered(sel_max, rep_max + sel_len),
                    new_range,
                    deleted_text: deleted_replacement_right_text.clone(),
                    inserted_text: inserted_text.to_string(),
                });
            } else {
                // 单段。
                let (rep_min_orig, rep_max_orig) = if rep_max <= sel_min {
                    (rep_min, rep_max)
                } else {
                    (rep_min + sel_len, rep_max + sel_len)
                };
                let new_range = if has_ins {
                    Utf8ByteRange::from_start_len(rep_min, ins_len)
                } else {
                    Utf8ByteRange::point(rep_min)
                };
                edits.push(TextEditDelta {
                    old_range: Utf8ByteRange::from_ordered(rep_min_orig, rep_max_orig),
                    new_range,
                    deleted_text: deleted_replacement_right_text.clone(),
                    inserted_text: inserted_text.to_string(),
                });
            }
        } else if has_ins {
            // 纯插入（无 replacement 删除）。
            let rep_min_orig = if rep_min <= sel_min {
                rep_min
            } else {
                rep_min + sel_len
            };
            edits.push(TextEditDelta {
                old_range: Utf8ByteRange::point(rep_min_orig),
                new_range: Utf8ByteRange::from_start_len(rep_min, ins_len),
                deleted_text: String::new(),
                inserted_text: inserted_text.to_string(),
            });
        }

        // 空操作（无选区 + 无 replacement + 空插入）已在上游过滤。
        if edits.is_empty() {
            return EditorEditOutcome::NoChange(self.noop_result(
                base_revision,
                old_cursor,
                old_selection_anchor,
                old_selection_head,
            ));
        }

        self.undo_stack.push(UndoEntry {
            edits: edits.clone(),
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;

        // DisplayPatch：每条 delta 一条局部 DisplayPatch（base 文档坐标）。
        // 所有 patch 共享同一个 base_revision/new_revision（原子 batch）。
        let display_patches: Vec<DisplayPatch> = edits
            .iter()
            .map(|d| DisplayPatch {
                base_revision,
                new_revision,
                replace_byte_range: d.old_range,
                inserted_text: d.inserted_text.clone(),
                resulting_selection_byte_range: EditorEditResult::selection_byte_range(
                    new_selection,
                ),
            })
            .collect();

        // content_delta / offset_pairs 从 delta 构造。
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

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection: make_selection(old_selection_anchor, old_selection_head),
            new_selection,
            cause,
            operation_kind: EditorOperationKind::CompositionCommit,
            // 原子 IME commit 从 delta 构造 offset map。
            offset_map: Some(OffsetMap::from_edits(old_len, &offset_pairs)),
            content_delta,
        })
    }
}
