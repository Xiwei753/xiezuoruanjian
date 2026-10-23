// SPDX-License-Identifier: GPL-3.0-only
//! Final, explainable arbitration between the original text and one candidate.
//!
//! Detection nominates suspicious spans. Candidate generation proposes text.
//! Neither stage may declare an error. KenLM anomaly (`char_outliers`) locates
//! a span; shape and pinyin relations only enlarge the candidate set. A
//! statistical suggestion is accepted only when the detector found a
//! model-external source-error channel: a directional character relation
//! attested by an explicit project typo entry, and the candidate implements
//! that exact relation in a complete dictionary word. The local language-model
//! comparison is then used once, as context confirmation. It is never combined
//! with itself as a second independent reason.
//!
//! The gate is a conservative, explainable baseline, not a calibrated error
//! probability; `confidence` therefore stays unset.

use crate::candidates::{CandidateOrigin, CorrectionCandidate};
use crate::detector::DetectionEvidence;
use crate::segment::SegmentationEvidence;

/// The gate is deliberately not calibrated as an error probability. It only
/// accepts a known complete-word candidate whose changed character is the one
/// attested by an explicit typo relation, and whose local normalized log10
/// score is strictly better than the original's.
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
        if candidate.changed_chars != 1 || candidate.changed_positions.len() != 1 {
            // Multi-character statistical guesses are not a spelling relation.
            return CorrectionDecision::KeepOriginal;
        }
        if !candidate.known_general_word {
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
        // Jieba's runtime dictionary alone is not treated as author-confirmed
        // validity: it also contains corpus artifacts (高梁 is one such entry).
        if evidence.source_spelling_evidence.general_dictionary_word {
            return CorrectionDecision::KeepOriginal;
        }

        // The only statistical source-error channel is an explicit typo
        // relation such as the 梁 -> 粱 direction attested by
        // 黄梁美梦 -> 黄粱美梦. Shape similarity and dictionary coverage of
        // the target are candidate-generation facts, not source-error proof.
        let Some(position) = candidate.changed_positions.first().copied() else {
            return CorrectionDecision::KeepOriginal;
        };
        let Some(replacement) = candidate.text.chars().nth(position) else {
            return CorrectionDecision::KeepOriginal;
        };
        let attested = evidence
            .source_spelling_evidence
            .attested_error_pairs
            .iter()
            .any(|(attested_position, attested_replacement)| {
                *attested_position == position && *attested_replacement == replacement
            });
        if !attested {
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
    fn attested_typo_relation_with_complete_word_can_be_accepted() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let segmenter = Segmenter::new();
        let sentence = "他种了高梁。";
        let start = sentence.find('高').expect("start");
        let end = start + "高梁".len();
        let mut evidence = evidence(&lexicon, &segmenter, sentence, (start, end));
        evidence.source_spelling_evidence.jieba_dictionary_word = true;
        evidence.source_spelling_evidence.attested_error_pairs = vec![(1, '粱')];
        assert_eq!(
            DecisionGate::evaluate(
                "高梁",
                &candidate("高粱", CandidateOrigin::AttestedSpelling),
                &evidence.segmentation,
                &evidence,
                0.0,
                0.05,
            ),
            CorrectionDecision::Accept
        );
    }

    #[test]
    fn shape_alternative_alone_is_not_source_error_evidence() {
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
                0.0,
                0.05,
            ),
            CorrectionDecision::KeepOriginal
        );
    }

    #[test]
    fn general_dictionary_word_is_kept_even_with_an_attested_pair() {
        let lexicon = Lexicon::from_paths(&crate::LexiconPaths::default()).expect("lexicon");
        let segmenter = Segmenter::new();
        let sentence = "他看见桥梁。";
        let start = sentence.find('桥').expect("start");
        let end = start + "桥梁".len();
        let mut evidence = evidence(&lexicon, &segmenter, sentence, (start, end));
        evidence.source_spelling_evidence.general_dictionary_word = true;
        evidence.source_spelling_evidence.attested_error_pairs = vec![(1, '粱')];
        assert_eq!(
            DecisionGate::evaluate(
                "桥梁",
                &candidate("桥粱", CandidateOrigin::NearStrokeWord),
                &evidence.segmentation,
                &evidence,
                0.0,
                0.05,
            ),
            CorrectionDecision::KeepOriginal
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
