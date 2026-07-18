use writer_core::editor::{
    EditorCommand, EditorEditOutcome, EditorEditResult, EditorKernel, EditorTransactionCause,
    EditorVisualIntent, AnimationMode as CoreAnimationMode, CursorRect, PreeditVisualTransaction,
};
use super::buffer::{clamp_to_char_boundary, normalize_plain_text};
use super::animation_coordinator::LinuxEditorAnimationCoordinator;
use super::texture_cache::TextureCache;
use super::PreeditAttribute;
use writer_core::editor::CompositionSession;
use crate::platform::linux_qt::LinuxQtClipboardFocusAdapter;

pub(crate) struct CommittedTextMirror {
    text: String,
    revision: u64,
    cursor: usize,
    selection_anchor: usize,
}

impl CommittedTextMirror {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            revision: 0,
            cursor: 0,
            selection_anchor: 0,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn selection_anchor(&self) -> usize {
        self.selection_anchor
    }

    pub fn has_selection(&self) -> bool {
        self.cursor != self.selection_anchor
    }

    pub fn selection_range(&self) -> (usize, usize) {
        if self.cursor <= self.selection_anchor {
            (self.cursor, self.selection_anchor)
        } else {
            (self.selection_anchor, self.cursor)
        }
    }

    pub fn selected_text(&self) -> String {
        if !self.has_selection() {
            return String::new();
        }
        let (start, end) = self.selection_range();
        self.text[start..end].to_string()
    }

    pub fn load_from_snapshot(&mut self, text: String, cursor: usize, revision: u64, anchor: usize) {
        self.text = text;
        self.cursor = clamp_to_char_boundary(&self.text, cursor);
        self.selection_anchor = clamp_to_char_boundary(&self.text, anchor);
        self.revision = revision;
    }

    pub fn apply_edit_result(&mut self, result: &EditorEditResult) -> Result<(), String> {
        for patch in &result.display_patches {
            if patch.base_revision != self.revision {
                return Err(format!(
                    "CommittedTextMirror revision discontinuity: expected {}, got {}. Must reload from kernel snapshot.",
                    self.revision, patch.base_revision
                ));
            }
            let (start, end) = patch.replace_byte_range;
            if start <= end && end <= self.text.len() && self.text.is_char_boundary(start) && self.text.is_char_boundary(end) {
                self.text.replace_range(start..end, &patch.inserted_text);
            }
            self.revision = patch.new_revision;
        }
        let (anchor, head) = result.new_selection_byte_range;
        self.cursor = clamp_to_char_boundary(&self.text, head);
        self.selection_anchor = clamp_to_char_boundary(&self.text, anchor);
        Ok(())
    }
}

pub(crate) struct CompositionState {
    pub preedit_text: String,
    pub preedit_cursor: usize,
    pub preedit_attributes: Vec<PreeditAttribute>,
    pub preedit_old_text: String,
    pub composition_session: Option<CompositionSession>,
    pub preedit_visual_transaction: Option<PreeditVisualTransaction>,
    pub preedit_cursor_rect: Option<CursorRect>,
    pub pending_preedit_cursor_rect: Option<CursorRect>,
    pub suppress_next_ime_commit: bool,
}

impl CompositionState {
    pub fn new() -> Self {
        Self {
            preedit_text: String::new(),
            preedit_cursor: 0,
            preedit_attributes: Vec::new(),
            preedit_old_text: String::new(),
            composition_session: None,
            preedit_visual_transaction: None,
            preedit_cursor_rect: None,
            pending_preedit_cursor_rect: None,
            suppress_next_ime_commit: false,
        }
    }

    pub fn is_composing(&self) -> bool {
        !self.preedit_text.is_empty() || self.composition_session.is_some()
    }

    pub fn clear(&mut self) {
        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.preedit_attributes.clear();
        self.preedit_old_text.clear();
        self.composition_session = None;
        self.preedit_visual_transaction = None;
        self.preedit_cursor_rect = None;
        self.pending_preedit_cursor_rect = None;
        self.suppress_next_ime_commit = false;
    }
}

pub(crate) struct LinuxEditorPipeline {
    kernel: EditorKernel,
    mirror: CommittedTextMirror,
    composition: CompositionState,
    engine: writer_core::editor::EditorEngine,
    animation_coordinator: LinuxEditorAnimationCoordinator,
    texture_cache: TextureCache,
    clipboard_adapter: LinuxQtClipboardFocusAdapter,
    text_revision: u64,
    visual_revision: u64,
    animation_enabled: bool,
    typing_animation_enabled: bool,
    typing_animation_duration_ms: u32,
    coordinated_cursor_animation_enabled: bool,
    smooth_cursor_enabled: bool,
    cursor_animation_duration_ms: u32,
}

impl LinuxEditorPipeline {
    pub fn new() -> Self {
        Self {
            kernel: EditorKernel::new(),
            mirror: CommittedTextMirror::new(),
            composition: CompositionState::new(),
            engine: writer_core::editor::EditorEngine::new(),
            animation_coordinator: LinuxEditorAnimationCoordinator::new(),
            texture_cache: TextureCache::new(),
            clipboard_adapter: LinuxQtClipboardFocusAdapter::new(),
            text_revision: 0,
            visual_revision: 0,
            animation_enabled: true,
            typing_animation_enabled: true,
            typing_animation_duration_ms: 160,
            coordinated_cursor_animation_enabled: false,
            smooth_cursor_enabled: true,
            cursor_animation_duration_ms: 120,
        }
    }

    pub fn kernel(&self) -> &EditorKernel {
        &self.kernel
    }

    pub fn kernel_mut(&mut self) -> &mut EditorKernel {
        &mut self.kernel
    }

    pub fn mirror(&self) -> &CommittedTextMirror {
        &self.mirror
    }

    pub fn mirror_mut(&mut self) -> &mut CommittedTextMirror {
        &mut self.mirror
    }

    pub fn composition(&self) -> &CompositionState {
        &self.composition
    }

    pub fn composition_mut(&mut self) -> &mut CompositionState {
        &mut self.composition
    }

    pub fn engine(&self) -> &writer_core::editor::EditorEngine {
        &self.engine
    }

    pub fn engine_mut(&mut self) -> &mut writer_core::editor::EditorEngine {
        &mut self.engine
    }

    pub fn animation_coordinator(&self) -> &LinuxEditorAnimationCoordinator {
        &self.animation_coordinator
    }

    pub fn animation_coordinator_mut(&mut self) -> &mut LinuxEditorAnimationCoordinator {
        &mut self.animation_coordinator
    }

    pub fn texture_cache(&self) -> &TextureCache {
        &self.texture_cache
    }

    pub fn texture_cache_mut(&mut self) -> &mut TextureCache {
        &mut self.texture_cache
    }

    pub fn clipboard_adapter(&self) -> &LinuxQtClipboardFocusAdapter {
        &self.clipboard_adapter
    }

    pub fn clipboard_adapter_mut(&mut self) -> &mut LinuxQtClipboardFocusAdapter {
        &mut self.clipboard_adapter
    }

    pub fn text_revision(&self) -> u64 {
        self.text_revision
    }

    pub fn set_text_revision(&mut self, rev: u64) {
        self.text_revision = rev;
    }

    pub fn visual_revision(&self) -> u64 {
        self.visual_revision
    }

    pub fn bump_visual_revision(&mut self) {
        self.visual_revision = self.visual_revision.wrapping_add(1);
    }

    pub fn bump_text_revision(&mut self) {
        self.text_revision = self.text_revision.wrapping_add(1);
    }

    pub fn animation_enabled(&self) -> bool {
        self.animation_enabled
    }

    pub fn set_animation_enabled(&mut self, enabled: bool) {
        self.animation_enabled = enabled;
        self.kernel.set_animation_enabled(enabled);
    }

    pub fn typing_animation_enabled(&self) -> bool {
        self.typing_animation_enabled
    }

    pub fn set_typing_animation_enabled(&mut self, enabled: bool) {
        self.typing_animation_enabled = enabled;
    }

    pub fn typing_animation_duration_ms(&self) -> u32 {
        self.typing_animation_duration_ms
    }

    pub fn set_typing_animation_duration_ms(&mut self, ms: u32) {
        self.typing_animation_duration_ms = ms;
        self.engine.set_animation_duration_ms(ms as u64);
        self.kernel.set_animation_duration_ms(ms as u64);
    }

    pub fn coordinated_cursor_animation_enabled(&self) -> bool {
        self.coordinated_cursor_animation_enabled
    }

    pub fn set_coordinated_cursor_animation_enabled(&mut self, enabled: bool) {
        self.coordinated_cursor_animation_enabled = enabled;
    }

    pub fn smooth_cursor_enabled(&self) -> bool {
        self.smooth_cursor_enabled
    }

    pub fn set_smooth_cursor_enabled(&mut self, enabled: bool) {
        self.smooth_cursor_enabled = enabled;
    }

    pub fn cursor_animation_duration_ms(&self) -> u32 {
        self.cursor_animation_duration_ms
    }

    pub fn set_cursor_animation_duration_ms(&mut self, ms: u32) {
        self.cursor_animation_duration_ms = ms;
    }

    pub fn load_text(&mut self, text: String, cursor: usize) -> bool {
        let normalized = normalize_plain_text(&text);
        let clamped_cursor = clamp_to_char_boundary(&normalized, cursor);
        match EditorKernel::with_text(normalized.clone(), clamped_cursor) {
            Ok(kernel) => {
                self.kernel = kernel;
                self.mirror.load_from_snapshot(
                    self.kernel.text().to_string(),
                    self.kernel.cursor(),
                    self.kernel.revision(),
                    self.kernel.selection_anchor(),
                );
                self.composition.clear();
                self.animation_coordinator.cancel_active_composition("load_text");
                true
            }
            Err(_) => false,
        }
    }

    pub fn insert_text(&mut self, byte_offset: usize, text: &str, cause: EditorTransactionCause) -> Option<EditorEditResult> {
        let command = EditorCommand::Insert {
            byte_offset,
            text: text.to_string(),
            cause,
            expected_revision: self.mirror.revision(),
        };
        let outcome = self.kernel.apply(command);
        match outcome {
            EditorEditOutcome::Applied(result) => {
                if let Err(_) = self.mirror.apply_edit_result(&result) {
                    self.mirror.load_from_snapshot(
                        self.kernel.text().to_string(),
                        self.kernel.cursor(),
                        self.kernel.revision(),
                        self.kernel.selection_anchor(),
                    );
                }
                Some(result)
            }
            EditorEditOutcome::NoChange(result) => Some(result),
            EditorEditOutcome::StaleRevision(result) => {
                self.mirror.load_from_snapshot(
                    self.kernel.text().to_string(),
                    self.kernel.cursor(),
                    self.kernel.revision(),
                    self.kernel.selection_anchor(),
                );
                Some(result)
            }
            EditorEditOutcome::InvalidOffset(result)
            | EditorEditOutcome::InvalidRange(result) => Some(result),
        }
    }

    pub fn delete_range(&mut self, byte_start: usize, byte_end_exclusive: usize, cause: EditorTransactionCause) -> Option<EditorEditResult> {
        let command = EditorCommand::Delete {
            byte_start,
            byte_end_exclusive,
            deleted_text: String::new(),
            cause,
            expected_revision: self.mirror.revision(),
        };
        let outcome = self.kernel.apply(command);
        match outcome {
            EditorEditOutcome::Applied(result) => {
                if let Err(_) = self.mirror.apply_edit_result(&result) {
                    self.mirror.load_from_snapshot(
                        self.kernel.text().to_string(),
                        self.kernel.cursor(),
                        self.kernel.revision(),
                        self.kernel.selection_anchor(),
                    );
                }
                Some(result)
            }
            EditorEditOutcome::NoChange(result) => Some(result),
            EditorEditOutcome::StaleRevision(result) => {
                self.mirror.load_from_snapshot(
                    self.kernel.text().to_string(),
                    self.kernel.cursor(),
                    self.kernel.revision(),
                    self.kernel.selection_anchor(),
                );
                Some(result)
            }
            EditorEditOutcome::InvalidOffset(result)
            | EditorEditOutcome::InvalidRange(result) => Some(result),
        }
    }

    pub fn replace_range(&mut self, byte_start: usize, byte_end_exclusive: usize, replacement: &str, cause: EditorTransactionCause) -> Option<EditorEditResult> {
        let command = EditorCommand::Replace {
            byte_start,
            byte_end_exclusive,
            replacement_text: replacement.to_string(),
            original_text: String::new(),
            cause,
            expected_revision: self.mirror.revision(),
        };
        let outcome = self.kernel.apply(command);
        match outcome {
            EditorEditOutcome::Applied(result) => {
                if let Err(_) = self.mirror.apply_edit_result(&result) {
                    self.mirror.load_from_snapshot(
                        self.kernel.text().to_string(),
                        self.kernel.cursor(),
                        self.kernel.revision(),
                        self.kernel.selection_anchor(),
                    );
                }
                Some(result)
            }
            EditorEditOutcome::NoChange(result) => Some(result),
            EditorEditOutcome::StaleRevision(result) => {
                self.mirror.load_from_snapshot(
                    self.kernel.text().to_string(),
                    self.kernel.cursor(),
                    self.kernel.revision(),
                    self.kernel.selection_anchor(),
                );
                Some(result)
            }
            EditorEditOutcome::InvalidOffset(result)
            | EditorEditOutcome::InvalidRange(result) => Some(result),
        }
    }

    pub fn set_selection(&mut self, anchor: usize, head: usize) -> Option<EditorEditResult> {
        let command = EditorCommand::SetSelection {
            anchor_byte_offset: anchor,
            head_byte_offset: head,
            expected_revision: self.mirror.revision(),
        };
        let outcome = self.kernel.apply(command);
        match outcome {
            EditorEditOutcome::Applied(result) => {
                if let Err(_) = self.mirror.apply_edit_result(&result) {
                    self.mirror.load_from_snapshot(
                        self.kernel.text().to_string(),
                        self.kernel.cursor(),
                        self.kernel.revision(),
                        self.kernel.selection_anchor(),
                    );
                }
                Some(result)
            }
            EditorEditOutcome::NoChange(result) => Some(result),
            EditorEditOutcome::StaleRevision(result) => {
                self.mirror.load_from_snapshot(
                    self.kernel.text().to_string(),
                    self.kernel.cursor(),
                    self.kernel.revision(),
                    self.kernel.selection_anchor(),
                );
                Some(result)
            }
            EditorEditOutcome::InvalidOffset(result)
            | EditorEditOutcome::InvalidRange(result) => Some(result),
        }
    }

    pub fn perform_undo(&mut self) -> Option<EditorEditResult> {
        let command = EditorCommand::Undo {
            expected_revision: self.mirror.revision(),
        };
        let outcome = self.kernel.apply(command);
        match outcome {
            EditorEditOutcome::Applied(result) => {
                if let Err(_) = self.mirror.apply_edit_result(&result) {
                    self.mirror.load_from_snapshot(
                        self.kernel.text().to_string(),
                        self.kernel.cursor(),
                        self.kernel.revision(),
                        self.kernel.selection_anchor(),
                    );
                }
                Some(result)
            }
            EditorEditOutcome::NoChange(_) => None,
            EditorEditOutcome::StaleRevision(result) => {
                self.mirror.load_from_snapshot(
                    self.kernel.text().to_string(),
                    self.kernel.cursor(),
                    self.kernel.revision(),
                    self.kernel.selection_anchor(),
                );
                Some(result)
            }
            EditorEditOutcome::InvalidOffset(_)
            | EditorEditOutcome::InvalidRange(_) => None,
        }
    }

    pub fn perform_redo(&mut self) -> Option<EditorEditResult> {
        let command = EditorCommand::Redo {
            expected_revision: self.mirror.revision(),
        };
        let outcome = self.kernel.apply(command);
        match outcome {
            EditorEditOutcome::Applied(result) => {
                if let Err(_) = self.mirror.apply_edit_result(&result) {
                    self.mirror.load_from_snapshot(
                        self.kernel.text().to_string(),
                        self.kernel.cursor(),
                        self.kernel.revision(),
                        self.kernel.selection_anchor(),
                    );
                }
                Some(result)
            }
            EditorEditOutcome::NoChange(_) => None,
            EditorEditOutcome::StaleRevision(result) => {
                self.mirror.load_from_snapshot(
                    self.kernel.text().to_string(),
                    self.kernel.cursor(),
                    self.kernel.revision(),
                    self.kernel.selection_anchor(),
                );
                Some(result)
            }
            EditorEditOutcome::InvalidOffset(_)
            | EditorEditOutcome::InvalidRange(_) => None,
        }
    }

    pub fn reload_from_kernel(&mut self) -> bool {
        self.mirror.load_from_snapshot(
            self.kernel.text().to_string(),
            self.kernel.cursor(),
            self.kernel.revision(),
            self.kernel.selection_anchor(),
        );
        self.composition.clear();
        self.animation_coordinator.cancel_active_composition("reload_from_kernel");
        true
    }

    pub fn clear_undo_redo(&mut self) {
        let text = self.kernel.text().to_string();
        let cursor = self.kernel.cursor();
        let anchor = self.kernel.selection_anchor();
        self.kernel = EditorKernel::with_text(text, cursor).unwrap_or_else(|_| EditorKernel::new());
        if anchor != cursor {
            let _ = self.kernel.apply(EditorCommand::SetSelection {
                anchor_byte_offset: anchor,
                head_byte_offset: cursor,
                expected_revision: self.kernel.revision(),
            });
        }
        self.mirror.load_from_snapshot(
            self.kernel.text().to_string(),
            self.kernel.cursor(),
            self.kernel.revision(),
            self.kernel.selection_anchor(),
        );
    }
}
