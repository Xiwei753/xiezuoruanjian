//! 持久 workspace dirty/recovery 索引（Issue #1065 复审 #7214314073 第 3/4 项）。
//!
//! 同步空闲判据不能再靠“分配高水位相等”、目录 mtime、对象计数或整份
//! manifest 指纹去猜收敛。这里登记真实待确认对象版本与恢复责任，作为现有
//! durable mutation 账本的可重建投影：
//!
//! - 写入点在追加账本后登记 `<sync_root, path>` 的最新事实；
//! - 确认点按真实发布回执的 `seq` 精确移除；
//! - 恢复标记（账本损坏、待清理远端对象等）单独登记，恢复完成才清除；
//! - 读取端用 `ledger_bytes` 校验索引与该 root 的账本是否同一快照：任何
//!   未登记追加都会表现为长度不一致，从而强制回到慢路径，而不是误报空闲。
//!
//! 索引损坏、缺项、root 未知（例如新作品尚未进入 checkpoint）都按“脏”处理。
//! 索引永远不是事实来源，只是让空闲查询不必枚举/扫描全部工作区。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// 索引文件相对 app data 根的位置。
pub(crate) const DIRTY_INDEX_PATH: &str = "app-meta/sync/dirty-index.json";

const SCHEMA_VERSION: u32 = 1;

/// 单个对象的未确认事实（与账本记录同源）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DirtyFact {
    #[serde(default)]
    pub seq: u64,
    #[serde(default)]
    pub op: String,
    #[serde(default)]
    pub content_hash: String,
    #[serde(default)]
    pub updated_at_ms: i64,
    #[serde(default)]
    pub deleted_at_ms: Option<i64>,
}

/// 单个同步根的待办/恢复责任。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DirtyTargetEntry {
    /// 登记时该 root 账本的字节长度，用于识别未登记的追加/压缩。
    #[serde(default)]
    pub ledger_bytes: u64,
    /// 存在未完成的恢复责任（账本损坏、待清理远端对象等）。
    #[serde(default)]
    pub recovery: bool,
    /// 未确认对象版本（path → 最新事实）。
    #[serde(default)]
    pub pending: BTreeMap<String, DirtyFact>,
}

/// 整个 workspace 的 dirty 索引。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct WorkspaceDirtyIndex {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub targets: BTreeMap<String, DirtyTargetEntry>,
}

impl Default for WorkspaceDirtyIndex {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            targets: BTreeMap::new(),
        }
    }
}

fn index_path(app_data_root: &Path) -> PathBuf {
    app_data_root.join(DIRTY_INDEX_PATH)
}

/// 同步根作为索引键：优先 canonical，失败时用原路径。
pub(crate) fn root_key(sync_root: &Path) -> String {
    std::fs::canonicalize(sync_root)
        .unwrap_or_else(|_| sync_root.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

fn ledger_bytes(sync_root: &Path) -> u64 {
    std::fs::metadata(sync_root.join(crate::sync::local_mutations::LOCAL_MUTATIONS_PATH))
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

pub(crate) fn load(app_data_root: &Path) -> Option<WorkspaceDirtyIndex> {
    let text = std::fs::read_to_string(index_path(app_data_root)).ok()?;
    serde_json::from_str(&text).ok()
}

fn save(app_data_root: &Path, index: &WorkspaceDirtyIndex) -> crate::error::Result<()> {
    let path = index_path(app_data_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::storage::atomic_write_string(&path, &serde_json::to_string(index)?)
}

/// 账本追加后登记真实事实。调用方须持有 workspace 写锁。
pub(crate) fn record_appended(
    app_data_root: &Path,
    sync_root: &Path,
    records: &[crate::sync::local_mutations::LocalMutationRecord],
) -> crate::error::Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    let mut index = load(app_data_root).unwrap_or_default();
    index.schema_version = SCHEMA_VERSION;
    let entry = index.targets.entry(root_key(sync_root)).or_default();
    for record in records {
        entry.pending.insert(
            record.path.clone(),
            DirtyFact {
                seq: record.seq,
                op: record.op.clone(),
                content_hash: record.content_hash.clone(),
                updated_at_ms: record.updated_at_ms,
                deleted_at_ms: record.deleted_at_ms,
            },
        );
    }
    entry.ledger_bytes = ledger_bytes(sync_root);
    save(app_data_root, &index)
}

/// 真实回执确认后按 `seq` 精确移除（同 path 更高 seq 保留）。
pub(crate) fn record_confirmed(
    app_data_root: &Path,
    sync_root: &Path,
    published: &[crate::sync::local_mutations::PublishedMutation],
) -> crate::error::Result<()> {
    let mut index = load(app_data_root).unwrap_or_default();
    index.schema_version = SCHEMA_VERSION;
    let entry = index.targets.entry(root_key(sync_root)).or_default();
    for mutation in published {
        let remove = entry
            .pending
            .get(&mutation.path)
            .is_some_and(|fact| fact.seq == 0 || fact.seq <= mutation.seq);
        if remove {
            entry.pending.remove(&mutation.path);
        }
    }
    entry.ledger_bytes = ledger_bytes(sync_root);
    save(app_data_root, &index)
}

/// 登记/清除恢复责任。清除只在调用方确认恢复真实完成后调用。
pub(crate) fn set_recovery(
    app_data_root: &Path,
    sync_root: &Path,
    recovery: bool,
) -> crate::error::Result<()> {
    let mut index = load(app_data_root).unwrap_or_default();
    index.schema_version = SCHEMA_VERSION;
    let entry = index.targets.entry(root_key(sync_root)).or_default();
    entry.recovery = recovery;
    entry.ledger_bytes = ledger_bytes(sync_root);
    save(app_data_root, &index)
}

/// 索引与该轮 checkpoint 完全一致且没有待办/恢复责任时，才允许走空闲快路径。
///
/// `expected` 是 checkpoint 里的 target key 与其同步根的对应关系。
/// 未知但当前仍存在的同步根（例如新建作品尚未进入 checkpoint）一律判脏，
/// 交给完整路径枚举；已不存在的历史 root 视为过期记录忽略。
pub(crate) fn is_clean_for_checkpoint(
    index: &WorkspaceDirtyIndex,
    expected: &[(String, PathBuf)],
) -> bool {
    if index.schema_version != SCHEMA_VERSION {
        return false;
    }
    let mut known_roots: BTreeSet<String> = BTreeSet::new();
    for (_, sync_root) in expected {
        let key = root_key(sync_root);
        known_roots.insert(key.clone());
        let Some(entry) = index.targets.get(&key) else {
            return false;
        };
        if entry.recovery
            || !entry.pending.is_empty()
            || entry.ledger_bytes != ledger_bytes(sync_root)
        {
            return false;
        }
    }
    // 未知 root：目录还在就必须走完整路径（可能是新建作品/新数据组）。
    for key in index.targets.keys() {
        if known_roots.contains(key) {
            continue;
        }
        if Path::new(key).exists() {
            return false;
        }
    }
    true
}
