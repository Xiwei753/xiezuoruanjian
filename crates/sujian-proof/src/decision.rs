// SPDX-License-Identifier: GPL-3.0-only
//! Final, explainable arbitration between the original text and one candidate.
//!
//! Detection nominates suspicious spans. Candidate generation proposes text.
//! Neither stage may declare an error: this module combines dictionary
//! coverage, segmentation ambiguity, character evidence, candidate provenance,
//! and a locally measured language-model gain into one decision.
//!
//! The thresholds are deliberately conservative and expressed as log10
//! probability per scored event. They are a precision-first baseline, not a
//! calibrated error probability; `confidence` therefore stays unset.

use crate::candidates::{CandidateOrigin, CorrectionCandidate};
use crate::detector::DetectionEvidence;
use crate::segment::SegmentationEvidence;

/// Thresholds are a precision-first baseline checked against the control
/// sentences with the bundled character model: 梁 in 高梁 measured 1.82,
/// 臀/布 in 裹臀布 measured 2.71/1.81, 看宿 and 磨过 stayed below 1.0, and
/// 高梁 → 高粱 gained about 0.27 log10 per event. They are not an error
/// probability and must be recalibrated if the model or tokenizer changes.
const MIN_LOG10_GAIN_PER_EVENT: f64 = 0.12;
/// A bare character has almost no context; require stronger evidence before
/// changing it.
const MIN_SINGLE_CHAR_LOG10_GAIN_PER_EVENT: f64 = 0.18;

/// A known word may use a weaker character anomaly; an unknown span must clear
/// the detector threshold plus one step, because compounds and names are common
/// in fiction.
const MIN_OUTLIER_STRENGTH_FOR_KNOWN_WORD: f64 = 1.7;
const MIN_OUTLIER_STRENGTH_FOR_UNKNOWN_SPAN: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CorrectionDecision {
    Accept,
    KeepOriginal,
}

pub(crate) struct DecisionGate;

impl DecisionGate {
    /// Decide whether a statistical candidate is worth showing. Exact,
    /// author-confirmed relations never reach this function.
    pub(crate) fn evaluate(
        source_span: &str,
        candidate: &CorrectionCandidate,
        segmentation: &SegmentationEvidence,
        evidence: &DetectionEvidence,
        original_score: f64,
        candidate_score: f64,
    ) -> CorrectionDecision {
        if evidence.exact_relation {
            return CorrectionDecision::Accept;
        }
        if segmentation.overlaps_protected {
            return CorrectionDecision::KeepOriginal;
        }
        // A registered word-level confusion relation is authored data. It may
        // still be safe when the statistical paths disagree about boundaries.
        if candidate.origin == CandidateOrigin::WordConfusion {
            return CorrectionDecision::Accept;
        }
        // Cutting a probable word in either segmentation path, or starting and
        // ending inside different paths' tokens, is ambiguity rather than
        // spelling evidence. Examples: 看|宿禾 and 裹|臀布.
        if segmentation.ambiguous_word
            || segmentation.crosses_stable_word
            || !segmentation.boundaries_agree
        {
            return CorrectionDecision::KeepOriginal;
        }
        // A complete dictionary word and an already-known general word are
        // legitimate prose. A single character (especially a pronoun) cannot be
        // changed by statistics at all; a longer known word needs both an
        // independent character anomaly and a stronger local gain.
        let source_len = source_span.chars().count();
        let source_is_known_word = segmentation.whole_word_known || evidence.source_is_general_word;
        if source_len == 1 && source_is_known_word {
            return CorrectionDecision::KeepOriginal;
        }
        if candidate.changed_chars == 0 || candidate.changed_chars > 1 {
            return CorrectionDecision::KeepOriginal;
        }
        // An unknown-string candidate is not a word; similarity alone is not a
        // replacement target.
        if !candidate.known_general_word {
            return CorrectionDecision::KeepOriginal;
        }
        // OOV membership and character rarity only nominate. There must be an
        // independent character anomaly at the position the candidate changes.
        let minimum_outlier_strength = if source_is_known_word {
            MIN_OUTLIER_STRENGTH_FOR_KNOWN_WORD
        } else {
            MIN_OUTLIER_STRENGTH_FOR_UNKNOWN_SPAN
        };
        let char_evidence_supports_change = candidate.changed_positions.iter().any(|position| {
            evidence
                .char_outliers
                .iter()
                .any(|(outlier_position, strength)| {
                    outlier_position == position && *strength >= minimum_outlier_strength
                })
        });
        if !char_evidence_supports_change {
            return CorrectionDecision::KeepOriginal;
        }

        let required_gain = if source_len == 1 {
            MIN_SINGLE_CHAR_LOG10_GAIN_PER_EVENT
        } else if source_is_known_word {
            // 高梁 → 高粱 is possible, but a valid two-character word gets a
            // higher bar than an unknown span so ordinary prose survives.
            MIN_SINGLE_CHAR_LOG10_GAIN_PER_EVENT
        } else {
            MIN_LOG10_GAIN_PER_EVENT
        };
        if candidate_score - original_score < required_gain {
            return CorrectionDecision::KeepOriginal;
        }
        CorrectionDecision::Accept
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexicon::Lexicon;
    use crate::segment::Segmenter;

    fn candidate(text: &str, origin: CandidateOrigin) -> CorrectionCandidate {
        CorrectionCandidate {
            text: text.to_owned(),
            changed_chars: 1,
            changed_positions: vec![1],
            origin,
            known_general_word: true,
        }
    }

    fn evidence(
        lexicon: &Lexicon,
        segmenter: &Segmenter,
        sentence: &str,
        span: (usize, usize),
    ) -> DetectionEvidence {
        DetectionEvidence {
            char_outliers: vec![(0, 2.0)],
            segmentation: segmenter.candidate_span_evidence(sentence, span.0, span.1, lexicon),
            ..DetectionEvidence::default()
        }
    }

    #[test]
    fn known_word_typo_with_character_evidence_can_be_accepted() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let segmenter = Segmenter::new();
        let sentence = "他种了高梁。";
        let start = sentence.find('高').expect("start");
        let end = start + "高梁".len();
        let mut evidence = evidence(&lexicon, &segmenter, sentence, (start, end));
        evidence.source_is_general_word = true;
        evidence.char_outliers = vec![(1, 1.82)];
        assert_eq!(
            DecisionGate::evaluate(
                "高梁",
                &candidate("高粱", CandidateOrigin::SingleCharConfusable),
                &evidence.segmentation,
                &evidence,
                0.0,
                9.0,
            ),
            CorrectionDecision::Accept
        );
    }

    #[test]
    fn single_path_word_is_ambiguous_and_kept() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let segmenter = Segmenter::new();
        let sentence = "不再看宿禾";
        let start = sentence.find('看').expect("start");
        let end = start + "看宿".len();
        let mut evidence = evidence(&lexicon, &segmenter, sentence, (start, end));
        evidence.char_outliers = vec![(1, 3.0)];
        assert_eq!(
            DecisionGate::evaluate(
                "看宿",
                &candidate("看速", CandidateOrigin::SingleCharConfusable),
                &evidence.segmentation,
                &evidence,
                0.0,
                9.0,
            ),
            CorrectionDecision::KeepOriginal
        );
    }

    #[test]
    fn known_valid_word_is_kept_even_with_a_better_candidate_score() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let segmenter = Segmenter::new();
        let sentence = "她捂住心脏";
        let start = sentence.find('她').expect("start");
        let end = start + '她'.len_utf8();
        let evidence = evidence(&lexicon, &segmenter, sentence, (start, end));
        assert_eq!(
            DecisionGate::evaluate(
                "她",
                &candidate("他", CandidateOrigin::SingleCharConfusable),
                &evidence.segmentation,
                &evidence,
                0.0,
                9.0,
            ),
            CorrectionDecision::KeepOriginal
        );
    }

    #[test]
    fn double_character_guess_is_never_accepted_without_a_word_relation() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let segmenter = Segmenter::new();
        let sentence = "裹臀布也早已不见踪影";
        let start = sentence.find('臀').expect("start");
        let end = start + "臀布".len();
        let evidence = evidence(&lexicon, &segmenter, sentence, (start, end));
        let mut candidate = candidate("胸部", CandidateOrigin::DoubleCharConfusable);
        candidate.changed_chars = 2;
        candidate.changed_positions = vec![0, 1];
        assert_eq!(
            DecisionGate::evaluate(
                "臀布",
                &candidate,
                &evidence.segmentation,
                &evidence,
                0.0,
                9.0,
            ),
            CorrectionDecision::KeepOriginal
        );
    }
}
