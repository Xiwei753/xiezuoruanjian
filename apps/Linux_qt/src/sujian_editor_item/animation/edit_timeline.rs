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
    timeline_progress: f64,
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
        "timeline_progress".to_string(),
        serde_json::json!(timeline_progress),
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
    /// Local distance through this submitted-source to canonical-target transition.
    pub eased_progress: f64,
    /// Continuous phase used to carry the response clock across a retarget.
    pub timeline_progress: f64,
    /// Shared distance velocity, in progress units per second.
    pub velocity_per_second: f64,
    /// Representative screen-space speed of the geometry following this timeline.
    pub spatial_speed_per_second: f64,
    pub caret_from: Option<CursorRect>,
    pub caret_target: Option<CursorRect>,
    pub caret_rect: Option<CursorRect>,
    /// Geometric caret velocity at this sample, in document units per second.
    pub caret_velocity: Option<CursorRect>,
}

/// Piecewise-linear caret path through real line boundaries. Time is still sampled once;
/// this path only maps that shared distance sample into cross-line geometry.
#[derive(Clone, Debug)]
pub(crate) struct CaretPath {
    points: Vec<CursorRect>,
    cumulative: Vec<f64>,
    total_length: f64,
}

impl CaretPath {
    pub(crate) fn new(points: Vec<CursorRect>) -> Option<Self> {
        if points.len() < 2 {
            return None;
        }
        let mut compact = Vec::with_capacity(points.len());
        for point in points {
            if compact
                .last()
                .is_none_or(|last| caret_distance(*last, point) > 0.01)
            {
                compact.push(point);
            }
        }
        if compact.len() < 2 {
            return None;
        }
        let mut cumulative = Vec::with_capacity(compact.len());
        cumulative.push(0.0);
        for pair in compact.windows(2) {
            cumulative
                .push(cumulative.last().copied().unwrap_or(0.0) + caret_distance(pair[0], pair[1]));
        }
        let total_length = cumulative.last().copied().unwrap_or(0.0);
        (total_length > 0.01).then_some(Self {
            points: compact,
            cumulative,
            total_length,
        })
    }

    pub(crate) fn sample(&self, progress: f64) -> CursorRect {
        let distance = progress.clamp(0.0, 1.0) * self.total_length;
        let segment = self
            .cumulative
            .windows(2)
            .position(|window| distance <= window[1])
            .unwrap_or(self.points.len() - 2);
        let start = self.cumulative[segment];
        let end = self.cumulative[segment + 1];
        let local = if end > start {
            (distance - start) / (end - start)
        } else {
            1.0
        };
        lerp_caret(self.points[segment], self.points[segment + 1], local)
    }

    pub(crate) fn derivative(&self, progress: f64) -> CursorRect {
        let distance = progress.clamp(0.0, 1.0) * self.total_length;
        let segment = self
            .cumulative
            .windows(2)
            .position(|window| distance <= window[1])
            .unwrap_or(self.points.len() - 2);
        let start = self.cumulative[segment];
        let end = self.cumulative[segment + 1];
        let segment_length = end - start;
        if segment_length <= f64::EPSILON {
            return zero_caret_velocity();
        }
        scale_caret(
            subtract_caret(self.points[segment + 1], self.points[segment]),
            self.total_length / segment_length,
        )
    }

    pub(crate) fn initial_progress_velocity(&self, velocity: CursorRect) -> f64 {
        project_velocity(self.derivative(0.0), velocity)
    }

    pub(crate) fn length(&self) -> f64 {
        self.total_length
    }

    pub(crate) fn contains_point(&self, x: f64, y: f64) -> bool {
        self.points
            .windows(2)
            .any(|pair| point_is_on_route(x, y, pair[0].x, pair[0].top, pair[1].x, pair[1].top))
    }
}

fn point_is_on_route(x: f64, y: f64, from_x: f64, from_y: f64, to_x: f64, to_y: f64) -> bool {
    let dx = to_x - from_x;
    let dy = to_y - from_y;
    let length_squared = dx * dx + dy * dy;
    if length_squared <= 1e-6 {
        return (x - to_x).abs() <= 0.5 && (y - to_y).abs() <= 0.5;
    }
    let t = (((x - from_x) * dx + (y - from_y) * dy) / length_squared).clamp(0.0, 1.0);
    let projected_x = from_x + t * dx;
    let projected_y = from_y + t * dy;
    (x - projected_x).hypot(y - projected_y) <= 1.0
}

fn caret_distance(a: CursorRect, b: CursorRect) -> f64 {
    (a.x - b.x)
        .hypot(a.top - b.top)
        .hypot(a.bottom - b.bottom)
        .hypot(a.baseline_y - b.baseline_y)
}

#[derive(Clone, Debug)]
pub(crate) struct EditVisualTimeline {
    pub transition_id: u64,
    pub document_session: u64,
    pub target_revision: LayoutRevision,
    started_at: Instant,
    duration_ms: u64,
    segment_start_shared_progress: f64,
    segment_start_motion_progress: f64,
    initial_progress_velocity_per_second: f64,
    reference_distance: f64,
    caret_from: Option<CursorRect>,
    caret_target: Option<CursorRect>,
    caret_path: Option<CaretPath>,
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
        caret_path: Option<CaretPath>,
        inherited_spatial_speed_per_second: Option<f64>,
        inherited_caret_velocity: Option<CursorRect>,
        reference_distance: f64,
        shared_progress_start: f64,
    ) -> Self {
        let shared_progress_start = shared_progress_start.clamp(0.0, 1.0);
        let response_seconds = duration_ms as f64 / 1000.0;
        let default_spatial_speed = if response_seconds <= f64::EPSILON {
            0.0
        } else {
            3.0 * reference_distance.max(1.0) / response_seconds
        };
        let initial_progress_velocity_per_second = inherited_caret_velocity
            .zip(caret_path.as_ref())
            .map(|(velocity, path)| path.initial_progress_velocity(velocity))
            .or_else(|| {
                inherited_caret_velocity
                    .zip(caret_from.zip(caret_target))
                    .map(|(velocity, (from, target))| {
                        project_velocity(subtract_caret(target, from), velocity)
                    })
            })
            .unwrap_or_else(|| {
                inherited_spatial_speed_per_second.unwrap_or(default_spatial_speed)
                    / reference_distance.max(1.0)
            })
            .max(0.0);
        Self {
            transition_id,
            document_session,
            target_revision,
            started_at,
            duration_ms,
            segment_start_shared_progress: shared_progress_start,
            segment_start_motion_progress: 0.0,
            initial_progress_velocity_per_second,
            reference_distance: reference_distance.max(1.0),
            caret_from,
            caret_target,
            caret_path,
        }
    }

    pub(crate) fn sample(&self, frame_now: Instant) -> EditMotionSample {
        let elapsed_seconds = frame_now
            .saturating_duration_since(self.started_at)
            .as_secs_f64();
        let response_seconds = self.duration_ms as f64 / 1000.0;
        let omega = if response_seconds <= f64::EPSILON {
            0.0
        } else {
            12.0 / response_seconds
        };
        let (mut eased_progress, velocity_per_second) = if omega <= f64::EPSILON {
            (1.0, 0.0)
        } else {
            damped_progress(
                self.segment_start_motion_progress,
                self.initial_progress_velocity_per_second,
                omega,
                elapsed_seconds,
            )
        };
        if 1.0 - eased_progress <= 1e-4 {
            eased_progress = 1.0;
        }
        let timeline_progress = self.segment_start_shared_progress
            + (1.0 - self.segment_start_shared_progress) * eased_progress;
        let velocity_per_second = if eased_progress >= 1.0 {
            0.0
        } else {
            velocity_per_second
        };
        let caret_velocity = self
            .caret_path
            .as_ref()
            .map(|path| scale_caret(path.derivative(eased_progress), velocity_per_second))
            .or_else(|| {
                self.caret_from
                    .zip(self.caret_target)
                    .map(|(from, target)| {
                        scale_caret(subtract_caret(target, from), velocity_per_second)
                    })
            });
        let caret_rect = self
            .caret_path
            .as_ref()
            .map(|path| path.sample(eased_progress))
            .or_else(|| {
                self.caret_from
                    .zip(self.caret_target)
                    .map(|(from, target)| lerp_caret(from, target, eased_progress))
            });
        EditMotionSample {
            transition_id: self.transition_id,
            document_session: self.document_session,
            target_revision: self.target_revision,
            eased_progress,
            timeline_progress,
            velocity_per_second,
            spatial_speed_per_second: velocity_per_second * self.reference_distance,
            caret_from: self.caret_from,
            caret_target: self.caret_target,
            caret_rect,
            caret_velocity,
        }
    }

    pub(crate) fn retime(&mut self, now: Instant, duration_ms: u64) -> f64 {
        let sample = self.sample(now);
        let progress = sample.eased_progress;
        self.duration_ms = duration_ms;
        self.segment_start_shared_progress = sample.timeline_progress;
        self.segment_start_motion_progress = progress;
        self.initial_progress_velocity_per_second = sample.velocity_per_second;
        self.started_at = now;
        progress
    }

    pub(crate) fn shift_started_at(&mut self, delta: Duration) {
        self.started_at = self
            .started_at
            .checked_add(delta)
            .unwrap_or(self.started_at);
    }

    pub(crate) fn duration_ms(&self) -> u64 {
        self.duration_ms
    }

    pub(crate) fn has_caret_motion(&self) -> bool {
        self.caret_from.is_some() && self.caret_target.is_some()
    }
}

fn damped_progress(
    start_progress: f64,
    initial_velocity: f64,
    omega: f64,
    elapsed_seconds: f64,
) -> (f64, f64) {
    let remaining = (1.0 - start_progress).max(0.0);
    if remaining <= f64::EPSILON {
        return (1.0, 0.0);
    }
    let initial_velocity = initial_velocity.clamp(0.0, omega * remaining);
    let slope = omega * remaining - initial_velocity;
    let decay = (-omega * elapsed_seconds).exp();
    let error = (remaining + slope * elapsed_seconds) * decay;
    let velocity = (initial_velocity + omega * slope * elapsed_seconds) * decay;
    ((1.0 - error).clamp(start_progress, 1.0), velocity.max(0.0))
}

fn project_velocity(route_derivative: CursorRect, velocity: CursorRect) -> f64 {
    let route = [
        route_derivative.x,
        route_derivative.top,
        route_derivative.bottom,
        route_derivative.baseline_y,
    ];
    let velocity = [
        velocity.x,
        velocity.top,
        velocity.bottom,
        velocity.baseline_y,
    ];
    let dot = route
        .iter()
        .zip(velocity)
        .map(|(route, velocity)| route * velocity)
        .sum::<f64>();
    let route_length_squared = route
        .iter()
        .map(|component| component * component)
        .sum::<f64>();
    if route_length_squared <= f64::EPSILON {
        0.0
    } else {
        (dot / route_length_squared).max(0.0)
    }
}

fn zero_caret_velocity() -> CursorRect {
    CursorRect {
        x: 0.0,
        top: 0.0,
        bottom: 0.0,
        baseline_y: 0.0,
    }
}

fn subtract_caret(a: CursorRect, b: CursorRect) -> CursorRect {
    CursorRect {
        x: a.x - b.x,
        top: a.top - b.top,
        bottom: a.bottom - b.bottom,
        baseline_y: a.baseline_y - b.baseline_y,
    }
}

fn scale_caret(caret: CursorRect, scale: f64) -> CursorRect {
    CursorRect {
        x: caret.x * scale,
        top: caret.top * scale,
        bottom: caret.bottom * scale,
        baseline_y: caret.baseline_y * scale,
    }
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
