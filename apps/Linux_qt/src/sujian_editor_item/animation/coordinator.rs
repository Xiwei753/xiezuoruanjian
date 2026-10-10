//! Linux Qt 正文过渡与光标动画的协调器。
//!
//! 正文每次编辑都由一个 `VisualEditState` 直接从最近收到 Qt 提交回执的 `VisualFrame`
//! 过渡到最新 canonical snapshot。协同 TextTransaction 的光标与正文共享其 timeline。

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use writer_core::editor::OffsetMap;

use crate::sujian_editor_item::animation::edit_timeline::{
    record_timeline_event, EditMotionSample,
};
use crate::sujian_editor_item::animation::visual_edit_state::VisualEditState;
use crate::sujian_editor_item::animation::visual_frame::VisualFrame;
use crate::sujian_editor_item::cursor_animation::{
    CursorAnimationPlan, CursorBlinkMode, CursorTransition,
};
use crate::sujian_editor_item::cursor_controller::CursorMoveSource;
use crate::sujian_editor_item::edit_motion::CursorRect;
use crate::sujian_editor_item::edit_motion::DeletedRangeEdge;
use crate::sujian_editor_item::frame_submission::{FrameSubmissionMailbox, SubmittedVisualFrame};
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

#[derive(Clone)]
struct SubmittedVisualFrameSource {
    ticket: crate::sujian_editor_item::frame_submission::SubmittedFrameTicket,
    visual_frame: VisualFrame,
}

pub(crate) struct VisualEditRequest {
    pub transaction_id: u64,
    pub operation_kind: String,
    pub base_snapshot: EditorLayoutSnapshot,
    pub target_snapshot: EditorLayoutSnapshot,
    pub offset_map: OffsetMap,
    pub deleted_range_edges: Vec<DeletedRangeEdge>,
    pub caret_motion: Option<(CursorRect, CursorRect)>,
    pub caret_byte_offsets: Option<(usize, usize)>,
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
    pub coordinated_animation_enabled: bool,
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
    last_submitted_visual_frame: Option<SubmittedVisualFrameSource>,
    /// 最近成功同步到 Scene Graph 的 owner table。取消或切章进入 handoff 后，
    /// 继续保留其动画行纹理 ID，直到静态层成功替换它。
    last_committed_ownership: Option<CommittedOwnership>,
    revision_transitions: VecDeque<FrameRevisionMap>,
    submission_mailbox: Arc<FrameSubmissionMailbox>,
    submission_window_generation: u64,
    pub(crate) document_session: u64,
    handoff_pending: bool,
    typing_animation_duration_ms: u32,
    cursor_animation_duration_ms: u32,
    paused_at: Option<Instant>,
}

impl LinuxEditorAnimationCoordinator {
    pub fn new() -> Self {
        Self {
            visual_edit_state: None,
            last_submitted_visual_frame: None,
            last_committed_ownership: None,
            revision_transitions: VecDeque::new(),
            submission_mailbox: Arc::default(),
            submission_window_generation: 0,
            document_session: 0,
            handoff_pending: false,
            typing_animation_duration_ms: 160,
            cursor_animation_duration_ms: 120,
            paused_at: None,
        }
    }

    pub(crate) fn frame_submission_mailbox(&self) -> Arc<FrameSubmissionMailbox> {
        Arc::clone(&self.submission_mailbox)
    }

    /// Apply only frames Qt has acknowledged. Called both at render synchronization and
    /// immediately before an edit chooses its visual source.
    pub(crate) fn consume_submitted_frames(&mut self) -> bool {
        self.sync_submission_window_generation();
        let frames = self.submission_mailbox.take_submitted_frames();
        let mut handoff_promoted = false;
        for frame in frames {
            handoff_promoted |= self.acknowledge_submitted_visual_frame(frame);
        }
        self.prune_revision_transitions();
        handoff_promoted
    }

    pub(crate) fn set_typing_animation_duration_ms(&mut self, ms: u32, now: Instant) {
        self.typing_animation_duration_ms = ms;
        let effective_now = self.effective_text_animation_time(now);
        if let Some(state) = self.visual_edit_state.as_mut() {
            state.retime(effective_now, u64::from(ms));
            let sample = state.sample(effective_now);
            record_timeline_event(
                "editor.edit.visual.duration_changed",
                state.transaction_id,
                state.transition_id,
                state.target_snapshot.revision,
                &state.operation_kind,
                ms,
                state.duration_ms(),
                None,
                sample.eased_progress,
                sample.timeline_progress,
                sample.caret_rect,
                sample.caret_target,
                state.reflow_cluster_count,
                state.crossfade_cluster_count,
                state.ownership_conflict_count,
            );
        }
    }

    pub(crate) fn set_cursor_animation_duration_ms(&mut self, ms: u32) {
        self.cursor_animation_duration_ms = ms;
    }

    /// 新编辑直接替换当前过渡，从 Qt 已确认提交的最新视觉状态重新计算。
    pub(crate) fn begin_visual_edit(&mut self, request: VisualEditRequest) {
        self.consume_submitted_frames();
        let retargeting = self.visual_edit_state.is_some();
        let effective_now = self.effective_text_animation_time(request.now);
        let effective_duration_ms = u64::from(self.typing_animation_duration_ms);
        let mut submitted_source = self
            .last_submitted_visual_frame
            .as_ref()
            .filter(|source| {
                source.ticket.document_session == self.document_session
                    && source.ticket.window_generation == self.submission_window_generation
            })
            .cloned();
        let frame_to_base_map = submitted_source
            .as_ref()
            .and_then(|source| {
                let frame = &source.visual_frame;
                let anchor = frame.canonical_revision?;
                self.map_revision_range(
                    anchor,
                    request.base_snapshot.revision,
                    frame.canonical_byte_len,
                )
            })
            .unwrap_or_else(|| {
                submitted_source = None;
                identity_map(snapshot_byte_len(&request.base_snapshot))
            });
        let inherited_spatial_speed_per_second = submitted_source
            .as_ref()
            .map(|source| source.visual_frame.spatial_speed_per_second);
        let inherited_caret_velocity = submitted_source
            .as_ref()
            .and_then(|source| source.visual_frame.caret_velocity);
        let inherited_shared_progress = submitted_source
            .as_ref()
            .map_or(0.0, |source| source.visual_frame.timeline_progress);
        let frame_to_target_map = if submitted_source.is_some() {
            frame_to_base_map.compose(&request.offset_map)
        } else {
            request.offset_map.clone()
        };

        let submitted_caret = submitted_source
            .as_ref()
            .and_then(|source| source.visual_frame.caret_rect);
        let caret_motion = request
            .caret_motion
            .map(|(fallback_from, to)| (submitted_caret.unwrap_or(fallback_from), to));

        self.revision_transitions.push_back(FrameRevisionMap {
            document_session: self.document_session,
            from_revision: request.base_snapshot.revision,
            to_revision: request.target_snapshot.revision,
            offset_map: request.offset_map.clone(),
        });

        if !request.animate {
            self.visual_edit_state = None;
            self.handoff_pending = true;
            self.prune_revision_transitions();
            return;
        }

        let state = VisualEditState::new(
            submitted_source.as_ref().map(|source| &source.visual_frame),
            &request.base_snapshot,
            request.target_snapshot,
            &frame_to_base_map,
            &frame_to_target_map,
            &request.deleted_range_edges,
            effective_now,
            effective_duration_ms,
            inherited_spatial_speed_per_second,
            inherited_caret_velocity,
            inherited_shared_progress,
            request.transaction_id,
            request.operation_kind,
            caret_motion,
            request.caret_byte_offsets,
            self.document_session,
        );
        let source_frame_id = submitted_source
            .as_ref()
            .map(|source| source.ticket.render_frame_id);
        self.visual_edit_state = Some(state);
        if let Some(state) = self.visual_edit_state.as_ref() {
            let sample = state.sample(effective_now);
            record_timeline_event(
                if retargeting {
                    "editor.edit.visual.retarget"
                } else {
                    "editor.edit.visual.start"
                },
                state.transaction_id,
                state.transition_id,
                state.target_snapshot.revision,
                &state.operation_kind,
                self.typing_animation_duration_ms,
                state.duration_ms(),
                source_frame_id,
                sample.eased_progress,
                sample.timeline_progress,
                sample.caret_from,
                sample.caret_target,
                state.reflow_cluster_count,
                state.crossfade_cluster_count,
                state.ownership_conflict_count,
            );
        }
        // 新 transition 覆盖了待收口的旧画面；它会和新的静态层一起原子提交。
        self.handoff_pending = false;
        self.prune_revision_transitions();
    }

    pub(crate) fn ownership_plan(
        &self,
        sample: Option<EditMotionSample>,
        canonical_snapshot: Option<&EditorLayoutSnapshot>,
        visual_caret: Option<VisualCaretGeometry>,
    ) -> RenderOwnershipPlan {
        let mut plan = match (self.visual_edit_state.as_ref(), sample) {
            (Some(state), Some(sample))
                if sample.transition_id == state.transition_id
                    && sample.document_session == self.document_session
                    && sample.target_revision == state.target_snapshot.revision =>
            {
                state.build_ownership_plan(sample, visual_caret)
            }
            _ => canonical_snapshot
                .map(RenderOwnershipPlan::canonical)
                .unwrap_or_default(),
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

    pub(crate) fn sample_edit_timeline(&self, frame_now: Instant) -> Option<EditMotionSample> {
        self.visual_edit_state
            .as_ref()
            .map(|state| state.sample(self.effective_text_animation_time(frame_now)))
    }

    /// 更新成功写入 Scene Graph 的 owner/resource 事实，并把视觉候选暂存到回执通道。
    /// 视觉 source 与终点状态要等 `afterFrameEnd` 回执后才生效。
    pub(crate) fn commit_rendered_plan(
        &mut self,
        plan: &RenderOwnershipPlan,
        caret_rect: Option<CursorRect>,
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
        committed_frame.caret_rect = caret_rect;
        let mut committed_animation_ids = Vec::new();
        let keeps_animation = resources_ready && !plan.handoff_pending;
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
        if let Some(layout_revision) = plan.target_layout_revision {
            self.submission_mailbox.stage_rendered_frame(
                self.document_session,
                layout_revision,
                plan.transition_id,
                committed_revision,
                committed_frame,
                if keeps_animation {
                    plan.terminal_motion_indices.clone()
                } else {
                    Vec::new()
                },
                if keeps_animation {
                    plan.submitted_visible_widths.clone()
                } else {
                    Vec::new()
                },
                keeps_animation && plan.terminal_frame,
                plan.handoff_pending || !resources_ready,
                plan.shared_progress,
                plan.timeline_progress,
                plan.ownership_conflict_count,
            );
        }
    }

    fn acknowledge_submitted_visual_frame(&mut self, frame: SubmittedVisualFrame) -> bool {
        let ticket = frame.ticket;
        if ticket.document_session != self.document_session
            || ticket.window_generation != self.submission_window_generation
            || frame.visual_frame.document_session != ticket.document_session
            || frame.visual_frame.canonical_revision != Some(ticket.layout_revision)
            || frame.visual_frame.ownership_revision != ticket.ownership_revision
        {
            return false;
        }
        if self
            .last_submitted_visual_frame
            .as_ref()
            .is_some_and(|previous| {
                previous.ticket.window_generation == ticket.window_generation
                    && previous.ticket.render_frame_id >= ticket.render_frame_id
            })
        {
            return false;
        }

        let same_active_transition = self.visual_edit_state.as_ref().is_some_and(|state| {
            state.transition_id == ticket.transition_id
                && state.target_snapshot.revision == ticket.layout_revision
        });
        let handoff_promoted =
            frame.handoff_pending && (same_active_transition || self.visual_edit_state.is_none());
        let submitted_caret = frame.visual_frame.caret_rect;
        self.last_submitted_visual_frame = Some(SubmittedVisualFrameSource {
            ticket,
            visual_frame: frame.visual_frame,
        });

        if same_active_transition {
            if let Some(state) = self.visual_edit_state.as_mut() {
                state.commit_submitted_visible_widths(&frame.visible_width_updates);
                state.commit_terminal_motions(&frame.terminal_motion_indices);
                if state.first_committed_frame_id.is_none() {
                    state.first_committed_frame_id = Some(ticket.render_frame_id);
                    record_timeline_event(
                        "editor.edit.visual.first_committed_frame",
                        state.transaction_id,
                        state.transition_id,
                        state.target_snapshot.revision,
                        &state.operation_kind,
                        self.typing_animation_duration_ms,
                        state.duration_ms(),
                        Some(ticket.render_frame_id),
                        frame.shared_progress,
                        frame.timeline_progress,
                        submitted_caret,
                        state.sample(Instant::now()).caret_target,
                        state.reflow_cluster_count,
                        state.crossfade_cluster_count,
                        frame.ownership_conflict_count,
                    );
                }
                if frame.terminal_frame {
                    state.terminal_frame_committed = true;
                }
            }
        }
        if handoff_promoted {
            if same_active_transition {
                if let Some(state) = self.visual_edit_state.as_ref() {
                    record_timeline_event(
                        "editor.edit.visual.handoff",
                        state.transaction_id,
                        state.transition_id,
                        state.target_snapshot.revision,
                        &state.operation_kind,
                        self.typing_animation_duration_ms,
                        state.duration_ms(),
                        Some(ticket.render_frame_id),
                        frame.shared_progress,
                        frame.timeline_progress,
                        submitted_caret,
                        state.sample(Instant::now()).caret_target,
                        state.reflow_cluster_count,
                        state.crossfade_cluster_count,
                        frame.ownership_conflict_count,
                    );
                }
                self.visual_edit_state = None;
            }
            self.handoff_pending = false;
        }
        self.prune_revision_transitions();
        handoff_promoted
    }

    fn sync_submission_window_generation(&mut self) {
        let generation = self.submission_mailbox.window_generation();
        if generation != self.submission_window_generation {
            self.submission_window_generation = generation;
            self.last_submitted_visual_frame = None;
        }
    }

    fn map_revision_range(
        &self,
        from_revision: LayoutRevision,
        to_revision: LayoutRevision,
        identity_byte_len: usize,
    ) -> Option<OffsetMap> {
        if from_revision == to_revision {
            return Some(identity_map(identity_byte_len));
        }
        if from_revision > to_revision {
            return None;
        }

        let mut revision = from_revision;
        let mut composed: Option<OffsetMap> = None;
        while revision < to_revision {
            let transition = self.revision_transitions.iter().find(|transition| {
                transition.document_session == self.document_session
                    && transition.from_revision == revision
            })?;
            if transition.to_revision <= revision || transition.to_revision > to_revision {
                return None;
            }
            composed = Some(match composed {
                Some(previous) => previous.compose(&transition.offset_map),
                None => transition.offset_map.clone(),
            });
            revision = transition.to_revision;
        }
        composed
    }

    fn prune_revision_transitions(&mut self) {
        let submitted_revision = self
            .last_submitted_visual_frame
            .as_ref()
            .filter(|source| {
                source.ticket.document_session == self.document_session
                    && source.ticket.window_generation == self.submission_window_generation
            })
            .map(|source| source.ticket.layout_revision);
        let unconfirmed_revision = self.submission_mailbox.oldest_unconfirmed_revision();
        let oldest_needed = submitted_revision
            .into_iter()
            .chain(unconfirmed_revision)
            .min();
        if let Some(oldest_needed) = oldest_needed {
            self.revision_transitions.retain(|transition| {
                transition.document_session == self.document_session
                    && transition.to_revision > oldest_needed
            });
        } else {
            self.revision_transitions.clear();
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
        self.consume_submitted_frames();
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
        let duration_ms = state.duration_ms();
        let committed_frame = self
            .last_submitted_visual_frame
            .as_ref()
            .filter(|source| {
                source.ticket.document_session == self.document_session
                    && source.ticket.window_generation == self.submission_window_generation
                    && source.ticket.layout_revision == target_snapshot.revision
            })
            .map(|source| source.visual_frame.clone());

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
                duration_ms,
                Some(frame.spatial_speed_per_second),
                frame.caret_velocity,
                frame.timeline_progress,
                state.transaction_id,
                state.operation_kind.clone(),
                None,
                None,
                self.document_session,
            ));
        } else if let Some(state) = self.visual_edit_state.as_mut() {
            // No frame from this revision has received a submission acknowledgment, so there is no
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

    /// Preserve every texture that can still be used by an active transition, a submitted
    /// visual source, a committed scene-graph owner, or a frame awaiting Qt acknowledgment.
    pub(crate) fn collect_active_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids = self.active_snapshot_ids();
        if let Some(source) = self.last_submitted_visual_frame.as_ref() {
            for cluster in &source.visual_frame.clusters {
                if !ids.contains(&cluster.snapshot_id) {
                    ids.push(cluster.snapshot_id);
                }
            }
        }
        if let Some(committed) = self.last_committed_ownership.as_ref() {
            for snapshot_id in &committed.animation_snapshot_ids {
                if !ids.contains(snapshot_id) {
                    ids.push(*snapshot_id);
                }
            }
        }
        for snapshot_id in self.submission_mailbox.active_snapshot_ids() {
            if !ids.contains(&snapshot_id) {
                ids.push(snapshot_id);
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
        had
    }

    /// 切换文档时结束旧视觉会话。旧 Scene Graph owner/纹理仍保留作已提交画面，
    /// 但不再允许它成为新文档 transition 的 source。
    pub(crate) fn reset_document_visual_session(&mut self) {
        self.document_session = self.document_session.wrapping_add(1);
        self.visual_edit_state = None;
        self.last_submitted_visual_frame = None;
        self.revision_transitions.clear();
        self.submission_mailbox.discard_all_frames();
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
        }
    }

    pub(crate) fn finish_paused_text_animation_to_canonical(&mut self) -> bool {
        let had = self.visual_edit_state.is_some();
        self.visual_edit_state = None;
        self.paused_at = None;
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
        let shared_text_timeline = inputs.coordinated_animation_enabled
            && inputs.movement_source == CursorMoveSource::TextTransaction
            && self
                .visual_edit_state
                .as_ref()
                .is_some_and(VisualEditState::has_caret_timeline);
        let source_allows_tween = match inputs.movement_source {
            CursorMoveSource::TextTransaction => {
                inputs.cursor_animation_enabled && !shared_text_timeline
            }
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
        let driver_transition_id = if inputs.coordinated_animation_enabled
            && inputs.movement_source == CursorMoveSource::TextTransaction
        {
            self.visual_edit_state
                .as_ref()
                .map_or(0, |state| state.transition_id)
        } else {
            0
        };

        CursorAnimationPlan {
            should_be_visible,
            transition,
            movement_source: inputs.movement_source,
            driver_revision: inputs.driver_revision,
            driver_session: self.document_session,
            driver_transition_id,
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
