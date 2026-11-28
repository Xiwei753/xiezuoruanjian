// =============================================================================
// chapter_save_queue.rs — 章节正文后台有序提交队列
// =============================================================================
//
// Issue #1059（评论 7160801242 第 4 项）：Qt 的正文保存不能再在 GUI 线程同步
// 调用 Core 写盘。QML 只把一份不可变快照（章节身份、正文、base 版本、base
// 正文、allow_empty、编辑器快照 revision/sha256）交给这里的单一 worker：
//
//   - UI 线程 `enqueue` 立刻返回 request id，并登记一条 pending 结果；
//   - 进程内唯一 worker 按入队顺序串行执行，同一章节的请求只会看到前一请求
//     已完成的回执；后一请求的 base 版本/base 正文由 worker 用前一回执推进，
//     QML 不需要（也不允许）在等待期间自行改写 committedChapter*；
//   - 完成结果写入进程内结果表，GUI 线程通过 `poll` / `wait` 取回；
//   - worker 不接触任何 QObject/QML 对象，只持有自己的 WriterCoreApi。
//
// 冲突语义：Core 的 `save_chapter_content_versioned` 在 base 版本不匹配时
// 不发布正文，并把 base/local/incoming 三方材料写到
// `<project_root>/app-meta/chapter-commits/conflicts/<transaction_id>/`。
// Core 的 CHAPTER_VERSION_CONFLICT 回执 params 直接携带真实 transactionId 与
// conflictDir；这里只把回执路径转给 QML，不按目录 mtime/正文相等猜测材料，
// 不复制正文、不维护第二套冲突状态。
// =============================================================================

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use writer_core::api::types::{ChapterSaveReceiptDto, ChapterVersionDto};
use writer_core::api::WriterCoreApi;
use writer_core::storage::git_repo_layout::GitRepoLayout;

use worker::chapter_save_worker_loop;
use worker::ChapterSaveWorkerState;

mod core_ack;
mod draft_jobs;
mod kept_draft;
mod restore_coordinator;
mod restore_draft;
#[cfg(test)]
mod tests;
mod worker;

pub(crate) use draft_jobs::{
    draft_restore_status, enqueue_fact, enqueue_restore_project, ChapterDraftRestoreStatus,
    ChapterDraftTextCommand, ChapterFactCommand, ChapterRestoreCommand,
};

/// 保存任务所属工作区。AppRef / AppBackend 仍只在 GUI 线程使用；
/// 后台只拿 roots + 已 bootstrap 的 GitRepoLayout 重新构造 WriterCoreApi。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ChapterSaveContext {
    data_root: String,
    projects_root: String,
    worktree_root: String,
    git_dir: String,
}

impl ChapterSaveContext {
    pub(crate) fn new(data_root: &str, projects_root: &str, layout: &GitRepoLayout) -> Self {
        Self {
            data_root: data_root.to_string(),
            projects_root: projects_root.to_string(),
            worktree_root: layout.worktree_root.to_string_lossy().into_owned(),
            git_dir: layout.git_dir.to_string_lossy().into_owned(),
        }
    }

    pub(crate) fn projects_root(&self) -> &str {
        &self.projects_root
    }

    fn key(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.data_root, self.projects_root, self.worktree_root, self.git_dir
        )
    }

    fn layout(&self) -> GitRepoLayout {
        GitRepoLayout {
            worktree_root: PathBuf::from(&self.worktree_root),
            git_dir: PathBuf::from(&self.git_dir),
        }
    }
}

/// 编辑器正文快照：GUI 只投递不可变数据，不直接改写草稿 registry。
///
/// Issue #1062 评论 7191076411 第 2 项：正文 revision/sha256 与事实连续水位、
/// durable base 必须由同一章节 actor（本章程唯一 worker）串行维护。GUI 线程
/// 把最新快照放进 [`DRAFT_TEXT_MAILBOX`]（同一章节只保留一份），命令本身只
/// 带身份；worker 处理到该命令时才在 actor 线程调用 `chapter_draft::note_text`。
pub(crate) struct DraftTextSnapshot {
    pub text: Arc<str>,
    pub revision: u64,
    pub sha256: String,
    /// 本次文档加载时的编辑器 revision（编辑器坐标，不是章节持久化 revision）。
    pub editor_base_revision: u64,
    pub session_id: u64,
    /// 对象 generation（当前编辑器载入的版本）；base 未初始化时用于身份。
    pub generation: String,
    pub base_generation: String,
    pub base_revision: u64,
    pub base_sha256: String,
    /// true = 保存/关窗/离开作品边界，落盘时立即写整章。
    pub force: bool,
    /// 快照产生时间，仅用于诊断快照在 mailbox 里等待了多久。
    pub updated_at: Instant,
}

/// 正文快照 mailbox：key = workspace + 项目 + 章节，同一章节只保留最新一份。
///
/// 旧实现由 GUI 线程直接改写草稿 registry、再投递只带身份的命令；Registry
/// 里的正文可能已经比事实队列新。现在正文快照和命令一起按 actor 处理顺序
/// 生效，正文也不再随每次编辑进入无界 mpsc。
static DRAFT_TEXT_MAILBOX: OnceLock<Mutex<HashMap<String, DraftTextSnapshot>>> = OnceLock::new();

fn draft_text_mailbox() -> &'static Mutex<HashMap<String, DraftTextSnapshot>> {
    DRAFT_TEXT_MAILBOX.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_draft_text_mailbox() -> MutexGuard<'static, HashMap<String, DraftTextSnapshot>> {
    draft_text_mailbox()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn draft_text_mailbox_key(
    context: &ChapterSaveContext,
    project_id: &str,
    chapter_id: &str,
) -> String {
    format!("{}\u{1f}{}\u{1f}{}", context.key(), project_id, chapter_id)
}

/// UI 线程：把最新正文快照覆盖进该章 mailbox，再投递只带身份的唤醒命令。
///
/// “覆盖 + 发送”在同一把 mailbox 锁内完成，worker 取走的一定是不旧于本次
/// 命令的快照；同一章节的旧快照直接被最新一份替换，不排队。
pub(crate) fn enqueue_draft_text(
    command: ChapterDraftTextCommand,
    snapshot: DraftTextSnapshot,
) -> Result<(), String> {
    let key = draft_text_mailbox_key(&command.context, &command.project_id, &command.chapter_id);
    let mut mailbox = lock_draft_text_mailbox();
    mailbox.insert(key.clone(), snapshot);
    let sent = worker_sender().send(ChapterSaveJob::DraftText(command));
    if sent.is_err() {
        // worker 已退出：没有消费者会再来取这份快照，立刻清理。
        mailbox.remove(&key);
        drop(mailbox);
        return Err("chapter save queue worker has exited".to_string());
    }
    Ok(())
}

/// worker 线程：取走该章最新正文快照（同一章节只保留一份）。
pub(crate) fn take_draft_text_snapshot(
    context: &ChapterSaveContext,
    project_id: &str,
    chapter_id: &str,
) -> Option<DraftTextSnapshot> {
    lock_draft_text_mailbox().remove(&draft_text_mailbox_key(context, project_id, chapter_id))
}

/// worker 退出时清空 mailbox：命令不会再被消费，正文快照不能留在进程里。
pub(super) fn clear_draft_text_mailbox() {
    lock_draft_text_mailbox().clear();
}

/// 一次保存请求的不可变身份。结果回执原样带回这些字段，QML 用它校验
/// “仍是同一个文档 + 同一个会话 + 同一份快照”后才推进 base。
#[derive(Clone, Debug)]
pub(crate) struct ChapterSaveIdentity {
    pub request_id: u32,
    pub session_id: u64,
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub snapshot_revision: u64,
    pub snapshot_sha256: String,
}

/// UI 线程构造、后台 worker 消费的保存任务。只带业务事实，不带 QObject。
pub(crate) struct ChapterSaveRequest {
    pub identity: ChapterSaveIdentity,
    /// 编辑器已提交快照正文（同一时点的 get_save_snapshot）。
    pub content: Arc<str>,
    pub allow_empty_overwrite: bool,
    /// 调用方 base 版本；None 表示旧的无版本写入语义（不做 CAS）。
    pub expected: Option<ChapterVersionDto>,
    /// 冲突材料里的 base 正文，只用于三方材料，不参与正文比较。
    pub base_content: Option<String>,
    pub context: ChapterSaveContext,
}

/// QML 传入的快照元数据（不含正文本身；正文 / base 正文单独作为 QString 传，
/// 避免在大章节保存时把整章正文额外 JSON 转义一次）。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChapterSaveSnapshotMeta {
    pub session_id: u64,
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    #[serde(default)]
    pub allow_empty_overwrite: bool,
    #[serde(default)]
    pub expected_generation: String,
    #[serde(default)]
    pub expected_revision: u64,
    #[serde(default)]
    pub expected_content_sha256: String,
    #[serde(default)]
    pub snapshot_revision: u64,
    #[serde(default)]
    pub snapshot_sha256: String,
}

impl ChapterSaveSnapshotMeta {
    pub(crate) fn parse(raw: &str) -> Result<Self, String> {
        serde_json::from_str(raw)
            .map_err(|error| format!("invalid chapter save snapshot meta: {error}"))
    }

    pub(crate) fn expected_version(&self) -> Option<ChapterVersionDto> {
        let has_expected = !self.expected_generation.is_empty()
            || self.expected_revision > 0
            || !self.expected_content_sha256.is_empty();
        has_expected.then(|| ChapterVersionDto {
            object_generation: self.expected_generation.clone(),
            revision: self.expected_revision,
            content_sha256: self.expected_content_sha256.clone(),
        })
    }

    pub(crate) fn identity(&self, request_id: u32) -> ChapterSaveIdentity {
        ChapterSaveIdentity {
            request_id,
            session_id: self.session_id,
            project_id: self.project_id.clone(),
            volume_id: self.volume_id.clone(),
            chapter_id: self.chapter_id.clone(),
            snapshot_revision: self.snapshot_revision,
            snapshot_sha256: self.snapshot_sha256.clone(),
        }
    }
}

/// 后台保存的终态错误。`code` / `message_key` 直接来自 Core 的稳定错误码契约。
#[derive(Clone, Debug)]
pub(crate) struct ChapterSaveError {
    pub code: String,
    pub message_key: String,
    pub message_args: serde_json::Value,
    pub raw_error: String,
    /// 是否 CHAPTER_VERSION_CONFLICT（冲突材料已在 Core 落盘）。
    pub conflict: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum ChapterSaveOutcome {
    Pending,
    Ok(ChapterSaveReceiptDto),
    Error(ChapterSaveError),
    /// 同一会话里更新的快照已经入队：本请求不再执行，等待方续接到
    /// `by`（Issue #1062 1.1：合并同一会话尚未执行的保存快照）。
    Superseded {
        by: u32,
    },
}

/// 结果表里的一条记录。正文用 Arc<str> 保存，轮询 pending 不会复制整章正文。
struct ChapterSaveEntry {
    identity: ChapterSaveIdentity,
    content: Arc<str>,
    outcome: ChapterSaveOutcome,
    chained: bool,
    conflict_draft_path: String,
    state: ChapterSaveState,
    delivered: bool,
    /// Issue #1069：UI 已明确放弃该会话的结果订阅；条目仍保留最小回执
    /// 元数据供迟到查询，但整章正文副本可以立即释放，且不再触发副作用。
    abandoned: bool,
}

/// GUI 线程读到的结果视图。
pub(crate) struct ChapterSaveSnapshot {
    pub identity: ChapterSaveIdentity,
    pub content: Arc<str>,
    pub outcome: ChapterSaveOutcome,
    pub chained: bool,
    pub conflict_draft_path: String,
    pub terminal: bool,
    /// 本次读取是否第一次消费终态（副作用只允许应用一次）。
    pub delivered_now: bool,
}

pub(crate) enum ChapterSaveWait {
    Found(ChapterSaveSnapshot),
    Timeout,
    Unknown,
}

/// 结果表只保留最近一段时间的记录：QML 轮询结束即删除本地描述，但迟到的
/// 轮询（例如 wait 与 Timer 都读取同一 id）仍应拿到同一份结果，不能报 unknown。
const MAX_RETAINED_RESULTS: usize = 128;
/// worker 为每个（项目, 章节）保留一条 base 链；限制条数，避免长会话里把
/// 每章正文都留在内存。
const MAX_CHAPTER_CHAINS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChapterSaveState {
    /// 已入队、正文在 mailbox，等待 worker 认领；可被同会话更高 revision 接替。
    Pending,
    /// worker 已原子认领并执行；不再可合并，channel 里只有 id。
    Started,
    /// 已有终态回执（Ok / Error / Superseded）。
    Terminal,
}

#[derive(Default)]
struct ChapterSaveStore {
    entries: HashMap<u32, ChapterSaveEntry>,
    order: VecDeque<u32>,
    /// 待执行请求：只在 Pending 状态持有完整快照；worker 认领时 move 走，
    /// channel 里只传 request id，正文不留在无界 mpsc 队列里。
    mailbox: HashMap<u32, ChapterSaveRequest>,
    /// 合并索引：workspace + 对象 generation + 真实会话 -> 最近的 Pending 请求。
    latest_pending: HashMap<String, u32>,
}

/// 保存队列占用快照（Issue #1062 3.4：保存/证明队列字节数）。
///
/// 只统计仍在结果表/mailbox 里的请求：终态且已交付的正文在交付后即释放，
/// 这里反映的是“真正还占着内存”的部分，不用历史累计值。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ChapterSaveQueueMetrics {
    /// 尚未终态的请求数（等待或正在写盘）。
    pub pending_requests: usize,
    /// 结果表保留的记录数（含已交付但未淘汰的回执）。
    pub retained_results: usize,
    /// 仍在内存里的正文快照字节数（结果表 + mailbox）。
    pub content_bytes: usize,
    /// 仍在 mailbox 里等待认领的请求数。
    pub mailbox_requests: usize,
    /// mailbox 里等待认领的正文快照字节数。
    pub mailbox_bytes: usize,
}

/// 读取保存队列当前占用。诊断线程使用，不阻塞 worker。
pub(crate) fn queue_metrics() -> ChapterSaveQueueMetrics {
    let (mutex, _) = results();
    let Ok(store) = mutex.lock() else {
        return ChapterSaveQueueMetrics::default();
    };
    let mut metrics = ChapterSaveQueueMetrics {
        retained_results: store.entries.len(),
        ..ChapterSaveQueueMetrics::default()
    };
    for entry in store.entries.values() {
        if entry.state != ChapterSaveState::Terminal {
            metrics.pending_requests += 1;
        }
        metrics.content_bytes = metrics.content_bytes.saturating_add(entry.content.len());
    }
    // mailbox 里的请求在 worker 认领前同样占着整章正文；单独统计，便于区分
    // “已入队未执行”的积压与结果表回执占用（Issue #1062 三.4）。
    for request in store.mailbox.values() {
        metrics.mailbox_requests = metrics.mailbox_requests.saturating_add(1);
        metrics.mailbox_bytes = metrics.mailbox_bytes.saturating_add(request.content.len());
        metrics.content_bytes = metrics.content_bytes.saturating_add(request.content.len());
    }
    metrics
}

static CHAPTER_SAVE_RESULTS: OnceLock<(Mutex<ChapterSaveStore>, Condvar)> = OnceLock::new();
static NEXT_REQUEST_ID: AtomicU32 = AtomicU32::new(1);
static CHAPTER_SAVE_WORKER: OnceLock<mpsc::Sender<ChapterSaveJob>> = OnceLock::new();

fn results() -> &'static (Mutex<ChapterSaveStore>, Condvar) {
    CHAPTER_SAVE_RESULTS.get_or_init(|| (Mutex::new(ChapterSaveStore::default()), Condvar::new()))
}

fn lock_store() -> std::sync::MutexGuard<'static, ChapterSaveStore> {
    let (mutex, _) = results();
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn clear_pending_index(store: &mut ChapterSaveStore, request_id: u32) {
    store
        .latest_pending
        .retain(|_, pending_id| *pending_id != request_id);
}

/// 全进程单调 request id；0 保留给“入队失败”。
pub(crate) fn next_request_id() -> u32 {
    loop {
        let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            return id;
        }
    }
}

fn prune_store(store: &mut ChapterSaveStore) {
    // Issue #1069：结果缓存和正文副本各有确定生命周期。先回收“终态且已交付/
    // 已放弃”的条目；如果表已满但这些条目都还没交付，也必须回收最老的终态
    // 条目，不能因为全是 delivered=false 就永远无法回收。
    while store.entries.len() >= MAX_RETAINED_RESULTS {
        if evict_retained_entry(store) {
            continue;
        }
        break;
    }
}

/// 回收一条可淘汰的终态记录；返回是否真的移除了条目。
fn evict_retained_entry(store: &mut ChapterSaveStore) -> bool {
    let slots = store.order.len();
    // 第一轮：终态且结果已被消费（或会话已放弃）——正文和回执都不再需要。
    for _ in 0..slots {
        let Some(oldest) = store.order.pop_front() else {
            return false;
        };
        let removable = store.entries.get(&oldest).is_some_and(|entry| {
            entry.state == ChapterSaveState::Terminal && (entry.delivered || entry.abandoned)
        });
        if removable {
            store.entries.remove(&oldest);
            return true;
        }
        store.order.push_back(oldest);
    }
    // 第二轮：全是未交付的终态历史。回收最老的一条；迟到轮询会拿到
    // unknown，调用方按显式失败收口，不会静默推进 base。
    for _ in 0..slots {
        let Some(oldest) = store.order.pop_front() else {
            return false;
        };
        let terminal = store
            .entries
            .get(&oldest)
            .is_some_and(|entry| entry.state == ChapterSaveState::Terminal);
        if terminal {
            // 正文副本先释放，再丢弃条目本身。
            if let Some(entry) = store.entries.get_mut(&oldest) {
                entry.content = Arc::from("");
            }
            store.entries.remove(&oldest);
            return true;
        }
        store.order.push_back(oldest);
    }
    false
}

/// 登记一条 Pending 请求（正文同时进入 mailbox；channel 只传 id）。
fn register_pending_entry(store: &mut ChapterSaveStore, request: ChapterSaveRequest) -> u32 {
    let request_id = request.identity.request_id;
    prune_store(store);
    store.order.push_back(request_id);
    store.entries.insert(
        request_id,
        ChapterSaveEntry {
            identity: request.identity.clone(),
            content: Arc::clone(&request.content),
            outcome: ChapterSaveOutcome::Pending,
            chained: false,
            conflict_draft_path: String::new(),
            state: ChapterSaveState::Pending,
            delivered: false,
            abandoned: false,
        },
    );
    store.mailbox.insert(request_id, request);
    request_id
}

/// 记录一个在入队前就确定失败的结果（Core 未初始化 / worker 已退出）。
pub(crate) fn fail_before_enqueue(
    identity: ChapterSaveIdentity,
    content: Arc<str>,
    error: ChapterSaveError,
) {
    let request_id = identity.request_id;
    {
        let mut store = lock_store();
        prune_store(&mut store);
        store.order.push_back(request_id);
        store.entries.insert(
            request_id,
            ChapterSaveEntry {
                identity,
                content,
                outcome: ChapterSaveOutcome::Pending,
                chained: false,
                conflict_draft_path: String::new(),
                state: ChapterSaveState::Pending,
                delivered: false,
                abandoned: false,
            },
        );
    }
    complete_error(request_id, error, false, String::new());
}

pub(crate) fn complete_ok(request_id: u32, receipt: ChapterSaveReceiptDto, chained: bool) {
    let (_, condvar) = results();
    let mut store = lock_store();
    if let Some(entry) = store.entries.get_mut(&request_id) {
        entry.outcome = ChapterSaveOutcome::Ok(receipt);
        entry.chained = chained;
        entry.state = ChapterSaveState::Terminal;
        // Issue #1069：会话已放弃的结果不会再交付 UI，正文副本现在即可释放。
        if entry.abandoned {
            entry.content = Arc::from("");
        }
    }
    store.mailbox.remove(&request_id);
    clear_pending_index(&mut store, request_id);
    condvar.notify_all();
}

pub(crate) fn complete_error(
    request_id: u32,
    error: ChapterSaveError,
    chained: bool,
    conflict_draft_path: String,
) {
    let (_, condvar) = results();
    let mut store = lock_store();
    if let Some(entry) = store.entries.get_mut(&request_id) {
        entry.outcome = ChapterSaveOutcome::Error(error);
        entry.chained = chained;
        entry.conflict_draft_path = conflict_draft_path;
        entry.state = ChapterSaveState::Terminal;
        // Issue #1069：已放弃会话的终态正文不再需要保留给 UI。
        if entry.abandoned {
            entry.content = Arc::from("");
        }
    }
    store.mailbox.remove(&request_id);
    clear_pending_index(&mut store, request_id);
    condvar.notify_all();
}

/// 原子认领：同一把锁里完成 Pending -> Started 与 mailbox 取出。
///
/// 返回 `None` 表示请求已被同会话更高 revision 接替（Superseded）或已被
/// 淘汰，worker 必须直接丢弃，不能再执行旧快照。
fn claim_save(request_id: u32) -> Option<ChapterSaveRequest> {
    let mut store = lock_store();
    let pending = store
        .entries
        .get(&request_id)
        .is_some_and(|entry| entry.state == ChapterSaveState::Pending);
    if !pending {
        store.mailbox.remove(&request_id);
        return None;
    }
    let request = store.mailbox.remove(&request_id);
    if request.is_none() {
        if let Some(entry) = store.entries.get_mut(&request_id) {
            entry.outcome = ChapterSaveOutcome::Error(ChapterSaveError {
                code: "SAVE_MAILBOX_MISSING".to_string(),
                message_key: "error.save_mailbox_missing".to_string(),
                message_args: serde_json::json!({}),
                raw_error: format!("request {request_id} pending without mailbox body"),
                conflict: false,
            });
            entry.state = ChapterSaveState::Terminal;
        }
        clear_pending_index(&mut store, request_id);
        return None;
    }
    if let Some(entry) = store.entries.get_mut(&request_id) {
        entry.state = ChapterSaveState::Started;
    }
    request
}

/// 同会话更高 revision 的快照接替旧 Pending 请求（同一把锁内完成状态转换）。
fn supersede_pending(
    store: &mut ChapterSaveStore,
    previous: u32,
    successor: u32,
    successor_revision: u64,
) -> bool {
    let replaceable = store.entries.get(&previous).is_some_and(|entry| {
        entry.state == ChapterSaveState::Pending
            && successor_revision > entry.identity.snapshot_revision
    });
    if !replaceable {
        return false;
    }
    store.mailbox.remove(&previous);
    if let Some(entry) = store.entries.get_mut(&previous) {
        entry.outcome = ChapterSaveOutcome::Superseded { by: successor };
        entry.state = ChapterSaveState::Terminal;
        entry.content = Arc::from("");
    }
    clear_pending_index(store, previous);
    true
}

fn snapshot_entry(entry: &mut ChapterSaveEntry, delivered_now: bool) -> ChapterSaveSnapshot {
    let snapshot = ChapterSaveSnapshot {
        identity: entry.identity.clone(),
        content: Arc::clone(&entry.content),
        outcome: entry.outcome.clone(),
        chained: entry.chained,
        conflict_draft_path: entry.conflict_draft_path.clone(),
        terminal: entry.state == ChapterSaveState::Terminal,
        delivered_now,
    };
    let terminal = entry.state == ChapterSaveState::Terminal;
    if terminal && (delivered_now || entry.abandoned) {
        // 终态首次交付（或会话已放弃）后不再需要正文副本：调用方只在
        // delivered_now 时应用保存副作用，保留整章正文只会让结果表随历史
        // 回执持续增长（Issue #1069：正文副本与结果缓存各有生命周期）。
        entry.content = Arc::from("");
    }
    snapshot
}

/// 非阻塞读取。终态只标记一次 delivered_now，调用方据此决定是否应用
/// current_save_status / 搜索索引等副作用。
pub(crate) fn poll(request_id: u32) -> Option<ChapterSaveSnapshot> {
    let (mutex, _) = results();
    let mut store = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let entry = store.entries.get_mut(&request_id)?;
    let delivered_now = entry.state == ChapterSaveState::Terminal && !entry.delivered;
    if delivered_now {
        entry.delivered = true;
    }
    Some(snapshot_entry(entry, delivered_now))
}

/// 切章 / 同步前等需要落盘保证的边界使用。只阻塞 GUI 线程等待 worker 完成
/// 写盘，不把 Core I/O 放回 GUI 线程；超时返回 Timeout，由调用方阻止切章。
pub(crate) fn wait(request_id: u32, timeout: Duration) -> ChapterSaveWait {
    let (mutex, condvar) = results();
    let mut store = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let deadline = Instant::now() + timeout;
    loop {
        let Some(entry) = store.entries.get_mut(&request_id) else {
            return ChapterSaveWait::Unknown;
        };
        if entry.state == ChapterSaveState::Terminal {
            let delivered_now = !entry.delivered;
            entry.delivered = true;
            return ChapterSaveWait::Found(snapshot_entry(entry, delivered_now));
        }
        let now = Instant::now();
        if now >= deadline {
            return ChapterSaveWait::Timeout;
        }
        let waited = condvar.wait_timeout(store, deadline - now);
        let (guard, _) = waited.unwrap_or_else(|poisoned| poisoned.into_inner());
        store = guard;
    }
}

/// Issue #1069：UI 明确放弃某个 `chapterSaveSession` 的结果订阅。
///
/// 只放弃结果交付与正文副本，不取消已经开始的 Core 写盘：in-flight 请求仍由
/// worker 完成，邮件箱里的待执行快照也照常执行。已终态条目的正文立即释放；
/// 未终态条目在 worker 结束时释放，迟到轮询只拿到最小回执元数据。
pub(crate) fn abandon_session_results(session_id: u64) {
    let (_, condvar) = results();
    let mut store = lock_store();
    let mut affected = 0usize;
    for entry in store.entries.values_mut() {
        if entry.identity.session_id != session_id {
            continue;
        }
        affected += 1;
        entry.abandoned = true;
        // 放弃后不再触发 current_save_status / 搜索索引等副作用。
        entry.delivered = true;
        entry.content = Arc::from("");
    }
    prune_store(&mut store);
    drop(store);
    condvar.notify_all();
    if affected > 0 {
        log::info!(
            "[chapter-save-queue] abandoned {affected} save results for session {session_id}"
        );
    }
}

enum ChapterSaveJob {
    /// 只传 request id；完整快照在 store.mailbox，worker 认领时原子取走。
    Save(u32),
    /// 一条编辑事实：actor 构造原始证明投影并持久化为可恢复草稿
    /// （Issue #1062 1.1：编辑事实与保存命令同一 actor）。
    RecordFact(ChapterFactCommand),
    /// 编辑器正文变化：更新草稿正文；`force` 表示保存/离开边界立即落盘。
    DraftText(ChapterDraftTextCommand),
    /// 项目进入时按同一批次协议重放未完成草稿。
    RestoreProject(ChapterRestoreCommand),
    /// 关窗/切工作区前的落盘屏障：worker 处理到这里时此前全部保存已完成。
    Drain(mpsc::Sender<()>),
}

/// 等待已入队的保存全部执行完（只用于关窗 / 切换工作区等退出边界）。
///
/// 返回 false 表示 worker 未启动、已退出或超时；调用方只记日志，不把
/// “队列忙”当成业务失败。真正的正文一致性仍由每笔 save 的 Core 决议保证。
pub(crate) fn drain(timeout: Duration) -> bool {
    let Some(sender) = CHAPTER_SAVE_WORKER.get() else {
        return true;
    };
    let (ack_sender, ack_receiver) = mpsc::channel();
    if sender.send(ChapterSaveJob::Drain(ack_sender)).is_err() {
        return false;
    }
    ack_receiver.recv_timeout(timeout).is_ok()
}

fn worker_sender() -> &'static mpsc::Sender<ChapterSaveJob> {
    CHAPTER_SAVE_WORKER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("chapter-save-queue".to_string())
            .spawn(move || chapter_save_worker_loop(receiver))
            .expect("chapter save queue worker must start");
        sender
    })
}

/// UI 线程调用：登记 pending 结果并按入队顺序交给唯一 worker，立刻返回。
pub(crate) fn enqueue(request: ChapterSaveRequest) -> Result<(), String> {
    let request_id = request.identity.request_id;
    let merge_key = save_merge_key(&request);
    let snapshot_revision = request.identity.snapshot_revision;
    {
        let (_, condvar) = results();
        let mut store = lock_store();
        register_pending_entry(&mut store, request);
        if let Some(previous) = store.latest_pending.get(&merge_key).copied() {
            if previous != request_id
                && supersede_pending(&mut store, previous, request_id, snapshot_revision)
            {
                // 被取代的请求立刻终态：等待方续接到接替者，worker 之后
                // 收到它的 id 时只会认领失败并丢弃。
                condvar.notify_all();
            }
        }
        store.latest_pending.insert(merge_key, request_id);
    }
    let sent = worker_sender().send(ChapterSaveJob::Save(request_id));
    if sent.is_err() {
        let mut store = lock_store();
        store.mailbox.remove(&request_id);
        clear_pending_index(&mut store, request_id);
        drop(store);
        complete_error(
            request_id,
            ChapterSaveError {
                code: "SAVE_QUEUE_UNAVAILABLE".to_string(),
                message_key: "error.save_queue_unavailable".to_string(),
                message_args: serde_json::json!({}),
                raw_error: "chapter save queue worker has exited".to_string(),
                conflict: false,
            },
            false,
            String::new(),
        );
        return Err(format!(
            "chapter save queue worker has exited (request_id={request_id})"
        ));
    }
    Ok(())
}

/// 合并 key：workspace + 项目 + 章节 + 卷 + 对象 generation + 真实会话。
///
/// 只有同一 workspace、同一项目、同一卷、同一章节、同一 generation 且同一
/// 会话里的更高 revision 快照才能接替（见 [`supersede_pending`]）；缺少
/// 章节/卷隔离会让同 workspace 内不同章节在 generation 为空且 session 数值
/// 相同时互相 supersede。卷进入 key 只为避免跨卷移动后误判，把不同卷的同名
/// 章节分开；supersede 仍要求 revision 严格更高。generation 变化（外部重建/
/// 迁移）或会话变化都不合并。
fn save_merge_key(request: &ChapterSaveRequest) -> String {
    let generation = request
        .expected
        .as_ref()
        .map_or("", |version| version.object_generation.as_str());
    format!(
        "{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}",
        request.context.key(),
        request.identity.project_id,
        request.identity.chapter_id,
        request.identity.volume_id,
        generation,
        request.identity.session_id,
    )
}
