//! 远端对象快照的增量下载（Issue #1066）。
//!
//! 传输层只做三件事：
//!
//! 1. 读远端固定 HEAD 上的对象索引（manifest）；
//! 2. 只下载"远端内容与已确认基线/本机内容都不同"的 blob 到 staging；
//! 3. 把远端索引记录原样写进 staging manifest，供提交阶段做唯一一次裁决。
//!
//! 这里不做业务合并、不做 LWW 裁决、不复制本地整棵目录：裁决只在
//! [`crate::sync::staging::StagingRun::compute_commit_plan`] 里发生一次。
//!
//! 远端索引缺失（首次同步 / 旧端只写了对象文件）时退化为列出远端对象并逐个
//! 下载，用实际内容建立第一版索引；远端索引存在时它是权威快照：索引里没有
//! 的已确认对象表示远端删除。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::error::Result;
use crate::sync::provider::SyncProvider;
use crate::sync::types::{
    AppDataGroup, ManifestFileRecord, SyncManifest, SyncState, SyncTarget,
};

use super::resolve::{read_staging_manifest, read_staging_manifest_for_group};

/// staging 根下的标记文件：远端还没有对象索引（首次同步/旧端）。
///
/// 提交阶段据此区分"远端索引说没有该对象"（= 删除）与"远端根本没有索引"
/// （= 未参与，不是删除）。
pub(crate) const REMOTE_INDEX_MISSING_MARKER: &str = ".remote-index-missing";

/// 一次增量下载的结果。
#[derive(Debug, Default, Clone)]
pub(crate) struct RemoteStageOutcome {
    /// 本轮从远端下载并写入 staging 的相对路径。
    pub downloaded: Vec<String>,
    /// 远端索引里记录的删除事实（对象相对路径）。
    pub remote_deletes: Vec<String>,
    /// staging 里写入的远端记录总数。
    pub records: usize,
    /// 远端索引不可用（首次同步）时为 true：此时远端"没有该对象"不是删除。
    pub remote_index_missing: bool,
}

/// staging manifest 的相对路径（按 target 的 group 决定）。
pub(crate) fn staging_manifest_rel(target: &SyncTarget) -> String {
    target
        .app_group
        .as_ref()
        .map(AppDataGroup::local_manifest_path)
        .unwrap_or_else(|| crate::sync::lww::SYNC_MANIFEST_PATH.to_string())
}

/// 读取远端索引；返回 `None` 表示远端还没有索引（首次同步/旧端）。
fn read_remote_index(
    provider: &dyn SyncProvider,
    target: &SyncTarget,
) -> Result<Option<SyncManifest>> {
    crate::sync::lww::fetch_remote_manifest_at(provider, &target.remote_prefix)
}

/// 本机已确认基线（known_files）。
fn load_known_files(
    sync_root: &Path,
    target: &SyncTarget,
) -> Result<BTreeMap<String, String>> {
    let state: SyncState = match &target.app_group {
        Some(group) => crate::sync::SyncService::load_sync_state_read_only_for_group(
            sync_root, group, None,
        )?,
        None => crate::sync::SyncService::load_sync_state_read_only(sync_root, None)?,
    };
    Ok(state.known_files.into_iter().collect())
}

/// 增量下载远端变化对象到 staging，并把远端索引记录写进 staging manifest。
///
/// - 远端内容与已确认基线相同 → 不下载（未参与本轮）；
/// - 远端内容与本机当前内容相同 → 不下载（下次提交会把基线推进到该内容）；
/// - 远端内容不同 → 只下载这一个 blob；
/// - 远端索引记录了 delete → 只写记录，不读 blob。
pub(crate) fn stage_remote_changes(
    provider: &dyn SyncProvider,
    target: &SyncTarget,
    sync_root: &Path,
    staging_root: &Path,
) -> Result<RemoteStageOutcome> {
    let known = load_known_files(sync_root, target)?;
    let mut outcome = RemoteStageOutcome::default();

    let remote_manifest = read_remote_index(provider, target)?;
    let mut records: BTreeMap<String, ManifestFileRecord> = BTreeMap::new();
    let marker = staging_root.join(REMOTE_INDEX_MISSING_MARKER);

    match remote_manifest {
        Some(manifest) => {
            let _ = std::fs::remove_file(&marker);
            for record in manifest.files {
                records.insert(record.path.clone(), record);
            }
        }
        None => {
            outcome.remote_index_missing = true;
            if let Some(parent) = marker.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&marker, b"1")?;
            // 首次同步/旧端：远端只有对象文件，没有索引。列出远端对象并下载，
            // 用实际内容建立第一版索引；此时"远端没有"不代表删除。
            let prefix = target.remote_prefix.trim_end_matches('/');
            for entry in provider.list(prefix)? {
                let rel = entry.path.trim_start_matches('/').to_string();
                if rel.is_empty() {
                    continue;
                }
                if let Some(group) = &target.app_group {
                    if !group.owns_path(&rel) {
                        continue;
                    }
                }
                let remote_path = format!("{prefix}/{rel}");
                let Some(object) = provider.read(&remote_path)? else {
                    continue;
                };
                let hash = crate::sync::hash::content_md5(&object.content);
                let record = ManifestFileRecord {
                    path: rel.clone(),
                    content_hash: hash,
                    updated_at_ms: 0,
                    deleted_at_ms: None,
                    device_id: String::new(),
                    op: "upsert".to_string(),
                    schema_version: 1,
                };
                records.insert(rel.clone(), record);
                write_staging_blob(staging_root, &rel, &object.content)?;
                outcome.downloaded.push(rel);
            }
        }
    }

    // 有索引时只下载真正变化的 blob。
    if !outcome.remote_index_missing {
        for record in records.values() {
            if record.op == "delete" {
                outcome.remote_deletes.push(record.path.clone());
                continue;
            }
            if record.content_hash.is_empty() {
                continue;
            }
            if known.get(&record.path).map(String::as_str) == Some(record.content_hash.as_str()) {
                // 远端停在已确认基线：本轮未参与。
                continue;
            }
            let local_path = sync_root.join(&record.path);
            if local_path.is_file() {
                let local_hash = crate::sync::hash::content_md5_file(&local_path).unwrap_or_default();
                if local_hash == record.content_hash {
                    // 本机已有同一内容，不需要网络下载。
                    continue;
                }
            }
            let remote_path = format!(
                "{}/{}",
                target.remote_prefix.trim_end_matches('/'),
                record.path
            );
            let Some(object) = provider.read(&remote_path)? else {
                // 索引声明存在但 blob 缺失：保留记录，提交阶段把它当作未参与。
                log::warn!(
                    "[sync] remote index lists {} but blob is missing at {}",
                    record.path,
                    remote_path
                );
                continue;
            };
            let actual = crate::sync::hash::content_md5(&object.content);
            if actual != record.content_hash {
                return Err(crate::Error::Other(format!(
                    "remote object content hash mismatch: path={} expected={} actual={actual}",
                    record.path, record.content_hash
                )));
            }
            write_staging_blob(staging_root, &record.path, &object.content)?;
            outcome.downloaded.push(record.path.clone());
        }
    }

    outcome.downloaded.sort();
    outcome.downloaded.dedup();
    outcome.records = records.len();

    // 远端索引记录原样写进 staging manifest：提交阶段只读这一份事实。
    let manifest = SyncManifest {
        files: records.into_values().collect(),
    };
    let rel = staging_manifest_rel(target);
    let path = staging_root.join(&rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_vec_pretty(&manifest)?;
    crate::storage::transaction::atomic_write_bytes(&path, &content)?;
    Ok(outcome)
}

fn write_staging_blob(staging_root: &Path, rel: &str, content: &[u8]) -> Result<()> {
    let path = staging_root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::storage::transaction::atomic_write_bytes(&path, content)
}

/// staging manifest 里的远端记录；提交阶段读取用。
pub(crate) fn read_staged_remote_records(
    target: &SyncTarget,
    staging_root: &Path,
) -> Result<BTreeMap<String, super::resolve::ManifestRecord>> {
    let records = match &target.app_group {
        Some(group) => read_staging_manifest_for_group(staging_root, group)?,
        None => read_staging_manifest(staging_root)?,
    };
    Ok(records.unwrap_or_default().into_iter().collect())
}

/// 远端索引记录里出现的路径集合（包含 delete 记录）。
pub(crate) fn staged_remote_paths(
    records: &BTreeMap<String, super::resolve::ManifestRecord>,
) -> BTreeSet<String> {
    records.keys().cloned().collect()
}
