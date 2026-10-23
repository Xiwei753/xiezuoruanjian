// SPDX-License-Identifier: GPL-3.0-only
//! Candidate-conditioned source-error detection.
//!
//! Detection nominates spans; candidate generation proposes replacements. This
//! module answers the last question: given one source span and one candidate,
//! is there independent evidence that the *source* is wrong, rather than merely
//! that a plausible alternative exists?
//!
//! The score is a small linear gate fitted against corrected Chinese text and
//! typo/normal-prose checks. Character substitution counts come from an
//! offline, openly licensed spelling-correction corpus; the remaining features
//! are dictionary coverage, exact/near pronunciation, word-boundary confidence,
//! and one local KenLM comparison. Shape similarity and dictionary coverage
//! only generate candidates and do not by themselves pass this gate.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::candidates::CorrectionCandidate;
use crate::detector::DetectionEvidence;

/// Character substitution counts derived from shibing624/CSC (Apache-2.0),
/// dev split. Counts are evidence, not fixed replacement rules.
const ERROR_PAIR_DATA: &str = include_str!("../assets/correction-data/error_pairs.txt");

/// Fitted against corrected CSC sentences plus typo/normal target text. The
/// score is a logistic log-odds; each channel has its own conservative boundary.
const BIAS: f64 = -1.10;
const SOURCE_OOV: f64 = 0.95;
const CANDIDATE_KNOWN: f64 = 0.25;
const EXACT_PINYIN: f64 = 0.55;
const NEAR_PINYIN: f64 = 0.40;
const CONFUSION_LOG: f64 = 0.30;
const BOUNDARY_CONFIDENT: f64 = 0.20;
const BOUNDARY_AMBIGUOUS: f64 = -0.25;
const LOCAL_GAIN: f64 = 4.0;
const OOV_ACCEPT_SCORE: f64 = 2.0;
const EXACT_PINYIN_ACCEPT_SCORE: f64 = 0.5;
const BROADER_ACCEPT_SCORE: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceErrorChannel {
    ConfusionCount,
    ExactPinyin,
    NearPinyin,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourceErrorEvidence {
    /// Logistic score in 0..=1. It is an internal gate value, not a user-facing
    /// confidence, so diagnostics keep `confidence: None`.
    pub(crate) probability: f64,
    pub(crate) accept: bool,
    pub(crate) channel: SourceErrorChannel,
    pub(crate) confusion_count: u32,
    pub(crate) source_is_oov: bool,
}

pub(crate) struct ErrorDetector;

impl ErrorDetector {
    pub(crate) fn assess(
        source_span: &str,
        candidate: &CorrectionCandidate,
        evidence: &DetectionEvidence,
        original_score: f64,
        candidate_score: f64,
    ) -> SourceErrorEvidence {
        let confusion_count = confusion_count(source_span, candidate);
        let source_is_oov = !evidence.source_spelling_evidence.general_dictionary_word
            && !evidence.source_spelling_evidence.jieba_dictionary_word;
        let boundary_confident = evidence.segmentation.boundaries_agree
            && !evidence.segmentation.ambiguous_word
            && !evidence.segmentation.crosses_stable_word;

        let exact_pinyin = candidate.phonetic_relation;
        let near_pinyin = candidate.near_phonetic_relation && !exact_pinyin;
        // Shape-only candidates are generation hypotheses. They become
        // actionable only when the corpus confusion table also supports the
        // changed character, or when pronunciation evidence exists.
        let has_external_channel = confusion_count > 0 || exact_pinyin || near_pinyin;
        if !has_external_channel {
            return SourceErrorEvidence {
                probability: 0.0,
                accept: false,
                channel: SourceErrorChannel::NearPinyin,
                confusion_count,
                source_is_oov,
            };
        }

        let channel = if confusion_count > 0 {
            SourceErrorChannel::ConfusionCount
        } else if exact_pinyin {
            SourceErrorChannel::ExactPinyin
        } else {
            SourceErrorChannel::NearPinyin
        };

        let gain = if candidate_score.is_finite() && original_score.is_finite() {
            candidate_score - original_score
        } else {
            return SourceErrorEvidence {
                probability: 0.0,
                accept: false,
                channel,
                confusion_count,
                source_is_oov,
            };
        };

        let mut score = BIAS;
        if source_is_oov {
            score += SOURCE_OOV;
        }
        if candidate.known_general_word {
            score += CANDIDATE_KNOWN;
        }
        if exact_pinyin {
            score += EXACT_PINYIN;
        } else if near_pinyin {
            score += NEAR_PINYIN;
        }
        score += CONFUSION_LOG * (1.0 + confusion_count as f64).ln();
        score += if boundary_confident {
            BOUNDARY_CONFIDENT
        } else {
            BOUNDARY_AMBIGUOUS
        };
        score += LOCAL_GAIN * gain;
        let threshold = if source_is_oov {
            OOV_ACCEPT_SCORE
        } else if exact_pinyin {
            EXACT_PINYIN_ACCEPT_SCORE
        } else {
            BROADER_ACCEPT_SCORE
        };
        let probability = 1.0 / (1.0 + (threshold - score).exp());
        SourceErrorEvidence {
            probability,
            accept: score >= threshold,
            channel,
            confusion_count,
            source_is_oov,
        }
    }
}

pub(crate) fn confusion_count(source: &str, candidate: &CorrectionCandidate) -> u32 {
    let source_chars = source.chars().collect::<Vec<_>>();
    let candidate_chars = candidate.text.chars().collect::<Vec<_>>();
    candidate
        .changed_positions
        .iter()
        .filter_map(|position| {
            let source = source_chars.get(*position)?;
            let candidate = candidate_chars.get(*position)?;
            error_pairs().get(&(*source, *candidate)).copied()
        })
        .sum()
}

fn error_pairs() -> &'static HashMap<(char, char), u32> {
    static PAIRS: OnceLock<HashMap<(char, char), u32>> = OnceLock::new();
    PAIRS.get_or_init(|| {
        let mut pairs = HashMap::new();
        for line in ERROR_PAIR_DATA.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split('\t');
            let (Some(source), Some(target), Some(count)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let (Some(source), Some(target)) = (source.chars().next(), target.chars().next())
            else {
                continue;
            };
            let Ok(count) = count.parse::<u32>() else {
                continue;
            };
            pairs.insert((source, target), count);
        }
        pairs
    })
}
