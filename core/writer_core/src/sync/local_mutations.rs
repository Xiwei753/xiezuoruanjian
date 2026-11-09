//! 本地对象变更事实账本。
//!
//! 成功的 Core 写事务在对象真正落盘后，把本次写过的同步对象记录到
//! `<sync_root>/app-meta/sync/local-mutations.jsonl`。这份账本是本机事实，
//! 不参与同步：同步清单投影用它取得真实的提交时间、设备身份和对象身份，
//! 不再依赖文件 mtime 猜测“这次修改是谁、什么时候写的”。
//!
//! 账本按 `path` 取最后一条记录：同一对象连续编辑在同步版本上聚合为最终
//! 内容，但每次事务的贡献事件仍由统计事件流独立保存。

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sync::types::SyncScope;
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
    let path = ledger_path(sync_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    for record in records {
        writeln!(file, "{}", serde_json::to_string(record)?)?;
    }
    file.sync_data()?;
    Ok(())
}

/// 文件路径是否属于账本自身；扫描/事务捕获都必须忽略它。
pub(crate) fn is_ledger_path(rel_path: &str) -> bool {
    rel_path.replace('\\', "/") == LOCAL_MUTATIONS_PATH
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
            let object_type = match parts[2] {
                "nodes" => Some("starmapNode"),
                "edges" => Some("starmapEdge"),
                "embeds" => Some("starmapEmbed"),
                "links" => Some("starmapLink"),
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
