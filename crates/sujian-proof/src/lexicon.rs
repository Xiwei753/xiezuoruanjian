// SPDX-License-Identifier: Apache-2.0
//! Author-owned vocabulary and exact replacement relations.
//!
//! This module deliberately contains no pinyin, shape, frequency, or language
//! model. Those were the second semantic judge of the old detector. Here the
//! lexicon only answers two questions:
//!
//! * is this complete span protected by the author (work dictionary, alias,
//!   ignored phrase, user word)?
//! * does the author explicitly map this exact written form to another one?
//!
//! Complete span protection is stronger than "the word is uncommon". An
//! unregistered fictional name is not protected merely because a dictionary
//! does not contain it; the neural proposal still has to pass calibration.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use crate::proper_names::{ProperNameKind, WorkEntityLexicon};

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct LexiconPaths {
    pub word_freq: Option<PathBuf>,
    pub custom_confusion: Option<PathBuf>,
    pub person_name: Option<PathBuf>,
    pub place_name: Option<PathBuf>,
    pub proper_name: Option<PathBuf>,
    pub ignored_words: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub struct Lexicon {
    base: Option<Arc<Lexicon>>,
    word_freq: HashSet<String>,
    custom_confusion: Vec<(String, String)>,
    person_names: HashSet<String>,
    place_names: HashSet<String>,
    proper_names: HashSet<String>,
    ignored_words: HashSet<String>,
    protected_words: HashSet<String>,
    protected_word_index: ProtectedWordTrie,
    work_entity_lexicon: WorkEntityLexicon,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct LexiconBaseCacheKey(Vec<(Option<PathBuf>, Option<u64>, Option<SystemTime>)>);

static LEXICON_BASE_CACHE: OnceLock<Mutex<Option<(LexiconBaseCacheKey, Arc<Lexicon>)>>> =
    OnceLock::new();

fn lexicon_base_cache_key(paths: &LexiconPaths) -> io::Result<LexiconBaseCacheKey> {
    let paths = [
        paths.word_freq.as_ref(),
        paths.custom_confusion.as_ref(),
        paths.person_name.as_ref(),
        paths.place_name.as_ref(),
        paths.proper_name.as_ref(),
        paths.ignored_words.as_ref(),
    ];
    let mut key = Vec::with_capacity(paths.len());
    for path in paths {
        let Some(path) = path else {
            key.push((None, None, None));
            continue;
        };
        match std::fs::metadata(path) {
            Ok(metadata) => key.push((
                Some(path.clone()),
                Some(metadata.len()),
                metadata.modified().ok(),
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                key.push((Some(path.clone()), None, None));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(LexiconBaseCacheKey(key))
}

#[derive(Clone, Debug, Default)]
struct ProtectedWordTrie {
    children: std::collections::HashMap<char, ProtectedWordTrie>,
    terminal: bool,
}

impl Lexicon {
    pub fn from_paths(paths: &LexiconPaths) -> io::Result<Self> {
        let key = lexicon_base_cache_key(paths)?;
        let cache = LEXICON_BASE_CACHE.get_or_init(|| Mutex::new(None));
        let mut cache = cache
            .lock()
            .map_err(|_| io::Error::other("correction lexicon base cache lock poisoned"))?;
        let base = if let Some((cached_key, base)) = cache.as_ref() {
            if *cached_key == key {
                Arc::clone(base)
            } else {
                let mut base = Self::from_embedded()?;
                base.load_paths(paths)?;
                let base = Arc::new(base);
                *cache = Some((key, Arc::clone(&base)));
                base
            }
        } else {
            let mut base = Self::from_embedded()?;
            base.load_paths(paths)?;
            let base = Arc::new(base);
            *cache = Some((key, Arc::clone(&base)));
            base
        };
        Ok(Self {
            base: Some(base),
            ..Self::default()
        })
    }

    fn load_paths(&mut self, paths: &LexiconPaths) -> io::Result<()> {
        if let Some(path) = &paths.word_freq {
            for (word, _) in read_word_freq(path)? {
                self.insert_word_frequency(word);
            }
        }
        if let Some(path) = &paths.custom_confusion {
            for (wrong, correct, _) in read_custom_confusion(path)? {
                self.insert_confusion(wrong, correct);
            }
        }
        for (path, kind) in [
            (paths.person_name.as_ref(), NameKind::Person),
            (paths.place_name.as_ref(), NameKind::Place),
        ] {
            if let Some(path) = path {
                for (word, _) in read_word_freq(path)? {
                    self.insert_named_span(word, kind);
                }
            }
        }
        if let Some(path) = &paths.proper_name {
            for name in read_word_set(path)? {
                self.insert_proper_name(name);
            }
        }
        if let Some(path) = &paths.ignored_words {
            for word in read_word_set(path)? {
                self.insert_ignored_word(word);
            }
        }
        Ok(())
    }

    fn from_embedded() -> io::Result<Self> {
        let mut lexicon = Self::default();
        for (wrong, correct, _) in read_custom_confusion_from(
            Cursor::new(include_str!(
                "../assets/correction-data/custom_confusion.txt"
            )),
            "bundled custom_confusion.txt",
        )? {
            lexicon.insert_confusion(wrong, correct);
        }
        for word in read_word_set_from(Cursor::new(include_str!(
            "../assets/correction-data/ignored_words.txt"
        )))? {
            lexicon.insert_ignored_word(word);
        }
        Ok(lexicon)
    }

    /// Add a user-dictionary word. A complete occurrence is protected text;
    /// it is never a candidate target for another span.
    pub fn insert_word_frequency(&mut self, word: String) {
        if word.is_empty() {
            return;
        }
        self.word_freq.insert(word.clone());
        self.protect(word);
    }

    pub fn insert_confusion(&mut self, wrong: String, correct: String) {
        if wrong.is_empty() || correct.is_empty() || wrong == correct {
            return;
        }
        if !self
            .custom_confusion
            .iter()
            .any(|(existing, _)| existing == &wrong)
        {
            self.custom_confusion.push((wrong, correct));
            self.custom_confusion
                .sort_by(|left, right| left.0.cmp(&right.0));
        }
    }

    pub fn insert_proper_name(&mut self, name: String) {
        if name.is_empty() {
            return;
        }
        self.proper_names.insert(name.clone());
        self.protect(name);
    }

    pub fn insert_ignored_word(&mut self, word: String) {
        if word.is_empty() {
            return;
        }
        self.ignored_words.insert(word.clone());
        self.protect(word);
    }

    /// Protect an author's work-dictionary entry. Only the complete entry is
    /// protected; an ordinary word inside a longer name is not protected.
    pub fn insert_protected_word(&mut self, word: String) {
        if word.is_empty() {
            return;
        }
        self.protect(word);
    }

    /// Record an author-confirmed person or place name.
    pub fn insert_work_entity(&mut self, word: String, kind: ProperNameKind) {
        self.insert_protected_word(word.clone());
        self.work_entity_lexicon.insert(word, kind);
    }

    /// Register an author-confirmed alternative exact spelling for an existing
    /// person/place. The alias is protected and never reported itself.
    pub fn insert_work_entity_alias(&mut self, canonical: &str, alias: String) -> bool {
        if !self
            .work_entity_lexicon
            .insert_alias(canonical, alias.clone())
        {
            return false;
        }
        self.protect(alias);
        true
    }

    /// Register an author-confirmed wrong spelling. The wrong spelling is
    /// deliberately not protected: an exact occurrence is reported with the
    /// canonical name as its replacement.
    pub fn insert_work_entity_misspelling(&mut self, canonical: &str, original: String) -> bool {
        self.work_entity_lexicon
            .insert_misspelling(canonical, original)
    }

    pub(crate) fn is_ignored(&self, word: &str) -> bool {
        self.ignored_words.contains(word)
            || self
                .base
                .as_ref()
                .is_some_and(|base| base.ignored_words.contains(word))
    }

    /// Merged UTF-8 byte ranges of complete protected entries found in `text`.
    pub(crate) fn protected_word_ranges(&self, text: &str) -> Vec<(usize, usize)> {
        let ranges = self.protected_word_ranges_unmerged(text);
        let mut merged = Vec::<(usize, usize)>::with_capacity(ranges.len());
        for (start, end) in ranges {
            if let Some(last) = merged.last_mut() {
                if start <= last.1 {
                    last.1 = last.1.max(end);
                    continue;
                }
            }
            merged.push((start, end));
        }
        merged
    }

    /// Unmerged ranges of complete protected entries.
    pub(crate) fn protected_word_ranges_unmerged(&self, text: &str) -> Vec<(usize, usize)> {
        let mut ranges = self.protected_word_ranges_in(&self.protected_word_index, text);
        if let Some(base) = &self.base {
            ranges.extend(base.protected_word_ranges_in(&base.protected_word_index, text));
        }
        ranges.sort_unstable();
        ranges
    }

    /// Whether one complete protected entry fully wraps `start..end`.
    pub(crate) fn protected_entry_wraps_range(&self, text: &str, start: usize, end: usize) -> bool {
        self.protected_word_ranges_unmerged(text)
            .into_iter()
            .any(|(entry_start, entry_end)| entry_start <= start && end <= entry_end)
    }

    /// Whether the project dictionary protects this complete UTF-8 byte span.
    pub(crate) fn is_work_protected_span(&self, text: &str, start: usize, end: usize) -> bool {
        self.protected_entry_wraps_range(text, start, end)
    }

    fn protected_word_ranges_in(
        &self,
        index: &ProtectedWordTrie,
        text: &str,
    ) -> Vec<(usize, usize)> {
        let chars = text.char_indices().collect::<Vec<_>>();
        let mut ranges = Vec::new();
        for start in 0..chars.len() {
            let Some(mut node) = index.children.get(&chars[start].1) else {
                continue;
            };
            for end in start..chars.len() {
                if end > start {
                    let Some(next) = node.children.get(&chars[end].1) else {
                        break;
                    };
                    node = next;
                }
                if node.terminal {
                    let end_byte = chars.get(end + 1).map_or(text.len(), |(byte, _)| *byte);
                    ranges.push((chars[start].0, end_byte));
                }
            }
        }
        ranges
    }

    pub(crate) fn work_entity_lexicon(&self) -> &WorkEntityLexicon {
        &self.work_entity_lexicon
    }

    pub(crate) fn confusion_entries(&self) -> Vec<(String, String)> {
        let mut entries = self.custom_confusion.clone();
        if let Some(base) = &self.base {
            for (wrong, correct) in &base.custom_confusion {
                if !entries.iter().any(|(existing, _)| existing == wrong) {
                    entries.push((wrong.clone(), correct.clone()));
                }
            }
        }
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        entries
    }

    fn protect(&mut self, word: String) {
        if !self.protected_words.insert(word.clone()) {
            return;
        }
        self.index_protected_word(&word);
    }

    fn index_protected_word(&mut self, word: &str) {
        let mut node = &mut self.protected_word_index;
        for ch in word.chars() {
            node = node.children.entry(ch).or_default();
        }
        node.terminal = true;
    }
}

#[derive(Clone, Copy)]
enum NameKind {
    Person,
    Place,
}

impl Lexicon {
    fn insert_named_span(&mut self, word: String, kind: NameKind) {
        match kind {
            NameKind::Person => {
                self.person_names.insert(word.clone());
            }
            NameKind::Place => {
                self.place_names.insert(word.clone());
            }
        }
        self.protect(word);
    }
}

fn read_word_freq(path: &Path) -> io::Result<Vec<(String, u64)>> {
    let file = File::open(path)?;
    read_word_freq_from(BufReader::new(file), &path.display().to_string())
}

fn read_word_freq_from<R: BufRead>(reader: R, source: &str) -> io::Result<Vec<(String, u64)>> {
    let mut result = Vec::new();
    for (line_idx, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let Some(word) = fields.next() else {
            continue;
        };
        let frequency = fields
            .next()
            .map_or(Ok(1), |value| value.parse::<u64>())
            .map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{source}:{}: invalid word frequency: {error}", line_idx + 1),
                )
            })?;
        result.push((word.to_owned(), frequency));
    }
    Ok(result)
}

fn read_custom_confusion(path: &Path) -> io::Result<Vec<(String, String, u64)>> {
    let file = File::open(path)?;
    read_custom_confusion_from(BufReader::new(file), &path.display().to_string())
}

fn read_custom_confusion_from<R: BufRead>(
    reader: R,
    source: &str,
) -> io::Result<Vec<(String, String, u64)>> {
    let mut result = Vec::new();
    for (line_idx, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let (Some(wrong), Some(correct)) = (fields.next(), fields.next()) else {
            continue;
        };
        let frequency = fields
            .next()
            .map_or(Ok(1), |value| value.parse::<u64>())
            .map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "{source}:{}: invalid confusion target frequency: {error}",
                        line_idx + 1
                    ),
                )
            })?;
        result.push((wrong.to_owned(), correct.to_owned(), frequency));
    }
    Ok(result)
}

fn read_word_set(path: &Path) -> io::Result<HashSet<String>> {
    read_word_set_from(BufReader::new(File::open(path)?))
}

fn read_word_set_from<R: BufRead>(reader: R) -> io::Result<HashSet<String>> {
    let mut result = HashSet::new();
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(word) = line.split_whitespace().next() {
            result.insert(word.to_owned());
        }
    }
    Ok(result)
}
