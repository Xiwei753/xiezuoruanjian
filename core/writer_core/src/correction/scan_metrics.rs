//! Structured metrics for one correction scan request.
//!
//! Kept out of `engine.rs` so the scanner body stays under the repository's
//! production-file line budget and the event schema has one owner.

use std::collections::BTreeMap;
use std::time::Instant;

use super::result_cache::{content_fingerprint, ChapterCorrectionCacheState};

pub(super) struct CorrectionScanMetrics {
    pub(super) started_at: Instant,
    pub(super) session_id: u64,
    pub(super) text_revision: u64,
    pub(super) chapter_digest: String,
    pub(super) request_kind: &'static str,
    pub(super) result: &'static str,
    pub(super) total_segments: usize,
    pub(super) cache_valid_segments: usize,
    pub(super) rescanned_segments: usize,
    pub(super) model_calls: usize,
    /// Wall time actually spent inside model inference (not cache/queue).
    pub(super) model_duration_ms: u128,
    pub(super) input_char_total: usize,
    pub(super) candidates_produced: usize,
    pub(super) candidates_attributed: usize,
    pub(super) candidates_filtered_by_segment: usize,
    pub(super) final_suggestions: usize,
    /// Dirty segments left unscanned because the scan stopped early.
    pub(super) dirty_preserved_segments: usize,
    /// Model window index where the scan observed a newer revision; 0 means
    /// the scan ran to completion.
    pub(super) aborted_at_window: usize,
    pub(super) cache_hit: bool,
    pub(super) model_available: bool,
    pub(super) engine_attached: bool,
    pub(super) resource_fingerprint: String,
}

impl CorrectionScanMetrics {
    pub(super) fn new(
        session_id: u64,
        text_revision: u64,
        chapter_id: &str,
        request_kind: &'static str,
        model_available: bool,
        engine_attached: bool,
        resource_fingerprint: String,
    ) -> Self {
        Self {
            started_at: Instant::now(),
            session_id,
            text_revision,
            chapter_digest: content_fingerprint(chapter_id),
            request_kind,
            result: "error",
            total_segments: 0,
            cache_valid_segments: 0,
            rescanned_segments: 0,
            model_calls: 0,
            model_duration_ms: 0,
            input_char_total: 0,
            candidates_produced: 0,
            candidates_attributed: 0,
            candidates_filtered_by_segment: 0,
            final_suggestions: 0,
            dirty_preserved_segments: 0,
            aborted_at_window: 0,
            cache_hit: false,
            model_available,
            engine_attached,
            resource_fingerprint,
        }
    }

    pub(super) fn observe_cache(&mut self, state: &ChapterCorrectionCacheState) {
        self.total_segments = state.segments.len();
        self.cache_valid_segments = state
            .segments
            .iter()
            .filter(|segment| segment.checked.is_some() && !segment.dirty)
            .count();
    }

    pub(super) fn fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("session_id", self.session_id.to_string()),
            ("text_revision", self.text_revision.to_string()),
            ("chapter_digest", self.chapter_digest.clone()),
            ("request_kind", self.request_kind.to_string()),
            ("result", self.result.to_string()),
            (
                "duration_ms",
                self.started_at.elapsed().as_millis().to_string(),
            ),
            ("total_segments", self.total_segments.to_string()),
            (
                "cache_valid_segments",
                self.cache_valid_segments.to_string(),
            ),
            ("rescanned_segments", self.rescanned_segments.to_string()),
            ("model_calls", self.model_calls.to_string()),
            ("model_duration_ms", self.model_duration_ms.to_string()),
            ("input_char_total", self.input_char_total.to_string()),
            ("candidates_produced", self.candidates_produced.to_string()),
            (
                "candidates_filtered_by_segment",
                self.candidates_filtered_by_segment.to_string(),
            ),
            ("final_suggestions", self.final_suggestions.to_string()),
            (
                "dirty_preserved_segments",
                self.dirty_preserved_segments.to_string(),
            ),
            ("aborted_at_window", self.aborted_at_window.to_string()),
            ("cache_hit", self.cache_hit.to_string()),
            ("model_available", self.model_available.to_string()),
            ("engine_attached", self.engine_attached.to_string()),
            ("resource_fingerprint", self.resource_fingerprint.clone()),
        ]
    }
}

pub(super) fn record_correction_scan_event(event: &str, fields: Vec<(&'static str, String)>) {
    let field_map = fields
        .into_iter()
        .map(|(key, value)| (key.to_string(), serde_json::Value::String(value)))
        .collect::<BTreeMap<_, _>>();
    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        sequence: 0,
        session_id: String::new(),
        level: writer_diagnostics::DiagnosticLevel::Info,
        origin: writer_diagnostics::DiagnosticOrigin::App,
        event: event.to_string(),
        target: "correction.scan".to_string(),
        message: None,
        fields: field_map,
    });
}
