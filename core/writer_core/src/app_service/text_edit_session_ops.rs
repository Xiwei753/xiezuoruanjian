use crate::api::{EditorEditResultDto, EditorEditOutcomeDto, EditorSessionSnapshotDto, EditorTransactionCauseDto, EditorVisualIntentDto};

impl super::WriterAppService {
    fn with_registry<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut crate::editor::TextEditSessionRegistry) -> R,
    {
        let mut registry = self.session_registry.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut registry)
    }

    fn with_session_in_registry<F, R>(&self, session_id: u64, f: F) -> Option<R>
    where
        F: FnOnce(&mut crate::editor::TextEditSession) -> R,
    {
        let mut registry = self.session_registry.lock().unwrap_or_else(|e| e.into_inner());
        registry.get_session_mut(crate::editor::TextEditSessionId(session_id)).map(f)
    }

    pub fn text_edit_session_create(
        &self,
        target_id: String,
        initial_text: String,
        initial_cursor_byte_offset: u32,
        is_persistent: u8,
    ) -> Option<u64> {
        let result = self.with_registry(|r| {
            r.create_session(
                target_id,
                initial_text,
                initial_cursor_byte_offset as usize,
                is_persistent != 0,
            )
        });
        match result {
            Ok(id) => Some(id.0),
            Err(_) => None,
        }
    }

    pub fn text_edit_session_close(&self, session_id: u64) -> u8 {
        self.with_registry(|r| {
            if r.close_session(crate::editor::TextEditSessionId(session_id)) {
                1u8
            } else {
                0u8
            }
        })
    }

    pub fn text_edit_session_reset(
        &self,
        session_id: u64,
        text: String,
        cursor_byte_offset: u32,
    ) -> u8 {
        match self.with_session_in_registry(session_id, |s| {
            s.generation = s.generation.saturating_add(1);
            let result = s.kernel.load_text(text, cursor_byte_offset as usize);
            result.is_applied()
        }) {
            Some(true) => 1u8,
            _ => 0u8,
        }
    }

    pub fn text_edit_session_insert(
        &self,
        session_id: u64,
        byte_offset: u32,
        text: String,
        cause: EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.text();
            let result = s.kernel.apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::new(current_text, byte_offset as usize),
                text,
                cause: core_cause,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into_result().into()
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_delete(
        &self,
        session_id: u64,
        byte_start: u32,
        byte_end_exclusive: u32,
        cause: EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.text();
            let result = s.kernel.apply(EditorCommand::Delete {
                byte_range: Utf8ByteRange::new_checked(current_text, byte_start as usize, byte_end_exclusive as usize).unwrap_or(Utf8ByteRange::new(0, 0).unwrap()),
                deleted_text: String::new(),
                cause: core_cause,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into_result().into()
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text_edit_session_replace(
        &self,
        session_id: u64,
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
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.text();
            let result = s.kernel.apply(EditorCommand::Replace {
                byte_range: Utf8ByteRange::new_checked(current_text, byte_start as usize, byte_end_exclusive as usize).unwrap_or(Utf8ByteRange::new(0, 0).unwrap()),
                replacement_text,
                original_text,
                cause: core_cause,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into_result().into()
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_set_selection(
        &self,
        session_id: u64,
        anchor_byte_offset: u32,
        head_byte_offset: u32,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.text();
            let result = s.kernel.apply(EditorCommand::SetSelection {
                anchor: Utf8ByteOffset::new(current_text, anchor_byte_offset as usize),
                head: Utf8ByteOffset::new(current_text, head_byte_offset as usize),
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into_result().into()
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_undo(
        &self,
        session_id: u64,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::Undo { expected_revision: EditorRevision::new(expected_revision) });
            result.into_result().into()
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_redo(
        &self,
        session_id: u64,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::Redo { expected_revision: EditorRevision::new(expected_revision) });
            result.into_result().into()
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_load_text(
        &self,
        session_id: u64,
        text: String,
        cursor_byte_offset: u32,
    ) -> EditorEditResultDto {
        self.with_session_in_registry(session_id, |s| {
            s.generation = s.generation.saturating_add(1);
            let result = s.kernel.load_text(text, cursor_byte_offset as usize);
            result.into_result().into()
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text_edit_session_commit_text(
        &self,
        session_id: u64,
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
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.text();
            let result = s.kernel.apply(EditorCommand::CommitText {
                byte_range: Utf8ByteRange::new_checked(current_text, byte_start as usize, byte_end_exclusive as usize).unwrap_or(Utf8ByteRange::new(0, 0).unwrap()),
                replacement_text,
                resulting_selection_anchor: Utf8ByteOffset::new(current_text, resulting_selection_anchor as usize),
                resulting_selection_head: Utf8ByteOffset::new(current_text, resulting_selection_head as usize),
                composition_session_id: EditorSessionId::new(composition_session_id),
                composition_base_revision: EditorRevision::new(composition_base_revision),
                composition_generation: EditorSessionGeneration::new(composition_generation),
                cause: core_cause,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into()
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text_edit_session_delete_surrounding(
        &self,
        session_id: u64,
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
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.text();
            let result = s.kernel.apply(EditorCommand::DeleteSurrounding {
                before_byte_range: Utf8ByteRange::new_checked(current_text, before_byte_start as usize, before_byte_end_exclusive as usize).unwrap_or(Utf8ByteRange::new(0, 0).unwrap()),
                after_byte_range: Utf8ByteRange::new_checked(current_text, after_byte_start as usize, after_byte_end_exclusive as usize).unwrap_or(Utf8ByteRange::new(0, 0).unwrap()),
                cause: core_cause,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into()
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_begin_composition(
        &self,
        session_id: u64,
        replace_start: u32,
        replace_end_exclusive: u32,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.text();
            let result = s.kernel.apply(EditorCommand::BeginComposition {
                replace_range: Utf8ByteRange::new_checked(current_text, replace_start as usize, replace_end_exclusive as usize).unwrap_or(Utf8ByteRange::new(0, 0).unwrap()),
                expected_revision: EditorRevision::new(expected_revision),
            });
            let mut dto: EditorEditResultDto = result.into();
            if dto.outcome == EditorEditOutcomeDto::Applied
                || dto.outcome == EditorEditOutcomeDto::AppliedWithAdjustedSelection
            {
                if let Some((cs_id, base_rev, gen)) = s.kernel.composition_session_info() {
                    dto.composition_session = Some(crate::api::types::CompositionSessionDto {
                        session_id: cs_id,
                        base_revision: base_rev,
                        generation: gen,
                    });
                }
            }
            dto
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_update_composition(
        &self,
        session_id: u64,
        composition_session_id: u64,
        composition_generation: u64,
        new_preedit_text: String,
        new_preedit_cursor_offset: u32,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.text();
            let result = s.kernel.apply(EditorCommand::UpdateComposition {
                composition_session_id: EditorSessionId::new(composition_session_id),
                composition_generation: EditorSessionGeneration::new(composition_generation),
                new_preedit_text,
                new_preedit_cursor_offset: Utf8ByteOffset::new(current_text, new_preedit_cursor_offset as usize),
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into()
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_finish_composition(
        &self,
        session_id: u64,
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::FinishComposition {
                composition_session_id: EditorSessionId::new(composition_session_id),
                composition_generation: EditorSessionGeneration::new(composition_generation),
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into()
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_cancel_composition(
        &self,
        session_id: u64,
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::CancelComposition {
                composition_session_id: EditorSessionId::new(composition_session_id),
                composition_generation: EditorSessionGeneration::new(composition_generation),
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into()
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_composition_update_visual_intent(
        &self,
        session_id: u64,
        composition_replace_start: u32,
        composition_replace_end_exclusive: u32,
        old_preedit_text: String,
        new_preedit_text: String,
    ) -> EditorVisualIntentDto {
        self.with_session_in_registry(session_id, |s| {
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
        .unwrap_or_else(EditorVisualIntentDto::default_fallback)
    }

    pub fn text_edit_session_set_animation_enabled(
        &self,
        session_id: u64,
        enabled: u8,
    ) {
        self.with_session_in_registry(session_id, |s| {
            s.kernel.set_animation_enabled(enabled != 0);
        });
    }

    pub fn text_edit_session_set_animation_duration_ms(
        &self,
        session_id: u64,
        duration_ms: u64,
    ) {
        self.with_session_in_registry(session_id, |s| {
            s.kernel.set_animation_duration_ms(duration_ms);
        });
    }

    pub fn text_edit_session_get_text(&self, session_id: u64) -> String {
        self.with_session_in_registry(session_id, |s| {
            s.kernel.text().to_string()
        })
        .unwrap_or_default()
    }

    pub fn text_edit_session_get_revision(&self, session_id: u64) -> u64 {
        self.with_session_in_registry(session_id, |s| {
            s.kernel.revision()
        })
        .unwrap_or(0)
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn text_edit_session_snapshot(&self, session_id: u64) -> EditorSessionSnapshotDto {
        self.with_session_in_registry(session_id, |s| {
            EditorSessionSnapshotDto {
                text: s.kernel.text().to_string(),
                revision: s.kernel.revision(),
                cursor: s.kernel.cursor() as u32,
                selection_anchor: s.kernel.selection_anchor() as u32,
                generation: s.generation,
                chapter_id: s.target_id.clone(),
            }
        })
        .unwrap_or_else(|| EditorSessionSnapshotDto {
            text: String::new(),
            revision: 0,
            cursor: 0,
            selection_anchor: 0,
            generation: 0,
            chapter_id: String::new(),
        })
    }

    pub fn text_edit_session_replace_all(
        &self,
        session_id: u64,
        search: String,
        replacement: String,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::ReplaceAll {
                search,
                replacement,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into_result().into()
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_insert_line_break(
        &self,
        session_id: u64,
        byte_offset: u32,
        auto_indent_prefix: String,
        cause: EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::EditorCommand;
            use crate::editor::strong_types::{EditorRevision, EditorSessionId, EditorSessionGeneration, Utf8ByteOffset, Utf8ByteRange};
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.text();
            let result = s.kernel.apply(EditorCommand::InsertLineBreak {
                byte_offset: Utf8ByteOffset::new(current_text, byte_offset as usize),
                auto_indent_prefix,
                cause: core_cause,
                expected_revision: EditorRevision::new(expected_revision),
            });
            result.into_result().into()
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }
}
