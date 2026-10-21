// SPDX-License-Identifier: Apache-2.0
// Modified behavioral port of pycorrector dictionary loaders in
// pycorrector/pycorrector/detector.py and proper_corrector.py by XuMing
// (xuming624); dictionary data assets and their licenses are listed separately
// in THIRD_PARTY_NOTICES.md.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use crate::proper_names::WorkEntityLexicon;

// Behavioral port of pycorrector's detector/corrector dictionary loading
// (Apache-2.0). Bundled and user-provided data are loaded by the same parsers.

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum WordFrequencySource {
    UserDictionary,
    CustomConfusion,
    PersonName,
    PlaceName,
    ProperName,
    Thuocl(u8),
}

#[derive(Clone, Copy, Debug)]
struct WordFrequency {
    source: WordFrequencySource,
    value: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Lexicon {
    base: Option<Arc<Lexicon>>,
    word_freq: HashMap<String, Vec<WordFrequency>>,
    words_by_pinyin: HashMap<(usize, Vec<char>), Vec<String>>,
    same_pinyin: HashMap<char, HashSet<char>>,
    pinyin_representatives: HashMap<char, char>,
    same_stroke: HashMap<char, HashSet<char>>,
    custom_confusion: HashMap<String, String>,
    person_names: HashMap<String, u64>,
    place_names: HashMap<String, u64>,
    proper_names: HashSet<String>,
    ignored_words: HashSet<String>,
    protected_words: HashSet<String>,
    /// 2–4-character windows derived from protected entries. They are matching
    /// helpers only and never protect text or become replacement candidates.
    protected_word_components: HashSet<String>,
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
        paths.same_pinyin.as_ref(),
        paths.same_stroke.as_ref(),
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
    children: HashMap<char, ProtectedWordTrie>,
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
                base.rebuild_candidate_indexes();
                let base = Arc::new(base);
                *cache = Some((key, Arc::clone(&base)));
                base
            }
        } else {
            let mut base = Self::from_embedded()?;
            base.load_paths(paths)?;
            base.rebuild_candidate_indexes();
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
            for (word, frequency) in read_word_freq(path)? {
                self.insert_word_frequency(word, frequency);
            }
        }
        if let Some(path) = &paths.same_pinyin {
            merge_character_groups(&mut self.same_pinyin, read_same_pinyin(path)?);
        }
        if let Some(path) = &paths.same_stroke {
            merge_character_groups(&mut self.same_stroke, read_same_stroke(path)?);
        }
        if let Some(path) = &paths.custom_confusion {
            for (wrong, correct, frequency) in read_custom_confusion(path)? {
                self.insert_confusion(wrong, correct.clone());
                self.insert_word_frequency_from(
                    correct,
                    frequency,
                    WordFrequencySource::CustomConfusion,
                );
            }
        }
        if let Some(path) = &paths.person_name {
            let person_names = read_word_freq(path)?;
            for (word, frequency) in &person_names {
                self.person_names.insert(word.clone(), *frequency);
                self.insert_word_frequency_from(
                    word.clone(),
                    *frequency,
                    WordFrequencySource::PersonName,
                );
            }
        }
        if let Some(path) = &paths.place_name {
            let place_names = read_word_freq(path)?;
            for (word, frequency) in &place_names {
                self.place_names.insert(word.clone(), *frequency);
                self.insert_word_frequency_from(
                    word.clone(),
                    *frequency,
                    WordFrequencySource::PlaceName,
                );
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
        load_thuocl_word_frequencies_from(
            Cursor::new(include_str!(
                "../assets/correction-data/thuocl_candidates.txt"
            )),
            "bundled thuocl_candidates.txt",
            &mut lexicon,
        )?;
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
            lexicon.insert_word_frequency_from(
                correct,
                frequency,
                WordFrequencySource::CustomConfusion,
            );
        }
        for (word, frequency) in read_word_freq_from(
            Cursor::new(include_str!("../assets/correction-data/person_name.txt")),
            "bundled person_name.txt",
        )? {
            lexicon.person_names.insert(word.clone(), frequency);
            lexicon.insert_word_frequency_from(word, frequency, WordFrequencySource::PersonName);
        }
        for (word, frequency) in read_word_freq_from(
            Cursor::new(include_str!("../assets/correction-data/place_name.txt")),
            "bundled place_name.txt",
        )? {
            lexicon.place_names.insert(word.clone(), frequency);
            lexicon.insert_word_frequency_from(word, frequency, WordFrequencySource::PlaceName);
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
        self.insert_word_frequency_from(word, frequency, WordFrequencySource::UserDictionary);
    }

    fn insert_word_frequency_from(
        &mut self,
        word: String,
        frequency: u64,
        source: WordFrequencySource,
    ) {
        let is_new_word = !self.word_freq.contains_key(&word)
            && !self
                .base
                .as_ref()
                .is_some_and(|base| base.word_freq.contains_key(&word));
        let sources = self.word_freq.entry(word.clone()).or_default();
        if let Some(current) = sources.iter_mut().find(|current| current.source == source) {
            if matches!(source, WordFrequencySource::Thuocl(_)) {
                current.value = current.value.max(frequency);
            } else {
                current.value = frequency;
            }
            return;
        }
        sources.push(WordFrequency {
            source,
            value: frequency,
        });

        if is_new_word {
            let char_len = word.chars().count();
            let signature = self.pinyin_signature(&word);
            self.words_by_pinyin
                .entry((char_len, signature))
                .or_default()
                .push(word);
        }
    }

    pub fn insert_confusion(&mut self, wrong: String, correct: String) {
        self.custom_confusion.insert(wrong, correct.clone());
        self.insert_word_frequency_from(correct, 1, WordFrequencySource::CustomConfusion);
    }

    pub fn insert_proper_name(&mut self, name: String) {
        self.proper_names.insert(name.clone());
        self.insert_word_frequency_from(name, 1, WordFrequencySource::ProperName);
    }

    pub fn insert_ignored_word(&mut self, word: String) {
        self.ignored_words.insert(word);
    }

    /// Protect an author's work-dictionary entry from correction candidates
    /// without adding it to the ordinary word-frequency candidate index.
    ///
    /// Only the full entry protects text. Its 2–4-character windows are kept
    /// separately for matching heuristics and never enter the protected trie,
    /// so an ordinary word inside a longer name is not silently protected.
    pub fn insert_protected_word(&mut self, word: String) {
        if word.is_empty() || !self.protected_words.insert(word.clone()) {
            return;
        }

        let chars = word.chars().collect::<Vec<_>>();
        self.index_protected_word(&word);
        let kind = if self.person_names.contains_key(&word) {
            crate::proper_names::ProperNameKind::Person
        } else if self.place_names.contains_key(&word) {
            crate::proper_names::ProperNameKind::Place
        } else {
            crate::proper_names::ProperNameKind::Other
        };
        self.work_entity_lexicon.insert(word, kind);

        for length in 2..=4.min(chars.len()) {
            for start in 0..=chars.len() - length {
                let component = chars[start..start + length].iter().collect::<String>();
                self.protected_word_components.insert(component);
            }
        }
    }

    /// Names and explicit protections exempt from OOV detection. Ordinary
    /// THUOCL candidate membership is deliberately not a general-word test.
    pub(crate) fn is_oov_exception(&self, word: &str) -> bool {
        self.person_names.contains_key(word)
            || self.place_names.contains_key(word)
            || self.proper_names.contains(word)
            || self.ignored_words.contains(word)
            || self.protected_words.contains(word)
            || self.base.as_ref().is_some_and(|base| {
                base.person_names.contains_key(word)
                    || base.place_names.contains_key(word)
                    || base.proper_names.contains(word)
                    || base.ignored_words.contains(word)
                    || base.protected_words.contains(word)
            })
    }

    pub(crate) fn is_ignored(&self, word: &str) -> bool {
        self.ignored_words.contains(word)
            || self
                .base
                .as_ref()
                .is_some_and(|base| base.ignored_words.contains(word))
    }

    pub(crate) fn is_protected_name_component(&self, word: &str) -> bool {
        // Only full ignored/protected entries count. Derived 2–4-character
        // windows must never protect an ordinary word on their own.
        self.ignored_words.contains(word)
            || self.protected_words.contains(word)
            || self.base.as_ref().is_some_and(|base| {
                base.ignored_words.contains(word) || base.protected_words.contains(word)
            })
    }

    /// True when `word` is a derived window of a longer work-dictionary entry.
    /// This is a matching guard only; it does not protect text by itself.
    pub(crate) fn is_protected_word_window(&self, word: &str) -> bool {
        self.protected_word_components.contains(word)
            || self
                .base
                .as_ref()
                .is_some_and(|base| base.protected_word_components.contains(word))
    }

    /// Return merged UTF-8 byte ranges of protected full entries found in
    /// `text`. Derived windows are intentionally excluded.
    pub(crate) fn protected_word_ranges(&self, text: &str) -> Vec<(usize, usize)> {
        let mut ranges = self.protected_word_ranges_in(&self.protected_word_index, text);
        if let Some(base) = &self.base {
            ranges.extend(base.protected_word_ranges_in(&base.protected_word_index, text));
        }
        ranges.sort_unstable();
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

    /// True when `word` is already established by a dictionary source. Such a
    /// form is never treated as a typo of a work entity.
    pub(crate) fn is_known_word(&self, word: &str) -> bool {
        self.word_freq.contains_key(word)
            || self.proper_names.contains(word)
            || self.person_names.contains_key(word)
            || self.place_names.contains_key(word)
            || self.ignored_words.contains(word)
            || self.protected_words.contains(word)
            || self.base.as_ref().is_some_and(|base| {
                base.word_freq.contains_key(word)
                    || base.proper_names.contains(word)
                    || base.person_names.contains_key(word)
                    || base.place_names.contains_key(word)
                    || base.ignored_words.contains(word)
                    || base.protected_words.contains(word)
            })
    }

    /// Author-confirmed entities for the current project. The shared base
    /// lexicon deliberately never carries work-dictionary entries.
    pub(crate) fn work_entity_lexicon(&self) -> &WorkEntityLexicon {
        &self.work_entity_lexicon
    }

    pub(crate) fn confusion_target(&self, wrong: &str) -> Option<&str> {
        self.custom_confusion
            .get(wrong)
            .or_else(|| {
                self.base
                    .as_ref()
                    .and_then(|base| base.custom_confusion.get(wrong))
            })
            .map(String::as_str)
    }

    pub(crate) fn confusion_entries(&self) -> Vec<(String, String)> {
        let mut entries = self.custom_confusion.clone();
        if let Some(base) = &self.base {
            for (wrong, correct) in &base.custom_confusion {
                entries
                    .entry(wrong.clone())
                    .or_insert_with(|| correct.clone());
            }
        }
        let mut entries = entries
            .into_iter()
            .map(|(wrong, correct)| (wrong, correct))
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        entries
    }

    pub(crate) fn word_candidates_by_pinyin(
        &self,
        word: &str,
    ) -> Box<dyn Iterator<Item = &str> + '_> {
        let base_candidates: Box<dyn Iterator<Item = &str>> = if let Some(base) = &self.base {
            Box::new(base.local_word_candidates_by_pinyin(word))
        } else {
            Box::new(std::iter::empty())
        };
        Box::new(base_candidates.chain(self.local_word_candidates_by_pinyin(word)))
    }

    fn local_word_candidates_by_pinyin<'a>(&'a self, word: &str) -> impl Iterator<Item = &'a str> {
        let char_len = word.chars().count();
        let signature = self.pinyin_signature(word);
        self.words_by_pinyin
            .get(&(char_len, signature))
            .into_iter()
            .flat_map(|words| words.iter().map(String::as_str))
    }

    pub(crate) fn confusable_chars(&self, ch: char) -> Vec<char> {
        let mut chars = HashSet::new();
        if let Some(same) = self.same_pinyin.get(&ch) {
            chars.extend(same.iter().copied());
        }
        if let Some(same) = self
            .base
            .as_ref()
            .and_then(|base| base.same_pinyin.get(&ch))
        {
            chars.extend(same.iter().copied());
        }
        if let Some(same) = self.same_stroke.get(&ch) {
            chars.extend(same.iter().copied());
        }
        if let Some(same) = self
            .base
            .as_ref()
            .and_then(|base| base.same_stroke.get(&ch))
        {
            chars.extend(same.iter().copied());
        }
        chars.remove(&ch);
        chars.into_iter().collect()
    }

    pub(crate) fn same_pinyin(&self, left: char, right: char) -> bool {
        self.same_pinyin
            .get(&left)
            .is_some_and(|chars| chars.contains(&right))
            || self.base.as_ref().is_some_and(|base| {
                base.same_pinyin
                    .get(&left)
                    .is_some_and(|chars| chars.contains(&right))
            })
    }

    pub(crate) fn is_confusable(&self, left: char, right: char) -> bool {
        self.same_pinyin(left, right)
            || self
                .same_stroke
                .get(&left)
                .is_some_and(|chars| chars.contains(&right))
            || self.base.as_ref().is_some_and(|base| {
                base.same_stroke
                    .get(&left)
                    .is_some_and(|chars| chars.contains(&right))
            })
    }

    fn pinyin_signature(&self, word: &str) -> Vec<char> {
        word.chars()
            .map(|ch| {
                self.pinyin_representatives
                    .get(&ch)
                    .or_else(|| {
                        self.base
                            .as_ref()
                            .and_then(|base| base.pinyin_representatives.get(&ch))
                    })
                    .copied()
                    .unwrap_or(ch)
            })
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
    }

    fn index_protected_word(&mut self, word: &str) {
        let mut node = &mut self.protected_word_index;
        for ch in word.chars() {
            node = node.children.entry(ch).or_default();
        }
        node.terminal = true;
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

const THUOCL_SOURCE_PRIORITY: [&str; 11] = [
    "IT",
    "animal",
    "caijing",
    "car",
    "chengyu",
    "diming",
    "food",
    "law",
    "lishimingren",
    "medical",
    "poem",
];

fn load_thuocl_word_frequencies_from<R: BufRead>(
    reader: R,
    source: &str,
    lexicon: &mut Lexicon,
) -> io::Result<()> {
    for (line_idx, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() || line.starts_with("# TSV:") {
            continue;
        }
        let mut fields = line.split_whitespace();
        let Some(word) = fields.next() else {
            continue;
        };
        let category = fields.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{source}:{}: missing THUOCL category", line_idx + 1),
            )
        })?;
        let category_name = category.strip_prefix("THUOCL_").unwrap_or(category);
        let category_id = THUOCL_SOURCE_PRIORITY
            .iter()
            .position(|candidate| *candidate == category_name)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{source}:{}: unknown THUOCL category", line_idx + 1),
                )
            })? as u8;
        let frequency = fields
            .next()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{source}:{}: missing THUOCL frequency", line_idx + 1),
                )
            })?
            .parse::<u64>()
            .map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "{source}:{}: invalid THUOCL frequency: {error}",
                        line_idx + 1
                    ),
                )
            })?;
        lexicon.insert_word_frequency_from(
            word.to_owned(),
            frequency,
            WordFrequencySource::Thuocl(category_id),
        );
    }
    Ok(())
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
