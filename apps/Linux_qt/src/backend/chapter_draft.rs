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
//     meta.json                 索引、基线、正文摘要/blob 引用；不含 projection 正文
//     texts/<rev>-<digest>.txt  不可变正文快照（先写 blob 再原子切换 meta 引用）
//     facts/<seq:08>-<id>.json  一条事实一个文件
//     pending-acks/<tx>.json    Core 已确认、本地尚未重放的提交
//
//   保留草稿归档（恢复失败时不再自动恢复）：
//     app-meta/chapter-draft-kept/<chapter_id>/<stamp>/
//
//   可自动重试事务的独立槽位（归档原件 + 重试标记，下次进入作品搬回活动槽位）：
//     app-meta/chapter-draft-retry/<chapter_id>/<stamp>.json
//
// 本文件做类型、内存 registry 与冻结/恢复/提交决议；磁盘布局、编解码与
// 状态加载在 `chapter_draft::store`，重试标记在 `chapter_draft::retry`。
//
// 关键约束：
// - 内存 registry 只保留索引 + 正文 + 基线，不保留 projection 历史；
// - 事实文件立即持久化，meta 索引允许滞后（每 2s 才写），读取时按 facts/
//   目录补齐，绝不因为索引滞后把草稿当成空草稿删除；
// - session/generation 参与冻结，跨会话事实不会被静默丢弃；
// - 旧格式单文件 `<chapter_id>.json` 首次读取时迁移到新目录，迁移失败保留旧文件。

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use store::DraftLocation;

mod ack;
mod ack_commit;
mod freeze;
mod ownership;
mod pending;
mod restore;
mod retry;
mod scheduler;
mod store;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use ack::pending_ack_count;
pub(crate) use ack::{
    complete_pending_ack, reconciled_ack, reconciled_ack_transactions, reconciled_acks,
    record_pending_ack, replay_pending_acks, PendingAck,
};
pub(crate) use ack_commit::{
    mark_committed, mark_committed_with_settlement, text_ready_for_commit, TextSettlement,
};
pub(crate) use freeze::freeze_batch_for;
pub(crate) use ownership::{archived_draft_ownership, draft_event_ownership, DraftEventOwnership};
pub(crate) use pending::{clear_pending_input, pending_inputs, spool_failed_input};
pub(crate) use restore::{
    discover_project_drafts, list_project_drafts, restore_plan, restore_plan_strict, snapshot,
    DraftDiscovery, UnreadableDraft,
};
pub(crate) use retry::{
    clear_retry_intent, list_retries, list_retries_strict, materialize_retry,
    materialize_retry_candidate, record_retry, remove_marker, retry_chapters,
    unreadable_retry_chapters, write_retry_intent, RetryCandidate, RetryMaterialize,
};
pub(crate) use scheduler::{draft_metrics, flush_due, next_persist_deadline};

const DRAFT_SCHEMA_VERSION: u32 = 2;
/// 正文与 meta 的落盘节流：编辑过程中每章最多每 2 秒写一次；保存/关窗等
/// 边界传 force 立即写。
const DRAFT_PERSIST_INTERVAL: Duration = Duration::from_secs(2);
/// 内存索引上限：超过时只丢内存里最老的条目，事实文件永远保留。
const MAX_MEMORY_FACT_INDEX: usize = 4096;
/// Core 投影构造失败的原始命令 spool 目录（与 facts/ 分开，恢复重试时用）。
pub(super) const PENDING_INPUTS_DIR: &str = "pending-inputs";
/// pending-inputs spool 不可读/损坏的错误前缀。
///
/// Issue #1062 评论 #7209683560 第 2 项：spool 里仍有未转换的原始编辑事实，
/// 事务加载必须原样失败（不能吞成空草稿），恢复链据此把它归为“可重试保留”
/// 而不是永久归档。
pub(crate) const PENDING_INPUTS_UNREADABLE: &str = "chapter draft pending inputs unreadable (kept)";
pub(super) const MIXED_SESSION_ERROR: &str =
    "draft has uncommitted facts from another session/generation; restore first";

/// 活动草稿目录是否仍然存在（恢复协调器判断“本入口是否已处理完该章”）。
pub(crate) fn draft_active_dir_exists(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> bool {
    store::DraftLocation::new(projects_root, project_id, chapter_id)
        .is_some_and(|location| location.dir.exists())
}

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
    /// 事件终态正文摘要（从原始投影 JSON 提取）；只用于判断快照正文是否
    /// 已被该事件证明覆盖，不参与 Core 批次校验。
    #[serde(default)]
    after_text_sha256: String,
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
    /// meta/事实索引读取失败（或 meta 缺失但有磁盘内容）的诊断；恢复必须
    /// 保留目录并向用户报告，不能当项目没有草稿。
    pub draft_error: Option<String>,
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
    /// 本次文档加载时的编辑器 revision 与基线摘要（编辑器坐标）。
    pub editor_base_revision: u64,
    pub editor_base_sha256: String,
    /// 已由 Core 回执确认收口的编辑器正文 revision/摘要。
    pub committed_text_revision: u64,
    pub committed_text_sha256: String,
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
    /// meta 当前引用的不可变正文 blob（空串=旧 text.txt 或没有正文）。
    persisted_text_blob: String,
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
    /// 已应用过的 Core journal transaction ID（幂等重放不回退 base）。
    applied_transactions: Vec<String>,
    /// 已应用的最高项目 journal commit_sequence；跨对象 generation 的 ack
    /// 顺序以它为准（Issue #1062 评论 7194373427 第 3 项）。
    applied_commit_sequence: u64,
    /// 本次文档加载时的编辑器 revision（与章节持久化 revision 不同坐标系）。
    editor_base_revision: u64,
    /// 文档加载基线的正文摘要；只有 revision == editor_base_revision 的首个
    /// 快照才能写入。
    editor_base_sha256: String,
    /// 记录 editor_base_* 所属编辑器会话；新会话（新一次加载）重新初始化。
    editor_base_session: u64,
    /// 最近一次 Core durable 回执确认收口的编辑器正文 revision/摘要。
    /// 恢复时据此把“已提交正文”与“未证明新编辑”区分开。
    committed_text_revision: u64,
    committed_text_sha256: String,
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
            persisted_text_blob: String::new(),
            text_dirty: false,
            facts: Vec::new(),
            next_seq: 1,
            failed_facts: Vec::new(),
            failed_inputs: Vec::new(),
            acked_event_ids: Vec::new(),
            acked_through_revision: 0,
            applied_transactions: Vec::new(),
            applied_commit_sequence: 0,
            editor_base_revision: 0,
            editor_base_sha256: String::new(),
            editor_base_session: 0,
            committed_text_revision: 0,
            committed_text_sha256: String::new(),
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
            draft_error: None,
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

/// 编辑器最近一次报告的正文候选；落盘由 actor 的 `persist_current` 按节流
/// 与证明水位决定。
///
/// Issue #1062 评论 7191076411 第 2 项：`base` 只允许在状态未初始化时写入。
/// 之后 base 由 Core durable receipt / pending-ack 唯一推进；GUI 在收到回执前
/// 继续输入时携带的旧 base 不得把已推进的 base 回退或覆盖。
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
    editor_base_revision: u64,
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
    // 新编辑器会话（新一次文档加载）重新记录基线 revision；同一会话内只
    // 允许首个等于基线的快照写入摘要。
    if state.editor_base_session != session_id {
        state.editor_base_session = session_id;
        state.editor_base_revision = editor_base_revision;
        state.editor_base_sha256.clear();
        // 新编辑器会话的 revision 从 0 重新计数，旧会话的已确认正文标识
        // 不能用来识别新会话的正文（否则同 revision 的新编辑会被误判成
        // 已提交残留）。
        state.committed_text_revision = 0;
        state.committed_text_sha256.clear();
    }
    if state.editor_base_sha256.is_empty() && revision == state.editor_base_revision {
        state.editor_base_sha256 = sha256.to_string();
    }
    if state.base_generation.is_empty()
        && state.base_revision == 0
        && state.base_sha256.is_empty()
        && (!base.0.is_empty() || base.1 > 0)
    {
        state.base_generation = base.0.to_string();
        state.base_revision = base.1;
        state.base_sha256 = base.2.to_string();
    }
    state.updated_at_ms = now_ms();
    state.meta_dirty = true;
}

/// 从 Core 返回的原始投影 JSON 读取事件终态正文摘要。
pub(super) fn projection_after_text_sha256(projection_json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(projection_json)
        .ok()
        .and_then(|value| {
            value
                .pointer("/event/proof/afterTextSha256")
                .and_then(|digest| digest.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default()
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
            &state.persisted_text_blob,
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
        let after_text_sha256 = projection_after_text_sha256(&fact.projection_json);
        state.facts.push(DraftFactIndex {
            event_id: fact.event_id.clone(),
            revision: fact.revision,
            session_id: fact.session_id,
            after_text_sha256,
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
        let write_text =
            state.text.is_some() && state.text_dirty && ack_commit::text_is_verified(state);
        let write_meta = state.meta_dirty || write_text;
        if !write_text && !write_meta {
            // 正文候选还没有被证明事实覆盖：只留在内存里等 actor 处理到
            // 对应事实。这里仍推进节流时间，避免调度器把“一直 dirty 但还
            // 不能发布”的状态当成到期任务反复唤醒。
            if state.text_dirty {
                state.last_persist = Some(now);
            }
            return Ok(());
        }
        let written_revision = state.text_revision;
        let written_sha256 = state.text_sha256.clone();
        let (meta_revision, meta_sha256, meta_blob) = if write_text {
            (
                written_revision,
                written_sha256.clone(),
                store::blob_name(written_revision, &written_sha256),
            )
        } else {
            (
                state.persisted_text_revision,
                state.persisted_text_sha256.clone(),
                state.persisted_text_blob.clone(),
            )
        };
        PersistPlan {
            text: write_text.then(|| state.text.clone().unwrap_or_default()),
            written_revision,
            written_sha256: written_sha256.clone(),
            written_blob: store::blob_name(written_revision, &written_sha256),
            meta: write_meta.then(|| {
                store::meta_from_state(&location, state, meta_revision, &meta_sha256, &meta_blob)
            }),
            write_text,
        }
    };
    // Issue #1062 评论 7194373427 第 2 项：先 durable 写出不可变 blob，再把
    // `textBlob` 原子安装进 meta。两步之间崩溃时 meta 仍指旧正文，新 blob 只是
    // 孤儿；绝不会出现“旧 meta 摘要校验新正文”的版本对。
    if let Some(text) = plan.text.as_deref() {
        store::write_text_blob(
            &location.dir,
            plan.written_revision,
            &plan.written_sha256,
            text,
        )?;
    }
    if let Some(meta) = plan.meta.as_ref() {
        store::write_meta(&location.dir, meta)?;
    }
    if let Some(state) = lock_registry().get_mut(&location.key) {
        if plan.write_text {
            state.persisted_text_revision = plan.written_revision;
            state.persisted_text_sha256 = plan.written_sha256.clone();
            state.persisted_text_blob = plan.written_blob.clone();
            if state.text_revision == plan.written_revision
                && state.text_sha256 == plan.written_sha256
            {
                state.text_dirty = false;
            }
        }
        state.meta_dirty = false;
        state.last_persist = Some(now);
    }
    // 引用切换成功后才回收旧 blob 与旧 text.txt；失败只留下孤儿文件，读取端
    // 永远只认 meta 引用的那一份。
    if plan.write_text {
        if let Err(error) = store::gc_text_blobs(&location.dir, Some(&plan.written_blob)) {
            log::warn!(
                "gc chapter draft text blobs deferred: project={} chapter={}: {error}",
                location.project_id,
                location.chapter_id
            );
        }
    }
    Ok(())
}

/// 草稿被 durable 决议完全确认后删除（terminal tombstone + 目录 + 旧格式文件）。
///
/// Issue #1062 评论 7195690539（附带结算链）：terminal 决议必须**先成功持久化**
/// 才允许回收目录。tombstone 写失败时返回 Err 并保留目录与 registry，调用方
/// 必须把它当成“本章仍在活动槽位”处理（归档或按章节阻断），不能静默删除。
/// 目录回收失败只延迟空间释放：terminal 已 durable，下次加载继续重试 GC。
pub(crate) fn remove(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Result<(), String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Ok(());
    };
    match store::read_meta_checked(&location.dir, &location.project_id, &location.chapter_id) {
        Ok(mut meta) => {
            meta.terminal = true;
            meta.facts.clear();
            meta.text_blob.clear();
            if let Err(error) = store::write_meta(&location.dir, &meta) {
                return Err(format!(
                    "write settled chapter draft tombstone failed: project={} chapter={}: {error}",
                    location.project_id, location.chapter_id
                ));
            }
        }
        Err(error) if error.is_empty() => {
            // 没有 meta：没有可持久化的决议，目录内容由下面统一回收。
        }
        Err(error) => {
            return Err(format!(
                "read chapter draft meta before settle failed: project={} chapter={}: {error}",
                location.project_id, location.chapter_id
            ));
        }
    }
    lock_registry().remove(&location.key);
    if let Err(error) = store::remove_settled_storage(&location) {
        // 决议已 durable；目录回收是可重试的独立 GC，不能反过来让调用方
        // 以为数据仍在活动槽位。
        log::warn!(
            "settled chapter draft gc deferred: project={} chapter={}: {error}",
            location.project_id,
            location.chapter_id
        );
    }
    Ok(())
}

// ── 保留草稿归档（Issue #1062 评论 7194373427 第 1 项）────────────────────

/// 把恢复失败、被保留（kept）的草稿目录搬进独立归档区。
///
/// 归档后 `chapter-drafts/<chapter_id>/` 不再存在，编辑器随后打开同一章时
/// `note_text` 会创建全新会话目录，不会覆盖旧草稿正文/状态。返回
/// `Ok(Some(path))` 表示已归档；`Ok(None)` 表示没有可归档内容（不创建归档
/// 目录）；`Err` 表示搬移失败，原目录必须保持原样，调用方不能当成“已保留”。
pub(crate) fn quarantine(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Result<Option<String>, String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Ok(None);
    };
    if !location.dir.is_dir() || !store::draft_dir_has_content(&location.dir) {
        return Ok(None);
    }
    let meta = store::read_meta_checked(&location.dir, project_id, chapter_id).ok();
    let stamp = quarantine_stamp(meta.as_ref());
    let chapter_root = store::kept_root(projects_root, project_id).join(chapter_id);
    let mut target = chapter_root.join(&stamp);
    // 同一毫秒重复归档同一章时追加 -1、-2…，绝不覆盖已有归档。
    let mut attempt = 1u32;
    while target.exists() {
        if attempt > 999 {
            return Err(format!(
                "quarantine chapter draft: no free stamp for project={project_id} chapter={chapter_id} stamp={stamp}"
            ));
        }
        target = chapter_root.join(format!("{stamp}-{attempt}"));
        attempt = attempt.saturating_add(1);
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "create chapter draft quarantine dir {}: {error}",
                parent.display()
            )
        })?;
    }
    // 只搬移目录，不复制不删除：rename 失败时原目录必须原封不动，恢复链
    // 下次仍然看得到它。
    std::fs::rename(&location.dir, &target).map_err(|error| {
        format!(
            "move chapter draft to quarantine {} -> {}: {error}",
            location.dir.display(),
            target.display()
        )
    })?;
    // 搬移成功后才丢弃内存状态；失败路径不能动 registry。
    lock_registry().remove(&location.key);
    log::info!(
        "quarantined chapter draft: project={project_id} chapter={chapter_id} target={}",
        target.display()
    );
    Ok(Some(target.to_string_lossy().into_owned()))
}

/// 归档目录名：`<now_ms>-s<session>-<generation>`，只允许 ASCII 字母数字与
/// `-`；generation 为空时不留空段。meta 不可读时退化成只有时间与会话。
fn quarantine_stamp(meta: Option<&store::MetaData>) -> String {
    let session = meta.map_or(0, |meta| meta.session_id);
    let generation: String = meta.map_or_else(String::new, |meta| {
        meta.generation
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
            .collect()
    });
    if generation.is_empty() {
        format!("{}-s{session}", now_ms())
    } else {
        format!("{}-s{session}-{generation}", now_ms())
    }
}

/// 当前已归档（不可写）的保留草稿章节 id；QML 诊断用。
pub(crate) fn kept_drafts(projects_root: &Path, project_id: &str) -> Vec<String> {
    store::list_kept_chapter_ids(projects_root, project_id)
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
    let loaded = match store::load_state(location) {
        Ok(loaded) => loaded,
        Err(error) => {
            log::warn!(
                "chapter draft load failed (kept): project={} chapter={}: {error}",
                location.project_id,
                location.chapter_id
            );
            return false;
        }
    };
    let mut drafts = lock_registry();
    if !drafts.contains_key(&location.key) {
        let state = loaded.unwrap_or_else(|| DraftState::new(location, volume_id));
        drafts.insert(location.key.clone(), state);
    }
    true
}

/// 确保状态可用：Ok(true)=已就绪，Ok(false)=没有草稿，Err=磁盘读到损坏数据。
fn ensure_state_result(location: &DraftLocation) -> Result<bool, String> {
    if lock_registry().contains_key(&location.key) {
        return Ok(true);
    }
    match store::load_state(location)? {
        Some(loaded) => {
            lock_registry()
                .entry(location.key.clone())
                .or_insert(loaded);
            Ok(true)
        }
        None => Ok(false),
    }
}

fn ensure_state(location: &DraftLocation) -> bool {
    if lock_registry().contains_key(&location.key) {
        return true;
    }
    let loaded = match store::load_state(location) {
        Ok(Some(loaded)) => loaded,
        Ok(None) => return false,
        Err(error) => {
            log::warn!(
                "chapter draft load failed (kept): project={} chapter={}: {error}",
                location.project_id,
                location.chapter_id
            );
            return false;
        }
    };
    lock_registry()
        .entry(location.key.clone())
        .or_insert(loaded);
    true
}

/// `persist_current` 在锁外执行 I/O 所需的不可变快照。
struct PersistPlan {
    text: Option<String>,
    written_revision: u64,
    written_sha256: String,
    written_blob: String,
    meta: Option<store::MetaData>,
    write_text: bool,
}
