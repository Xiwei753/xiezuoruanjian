// SPDX-License-Identifier: Apache-2.0
// Modified behavioral port of pycorrector dictionary loaders in
// pycorrector/pycorrector/detector.py and proper_corrector.py by XuMing
// (xuming624); dictionary data assets and their licenses are listed separately
// in THIRD_PARTY_NOTICES.md.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Cursor};
use std::path::{Path, PathBuf};

// Behavioral port of pycorrector's detector/corrector dictionary loading
// (Apache-2.0). Bundled and user-provided data are loaded by the same parsers.

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
    words_by_pinyin: HashMap<(usize, Vec<char>), Vec<String>>,
    same_pinyin: HashMap<char, HashSet<char>>,
    pinyin_representatives: HashMap<char, char>,
    same_stroke: HashMap<char, HashSet<char>>,
    custom_confusion: HashMap<String, String>,
    person_names: HashMap<String, u64>,
    place_names: HashMap<String, u64>,
    proper_names: HashSet<String>,
    proper_name_wildcards: HashMap<(usize, usize, String), Vec<String>>,
    ignored_words: HashSet<String>,
    ignored_word_components: HashSet<String>,
}

impl Lexicon {
    pub fn from_paths(paths: &LexiconPaths) -> io::Result<Self> {
        let mut lexicon = Self::from_embedded()?;
        if let Some(path) = &paths.word_freq {
            for (word, frequency) in read_word_freq(path)? {
                lexicon.insert_word_frequency(word, frequency);
            }
        }
        if let Some(path) = &paths.same_pinyin {
            merge_character_groups(&mut lexicon.same_pinyin, read_same_pinyin(path)?);
        }
        if let Some(path) = &paths.same_stroke {
            merge_character_groups(&mut lexicon.same_stroke, read_same_stroke(path)?);
        }
        if let Some(path) = &paths.custom_confusion {
            for (wrong, correct, frequency) in read_custom_confusion(path)? {
                lexicon.insert_confusion(wrong, correct.clone());
                lexicon.insert_word_frequency(correct, frequency);
            }
        }
        if let Some(path) = &paths.person_name {
            let person_names = read_word_freq(path)?;
            for (word, frequency) in &person_names {
                lexicon.person_names.insert(word.clone(), *frequency);
                lexicon.insert_word_frequency(word.clone(), *frequency);
            }
        }
        if let Some(path) = &paths.place_name {
            let place_names = read_word_freq(path)?;
            for (word, frequency) in &place_names {
                lexicon.place_names.insert(word.clone(), *frequency);
                lexicon.insert_word_frequency(word.clone(), *frequency);
            }
        }
        if let Some(path) = &paths.proper_name {
            for name in read_word_set(path)? {
                lexicon.insert_proper_name(name);
            }
        }
        if let Some(path) = &paths.ignored_words {
            for word in read_word_set(path)? {
                lexicon.insert_ignored_word(word);
            }
        }
        lexicon.rebuild_candidate_indexes();
        Ok(lexicon)
    }

    fn from_embedded() -> io::Result<Self> {
        let mut lexicon = Self::default();
        for (word, frequency) in read_word_freq_from(
            Cursor::new(include_str!("../assets/correction-data/word_freq.txt")),
            "bundled word_freq.txt",
        )? {
            lexicon.insert_word_frequency(word, frequency);
        }
        merge_character_groups(
            &mut lexicon.same_pinyin,
            read_same_pinyin_from(Cursor::new(include_str!(
                "../assets/correction-data/same_pinyin.txt"
            )))?,
        );
        merge_character_groups(
            &mut lexicon.same_stroke,
            read_same_stroke_from(Cursor::new(include_str!(
                "../assets/correction-data/same_stroke.txt"
            )))?,
        );
        for (wrong, correct, frequency) in read_custom_confusion_from(
            Cursor::new(include_str!(
                "../assets/correction-data/custom_confusion.txt"
            )),
            "bundled custom_confusion.txt",
        )? {
            lexicon.insert_confusion(wrong, correct.clone());
            lexicon.insert_word_frequency(correct, frequency);
        }
        for (word, frequency) in read_word_freq_from(
            Cursor::new(include_str!("../assets/correction-data/person_name.txt")),
            "bundled person_name.txt",
        )? {
            lexicon.person_names.insert(word.clone(), frequency);
            lexicon.insert_word_frequency(word, frequency);
        }
        for (word, frequency) in read_word_freq_from(
            Cursor::new(include_str!("../assets/correction-data/place_name.txt")),
            "bundled place_name.txt",
        )? {
            lexicon.place_names.insert(word.clone(), frequency);
            lexicon.insert_word_frequency(word, frequency);
        }
        for name in read_word_set_from(Cursor::new(include_str!(
            "../assets/correction-data/proper_name.txt"
        )))? {
            lexicon.insert_proper_name(name);
        }
        for word in read_word_set_from(Cursor::new(include_str!(
            "../assets/correction-data/ignored_words.txt"
        )))? {
            lexicon.insert_ignored_word(word);
        }
        Ok(lexicon)
    }

    /// Add or update a word without rebuilding a shared lexicon. Call this on
    /// the uniquely owned lexicon before sharing it through `Arc`.
    pub fn insert_word_frequency(&mut self, word: String, frequency: u64) {
        let char_len = word.chars().count();
        if self.word_freq.contains_key(&word) {
            self.word_freq.insert(word, frequency);
            return;
        }
        let signature = self.pinyin_signature(&word);
        self.words_by_pinyin
            .entry((char_len, signature))
            .or_default()
            .push(word.clone());
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
            self.index_proper_name(&name, char_len);
        }
        if !self.word_freq.contains_key(&name) {
            self.insert_word_frequency(name, 1);
        }
    }

    pub fn insert_ignored_word(&mut self, word: String) {
        if !self.ignored_words.insert(word.clone()) {
            return;
        }
        let chars = word.chars().collect::<Vec<_>>();
        for length in 2..=4.min(chars.len()) {
            for start in 0..=chars.len() - length {
                self.ignored_word_components
                    .insert(chars[start..start + length].iter().collect());
            }
        }
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

    pub(crate) fn is_protected_name_component(&self, word: &str) -> bool {
        self.ignored_words.contains(word) || self.ignored_word_components.contains(word)
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

    pub(crate) fn word_candidates_by_pinyin(&self, word: &str) -> impl Iterator<Item = &str> {
        let char_len = word.chars().count();
        let signature = self.pinyin_signature(word);
        self.words_by_pinyin
            .get(&(char_len, signature))
            .into_iter()
            .flat_map(|words| words.iter().map(String::as_str))
    }

    pub(crate) fn near_proper_name_candidates<'a>(&'a self, source: &[char]) -> Vec<&'a str> {
        let char_len = source.len();
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        for position in 0..char_len {
            let remainder = source
                .iter()
                .enumerate()
                .filter_map(|(index, ch)| (index != position).then_some(*ch))
                .collect::<String>();
            if let Some(names) = self
                .proper_name_wildcards
                .get(&(char_len, position, remainder))
            {
                for name in names {
                    if seen.insert(name.as_str()) {
                        candidates.push(name.as_str());
                    }
                }
            }
        }
        candidates
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

    fn pinyin_signature(&self, word: &str) -> Vec<char> {
        word.chars()
            .map(|ch| self.pinyin_representatives.get(&ch).copied().unwrap_or(ch))
            .collect()
    }

    fn pinyin_components(&self) -> HashMap<char, char> {
        let mut parent = HashMap::<char, char>::new();
        for (&key, values) in &self.same_pinyin {
            parent.entry(key).or_insert(key);
            for &value in values {
                parent.entry(value).or_insert(value);
                union_chars(&mut parent, key, value);
            }
        }
        let chars = parent.keys().copied().collect::<Vec<_>>();
        chars
            .into_iter()
            .map(|ch| (ch, find_char_root(&mut parent, ch)))
            .collect()
    }

    fn rebuild_candidate_indexes(&mut self) {
        self.pinyin_representatives = self.pinyin_components();
        self.words_by_pinyin.clear();
        let words = self.word_freq.keys().cloned().collect::<Vec<_>>();
        for word in words {
            let char_len = word.chars().count();
            let signature = self.pinyin_signature(&word);
            self.words_by_pinyin
                .entry((char_len, signature))
                .or_default()
                .push(word);
        }

        self.proper_name_wildcards.clear();
        let proper_names = self.proper_names.iter().cloned().collect::<Vec<_>>();
        for name in proper_names {
            let char_len = name.chars().count();
            self.index_proper_name(&name, char_len);
        }
    }

    fn index_proper_name(&mut self, name: &str, char_len: usize) {
        if !(2..=4).contains(&char_len) {
            return;
        }
        let chars = name.chars().collect::<Vec<_>>();
        for position in 0..char_len {
            let remainder = chars
                .iter()
                .enumerate()
                .filter_map(|(index, ch)| (index != position).then_some(*ch))
                .collect::<String>();
            self.proper_name_wildcards
                .entry((char_len, position, remainder))
                .or_default()
                .push(name.to_owned());
        }
    }
}

fn find_char_root(parent: &mut HashMap<char, char>, ch: char) -> char {
    let root = parent.get(&ch).copied().unwrap_or(ch);
    if root == ch {
        return ch;
    }
    let canonical = find_char_root(parent, root);
    parent.insert(ch, canonical);
    canonical
}

fn union_chars(parent: &mut HashMap<char, char>, left: char, right: char) {
    let left_root = find_char_root(parent, left);
    let right_root = find_char_root(parent, right);
    if left_root != right_root {
        let (canonical, child) = if left_root < right_root {
            (left_root, right_root)
        } else {
            (right_root, left_root)
        };
        parent.insert(child, canonical);
    }
}

fn read_word_freq(path: &Path) -> io::Result<HashMap<String, u64>> {
    let file = File::open(path)?;
    read_word_freq_from(BufReader::new(file), &path.display().to_string())
}

fn read_word_freq_from<R: BufRead>(reader: R, source: &str) -> io::Result<HashMap<String, u64>> {
    let mut result = HashMap::new();
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
                    format!(
                        "{}:{}: invalid word frequency: {error}",
                        source,
                        line_idx + 1
                    ),
                )
            })?;
        result.insert(word.to_owned(), frequency);
    }
    Ok(result)
}

fn read_same_pinyin(path: &Path) -> io::Result<HashMap<char, HashSet<char>>> {
    read_same_pinyin_from(BufReader::new(File::open(path)?))
}

fn read_same_pinyin_from<R: BufRead>(reader: R) -> io::Result<HashMap<char, HashSet<char>>> {
    let mut result = HashMap::<char, HashSet<char>>::new();
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() < 2 {
            continue;
        }
        let Some(key) = fields[0].chars().next() else {
            continue;
        };
        let group = result.entry(key).or_default();
        group.extend(fields[1].chars());
        if let Some(additional_chars) = fields.get(2) {
            group.extend(additional_chars.chars());
        }
        group.remove(&key);
    }
    Ok(result)
}

fn read_same_stroke(path: &Path) -> io::Result<HashMap<char, HashSet<char>>> {
    read_same_stroke_from(BufReader::new(File::open(path)?))
}

fn read_same_stroke_from<R: BufRead>(reader: R) -> io::Result<HashMap<char, HashSet<char>>> {
    let mut result = HashMap::<char, HashSet<char>>::new();
    for line in reader.lines() {
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
                        "{}:{}: invalid confusion target frequency: {error}",
                        source,
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

fn merge_character_groups(
    target: &mut HashMap<char, HashSet<char>>,
    source: HashMap<char, HashSet<char>>,
) {
    for (ch, group) in source {
        target.entry(ch).or_default().extend(group);
    }
}
