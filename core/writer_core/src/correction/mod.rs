//! WriterCore adapter for the platform independent `sujian-proof` engine.
//!
//! Model paths, project dictionaries, and ignore choices are local correction
//! preferences. A model is never packaged by this module.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sujian_proof::lm::KenlmModel;
use sujian_proof::{CorrectionEngine, Lexicon, LexiconPaths};

use crate::api::{
    CorrectionDictionaryEntryDto, CorrectionDictionaryEntryKindDto, CorrectionEntityNameDto,
    CorrectionEntityNameKindDto, CorrectionIgnoreScopeDto, CorrectionModelPathStatusDto,
    CorrectionResourceStatusDto, WriterError,
};

mod engine;
mod result_cache;

pub(crate) use engine::CorrectionScanner;
pub(crate) use result_cache::EditRange;
use result_cache::{
    apply_edits_light, cache_file_name, mark_unseen_segments_in_edit_windows, modified_stamp,
    parts_fingerprint, ChapterCorrectionCacheState, ChapterCorrectionSlot,
};

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
    /// Every author-added word. All entries protect their exact text.
    #[serde(default)]
    dictionary_entries: BTreeSet<String>,
    /// Explicitly typed subsets. Legacy settings do not populate these, so an
    /// old string entry never silently becomes a person or place name.
    #[serde(default)]
    dictionary_person_entries: BTreeSet<String>,
    #[serde(default)]
    dictionary_place_entries: BTreeSet<String>,
    /// Exact author-confirmed aliases/wrong spellings for registered names.
    #[serde(default)]
    entity_names: BTreeSet<CorrectionEntityNameDto>,
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

type ChapterResultCacheRegistry = HashMap<(String, String), Arc<ChapterCorrectionSlot>>;

struct CorrectionStoreInner {
    path: PathBuf,
    preferences: Mutex<PersistedCorrectionPreferences>,
    once_ignored: Mutex<HashSet<OnceIgnoredDiagnostic>>,
    active_project: Mutex<ActiveCorrectionProject>,
    next_lexicon_version: AtomicU64,
    lexicons: Mutex<HashMap<String, CachedLexicon>>,
    engines: Mutex<HashMap<String, CachedCorrectionEngine>>,
    result_caches: Mutex<ChapterResultCacheRegistry>,
}

struct ActiveCorrectionProject {
    project_id: Option<String>,
    generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LexiconFileStamp {
    name: &'static str,
    len: Option<u64>,
    modified: Option<SystemTime>,
}

struct CachedLexicon {
    files: Vec<LexiconFileStamp>,
    dictionary_entries: Vec<CorrectionDictionaryEntryDto>,
    entity_names: Vec<CorrectionEntityNameDto>,
    version: u64,
    lexicon: Arc<Lexicon>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ModelFileStamp {
    len: u64,
    modified: Option<SystemTime>,
}

struct CachedModel {
    stamp: ModelFileStamp,
    model: Weak<KenlmModel>,
}

/// Identity of the exact model file an engine was built from. The path alone is
/// not enough: replacing the model at the same path must invalidate both the
/// scanner result cache and the process-wide engine cache.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ModelCacheKey {
    path: PathBuf,
    stamp: ModelFileStamp,
}

struct CachedCorrectionEngine {
    lexicon_version: u64,
    model_key: Option<ModelCacheKey>,
    engine: Arc<CorrectionEngine>,
}

/// A process-shared, app-data-root-scoped correction preferences store.
#[derive(Clone)]
pub struct CorrectionStore {
    inner: Arc<CorrectionStoreInner>,
}

static STORE_REGISTRY: OnceLock<Mutex<HashMap<PathBuf, Arc<CorrectionStoreInner>>>> =
    OnceLock::new();
static MODEL_CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedModel>>> = OnceLock::new();

fn model_cache() -> &'static Mutex<HashMap<PathBuf, CachedModel>> {
    MODEL_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn canonical_model_path(path: &Path) -> Result<PathBuf, WriterError> {
    path.canonicalize()
        .map_err(|error| WriterError::Io(error.to_string()))
}

fn model_file_stamp(path: &Path) -> Result<ModelFileStamp, WriterError> {
    let metadata = std::fs::metadata(path).map_err(|error| WriterError::Io(error.to_string()))?;
    Ok(ModelFileStamp {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn open_model_candidate(path: &Path) -> Result<(ModelCacheKey, Arc<KenlmModel>), WriterError> {
    // KenlmModel::open is the sole format validator. Validate before taking any
    // preference/cache lock so rejected candidates cannot disturb active state.
    let path = canonical_model_path(path)?;
    let stamp_before = model_file_stamp(&path)?;
    let model = KenlmModel::open(&path)
        .map_err(|error| WriterError::Other(format!("invalid correction model: {error}")))?;
    let stamp_after = model_file_stamp(&path)?;
    if stamp_before != stamp_after {
        return Err(WriterError::Other(
            "correction model changed while it was being opened; retry the request".into(),
        ));
    }
    Ok((
        ModelCacheKey {
            path,
            stamp: stamp_after,
        },
        Arc::new(model),
    ))
}

const CORRECTION_DICTIONARY_FILES: [&str; 8] = [
    "word_freq.txt",
    "same_pinyin.txt",
    "same_stroke.txt",
    "custom_confusion.txt",
    "person_name.txt",
    "place_name.txt",
    "proper_name.txt",
    "ignored_words.txt",
];

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
                active_project: Mutex::new(ActiveCorrectionProject {
                    project_id: None,
                    generation: 0,
                }),
                next_lexicon_version: AtomicU64::new(1),
                lexicons: Mutex::new(HashMap::new()),
                engines: Mutex::new(HashMap::new()),
                result_caches: Mutex::new(HashMap::new()),
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
        let (key, candidate) = open_model_candidate(path)?;
        let mut models = model_cache()
            .lock()
            .map_err(|_| WriterError::Other("correction model cache lock poisoned".into()))?;
        let mut preferences = self
            .inner
            .preferences
            .lock()
            .map_err(|_| WriterError::Other("correction preferences lock poisoned".into()))?;
        let mut engines = self
            .inner
            .engines
            .lock()
            .map_err(|_| WriterError::Other("correction engine cache lock poisoned".into()))?;

        let mut next = preferences.clone();
        next.model_path = Some(key.path.clone());
        let json = serde_json::to_string_pretty(&next)
            .map_err(|error| WriterError::Json(error.to_string()))?;
        crate::storage::atomic_write_string(&self.inner.path, &json)
            .map_err(|error| WriterError::Io(error.to_string()))?;

        // The candidate is already validated. Cache only a weak mapping: an
        // active engine/scanner owns the strong reference, and idle models can
        // be released as soon as their last engine is dropped.
        models.retain(|_, model| model.model.strong_count() > 0);
        models.insert(
            key.path.clone(),
            CachedModel {
                stamp: key.stamp,
                model: Arc::downgrade(&candidate),
            },
        );
        *preferences = next;
        // Selecting a model always starts a fresh engine, even when the path is
        // unchanged but the file behind it was replaced.
        engines.clear();
        Ok(())
    }

    pub fn configured_model_path(&self) -> Result<Option<PathBuf>, WriterError> {
        let guard = self
            .inner
            .preferences
            .lock()
            .map_err(|_| WriterError::Other("correction preferences lock poisoned".into()))?;
        Ok(guard.model_path.clone())
    }

    pub fn resource_status(&self) -> Result<CorrectionResourceStatusDto, WriterError> {
        let model_path = self.configured_model_path()?;
        let model_file_available = model_path.as_ref().is_some_and(|path| {
            std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
        });
        let data_root = self
            .inner
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("correction-data");
        let mut installed_dictionary_files = Vec::new();
        let mut missing_dictionary_files = Vec::new();
        for name in CORRECTION_DICTIONARY_FILES {
            let ready = std::fs::metadata(data_root.join(name))
                .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0);
            if ready {
                installed_dictionary_files.push(name.to_string());
            } else {
                missing_dictionary_files.push(name.to_string());
            }
        }
        Ok(CorrectionResourceStatusDto {
            model_configured: model_path.is_some(),
            model_file_available,
            model_path: model_path.map(|path| path.to_string_lossy().into_owned()),
            bundled_lexicon_available: true,
            installed_dictionary_files,
            missing_dictionary_files,
        })
    }

    pub fn model_path_status(
        &self,
        path: &Path,
    ) -> Result<CorrectionModelPathStatusDto, WriterError> {
        let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let mut models = model_cache()
            .lock()
            .map_err(|_| WriterError::Other("correction model cache lock poisoned".into()))?;
        let stores = STORE_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()));
        let stores = stores
            .lock()
            .map_err(|_| WriterError::Other("correction store registry lock poisoned".into()))?;

        let mut configured = false;
        for inner in stores.values() {
            let preferences = inner
                .preferences
                .lock()
                .map_err(|_| WriterError::Other("correction preferences lock poisoned".into()))?;
            if preferences
                .model_path
                .as_ref()
                .is_some_and(|configured_path| {
                    configured_path
                        .canonicalize()
                        .unwrap_or_else(|_| configured_path.clone())
                        == key
                })
            {
                configured = true;
            }
        }

        if !configured {
            for inner in stores.values() {
                let mut engines = inner.engines.lock().map_err(|_| {
                    WriterError::Other("correction engine cache lock poisoned".into())
                })?;
                engines.retain(|_, cached| {
                    cached.model_key.as_ref().map(|model| &model.path) != Some(&key)
                        || Arc::strong_count(&cached.engine) > 1
                });
            }
        }

        let in_use = models
            .get(&key)
            .is_some_and(|cached| cached.model.strong_count() > 0);
        if !configured && !in_use {
            models.remove(&key);
        }

        Ok(CorrectionModelPathStatusDto {
            configured,
            in_use,
            deletion_eligible: !configured && !in_use,
        })
    }

    pub fn dictionary_entries(&self, project_id: &str) -> Result<Vec<String>, WriterError> {
        Ok(self
            .dictionary_entry_records(project_id)?
            .into_iter()
            .map(|entry| entry.text)
            .collect())
    }

    pub fn dictionary_entry_records(
        &self,
        project_id: &str,
    ) -> Result<Vec<CorrectionDictionaryEntryDto>, WriterError> {
        let guard = self
            .inner
            .preferences
            .lock()
            .map_err(|_| WriterError::Other("correction preferences lock poisoned".into()))?;
        let Some(project) = guard.projects.get(project_id) else {
            return Ok(Vec::new());
        };
        let mut records = project
            .dictionary_entries
            .iter()
            .map(|entry| CorrectionDictionaryEntryDto {
                text: entry.clone(),
                kind: if project.dictionary_person_entries.contains(entry) {
                    CorrectionDictionaryEntryKindDto::Person
                } else if project.dictionary_place_entries.contains(entry) {
                    CorrectionDictionaryEntryKindDto::Place
                } else {
                    CorrectionDictionaryEntryKindDto::Term
                },
            })
            .collect::<Vec<_>>();
        // Preserve a typed entry even if a malformed/older settings file did
        // not also contain it in the general set.
        for entry in &project.dictionary_person_entries {
            if !project.dictionary_entries.contains(entry) {
                records.push(CorrectionDictionaryEntryDto {
                    text: entry.clone(),
                    kind: CorrectionDictionaryEntryKindDto::Person,
                });
            }
        }
        for entry in &project.dictionary_place_entries {
            if !project.dictionary_entries.contains(entry) {
                records.push(CorrectionDictionaryEntryDto {
                    text: entry.clone(),
                    kind: CorrectionDictionaryEntryKindDto::Place,
                });
            }
        }
        records.sort_by(|left, right| left.text.cmp(&right.text));
        Ok(records)
    }

    pub fn entity_name_records(
        &self,
        project_id: &str,
    ) -> Result<Vec<CorrectionEntityNameDto>, WriterError> {
        let guard = self
            .inner
            .preferences
            .lock()
            .map_err(|_| WriterError::Other("correction preferences lock poisoned".into()))?;
        Ok(guard
            .projects
            .get(project_id)
            .map(|project| project.entity_names.iter().cloned().collect())
            .unwrap_or_default())
    }

    pub fn add_entity_name(
        &self,
        project_id: &str,
        canonical: &str,
        name: &str,
        kind: CorrectionEntityNameKindDto,
    ) -> Result<bool, WriterError> {
        let canonical = canonical.trim();
        let name = name.trim();
        if canonical.is_empty() || name.is_empty() {
            return Err(WriterError::Other(
                "entity canonical name and written form must be non-empty".into(),
            ));
        }
        if canonical.chars().any(char::is_control) || name.chars().any(char::is_control) {
            return Err(WriterError::Other(
                "entity names must not contain control characters".into(),
            ));
        }
        if !sujian_proof::is_valid_work_entity(name) {
            return Err(WriterError::Other(
                "entity written form must be a complete name (at most 64 characters)".into(),
            ));
        }
        if canonical == name {
            return Err(WriterError::Other(
                "entity written form must differ from the canonical name".into(),
            ));
        }
        self.update_preferences(|preferences| {
            let project = preferences
                .projects
                .entry(project_id.to_string())
                .or_default();
            if !project.dictionary_person_entries.contains(canonical)
                && !project.dictionary_place_entries.contains(canonical)
            {
                return Err(WriterError::Other(
                    "entity canonical name must be a registered person or place".into(),
                ));
            }
            if project.dictionary_entries.contains(name)
                || project.dictionary_person_entries.contains(name)
                || project.dictionary_place_entries.contains(name)
            {
                return Err(WriterError::Other(
                    "entity written form is already another work-dictionary entry".into(),
                ));
            }
            if let Some(existing) = project
                .entity_names
                .iter()
                .find(|existing| existing.name == name)
            {
                if existing.canonical == canonical && existing.kind == kind {
                    return Ok(false);
                }
                return Err(WriterError::Other(
                    "entity written form is already registered for another name".into(),
                ));
            }
            project.entity_names.insert(CorrectionEntityNameDto {
                canonical: canonical.to_string(),
                name: name.to_string(),
                kind,
            });
            Ok(true)
        })?
    }

    /// Remove one author-confirmed alias or misspelling relation. The
    /// canonical person/place entry and any other relations are preserved.
    pub fn remove_entity_name(
        &self,
        project_id: &str,
        canonical: &str,
        name: &str,
    ) -> Result<bool, WriterError> {
        let canonical = canonical.trim();
        let name = name.trim();
        if canonical.is_empty() || name.is_empty() {
            return Err(WriterError::Other(
                "entity canonical name and written form must be non-empty".into(),
            ));
        }
        self.update_preferences(|preferences| {
            let Some(project) = preferences.projects.get_mut(project_id) else {
                return Ok(false);
            };
            let before = project.entity_names.len();
            project
                .entity_names
                .retain(|entry| entry.canonical != canonical || entry.name != name);
            Ok(project.entity_names.len() != before)
        })?
    }

    /// Root directory for per-chapter correction result caches. The cache holds
    /// derived data only; it never stores chapter text and never syncs.
    fn result_cache_dir(&self) -> PathBuf {
        self.inner
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("correction-cache")
    }

    /// Process-shared single-flight state for one chapter's correction cache.
    pub(super) fn chapter_result_cache(
        &self,
        project_id: &str,
        chapter_id: &str,
    ) -> Result<Arc<ChapterCorrectionSlot>, WriterError> {
        let key = (project_id.to_string(), chapter_id.to_string());
        let mut caches = self.inner.result_caches.lock().map_err(|_| {
            WriterError::Other("correction result cache registry lock poisoned".into())
        })?;
        if let Some(existing) = caches.get(&key) {
            return Ok(Arc::clone(existing));
        }
        let path = self
            .result_cache_dir()
            .join(cache_file_name(project_id, chapter_id));
        let state = Arc::new(ChapterCorrectionSlot::new(
            ChapterCorrectionCacheState::load(path),
        ));
        caches.insert(key, Arc::clone(&state));
        Ok(state)
    }

    /// Lightweight edit phase: apply the transaction's byte mappings to the
    /// committed segment index without running the model or fingerprinting the
    /// chapter. The next pending-scan request re-segments these windows against
    /// the newest text.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_correction_edits(
        &self,
        project_id: &str,
        chapter_id: &str,
        session_id: u64,
        text_revision: u64,
        base_revision: u64,
        edits: &[EditRange],
    ) -> Result<(), WriterError> {
        if base_revision > text_revision {
            return Err(WriterError::Other(
                "correction edit base revision is newer than the current revision".into(),
            ));
        }
        let slot = self.chapter_result_cache(project_id, chapter_id)?;
        let mut guard = slot
            .state
            .lock()
            .map_err(|_| WriterError::Other("correction result cache lock poisoned".into()))?;
        if guard.last_session_id != 0 && session_id < guard.last_session_id {
            return Ok(());
        }
        let same_session = guard.last_session_id == session_id;
        if same_session && text_revision < guard.last_revision {
            return Ok(());
        }
        if same_session && text_revision == guard.last_revision {
            // A restore may already have rebuilt this revision. Keep the
            // transaction's local windows scannable without replaying it into
            // an index that already describes the same text.
            mark_unseen_segments_in_edit_windows(&mut guard, edits);
            return Ok(());
        }
        if !same_session || guard.last_revision != base_revision || guard.needs_reconcile {
            guard.needs_reconcile = true;
        }
        apply_edits_light(&mut guard, edits)?;
        guard.last_session_id = session_id;
        guard.last_revision = text_revision;
        Ok(())
    }

    /// Stable identity of everything that changes scan results except the text
    /// itself: the model file, the lexicon files, and project dictionary
    /// entries. Ignore choices are applied when a snapshot is assembled and are
    /// deliberately excluded so they never force a rescan.
    pub(super) fn scanner_fingerprint(&self, project_id: &str) -> Result<String, WriterError> {
        let (model_path, disabled_rules) = {
            let guard =
                self.inner.preferences.lock().map_err(|_| {
                    WriterError::Other("correction preferences lock poisoned".into())
                })?;
            let mut disabled_rules = guard
                .projects
                .get(project_id)
                .map(|project| project.disabled_rules.iter().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            disabled_rules.sort();
            (guard.model_path.clone(), disabled_rules)
        };
        let mut parts = vec![
            "correction-scanner-v5".to_string(),
            "correction-work-entities-v3".to_string(),
            model_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| "no-model".to_string()),
        ];
        if let Some(path) = &model_path {
            match std::fs::metadata(path) {
                Ok(metadata) => parts.push(format!(
                    "model:{}:{}",
                    metadata.len(),
                    modified_stamp(metadata.modified().ok())
                )),
                Err(_) => parts.push("model:missing".to_string()),
            }
        }
        let data_root = self
            .inner
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("correction-data");
        for name in CORRECTION_DICTIONARY_FILES {
            let (_, stamp) = optional_lexicon_file(&data_root, name)?;
            parts.push(format!(
                "lexicon:{name}:{:?}:{}",
                stamp.len,
                modified_stamp(stamp.modified)
            ));
        }
        for entry in self.dictionary_entry_records(project_id)? {
            parts.push(format!("dictionary:{:?}:{}", entry.kind, entry.text));
        }
        for relation in self.entity_name_records(project_id)? {
            parts.push(format!(
                "entity-name:{:?}:{}:{}",
                relation.kind, relation.canonical, relation.name
            ));
        }
        for rule in disabled_rules {
            parts.push(format!("disabled-rule:{rule}"));
        }
        Ok(parts_fingerprint(&parts))
    }

    pub fn lexicon_for_project(&self, project_id: &str) -> Result<Arc<Lexicon>, WriterError> {
        self.lexicon_snapshot_for_project(project_id)
            .map(|(_, lexicon)| lexicon)
    }

    pub(super) fn lexicon_snapshot_for_project(
        &self,
        project_id: &str,
    ) -> Result<(u64, Arc<Lexicon>), WriterError> {
        let project_generation = self.activate_project_cache(project_id)?;
        let dictionary_entries = self.dictionary_entry_records(project_id)?;
        let entity_names = self.entity_name_records(project_id)?;
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

        {
            let active_project = self.inner.active_project.lock().map_err(|_| {
                WriterError::Other("active correction project lock poisoned".into())
            })?;
            if active_project.project_id.as_deref() == Some(project_id)
                && active_project.generation == project_generation
            {
                let cache = self.inner.lexicons.lock().map_err(|_| {
                    WriterError::Other("correction lexicon cache lock poisoned".into())
                })?;
                if let Some(cached) = cache.get(project_id) {
                    if cached.files == files
                        && cached.dictionary_entries == dictionary_entries
                        && cached.entity_names == entity_names
                    {
                        return Ok((cached.version, Arc::clone(&cached.lexicon)));
                    }
                }
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
            match entry.kind {
                CorrectionDictionaryEntryKindDto::Person => {
                    lexicon.insert_work_entity(
                        entry.text.clone(),
                        sujian_proof::ProperNameKind::Person,
                    );
                }
                CorrectionDictionaryEntryKindDto::Place => {
                    lexicon.insert_work_entity(
                        entry.text.clone(),
                        sujian_proof::ProperNameKind::Place,
                    );
                }
                CorrectionDictionaryEntryKindDto::Term => {
                    lexicon.insert_protected_word(entry.text.clone());
                }
            }
        }
        for relation in &entity_names {
            match relation.kind {
                CorrectionEntityNameKindDto::Alias => {
                    lexicon.insert_work_entity_alias(&relation.canonical, relation.name.clone());
                }
                CorrectionEntityNameKindDto::Misspelling => {
                    lexicon
                        .insert_work_entity_misspelling(&relation.canonical, relation.name.clone());
                }
            }
        }
        let lexicon = Arc::new(lexicon);

        // File loading can overlap a switch to another project. In that case
        // keep this snapshot local to the in-flight scanner instead of
        // repopulating the strong cache for a project that is no longer active.
        let active_project =
            self.inner.active_project.lock().map_err(|_| {
                WriterError::Other("active correction project lock poisoned".into())
            })?;
        if active_project.project_id.as_deref() != Some(project_id)
            || active_project.generation != project_generation
        {
            return Ok((self.next_lexicon_version()?, lexicon));
        }

        let mut cache = self
            .inner
            .lexicons
            .lock()
            .map_err(|_| WriterError::Other("correction lexicon cache lock poisoned".into()))?;
        if let Some(cached) = cache.get(project_id) {
            if cached.files == files
                && cached.dictionary_entries == dictionary_entries
                && cached.entity_names == entity_names
            {
                return Ok((cached.version, Arc::clone(&cached.lexicon)));
            }
        }

        let version = self.next_lexicon_version()?;
        let mut engines = self
            .inner
            .engines
            .lock()
            .map_err(|_| WriterError::Other("correction engine cache lock poisoned".into()))?;
        engines.remove(project_id);
        cache.insert(
            project_id.to_string(),
            CachedLexicon {
                files,
                dictionary_entries,
                entity_names,
                version,
                lexicon: Arc::clone(&lexicon),
            },
        );
        Ok((version, lexicon))
    }

    fn activate_project_cache(&self, project_id: &str) -> Result<u64, WriterError> {
        let mut active_project =
            self.inner.active_project.lock().map_err(|_| {
                WriterError::Other("active correction project lock poisoned".into())
            })?;
        if active_project.project_id.as_deref() == Some(project_id) {
            return Ok(active_project.generation);
        }
        let generation = active_project.generation.checked_add(1).ok_or_else(|| {
            WriterError::Other("active correction project generation exhausted".into())
        })?;

        // Keep the lock order consistent with cache publication. Dropping the
        // maps releases idle projects immediately; scans already holding their
        // own Arcs can finish without being interrupted.
        let mut lexicons = self
            .inner
            .lexicons
            .lock()
            .map_err(|_| WriterError::Other("correction lexicon cache lock poisoned".into()))?;
        let mut engines = self
            .inner
            .engines
            .lock()
            .map_err(|_| WriterError::Other("correction engine cache lock poisoned".into()))?;
        let mut result_caches = self.inner.result_caches.lock().map_err(|_| {
            WriterError::Other("correction result cache registry lock poisoned".into())
        })?;
        lexicons.clear();
        engines.clear();
        result_caches.retain(|(cached_project, _), _| cached_project.as_str() == project_id);
        active_project.project_id = Some(project_id.to_string());
        active_project.generation = generation;
        Ok(generation)
    }

    fn next_lexicon_version(&self) -> Result<u64, WriterError> {
        self.inner
            .next_lexicon_version
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |version| {
                version.checked_add(1)
            })
            .map_err(|_| WriterError::Other("correction lexicon version exhausted".into()))
    }

    pub(super) fn configured_model_snapshot(
        &self,
    ) -> Result<(Option<ModelCacheKey>, Option<Arc<KenlmModel>>), WriterError> {
        let mut models = model_cache()
            .lock()
            .map_err(|_| WriterError::Other("correction model cache lock poisoned".into()))?;
        let preferences = self
            .inner
            .preferences
            .lock()
            .map_err(|_| WriterError::Other("correction preferences lock poisoned".into()))?;
        let Some(path) = preferences.model_path.clone() else {
            return Ok((None, None));
        };
        let path = canonical_model_path(&path)?;
        let stamp = model_file_stamp(&path)?;
        if let Some(cached) = models.get(&path) {
            if cached.stamp == stamp {
                if let Some(model) = cached.model.upgrade() {
                    return Ok((Some(ModelCacheKey { path, stamp }), Some(model)));
                }
            }
        }
        let model = KenlmModel::open(&path)
            .map_err(|error| WriterError::Other(format!("invalid correction model: {error}")))?;
        let stamp_after = model_file_stamp(&path)?;
        if stamp_after != stamp {
            return Err(WriterError::Other(
                "correction model changed while it was being opened; retry the request".into(),
            ));
        }
        let model = Arc::new(model);
        models.insert(
            path.clone(),
            CachedModel {
                stamp,
                model: Arc::downgrade(&model),
            },
        );
        Ok((Some(ModelCacheKey { path, stamp }), Some(model)))
    }

    pub(super) fn cached_engine(
        &self,
        project_id: &str,
        lexicon_version: u64,
        model_key: Option<&ModelCacheKey>,
    ) -> Result<Option<Arc<CorrectionEngine>>, WriterError> {
        let active_project =
            self.inner.active_project.lock().map_err(|_| {
                WriterError::Other("active correction project lock poisoned".into())
            })?;
        if active_project.project_id.as_deref() != Some(project_id) {
            return Ok(None);
        }
        let lexicons = self
            .inner
            .lexicons
            .lock()
            .map_err(|_| WriterError::Other("correction lexicon cache lock poisoned".into()))?;
        if !lexicons
            .get(project_id)
            .is_some_and(|cached| cached.version == lexicon_version)
        {
            return Ok(None);
        }
        let engines = self
            .inner
            .engines
            .lock()
            .map_err(|_| WriterError::Other("correction engine cache lock poisoned".into()))?;
        Ok(engines.get(project_id).and_then(|cached| {
            (cached.lexicon_version == lexicon_version && cached.model_key.as_ref() == model_key)
                .then(|| Arc::clone(&cached.engine))
        }))
    }

    pub(super) fn cache_engine_if_model_current(
        &self,
        project_id: &str,
        lexicon_version: u64,
        model_key: Option<ModelCacheKey>,
        engine: Arc<CorrectionEngine>,
    ) -> Result<Arc<CorrectionEngine>, WriterError> {
        // Match set_model_path's lock order and keep the preference stable while
        // publishing an engine built from its model snapshot.
        let _models = model_cache()
            .lock()
            .map_err(|_| WriterError::Other("correction model cache lock poisoned".into()))?;
        let preferences = self
            .inner
            .preferences
            .lock()
            .map_err(|_| WriterError::Other("correction preferences lock poisoned".into()))?;
        let current_path = preferences
            .model_path
            .as_deref()
            .map(|path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
        if current_path != model_key.as_ref().map(|key| key.path.clone()) {
            return Ok(engine);
        }
        let active_project =
            self.inner.active_project.lock().map_err(|_| {
                WriterError::Other("active correction project lock poisoned".into())
            })?;
        if active_project.project_id.as_deref() != Some(project_id) {
            return Ok(engine);
        }
        let lexicons = self
            .inner
            .lexicons
            .lock()
            .map_err(|_| WriterError::Other("correction lexicon cache lock poisoned".into()))?;
        if !lexicons
            .get(project_id)
            .is_some_and(|cached| cached.version == lexicon_version)
        {
            return Ok(engine);
        }
        let mut engines = self
            .inner
            .engines
            .lock()
            .map_err(|_| WriterError::Other("correction engine cache lock poisoned".into()))?;
        if let Some(cached) = engines.get(project_id) {
            if cached.lexicon_version == lexicon_version && cached.model_key == model_key {
                return Ok(Arc::clone(&cached.engine));
            }
        }
        engines.insert(
            project_id.to_string(),
            CachedCorrectionEngine {
                lexicon_version,
                model_key,
                engine: Arc::clone(&engine),
            },
        );
        Ok(engine)
    }

    pub fn add_dictionary_entry(&self, project_id: &str, entry: &str) -> Result<bool, WriterError> {
        self.add_dictionary_entry_with_kind(
            project_id,
            entry,
            CorrectionDictionaryEntryKindDto::Term,
        )
    }

    pub fn add_dictionary_entry_with_kind(
        &self,
        project_id: &str,
        entry: &str,
        kind: CorrectionDictionaryEntryKindDto,
    ) -> Result<bool, WriterError> {
        let entry = entry.trim();
        if entry.is_empty() || entry.chars().any(char::is_control) {
            return Err(WriterError::Other(
                "dictionary entry must be non-empty text without control characters".into(),
            ));
        }
        if matches!(
            kind,
            CorrectionDictionaryEntryKindDto::Person | CorrectionDictionaryEntryKindDto::Place
        ) && !sujian_proof::is_valid_work_entity(entry)
        {
            return Err(WriterError::Other(
                "人物/地点词条必须是完整的作品名称（不超过 64 个字符）".into(),
            ));
        }
        self.update_preferences(|preferences| {
            let project = preferences
                .projects
                .entry(project_id.to_string())
                .or_default();
            let previous_kind = if project.dictionary_person_entries.contains(entry) {
                CorrectionDictionaryEntryKindDto::Person
            } else if project.dictionary_place_entries.contains(entry) {
                CorrectionDictionaryEntryKindDto::Place
            } else {
                CorrectionDictionaryEntryKindDto::Term
            };
            let was_present = project.dictionary_entries.contains(entry);
            project.dictionary_entries.insert(entry.to_string());
            // The type is exclusive: re-adding an existing word as a different
            // kind updates its meaning instead of keeping stale membership.
            project.dictionary_person_entries.remove(entry);
            project.dictionary_place_entries.remove(entry);
            match kind {
                CorrectionDictionaryEntryKindDto::Term => {}
                CorrectionDictionaryEntryKindDto::Person => {
                    project.dictionary_person_entries.insert(entry.to_string());
                }
                CorrectionDictionaryEntryKindDto::Place => {
                    project.dictionary_place_entries.insert(entry.to_string());
                }
            }
            !was_present || previous_kind != kind
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
