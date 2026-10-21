// SPDX-License-Identifier: Apache-2.0
// Work-scoped proper-name protection.
//
// External person/place/proper-name files are candidate corpora, not proof
// that a name belongs to the current work. Only entries the author explicitly
// added to a project's work dictionary are confirmed entities. A near match to
// such an entity may be surfaced as a low-severity check, while ordinary words
// and unconfirmed variants are never force-replaced by a corpus name.

use crate::lexicon::Lexicon;
use crate::segment::{is_chinese_string, Segmenter, TokenSpan};

/// Maximum length considered for a work entity. Longer dictionary entries are
/// still protected as exact text but are not used for near-match checks.
const MAX_ENTITY_CHARS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProperNameKind {
    Person,
    Place,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkEntity {
    pub(crate) text: String,
    pub(crate) kind: ProperNameKind,
}

/// Author-confirmed full names from the current project's work dictionary.
#[derive(Clone, Debug, Default)]
pub(crate) struct WorkEntityLexicon {
    entities: Vec<WorkEntity>,
}

impl WorkEntityLexicon {
    pub(crate) fn insert(&mut self, text: String, kind: ProperNameKind) {
        if text.is_empty() || !is_chinese_string(&text) {
            return;
        }
        let char_len = text.chars().count();
        if !(2..=MAX_ENTITY_CHARS).contains(&char_len) {
            return;
        }
        if self.entities.iter().any(|entity| entity.text == text) {
            return;
        }
        self.entities.push(WorkEntity { text, kind });
    }

    pub(crate) fn entities(&self) -> &[WorkEntity] {
        &self.entities
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProperNameCandidate {
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
    pub(crate) original: String,
    pub(crate) replacement: String,
    pub(crate) kind: ProperNameKind,
}

/// Finds text spans that differ from one confirmed work entity by exactly one
/// table-supported confusable character.
///
/// The variant must not be a known word, an ignored word, or another protected
/// entity. When those conditions do not hold the function returns nothing: an
/// uncertain name is not an error.
pub(crate) fn detect_work_entity_variants(
    sentence: &str,
    lexicon: &Lexicon,
    segmenter: &Segmenter,
) -> Vec<ProperNameCandidate> {
    let entity_lexicon = lexicon.work_entity_lexicon();
    if entity_lexicon.is_empty() || !is_chinese_string(sentence) {
        return Vec::new();
    }

    let chars = segmenter.characters(sentence);
    if chars.len() < 2 {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    for entity in entity_lexicon.entities() {
        let expected = entity.text.chars().collect::<Vec<_>>();
        if expected.len() < 2 || expected.len() > chars.len() {
            continue;
        }
        for start in 0..=chars.len() - expected.len() {
            let window = &chars[start..start + expected.len()];
            let source = window
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>();
            if source == entity.text
                || lexicon.is_ignored(&source)
                || lexicon.is_protected_name_component(&source)
                || lexicon.is_protected_word_window(&source)
                || lexicon.is_known_word(&source)
                || segmenter.has_word(&source)
            {
                continue;
            }

            if !has_single_confusable_substitution(window, &expected, lexicon) {
                continue;
            }

            let local_start = window.first().map_or(0, |token| token.start_byte);
            let local_end = window.last().map_or(local_start, |token| token.end_byte);
            candidates.push(ProperNameCandidate {
                start_byte: local_start,
                end_byte: local_end,
                original: source,
                replacement: entity.text.clone(),
                kind: entity.kind,
            });
        }
    }

    candidates.sort_by_key(|candidate| (candidate.start_byte, candidate.end_byte));
    candidates.dedup_by(|left, right| {
        left.start_byte == right.start_byte
            && left.end_byte == right.end_byte
            && left.replacement == right.replacement
    });
    candidates
}

/// A near match is evidence only when exactly one character differs and that
/// difference is in the confusion table. Known homophones with two or more
/// differences are not attributable to the author-confirmed entity.
fn has_single_confusable_substitution(
    window: &[TokenSpan],
    expected: &[char],
    lexicon: &Lexicon,
) -> bool {
    let mut difference_found = false;
    for (token, expected_char) in window.iter().zip(expected.iter().copied()) {
        let Some(actual) = token.text.chars().next() else {
            return false;
        };
        if actual == expected_char {
            continue;
        }
        if difference_found || !lexicon.is_confusable(actual, expected_char) {
            return false;
        }
        difference_found = true;
    }
    difference_found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexicon::LexiconPaths;

    fn lexicon() -> Lexicon {
        Lexicon::from_paths(&LexiconPaths::default()).expect("embedded lexicon")
    }

    #[test]
    fn external_proper_names_alone_never_create_replacements() {
        let mut lexicon = lexicon();
        lexicon.insert_proper_name("祁门".to_string());
        let segmenter = Segmenter::new();
        assert!(detect_work_entity_variants("奇门", &lexicon, &segmenter).is_empty());
    }

    #[test]
    fn work_dictionary_full_entry_protects_only_the_full_name() {
        let mut lexicon = lexicon();
        lexicon.insert_protected_word("祁门".to_string());
        assert_eq!(lexicon.protected_word_ranges("祁门"), vec![(0, 6)]);
        assert!(lexicon.protected_word_ranges("奇门").is_empty());
    }

    #[test]
    fn valid_ordinary_words_are_never_flagged_as_entity_variants() {
        let mut lexicon = lexicon();
        lexicon.insert_protected_word("祁门".to_string());
        lexicon.insert_protected_word("周于".to_string());
        let segmenter = Segmenter::new();
        assert!(
            detect_work_entity_variants("奇门", &lexicon, &segmenter).is_empty(),
            "奇门 is a valid ordinary word"
        );
        assert!(
            detect_work_entity_variants("周围", &lexicon, &segmenter).is_empty(),
            "周围 is a valid ordinary word"
        );
    }

    #[test]
    fn novel_person_name_is_protected_as_a_full_entry() {
        let mut lexicon = lexicon();
        lexicon.insert_protected_word("宿泽柳".to_string());
        assert_eq!(lexicon.protected_word_ranges("宿泽柳说道"), vec![(0, 9)]);
        assert!(lexicon.protected_word_ranges("宿泽").is_empty());
        let segmenter = Segmenter::new();
        assert!(detect_work_entity_variants("宿泽柳", &lexicon, &segmenter).is_empty());
    }

    #[test]
    fn confirmed_work_entity_variant_is_only_a_check_candidate() {
        let mut lexicon = lexicon();
        lexicon.insert_protected_word("祁门客栈".to_string());
        let segmenter = Segmenter::new();
        let candidates = detect_work_entity_variants("奇门客栈", &lexicon, &segmenter);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].original, "奇门客栈");
        assert_eq!(candidates[0].replacement, "祁门客栈");
    }
}
