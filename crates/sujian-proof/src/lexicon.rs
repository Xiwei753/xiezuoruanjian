// SPDX-License-Identifier: Apache-2.0
// Modified behavioral port of pycorrector dictionary loaders in
// pycorrector/pycorrector/detector.py and proper_corrector.py by XuMing
// (xuming624); this file loads user-provided paths and contains no upstream data.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

// Behavioral port of pycorrector's detector/corrector dictionary loading
// (Apache-2.0). No upstream dictionary data is bundled or copied here.

#[derive(Clone, Debug, Default)]
pub struct LexiconPaths {
    pub word_freq: Option<PathBuf>,
    pub same_pinyin: Option<PathBuf>,
    pub same_stroke: Option<PathBuf>,
    pub custom_confusion: Option<PathBuf>,
    pub person_name: Option<PathBuf>,
    pub place_name: Option<PathBuf>,
    pub proper_name: Option<PathBuf>,
    pub ignored_words: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub struct Lexicon {
    word_freq: HashMap<String, u64>,
    words_by_char_len: BTreeMap<usize, Vec<String>>,
    same_pinyin: HashMap<char, HashSet<char>>,
    same_stroke: HashMap<char, HashSet<char>>,
    custom_confusion: HashMap<String, String>,
    person_names: HashMap<String, u64>,
    place_names: HashMap<String, u64>,
    proper_names: HashSet<String>,
    proper_names_by_char_len: BTreeMap<usize, Vec<String>>,
    ignored_words: HashSet<String>,
}

impl Lexicon {
    pub fn from_paths(paths: &LexiconPaths) -> io::Result<Self> {
        let mut lexicon = Self::default();
        if let Some(path) = &paths.word_freq {
            for (word, frequency) in read_word_freq(path)? {
                lexicon.insert_word_frequency(word, frequency);
            }
        }
        if let Some(path) = &paths.same_pinyin {
            lexicon.same_pinyin = read_same_pinyin(path)?;
        }
        if let Some(path) = &paths.same_stroke {
            lexicon.same_stroke = read_same_stroke(path)?;
        }
        if let Some(path) = &paths.custom_confusion {
            for (wrong, correct, frequency) in read_custom_confusion(path)? {
                lexicon.insert_confusion(wrong, correct.clone());
                lexicon.insert_word_frequency(correct, frequency);
            }
        }
        if let Some(path) = &paths.person_name {
            lexicon.person_names = read_word_freq(path)?;
            for (word, frequency) in lexicon.person_names.clone() {
                lexicon.insert_word_frequency(word, frequency);
            }
        }
        if let Some(path) = &paths.place_name {
            lexicon.place_names = read_word_freq(path)?;
            for (word, frequency) in lexicon.place_names.clone() {
                lexicon.insert_word_frequency(word, frequency);
            }
        }
        if let Some(path) = &paths.proper_name {
            for name in read_word_set(path)? {
                lexicon.insert_proper_name(name);
            }
        }
        if let Some(path) = &paths.ignored_words {
            lexicon.ignored_words = read_word_set(path)?;
        }
        Ok(lexicon)
    }

    /// Add or update a word without rebuilding a shared lexicon. Call this on
    /// the uniquely owned lexicon before sharing it through `Arc`.
    pub fn insert_word_frequency(&mut self, word: String, frequency: u64) {
        let char_len = word.chars().count();
        if !self.word_freq.contains_key(&word) {
            self.words_by_char_len
                .entry(char_len)
                .or_default()
                .push(word.clone());
        }
        self.word_freq.insert(word, frequency);
    }

    pub fn insert_confusion(&mut self, wrong: String, correct: String) {
        self.custom_confusion.insert(wrong, correct.clone());
        if !self.word_freq.contains_key(&correct) {
            self.insert_word_frequency(correct, 1);
        }
    }

    pub fn insert_proper_name(&mut self, name: String) {
        let char_len = name.chars().count();
        if self.proper_names.insert(name.clone()) {
            let names = self.proper_names_by_char_len.entry(char_len).or_default();
            names.push(name.clone());
            names.sort();
        }
        if !self.word_freq.contains_key(&name) {
            self.insert_word_frequency(name, 1);
        }
    }

    pub fn insert_ignored_word(&mut self, word: String) {
        self.ignored_words.insert(word);
    }

    pub(crate) fn is_known(&self, word: &str) -> bool {
        self.word_freq.contains_key(word)
            || self.person_names.contains_key(word)
            || self.place_names.contains_key(word)
            || self.proper_names.contains(word)
    }

    pub(crate) fn is_ignored(&self, word: &str) -> bool {
        self.ignored_words.contains(word)
    }

    pub(crate) fn is_proper_name(&self, word: &str) -> bool {
        self.proper_names.contains(word)
    }

    pub(crate) fn word_frequency(&self, word: &str) -> u64 {
        self.word_freq.get(word).copied().unwrap_or(0)
    }

    pub(crate) fn confusion_target(&self, wrong: &str) -> Option<&str> {
        self.custom_confusion.get(wrong).map(String::as_str)
    }

    pub(crate) fn confusion_entries(&self) -> Vec<(String, String)> {
        let mut entries = self
            .custom_confusion
            .iter()
            .map(|(wrong, correct)| (wrong.clone(), correct.clone()))
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        entries
    }

    pub(crate) fn word_candidates_of_len(&self, char_len: usize) -> impl Iterator<Item = &str> {
        self.words_by_char_len
            .get(&char_len)
            .into_iter()
            .flat_map(|words| words.iter().map(String::as_str))
    }

    pub(crate) fn proper_names_of_len(&self, char_len: usize) -> impl Iterator<Item = &str> {
        self.proper_names_by_char_len
            .get(&char_len)
            .into_iter()
            .flat_map(|names| names.iter().map(String::as_str))
    }

    pub(crate) fn confusable_chars(&self, ch: char) -> Vec<char> {
        let mut chars = HashSet::new();
        if let Some(same) = self.same_pinyin.get(&ch) {
            chars.extend(same.iter().copied());
        }
        if let Some(same) = self.same_stroke.get(&ch) {
            chars.extend(same.iter().copied());
        }
        chars.remove(&ch);
        chars.into_iter().collect()
    }

    pub(crate) fn same_pinyin(&self, left: char, right: char) -> bool {
        self.same_pinyin
            .get(&left)
            .is_some_and(|chars| chars.contains(&right))
    }

    pub(crate) fn is_confusable(&self, left: char, right: char) -> bool {
        self.same_pinyin(left, right)
            || self
                .same_stroke
                .get(&left)
                .is_some_and(|chars| chars.contains(&right))
    }
}

fn open_lines(path: &Path) -> io::Result<impl Iterator<Item = io::Result<String>>> {
    let file = File::open(path)?;
    Ok(BufReader::new(file).lines())
}

fn read_word_freq(path: &Path) -> io::Result<HashMap<String, u64>> {
    let mut result = HashMap::new();
    for (line_idx, line) in open_lines(path)?.enumerate() {
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
                    format!(
                        "{}:{}: invalid word frequency: {error}",
                        path.display(),
                        line_idx + 1
                    ),
                )
            })?;
        result.insert(word.to_owned(), frequency);
    }
    Ok(result)
}

fn read_same_pinyin(path: &Path) -> io::Result<HashMap<char, HashSet<char>>> {
    let mut result = HashMap::<char, HashSet<char>>::new();
    for line in open_lines(path)? {
        let line = line?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() < 3 {
            continue;
        }
        let Some(key) = fields[0].chars().next() else {
            continue;
        };
        let group = result.entry(key).or_default();
        group.extend(fields[1].chars());
        group.extend(fields[2].chars());
        group.remove(&key);
    }
    Ok(result)
}

fn read_same_stroke(path: &Path) -> io::Result<HashMap<char, HashSet<char>>> {
    let mut result = HashMap::<char, HashSet<char>>::new();
    for line in open_lines(path)? {
        let line = line?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let chars = line
            .split('\t')
            .filter_map(|item| item.chars().next())
            .collect::<Vec<_>>();
        for &ch in &chars {
            let group = result.entry(ch).or_default();
            group.extend(chars.iter().copied().filter(|other| *other != ch));
        }
    }
    Ok(result)
}

fn read_custom_confusion(path: &Path) -> io::Result<Vec<(String, String, u64)>> {
    let mut result = Vec::new();
    for (line_idx, line) in open_lines(path)?.enumerate() {
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
                        "{}:{}: invalid confusion target frequency: {error}",
                        path.display(),
                        line_idx + 1
                    ),
                )
            })?;
        result.push((wrong.to_owned(), correct.to_owned(), frequency));
    }
    Ok(result)
}

fn read_word_set(path: &Path) -> io::Result<HashSet<String>> {
    let mut result = HashSet::new();
    for line in open_lines(path)? {
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
