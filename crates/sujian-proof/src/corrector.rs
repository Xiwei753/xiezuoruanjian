// SPDX-License-Identifier: GPL-3.0-only
//! Correction pipeline.
//!
//! Order is deliberate:
//! 1. deterministic author relations (confirmed misspellings and literal
//!    confusion entries) plus layout/punctuation rules;
//! 2. equal-length neural character proposals, calibrated and filtered against
//!    author-owned protection;
//! 3. one final merge that keeps the original as the competing text.
//!
//! No branch rewrites the caller's text. Every diagnostic carries a UTF-8 byte
//! range into the original string.

use std::sync::Arc;

use crate::confusion::ConfusionMatcher;
use crate::lexicon::Lexicon;
use crate::neural::NeuralPredictor;
use crate::postprocess;
use crate::rules::RuleSet;
use crate::segment::Segmenter;
use crate::{CorrectionConfig, CorrectionDiagnostic, CorrectionError};

pub(crate) struct Corrector {
    predictor: Option<Arc<dyn NeuralPredictor>>,
    lexicon: Arc<Lexicon>,
    rules: RuleSet,
    config: CorrectionConfig,
    segmenter: Segmenter,
    confusion_matcher: ConfusionMatcher,
}

impl Corrector {
    pub(crate) fn new(
        predictor: Arc<dyn NeuralPredictor>,
        lexicon: Lexicon,
        rules: RuleSet,
        config: CorrectionConfig,
    ) -> Result<Self, CorrectionError> {
        Self::new_shared(predictor, Arc::new(lexicon), rules, config)
    }

    pub(crate) fn new_shared(
        predictor: Arc<dyn NeuralPredictor>,
        lexicon: Arc<Lexicon>,
        rules: RuleSet,
        config: CorrectionConfig,
    ) -> Result<Self, CorrectionError> {
        let confusion_matcher = ConfusionMatcher::new(&lexicon.confusion_entries())
            .map_err(CorrectionError::InvalidConfiguration)?;
        Ok(Self {
            predictor: Some(predictor),
            lexicon,
            rules,
            config,
            segmenter: Segmenter::new(),
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
            predictor: None,
            lexicon,
            rules,
            config,
            segmenter: Segmenter::new(),
            confusion_matcher,
        })
    }

    /// Generate byte-addressed suggestions without changing `text`.
    pub(crate) fn suggest(&self, text: &str) -> Result<Vec<CorrectionDiagnostic>, CorrectionError> {
        let mut diagnostics = self.confirmed_misspelling_diagnostics(text);
        diagnostics.extend(self.confusion_diagnostics(text));
        diagnostics.extend(self.rule_diagnostics(text));

        if let Some(predictor) = &self.predictor {
            diagnostics.extend(self.neural_diagnostics(text, predictor.as_ref())?);
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

    fn neural_diagnostics(
        &self,
        text: &str,
        predictor: &dyn NeuralPredictor,
    ) -> Result<Vec<CorrectionDiagnostic>, CorrectionError> {
        // One call per scanned range. `ProofreadingModel` already splits long
        // ranges into overlapping 512-token windows and merges predictions at
        // the same original byte offset, so expanding each sentence by ±24
        // characters and running BERT again for every sentence only creates
        // duplicate overlapping inference. Core filters the returned absolute
        // byte ranges back to the dirty segments it owns.
        let predictions = predictor.predict(text)?;
        let accepted =
            postprocess::postprocess(text, predictions, &self.lexicon, predictor.calibrator());
        Ok(accepted
            .into_iter()
            .map(|accepted| CorrectionDiagnostic {
                start_byte: accepted.start_byte,
                end_byte: accepted.end_byte,
                original: accepted.original,
                replacement: accepted.replacement,
                source: "neural_csc".to_string(),
                rule_id: "neural.macbert4csc".to_string(),
                severity: "warning".to_string(),
                confidence: Some(accepted.probability),
            })
            .collect())
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

/// Find every exact occurrence of `needle` in `haystack`, in bytes.
pub(crate) fn exact_name_occurrences(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    haystack
        .match_indices(needle)
        .map(|(start, _)| (start, start + needle.len()))
        .collect()
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
        if overlaps_other || same_span_alternative {
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
        "proper_name" => 0,
        "confusion_word" => 1,
        "rule" => 2,
        "neural_csc" => 3,
        _ => 4,
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
    use crate::postprocess::Calibrator;
    use crate::{CorrectionEngine, NeuralPredictor, PositionPrediction, ProperNameKind};

    struct FixedPredictor {
        proposals: Vec<(char, char, f32, f32)>,
        calibrator: Calibrator,
    }

    impl NeuralPredictor for FixedPredictor {
        fn predict(&self, text: &str) -> Result<Vec<PositionPrediction>, CorrectionError> {
            let mut predictions = Vec::new();
            for (byte, ch) in text.char_indices() {
                if let Some((_, candidate, original_log, candidate_log)) = self
                    .proposals
                    .iter()
                    .find(|(original, _, _, _)| *original == ch)
                {
                    predictions.push(PositionPrediction {
                        start_byte: byte,
                        end_byte: byte + ch.len_utf8(),
                        original: ch,
                        candidate: *candidate,
                        original_log_probability: *original_log,
                        candidate_log_probability: *candidate_log,
                    });
                }
            }
            Ok(predictions)
        }

        fn calibrator(&self) -> &Calibrator {
            &self.calibrator
        }
    }

    fn calibrator() -> Calibrator {
        Calibrator {
            version: "test".to_string(),
            intercept: 0.0,
            margin_weight: 1.0,
            candidate_weight: 0.0,
            threshold: 0.5,
        }
    }

    fn predictor(proposals: Vec<(char, char, f32, f32)>) -> Arc<dyn NeuralPredictor> {
        Arc::new(FixedPredictor {
            proposals,
            calibrator: calibrator(),
        })
    }

    fn engine(lexicon: Lexicon, proposals: Vec<(char, char, f32, f32)>) -> CorrectionEngine {
        CorrectionEngine::new(
            predictor(proposals),
            lexicon,
            RuleSet::empty(),
            CorrectionConfig::default(),
        )
        .expect("engine")
    }

    #[test]
    fn protected_complete_work_name_is_never_rewritten() {
        let mut lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        lexicon.insert_protected_word("甲乙".to_string());
        let diagnostics = engine(lexicon, vec![('甲', '假', -3.0, -0.1)])
            .scan("甲乙走进了房间。")
            .expect("scan");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn a_confident_unprotected_character_is_reported_with_calibrated_confidence() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let diagnostics = engine(lexicon, vec![('幅', '副', -3.0, -0.1)])
            .scan("他画了一幅画。")
            .expect("scan");
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.original == "幅"
                && diagnostic.replacement == "副"
                && diagnostic.rule_id == "neural.macbert4csc"
                && diagnostic.confidence.is_some()
        }));
    }

    #[test]
    fn weak_model_proposals_are_suppressed() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let diagnostics = engine(lexicon, vec![('她', '他', -0.1, -0.2)])
            .scan("她笑了。")
            .expect("scan");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn confirmed_misspelling_is_an_error_even_when_weak_hints_are_disabled() {
        let mut lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        lexicon.insert_work_entity("甲乙丙".to_string(), ProperNameKind::Person);
        lexicon.insert_work_entity_misspelling("甲乙丙", "甲乙丁".to_string());
        let config = CorrectionConfig {
            include_weak_hints: false,
            ..CorrectionConfig::default()
        };
        let diagnostics =
            CorrectionEngine::new(predictor(Vec::new()), lexicon, RuleSet::empty(), config)
                .expect("engine")
                .scan("甲乙丁说道。")
                .expect("scan");
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.rule_id == "proper.confirmed_misspelling"
                && diagnostic.replacement == "甲乙丙"
                && diagnostic.severity == "error"
        }));
    }

    #[test]
    fn rules_only_scans_do_not_require_a_model() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let diagnostics = CorrectionEngine::rules_only(
            lexicon,
            RuleSet::with_common_rules(),
            CorrectionConfig::default(),
        )
        .expect("engine")
        .scan("他说。。。")
        .expect("scan");
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.source == "rule"));
    }
}
