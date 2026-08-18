use crate::api::{
    AnimatedSliceRoleDto, EditorByteRangeDto, EditorCompositionStateDto, EditorEditOutcomeDto,
    EditorEditResultDto, EditorSessionSnapshotDto, EditorTransactionCauseDto, OffsetMapDto,
    RebaseSliceMappingDto,
};

/// #629 评论6 Part B：把 EditorKernel.composition_state() 返回的元组转成 DTO。
///
/// 元组字段顺序：(session_id, base_revision, generation, replace_byte_start,
/// replace_byte_end_exclusive, preedit_text, preedit_cursor_utf16)。
/// 调用方已在 composition 活跃且 outcome 为 Applied/AppliedWithAdjustedSelection 时调用，
/// 此处不再做守卫，只做类型转换。
fn make_composition_state_dto(
    state: (u64, u64, u64, u32, u32, String, u32),
) -> EditorCompositionStateDto {
    EditorCompositionStateDto {
        session_id: state.0,
        base_revision: state.1,
        generation: state.2,
        replace_byte_start: state.3,
        replace_byte_end_exclusive: state.4,
        preedit_text: state.5,
        preedit_cursor_utf16: state.6,
    }
}

/// #629 评论8 第4项：统一 composition 回填出口。
///
/// 任何 Core 命令成功后（Applied/AppliedWithAdjustedSelection/NoChange），只要 kernel 里
/// composition 仍然活跃，DTO 的 `composition_session`/`composition` 就必须反映真实 kernel 状态；
/// 不要只在 begin/update 两个函数里手工补字段。setSelection 等不会结束 composition 的命令
/// 也走同一个出口，避免再次出现 Core 有 composition、平台 DTO 却是 null。
///
/// finish/cancel 成功后 kernel 已无活跃 composition，保持 None；失败命令（stale/invalid）
/// 不写回，平台端收到失败后从 snapshot() 恢复（snapshot 自带当前 composition）。
fn backfill_active_composition(
    dto: &mut EditorEditResultDto,
    kernel: &crate::editor::EditorKernel,
) {
    if dto.outcome != EditorEditOutcomeDto::Applied
        && dto.outcome != EditorEditOutcomeDto::AppliedWithAdjustedSelection
        && dto.outcome != EditorEditOutcomeDto::NoChange
    {
        return;
    }
    if let Some((cs_id, base_rev, gen)) = kernel.composition_session_info() {
        dto.composition_session = Some(crate::api::types::CompositionSessionDto {
            session_id: cs_id,
            base_revision: base_rev,
            generation: gen,
        });
    }
    if let Some(state) = kernel.composition_state() {
        dto.composition = Some(make_composition_state_dto(state));
    }
}

impl super::WriterAppService {
    fn with_registry<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut crate::editor::TextEditSessionRegistry) -> R,
    {
        let mut registry = self
            .session_registry
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        f(&mut registry)
    }

    fn with_session_in_registry<F, R>(&self, session_id: u64, f: F) -> Option<R>
    where
        F: FnOnce(&mut crate::editor::TextEditSession) -> R,
    {
        let mut registry = self
            .session_registry
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        registry
            .get_session_mut(crate::editor::TextEditSessionId::new(session_id))
            .map(f)
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
            Ok(id) => Some(id.as_u64()),
            Err(_) => None,
        }
    }

    pub fn text_edit_session_close(&self, session_id: u64) -> u8 {
        self.with_registry(|r| {
            if r.close_session(crate::editor::TextEditSessionId::new(session_id)) {
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
        use crate::editor::strong_types::Utf8ByteOffset;
        match self.with_session_in_registry(session_id, |s| {
            let offset = match Utf8ByteOffset::try_new(&text, cursor_byte_offset as usize) {
                Ok(o) => o,
                Err(_) => return false,
            };
            s.generation = s.generation.saturating_add(1);
            let result = s.kernel.load_text(text, offset.value());
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
        use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset};
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.rope();
            let offset = match Utf8ByteOffset::try_new_rope(current_text, byte_offset as usize) {
                Ok(o) => o,
                Err(_) => return EditorEditResultDto::invalid_offset_fallback(),
            };
            let result = s.kernel.apply(EditorCommand::Insert {
                byte_offset: offset,
                text,
                cause: core_cause,
                expected_revision: EditorRevision::new(expected_revision),
            });
            let mut dto: EditorEditResultDto = result.into_result().into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
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
        use crate::editor::strong_types::{EditorRevision, Utf8ByteRange};
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.rope();
            let byte_range = match Utf8ByteRange::try_new_rope(
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
            let mut dto: EditorEditResultDto = result.into_result().into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
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
        use crate::editor::strong_types::{EditorRevision, Utf8ByteRange};
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.rope();
            let byte_range = match Utf8ByteRange::try_new_rope(
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
            let mut dto: EditorEditResultDto = result.into_result().into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
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
        use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset};
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.rope();
            let anchor =
                match Utf8ByteOffset::try_new_rope(current_text, anchor_byte_offset as usize) {
                    Ok(o) => o,
                    Err(_) => return EditorEditResultDto::invalid_offset_fallback(),
                };
            let head = match Utf8ByteOffset::try_new_rope(current_text, head_byte_offset as usize) {
                Ok(o) => o,
                Err(_) => return EditorEditResultDto::invalid_offset_fallback(),
            };
            let result = s.kernel.apply(EditorCommand::SetSelection {
                anchor,
                head,
                expected_revision: EditorRevision::new(expected_revision),
            });
            let mut dto: EditorEditResultDto = result.into_result().into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_undo(
        &self,
        session_id: u64,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::EditorRevision;
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::Undo {
                expected_revision: EditorRevision::new(expected_revision),
            });
            let mut dto: EditorEditResultDto = result.into_result().into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_redo(
        &self,
        session_id: u64,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::EditorRevision;
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::Redo {
                expected_revision: EditorRevision::new(expected_revision),
            });
            let mut dto: EditorEditResultDto = result.into_result().into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_load_text(
        &self,
        session_id: u64,
        text: String,
        cursor_byte_offset: u32,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::Utf8ByteOffset;
        self.with_session_in_registry(session_id, |s| {
            let offset = match Utf8ByteOffset::try_new(&text, cursor_byte_offset as usize) {
                Ok(o) => o,
                Err(_) => return EditorEditResultDto::invalid_offset_fallback(),
            };
            s.generation = s.generation.saturating_add(1);
            let result = s.kernel.load_text(text, offset.value());
            let mut dto: EditorEditResultDto = result.into_result().into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
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
        use crate::editor::strong_types::{
            EditorRevision, EditorSessionGeneration, EditorSessionId, Utf8ByteOffset, Utf8ByteRange,
        };
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.rope();
            let byte_range = match Utf8ByteRange::try_new_rope(
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
            let mut dto: EditorEditResultDto = result.into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
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
        use crate::editor::strong_types::{EditorRevision, Utf8ByteRange};
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.rope();
            let before_byte_range = match Utf8ByteRange::try_new_rope(
                current_text,
                before_byte_start as usize,
                before_byte_end_exclusive as usize,
            ) {
                Ok(r) => r,
                Err(_) => return EditorEditResultDto::invalid_range_fallback(),
            };
            let after_byte_range = match Utf8ByteRange::try_new_rope(
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
            let mut dto: EditorEditResultDto = result.into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn text_edit_session_begin_composition(
        &self,
        session_id: u64,
        replace_start: u32,
        replace_end_exclusive: u32,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{EditorRevision, Utf8ByteRange};
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.rope();
            let replace_range = match Utf8ByteRange::try_new_rope(
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
            // #629 评论8 第4项：统一回填出口（begin 时 preedit_text 为空，但 composition 已活跃）。
            backfill_active_composition(&mut dto, &s.kernel);
            dto
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    #[allow(clippy::excessive_nesting)]
    pub fn text_edit_session_update_composition(
        &self,
        session_id: u64,
        composition_session_id: u64,
        composition_generation: u64,
        new_preedit_text: String,
        new_preedit_cursor_utf16: u32,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{
            EditorRevision, EditorSessionGeneration, EditorSessionId, Utf16CodeUnitOffset,
        };
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let preedit_utf16_len: usize = new_preedit_text.chars().map(|c| c.len_utf16()).sum();
            if new_preedit_cursor_utf16 as usize > preedit_utf16_len {
                return EditorEditResultDto::invalid_offset_fallback();
            }
            let result = s.kernel.apply(EditorCommand::UpdateComposition {
                composition_session_id: match EditorSessionId::try_new(composition_session_id) {
                    Ok(id) => id,
                    Err(_) => return EditorEditResultDto::stale_fallback(),
                },
                composition_generation: EditorSessionGeneration::new(composition_generation),
                new_preedit_text,
                new_preedit_cursor_utf16: Utf16CodeUnitOffset::unchecked(
                    new_preedit_cursor_utf16 as usize,
                ),
                expected_revision: EditorRevision::new(expected_revision),
            });
            let mut dto: EditorEditResultDto = result.into();
            // #629 评论8 第4项：统一回填出口（update 后 preedit_text 为最新值）。
            backfill_active_composition(&mut dto, &s.kernel);
            dto
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
        use crate::editor::strong_types::{
            EditorRevision, EditorSessionGeneration, EditorSessionId,
        };
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::FinishComposition {
                composition_session_id: match EditorSessionId::try_new(composition_session_id) {
                    Ok(id) => id,
                    Err(_) => return EditorEditResultDto::stale_fallback(),
                },
                composition_generation: EditorSessionGeneration::new(composition_generation),
                expected_revision: EditorRevision::new(expected_revision),
            });
            let mut dto: EditorEditResultDto = result.into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
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
        use crate::editor::strong_types::{
            EditorRevision, EditorSessionGeneration, EditorSessionId,
        };
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::CancelComposition {
                composition_session_id: match EditorSessionId::try_new(composition_session_id) {
                    Ok(id) => id,
                    Err(_) => return EditorEditResultDto::stale_fallback(),
                },
                composition_generation: EditorSessionGeneration::new(composition_generation),
                expected_revision: EditorRevision::new(expected_revision),
            });
            let mut dto: EditorEditResultDto = result.into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    // #629 R8: composition 专用 grapheme 语义操作。
    // 只改 composition session 的 preeditText / preeditCursorUtf16 / generation；
    // 不修改 committed 正文，不把 raw platform event 带入 Core。

    pub fn text_edit_session_composition_move_grapheme_left(
        &self,
        session_id: u64,
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{
            EditorRevision, EditorSessionGeneration, EditorSessionId,
        };
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::CompositionMoveGraphemeLeft {
                composition_session_id: match EditorSessionId::try_new(composition_session_id) {
                    Ok(id) => id,
                    Err(_) => return EditorEditResultDto::stale_fallback(),
                },
                composition_generation: EditorSessionGeneration::new(composition_generation),
                expected_revision: EditorRevision::new(expected_revision),
            });
            let mut dto: EditorEditResultDto = result.into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_composition_move_grapheme_right(
        &self,
        session_id: u64,
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{
            EditorRevision, EditorSessionGeneration, EditorSessionId,
        };
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::CompositionMoveGraphemeRight {
                composition_session_id: match EditorSessionId::try_new(composition_session_id) {
                    Ok(id) => id,
                    Err(_) => return EditorEditResultDto::stale_fallback(),
                },
                composition_generation: EditorSessionGeneration::new(composition_generation),
                expected_revision: EditorRevision::new(expected_revision),
            });
            let mut dto: EditorEditResultDto = result.into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_composition_delete_grapheme_backward(
        &self,
        session_id: u64,
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{
            EditorRevision, EditorSessionGeneration, EditorSessionId,
        };
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s
                .kernel
                .apply(EditorCommand::CompositionDeleteGraphemeBackward {
                    composition_session_id: match EditorSessionId::try_new(composition_session_id) {
                        Ok(id) => id,
                        Err(_) => return EditorEditResultDto::stale_fallback(),
                    },
                    composition_generation: EditorSessionGeneration::new(composition_generation),
                    expected_revision: EditorRevision::new(expected_revision),
                });
            let mut dto: EditorEditResultDto = result.into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_composition_delete_grapheme_forward(
        &self,
        session_id: u64,
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{
            EditorRevision, EditorSessionGeneration, EditorSessionId,
        };
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s
                .kernel
                .apply(EditorCommand::CompositionDeleteGraphemeForward {
                    composition_session_id: match EditorSessionId::try_new(composition_session_id) {
                        Ok(id) => id,
                        Err(_) => return EditorEditResultDto::stale_fallback(),
                    },
                    composition_generation: EditorSessionGeneration::new(composition_generation),
                    expected_revision: EditorRevision::new(expected_revision),
                });
            let mut dto: EditorEditResultDto = result.into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_set_animation_enabled(&self, session_id: u64, enabled: u8) {
        self.with_session_in_registry(session_id, |s| {
            s.kernel.set_animation_enabled(enabled != 0);
        });
    }

    pub fn text_edit_session_set_animation_duration_ms(&self, session_id: u64, duration_ms: u64) {
        self.with_session_in_registry(session_id, |s| {
            s.kernel.set_animation_duration_ms(duration_ms);
        });
    }

    pub fn text_edit_session_get_text(&self, session_id: u64) -> String {
        self.with_session_in_registry(session_id, |s| s.kernel.snapshot_text())
            .unwrap_or_default()
    }

    pub fn text_edit_session_get_revision(&self, session_id: u64) -> u64 {
        self.with_session_in_registry(session_id, |s| s.kernel.revision())
            .unwrap_or(0)
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn text_edit_session_snapshot(&self, session_id: u64) -> EditorSessionSnapshotDto {
        self.with_session_in_registry(session_id, |s| EditorSessionSnapshotDto {
            text: s.kernel.snapshot_text(),
            revision: s.kernel.revision(),
            cursor: s.kernel.cursor() as u32,
            selection_anchor: s.kernel.selection_anchor() as u32,
            generation: s.generation,
            chapter_id: s.target_id.clone(),
            // #629 评论6 Part B：composition 活跃时返回当前 composition 完整状态，
            // 平台端据此构造临时显示文本和下划线。无 composition 时为 None。
            composition: s.kernel.composition_state().map(make_composition_state_dto),
        })
        .unwrap_or_else(|| EditorSessionSnapshotDto {
            text: String::new(),
            revision: 0,
            cursor: 0,
            selection_anchor: 0,
            generation: 0,
            chapter_id: String::new(),
            composition: None,
        })
    }

    pub fn text_edit_session_replace_all(
        &self,
        session_id: u64,
        search: String,
        replacement: String,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::EditorRevision;
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::ReplaceAll {
                search,
                replacement,
                expected_revision: EditorRevision::new(expected_revision),
            });
            let mut dto: EditorEditResultDto = result.into_result().into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_insert_line_break(
        &self,
        session_id: u64,
        byte_offset: u32,
        auto_indent_enabled: u8,
        cause: EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> EditorEditResultDto {
        use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset};
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let current_text = s.kernel.rope();
            let offset = match Utf8ByteOffset::try_new_rope(current_text, byte_offset as usize) {
                Ok(o) => o,
                Err(_) => return EditorEditResultDto::invalid_offset_fallback(),
            };
            let result = s.kernel.apply(EditorCommand::InsertLineBreak {
                byte_offset: offset,
                auto_indent_enabled: auto_indent_enabled != 0,
                cause: core_cause,
                expected_revision: EditorRevision::new(expected_revision),
            });
            let mut dto: EditorEditResultDto = result.into_result().into();
            backfill_active_composition(&mut dto, &s.kernel);
            dto
        })
        .unwrap_or_else(EditorEditResultDto::stale_fallback)
    }

    /// #606: 返回严格在 `byte_offset` 之前的最近 grapheme cluster 边界（UTF-8 byte offset）。
    ///
    /// session-scoped 版本，与 [editor_kernel_previous_grapheme_boundary] 语义一致，
    /// 但作用于指定 session 的 EditorKernel。平台端 TextEditSessionBridge 通过此方法
    /// 调用 Core 的 unicode_segmentation，不再依赖 ICU BreakIterator。
    pub fn text_edit_session_previous_grapheme_boundary(
        &self,
        session_id: u64,
        byte_offset: u32,
    ) -> u32 {
        self.with_session_in_registry(session_id, |s| {
            s.kernel.previous_grapheme_boundary(byte_offset)
        })
        .unwrap_or(byte_offset)
    }

    /// #606: 返回严格在 `byte_offset` 之后的最近 grapheme cluster 边界（UTF-8 byte offset）。
    ///
    /// session-scoped 版本，与 [editor_kernel_next_grapheme_boundary] 语义一致，
    /// 但作用于指定 session 的 EditorKernel。平台端 TextEditSessionBridge 通过此方法
    /// 调用 Core 的 unicode_segmentation，不再依赖 ICU BreakIterator。
    pub fn text_edit_session_next_grapheme_boundary(
        &self,
        session_id: u64,
        byte_offset: u32,
    ) -> u32 {
        self.with_session_in_registry(session_id, |s| s.kernel.next_grapheme_boundary(byte_offset))
            .unwrap_or(byte_offset)
    }

    /// #606: 计算旧事务逻辑 slice → 新事务逻辑 slice 的对应关系。
    ///
    /// session-scoped 版本，与 [editor_kernel_compute_rebase_slice_mappings] 语义一致，
    /// 但绑定到指定 session。匹配依据只用 byte range/OffsetMap/角色兼容，
    /// 不使用像素坐标 — 平台端 RebasePlanner 不再自己匹配。
    pub fn text_edit_session_compute_rebase_slice_mappings(
        &self,
        session_id: u64,
        old_slice_roles: Vec<AnimatedSliceRoleDto>,
        old_slice_byte_ranges: Vec<EditorByteRangeDto>,
        new_slice_roles: Vec<AnimatedSliceRoleDto>,
        new_slice_byte_ranges: Vec<EditorByteRangeDto>,
        offset_map: Option<OffsetMapDto>,
    ) -> Vec<RebaseSliceMappingDto> {
        // 校验 session 存在（映射本身不依赖 session 状态，但保持 session-scoped 契约：
        // 不存在的 session 返回空映射，平台端按 End 处理）。
        if self.with_session_in_registry(session_id, |_| ()).is_none() {
            return Vec::new();
        }
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
        let core_offset_map: Option<crate::editor::OffsetMap> = offset_map.map(Into::into);
        let mappings =
            crate::editor::compute_rebase_slice_mappings(crate::editor::SliceMatchInput {
                old_slice_roles: &old_roles,
                old_slice_byte_ranges: &old_ranges,
                new_slice_roles: &new_roles,
                new_slice_byte_ranges: &new_ranges,
                offset_map: core_offset_map.as_ref(),
            });
        mappings.into_iter().map(Into::into).collect()
    }
}

#[cfg(test)]
#[path = "text_edit_session_ops_tests.rs"]
mod tests;
