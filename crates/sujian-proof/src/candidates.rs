// SPDX-License-Identifier: Apache-2.0
// Modified behavioral port of pycorrector/pycorrector/corrector.py by XuMing
// (xuming624), especially Corrector.generate_items; no Python runtime/data is copied.

use std::collections::HashSet;

use crate::lexicon::Lexicon;
use crate::segment::is_chinese_string;

// Candidate shapes follow pycorrector Corrector.generate_items (Apache-2.0).
// Pinyin equivalence is taken only from the supplied same_pinyin table; this
// module does not reproduce pypinyin or ship its upstream data.

#[derive(Clone, Debug)]
pub(crate) struct CandidateGenerator {
    fragment: usize,
}

impl Default for CandidateGenerator {
    fn default() -> Self {
        Self { fragment: 1 }
    }
}

impl CandidateGenerator {
    pub(crate) fn new(fragment: usize) -> Self {
        Self {
            fragment: fragment.max(1),
        }
    }

    /// Return replacement candidates only. The corrector adds the original
    /// text as a protected scoring option, matching pycorrector's separate
    /// `get_lm_correct_item` stage.
    pub(crate) fn generate(&self, word: &str, lexicon: &Lexicon) -> Vec<String> {
        let chars = word.chars().collect::<Vec<_>>();
        if chars.is_empty() || !is_chinese_string(word) {
            return Vec::new();
        }

        let mut candidates = HashSet::<String>::new();
        if let Some(target) = lexicon.confusion_target(word) {
            insert_if_candidate(&mut candidates, word, target.to_owned());
        }

        // Pycorrector first looks up known words whose pronunciation matches
        // the complete source term. Here that relation is table driven.
        for candidate in lexicon.word_candidates_by_pinyin(word) {
            if candidate != word && same_pinyin_word(word, candidate, lexicon) {
                insert_if_candidate(&mut candidates, word, candidate.to_owned());
            }
        }

        match chars.len() {
            1 => {
                for replacement in lexicon.confusable_chars(chars[0]) {
                    insert_if_candidate(&mut candidates, word, replacement.to_string());
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
                    );
                }
                for replacement in &second {
                    insert_if_candidate(
                        &mut candidates,
                        word,
                        format!("{}{replacement}", chars[0]),
                    );
                }
                for left in &first {
                    for right in &second {
                        insert_if_candidate(&mut candidates, word, format!("{left}{right}"));
                    }
                }
            }
            _ => {
                // The upstream long-token templates change the middle char,
                // or replace the first/last word through same-pinyin lookup.
                for replacement in lexicon.confusable_chars(chars[1]) {
                    let mut candidate = chars.clone();
                    candidate[1] = replacement;
                    insert_if_candidate(&mut candidates, word, candidate.into_iter().collect());
                }

                let prefix = chars[..chars.len() - 1].iter().collect::<String>();
                for candidate in lexicon.word_candidates_by_pinyin(&prefix) {
                    if candidate != prefix && same_pinyin_word(&prefix, candidate, lexicon) {
                        insert_if_candidate(
                            &mut candidates,
                            word,
                            format!("{candidate}{}", chars[chars.len() - 1]),
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
                        );
                    }
                }
            }
        }

        let mut ordered = candidates.into_iter().collect::<Vec<_>>();
        ordered.sort_by(|left, right| {
            lexicon
                .word_frequency(right)
                .cmp(&lexicon.word_frequency(left))
                .then_with(|| left.cmp(right))
        });
        let keep = ordered.len() / self.fragment + usize::from(!ordered.is_empty());
        ordered.truncate(keep);
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

fn insert_if_candidate(candidates: &mut HashSet<String>, original: &str, candidate: String) {
    if candidate != original && is_chinese_string(&candidate) {
        candidates.insert(candidate);
    }
}
