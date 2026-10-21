// SPDX-License-Identifier: Apache-2.0
// Modified behavioral port of pycorrector/pycorrector/corrector.py by XuMing
// (xuming624), especially Corrector.correct/get_lm_correct_item.

use std::sync::Arc;

use crate::candidates::CandidateGenerator;
use crate::confusion::ConfusionMatcher;
use crate::detector::{DetectionKind, Detector};
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
                    DetectionKind::ConfusionWord => "confusion_word",
                    DetectionKind::ProperName => "proper_name",
                    DetectionKind::OutOfVocabularyWord => "oov",
                    DetectionKind::Character => "character_ngram",
                };
                if let Some(replacement) = suspect.exact_replacement {
                    if replacement != suspect.original {
                        // A near match of an author-confirmed work entity is
                        // evidence for a check, not proof of a typo. Only an
                        // explicit custom confusion entry keeps error severity.
                        let (rule_id, severity) = match suspect.kind {
                            DetectionKind::ConfusionWord => ("confusion.custom", "error"),
                            DetectionKind::ProperName => (
                                match suspect.entity_kind {
                                    Some(crate::proper_names::ProperNameKind::Person) => {
                                        "proper.person_check"
                                    }
                                    Some(crate::proper_names::ProperNameKind::Place) => {
                                        "proper.place_check"
                                    }
                                    _ => "proper.entity_check",
                                },
                                "warning",
                            ),
                            _ => ("detector.exact", "error"),
                        };
                        diagnostics.push(CorrectionDiagnostic {
                            start_byte: suspect.start_byte,
                            end_byte: suspect.end_byte,
                            original: suspect.original,
                            replacement,
                            source: source.to_owned(),
                            rule_id: rule_id.to_owned(),
                            severity: severity.to_owned(),
                            confidence: None,
                        });
                    }
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
            diagnostics.extend(self.confusion_diagnostics(text));
            diagnostics.extend(self.rule_diagnostics(text));
            if !self.config.include_weak_hints {
                diagnostics.retain(|diagnostic| diagnostic.severity == "error");
            }
        }

        let protected_ranges = self.lexicon.protected_word_ranges(text);
        diagnostics.retain(|diagnostic| {
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
        "confusion_word" => 0,
        "proper_name" => 1,
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

    /// A flat model deliberately provides no language evidence. Proper-name
    /// findings must still be warning-level checks and ordinary words must
    /// never be rewritten merely because they resemble a work entity.
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

    #[test]
    fn confirmed_entity_variant_is_warning_not_error() {
        let mut lexicon = Lexicon::from_paths(&LexiconPaths::default()).expect("embedded lexicon");
        lexicon.insert_protected_word("祁门客栈".to_string());
        let diagnostics = corrector(lexicon)
            .suggest("他住进了奇门客栈。")
            .expect("scan");
        let finding = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.original == "奇门客栈")
            .expect("entity check");
        assert_eq!(finding.replacement, "祁门客栈");
        assert_eq!(finding.severity, "warning");
        assert_eq!(finding.rule_id, "proper.entity_check");
    }

    #[test]
    fn valid_ordinary_words_are_not_replaced_by_work_entities() {
        let mut lexicon = Lexicon::from_paths(&LexiconPaths::default()).expect("embedded lexicon");
        lexicon.insert_protected_word("祁门".to_string());
        lexicon.insert_protected_word("周于".to_string());
        let corrector = corrector(lexicon);
        assert!(corrector.suggest("奇门").expect("scan").is_empty());
        assert!(corrector.suggest("周围").expect("scan").is_empty());
    }
}
