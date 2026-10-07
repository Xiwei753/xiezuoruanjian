//! Linux Qt 正文过渡与光标动画的协调器。
//!
//! 正文每次编辑都由一个 `VisualEditState` 直接从最近成功绘制的 `VisualFrame`
//! 过渡到最新 canonical snapshot。光标仍由独立的 CursorController 管理。

use std::time::Instant;

use writer_core::editor::OffsetMap;

use crate::sujian_editor_item::animation::visual_edit_state::VisualEditState;
use crate::sujian_editor_item::animation::visual_frame::VisualFrame;
use crate::sujian_editor_item::cursor_animation::{
    CursorAnimationPlan, CursorBlinkMode, CursorTransition,
};
use crate::sujian_editor_item::edit_motion::CursorRect;
use crate::sujian_editor_item::edit_motion::DeletedRangeEdge;
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::layout_snapshot::{EditorLayoutSnapshot, LineSnapshotId};
use crate::sujian_editor_item::render_ownership::RenderOwnershipPlan;

#[derive(Clone)]
struct FrameRevisionMap {
    from_revision: LayoutRevision,
    to_revision: LayoutRevision,
    offset_map: OffsetMap,
}

pub(crate) struct VisualEditRequest {
    pub base_snapshot: EditorLayoutSnapshot,
    pub target_snapshot: EditorLayoutSnapshot,
    pub offset_map: OffsetMap,
    pub deleted_range_edges: Vec<DeletedRangeEdge>,
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
    pub duration_ms: u64,
    pub visual_x: f64,
    pub visual_y: f64,
    pub force_snap_next: bool,
    pub baseline_y: f64,
}

pub(crate) struct LinuxEditorAnimationCoordinator {
    visual_edit_state: Option<VisualEditState>,
    last_committed_visual_frame: Option<VisualFrame>,
    frame_to_current_map: Option<FrameRevisionMap>,
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
            frame_to_current_map: None,
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
        let frame = self.last_committed_visual_frame.as_ref();
        let frame_to_base_map = match frame {
            Some(frame) => match (frame.canonical_revision, self.frame_to_current_map.as_ref()) {
                (Some(anchor), Some(mapping))
                    if mapping.from_revision == anchor
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
                        from_revision,
                        to_revision: request.target_snapshot.revision,
                        offset_map: frame_to_target_map.clone(),
                    });
        }

        self.last_edit_at = Some(request.now);
        if !request.animate {
            self.visual_edit_state = None;
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
        );
        self.visual_edit_state = Some(state);
    }

    pub(crate) fn ownership_plan(
        &self,
        frame_now: Instant,
        canonical_snapshot: Option<&EditorLayoutSnapshot>,
    ) -> RenderOwnershipPlan {
        if let Some(state) = self.visual_edit_state.as_ref() {
            state.build_ownership_plan(self.effective_text_animation_time(frame_now))
        } else {
            canonical_snapshot
                .map(RenderOwnershipPlan::canonical)
                .unwrap_or_default()
        }
    }

    /// 只在 static + animation 整帧提交成功后调用。
    pub(crate) fn commit_rendered_plan(
        &mut self,
        plan: &RenderOwnershipPlan,
        resources_ready: bool,
    ) {
        let committed_frame = if resources_ready {
            plan.candidate_frame.clone()
        } else {
            plan.canonical_frame.clone()
        };
        self.frame_to_current_map =
            committed_frame
                .canonical_revision
                .map(|revision| FrameRevisionMap {
                    from_revision: revision,
                    to_revision: revision,
                    offset_map: identity_map(committed_frame.canonical_byte_len),
                });
        self.last_committed_visual_frame = Some(committed_frame);
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

    pub(crate) fn active_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        self.visual_edit_state
            .as_ref()
            .map(VisualEditState::active_snapshot_ids)
            .unwrap_or_default()
    }

    /// Keep the old call name at existing cache-retention sites; the set now comes from
    /// the committed visual frame and the single active visual edit.
    pub(crate) fn collect_active_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        self.active_snapshot_ids()
    }

    pub(crate) fn has_active_visual_edit(&self) -> bool {
        self.visual_edit_state.is_some()
    }

    pub(crate) fn has_active_text_animation(&self, _frame_now: Instant) -> bool {
        !self.is_paused() && self.visual_edit_state.is_some()
    }

    pub(crate) fn clear_visual_edit(&mut self) {
        self.visual_edit_state = None;
    }

    pub(crate) fn suppress_all(&mut self) -> bool {
        let had = self.visual_edit_state.is_some();
        self.clear_visual_edit();
        had
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
        let hard_snap = inputs.force_snap_next || inputs.selection_gesture_active;
        let allow_cross_line_tween = inputs.smooth_cursor_enabled && !inputs.is_scrolling;
        let needs_tween = (old_rect.x - new_rect.x).abs() > f64::EPSILON
            || (old_rect.top - new_rect.top).abs() > f64::EPSILON;
        let can_tween = !hard_snap
            && needs_tween
            && (allow_cross_line_tween || (old_rect.top - new_rect.top).abs() <= f64::EPSILON);

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
