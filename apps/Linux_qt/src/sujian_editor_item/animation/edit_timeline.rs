//! A single frame clock for one text edit and its TextTransaction caret.

use std::time::{Duration, Instant};

use super::super::edit_motion::CursorRect;
use super::super::layout_revision::LayoutRevision;

#[allow(clippy::too_many_arguments)]
pub(crate) fn record_timeline_event(
    event: &str,
    transaction_id: u64,
    transition_id: u64,
    revision: LayoutRevision,
    operation_kind: &str,
    configured_duration_ms: u32,
    effective_duration_ms: u64,
    frame_id: Option<u64>,
    shared_progress: f64,
    caret_visual: Option<CursorRect>,
    caret_target: Option<CursorRect>,
    reflow_cluster_count: usize,
    crossfade_cluster_count: usize,
    ownership_conflict_count: usize,
) {
    let mut fields = std::collections::BTreeMap::new();
    fields.insert(
        "transaction_id".to_string(),
        serde_json::json!(transaction_id),
    );
    fields.insert(
        "transition_id".to_string(),
        serde_json::json!(transition_id),
    );
    fields.insert("revision".to_string(), serde_json::json!(revision.0));
    fields.insert(
        "operation_kind".to_string(),
        serde_json::json!(operation_kind),
    );
    fields.insert(
        "configured_duration_ms".to_string(),
        serde_json::json!(configured_duration_ms),
    );
    fields.insert(
        "effective_duration_ms".to_string(),
        serde_json::json!(effective_duration_ms),
    );
    fields.insert("frame_id".to_string(), serde_json::json!(frame_id));
    fields.insert(
        "shared_progress".to_string(),
        serde_json::json!(shared_progress),
    );
    fields.insert(
        "caret_visual_xy".to_string(),
        serde_json::json!(caret_visual.map(|caret| [caret.x, caret.top])),
    );
    fields.insert(
        "caret_target_xy".to_string(),
        serde_json::json!(caret_target.map(|caret| [caret.x, caret.top])),
    );
    fields.insert(
        "reflow_cluster_count".to_string(),
        serde_json::json!(reflow_cluster_count),
    );
    fields.insert(
        "crossfade_cluster_count".to_string(),
        serde_json::json!(crossfade_cluster_count),
    );
    fields.insert(
        "ownership_conflict_count".to_string(),
        serde_json::json!(ownership_conflict_count),
    );
    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        sequence: 0,
        session_id: String::new(),
        level: writer_diagnostics::DiagnosticLevel::Info,
        origin: writer_diagnostics::DiagnosticOrigin::App,
        event: event.to_string(),
        target: "editor.anim.timeline".to_string(),
        message: None,
        fields,
    });
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EditMotionSample {
    pub transition_id: u64,
    pub document_session: u64,
    pub target_revision: LayoutRevision,
    /// Un-eased time progress. This is monotonic for the lifetime of a transition.
    pub progress: f64,
    /// The one eased value consumed by text geometry and the coordinated caret.
    pub eased_progress: f64,
    pub caret_from: Option<CursorRect>,
    pub caret_target: Option<CursorRect>,
    pub caret_rect: Option<CursorRect>,
}

#[derive(Clone, Debug)]
pub(crate) struct EditVisualTimeline {
    pub transition_id: u64,
    pub document_session: u64,
    pub target_revision: LayoutRevision,
    started_at: Instant,
    duration_ms: u64,
    caret_from: Option<CursorRect>,
    caret_target: Option<CursorRect>,
}

impl EditVisualTimeline {
    pub(crate) fn new(
        transition_id: u64,
        document_session: u64,
        target_revision: LayoutRevision,
        started_at: Instant,
        duration_ms: u64,
        caret_from: Option<CursorRect>,
        caret_target: Option<CursorRect>,
    ) -> Self {
        Self {
            transition_id,
            document_session,
            target_revision,
            started_at,
            duration_ms,
            caret_from,
            caret_target,
        }
    }

    pub(crate) fn sample(&self, frame_now: Instant) -> EditMotionSample {
        let progress = if self.duration_ms == 0 {
            1.0
        } else {
            (frame_now
                .saturating_duration_since(self.started_at)
                .as_secs_f64()
                * 1000.0
                / self.duration_ms as f64)
                .clamp(0.0, 1.0)
        };
        let eased_progress = ease_out_cubic(progress);
        let caret_rect = self
            .caret_from
            .zip(self.caret_target)
            .map(|(from, target)| lerp_caret(from, target, eased_progress));
        EditMotionSample {
            transition_id: self.transition_id,
            document_session: self.document_session,
            target_revision: self.target_revision,
            progress,
            eased_progress,
            caret_from: self.caret_from,
            caret_target: self.caret_target,
            caret_rect,
        }
    }

    pub(crate) fn retime(&mut self, now: Instant, duration_ms: u64) -> f64 {
        let progress = self.sample(now).progress;
        self.duration_ms = duration_ms;
        let elapsed_ms = (progress * duration_ms as f64).round() as u64;
        self.started_at = now
            .checked_sub(Duration::from_millis(elapsed_ms))
            .unwrap_or(now);
        progress
    }

    pub(crate) fn shift_started_at(&mut self, delta: Duration) {
        self.started_at = self
            .started_at
            .checked_add(delta)
            .unwrap_or(self.started_at);
    }

    pub(crate) fn remaining_duration_ms(&self, now: Instant) -> u64 {
        if self.duration_ms == 0 {
            return 0;
        }
        let progress = self.sample(now).progress;
        (self.duration_ms as f64 * (1.0 - progress))
            .round()
            .clamp(1.0, self.duration_ms as f64) as u64
    }

    pub(crate) fn duration_ms(&self) -> u64 {
        self.duration_ms
    }

    pub(crate) fn has_caret_motion(&self) -> bool {
        self.caret_from.is_some() && self.caret_target.is_some()
    }
}

fn ease_out_cubic(progress: f64) -> f64 {
    let progress = progress.clamp(0.0, 1.0);
    1.0 - (1.0 - progress).powi(3)
}

fn lerp_caret(from: CursorRect, target: CursorRect, progress: f64) -> CursorRect {
    let lerp = |a: f64, b: f64| a + (b - a) * progress;
    CursorRect {
        x: lerp(from.x, target.x),
        top: lerp(from.top, target.top),
        bottom: lerp(from.bottom, target.bottom),
        baseline_y: lerp(from.baseline_y, target.baseline_y),
    }
}
