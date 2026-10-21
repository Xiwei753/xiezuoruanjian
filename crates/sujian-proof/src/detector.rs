// SPDX-License-Identifier: Apache-2.0
// Modified behavioral port of pycorrector/pycorrector/detector.py by XuMing
// (xuming624); this Rust implementation does not bundle upstream data files.

use crate::confusion::ConfusionMatcher;
use crate::lexicon::Lexicon;
use crate::lm::LanguageScorer;
use crate::segment::{is_chinese_string, is_han, Segmenter};
use crate::CorrectionError;

// Behavioral port of pycorrector Detector._detect and _get_maybe_error_index
// (Apache-2.0): confusion terms, dictionary OOV tokens,
// then character-level 2/3-gram MAD outliers. All returned offsets are UTF-8
// byte ranges; pycorrector's character indices are converted at the boundary.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DetectionKind {
    ConfirmedEntitySpelling,
    ConfusionWord,
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
        let mut protected_ranges = Vec::new();
        let mut protected_entries = Vec::new();
        for segment in segmenter.sentence_segments(text) {
            let sentence = &text[segment.start_byte..segment.end_byte];
            protected_ranges.extend(
                lexicon
                    .protected_word_ranges(sentence)
                    .into_iter()
                    .map(|(start, end)| (segment.start_byte + start, segment.start_byte + end)),
            );
            protected_entries.extend(
                lexicon
                    .protected_word_ranges_unmerged(sentence)
                    .into_iter()
                    .map(|(start, end)| (segment.start_byte + start, segment.start_byte + end)),
            );

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

            // Exact, author-confirmed wrong spellings only. There is no
            // similarity scan over registered names.
            for relation in lexicon.work_entity_lexicon().misspellings() {
                for (start, end) in exact_name_occurrences(sentence, &relation.original) {
                    candidates.push(Suspect {
                        start_byte: segment.start_byte + start,
                        end_byte: segment.start_byte + end,
                        original: relation.original.clone(),
                        kind: DetectionKind::ConfirmedEntitySpelling,
                        exact_replacement: Some(relation.replacement.clone()),
                    });
                }
            }

            if !sentence.chars().any(is_han) {
                continue;
            }

            if self.detect_words {
                for token in segmenter.words(sentence) {
                    if !is_chinese_string(&token.text)
                        || lexicon.is_ignored(&token.text)
                        || lexicon.is_oov_exception(&token.text)
                        || segmenter.has_word(&token.text)
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

        candidates.retain(|candidate| {
            let overlaps_protected = protected_ranges
                .iter()
                .any(|(start, end)| candidate.start_byte < *end && *start < candidate.end_byte);
            if !overlaps_protected {
                return true;
            }
            // An explicitly registered misspelling may contain a protected
            // canonical name as a prefix. Only drop it when a single complete
            // protected entry actually wraps the whole confirmed spelling.
            if candidate.kind != DetectionKind::ConfirmedEntitySpelling {
                return false;
            }
            !protected_entries.iter().any(|(start, end)| {
                *start <= candidate.start_byte && candidate.end_byte <= *end
            })
        });
        Ok(resolve_overlaps(candidates))
    }
}

/// Exact byte occurrences of an author-confirmed wrong spelling. ASCII edges
/// do not match inside a longer Latin identifier (`X-42` in `ZX-42B`).
pub(crate) fn exact_name_occurrences(text: &str, name: &str) -> Vec<(usize, usize)> {
    if name.is_empty() {
        return Vec::new();
    }
    let mut occurrences = Vec::new();
    let mut search_from = 0usize;
    while let Some(relative) = text[search_from..].find(name) {
        let start = search_from + relative;
        let end = start + name.len();
        let left_ok = !name.chars().next().is_some_and(is_ascii_name_char)
            || text[..start]
                .chars()
                .next_back()
                .is_none_or(|previous| !is_ascii_name_char(previous));
        let right_ok = !name.chars().next_back().is_some_and(is_ascii_name_char)
            || text[end..]
                .chars()
                .next()
                .is_none_or(|next| !is_ascii_name_char(next));
        // For mixed-script names, only treat the edge as an ASCII identifier
        // when that edge character itself participates in an ASCII token.
        let left_ok = left_ok || !name.chars().next().is_some_and(|first| first.is_ascii());
        let right_ok = right_ok || !name.chars().next_back().is_some_and(|last| last.is_ascii());
        if left_ok && right_ok {
            occurrences.push((start, end));
        }
        search_from = end.max(start + 1);
    }
    occurrences
}

fn is_ascii_name_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || value == '_'
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
        DetectionKind::ConfirmedEntitySpelling => 0,
        DetectionKind::ConfusionWord => 1,
        DetectionKind::OutOfVocabularyWord => 2,
        DetectionKind::Character => 3,
    }
}
