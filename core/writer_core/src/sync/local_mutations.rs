//! 本地对象变更事实账本。
//!
//! 成功的 Core 写事务在对象真正落盘后，把本次写过的同步对象记录到
//! `<sync_root>/app-meta/sync/local-mutations.jsonl`。这份账本是本机事实，
//! 不参与同步：同步清单投影用它取得真实的提交时间、设备身份和对象身份，
//! 不再依赖文件 mtime 猜测“这次修改是谁、什么时候写的”。
//!
//! 账本按 `path` 取最后一条记录：同一对象连续编辑在同步版本上聚合为最终
//! 内容，但每次事务的贡献事件仍由统计事件流独立保存。

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sync::types::{AppDataGroup, SyncScope};
use crate::sync::SyncService;

pub(crate) const LOCAL_MUTATIONS_PATH: &str = "app-meta/sync/local-mutations.jsonl";
/// 序号高水位：`seq` 只能单调递增，压缩/归档/重启都不得回退。
const MUTATION_SEQ_PATH: &str = "app-meta/sync/mutation-seq.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct MutationSeqState {
    /// 已分配过的最大序号；下一次分配从 `+1` 开始。
    #[serde(default)]
    high_watermark: u64,
}

fn seq_state_path(sync_root: &Path) -> PathBuf {
    sync_root.join(MUTATION_SEQ_PATH)
}

fn load_seq_state(sync_root: &Path) -> MutationSeqState {
    match fs::read_to_string(seq_state_path(sync_root)) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|error| {
            log::warn!(
                "[sync] mutation seq state unreadable at {}: {error}; falling back to ledger maximum",
                sync_root.display()
            );
            MutationSeqState::default()
        }),
        Err(_) => MutationSeqState::default(),
    }
}

fn save_seq_state(sync_root: &Path, state: &MutationSeqState) -> crate::error::Result<()> {
    let path = seq_state_path(sync_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string(state)?;
    crate::storage::atomic_write_string(&path, &text)
}

/// 所有 target 状态里已确认过的最大序号。
///
/// 迁移旧账本、或账本被压缩/归档后重新开始时，新序号必须大于任何已确认序号，
/// 否则同一路径的新事实会被误判为"已确认"。
fn confirmed_watermark(sync_root: &Path) -> u64 {
    fn watermark_of(path: &Path) -> u64 {
        let Ok(text) = fs::read_to_string(path) else {
            return 0;
        };
        let Ok(state) = serde_json::from_str::<crate::sync::types::SyncState>(&text) else {
            return 0;
        };
        state
            .confirmed_mutation_seq
            .values()
            .copied()
            .max()
            .unwrap_or(0)
    }
    let mut highest = 0u64;
    for candidate in [
        sync_root.join("app-meta/sync/state.local.json"),
        sync_root.join("app-meta/sync/sync_state.json"),
    ] {
        highest = highest.max(watermark_of(&candidate));
    }
    if let Ok(entries) = fs::read_dir(sync_root.join("app-meta/sync/groups")) {
        for entry in entries.flatten() {
            let path = entry.path().join("state.local.json");
            highest = highest.max(watermark_of(&path));
        }
    }
    highest
}

/// 当前序号高水位（只读，用于 target 版本指纹）。
pub(crate) fn seq_high_watermark(sync_root: &Path) -> u64 {
    load_seq_state(sync_root).high_watermark
}

/// 预留 count 个序号并**先持久化高水位**，再交给调用方使用。
fn reserve_sequences(sync_root: &Path, count: usize) -> crate::error::Result<Vec<u64>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut state = load_seq_state(sync_root);
    let ledger_max = load_records(sync_root)?
        .iter()
        .map(|record| record.seq)
        .max()
        .unwrap_or(0);
    let floor = ledger_max.max(confirmed_watermark(sync_root));
    let start = state.high_watermark.max(floor) + 1;
    let count = u64::try_from(count).unwrap_or(u64::MAX);
    state.high_watermark = start.saturating_add(count).saturating_sub(1);
    save_seq_state(sync_root, &state)?;
    Ok((start..=state.high_watermark).collect())
}

/// 账本的写入串行化：Append/Ack/压缩都在同一把工作区写锁内完成。
///
/// 项目 target 的同步根是 `<app>/projects/<id>`，锁仍取 App 数据根，避免
/// 每个项目各拿一把锁导致确认与并发 append 交错。
fn ledger_write_lock(sync_root: &Path) -> Option<crate::storage::WorkspaceWriteLock> {
    let app_root = ledger_app_root(sync_root);
    match crate::storage::WorkspaceWriteLock::acquire(&app_root) {
        Ok(lock) => Some(lock),
        Err(error) => {
            log::warn!(
                "[sync] local mutation ledger lock unavailable for {}: {error}",
                sync_root.display()
            );
            None
        }
    }
}

fn ledger_app_root(sync_root: &Path) -> PathBuf {
    if let Some(parent) = sync_root.parent() {
        if parent.file_name().and_then(|name| name.to_str()) == Some("projects") {
            if let Some(root) = parent.parent() {
                return root.to_path_buf();
            }
        }
    }
    sync_root.to_path_buf()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct LocalMutationRecord {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub transaction_id: String,
    /// 该记录所属同步根的绝对路径；仅用于崩溃恢复时定位账本。
    pub sync_root: String,
    /// 相对同步根的规范化路径。
    pub path: String,
    #[serde(default)]
    pub object_type: Option<String>,
    #[serde(default)]
    pub object_id: Option<String>,
    /// `upsert` 或 `delete`。
    pub op: String,
    #[serde(default)]
    pub base_hash: Option<String>,
    #[serde(default)]
    pub content_hash: String,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub deleted_at_ms: Option<i64>,
    pub device_id: String,
    #[serde(default)]
    pub object_revision: Option<u64>,
    /// 该事实在账本内的单调序号；推送成功后按序号确认并压缩历史前缀。
    #[serde(default)]
    pub seq: u64,
}

fn schema_version() -> u32 {
    1
}

impl LocalMutationRecord {
    pub(crate) fn is_upsert(&self) -> bool {
        self.op == "upsert"
    }

    pub(crate) fn is_delete(&self) -> bool {
        self.op == "delete"
    }
}

pub(crate) fn ledger_path(sync_root: &Path) -> PathBuf {
    sync_root.join(LOCAL_MUTATIONS_PATH)
}

/// 按行读取账本事实。
///
/// 账本现在是增量调度的唯一依据：损坏行不能再被静默跳过，否则那条业务事实
/// 会永久漏同步。损坏由 [`repair_corrupt_ledger`] 显式归档并重建基线，
/// 这里只负责如实报错（Issue #1065 复审 #4）。
pub(crate) fn load_records(
    sync_root: &Path,
) -> crate::error::Result<Vec<LocalMutationRecord>> {
    let path = ledger_path(sync_root);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut records = Vec::new();
    for (line_index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let record = serde_json::from_str::<LocalMutationRecord>(line).map_err(|error| {
            crate::Error::Other(format!(
                "local mutation ledger corrupt at line {}: {error}",
                line_index + 1
            ))
        })?;
        records.push(record);
    }
    Ok(records)
}

/// 旧版账本（没有序号的事实）迁移：按文件顺序补齐 1..n 并原子重写。
///
/// 序号是确认与压缩的唯一依据；seq==0 的事实无法被推送回执确认，会永久留在
/// 待同步集合里。迁移只在检测到 0 时发生一次。
fn ensure_sequences(
    sync_root: &Path,
    records: &mut [LocalMutationRecord],
) -> crate::error::Result<bool> {
    let unnumbered = records.iter().filter(|record| record.seq == 0).count();
    if unnumbered == 0 {
        return Ok(false);
    }
    // 旧账本补号必须从持久高水位往上发，不能从 1 重排：已有确认水位属于
    // `confirmed_mutation_seq`，重排会把新事实落进已确认区间被误判为已同步。
    let mut reserved = reserve_sequences(sync_root, unnumbered)?.into_iter();
    for record in records.iter_mut().filter(|record| record.seq == 0) {
        if let Some(seq) = reserved.next() {
            // 按文件出现顺序取升序序号，保证"后写入的更大"。
            record.seq = seq;
        }
    }
    let mut output = String::new();
    for record in records.iter() {
        output.push_str(&serde_json::to_string(record)?);
        output.push('\n');
    }
    crate::storage::atomic_write_string(&ledger_path(sync_root), &output)?;
    log::info!("[sync] local mutation ledger migrated to sequential seq");
    Ok(true)
}

/// 账本写出中断/被外部改写后的恢复：归档损坏文件，按空账本重建。
///
/// 账本只是调度索引，不是业务事实来源：归档后 target 会走一次完整核对
/// （见 `version_snapshot` 的 `ledger_corrupt` 不 eligible），由真实磁盘内容
/// 建立新基线。返回是否发生了重建。
pub(crate) fn repair_corrupt_ledger(sync_root: &Path) -> crate::error::Result<bool> {
    if load_records(sync_root).is_ok() {
        return Ok(false);
    }
    let path = ledger_path(sync_root);
    if path.exists() {
        let archived = path.with_extension(format!(
            "corrupt-{}.jsonl",
            chrono::Utc::now().timestamp()
        ));
        // 归档失败必须保留原文件并如实报错：删掉损坏账本会同时丢掉唯一能把
        // 重建基线关联到历史事实的线索。
        fs::rename(&path, &archived).map_err(|error| {
            crate::Error::Other(format!(
                "cannot archive corrupt local mutation ledger {}: {error}",
                path.display()
            ))
        })?;
        log::warn!(
            "[sync] local mutation ledger corrupt; archived to {} and rebuilt",
            archived.display()
        );
    }
    Ok(true)
}

/// 读取账本最新事实（按 path 取最后一条）。
///
/// 调用方包含 manifest 投影等必须能继续工作的路径：损坏行先归档重建再按空
/// 索引继续，具体 target 的"必须完整核对"由 `version_snapshot` 负责标记。
pub(crate) fn load_latest(
    sync_root: &Path,
) -> crate::error::Result<HashMap<String, LocalMutationRecord>> {
    let records = match load_records(sync_root) {
        Ok(records) => records,
        Err(error) => {
            log::warn!("[sync] local mutation ledger corrupt; rebuilding: {error}");
            repair_corrupt_ledger(sync_root)?;
            Vec::new()
        }
    };
    let mut latest = HashMap::new();
    for record in records {
        latest.insert(record.path.clone(), record);
    }
    Ok(latest)
}

pub(crate) fn append_batch(
    sync_root: &Path,
    records: &[LocalMutationRecord],
) -> crate::error::Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    // Append 与确认/压缩共用同一把工作区写锁：并发 MCP/编辑写入不能把刚确认
    // 的历史又写回去，序号分配也不能与压缩交错（复审 #2）。
    let _lock = ledger_write_lock(sync_root);
    // 账本损坏时先归档重建：写入路径不能因为历史损坏而拒绝记录新的业务事实。
    repair_corrupt_ledger(sync_root)?;
    let mut existing = load_records(sync_root)?;
    ensure_sequences(sync_root, &mut existing)?;
    let mut latest: HashMap<String, LocalMutationRecord> = HashMap::new();
    for record in existing {
        latest.insert(record.path.clone(), record);
    }
    // 同一对象重复写入同一内容不产生新的待同步事实。账本在发布确认后才压缩，
    // 因此这里只需和当前最后一条事实比较：op/内容哈希/对象版本都没变就不是
    // 业务变更，不追加记录，也不产生新的待同步代次。
    let mut fresh: Vec<LocalMutationRecord> = Vec::new();
    for record in records {
        let unchanged = latest.get(&record.path).is_some_and(|previous| {
            previous.op == record.op
                && previous.content_hash == record.content_hash
                && previous.object_revision == record.object_revision
        });
        if unchanged {
            continue;
        }
        let mut record = record.clone();
        record.seq = 0;
        latest.insert(record.path.clone(), record.clone());
        fresh.push(record);
    }
    if fresh.is_empty() {
        return Ok(());
    }
    // 先按去重后的真实条数预留序号并持久化高水位，再落盘事实。
    let mut reserved = reserve_sequences(sync_root, fresh.len())?.into_iter();
    for record in &mut fresh {
        record.seq = reserved.next().unwrap_or(0);
    }
    let path = ledger_path(sync_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    for record in fresh {
        writeln!(file, "{}", serde_json::to_string(&record)?)?;
    }
    file.sync_data()?;
    Ok(())
}

/// 从一次已 durable 提交投影出待同步事实。
///
/// 业务事务（章节提交、设置、星图、校对结果）只把"已落盘的文件"和"已确认的
/// 删除事实"交给这里，不引入第二套事务系统：内容哈希取落盘后的真实文件，
/// 删除必须由调用方显式声明。序号由 [`append_batch`] 统一分配。
pub(crate) fn records_for_committed_paths(
    sync_root: &Path,
    transaction_id: &str,
    device_id: &str,
    upserted: &[String],
    deleted: &[String],
    now_ms: i64,
) -> Vec<LocalMutationRecord> {
    let sync_root_text = sync_root.to_string_lossy().into_owned();
    let mut records = Vec::new();
    for rel_path in upserted {
        let path = sync_root.join(rel_path);
        let Ok(content) = fs::read(&path) else {
            log::warn!(
                "[sync] committed object missing after commit; skip mutation: {}",
                path.display()
            );
            continue;
        };
        let (object_type, object_id) = object_identity(rel_path);
        records.push(LocalMutationRecord {
            schema_version: 1,
            transaction_id: transaction_id.to_string(),
            sync_root: sync_root_text.clone(),
            path: rel_path.clone(),
            object_type,
            object_id,
            op: "upsert".to_string(),
            base_hash: None,
            content_hash: crate::sync::hash::content_md5(&content),
            updated_at_ms: now_ms,
            deleted_at_ms: None,
            device_id: device_id.to_string(),
            object_revision: object_revision(&path),
            seq: 0,
        });
    }
    for rel_path in deleted {
        let (object_type, object_id) = object_identity(rel_path);
        records.push(LocalMutationRecord {
            schema_version: 1,
            transaction_id: transaction_id.to_string(),
            sync_root: sync_root_text.clone(),
            path: rel_path.clone(),
            object_type,
            object_id,
            op: "delete".to_string(),
            base_hash: None,
            content_hash: String::new(),
            updated_at_ms: now_ms,
            deleted_at_ms: Some(now_ms),
            device_id: device_id.to_string(),
            object_revision: None,
            seq: 0,
        });
    }
    records
}

/// 当前同步根下尚未被确认的本地业务变更事实（按 path 取最后一条）。
///
/// 这是同步调度的持久化脏对象索引：调度只读这份索引，不再为"有没有本地修改"
/// 重新扫描磁盘。外部工具绕过 Core 事务直接改文件不会出现在这里，由显式校验/
/// 导入事务建立真实基线（见 `version_snapshot`）。
pub(crate) fn pending_records(
    sync_root: &Path,
    scope: SyncScope,
    app_group: Option<&AppDataGroup>,
) -> crate::error::Result<BTreeMap<String, LocalMutationRecord>> {
    let ledger = match latest_ledger(sync_root) {
        Ok(ledger) => ledger,
        Err(error) => {
            // 写入/提交路径（账本结算与 defer 判定）需要可用索引：损坏时在这里
            // 归档重建一次。只读查询（版本快照/待上传判定）仍保持严格报错。
            log::warn!(
                "[sync] local mutation ledger corrupt ({}), rebuilding for commit path",
                error
            );
            repair_corrupt_ledger(sync_root)?;
            latest_ledger(sync_root)?
        }
    };
    pending_records_in(&ledger, sync_root, scope, app_group)
}

/// 读取并修复账本，返回"路径 → 最新事实"的索引（已补序号）。
///
/// App 数据组共享同一份账本：一轮调度里只读一次，再按 group 过滤，
/// 不能在每个 group 上重复加载同一个 JSONL（复审 #5）。
pub(crate) fn latest_ledger(
    sync_root: &Path,
) -> crate::error::Result<HashMap<String, LocalMutationRecord>> {
    // 读取查询不修复、不重写账本：损坏时如实返回错误，由调用方把该 target
    // 判为 ledger_corrupt 并回到完整核对；归档重建只发生在真正的写入路径
    // （append / 确认）里（复审 #2）。
    let mut records = load_records(sync_root)?;
    ensure_sequences(sync_root, &mut records)?;
    let mut latest: HashMap<String, LocalMutationRecord> = HashMap::new();
    for record in records {
        latest.insert(record.path.clone(), record);
    }
    Ok(latest)
}

/// 从一个已读入的账本索引里挑出某个 target 的待同步事实。
pub(crate) fn pending_records_in(
    ledger: &HashMap<String, LocalMutationRecord>,
    sync_root: &Path,
    scope: SyncScope,
    app_group: Option<&AppDataGroup>,
) -> crate::error::Result<BTreeMap<String, LocalMutationRecord>> {
    // 账本已在 read 阶段修复（latest_ledger），这里不再重复完整读一遍：
    // App 组可能很多，逐组重读整份 JSONL 会把增量调度退化成全量 IO。
    // 已确认随远端提交发布的事实不算"待同步"。确认只看推送回执写入的序号，
    // 不用 known_files 是否包含该路径推断删除（复审 #3）。
    let confirmed_seq = match app_group {
        Some(group) => {
            crate::sync::SyncService::load_sync_state_read_only_for_group(sync_root, group, None)
                .map(|state| state.confirmed_mutation_seq)
                .unwrap_or_default()
        }
        None => crate::sync::SyncService::load_sync_state_read_only(sync_root, None)
            .map(|state| state.confirmed_mutation_seq)
            .unwrap_or_default(),
    };
    let mut pending = BTreeMap::new();
    for (path, record) in ledger {
        if !is_sync_business_path(path, scope) {
            continue;
        }
        if let Some(group) = app_group {
            if !group.owns_path(path) {
                continue;
            }
        }
        // seq == 0 是旧版本账本（尚未分配序号）写入的事实，永远不能被判为已确认。
        if record.seq != 0 && record.seq <= confirmed_seq.get(path).copied().unwrap_or(0) {
            continue;
        }
        pending.insert(path.clone(), record.clone());
    }
    Ok(pending)
}

/// 待同步对象的稳定代次：路径 + op + 内容哈希的 SHA-256。
///
/// 平台把它当作"还有没有未确认修改"的轻量判据，不需要理解对象内容；
/// 同一批待同步事实在任意一轮调度里得到同一个代次。
pub(crate) fn pending_digest(records: &BTreeMap<String, LocalMutationRecord>) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for (path, record) in records {
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(record.op.as_bytes());
        hasher.update(b"\0");
        hasher.update(record.content_hash.as_bytes());
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
}

/// 文件路径是否属于账本自身；扫描/事务捕获都必须忽略它。
pub(crate) fn is_ledger_path(rel_path: &str) -> bool {
    rel_path.replace('\\', "/") == LOCAL_MUTATIONS_PATH
}

/// 推送回执里的一条已发布对象事实。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PublishedMutation {
    pub path: String,
    /// `upsert` / `delete` / `superseded`。
    ///
    /// `superseded` 表示这条本地事实已被远端胜出覆盖（合并决议），不再作为
    /// 待上传事实回推；只推进确认序号，不改动 known_files / 墓碑。
    pub op: String,
    pub content_hash: String,
    /// 构建发布批次时读取的那条账本事实的序号。
    pub seq: u64,
    /// 业务修改时间（取自账本事实，不用确认时刻冒充）。
    pub updated_at_ms: i64,
    /// 删除发生时间（仅 delete 事实有值）。
    pub deleted_at_ms: Option<i64>,
}

/// 待重放的确认意图（推送已成功但本地状态还没落盘时崩溃/失败）。
const PENDING_ACK_PATH: &str = "app-meta/sync/pending-acks.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PendingAcks {
    #[serde(default)]
    entries: Vec<PendingAckEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingAckEntry {
    /// `app` 或 `project`。
    target_kind: String,
    /// App 数据组 id 或作品 id。
    target_id: String,
    published: Vec<PublishedMutation>,
}

fn pending_ack_path(sync_root: &Path) -> PathBuf {
    sync_root.join(PENDING_ACK_PATH)
}

fn stage_pending_ack(
    sync_root: &Path,
    target: &crate::sync::types::SyncTarget,
    published: &[PublishedMutation],
) -> crate::error::Result<()> {
    let (kind, id) = pending_ack_identity(target);
    let mut acks = load_pending_acks(sync_root);
    acks.entries.retain(|entry| {
        !(entry.target_kind == kind && entry.target_id == id)
    });
    acks.entries.push(PendingAckEntry {
        target_kind: kind,
        target_id: id,
        published: published.to_vec(),
    });
    let text = serde_json::to_string(&acks)?;
    crate::storage::atomic_write_string(&pending_ack_path(sync_root), &text)
}

fn load_pending_acks(sync_root: &Path) -> PendingAcks {
    match fs::read_to_string(pending_ack_path(sync_root)) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => PendingAcks::default(),
    }
}

/// 重放"已推送但未结算"的确认意图。
///
/// 推送成功与本地状态落盘之间崩溃时，账本事实必须保留并且下一轮能重新结算，
/// 不能只 warn 一句就默认成功（复审 #3）。
pub(crate) fn replay_pending_acks(sync_root: &Path) -> crate::error::Result<()> {
    let mut acks = load_pending_acks(sync_root);
    if acks.entries.is_empty() {
        return Ok(());
    }
    let _lock = ledger_write_lock(sync_root);
    let mut remaining: Vec<PendingAckEntry> = Vec::new();
    for entry in std::mem::take(&mut acks.entries) {
        let target = if entry.target_kind == "app" {
            match crate::sync::types::AppDataGroup::from_id(&entry.target_id) {
                Ok(group) => crate::sync::types::SyncTarget::app_data_group(group),
                Err(_) => continue,
            }
        } else {
            crate::sync::types::SyncTarget::project(&entry.target_id)
        };
        match apply_published(sync_root, &target, &entry.published) {
            Ok(()) => log::info!(
                "[sync] replayed pending mutation ack for {}:{} ({} facts)",
                entry.target_kind,
                entry.target_id,
                entry.published.len()
            ),
            Err(error) => {
                log::warn!(
                    "[sync] pending mutation ack replay failed for {}:{}: {error}",
                    entry.target_kind,
                    entry.target_id
                );
                remaining.push(entry);
            }
        }
    }
    acks.entries = remaining;
    let path = pending_ack_path(sync_root);
    if acks.entries.is_empty() {
        let _ = fs::remove_file(path);
        return Ok(());
    }
    let text = serde_json::to_string(&acks)?;
    crate::storage::atomic_write_string(&path, &text)
}

/// 远端提交成功后按回执确认 mutation：推进 known_files、记录确认序号、压缩历史前缀。
///
/// `published` 必须来自本轮真正成功的远端提交；确认只覆盖 seq 及其之前的历史，
/// 同步期间新写入的并发事实（更大的 seq）保留为下一轮的 outbox。
pub(crate) fn mark_confirmed(
    sync_root: &Path,
    target: &crate::sync::types::SyncTarget,
    published: &[PublishedMutation],
) -> crate::error::Result<()> {
    if published.is_empty() {
        return Ok(());
    }
    // 先持久化确认意图：推送已经成功，本地结算失败时下一轮必须能重放，
    // 不能只 warn 就当作已确认（复审 #3）。
    stage_pending_ack(sync_root, target, published)?;
    // 确认属于写事务：与并发 append 互斥，避免"读到旧账本→写回压缩结果"覆盖
    // 同步期间新产生的业务事实（复审 #2）。
    let _lock = ledger_write_lock(sync_root);
    apply_published(sync_root, target, published)?;
    clear_pending_ack(sync_root, target)
}

fn pending_ack_identity(target: &crate::sync::types::SyncTarget) -> (String, String) {
    match target.app_group.as_ref() {
        Some(group) => ("app".to_string(), group.id()),
        None => (
            "project".to_string(),
            target.project_id.clone().unwrap_or_default(),
        ),
    }
}

fn clear_pending_ack(
    sync_root: &Path,
    target: &crate::sync::types::SyncTarget,
) -> crate::error::Result<()> {
    let (kind, id) = pending_ack_identity(target);
    let mut acks = load_pending_acks(sync_root);
    let before = acks.entries.len();
    acks.entries
        .retain(|entry| !(entry.target_kind == kind && entry.target_id == id));
    if acks.entries.len() == before {
        return Ok(());
    }
    let path = pending_ack_path(sync_root);
    if acks.entries.is_empty() {
        let _ = fs::remove_file(path);
        return Ok(());
    }
    let text = serde_json::to_string(&acks)?;
    crate::storage::atomic_write_string(&path, &text)
}

fn apply_published(
    sync_root: &Path,
    target: &crate::sync::types::SyncTarget,
    published: &[PublishedMutation],
) -> crate::error::Result<()> {
    let mut state = crate::sync::SyncService::load_sync_state_for_target(sync_root, target)?;
    for entry in published {
        match entry.op.as_str() {
            "delete" => {
                state.known_files.remove(&entry.path);
                state.known_files_updated_at.remove(&entry.path);
                // 只清掉本次发布对应的墓碑；同步期间新产生的删除（更晚时间）
                // 必须保留给下一轮（复审 #3）。
                let published_at = entry.deleted_at_ms.map(|ms| ms / 1000);
                state.tombstones.retain(|tombstone| {
                    if tombstone.original_path != entry.path {
                        return true;
                    }
                    match published_at {
                        Some(published_at) => tombstone.deleted_at > published_at,
                        // 没有删除时间就无法证明这条墓碑更晚：保守保留。
                        None => true,
                    }
                });
            }
            "superseded" => {}
            _ => {
                state
                    .known_files
                    .insert(entry.path.clone(), entry.content_hash.clone());
                // 记录业务修改时间，不用确认时刻冒充（否则三路 LWW 会拿
                // "现在"去赢远端更新的内容）。
                state
                    .known_files_updated_at
                    .insert(entry.path.clone(), entry.updated_at_ms);
            }
        }
        let current = state
            .confirmed_mutation_seq
            .get(&entry.path)
            .copied()
            .unwrap_or(0);
        if entry.seq > current {
            state.confirmed_mutation_seq.insert(entry.path.clone(), entry.seq);
        }
    }
    crate::sync::SyncService::save_sync_state_for_target(sync_root, target, &state)?;
    compact_confirmed_prefix(sync_root, published)
}

/// 按已确认序号压缩账本历史前缀。
///
/// 只逐条删除"hash 相同"的行会让同一路径上更早的旧事实复活（复审 #3）：
/// 确认是单调的序号事实，因此删掉该路径上 seq <= 已确认序号的整段历史。
fn compact_confirmed_prefix(
    sync_root: &Path,
    published: &[PublishedMutation],
) -> crate::error::Result<()> {
    let confirmed: HashMap<&str, u64> = published
        .iter()
        .map(|entry| (entry.path.as_str(), entry.seq))
        .collect();
    let records = load_records(sync_root)?;
    let mut kept: Vec<LocalMutationRecord> = Vec::with_capacity(records.len());
    let mut changed = false;
    for record in records {
        let confirmed_seq = confirmed.get(record.path.as_str()).copied().unwrap_or(0);
        if record.seq != 0 && record.seq <= confirmed_seq {
            changed = true;
            continue;
        }
        kept.push(record);
    }
    if !changed {
        return Ok(());
    }
    let mut output = String::new();
    for record in &kept {
        output.push_str(&serde_json::to_string(record)?);
        output.push('\n');
    }
    crate::storage::atomic_write_string(&ledger_path(sync_root), &output)
}

/// 从同步对象相对路径解析稳定对象身份。
pub(crate) fn object_identity(rel_path: &str) -> (Option<String>, Option<String>) {
    let path = rel_path.replace('\\', "/");
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() == 2 && parts[0] == "starmaps" {
        if let Some(starmap_id) = parts[1].strip_suffix(".meta.json") {
            return (Some("starmap".to_string()), Some(starmap_id.to_string()));
        }
    }
    if parts.len() == 3 && parts[0] == "starmaps" && parts[2] == "graph.json" {
        return (Some("starmap".to_string()), Some(parts[1].to_string()));
    }
    if parts.len() >= 3 && parts[0] == "starmaps" {
        if parts.len() == 3 && parts[1] == "library" && parts[2] == "index.json" {
            return (None, None);
        }
        if parts[1] == "library" && parts.len() >= 4 && parts[2] == "nodes" {
            if let Some(id) = parts.last().and_then(|name| name.strip_suffix(".json")) {
                return (Some("starmapLibraryNode".to_string()), Some(id.to_string()));
            }
        }
        if parts.len() >= 5 {
            // Issue #1060：链接唯一所有者在 annotations 仓库，
            // `starmaps/<id>/links/*.json` 不再产生，只保留 node/edge/embed。
            let object_type = match parts[2] {
                "nodes" => Some("starmapNode"),
                "edges" => Some("starmapEdge"),
                "embeds" => Some("starmapEmbed"),
                _ => None,
            };
            if let Some(object_type) = object_type {
                if let Some(id) = parts.last().and_then(|name| name.strip_suffix(".json")) {
                    return (Some(object_type.to_string()), Some(id.to_string()));
                }
            }
        }
    }
    if parts.len() == 3 && parts[0] == "annotations" {
        let object_type = match parts[1] {
            "tags" => Some("tag"),
            "bindings" => Some("tagBinding"),
            "entries" => Some("namedEntry"),
            "anchors" => Some("textAnchor"),
            "links" => Some("objectLink"),
            _ => None,
        };
        if let Some(object_type) = object_type {
            if let Some(id) = parts[2].strip_suffix(".json") {
                return (Some(object_type.to_string()), Some(id.to_string()));
            }
        }
    }
    (None, None)
}

/// 读取对象 JSON 的修改版本（优先 `revision`，其次 `updatedAt`/`updated_at`）。
pub(crate) fn object_revision(path: &Path) -> Option<u64> {
    let content = fs::read(path).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&content).ok()?;
    value
        .get("revision")
        .and_then(serde_json::Value::as_u64)
        .or_else(|| value.get("updatedAt").and_then(serde_json::Value::as_u64))
        .or_else(|| value.get("updated_at").and_then(serde_json::Value::as_u64))
}

/// 该相对路径是否属于同步业务对象（排除同步自身状态与账本）。
pub(crate) fn is_sync_business_path(rel_path: &str, scope: SyncScope) -> bool {
    if is_ledger_path(rel_path) {
        return false;
    }
    SyncService::is_whitelisted_path(rel_path, scope)
        && !SyncService::is_blacklisted_path(rel_path, scope)
}
