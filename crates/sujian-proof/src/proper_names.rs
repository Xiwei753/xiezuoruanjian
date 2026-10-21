// SPDX-License-Identifier: Apache-2.0
// Work-scoped proper-name inventory.
//
// External person/place/proper-name files are candidate corpora, not proof
// that a name belongs to the current work. Only entries the author explicitly
// marked as a person or place are recorded here. They are protected as exact
// text. This module deliberately does not guess that a similar span is a typo:
// without reliable entity-usage evidence, a near match must produce no
// suggestion at all rather than a warning that the author has to dismiss.
// A known-entity consistency check is available only through an explicit
// author-confirmed alias or misspelling relation. There is no automatic
// similarity scan and no unknown-name recognition.

#![allow(dead_code)]

/// Upper bound for an author-confirmed work entity. This is only a sanity
/// limit for the inventory; exact protection handles arbitrary text.
const MAX_ENTITY_CHARS: usize = 64;

/// Whether an author-declared person or place is a complete work name that
/// the entity inventory can accept. Names may mix scripts and use punctuation
/// (`X-42`, `甲·乙`), so the only requirements are non-empty
/// content without control characters and a reasonable length.
pub fn is_valid_work_entity(text: &str) -> bool {
    !text.trim().is_empty()
        && !text.chars().any(char::is_control)
        && text.chars().count() <= MAX_ENTITY_CHARS
}

/// Explicit author-confirmed type of a work-dictionary entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProperNameKind {
    Person,
    Place,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkEntity {
    pub(crate) text: String,
    pub(crate) kind: ProperNameKind,
}

/// Author-confirmed exact spelling relation for one work entity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConfirmedEntityMisspelling {
    pub(crate) original: String,
    pub(crate) replacement: String,
}

/// Author-confirmed full names from the current project's work dictionary.
///
/// Ordinary work-dictionary terms are intentionally absent. They are still
/// protected by [`crate::lexicon::Lexicon::insert_protected_word`], but they
/// can never become the replacement target for another span. Exact aliases and
/// explicitly marked misspellings are authored relations, not similarity
/// guesses.
#[derive(Clone, Debug, Default)]
pub(crate) struct WorkEntityLexicon {
    entities: Vec<WorkEntity>,
    aliases: Vec<String>,
    misspellings: Vec<ConfirmedEntityMisspelling>,
}

impl WorkEntityLexicon {
    pub(crate) fn insert(&mut self, text: String, kind: ProperNameKind) {
        if !is_valid_work_entity(&text) {
            return;
        }
        if self.entities.iter().any(|entity| entity.text == text) {
            return;
        }
        self.entities.push(WorkEntity { text, kind });
    }

    pub(crate) fn insert_alias(&mut self, canonical: &str, alias: String) -> bool {
        if !is_valid_work_entity(&alias)
            || alias == canonical
            || self.aliases.iter().any(|existing| existing == &alias)
            || self.entities.iter().any(|entity| entity.text == alias)
        {
            return false;
        }
        if !self.entities.iter().any(|entity| entity.text == canonical) {
            return false;
        }
        self.aliases.push(alias);
        true
    }

    pub(crate) fn insert_misspelling(&mut self, canonical: &str, original: String) -> bool {
        if !is_valid_work_entity(&original)
            || original == canonical
            || self
                .misspellings
                .iter()
                .any(|existing| existing.original == original)
            || self.aliases.iter().any(|alias| alias == &original)
            || self.entities.iter().any(|entity| entity.text == original)
        {
            return false;
        }
        if !self.entities.iter().any(|entity| entity.text == canonical) {
            return false;
        }
        self.misspellings.push(ConfirmedEntityMisspelling {
            original,
            replacement: canonical.to_string(),
        });
        true
    }

    pub(crate) fn entities(&self) -> &[WorkEntity] {
        &self.entities
    }

    pub(crate) fn aliases(&self) -> &[String] {
        &self.aliases
    }

    /// Exact wrong-spelling relations. `original` is not protected text; the
    /// replacement is an author-confirmed canonical name.
    pub(crate) fn misspellings(&self) -> &[ConfirmedEntityMisspelling] {
        &self.misspellings
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entities.is_empty() && self.aliases.is_empty() && self.misspellings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_script_and_punctuated_names_are_recorded() {
        let mut lexicon = WorkEntityLexicon::default();
        lexicon.insert("甲乙丙".to_string(), ProperNameKind::Person);
        lexicon.insert("甲地乙坊".to_string(), ProperNameKind::Place);
        lexicon.insert("甲·乙".to_string(), ProperNameKind::Person);
        lexicon.insert("X-42".to_string(), ProperNameKind::Person);
        lexicon.insert("乙丙研究所".to_string(), ProperNameKind::Place);
        let texts = lexicon
            .entities()
            .iter()
            .map(|entity| entity.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            vec!["甲乙丙", "甲地乙坊", "甲·乙", "X-42", "乙丙研究所"]
        );
    }

    #[test]
    fn duplicate_entries_keep_the_first_confirmed_type() {
        let mut lexicon = WorkEntityLexicon::default();
        lexicon.insert("甲乙丙".to_string(), ProperNameKind::Person);
        lexicon.insert("甲乙丙".to_string(), ProperNameKind::Place);
        assert_eq!(lexicon.entities().len(), 1);
        assert_eq!(lexicon.entities()[0].kind, ProperNameKind::Person);
    }

    #[test]
    fn aliases_and_explicit_misspellings_require_a_registered_canonical() {
        let mut lexicon = WorkEntityLexicon::default();
        assert!(!lexicon.insert_alias("甲乙丙", "乙丙".to_string()));
        assert!(!lexicon.insert_misspelling("甲乙丙", "甲乙丁".to_string()));

        lexicon.insert("甲乙丙".to_string(), ProperNameKind::Person);
        assert!(lexicon.insert_alias("甲乙丙", "乙丙".to_string()));
        assert!(lexicon.insert_misspelling("甲乙丙", "甲乙丁".to_string()));
        assert_eq!(lexicon.aliases(), ["乙丙"]);
        assert_eq!(lexicon.misspellings().len(), 1);
        assert_eq!(lexicon.misspellings()[0].original, "甲乙丁");
        assert_eq!(lexicon.misspellings()[0].replacement, "甲乙丙");

        // A canonical spelling cannot also be its own alias, and a wrong
        // spelling cannot silently become another registered entity.
        assert!(!lexicon.insert_alias("甲乙丙", "甲乙丙".to_string()));
        assert!(!lexicon.insert_misspelling("甲乙丙", "乙丙".to_string()));
    }

    #[test]
    fn empty_control_or_long_entries_are_ignored() {
        let mut lexicon = WorkEntityLexicon::default();
        lexicon.insert(String::new(), ProperNameKind::Person);
        lexicon.insert("   ".to_string(), ProperNameKind::Place);
        lexicon.insert("甲\n乙".to_string(), ProperNameKind::Person);
        lexicon.insert("很".repeat(MAX_ENTITY_CHARS + 1), ProperNameKind::Place);
        assert!(lexicon.is_empty());
    }
}
