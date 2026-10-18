//! WriterCore adapter for the platform independent `sujian-proof` engine.
//!
//! Model paths, project dictionaries, and ignore choices are local correction
//! preferences. A model is never packaged by this module.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sujian_proof::{Lexicon, LexiconPaths};

use crate::api::{CorrectionIgnoreScopeDto, WriterError};

mod engine;

pub(crate) use engine::{scan_text, CorrectionScanner};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IgnoredDiagnostic {
    rule_id: String,
    original: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct OnceIgnoredDiagnostic {
    project_id: String,
    chapter_id: String,
    rule_id: String,
    original: String,
    text_revision: u64,
    start_byte: u32,
    end_byte: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectCorrectionPreferences {
    #[serde(default)]
    dictionary_entries: BTreeSet<String>,
    #[serde(default)]
    ignored_permanently: BTreeSet<IgnoredDiagnostic>,
    #[serde(default)]
    ignored_by_chapter: BTreeMap<String, BTreeSet<IgnoredDiagnostic>>,
    #[serde(default)]
    disabled_rules: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedCorrectionPreferences {
    #[serde(default)]
    model_path: Option<PathBuf>,
    #[serde(default)]
    projects: BTreeMap<String, ProjectCorrectionPreferences>,
}

struct CorrectionStoreInner {
    path: PathBuf,
    preferences: Mutex<PersistedCorrectionPreferences>,
    once_ignored: Mutex<HashSet<OnceIgnoredDiagnostic>>,
    lexicons: Mutex<HashMap<String, CachedLexicon>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LexiconFileStamp {
    name: &'static str,
    len: Option<u64>,
    modified: Option<SystemTime>,
}

struct CachedLexicon {
    files: Vec<LexiconFileStamp>,
    dictionary_entries: Vec<String>,
    lexicon: Arc<Lexicon>,
}

/// A process-shared, app-data-root-scoped correction preferences store.
#[derive(Clone)]
pub struct CorrectionStore {
    inner: Arc<CorrectionStoreInner>,
}

static STORE_REGISTRY: OnceLock<Mutex<HashMap<PathBuf, Arc<CorrectionStoreInner>>>> =
    OnceLock::new();

impl CorrectionStore {
    pub fn open(app_data_root: &Path) -> Result<Self, WriterError> {
        let registry = STORE_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()));
        let mut stores = registry
            .lock()
            .map_err(|_| WriterError::Other("correction store registry lock poisoned".into()))?;
        let key = app_data_root.to_path_buf();
        let inner = if let Some(inner) = stores.get(&key) {
            Arc::clone(inner)
        } else {
            let path = key.join("correction.local.json");
            let preferences = if path.exists() {
                let content = std::fs::read_to_string(&path)
                    .map_err(|error| WriterError::Io(error.to_string()))?;
                serde_json::from_str(&content)
                    .map_err(|error| WriterError::Json(error.to_string()))?
            } else {
                PersistedCorrectionPreferences::default()
            };
            let inner = Arc::new(CorrectionStoreInner {
                path,
                preferences: Mutex::new(preferences),
                once_ignored: Mutex::new(HashSet::new()),
                lexicons: Mutex::new(HashMap::new()),
            });
            stores.insert(key, Arc::clone(&inner));
            inner
        };
        Ok(Self { inner })
    }

    fn update_preferences<F, R>(&self, update: F) -> Result<R, WriterError>
    where
        F: FnOnce(&mut PersistedCorrectionPreferences) -> R,
    {
        let mut guard = self
            .inner
            .preferences
            .lock()
            .map_err(|_| WriterError::Other("correction preferences lock poisoned".into()))?;
        let mut next = guard.clone();
        let result = update(&mut next);
        let json = serde_json::to_string_pretty(&next)
            .map_err(|error| WriterError::Json(error.to_string()))?;
        crate::storage::atomic_write_string(&self.inner.path, &json)
            .map_err(|error| WriterError::Io(error.to_string()))?;
        *guard = next;
        Ok(result)
    }

    pub fn set_model_path(&self, path: &Path) -> Result<(), WriterError> {
        let metadata =
            std::fs::metadata(path).map_err(|error| WriterError::Io(error.to_string()))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(WriterError::Other(
                "correction model path must name a non-empty regular file".into(),
            ));
        }
        self.update_preferences(|preferences| preferences.model_path = Some(path.to_path_buf()))
    }

    pub fn model_path(&self) -> Result<PathBuf, WriterError> {
        let guard = self
            .inner
            .preferences
            .lock()
            .map_err(|_| WriterError::Other("correction preferences lock poisoned".into()))?;
        guard
            .model_path
            .clone()
            .ok_or_else(|| WriterError::Other("correction model path is not configured".into()))
    }

    pub fn dictionary_entries(&self, project_id: &str) -> Result<Vec<String>, WriterError> {
        let guard = self
            .inner
            .preferences
            .lock()
            .map_err(|_| WriterError::Other("correction preferences lock poisoned".into()))?;
        Ok(guard
            .projects
            .get(project_id)
            .map(|project| project.dictionary_entries.iter().cloned().collect())
            .unwrap_or_default())
    }

    pub fn lexicon_for_project(&self, project_id: &str) -> Result<Arc<Lexicon>, WriterError> {
        let dictionary_entries = self.dictionary_entries(project_id)?;
        let data_root = self
            .inner
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("correction-data");

        let (word_freq, word_freq_stamp) = optional_lexicon_file(&data_root, "word_freq.txt")?;
        let (same_pinyin, same_pinyin_stamp) =
            optional_lexicon_file(&data_root, "same_pinyin.txt")?;
        let (same_stroke, same_stroke_stamp) =
            optional_lexicon_file(&data_root, "same_stroke.txt")?;
        let (custom_confusion, custom_confusion_stamp) =
            optional_lexicon_file(&data_root, "custom_confusion.txt")?;
        let (person_name, person_name_stamp) =
            optional_lexicon_file(&data_root, "person_name.txt")?;
        let (place_name, place_name_stamp) = optional_lexicon_file(&data_root, "place_name.txt")?;
        let (proper_name, proper_name_stamp) =
            optional_lexicon_file(&data_root, "proper_name.txt")?;
        let (ignored_words, ignored_words_stamp) =
            optional_lexicon_file(&data_root, "ignored_words.txt")?;
        let files = vec![
            word_freq_stamp,
            same_pinyin_stamp,
            same_stroke_stamp,
            custom_confusion_stamp,
            person_name_stamp,
            place_name_stamp,
            proper_name_stamp,
            ignored_words_stamp,
        ];

        let mut cache = self
            .inner
            .lexicons
            .lock()
            .map_err(|_| WriterError::Other("correction lexicon cache lock poisoned".into()))?;
        if let Some(cached) = cache.get(project_id) {
            if cached.files == files && cached.dictionary_entries == dictionary_entries {
                return Ok(Arc::clone(&cached.lexicon));
            }
        }

        let mut lexicon = Lexicon::from_paths(&LexiconPaths {
            word_freq,
            same_pinyin,
            same_stroke,
            custom_confusion,
            person_name,
            place_name,
            proper_name,
            ignored_words,
        })
        .map_err(|error| WriterError::Io(error.to_string()))?;
        for entry in &dictionary_entries {
            lexicon.insert_word_frequency(entry.clone(), 1);
        }
        let lexicon = Arc::new(lexicon);
        cache.insert(
            project_id.to_string(),
            CachedLexicon {
                files,
                dictionary_entries,
                lexicon: Arc::clone(&lexicon),
            },
        );
        Ok(lexicon)
    }

    pub fn add_dictionary_entry(&self, project_id: &str, entry: &str) -> Result<bool, WriterError> {
        let entry = entry.trim();
        if entry.is_empty() || entry.chars().any(char::is_control) {
            return Err(WriterError::Other(
                "dictionary entry must be non-empty text without control characters".into(),
            ));
        }
        self.update_preferences(|preferences| {
            preferences
                .projects
                .entry(project_id.to_string())
                .or_default()
                .dictionary_entries
                .insert(entry.to_string())
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ignore(
        &self,
        project_id: &str,
        chapter_id: &str,
        rule_id: &str,
        original: &str,
        scope: CorrectionIgnoreScopeDto,
        text_revision: u64,
        start_byte: u32,
        end_byte: u32,
    ) -> Result<bool, WriterError> {
        let item = IgnoredDiagnostic {
            rule_id: rule_id.to_string(),
            original: original.to_string(),
        };
        match scope {
            CorrectionIgnoreScopeDto::Once => {
                let ignored = OnceIgnoredDiagnostic {
                    project_id: project_id.to_string(),
                    chapter_id: chapter_id.to_string(),
                    rule_id: rule_id.to_string(),
                    original: original.to_string(),
                    text_revision,
                    start_byte,
                    end_byte,
                };
                let mut once = self.inner.once_ignored.lock().map_err(|_| {
                    WriterError::Other("one-time correction ignore lock poisoned".into())
                })?;
                Ok(once.insert(ignored))
            }
            CorrectionIgnoreScopeDto::Chapter => self.update_preferences(|preferences| {
                preferences
                    .projects
                    .entry(project_id.to_string())
                    .or_default()
                    .ignored_by_chapter
                    .entry(chapter_id.to_string())
                    .or_default()
                    .insert(item)
            }),
            CorrectionIgnoreScopeDto::Permanent => self.update_preferences(|preferences| {
                preferences
                    .projects
                    .entry(project_id.to_string())
                    .or_default()
                    .ignored_permanently
                    .insert(item)
            }),
            CorrectionIgnoreScopeDto::Rule => self.update_preferences(|preferences| {
                preferences
                    .projects
                    .entry(project_id.to_string())
                    .or_default()
                    .disabled_rules
                    .insert(rule_id.to_string())
            }),
        }
    }

    pub fn filter_ignored(
        &self,
        project_id: &str,
        chapter_id: &str,
        text_revision: u64,
        suggestions: Vec<sujian_proof::CorrectionDiagnostic>,
    ) -> Result<Vec<sujian_proof::CorrectionDiagnostic>, WriterError> {
        let guard = self
            .inner
            .preferences
            .lock()
            .map_err(|_| WriterError::Other("correction preferences lock poisoned".into()))?;
        let project = guard.projects.get(project_id);
        let mut once =
            self.inner.once_ignored.lock().map_err(|_| {
                WriterError::Other("one-time correction ignore lock poisoned".into())
            })?;
        once.retain(|ignored| {
            ignored.project_id != project_id
                || ignored.chapter_id != chapter_id
                || ignored.text_revision == text_revision
        });
        Ok(suggestions
            .into_iter()
            .filter(|suggestion| {
                if let Some(project) = project {
                    if project.disabled_rules.contains(&suggestion.rule_id) {
                        return false;
                    }
                    let item = IgnoredDiagnostic {
                        rule_id: suggestion.rule_id.clone(),
                        original: suggestion.original.clone(),
                    };
                    if project.ignored_permanently.contains(&item)
                        || project
                            .ignored_by_chapter
                            .get(chapter_id)
                            .map(|ignored| ignored.contains(&item))
                            .unwrap_or(false)
                    {
                        return false;
                    }
                }
                let start_byte = u32::try_from(suggestion.start_byte).ok();
                let end_byte = u32::try_from(suggestion.end_byte).ok();
                !once.iter().any(|ignored| {
                    ignored.project_id == project_id
                        && ignored.chapter_id == chapter_id
                        && ignored.rule_id == suggestion.rule_id
                        && ignored.original == suggestion.original
                        && ignored.text_revision == text_revision
                        && Some(ignored.start_byte) == start_byte
                        && Some(ignored.end_byte) == end_byte
                })
            })
            .collect())
    }
}

fn optional_lexicon_file(
    root: &Path,
    name: &'static str,
) -> Result<(Option<PathBuf>, LexiconFileStamp), WriterError> {
    let path = root.join(name);
    match std::fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => Ok((
            Some(path),
            LexiconFileStamp {
                name,
                len: Some(metadata.len()),
                modified: metadata.modified().ok(),
            },
        )),
        Ok(_) => Err(WriterError::Other(format!(
            "correction lexicon path is not a regular file: {}",
            path.display()
        ))),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok((
            None,
            LexiconFileStamp {
                name,
                len: None,
                modified: None,
            },
        )),
        Err(error) => Err(WriterError::Io(error.to_string())),
    }
}
