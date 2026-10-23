// SPDX-License-Identifier: Apache-2.0
// Modified behavioral port of pycorrector/pycorrector/corrector.py by XuMing
// (xuming624), especially Corrector.generate_items; no Python runtime/data is copied.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use crate::lexicon::Lexicon;
use crate::segment::{is_chinese_string, Segmenter};

// Candidate shapes follow pycorrector Corrector.generate_items (Apache-2.0).
// Pinyin equivalence is taken only from the supplied same_pinyin table; this
// module does not reproduce pypinyin or ship its upstream data.

/// How a candidate was produced. The decision layer must be able to tell an
/// author-registered whole-word relation apart from a statistical two-character
/// guess; they are not interchangeable evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CandidateOrigin {
    /// Both characters came from the confusable-character tables.
    DoubleCharConfusable,
    /// Exactly one character came from a confusable-character table.
    SingleCharConfusable,
    /// A complete known word with the same pinyin signature.
    SamePinyinWord,
    /// An explicit whole-word confusion relation.
    WordConfusion,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CorrectionCandidate {
    pub(crate) text: String,
    pub(crate) changed_chars: usize,
    /// Zero-based Unicode scalar offsets that differ from the source.
    pub(crate) changed_positions: Vec<usize>,
    pub(crate) origin: CandidateOrigin,
    /// Whether the candidate is a general-dictionary word. Being only a
    /// plausible pinyin match is not enough to replace prose.
    pub(crate) known_general_word: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CandidateGenerator;

impl CandidateGenerator {
    pub(crate) fn new() -> Self {
        Self
    }

    /// Return replacement candidates with their provenance. The corrector adds
    /// the original text as a protected scoring option; it never treats a
    /// candidate as an error by itself.
    pub(crate) fn generate(
        &self,
        word: &str,
        lexicon: &Lexicon,
        segmenter: &Segmenter,
    ) -> Vec<CorrectionCandidate> {
        let chars = word.chars().collect::<Vec<_>>();
        if chars.is_empty() || !is_chinese_string(word) {
            return Vec::new();
        }

        let mut candidates = BTreeMap::<String, CorrectionCandidate>::new();
        if let Some(target) = lexicon.confusion_target(word) {
            insert_if_candidate(
                &mut candidates,
                word,
                target.to_owned(),
                CandidateOrigin::WordConfusion,
                lexicon,
                segmenter,
            );
        }

        // Pycorrector first looks up known words whose pronunciation matches
        // the complete source term. Here that relation is table driven.
        for candidate in lexicon.word_candidates_by_pinyin(word) {
            if candidate != word && same_pinyin_word(word, candidate, lexicon) {
                insert_if_candidate(
                    &mut candidates,
                    word,
                    candidate.to_owned(),
                    CandidateOrigin::SamePinyinWord,
                    lexicon,
                    segmenter,
                );
            }
        }

        match chars.len() {
            1 => {
                for replacement in lexicon.confusable_chars(chars[0]) {
                    insert_if_candidate(
                        &mut candidates,
                        word,
                        replacement.to_string(),
                        CandidateOrigin::SingleCharConfusable,
                        lexicon,
                        segmenter,
                    );
                }
            }
            2 => {
                let first = lexicon.confusable_chars(chars[0]);
                let second = lexicon.confusable_chars(chars[1]);
                for replacement in &first {
                    insert_if_candidate(
                        &mut candidates,
                        word,
                        format!("{replacement}{}", chars[1]),
                        CandidateOrigin::SingleCharConfusable,
                        lexicon,
                        segmenter,
                    );
                }
                for replacement in &second {
                    insert_if_candidate(
                        &mut candidates,
                        word,
                        format!("{}{replacement}", chars[0]),
                        CandidateOrigin::SingleCharConfusable,
                        lexicon,
                        segmenter,
                    );
                }
                // Two simultaneous substitutions are exploratory guesses only.
                // The decision layer rejects them unless an explicit whole-word
                // relation exists.
                for left in &first {
                    for right in &second {
                        insert_if_candidate(
                            &mut candidates,
                            word,
                            format!("{left}{right}"),
                            CandidateOrigin::DoubleCharConfusable,
                            lexicon,
                            segmenter,
                        );
                    }
                }
            }
            _ => {
                // The upstream long-token templates change the middle char,
                // or replace the first/last word through same-pinyin lookup.
                for replacement in lexicon.confusable_chars(chars[1]) {
                    let mut candidate = chars.clone();
                    candidate[1] = replacement;
                    insert_if_candidate(
                        &mut candidates,
                        word,
                        candidate.into_iter().collect(),
                        CandidateOrigin::SingleCharConfusable,
                        lexicon,
                        segmenter,
                    );
                }

                let prefix = chars[..chars.len() - 1].iter().collect::<String>();
                for candidate in lexicon.word_candidates_by_pinyin(&prefix) {
                    if candidate != prefix && same_pinyin_word(&prefix, candidate, lexicon) {
                        insert_if_candidate(
                            &mut candidates,
                            word,
                            format!("{candidate}{}", chars[chars.len() - 1]),
                            CandidateOrigin::SamePinyinWord,
                            lexicon,
                            segmenter,
                        );
                    }
                }

                let suffix = chars[1..].iter().collect::<String>();
                for candidate in lexicon.word_candidates_by_pinyin(&suffix) {
                    if candidate != suffix && same_pinyin_word(&suffix, candidate, lexicon) {
                        insert_if_candidate(
                            &mut candidates,
                            word,
                            format!("{}{candidate}", chars[0]),
                            CandidateOrigin::SamePinyinWord,
                            lexicon,
                            segmenter,
                        );
                    }
                }
            }
        }

        let mut ordered = candidates.into_values().collect::<Vec<_>>();
        // Stronger provenance and fewer changes first; the local language model
        // still compares every surviving candidate against the original.
        ordered.sort_by(|left, right| {
            Reverse(left.origin)
                .cmp(&Reverse(right.origin))
                .then_with(|| left.changed_chars.cmp(&right.changed_chars))
                .then_with(|| left.text.cmp(&right.text))
        });
        ordered
    }
}

fn same_pinyin_word(left: &str, right: &str, lexicon: &Lexicon) -> bool {
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .all(|(left, right)| left == right || lexicon.same_pinyin(*left, *right))
        && left != right
}

fn insert_if_candidate(
    candidates: &mut BTreeMap<String, CorrectionCandidate>,
    original: &str,
    candidate: String,
    origin: CandidateOrigin,
    lexicon: &Lexicon,
    segmenter: &Segmenter,
) {
    if candidate == original || !is_chinese_string(&candidate) {
        return;
    }
    let (changed_chars, changed_positions) = changed_characters(original, &candidate);
    if changed_chars == 0 {
        return;
    }
    let known_general_word =
        lexicon.is_general_known_word(&candidate) || segmenter.has_word(&candidate);
    match candidates.get_mut(&candidate) {
        Some(existing) => {
            if origin > existing.origin {
                existing.origin = origin;
            }
        }
        None => {
            candidates.insert(
                candidate.clone(),
                CorrectionCandidate {
                    text: candidate,
                    changed_chars,
                    changed_positions,
                    origin,
                    known_general_word,
                },
            );
        }
    }
}

fn changed_characters(original: &str, candidate: &str) -> (usize, Vec<usize>) {
    let original = original.chars().collect::<Vec<_>>();
    let candidate = candidate.chars().collect::<Vec<_>>();
    let positions = (0..original.len().max(candidate.len()))
        .filter(|index| original.get(*index) != candidate.get(*index))
        .collect::<Vec<_>>();
    (positions.len(), positions)
}
