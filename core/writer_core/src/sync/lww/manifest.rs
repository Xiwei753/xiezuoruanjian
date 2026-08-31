//! #644 评论 5462823517 第3节：LWW manifest/state/local record 构造。
//!
//! 从 lww.rs 抽出的 manifest 相关：SYNC_MANIFEST_PATH 常量、lww_record_time。
//!
//! #644 评论 5473105049 第5节：local record / tombstone record / remote record 构造
//! 也收归本模块，`mod.rs` 只保留重试和一次 attempt 的编排。

use crate::sync::types::{ManifestFileRecord, SyncManifest, SyncScope, SyncState};
use crate::sync::SyncService;
use std::collections::HashMap;
use std::path::Path;

/// 同步清单文件路径——记录本地所有文件的哈希、操作类型和时间戳。
/// 这是 LWW 同步的唯一事实来源：三路比较的 base_hash 即从此文件读取。
pub(super) const SYNC_MANIFEST_PATH: &str = "app-meta/sync/manifest.sync.json";

/// 获取 LWW 比较时间戳。
///
/// 对于 delete 操作，优先使用 `deleted_at_ms`（精确的删除时间），
/// 回退到 `updated_at_ms`（删除操作记录的更新时间）。
/// 对于 upsert 操作，直接使用 `updated_at_ms`。
pub(super) fn lww_record_time(record: &ManifestFileRecord) -> i64 {
    if record.op == "delete" {
        record.deleted_at_ms.unwrap_or(record.updated_at_ms)
    } else {
        record.updated_at_ms
    }
}

/// #644 评论 5473105049 第5节：构建本地文件记录（upsert + delete 墓碑）。
///
/// 从 `execute_lww_sync_attempt` 抽出。扫描结果中 `SyncKind::Upload` 的文件生成
/// upsert 记录；`known_files` 中存在但本地已删除的文件生成 delete 墓碑记录。
///
/// `updated_at_ms` 确定策略：
/// 1. 已知文件且哈希未变 → 使用上次同步记录的时间戳
/// 2. 已知文件但哈希已变 → 使用文件系统 mtime
/// 3. 新文件 → 使用文件系统 mtime
/// 4. mtime 读取失败 → 退回 `now_ms`
#[allow(clippy::excessive_nesting)]
pub(super) fn build_local_records(
    sync_root: &Path,
    local_entries: &[crate::sync::types::SyncFileEntry],
    state: &SyncState,
    scope: SyncScope,
    now_ms: i64,
) -> HashMap<String, ManifestFileRecord> {
    let mut local_records = HashMap::new();

    for entry in local_entries {
        if entry.sync_kind == crate::sync::types::SyncKind::Upload
            && entry.relative_path != SYNC_MANIFEST_PATH
        {
            let path = entry.relative_path.clone();
            let local_hash = entry.file_hash.clone();

            let updated_at_ms = if let Some(known_hash) = state.known_files.get(&path) {
                if *known_hash == local_hash {
                    state
                        .known_files_updated_at
                        .get(&path)
                        .cloned()
                        .unwrap_or(0)
                } else {
                    read_mtime_ms(sync_root, &path, now_ms)
                }
            } else {
                read_mtime_ms(sync_root, &path, now_ms)
            };

            local_records.insert(
                path.clone(),
                ManifestFileRecord {
                    path,
                    content_hash: local_hash,
                    updated_at_ms,
                    deleted_at_ms: None,
                    device_id: state.device_id.clone(),
                    op: "upsert".to_string(),
                    schema_version: 1,
                },
            );
        }
    }

    // 构建本地删除墓碑记录
    for path in state.known_files.keys() {
        if !local_records.contains_key(path) {
            if !SyncService::is_whitelisted_path(path, scope)
                || SyncService::is_blacklisted_path(path, scope)
            {
                continue;
            }
            if !sync_root.join(path).exists() {
                let mut updated_at_ms = now_ms;
                if let Some(tombstone) = state.tombstones.iter().find(|t| t.original_path == *path)
                {
                    updated_at_ms = tombstone.deleted_at * 1000;
                }

                local_records.insert(
                    path.clone(),
                    ManifestFileRecord {
                        path: path.clone(),
                        content_hash: String::new(),
                        updated_at_ms,
                        deleted_at_ms: Some(updated_at_ms),
                        device_id: state.device_id.clone(),
                        op: "delete".to_string(),
                        schema_version: 1,
                    },
                );
            }
        }
    }

    local_records
}

/// #644 评论 5473105049 第5节：构建远端文件记录。
///
/// 从远端 manifest 和 tree 构建 `path → ManifestFileRecord` 映射。
/// 远端 tree 中存在但 manifest 中无记录的文件（首次同步或 manifest 损失），
/// 用 tree SHA 作为 content_hash 补充记录。
pub(super) fn build_remote_records(
    remote_manifest: SyncManifest,
    remote_tree_files: &HashMap<String, String>,
    scope: SyncScope,
) -> HashMap<String, ManifestFileRecord> {
    let mut remote_records = HashMap::new();
    for rec in remote_manifest.files {
        if rec.path != SYNC_MANIFEST_PATH {
            remote_records.insert(rec.path.clone(), rec);
        }
    }

    for (path, sha) in remote_tree_files {
        if path != SYNC_MANIFEST_PATH && !remote_records.contains_key(path) {
            if !SyncService::is_whitelisted_path(path, scope)
                || SyncService::is_blacklisted_path(path, scope)
            {
                continue;
            }
            remote_records.insert(
                path.clone(),
                ManifestFileRecord {
                    path: path.clone(),
                    content_hash: sha.clone(),
                    updated_at_ms: 0,
                    deleted_at_ms: None,
                    device_id: "remote".to_string(),
                    op: "upsert".to_string(),
                    schema_version: 1,
                },
            );
        }
    }

    remote_records
}

/// 读取文件 mtime，失败时回退 `fallback_ms`。
fn read_mtime_ms(sync_root: &Path, path: &str, fallback_ms: i64) -> i64 {
    std::fs::metadata(sync_root.join(path))
        .and_then(|m| m.modified())
        .and_then(|t| {
            t.duration_since(std::time::SystemTime::UNIX_EPOCH)
                .map_err(std::io::Error::other)
        })
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(fallback_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::types::ManifestFileRecord;

    #[test]
    fn test_lww_record_time_non_delete() {
        let record = ManifestFileRecord {
            path: "test.md".to_string(),
            content_hash: "hash".to_string(),
            op: "upsert".to_string(),
            updated_at_ms: 1000,
            deleted_at_ms: Some(2000), // Should be ignored
            device_id: "dev1".to_string(),
            schema_version: 1,
        };
        assert_eq!(lww_record_time(&record), 1000);
    }

    #[test]
    fn test_lww_record_time_delete_with_deleted_at_ms() {
        let record = ManifestFileRecord {
            path: "test.md".to_string(),
            content_hash: "hash".to_string(),
            op: "delete".to_string(),
            updated_at_ms: 1000,
            deleted_at_ms: Some(2000),
            device_id: "dev1".to_string(),
            schema_version: 1,
        };
        assert_eq!(lww_record_time(&record), 2000);
    }

    #[test]
    fn test_lww_record_time_delete_without_deleted_at_ms() {
        let record = ManifestFileRecord {
            path: "test.md".to_string(),
            content_hash: "hash".to_string(),
            op: "delete".to_string(),
            updated_at_ms: 1000,
            deleted_at_ms: None,
            device_id: "dev1".to_string(),
            schema_version: 1,
        };
        assert_eq!(lww_record_time(&record), 1000); // Fallback to updated_at_ms
    }

    #[test]
    fn test_lww_record_time_tie_breaker_with_deleted_at_ms() {
        // Local is a newer edit based on updated_at_ms
        let local_rec = ManifestFileRecord {
            path: "a.txt".to_string(),
            content_hash: "hash1".to_string(),
            op: "upsert".to_string(),
            updated_at_ms: 1500,
            deleted_at_ms: None,
            device_id: "dev1".to_string(),
            schema_version: 1,
        };
        // Remote is a delete. updated_at_ms is older, but deleted_at_ms is newer
        let remote_rec = ManifestFileRecord {
            path: "a.txt".to_string(),
            content_hash: "hash1".to_string(),
            op: "delete".to_string(),
            updated_at_ms: 1000,
            deleted_at_ms: Some(2000),
            device_id: "dev2".to_string(),
            schema_version: 1,
        };

        let local_time = lww_record_time(&local_rec);
        let remote_time = lww_record_time(&remote_rec);

        // Remote time should be 2000 (from deleted_at_ms) and win against local's 1500
        assert!(remote_time > local_time);
        assert_eq!(remote_time, 2000);
    }
}
