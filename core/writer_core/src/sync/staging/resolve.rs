use std::path::Path;

use crate::error::Result;

/// staging manifest 中的文件记录（JSON 反序列化用）。
///
/// 与 `types::ManifestFileRecord` 字段一致，但不依赖 `github-api` feature gate。
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

/// 从 staging 的权威清单读取远端对象记录。
pub(crate) fn read_staging_manifest(
    staging_root: &Path,
) -> Result<Option<std::collections::HashMap<String, ManifestRecord>>> {
    read_manifest_at(staging_root, crate::sync::lww::SYNC_MANIFEST_PATH)
}

/// 读取 live 的权威清单，作为本轮比较的本地版本事实。
///
/// 与 staging manifest 使用同一套解析规则（v2 存储优先，旧单文件只读回退）；
/// 不存在返回 `Ok(None)`，损坏返回 `Err`，不能静默退回 mtime。
pub(crate) fn read_live_manifest(
    live_root: &Path,
) -> Result<Option<std::collections::HashMap<String, ManifestRecord>>> {
    read_manifest_at(live_root, crate::sync::lww::SYNC_MANIFEST_PATH)
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
    // 分组自己的权威清单（v2 优先，旧单文件只读回退）。
    if let Some(mut records) = read_manifest_at(root, &group.local_manifest_path())? {
        records.retain(|path, _| group.owns_path(path));
        return Ok(Some(records));
    }
    // 分组还没有自己的清单时，聚合清单是只读迁移输入：只保留该组拥有的路径。
    let mut records = match read_manifest_at(root, crate::sync::lww::SYNC_MANIFEST_PATH)? {
        Some(records) => records,
        None => return Ok(None),
    };
    records.retain(|path, _| group.owns_path(path));
    Ok(Some(records))
}

/// 读取 `root` 下的权威清单：v2 存储优先，旧单文件只读回退。
///
/// `manifest_rel` 是清单的旧单文件相对路径；v2 存储固定在该路径的父目录下
/// `manifest/` 里（与 [`crate::sync::lww::ManifestStore`] 一致）。
fn read_manifest_at(
    root: &Path,
    manifest_rel: &str,
) -> Result<Option<std::collections::HashMap<String, ManifestRecord>>> {
    let store = crate::sync::lww::ManifestStore::new(root, manifest_rel);
    if let Some(manifest) = store.load_all()? {
        return Ok(Some(
            manifest
                .files
                .into_iter()
                .map(|record| {
                    let records = ManifestRecord {
                        path: record.path,
                        content_hash: record.content_hash,
                        updated_at_ms: record.updated_at_ms,
                        deleted_at_ms: record.deleted_at_ms,
                        device_id: record.device_id,
                        op: record.op,
                    };
                    (records.path.clone(), records)
                })
                .collect(),
        ));
    }
    let manifest_path = root.join(manifest_rel);
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
