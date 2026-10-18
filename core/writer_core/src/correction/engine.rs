use std::sync::Arc;

use sujian_proof::{CorrectionConfig, CorrectionEngine, LanguageScorer, RuleSet};

use crate::api::{CorrectionSuggestionDto, WriterError};

use super::CorrectionStore;

pub(crate) struct CorrectionScanner {
    store: CorrectionStore,
    project_id: String,
    chapter_id: String,
    text_revision: u64,
    engine: Arc<CorrectionEngine>,
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

    pub(crate) fn scan_range(
        &self,
        text: &str,
        range: (u32, u32),
        offset_base: u32,
    ) -> Result<Vec<CorrectionSuggestionDto>, WriterError> {
        let start =
            usize::try_from(range.0).map_err(|error| WriterError::Other(error.to_string()))?;
        let end =
            usize::try_from(range.1).map_err(|error| WriterError::Other(error.to_string()))?;
        if start > end || text.get(start..end).is_none() {
            return Err(WriterError::Other(format!(
                "invalid correction byte range {}..{} for text of {} bytes",
                range.0,
                range.1,
                text.len()
            )));
        }

        // Sentence and chapter shard requests scan bounded neighboring context.
        const CONTEXT_BYTES: usize = 256;
        let mut context_start = start.saturating_sub(CONTEXT_BYTES);
        while !text.is_char_boundary(context_start) {
            context_start -= 1;
        }
        let mut context_end = end.saturating_add(CONTEXT_BYTES).min(text.len());
        while !text.is_char_boundary(context_end) {
            context_end -= 1;
        }
        let context_text = &text[context_start..context_end];
        let local_start = start - context_start;
        let local_end = end - context_start;

        let suggestions = self
            .engine
            .scan(context_text)
            .map_err(|error| WriterError::Other(error.to_string()))?;
        let mut selected = suggestions
            .into_iter()
            .filter(|suggestion| {
                suggestion.start_byte >= local_start && suggestion.end_byte <= local_end
            })
            .collect::<Vec<_>>();
        let offset =
            usize::try_from(offset_base).map_err(|error| WriterError::Other(error.to_string()))?;
        for suggestion in &mut selected {
            suggestion.start_byte += context_start + offset;
            suggestion.end_byte += context_start + offset;
        }
        let selected = self.store.filter_ignored(
            &self.project_id,
            &self.chapter_id,
            self.text_revision,
            selected,
        )?;

        selected
            .into_iter()
            .map(|suggestion| CorrectionSuggestionDto::try_from((suggestion, self.text_revision)))
            .collect()
    }

    pub(crate) fn scan_chapter(
        &self,
        text: &str,
    ) -> Result<Vec<CorrectionSuggestionDto>, WriterError> {
        u32::try_from(text.len()).map_err(|error| {
            WriterError::Other(format!("chapter exceeds correction byte limit: {error}"))
        })?;
        if text.is_empty() {
            return Ok(Vec::new());
        }

        const CORE_CHUNK_BYTES: usize = 16 * 1024;
        const CONTEXT_BYTES: usize = 256;
        let mut suggestions = Vec::new();
        let mut core_start = 0usize;
        while core_start < text.len() {
            let mut core_end = core_start.saturating_add(CORE_CHUNK_BYTES).min(text.len());
            while !text.is_char_boundary(core_end) {
                core_end -= 1;
            }

            let mut context_start = core_start.saturating_sub(CONTEXT_BYTES);
            while !text.is_char_boundary(context_start) {
                context_start -= 1;
            }
            let mut context_end = core_end.saturating_add(CONTEXT_BYTES).min(text.len());
            while !text.is_char_boundary(context_end) {
                context_end -= 1;
            }
            let context = &text[context_start..context_end];
            let context_len = u32::try_from(context.len())
                .map_err(|error| WriterError::Other(error.to_string()))?;
            let base = u32::try_from(context_start)
                .map_err(|error| WriterError::Other(error.to_string()))?;
            let core_start_byte =
                u32::try_from(core_start).map_err(|error| WriterError::Other(error.to_string()))?;
            let core_end_byte =
                u32::try_from(core_end).map_err(|error| WriterError::Other(error.to_string()))?;
            suggestions.extend(
                self.scan_range(context, (0, context_len), base)?
                    .into_iter()
                    .filter(|suggestion| {
                        suggestion.start_byte >= core_start_byte
                            && suggestion.start_byte < core_end_byte
                    }),
            );
            core_start = core_end;
        }
        Ok(suggestions)
    }
}

pub(crate) fn scan_text(
    store: &CorrectionStore,
    project_id: &str,
    chapter_id: &str,
    text_revision: u64,
    text: &str,
    range: (u32, u32),
    offset_base: u32,
) -> Result<Vec<CorrectionSuggestionDto>, WriterError> {
    CorrectionScanner::new(
        store.clone(),
        project_id.to_owned(),
        chapter_id.to_owned(),
        text_revision,
    )?
    .scan_range(text, range, offset_base)
}
