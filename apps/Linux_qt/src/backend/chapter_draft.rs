// =============================================================================
// chapter_draft.rs — 未提交正文与证明事实的可恢复草稿
// =============================================================================
//
// Issue #1062 第 1.1/1.3 项：编辑事实和保存命令归同一 workspace/chapter actor
// （`chapter_save_queue` 的 worker）所有。actor 在每次编辑事务后记录原始证明
// 事实（`record_editor_proof_fact` 返回的 JSON），并在正文变化时记录草稿正文；
// 保存失败、进程退出或重新进入作品时，草稿按同一批次协议重新提交，正文和
// 证明链都不会丢。
//
// 本模块只做草稿状态与磁盘格式，不自己起线程：
//   - 状态由 actor 线程读写；UI 线程只做只读恢复查询和入队命令；
//   - 草稿文件在 `<project_root>/app-meta/chapter-drafts/<chapter_id>.json`，
//     app-meta 是 LocalOnly，不同步（`sync/content_class.rs`）；
//   - 正文以普通字符串保存（JSON 转义），原子写。
//
// 不干什么：
// - 不在这里调用 Core 保存（由 chapter_save_queue 的 worker 负责）；
// - 不复制 Core 的版本/证明决议，只缓存“尚未确认的原始事实”。
// =============================================================================

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const DRAFT_SCHEMA_VERSION: u32 = 1;
/// 草稿正文的落盘节流：编辑过程中最多每 2 秒写一次整章正文，
/// 保存/关窗/离开作品等边界仍立即 flush。
const DRAFT_TEXT_PERSIST_INTERVAL: Duration = Duration::from_secs(2);
/// 单章草稿最多保留的证明事实数量；超过说明保存长期失败，
/// 继续增长只会占内存，保留最早的连续前缀仍可恢复。
const MAX_DRAFT_FACTS: usize = 4096;

/// 一条尚未随正文 durable 的原始证明事实。
#[derive(Clone, Debug)]
pub(crate) struct DraftFact {
    pub event_id: String,
    /// 该事件提交后的编辑器 revision，用于按快照截取连续前缀。
    pub revision: u64,
    /// `record_editor_proof_fact` 返回的原始投影 JSON。
    pub projection_json: String,
}

/// 一章的未提交草稿。
#[derive(Clone, Debug)]
pub(crate) struct ChapterDraft {
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    /// 本批事实开始前最后一次 durable 的章节版本。
    pub base_generation: String,
    pub base_revision: u64,
    pub base_sha256: String,
    /// 编辑器最近一次报告的正文；`None` 表示还没有正文更新。
    pub text: Option<String>,
    pub text_revision: u64,
    pub text_sha256: String,
    pub facts: Vec<DraftFact>,
    pub updated_at_ms: i64,
    /// 上次正文落盘时间（仅进程内，不持久化）。
    pub last_text_persist: Option<Instant>,
}

impl ChapterDraft {
    fn key(&self) -> String {
        draft_key(&self.project_id, &self.chapter_id)
    }

    /// 只按 revision 截取连续前缀：返回 (批次 JSON 数组, 批次内最后 revision)。
    ///
    /// 事实必须从队首连续：中间缺一条就整批拒绝（返回 Err），不能跳过
    /// 队首去拼更晚的事件，否则会把快照之前尚未 durable 的编辑漏掉。
    pub(crate) fn freeze_batch(&self, snapshot_revision: u64) -> Result<Vec<DraftFact>, String> {
        if self.facts.is_empty() {
            return Ok(Vec::new());
        }
        // 没有编辑器快照 revision 时无法判断哪些事实属于本次正文；宁可直接
        // 失败让平台重试，也不能把事实留给下一次保存导致证明链断裂。
        if snapshot_revision == 0 {
            return Err("snapshot revision 缺失，无法冻结证明批次".to_string());
        }
        let mut batch = Vec::new();
        for fact in &self.facts {
            if fact.revision > snapshot_revision {
                break;
            }
            if fact.revision <= self.base_revision {
                return Err(format!(
                    "fact revision {} 未超过草稿基线 {}，拒绝冻结",
                    fact.revision, self.base_revision
                ));
            }
            if batch
                .last()
                .is_some_and(|previous: &DraftFact| fact.revision <= previous.revision)
            {
                return Err(format!("fact revision {} 未严格递增", fact.revision));
            }
            batch.push(fact.clone());
        }
        Ok(batch)
    }

    /// 已确认（journal durable）的事实从草稿移除。
    pub(crate) fn ack(&mut self, event_ids: &[String]) {
        if event_ids.is_empty() {
            return;
        }
        let acknowledged: std::collections::HashSet<&String> = event_ids.iter().collect();
        self.facts
            .retain(|fact| !acknowledged.contains(&fact.event_id));
    }
}

/// 恢复查询结果（UI 线程只读）。
#[derive(Clone, Debug)]
pub(crate) struct ChapterDraftSnapshot {
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub base_generation: String,
    pub base_revision: u64,
    pub base_sha256: String,
    pub text: Option<String>,
    pub text_revision: u64,
    pub text_sha256: String,
    pub fact_count: usize,
    pub updated_at_ms: i64,
}

impl From<&ChapterDraft> for ChapterDraftSnapshot {
    fn from(draft: &ChapterDraft) -> Self {
        Self {
            project_id: draft.project_id.clone(),
            volume_id: draft.volume_id.clone(),
            chapter_id: draft.chapter_id.clone(),
            base_generation: draft.base_generation.clone(),
            base_revision: draft.base_revision,
            base_sha256: draft.base_sha256.clone(),
            text: draft.text.clone(),
            text_revision: draft.text_revision,
            text_sha256: draft.text_sha256.clone(),
            fact_count: draft.facts.len(),
            updated_at_ms: draft.updated_at_ms,
        }
    }
}

fn registry() -> &'static Mutex<HashMap<String, ChapterDraft>> {
    static DRAFTS: OnceLock<Mutex<HashMap<String, ChapterDraft>>> = OnceLock::new();
    DRAFTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn draft_key(project_id: &str, chapter_id: &str) -> String {
    format!("{project_id}\u{1f}{chapter_id}")
}

pub(crate) fn draft_path(projects_root: &Path, project_id: &str, chapter_id: &str) -> PathBuf {
    projects_root
        .join(project_id)
        .join("app-meta")
        .join("chapter-drafts")
        .join(format!("{chapter_id}.json"))
}

fn draft_dir(projects_root: &Path, project_id: &str) -> PathBuf {
    projects_root
        .join(project_id)
        .join("app-meta")
        .join("chapter-drafts")
}

/// 编辑器最近一次报告的正文更新。只更新内存与元数据，正文落盘由 actor
/// 按节流决定（`persist`）。
pub(crate) fn note_text(
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    text: String,
    revision: u64,
    sha256: &str,
    base: (&str, u64, &str),
) {
    let Ok(mut drafts) = registry().lock() else {
        return;
    };
    let key = draft_key(project_id, chapter_id);
    let entry = drafts.entry(key).or_insert_with(|| ChapterDraft {
        project_id: project_id.to_string(),
        volume_id: volume_id.to_string(),
        chapter_id: chapter_id.to_string(),
        base_generation: base.0.to_string(),
        base_revision: base.1,
        base_sha256: base.2.to_string(),
        text: None,
        text_revision: 0,
        text_sha256: String::new(),
        facts: Vec::new(),
        updated_at_ms: now_ms(),
        last_text_persist: None,
    });
    entry.volume_id = volume_id.to_string();
    entry.text = Some(text);
    entry.text_revision = revision;
    entry.text_sha256 = sha256.to_string();
    entry.base_generation = base.0.to_string();
    entry.base_revision = base.1;
    entry.base_sha256 = base.2.to_string();
    entry.updated_at_ms = now_ms();
}

/// 记录一条原始证明事实。重复 event_id 忽略，保持顺序。
pub(crate) fn note_fact(project_id: &str, chapter_id: &str, volume_id: &str, fact: DraftFact) {
    let Ok(mut drafts) = registry().lock() else {
        return;
    };
    let key = draft_key(project_id, chapter_id);
    let entry = drafts.entry(key).or_insert_with(|| ChapterDraft {
        project_id: project_id.to_string(),
        volume_id: volume_id.to_string(),
        chapter_id: chapter_id.to_string(),
        base_generation: String::new(),
        base_revision: 0,
        base_sha256: String::new(),
        text: None,
        text_revision: 0,
        text_sha256: String::new(),
        facts: Vec::new(),
        updated_at_ms: now_ms(),
        last_text_persist: None,
    });
    entry.volume_id = volume_id.to_string();
    if entry
        .facts
        .iter()
        .any(|existing| existing.event_id == fact.event_id)
    {
        return;
    }
    if entry.facts.len() >= MAX_DRAFT_FACTS {
        // 超过上限时整批清空而不是丢最老的一条：丢中间会留下断链，之后
        // 每次保存都会 PROOF_BATCH_MISMATCH 永久失败。清空后保存不带证明，
        // 但正文仍由草稿 text 快照保护，至少可以恢复。
        log::warn!(
            "chapter draft facts exceeded limit, clearing chain: project={project_id} chapter={chapter_id}"
        );
        entry.facts.clear();
    }
    entry.facts.push(fact);
    entry.updated_at_ms = now_ms();
}

/// 读取（必要时从磁盘加载）指定章节的草稿快照。
///
/// 内存里的 entry 总是比磁盘新；内存没有时才读文件。只读，不改状态。
pub(crate) fn snapshot(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Option<ChapterDraftSnapshot> {
    let key = draft_key(project_id, chapter_id);
    if let Ok(drafts) = registry().lock() {
        if let Some(draft) = drafts.get(&key) {
            return Some(draft.into());
        }
    }
    let path = draft_path(projects_root, project_id, chapter_id);
    let content = std::fs::read_to_string(&path).ok()?;
    let draft = decode_draft(&content)?;
    let snapshot = ChapterDraftSnapshot::from(&draft);
    if let Ok(mut drafts) = registry().lock() {
        drafts.entry(key).or_insert(draft);
    }
    Some(snapshot)
}

/// 列出项目内所有需要恢复/续写的草稿。
pub(crate) fn list_project_drafts(projects_root: &Path, project_id: &str) -> Vec<ChapterDraft> {
    let mut drafts: Vec<ChapterDraft> = Vec::new();
    if let Ok(registry) = registry().lock() {
        drafts.extend(
            registry
                .values()
                .filter(|draft| draft.project_id == project_id)
                .cloned(),
        );
    }
    let dir = draft_dir(projects_root, project_id);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some(draft) = decode_draft(&content) else {
                continue;
            };
            if draft.project_id != project_id {
                continue;
            }
            if drafts
                .iter()
                .any(|existing| existing.chapter_id == draft.chapter_id)
            {
                continue;
            }
            drafts.push(draft);
        }
    }
    drafts
}

/// 恢复草稿所需的完整事实批次与正文。
#[derive(Clone, Debug)]
pub(crate) struct ChapterDraftRestorePlan {
    pub volume_id: String,
    pub chapter_id: String,
    /// 原始投影 JSON 数组；空串表示没有事实，只用草稿正文。
    pub batch_json: String,
    pub text: Option<String>,
    pub fact_count: usize,
}

/// 读取草稿的恢复计划；事实按 revision 升序排列。
pub(crate) fn restore_plan(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Option<ChapterDraftRestorePlan> {
    let draft = load_draft(projects_root, project_id, chapter_id)?;
    let mut facts = draft.facts.clone();
    facts.sort_by_key(|fact| fact.revision);
    let batch_json = if facts.is_empty() {
        String::new()
    } else {
        let values: Vec<serde_json::Value> = facts
            .iter()
            .filter_map(|fact| serde_json::from_str(&fact.projection_json).ok())
            .collect();
        serde_json::to_string(&values).ok()?
    };
    Some(ChapterDraftRestorePlan {
        volume_id: draft.volume_id.clone(),
        chapter_id: draft.chapter_id.clone(),
        batch_json,
        text: draft.text.clone(),
        fact_count: facts.len(),
    })
}

fn load_draft(projects_root: &Path, project_id: &str, chapter_id: &str) -> Option<ChapterDraft> {
    let key = draft_key(project_id, chapter_id);
    if let Ok(drafts) = registry().lock() {
        if let Some(draft) = drafts.get(&key) {
            return Some(draft.clone());
        }
    }
    let path = draft_path(projects_root, project_id, chapter_id);
    let content = std::fs::read_to_string(&path).ok()?;
    let draft = decode_draft(&content)?;
    if let Ok(mut drafts) = registry().lock() {
        drafts.entry(key).or_insert_with(|| draft.clone());
    }
    Some(draft)
}

/// 把指定章节的当前草稿写盘（actor 调用）。
pub(crate) fn persist_current(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    force_text: bool,
) -> Result<(), String> {
    let key = draft_key(project_id, chapter_id);
    let draft = registry()
        .lock()
        .ok()
        .and_then(|drafts| drafts.get(&key).cloned())
        .ok_or_else(|| "chapter draft not found".to_string())?;
    persist(projects_root, &draft, force_text)
}

/// 按快照 revision 截取连续事实前缀（actor 调用）。
pub(crate) fn freeze_batch_for(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    snapshot_revision: u64,
) -> Result<Vec<DraftFact>, String> {
    let key = draft_key(project_id, chapter_id);
    if let Ok(drafts) = registry().lock() {
        if let Some(draft) = drafts.get(&key) {
            return draft.freeze_batch(snapshot_revision);
        }
    }
    // 内存没有时尝试磁盘（进程重启后第一次保存）。
    let path = draft_path(projects_root, project_id, chapter_id);
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    let Some(draft) = decode_draft(&content) else {
        return Ok(Vec::new());
    };
    let batch = draft.freeze_batch(snapshot_revision);
    if let Ok(mut drafts) = registry().lock() {
        drafts.entry(key).or_insert(draft);
    }
    batch
}

/// journal 确认后更新草稿：移除已确认事实，推进基线；没有剩余事实时删除。
///
/// 返回 true 表示草稿已完全确认并删除。
pub(crate) fn mark_committed(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    acknowledged: &[String],
    base_generation: &str,
    base_revision: u64,
    base_sha256: &str,
) -> bool {
    let key = draft_key(project_id, chapter_id);
    let mut removed = false;
    if let Ok(mut drafts) = registry().lock() {
        if let Some(draft) = drafts.get_mut(&key) {
            draft.ack(acknowledged);
            draft.base_generation = base_generation.to_string();
            draft.base_revision = base_revision;
            draft.base_sha256 = base_sha256.to_string();
            draft.updated_at_ms = now_ms();
            removed = draft.facts.is_empty();
        }
    }
    if removed {
        remove(projects_root, project_id, chapter_id);
    }
    removed
}

pub(crate) fn persist(
    projects_root: &Path,
    draft: &ChapterDraft,
    force_text: bool,
) -> Result<(), String> {
    let should_write_text = force_text
        || draft
            .last_text_persist
            .map(|last| last.elapsed() >= DRAFT_TEXT_PERSIST_INTERVAL)
            .unwrap_or(true);
    if !should_write_text && draft.facts.is_empty() {
        return Ok(());
    }
    let path = draft_path(projects_root, &draft.project_id, &draft.chapter_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("create draft dir: {error}"))?;
    }
    let value = encode_draft(draft);
    let json = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    writer_core::storage::atomic_write_string(&path, &json)
        .map_err(|error| format!("write chapter draft: {error}"))?;
    if let Ok(mut drafts) = registry().lock() {
        if let Some(entry) = drafts.get_mut(&draft_key(&draft.project_id, &draft.chapter_id)) {
            entry.last_text_persist = Some(Instant::now());
        }
    }
    Ok(())
}

/// 草稿完全被 journal 确认后删除（文件 + 内存）。
pub(crate) fn remove(projects_root: &Path, project_id: &str, chapter_id: &str) {
    if let Ok(mut drafts) = registry().lock() {
        drafts.remove(&draft_key(project_id, chapter_id));
    }
    let path = draft_path(projects_root, project_id, chapter_id);
    if path.exists() {
        if let Err(error) = std::fs::remove_file(&path) {
            log::warn!("remove chapter draft {} failed: {error}", path.display());
        }
    }
}

/// 队列占用（Issue #1062 3.4：保存/证明队列字节数）。
pub(crate) fn draft_metrics() -> (usize, usize) {
    let Ok(drafts) = registry().lock() else {
        return (0, 0);
    };
    let mut bytes = 0usize;
    for draft in drafts.values() {
        bytes = bytes.saturating_add(draft.text.as_ref().map_or(0, String::len));
        for fact in &draft.facts {
            bytes = bytes.saturating_add(fact.projection_json.len());
        }
    }
    (drafts.len(), bytes)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn encode_draft(draft: &ChapterDraft) -> serde_json::Value {
    serde_json::json!({
        "schemaVersion": DRAFT_SCHEMA_VERSION,
        "projectId": draft.project_id,
        "volumeId": draft.volume_id,
        "chapterId": draft.chapter_id,
        "baseGeneration": draft.base_generation,
        "baseRevision": draft.base_revision,
        "baseSha256": draft.base_sha256,
        "textRevision": draft.text_revision,
        "textSha256": draft.text_sha256,
        "text": draft.text,
        "facts": draft.facts.iter().map(|fact| serde_json::json!({
            "eventId": fact.event_id,
            "revision": fact.revision,
            "projection": serde_json::from_str::<serde_json::Value>(&fact.projection_json)
                .unwrap_or(serde_json::Value::Null),
        })).collect::<Vec<_>>(),
        "updatedAtMs": draft.updated_at_ms,
    })
}

fn decode_draft(content: &str) -> Option<ChapterDraft> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;
    let project_id = value.get("projectId")?.as_str()?.to_string();
    let chapter_id = value.get("chapterId")?.as_str()?.to_string();
    if project_id.is_empty() || chapter_id.is_empty() {
        return None;
    }
    let schema_version = value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if schema_version != u64::from(DRAFT_SCHEMA_VERSION) {
        log::warn!(
            "unsupported chapter draft schema {schema_version}, skip: project={project_id} chapter={chapter_id}"
        );
        return None;
    }
    let facts = value
        .get("facts")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let event_id = item.get("eventId")?.as_str()?.to_string();
                    let revision = item.get("revision")?.as_u64()?;
                    let projection = item.get("projection")?;
                    if projection.is_null() {
                        return None;
                    }
                    Some(DraftFact {
                        event_id,
                        revision,
                        projection_json: serde_json::to_string(projection).ok()?,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(ChapterDraft {
        project_id,
        volume_id: value
            .get("volumeId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string(),
        chapter_id,
        base_generation: value
            .get("baseGeneration")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string(),
        base_revision: value
            .get("baseRevision")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        base_sha256: value
            .get("baseSha256")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string(),
        text: value
            .get("text")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        text_revision: value
            .get("textRevision")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        text_sha256: value
            .get("textSha256")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string(),
        facts,
        updated_at_ms: value
            .get("updatedAtMs")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0),
        last_text_persist: None,
    })
}
