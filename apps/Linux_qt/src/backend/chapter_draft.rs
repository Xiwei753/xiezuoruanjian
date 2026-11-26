// =============================================================================
// chapter_draft.rs — 未提交正文与证明事实的可恢复草稿
// =============================================================================
//
// Issue #1062 第 1.1/1.3 项：编辑事实与正文变化都由 `chapter_save_queue` 的
// 唯一 worker（章节 actor）串行记录；保存失败、进程退出或重新进入作品时，
// 草稿按同一批次协议重新提交，正文和证明链都不会丢。
//
// 存储布局（app-meta 是 LocalOnly，不同步）：
//
//   <projects_root>/<project_id>/app-meta/chapter-drafts/<chapter_id>/
//     meta.json                 索引、基线、正文摘要；不含 projection 正文
//     text.txt                  整章正文（原子写，2s 节流；force 立即）
//     facts/<seq:08>-<id>.json  一条事实一个文件
//
// 本文件做类型、内存 registry 与冻结/恢复/提交决议；磁盘布局、编解码与
// 状态加载在 `chapter_draft::store`。
//
// 关键约束：
// - 内存 registry 只保留索引 + 正文 + 基线，不保留 projection 历史；
// - 事实文件立即持久化，meta 索引允许滞后（每 2s 才写），读取时按 facts/
//   目录补齐，绝不因为索引滞后把草稿当成空草稿删除；
// - session/generation 参与冻结，跨会话事实不会被静默丢弃；
// - 旧格式单文件 `<chapter_id>.json` 首次读取时迁移到新目录，迁移失败保留旧文件。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use store::DraftLocation;

mod freeze;
mod pending;
mod restore;
mod scheduler;
mod store;
#[cfg(test)]
mod tests;

pub(crate) use freeze::freeze_batch_for;
pub(crate) use pending::{clear_pending_input, pending_inputs, spool_failed_input};
pub(crate) use restore::{list_project_drafts, restore_plan, snapshot};
pub(crate) use scheduler::{draft_metrics, flush_due, next_persist_deadline};

const DRAFT_SCHEMA_VERSION: u32 = 2;
/// 正文与 meta 的落盘节流：编辑过程中每章最多每 2 秒写一次；保存/关窗等
/// 边界传 force 立即写。
const DRAFT_PERSIST_INTERVAL: Duration = Duration::from_secs(2);
/// 内存索引上限：超过时只丢内存里最老的条目，事实文件永远保留。
const MAX_MEMORY_FACT_INDEX: usize = 4096;
/// Core 投影构造失败的原始命令 spool 目录（与 facts/ 分开，恢复重试时用）。
pub(super) const PENDING_INPUTS_DIR: &str = "pending-inputs";
pub(super) const MIXED_SESSION_ERROR: &str =
    "draft has uncommitted facts from another session/generation; restore first";

/// 一条尚未随正文 durable 的原始证明事实。
#[derive(Clone, Debug)]
pub(crate) struct DraftFact {
    pub event_id: String,
    /// 该事件提交后的编辑器 revision，用于按快照截取连续前缀。
    pub revision: u64,
    /// 产生该事件的编辑器会话。
    pub session_id: u64,
    /// `record_editor_proof_fact` 返回的原始投影 JSON。
    pub projection_json: String,
}

/// 从磁盘读回、带 projection 的事实（冻结批次用）。
#[derive(Clone, Debug)]
pub(crate) struct LoadedFact {
    pub event_id: String,
    pub revision: u64,
    pub projection_json: String,
}

/// 内存索引条目（不含 projection 正文）。
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", default)]
struct DraftFactIndex {
    event_id: String,
    revision: u64,
    session_id: u64,
    seq: u64,
}

/// 一章草稿的轻量摘要；`list_project_drafts` 与恢复过滤使用。
#[derive(Clone, Debug)]
pub(crate) struct ChapterDraft {
    pub volume_id: String,
    pub chapter_id: String,
    pub base_generation: String,
    pub base_revision: u64,
    pub base_sha256: String,
    pub text_present: bool,
    pub text_revision: u64,
    pub text_sha256: String,
    pub fact_count: usize,
    /// 尚未构造出证明投影的原始事件数（pending-inputs/）。
    ///
    /// Issue #1062 复审：只有 pending-input 的草稿（首次事件在 facts/text 之前
    /// 崩溃）也必须进入恢复队列，否则 write-ahead spool 永远无人重试。
    pub pending_input_count: usize,
    /// pending-inputs 读取失败时的诊断；有错误时仍视为“有待处理”，不能当空。
    pub pending_input_error: Option<String>,
    pub session_id: u64,
    pub generation: String,
    pub updated_at_ms: i64,
}

impl ChapterDraft {
    /// 草稿读不出来时写进恢复状态，便于在不删草稿的前提下定位原因。
    pub(crate) fn diagnostic_summary(&self) -> String {
        format!(
            "volume={} base={}@{} sha={} textRevision={} textSha={} facts={} session={} generation={} updatedAtMs={}",
            self.volume_id,
            self.base_generation,
            self.base_revision,
            self.base_sha256,
            self.text_revision,
            self.text_sha256,
            self.fact_count,
            self.session_id,
            self.generation,
            self.updated_at_ms
        )
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
    pub session_id: u64,
    pub generation: String,
}

impl ChapterDraftSnapshot {
    fn from_state(state: &DraftState) -> Self {
        Self {
            project_id: state.project_id.clone(),
            volume_id: state.volume_id.clone(),
            chapter_id: state.chapter_id.clone(),
            base_generation: state.base_generation.clone(),
            base_revision: state.base_revision,
            base_sha256: state.base_sha256.clone(),
            text: state.text.clone(),
            text_revision: state.text_revision,
            text_sha256: state.text_sha256.clone(),
            fact_count: state.facts.len(),
            updated_at_ms: state.updated_at_ms,
            session_id: state.session_id,
            generation: state.generation.clone(),
        }
    }
}

/// 恢复草稿所需的完整事实批次与正文。
#[derive(Clone, Debug)]
pub(crate) struct ChapterDraftRestorePlan {
    pub volume_id: String,
    pub chapter_id: String,
    pub base_generation: String,
    pub base_revision: u64,
    pub base_sha256: String,
    pub text: Option<String>,
    pub text_revision: u64,
    pub text_sha256: String,
    pub fact_count: usize,
    /// 原始投影 JSON 数组；空数组表示没有事实，只用草稿正文。
    pub batch_json: String,
    /// 与 batch_json 对应的带 revision 事实（恢复时按 text_revision 划分
    /// 已落盘正文与仍需 replay 的后续事件）。
    pub facts: Vec<LoadedFact>,
    /// 事件链最后一条事实的 revision；0 表示没有事实。
    pub event_end_revision: u64,
    pub(crate) session_id: u64,
    pub(crate) generation: String,
    pub mixed_sessions: bool,
}

impl ChapterDraftRestorePlan {
    /// 诊断用：草稿记录的最后会话与 generation，不参与保存决议。
    pub(crate) fn session_summary(&self) -> String {
        format!("session={} generation={}", self.session_id, self.generation)
    }
}

/// 内存 registry 的完整草稿状态：索引 + 正文 + 基线。
struct DraftState {
    project_id: String,
    volume_id: String,
    chapter_id: String,
    workspace_key: String,
    generation: String,
    session_id: u64,
    base_generation: String,
    base_revision: u64,
    base_sha256: String,
    text: Option<String>,
    text_revision: u64,
    text_sha256: String,
    persisted_text_revision: u64,
    persisted_text_sha256: String,
    text_dirty: bool,
    facts: Vec<DraftFactIndex>,
    next_seq: u64,
    /// 事实文件写盘失败的事件（连同已分配的 seq）：保留原始 projection 供
    /// 下一次 flush/冻结重试，同时让快照不可越过失败 revision（Issue #1062
    /// 复审第 3 项）。seq 必须保持稳定，否则重试会写到新文件、旧路径残留。
    failed_facts: Vec<(u64, DraftFact)>,
    /// Core 侧投影构造失败、原始 command 已落盘等待重试的事件 (eventId, revision)。
    failed_inputs: Vec<(String, u64)>,
    /// 已随 durable receipt 确认、等待文件 GC 的 event ID（meta 先写索引）。
    acked_event_ids: Vec<String>,
    acked_through_revision: u64,
    projects_root: std::path::PathBuf,
    updated_at_ms: i64,
    meta_dirty: bool,
    last_persist: Option<Instant>,
}

impl DraftState {
    fn new(location: &DraftLocation, volume_id: &str) -> Self {
        Self {
            project_id: location.project_id.clone(),
            volume_id: volume_id.to_string(),
            chapter_id: location.chapter_id.clone(),
            workspace_key: location.workspace_key.clone(),
            generation: String::new(),
            session_id: 0,
            base_generation: String::new(),
            base_revision: 0,
            base_sha256: String::new(),
            text: None,
            text_revision: 0,
            text_sha256: String::new(),
            persisted_text_revision: 0,
            persisted_text_sha256: String::new(),
            text_dirty: false,
            facts: Vec::new(),
            next_seq: 1,
            failed_facts: Vec::new(),
            failed_inputs: Vec::new(),
            acked_event_ids: Vec::new(),
            acked_through_revision: 0,
            projects_root: location.projects_root.clone(),
            updated_at_ms: now_ms(),
            meta_dirty: false,
            last_persist: None,
        }
    }

    fn trim_index(&mut self) {
        if self.facts.len() <= MAX_MEMORY_FACT_INDEX {
            return;
        }
        self.facts.sort_by_key(|fact| fact.seq);
        let drop_count = self.facts.len() - MAX_MEMORY_FACT_INDEX;
        self.facts.drain(0..drop_count);
    }

    fn summary(&self) -> ChapterDraft {
        ChapterDraft {
            volume_id: self.volume_id.clone(),
            chapter_id: self.chapter_id.clone(),
            base_generation: self.base_generation.clone(),
            base_revision: self.base_revision,
            base_sha256: self.base_sha256.clone(),
            text_present: self.text.is_some(),
            text_revision: self.text_revision,
            text_sha256: self.text_sha256.clone(),
            fact_count: self.facts.len(),
            pending_input_count: self.failed_inputs.len(),
            pending_input_error: None,
            session_id: self.session_id,
            generation: self.generation.clone(),
            updated_at_ms: self.updated_at_ms,
        }
    }
}

fn registry() -> &'static Mutex<HashMap<String, DraftState>> {
    static DRAFTS: OnceLock<Mutex<HashMap<String, DraftState>>> = OnceLock::new();
    DRAFTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_registry() -> MutexGuard<'static, HashMap<String, DraftState>> {
    registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

// ── 公开 API ──────────────────────────────────────────────────────────────

/// 编辑器最近一次报告的正文更新；落盘由 actor 的 `persist_current` 按节流决定。
#[allow(clippy::too_many_arguments)]
pub(crate) fn note_text(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    session_id: u64,
    generation: &str,
    text: String,
    revision: u64,
    sha256: &str,
    base: (&str, u64, &str),
) {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        log::warn!("draft note_text invalid id: project={project_id} chapter={chapter_id}");
        return;
    };
    if !ensure_state_or_new(&location, volume_id) {
        return;
    }
    let mut drafts = lock_registry();
    let Some(state) = drafts.get_mut(&location.key) else {
        return;
    };
    state.volume_id = volume_id.to_string();
    state.session_id = session_id;
    if !generation.is_empty() {
        state.generation = generation.to_string();
    }
    state.text = Some(text);
    state.text_revision = revision;
    state.text_sha256 = sha256.to_string();
    state.text_dirty = true;
    if !base.0.is_empty() || base.1 > 0 {
        state.base_generation = base.0.to_string();
        state.base_revision = base.1;
        state.base_sha256 = base.2.to_string();
    }
    state.updated_at_ms = now_ms();
    state.meta_dirty = true;
}

/// 确保本章草稿已有 meta 事务头（write-ahead）。
///
/// Issue #1062 复审第 3 项：首次事实写入前必须先落事务头。否则事实文件
/// 已经在 facts/ 里、meta 还没有时崩溃，`read_new_state` 会返回 None，
/// 恢复列表完全看不到这批事件。已有 meta 时只做存在性检查，不重写。
pub(super) fn ensure_meta_header(
    location: &DraftLocation,
    volume_id: &str,
    base: Option<(&str, u64, &str)>,
) -> Result<(), String> {
    if location.dir.join(store::META_FILE).exists() {
        return Ok(());
    }
    if !ensure_state_or_new(location, volume_id) {
        return Err(format!(
            "chapter draft state unavailable: {}/{}",
            location.project_id, location.chapter_id
        ));
    }
    let meta = {
        let mut drafts = lock_registry();
        let Some(state) = drafts.get_mut(&location.key) else {
            return Err(format!(
                "chapter draft state unavailable: {}/{}",
                location.project_id, location.chapter_id
            ));
        };
        state.volume_id = volume_id.to_string();
        if let Some((generation, revision, sha256)) = base {
            if state.base_generation.is_empty()
                && state.base_revision == 0
                && (!generation.is_empty() || revision > 0)
            {
                state.base_generation = generation.to_string();
                state.base_revision = revision;
                state.base_sha256 = sha256.to_string();
            }
        }
        store::meta_from_state(
            location,
            state,
            state.persisted_text_revision,
            &state.persisted_text_sha256,
        )
    };
    store::write_meta(&location.dir, &meta)
}

/// 记录一条原始证明事实：事实文件立即落盘，meta 索引按节流稍后写。
///
/// Issue #1062 复审第 3 项：写盘失败不能静默丢弃。失败的事件保留在内存
/// `failed_facts` 里（含完整 projection），下一次 flush/冻结前重试；重试仍
/// 失败时，该事件 revision 及之前的正文快照一律拒绝提交（见
/// [`freeze_batch_for`]），避免“编辑事实丢了但正文照样落盘”把证明链断开。
pub(crate) fn note_fact(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    session_id: u64,
    generation: &str,
    fact: DraftFact,
) -> Result<(), String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Err(format!("invalid draft identity: {project_id}/{chapter_id}"));
    };
    if !ensure_state_or_new(&location, volume_id) {
        return Err(format!(
            "chapter draft state unavailable: {project_id}/{chapter_id}"
        ));
    }
    // 先写事务头，再写第一条事实；失败按事实写盘失败处理（fail-closed）。
    ensure_meta_header(&location, volume_id, None)?;
    let seq = {
        let mut drafts = lock_registry();
        let Some(state) = drafts.get_mut(&location.key) else {
            return Err(format!(
                "chapter draft state unavailable: {project_id}/{chapter_id}"
            ));
        };
        if state
            .facts
            .iter()
            .any(|existing| existing.event_id == fact.event_id)
            || state
                .failed_facts
                .iter()
                .any(|(_, existing)| existing.event_id == fact.event_id)
        {
            return Ok(());
        }
        let seq = state.next_seq;
        state.next_seq = state.next_seq.saturating_add(1);
        state.volume_id = volume_id.to_string();
        state.session_id = session_id;
        if !generation.is_empty() {
            state.generation = generation.to_string();
        }
        state.facts.push(DraftFactIndex {
            event_id: fact.event_id.clone(),
            revision: fact.revision,
            session_id: fact.session_id,
            seq,
        });
        state.trim_index();
        state.updated_at_ms = now_ms();
        state.meta_dirty = true;
        seq
    };
    if let Err(error) = store::write_fact_file(
        &location.dir,
        seq,
        &fact.event_id,
        fact.revision,
        fact.session_id,
        &fact.projection_json,
    ) {
        let event_id = fact.event_id.clone();
        if let Some(state) = lock_registry().get_mut(&location.key) {
            state.facts.retain(|existing| existing.event_id != event_id);
            if !state
                .failed_facts
                .iter()
                .any(|(_, existing)| existing.event_id == event_id)
            {
                state.failed_facts.push((seq, fact));
            }
            state.meta_dirty = true;
        }
        log::warn!(
            "write chapter draft fact failed: project={project_id} chapter={chapter_id} event={event_id}: {error}"
        );
        return Err(format!(
            "write draft fact failed: project={project_id} chapter={chapter_id} event={event_id}: {error}"
        ));
    }
    Ok(())
}

/// Core 侧投影构造失败的原始命令（event ID/revision/session/generation/input JSON）。
/// 同样 Err，绝不跳过。
/// 返回 true 表示草稿目录已完全删除（无事实且无正文）。
pub(crate) fn mark_committed(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    acknowledged: &[String],
    base_generation: &str,
    base_revision: u64,
    base_sha256: &str,
) -> bool {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return false;
    };
    if !ensure_state(&location) {
        return false;
    }
    let acknowledged_set: HashSet<&str> = acknowledged.iter().map(String::as_str).collect();
    let (merged, previous) = {
        let drafts = lock_registry();
        let Some(state) = drafts.get(&location.key) else {
            return false;
        };
        let acked: HashSet<String> = state.acked_event_ids.iter().cloned().collect();
        let index = state.facts.clone();
        let merged = store::merge_index(&location.dir, &index, &acked).unwrap_or_else(|error| {
            log::warn!(
                "chapter draft mark_committed merge failed: project={project_id} chapter={chapter_id}: {error}"
            );
            index.clone()
        });
        let previous = PersistedDraftHead {
            facts: state.facts.clone(),
            base_generation: state.base_generation.clone(),
            base_revision: state.base_revision,
            base_sha256: state.base_sha256.clone(),
            acked_event_ids: state.acked_event_ids.clone(),
            acked_through_revision: state.acked_through_revision,
            updated_at_ms: state.updated_at_ms,
        };
        (merged, previous)
    };
    let mut remaining = Vec::with_capacity(merged.len());
    let mut removed = Vec::new();
    for fact in merged {
        if acknowledged_set.contains(fact.event_id.as_str()) {
            removed.push(store::fact_file_path(
                &location.dir,
                fact.seq,
                &fact.event_id,
            ));
        } else {
            remaining.push(fact);
        }
    }
    // Issue #1062 复审第 4 项：先把 ack 决议（剩余索引 + 已确认 event ID）
    // 持久化，再删除事实文件。顺序反过来时，两步之间崩溃会让 meta 继续
    // 引用已删文件，下一次 freeze 把已提交事件当成损坏而永久卡住。
    let (empty, meta, previous) = {
        let mut drafts = lock_registry();
        let Some(state) = drafts.get_mut(&location.key) else {
            return false;
        };
        let mut acked_ids = state.acked_event_ids.clone();
        for event_id in acknowledged {
            if !acked_ids.iter().any(|existing| existing == event_id) {
                acked_ids.push(event_id.clone());
            }
        }
        let overflow = acked_ids.len().saturating_sub(MAX_MEMORY_FACT_INDEX);
        if overflow > 0 {
            acked_ids.drain(0..overflow);
        }
        state.facts = remaining;
        state.trim_index();
        state.acked_event_ids = acked_ids;
        state.acked_through_revision = state.acked_through_revision.max(base_revision);
        state.base_generation = base_generation.to_string();
        state.base_revision = base_revision;
        state.base_sha256 = base_sha256.to_string();
        state.updated_at_ms = now_ms();
        state.meta_dirty = true;
        let empty = state.facts.is_empty() && state.text.is_none();
        let meta = (!empty).then(|| {
            store::meta_from_state(
                &location,
                state,
                state.persisted_text_revision,
                &state.persisted_text_sha256,
            )
        });
        (empty, meta, previous)
    };
    if empty {
        drop_registry_and_storage(&location);
        return true;
    }
    let mut meta_persisted = true;
    if let Some(meta) = meta {
        match store::write_meta(&location.dir, &meta) {
            Ok(()) => {
                if let Some(state) = lock_registry().get_mut(&location.key) {
                    state.meta_dirty = false;
                    state.last_persist = Some(Instant::now());
                }
            }
            Err(error) => {
                meta_persisted = false;
                log::warn!(
                    "persist draft ack meta failed: {project_id}/{chapter_id}: {error}; keep fact files"
                );
                if let Some(state) = lock_registry().get_mut(&location.key) {
                    state.facts = previous.facts;
                    state.base_generation = previous.base_generation;
                    state.base_revision = previous.base_revision;
                    state.base_sha256 = previous.base_sha256;
                    state.acked_event_ids = previous.acked_event_ids;
                    state.acked_through_revision = previous.acked_through_revision;
                    state.updated_at_ms = previous.updated_at_ms;
                    state.meta_dirty = true;
                }
            }
        }
    }
    if !meta_persisted {
        return false;
    }
    // ack 决议已持久化，现在才回收已确认前缀文件。删除失败只会留下孤儿
    // 文件：索引与 acked 记录都已不含它们，后续读取会跳过，不会卡住恢复。
    for path in removed {
        if let Err(error) = std::fs::remove_file(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!("remove chapter draft fact {}: {error}", path.display());
            }
        }
    }
    false
}

/// mark_committed 写 meta 失败时要恢复的草稿头（索引/基线/ack 记录）。
struct PersistedDraftHead {
    facts: Vec<DraftFactIndex>,
    base_generation: String,
    base_revision: u64,
    base_sha256: String,
    acked_event_ids: Vec<String>,
    acked_through_revision: u64,
    updated_at_ms: i64,
}

/// 把草稿的正文/meta 按 2s 节流写盘（force 立即）；失败返回可诊断的错误文本。
pub(crate) fn persist_current(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    force: bool,
) -> Result<(), String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Err(format!("invalid draft identity: {project_id}/{chapter_id}"));
    };
    if !ensure_state(&location) {
        return Err(format!(
            "chapter draft not found: {project_id}/{chapter_id}"
        ));
    }
    let now = Instant::now();
    let plan = {
        let mut drafts = lock_registry();
        let Some(state) = drafts.get_mut(&location.key) else {
            return Err(format!(
                "chapter draft not found: {project_id}/{chapter_id}"
            ));
        };
        let due = force
            || state
                .last_persist
                .map_or(true, |last| last.elapsed() >= DRAFT_PERSIST_INTERVAL);
        if !due {
            return Ok(());
        }
        let write_text = state.text.is_some() && state.text_dirty;
        let write_meta = state.meta_dirty || write_text;
        if !write_text && !write_meta {
            return Ok(());
        }
        let written_revision = state.text_revision;
        let written_sha256 = state.text_sha256.clone();
        let (meta_revision, meta_sha256) = if write_text {
            (written_revision, written_sha256.clone())
        } else {
            (
                state.persisted_text_revision,
                state.persisted_text_sha256.clone(),
            )
        };
        PersistPlan {
            text: write_text.then(|| state.text.clone().unwrap_or_default()),
            written_revision,
            written_sha256,
            meta: write_meta
                .then(|| store::meta_from_state(&location, state, meta_revision, &meta_sha256)),
            write_text,
        }
    };
    if let Some(text) = plan.text.as_deref() {
        store::write_text(&location.dir, text)?;
    }
    if let Some(meta) = plan.meta.as_ref() {
        store::write_meta(&location.dir, meta)?;
    }
    if let Some(state) = lock_registry().get_mut(&location.key) {
        if plan.write_text {
            state.persisted_text_revision = plan.written_revision;
            state.persisted_text_sha256 = plan.written_sha256.clone();
            if state.text_revision == plan.written_revision
                && state.text_sha256 == plan.written_sha256
            {
                state.text_dirty = false;
            }
        }
        state.meta_dirty = false;
        state.last_persist = Some(now);
    }
    Ok(())
}

/// 草稿被 journal 完全确认后删除（目录 + 旧格式文件 + 内存）。
pub(crate) fn remove(projects_root: &Path, project_id: &str, chapter_id: &str) {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return;
    };
    drop_registry_and_storage(&location);
}

/// 测试专用：移除指定章节的进程内 registry 条目，模拟重启后从磁盘加载。
#[cfg(test)]
pub(crate) fn forget_for_test(projects_root: &Path, project_id: &str, chapter_id: &str) {
    if let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) {
        lock_registry().remove(&location.key);
    }
}

// ── 内部辅助 ──────────────────────────────────────────────────────────────

fn ensure_state_or_new(location: &DraftLocation, volume_id: &str) -> bool {
    if lock_registry().contains_key(&location.key) {
        return true;
    }
    let loaded = store::load_state(location);
    let mut drafts = lock_registry();
    if !drafts.contains_key(&location.key) {
        let state = loaded.unwrap_or_else(|| DraftState::new(location, volume_id));
        drafts.insert(location.key.clone(), state);
    }
    true
}

fn ensure_state(location: &DraftLocation) -> bool {
    if lock_registry().contains_key(&location.key) {
        return true;
    }
    let Some(loaded) = store::load_state(location) else {
        return false;
    };
    lock_registry()
        .entry(location.key.clone())
        .or_insert(loaded);
    true
}

fn drop_registry_and_storage(location: &DraftLocation) {
    lock_registry().remove(&location.key);
    if location.dir.exists() {
        if let Err(error) = std::fs::remove_dir_all(&location.dir) {
            log::warn!(
                "remove chapter draft dir failed: project={} chapter={} path={}: {error}",
                location.project_id,
                location.chapter_id,
                location.dir.display()
            );
        }
    }
    let legacy = location.legacy_path();
    if legacy.exists() {
        if let Err(error) = std::fs::remove_file(&legacy) {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!(
                    "remove legacy chapter draft failed: project={} chapter={} path={}: {error}",
                    location.project_id,
                    location.chapter_id,
                    legacy.display()
                );
            }
        }
    }
}

/// `persist_current` 在锁外执行 I/O 所需的不可变快照。
struct PersistPlan {
    text: Option<String>,
    written_revision: u64,
    written_sha256: String,
    meta: Option<store::MetaData>,
    write_text: bool,
}
