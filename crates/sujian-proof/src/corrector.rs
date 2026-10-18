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

// Behavioral port of pycorrector Corrector.correct/get_lm_correct_item
// (Apache-2.0). The original stays in every candidate set; log10 KenLM scores
// rank alternatives only, and confidence remains unset because scores are not
// calibrated probabilities.

pub(crate) struct TraditionalCorrector {
    scorer: Arc<dyn LanguageScorer>,
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
            scorer,
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
        let suspects = self.detector.detect(
            text,
            &self.lexicon,
            &self.segmenter,
            &self.confusion_matcher,
            self.scorer.as_ref(),
        )?;
        let mut diagnostics = Vec::new();

        for suspect in suspects {
            let source = match suspect.kind {
                DetectionKind::ConfusionWord => "confusion_word",
                DetectionKind::ProperName => "proper_name",
                DetectionKind::OutOfVocabularyWord => "oov",
                DetectionKind::Character => "character_ngram",
            };
            if let Some(replacement) = suspect.exact_replacement {
                if replacement != suspect.original {
                    diagnostics.push(CorrectionDiagnostic {
                        start_byte: suspect.start_byte,
                        end_byte: suspect.end_byte,
                        original: suspect.original,
                        replacement,
                        source: source.to_owned(),
                        rule_id: match suspect.kind {
                            DetectionKind::ConfusionWord => "confusion.custom".to_owned(),
                            DetectionKind::ProperName => "proper.similarity".to_owned(),
                            _ => "detector.exact".to_owned(),
                        },
                        severity: "error".to_owned(),
                        confidence: None,
                    });
                }
                continue;
            }

            let candidate_generator = CandidateGenerator::new(1);
            let mut candidates = candidate_generator.generate(&suspect.original, &self.lexicon);
            candidates.truncate(self.config.max_candidates_per_error);
            if candidates.is_empty() || self.config.max_candidates_per_error == 0 {
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

            // Include the unchanged source as a candidate and require every
            // proposed replacement to improve its sentence's model score.
            let original_score = self.score_text(segment_text)?;
            let mut ranked = Vec::with_capacity(candidates.len());
            for candidate in candidates {
                let mut alternative =
                    String::with_capacity(before.len() + candidate.len() + after.len());
                alternative.push_str(before);
                alternative.push_str(&candidate);
                alternative.push_str(after);
                let score = self.score_text(&alternative)?;
                if score > original_score {
                    ranked.push((candidate, score));
                }
            }
            ranked.sort_by(|left, right| {
                right
                    .1
                    .total_cmp(&left.1)
                    .then_with(|| left.0.cmp(&right.0))
            });

            let source_kind = source.to_owned();
            let rule_id = match suspect.kind {
                DetectionKind::OutOfVocabularyWord => "detector.oov",
                DetectionKind::Character => "detector.char_mad",
                _ => "detector.candidate",
            };
            for (replacement, _) in ranked
                .into_iter()
                .take(self.config.max_candidates_per_error)
            {
                diagnostics.push(CorrectionDiagnostic {
                    start_byte: suspect.start_byte,
                    end_byte: suspect.end_byte,
                    original: suspect.original.clone(),
                    replacement,
                    source: source_kind.clone(),
                    rule_id: rule_id.to_owned(),
                    severity: "error".to_owned(),
                    confidence: None,
                });
            }
        }

        if self.config.include_weak_hints {
            diagnostics.extend(self.rule_diagnostics(text));
        } else {
            diagnostics.extend(
                self.rule_diagnostics(text)
                    .into_iter()
                    .filter(|diagnostic| diagnostic.severity == "error"),
            );
        }
        Ok(filter_diagnostics(text, diagnostics)?)
    }

    fn score_text(&self, text: &str) -> Result<f64, CorrectionError> {
        let tokens = self.segmenter.char_tokens(text);
        self.scorer.score_tokens(&tokens)
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
        if same_span_alternative
            && filtered.iter().any(|existing: &CorrectionDiagnostic| {
                existing.start_byte == diagnostic.start_byte
                    && existing.end_byte == diagnostic.end_byte
                    && existing.replacement == diagnostic.replacement
            })
        {
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
