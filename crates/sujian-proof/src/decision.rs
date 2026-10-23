// SPDX-License-Identifier: GPL-3.0-only
//! Final, explainable arbitration between the original text and one candidate.
//!
//! Detection nominates suspicious spans. Candidate generation proposes text.
//! [`crate::error_detection::ErrorDetector`] supplies candidate-conditioned,
//! model-external evidence that the source is misspelled. This gate only
//! applies structural protections and the candidate's final context rank; it
//! does not turn shape similarity or a dictionary alternative into an error.
//!
//! The gate is a conservative, explainable baseline, not a calibrated error
//! probability; `confidence` therefore stays unset.

use crate::candidates::{CandidateOrigin, CorrectionCandidate};
use crate::detector::DetectionEvidence;
use crate::error_detection::SourceErrorEvidence;
use crate::segment::SegmentationEvidence;

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
        source_error: &SourceErrorEvidence,
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
        if candidate.changed_chars != 1 || candidate.changed_positions.len() != 1 {
            // Multi-character statistical guesses are not a spelling relation.
            return CorrectionDecision::KeepOriginal;
        }
        if source_span.chars().count() > 2 {
            // Longer unregistered forms are usually names, places, or invented
            // terms. A character model plus a dictionary alternative is not
            // enough to call them misspellings; explicit relations above still
            // handle registered multi-character errors.
            return CorrectionDecision::KeepOriginal;
        }
        if !candidate.known_general_word {
            return CorrectionDecision::KeepOriginal;
        }

        // 的/得/地/着/了/过 are structural markers. A character model often
        // prefers a different marker, but "教得很好" and "睡着" are not
        // misspellings; changing the marker needs an explicit relation.
        const STRUCTURAL_MARKERS: [char; 6] = ['的', '得', '地', '着', '了', '过'];
        let changed_source = candidate
            .changed_positions
            .first()
            .and_then(|position| source_span.chars().nth(*position));
        if changed_source.is_some_and(|ch| STRUCTURAL_MARKERS.contains(&ch)) {
            return CorrectionDecision::KeepOriginal;
        }

        // Single-character pronouns and other known characters need context
        // semantics that a character language model cannot supply.
        if source_span.chars().count() == 1
            && evidence.source_spelling_evidence.jieba_dictionary_word
        {
            return CorrectionDecision::KeepOriginal;
        }

        // A span covered by the curated general dictionary is a valid complete
        // word. Shape similarity and a better local score cannot overturn it.
        if evidence.source_spelling_evidence.general_dictionary_word {
            return CorrectionDecision::KeepOriginal;
        }
        // Jieba also contains common valid words. A pronunciation alternative
        // alone cannot overturn them; a genuine shape relation can, because it
        // explains why the author typed the wrong character.
        if evidence.source_spelling_evidence.jieba_dictionary_word
            && !candidate.orthographic_relation
        {
            return CorrectionDecision::KeepOriginal;
        }

        if !source_error.accept || !source_error.probability.is_finite() {
            return CorrectionDecision::KeepOriginal;
        }
        CorrectionDecision::Accept
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error_detection::SourceErrorChannel;
    use crate::lexicon::Lexicon;
    use crate::segment::Segmenter;

    fn candidate(text: &str, origin: CandidateOrigin) -> CorrectionCandidate {
        CorrectionCandidate {
            text: text.to_owned(),
            changed_chars: 1,
            changed_positions: vec![1],
            origin,
            phonetic_relation: true,
            near_phonetic_relation: false,
            orthographic_relation: false,
            known_general_word: true,
            rank: 0,
        }
    }

    fn source_error(accept: bool) -> SourceErrorEvidence {
        SourceErrorEvidence {
            probability: if accept { 0.8 } else { 0.2 },
            accept,
            channel: SourceErrorChannel::ExactPinyin,
            confusion_count: 0,
            source_is_oov: true,
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
    fn statistical_error_evidence_with_complete_word_can_be_accepted() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let segmenter = Segmenter::new();
        let sentence = "他种了高梁。";
        let start = sentence.find('高').expect("start");
        let end = start + "高梁".len();
        let mut evidence = evidence(&lexicon, &segmenter, sentence, (start, end));
        evidence.source_spelling_evidence.jieba_dictionary_word = true;
        let mut candidate = candidate("高粱", CandidateOrigin::NearStrokeWord);
        candidate.orthographic_relation = true;
        assert_eq!(
            DecisionGate::evaluate(
                "高梁",
                &candidate,
                &evidence.segmentation,
                &evidence,
                &source_error(true),
            ),
            CorrectionDecision::Accept
        );
    }

    #[test]
    fn shape_alternative_without_source_error_is_kept() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let segmenter = Segmenter::new();
        let sentence = "他种了高梁。";
        let start = sentence.find('高').expect("start");
        let end = start + "高梁".len();
        let mut evidence = evidence(&lexicon, &segmenter, sentence, (start, end));
        evidence.shape_candidates = vec![1];
        let mut candidate = candidate("高粱", CandidateOrigin::NearStrokeWord);
        candidate.orthographic_relation = true;
        assert_eq!(
            DecisionGate::evaluate(
                "高梁",
                &candidate,
                &evidence.segmentation,
                &evidence,
                &source_error(false),
            ),
            CorrectionDecision::KeepOriginal
        );
    }

    #[test]
    fn general_dictionary_word_is_kept_even_with_error_evidence() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let segmenter = Segmenter::new();
        let sentence = "他看见桥梁。";
        let start = sentence.find('桥').expect("start");
        let end = start + "桥梁".len();
        let mut evidence = evidence(&lexicon, &segmenter, sentence, (start, end));
        evidence.source_spelling_evidence.general_dictionary_word = true;
        assert_eq!(
            DecisionGate::evaluate(
                "桥梁",
                &candidate("桥粱", CandidateOrigin::NearStrokeWord),
                &evidence.segmentation,
                &evidence,
                &source_error(true),
            ),
            CorrectionDecision::KeepOriginal
        );
    }

    #[test]
    fn jieba_word_needs_shape_evidence_to_be_overturned() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let segmenter = Segmenter::new();
        let sentence = "他种了高梁。";
        let start = sentence.find('高').expect("start");
        let end = start + "高梁".len();
        let mut evidence = evidence(&lexicon, &segmenter, sentence, (start, end));
        evidence.source_spelling_evidence.jieba_dictionary_word = true;
        assert_eq!(
            DecisionGate::evaluate(
                "高梁",
                &candidate("高粱", CandidateOrigin::SamePinyinWord),
                &evidence.segmentation,
                &evidence,
                &source_error(true),
            ),
            CorrectionDecision::KeepOriginal
        );
    }

    #[test]
    fn same_pinyin_word_without_source_error_is_kept() {
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
                &source_error(false),
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
                &source_error(true),
            ),
            CorrectionDecision::KeepOriginal
        );
    }

    #[test]
    fn known_single_character_is_kept() {
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
                &source_error(true),
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
                &source_error(true),
            ),
            CorrectionDecision::KeepOriginal
        );
    }
}
