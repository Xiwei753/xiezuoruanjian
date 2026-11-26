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
// 因此冲突正文（本次快照的 incoming.md）在返回 CHAPTER_VERSION_CONFLICT 时
// 已经持久化；这里额外只做只读定位，把材料目录回给 QML 便于提示/排查，
// 不复制正文、不维护第二套冲突状态。
// =============================================================================

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use writer_core::api::types::{ChapterSaveReceiptDto, ChapterVersionDto};
use writer_core::api::WriterCoreApi;
use writer_core::storage::git_repo_layout::GitRepoLayout;

use conflict_material::find_conflict_material;

mod conflict_material;
mod draft_jobs;

pub(crate) use draft_jobs::{
    draft_restore_status, enqueue_draft_text, enqueue_fact, enqueue_restore_project,
    ChapterDraftRestoreStatus, ChapterDraftTextCommand, ChapterFactCommand, ChapterRestoreCommand,
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
    terminal: bool,
    delivered: bool,
    /// worker 是否已经开始执行；已开始的请求不能再用新快照合并。
    started: bool,
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
/// 冲突材料定位的时间窗口：只看本轮保存刚写入的材料目录。
pub(super) const CONFLICT_MATERIAL_WINDOW: Duration = Duration::from_secs(120);

#[derive(Default)]
struct ChapterSaveStore {
    entries: HashMap<u32, ChapterSaveEntry>,
    order: VecDeque<u32>,
}

/// 保存队列占用快照（Issue #1062 3.4：保存/证明队列字节数）。
///
/// 只统计仍在结果表里的请求：终态且已交付的正文在交付后即释放，
/// 这里反映的是“真正还占着内存”的部分，不用历史累计值。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ChapterSaveQueueMetrics {
    /// 尚未终态的请求数（等待 worker 写盘）。
    pub pending_requests: usize,
    /// 结果表保留的记录数（含已交付但未淘汰的回执）。
    pub retained_results: usize,
    /// 结果表仍持有的正文快照字节数。
    pub content_bytes: usize,
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
        if !entry.terminal {
            metrics.pending_requests += 1;
        }
        metrics.content_bytes = metrics.content_bytes.saturating_add(entry.content.len());
    }
    metrics
}

static CHAPTER_SAVE_RESULTS: OnceLock<(Mutex<ChapterSaveStore>, Condvar)> = OnceLock::new();
static NEXT_REQUEST_ID: AtomicU32 = AtomicU32::new(1);
static CHAPTER_SAVE_WORKER: OnceLock<mpsc::Sender<ChapterSaveJob>> = OnceLock::new();

/// 同一（项目, 卷, 章节, 会话）最近一次入队的保存 id。
static LATEST_PENDING_SAVES: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new();
/// 已被更新快照取代、尚未执行的保存：旧 id -> 接替者 id。
static SUPERSEDED_SAVES: OnceLock<Mutex<HashMap<u32, u32>>> = OnceLock::new();

fn latest_pending_saves() -> &'static Mutex<HashMap<String, u32>> {
    LATEST_PENDING_SAVES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn superseded_saves() -> &'static Mutex<HashMap<u32, u32>> {
    SUPERSEDED_SAVES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 请求是否已被 worker 取走执行（或已终态）。
fn entry_started(request_id: u32) -> bool {
    let (mutex, _) = results();
    let Ok(store) = mutex.lock() else {
        return true;
    };
    store
        .entries
        .get(&request_id)
        .is_some_and(|entry| entry.started || entry.terminal)
}

fn take_superseded(request_id: u32) -> Option<u32> {
    superseded_saves()
        .lock()
        .ok()
        .and_then(|mut map| map.remove(&request_id))
}

fn results() -> &'static (Mutex<ChapterSaveStore>, Condvar) {
    CHAPTER_SAVE_RESULTS.get_or_init(|| (Mutex::new(ChapterSaveStore::default()), Condvar::new()))
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
    while store.entries.len() >= MAX_RETAINED_RESULTS {
        let Some(oldest) = store.order.pop_front() else {
            return;
        };
        let evict = store
            .entries
            .get(&oldest)
            .map(|entry| entry.terminal && entry.delivered)
            .unwrap_or(true);
        if evict {
            store.entries.remove(&oldest);
        } else {
            // 还在队列里/尚未交付的结果不淘汰，放回队尾等下一次机会。
            store.order.push_back(oldest);
            return;
        }
    }
}

fn register_entry(identity: ChapterSaveIdentity, content: Arc<str>) {
    let (mutex, _) = results();
    let mut store = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    prune_store(&mut store);
    store.order.push_back(identity.request_id);
    store.entries.insert(
        identity.request_id,
        ChapterSaveEntry {
            identity,
            content,
            outcome: ChapterSaveOutcome::Pending,
            chained: false,
            conflict_draft_path: String::new(),
            terminal: false,
            delivered: false,
            started: false,
        },
    );
}

/// 记录一个在入队前就确定失败的结果（Core 未初始化 / worker 已退出）。
pub(crate) fn fail_before_enqueue(
    identity: ChapterSaveIdentity,
    content: Arc<str>,
    error: ChapterSaveError,
) {
    let request_id = identity.request_id;
    register_entry(identity, content);
    complete_error(request_id, error, false, String::new());
}

pub(crate) fn complete_ok(request_id: u32, receipt: ChapterSaveReceiptDto, chained: bool) {
    let (mutex, condvar) = results();
    let mut store = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(entry) = store.entries.get_mut(&request_id) {
        entry.outcome = ChapterSaveOutcome::Ok(receipt);
        entry.chained = chained;
        entry.terminal = true;
    }
    if let Ok(mut superseded) = superseded_saves().lock() {
        superseded.remove(&request_id);
    }
    condvar.notify_all();
}

pub(crate) fn complete_error(
    request_id: u32,
    error: ChapterSaveError,
    chained: bool,
    conflict_draft_path: String,
) {
    let (mutex, condvar) = results();
    let mut store = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(entry) = store.entries.get_mut(&request_id) {
        entry.outcome = ChapterSaveOutcome::Error(error);
        entry.chained = chained;
        entry.conflict_draft_path = conflict_draft_path;
        entry.terminal = true;
    }
    if let Ok(mut superseded) = superseded_saves().lock() {
        superseded.remove(&request_id);
    }
    condvar.notify_all();
}

/// 标记请求已被 worker 取走：此后入队的新快照不能再合并它。
fn mark_entry_started(request_id: u32) {
    let (mutex, _) = results();
    let Ok(mut store) = mutex.lock() else {
        return;
    };
    if let Some(entry) = store.entries.get_mut(&request_id) {
        entry.started = true;
    }
}

/// 被更新快照取代的请求：立刻终态，不再写盘；等待方由 QML 续接到 `by`。
///
/// 在 `enqueue` 里同步调用（worker 还没轮到它），worker 随后只会丢弃该任务。
fn complete_superseded(request_id: u32, superseded_by: u32) {
    let (mutex, condvar) = results();
    let mut store = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(entry) = store.entries.get_mut(&request_id) {
        entry.outcome = ChapterSaveOutcome::Superseded { by: superseded_by };
        entry.terminal = true;
        entry.content = Arc::from("");
    }
    if let Ok(mut superseded) = superseded_saves().lock() {
        superseded.remove(&request_id);
    }
    condvar.notify_all();
}

fn snapshot_entry(entry: &mut ChapterSaveEntry, delivered_now: bool) -> ChapterSaveSnapshot {
    let snapshot = ChapterSaveSnapshot {
        identity: entry.identity.clone(),
        content: Arc::clone(&entry.content),
        outcome: entry.outcome.clone(),
        chained: entry.chained,
        conflict_draft_path: entry.conflict_draft_path.clone(),
        terminal: entry.terminal,
        delivered_now,
    };
    if delivered_now && entry.terminal {
        // 终态首次交付后不再需要正文副本：调用方只在 delivered_now 时应用
        // 保存副作用，保留整章正文只会让结果表随历史回执持续增长
        // （Issue #1062 第 1.3 项：清理已完成回执和历史快照引用）。
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
    let delivered_now = entry.terminal && !entry.delivered;
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
        if entry.terminal {
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

enum ChapterSaveJob {
    Save(ChapterSaveRequest),
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
    let session_key = format!(
        "{}\x1f{}\x1f{}\x1f{}",
        request.identity.project_id,
        request.identity.volume_id,
        request.identity.chapter_id,
        request.identity.session_id,
    );
    register_entry(request.identity.clone(), Arc::clone(&request.content));
    // Issue #1062 1.1：合并同一会话尚未执行的保存快照。编辑事实已经随
    // 每次编辑进入 actor 草稿，替换正文快照不会丢证明事件序列；被取代的
    // 请求立刻返回 Superseded，等待方续接到接替者。
    if let Ok(mut latest) = latest_pending_saves().lock() {
        if let Some(previous) = latest.insert(session_key, request_id) {
            if previous != request_id && !entry_started(previous) {
                if let Ok(mut superseded) = superseded_saves().lock() {
                    superseded.insert(previous, request_id);
                }
                // 立即给出终态：被取代的请求不会执行，QML 无需等 worker
                // 走到它才把等待方续接到接替者。
                complete_superseded(previous, request_id);
            }
        }
    }
    worker_sender()
        .send(ChapterSaveJob::Save(request))
        .map_err(|_| format!("chapter save queue worker has exited (request_id={request_id})"))
}

// ── worker ────────────────────────────────────────────────────────────────

struct ChapterSaveChain {
    /// 本链第一条请求的 base 版本；队列里多份从同一版本入队的快照
    /// 都以它为锚点，不能因为前一请求推进了 expected 就把后续请求判成冲突。
    anchor: Option<ChapterVersionDto>,
    /// 最近一次请求实际使用的 base 版本（成功/失败都记录）。
    expected: Option<ChapterVersionDto>,
    /// 最近一次成功写入后的版本，用来推进后续请求的 base。
    receipt: Option<ChapterVersionDto>,
    /// 与 receipt 对应的正文，作为冲突材料的 base。
    text: Arc<str>,
    /// 已排队/完成请求中最大的编辑器快照 revision。旧快照不得借用新版本。
    snapshot_revision: u64,
    /// 链上出现失败/外部更新后失效，后续请求只能重新对齐磁盘版本。
    invalid: bool,
    last_used: Instant,
}

struct ChapterSaveWorkerState {
    apis: HashMap<String, WriterCoreApi>,
    chains: HashMap<String, ChapterSaveChain>,
}

impl ChapterSaveWorkerState {
    fn new() -> Self {
        Self {
            apis: HashMap::new(),
            chains: HashMap::new(),
        }
    }

    fn api_for(&mut self, context: &ChapterSaveContext) -> &WriterCoreApi {
        let key = context.key();
        self.apis.entry(key).or_insert_with(|| {
            crate::backend::app_backend::with_layout_core_api(
                &context.data_root,
                &context.projects_root,
                &context.layout(),
            )
        })
    }

    fn prune_chains(&mut self, keep_key: &str) {
        if self.chains.len() <= MAX_CHAPTER_CHAINS {
            return;
        }
        let mut candidates: Vec<(Instant, String)> = self
            .chains
            .iter()
            .filter(|(key, _)| key.as_str() != keep_key)
            .map(|(key, chain)| (chain.last_used, key.clone()))
            .collect();
        candidates.sort_by_key(|(last_used, _)| *last_used);
        for (_, key) in candidates {
            if self.chains.len() <= MAX_CHAPTER_CHAINS {
                break;
            }
            self.chains.remove(&key);
        }
    }
}

fn chapter_chain_key(request: &ChapterSaveRequest) -> String {
    // 卷 ID 不进入链身份：Issue #1051 允许当前章节跨卷移动，而 object_generation
    // / revision 随章节留在原地；用项目 + 章节才能在移动后继续用前一回执推进 base。
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}",
        request.context.key(),
        request.identity.project_id,
        request.identity.chapter_id,
        request.identity.session_id
    )
}

fn resolve_chain_base(
    chain: Option<&ChapterSaveChain>,
    expected: &Option<ChapterVersionDto>,
    base_content: &Option<String>,
    snapshot_revision: u64,
) -> (Option<ChapterVersionDto>, Option<String>, bool) {
    let mut effective = expected.clone();
    let mut effective_base = base_content.clone();
    let mut chained = false;
    if let Some(chain) = chain {
        if chain.invalid {
            return (effective, effective_base, false);
        }
        if let (Some(request_base), Some(receipt)) = (expected.as_ref(), chain.receipt.as_ref()) {
            // 请求 base 要么是链当前 expected（QML 已用上一回执推进），
            // 要么仍是链锚点（多份快照在队列积压时都从 V0 入队）。两种
            // 情况都允许接到直接前序回执；但快照必须严格晚于链上已见快照。
            let matches_expected = chain.expected.as_ref().is_some_and(|chain_base| {
                chain_base.object_generation == request_base.object_generation
                    && chain_base.revision == request_base.revision
                    && chain_base.content_sha256 == request_base.content_sha256
            });
            let matches_anchor = chain.anchor.as_ref().is_some_and(|anchor| {
                anchor.object_generation == request_base.object_generation
                    && anchor.revision == request_base.revision
                    && anchor.content_sha256 == request_base.content_sha256
            });
            if (matches_expected || matches_anchor)
                && snapshot_revision > chain.snapshot_revision
                && request_base.object_generation == receipt.object_generation
                && receipt.revision > request_base.revision
            {
                effective = Some(receipt.clone());
                effective_base = Some(chain.text.to_string());
                chained = true;
            }
        }
    }
    (effective, effective_base, chained)
}

fn chapter_save_worker_loop(receiver: mpsc::Receiver<ChapterSaveJob>) {
    let mut state = ChapterSaveWorkerState::new();
    while let Ok(job) = receiver.recv() {
        match job {
            ChapterSaveJob::Save(request) => handle_chapter_save(&mut state, request),
            ChapterSaveJob::RecordFact(command) => {
                draft_jobs::handle_record_fact(&mut state, command);
            }
            ChapterSaveJob::DraftText(command) => draft_jobs::handle_draft_text(command),
            ChapterSaveJob::RestoreProject(command) => {
                draft_jobs::handle_restore_project(&mut state, command);
            }
            ChapterSaveJob::Drain(ack) => {
                let _ = ack.send(());
            }
        }
    }
}

fn handle_chapter_save(state: &mut ChapterSaveWorkerState, request: ChapterSaveRequest) {
    let request_id = request.identity.request_id;
    if take_superseded(request_id).is_some() {
        // 入队时已经被更新快照取代并给出过 Superseded 终态，这里只丢弃任务。
        return;
    }
    mark_entry_started(request_id);
    let chain_key = chapter_chain_key(&request);
    let (effective_expected, effective_base, chained) = {
        let chain = state.chains.get(&chain_key);
        resolve_chain_base(
            chain,
            &request.expected,
            &request.base_content,
            request.identity.snapshot_revision,
        )
    };
    if let Some(chain) = state.chains.get_mut(&chain_key) {
        chain.expected = effective_expected.clone();
        chain.last_used = Instant::now();
    } else {
        state.chains.insert(
            chain_key.clone(),
            ChapterSaveChain {
                anchor: effective_expected.clone(),
                expected: effective_expected.clone(),
                receipt: None,
                text: Arc::from(""),
                snapshot_revision: request.identity.snapshot_revision,
                invalid: false,
                last_used: Instant::now(),
            },
        );
    }
    state.prune_chains(&chain_key);

    let api = state.api_for(&request.context);
    let projects_root = PathBuf::from(&request.context.projects_root);
    // Issue #1062 1.1：证明批次由 actor 从自己的编辑事实草稿里冻结，不再用
    // QML 传来的 event ID 去 Core 全局 pending map 猜批次。快照 revision 之前
    // 的连续事实前缀随正文一起提交；之后的事实留给下一次保存。
    let frozen_facts = match super::chapter_draft::freeze_batch_for(
        &projects_root,
        &request.identity.project_id,
        &request.identity.chapter_id,
        request.identity.snapshot_revision,
    ) {
        Ok(facts) => facts,
        Err(error) => {
            // 冻结失败不能退化成“无证明保存”：否则这批事实的 before 链会
            // 与之后提交的正文基线脱节，后续保存永久 PROOF_BATCH_MISMATCH。
            crate::backend::app_backend::debug_error_static(
                "chapter_save_queue",
                "freeze_proof_batch_failed",
                &format!(
                    "request_id={} project={} chapter={}: {error}",
                    request_id, request.identity.project_id, request.identity.chapter_id
                ),
            );
            complete_error(
                request_id,
                ChapterSaveError {
                    code: "PROOF_BATCH_FREEZE_FAILED".to_string(),
                    message_key: "error.proof_batch_freeze_failed".to_string(),
                    message_args: serde_json::json!({ "detail": error }),
                    raw_error: error,
                    conflict: false,
                },
                chained,
                String::new(),
            );
            return;
        }
    };
    let frozen_event_ids: Vec<String> = frozen_facts
        .iter()
        .map(|fact| fact.event_id.clone())
        .collect();
    let proof_batch_json = match frozen_facts
        .iter()
        .map(|fact| {
            serde_json::from_str::<serde_json::Value>(&fact.projection_json)
                .map_err(|error| format!("event={} projection 无法解析: {error}", fact.event_id))
        })
        .collect::<Result<Vec<_>, _>>()
        .and_then(|values| {
            serde_json::to_string(&values).map_err(|error| format!("批次序列化失败: {error}"))
        }) {
        Ok(json) => json,
        Err(error) => {
            crate::backend::app_backend::debug_error_static(
                "chapter_save_queue",
                "freeze_proof_batch_failed",
                &format!(
                    "request_id={} project={} chapter={}: {error}",
                    request_id, request.identity.project_id, request.identity.chapter_id
                ),
            );
            complete_error(
                request_id,
                ChapterSaveError {
                    code: "PROOF_BATCH_FREEZE_FAILED".to_string(),
                    message_key: "error.proof_batch_freeze_failed".to_string(),
                    message_args: serde_json::json!({ "detail": error }),
                    raw_error: error,
                    conflict: false,
                },
                chained,
                String::new(),
            );
            return;
        }
    };
    let save_result = api.save_chapter_content_versioned_with_proof_batch(
        &request.identity.project_id,
        &request.identity.volume_id,
        &request.identity.chapter_id,
        &request.content,
        effective_expected,
        effective_base,
        request.identity.snapshot_revision,
        &proof_batch_json,
        request.allow_empty_overwrite,
    );

    match save_result {
        Ok(receipt) => {
            // journal 已确认：只 ack 本批包含的事件，快照之后的编辑事实留在
            // 草稿里等下一次保存；没有剩余事实时删除草稿。
            if !frozen_event_ids.is_empty() {
                // 以 journal 回执确认的 event IDs 为准（Core 只回本批进入
                // durable journal 的投影）；回执缺失时才退回冻结批次。
                let acknowledged = if receipt.pending_projections.is_empty() {
                    frozen_event_ids.clone()
                } else {
                    receipt.pending_projections.clone()
                };
                super::chapter_draft::mark_committed(
                    &projects_root,
                    &request.identity.project_id,
                    &request.identity.chapter_id,
                    &acknowledged,
                    &receipt.version.object_generation,
                    receipt.version.revision,
                    &receipt.version.content_sha256,
                );
            } else {
                super::chapter_draft::remove(
                    &projects_root,
                    &request.identity.project_id,
                    &request.identity.chapter_id,
                );
            }
            if let Some(chain) = state.chains.get_mut(&chain_key) {
                chain.receipt = Some(receipt.version.clone());
                chain.text = Arc::clone(&request.content);
                chain.snapshot_revision = chain
                    .snapshot_revision
                    .max(request.identity.snapshot_revision);
                chain.invalid = false;
                chain.last_used = Instant::now();
            }
            complete_ok(request_id, receipt, chained);
        }
        Err(error) => {
            if let Some(chain) = state.chains.get_mut(&chain_key) {
                // 失败后链不能再把后续快照接到旧回执上；下一请求必须
                // 重新读取磁盘版本或显式冲突。
                chain.invalid = true;
                chain.last_used = Instant::now();
            }
            let code = error.code().to_string();
            let conflict = code == "CHAPTER_VERSION_CONFLICT";
            let conflict_draft_path = if conflict {
                // 只读定位：Core 在返回冲突前已写好本次快照的 incoming.md，
                // 这里把目录回给 QML 便于提示/排查，不复制正文、不改状态。
                find_conflict_material(&request)
            } else {
                String::new()
            };
            crate::backend::app_backend::debug_error_static(
                "chapter_save_queue",
                "chapter_save_failed",
                &format!(
                    "request_id={} code={} chained={} conflict_dir={}",
                    request_id, code, chained, conflict_draft_path
                ),
            );
            complete_error(
                request_id,
                ChapterSaveError {
                    code,
                    message_key: error.message_key().to_string(),
                    message_args: serde_json::to_value(error.params())
                        .unwrap_or_else(|_| serde_json::json!({})),
                    raw_error: error.to_string(),
                    conflict,
                },
                chained,
                conflict_draft_path,
            );
        }
    }
}
