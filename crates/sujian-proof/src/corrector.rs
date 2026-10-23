// SPDX-License-Identifier: Apache-2.0
// Modified behavioral port of pycorrector/pycorrector/corrector.py by XuMing
// (xuming624), especially Corrector.correct/get_lm_correct_item.

use std::sync::Arc;

use crate::candidates::{CandidateGenerator, CandidateOrigin, CorrectionCandidate};
use crate::confusion::ConfusionMatcher;
use crate::decision::{CorrectionDecision, DecisionGate};
use crate::detector::{exact_name_occurrences, DetectionEvidence, DetectionKind, Detector};
use crate::lexicon::Lexicon;
use crate::lm::LanguageScorer;
use crate::rules::RuleSet;
use crate::segment::{Segmenter, TextSegment};
use crate::{CorrectionConfig, CorrectionDiagnostic, CorrectionError};

/// Characters of surrounding sentence scored on each side of a candidate.
/// Comparison happens on the same local window for the original and every
/// replacement, so a global sentence length cannot dominate the decision.
const LOCAL_CONTEXT_CHARS: usize = 6;

// The original text is always a competing candidate. KenLM scores rank
// alternatives; they are not calibrated error probabilities. Only the
// explicit decision gate may turn a candidate into a suggestion.

pub(crate) struct TraditionalCorrector {
    scorer: Option<Arc<dyn LanguageScorer>>,
    lexicon: Arc<Lexicon>,
    rules: RuleSet,
    config: CorrectionConfig,
    segmenter: Segmenter,
    detector: Detector,
    confusion_matcher: ConfusionMatcher,
}

#[derive(Clone, Copy, Debug)]
struct LocalScore {
    log10_score: f64,
    events: usize,
}

impl LocalScore {
    /// Per-scored-event log10 score. Length-changing replacements are compared
    /// with the same normalization instead of favoring shorter text.
    fn normalized(self) -> f64 {
        self.log10_score / self.events.max(1) as f64
    }
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
                let candidates =
                    candidate_generator.generate(&suspect.original, &self.lexicon, &self.segmenter);
                let candidates = self.candidates_for_scoring(candidates, &suspect.evidence);
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

                let original_score = self.local_context_score(
                    segment_text,
                    relative_start,
                    relative_end,
                    &suspect.original,
                )?;
                let mut accepted = Vec::<(f64, CorrectionCandidate)>::new();
                for candidate in candidates {
                    let candidate_score = self.local_context_score(
                        segment_text,
                        relative_start,
                        relative_end,
                        &candidate.text,
                    )?;
                    if DecisionGate::evaluate(
                        &suspect.original,
                        &candidate,
                        &suspect.evidence.segmentation,
                        &suspect.evidence,
                        original_score.normalized(),
                        candidate_score.normalized(),
                    ) == CorrectionDecision::Accept
                    {
                        accepted.push((
                            candidate_score.normalized() - original_score.normalized(),
                            candidate,
                        ));
                    }
                }
                accepted.sort_by(|(left_gain, left), (right_gain, right)| {
                    right_gain
                        .total_cmp(left_gain)
                        .then_with(|| left.text.cmp(&right.text))
                });
                let Some((_, chosen)) = accepted.into_iter().next() else {
                    continue;
                };

                diagnostics.push(CorrectionDiagnostic {
                    start_byte: suspect.start_byte,
                    end_byte: suspect.end_byte,
                    original: suspect.original,
                    replacement: chosen.text,
                    source: source.to_owned(),
                    rule_id: statistical_rule_id(&suspect.evidence).to_owned(),
                    // Statistical candidates are never certain mistakes. The
                    // fixed author-confirmed paths above remain `error`.
                    severity: "warning".to_owned(),
                    confidence: None,
                });
            }

            diagnostics.extend(self.rule_diagnostics(text));
        } else {
            diagnostics.extend(self.confirmed_misspelling_diagnostics(text));
            diagnostics.extend(self.confusion_diagnostics(text));
            diagnostics.extend(self.rule_diagnostics(text));
        }

        if !self.config.include_weak_hints {
            diagnostics.retain(|diagnostic| diagnostic.severity == "error");
        }

        let protected_ranges = self.lexicon.protected_word_ranges(text);
        diagnostics.retain(|diagnostic| {
            if diagnostic.rule_id == "proper.confirmed_misspelling" {
                return !self.lexicon.is_work_protected_span(
                    text,
                    diagnostic.start_byte,
                    diagnostic.end_byte,
                );
            }
            !protected_ranges
                .iter()
                .any(|(start, end)| diagnostic.start_byte < *end && *start < diagnostic.end_byte)
        });
        filter_diagnostics(text, diagnostics)
    }

    /// Filter and order generated candidates without touching the model, then
    /// enforce the configured scoring budget. Only candidates that implement
    /// the detector's explicit source-error relation can be accepted, so other
    /// guesses never consume model queries.
    fn candidates_for_scoring(
        &self,
        mut candidates: Vec<CorrectionCandidate>,
        evidence: &DetectionEvidence,
    ) -> Vec<CorrectionCandidate> {
        let budget = self.config.max_candidates_per_error;
        if budget == 0 {
            return Vec::new();
        }
        candidates.retain(|candidate| {
            candidate.origin == CandidateOrigin::WordConfusion
                || (candidate.known_general_word
                    && candidate_matches_source_error(candidate, evidence))
        });
        candidates.sort_by(|left, right| {
            let left_source_error = candidate_matches_source_error(left, evidence);
            let right_source_error = candidate_matches_source_error(right, evidence);
            right_source_error
                .cmp(&left_source_error)
                .then_with(|| right.origin.cmp(&left.origin))
                .then_with(|| left.changed_chars.cmp(&right.changed_chars))
                .then_with(|| right.known_general_word.cmp(&left.known_general_word))
                .then_with(|| left.rank.cmp(&right.rank))
                .then_with(|| left.text.cmp(&right.text))
        });
        candidates.truncate(budget);
        candidates
    }

    /// Score `replacement` in the same local sentence window as the original.
    /// `score_tokens()` keeps the sentence boundary convention; both sides are
    /// normalized per scored event so a length-changing candidate is compared
    /// on equal terms.
    fn local_context_score(
        &self,
        segment_text: &str,
        start: usize,
        end: usize,
        replacement: &str,
    ) -> Result<LocalScore, CorrectionError> {
        let context_start = context_start(segment_text, start, LOCAL_CONTEXT_CHARS);
        let context_end = context_end(segment_text, end, LOCAL_CONTEXT_CHARS);
        let mut local = String::with_capacity(context_end - context_start + replacement.len());
        local.push_str(&segment_text[context_start..start]);
        local.push_str(replacement);
        local.push_str(&segment_text[end..context_end]);

        let tokens = self.segmenter.char_tokens(&local);
        let log10_score = self
            .scorer
            .as_ref()
            .ok_or(CorrectionError::ModelUnavailable)?
            .score_tokens(&tokens)?;
        Ok(LocalScore {
            log10_score,
            events: tokens.len() + 1,
        })
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

fn candidate_matches_source_error(
    candidate: &CorrectionCandidate,
    evidence: &DetectionEvidence,
) -> bool {
    let [position] = candidate.changed_positions.as_slice() else {
        return false;
    };
    let Some(replacement) = candidate.text.chars().nth(*position) else {
        return false;
    };
    evidence
        .source_spelling_evidence
        .attested_error_pairs
        .iter()
        .any(|(attested_position, attested_replacement)| {
            attested_position == position && *attested_replacement == replacement
        })
}

fn statistical_rule_id(evidence: &DetectionEvidence) -> &'static str {
    if !evidence
        .source_spelling_evidence
        .attested_error_pairs
        .is_empty()
    {
        return "detector.attested_error";
    }
    "detector.candidate"
}

fn context_start(text: &str, end: usize, max_chars: usize) -> usize {
    text[..end]
        .char_indices()
        .rev()
        .nth(max_chars.saturating_sub(1))
        .map_or(0, |(byte, _)| byte)
}

fn context_end(text: &str, start: usize, max_chars: usize) -> usize {
    text[start..]
        .char_indices()
        .nth(max_chars)
        .map_or(text.len(), |(byte, _)| start + byte)
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
            rules_only(lexicon).suggest("甲乙丙丁说道。").expect("scan"),
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
            rules_only(lexicon).suggest("甲乙丁说道。").expect("scan"),
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
