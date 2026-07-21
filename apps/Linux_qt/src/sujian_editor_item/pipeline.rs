use writer_core::editor::{
    EditorCommand, EditorEditOutcome, EditorEditResult, EditorKernel, EditorTransactionCause, CursorRect, PreeditVisualTransaction,
    EditorVisualTransaction, EditorSelection, EditorCursor, EditorAnimationKind,
};
use super::buffer::{clamp_to_char_boundary, normalize_plain_text, EditorSnapshot};
use super::animation_coordinator::LinuxEditorAnimationCoordinator;
use super::texture_cache::TextureCache;
use super::layout_snapshot::EditorLayoutSnapshot;
use super::layout_revision::LayoutRevision;
use super::line_snapshot_builder::LineSnapshotBuilder;
use super::transaction_key::VisualTransactionKey;
use super::PreeditAttribute;
use writer_core::editor::CompositionSession;
use crate::editor::layout;
use crate::platform::linux_qt::LinuxQtClipboardFocusAdapter;

/// Qt 侧已确认正文镜像 — 持有与 Rust EditorKernel revision 对应的纯文本快照。
///
/// 不维护第二份可独立编辑的正文真相。所有修改必须通过 EditorKernel.apply() →
/// EditorEditResult → apply_edit_result() 增量同步。镜像的 revision 必须与
/// kernel 的 base_revision 匹配，否则 patch 被拒绝。
///
/// cursor 和 selection_anchor 均为 UTF-8 byte offset。
/// selection_anchor 是选区锚点（非移动端），cursor 是光标（移动端/插入点）。
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
            if start > self.text.len() || end > self.text.len() {
                return Err(format!(
                    "CommittedTextMirror patch range out of bounds: [{}, {}) vs text len {}. Must reload from kernel snapshot.",
                    start, end, self.text.len()
                ));
            }
            if !self.text.is_char_boundary(start) || !self.text.is_char_boundary(end) {
                return Err(format!(
                    "CommittedTextMirror patch range not on char boundary: [{}, {}). Must reload from kernel snapshot.",
                    start, end
                ));
            }
            self.text.replace_range(start..end, &patch.inserted_text);
            self.revision = patch.new_revision;
        }
        let (anchor, head) = result.new_selection_byte_range;
        if anchor > self.text.len() || head > self.text.len() {
            return Err(format!(
                "CommittedTextMirror selection out of bounds: ({}, {}) vs text len {}. Must reload from kernel snapshot.",
                anchor, head, self.text.len()
            ));
        }
        if !self.text.is_char_boundary(anchor) || !self.text.is_char_boundary(head) {
            return Err(format!(
                "CommittedTextMirror selection not on char boundary: ({}, {}). Must reload from kernel snapshot.",
                anchor, head
            ));
        }
        self.cursor = head;
        self.selection_anchor = anchor;
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

pub(crate) struct CompositionCommitParams {
    pub inserted_text: String,
    pub session_replace_start: usize,
    pub session_replace_end: usize,
    pub cause: EditorTransactionCause,
}

pub(crate) struct CompositionCommitResult {
    pub pending_preedit_cursor_rect: Option<CursorRect>,
    pub was_composing: bool,
    pub preedit_byte_start: usize,
    pub preedit_byte_end: usize,
    pub saved_virtual_text: String,
    pub session_replace_start: usize,
    pub session_replace_end: usize,
    pub candidate_byte_start: usize,
    pub candidate_byte_end: usize,
    pub committed_replace_start: usize,
    pub committed_replace_end: usize,
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

    pub fn save_pending_preedit_cursor_rect(&mut self) {
        if !self.preedit_text.is_empty() && self.pending_preedit_cursor_rect.is_none() {
            self.pending_preedit_cursor_rect = self.preedit_cursor_rect.clone();
        }
    }

    pub fn take_pending_preedit_cursor_rect(&mut self) -> Option<CursorRect> {
        self.pending_preedit_cursor_rect.take()
    }

    pub fn clear_preedit_fields(&mut self) {
        self.preedit_text.clear();
        self.preedit_cursor = 0;
        self.preedit_attributes.clear();
        self.preedit_old_text.clear();
        self.preedit_visual_transaction = None;
        self.preedit_cursor_rect = None;
    }

    pub fn session_replace_range(&self, fallback_cursor: usize) -> (usize, usize) {
        self.composition_session
            .as_ref()
            .map(|s| (s.replace_start, s.replace_end_exclusive))
            .unwrap_or((fallback_cursor, fallback_cursor))
    }

    pub fn virtual_text(&self) -> String {
        self.composition_session
            .as_ref()
            .map(|s| s.virtual_text())
            .unwrap_or_default()
    }

    pub fn finish_session(&mut self) {
        self.composition_session = None;
    }
}

pub(crate) struct VisualTransactionContext {
    pub typing_animation_enabled: bool,
    pub is_scrolling: bool,
    pub is_loading: bool,
    pub is_applying_format: bool,
    pub is_applying_settings: bool,
    pub bounding_width: f64,
    pub font_pixel_size: f64,
    pub font_family: String,
    pub scroll_y: f64,
    pub viewport_height: f64,
    pub text_indent: f64,
    pub line_spacing: f64,
    pub padding: f64,
    pub text_color: String,
    pub dpr: f64,
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
    current_layout_snapshot: Option<EditorLayoutSnapshot>,
    previous_layout_snapshot: Option<EditorLayoutSnapshot>,
    previous_canonical_snapshot: Option<crate::editor::layout::CanonicalDocumentVisualSnapshot>,
    layout_revision: LayoutRevision,
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
            current_layout_snapshot: None,
            previous_layout_snapshot: None,
            previous_canonical_snapshot: None,
            layout_revision: LayoutRevision::initial(),
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
        self.engine.set_animation_duration_ms(u64::from(ms));
        self.kernel.set_animation_duration_ms(u64::from(ms));
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
            EditorEditOutcome::Applied(result)
            | EditorEditOutcome::AppliedWithAdjustedSelection(result) => {
                if self.mirror.apply_edit_result(&result).is_err() {
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
            EditorEditOutcome::Applied(result)
            | EditorEditOutcome::AppliedWithAdjustedSelection(result) => {
                if self.mirror.apply_edit_result(&result).is_err() {
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
            EditorEditOutcome::Applied(result) | EditorEditOutcome::AppliedWithAdjustedSelection(result) => {
                if self.mirror.apply_edit_result(&result).is_err() {
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
            EditorEditOutcome::Applied(result) | EditorEditOutcome::AppliedWithAdjustedSelection(result) => {
                if self.mirror.apply_edit_result(&result).is_err() {
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
            EditorEditOutcome::Applied(result) | EditorEditOutcome::AppliedWithAdjustedSelection(result) => {
                if self.mirror.apply_edit_result(&result).is_err() {
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
            EditorEditOutcome::Applied(result) | EditorEditOutcome::AppliedWithAdjustedSelection(result) => {
                if self.mirror.apply_edit_result(&result).is_err() {
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

    pub fn prepare_composition_commit(
        &mut self,
        inserted_text: &str,
        fallback_cursor: usize,
        preedit_byte_start: usize,
        preedit_byte_end: usize,
    ) -> CompositionCommitResult {
        self.composition.save_pending_preedit_cursor_rect();
        let pending_pcr = self.composition.take_pending_preedit_cursor_rect();
        let was_composing = self.composition.is_composing();
        let saved_virtual_text = self.composition.virtual_text();
        let (session_replace_start, session_replace_end) = self.composition.session_replace_range(fallback_cursor);
        let candidate_byte_start = session_replace_start;
        let candidate_byte_end = session_replace_start + inserted_text.len();
        let committed_replace_start = session_replace_start;
        let committed_replace_end = session_replace_end;

        self.composition.clear_preedit_fields();

        CompositionCommitResult {
            pending_preedit_cursor_rect: pending_pcr,
            was_composing,
            preedit_byte_start,
            preedit_byte_end,
            saved_virtual_text,
            session_replace_start,
            session_replace_end,
            candidate_byte_start,
            candidate_byte_end,
            committed_replace_start,
            committed_replace_end,
        }
    }

    pub fn finish_composition_commit(&mut self) {
        self.composition.finish_session();
    }

    pub fn current_layout_snapshot(&self) -> &Option<EditorLayoutSnapshot> {
        &self.current_layout_snapshot
    }

    pub fn set_current_layout_snapshot(&mut self, snapshot: Option<EditorLayoutSnapshot>) {
        self.current_layout_snapshot = snapshot;
    }

    pub fn previous_layout_snapshot(&self) -> &Option<EditorLayoutSnapshot> {
        &self.previous_layout_snapshot
    }

    pub fn set_previous_layout_snapshot(&mut self, snapshot: Option<EditorLayoutSnapshot>) {
        self.previous_layout_snapshot = snapshot;
    }

    pub fn previous_canonical_snapshot(&self) -> &Option<crate::editor::layout::CanonicalDocumentVisualSnapshot> {
        &self.previous_canonical_snapshot
    }

    pub fn set_previous_canonical_snapshot(&mut self, snapshot: Option<crate::editor::layout::CanonicalDocumentVisualSnapshot>) {
        self.previous_canonical_snapshot = snapshot;
    }

    pub fn layout_revision_val(&self) -> LayoutRevision {
        self.layout_revision
    }

    pub fn set_layout_revision_val(&mut self, rev: LayoutRevision) {
        self.layout_revision = rev;
    }

    pub fn bump_layout_revision(&mut self) -> LayoutRevision {
        self.layout_revision = LayoutRevision::next();
        self.layout_revision
    }

    pub fn prepare_transaction_textures(&mut self, key: VisualTransactionKey) {
        let tx = self.animation_coordinator.prepared_queue.active_transactions()
            .iter()
            .find(|t| t.key == key)
            .cloned();

        if let Some(t) = tx {
            let snapshot_ids = t.snapshot_ids();
            if snapshot_ids.is_empty() {
                self.animation_coordinator.prepared_queue.mark_texture_prepared(key);
                return;
            }

            let mut all_found = true;
            for id in &snapshot_ids {
                if !self.texture_cache.contains_line(id) {
                    all_found = false;
                    break;
                }
            }

            if all_found {
                self.animation_coordinator.prepared_queue.mark_texture_prepared(key);
                return;
            }

            if let Some(ref old_snap) = t.old_snapshot {
                for line in &old_snap.line_snapshots {
                    if let Some(ref image) = line.image {
                        self.texture_cache.insert_line(line.id, image.clone());
                    }
                }
            }
            if let Some(ref new_snap) = t.new_snapshot {
                for line in &new_snap.line_snapshots {
                    if let Some(ref image) = line.image {
                        self.texture_cache.insert_line(line.id, image.clone());
                    }
                }
            }

            let mut any_missing = false;
            for id in &snapshot_ids {
                if !self.texture_cache.contains_line(id) {
                    any_missing = true;
                    break;
                }
            }

            if any_missing {
                super::editor_animation_debug_log(&format!(
                    "prepare_transaction_textures: some line textures missing for tid={}, cancelling",
                    key.transaction_id
                ));
                self.animation_coordinator.cancel_by_key(key, "texture_failed");
            } else {
                self.animation_coordinator.prepared_queue.mark_texture_prepared(key);
            }
        }
    }

    pub fn record_visual_transaction(
        &mut self,
        ctx: &VisualTransactionContext,
        old: &EditorSnapshot,
        new: &EditorSnapshot,
        cause: EditorTransactionCause,
    ) -> Option<EditorVisualTransaction> {
        let transaction = self.engine.create_transaction(
            &old.text,
            &new.text,
            EditorSelection {
                anchor: EditorCursor::new(&old.text, old.selection_anchor),
                head: EditorCursor::new(&old.text, old.cursor),
            },
            EditorSelection {
                anchor: EditorCursor::new(&new.text, new.selection_anchor),
                head: EditorCursor::new(&new.text, new.cursor),
            },
            cause,
        );
        let mut vt = self.engine.visual_transaction(&transaction);

        if ctx.typing_animation_enabled && vt.is_some() && !ctx.is_scrolling {
            if let Some(ref mut vt) = vt {
                let (affected_byte_start, affected_byte_end) = vt.inserted_range
                    .or(vt.deleted_range)
                    .unwrap_or_else(|| {
                        let changes = writer_core::editor::diff_plain_text(&vt.old_text, &vt.new_text);
                        let mut min_b = usize::MAX;
                        let mut max_b = 0usize;
                        for change in &changes {
                            match change {
                                writer_core::editor::EditorChange::Insert { index, text } => {
                                    min_b = min_b.min(*index);
                                    max_b = (*index + text.len()).max(max_b);
                                }
                                writer_core::editor::EditorChange::Delete { index, text } => {
                                    min_b = min_b.min(*index);
                                    max_b = (*index + text.len()).max(max_b);
                                }
                                _ => {}
                            }
                        }
                        (min_b.min(max_b), max_b)
                    });

                let prev_new_snapshot = self.previous_canonical_snapshot.as_ref();

                let old_doc_snapshot = layout::prepare_affected_paragraphs_visual_snapshot(
                    &vt.old_text,
                    0,
                    ctx.font_pixel_size,
                    &ctx.font_family,
                    ctx.line_spacing,
                    ctx.padding,
                    ctx.text_indent,
                    ctx.bounding_width,
                    ctx.dpr,
                    &ctx.text_color,
                    affected_byte_start,
                    affected_byte_end,
                    prev_new_snapshot,
                );
                let new_doc_snapshot = layout::prepare_affected_paragraphs_visual_snapshot(
                    &new.text,
                    0,
                    ctx.font_pixel_size,
                    &ctx.font_family,
                    ctx.line_spacing,
                    ctx.padding,
                    ctx.text_indent,
                    ctx.bounding_width,
                    ctx.dpr,
                    &ctx.text_color,
                    affected_byte_start,
                    affected_byte_end,
                    prev_new_snapshot,
                );

                let old_caret = old_doc_snapshot.cursor_rect(
                    vt.old_selection.head.index,
                    layout::CaretAffinity::Downstream,
                    ctx.scroll_y,
                    ctx.viewport_height,
                );
                let new_caret = new_doc_snapshot.cursor_rect(
                    vt.new_selection.head.index,
                    layout::CaretAffinity::Downstream,
                    ctx.scroll_y,
                    ctx.viewport_height,
                );

                vt.old_cursor_rect = Some(make_cursor_rect_from_caret_doc(&old_caret, &old_doc_snapshot, &ctx.font_family, ctx.scroll_y));
                vt.new_cursor_rect = Some(make_cursor_rect_from_caret_doc(&new_caret, &new_doc_snapshot, &ctx.font_family, ctx.scroll_y));

                match vt.kind {
                    EditorAnimationKind::Insert => {
                        vt.insert_glyph_rects = Some(Vec::new());
                        vt.reflow_glyph_rects = None;
                    }
                    EditorAnimationKind::Delete => {
                        vt.deleted_glyph_rects = None;
                    }
                    EditorAnimationKind::Cursor => {}
                }

                let old_revision = self.layout_revision;
                let new_revision = LayoutRevision::next();

                let (old_snap, new_snap) = LineSnapshotBuilder::build_old_new_from_canonical(
                    &old_doc_snapshot,
                    &new_doc_snapshot,
                    old_revision,
                    new_revision,
                    ctx.scroll_y,
                    ctx.viewport_height,
                );

                let key = self.animation_coordinator.process_transaction(
                    vt,
                    ctx.typing_animation_enabled,
                    ctx.is_scrolling,
                    ctx.is_loading,
                    ctx.is_applying_format,
                    ctx.is_applying_settings,
                    vt.old_cursor_rect.clone(),
                    vt.new_cursor_rect.clone(),
                    &old_snap,
                    &new_snap,
                );
                if let Some(key) = key {
                    self.prepare_transaction_textures(key);
                    self.layout_revision = new_revision;
                }

                self.previous_layout_snapshot = Some(self.current_layout_snapshot.clone().unwrap_or_else(|| {
                    EditorLayoutSnapshot::new(old_doc_snapshot.to_layout_snapshot(), Vec::new(), None, layout::CaretAffinity::Downstream)
                }));
                self.current_layout_snapshot = Some(new_snap);
                self.previous_canonical_snapshot = Some(new_doc_snapshot);

                super::editor_animation_debug_log(&format!(
                    "record_visual_transaction: processed via canonical document snapshot pipeline, kind={:?}, has_active_insert={}",
                    vt.kind,
                    self.animation_coordinator.has_active_insert()
                ));
            }
        }

        vt
    }
}

fn make_cursor_rect_from_caret_doc(
    caret: &layout::CursorLayoutRect,
    doc_snapshot: &layout::CanonicalDocumentVisualSnapshot,
    font_family: &str,
    scroll_y: f64,
) -> CursorRect {
    let line = doc_snapshot.visual_lines.iter().find(|l| l.id == caret.visual_line_id);
    let baseline_y = match line {
        Some(l) => layout::text_baseline_y(l, doc_snapshot.font_size, font_family) - scroll_y,
        None => caret.y + caret.h * 0.8,
    };
    CursorRect {
        x: caret.x,
        top: caret.y,
        bottom: caret.y + caret.h,
        baseline_y,
    }
}
