// SPDX-License-Identifier: Apache-2.0
// Modified behavioral port of pycorrector/pycorrector/corrector.py by XuMing
// (xuming624), especially Corrector.correct/get_lm_correct_item.

use std::sync::Arc;

use crate::candidates::CandidateGenerator;
use crate::confusion::ConfusionMatcher;
use crate::detector::{exact_name_occurrences, DetectionKind, Detector};
use crate::lexicon::Lexicon;
use crate::lm::LanguageScorer;
use crate::rules::RuleSet;
use crate::segment::{Segmenter, TextSegment};
use crate::{CorrectionConfig, CorrectionDiagnostic, CorrectionError};

const PYCORRECTOR_PPL_THRESHOLD: f64 = 57.0;

// Behavioral port of pycorrector Corrector.correct/get_lm_correct_item
// (Apache-2.0). The original stays in every candidate set; KenLM perplexity
// ranks candidates within pycorrector's tolerance, and confidence remains unset
// because model scores are not calibrated probabilities.

pub(crate) struct TraditionalCorrector {
    scorer: Option<Arc<dyn LanguageScorer>>,
    lexicon: Arc<Lexicon>,
    rules: RuleSet,
    config: CorrectionConfig,
    segmenter: Segmenter,
    detector: Detector,
    confusion_matcher: ConfusionMatcher,
}

impl TraditionalCorrector {
    pub(crate) fn new(
        scorer: Arc<dyn LanguageScorer>,
        lexicon: Lexicon,
        rules: RuleSet,
        config: CorrectionConfig,
    ) -> Result<Self, CorrectionError> {
        Self::new_shared(scorer, Arc::new(lexicon), rules, config)
    }

    pub(crate) fn new_shared(
        scorer: Arc<dyn LanguageScorer>,
        lexicon: Arc<Lexicon>,
        rules: RuleSet,
        config: CorrectionConfig,
    ) -> Result<Self, CorrectionError> {
        let confusion_matcher = ConfusionMatcher::new(&lexicon.confusion_entries())
            .map_err(CorrectionError::InvalidConfiguration)?;
        Ok(Self {
            scorer: Some(scorer),
            lexicon,
            rules,
            config,
            segmenter: Segmenter::new(),
            detector: Detector::default(),
            confusion_matcher,
        })
    }

    pub(crate) fn rules_only(
        lexicon: Arc<Lexicon>,
        rules: RuleSet,
        config: CorrectionConfig,
    ) -> Result<Self, CorrectionError> {
        let confusion_matcher = ConfusionMatcher::new(&lexicon.confusion_entries())
            .map_err(CorrectionError::InvalidConfiguration)?;
        Ok(Self {
            scorer: None,
            lexicon,
            rules,
            config,
            segmenter: Segmenter::new(),
            detector: Detector::default(),
            confusion_matcher,
        })
    }

    /// Generate byte-addressed suggestions without changing `text`.
    pub(crate) fn suggest(&self, text: &str) -> Result<Vec<CorrectionDiagnostic>, CorrectionError> {
        let mut diagnostics = Vec::new();
        if let Some(scorer) = self.scorer.as_ref() {
            let suspects = self.detector.detect(
                text,
                &self.lexicon,
                &self.segmenter,
                &self.confusion_matcher,
                scorer.as_ref(),
            )?;

            for suspect in suspects {
                let source = match suspect.kind {
                    DetectionKind::ConfirmedEntitySpelling => "proper_name",
                    DetectionKind::ConfusionWord => "confusion_word",
                    DetectionKind::OutOfVocabularyWord => "oov",
                    DetectionKind::Character => "character_ngram",
                };
                if let Some(replacement) = suspect.exact_replacement {
                    if replacement == suspect.original {
                        continue;
                    }
                    let rule_id = match suspect.kind {
                        DetectionKind::ConfirmedEntitySpelling => "proper.confirmed_misspelling",
                        DetectionKind::ConfusionWord => "confusion.custom",
                        _ => continue,
                    };
                    diagnostics.push(CorrectionDiagnostic {
                        start_byte: suspect.start_byte,
                        end_byte: suspect.end_byte,
                        original: suspect.original,
                        replacement,
                        source: source.to_owned(),
                        rule_id: rule_id.to_owned(),
                        severity: "error".to_owned(),
                        confidence: None,
                    });
                    continue;
                }

                if self.config.max_candidates_per_error == 0 {
                    continue;
                }
                let candidate_generator = CandidateGenerator::new();
                let candidates = candidate_generator.generate(&suspect.original, &self.lexicon);
                if candidates.is_empty() {
                    continue;
                }

                let segment =
                    segment_for_span(text, suspect.start_byte, suspect.end_byte, &self.segmenter)
                        .ok_or(CorrectionError::InvalidRange {
                        start: suspect.start_byte,
                        end: suspect.end_byte,
                        text_len: text.len(),
                    })?;
                let segment_text = &text[segment.start_byte..segment.end_byte];
                let relative_start = suspect.start_byte - segment.start_byte;
                let relative_end = suspect.end_byte - segment.start_byte;
                let before = &segment_text[..relative_start];
                let after = &segment_text[relative_end..];

                // pycorrector keeps the original in the PPL candidate set. A model
                // score is a ranking signal, not confidence: only flag an error
                // when the source falls outside the upstream-compatible tolerance.
                let original_ppl = self.perplexity_text(segment_text)?;
                let mut ranked = Vec::with_capacity(candidates.len() + 1);
                ranked.push((None, original_ppl));
                for candidate in candidates {
                    let mut alternative =
                        String::with_capacity(before.len() + candidate.len() + after.len());
                    alternative.push_str(before);
                    alternative.push_str(&candidate);
                    alternative.push_str(after);
                    let perplexity = self.perplexity_text(&alternative)?;
                    ranked.push((Some(candidate), perplexity));
                }
                ranked.sort_by(|left, right| {
                    left.1
                        .total_cmp(&right.1)
                        .then_with(|| left.0.cmp(&right.0))
                });
                ranked.truncate(self.config.max_candidates_per_error.saturating_add(1));

                let Some((_, best_ppl)) = ranked.first() else {
                    continue;
                };
                let acceptable_limit = best_ppl + PYCORRECTOR_PPL_THRESHOLD;
                let original_acceptable = original_ppl <= acceptable_limit;

                // If the source remains in pycorrector's acceptable set, preserve
                // it without emitting statistically generated alternatives. LM
                // rankings are not a calibrated reason to warn on valid prose.
                if original_acceptable {
                    continue;
                }

                let source_kind = source.to_owned();
                let rule_id = match suspect.kind {
                    DetectionKind::OutOfVocabularyWord => "detector.oov",
                    DetectionKind::Character => "detector.char_mad",
                    _ => "detector.candidate",
                };
                let chosen = ranked.into_iter().find_map(|(replacement, perplexity)| {
                    (perplexity <= acceptable_limit)
                        .then_some(replacement)
                        .flatten()
                });
                if let Some(replacement) = chosen {
                    diagnostics.push(CorrectionDiagnostic {
                        start_byte: suspect.start_byte,
                        end_byte: suspect.end_byte,
                        original: suspect.original,
                        replacement,
                        source: source_kind,
                        rule_id: rule_id.to_owned(),
                        severity: "error".to_owned(),
                        confidence: None,
                    });
                }
            }

            if !self.config.include_weak_hints {
                diagnostics.extend(
                    self.rule_diagnostics(text)
                        .into_iter()
                        .filter(|diagnostic| diagnostic.severity == "error"),
                );
            } else {
                diagnostics.extend(self.rule_diagnostics(text));
            }
        } else {
            diagnostics.extend(self.confirmed_misspelling_diagnostics(text));
            diagnostics.extend(self.confusion_diagnostics(text));
            diagnostics.extend(self.rule_diagnostics(text));
            if !self.config.include_weak_hints {
                diagnostics.retain(|diagnostic| diagnostic.severity == "error");
            }
        }

        let protected_ranges = self.lexicon.protected_word_ranges(text);
        diagnostics.retain(|diagnostic| {
            if diagnostic.rule_id == "proper.confirmed_misspelling" {
                return !self.lexicon.protected_entry_wraps_range(
                    text,
                    diagnostic.start_byte,
                    diagnostic.end_byte,
                );
            }
            !protected_ranges
                .iter()
                .any(|(start, end)| diagnostic.start_byte < *end && *start < diagnostic.end_byte)
        });
        Ok(filter_diagnostics(text, diagnostics)?)
    }

    fn perplexity_text(&self, text: &str) -> Result<f64, CorrectionError> {
        let tokens = self.segmenter.char_tokens(text);
        self.scorer
            .as_ref()
            .ok_or(CorrectionError::ModelUnavailable)?
            .perplexity(&tokens)
    }

    fn confirmed_misspelling_diagnostics(&self, text: &str) -> Vec<CorrectionDiagnostic> {
        let mut diagnostics = Vec::new();
        for segment in self.segmenter.sentence_segments(text) {
            let sentence = &text[segment.start_byte..segment.end_byte];
            for relation in self.lexicon.work_entity_lexicon().misspellings() {
                for (start, end) in exact_name_occurrences(sentence, &relation.original) {
                    diagnostics.push(CorrectionDiagnostic {
                        start_byte: segment.start_byte + start,
                        end_byte: segment.start_byte + end,
                        original: relation.original.clone(),
                        replacement: relation.replacement.clone(),
                        source: "proper_name".to_owned(),
                        rule_id: "proper.confirmed_misspelling".to_owned(),
                        severity: "error".to_owned(),
                        confidence: None,
                    });
                }
            }
        }
        diagnostics
    }

    fn rule_diagnostics(&self, text: &str) -> Vec<CorrectionDiagnostic> {
        self.rules
            .detect(text)
            .into_iter()
            .map(|matched| CorrectionDiagnostic {
                start_byte: matched.start_byte,
                end_byte: matched.end_byte,
                original: matched.original,
                replacement: matched.replacement,
                source: "rule".to_owned(),
                rule_id: matched.rule_id,
                severity: matched.severity,
                confidence: None,
            })
            .collect()
    }

    fn confusion_diagnostics(&self, text: &str) -> Vec<CorrectionDiagnostic> {
        let mut diagnostics = Vec::new();
        for segment in self.segmenter.sentence_segments(text) {
            let sentence = &text[segment.start_byte..segment.end_byte];
            for matched in self.confusion_matcher.scan_text(sentence) {
                if self.lexicon.is_ignored(&matched.original_text) {
                    continue;
                }
                diagnostics.push(CorrectionDiagnostic {
                    start_byte: segment.start_byte + matched.start_byte,
                    end_byte: segment.start_byte + matched.end_byte,
                    original: matched.original_text,
                    replacement: matched.suggestion,
                    source: "confusion_word".to_owned(),
                    rule_id: "confusion.custom".to_owned(),
                    severity: "error".to_owned(),
                    confidence: None,
                });
            }
        }
        diagnostics
    }
}

fn segment_for_span(
    text: &str,
    start_byte: usize,
    end_byte: usize,
    segmenter: &Segmenter,
) -> Option<TextSegment> {
    segmenter
        .sentence_segments(text)
        .into_iter()
        .find(|segment| segment.start_byte <= start_byte && end_byte <= segment.end_byte)
}

fn filter_diagnostics(
    text: &str,
    mut diagnostics: Vec<CorrectionDiagnostic>,
) -> Result<Vec<CorrectionDiagnostic>, CorrectionError> {
    diagnostics.retain(|diagnostic| {
        diagnostic.start_byte < diagnostic.end_byte
            && diagnostic.replacement != diagnostic.original
            && !diagnostic.replacement.is_empty()
    });
    for diagnostic in &diagnostics {
        let original = text.get(diagnostic.start_byte..diagnostic.end_byte).ok_or(
            CorrectionError::InvalidRange {
                start: diagnostic.start_byte,
                end: diagnostic.end_byte,
                text_len: text.len(),
            },
        )?;
        if original != diagnostic.original {
            return Err(CorrectionError::InvalidRange {
                start: diagnostic.start_byte,
                end: diagnostic.end_byte,
                text_len: text.len(),
            });
        }
    }

    diagnostics.sort_by(|left, right| {
        source_priority(&left.source)
            .cmp(&source_priority(&right.source))
            .then_with(|| {
                severity_priority(&left.severity).cmp(&severity_priority(&right.severity))
            })
            .then_with(|| {
                (right.end_byte - right.start_byte).cmp(&(left.end_byte - left.start_byte))
            })
            .then_with(|| left.start_byte.cmp(&right.start_byte))
            .then_with(|| left.replacement.cmp(&right.replacement))
    });

    let mut filtered = Vec::new();
    for diagnostic in diagnostics {
        let same_span_alternative = filtered.iter().any(|existing: &CorrectionDiagnostic| {
            existing.start_byte == diagnostic.start_byte
                && existing.end_byte == diagnostic.end_byte
                && existing.original == diagnostic.original
        });
        let overlaps_other = filtered.iter().any(|existing: &CorrectionDiagnostic| {
            (existing.start_byte != diagnostic.start_byte
                || existing.end_byte != diagnostic.end_byte)
                && diagnostic.start_byte < existing.end_byte
                && existing.start_byte < diagnostic.end_byte
        });
        if overlaps_other {
            continue;
        }
        // Only the best-ranked suggestion for a span is shown. Stacking
        // alternative replacements on the same bytes is noise, especially for
        // uncertain proper-name checks.
        if same_span_alternative {
            continue;
        }
        filtered.push(diagnostic);
    }
    filtered.sort_by(|left, right| {
        left.start_byte
            .cmp(&right.start_byte)
            .then_with(|| left.end_byte.cmp(&right.end_byte))
            .then_with(|| left.replacement.cmp(&right.replacement))
    });
    Ok(filtered)
}

fn source_priority(source: &str) -> u8 {
    match source {
        // Only explicit author-confirmed misspellings use `proper_name`; they
        // must outrank ordinary confusion candidates, matching the detector.
        "proper_name" => 0,
        "confusion_word" => 1,
        "rule" => 2,
        "oov" => 3,
        "character_ngram" => 4,
        _ => 5,
    }
}

fn severity_priority(severity: &str) -> u8 {
    match severity {
        "error" => 0,
        "warning" => 1,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexicon::LexiconPaths;

    /// A flat model deliberately provides no language evidence. Work-dictionary
    /// terms are protection only, so a similar span must not be rewritten just
    /// because it resembles a confirmed entity.
    struct FlatScorer;

    impl LanguageScorer for FlatScorer {
        fn score_tokens(&self, _tokens: &[&str]) -> Result<f64, CorrectionError> {
            Ok(0.0)
        }

        fn perplexity(&self, _tokens: &[&str]) -> Result<f64, CorrectionError> {
            Ok(1.0)
        }
    }

    fn corrector(lexicon: Lexicon) -> TraditionalCorrector {
        TraditionalCorrector::new(
            Arc::new(FlatScorer),
            lexicon,
            RuleSet::empty(),
            CorrectionConfig::default(),
        )
        .expect("corrector")
    }

    fn rules_only(lexicon: Lexicon) -> TraditionalCorrector {
        TraditionalCorrector::rules_only(
            Arc::new(lexicon),
            RuleSet::empty(),
            CorrectionConfig::default(),
        )
        .expect("corrector")
    }

    #[test]
    fn work_dictionary_terms_are_protected_not_replacement_targets() {
        let mut lexicon = Lexicon::from_paths(&LexiconPaths::default()).expect("embedded lexicon");
        lexicon.insert_protected_word("甲地乙坊".to_string());
        let diagnostics = corrector(lexicon)
            .suggest("他住进了甲地乙房。")
            .expect("scan");
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.original != "甲地乙房"),
            "an unconfirmed similar span must not be replaced by a work term"
        );
    }

    fn entity_lexicon() -> Lexicon {
        let mut lexicon = Lexicon::from_paths(&LexiconPaths::default()).expect("embedded lexicon");
        lexicon.insert_work_entity("甲乙丙".to_string(), crate::ProperNameKind::Person);
        lexicon.insert_work_entity_alias("甲乙丙", "乙丙".to_string());
        lexicon.insert_work_entity_misspelling("甲乙丙", "甲乙丁".to_string());
        lexicon
    }

    #[test]
    fn explicit_wrong_spelling_is_replaced_but_alias_is_protected() {
        let diagnostics = corrector(entity_lexicon())
            .suggest("甲乙丁说道。")
            .expect("scan");
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.original == "甲乙丁"
                && diagnostic.replacement == "甲乙丙"
                && diagnostic.rule_id == "proper.confirmed_misspelling"
                && diagnostic.severity == "error"
        }));

        let alias_diagnostics = corrector(entity_lexicon())
            .suggest("乙丙说道。")
            .expect("scan");
        assert!(
            alias_diagnostics
                .iter()
                .all(|diagnostic| diagnostic.original != "乙丙"),
            "a confirmed alias is protected and must not be flagged"
        );
    }

    #[test]
    fn valid_ordinary_words_are_not_replaced_by_work_entities() {
        let mut lexicon = Lexicon::from_paths(&LexiconPaths::default()).expect("embedded lexicon");
        lexicon.insert_protected_word("甲乙".to_string());
        lexicon.insert_protected_word("乙丙".to_string());
        let corrector = corrector(lexicon);
        assert!(corrector.suggest("甲丁").expect("scan").is_empty());
        assert!(corrector.suggest("乙丁").expect("scan").is_empty());
    }

    #[test]
    fn canonical_prefixed_misspelling_is_still_replaced_in_both_paths() {
        let mut lexicon = Lexicon::from_paths(&LexiconPaths::default()).expect("embedded lexicon");
        lexicon.insert_work_entity("甲乙丙".to_string(), crate::ProperNameKind::Person);
        lexicon.insert_work_entity_misspelling("甲乙丙", "甲乙丙丁".to_string());

        for diagnostics in [
            corrector(lexicon.clone())
                .suggest("甲乙丙丁说道。")
                .expect("scan"),
            rules_only(lexicon)
                .suggest("甲乙丙丁说道。")
                .expect("scan"),
        ] {
            assert!(diagnostics.iter().any(|diagnostic| {
                diagnostic.original == "甲乙丙丁"
                    && diagnostic.replacement == "甲乙丙"
                    && diagnostic.rule_id == "proper.confirmed_misspelling"
            }));
        }
    }

    #[test]
    fn confirmed_misspelling_outranks_confusion_in_both_paths() {
        let mut lexicon = Lexicon::from_paths(&LexiconPaths::default()).expect("embedded lexicon");
        lexicon.insert_work_entity("甲乙丙".to_string(), crate::ProperNameKind::Person);
        lexicon.insert_work_entity_misspelling("甲乙丙", "甲乙丁".to_string());
        lexicon.insert_confusion("甲乙丁".to_string(), "甲乙戊".to_string());

        for diagnostics in [
            corrector(lexicon.clone())
                .suggest("甲乙丁说道。")
                .expect("scan"),
            rules_only(lexicon)
                .suggest("甲乙丁说道。")
                .expect("scan"),
        ] {
            let chosen = diagnostics
                .iter()
                .find(|diagnostic| diagnostic.original == "甲乙丁")
                .expect("confirmed misspelling diagnostic");
            assert_eq!(chosen.replacement, "甲乙丙");
            assert_eq!(chosen.rule_id, "proper.confirmed_misspelling");
        }
    }
}
