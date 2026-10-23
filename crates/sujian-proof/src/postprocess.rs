// SPDX-License-Identifier: GPL-3.0-only
//! Calibration and protection applied to raw character predictions.
//!
//! The neural model only supplies two log probabilities for each original Han
//! position: the probability of the character that is already there and the
//! probability of the best single-character alternative. It does not know
//! whether a span is protected by the author's work dictionary. This module
//! owns that decision. A calibrated logistic model converts the margin into a
//! probability; the threshold is part of the exported calibrator, not a
//! constant in the correction path.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::lexicon::Lexicon;
use crate::neural::PositionPrediction;
use crate::CorrectionError;

/// Exported logistic calibration parameters. The values are fitted offline on
/// public shibing624/CSC features plus constructed anonymous fiction/name
/// negatives, grouped by sentence into disjoint fit/calibration/final sets.
/// Changing the data or parameters requires a new calibrator version and
/// scanner fingerprint.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Calibrator {
    pub version: String,
    pub intercept: f32,
    pub margin_weight: f32,
    pub candidate_weight: f32,
    pub threshold: f32,
}

impl Calibrator {
    pub fn from_json(content: &str) -> Result<Self, CorrectionError> {
        let calibrator: Self = serde_json::from_str(content)
            .map_err(|error| CorrectionError::Model(format!("invalid calibrator: {error}")))?;
        if !calibrator.intercept.is_finite()
            || !calibrator.margin_weight.is_finite()
            || !calibrator.candidate_weight.is_finite()
            || !calibrator.threshold.is_finite()
        {
            return Err(CorrectionError::Model(
                "calibrator parameters must be finite".to_string(),
            ));
        }
        Ok(calibrator)
    }

    pub(crate) fn open(path: &Path) -> Result<Self, CorrectionError> {
        let content = std::fs::read_to_string(path).map_err(|error| {
            CorrectionError::Model(format!(
                "failed to read calibrator {}: {error}",
                path.display()
            ))
        })?;
        Self::from_json(&content)
    }

    /// Probability that the suggested replacement is a correction worth
    /// showing. This is a fitted probability, not a language-model score.
    pub(crate) fn probability(&self, prediction: &PositionPrediction) -> f32 {
        let margin = prediction.candidate_log_probability - prediction.original_log_probability;
        let logit = self.intercept
            + self.margin_weight * margin
            + self.candidate_weight * prediction.candidate_log_probability;
        if !logit.is_finite() {
            return 0.0;
        }
        1.0 / (1.0 + (-logit).exp())
    }

    pub(crate) fn accepts(&self, prediction: &PositionPrediction) -> bool {
        self.probability(prediction) >= self.threshold
    }
}

/// One validated model proposal in original UTF-8 coordinates.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AcceptedPrediction {
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
    pub(crate) original: String,
    pub(crate) replacement: String,
    pub(crate) probability: f32,
}

/// Merge model proposals with author-owned protection. Positions inside a
/// complete work-dictionary entry, an alias, an ignored span, or a registered
/// entity never become statistical suggestions. The original text is retained
/// in every diagnostic.
pub(crate) fn postprocess(
    text: &str,
    predictions: Vec<PositionPrediction>,
    lexicon: &Lexicon,
    calibrator: &Calibrator,
) -> Vec<AcceptedPrediction> {
    let protected = lexicon.protected_word_ranges(text);
    let mut accepted = BTreeMap::<usize, AcceptedPrediction>::new();
    for prediction in predictions {
        let Some(original) = text
            .get(prediction.start_byte..prediction.end_byte)
            .and_then(|value| value.chars().next())
        else {
            continue;
        };
        if original != prediction.original {
            continue;
        }
        if prediction.candidate == prediction.original {
            continue;
        }
        if protected
            .iter()
            .any(|(start, end)| prediction.start_byte < *end && *start < prediction.end_byte)
        {
            continue;
        }
        if lexicon.is_ignored(&prediction.original.to_string()) {
            continue;
        }
        if !calibrator.accepts(&prediction) {
            continue;
        }
        let replacement = prediction.candidate.to_string();
        if replacement == original.to_string() {
            continue;
        }
        accepted.insert(
            prediction.start_byte,
            AcceptedPrediction {
                start_byte: prediction.start_byte,
                end_byte: prediction.end_byte,
                original: original.to_string(),
                replacement,
                probability: calibrator.probability(&prediction),
            },
        );
    }
    accepted.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexicon::LexiconPaths;

    fn prediction(start_byte: usize, ch: char, candidate: char) -> PositionPrediction {
        PositionPrediction {
            start_byte,
            end_byte: start_byte + ch.len_utf8(),
            original: ch,
            candidate,
            original_log_probability: -2.0,
            candidate_log_probability: -0.1,
        }
    }

    fn calibrated() -> Calibrator {
        Calibrator {
            version: "test".to_string(),
            intercept: 0.0,
            margin_weight: 1.0,
            candidate_weight: 0.0,
            threshold: 0.5,
        }
    }

    #[test]
    fn protected_complete_name_is_not_changed() {
        let mut lexicon = Lexicon::from_paths(&LexiconPaths::default()).expect("lexicon");
        lexicon.insert_protected_word("甲乙".to_string());
        let accepted = postprocess(
            "甲乙走了。",
            vec![prediction(0, '甲', '假')],
            &lexicon,
            &calibrated(),
        );
        assert!(accepted.is_empty());
    }

    #[test]
    fn a_confident_unprotected_prediction_is_kept() {
        let lexicon = Lexicon::from_paths(&LexiconPaths::default()).expect("lexicon");
        let accepted = postprocess(
            "拔草。",
            vec![prediction(0, '拔', '拨')],
            &lexicon,
            &calibrated(),
        );
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].original, "拔");
        assert_eq!(accepted[0].replacement, "拨");
    }

    #[test]
    fn ambiguous_pronoun_is_rejected_by_the_calibrated_probability() {
        let lexicon = Lexicon::from_paths(&LexiconPaths::default()).expect("lexicon");
        let shipped =
            Calibrator::from_json(include_str!("../assets/correction-data/calibrator.json"))
                .expect("shipped calibrator");
        let accepted = postprocess(
            "他画了一幅画。",
            vec![prediction(0, '他', '她')],
            &lexicon,
            &shipped,
        );
        assert!(accepted.is_empty());
    }

    #[test]
    fn pronoun_is_not_hard_blocked_before_calibration() {
        let lexicon = Lexicon::from_paths(&LexiconPaths::default()).expect("lexicon");
        let accepted = postprocess(
            "他画了一幅画。",
            vec![prediction(0, '他', '她')],
            &lexicon,
            &calibrated(),
        );
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].replacement, "她");
    }

    #[test]
    fn low_margin_is_rejected_by_the_calibrator() {
        let lexicon = Lexicon::from_paths(&LexiconPaths::default()).expect("lexicon");
        let mut weak = prediction(0, '拔', '拨');
        weak.original_log_probability = -0.1;
        weak.candidate_log_probability = -0.2;
        assert!(postprocess("拔草。", vec![weak], &lexicon, &calibrated()).is_empty());
    }
}
