//! Linux Qt 正文过渡与光标动画的协调器。
//!
//! 正文每次编辑都由一个 `VisualEditState` 直接从最近成功绘制的 `VisualFrame`
//! 过渡到最新 canonical snapshot。光标仍由独立的 CursorController 管理。

use std::collections::HashMap;
use std::time::Instant;

use writer_core::editor::OffsetMap;

use crate::sujian_editor_item::animation::visual_edit_state::VisualEditState;
use crate::sujian_editor_item::animation::visual_frame::VisualFrame;
use crate::sujian_editor_item::cursor_animation::{
    CursorAnimationPlan, CursorBlinkMode, CursorTransition,
};
use crate::sujian_editor_item::cursor_controller::CursorMoveSource;
use crate::sujian_editor_item::edit_motion::CursorRect;
use crate::sujian_editor_item::edit_motion::DeletedRangeEdge;
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::layout_snapshot::{EditorLayoutSnapshot, LineSnapshotId};
use crate::sujian_editor_item::render_ownership::{
    ClusterOwnerKey, ClusterVisualOwner, RenderOwnershipPlan,
};
use crate::sujian_editor_item::render_plan::VisualCaretGeometry;

struct CommittedOwnership {
    document_session: u64,
    ownership_revision: u64,
    cluster_owners: HashMap<ClusterOwnerKey, ClusterVisualOwner>,
    animation_snapshot_ids: Vec<LineSnapshotId>,
}

#[derive(Clone)]
struct FrameRevisionMap {
    document_session: u64,
    from_revision: LayoutRevision,
    to_revision: LayoutRevision,
    offset_map: OffsetMap,
}

pub(crate) struct VisualEditRequest {
    pub base_snapshot: EditorLayoutSnapshot,
    pub target_snapshot: EditorLayoutSnapshot,
    pub offset_map: OffsetMap,
    pub deleted_range_edges: Vec<DeletedRangeEdge>,
    pub caret_motion: Option<(CursorRect, CursorRect)>,
    pub animate: bool,
    pub now: Instant,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CursorMoveInputs {
    pub cursor_x: f64,
    pub cursor_y: f64,
    pub cursor_h: f64,
    pub editor_enabled: bool,
    pub has_selection: bool,
    pub is_scrolling: bool,
    pub selection_gesture_active: bool,
    pub is_preediting: bool,
    pub smooth_cursor_enabled: bool,
    pub cursor_animation_enabled: bool,
    pub movement_source: CursorMoveSource,
    pub driver_revision: Option<LayoutRevision>,
    pub visual_position_valid: bool,
    pub duration_ms: u64,
    pub visual_x: f64,
    pub visual_y: f64,
    pub force_snap_next: bool,
    pub baseline_y: f64,
}

pub(crate) struct LinuxEditorAnimationCoordinator {
    visual_edit_state: Option<VisualEditState>,
    last_committed_visual_frame: Option<VisualFrame>,
    /// 最近真正提交到 Scene Graph 的 owner table。取消或切章进入 handoff 后，
    /// 继续保留其动画行纹理 ID，直到静态层成功替换它。
    last_committed_ownership: Option<CommittedOwnership>,
    frame_to_current_map: Option<FrameRevisionMap>,
    pub(crate) document_session: u64,
    handoff_pending: bool,
    last_edit_at: Option<Instant>,
    typing_animation_duration_ms: u32,
    cursor_animation_duration_ms: u32,
    paused_at: Option<Instant>,
}

impl LinuxEditorAnimationCoordinator {
    pub fn new() -> Self {
        Self {
            visual_edit_state: None,
            last_committed_visual_frame: None,
            last_committed_ownership: None,
            frame_to_current_map: None,
            document_session: 0,
            handoff_pending: false,
            last_edit_at: None,
            typing_animation_duration_ms: 160,
            cursor_animation_duration_ms: 120,
            paused_at: None,
        }
    }

    pub(crate) fn set_typing_animation_duration_ms(&mut self, ms: u32) {
        self.typing_animation_duration_ms = ms;
    }

    pub(crate) fn set_cursor_animation_duration_ms(&mut self, ms: u32) {
        self.cursor_animation_duration_ms = ms;
    }

    /// 新编辑直接替换当前过渡，从上一帧成功提交的视觉状态重新计算。
    pub(crate) fn begin_visual_edit(&mut self, request: VisualEditRequest) {
        let previous_edit_at = self.last_edit_at;
        let frame = self
            .last_committed_visual_frame
            .as_ref()
            .filter(|frame| frame.document_session == self.document_session);
        let frame_to_base_map = match frame {
            Some(frame) => match (frame.canonical_revision, self.frame_to_current_map.as_ref()) {
                (Some(anchor), Some(mapping))
                    if mapping.document_session == self.document_session
                        && mapping.from_revision == anchor
                        && mapping.to_revision == request.base_snapshot.revision =>
                {
                    mapping.offset_map.clone()
                }
                (Some(anchor), _) if anchor == request.base_snapshot.revision => {
                    identity_map(frame.canonical_byte_len)
                }
                _ => OffsetMap {
                    entries: Vec::new(),
                },
            },
            None => identity_map(snapshot_byte_len(&request.base_snapshot)),
        };
        let frame_to_target_map = if frame.is_some() {
            frame_to_base_map.compose(&request.offset_map)
        } else {
            request.offset_map.clone()
        };

        if let Some(frame) = frame {
            self.frame_to_current_map =
                frame
                    .canonical_revision
                    .map(|from_revision| FrameRevisionMap {
                        document_session: self.document_session,
                        from_revision,
                        to_revision: request.target_snapshot.revision,
                        offset_map: frame_to_target_map.clone(),
                    });
        }

        self.last_edit_at = Some(request.now);
        if !request.animate {
            self.visual_edit_state = None;
            self.handoff_pending = true;
            return;
        }

        let state = VisualEditState::new(
            frame,
            &request.base_snapshot,
            request.target_snapshot,
            &frame_to_base_map,
            &frame_to_target_map,
            &request.deleted_range_edges,
            request.now,
            u64::from(self.typing_animation_duration_ms),
            previous_edit_at,
            request.caret_motion,
            self.document_session,
        );
        self.visual_edit_state = Some(state);
        // 新 transition 覆盖了待收口的旧画面；它会和新的静态层一起原子提交。
        self.handoff_pending = false;
    }

    pub(crate) fn ownership_plan(
        &self,
        frame_now: Instant,
        canonical_snapshot: Option<&EditorLayoutSnapshot>,
        visual_caret: Option<VisualCaretGeometry>,
    ) -> RenderOwnershipPlan {
        let mut plan = if let Some(state) = self.visual_edit_state.as_ref() {
            state.build_ownership_plan(self.effective_text_animation_time(frame_now), visual_caret)
        } else {
            canonical_snapshot
                .map(RenderOwnershipPlan::canonical)
                .unwrap_or_default()
        };
        plan.document_session = self.document_session;
        if self.handoff_pending {
            plan.handoff_pending = true;
            plan.terminal_frame = false;
            plan.animated_glyphs.clear();
            plan.candidate_frame = plan.canonical_frame.clone();
        }
        plan
    }

    /// 只在 static + animation 整帧提交成功后调用。
    pub(crate) fn commit_rendered_plan(
        &mut self,
        plan: &RenderOwnershipPlan,
        resources_ready: bool,
    ) {
        if plan.document_session != self.document_session {
            return;
        }
        let committed_frame = if resources_ready {
            plan.candidate_frame.clone()
        } else {
            plan.canonical_frame.clone()
        };
        let mut committed_frame = committed_frame;
        committed_frame.document_session = self.document_session;
        self.frame_to_current_map =
            committed_frame
                .canonical_revision
                .map(|revision| FrameRevisionMap {
                    document_session: self.document_session,
                    from_revision: revision,
                    to_revision: revision,
                    offset_map: identity_map(committed_frame.canonical_byte_len),
                });
        self.last_committed_visual_frame = Some(committed_frame);
        let mut committed_animation_ids = Vec::new();
        let keeps_animation = resources_ready && !plan.handoff_pending;
        if keeps_animation {
            if let Some(state) = self.visual_edit_state.as_mut() {
                state.commit_terminal_motions(&plan.terminal_motion_indices);
            }
        }
        if keeps_animation {
            for glyph in &plan.animated_glyphs {
                if !committed_animation_ids.contains(&glyph.snapshot_id) {
                    committed_animation_ids.push(glyph.snapshot_id);
                }
            }
            for exclusion in &plan.static_exclusions {
                if !committed_animation_ids.contains(&exclusion.snapshot_id) {
                    committed_animation_ids.push(exclusion.snapshot_id);
                }
            }
        }
        let committed_revision = if keeps_animation {
            plan.ownership_revision
        } else {
            0
        };
        let ownership_is_unchanged = self
            .last_committed_ownership
            .as_ref()
            .map(|committed| {
                committed.document_session == self.document_session
                    && committed.ownership_revision == committed_revision
                    && committed.animation_snapshot_ids == committed_animation_ids
                    && if keeps_animation {
                        committed.cluster_owners == plan.cluster_owners
                    } else {
                        committed.cluster_owners.len() == plan.cluster_owners.len()
                            && plan.cluster_owners.keys().all(|key| {
                                committed.cluster_owners.get(key)
                                    == Some(&ClusterVisualOwner::Static)
                            })
                    }
            })
            .unwrap_or(false);
        if !ownership_is_unchanged {
            let mut committed_owners = plan.cluster_owners.clone();
            if !keeps_animation {
                for owner in committed_owners.values_mut() {
                    *owner = ClusterVisualOwner::Static;
                }
            }
            self.last_committed_ownership = Some(CommittedOwnership {
                document_session: self.document_session,
                ownership_revision: committed_revision,
                cluster_owners: committed_owners,
                animation_snapshot_ids: committed_animation_ids,
            });
        }
        self.handoff_pending = false;
        if plan.handoff_pending || !resources_ready {
            self.visual_edit_state = None;
        } else if plan.terminal_frame {
            if let Some(state) = self.visual_edit_state.as_mut() {
                state.terminal_frame_committed = true;
            }
        }
    }

    pub(crate) fn active_target_snapshot(&self) -> Option<&EditorLayoutSnapshot> {
        self.visual_edit_state
            .as_ref()
            .map(|state| &state.target_snapshot)
    }

    /// Move a caret-driven text transition onto its own saved visual frame before an
    /// unrelated caret movement takes over CursorController's single animation route.
    pub(crate) fn detach_caret_driven_transition(&mut self, now: Instant) {
        let Some(state) = self.visual_edit_state.as_ref() else {
            return;
        };
        if !state.has_caret_driven_motions() || state.terminal_frame_committed {
            return;
        }
        if self.is_paused() {
            if let Some(state) = self.visual_edit_state.as_mut() {
                state.clear_caret_driver();
            }
            return;
        }

        let target_snapshot = state.target_snapshot.clone();
        let remaining_ms = state.remaining_duration_ms(now);
        let committed_frame = self
            .last_committed_visual_frame
            .as_ref()
            .filter(|frame| {
                frame.document_session == self.document_session
                    && frame.canonical_revision == Some(target_snapshot.revision)
            })
            .cloned();

        if let Some(frame) = committed_frame {
            let identity = identity_map(frame.canonical_byte_len);
            self.visual_edit_state = Some(VisualEditState::new(
                Some(&frame),
                &target_snapshot,
                target_snapshot.clone(),
                &identity,
                &identity,
                &[],
                now,
                remaining_ms,
                None,
                None,
                self.document_session,
            ));
        } else if let Some(state) = self.visual_edit_state.as_mut() {
            // No frame from this revision has been presented yet, so there is no
            // caret-driven slice to preserve. Keep the existing text clock and detach
            // its boundary driver before the unrelated caret route starts.
            state.clear_caret_driver();
        }
    }

    pub(crate) fn active_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        self.visual_edit_state
            .as_ref()
            .map(VisualEditState::active_snapshot_ids)
            .unwrap_or_default()
    }

    /// Keep the old call name at existing cache-retention sites. Preserve resources
    /// referenced by both the latest transition and the still-visible committed plan.
    pub(crate) fn collect_active_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids = self.active_snapshot_ids();
        if let Some(committed) = self.last_committed_ownership.as_ref() {
            for snapshot_id in &committed.animation_snapshot_ids {
                if !ids.contains(snapshot_id) {
                    ids.push(*snapshot_id);
                }
            }
        }
        ids
    }

    pub(crate) fn has_active_visual_edit(&self) -> bool {
        self.visual_edit_state.is_some()
    }

    pub(crate) fn has_active_text_animation(&self, _frame_now: Instant) -> bool {
        !self.is_paused() && self.visual_edit_state.is_some()
    }

    pub(crate) fn clear_visual_edit(&mut self) {
        let had = self.visual_edit_state.is_some();
        self.visual_edit_state = None;
        self.handoff_pending |= had;
    }

    pub(crate) fn suppress_all(&mut self) -> bool {
        let had = self.visual_edit_state.is_some();
        self.clear_visual_edit();
        self.paused_at = None;
        self.last_edit_at = None;
        had
    }

    /// 切换文档时结束旧视觉会话。旧 Scene Graph owner/纹理仍保留作已提交画面，
    /// 但不再允许它成为新文档 transition 的 source。
    pub(crate) fn reset_document_visual_session(&mut self) {
        self.document_session = self.document_session.wrapping_add(1);
        self.visual_edit_state = None;
        self.last_committed_visual_frame = None;
        self.frame_to_current_map = None;
        self.last_edit_at = None;
        self.paused_at = None;
        self.handoff_pending = true;
    }

    pub(crate) fn pause_all(&mut self, now: Instant) -> Vec<LineSnapshotId> {
        self.paused_at = Some(now);
        self.collect_active_snapshot_ids()
    }

    pub(crate) fn resume_all(&mut self, now: Instant) {
        if let Some(paused_at) = self.paused_at.take() {
            let delta = now.saturating_duration_since(paused_at);
            if let Some(state) = self.visual_edit_state.as_mut() {
                state.shift_started_at(delta);
            }
            if let Some(last_edit_at) = self.last_edit_at.as_mut() {
                *last_edit_at = last_edit_at.checked_add(delta).unwrap_or(*last_edit_at);
            }
        }
    }

    pub(crate) fn finish_paused_text_animation_to_canonical(&mut self) -> bool {
        let had = self.visual_edit_state.is_some();
        self.visual_edit_state = None;
        self.paused_at = None;
        self.last_edit_at = None;
        self.handoff_pending |= had;
        had
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.paused_at.is_some()
    }

    pub(crate) fn effective_text_animation_time(&self, frame_now: Instant) -> Instant {
        self.paused_at.unwrap_or(frame_now)
    }

    pub(crate) fn build_cursor_plan(&self, inputs: &CursorMoveInputs) -> CursorAnimationPlan {
        let should_be_visible =
            inputs.editor_enabled && !inputs.has_selection && !inputs.is_preediting;
        let old_rect = CursorRect {
            x: inputs.visual_x,
            top: inputs.visual_y,
            bottom: inputs.visual_y + inputs.cursor_h,
            baseline_y: inputs.baseline_y,
        };
        let new_rect = CursorRect {
            x: inputs.cursor_x,
            top: inputs.cursor_y,
            bottom: inputs.cursor_y + inputs.cursor_h,
            baseline_y: inputs.baseline_y,
        };
        let source_allows_tween = match inputs.movement_source {
            CursorMoveSource::TextTransaction => inputs.cursor_animation_enabled,
            CursorMoveSource::PointerClick | CursorMoveSource::KeyboardNavigation => {
                inputs.smooth_cursor_enabled
            }
            CursorMoveSource::DragSelection
            | CursorMoveSource::LayoutChange
            | CursorMoveSource::Scroll => false,
        };
        let hard_snap = inputs.force_snap_next
            || inputs.selection_gesture_active
            || inputs.is_scrolling
            || !inputs.visual_position_valid;
        let needs_tween = (old_rect.x - new_rect.x).abs() > f64::EPSILON
            || (old_rect.top - new_rect.top).abs() > f64::EPSILON;
        let can_tween = !hard_snap && source_allows_tween && needs_tween;

        let transition = if can_tween && inputs.duration_ms > 0 {
            CursorTransition::Tween {
                old_rect,
                new_rect,
                duration_ms: inputs.duration_ms,
            }
        } else {
            CursorTransition::Snap
        };

        CursorAnimationPlan {
            should_be_visible,
            transition,
            movement_source: inputs.movement_source,
            driver_revision: inputs.driver_revision,
            cursor_x: new_rect.x,
            cursor_y: new_rect.top,
            cursor_h: inputs.cursor_h,
            cursor_baseline_y: inputs.baseline_y,
            hidden_by_selection: inputs.has_selection,
        }
    }
}

fn identity_map(byte_len: usize) -> OffsetMap {
    OffsetMap::from_single_edit(byte_len, (0, 0), 0)
}

fn snapshot_byte_len(snapshot: &EditorLayoutSnapshot) -> usize {
    snapshot
        .line_snapshots
        .iter()
        .map(|line| line.byte_end)
        .max()
        .unwrap_or(0)
}

impl Default for LinuxEditorAnimationCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn blink_mode_for_text_animation(animation_active: bool) -> CursorBlinkMode {
    if animation_active {
        CursorBlinkMode::Suppressed
    } else {
        CursorBlinkMode::Normal
    }
}
