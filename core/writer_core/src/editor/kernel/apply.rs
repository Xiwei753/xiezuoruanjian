use super::result::{EditorContentDelta, EditorEditOutcome, EditorEditResult};
use super::types::EditorCommand;
use super::types::{CoordinatedCursor, DisplayPatch, EditorOperationKind, EditorVisualIntent};
use super::{EditorKernel, TextEditDelta, UndoEntry};

use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset, Utf8ByteRange};
use crate::editor::transaction::{
    choose_animation_mode, count_grapheme_clusters, text_contains_complex_grapheme, AnimationMode,
    EditorTransactionCause, OffsetMap,
};

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
            } => {
                if *expected_revision != base_revision {
                    return EditorEditOutcome::StaleRevision(self.stale_session_result());
                }
            }
        }

        let old_cursor = self.cursor;
        let old_selection =
            Utf8ByteRange::from_ordered(self.selection_anchor.value(), self.cursor.value());

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
                old_selection,
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
                old_selection,
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
                old_selection,
            ),
            EditorCommand::SetSelection { anchor, head, .. } => self.apply_set_selection(
                anchor.value(),
                head.value(),
                base_revision,
                old_cursor,
                old_selection,
            ),
            EditorCommand::Undo { .. } => self.apply_undo(base_revision, old_cursor, old_selection),
            EditorCommand::Redo { .. } => self.apply_redo(base_revision, old_cursor, old_selection),
            EditorCommand::ReplaceAll {
                search,
                replacement,
                ..
            } => self.apply_replace_all(
                &search,
                &replacement,
                base_revision,
                old_cursor,
                old_selection,
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
                old_selection,
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
                old_selection,
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
                old_selection,
            ),
            EditorCommand::BeginComposition { replace_range, .. } => self.apply_begin_composition(
                replace_range.start().value(),
                replace_range.end().value(),
                base_revision,
                old_cursor,
                old_selection,
            ),
            EditorCommand::UpdateComposition {
                composition_session_id,
                composition_generation,
                new_preedit_text,
                new_preedit_cursor_offset,
                ..
            } => self.apply_update_composition(
                composition_session_id.value(),
                composition_generation.value(),
                &new_preedit_text,
                new_preedit_cursor_offset.value(),
                base_revision,
                old_cursor,
                old_selection,
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
                old_selection,
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
                old_selection,
            ),
        }
    }

    fn apply_insert(
        &mut self,
        byte_offset: usize,
        text: &str,
        cause: EditorTransactionCause,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        if byte_offset > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }
        if !self.text.is_char_boundary(byte_offset) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }

        self.composition_session = None;

        // #624 评论8：局部 Rope edit，不 clone 全文。
        self.text.insert(byte_offset, text);
        self.revision = self.revision.next();
        let new_cursor_val = byte_offset + text.len();
        self.cursor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.selection_anchor = Utf8ByteOffset::unchecked(new_cursor_val);

        let new_selection = Utf8ByteRange::point(new_cursor_val);
        let delta = TextEditDelta {
            old_range: Utf8ByteRange::point(byte_offset),
            new_range: Utf8ByteRange::from_start_len(byte_offset, text.len()),
            deleted_text: String::new(),
            inserted_text: text.to_string(),
        };
        self.undo_stack.push(UndoEntry {
            edits: vec![delta],
            old_selection,
            new_selection,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let new_affected = vec![Utf8ByteRange::from_start_len(byte_offset, text.len())];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::point(byte_offset),
            inserted_text: text.to_string(),
            resulting_selection_byte_range: new_selection,
        }];

        let is_loading = cause == EditorTransactionCause::Load;
        let is_format = cause == EditorTransactionCause::Format;

        let animation_mode = if !self.animation_enabled || is_loading || is_format {
            AnimationMode::SystemSuppressed
        } else {
            let cluster_count = count_grapheme_clusters(text);
            let contains_newline = text.contains('\n');
            let contains_complex = text_contains_complex_grapheme(text);
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false,
                is_loading,
                is_format,
                false,
                self.animation_enabled,
            )
        };

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Insert,
            old_affected_byte_ranges: vec![],
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(new_cursor_val),
                should_animate: self.animation_enabled
                    && old_cursor.value() != new_cursor_val
                    && !is_loading
                    && !is_format,
            },
            // #624 评论8：单次编辑从 delta 直接构造 offset map，不再扫全文。
            offset_map: Some(OffsetMap::from_single_edit(
                self.text.byte_len() - text.len(),
                (byte_offset, byte_offset),
                text.len(),
            )),
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
            content_delta: EditorContentDelta::from_inserted_text(text),
        })
    }

    // TODO(#597): 既有代码可读性技术债，待后续重构拆分
    #[allow(clippy::too_many_lines)]
    fn apply_delete(
        &mut self,
        byte_start: usize,
        byte_end_exclusive: usize,
        cause: EditorTransactionCause,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        let (byte_start, byte_end_exclusive) =
            Self::normalize_range(byte_start, byte_end_exclusive);
        if byte_start > self.text.byte_len() || byte_end_exclusive > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }
        if !self.text.is_char_boundary(byte_start)
            || !self.text.is_char_boundary(byte_end_exclusive)
        {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }
        if byte_start >= byte_end_exclusive {
            return EditorEditOutcome::InvalidRange(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }

        // #624 评论8：先取局部删除文本，再局部 Rope delete，不 clone 全文。
        let deleted_text = self
            .text
            .byte_slice(byte_start..byte_end_exclusive)
            .to_string();

        self.composition_session = None;

        self.text.delete(byte_start..byte_end_exclusive);
        self.revision = self.revision.next();
        self.cursor = Utf8ByteOffset::unchecked(byte_start);
        self.selection_anchor = Utf8ByteOffset::unchecked(byte_start);

        let new_selection = Utf8ByteRange::point(byte_start);
        let delta = TextEditDelta {
            old_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            new_range: Utf8ByteRange::point(byte_start),
            deleted_text: deleted_text.clone(),
            inserted_text: String::new(),
        };
        self.undo_stack.push(UndoEntry {
            edits: vec![delta],
            old_selection,
            new_selection,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let old_affected = vec![Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive)];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            inserted_text: String::new(),
            resulting_selection_byte_range: new_selection,
        }];

        let is_loading = cause == EditorTransactionCause::Load;
        let is_format = cause == EditorTransactionCause::Format;

        let animation_mode = if !self.animation_enabled || is_loading || is_format {
            AnimationMode::SystemSuppressed
        } else {
            let cluster_count = count_grapheme_clusters(&deleted_text);
            let contains_newline = deleted_text.contains('\n');
            let contains_complex = text_contains_complex_grapheme(&deleted_text);
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false,
                is_loading,
                is_format,
                false,
                self.animation_enabled,
            )
        };

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Delete,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: vec![],
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(byte_start),
                should_animate: self.animation_enabled
                    && old_cursor.value() != byte_start
                    && !is_loading
                    && !is_format,
            },
            // #624 评论8：单次删除从 delta 直接构造 offset map。
            offset_map: Some(OffsetMap::from_single_edit(
                self.text.byte_len() + (byte_end_exclusive - byte_start),
                (byte_start, byte_end_exclusive),
                0,
            )),
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
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
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        let (byte_start, byte_end_exclusive) =
            Self::normalize_range(byte_start, byte_end_exclusive);
        if byte_start > self.text.byte_len() || byte_end_exclusive > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }
        if !self.text.is_char_boundary(byte_start)
            || !self.text.is_char_boundary(byte_end_exclusive)
        {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }

        // #624 评论8：先取局部删除文本，再局部 Rope replace，不 clone 全文。
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

        let new_selection = Utf8ByteRange::point(new_cursor_val);
        let delta = TextEditDelta {
            old_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            new_range: Utf8ByteRange::from_start_len(byte_start, replacement_text.len()),
            deleted_text: deleted_text.clone(),
            inserted_text: replacement_text.to_string(),
        };
        self.undo_stack.push(UndoEntry {
            edits: vec![delta],
            old_selection,
            new_selection,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let new_selection = Utf8ByteRange::point(new_cursor_val);
        let old_affected = vec![Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive)];
        let new_affected = vec![Utf8ByteRange::from_start_len(
            byte_start,
            replacement_text.len(),
        )];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            inserted_text: replacement_text.to_string(),
            resulting_selection_byte_range: new_selection,
        }];

        let is_loading = cause == EditorTransactionCause::Load;
        let is_format = cause == EditorTransactionCause::Format;

        let animation_mode = if !self.animation_enabled || is_loading || is_format {
            AnimationMode::SystemSuppressed
        } else {
            let diff_text = if !replacement_text.is_empty() {
                replacement_text
            } else {
                &deleted_text
            };
            let cluster_count = count_grapheme_clusters(diff_text);
            let contains_newline = diff_text.contains('\n');
            let contains_complex = text_contains_complex_grapheme(diff_text);
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false,
                is_loading,
                is_format,
                false,
                self.animation_enabled,
            )
        };

        let operation_kind = if byte_start == byte_end_exclusive {
            EditorOperationKind::Insert
        } else if replacement_text.is_empty() {
            EditorOperationKind::Delete
        } else {
            EditorOperationKind::Replace
        };

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(new_cursor_val),
                should_animate: self.animation_enabled
                    && old_cursor.value() != new_cursor_val
                    && !is_loading
                    && !is_format,
            },
            // #624 评论8：单次替换从 delta 直接构造 offset map。
            offset_map: Some(OffsetMap::from_single_edit(
                self.text.byte_len() - replacement_text.len() + (byte_end_exclusive - byte_start),
                (byte_start, byte_end_exclusive),
                replacement_text.len(),
            )),
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
            content_delta: EditorContentDelta::from_texts(replacement_text, &deleted_text),
        })
    }

    fn apply_insert_line_break(
        &mut self,
        byte_offset: usize,
        auto_indent_enabled: bool,
        cause: EditorTransactionCause,
        base_revision: EditorRevision,
        old_cursor: Utf8ByteOffset,
        old_selection: Utf8ByteRange,
    ) -> EditorEditOutcome {
        if byte_offset > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }
        if !self.text.is_char_boundary(byte_offset) {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }
        // #606: Core 端 auto-indent — 从正文按 UTF-8 安全边界找到当前逻辑行开头，
        // 读取已有前导空白（空格/Tab），构造插入文本为 \n + prefix。
        // auto_indent_enabled 为 false 时只插入 \n。
        // #624 评论8：行首定位与前导空白读取都基于光标附近 RopeSlice，不 materialize 全文。
        let text = if auto_indent_enabled {
            let prefix = Self::compute_auto_indent_prefix(&self.text, byte_offset);
            format!("\n{}", prefix)
        } else {
            "\n".to_string()
        };

        self.composition_session = None;

        // #624 评论8：局部 Rope insert，不 clone 全文。
        self.text.insert(byte_offset, &text);
        self.revision = self.revision.next();
        let new_cursor_val = byte_offset + text.len();
        self.cursor = Utf8ByteOffset::unchecked(new_cursor_val);
        self.selection_anchor = Utf8ByteOffset::unchecked(new_cursor_val);

        let new_selection = Utf8ByteRange::point(new_cursor_val);
        let delta = TextEditDelta {
            old_range: Utf8ByteRange::point(byte_offset),
            new_range: Utf8ByteRange::from_start_len(byte_offset, text.len()),
            deleted_text: String::new(),
            inserted_text: text.clone(),
        };
        self.undo_stack.push(UndoEntry {
            edits: vec![delta],
            old_selection,
            new_selection,
        });
        self.redo_stack.clear();

        let new_revision = self.revision;
        let new_selection = Utf8ByteRange::point(new_cursor_val);
        let new_affected = vec![Utf8ByteRange::from_start_len(byte_offset, text.len())];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::point(byte_offset),
            inserted_text: text.clone(),
            resulting_selection_byte_range: new_selection,
        }];

        let animation_mode = if !self.animation_enabled {
            AnimationMode::SystemSuppressed
        } else {
            let cluster_count = count_grapheme_clusters(&text);
            let contains_newline = text.contains('\n');
            let contains_complex = text_contains_complex_grapheme(&text);
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false,
                false,
                false,
                false,
                self.animation_enabled,
            )
        };

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: EditorOperationKind::Insert,
            old_affected_byte_ranges: vec![],
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(new_cursor_val),
                should_animate: self.animation_enabled && old_cursor.value() != new_cursor_val,
            },
            // #624 评论8：单次换行插入从 delta 直接构造 offset map。
            offset_map: Some(OffsetMap::from_single_edit(
                self.text.byte_len() - text.len(),
                (byte_offset, byte_offset),
                text.len(),
            )),
        };

        EditorEditOutcome::Applied(EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
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
    /// #624 评论8：Rope 局部版本 — 只在光标前 `[0, byte_offset)` slice 上迭代，
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
        old_selection: Utf8ByteRange,
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
                    old_selection,
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
                old_selection,
            ));
        }
        if byte_start > self.text.byte_len() || byte_end_exclusive > self.text.byte_len() {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }
        if !self.text.is_char_boundary(byte_start)
            || !self.text.is_char_boundary(byte_end_exclusive)
        {
            return EditorEditOutcome::InvalidOffset(self.noop_result(
                base_revision,
                old_cursor,
                old_selection,
            ));
        }

        // #624 评论8：先取局部删除文本，再局部 Rope replace，不 clone 全文。
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

        let new_selection = Utf8ByteRange::from_ordered(sel_anchor, sel_head);
        let delta = TextEditDelta {
            old_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            new_range: Utf8ByteRange::from_start_len(byte_start, replacement_text.len()),
            deleted_text: deleted_text.clone(),
            inserted_text: replacement_text.to_string(),
        };
        self.undo_stack.push(UndoEntry {
            edits: vec![delta],
            old_selection,
            new_selection,
        });
        self.redo_stack.clear();
        let preedit_byte_len = self
            .composition_session
            .as_ref()
            .map(|s| s.preedit_text.len())
            .unwrap_or(0);
        let is_composition_commit = self.composition_session.is_some();
        self.composition_session = None;

        let new_revision = self.revision;
        let old_affected = if preedit_byte_len > 0 {
            vec![Utf8ByteRange::from_start_len(byte_start, preedit_byte_len)]
        } else {
            vec![Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive)]
        };
        let new_affected = vec![Utf8ByteRange::from_start_len(
            byte_start,
            replacement_text.len(),
        )];

        let display_patches = vec![DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::from_ordered(byte_start, byte_end_exclusive),
            inserted_text: replacement_text.to_string(),
            resulting_selection_byte_range: new_selection,
        }];

        let cluster_count = count_grapheme_clusters(replacement_text);
        let contains_newline = replacement_text.contains('\n');
        let contains_complex = text_contains_complex_grapheme(replacement_text);
        let animation_mode = if !self.animation_enabled {
            AnimationMode::SystemSuppressed
        } else {
            choose_animation_mode(
                cluster_count,
                contains_newline,
                contains_complex,
                false,
                false,
                false,
                false,
                self.animation_enabled,
            )
        };

        let visual_intent = EditorVisualIntent {
            cause,
            operation_kind: if is_composition_commit {
                EditorOperationKind::CompositionCommit
            } else if byte_start == byte_end_exclusive {
                EditorOperationKind::Insert
            } else if replacement_text.is_empty() {
                EditorOperationKind::Delete
            } else {
                EditorOperationKind::Replace
            },
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: new_affected,
            animation_mode,
            duration_ms: self.animation_duration_ms,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(sel_head),
                should_animate: self.animation_enabled && old_cursor.value() != sel_head,
            },
            // #624 评论8：单次 commit 从 delta 直接构造 offset map。
            offset_map: Some(OffsetMap::from_single_edit(
                self.text.byte_len() - replacement_text.len() + (byte_end_exclusive - byte_start),
                (byte_start, byte_end_exclusive),
                replacement_text.len(),
            )),
        };

        let edit_result = EditorEditResult {
            transaction_id: self.take_transaction_id(),
            base_revision,
            new_revision,
            display_patches,
            old_selection_byte_range: old_selection,
            new_selection_byte_range: new_selection,
            visual_intent,
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
        old_selection: Utf8ByteRange,
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

        if let Some((as_, ae)) = after_range {
            if as_ > self.text.byte_len()
                || ae > self.text.byte_len()
                || !self.text.is_char_boundary(as_)
                || !self.text.is_char_boundary(ae)
            {
                return EditorEditOutcome::InvalidOffset(self.noop_result(
                    base_revision,
                    old_cursor,
                    old_selection,
                ));
            }
            if as_ >= ae || as_ < sel_max {
                return EditorEditOutcome::InvalidRange(self.noop_result(
                    base_revision,
                    old_cursor,
                    old_selection,
                ));
            }
            // #624 评论8：局部 Rope delete + 记录 delta。
            let deleted = self.text.byte_slice(as_..ae).to_string();
            self.text.delete(as_..ae);
            edits.push(TextEditDelta {
                old_range: Utf8ByteRange::from_ordered(as_, ae),
                new_range: Utf8ByteRange::point(as_),
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
                    old_selection,
                ));
            }
            if bs >= be || be > sel_min {
                return EditorEditOutcome::InvalidRange(self.noop_result(
                    base_revision,
                    old_cursor,
                    old_selection,
                ));
            }
            // #624 评论8：局部 Rope delete + 记录 delta。
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
                old_selection,
            ));
        }

        self.revision = self.revision.next();
        self.composition_session = None;

        let before_deleted_len: usize = if let Some((bs, be)) = before_range {
            be.saturating_sub(bs)
        } else {
            0
        };

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

        let new_selection = Utf8ByteRange::from_ordered(new_sel_anchor, new_sel_head);

        // #624 评论8：content delta / offset map / affected ranges 全部从 delta 构造，
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
        let new_revision = self.revision;

        // #624 评论10：原子 patch batch — 每条 delta 一条局部 DisplayPatch
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
            cause,
            operation_kind: EditorOperationKind::Delete,
            old_affected_byte_ranges: old_affected,
            new_affected_byte_ranges: vec![],
            animation_mode: AnimationMode::SystemSuppressed,
            duration_ms: 0,
            coordinated_cursor: CoordinatedCursor {
                old_offset: old_cursor,
                new_offset: Utf8ByteOffset::unchecked(new_sel_head),
                should_animate: false,
            },
            offset_map: Some(OffsetMap::from_edits(old_len, &offset_pairs)),
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
