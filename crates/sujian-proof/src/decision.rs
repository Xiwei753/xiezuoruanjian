// SPDX-License-Identifier: GPL-3.0-only
//! Final, explainable arbitration between the original text and one candidate.
//!
//! Detection nominates suspicious spans. Candidate generation proposes text.
//! Neither stage may declare an error: this module combines dictionary
//! coverage, segmentation ambiguity, an explicit table relation, and a locally
//! measured language-model gain into one decision.
//!
//! KenLM anomaly (`char_outliers`) locates a span; it is not acceptance
//! evidence. A same-pinyin hit only expands the candidate set. Statistical
//! acceptance needs a spelling relation outside the model, such as an explicit
//! confusion entry or a complete word formed by a shape-near substitution, and
//! then exactly one local score comparison. The gate is a conservative,
//! explainable baseline, not a calibrated error probability; `confidence`
//! therefore stays unset.

use crate::candidates::{CandidateOrigin, CorrectionCandidate};
use crate::detector::DetectionEvidence;
use crate::segment::SegmentationEvidence;

/// The gate is deliberately not calibrated as an error probability. It only
/// requires that the candidate has model-external spelling evidence and that
/// its local normalized log10 score is strictly better than the original's.
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
        // Complete author-protected entries and confirmed stable dictionary
        // words are direct protection. Mere two-path disagreement is not.
        if segmentation.overlaps_protected || segmentation.crosses_stable_word {
            return CorrectionDecision::KeepOriginal;
        }
        // A registered whole-word confusion relation is authored data and may
        // change more than one character.
        if candidate.origin == CandidateOrigin::WordConfusion {
            return CorrectionDecision::Accept;
        }
        if candidate.changed_chars == 0 || candidate.changed_chars != 1 {
            // Multi-character statistical guesses are not a spelling relation.
            return CorrectionDecision::KeepOriginal;
        }
        if !candidate.known_general_word {
            return CorrectionDecision::KeepOriginal;
        }

        let source_len = source_span.chars().count();
        let source_is_known_word = segmentation.whole_word_known || evidence.source_is_general_word;
        // Single-character pronouns and other known characters need context
        // semantics that a character language model cannot supply.
        if source_len == 1 && source_is_known_word {
            return CorrectionDecision::KeepOriginal;
        }

        // A homophone is a candidate-generation channel, not proof that the
        // original spelling is wrong. Acceptance requires a model-external
        // relation: a complete known word formed by a shape-near substitution
        // at one of the positions detector flagged, or an explicit relation
        // handled above.
        let targets_orthographic_conflict = candidate.origin == CandidateOrigin::NearStrokeWord
            && candidate.orthographic_relation
            && candidate
                .changed_positions
                .iter()
                .any(|position| evidence.orthographic_conflicts.contains(position));
        if !targets_orthographic_conflict {
            return CorrectionDecision::KeepOriginal;
        }

        // Use exactly one model comparison. The earlier anomaly score selected
        // the span; it is not added here as another reason.
        if !candidate_score.is_finite() || !original_score.is_finite() {
            return CorrectionDecision::KeepOriginal;
        }
        if candidate_score <= original_score {
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
            phonetic_relation: true,
            orthographic_relation: false,
            known_general_word: true,
            rank: 0,
        }
    }

    fn evidence(
        lexicon: &Lexicon,
        segmenter: &Segmenter,
        sentence: &str,
        span: (usize, usize),
    ) -> DetectionEvidence {
        let analysis = segmenter.analyze_sentence(sentence, lexicon);
        DetectionEvidence {
            char_outliers: vec![(0, 2.0)],
            segmentation: segmenter.candidate_span_evidence(&analysis, span.0, span.1),
            ..DetectionEvidence::default()
        }
    }

    #[test]
    fn known_word_typo_with_shape_conflict_can_be_accepted() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let segmenter = Segmenter::new();
        let sentence = "他种了高梁。";
        let start = sentence.find('高').expect("start");
        let end = start + "高梁".len();
        let mut evidence = evidence(&lexicon, &segmenter, sentence, (start, end));
        evidence.source_is_general_word = true;
        evidence.orthographic_conflicts = vec![1];
        let mut candidate = candidate("高粱", CandidateOrigin::NearStrokeWord);
        candidate.orthographic_relation = true;
        assert_eq!(
            DecisionGate::evaluate(
                "高梁",
                &candidate,
                &evidence.segmentation,
                &evidence,
                0.0,
                0.05,
            ),
            CorrectionDecision::Accept
        );
    }

    #[test]
    fn same_pinyin_word_alone_is_not_spelling_evidence() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let segmenter = Segmenter::new();
        let sentence = "他按装了灯。";
        let start = sentence.find('按').expect("start");
        let end = start + "按装".len();
        let mut evidence = evidence(&lexicon, &segmenter, sentence, (start, end));
        evidence.oov_general_dictionary_miss = true;
        evidence.char_outliers.clear();
        assert_eq!(
            DecisionGate::evaluate(
                "按装",
                &candidate("安装", CandidateOrigin::SamePinyinWord),
                &evidence.segmentation,
                &evidence,
                0.0,
                0.05,
            ),
            CorrectionDecision::KeepOriginal
        );
    }

    #[test]
    fn unknown_oov_candidate_is_kept() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let segmenter = Segmenter::new();
        let sentence = "不再看宿禾";
        let start = sentence.find('看').expect("start");
        let end = start + "看宿".len();
        let mut evidence = evidence(&lexicon, &segmenter, sentence, (start, end));
        evidence.char_outliers = vec![(1, 3.0)];
        let mut candidate = candidate("看速", CandidateOrigin::SingleCharConfusable);
        candidate.known_general_word = false;
        assert_eq!(
            DecisionGate::evaluate(
                "看宿",
                &candidate,
                &evidence.segmentation,
                &evidence,
                0.0,
                0.05,
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
                0.05,
            ),
            CorrectionDecision::KeepOriginal
        );
    }

    #[test]
    fn multiple_character_change_requires_a_whole_word_relation() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let segmenter = Segmenter::new();
        let sentence = "裹臀布也早已不见踪影";
        let start = sentence.find('臀').expect("start");
        let end = start + "臀布".len();
        let evidence = evidence(&lexicon, &segmenter, sentence, (start, end));
        let mut candidate = candidate("胸部", CandidateOrigin::SingleCharConfusable);
        candidate.changed_chars = 2;
        candidate.changed_positions = vec![0, 1];
        assert_eq!(
            DecisionGate::evaluate(
                "臀布",
                &candidate,
                &evidence.segmentation,
                &evidence,
                0.0,
                0.05,
            ),
            CorrectionDecision::KeepOriginal
        );
    }
}
