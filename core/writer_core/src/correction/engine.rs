use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use sujian_proof::{
    CorrectionConfig, CorrectionDiagnostic, CorrectionEngine, LanguageScorer, RuleSet,
};

use crate::api::{
    CorrectionChapterSnapshotDto, CorrectionCheckedRangeDto, CorrectionSuggestionDto, WriterError,
};

use super::result_cache::{
    canonical_segments, content_fingerprint, context_range, CachedSegment, CachedSuggestion,
    ChapterCorrectionCacheState,
};
use super::CorrectionStore;

pub(crate) struct CorrectionScanner {
    store: CorrectionStore,
    project_id: String,
    chapter_id: String,
    text_revision: u64,
    engine: Arc<CorrectionEngine>,
}

/// Which invalid segments a check request recomputes.
enum ScanSelection {
    /// Restore only: verify the cache but never run the engine.
    RestoreOnly,
    /// Recompute every missing or invalid segment.
    All,
    /// Recompute only segments overlapping the requested byte range.
    Range(usize, usize),
}

impl ScanSelection {
    fn covers(&self, start: usize, end: usize) -> bool {
        match self {
            ScanSelection::RestoreOnly => false,
            ScanSelection::All => true,
            ScanSelection::Range(range_start, range_end) => {
                start < *range_end && *range_start < end
            }
        }
    }
}

/// One canonical segment of the current text plus its cache lookup result.
struct SegmentPlan {
    start: usize,
    end: usize,
    context_start: usize,
    context_end: usize,
    text_fingerprint: String,
    context_fingerprint: String,
    /// `Some` once the segment is checked for the current text, by a valid
    /// cache hit or by a fresh scan. `None` keeps it unchecked.
    checked: Option<Vec<CachedSuggestion>>,
    /// True while `checked` came from the persistent cache unchanged.
    cached_hit: bool,
}

fn to_u32(value: usize) -> Result<u32, WriterError> {
    u32::try_from(value).map_err(|error| WriterError::Other(error.to_string()))
}

/// A cached entry is reusable only while its suggestions still describe the
/// current text slice at the segment's new position.
fn cached_segment_matches(text: &str, plan: &SegmentPlan, cached: &CachedSegment) -> bool {
    let segment_len = plan.end - plan.start;
    cached.suggestions.iter().all(|suggestion| {
        let (Ok(start), Ok(end)) = (
            usize::try_from(suggestion.start_byte),
            usize::try_from(suggestion.end_byte),
        ) else {
            return false;
        };
        start < end
            && end <= segment_len
            && text.get(plan.start + start..plan.start + end) == Some(suggestion.original.as_str())
    })
}

impl CorrectionScanner {
    pub(crate) fn new(
        store: CorrectionStore,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
    ) -> Result<Self, WriterError> {
        let (lexicon_version, lexicon) = store.lexicon_snapshot_for_project(&project_id)?;
        let (model_path, model) = store.configured_model_snapshot()?;
        let engine = if let Some(engine) =
            store.cached_engine(&project_id, lexicon_version, model_path.as_deref())?
        {
            engine
        } else {
            let rules = RuleSet::with_common_rules();
            let config = CorrectionConfig::default();
            let engine = if let Some(model) = model {
                let scorer: Arc<dyn LanguageScorer> = model;
                CorrectionEngine::new_shared(scorer, lexicon, rules, config)
            } else {
                CorrectionEngine::rules_only_shared(lexicon, rules, config)
            }
            .map_err(|error| WriterError::Other(error.to_string()))?;
            store.cache_engine_if_model_current(
                &project_id,
                lexicon_version,
                model_path,
                Arc::new(engine),
            )?
        };
        Ok(Self {
            store,
            project_id,
            chapter_id,
            text_revision,
            engine,
        })
    }

    /// Cached suggestions and verified coverage only; never scans.
    pub(crate) fn chapter_snapshot(
        &self,
        text: &str,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        self.check(text, ScanSelection::RestoreOnly)
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
        self.check(text, ScanSelection::Range(start, end))
    }

    /// Recomputes every missing or invalid segment.
    pub(crate) fn check_chapter(
        &self,
        text: &str,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        self.check(text, ScanSelection::All)
    }

    fn check(
        &self,
        text: &str,
        selection: ScanSelection,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        u32::try_from(text.len()).map_err(|error| {
            WriterError::Other(format!("chapter exceeds correction byte limit: {error}"))
        })?;

        let state = self
            .store
            .chapter_result_cache(&self.project_id, &self.chapter_id)?;
        let scanner_fingerprint = self.store.scanner_fingerprint(&self.project_id)?;
        let (mut plans, mut dirty) = plan_segments(text, &state, &scanner_fingerprint)?;

        for plan in plans.iter_mut() {
            if plan.checked.is_some() || !selection.covers(plan.start, plan.end) {
                continue;
            }
            plan.checked = Some(self.scan_segment(text, plan)?);
            plan.cached_hit = false;
            dirty = true;
        }

        let mut next_segments = Vec::new();
        let mut seen_keys = HashSet::new();
        for plan in &plans {
            let Some(suggestions) = &plan.checked else {
                continue;
            };
            let key = (
                plan.text_fingerprint.clone(),
                plan.context_fingerprint.clone(),
            );
            if !seen_keys.insert(key) {
                continue;
            }
            if !plan.cached_hit {
                dirty = true;
            }
            next_segments.push(CachedSegment {
                start_byte: to_u32(plan.start)?,
                end_byte: to_u32(plan.end)?,
                text_fingerprint: plan.text_fingerprint.clone(),
                context_fingerprint: plan.context_fingerprint.clone(),
                suggestions: suggestions.clone(),
            });
        }

        let mut guard = state
            .lock()
            .map_err(|_| WriterError::Other("correction result cache lock poisoned".into()))?;
        guard.scanner_fingerprint = scanner_fingerprint;
        guard.segments = next_segments;
        guard.dirty |= dirty;
        guard.save()?;

        self.assemble_snapshot(&plans)
    }

    /// Cache-miss compute entry: scans one segment with its context and keeps
    /// the suggestions inside the segment's responsibility range.
    fn scan_segment(
        &self,
        text: &str,
        plan: &SegmentPlan,
    ) -> Result<Vec<CachedSuggestion>, WriterError> {
        let context = &text[plan.context_start..plan.context_end];
        let local_start = plan.start - plan.context_start;
        let local_end = plan.end - plan.context_start;
        let diagnostics = self
            .engine
            .scan(context)
            .map_err(|error| WriterError::Other(error.to_string()))?;
        diagnostics
            .into_iter()
            .filter(|diagnostic| {
                diagnostic.start_byte >= local_start && diagnostic.end_byte <= local_end
            })
            .map(|diagnostic| {
                Ok(CachedSuggestion {
                    start_byte: to_u32(diagnostic.start_byte - local_start)?,
                    end_byte: to_u32(diagnostic.end_byte - local_start)?,
                    original: diagnostic.original,
                    replacement: diagnostic.replacement,
                    source: diagnostic.source,
                    rule_id: diagnostic.rule_id,
                    severity: diagnostic.severity,
                })
            })
            .collect()
    }

    fn assemble_snapshot(
        &self,
        plans: &[SegmentPlan],
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        let mut diagnostics = Vec::new();
        let mut checked_ranges = Vec::new();
        for plan in plans {
            let Some(suggestions) = &plan.checked else {
                continue;
            };
            checked_ranges.push(CorrectionCheckedRangeDto {
                start_byte: to_u32(plan.start)?,
                end_byte: to_u32(plan.end)?,
            });
            for suggestion in suggestions {
                diagnostics.push(CorrectionDiagnostic {
                    start_byte: plan.start
                        + usize::try_from(suggestion.start_byte)
                            .map_err(|error| WriterError::Other(error.to_string()))?,
                    end_byte: plan.start
                        + usize::try_from(suggestion.end_byte)
                            .map_err(|error| WriterError::Other(error.to_string()))?,
                    original: suggestion.original.clone(),
                    replacement: suggestion.replacement.clone(),
                    source: suggestion.source.clone(),
                    rule_id: suggestion.rule_id.clone(),
                    severity: suggestion.severity.clone(),
                    confidence: None,
                });
            }
        }

        // Ignore choices are applied on the response so changing them never
        // requires rescanning the chapter.
        let filtered = self.store.filter_ignored(
            &self.project_id,
            &self.chapter_id,
            self.text_revision,
            diagnostics,
        )?;
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
}

/// Verifies cached entries against the current text and reports whether the
/// in-memory cache drifted from what is on disk.
fn plan_segments(
    text: &str,
    state: &Arc<Mutex<ChapterCorrectionCacheState>>,
    scanner_fingerprint: &str,
) -> Result<(Vec<SegmentPlan>, bool), WriterError> {
    let mut guard = state
        .lock()
        .map_err(|_| WriterError::Other("correction result cache lock poisoned".into()))?;
    let mut dirty = false;
    if guard.scanner_fingerprint != scanner_fingerprint {
        if !guard.segments.is_empty() {
            guard.segments.clear();
        }
        guard.scanner_fingerprint = scanner_fingerprint.to_string();
        guard.dirty = true;
    }
    let cached_segments = std::mem::take(&mut guard.segments);
    drop(guard);

    let mut index: HashMap<(String, String), CachedSegment> = HashMap::new();
    for segment in cached_segments {
        let key = (
            segment.text_fingerprint.clone(),
            segment.context_fingerprint.clone(),
        );
        if index.insert(key, segment).is_some() {
            // Duplicate keys can only come from an older, larger segmentation.
            dirty = true;
        }
    }

    let mut used_keys = HashSet::new();
    let mut plans = Vec::new();
    for (start, end) in canonical_segments(text) {
        let text_fingerprint = content_fingerprint(&text[start..end]);
        let (context_start, context_end) = context_range(text, start, end);
        let context_fingerprint = content_fingerprint(&text[context_start..context_end]);
        let key = (text_fingerprint.clone(), context_fingerprint.clone());
        let mut plan = SegmentPlan {
            start,
            end,
            context_start,
            context_end,
            text_fingerprint,
            context_fingerprint,
            checked: None,
            cached_hit: false,
        };
        let Some(cached) = index.get(&key) else {
            plans.push(plan);
            continue;
        };
        if !cached_segment_matches(text, &plan, cached) {
            dirty = true;
            plans.push(plan);
            continue;
        }
        used_keys.insert(key);
        if cached.start_byte != to_u32(start)? || cached.end_byte != to_u32(end)? {
            // The content moved; keep the result but persist new offsets.
            dirty = true;
        }
        plan.checked = Some(cached.suggestions.clone());
        plan.cached_hit = true;
        plans.push(plan);
    }
    if index.len() != used_keys.len() {
        // Entries that no longer match any segment of the current text.
        dirty = true;
    }
    Ok((plans, dirty))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_selection_coverage() {
        assert!(!ScanSelection::RestoreOnly.covers(0, 10));
        assert!(ScanSelection::All.covers(0, 10));
        assert!(ScanSelection::Range(5, 8).covers(0, 10));
        assert!(!ScanSelection::Range(10, 12).covers(0, 10));
        assert!(!ScanSelection::Range(0, 0).covers(0, 10));
    }

    #[test]
    fn cached_matches_requires_current_original_text() {
        let text = "甲乙丙丁";
        let plan = SegmentPlan {
            start: 3,
            end: 12,
            context_start: 0,
            context_end: 12,
            text_fingerprint: String::new(),
            context_fingerprint: String::new(),
            checked: None,
            cached_hit: false,
        };
        let cached = CachedSegment {
            start_byte: 3,
            end_byte: 12,
            text_fingerprint: String::new(),
            context_fingerprint: String::new(),
            suggestions: vec![CachedSuggestion {
                start_byte: 0,
                end_byte: 3,
                original: "乙".to_string(),
                replacement: "已".to_string(),
                source: "rule".to_string(),
                rule_id: "test".to_string(),
                severity: "error".to_string(),
            }],
        };
        assert!(cached_segment_matches(text, &plan, &cached));

        let mismatch = CachedSegment {
            suggestions: vec![CachedSuggestion {
                start_byte: 0,
                end_byte: 3,
                original: "甲".to_string(),
                replacement: "已".to_string(),
                source: "rule".to_string(),
                rule_id: "test".to_string(),
                severity: "error".to_string(),
            }],
            ..cached
        };
        assert!(!cached_segment_matches(text, &plan, &mismatch));
    }
}
