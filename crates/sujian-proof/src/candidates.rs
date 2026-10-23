// SPDX-License-Identifier: Apache-2.0
// Modified behavioral port of pycorrector/pycorrector/corrector.py by XuMing
// (xuming624), especially Corrector.generate_items; no Python runtime/data is copied.

use std::collections::BTreeMap;

use crate::lexicon::Lexicon;
use crate::segment::{is_chinese_string, Segmenter};

// Candidate shapes follow pycorrector Corrector.generate_items (Apache-2.0).
// Pinyin equivalence is taken only from the supplied same_pinyin table; this
// module does not reproduce pypinyin or ship its upstream data.

/// How a candidate was produced. The decision layer must be able to tell an
/// author-registered whole-word relation apart from a statistical guess; they
/// are not interchangeable evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CandidateOrigin {
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
    /// Whether every changed character has a same-pinyin relation to the
    /// source character. This is table evidence outside the language model.
    pub(crate) phonetic_relation: bool,
    /// Whether the candidate is a general-dictionary word. Being only a
    /// plausible pinyin match is not enough to replace prose.
    pub(crate) known_general_word: bool,
    /// Stable generation order, used to bound model scoring without sorting
    /// candidates by their text.
    pub(crate) rank: usize,
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
        let mut next_rank = 0usize;
        if let Some(target) = lexicon.confusion_target(word) {
            insert_if_candidate(
                &mut candidates,
                word,
                target.to_owned(),
                CandidateOrigin::WordConfusion,
                lexicon,
                segmenter,
                &mut next_rank,
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
                    &mut next_rank,
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
                        &mut next_rank,
                    );
                }
            }
            2 => {
                // Only one character may change. Two simultaneous confusable
                // substitutions are not a relation and must not be generated:
                // they are neither a registered word nor independent evidence.
                for replacement in lexicon.confusable_chars(chars[0]) {
                    insert_if_candidate(
                        &mut candidates,
                        word,
                        format!("{replacement}{}", chars[1]),
                        CandidateOrigin::SingleCharConfusable,
                        lexicon,
                        segmenter,
                        &mut next_rank,
                    );
                }
                for replacement in lexicon.confusable_chars(chars[1]) {
                    insert_if_candidate(
                        &mut candidates,
                        word,
                        format!("{}{replacement}", chars[0]),
                        CandidateOrigin::SingleCharConfusable,
                        lexicon,
                        segmenter,
                        &mut next_rank,
                    );
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
                        &mut next_rank,
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
                            &mut next_rank,
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
                            &mut next_rank,
                        );
                    }
                }
            }
        }

        let mut ordered = candidates.into_values().collect::<Vec<_>>();
        // Stronger provenance and fewer changes first. Generation rank breaks
        // ties without using the candidate's text, so a real correction is not
        // lost to lexicographic truncation.
        ordered.sort_by(|left, right| {
            right
                .origin
                .cmp(&left.origin)
                .then_with(|| left.changed_chars.cmp(&right.changed_chars))
                .then_with(|| left.rank.cmp(&right.rank))
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
    next_rank: &mut usize,
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
    let phonetic_relation = same_pinyin_word(original, &candidate, lexicon);
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
                    phonetic_relation,
                    known_general_word,
                    rank: *next_rank,
                },
            );
            *next_rank += 1;
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
