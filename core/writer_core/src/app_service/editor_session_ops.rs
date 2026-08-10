use crate::api::{
    AnimatedSliceRoleDto, EditorByteRangeDto, EditorEditOutcomeDto, EditorEditResultDto,
    EditorSessionSnapshotDto, EditorTransactionCauseDto, RebaseSliceMappingDto,
};

use super::EditorSession;

impl super::WriterAppService {
    fn with_session<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut EditorSession) -> R,
    {
        let mut session = self
            .editor_session
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        f(&mut session)
    }

    pub fn editor_kernel_insert(
        &self,
        byte_offset: u32,
        text: String,
        cause: EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset};
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let current_text = s.kernel.text();
            let offset = match Utf8ByteOffset::try_new(current_text, byte_offset as usize) {
                Ok(o) => o,
                Err(_) => return EditorEditResultDto::invalid_offset_fallback(),
            };
            let result = s.kernel.apply(EditorCommand::Insert {
                byte_offset: offset,
                text,
                cause: core_cause,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_delete(
        &self,
        byte_start: u32,
        byte_end_exclusive: u32,
        cause: EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{EditorRevision, Utf8ByteRange};
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let current_text = s.kernel.text();
            let byte_range = match Utf8ByteRange::try_new(
                current_text,
                byte_start as usize,
                byte_end_exclusive as usize,
            ) {
                Ok(r) => r,
                Err(_) => return EditorEditResultDto::invalid_range_fallback(),
            };
            let result = s.kernel.apply(EditorCommand::Delete {
                byte_range,
                deleted_text: String::new(),
                cause: core_cause,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_replace(
        &self,
        byte_start: u32,
        byte_end_exclusive: u32,
        replacement_text: String,
        original_text: String,
        cause: EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{EditorRevision, Utf8ByteRange};
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let current_text = s.kernel.text();
            let byte_range = match Utf8ByteRange::try_new(
                current_text,
                byte_start as usize,
                byte_end_exclusive as usize,
            ) {
                Ok(r) => r,
                Err(_) => return EditorEditResultDto::invalid_range_fallback(),
            };
            let result = s.kernel.apply(EditorCommand::Replace {
                byte_range,
                replacement_text,
                original_text,
                cause: core_cause,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_set_selection(
        &self,
        anchor_byte_offset: u32,
        head_byte_offset: u32,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset};
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let current_text = s.kernel.text();
            let anchor = match Utf8ByteOffset::try_new(current_text, anchor_byte_offset as usize) {
                Ok(o) => o,
                Err(_) => return EditorEditResultDto::invalid_offset_fallback(),
            };
            let head = match Utf8ByteOffset::try_new(current_text, head_byte_offset as usize) {
                Ok(o) => o,
                Err(_) => return EditorEditResultDto::invalid_offset_fallback(),
            };
            let result = s.kernel.apply(EditorCommand::SetSelection {
                anchor,
                head,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_undo(&self, expected_revision: u64) -> EditorEditResultDto {
        use crate::editor::strong_types::EditorRevision;
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Undo {
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_redo(&self, expected_revision: u64) -> EditorEditResultDto {
        use crate::editor::strong_types::EditorRevision;
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Redo {
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_load_text(
        &self,
        text: String,
        cursor_byte_offset: u32,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::Utf8ByteOffset;
        self.with_session(|s| {
            let offset = match Utf8ByteOffset::try_new(&text, cursor_byte_offset as usize) {
                Ok(o) => o,
                Err(_) => return EditorEditResultDto::invalid_offset_fallback(),
            };
            s.generation = s.generation.saturating_add(1);
            let result = s.kernel.load_text(text, offset.value());
            result.into_result().into()
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn editor_kernel_commit_text(
        &self,
        byte_start: u32,
        byte_end_exclusive: u32,
        replacement_text: String,
        resulting_selection_anchor: u32,
        resulting_selection_head: u32,
        composition_session_id: u64,
        composition_base_revision: u64,
        composition_generation: u64,
        cause: EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{
            EditorRevision, EditorSessionGeneration, EditorSessionId, Utf8ByteOffset, Utf8ByteRange,
        };
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let current_text = s.kernel.text();
            let byte_range = match Utf8ByteRange::try_new(
                current_text,
                byte_start as usize,
                byte_end_exclusive as usize,
            ) {
                Ok(r) => r,
                Err(_) => return EditorEditResultDto::invalid_range_fallback(),
            };
            let anchor = Utf8ByteOffset::unchecked(resulting_selection_anchor as usize);
            let head = Utf8ByteOffset::unchecked(resulting_selection_head as usize);
            let composition_session_id = if composition_session_id == 0 {
                EditorSessionId::new(0)
            } else {
                match EditorSessionId::try_new(composition_session_id) {
                    Ok(id) => id,
                    Err(_) => return EditorEditResultDto::stale_fallback(),
                }
            };
            let result = s.kernel.apply(EditorCommand::CommitText {
                byte_range,
                replacement_text,
                resulting_selection_anchor: anchor,
                resulting_selection_head: head,
                composition_session_id,
                composition_base_revision: EditorRevision::new(composition_base_revision),
                composition_generation: EditorSessionGeneration::new(composition_generation),
                cause: core_cause,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into()
        })
    }

    pub fn editor_kernel_delete_surrounding(
        &self,
        before_byte_start: u32,
        before_byte_end_exclusive: u32,
        after_byte_start: u32,
        after_byte_end_exclusive: u32,
        cause: EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{EditorRevision, Utf8ByteRange};
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let current_text = s.kernel.text();
            let before_byte_range = match Utf8ByteRange::try_new(
                current_text,
                before_byte_start as usize,
                before_byte_end_exclusive as usize,
            ) {
                Ok(r) => r,
                Err(_) => return EditorEditResultDto::invalid_range_fallback(),
            };
            let after_byte_range = match Utf8ByteRange::try_new(
                current_text,
                after_byte_start as usize,
                after_byte_end_exclusive as usize,
            ) {
                Ok(r) => r,
                Err(_) => return EditorEditResultDto::invalid_range_fallback(),
            };
            let result = s.kernel.apply(EditorCommand::DeleteSurrounding {
                before_byte_range,
                after_byte_range,
                cause: core_cause,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into()
        })
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn editor_kernel_begin_composition(
        &self,
        replace_start: u32,
        replace_end_exclusive: u32,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{EditorRevision, Utf8ByteRange};
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let current_text = s.kernel.text();
            let replace_range = match Utf8ByteRange::try_new(
                current_text,
                replace_start as usize,
                replace_end_exclusive as usize,
            ) {
                Ok(r) => r,
                Err(_) => return EditorEditResultDto::invalid_range_fallback(),
            };
            let result = s.kernel.apply(EditorCommand::BeginComposition {
                replace_range,
                expected_revision: EditorRevision::new(expected_revision),
            });
            let mut dto: EditorEditResultDto = result.into();
            if dto.outcome == EditorEditOutcomeDto::Applied
                || dto.outcome == EditorEditOutcomeDto::AppliedWithAdjustedSelection
            {
                if let Some((session_id, base_revision, generation)) =
                    s.kernel.composition_session_info()
                {
                    dto.composition_session = Some(crate::api::types::CompositionSessionDto {
                        session_id,
                        base_revision,
                        generation,
                    });
                }
            }
            dto
        })
    }

    pub fn editor_kernel_update_composition(
        &self,
        composition_session_id: u64,
        composition_generation: u64,
        new_preedit_text: String,
        new_preedit_cursor_offset: u32,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{
            EditorRevision, EditorSessionGeneration, EditorSessionId, Utf8ByteOffset,
        };
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let preedit_utf16_len: usize = new_preedit_text.chars().map(|c| c.len_utf16()).sum();
            if new_preedit_cursor_offset as usize > preedit_utf16_len {
                return EditorEditResultDto::invalid_offset_fallback();
            }
            let result = s.kernel.apply(EditorCommand::UpdateComposition {
                composition_session_id: match EditorSessionId::try_new(composition_session_id) {
                    Ok(id) => id,
                    Err(_) => return EditorEditResultDto::stale_fallback(),
                },
                composition_generation: EditorSessionGeneration::new(composition_generation),
                new_preedit_text,
                new_preedit_cursor_offset: Utf8ByteOffset::unchecked(
                    new_preedit_cursor_offset as usize,
                ),
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into()
        })
    }

    pub fn editor_kernel_finish_composition(
        &self,
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{
            EditorRevision, EditorSessionGeneration, EditorSessionId,
        };
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::FinishComposition {
                composition_session_id: match EditorSessionId::try_new(composition_session_id) {
                    Ok(id) => id,
                    Err(_) => return EditorEditResultDto::stale_fallback(),
                },
                composition_generation: EditorSessionGeneration::new(composition_generation),
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into()
        })
    }

    pub fn editor_kernel_cancel_composition(
        &self,
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{
            EditorRevision, EditorSessionGeneration, EditorSessionId,
        };
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::CancelComposition {
                composition_session_id: match EditorSessionId::try_new(composition_session_id) {
                    Ok(id) => id,
                    Err(_) => return EditorEditResultDto::stale_fallback(),
                },
                composition_generation: EditorSessionGeneration::new(composition_generation),
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into()
        })
    }

    pub fn editor_kernel_composition_session_info(&self) -> Option<(u64, u64, u64)> {
        self.with_session(|s| s.kernel.composition_session_info())
    }

    pub fn editor_kernel_set_animation_enabled(&self, enabled: u8) {
        self.with_session(|s| {
            s.kernel.set_animation_enabled(enabled != 0);
        })
    }

    pub fn editor_kernel_set_animation_duration_ms(&self, duration_ms: u64) {
        self.with_session(|s| {
            s.kernel.set_animation_duration_ms(duration_ms);
        })
    }

    pub fn editor_kernel_get_text(&self) -> String {
        self.with_session(|s| s.kernel.text().to_string())
    }

    pub fn editor_kernel_get_revision(&self) -> u64 {
        self.with_session(|s| s.kernel.revision())
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn editor_kernel_get_cursor(&self) -> u32 {
        self.with_session(|s| s.kernel.cursor() as u32)
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn editor_kernel_get_selection_anchor(&self) -> u32 {
        self.with_session(|s| s.kernel.selection_anchor() as u32)
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn editor_kernel_session_snapshot(&self) -> EditorSessionSnapshotDto {
        self.with_session(|s| EditorSessionSnapshotDto {
            text: s.kernel.text().to_string(),
            revision: s.kernel.revision(),
            cursor: s.kernel.cursor() as u32,
            selection_anchor: s.kernel.selection_anchor() as u32,
            generation: s.generation,
            chapter_id: s.chapter_id.clone().unwrap_or_default(),
        })
    }

    /// #606: 返回严格在 `byte_offset` 之前的最近 grapheme cluster 边界（UTF-8 byte offset）。
    ///
    /// 平台端 Backspace/Delete 的 grapheme 边界计算由 Core 唯一决定，
    /// 不再依赖 ICU BreakIterator。
    pub fn editor_kernel_previous_grapheme_boundary(&self, byte_offset: u32) -> u32 {
        self.with_session(|s| s.kernel.previous_grapheme_boundary(byte_offset))
    }

    /// #606: 返回严格在 `byte_offset` 之后的最近 grapheme cluster 边界（UTF-8 byte offset）。
    ///
    /// 平台端 Backspace/Delete 的 grapheme 边界计算由 Core 唯一决定，
    /// 不再依赖 ICU BreakIterator。
    pub fn editor_kernel_next_grapheme_boundary(&self, byte_offset: u32) -> u32 {
        self.with_session(|s| s.kernel.next_grapheme_boundary(byte_offset))
    }

    /// #606: 计算旧事务逻辑 slice → 新事务逻辑 slice 的对应关系。
    ///
    /// 平台无关的唯一事实来源 — Android `RebasePlanner` 不再自己匹配，
    /// 直接消费此结果。方法本身不依赖 editor session 状态（无 `with_session`），
    /// 但放在 `WriterAppService` 上以保持 UniFFI 接口聚合。
    pub fn editor_kernel_compute_rebase_slice_mappings(
        &self,
        old_slice_roles: Vec<AnimatedSliceRoleDto>,
        old_slice_byte_ranges: Vec<EditorByteRangeDto>,
        new_slice_roles: Vec<AnimatedSliceRoleDto>,
        new_slice_byte_ranges: Vec<EditorByteRangeDto>,
    ) -> Vec<RebaseSliceMappingDto> {
        let old_roles: Vec<crate::editor::AnimatedSliceRole> =
            old_slice_roles.into_iter().map(Into::into).collect();
        let new_roles: Vec<crate::editor::AnimatedSliceRole> =
            new_slice_roles.into_iter().map(Into::into).collect();
        let old_ranges: Vec<(usize, usize)> = old_slice_byte_ranges
            .into_iter()
            .map(|r| (r.start as usize, r.end_exclusive as usize))
            .collect();
        let new_ranges: Vec<(usize, usize)> = new_slice_byte_ranges
            .into_iter()
            .map(|r| (r.start as usize, r.end_exclusive as usize))
            .collect();
        let mappings = crate::editor::compute_rebase_slice_mappings(
            &old_roles,
            &old_ranges,
            &new_roles,
            &new_ranges,
        );
        mappings.into_iter().map(Into::into).collect()
    }

    pub fn editor_kernel_replace_all(
        &self,
        search: String,
        replacement: String,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::EditorRevision;
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::ReplaceAll {
                search,
                replacement,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_insert_line_break(
        &self,
        byte_offset: u32,
        auto_indent_enabled: u8,
        cause: EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset};
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let current_text = s.kernel.text();
            let offset = match Utf8ByteOffset::try_new(current_text, byte_offset as usize) {
                Ok(o) => o,
                Err(_) => return EditorEditResultDto::invalid_offset_fallback(),
            };
            let result = s.kernel.apply(EditorCommand::InsertLineBreak {
                byte_offset: offset,
                auto_indent_enabled: auto_indent_enabled != 0,
                cause: core_cause,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into_result().into()
        })
    }
}
