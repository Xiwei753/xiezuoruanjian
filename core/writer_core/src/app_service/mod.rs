mod project_ops;
mod volume_chapter_ops;
mod settings_ops;
mod sync_ops;
mod stats_ops;
mod theme_ops;
mod starmap_ops;

use crate::api::{WriterCoreApi, WriterError};

use std::sync::Mutex;

struct EditorSession {
    kernel: crate::editor::EditorKernel,
    chapter_id: Option<String>,
    generation: u64,
}

/// Thin UniFFI adapter. Stable Core API behavior lives in `api::WriterCoreApi`.
///
/// ## 线程安全
///
/// `editor_session` 和 `session_registry` 各自用 `Mutex` 保护，保证线程安全。
/// `Mutex` 只在单次 FFI 调用期间持有，不跨调用持有，避免死锁。
/// **不得在持有 `editor_session` 锁的同时获取 `session_registry` 锁**（锁序：先 session 后 registry）。
///
/// ## 双会话路径
///
/// `editor_session` 是旧版正文章节专用路径（单 EditorKernel，单 generation），
/// `session_registry` 是新版多目标会话路径（项目名/章节名/星图标题/正文等，各自独立 EditorKernel 和 generation）。
/// 两者独立维护，不共享 EditorKernel 实例。同一时刻同一章节只能通过一条路径访问。
pub struct WriterAppService {
    api: WriterCoreApi,
    editor_session: Mutex<EditorSession>,
    session_registry: Mutex<crate::editor::TextEditSessionRegistry>,
}

impl WriterAppService {
    pub fn new(workspace_path: String) -> Self {
        Self {
            api: WriterCoreApi::new(workspace_path),
            editor_session: Mutex::new(EditorSession {
                kernel: crate::editor::EditorKernel::new(),
                chapter_id: None,
                generation: 0,
            }),
            session_registry: Mutex::new(crate::editor::TextEditSessionRegistry::new()),
        }
    }

    // ── Actions ──

    pub fn list_registered_actions(
        &self,
    ) -> Result<Vec<crate::api::types::ActionDescriptorDto>, WriterError> {
        self.api.list_registered_actions()
    }

    pub fn execute_action(
        &self,
        action_id: String,
        args_json: String,
        context_json: String,
    ) -> Result<crate::api::types::ActionResultDto, WriterError> {
        self.api
            .execute_action_ext(&action_id, &args_json, &context_json)
    }

    pub fn ai_available(&self) -> bool {
        self.api.ai_available()
    }

    // ── Layout Policy ──

    pub fn resolve_layout(
        &self,
        metrics: crate::api::WindowMetricsDto,
    ) -> crate::api::LayoutPlanDto {
        let core_metrics: crate::layout_policy::WindowMetrics = metrics.into();
        let plan = crate::layout_policy::resolve_layout(&core_metrics);
        plan.into()
    }

    // ── Screen Policy ──

    pub fn resolve_screen_policy(
        &self,
        screen_role: crate::api::ScreenRoleDto,
        shell_mode: crate::api::ShellModeDto,
    ) -> crate::api::ScreenPolicyDto {
        let core_role: crate::screen_policy::ScreenRole = screen_role.into();
        let core_mode: crate::layout_policy::ShellMode = shell_mode.into();
        let action_slots = crate::screen_policy::resolve_screen_policy(core_role, core_mode);
        crate::api::ScreenPolicyDto {
            screen_role: core_role.into(),
            action_slots: action_slots.into_iter().map(Into::into).collect(),
        }
    }

    // ── Legacy Editor Session ──

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
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Insert {
                byte_offset: byte_offset as usize,
                text,
                cause: core_cause,
                expected_revision,
            });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_delete(
        &self,
        byte_start: u32,
        byte_end_exclusive: u32,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Delete {
                byte_start: byte_start as usize,
                byte_end_exclusive: byte_end_exclusive as usize,
                deleted_text: String::new(),
                cause: core_cause,
                expected_revision,
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
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Replace {
                byte_start: byte_start as usize,
                byte_end_exclusive: byte_end_exclusive as usize,
                replacement_text,
                original_text,
                cause: core_cause,
                expected_revision,
            });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_set_selection(
        &self,
        anchor_byte_offset: u32,
        head_byte_offset: u32,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::SetSelection {
                anchor_byte_offset: anchor_byte_offset as usize,
                head_byte_offset: head_byte_offset as usize,
                expected_revision,
            });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_undo(&self, expected_revision: u64) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Undo { expected_revision });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_redo(&self, expected_revision: u64) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::Redo { expected_revision });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_load_text(
        &self,
        text: String,
        cursor_byte_offset: u32,
    ) -> crate::api::EditorEditResultDto {
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
    ) -> crate::api::EditorVisualIntentDto {
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
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::CommitText {
                byte_start: byte_start as usize,
                byte_end_exclusive: byte_end_exclusive as usize,
                replacement_text,
                resulting_selection_anchor: resulting_selection_anchor as usize,
                resulting_selection_head: resulting_selection_head as usize,
                composition_session_id,
                composition_base_revision,
                composition_generation,
                cause: core_cause,
                expected_revision,
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
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::DeleteSurrounding {
                before_byte_start: before_byte_start as usize,
                before_byte_end_exclusive: before_byte_end_exclusive as usize,
                after_byte_start: after_byte_start as usize,
                after_byte_end_exclusive: after_byte_end_exclusive as usize,
                cause: core_cause,
                expected_revision,
            });
            result.into()
        })
    }

    pub fn editor_kernel_begin_composition(
        &self,
        replace_start: u32,
        replace_end_exclusive: u32,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::BeginComposition {
                replace_start: replace_start as usize,
                replace_end_exclusive: replace_end_exclusive as usize,
                expected_revision,
            });
            let mut dto: crate::api::EditorEditResultDto = result.into();
            if dto.outcome == crate::api::EditorEditOutcomeDto::Applied
                || dto.outcome == crate::api::EditorEditOutcomeDto::AppliedWithAdjustedSelection
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
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::UpdateComposition {
                composition_session_id,
                composition_generation,
                new_preedit_text,
                new_preedit_cursor_offset: new_preedit_cursor_offset as usize,
                expected_revision,
            });
            result.into()
        })
    }

    pub fn editor_kernel_finish_composition(
        &self,
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::FinishComposition {
                composition_session_id,
                composition_generation,
                expected_revision,
            });
            result.into()
        })
    }

    pub fn editor_kernel_cancel_composition(
        &self,
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::CancelComposition {
                composition_session_id,
                composition_generation,
                expected_revision,
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
    pub fn editor_kernel_session_snapshot(&self) -> crate::api::EditorSessionSnapshotDto {
        self.with_session(|s| {
            crate::api::EditorSessionSnapshotDto {
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
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::ReplaceAll {
                search,
                replacement,
                expected_revision,
            });
            result.into_result().into()
        })
    }

    pub fn editor_kernel_insert_line_break(
        &self,
        byte_offset: u32,
        auto_indent_prefix: String,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session(|s| {
            let result = s.kernel.apply(EditorCommand::InsertLineBreak {
                byte_offset: byte_offset as usize,
                auto_indent_prefix,
                cause: core_cause,
                expected_revision,
            });
            result.into_result().into()
        })
    }

    // ── Text Edit Session ──

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
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::Insert {
                byte_offset: byte_offset as usize,
                text,
                cause: core_cause,
                expected_revision,
            });
            result.into_result().into()
        })
        .unwrap_or_else(crate::api::EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_delete(
        &self,
        session_id: u64,
        byte_start: u32,
        byte_end_exclusive: u32,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::Delete {
                byte_start: byte_start as usize,
                byte_end_exclusive: byte_end_exclusive as usize,
                deleted_text: String::new(),
                cause: core_cause,
                expected_revision,
            });
            result.into_result().into()
        })
        .unwrap_or_else(crate::api::EditorEditResultDto::stale_fallback)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text_edit_session_replace(
        &self,
        session_id: u64,
        byte_start: u32,
        byte_end_exclusive: u32,
        replacement_text: String,
        original_text: String,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::Replace {
                byte_start: byte_start as usize,
                byte_end_exclusive: byte_end_exclusive as usize,
                replacement_text,
                original_text,
                cause: core_cause,
                expected_revision,
            });
            result.into_result().into()
        })
        .unwrap_or_else(crate::api::EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_set_selection(
        &self,
        session_id: u64,
        anchor_byte_offset: u32,
        head_byte_offset: u32,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::SetSelection {
                anchor_byte_offset: anchor_byte_offset as usize,
                head_byte_offset: head_byte_offset as usize,
                expected_revision,
            });
            result.into_result().into()
        })
        .unwrap_or_else(crate::api::EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_undo(
        &self,
        session_id: u64,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::Undo { expected_revision });
            result.into_result().into()
        })
        .unwrap_or_else(crate::api::EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_redo(
        &self,
        session_id: u64,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::Redo { expected_revision });
            result.into_result().into()
        })
        .unwrap_or_else(crate::api::EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_load_text(
        &self,
        session_id: u64,
        text: String,
        cursor_byte_offset: u32,
    ) -> crate::api::EditorEditResultDto {
        self.with_session_in_registry(session_id, |s| {
            s.generation = s.generation.saturating_add(1);
            let result = s.kernel.load_text(text, cursor_byte_offset as usize);
            result.into_result().into()
        })
        .unwrap_or_else(crate::api::EditorEditResultDto::stale_fallback)
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
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::CommitText {
                byte_start: byte_start as usize,
                byte_end_exclusive: byte_end_exclusive as usize,
                replacement_text,
                resulting_selection_anchor: resulting_selection_anchor as usize,
                resulting_selection_head: resulting_selection_head as usize,
                composition_session_id,
                composition_base_revision,
                composition_generation,
                cause: core_cause,
                expected_revision,
            });
            result.into()
        })
        .unwrap_or_else(crate::api::EditorEditResultDto::stale_fallback)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text_edit_session_delete_surrounding(
        &self,
        session_id: u64,
        before_byte_start: u32,
        before_byte_end_exclusive: u32,
        after_byte_start: u32,
        after_byte_end_exclusive: u32,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::DeleteSurrounding {
                before_byte_start: before_byte_start as usize,
                before_byte_end_exclusive: before_byte_end_exclusive as usize,
                after_byte_start: after_byte_start as usize,
                after_byte_end_exclusive: after_byte_end_exclusive as usize,
                cause: core_cause,
                expected_revision,
            });
            result.into()
        })
        .unwrap_or_else(crate::api::EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_begin_composition(
        &self,
        session_id: u64,
        replace_start: u32,
        replace_end_exclusive: u32,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::BeginComposition {
                replace_start: replace_start as usize,
                replace_end_exclusive: replace_end_exclusive as usize,
                expected_revision,
            });
            let mut dto: crate::api::EditorEditResultDto = result.into();
            if dto.outcome == crate::api::EditorEditOutcomeDto::Applied
                || dto.outcome == crate::api::EditorEditOutcomeDto::AppliedWithAdjustedSelection
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
        .unwrap_or_else(crate::api::EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_update_composition(
        &self,
        session_id: u64,
        composition_session_id: u64,
        composition_generation: u64,
        new_preedit_text: String,
        new_preedit_cursor_offset: u32,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::UpdateComposition {
                composition_session_id,
                composition_generation,
                new_preedit_text,
                new_preedit_cursor_offset: new_preedit_cursor_offset as usize,
                expected_revision,
            });
            result.into()
        })
        .unwrap_or_else(crate::api::EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_finish_composition(
        &self,
        session_id: u64,
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::FinishComposition {
                composition_session_id,
                composition_generation,
                expected_revision,
            });
            result.into()
        })
        .unwrap_or_else(crate::api::EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_cancel_composition(
        &self,
        session_id: u64,
        composition_session_id: u64,
        composition_generation: u64,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::CancelComposition {
                composition_session_id,
                composition_generation,
                expected_revision,
            });
            result.into()
        })
        .unwrap_or_else(crate::api::EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_composition_update_visual_intent(
        &self,
        session_id: u64,
        composition_replace_start: u32,
        composition_replace_end_exclusive: u32,
        old_preedit_text: String,
        new_preedit_text: String,
    ) -> crate::api::EditorVisualIntentDto {
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
        .unwrap_or_else(crate::api::EditorVisualIntentDto::default_fallback)
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
    pub fn text_edit_session_snapshot(&self, session_id: u64) -> crate::api::EditorSessionSnapshotDto {
        self.with_session_in_registry(session_id, |s| {
            crate::api::EditorSessionSnapshotDto {
                text: s.kernel.text().to_string(),
                revision: s.kernel.revision(),
                cursor: s.kernel.cursor() as u32,
                selection_anchor: s.kernel.selection_anchor() as u32,
                generation: s.generation,
                chapter_id: s.target_id.clone(),
            }
        })
        .unwrap_or_else(|| crate::api::EditorSessionSnapshotDto {
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
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::ReplaceAll {
                search,
                replacement,
                expected_revision,
            });
            result.into_result().into()
        })
        .unwrap_or_else(crate::api::EditorEditResultDto::stale_fallback)
    }

    pub fn text_edit_session_insert_line_break(
        &self,
        session_id: u64,
        byte_offset: u32,
        auto_indent_prefix: String,
        cause: crate::api::EditorTransactionCauseDto,
        expected_revision: u64,
    ) -> crate::api::EditorEditResultDto {
        use crate::editor::EditorCommand;
        let core_cause: crate::editor::EditorTransactionCause = cause.into();
        self.with_session_in_registry(session_id, |s| {
            let result = s.kernel.apply(EditorCommand::InsertLineBreak {
                byte_offset: byte_offset as usize,
                auto_indent_prefix,
                cause: core_cause,
                expected_revision,
            });
            result.into_result().into()
        })
        .unwrap_or_else(crate::api::EditorEditResultDto::stale_fallback)
    }
}
