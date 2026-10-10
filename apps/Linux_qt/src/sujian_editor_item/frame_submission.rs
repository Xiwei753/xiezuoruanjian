//! Thread-safe staging and acknowledgments for frames Qt actually submits.
//!
//! `updatePaintNode()` stages a visual snapshot during scene-graph synchronization.
//! `afterSynchronizing` associates that snapshot with the window frame, and
//! `afterFrameEnd` moves it to the submitted queue. GUI state is only updated when that
//! queue is drained; the Qt callbacks touch this mailbox alone.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use super::animation::visual_frame::VisualFrame;
use super::layout_revision::LayoutRevision;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SubmittedFrameTicket {
    pub document_session: u64,
    pub layout_revision: LayoutRevision,
    pub transition_id: u64,
    pub ownership_revision: u64,
    pub render_frame_id: u64,
    pub window_generation: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct SubmittedVisualFrame {
    pub ticket: SubmittedFrameTicket,
    pub visual_frame: VisualFrame,
    pub terminal_motion_indices: Vec<usize>,
    pub visible_width_updates: Vec<(usize, f64)>,
    pub terminal_frame: bool,
    pub handoff_pending: bool,
    pub animation_frame: bool,
}

#[derive(Default)]
struct FrameMailboxState {
    window_generation: u64,
    pending_sync: Option<SubmittedVisualFrame>,
    synchronized: Option<SubmittedVisualFrame>,
    submitted: VecDeque<SubmittedVisualFrame>,
}

pub(crate) struct FrameSubmissionMailbox {
    next_frame_id: AtomicU64,
    state: Mutex<FrameMailboxState>,
}

impl Default for FrameSubmissionMailbox {
    fn default() -> Self {
        Self {
            next_frame_id: AtomicU64::new(1),
            state: Mutex::new(FrameMailboxState::default()),
        }
    }
}

impl FrameSubmissionMailbox {
    /// Stage a successfully rendered frame. It is not a visual source until Qt submits it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn stage_rendered_frame(
        &self,
        document_session: u64,
        layout_revision: LayoutRevision,
        transition_id: u64,
        ownership_revision: u64,
        visual_frame: VisualFrame,
        terminal_motion_indices: Vec<usize>,
        visible_width_updates: Vec<(usize, f64)>,
        terminal_frame: bool,
        handoff_pending: bool,
        animation_frame: bool,
    ) {
        let mut state = self.lock_state();
        let staged = SubmittedVisualFrame {
            ticket: SubmittedFrameTicket {
                document_session,
                layout_revision,
                transition_id,
                ownership_revision,
                render_frame_id: self.next_frame_id.fetch_add(1, Ordering::Relaxed),
                window_generation: state.window_generation,
            },
            visual_frame,
            terminal_motion_indices,
            visible_width_updates,
            terminal_frame,
            handoff_pending,
            animation_frame,
        };
        state.pending_sync = Some(staged);
    }

    /// Rebind signals to another window and invalidate every ticket from the old one.
    pub(crate) fn reset_window(&self) -> u64 {
        let mut state = self.lock_state();
        state.window_generation = state.window_generation.wrapping_add(1);
        state.pending_sync = None;
        state.synchronized = None;
        state.submitted.clear();
        state.window_generation
    }

    pub(crate) fn discard_all_frames(&self) {
        let mut state = self.lock_state();
        state.pending_sync = None;
        state.synchronized = None;
        state.submitted.clear();
    }

    pub(crate) fn window_generation(&self) -> u64 {
        self.lock_state().window_generation
    }

    /// Associate the latest staged plan with this window synchronization.
    /// Empty synchronization clears an older plan that was never submitted.
    pub(crate) fn after_synchronizing(&self, window_generation: u64) {
        let mut state = self.lock_state();
        if state.window_generation != window_generation {
            return;
        }
        state.synchronized = state.pending_sync.take();
    }

    /// Confirm the plan captured for this synchronization after Qt submits the frame.
    pub(crate) fn after_frame_end(&self, window_generation: u64) {
        let mut state = self.lock_state();
        if state.window_generation != window_generation {
            return;
        }
        if let Some(frame) = state.synchronized.take() {
            state.submitted.push_back(frame);
        }
    }

    pub(crate) fn take_submitted_frames(&self) -> Vec<SubmittedVisualFrame> {
        self.lock_state().submitted.drain(..).collect()
    }

    pub(crate) fn oldest_unconfirmed_revision(&self) -> Option<LayoutRevision> {
        let state = self.lock_state();
        state
            .pending_sync
            .iter()
            .chain(state.synchronized.iter())
            .chain(state.submitted.iter())
            .map(|frame| frame.ticket.layout_revision)
            .min()
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, FrameMailboxState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
