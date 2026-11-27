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
    mark_unseen_segments_in_edit_windows, reconcile_edits, reconcile_full, suggestions_match,
    CachedSuggestion, ChapterCorrectionCacheState, ChapterCorrectionSlot, EditRange,
};
use super::scan_metrics::{record_correction_scan_event, CorrectionScanMetrics};
use super::synced_results::{self, SyncedCorrectionRecord};
use super::{CorrectionStore, ModelCacheKey, ModelLoadSlot};

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
    /// `None` for a restore-only scanner: reading the committed chapter cache
    /// must never open the ONNX session or build an engine.
    engine: Option<Arc<CorrectionEngine>>,
    /// Exact model identity this scanner was built from. Activity refreshes use
    /// this key so scanning model A never extends model B's idle deadline.
    model_key: Option<ModelCacheKey>,
    /// Shared slot for `model_key`; kept to avoid a global cache lock per
    /// inference window when refreshing the idle deadline.
    model_slot: Option<Arc<ModelLoadSlot>>,
    model_available: bool,
    /// Fingerprint of the exact lexicon/model/dictionary snapshot captured
    /// when this scanner was built. It must travel with the engine so a
    /// preference change between construction and `check` cannot label old
    /// results with a newer scanner generation.
    scanner_fingerprint: String,
    /// 跨设备稳定身份 + 本次扫描实际使用的模式，写入作品级同步记录。
    scanner_stable: String,
    record_mode: &'static str,
    model_identity: String,
    lexicon_identity: String,
    rules_identity: String,
    /// 本章当前同步结果不可变索引，构造 scanner 时在章节 slot 锁外加载。
    sync_index: Arc<synced_results::SyncedResultIndex>,
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

/// 一次扫描从模型执行一直到 pending durable / checked 投影终态都持有
/// `scanning` 所有权。释放 slot 锁写待发布文件时仍保留 lease，其它扫描在
/// condvar 上等待；所有成功、失败、stale 出口都统一释放并唤醒。
struct ScanLease<'a> {
    slot: &'a ChapterCorrectionSlot,
    active: bool,
}

impl<'a> ScanLease<'a> {
    fn new(slot: &'a ChapterCorrectionSlot) -> Self {
        Self { slot, active: true }
    }

    fn release(&mut self, guard: &mut ChapterCorrectionCacheState) {
        if !self.active {
            return;
        }
        guard.scanning = false;
        self.slot.condvar.notify_all();
        self.active = false;
    }

    fn finish(&mut self) -> Result<(), WriterError> {
        if !self.active {
            return Ok(());
        }
        let mut guard = lock_slot(self.slot)?;
        self.release(&mut guard);
        Ok(())
    }
}

impl Drop for ScanLease<'_> {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Ok(mut guard) = lock_slot(self.slot) {
            self.release(&mut guard);
        }
    }
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
        // 待发布事务独立于模型/身份准备：即使后续加载失败也先重放。
        store.retry_publications(&project_id, &chapter_id);
        // Bracket resource loading with the fingerprint so the captured
        // generation always describes the exact lexicon/model the engine was
        // built from, even if a dictionary or settings change races setup.
        let identity_before = store.scanner_identity(&project_id)?;
        let scanner_fingerprint_before = identity_before.fingerprint.clone();
        let (lexicon_version, lexicon) = store.lexicon_snapshot_for_project(&project_id)?;
        let model_load_started_at = Instant::now();
        let model_snapshot = match store.configured_model_snapshot() {
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
        let model_available = model_snapshot.model.is_some();
        // Issue #1061: a cache hit must not look like a 500 ms session
        // initialization. Log the single-flight outcome, whether a real ONNX
        // session was created, and how many sessions this process holds.
        let model_lifetime = super::model_lifetime_metrics();
        record_correction_scan_event(
            "correction.model.load",
            vec![
                ("session_id", session_id.to_string()),
                ("text_revision", text_revision.to_string()),
                ("chapter_digest", content_fingerprint(&chapter_id)),
                ("result", "success".to_string()),
                ("model_available", model_available.to_string()),
                ("model_cache_hit", model_snapshot.cache_hit.to_string()),
                (
                    "session_initialized",
                    model_snapshot.session_initialized.to_string(),
                ),
                (
                    "model_load_ms",
                    model_snapshot
                        .load_ms
                        .max(model_load_started_at.elapsed().as_millis())
                        .to_string(),
                ),
                ("model_instances", model_lifetime.cached.to_string()),
                ("model_engine_refs", model_lifetime.engine_refs.to_string()),
                (
                    "model_retired_models",
                    model_lifetime.retired_models.to_string(),
                ),
                (
                    "model_retired_engines",
                    model_lifetime.retired_engines.to_string(),
                ),
                ("model_generation", model_lifetime.generation.to_string()),
                ("resource_fingerprint", scanner_fingerprint_before.clone()),
            ],
        );
        let model_key = model_snapshot.key.clone();
        let model_slot = model_key
            .as_ref()
            .and_then(CorrectionStore::model_slot_for_key);
        let engine = if let Some(engine) =
            store.cached_engine(&project_id, lexicon_version, model_key.as_ref())?
        {
            engine
        } else {
            let rules = RuleSet::with_common_rules();
            let config = CorrectionConfig::default();
            let engine = if let Some(model) = model_snapshot.model {
                let predictor: Arc<dyn NeuralPredictor> = model;
                CorrectionEngine::new_shared(predictor, lexicon, rules, config)
            } else {
                CorrectionEngine::rules_only_shared(lexicon, rules, config)
            }
            .map_err(|error| WriterError::Other(error.to_string()))?;
            store.cache_engine_if_model_current(
                &project_id,
                lexicon_version,
                model_snapshot.key,
                Arc::new(engine),
            )?
        };
        let identity_after = store.scanner_identity(&project_id)?;
        if identity_after.fingerprint != identity_before.fingerprint {
            return Err(WriterError::Other(
                "correction scanner configuration changed during setup; retry the request".into(),
            ));
        }
        // `mode="model"` 当且仅当本次扫描实际构建/使用了模型引擎；纯规则扫描
        // 必须写成 `rules`，不能冒用模型结果。
        let record_mode = if model_available {
            synced_results::MODE_MODEL
        } else {
            synced_results::MODE_RULES
        };
        let sync_index = store.synced_records_for_chapter(
            &project_id,
            &chapter_id,
            record_mode,
            &identity_before.scanner_stable,
        )?;
        Ok(Self {
            store,
            project_id,
            chapter_id,
            session_id,
            text_revision,
            engine: Some(engine),
            model_key,
            model_slot,
            model_available,
            scanner_fingerprint: identity_before.fingerprint,
            scanner_stable: identity_before.scanner_stable,
            record_mode,
            model_identity: identity_before.model_identity,
            lexicon_identity: identity_before.lexicon_identity,
            rules_identity: identity_before.rules_identity,
            sync_index,
        })
    }

    /// Build a scanner that may only restore the committed chapter cache.
    ///
    /// Chapter open and read-only display refreshes use this path. It computes
    /// the same resource fingerprint as [`Self::new`] so restored results are
    /// verified against the current model/dictionary generation, but it never
    /// resolves the configured model, opens an ONNX session, or builds a
    /// lexicon. An empty chapter therefore costs one cache lookup, not a
    /// 500–800 ms model initialization.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn restore_only(
        store: CorrectionStore,
        project_id: String,
        chapter_id: String,
        session_id: u64,
        text_revision: u64,
    ) -> Result<Self, WriterError> {
        store.retry_publications(&project_id, &chapter_id);
        let identity = store.scanner_identity(&project_id)?;
        let record_mode = identity.configured_mode;
        let sync_index = store.synced_records_for_chapter(
            &project_id,
            &chapter_id,
            record_mode,
            &identity.scanner_stable,
        )?;
        Ok(Self {
            store,
            project_id,
            chapter_id,
            session_id,
            text_revision,
            engine: None,
            model_key: None,
            model_slot: None,
            model_available: false,
            scanner_fingerprint: identity.fingerprint,
            scanner_stable: identity.scanner_stable,
            // Restore-only 不构建引擎；恢复记录按当前配置模式读取（有模型配置
            // 读 model 记录，否则读 rules 记录）。
            record_mode,
            model_identity: identity.model_identity,
            lexicon_identity: identity.lexicon_identity,
            rules_identity: identity.rules_identity,
            sync_index,
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
        let restore_only = matches!(request, CorrectionCheckRequest::RestoreOnly);
        let mut metrics = CorrectionScanMetrics::new(
            self.session_id,
            self.text_revision,
            &self.chapter_id,
            request_kind,
            self.model_available,
            self.engine.is_some(),
            self.scanner_fingerprint.clone(),
        );
        metrics.result = "running";
        record_correction_scan_event("correction.scan.start", metrics.fields());

        let result = self.check_inner(text, request, &mut metrics);
        if metrics.result == "running" {
            // Issue #1062 三.3：restore-only 只读缓存，绝不能记成一次完成的扫描
            // （`success`），否则诊断会把未扫描的脏区读成已确认。
            metrics.result = match (restore_only, result.is_ok()) {
                (true, true) => "restore_only",
                (_, true) => "success",
                (_, false) => "error",
            };
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
        if self.engine.is_none() {
            return Err(WriterError::Other(
                "correction scanner was created for restore-only use".into(),
            ));
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
            return self.unfinished_snapshot();
        }
        self.prepare_state(&mut guard, text, &request, scanner_fingerprint)?;
        metrics.observe_cache(&guard);
        let dirty = self.dirty_indices(&guard, &request);
        metrics.rescanned_segments = dirty.len();
        if dirty.is_empty() {
            metrics.cache_hit = metrics.cache_valid_segments > 0;
            let snapshot = self.assemble_snapshot(&guard)?;
            guard.save()?;
            drop(guard);
            return Ok(snapshot);
        }

        // The model scan runs outside the cache lock. Competing edit and
        // chapter requests wait on the condvar; restore requests keep
        // reading the committed view.
        guard.scanning = true;
        let mut scan_lease = ScanLease::new(&slot);
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
        // Issue #1061: run one model window at a time and stop at a window
        // boundary when the chapter's visible revision already moved. The
        // partial results belong to the old text, so they are discarded; the
        // unscanned dirty segments keep their `dirty` flag for the next
        // pending request instead of being cleared or rescanned as "clean".
        let mut scanned = Vec::with_capacity(groups.len());
        let mut aborted_at_window: Option<usize> = None;
        let mut scan_error: Option<WriterError> = None;
        for group in &groups {
            if self.revision_advanced_since_start()? {
                aborted_at_window = Some(scanned.len());
                break;
            }
            let start = group.first().map(|job| job.1).unwrap_or(0);
            let end = group.last().map(|job| job.2).unwrap_or(start);
            let window_started = Instant::now();
            let window_result = self.scan_range(text, start, end);
            if let (Some(key), Some(slot)) = (self.model_key.as_ref(), self.model_slot.as_ref()) {
                self.store.note_model_activity(slot, key);
            }
            metrics.model_duration_ms = metrics
                .model_duration_ms
                .saturating_add(window_started.elapsed().as_millis());
            match window_result {
                Ok(candidates) => {
                    metrics.candidates_produced =
                        metrics.candidates_produced.saturating_add(candidates.len());
                    scanned.push(candidates);
                }
                Err(error) => {
                    scan_error = Some(error);
                    break;
                }
            }
        }

        let mut guard = lock_slot(&slot)?;
        if let Some(window_index) = aborted_at_window {
            scan_lease.release(&mut guard);
            metrics.result = "stale";
            metrics.dirty_preserved_segments = dirty.len();
            metrics.aborted_at_window = window_index;
            return self.unfinished_snapshot();
        }
        if let Some(error) = scan_error {
            scan_lease.release(&mut guard);
            metrics.dirty_preserved_segments = dirty.len();
            return Err(error);
        }
        // A lightweight edit transaction may have advanced the revision while
        // the model ran outside the lock. Those results belong to the old text
        // and must not be committed over the newer index.
        if guard.last_session_id != self.session_id
            || guard.last_revision != self.text_revision
            || guard.needs_reconcile
        {
            scan_lease.release(&mut guard);
            metrics.result = "stale";
            return self.unfinished_snapshot();
        }
        // Never label old-engine results with a newer generation: if the
        // model, dictionary, or disabled-rule set changed while the scan ran,
        // discard these results and let the caller retry on a fresh scanner.
        if let Err(error) = self.ensure_scanner_current() {
            scan_lease.release(&mut guard);
            return Err(error);
        }
        // 先冻结本次模型/规则结果，不提前修改 segment.checked：待发布事实
        // durable 之前缓存仍保持“未确认”，崩溃或写失败时不会伪装成已完成。
        let mut changed = false;
        let mut frozen: Vec<(usize, Vec<CachedSuggestion>, Option<SyncedCorrectionRecord>)> =
            Vec::new();
        for (group, diagnostics) in groups.iter().zip(scanned) {
            for (index, start, end) in group {
                let suggestions = suggestions_in_segment(&diagnostics, *start, *end)?;
                metrics.candidates_attributed = metrics
                    .candidates_attributed
                    .saturating_add(suggestions.len());
                if guard
                    .segments
                    .get(*index)
                    .is_some_and(|segment| segment.checked.as_ref() != Some(&suggestions))
                {
                    changed = true;
                }
                // 空 suggestions 也要写：它表示「已检查无建议」。
                let record = self.synced_record_for_suggestions(&guard, *index, text, &suggestions);
                frozen.push((*index, suggestions, record));
            }
        }
        metrics.candidates_filtered_by_segment = metrics
            .candidates_produced
            .saturating_sub(metrics.candidates_attributed);
        let synced_records: Vec<SyncedCorrectionRecord> = frozen
            .iter()
            .filter_map(|(_, _, record)| record.clone())
            .collect();
        if synced_records.is_empty() {
            // 没有可发布记录（例如 segment 已越界）时可直接投影；这不会留下
            // 未完成的同步事务。
            for (index, suggestions, record) in &frozen {
                let Some(record) = record.as_ref() else {
                    continue;
                };
                if let Some(segment) = guard.segments.get_mut(*index) {
                    segment.checked = Some(suggestions.clone());
                    segment.dirty = false;
                    segment.synced_record_key = Some(record.record_key());
                }
            }
            guard.semantic_dirty |= changed;
            if changed {
                guard.save()?;
            }
            let snapshot = self.assemble_snapshot(&guard)?;
            scan_lease.release(&mut guard);
            drop(guard);
            return Ok(snapshot);
        }
        drop(guard);
        // 在章节 slot 锁外先写独立待发布事实；失败保留内存待办并返回明确的
        // unfinished 快照，绝不把未持久化结果投影成已完成。
        let publication_durable = self
            .store
            .enqueue_publication(&self.project_id, &self.chapter_id, &synced_records);
        if !publication_durable {
            log::warn!(
                "[correction] publication enqueue incomplete chapter_digest={}",
                content_fingerprint(&self.chapter_id)
            );
            metrics.result = "publication-pending";
            metrics.dirty_preserved_segments = dirty.len();
            // 仍持有扫描 token 完成第一次重放：若重放成功，等待中的下一个
            // 请求能从同步索引直接投影，不必为同一正文再跑模型。
            self.store.retry_publications(&self.project_id, &self.chapter_id);
            let _ = scan_lease.finish();
            return self.unfinished_snapshot();
        }
        let mut guard = lock_slot(&slot)?;
        let scanner_current = self.ensure_scanner_current().is_ok();
        if guard.last_session_id != self.session_id
            || guard.last_revision != self.text_revision
            || guard.needs_reconcile
            || !scanner_current
        {
            scan_lease.release(&mut guard);
            drop(guard);
            metrics.result = "stale";
            metrics.dirty_preserved_segments = dirty.len();
            // payload 已 durable；立即尝试发布，下一次 restore 可直接从索引
            // 投影，不为同一正文重跑模型。
            self.store.retry_publications(&self.project_id, &self.chapter_id);
            return self.unfinished_snapshot();
        }
        // durable 后重新核对 session/revision/scanner 才允许投影。冻结结果
        // 到这里才写入 checked/dirty/record_key。
        for (index, suggestions, record) in frozen {
            let Some(record) = record else {
                continue;
            };
            if let Some(segment) = guard.segments.get_mut(index) {
                segment.checked = Some(suggestions);
                segment.dirty = false;
                segment.synced_record_key = Some(record.record_key());
            }
        }
        guard.semantic_dirty = true;
        guard.save()?;
        let snapshot = self.assemble_snapshot(&guard)?;
        scan_lease.release(&mut guard);
        drop(guard);
        self.store.retry_publications(&self.project_id, &self.chapter_id);
        Ok(snapshot)
    }

    /// True when another edit/session already moved this chapter past the
    /// scanner's text. Checked between model windows, never inside one.
    fn revision_advanced_since_start(&self) -> Result<bool, WriterError> {
        let Some((session_id, text_revision)) = self
            .store
            .chapter_revision_progress(&self.project_id, &self.chapter_id)?
        else {
            return Ok(false);
        };
        Ok(session_id > self.session_id
            || (session_id == self.session_id && text_revision > self.text_revision))
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
            return self.unfinished_snapshot();
        }
        if self.is_stale(&guard) {
            metrics.observe_cache(&guard);
            metrics.result = "stale";
            return self.unfinished_snapshot();
        }
        let missing_synced_keys = guard
            .segments
            .iter()
            .any(|segment| segment.checked.is_some() && segment.synced_record_key.is_none());
        let had_legacy_pending = guard.legacy_publication_pending;
        self.prepare_state(
            &mut guard,
            text,
            &CorrectionCheckRequest::RestoreOnly,
            scanner_fingerprint,
        )?;
        metrics.observe_cache(&guard);
        metrics.cache_hit = metrics.cache_valid_segments > 0;
        // 旧版本 cache 的 publicationPending 只在当前 checked 内容仍可验证时
        // 一次性迁移到独立 publication 文件；此后 cache 不再承担待发布事务。
        let legacy_records: Vec<SyncedCorrectionRecord> =
            if had_legacy_pending || missing_synced_keys {
                // 旧 cache 已有 checked 但没有同步记录键：按当前仍可验证的
                // segment 内容构造一次强摘要记录，交给独立 publication 迁移。
                guard
                    .segments
                    .iter()
                    .enumerate()
                    .filter_map(|(index, _)| {
                        self.synced_record_for_segment(&guard, index, text)
                    })
                    .collect()
            } else {
                Vec::new()
            };
        // 先保留旧标记与已投影的新 key：待发布 durable 之前不能清迁移材料。
        let snapshot = self.assemble_snapshot(&guard)?;
        if legacy_records.is_empty() {
            guard.legacy_publication_pending = false;
            guard.semantic_dirty |= had_legacy_pending;
            drop(guard);
            let mut save_guard = lock_slot(slot)?;
            if let Err(error) = save_guard.save() {
                log::warn!(
                    "[correction] restore projection cache save failed chapter_digest={} error={}",
                    content_fingerprint(&self.chapter_id),
                    error
                );
            }
            return Ok(snapshot);
        }
        drop(guard);
        let durable = self
            .store
            .enqueue_publication(&self.project_id, &self.chapter_id, &legacy_records);
        if !durable {
            log::warn!(
                "[correction] legacy pending migration enqueue incomplete chapter_digest={}",
                content_fingerprint(&self.chapter_id)
            );
            metrics.result = "publication-pending";
            self.store.retry_publications(&self.project_id, &self.chapter_id);
            return self.unfinished_snapshot();
        }
        self.store.retry_publications(&self.project_id, &self.chapter_id);
        let mut guard = lock_slot(slot)?;
        // durable 后重新核对 session/revision/scanner；期间有编辑/新扫描时
        // 不能把旧投影写进新 cache。
        if guard.last_session_id != self.session_id
            || guard.last_revision != self.text_revision
            || guard.needs_reconcile
            || self.is_stale(&guard)
        {
            drop(guard);
            metrics.result = "stale";
            return self.unfinished_snapshot();
        }
        guard.legacy_publication_pending = false;
        guard.semantic_dirty = true;
        if let Err(error) = guard.save() {
            log::warn!(
                "[correction] restore projection cache save failed chapter_digest={} error={}",
                content_fingerprint(&self.chapter_id),
                error
            );
        }
        drop(guard);
        Ok(snapshot)
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
            guard.checked_mode = self.record_mode.to_string();
            guard.legacy_publication_pending = false;
            guard.needs_reconcile = true;
            guard.semantic_dirty = true;
            guard.last_session_id = 0;
            guard.last_revision = 0;
        }
        // 规则降级/推理失败不能确认模型覆盖：目标模式变化时清空 checked、
        // 标记 dirty，让下一次真实扫描重新建立覆盖；旧缓存未知模式也不会
        // 被继承为模型覆盖。
        if guard.checked_mode != self.record_mode {
            for segment in &mut guard.segments {
                segment.checked = None;
                segment.dirty = true;
            }
            guard.checked_mode = self.record_mode.to_string();
            guard.legacy_publication_pending = false;
            guard.needs_reconcile = true;
            guard.semantic_dirty = true;
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
        // Issue #1064：在窗口重建/失效之后回填作品级校对结果。只填
        // `checked.is_none()` 且 text/context 指纹都匹配的 segment；不启动
        // 模型、不改正文，也不全量重写本地 cache。编辑事务的逐键映射不读
        // 同步目录（每笔都读目录会把按键路径变成 I/O 热路径）。
        if matches!(
            request,
            CorrectionCheckRequest::RestoreOnly
                | CorrectionCheckRequest::All
                | CorrectionCheckRequest::Pending
        ) {
            // Pending 是周期/手动的整章增量检查：先恢复可用同步覆盖，再判断
            // 是否真的有未检查段需要初始化模型；Edits 热路径仍不读同步目录。
            self.restore_synced_records(guard, text);
        }
        guard.last_session_id = self.session_id;
        guard.last_revision = self.text_revision;
        Ok(())
    }

    /// 用同步结果索引投影段：补未检查段，并对已有 checked 段重新选择
    /// 确定性赢家（同输入身份的不同 payload 按 record_key 字典序取最小）。
    /// 索引已在 scanner 构造时（章节 slot 锁外）构建，本函数不读盘、不复制
    /// 历史建议全集，只取当前内容键的候选。
    fn restore_synced_records(&self, guard: &mut ChapterCorrectionCacheState, text: &str) {
        let has_missing_keys = guard
            .segments
            .iter()
            .any(|segment| segment.checked.is_some() && segment.synced_record_key.is_none());
        if self.sync_index.is_empty() && !has_missing_keys {
            return;
        }
        let mut changed = false;
        for index in 0..guard.segments.len() {
            let (start, end, current_checked, current_key) = {
                let segment = &guard.segments[index];
                (
                    segment.start_byte as usize,
                    segment.end_byte as usize,
                    segment.checked.clone(),
                    segment.synced_record_key.clone(),
                )
            };
            if start >= end || end > text.len() {
                continue;
            }
            let (context_start, context_end) = context_range(text, start, end);
            let Some(segment_text) = text.get(start..end) else {
                continue;
            };
            let Some(context_text) = text.get(context_start..context_end) else {
                continue;
            };
            let candidates = self.sync_index.candidates(
                &synced_results::text_sha256(segment_text),
                &synced_results::text_sha256(context_text),
            );
            // 本地已检查结果的记录键也参与确定性比较；旧缓存缺键时按当前
            // segment 内容重建强摘要键，保留刚持久化的 key，只有真实失效才清。
            let local_candidate = match (&current_checked, &current_key) {
                (Some(suggestions), Some(key)) => Some((key.clone(), suggestions.clone())),
                (Some(suggestions), None) => self
                    .synced_record_for_segment(guard, index, text)
                    .map(|record| (record.record_key(), suggestions.clone())),
                _ => None,
            };
            if candidates.is_none() {
                let Some((key, suggestions)) = local_candidate else {
                    continue;
                };
                if !suggestions_match(text, start, end, &suggestions) {
                    if let Some(segment) = guard.segments.get_mut(index) {
                        segment.checked = None;
                        segment.dirty = true;
                        segment.synced_record_key = None;
                        changed = true;
                    }
                    continue;
                }
                if current_key.is_none() {
                    if let Some(segment) = guard.segments.get_mut(index) {
                        segment.synced_record_key = Some(key);
                        changed = true;
                    }
                }
                continue;
            }
            let mut winner: Option<(String, Vec<CachedSuggestion>)> = local_candidate
                .filter(|(_, suggestions)| suggestions_match(text, start, end, suggestions));
            // 本地 checked 与当前 segment 内容不再匹配时必须显式失效；否则
            // 会一直保留旧 key/建议，永远等不到有效的同步赢家。
            let local_invalid = current_checked.is_some()
                && !winner.as_ref().is_some_and(|(_, suggestions)| {
                    suggestions_match(text, start, end, suggestions)
                });
            for (key, suggestions) in candidates.unwrap_or(&[]) {
                if !suggestions_match(text, start, end, suggestions) {
                    continue;
                }
                if winner
                    .as_ref()
                    .map_or(true, |(winner_key, _)| key < winner_key)
                {
                    winner = Some((key.clone(), suggestions.clone()));
                }
            }
            let Some((winner_key, suggestions)) = winner else {
                if local_invalid {
                    if let Some(segment) = guard.segments.get_mut(index) {
                        segment.checked = None;
                        segment.dirty = true;
                        segment.synced_record_key = None;
                        changed = true;
                    }
                }
                continue;
            };
            let same = guard.segments.get(index).is_some_and(|segment| {
                segment.synced_record_key.as_deref() == Some(winner_key.as_str())
                    && segment.checked.as_ref() == Some(&suggestions)
            });
            if same {
                continue;
            }
            if let Some(segment) = guard.segments.get_mut(index) {
                segment.checked = Some(suggestions);
                segment.dirty = false;
                segment.synced_record_key = Some(winner_key);
                changed = true;
            }
        }
        // 投影变化必须写回持久 cache，重启后仍保持同一确定性赢家。
        guard.semantic_dirty |= changed;
    }

    /// 为本次成功写入的 segment 构造同步记录（只含派生结果，不含正文）。
    fn synced_record_for_segment(
        &self,
        guard: &ChapterCorrectionCacheState,
        index: usize,
        text: &str,
    ) -> Option<SyncedCorrectionRecord> {
        let suggestions = guard.segments.get(index)?.checked.clone()?;
        self.synced_record_for_suggestions(guard, index, text, &suggestions)
    }

    /// 冻结扫描结果时使用：不要求 segment.checked 已经投影，先按候选结果
    /// 构造记录，待待发布 durable 后再回写缓存。
    fn synced_record_for_suggestions(
        &self,
        guard: &ChapterCorrectionCacheState,
        index: usize,
        text: &str,
        suggestions: &[CachedSuggestion],
    ) -> Option<SyncedCorrectionRecord> {
        let segment = guard.segments.get(index)?;
        let start = segment.start_byte as usize;
        let end = segment.end_byte as usize;
        let segment_text = text.get(start..end)?;
        let (context_start, context_end) = context_range(text, start, end);
        let context_text = text.get(context_start..context_end)?;
        if !suggestions_match(text, start, end, suggestions) {
            return None;
        }
        Some(SyncedCorrectionRecord {
            schema_version: synced_results::SCHEMA_VERSION,
            project_id: self.project_id.clone(),
            chapter_id: self.chapter_id.clone(),
            mode: self.record_mode.to_string(),
            scanner_stable: self.scanner_stable.clone(),
            model_identity: self.model_identity.clone(),
            lexicon_identity: self.lexicon_identity.clone(),
            rules_identity: self.rules_identity.clone(),
            text_sha256: synced_results::text_sha256(segment_text),
            context_sha256: synced_results::text_sha256(context_text),
            suggestions: suggestions.to_vec(),
            checked: true,
        })
    }

    fn dirty_indices(
        &self,
        guard: &ChapterCorrectionCacheState,
        request: &CorrectionCheckRequest,
    ) -> Vec<usize> {
        match request {
            CorrectionCheckRequest::RestoreOnly => Vec::new(),
            // A segment stays `dirty` until a scan replaces its cached result.
            // Filtering by `checked.is_none()` would leave a dirty segment with
            // an older cached value unscanned forever, so pending/edits always
            // pick up every dirty segment.
            CorrectionCheckRequest::Edits { .. } | CorrectionCheckRequest::Pending => guard
                .segments
                .iter()
                .enumerate()
                // 未验证段（冷启动/整章重建）也必须进模型；只按 dirty 过滤会
                // 让初次 Pending 扫描永远找不到工作。
                .filter(|(_, segment)| segment.dirty || segment.checked.is_none())
                .map(|(index, _)| index)
                .collect(),
            CorrectionCheckRequest::All => guard
                .segments
                .iter()
                .enumerate()
                .filter(|(_, segment)| segment.dirty || segment.checked.is_none())
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
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| WriterError::Other("correction engine is not available".into()))?;
        let (context_start, context_end) = context_range(text, start, end);
        engine
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
        let has_unchecked_segments = guard
            .segments
            .iter()
            .any(|segment| segment.dirty || segment.checked.is_none());
        let mut suggestions = filtered
            .into_iter()
            .map(|diagnostic| CorrectionSuggestionDto::try_from((diagnostic, self.text_revision)))
            .collect::<Result<Vec<_>, _>>()?;
        suggestions.sort_by_key(|suggestion| (suggestion.start_byte, suggestion.end_byte));
        Ok(CorrectionChapterSnapshotDto {
            text_revision: self.text_revision,
            suggestions,
            checked_ranges,
            has_unchecked_segments,
        })
    }

    /// Snapshot for a request that could not verify the current revision
    /// (stale scanner, scan aborted at a window boundary, or a restore that
    /// raced a newer scan). It carries no suggestions, but it must never claim
    /// the revision is fully checked: the dirty regions are still pending, and
    /// `has_unchecked_segments` is what tells the editor to request the model
    /// scan again (issue #1062 三.3).
    fn unfinished_snapshot(&self) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        Ok(CorrectionChapterSnapshotDto {
            text_revision: self.text_revision,
            suggestions: Vec::new(),
            checked_ranges: Vec::new(),
            has_unchecked_segments: true,
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

    /// Issue #1062 三.3：restore-only 只回放缓存，任何一次都不能把该 revision
    /// 报告成“已扫描”。未检查的段必须继续以 `has_unchecked_segments` 暴露，
    /// 否则编辑器会把恢复显示当成一次完成的扫描。
    #[test]
    fn restore_only_snapshot_never_claims_the_revision_is_checked() {
        let root = std::env::temp_dir().join(format!(
            "sujian-correction-restore-only-{}",
            std::process::id()
        ));
        let store = CorrectionStore::open(&root).expect("correction store");
        let text = "第一句。第二句。第三句。";
        for _ in 0..2 {
            let scanner = CorrectionScanner::restore_only(
                store.clone(),
                "project".to_string(),
                "chapter".to_string(),
                7,
                11,
            )
            .expect("restore-only scanner");
            let snapshot = scanner.chapter_snapshot(text).expect("restore snapshot");
            assert_eq!(snapshot.text_revision, 11);
            assert!(
                snapshot.suggestions.is_empty() && snapshot.checked_ranges.is_empty(),
                "restore-only must never present cached results as a scan"
            );
            assert!(
                snapshot.has_unchecked_segments,
                "restore-only must keep the revision pending instead of confirming a scan"
            );
        }
    }
}
