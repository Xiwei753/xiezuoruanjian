use std::path::Path;

use crate::error::Result;

/// #644 评论 5474166587 问题3：从文件系统构造本地侧 LWW 记录。
///
/// 读取文件 mtime 作为 `updated_at_ms`；`op` 按内容是否存在决定（Some → upsert，
/// None → delete）。`device_id` 用 live 的 device_id。
///
/// #644 评论 5475110422 第4节：delete 时从 `tombstones` 查找 `deleted_at`，
/// 不再固定写 0。若 tombstones 中无记录，返回 `None`（调用方应报错或补 tombstone）。
pub(crate) fn build_local_lww_record(
    root: &Path,
    rel: &Path,
    content: &Option<Vec<u8>>,
    device_id: &str,
    tombstones: &[crate::sync::types::Tombstone],
) -> Option<crate::sync::content_class::LwwRecord> {
    use crate::sync::content_class::LwwRecord;

    let rel_str = rel.to_string_lossy().to_string();

    let (content_hash, op, updated_at_ms, deleted_at_ms) = match content {
        Some(bytes) => {
            let hash = md5_hex(&Some(bytes.clone()));
            let mtime = read_mtime_ms(root, rel).unwrap_or(0);
            (hash, "upsert", mtime, None)
        }
        None => {
            // #644 评论 5475110422 第4节：从 tombstones 查找删除时间。
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

/// #644 评论 5474772497 第2节：staging manifest 中的文件记录（JSON 反序列化用）。
///
/// 与 `types::ManifestFileRecord` 字段一致，但不依赖 `github-api` feature gate。
/// 仅用于从 staging 的 `manifest.sync.json` 读取远端 LWW 元数据。
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct ManifestRecord {
    path: String,
    #[serde(rename = "content_hash")]
    _content_hash: String,
    updated_at_ms: i64,
    #[serde(default)]
    deleted_at_ms: Option<i64>,
    device_id: String,
    op: String,
}

/// #644 评论 5474772497 第2节：从 staging 的 manifest.sync.json 读取远端 LWW 记录。
///
/// #644 评论 5475805198 第4节：改为 `Result<Option<...>>`。
/// - manifest 不存在 → `Ok(None)`（Git backend 不产生 manifest，mtime fallback）。
/// - manifest 存在且解析成功 → `Ok(Some(map))`。
/// - manifest 存在但读/解析失败 → `Err`（GithubApi backend 的 manifest 是事实来源，
///   解析失败不能静默切成另一套决策规则）。
///
/// 调用方根据 `is_github_api` 决定是否允许 `None`（mtime fallback）。
pub(crate) fn read_staging_manifest(
    staging_root: &Path,
) -> Result<Option<std::collections::HashMap<String, ManifestRecord>>> {
    let manifest_path = staging_root.join("app-meta/sync/manifest.sync.json");
    if !manifest_path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&manifest_path).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "read_staging_manifest: failed to read manifest.sync.json: {}",
            e
        )))
    })?;
    #[derive(serde::Deserialize)]
    struct ManifestContainer {
        files: Vec<ManifestRecord>,
    }
    let container: ManifestContainer = serde_json::from_str(&content).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "read_staging_manifest: manifest.sync.json parse error: {}",
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

/// #644 评论 5474772497 第2节：从 manifest 记录构造远端侧 LWW 记录。
///
/// 使用 manifest 中的真实 `updated_at_ms`、`device_id`、`op`，
/// 而非文件系统 mtime 和固定 "remote" 字符串。
pub(crate) fn build_remote_lww_record_from_manifest(
    manifest_record: &ManifestRecord,
    incoming_content: &Option<Vec<u8>>,
) -> crate::sync::content_class::LwwRecord {
    use crate::sync::content_class::LwwRecord;

    let content_hash = match incoming_content {
        Some(bytes) => md5_hex(&Some(bytes.clone())),
        None => String::new(),
    };

    LwwRecord {
        content_hash,
        updated_at_ms: manifest_record.updated_at_ms,
        deleted_at_ms: manifest_record.deleted_at_ms,
        device_id: manifest_record.device_id.clone(),
        op: manifest_record.op.clone(),
    }
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

/// #644 评论 5475110422 第4节：读取 live 的完整 SyncState。
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
