// SPDX-License-Identifier: Apache-2.0
// Modified behavioral port of pycorrector/pycorrector/detector.py by XuMing
// (xuming624); this Rust implementation does not bundle upstream data files.

use crate::confusion::ConfusionMatcher;
use crate::lexicon::Lexicon;
use crate::lm::LanguageScorer;
use crate::segment::{is_chinese_string, is_han, Segmenter, TokenSpan};
use crate::CorrectionError;

// Behavioral port of pycorrector Detector._detect and _get_maybe_error_index
// (Apache-2.0): confusion terms, near proper names, dictionary OOV tokens,
// then character-level 2/3-gram MAD outliers. All returned offsets are UTF-8
// byte ranges; pycorrector's character indices are converted at the boundary.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DetectionKind {
    ConfusionWord,
    ProperName,
    OutOfVocabularyWord,
    Character,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Suspect {
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
    pub(crate) original: String,
    pub(crate) kind: DetectionKind,
    pub(crate) exact_replacement: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Detector {
    detect_words: bool,
    detect_characters: bool,
}

impl Default for Detector {
    fn default() -> Self {
        Self {
            detect_words: true,
            detect_characters: true,
        }
    }
}

impl Detector {
    pub(crate) fn detect(
        &self,
        text: &str,
        lexicon: &Lexicon,
        segmenter: &Segmenter,
        confusion_matcher: &ConfusionMatcher,
        scorer: &dyn LanguageScorer,
    ) -> Result<Vec<Suspect>, CorrectionError> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }

        let mut candidates = Vec::new();
        for segment in segmenter.sentence_segments(text) {
            let sentence = &text[segment.start_byte..segment.end_byte];

            for matched in confusion_matcher.scan_text(sentence) {
                if lexicon.is_ignored(&matched.original_text) {
                    continue;
                }
                candidates.push(Suspect {
                    start_byte: segment.start_byte + matched.start_byte,
                    end_byte: segment.start_byte + matched.end_byte,
                    original: matched.original_text,
                    kind: DetectionKind::ConfusionWord,
                    exact_replacement: Some(matched.suggestion),
                });
            }

            if !sentence.chars().any(is_han) {
                continue;
            }

            if is_chinese_string(sentence) {
                candidates.extend(detect_near_proper_names(
                    sentence,
                    segment.start_byte,
                    lexicon,
                    segmenter,
                ));
            }

            if self.detect_words {
                for token in segmenter.words(sentence) {
                    if !is_chinese_string(&token.text)
                        || lexicon.is_ignored(&token.text)
                        || lexicon.is_known(&token.text)
                    {
                        continue;
                    }
                    candidates.push(Suspect {
                        start_byte: segment.start_byte + token.start_byte,
                        end_byte: segment.start_byte + token.end_byte,
                        original: token.text,
                        kind: DetectionKind::OutOfVocabularyWord,
                        exact_replacement: None,
                    });
                }
            }

            if self.detect_characters {
                candidates.extend(detect_character_outliers(
                    sentence,
                    segment.start_byte,
                    segmenter,
                    lexicon,
                    scorer,
                )?);
            }
        }

        Ok(resolve_overlaps(candidates))
    }
}

fn detect_near_proper_names(
    sentence: &str,
    base_byte: usize,
    lexicon: &Lexicon,
    segmenter: &Segmenter,
) -> Vec<Suspect> {
    let chars = segmenter.characters(sentence);
    let mut matches = Vec::new();
    for len in 2..=4.min(chars.len()) {
        for start in 0..=chars.len() - len {
            let source_chars = &chars[start..start + len];
            let source = source_chars
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>();
            if lexicon.is_proper_name(&source) || lexicon.is_protected_name_component(&source) {
                continue;
            }

            let source_char_values = source_chars
                .iter()
                .filter_map(|token| token.text.chars().next())
                .collect::<Vec<_>>();
            let mut best: Option<(String, f32)> = None;
            for proper in lexicon.near_proper_name_candidates(&source_char_values) {
                let score = proper_name_similarity(source_chars, proper, lexicon);
                if score <= 0.85 || proper == source {
                    continue;
                }
                let replace = best.as_ref().map_or(true, |(best_name, best_score)| {
                    score > *best_score
                        || (score == *best_score
                            && (lexicon.word_frequency(proper) > lexicon.word_frequency(best_name)
                                || (lexicon.word_frequency(proper)
                                    == lexicon.word_frequency(best_name)
                                    && proper < best_name.as_str())))
                });
                if replace {
                    best = Some((proper.to_owned(), score));
                }
            }
            if let Some((replacement, _)) = best {
                let local_start = source_chars
                    .first()
                    .map(|token| token.start_byte)
                    .unwrap_or(0);
                let local_end = source_chars
                    .last()
                    .map(|token| token.end_byte)
                    .unwrap_or(local_start);
                matches.push(Suspect {
                    start_byte: base_byte + local_start,
                    end_byte: base_byte + local_end,
                    original: source,
                    kind: DetectionKind::ProperName,
                    exact_replacement: Some(replacement),
                });
            }
        }
    }
    matches
}

fn proper_name_similarity(source: &[TokenSpan], proper: &str, lexicon: &Lexicon) -> f32 {
    let expected = proper.chars().collect::<Vec<_>>();
    if expected.len() != source.len() {
        return 0.0;
    }
    let mut score = 0.0;
    let mut differences = 0usize;
    for (token, expected) in source.iter().zip(expected) {
        let actual = token.text.chars().next().unwrap_or_default();
        if actual == expected {
            score += 1.0;
        } else if lexicon.is_confusable(actual, expected) {
            differences += 1;
            // The upstream threshold is 0.85; the supplied tables only encode
            // a boolean relation, so allow only one table-supported change.
            score += 0.9;
        } else {
            return 0.0;
        }
    }
    if differences > 1 {
        return 0.0;
    }
    score / source.len() as f32
}

fn detect_character_outliers(
    sentence: &str,
    base_byte: usize,
    segmenter: &Segmenter,
    lexicon: &Lexicon,
    scorer: &dyn LanguageScorer,
) -> Result<Vec<Suspect>, CorrectionError> {
    let chars = segmenter.characters(sentence);
    if chars.len() < 2 {
        return Ok(Vec::new());
    }

    let mut combined = vec![0.0; chars.len()];
    let mut score_sets = 0usize;
    for n in [2, 3] {
        if chars.len() < n {
            continue;
        }
        let mut ngram_scores = Vec::with_capacity(chars.len() - n + 1);
        for start in 0..=chars.len() - n {
            let tokens = chars[start..start + n]
                .iter()
                .map(|token| token.text.as_str())
                .collect::<Vec<_>>();
            ngram_scores.push(scorer.score_tokens_without_boundaries(&tokens)?);
        }
        let aligned = align_window_scores(&ngram_scores, n, chars.len());
        for (position, score) in aligned.into_iter().enumerate() {
            combined[position] += score;
        }
        score_sets += 1;
    }
    if score_sets == 0 {
        return Ok(Vec::new());
    }
    for score in &mut combined {
        *score /= score_sets as f64;
    }

    let outliers = mad_outlier_indices(&combined, 0.6745, 2.0);
    let mut matches = Vec::new();
    for index in outliers {
        let token = &chars[index];
        if !is_chinese_string(&token.text) || lexicon.is_ignored(&token.text) {
            continue;
        }
        matches.push(Suspect {
            start_byte: base_byte + token.start_byte,
            end_byte: base_byte + token.end_byte,
            original: token.text.clone(),
            kind: DetectionKind::Character,
            exact_replacement: None,
        });
    }
    Ok(matches)
}

fn align_window_scores(scores: &[f64], n: usize, char_len: usize) -> Vec<f64> {
    if scores.is_empty() {
        return vec![0.0; char_len];
    }
    let mut padded = Vec::with_capacity(scores.len() + (2 * (n - 1)));
    padded.extend(std::iter::repeat(scores[0]).take(n - 1));
    padded.extend_from_slice(scores);
    padded.extend(std::iter::repeat(*scores.last().unwrap_or(&scores[0])).take(n - 1));
    (0..char_len)
        .map(|start| padded[start..start + n].iter().sum::<f64>() / n as f64)
        .collect()
}

fn mad_outlier_indices(scores: &[f64], ratio: f64, threshold: f64) -> Vec<usize> {
    if scores.is_empty() || scores.iter().any(|score| !score.is_finite()) {
        return Vec::new();
    }
    let score_median = median(scores);
    let deviations = scores
        .iter()
        .map(|score| (score - score_median).abs())
        .collect::<Vec<_>>();
    let mad = median(&deviations);
    if mad == 0.0 {
        return Vec::new();
    }
    scores
        .iter()
        .zip(deviations)
        .enumerate()
        .filter_map(|(index, (score, deviation))| {
            ((ratio * deviation / mad > threshold) && *score < score_median).then_some(index)
        })
        .collect()
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}

fn resolve_overlaps(mut candidates: Vec<Suspect>) -> Vec<Suspect> {
    candidates.sort_by(|left, right| {
        detection_priority(left.kind)
            .cmp(&detection_priority(right.kind))
            .then_with(|| {
                (right.end_byte - right.start_byte).cmp(&(left.end_byte - left.start_byte))
            })
            .then_with(|| left.start_byte.cmp(&right.start_byte))
            .then_with(|| left.original.cmp(&right.original))
    });
    let mut accepted = Vec::<Suspect>::new();
    for candidate in candidates {
        let overlaps = accepted.iter().any(|previous| {
            candidate.start_byte < previous.end_byte && previous.start_byte < candidate.end_byte
        });
        if !overlaps {
            accepted.push(candidate);
        }
    }
    accepted.sort_by(|left, right| {
        left.start_byte
            .cmp(&right.start_byte)
            .then_with(|| left.end_byte.cmp(&right.end_byte))
    });
    accepted
}

fn detection_priority(kind: DetectionKind) -> u8 {
    match kind {
        DetectionKind::ConfusionWord => 0,
        DetectionKind::ProperName => 1,
        DetectionKind::OutOfVocabularyWord => 2,
        DetectionKind::Character => 3,
    }
}
