use crate::api::{EditorEditResultDto, EditorEditOutcomeDto, EditorSessionSnapshotDto, EditorTransactionCauseDto, EditorVisualIntentDto};

use super::EditorSession;

impl super::WriterAppService {
    fn with_session<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut EditorSession) -> R,
    {
        let mut session = self.editor_session.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut session)
    }

    pub fn editor_kernel_insert(
        &self,
        byte_offset: u32,
        text: String,
        cause: EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(byte_offset as usize),
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
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Delete {
                byte_range: Utf8ByteRange::new(byte_start as usize, byte_end_exclusive as usize).unwrap_or(Utf8ByteRange::new(0, 0).unwrap()),
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
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Replace {
                byte_range: Utf8ByteRange::new(byte_start as usize, byte_end_exclusive as usize).unwrap_or(Utf8ByteRange::new(0, 0).unwrap()),
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
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::SetSelection {
                anchor: Utf8ByteOffset::unchecked(anchor_byte_offset as usize),
                head: Utf8ByteOffset::unchecked(head_byte_offset as usize),
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_undo(&self, expected_revision: u64) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Undo { expected_revision: EditorRevision::new(expected_revision) });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_redo(&self, expected_revision: u64) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Redo { expected_revision: EditorRevision::new(expected_revision) });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_load_text(
        &self,
        text: String,
        cursor_byte_offset: u32,
    ) -> EditorEditResultDto {
        self.with_session(|s| {
            s.generation = s.generation.saturating_add(1);
            let result = s.kernel.load_text(text, cursor_byte_offset as usize);
            result.into_result().into()
        })
    }

    pub fn editor_kernel_composition_update_visual_intent(
        &self,
        composition_replace_start: u32,
        composition_replace_end_exclusive: u32,
        old_preedit_text: String,
        new_preedit_text: String,
    ) -> EditorVisualIntentDto {
        self.with_session(|s| {
            let replace_range = if composition_replace_start < composition_replace_end_exclusive {
                Some((composition_replace_start as usize, composition_replace_end_exclusive as usize))
            } else {
                None
            };
            let intent = s.kernel.composition_update_visual_intent(
                replace_range,
                &old_preedit_text,
                &new_preedit_text,
            );
            intent.into()
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
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::CommitText {
                byte_range: Utf8ByteRange::new(byte_start as usize, byte_end_exclusive as usize).unwrap_or(Utf8ByteRange::new(0, 0).unwrap()),
                replacement_text,
                resulting_selection_anchor: Utf8ByteOffset::unchecked(resulting_selection_anchor as usize),
                resulting_selection_head: Utf8ByteOffset::unchecked(resulting_selection_head as usize),
                composition_session_id: EditorSessionId::new(composition_session_id),
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
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::DeleteSurrounding {
                before_byte_range: Utf8ByteRange::new(before_byte_start as usize, before_byte_end_exclusive as usize).unwrap_or(Utf8ByteRange::new(0, 0).unwrap()),
                after_byte_range: Utf8ByteRange::new(after_byte_start as usize, after_byte_end_exclusive as usize).unwrap_or(Utf8ByteRange::new(0, 0).unwrap()),
                cause: core_cause,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into()
        })
    }

    pub fn editor_kernel_begin_composition(
        &self,
        replace_start: u32,
        replace_end_exclusive: u32,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::BeginComposition {
                replace_range: Utf8ByteRange::new(replace_start as usize, replace_end_exclusive as usize).unwrap_or(Utf8ByteRange::new(0, 0).unwrap()),
                expected_revision: EditorRevision::new(expected_revision),
            });
            let mut dto: EditorEditResultDto = result.into();
            if dto.outcome == EditorEditOutcomeDto::Applied
                || dto.outcome == EditorEditOutcomeDto::AppliedWithAdjustedSelection
            {
                if let Some((session_id, base_revision, generation)) = s.kernel.composition_session_info() {
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
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::UpdateComposition {
                composition_session_id: EditorSessionId::new(composition_session_id),
                composition_generation: EditorSessionGeneration::new(composition_generation),
                new_preedit_text,
                new_preedit_cursor_offset: Utf8ByteOffset::unchecked(new_preedit_cursor_offset as usize),
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
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::FinishComposition {
                composition_session_id: EditorSessionId::new(composition_session_id),
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
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::CancelComposition {
                composition_session_id: EditorSessionId::new(composition_session_id),
                composition_generation: EditorSessionGeneration::new(composition_generation),
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into()
        })
    }

    pub fn editor_kernel_composition_session_info(&self) -> Option<(u64, u64, u64)> {
        self.with_session(|s| {
            s.kernel.composition_session_info()
        })
    }

    pub fn editor_kernel_set_animation_enabled(
        &self,
        enabled: u8,
    ) {
        self.with_session(|s| {
            s.kernel.set_animation_enabled(enabled != 0);
        })
    }

    pub fn editor_kernel_set_animation_duration_ms(
        &self,
        duration_ms: u64,
    ) {
        self.with_session(|s| {
            s.kernel.set_animation_duration_ms(duration_ms);
        })
    }

    pub fn editor_kernel_get_text(&self) -> String {
        self.with_session(|s| {
            s.kernel.text().to_string()
        })
    }

    pub fn editor_kernel_get_revision(&self) -> u64 {
        self.with_session(|s| {
            s.kernel.revision()
        })
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn editor_kernel_get_cursor(&self) -> u32 {
        self.with_session(|s| {
            s.kernel.cursor() as u32
        })
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn editor_kernel_get_selection_anchor(&self) -> u32 {
        self.with_session(|s| {
            s.kernel.selection_anchor() as u32
        })
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn editor_kernel_session_snapshot(&self) -> EditorSessionSnapshotDto {
        self.with_session(|s| {
            EditorSessionSnapshotDto {
                text: s.kernel.text().to_string(),
                revision: s.kernel.revision(),
                cursor: s.kernel.cursor() as u32,
                selection_anchor: s.kernel.selection_anchor() as u32,
                generation: s.generation,
                chapter_id: s.chapter_id.clone().unwrap_or_default(),
            }
        })
    }

    pub fn editor_kernel_replace_all(
        &self,
        search: String,
        replacement: String,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
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
        auto_indent_prefix: String,
        cause: EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::InsertLineBreak {
                byte_offset: Utf8ByteOffset::unchecked(byte_offset as usize),
                auto_indent_prefix,
                cause: core_cause,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into_result().into()
        })
    }
}
