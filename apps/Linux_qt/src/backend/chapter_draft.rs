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

mod store;
#[cfg(test)]
mod tests;

const DRAFT_SCHEMA_VERSION: u32 = 2;
/// 正文与 meta 的落盘节流：编辑过程中每章最多每 2 秒写一次；保存/关窗等
/// 边界传 force 立即写。
const DRAFT_PERSIST_INTERVAL: Duration = Duration::from_secs(2);
/// 内存索引上限：超过时只丢内存里最老的条目，事实文件永远保留。
const MAX_MEMORY_FACT_INDEX: usize = 4096;
const MIXED_SESSION_ERROR: &str =
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
    session_id: u64,
    generation: String,
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

/// 记录一条原始证明事实：事实文件立即落盘，meta 索引按节流稍后写。
pub(crate) fn note_fact(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    session_id: u64,
    generation: &str,
    fact: DraftFact,
) {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        log::warn!("draft note_fact invalid id: project={project_id} chapter={chapter_id}");
        return;
    };
    if !ensure_state_or_new(&location, volume_id) {
        return;
    }
    let seq = {
        let mut drafts = lock_registry();
        let Some(state) = drafts.get_mut(&location.key) else {
            return;
        };
        if state
            .facts
            .iter()
            .any(|existing| existing.event_id == fact.event_id)
        {
            return;
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
        if let Some(state) = lock_registry().get_mut(&location.key) {
            state
                .facts
                .retain(|existing| existing.event_id != fact.event_id);
        }
        log::warn!(
            "write chapter draft fact failed: project={project_id} chapter={chapter_id} event={}: {error}",
            fact.event_id
        );
    }
}

/// 按会话/generation/快照截取连续事实前缀，并读回 projection。
///
/// 只取 `session_id == session && generation 匹配 && revision <= snapshot`
/// 的连续前缀；前缀之前存在其他会话/generation 的未提交事实时返回 Err，
/// 不能静默丢弃（否则这些事实会随下一次成功保存被删除）。文件缺失或损坏
/// 同样 Err，绝不跳过。
pub(crate) fn freeze_batch_for(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    session_id: u64,
    generation: &str,
    snapshot_revision: u64,
) -> Result<Vec<LoadedFact>, String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Err(format!("invalid draft identity: {project_id}/{chapter_id}"));
    };
    if !ensure_state(&location) {
        if location.legacy_path().exists() {
            return Err(format!(
                "legacy draft cannot migrate: {project_id}/{chapter_id}"
            ));
        }
        return Ok(Vec::new());
    }
    let (index, draft_generation) = {
        let drafts = lock_registry();
        let Some(state) = drafts.get(&location.key) else {
            return Ok(Vec::new());
        };
        (state.facts.clone(), state.generation.clone())
    };
    let merged = store::merge_index(&location.dir, &index)?;
    let selected = select_prefix(
        &merged,
        session_id,
        &draft_generation,
        generation,
        snapshot_revision,
    )?;
    // 空前缀但草稿还有事实：这些事实都比快照新（或快照缺失）。不能返回空
    // 批次——保存成功后调用方会据此删掉整章草稿，未提交证明就丢了。
    if selected.is_empty() && !merged.is_empty() {
        return Err(format!(
            "draft facts are beyond snapshot {snapshot_revision}: project={project_id} chapter={chapter_id}"
        ));
    }
    let mut batch = Vec::with_capacity(selected.len());
    for fact in selected {
        let path = store::fact_file_path(&location.dir, fact.seq, &fact.event_id);
        let parsed = store::read_fact_file(&path)?;
        if parsed.event_id != fact.event_id
            || parsed.revision != fact.revision
            || parsed.session_id != fact.session_id
        {
            return Err(format!(
                "draft fact mismatch: {project_id}/{chapter_id} event={} path={}",
                fact.event_id,
                path.display()
            ));
        }
        batch.push(LoadedFact {
            event_id: parsed.event_id,
            revision: parsed.revision,
            projection_json: parsed.projection_json,
        });
    }
    Ok(batch)
}

/// journal 确认后删除已 ack 的前缀事实文件并推进基线；正文与剩余事实保留。
///
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
    let acknowledged: HashSet<&str> = acknowledged.iter().map(String::as_str).collect();
    let index = {
        let drafts = lock_registry();
        drafts
            .get(&location.key)
            .map(|state| state.facts.clone())
            .unwrap_or_default()
    };
    let merged = store::merge_index(&location.dir, &index).unwrap_or_else(|error| {
        log::warn!(
            "chapter draft mark_committed merge failed: project={project_id} chapter={chapter_id}: {error}"
        );
        index.clone()
    });
    let mut remaining = Vec::with_capacity(merged.len());
    let mut removed = Vec::new();
    for fact in merged {
        if acknowledged.contains(fact.event_id.as_str()) {
            removed.push(store::fact_file_path(
                &location.dir,
                fact.seq,
                &fact.event_id,
            ));
        } else {
            remaining.push(fact);
        }
    }
    for path in removed {
        if let Err(error) = std::fs::remove_file(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!("remove chapter draft fact {}: {error}", path.display());
            }
        }
    }
    let (empty, meta) = {
        let mut drafts = lock_registry();
        let Some(state) = drafts.get_mut(&location.key) else {
            return false;
        };
        state.facts = remaining;
        state.trim_index();
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
        (empty, meta)
    };
    if empty {
        drop_registry_and_storage(&location);
        return true;
    }
    if let Some(meta) = meta {
        if let Err(error) = store::write_meta(&location.dir, &meta) {
            log::warn!("persist draft meta after commit: {project_id}/{chapter_id}: {error}");
        } else if let Some(state) = lock_registry().get_mut(&location.key) {
            state.meta_dirty = false;
            state.last_persist = Some(Instant::now());
        }
    }
    false
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

/// 读取指定章节的草稿快照（只读，不迁移旧格式、不写盘）。
pub(crate) fn snapshot(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Option<ChapterDraftSnapshot> {
    let location = DraftLocation::new(projects_root, project_id, chapter_id)?;
    if let Some(state) = lock_registry().get(&location.key) {
        return Some(ChapterDraftSnapshot::from_state(state));
    }
    if let Some(state) = store::read_new_state(&location, false) {
        return Some(ChapterDraftSnapshot::from_state(&state));
    }
    let legacy = store::read_legacy(&location.legacy_path(), project_id, chapter_id)?;
    Some(ChapterDraftSnapshot::from_state(
        &store::legacy_state(&location, legacy).0,
    ))
}

/// 列出项目内所有需要恢复的草稿（有事实或有正文）。
pub(crate) fn list_project_drafts(projects_root: &Path, project_id: &str) -> Vec<ChapterDraft> {
    if !store::is_safe_id(project_id) {
        return Vec::new();
    }
    let workspace = store::workspace_key_of(projects_root);
    let root = store::drafts_root(projects_root, project_id);
    let mut drafts: Vec<ChapterDraft> = {
        let registry = lock_registry();
        registry
            .values()
            .filter(|state| state.project_id == project_id && state.workspace_key == workspace)
            .map(DraftState::summary)
            .collect()
    };
    drafts.sort_by(|left, right| left.chapter_id.cmp(&right.chapter_id));
    let Ok(entries) = std::fs::read_dir(&root) else {
        return drafts;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let Some(chapter_id) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !store::is_safe_id(chapter_id)
                || drafts.iter().any(|draft| draft.chapter_id == chapter_id)
            {
                continue;
            }
            if let Some(summary) = store::read_summary_from_dir(&path, project_id, chapter_id) {
                drafts.push(summary);
            }
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(chapter_id) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        if !store::is_safe_id(chapter_id)
            || drafts.iter().any(|draft| draft.chapter_id == chapter_id)
        {
            continue;
        }
        if let Some(legacy) = store::read_legacy(&path, project_id, chapter_id) {
            let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
                continue;
            };
            drafts.push(store::legacy_state(&location, legacy).0.summary());
        }
    }
    drafts
}

/// 读取草稿的恢复计划；任何文件缺失/损坏/正文摘要不一致都返回 None，
/// 且不删除任何草稿文件（保留给下一次重试或人工排查）。
pub(crate) fn restore_plan(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Option<ChapterDraftRestorePlan> {
    let location = DraftLocation::new(projects_root, project_id, chapter_id)?;
    let state =
        store::read_new_state(&location, false).or_else(|| store::migrate_legacy(&location))?;
    if let Some(text) = state.text.as_ref() {
        if !state.text_sha256.is_empty() {
            let actual = writer_core::writing_stats::proof::content_digest_of_text(text);
            if actual != state.text_sha256 {
                log::warn!(
                    "draft text corrupt: {project_id}/{chapter_id} expected={} actual={actual}",
                    state.text_sha256
                );
                return None;
            }
        }
    } else if state.text_revision > 0 || !state.text_sha256.is_empty() {
        log::warn!(
            "chapter draft text missing: project={project_id} chapter={chapter_id} text_revision={}",
            state.text_revision
        );
        return None;
    }
    let mut values = Vec::with_capacity(state.facts.len());
    for fact in &state.facts {
        match read_projection(&location, fact) {
            Ok(value) => values.push(value),
            Err(error) => {
                log::warn!("draft fact unreadable: {project_id}/{chapter_id}: {error}");
                return None;
            }
        }
    }
    let batch_json = match serde_json::to_string(&values) {
        Ok(json) => json,
        Err(error) => {
            log::warn!("draft batch encode failed: {project_id}/{chapter_id}: {error}");
            return None;
        }
    };
    let sessions: HashSet<u64> = state.facts.iter().map(|fact| fact.session_id).collect();
    Some(ChapterDraftRestorePlan {
        volume_id: state.volume_id.clone(),
        chapter_id: state.chapter_id.clone(),
        base_generation: state.base_generation.clone(),
        base_revision: state.base_revision,
        base_sha256: state.base_sha256.clone(),
        text: state.text.clone(),
        text_revision: state.text_revision,
        text_sha256: state.text_sha256.clone(),
        fact_count: state.facts.len(),
        batch_json,
        session_id: state.session_id,
        generation: state.generation.clone(),
        mixed_sessions: sessions.len() > 1,
    })
}

/// 内存占用：内存索引事实条数、正文总字节数（不含 projection 历史）。
pub(crate) fn draft_metrics() -> (usize, usize) {
    let registry = lock_registry();
    let mut facts = 0usize;
    let mut bytes = 0usize;
    for state in registry.values() {
        facts = facts.saturating_add(state.facts.len());
        bytes = bytes.saturating_add(state.text.as_ref().map_or(0, String::len));
    }
    (facts, bytes)
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

fn read_projection(
    location: &DraftLocation,
    fact: &DraftFactIndex,
) -> Result<serde_json::Value, String> {
    let path = store::fact_file_path(&location.dir, fact.seq, &fact.event_id);
    let parsed = store::read_fact_file(&path)?;
    if parsed.event_id != fact.event_id || parsed.revision != fact.revision {
        return Err(format!(
            "draft fact mismatch event={} path={}",
            fact.event_id,
            path.display()
        ));
    }
    serde_json::from_str(&parsed.projection_json).map_err(|error| {
        format!(
            "draft fact projection invalid event={}: {error}",
            fact.event_id
        )
    })
}

fn select_prefix(
    facts: &[DraftFactIndex],
    session_id: u64,
    draft_generation: &str,
    generation: &str,
    snapshot_revision: u64,
) -> Result<Vec<DraftFactIndex>, String> {
    let generation_matches = draft_generation.is_empty() || draft_generation == generation;
    let mut selected: Vec<DraftFactIndex> = Vec::new();
    let mut leading_foreign = false;
    for fact in facts {
        if fact.revision > snapshot_revision {
            break;
        }
        if fact.session_id == session_id && generation_matches {
            if leading_foreign {
                return Err(MIXED_SESSION_ERROR.to_string());
            }
            selected.push(fact.clone());
        } else if selected.is_empty() {
            // 先看到别的会话/generation 的旧事实：若后面还有本会话事实，
            // 这些旧事实的 revision 更小，绝不能跳过。
            leading_foreign = true;
        } else if fact.revision < selected[0].revision {
            return Err(MIXED_SESSION_ERROR.to_string());
        } else {
            // 本会话连续前缀到此结束，之后的事实留给后续快照。
            break;
        }
    }
    if leading_foreign && selected.is_empty() {
        return Err(MIXED_SESSION_ERROR.to_string());
    }
    Ok(selected)
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
