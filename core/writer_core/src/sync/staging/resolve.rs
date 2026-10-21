use std::path::Path;

use crate::error::Result;

///   从文件系统构造本地侧 LWW 记录。
///
/// 读取文件 mtime 作为 `updated_at_ms`；`op` 按内容是否存在决定（Some → upsert，
/// None → delete）。`device_id` 用 live 的 device_id。
///
/// delete 时从 `tombstones` 查找 `deleted_at`，
/// 不再固定写 0。若 tombstones 中无记录，返回 `None`（调用方应报错或补 tombstone）。
pub(crate) fn build_local_lww_record(
    root: &Path,
    rel: &Path,
    content: &Option<Vec<u8>>,
    device_id: &str,
    tombstones: &[crate::sync::types::Tombstone],
) -> Option<crate::sync::content_class::LwwRecord> {
    build_local_lww_record_with_manifest(root, rel, content, device_id, tombstones, None)
}

/// 基于持久 manifest 事实构造本地 LWW 记录。
///
/// 已存在 manifest 记录且当前内容哈希未变时，直接复用 manifest 的
/// `updated_at_ms` / `deleted_at_ms` / `device_id`；只有真正发生本地变化
/// （新文件或内容变化）才用文件 mtime 建立新记录。文件缺失只有在存在真实
/// tombstone 时才产生 delete 记录；否则返回 `None`，调用方不得伪造删除。
pub(crate) fn build_local_lww_record_with_manifest(
    root: &Path,
    rel: &Path,
    content: &Option<Vec<u8>>,
    device_id: &str,
    tombstones: &[crate::sync::types::Tombstone],
    manifest: Option<&std::collections::HashMap<String, ManifestRecord>>,
) -> Option<crate::sync::content_class::LwwRecord> {
    use crate::sync::content_class::LwwRecord;

    let rel_str = rel.to_string_lossy().to_string();

    let (content_hash, op, updated_at_ms, deleted_at_ms) = match content {
        Some(bytes) => {
            let hash = md5_hex(&Some(bytes.clone()));
            if let Some(record) = manifest.and_then(|records| records.get(&rel_str)) {
                if record.op == "upsert" && record.content_hash == hash {
                    return Some(record.to_lww_record());
                }
            }
            let mtime = read_mtime_ms(root, rel).unwrap_or(0);
            (hash, "upsert", mtime, None)
        }
        None => {
            // 从 tombstones 查找删除时间。
            // 没有 tombstone 记录 → 无法确定删除时间，返回 None。
            let ts = tombstones.iter().find(|t| t.original_path == rel_str)?;
            let deleted_at = ts.deleted_at * 1000; // tombstone.deleted_at 是秒，LWW 用毫秒
            (String::new(), "delete", deleted_at, Some(deleted_at))
        }
    };

    Some(LwwRecord {
        content_hash,
        updated_at_ms,
        deleted_at_ms,
        device_id: device_id.to_string(),
        op: op.to_string(),
    })
}

/// staging manifest 中的文件记录（JSON 反序列化用）。
///
/// 与 `types::ManifestFileRecord` 字段一致，但不依赖 `github-api` feature gate。
/// 仅用于从 staging 的 `manifest.sync.json` 读取远端 LWW 元数据。
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct ManifestRecord {
    pub(crate) path: String,
    #[serde(rename = "content_hash")]
    pub(crate) content_hash: String,
    pub(crate) updated_at_ms: i64,
    #[serde(default)]
    pub(crate) deleted_at_ms: Option<i64>,
    pub(crate) device_id: String,
    pub(crate) op: String,
}

impl ManifestRecord {
    fn to_lww_record(&self) -> crate::sync::content_class::LwwRecord {
        crate::sync::content_class::LwwRecord {
            content_hash: self.content_hash.clone(),
            updated_at_ms: self.updated_at_ms,
            deleted_at_ms: self.deleted_at_ms,
            device_id: self.device_id.clone(),
            op: self.op.clone(),
        }
    }
}

/// 从 staging 的 manifest.sync.json 读取远端 LWW 记录。
///
/// 改为 `Result<Option<...>>`。
/// - manifest 不存在 → `Ok(None)`（Git backend 不产生 manifest，mtime fallback）。
/// - manifest 存在且解析成功 → `Ok(Some(map))`。
/// - manifest 存在但读/解析失败 → `Err`（GithubApi backend 的 manifest 是事实来源，
///   解析失败不能静默切成另一套决策规则）。
///
/// 调用方根据 `is_github_api` 决定是否允许 `None`（mtime fallback）。
pub(crate) fn read_staging_manifest(
    staging_root: &Path,
) -> Result<Option<std::collections::HashMap<String, ManifestRecord>>> {
    read_manifest_at(&staging_root.join("app-meta/sync/manifest.sync.json"))
}

/// 读取 live 的持久 manifest，作为本轮比较的本地版本事实。
///
/// 与 staging manifest 使用同一套解析规则；不存在返回 `Ok(None)`，
/// 解析失败返回 `Err`，不能静默退回 mtime。
pub(crate) fn read_live_manifest(
    live_root: &Path,
) -> Result<Option<std::collections::HashMap<String, ManifestRecord>>> {
    read_manifest_at(&live_root.join("app-meta/sync/manifest.sync.json"))
}

/// 读取 live 中某个 App group 的持久 manifest，缺少分组 manifest 时
/// 只读回退到聚合 manifest 并过滤出该组路径。
pub(crate) fn read_live_manifest_for_group(
    live_root: &Path,
    group: &crate::sync::types::AppDataGroup,
) -> Result<Option<std::collections::HashMap<String, ManifestRecord>>> {
    read_manifest_for_group(live_root, group)
}

/// Read this App group's manifest, falling back to the aggregate manifest as a
/// read-only migration source. Records outside the group are discarded.
pub(crate) fn read_staging_manifest_for_group(
    staging_root: &Path,
    group: &crate::sync::types::AppDataGroup,
) -> Result<Option<std::collections::HashMap<String, ManifestRecord>>> {
    read_manifest_for_group(staging_root, group)
}

fn read_manifest_for_group(
    root: &Path,
    group: &crate::sync::types::AppDataGroup,
) -> Result<Option<std::collections::HashMap<String, ManifestRecord>>> {
    let grouped_path = root.join(group.local_manifest_path());
    let manifest_path = if grouped_path.exists() {
        grouped_path
    } else {
        root.join("app-meta/sync/manifest.sync.json")
    };
    let Some(mut records) = read_manifest_at(&manifest_path)? else {
        return Ok(None);
    };
    records.retain(|path, _| group.owns_path(path));
    Ok(Some(records))
}

fn read_manifest_at(
    manifest_path: &Path,
) -> Result<Option<std::collections::HashMap<String, ManifestRecord>>> {
    if !manifest_path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&manifest_path).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "manifest_read_failed: path={} reason=failed to read manifest: {}",
            manifest_path.display(),
            e
        )))
    })?;
    #[derive(serde::Deserialize)]
    struct ManifestContainer {
        files: Vec<ManifestRecord>,
    }
    let container: ManifestContainer = serde_json::from_str(&content).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "manifest_read_failed: path={} reason=manifest parse error: {}",
            manifest_path.display(),
            e
        )))
    })?;
    Ok(Some(
        container
            .files
            .into_iter()
            .map(|r| (r.path.clone(), r))
            .collect(),
    ))
}

/// 从 manifest 记录构造远端侧 LWW 记录。
///
/// 只使用持久 manifest 中的 `content_hash`、`updated_at_ms`、`deleted_at_ms`、
/// `device_id`、`op`，不根据本轮内容重算哈希或扫描时间。
///
/// 返回 `None` 表示 manifest 没有可用的远端事实：
/// - `op == "upsert"` 但本轮没有 incoming 内容（内容缺失，不能当作删除）；
/// - 未知 op。
/// 调用方遇到 `None` 时必须保留本地/报告缺失，不能伪造一个远端删除记录。
pub(crate) fn build_remote_lww_record_from_manifest(
    manifest_record: &ManifestRecord,
    incoming_content: &Option<Vec<u8>>,
) -> Option<crate::sync::content_class::LwwRecord> {
    if manifest_record.op == "upsert" && incoming_content.is_none() {
        return None;
    }
    if manifest_record.op != "upsert" && manifest_record.op != "delete" {
        return None;
    }
    Some(manifest_record.to_lww_record())
}

/// 读取文件 mtime（Unix 毫秒），失败返回 None。
pub(crate) fn read_mtime_ms(root: &Path, rel: &Path) -> Option<i64> {
    let path = root.join(rel);
    std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .and_then(|t| {
            t.duration_since(std::time::SystemTime::UNIX_EPOCH)
                .map_err(std::io::Error::other)
        })
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .ok()
}

/// 读取 live 的 device_id（从 app-meta/sync/state.local.json）。
///
/// 读取失败（文件不存在、JSON 损坏等）时返回 None，调用方回退空字符串，
/// LWW 退化为纯时间戳比较（仍优于固定 remote-wins）。
pub(crate) fn read_live_device_id(live_root: &Path) -> Option<String> {
    let state_path = live_root.join("app-meta/sync/state.local.json");
    if !state_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&state_path).ok()?;
    let state: crate::sync::types::SyncState = serde_json::from_str(&content).ok()?;
    if state.device_id.is_empty() {
        None
    } else {
        Some(state.device_id)
    }
}

/// 读取 live 的完整 SyncState。
///
/// 用于获取 tombstones（delete 的 deleted_at 时间戳）。
/// 读取失败时返回 None。
pub(crate) fn read_live_sync_state(live_root: &Path) -> Option<crate::sync::types::SyncState> {
    let state_path = live_root.join("app-meta/sync/state.local.json");
    if !state_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&state_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// 计算字节内容的 MD5 hex 摘要。`None`（文件不存在）返回空字符串。
pub(crate) fn md5_hex(content: &Option<Vec<u8>>) -> String {
    match content {
        Some(bytes) => {
            use std::io::Write;
            let mut hasher = md5::Context::new();
            hasher.write_all(bytes).ok();
            format!("{:x}", hasher.compute())
        }
        None => String::new(),
    }
}
