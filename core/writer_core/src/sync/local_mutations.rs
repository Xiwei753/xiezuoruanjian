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

/// 读取账本最新事实。损坏行只跳过，不阻断同步；账本本身不是业务正文。
pub(crate) fn load_latest(
    sync_root: &Path,
) -> crate::error::Result<HashMap<String, LocalMutationRecord>> {
    let path = ledger_path(sync_root);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error.into()),
    };
    let mut records = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<LocalMutationRecord>(line) {
            Ok(record) => {
                records.insert(record.path.clone(), record);
            }
            Err(error) => {
                log::warn!("[sync] skip malformed local mutation fact: {error}");
            }
        }
    }
    Ok(records)
}

pub(crate) fn append_batch(
    sync_root: &Path,
    records: &[LocalMutationRecord],
) -> crate::error::Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    // 同一对象重复写入同一内容不产生新的待同步事实。账本在成功发布后才压缩，
    // 因此这里只需和当前最后一条事实比较：op/内容哈希/对象版本都没变就不是
    // 业务变更，不追加记录，也不产生新的待同步代次。
    let latest = load_latest(sync_root)?;
    let fresh: Vec<&LocalMutationRecord> = records
        .iter()
        .filter(|record| match latest.get(&record.path) {
            Some(previous) => {
                previous.op != record.op
                    || previous.content_hash != record.content_hash
                    || previous.object_revision != record.object_revision
            }
            None => true,
        })
        .collect();
    if fresh.is_empty() {
        return Ok(());
    }
    let path = ledger_path(sync_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    for record in fresh {
        writeln!(file, "{}", serde_json::to_string(record)?)?;
    }
    file.sync_data()?;
    Ok(())
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
    // 已被共同基线确认的事实不算"待同步"：账本按组压缩，未压缩的旧事实
    // 不能把 target 永久标脏（否则每次空闲调度都会重跑该 target 的合并）。
    let known_files = match app_group {
        Some(group) => {
            crate::sync::SyncService::load_sync_state_read_only_for_group(sync_root, group, None)
                .map(|state| state.known_files)
                .unwrap_or_default()
        }
        None => crate::sync::SyncService::load_sync_state_read_only(sync_root, None)
            .map(|state| state.known_files)
            .unwrap_or_default(),
    };
    let mut pending = BTreeMap::new();
    for (path, record) in load_latest(sync_root)? {
        if !is_sync_business_path(&path, scope) {
            continue;
        }
        if let Some(group) = app_group {
            if !group.owns_path(&path) {
                continue;
            }
        }
        let confirmed = if record.is_upsert() {
            known_files.get(&path) == Some(&record.content_hash)
        } else if record.is_delete() {
            !known_files.contains_key(&path)
        } else {
            false
        };
        if confirmed {
            continue;
        }
        pending.insert(path, record);
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

/// 同步成功提交后，删除已被共同基线确认的账本事实。
///
/// 只压缩指定分组：其他分组的事实仍可能尚未发布。upsert 只有在组级
/// `known_files` 已等于该事实的 content_hash 时才算确认；delete 只有在该路径
/// 已从组级 known_files 消失时才算确认。
pub(crate) fn compact_confirmed(
    sync_root: &Path,
    group: &AppDataGroup,
    known_files: &HashMap<String, String>,
) -> crate::error::Result<()> {
    let path = ledger_path(sync_root);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let mut kept = Vec::new();
    let mut changed = false;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<LocalMutationRecord>(line) {
            Ok(record) => {
                let same_group = crate::sync::config_store::app_data_group_for_path(&record.path)
                    .as_ref()
                    == Some(group);
                let confirmed = same_group
                    && if record.is_upsert() {
                        known_files.get(&record.path) == Some(&record.content_hash)
                    } else if record.is_delete() {
                        !known_files.contains_key(&record.path)
                    } else {
                        false
                    };
                if confirmed {
                    changed = true;
                } else {
                    kept.push(line.to_string());
                }
            }
            Err(_) => kept.push(line.to_string()),
        }
    }
    if !changed {
        return Ok(());
    }
    let mut output = kept.join("\n");
    if !output.is_empty() {
        output.push('\n');
    }
    crate::storage::atomic_write_string(&path, &output)?;
    Ok(())
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
