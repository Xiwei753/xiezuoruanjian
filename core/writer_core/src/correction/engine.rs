use std::collections::BTreeMap;
use std::sync::{Arc, MutexGuard};
use std::time::Instant;

use sujian_proof::{
    CorrectionConfig, CorrectionDiagnostic, CorrectionEngine, NeuralPredictor, RuleSet,
};

use crate::api::{
    CorrectionChapterSnapshotDto, CorrectionCheckedRangeDto, CorrectionSuggestionDto, WriterError,
};

use super::result_cache::{
    apply_pending_windows, content_fingerprint, context_range,
    mark_unseen_segments_in_edit_windows, reconcile_edits, reconcile_full, CachedSuggestion,
    ChapterCorrectionCacheState, ChapterCorrectionSlot, EditRange,
};
use super::CorrectionStore;

struct CorrectionScanMetrics {
    started_at: Instant,
    session_id: u64,
    text_revision: u64,
    chapter_digest: String,
    request_kind: &'static str,
    result: &'static str,
    total_segments: usize,
    cache_valid_segments: usize,
    rescanned_segments: usize,
    model_calls: usize,
    input_char_total: usize,
    candidates_produced: usize,
    candidates_attributed: usize,
    candidates_filtered_by_segment: usize,
    final_suggestions: usize,
    cache_hit: bool,
    model_available: bool,
    resource_fingerprint: String,
}

impl CorrectionScanMetrics {
    fn new(scanner: &CorrectionScanner, request_kind: &'static str) -> Self {
        Self {
            started_at: Instant::now(),
            session_id: scanner.session_id,
            text_revision: scanner.text_revision,
            chapter_digest: content_fingerprint(&scanner.chapter_id),
            request_kind,
            result: "error",
            total_segments: 0,
            cache_valid_segments: 0,
            rescanned_segments: 0,
            model_calls: 0,
            input_char_total: 0,
            candidates_produced: 0,
            candidates_attributed: 0,
            candidates_filtered_by_segment: 0,
            final_suggestions: 0,
            cache_hit: false,
            model_available: scanner.model_available,
            resource_fingerprint: scanner.scanner_fingerprint.clone(),
        }
    }

    fn observe_cache(&mut self, state: &ChapterCorrectionCacheState) {
        self.total_segments = state.segments.len();
        self.cache_valid_segments = state
            .segments
            .iter()
            .filter(|segment| segment.checked.is_some() && !segment.dirty)
            .count();
    }

    fn fields(&self) -> Vec<(&'static str, String)> {
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
            ("input_char_total", self.input_char_total.to_string()),
            ("candidates_produced", self.candidates_produced.to_string()),
            (
                "candidates_filtered_by_segment",
                self.candidates_filtered_by_segment.to_string(),
            ),
            ("final_suggestions", self.final_suggestions.to_string()),
            ("cache_hit", self.cache_hit.to_string()),
            ("model_available", self.model_available.to_string()),
            ("resource_fingerprint", self.resource_fingerprint.clone()),
        ]
    }
}

fn record_correction_scan_event(event: &str, fields: Vec<(&'static str, String)>) {
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

/// Merge dirty segments whose expanded context windows touch or overlap. A
/// segment's context is deterministic, so this does not change which text the
/// model sees; it only avoids running the same model window once per segment.
fn group_dirty_jobs(text: &str, jobs: &[(usize, usize, usize)]) -> Vec<Vec<(usize, usize, usize)>> {
    let mut groups: Vec<Vec<(usize, usize, usize)>> = Vec::new();
    let mut group_context_end = 0usize;
    for &job in jobs {
        let (context_start, context_end) = context_range(text, job.1, job.2);
        if let Some(group) = groups.last_mut() {
            if context_start <= group_context_end {
                group.push(job);
                group_context_end = group_context_end.max(context_end);
                continue;
            }
        }
        groups.push(vec![job]);
        group_context_end = context_end;
    }
    groups
}

fn suggestions_in_segment(
    diagnostics: &[CorrectionDiagnostic],
    start: usize,
    end: usize,
) -> Result<Vec<CachedSuggestion>, WriterError> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.start_byte >= start && diagnostic.end_byte <= end)
        .map(|diagnostic| {
            Ok(CachedSuggestion {
                start_byte: to_u32(diagnostic.start_byte - start)?,
                end_byte: to_u32(diagnostic.end_byte - start)?,
                original: diagnostic.original.clone(),
                replacement: diagnostic.replacement.clone(),
                source: diagnostic.source.clone(),
                rule_id: diagnostic.rule_id.clone(),
                severity: diagnostic.severity.clone(),
            })
        })
        .collect()
}

pub(crate) struct CorrectionScanner {
    store: CorrectionStore,
    project_id: String,
    chapter_id: String,
    session_id: u64,
    text_revision: u64,
    engine: Arc<CorrectionEngine>,
    model_available: bool,
    /// Fingerprint of the exact lexicon/model/dictionary snapshot captured
    /// when this scanner was built. It must travel with the engine so a
    /// preference change between construction and `check` cannot label old
    /// results with a newer scanner generation.
    scanner_fingerprint: String,
}

/// What a check request is allowed to recompute.
pub(crate) enum CorrectionCheckRequest {
    /// Restore only: verify the committed cache but never run the engine.
    RestoreOnly,
    /// Apply the edit transaction's real change ranges, then scan only the
    /// segments those edits touched.
    Edits {
        base_revision: u64,
        edits: Vec<EditRange>,
    },
    /// Segment the dirty windows recorded by lightweight edit updates and
    /// scan only those segments. This is the model phase of the two-stage
    /// editor flow.
    Pending,
    /// Recompute only unchecked segments overlapping the requested byte range.
    Range(usize, usize),
    /// Fill every unseen or invalid segment.
    All,
}

fn to_u32(value: usize) -> Result<u32, WriterError> {
    u32::try_from(value).map_err(|error| WriterError::Other(error.to_string()))
}

/// Cached suggestion offsets are relative to the segment start, including the
/// end offset. Convert both endpoints from the same base.
fn absolute_suggestion_range(
    segment_start: u32,
    suggestion: &CachedSuggestion,
) -> Result<(usize, usize), WriterError> {
    let start = usize::try_from(suggestion.start_byte)
        .map_err(|error| WriterError::Other(error.to_string()))?;
    let end = usize::try_from(suggestion.end_byte)
        .map_err(|error| WriterError::Other(error.to_string()))?;
    let base = segment_start as usize;
    Ok((base + start, base + end))
}

fn lock_slot(
    slot: &ChapterCorrectionSlot,
) -> Result<MutexGuard<'_, ChapterCorrectionCacheState>, WriterError> {
    slot.state
        .lock()
        .map_err(|_| WriterError::Other("correction result cache lock poisoned".into()))
}

impl CorrectionScanner {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        store: CorrectionStore,
        project_id: String,
        chapter_id: String,
        session_id: u64,
        text_revision: u64,
    ) -> Result<Self, WriterError> {
        // Bracket resource loading with the fingerprint so the captured
        // generation always describes the exact lexicon/model the engine was
        // built from, even if a dictionary or settings change races setup.
        let scanner_fingerprint_before = store.scanner_fingerprint(&project_id)?;
        let (lexicon_version, lexicon) = store.lexicon_snapshot_for_project(&project_id)?;
        let model_load_started_at = Instant::now();
        let (model_key, model) = match store.configured_model_snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                record_correction_scan_event(
                    "correction.model.load",
                    vec![
                        ("session_id", session_id.to_string()),
                        ("text_revision", text_revision.to_string()),
                        ("chapter_digest", content_fingerprint(&chapter_id)),
                        ("result", "error".to_string()),
                        ("model_available", "false".to_string()),
                        (
                            "model_load_ms",
                            model_load_started_at.elapsed().as_millis().to_string(),
                        ),
                        ("resource_fingerprint", scanner_fingerprint_before.clone()),
                    ],
                );
                return Err(error);
            }
        };
        let model_available = model.is_some();
        record_correction_scan_event(
            "correction.model.load",
            vec![
                ("session_id", session_id.to_string()),
                ("text_revision", text_revision.to_string()),
                ("chapter_digest", content_fingerprint(&chapter_id)),
                ("result", "success".to_string()),
                ("model_available", model_available.to_string()),
                (
                    "model_load_ms",
                    model_load_started_at.elapsed().as_millis().to_string(),
                ),
                ("resource_fingerprint", scanner_fingerprint_before.clone()),
            ],
        );
        let engine = if let Some(engine) =
            store.cached_engine(&project_id, lexicon_version, model_key.as_ref())?
        {
            engine
        } else {
            let rules = RuleSet::with_common_rules();
            let config = CorrectionConfig::default();
            let engine = if let Some(model) = model {
                let predictor: Arc<dyn NeuralPredictor> = model;
                CorrectionEngine::new_shared(predictor, lexicon, rules, config)
            } else {
                CorrectionEngine::rules_only_shared(lexicon, rules, config)
            }
            .map_err(|error| WriterError::Other(error.to_string()))?;
            store.cache_engine_if_model_current(
                &project_id,
                lexicon_version,
                model_key,
                Arc::new(engine),
            )?
        };
        let scanner_fingerprint = store.scanner_fingerprint(&project_id)?;
        if scanner_fingerprint != scanner_fingerprint_before {
            return Err(WriterError::Other(
                "correction scanner configuration changed during setup; retry the request".into(),
            ));
        }
        Ok(Self {
            store,
            project_id,
            chapter_id,
            session_id,
            text_revision,
            engine,
            model_available,
            scanner_fingerprint,
        })
    }

    /// Cached suggestions and verified coverage only; never scans.
    pub(crate) fn chapter_snapshot(
        &self,
        text: &str,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        self.check(text, CorrectionCheckRequest::RestoreOnly)
    }

    /// Applies the edit transaction's real change ranges and returns the
    /// merged chapter snapshot. Only touched segments run the model.
    pub(crate) fn apply_edits(
        &self,
        text: &str,
        base_revision: u64,
        edits: Vec<EditRange>,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        self.check(
            text,
            CorrectionCheckRequest::Edits {
                base_revision,
                edits,
            },
        )
    }

    /// Segments the dirty windows applied by the lightweight edit phase and
    /// scans only those segments.
    pub(crate) fn scan_pending(
        &self,
        text: &str,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        self.check(text, CorrectionCheckRequest::Pending)
    }

    /// Verifies the cache, recomputes the requested range, and returns the
    /// merged chapter snapshot.
    pub(crate) fn ensure_checked(
        &self,
        text: &str,
        start_byte: u32,
        end_byte: u32,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        let start =
            usize::try_from(start_byte).map_err(|error| WriterError::Other(error.to_string()))?;
        let end =
            usize::try_from(end_byte).map_err(|error| WriterError::Other(error.to_string()))?;
        if start > end
            || end > text.len()
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
        {
            return Err(WriterError::Other(format!(
                "invalid correction byte range {start_byte}..{end_byte} for text of {} bytes",
                text.len()
            )));
        }
        self.check(text, CorrectionCheckRequest::Range(start, end))
    }

    /// Fills every unseen or invalid segment.
    pub(crate) fn check_chapter(
        &self,
        text: &str,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        self.check(text, CorrectionCheckRequest::All)
    }

    fn check(
        &self,
        text: &str,
        request: CorrectionCheckRequest,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        let request_kind = match &request {
            CorrectionCheckRequest::RestoreOnly => "restore_only",
            CorrectionCheckRequest::Edits { .. } => "edits",
            CorrectionCheckRequest::Pending => "pending",
            CorrectionCheckRequest::Range(_, _) => "range",
            CorrectionCheckRequest::All => "all",
        };
        let mut metrics = CorrectionScanMetrics::new(self, request_kind);
        metrics.result = "running";
        record_correction_scan_event("correction.scan.start", metrics.fields());

        let result = self.check_inner(text, request, &mut metrics);
        if metrics.result == "running" {
            metrics.result = if result.is_ok() { "success" } else { "error" };
        }
        if let Ok(snapshot) = &result {
            metrics.final_suggestions = snapshot.suggestions.len();
        }
        record_correction_scan_event("correction.scan.end", metrics.fields());
        result
    }

    fn check_inner(
        &self,
        text: &str,
        request: CorrectionCheckRequest,
        metrics: &mut CorrectionScanMetrics,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        u32::try_from(text.len()).map_err(|error| {
            WriterError::Other(format!("chapter exceeds correction byte limit: {error}"))
        })?;

        let slot = self
            .store
            .chapter_result_cache(&self.project_id, &self.chapter_id)?;
        // Refuse a scanner built before a dictionary/model/rule change. Its
        // engine still contains the old resource snapshot; letting it mutate
        // the chapter cache would clear or overwrite results generated by a
        // newer scanner generation.
        self.ensure_scanner_current()?;
        let scanner_fingerprint = &self.scanner_fingerprint;

        if matches!(request, CorrectionCheckRequest::RestoreOnly) {
            let guard = lock_slot(&slot)?;
            metrics.observe_cache(&guard);
            metrics.cache_hit = metrics.cache_valid_segments > 0;
            drop(guard);
            return self.restore_snapshot(&slot, text, scanner_fingerprint, metrics);
        }

        let mut guard = lock_slot(&slot)?;
        while guard.scanning {
            guard = slot
                .condvar
                .wait(guard)
                .map_err(|_| WriterError::Other("correction result cache lock poisoned".into()))?;
        }
        // Preferences or dictionaries may have changed while this request was
        // waiting behind another scan.
        self.ensure_scanner_current()?;
        if self.is_stale(&guard) {
            metrics.result = "stale";
            metrics.observe_cache(&guard);
            return self.empty_snapshot();
        }
        self.prepare_state(&mut guard, text, &request, scanner_fingerprint)?;
        metrics.observe_cache(&guard);
        let dirty = self.dirty_indices(&guard, &request);
        metrics.rescanned_segments = dirty.len();
        if dirty.is_empty() {
            metrics.cache_hit = metrics.cache_valid_segments > 0;
            guard.save()?;
            return self.assemble_snapshot(&guard);
        }

        // The model scan runs outside the cache lock. Competing edit and
        // chapter requests wait on the condvar; restore requests keep
        // reading the committed view.
        guard.scanning = true;
        let jobs = dirty
            .iter()
            .filter_map(|index| {
                guard.segments.get(*index).map(|segment| {
                    (
                        *index,
                        segment.start_byte as usize,
                        segment.end_byte as usize,
                    )
                })
            })
            .collect::<Vec<_>>();
        drop(guard);

        // Adjacent dirty segments share their ±context window. Scanning them
        // independently runs the same BERT window repeatedly; group ranges
        // whose expanded contexts overlap and attribute the single result
        // back to each segment below.
        let groups = group_dirty_jobs(text, &jobs);
        metrics.model_calls = if self.model_available {
            groups.len()
        } else {
            0
        };
        for group in &groups {
            let start = group.first().map(|job| job.1).unwrap_or(0);
            let end = group.last().map(|job| job.2).unwrap_or(start);
            let (context_start, context_end) = context_range(text, start, end);
            metrics.input_char_total = metrics
                .input_char_total
                .saturating_add(text[context_start..context_end].chars().count());
        }
        let scanned = groups
            .iter()
            .map(|group| {
                let start = group.first().map(|job| job.1).unwrap_or(0);
                let end = group.last().map(|job| job.2).unwrap_or(start);
                let result = self.scan_range(text, start, end);
                if let Ok(candidates) = &result {
                    metrics.candidates_produced =
                        metrics.candidates_produced.saturating_add(candidates.len());
                }
                result
            })
            .collect::<Result<Vec<_>, _>>();

        let mut guard = lock_slot(&slot)?;
        guard.scanning = false;
        slot.condvar.notify_all();
        let scanned = scanned?;
        // A lightweight edit transaction may have advanced the revision while
        // the model ran outside the lock. Those results belong to the old text
        // and must not be committed over the newer index.
        if guard.last_session_id != self.session_id
            || guard.last_revision != self.text_revision
            || guard.needs_reconcile
        {
            metrics.result = "stale";
            return self.empty_snapshot();
        }
        // Never label old-engine results with a newer generation: if the
        // model, dictionary, or disabled-rule set changed while the scan ran,
        // discard these results and let the caller retry on a fresh scanner.
        self.ensure_scanner_current()?;
        let mut changed = false;
        for (group, diagnostics) in groups.iter().zip(scanned) {
            for (index, start, end) in group {
                let suggestions = suggestions_in_segment(&diagnostics, *start, *end)?;
                metrics.candidates_attributed = metrics
                    .candidates_attributed
                    .saturating_add(suggestions.len());
                if let Some(segment) = guard.segments.get_mut(*index) {
                    if segment.checked.as_ref() != Some(&suggestions) {
                        changed = true;
                    }
                    segment.checked = Some(suggestions);
                    segment.dirty = false;
                }
            }
        }
        metrics.candidates_filtered_by_segment = metrics
            .candidates_produced
            .saturating_sub(metrics.candidates_attributed);
        // A rescan that produces the same suggestions and only shifted offsets
        // does not need to rewrite the chapter cache file.
        guard.semantic_dirty |= changed;
        guard.save()?;
        self.assemble_snapshot(&guard)
    }

    fn ensure_scanner_current(&self) -> Result<(), WriterError> {
        if self.store.scanner_fingerprint(&self.project_id)? != self.scanner_fingerprint {
            return Err(WriterError::Other(
                "correction scanner configuration changed; retry the request".into(),
            ));
        }
        Ok(())
    }

    fn restore_snapshot(
        &self,
        slot: &ChapterCorrectionSlot,
        text: &str,
        scanner_fingerprint: &str,
        metrics: &mut CorrectionScanMetrics,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        let mut guard = lock_slot(slot)?;
        if guard.scanning {
            metrics.observe_cache(&guard);
            metrics.cache_hit = metrics.cache_valid_segments > 0;
            // A scan has already prepared the newest text view. Restoring it
            // must never wait for the model, and an older revision must not be
            // presented with newer byte offsets.
            if guard.last_session_id == self.session_id && guard.last_revision == self.text_revision
            {
                return self.assemble_snapshot(&guard);
            }
            metrics.result = "stale";
            return self.empty_snapshot();
        }
        if self.is_stale(&guard) {
            metrics.observe_cache(&guard);
            metrics.result = "stale";
            return self.empty_snapshot();
        }
        self.prepare_state(
            &mut guard,
            text,
            &CorrectionCheckRequest::RestoreOnly,
            scanner_fingerprint,
        )?;
        metrics.observe_cache(&guard);
        metrics.cache_hit = metrics.cache_valid_segments > 0;
        // Restoring is a read-only display refresh: never save here, even when
        // the in-memory offset index had to be rebuilt.
        self.assemble_snapshot(&guard)
    }

    fn is_stale(&self, guard: &ChapterCorrectionCacheState) -> bool {
        // Editor sessions are allocated monotonically by the platform. A
        // request from an older session can still be queued while a reopened
        // document is already scanning; it must never replace the newer
        // session's cache. A freshly loaded cache has session 0, which accepts
        // the first request after an application restart.
        if guard.last_session_id != 0 && self.session_id < guard.last_session_id {
            return true;
        }
        guard.last_session_id == self.session_id && self.text_revision < guard.last_revision
    }

    fn prepare_state(
        &self,
        guard: &mut ChapterCorrectionCacheState,
        text: &str,
        request: &CorrectionCheckRequest,
        scanner_fingerprint: &str,
    ) -> Result<(), WriterError> {
        if guard.scanner_fingerprint != scanner_fingerprint {
            guard.segments.clear();
            guard.scanner_fingerprint = scanner_fingerprint.to_string();
            guard.needs_reconcile = true;
            guard.semantic_dirty = true;
            guard.last_session_id = 0;
            guard.last_revision = 0;
        }

        let same_session = guard.last_session_id == self.session_id;
        match request {
            CorrectionCheckRequest::Edits {
                base_revision,
                edits,
            } => {
                if same_session && guard.last_revision == self.text_revision {
                    // A restore may already have rebuilt the index for this
                    // revision. Mark only unseen segments touched by the real
                    // transaction; never rescan the whole chapter.
                    mark_unseen_segments_in_edit_windows(guard, edits);
                } else if same_session
                    && guard.last_revision == *base_revision
                    && !guard.needs_reconcile
                {
                    reconcile_edits(text, guard, edits)?;
                } else {
                    reconcile_full(text, guard);
                    // A lost edit notification or a new editor session is not
                    // the normal path. The rebuilt index covers the whole
                    // chapter, but this transaction may only run locally;
                    // unseen segments elsewhere wait for a manual chapter
                    // check.
                    mark_unseen_segments_in_edit_windows(guard, edits);
                }
            }
            CorrectionCheckRequest::Pending => {
                apply_pending_windows(
                    text,
                    guard,
                    !same_session || guard.last_revision != self.text_revision,
                )?;
            }
            CorrectionCheckRequest::RestoreOnly
            | CorrectionCheckRequest::Range(_, _)
            | CorrectionCheckRequest::All => {
                apply_pending_windows(
                    text,
                    guard,
                    !same_session || guard.last_revision != self.text_revision,
                )?;
            }
        }
        guard.last_session_id = self.session_id;
        guard.last_revision = self.text_revision;
        Ok(())
    }

    fn dirty_indices(
        &self,
        guard: &ChapterCorrectionCacheState,
        request: &CorrectionCheckRequest,
    ) -> Vec<usize> {
        match request {
            CorrectionCheckRequest::RestoreOnly => Vec::new(),
            CorrectionCheckRequest::Edits { .. } | CorrectionCheckRequest::Pending => guard
                .segments
                .iter()
                .enumerate()
                .filter(|(_, segment)| segment.dirty && segment.checked.is_none())
                .map(|(index, _)| index)
                .collect(),
            CorrectionCheckRequest::All => guard
                .segments
                .iter()
                .enumerate()
                .filter(|(_, segment)| segment.checked.is_none())
                .map(|(index, _)| index)
                .collect(),
            CorrectionCheckRequest::Range(range_start, range_end) => guard
                .segments
                .iter()
                .enumerate()
                .filter(|(_, segment)| {
                    segment.checked.is_none()
                        && (segment.start_byte as usize) < *range_end
                        && *range_start < segment.end_byte as usize
                })
                .map(|(index, _)| index)
                .collect(),
        }
    }

    /// Cache-miss compute entry: scans a merged dirty range with its context.
    /// Returned offsets are absolute in `text`; callers split them per segment.
    fn scan_range(
        &self,
        text: &str,
        start: usize,
        end: usize,
    ) -> Result<Vec<CorrectionDiagnostic>, WriterError> {
        let (context_start, context_end) = context_range(text, start, end);
        self.engine
            .scan(&text[context_start..context_end])
            .map(|diagnostics| {
                diagnostics
                    .into_iter()
                    .map(|mut diagnostic| {
                        diagnostic.start_byte += context_start;
                        diagnostic.end_byte += context_start;
                        diagnostic
                    })
                    .collect()
            })
            .map_err(|error| WriterError::Other(error.to_string()))
    }

    fn assemble_snapshot(
        &self,
        guard: &ChapterCorrectionCacheState,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        let mut diagnostics = Vec::new();
        let mut checked_ranges = Vec::new();
        for segment in &guard.segments {
            let Some(suggestions) = &segment.checked else {
                continue;
            };
            checked_ranges.push(CorrectionCheckedRangeDto {
                start_byte: segment.start_byte,
                end_byte: segment.end_byte,
            });
            for suggestion in suggestions {
                let (start_byte, end_byte) =
                    absolute_suggestion_range(segment.start_byte, suggestion)?;
                diagnostics.push(CorrectionDiagnostic {
                    start_byte,
                    end_byte,
                    original: suggestion.original.clone(),
                    replacement: suggestion.replacement.clone(),
                    source: suggestion.source.clone(),
                    rule_id: suggestion.rule_id.clone(),
                    severity: suggestion.severity.clone(),
                    confidence: None,
                });
            }
        }
        checked_ranges.sort_by_key(|range| (range.start_byte, range.end_byte));
        checked_ranges.dedup();

        // Ignore choices are applied on the response so changing them never
        // requires rescanning the chapter.
        let candidates_before_ignore = diagnostics.len();
        let filtered = self.store.filter_ignored(
            &self.project_id,
            &self.chapter_id,
            self.text_revision,
            diagnostics,
        )?;
        let filtered_by_user_ignore = candidates_before_ignore.saturating_sub(filtered.len());
        record_correction_scan_event(
            "correction.candidates.filtered",
            vec![
                ("session_id", self.session_id.to_string()),
                ("text_revision", self.text_revision.to_string()),
                ("chapter_digest", content_fingerprint(&self.chapter_id)),
                ("filter_reason", "user_ignore".to_string()),
                (
                    "candidates_before_filter",
                    candidates_before_ignore.to_string(),
                ),
                ("candidates_filtered", filtered_by_user_ignore.to_string()),
                ("candidates_after_filter", filtered.len().to_string()),
            ],
        );
        let mut suggestions = filtered
            .into_iter()
            .map(|diagnostic| CorrectionSuggestionDto::try_from((diagnostic, self.text_revision)))
            .collect::<Result<Vec<_>, _>>()?;
        suggestions.sort_by_key(|suggestion| (suggestion.start_byte, suggestion.end_byte));
        Ok(CorrectionChapterSnapshotDto {
            text_revision: self.text_revision,
            suggestions,
            checked_ranges,
        })
    }

    fn empty_snapshot(&self) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        Ok(CorrectionChapterSnapshotDto {
            text_revision: self.text_revision,
            suggestions: Vec::new(),
            checked_ranges: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_suggestion_offsets_are_both_relative_to_the_segment_start() {
        let suggestion = CachedSuggestion {
            start_byte: 3,
            end_byte: 8,
            original: "甲乙".to_string(),
            replacement: "丙丁".to_string(),
            source: "test".to_string(),
            rule_id: "test".to_string(),
            severity: "warning".to_string(),
        };
        assert_eq!(
            absolute_suggestion_range(100, &suggestion).expect("range"),
            (103, 108)
        );
    }
}
