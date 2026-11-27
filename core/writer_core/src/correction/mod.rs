//! WriterCore adapter for the platform independent `sujian-proof` engine.
//!
//! Model paths, project dictionaries, and ignore choices are local correction
//! preferences. A model is never packaged by this module.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use sujian_proof::{CorrectionEngine, Lexicon, LexiconPaths, ProofreadingModel};

use sha2::Digest;

use crate::api::{
    AnnotationNamedEntryDto, CorrectionDictionaryEntryDto, CorrectionDictionaryEntryKindDto,
    CorrectionEntityNameDto, CorrectionEntityNameKindDto, CorrectionIgnoreScopeDto,
    CorrectionModelPathStatusDto, CorrectionResourceStatusDto, WriterError,
};

mod engine;
mod result_cache;
mod publication;
mod scan_metrics;
mod synced_results;

pub(crate) use engine::CorrectionScanner;
pub(crate) use synced_results::is_synced_result_path;
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

/// Bound for in-memory chapter correction caches. Idle slots are cheap to
/// reload from disk when a chapter is reopened; keeping every visited chapter
/// for the process lifetime is not (Issue #1061).
const CHAPTER_RESULT_CACHE_LIMIT: usize = 16;

/// One project's published annotation entries plus the source directory
/// version they were read from. External sync/MCP writers touch the same
/// `annotations/entries` files, so the version — not just "this process has
/// synced once" — decides whether the cache is still usable.
#[derive(Clone)]
struct CachedNamedEntries {
    entries: Vec<AnnotationNamedEntryDto>,
    source_stamp: String,
}

struct CorrectionStoreInner {
    path: PathBuf,
    /// 作品根目录。用于作品级校对结果的同步记录；`None` 表示该进程只做本地
    /// 缓存（`CorrectionStore::open` 的既有语义），不写同步记录。
    projects_root: Mutex<Option<PathBuf>>,
    preferences: Mutex<PersistedCorrectionPreferences>,
    /// Read-only projection of annotation NamedEntry records for proofreading.
    /// Writes go through AnnotationStore; this is only a process cache.
    named_entries: Mutex<HashMap<String, CachedNamedEntries>>,
    once_ignored: Mutex<HashSet<OnceIgnoredDiagnostic>>,
    active_project: Mutex<ActiveCorrectionProject>,
    next_lexicon_version: AtomicU64,
    lexicons: Mutex<HashMap<String, CachedLexicon>>,
    engines: Mutex<HashMap<String, CachedCorrectionEngine>>,
    result_caches: Mutex<ChapterResultCacheRegistry>,
    /// 章节同步结果索引：按目录修改时间增量刷新，restore 的 slot 锁内只查内存。
    synced_records: Mutex<HashMap<SyncedRecordCacheKey, CachedSyncedRecords>>,
    /// 待发布事实（完整 record）在内存中的失败回退；durable 文件在 publication 模块。
    pending_publications: Mutex<HashMap<(String, String), Vec<synced_results::SyncedCorrectionRecord>>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SyncedRecordCacheKey {
    project_id: String,
    chapter_id: String,
    mode: String,
    scanner_stable: String,
}

struct CachedSyncedRecords {
    dir_signature: Option<SystemTime>,
    index: Arc<synced_results::SyncedResultIndex>,
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
    named_entries: Vec<AnnotationNamedEntryDto>,
    /// 生成这份词库时读到的统一关系源版本（AnnotationStore 专名目录 stamp）。
    /// 增量/整表重建的判定见 [`CachedLexicon::additions`]。
    relation_version: String,
    version: u64,
    lexicon: Arc<Lexicon>,
}

/// 只新增、不删除/不改名的词表增量。
struct LexiconAdditions {
    dictionary: Vec<CorrectionDictionaryEntryDto>,
    entity_names: Vec<CorrectionEntityNameDto>,
    named_entities: Vec<String>,
}

impl CachedLexicon {
    /// 计算新快照相对这份缓存“只新增”的词条。
    ///
    /// Issue #1062 三.3：专名缓存按统一关系源版本增量更新。版本前进说明
    /// AnnotationStore 的专名/关系变了，这里只把新增的专名、别名与错拼关系
    /// 叠加到已有词库上；作者词典新增词条同理。任何被删除或改名的词条都返回
    /// `None`，交给调用方整表重建，避免旧关系残留在 `WorkEntityLexicon` 里。
    fn additions(
        &self,
        dictionary_entries: &[CorrectionDictionaryEntryDto],
        entity_names: &[CorrectionEntityNameDto],
        named_entries: &[AnnotationNamedEntryDto],
        relation_version: &str,
    ) -> Option<LexiconAdditions> {
        if self
            .dictionary_entries
            .iter()
            .any(|old| !dictionary_entries.contains(old))
        {
            return None;
        }
        // 改名/删除的专名会让已派生的别名与错拼关系失效，无法用增量表达。
        if self.named_entries.iter().any(|old| {
            !named_entries
                .iter()
                .any(|current| current.canonical_name == old.canonical_name)
        }) {
            return None;
        }
        let relation_changed = self.relation_version != relation_version;
        // 关系被删除/改名后旧别名必须从 WorkEntityLexicon 消失，无法用增量表达。
        if self
            .entity_names
            .iter()
            .any(|old| !entity_names.contains(old))
        {
            return None;
        }
        // 版本没变却多出关系，说明专名缓存与关系源不同步：宁可整表重建。
        if !relation_changed && self.entity_names.len() != entity_names.len() {
            return None;
        }
        let dictionary = dictionary_entries
            .iter()
            .filter(|entry| !self.dictionary_entries.contains(entry))
            .cloned()
            .collect::<Vec<_>>();
        let named_entities = named_entries
            .iter()
            .filter(|entry| {
                !self
                    .named_entries
                    .iter()
                    .any(|old| old.canonical_name == entry.canonical_name)
            })
            .map(|entry| entry.canonical_name.clone())
            .collect::<Vec<_>>();
        let entity_names = entity_names
            .iter()
            .filter(|relation| !self.entity_names.contains(relation))
            .cloned()
            .collect::<Vec<_>>();
        Some(LexiconAdditions {
            dictionary,
            entity_names,
            named_entities,
        })
    }

    /// 在缓存的 `Lexicon` 上只叠加新增词条；词库文件与基础词表完全不重读。
    fn overlaid(
        &self,
        additions: &LexiconAdditions,
        dictionary_entries: &[CorrectionDictionaryEntryDto],
    ) -> Arc<Lexicon> {
        let mut lexicon = (*self.lexicon).clone();
        for entry in &additions.dictionary {
            match entry.kind {
                CorrectionDictionaryEntryKindDto::Person => lexicon
                    .insert_work_entity(entry.text.clone(), sujian_proof::ProperNameKind::Person),
                CorrectionDictionaryEntryKindDto::Place => lexicon
                    .insert_work_entity(entry.text.clone(), sujian_proof::ProperNameKind::Place),
                CorrectionDictionaryEntryKindDto::Term => {
                    lexicon.insert_protected_word(entry.text.clone());
                }
            }
        }
        for canonical in &additions.named_entities {
            // 与整表重建同一规则：类型化词典已经贡献的专名不重复插入。
            if !dictionary_entries.iter().any(|entry| {
                entry.text == *canonical
                    && matches!(
                        entry.kind,
                        CorrectionDictionaryEntryKindDto::Person
                            | CorrectionDictionaryEntryKindDto::Place
                    )
            }) {
                lexicon.insert_work_entity(canonical.clone(), sujian_proof::ProperNameKind::Person);
            }
        }
        for relation in &additions.entity_names {
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
        Arc::new(lexicon)
    }
}

/// 一次词库快照的全部输入：词库文件戳、作者词典、专名关系与关系源版本。
struct LexiconSnapshotParts {
    files: Vec<LexiconFileStamp>,
    dictionary_entries: Vec<CorrectionDictionaryEntryDto>,
    entity_names: Vec<CorrectionEntityNameDto>,
    named_entries: Vec<AnnotationNamedEntryDto>,
    relation_version: String,
}

/// 词库缓存查询结果。
enum LexiconCacheOutcome {
    /// 缓存命中；返回前可能已刷新关系源版本戳。
    Hit(u64, Arc<Lexicon>),
    /// 只叠加新增词条得到的新词库，尚未发布。
    Incremental(Arc<Lexicon>),
    /// 必须整表重建。
    Miss,
}

/// 当前活动项目的词库缓存锁。
type ActiveLexiconCache<'a> = MutexGuard<'a, HashMap<String, CachedLexicon>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ModelFileStamp {
    len: u64,
    modified: Option<SystemTime>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ModelBundleStamp {
    model: ModelFileStamp,
    vocab: ModelFileStamp,
    tokenizer: ModelFileStamp,
    calibrator: Option<ModelFileStamp>,
}

/// Versioned identity of the exact model bundle an engine was built from.
/// The path alone is not enough: replacing any resource must invalidate both
/// the scanner result cache and the process-wide engine cache.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ModelIdentity {
    model_sha256: String,
    vocab_sha256: String,
    tokenizer_sha256: String,
    calibrator_version: String,
}

/// One model bundle's single-flight load state. Waiting callers park on the
/// condvar instead of hashing/opening the same 474 MB session twice.
struct ModelLoadSlot {
    state: Mutex<ModelLoadState>,
    wake: Condvar,
}

enum ModelLoadState {
    Empty,
    Loading,
    Ready {
        stamp: ModelBundleStamp,
        identity: ModelIdentity,
        model: Arc<ProofreadingModel>,
        /// Last scanner use, for the idle-release grace period. Active scans
        /// keep a clone of `model`, so `Arc::strong_count > 1` protects them.
        last_used: Instant,
    },
}

/// The model bundle captured for one scanner, plus the facts diagnostics need
/// to tell a cache hit apart from a real ONNX Runtime session initialization.
pub(super) struct ConfiguredModelSnapshot {
    pub key: Option<ModelCacheKey>,
    pub model: Option<Arc<ProofreadingModel>>,
    pub cache_hit: bool,
    pub session_initialized: bool,
    pub load_ms: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ModelCacheKey {
    path: PathBuf,
    stamp: ModelBundleStamp,
    identity: ModelIdentity,
}

struct CachedCorrectionEngine {
    lexicon_version: u64,
    model_key: Option<ModelCacheKey>,
    engine: Arc<CorrectionEngine>,
}

/// 一次 scanner 代际的两类身份：本地缓存指纹 + 跨设备稳定身份。
pub(super) struct ScannerSyncedIdentity {
    /// 既有 `scanner_fingerprint` 语义（含路径/mtime），本地缓存失效仍靠它。
    pub(super) fingerprint: String,
    /// 跨设备稳定 scanner 身份（路径无关的内容/配置事实）。
    pub(super) scanner_stable: String,
    pub(super) model_identity: String,
    pub(super) lexicon_identity: String,
    pub(super) rules_identity: String,
    /// 只按当前配置判断的模式；真正扫描时以实际构建的引擎为准。
    pub(super) configured_mode: &'static str,
}

/// A process-shared, app-data-root-scoped correction preferences store.
#[derive(Clone)]
pub struct CorrectionStore {
    inner: Arc<CorrectionStoreInner>,
}

type StoreRegistry = HashMap<PathBuf, Arc<CorrectionStoreInner>>;
static STORE_REGISTRY: OnceLock<Mutex<StoreRegistry>> = OnceLock::new();
type ModelSlotRegistry = HashMap<PathBuf, Arc<ModelLoadSlot>>;
static MODEL_CACHE: OnceLock<Mutex<ModelSlotRegistry>> = OnceLock::new();
type ModelIdentityCache = HashMap<(PathBuf, ModelBundleStamp), ModelIdentity>;
static MODEL_IDENTITY_CACHE: OnceLock<Mutex<ModelIdentityCache>> = OnceLock::new();
/// Bumped whenever the configured model resource is replaced or released.
/// A load that started before the bump must not publish its session.
static MODEL_CONFIG_GENERATION: AtomicU64 = AtomicU64::new(1);

fn model_cache() -> &'static Mutex<ModelSlotRegistry> {
    MODEL_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn model_slot(path: &Path) -> Result<Arc<ModelLoadSlot>, WriterError> {
    let _ = ensure_model_lifecycle_manager();
    let mut slots = model_cache()
        .lock()
        .map_err(|_| WriterError::Other("correction model cache lock poisoned".into()))?;
    Ok(Arc::clone(slots.entry(path.to_path_buf()).or_insert_with(
        || {
            Arc::new(ModelLoadSlot {
                state: Mutex::new(ModelLoadState::Empty),
                wake: Condvar::new(),
            })
        },
    )))
}

/// Drop engine-cache entries that point at a model resource the user no
/// longer has configured, unless a scan still owns the engine.
fn take_unconfigured_engines(
    inner: &CorrectionStoreInner,
    key: &Path,
) -> Vec<Arc<CorrectionEngine>> {
    let Ok(mut engines) = inner.engines.lock() else {
        return Vec::new();
    };
    let mut removed = Vec::new();
    engines.retain(|_, cached| {
        let matches_path =
            cached.model_key.as_ref().map(|model| &model.path) == Some(&key.to_path_buf());
        if matches_path && Arc::strong_count(&cached.engine) == 1 {
            removed.push(Arc::clone(&cached.engine));
            false
        } else {
            true
        }
    });
    removed
}

/// Move one idle model slot out and wake waiters so they reload from config.
fn take_model_slot(slot: &ModelLoadSlot) -> Option<Arc<ProofreadingModel>> {
    let Ok(mut state) = slot.state.lock() else {
        return None;
    };
    let previous = std::mem::replace(&mut *state, ModelLoadState::Empty);
    slot.wake.notify_all();
    match previous {
        ModelLoadState::Ready { model, .. } => Some(model),
        other => {
            *state = other;
            None
        }
    }
}

/// Wait for a single-flight model load without holding any other lock.
fn wait_model_slot<'a>(
    slot: &'a ModelLoadSlot,
    guard: MutexGuard<'a, ModelLoadState>,
) -> Result<MutexGuard<'a, ModelLoadState>, WriterError> {
    slot.wake
        .wait(guard)
        .map_err(|_| WriterError::Other("correction model slot lock poisoned".into()))
}

/// True when a live engine for this store still references the model bundle.
/// Used only to decide whether an unconfigured model may be released.
fn engine_references_model(inner: &CorrectionStoreInner, key: &Path) -> bool {
    let Ok(engines) = inner.engines.lock() else {
        return false;
    };
    engines.values().any(|cached| {
        cached.model_key.as_ref().map(|model| &model.path) == Some(&key.to_path_buf())
            && Arc::strong_count(&cached.engine) > 1
    })
}

/// Number of live ONNX sessions held by this process. Diagnostics only.
pub(super) fn model_instance_count() -> usize {
    let Ok(slots) = model_cache().lock() else {
        return 0;
    };
    let mut count = 0;
    for slot in slots.values() {
        let Ok(state) = slot.state.lock() else {
            continue;
        };
        if matches!(*state, ModelLoadState::Ready { .. }) {
            count += 1;
        }
    }
    count
}

/// Count engines that still hold a reference to a model session. This includes
/// engines whose model key has already been superseded in `MODEL_CACHE`, so
/// diagnostics can see in-flight/old sessions that `model_instance_count`
/// (cached slots only) does not count.
pub(super) fn engine_model_reference_count() -> usize {
    let Some(stores) = STORE_REGISTRY.get() else {
        return 0;
    };
    let Ok(stores) = stores.lock() else {
        return 0;
    };
    let mut count = 0;
    for inner in stores.values() {
        let Ok(engines) = inner.engines.lock() else {
            continue;
        };
        count += engines
            .values()
            .filter(|cached| cached.model_key.is_some())
            .count();
    }
    count
}

/// Snapshot of the session lifecycle used by diagnostics. `cached` counts Ready
/// slots; `engine_refs` counts engines that still hold a session (including old
/// generations), and `generation` identifies the current model configuration.
pub(super) struct ModelLifetimeMetrics {
    pub cached: usize,
    pub engine_refs: usize,
    pub retired_models: usize,
    pub retired_engines: usize,
    pub generation: u64,
}

pub(super) fn model_lifetime_metrics() -> ModelLifetimeMetrics {
    let (retired_models, retired_engines) = retired_model_resources()
        .lock()
        .map(|queue| {
            queue
                .iter()
                .fold((0usize, 0usize), |(models, engines), batch| {
                    (
                        models.saturating_add(batch.models.len()),
                        engines.saturating_add(batch.engines.len()),
                    )
                })
        })
        .unwrap_or((0, 0));
    ModelLifetimeMetrics {
        cached: model_instance_count(),
        engine_refs: engine_model_reference_count(),
        retired_models,
        retired_engines,
        generation: MODEL_CONFIG_GENERATION.load(Ordering::SeqCst),
    }
}

/// 当前进程的校对模型生命周期只读快照（Issue #1062 三.4）。
///
/// 平台诊断（`process.resource`）用它观察 ORT session 是否被重复创建或迟迟
/// 不释放。函数只读取现有缓存计数、淘汰队列长度和配置代数，不加载模型、
/// 不改变缓存；注册表未初始化时按“未配置”返回。
pub fn model_lifetime_snapshot() -> serde_json::Value {
    let metrics = model_lifetime_metrics();
    serde_json::json!({
        "cached": metrics.cached,
        "engine_refs": metrics.engine_refs,
        "retired_models": metrics.retired_models,
        "retired_engines": metrics.retired_engines,
        "generation": metrics.generation,
        "model_configured": model_path_configured(),
    })
}

/// 是否有任一已打开的 `CorrectionStore` 配置了模型路径。
///
/// 锁顺序与 `model_path_status` 一致（stores → preferences），诊断线程只做
/// 短暂只读；锁中毒或注册表未建立都按未配置处理，不 panic、不阻塞模型加载。
fn model_path_configured() -> bool {
    let Some(stores) = STORE_REGISTRY.get() else {
        return false;
    };
    let Ok(stores) = stores.lock() else {
        return false;
    };
    stores.values().any(|inner| {
        inner
            .preferences
            .lock()
            .is_ok_and(|preferences| preferences.model_path.is_some())
    })
}

/// How long a ready ONNX session may stay unused before opportunistic release.
/// Scans keep an `Arc<ProofreadingModel>` lease, so an active or queued scan is
/// never released; only a session with no engine/scan reference is collected.
const IDLE_MODEL_GRACE: Duration = Duration::from_secs(120);

/// Release every idle model session and invalidate loads that are still in
/// flight. Called only when the configured model is really replaced or the
/// resource is no longer in use.
fn model_identity_cache() -> &'static Mutex<ModelIdentityCache> {
    MODEL_IDENTITY_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Resolve a user path to the model resource directory. A directory is used as
/// is; `.../model.onnx` resolves to its parent. The old single-file KenLM
/// format is rejected with an explicit migration error and can never be passed
/// to ONNX Runtime.
fn canonical_model_path(path: &Path) -> Result<PathBuf, WriterError> {
    let path = path
        .canonicalize()
        .map_err(|error| WriterError::Io(error.to_string()))?;
    if path.is_dir() {
        return Ok(path);
    }
    if path.is_file()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("model.onnx"))
    {
        return path.parent().map(Path::to_path_buf).ok_or_else(|| {
            WriterError::Other("correction model has no resource directory".into())
        });
    }
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("klm"))
    {
        return Err(WriterError::Other(
            "此前的 KenLM .klm 校对模型已停用。请下载 MacBERT4CSC ONNX 模型资源目录（含 model.onnx、vocab.txt 与 tokenizer.json），重新选择模型目录以完成迁移。"
                .into(),
        ));
    }
    Err(WriterError::Other(
        "校对模型路径应为包含 model.onnx、vocab.txt、tokenizer.json 的资源目录".into(),
    ))
}

fn model_file_stamp(path: &Path) -> Result<ModelFileStamp, WriterError> {
    let metadata = std::fs::metadata(path).map_err(|error| WriterError::Io(error.to_string()))?;
    if !metadata.is_file() {
        return Err(WriterError::Other(format!(
            "correction model resource is not a file: {}",
            path.display()
        )));
    }
    Ok(ModelFileStamp {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn model_bundle_stamp(directory: &Path) -> Result<ModelBundleStamp, WriterError> {
    let calibrator_path = directory.join("calibrator.json");
    Ok(ModelBundleStamp {
        model: model_file_stamp(&directory.join("model.onnx"))?,
        vocab: model_file_stamp(&directory.join("vocab.txt"))?,
        tokenizer: model_file_stamp(&directory.join("tokenizer.json"))?,
        calibrator: std::fs::metadata(&calibrator_path)
            .ok()
            .and_then(|metadata| {
                metadata.is_file().then(|| ModelFileStamp {
                    len: metadata.len(),
                    modified: metadata.modified().ok(),
                })
            }),
    })
}

fn sha256_file(path: &Path) -> Result<String, WriterError> {
    let mut file = std::fs::File::open(path).map_err(|error| WriterError::Io(error.to_string()))?;
    let mut hasher = sha2::Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|error| WriterError::Io(error.to_string()))?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// 词典文件内容指纹缓存。键含长度与 mtime：文件内容变化时 mtime 会变，
/// 因此重复构造 scanner（含每章 open 的 restore-only 路径）不必反复整文件哈希。
type LexiconContentDigestCache = HashMap<(PathBuf, u64, Option<SystemTime>), String>;
static LEXICON_CONTENT_DIGESTS: OnceLock<Mutex<LexiconContentDigestCache>> = OnceLock::new();

fn lexicon_content_digest_cache() -> &'static Mutex<LexiconContentDigestCache> {
    LEXICON_CONTENT_DIGESTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lexicon_file_content_digest(
    path: &Path,
    stamp: &LexiconFileStamp,
) -> Result<String, WriterError> {
    let key = (
        path.to_path_buf(),
        stamp.len.unwrap_or_default(),
        stamp.modified,
    );
    {
        let cache = lexicon_content_digest_cache()
            .lock()
            .map_err(|_| WriterError::Other("correction lexicon digest cache poisoned".into()))?;
        if let Some(digest) = cache.get(&key) {
            return Ok(digest.clone());
        }
    }
    let digest = sha256_file(path)?;
    let mut cache = lexicon_content_digest_cache()
        .lock()
        .map_err(|_| WriterError::Other("correction lexicon digest cache poisoned".into()))?;
    cache.insert(key, digest.clone());
    Ok(digest)
}

/// 稳定身份用的 SHA-256 串联（与本地缓存用的 64 位 [`parts_fingerprint`] 区分）。
fn stable_parts_digest(parts: &[String]) -> String {
    let mut hasher = sha2::Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0x1f]);
    }
    format!("{:x}", hasher.finalize())
}

fn model_identity(
    directory: &Path,
    stamp: &ModelBundleStamp,
) -> Result<ModelIdentity, WriterError> {
    let key = (directory.to_path_buf(), stamp.clone());
    {
        let cache = model_identity_cache().lock().map_err(|_| {
            WriterError::Other("correction model identity cache lock poisoned".into())
        })?;
        if let Some(identity) = cache.get(&key) {
            return Ok(identity.clone());
        }
    }
    let identity = ModelIdentity {
        model_sha256: sha256_file(&directory.join("model.onnx"))?,
        vocab_sha256: sha256_file(&directory.join("vocab.txt"))?,
        tokenizer_sha256: sha256_file(&directory.join("tokenizer.json"))?,
        calibrator_version: match std::fs::read_to_string(directory.join("calibrator.json")) {
            Ok(content) => {
                sujian_proof::Calibrator::from_json(&content)
                    .map_err(|error| WriterError::Other(error.to_string()))?
                    .version
            }
            Err(_) => "embedded-calibrator".to_string(),
        },
    };
    let mut cache = model_identity_cache()
        .lock()
        .map_err(|_| WriterError::Other("correction model identity cache lock poisoned".into()))?;
    cache.insert(key, identity.clone());
    Ok(identity)
}

fn validate_model_candidate(path: &Path) -> Result<ModelCacheKey, WriterError> {
    // Selecting a model only validates its immutable resource identity. The
    // expensive ONNX Runtime session is created exactly once by the first
    // engine that consumes `configured_model_snapshot`; opening it here and
    // storing only a weak reference would load the 474 MB model twice.
    let path = canonical_model_path(path)?;
    let stamp_before = model_bundle_stamp(&path)?;
    let identity = model_identity(&path, &stamp_before)?;
    let stamp_after = model_bundle_stamp(&path)?;
    if stamp_before != stamp_after {
        return Err(WriterError::Other(
            "correction model changed while it was being validated; retry the request".into(),
        ));
    }
    Ok(ModelCacheKey {
        path,
        stamp: stamp_after,
        identity,
    })
}

fn model_bundle_identity_for_fingerprint(
    path: &Path,
) -> Result<(ModelBundleStamp, ModelIdentity), WriterError> {
    let directory = if path.is_dir() {
        path.to_path_buf()
    } else {
        canonical_model_path(path)?
    };
    let stamp = model_bundle_stamp(&directory)?;
    let identity = model_identity(&directory, &stamp)?;
    Ok((stamp, identity))
}

const CORRECTION_DICTIONARY_FILES: [&str; 6] = [
    "word_freq.txt",
    "custom_confusion.txt",
    "person_name.txt",
    "place_name.txt",
    "proper_name.txt",
    "ignored_words.txt",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelReleaseReason {
    Idle,
    ConfigurationReplaced,
}

impl ModelReleaseReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::ConfigurationReplaced => "configuration_replaced",
        }
    }
}

/// Resources moved out of the caches but not dropped yet. The lifecycle
/// manager thread drops them, so the GUI thread never destroys a large ONNX
/// Runtime session while switching chapters or changing settings.
struct RetiredModelResources {
    reason: ModelReleaseReason,
    generation: u64,
    idle_ms: Option<u128>,
    models: Vec<Arc<ProofreadingModel>>,
    engines: Vec<Arc<CorrectionEngine>>,
}

struct ModelManagerWake {
    state: Mutex<()>,
    condvar: Condvar,
}

static RETIRED_MODEL_RESOURCES: OnceLock<Mutex<Vec<RetiredModelResources>>> = OnceLock::new();
static MODEL_MANAGER_WAKE: OnceLock<ModelManagerWake> = OnceLock::new();
static MODEL_MANAGER_READY: OnceLock<bool> = OnceLock::new();

fn retired_model_resources() -> &'static Mutex<Vec<RetiredModelResources>> {
    RETIRED_MODEL_RESOURCES.get_or_init(|| Mutex::new(Vec::new()))
}

fn model_manager_wake() -> &'static ModelManagerWake {
    MODEL_MANAGER_WAKE.get_or_init(|| ModelManagerWake {
        state: Mutex::new(()),
        condvar: Condvar::new(),
    })
}

/// Start the per-process model lifecycle thread once. Returns false only when
/// the thread could not be spawned; callers then release synchronously so a
/// failed spawn cannot leak sessions forever.
fn ensure_model_lifecycle_manager() -> bool {
    *MODEL_MANAGER_READY.get_or_init(|| {
        let wake = model_manager_wake();
        match thread::Builder::new()
            .name("correction-model-lifecycle".to_string())
            .spawn(move || model_lifecycle_loop(wake))
        {
            Ok(_) => true,
            Err(error) => {
                log::warn!(
                    target: "correction",
                    "correction_model_lifecycle_spawn_failed: {error}"
                );
                false
            }
        }
    })
}

fn model_lifecycle_loop(wake: &'static ModelManagerWake) {
    loop {
        let guard = wake
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (guard, _) = wake
            .condvar
            .wait_timeout(guard, Duration::from_secs(15))
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        drop(guard);
        release_retired_model_resources("correction-model-lifecycle");
        if let Some(resources) = collect_idle_model_resources() {
            release_model_resources(resources, "correction-model-lifecycle");
        }
    }
}

fn release_retired_model_resources(thread_label: &'static str) {
    let batches = {
        let Ok(mut queue) = retired_model_resources().lock() else {
            return;
        };
        std::mem::take(&mut *queue)
    };
    for resources in batches {
        release_model_resources(resources, thread_label);
    }
}

/// Drop the actual ONNX Runtime owners. Must be called without any correction
/// cache lock held, from the lifecycle thread normally.
fn release_model_resources(resources: RetiredModelResources, thread_label: &'static str) {
    let model_count = resources.models.len();
    let engine_count = resources.engines.len();
    if model_count == 0 && engine_count == 0 {
        return;
    }
    let started = Instant::now();
    // Drop engines first: their predictors may hold the same model session.
    drop(resources.engines);
    drop(resources.models);
    let release_ms = started.elapsed().as_millis();
    let metrics = model_lifetime_metrics();
    let mut fields = BTreeMap::new();
    fields.insert(
        "reason".to_string(),
        serde_json::json!(resources.reason.as_str()),
    );
    fields.insert("thread".to_string(), serde_json::json!(thread_label));
    fields.insert("release_ms".to_string(), serde_json::json!(release_ms));
    fields.insert(
        "released_models".to_string(),
        serde_json::json!(model_count),
    );
    fields.insert(
        "released_engines".to_string(),
        serde_json::json!(engine_count),
    );
    if let Some(idle_ms) = resources.idle_ms {
        fields.insert("idle_ms".to_string(), serde_json::json!(idle_ms));
    }
    fields.insert(
        "model_generation".to_string(),
        serde_json::json!(resources.generation),
    );
    fields.insert(
        "model_cached".to_string(),
        serde_json::json!(metrics.cached),
    );
    fields.insert(
        "model_engine_refs".to_string(),
        serde_json::json!(metrics.engine_refs),
    );
    fields.insert(
        "model_retired_models".to_string(),
        serde_json::json!(metrics.retired_models),
    );
    fields.insert(
        "model_retired_engines".to_string(),
        serde_json::json!(metrics.retired_engines),
    );
    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        sequence: 0,
        session_id: String::new(),
        level: writer_diagnostics::DiagnosticLevel::Info,
        origin: writer_diagnostics::DiagnosticOrigin::App,
        event: "correction.model.released".to_string(),
        target: "correction.model".to_string(),
        message: None,
        fields,
    });
}

fn retire_model_resources(resources: RetiredModelResources) {
    if resources.models.is_empty() && resources.engines.is_empty() {
        return;
    }
    if !ensure_model_lifecycle_manager() {
        release_model_resources(resources, "caller-fallback");
        return;
    }
    if let Ok(mut queue) = retired_model_resources().lock() {
        queue.push(resources);
    }
    model_manager_wake().condvar.notify_all();
}

/// Collect idle Ready sessions and their idle engine-cache entries. Active
/// scanner/engine leases are left in place; the model slot keeps its `Arc`
/// until the scanner naturally finishes and the next manager pass runs.
fn collect_idle_model_resources() -> Option<RetiredModelResources> {
    let slots = model_cache().lock().ok()?;
    let now = Instant::now();
    let expired_paths: HashSet<PathBuf> = slots
        .iter()
        .filter_map(|(path, slot)| {
            let state = slot.state.lock().ok()?;
            match &*state {
                ModelLoadState::Ready { last_used, .. }
                    if now.saturating_duration_since(*last_used) >= IDLE_MODEL_GRACE =>
                {
                    Some(path.clone())
                }
                _ => None,
            }
        })
        .collect();
    if expired_paths.is_empty() {
        return None;
    }

    let mut engines = Vec::new();
    let mut active_paths: HashSet<PathBuf> = HashSet::new();
    if let Some(stores) = STORE_REGISTRY.get() {
        if let Ok(stores) = stores.lock() {
            for inner in stores.values() {
                let Ok(mut cache) = inner.engines.lock() else {
                    continue;
                };
                let keys: Vec<_> = cache
                    .iter()
                    .filter(|(_, cached)| {
                        cached
                            .model_key
                            .as_ref()
                            .is_some_and(|key| expired_paths.contains(&key.path))
                    })
                    .map(|(key, _)| key.clone())
                    .collect();
                for key in keys {
                    let Some(cached) = cache.get(&key) else {
                        continue;
                    };
                    let Some(model_key) = cached.model_key.as_ref() else {
                        continue;
                    };
                    if Arc::strong_count(&cached.engine) > 1 {
                        active_paths.insert(model_key.path.clone());
                        continue;
                    }
                    if let Some(cached) = cache.remove(&key) {
                        engines.push(cached.engine);
                    }
                }
            }
        }
    }

    let mut models = Vec::new();
    let mut idle_ms = 0u128;
    for path in &expired_paths {
        let Some(slot) = slots.get(path) else {
            continue;
        };
        let Ok(mut state) = slot.state.lock() else {
            continue;
        };
        let ModelLoadState::Ready {
            last_used, model, ..
        } = &*state
        else {
            continue;
        };
        let idle = last_used.elapsed();
        if idle < IDLE_MODEL_GRACE || active_paths.contains(path) || Arc::strong_count(model) != 1 {
            continue;
        }
        let previous = std::mem::replace(&mut *state, ModelLoadState::Empty);
        if let ModelLoadState::Ready { model, .. } = previous {
            idle_ms = idle_ms.max(idle.as_millis());
            models.push(model);
        }
        slot.wake.notify_all();
    }
    drop(slots);
    if models.is_empty() && engines.is_empty() {
        return None;
    }
    Some(RetiredModelResources {
        reason: ModelReleaseReason::Idle,
        generation: MODEL_CONFIG_GENERATION.load(Ordering::SeqCst),
        idle_ms: Some(idle_ms),
        models,
        engines,
    })
}

impl CorrectionStore {
    pub fn open(app_data_root: &Path) -> Result<Self, WriterError> {
        Self::open_inner(app_data_root, None)
    }

    /// 打开校对存储并绑定作品根目录，让扫描结果同时写入作品级同步记录。
    ///
    /// 进程注册表仍按 `app_data_root` 复用；已存在的 inner 若还没有作品根，
    /// 这里补上（只补一次），后续调用不再改变。
    pub fn open_with_projects_root(
        app_data_root: &Path,
        projects_root: &Path,
    ) -> Result<Self, WriterError> {
        Self::open_inner(app_data_root, Some(projects_root))
    }

    fn open_inner(app_data_root: &Path, projects_root: Option<&Path>) -> Result<Self, WriterError> {
        let registry = STORE_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()));
        let mut stores = registry
            .lock()
            .map_err(|_| WriterError::Other("correction store registry lock poisoned".into()))?;
        let key = app_data_root.to_path_buf();
        let existing = stores.get(&key).map(Arc::clone);
        let inner = if let Some(inner) = existing {
            inner
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
                projects_root: Mutex::new(projects_root.map(Path::to_path_buf)),
                preferences: Mutex::new(preferences),
                named_entries: Mutex::new(HashMap::new()),
                once_ignored: Mutex::new(HashSet::new()),
                active_project: Mutex::new(ActiveCorrectionProject {
                    project_id: None,
                    generation: 0,
                }),
                next_lexicon_version: AtomicU64::new(1),
                lexicons: Mutex::new(HashMap::new()),
                engines: Mutex::new(HashMap::new()),
                result_caches: Mutex::new(HashMap::new()),
                synced_records: Mutex::new(HashMap::new()),
                pending_publications: Mutex::new(HashMap::new()),
            });
            stores.insert(key.clone(), Arc::clone(&inner));
            inner
        };
        drop(stores);
        if let Some(projects_root) = projects_root {
            let mut root = inner
                .projects_root
                .lock()
                .map_err(|_| WriterError::Other("correction projects root lock poisoned".into()))?;
            if root.is_none() {
                *root = Some(projects_root.to_path_buf());
            }
        }
        Ok(Self { inner })
    }

    /// 作品根目录；未绑定作品根时返回 `None`（不写同步记录）。
    pub(super) fn project_root(&self, project_id: &str) -> Option<PathBuf> {
        let root = self.inner.projects_root.lock().ok()?;
        root.as_ref().map(|root| root.join(project_id))
    }

    /// 本章同步结果索引：按目录修改时间增量刷新。
    ///
    /// 目录读取与索引构建都在全局/章节锁之外；缓存只保存不可变索引，且并入
    /// 既有章节缓存上限（同章旧 scanner 代际在替换时淘汰）。
    pub(super) fn synced_records_for_chapter(
        &self,
        project_id: &str,
        chapter_id: &str,
        mode: &str,
        scanner_stable: &str,
    ) -> Result<Arc<synced_results::SyncedResultIndex>, WriterError> {
        let Some(project_root) = self.project_root(project_id) else {
            return Ok(Arc::new(synced_results::SyncedResultIndex::default()));
        };
        let Some(dir) = synced_results::chapter_records_dir(&project_root, chapter_id) else {
            return Ok(Arc::new(synced_results::SyncedResultIndex::default()));
        };
        let key = SyncedRecordCacheKey {
            project_id: project_id.to_string(),
            chapter_id: chapter_id.to_string(),
            mode: mode.to_string(),
            scanner_stable: scanner_stable.to_string(),
        };
        let signature = std::fs::metadata(&dir).and_then(|meta| meta.modified()).ok();
        {
            let cache = self.inner.synced_records.lock().map_err(|_| {
                WriterError::Other("correction synced record cache lock poisoned".into())
            })?;
            if let Some(entry) = cache.get(&key) {
                if entry.dir_signature == signature {
                    return Ok(Arc::clone(&entry.index));
                }
            }
        }
        // 读目录/反序列化不持有全局索引锁，restore 的章节 slot 锁也不在此处。
        let mut records = synced_results::load_records(
            &project_root,
            project_id,
            chapter_id,
            mode,
            scanner_stable,
        );
        let after = std::fs::metadata(&dir).and_then(|meta| meta.modified()).ok();
        if after != signature {
            records = synced_results::load_records(
                &project_root,
                project_id,
                chapter_id,
                mode,
                scanner_stable,
            );
        }
        let index = Arc::new(synced_results::SyncedResultIndex::build(&records));
        let mut cache = self.inner.synced_records.lock().map_err(|_| {
            WriterError::Other("correction synced record cache lock poisoned".into())
        })?;
        if let Some(existing) = cache.get(&key) {
            if existing.dir_signature == after {
                return Ok(Arc::clone(&existing.index));
            }
        }
        // 同一章的旧 scanner/mode 代际直接释放；总条目受既有章节缓存上限约束。
        cache.retain(|candidate, _| {
            candidate.project_id != key.project_id
                || candidate.chapter_id != key.chapter_id
                || (candidate.mode == key.mode && candidate.scanner_stable == key.scanner_stable)
        });
        while cache.len() >= CHAPTER_RESULT_CACHE_LIMIT {
            let Some(oldest) = cache.keys().next().cloned() else {
                break;
            };
            cache.remove(&oldest);
        }
        cache.insert(
            key,
            CachedSyncedRecords {
                dir_signature: after,
                index: Arc::clone(&index),
            },
        );
        Ok(index)
    }

    /// 扫描完成先把完整结果写进独立待发布文件；只有 durable 成功才返回 true。
    /// 首次写盘失败时保留内存副本，后续请求继续重试，不依赖 result cache。
    pub(super) fn enqueue_publication(
        &self,
        project_id: &str,
        chapter_id: &str,
        records: &[synced_results::SyncedCorrectionRecord],
    ) -> bool {
        if records.is_empty() {
            return true;
        }
        let Some(project_root) = self.project_root(project_id) else {
            // 未绑定作品根：只有本地 corpus，不存在同步待发布责任。
            return true;
        };
        match publication::write_pending(&project_root, project_id, chapter_id, records) {
            Ok(()) => true,
            Err(error) => {
                log::warn!(
                    "[correction] durable pending publication write failed chapter_digest={} error={}",
                    content_fingerprint(chapter_id),
                    error
                );
                if let Ok(mut memory) = self.inner.pending_publications.lock() {
                    let entry = memory
                        .entry((project_id.to_string(), chapter_id.to_string()))
                        .or_default();
                    for record in records {
                        if !entry.iter().any(|existing| {
                            existing.record_key() == record.record_key()
                        }) {
                            entry.push(record.clone());
                        }
                    }
                }
                false
            }
        }
    }

    /// 发布独立待发布事实；可在章节 slot 锁外、与当前正文/revision 无关地调用。
    pub(super) fn retry_publications(&self, project_id: &str, chapter_id: &str) -> bool {
        let Some(project_root) = self.project_root(project_id) else {
            return false;
        };
        let memory_records = self
            .inner
            .pending_publications
            .lock()
            .ok()
            .and_then(|mut memory| {
                memory
                    .remove(&(project_id.to_string(), chapter_id.to_string()))
            })
            .unwrap_or_default();
        if !memory_records.is_empty()
            && publication::write_pending(
                &project_root,
                project_id,
                chapter_id,
                &memory_records,
            )
            .is_err()
        {
            // 仍无法 durable 写回：把内存待办放回去，等下一次请求。
            if let Ok(mut memory) = self.inner.pending_publications.lock() {
                memory
                    .entry((project_id.to_string(), chapter_id.to_string()))
                    .or_default()
                    .extend(memory_records);
            }
            return false;
        }
        match publication::publish_pending(&project_root, project_id, chapter_id) {
            Ok(_) => {
                // 新记录文件可能与已缓存的目录 mtime 落在同一时间粒度；发布后
                // 显式释放该章的索引，下一次构造必然重新读取新文件。
                if let Ok(mut cache) = self.inner.synced_records.lock() {
                    cache.retain(|key, _| {
                        key.project_id != project_id || key.chapter_id != chapter_id
                    });
                }
                true
            }
            Err(error) => {
                log::warn!(
                    "[correction] pending publication retry failed chapter_digest={} error={}",
                    content_fingerprint(chapter_id),
                    error
                );
                false
            }
        }
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
        let key = validate_model_candidate(path)?;
        let slots = model_cache()
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

        // A real replacement (even the same path with different resources)
        // releases the old session and invalidates loads still in flight. The
        // next scanner opens exactly one new session.
        MODEL_CONFIG_GENERATION.fetch_add(1, Ordering::SeqCst);
        let generation = MODEL_CONFIG_GENERATION.load(Ordering::SeqCst);
        let mut retired_models = Vec::new();
        for slot in slots.values() {
            let Ok(mut state) = slot.state.lock() else {
                continue;
            };
            if matches!(*state, ModelLoadState::Loading) {
                slot.wake.notify_all();
                continue;
            }
            let previous = std::mem::replace(&mut *state, ModelLoadState::Empty);
            if let ModelLoadState::Ready { model, .. } = previous {
                retired_models.push(model);
            }
            slot.wake.notify_all();
        }
        let retired_engines: Vec<Arc<CorrectionEngine>> =
            engines.drain().map(|(_, cached)| cached.engine).collect();
        *preferences = next;
        drop(engines);
        drop(preferences);
        drop(slots);
        retire_model_resources(RetiredModelResources {
            reason: ModelReleaseReason::ConfigurationReplaced,
            generation,
            idle_ms: None,
            models: retired_models,
            engines: retired_engines,
        });
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

    /// Publish a chapter/document leave signal without touching model locks or
    /// destroying an ONNX session on the GUI thread. The lifecycle thread
    /// decides whether an idle session is actually past its grace deadline.
    /// Resolve the process-shared slot for one exact model key. The scanner
    /// keeps this handle so per-window activity updates do not lock or traverse
    /// the process-wide path cache.
    pub(super) fn model_slot_for_key(key: &ModelCacheKey) -> Option<Arc<ModelLoadSlot>> {
        let slots = model_cache().lock().ok()?;
        slots.get(&key.path).cloned()
    }

    /// Extend the idle deadline of exactly the model this scanner is using.
    /// Scanning model A must not keep an unrelated idle model B alive; the
    /// stamp/identity check also rejects a slot replaced by a newer generation.
    pub(super) fn note_model_activity(&self, slot: &ModelLoadSlot, key: &ModelCacheKey) {
        let Ok(mut state) = slot.state.lock() else {
            return;
        };
        if let ModelLoadState::Ready {
            stamp,
            identity,
            last_used,
            ..
        } = &mut *state
        {
            if *stamp == key.stamp && *identity == key.identity {
                *last_used = Instant::now();
            }
        }
    }

    pub fn note_correction_session_left(&self) {
        let mut fields = BTreeMap::new();
        fields.insert(
            "model_generation".to_string(),
            serde_json::json!(MODEL_CONFIG_GENERATION.load(Ordering::SeqCst)),
        );
        writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            sequence: 0,
            session_id: String::new(),
            level: writer_diagnostics::DiagnosticLevel::Info,
            origin: writer_diagnostics::DiagnosticOrigin::App,
            event: "correction.model.session_left".to_string(),
            target: "correction.model".to_string(),
            message: None,
            fields,
        });
        model_manager_wake().condvar.notify_all();
    }

    pub fn resource_status(&self) -> Result<CorrectionResourceStatusDto, WriterError> {
        let model_path = self.configured_model_path()?;
        let model_migration_required = model_path.as_ref().is_some_and(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("klm"))
        });
        let model_file_available = model_path.as_ref().is_some_and(|path| {
            path.is_dir()
                && std::fs::metadata(path.join("model.onnx"))
                    .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
                && std::fs::metadata(path.join("vocab.txt"))
                    .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
                && std::fs::metadata(path.join("tokenizer.json"))
                    .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
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
            model_migration_required,
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

        let mut retired_engines = Vec::new();
        if !configured {
            for inner in stores.values() {
                retired_engines.extend(take_unconfigured_engines(inner, &key));
            }
        }

        // The process-shared session is a strong cache: it only counts as
        // "in use" while a scan still owns its engine or a load is running.
        // Otherwise an unconfigured model can be released for deletion.
        let engine_in_use = stores
            .values()
            .any(|inner| engine_references_model(inner, &key));
        let loading = models.get(&key).is_some_and(|slot| {
            slot.state
                .lock()
                .is_ok_and(|state| matches!(*state, ModelLoadState::Loading))
        });
        let in_use = engine_in_use || loading;
        let mut retired_model = None;
        if !configured && !in_use {
            MODEL_CONFIG_GENERATION.fetch_add(1, Ordering::SeqCst);
            if let Some(slot) = models.remove(&key) {
                retired_model = take_model_slot(&slot);
            }
        }

        let status = CorrectionModelPathStatusDto {
            configured,
            in_use,
            deletion_eligible: !configured && !in_use,
        };
        drop(stores);
        drop(models);
        if retired_model.is_some() || !retired_engines.is_empty() {
            retire_model_resources(RetiredModelResources {
                reason: ModelReleaseReason::ConfigurationReplaced,
                generation: MODEL_CONFIG_GENERATION.load(Ordering::SeqCst),
                idle_ms: None,
                models: retired_model.into_iter().collect(),
                engines: retired_engines,
            });
        }
        Ok(status)
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
        if let Some(cached) = self
            .inner
            .named_entries
            .lock()
            .map_err(|_| WriterError::Other("correction named entry cache lock poisoned".into()))?
            .get(project_id)
            .cloned()
        {
            let entries = cached.entries;
            let mut records = entries
                .iter()
                .flat_map(|entry| {
                    entry
                        .aliases
                        .iter()
                        .map(|name| CorrectionEntityNameDto {
                            canonical: entry.canonical_name.clone(),
                            name: name.clone(),
                            kind: CorrectionEntityNameKindDto::Alias,
                        })
                        .chain(entry.confirmed_misspellings.iter().map(|name| {
                            CorrectionEntityNameDto {
                                canonical: entry.canonical_name.clone(),
                                name: name.clone(),
                                kind: CorrectionEntityNameKindDto::Misspelling,
                            }
                        }))
                })
                .collect::<Vec<_>>();
            records.sort();
            return Ok(records);
        }
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

    /// Supply the canonical annotation entries used by the correction engine.
    /// This cache never persists names; AnnotationStore remains the single
    /// writable source of truth.
    ///
    /// A read-only refresh calls this only when the source directory version
    /// changed. Only a real entry change may invalidate the strong correction
    /// engine; rebuilding it unconditionally dropped the model's last strong
    /// reference and reopened the ONNX session hundreds of milliseconds later.
    ///
    /// Issue #1062 三.3：关系源版本变化不再直接丢弃词库缓存。下一次
    /// [`Self::lexicon_snapshot_for_project`] 会用 [`CachedLexicon::additions`]
    /// 判断这次变化能否只叠加新专名；只有删除/改名才整表重建。
    pub(crate) fn set_named_entries_for_project(
        &self,
        project_id: &str,
        mut entries: Vec<AnnotationNamedEntryDto>,
        source_stamp: String,
    ) -> Result<bool, WriterError> {
        // Stable order keeps equality independent of AnnotationStore file order.
        entries.sort_by(|left, right| left.entry_id.cmp(&right.entry_id));
        {
            let mut guard = self.inner.named_entries.lock().map_err(|_| {
                WriterError::Other("correction named entry cache lock poisoned".into())
            })?;
            if let Some(cached) = guard.get_mut(project_id) {
                if cached.entries == entries {
                    // Same names, but the source directory was rewritten (for
                    // example by sync). Record the new version so the next
                    // request skips the read; the lexicon/engine stay valid.
                    cached.source_stamp = source_stamp;
                    return Ok(false);
                }
            }
            guard.insert(
                project_id.to_string(),
                CachedNamedEntries {
                    entries,
                    source_stamp,
                },
            );
        }
        // The engine embeds the previous lexicon snapshot, so it must never be
        // reused after a relation change. The lexicon itself is kept until the
        // next snapshot decides between an incremental overlay and a rebuild.
        if let Ok(mut engines) = self.inner.engines.lock() {
            engines.remove(project_id);
        }
        Ok(true)
    }

    /// True when the published annotation entries came from the currently
    /// observed source version. A missing or outdated cache means a read-only
    /// caller must read AnnotationStore again; annotation writes from another
    /// process (sync/MCP) change the stamp without going through this process.
    pub(crate) fn named_entries_initialized(
        &self,
        project_id: &str,
        source_stamp: &str,
    ) -> Result<bool, WriterError> {
        Ok(self
            .inner
            .named_entries
            .lock()
            .map_err(|_| WriterError::Other("correction named entry cache lock poisoned".into()))?
            .get(project_id)
            .is_some_and(|cached| cached.source_stamp == source_stamp))
    }

    /// Return the legacy name relations solely for the one-time import into
    /// annotations/entries. The old table is cleared after that import.
    pub(crate) fn legacy_entity_name_records(
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

    pub(crate) fn clear_legacy_entity_name_records(
        &self,
        project_id: &str,
    ) -> Result<(), WriterError> {
        self.update_preferences(|preferences| {
            if let Some(project) = preferences.projects.get_mut(project_id) {
                project.entity_names.clear();
            }
        })
    }

    /// Remove proper-entity dictionary rows after their canonical names have
    /// been migrated to project annotation entries. Ordinary protected terms
    /// stay in the correction preferences store.
    pub(crate) fn clear_legacy_typed_entity_dictionary_entries(
        &self,
        project_id: &str,
    ) -> Result<(), WriterError> {
        self.update_preferences(|preferences| {
            if let Some(project) = preferences.projects.get_mut(project_id) {
                let typed_entities = project
                    .dictionary_person_entries
                    .iter()
                    .chain(project.dictionary_place_entries.iter())
                    .cloned()
                    .collect::<std::collections::HashSet<_>>();
                project
                    .dictionary_entries
                    .retain(|entry| !typed_entities.contains(entry));
                project.dictionary_person_entries.clear();
                project.dictionary_place_entries.clear();
            }
        })
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
        caches.insert(key.clone(), Arc::clone(&state));
        // Evict idle chapter slots beyond the limit. A slot still owned by a
        // scanner/request has strong_count > 1 and is left untouched; evicted
        // slots reload from their disk cache on the next open.
        if caches.len() > CHAPTER_RESULT_CACHE_LIMIT {
            let mut idle: Vec<(String, String)> = caches
                .iter()
                .filter(|(cached_key, slot)| **cached_key != key && Arc::strong_count(slot) == 1)
                .map(|(cached_key, _)| cached_key.clone())
                .collect();
            idle.sort();
            let excess = caches.len().saturating_sub(CHAPTER_RESULT_CACHE_LIMIT);
            let evicted = idle.len().min(excess);
            for cached_key in idle.into_iter().take(evicted) {
                caches.remove(&cached_key);
            }
            if evicted > 0 {
                let mut fields = BTreeMap::new();
                fields.insert("evicted".to_string(), serde_json::json!(evicted));
                fields.insert("remaining".to_string(), serde_json::json!(caches.len()));
                fields.insert(
                    "limit".to_string(),
                    serde_json::json!(CHAPTER_RESULT_CACHE_LIMIT),
                );
                writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                    sequence: 0,
                    session_id: String::new(),
                    level: writer_diagnostics::DiagnosticLevel::Info,
                    origin: writer_diagnostics::DiagnosticOrigin::App,
                    event: "correction.cache.evicted".to_string(),
                    target: "correction.cache".to_string(),
                    message: None,
                    fields,
                });
            }
        }
        Ok(state)
    }

    /// Latest `(session, revision)` already visible to a chapter's correction
    /// slot, without creating or loading a cache. A long model scan uses this
    /// between windows to stop early when a newer edit arrived; the dirty
    /// windows the edit recorded stay untouched for the next pending scan.
    pub(super) fn chapter_revision_progress(
        &self,
        project_id: &str,
        chapter_id: &str,
    ) -> Result<Option<(u64, u64)>, WriterError> {
        let key = (project_id.to_string(), chapter_id.to_string());
        let caches = self.inner.result_caches.lock().map_err(|_| {
            WriterError::Other("correction result cache registry lock poisoned".into())
        })?;
        let Some(slot) = caches.get(&key) else {
            return Ok(None);
        };
        let guard = slot
            .state
            .lock()
            .map_err(|_| WriterError::Other("correction result cache lock poisoned".into()))?;
        Ok(Some((guard.last_session_id, guard.last_revision)))
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
        Ok(self.scanner_identity(project_id)?.fingerprint)
    }

    /// 同时产出两类身份：
    ///
    /// - `fingerprint`：既有本地缓存指纹，含模型/词典路径与 mtime，语义不变；
    /// - 稳定身份（model/lexicon/rules + `scanner_stable`）：不含绝对路径、
    ///   mtime、设备或时间戳，供跨设备校对结果记录使用。
    pub(super) fn scanner_identity(
        &self,
        project_id: &str,
    ) -> Result<ScannerSyncedIdentity, WriterError> {
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
            "correction-scanner-v11".to_string(),
            "correction-work-entities-v3".to_string(),
            model_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| "no-model".to_string()),
        ];
        let model_identity = if let Some(path) = &model_path {
            match model_bundle_identity_for_fingerprint(path) {
                Ok((stamp, identity)) => {
                    parts.push(format!(
                        "model:{}:{}:{}:{}:{}:{}",
                        stamp.model.len,
                        modified_stamp(stamp.model.modified),
                        identity.model_sha256,
                        identity.vocab_sha256,
                        identity.tokenizer_sha256,
                        identity.calibrator_version
                    ));
                    format!(
                        "{}:{}:{}:{}",
                        identity.model_sha256,
                        identity.vocab_sha256,
                        identity.tokenizer_sha256,
                        identity.calibrator_version
                    )
                }
                Err(_) => {
                    parts.push("model:missing".to_string());
                    "model:missing".to_string()
                }
            }
        } else {
            "no-model".to_string()
        };
        let data_root = self
            .inner
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("correction-data");
        let mut lexicon_parts = Vec::new();
        for name in CORRECTION_DICTIONARY_FILES {
            let (path, stamp) = optional_lexicon_file(&data_root, name)?;
            parts.push(format!(
                "lexicon:{name}:{:?}:{}",
                stamp.len,
                modified_stamp(stamp.modified)
            ));
            match path.as_deref().filter(|_| stamp.len.is_some()) {
                Some(path) => lexicon_parts.push(format!(
                    "lexicon:{name}:{}",
                    lexicon_file_content_digest(path, &stamp)?
                )),
                None => lexicon_parts.push(format!("lexicon:{name}:missing")),
            }
        }
        for entry in self.dictionary_entry_records(project_id)? {
            parts.push(format!("dictionary:{:?}:{}", entry.kind, entry.text));
            lexicon_parts.push(format!("dictionary:{:?}:{}", entry.kind, entry.text));
        }
        for relation in self.entity_name_records(project_id)? {
            parts.push(format!(
                "entity-name:{:?}:{}:{}",
                relation.kind, relation.canonical, relation.name
            ));
            lexicon_parts.push(format!(
                "entity-name:{:?}:{}:{}",
                relation.kind, relation.canonical, relation.name
            ));
        }
        let mut named_entries = Vec::new();
        if let Some(cached) = self
            .inner
            .named_entries
            .lock()
            .map_err(|_| WriterError::Other("correction named entry cache lock poisoned".into()))?
            .get(project_id)
        {
            for entry in &cached.entries {
                parts.push(format!("named-entry:{}", entry.canonical_name));
                named_entries.push(entry.canonical_name.clone());
            }
        }
        // 专名缓存按 entry_id 排序，跨设备的 entry_id 顺序可能不同；稳定身份
        // 只按专名本身排序，保证同一词库在两端得到同一个身份。
        named_entries.sort();
        for name in named_entries {
            lexicon_parts.push(format!("named-entry:{name}"));
        }
        let rules_identity = disabled_rules.join("|");
        for rule in &disabled_rules {
            parts.push(format!("disabled-rule:{rule}"));
        }
        let lexicon_identity = stable_parts_digest(&lexicon_parts);
        let scanner_stable = synced_results::scanner_stable_identity(
            &model_identity,
            &lexicon_identity,
            &rules_identity,
        );
        // 只有模型 bundle 实际可用时才算 model 模式；路径配置了但文件缺失
        // 的规则降级不能被 restore-only/缓存当作模型覆盖。
        let model_mode_available = model_path.is_some() && model_identity != "model:missing";
        Ok(ScannerSyncedIdentity {
            fingerprint: parts_fingerprint(&parts),
            scanner_stable,
            model_identity,
            lexicon_identity,
            rules_identity,
            configured_mode: if model_mode_available {
                synced_results::MODE_MODEL
            } else {
                synced_results::MODE_RULES
            },
        })
    }

    pub fn lexicon_for_project(&self, project_id: &str) -> Result<Arc<Lexicon>, WriterError> {
        self.lexicon_snapshot_for_project(project_id)
            .map(|(_, lexicon)| lexicon)
    }

    /// 锁住当前活动项目的词库缓存；项目已切换或代数不符时返回 `None`。
    fn active_lexicon_cache(
        &self,
        project_id: &str,
        project_generation: u64,
    ) -> Result<Option<ActiveLexiconCache<'_>>, WriterError> {
        let active_project =
            self.inner.active_project.lock().map_err(|_| {
                WriterError::Other("active correction project lock poisoned".into())
            })?;
        if active_project.project_id.as_deref() != Some(project_id)
            || active_project.generation != project_generation
        {
            return Ok(None);
        }
        self.inner
            .lexicons
            .lock()
            .map(Some)
            .map_err(|_| WriterError::Other("correction lexicon cache lock poisoned".into()))
    }

    /// 命中缓存或算出只叠新增词的词库；两者都不成立时要求整表重建。
    fn cached_lexicon_snapshot(
        &self,
        project_id: &str,
        project_generation: u64,
        parts: &LexiconSnapshotParts,
    ) -> Result<LexiconCacheOutcome, WriterError> {
        let Some(mut cache) = self.active_lexicon_cache(project_id, project_generation)? else {
            return Ok(LexiconCacheOutcome::Miss);
        };
        let Some(cached) = cache.get_mut(project_id) else {
            return Ok(LexiconCacheOutcome::Miss);
        };
        if cached.files != parts.files {
            return Ok(LexiconCacheOutcome::Miss);
        }
        if cached.dictionary_entries == parts.dictionary_entries
            && cached.entity_names == parts.entity_names
            && cached.named_entries == parts.named_entries
        {
            // 名字没有变化，只有 sync/MCP 重写了关系源目录：只更新版本戳，
            // 词库与引擎保持有效。
            if cached.relation_version != parts.relation_version {
                cached.relation_version = parts.relation_version.clone();
            }
            return Ok(LexiconCacheOutcome::Hit(
                cached.version,
                Arc::clone(&cached.lexicon),
            ));
        }
        let Some(additions) = cached.additions(
            &parts.dictionary_entries,
            &parts.entity_names,
            &parts.named_entries,
            &parts.relation_version,
        ) else {
            return Ok(LexiconCacheOutcome::Miss);
        };
        Ok(LexiconCacheOutcome::Incremental(
            cached.overlaid(&additions, &parts.dictionary_entries),
        ))
    }

    /// 发布叠加后的词库：新版本号让旧引擎失效，词库文件保持原样。
    fn publish_incremental_lexicon(
        &self,
        project_id: &str,
        parts: LexiconSnapshotParts,
        lexicon: Arc<Lexicon>,
    ) -> Result<u64, WriterError> {
        let version = self.next_lexicon_version()?;
        let mut cache = self
            .inner
            .lexicons
            .lock()
            .map_err(|_| WriterError::Other("correction lexicon cache lock poisoned".into()))?;
        let mut engines = self
            .inner
            .engines
            .lock()
            .map_err(|_| WriterError::Other("correction engine cache lock poisoned".into()))?;
        engines.remove(project_id);
        cache.insert(
            project_id.to_string(),
            CachedLexicon {
                files: parts.files,
                dictionary_entries: parts.dictionary_entries,
                entity_names: parts.entity_names,
                named_entries: parts.named_entries,
                relation_version: parts.relation_version,
                version,
                lexicon: Arc::clone(&lexicon),
            },
        );
        Ok(version)
    }

    pub(super) fn lexicon_snapshot_for_project(
        &self,
        project_id: &str,
    ) -> Result<(u64, Arc<Lexicon>), WriterError> {
        let project_generation = self.activate_project_cache(project_id)?;
        let dictionary_entries = self.dictionary_entry_records(project_id)?;
        let entity_names = self.entity_name_records(project_id)?;
        // 关系源版本与专名记录在同一次加锁里读出，增量判断不会把两个版本的
        // 数据混在一起（issue #1062 三.3）。
        let (named_entries, relation_version) = {
            let guard = self.inner.named_entries.lock().map_err(|_| {
                WriterError::Other("correction named entry cache lock poisoned".into())
            })?;
            match guard.get(project_id) {
                Some(cached) => (cached.entries.clone(), cached.source_stamp.clone()),
                None => (Vec::new(), String::new()),
            }
        };
        let data_root = self
            .inner
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("correction-data");

        let (word_freq, word_freq_stamp) = optional_lexicon_file(&data_root, "word_freq.txt")?;
        let (custom_confusion, custom_confusion_stamp) =
            optional_lexicon_file(&data_root, "custom_confusion.txt")?;
        let (person_name, person_name_stamp) =
            optional_lexicon_file(&data_root, "person_name.txt")?;
        let (place_name, place_name_stamp) = optional_lexicon_file(&data_root, "place_name.txt")?;
        let (proper_name, proper_name_stamp) =
            optional_lexicon_file(&data_root, "proper_name.txt")?;
        let (ignored_words, ignored_words_stamp) =
            optional_lexicon_file(&data_root, "ignored_words.txt")?;
        let parts = LexiconSnapshotParts {
            files: vec![
                word_freq_stamp,
                custom_confusion_stamp,
                person_name_stamp,
                place_name_stamp,
                proper_name_stamp,
                ignored_words_stamp,
            ],
            dictionary_entries,
            entity_names,
            named_entries,
            relation_version,
        };

        match self.cached_lexicon_snapshot(project_id, project_generation, &parts)? {
            LexiconCacheOutcome::Hit(version, lexicon) => return Ok((version, lexicon)),
            LexiconCacheOutcome::Incremental(lexicon) => {
                let version =
                    self.publish_incremental_lexicon(project_id, parts, Arc::clone(&lexicon))?;
                return Ok((version, lexicon));
            }
            LexiconCacheOutcome::Miss => {}
        }

        let mut lexicon = Lexicon::from_paths(&LexiconPaths {
            word_freq,
            custom_confusion,
            person_name,
            place_name,
            proper_name,
            ignored_words,
        })
        .map_err(|error| WriterError::Io(error.to_string()))?;
        for entry in &parts.dictionary_entries {
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
        for entry in &parts.named_entries {
            if !parts.dictionary_entries.iter().any(|dictionary_entry| {
                dictionary_entry.text == entry.canonical_name
                    && matches!(
                        dictionary_entry.kind,
                        CorrectionDictionaryEntryKindDto::Person
                            | CorrectionDictionaryEntryKindDto::Place
                    )
            }) {
                // The proof engine currently classifies proper entities as
                // person/place names. The annotation model intentionally has
                // no such tag; Person is only its internal protected-name lane.
                lexicon.insert_work_entity(
                    entry.canonical_name.clone(),
                    sujian_proof::ProperNameKind::Person,
                );
            }
        }
        for relation in &parts.entity_names {
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
            if cached.files == parts.files
                && cached.dictionary_entries == parts.dictionary_entries
                && cached.entity_names == parts.entity_names
                && cached.named_entries == parts.named_entries
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
                files: parts.files,
                dictionary_entries: parts.dictionary_entries,
                entity_names: parts.entity_names,
                named_entries: parts.named_entries,
                relation_version: parts.relation_version,
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

    /// Resolve the configured model into a single process-shared session.
    ///
    /// The expensive build (file hashing, ONNX session creation) runs outside
    /// every global lock. Concurrent scanners that want the same bundle wait
    /// on one single-flight slot; a real replacement/unconfiguration bumps
    /// `MODEL_CONFIG_GENERATION` so a load that started earlier never
    /// publishes its stale session.
    pub(super) fn configured_model_snapshot(&self) -> Result<ConfiguredModelSnapshot, WriterError> {
        let started = Instant::now();
        let configured_path = self.configured_model_path()?;
        let Some(configured_path) = configured_path else {
            return Ok(ConfiguredModelSnapshot {
                key: None,
                model: None,
                cache_hit: false,
                session_initialized: false,
                load_ms: 0,
            });
        };
        let path = canonical_model_path(&configured_path)?;
        let stamp_before = model_bundle_stamp(&path)?;
        let identity = model_identity(&path, &stamp_before)?;
        let slot = model_slot(&path)?;

        // Fast path, or wait for a concurrent load of the same bundle. A load
        // of a different stamp is also awaited so the process never holds two
        // sessions for one path beyond in-flight scan Arcs.
        let mut state = slot
            .state
            .lock()
            .map_err(|_| WriterError::Other("correction model slot lock poisoned".into()))?;
        loop {
            match &mut *state {
                ModelLoadState::Ready {
                    stamp,
                    identity: ready_identity,
                    model,
                    last_used,
                } if *stamp == stamp_before => {
                    *last_used = Instant::now();
                    let snapshot = ConfiguredModelSnapshot {
                        key: Some(ModelCacheKey {
                            path: path.clone(),
                            stamp: stamp.clone(),
                            identity: ready_identity.clone(),
                        }),
                        model: Some(Arc::clone(model)),
                        cache_hit: true,
                        session_initialized: false,
                        load_ms: started.elapsed().as_millis(),
                    };
                    return Ok(snapshot);
                }
                ModelLoadState::Loading => {
                    state = wait_model_slot(&slot, state)?;
                }
                _ => break,
            }
        }
        *state = ModelLoadState::Loading;
        drop(state);
        self.initialize_model_session(path, stamp_before, identity, slot, started)
    }

    /// Open one ONNX session outside every global lock, then publish it only
    /// if the model configuration and the bundle stamp are still current.
    fn initialize_model_session(
        &self,
        path: PathBuf,
        stamp_before: ModelBundleStamp,
        identity: ModelIdentity,
        slot: Arc<ModelLoadSlot>,
        started: Instant,
    ) -> Result<ConfiguredModelSnapshot, WriterError> {
        let generation = MODEL_CONFIG_GENERATION.load(Ordering::SeqCst);
        let opened = ProofreadingModel::open(&path)
            .map_err(|error| WriterError::Other(format!("invalid correction model: {error}")));
        let publishable = opened.is_ok();
        let stamp_after = if publishable {
            model_bundle_stamp(&path)
        } else {
            Ok(stamp_before.clone())
        };
        // Read preferences without the slot lock: `set_model_path` takes that
        // order and waiting on it from inside the slot state would deadlock.
        let configured_unchanged_before_lock = self
            .configured_model_path()?
            .as_deref()
            .is_some_and(|current| {
                canonical_model_path(current).is_ok_and(|current| current == path)
            });

        let mut state = slot
            .state
            .lock()
            .map_err(|_| WriterError::Other("correction model slot lock poisoned".into()))?;
        let configured_unchanged = configured_unchanged_before_lock
            && generation == MODEL_CONFIG_GENERATION.load(Ordering::SeqCst)
            && stamp_after
                .as_ref()
                .is_ok_and(|stamp| *stamp == stamp_before);
        let opened = match opened {
            Ok(model) => model,
            Err(error) => {
                *state = ModelLoadState::Empty;
                slot.wake.notify_all();
                return Err(error);
            }
        };
        let model = Arc::new(opened);
        if !configured_unchanged {
            *state = ModelLoadState::Empty;
            slot.wake.notify_all();
            drop(state);
            // The session opened against a superseded configuration has never
            // been published. Hand it to the lifecycle thread so the large
            // ONNX Runtime object is not destroyed on this request thread.
            retire_model_resources(RetiredModelResources {
                reason: ModelReleaseReason::ConfigurationReplaced,
                generation: MODEL_CONFIG_GENERATION.load(Ordering::SeqCst),
                idle_ms: None,
                models: vec![model],
                engines: Vec::new(),
            });
            return match stamp_after {
                Ok(_) => Err(WriterError::Other(
                    "correction model configuration changed while it was being opened; retry the request".into(),
                )),
                Err(error) => Err(error),
            };
        }
        *state = ModelLoadState::Ready {
            stamp: stamp_before.clone(),
            identity: identity.clone(),
            model: Arc::clone(&model),
            last_used: Instant::now(),
        };
        slot.wake.notify_all();
        Ok(ConfiguredModelSnapshot {
            key: Some(ModelCacheKey {
                path,
                stamp: stamp_before,
                identity,
            }),
            model: Some(model),
            cache_hit: false,
            session_initialized: true,
            load_ms: started.elapsed().as_millis(),
        })
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
