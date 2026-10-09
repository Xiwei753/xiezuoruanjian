//! Thread-safe acknowledgments for frames Qt actually submits.
//!
//! `updatePaintNode()` runs during scene-graph synchronization, so it only publishes a
//! candidate ticket. `afterSynchronizing` moves the ticket into the current window frame;
//! `afterFrameEnd` confirms it after Qt submits that frame. Both Qt callbacks only touch
//! this mailbox and never inspect editor or GUI state.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use super::layout_revision::LayoutRevision;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SubmittedFrameTicket {
    pub document_session: u64,
    pub layout_revision: LayoutRevision,
    pub render_frame_id: u64,
}

#[derive(Default)]
struct FrameMailboxState {
    pending_sync: Option<SubmittedFrameTicket>,
    synchronized: Option<SubmittedFrameTicket>,
    submitted: VecDeque<SubmittedFrameTicket>,
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
    /// Publish a successfully rendered animated plan for the next Qt synchronization.
    pub(crate) fn publish_rendered_animation(
        &self,
        document_session: u64,
        layout_revision: LayoutRevision,
    ) {
        let ticket = SubmittedFrameTicket {
            document_session,
            layout_revision,
            render_frame_id: self.next_frame_id.fetch_add(1, Ordering::Relaxed),
        };
        let mut state = self.lock_state();
        state.pending_sync = Some(ticket);
    }

    /// Associate a pending updatePaintNode ticket with the synchronization just completed.
    /// An empty sync deliberately clears an older unsubmitted ticket.
    pub(crate) fn after_synchronizing(&self) {
        let mut state = self.lock_state();
        state.synchronized = state.pending_sync.take();
    }

    /// Confirm only the ticket captured for this frame, after Qt submits the scene graph.
    pub(crate) fn after_frame_end(&self) {
        let mut state = self.lock_state();
        if let Some(ticket) = state.synchronized.take() {
            state.submitted.push_back(ticket);
        }
    }

    pub(crate) fn take_submitted_frames(&self) -> Vec<SubmittedFrameTicket> {
        self.lock_state().submitted.drain(..).collect()
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, FrameMailboxState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
