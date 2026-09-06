//! #644 评论 5462823517 第3节：LWW manifest/state/local record 构造。
//!
//! 从 lww.rs 抽出的 manifest 相关：SYNC_MANIFEST_PATH 常量、lww_record_time。
//!
//! #644 评论 5473105049 第5节：local record / tombstone record / remote record 构造
//! 也收归本模块，`mod.rs` 只保留重试和一次 attempt 的编排。
//!
//! #645 评论 5504296097 问题1：`snapshot_local_records_read_only` 是真正的只读
//! local record 投影 helper，保留 per-file LWW（含真实 winner device_id），
//! 绝不伪造 now_ms 作为删除时间。`snapshot_local_target_lifecycle` 和 LWW
//! `execute_lww_sync_attempt` 都复用它。

use crate::sync::types::{ManifestFileRecord, SyncManifest, SyncScope};
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

/// #645 评论 5504296097 问题1：真正的只读 local record 投影 helper。
///
/// 读 old manifest + read-only SyncState → scan 当前文件，产出完整 LWW 投影：
///
/// - 当前 hash 与 old manifest record hash 相同 → 直接 clone old manifest record，
///   保留原 `updated_at` / `deleted_at` / `device_id` / `op`（**不**丢 winner device_id）；
/// - 当前 hash 改了或是新文件 → 用当前文件 mtime + 当前真实 device_id 生成新 upsert；
/// - known file 消失且有真实 tombstone → 用 tombstone 的 `deleted_at` + `deleted_by`/device_id
///   生成 delete record（**不**伪造 now_ms）；
/// - known file 消失且无 tombstone → 返回 `Err`（调用方应走 Retry，绝不能用 now_ms 伪造删除时间）。
///
/// 这个 helper 是真正只读的：用 [`SyncService::load_sync_state_read_only`] 加载 state，
/// 不写文件、不删旧文件。
///
/// 返回 `HashMap<path, ManifestFileRecord>`，供 `build_sync_plan`（plan/dry-run 路径）
/// 与 LWW `execute_lww_sync_attempt` 同源复用。
#[allow(clippy::too_many_lines, clippy::excessive_nesting)]
pub fn snapshot_local_records_read_only(
    sync_root: &Path,
    scope: SyncScope,
    preferred_device_id: &str,
) -> crate::error::Result<HashMap<String, ManifestFileRecord>> {
    // 1. 读 old manifest（manifest.sync.json）→ HashMap<path, ManifestFileRecord>。
    // #645 评论 5504296097 问题2：manifest 存在但损坏 → 返回 Err（不静默 fallback）。
    // 调用方（compute_local_project_lifecycle_candidate）据此返回 Retry，
    // 避免在无可靠本地 LWW 证据时做破坏性 DeleteLocalProject 决策。
    // manifest 不存在 → 空 HashMap（首次同步，正常）。
    let manifest_path = sync_root.join(SYNC_MANIFEST_PATH);
    let old_manifest_records: HashMap<String, ManifestFileRecord> = if manifest_path.exists() {
        let content = std::fs::read(&manifest_path).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "snapshot_local_records_read_only: manifest read failed at {}: {e}",
                manifest_path.display()
            )))
        })?;
        serde_json::from_slice::<SyncManifest>(&content)
            .map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "snapshot_local_records_read_only: manifest corrupt at {}: {e}",
                    manifest_path.display()
                )))
            })?
            .files
            .into_iter()
            .map(|r| (r.path.clone(), r))
            .collect()
    } else {
        HashMap::new()
    };

    // 2. 读 SyncState（read-only，不写文件）。
    let state = SyncService::load_sync_state_read_only(
        sync_root,
        if preferred_device_id.is_empty() {
            None
        } else {
            Some(preferred_device_id)
        },
    )?;

    // 3. scan 当前文件。
    let local_entries = crate::sync::scanner::scan_for_sync(sync_root, scope)?;
    let now_ms = chrono::Utc::now().timestamp_millis();

    let mut records = HashMap::new();

    for entry in &local_entries {
        if entry.sync_kind == crate::sync::types::SyncKind::Upload
            && entry.relative_path != SYNC_MANIFEST_PATH
        {
            let path = entry.relative_path.clone();
            let local_hash = entry.file_hash.clone();

            // #645 评论 5504296097 问题1.1：当前 hash 与 old manifest record hash 相同
            // → 直接 clone old manifest record，保留原 device_id。
            if let Some(old_rec) = old_manifest_records.get(&path) {
                if old_rec.content_hash == local_hash && old_rec.op == "upsert" {
                    records.insert(path.clone(), old_rec.clone());
                    continue;
                }
            }

            // 当前 hash 改了或是新文件 → 用当前文件 mtime + 当前真实 device_id。
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

            records.insert(
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

    // 4. known file 消失 → 有 tombstone 用真实删除时间；无 tombstone 返回 Err。
    for path in state.known_files.keys() {
        if records.contains_key(path) {
            continue;
        }
        if !SyncService::is_whitelisted_path(path, scope)
            || SyncService::is_blacklisted_path(path, scope)
        {
            continue;
        }
        if !sync_root.join(path).exists() {
            // 有真实 tombstone → 用 tombstone 的 deleted_at + deleted_by/device_id。
            if let Some(tombstone) = state.tombstones.iter().find(|t| t.original_path == *path) {
                let deleted_at_ms = tombstone.deleted_at * 1000;
                records.insert(
                    path.clone(),
                    ManifestFileRecord {
                        path: path.clone(),
                        content_hash: String::new(),
                        updated_at_ms: deleted_at_ms,
                        deleted_at_ms: Some(deleted_at_ms),
                        device_id: if tombstone.deleted_by.is_empty() {
                            state.device_id.clone()
                        } else {
                            tombstone.deleted_by.clone()
                        },
                        op: "delete".to_string(),
                        schema_version: 1,
                    },
                );
            } else {
                // #645 评论 5504296097 问题1.2：无 tombstone → 绝不伪造 now_ms。
                // 返回 Err，调用方应走 Retry。
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "snapshot_local_records_read_only: known file {} missing without tombstone \
                     — cannot fabricate delete time",
                    path
                ))));
            }
        }
    }

    // 5. #645 评论 5504296097 问题1 修复：manifest 中有 upsert record 但文件不在磁盘上
    //    且不在 known_files → 不再把旧 upsert 冒充当前本地事实。
    //    - 文件实际存在 → 说明被 whitelisted/blacklisted 跳过，不管（上面 step 3 已处理）；
    //    - 文件不存在 + 有 tombstone（state.tombstones 中 original_path 匹配）
    //      → 生成 Delete record（用 tombstone 的 deleted_at*1000 作为 deleted_at_ms
    //      和 updated_at_ms，device_id 用 tombstone.deleted_by 或 state.device_id）；
    //    - 文件不存在 + 无 tombstone → 返回 Err（不能伪造当前本地事实）。
    for (path, old_rec) in &old_manifest_records {
        if records.contains_key(path) {
            continue;
        }
        if old_rec.op != "upsert" {
            continue;
        }
        // 文件实际存在 → 上面 step 3 已处理（whitelisted 之外或被跳过的）。
        if sync_root.join(path).exists() {
            continue;
        }
        // 文件不存在 → 查找 tombstone。
        if let Some(tombstone) = state.tombstones.iter().find(|t| t.original_path == *path) {
            let deleted_at_ms = tombstone.deleted_at * 1000;
            records.insert(
                path.clone(),
                ManifestFileRecord {
                    path: path.clone(),
                    content_hash: String::new(),
                    updated_at_ms: deleted_at_ms,
                    deleted_at_ms: Some(deleted_at_ms),
                    device_id: if tombstone.deleted_by.is_empty() {
                        state.device_id.clone()
                    } else {
                        tombstone.deleted_by.clone()
                    },
                    op: "delete".to_string(),
                    schema_version: 1,
                },
            );
        } else {
            // 无 tombstone → 不能把旧 upsert 冒充当前本地事实，返回 Err。
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "snapshot_local_records_read_only: old manifest upsert for {path} but file missing \
                 without tombstone — cannot fabricate current local fact"
            ))));
        }
    }

    Ok(records)
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

    /// #645 评论 5504296097 问题1：snapshot_local_records_read_only 基本测试。
    /// 验证：当前文件 hash 与 old manifest record hash 相同时，保留原 device_id。
    #[test]
    fn test_snapshot_local_records_read_only_preserves_device_id() {
        use crate::sync::types::SyncScope;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let sync_root = tmp.path();

        // 写一个 project.json 文件。
        std::fs::write(sync_root.join("project.json"), b"content").unwrap();

        // 写 old manifest，记录 device_id = "winner-device"。
        let manifest = crate::sync::types::SyncManifest {
            files: vec![ManifestFileRecord {
                path: "project.json".to_string(),
                content_hash: format!("{:x}", md5::compute(b"content")),
                updated_at_ms: 1000,
                deleted_at_ms: None,
                device_id: "winner-device".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };
        std::fs::create_dir_all(sync_root.join("app-meta/sync")).unwrap();
        let _ = std::fs::write(
            sync_root.join(SYNC_MANIFEST_PATH),
            serde_json::to_string(&manifest).unwrap(),
        );

        let records =
            snapshot_local_records_read_only(sync_root, SyncScope::Project, "current-device")
                .unwrap();
        let rec = records.get("project.json").unwrap();
        // #645 评论 5504296097 问题1.1：保留原 winner 的 device_id，不写 currentA当前设备。
        assert_eq!(rec.device_id, "winner-device");
    }
}
