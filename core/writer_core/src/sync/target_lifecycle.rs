//! Target 生命周期 catalog — 远端持久、provider-neutral（Issue #645 评论 5504296097 问题3）。
//!
//! catalog 放在不会随 `projects/<id>/` 一起被删除的位置：
//! `app/app-meta/sync/targets.sync.json`（app target 的 remote_prefix 下）。
//!
//! ## 职责
//!
//! - 远端 `targets.sync.json`（本模块）：负责"跨设备都必须知道这个 target 的生命周期"，
//!   离线旧设备上线时先读 catalog，看到 delete tombstone 就不会把旧 project 重新上传。
//! - 本地 `pending_deleted_targets.json`（`pending_deleted` 模块）：负责
//!   "本机删除事务还没同步完成"，本机状态。两个职责不混。
//!
//! ## provider-neutral
//!
//! catalog 只通过 `SyncProvider::read/write` 操作；GitHub SHA、WebDAV ETag 等留在 Provider 里。

use crate::sync::provider::model::{RemoteVersion, WritePrecondition};
use crate::sync::provider::SyncProvider;
use crate::sync::types::{
    RemoteTargetCatalogSnapshot, TargetLifecycleCatalog, TargetLifecycleRecord, TargetOp,
};

/// catalog 在远端的固定路径：app target 的 remote_prefix（`"app"`）下。
///
/// 这个位置不会随 `projects/<id>/` 一起被删除，保证 delete tombstone 持久存在。
pub const TARGET_CATALOG_REMOTE_PATH: &str = "app/app-meta/sync/targets.sync.json";

/// #645 评论 5504296097 问题4：加载远端 catalog，返回带版本标识的快照。
///
/// - 远端不存在 catalog → 返回空 catalog + 写入方应用 `CreateNew`；
/// - 解析失败 → 返回 `Err`（不吞错误，调用方决定 Retry）。
///
/// 返回的 `version` 用于后续 CAS 写入（`IfMatch`），防止多设备并发覆盖。
pub fn load_remote_catalog(
    provider: &dyn SyncProvider,
) -> crate::error::Result<RemoteTargetCatalogSnapshot> {
    let obj = provider
        .read(TARGET_CATALOG_REMOTE_PATH)
        .map_err(crate::Error::from)?;
    let Some(obj) = obj else {
        // 文件不存在：首次写应用 CreateNew，版本用 sentinel 表示不存在。
        return Ok(RemoteTargetCatalogSnapshot {
            catalog: TargetLifecycleCatalog::default(),
            version: RemoteVersion::new("__nonexistent__"),
        });
    };
    let version = obj.version.clone();
    let catalog: TargetLifecycleCatalog = serde_json::from_slice(&obj.content).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "load_remote_catalog: parse {}: {e}",
            TARGET_CATALOG_REMOTE_PATH
        )))
    })?;
    Ok(RemoteTargetCatalogSnapshot { catalog, version })
}

/// #645 评论 5504296097 问题3/4：CAS 写远端 catalog，返回持久化后的完整 snapshot。
///
/// 使用 `WritePrecondition::IfMatch(version)` 防止多设备并发覆盖。
/// `PreconditionFailed` 时自动重读远端 catalog、LWW 合并本地变更、再 IfMatch 写入。
/// 重试次数限制为 3（超过视为并发冲突过于激烈，返回 `Err`）。
///
/// `snapshot.version` 为 `__nonexistent__` 时使用 `CreateNew`（首次写入）。
/// 序列化失败或 provider.write 失败 → `Err`。
///
/// #645 评论 5504296097 问题3：返回 `RemoteTargetCatalogSnapshot`（实际持久化后的完整
/// catalog + version），调用方必须用返回值更新本地 catalog 和 version，避免
/// "version 是新的、内容还是旧的"非法组合。
pub fn write_remote_catalog(
    provider: &dyn SyncProvider,
    snapshot: &RemoteTargetCatalogSnapshot,
) -> crate::error::Result<RemoteTargetCatalogSnapshot> {
    let mut current_version = snapshot.version.clone();
    let mut current_catalog = snapshot.catalog.clone();
    let max_retries = 3;

    for attempt in 0..max_retries {
        let content = serde_json::to_vec(&current_catalog).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "write_remote_catalog: serialize: {e}"
            )))
        })?;

        let precondition = if current_version.as_str() == "__nonexistent__" {
            WritePrecondition::CreateNew
        } else {
            WritePrecondition::IfMatch(current_version.clone())
        };

        match provider.write(TARGET_CATALOG_REMOTE_PATH, &content, precondition) {
            Ok(new_version) => {
                log::debug!(
                    "[sync] write_remote_catalog: succeeded (attempt={})",
                    attempt + 1
                );
                // #645 评论 5504296097 问题3：返回实际持久化后的完整 snapshot。
                // provider.write 成功后远端内容就是 current_catalog，version 是 new_version。
                return Ok(RemoteTargetCatalogSnapshot {
                    catalog: current_catalog.clone(),
                    version: new_version,
                });
            }
            Err(crate::sync::provider::error::ProviderError::PreconditionFailed { .. }) => {
                // #645 评论 5504296097 问题4：CAS 冲突 → 重读远端最新 catalog，
                // LWW 合并本地变更后重试。
                log::info!(
                    "[sync] write_remote_catalog: PreconditionFailed (attempt={}), \
                     re-reading and merging",
                    attempt + 1
                );
                let reloaded = load_remote_catalog(provider)?;
                current_catalog = merge_catalogs(&[reloaded.catalog, current_catalog]);
                current_version = reloaded.version;
            }
            Err(e) => {
                return Err(crate::Error::from(e));
            }
        }
    }

    Err(crate::Error::Io(std::io::Error::other(format!(
        "write_remote_catalog: CAS retry exhausted after {max_retries} attempts"
    ))))
}

/// #645 评论 5504296097 问题2：provider-neutral 原子决策接口。
///
/// 把一条 candidate lifecycle record 通过 CAS 写入远端 catalog。每次 CAS 冲突后：
/// 重读最新 snapshot → candidate 与最新 remote record 重新做 target-level LWW：
/// - candidate 仍赢 → merge → IfMatch 再写；
/// - remote 已经赢 → 返回 `LostToRemote` → 绝不能继续执行旧的 delete/upload 决策；
/// - Retry → 不删任何远端文件 → pending 保留。
///
/// `Applied` 才允许继续执行后续操作（delete/upload）；`LostToRemote` 改走
/// `RestoreProject / DeleteLocalProject`；`Retry` 不删。
#[allow(clippy::excessive_nesting)]
pub fn apply_lifecycle_record(
    provider: &dyn SyncProvider,
    snapshot: &RemoteTargetCatalogSnapshot,
    candidate: TargetLifecycleRecord,
) -> crate::sync::types::TargetLifecycleApplyResult {
    use crate::sync::types::TargetLifecycleApplyResult;

    let max_retries = 3;
    let mut current_snapshot = snapshot.clone();
    let current_candidate = candidate.clone();

    for attempt in 0..max_retries {
        // 1. 检查 candidate 与当前 remote record 的 LWW 关系。
        let remote_record = find_record(&current_snapshot.catalog, &current_candidate.target_id);
        let candidate_wins = match &remote_record {
            None => true, // 远端无记录，candidate 胜出
            Some(existing) => lww_record_wins(&current_candidate, existing),
        };

        if !candidate_wins {
            // remote 已经赢 → 返回 LostToRemote，绝不继续执行 delete/upload。
            // candidate_wins == false 蕴含 remote_record.is_some()。
            let remote_time = remote_record.map(record_lww_time).unwrap_or(0);
            log::info!(
                "[sync] apply_lifecycle_record: LostToRemote target={} \
                 candidate_time={} remote_time={} — aborting write",
                current_candidate.target_id,
                record_lww_time(&current_candidate),
                remote_time,
            );
            return TargetLifecycleApplyResult::LostToRemote(current_snapshot.clone());
        }

        // 2. candidate 仍赢 → merge → IfMatch 写。
        let mut merged_catalog = current_snapshot.catalog.clone();
        upsert_record(&mut merged_catalog, current_candidate.clone());
        let write_snapshot = RemoteTargetCatalogSnapshot {
            catalog: merged_catalog,
            version: current_snapshot.version.clone(),
        };

        match write_remote_catalog(provider, &write_snapshot) {
            Ok(persisted) => {
                log::debug!(
                    "[sync] apply_lifecycle_record: Applied (attempt={}) target={}",
                    attempt + 1,
                    current_candidate.target_id
                );
                return TargetLifecycleApplyResult::Applied(persisted);
            }
            Err(crate::Error::SyncRemoteError { category, .. })
                if category == "precondition_failed" =>
            {
                // CAS 冲突 → 重读最新 snapshot，重新判定 winner。
                log::info!(
                    "[sync] apply_lifecycle_record: CAS conflict (attempt={}), \
                     re-reading snapshot",
                    attempt + 1
                );
                match load_remote_catalog(provider) {
                    Ok(reloaded) => {
                        current_snapshot = reloaded;
                        // candidate 不变，下一轮重新与最新 remote record 比较。
                    }
                    Err(e) => {
                        return TargetLifecycleApplyResult::Retry(e);
                    }
                }
            }
            Err(e) => {
                return TargetLifecycleApplyResult::Retry(e);
            }
        }
    }

    TargetLifecycleApplyResult::Retry(crate::Error::Io(std::io::Error::other(format!(
        "apply_lifecycle_record: CAS retry exhausted after {max_retries} attempts for target={}",
        candidate.target_id
    ))))
}

/// 按 `target_id` upsert 一条记录（同 `target_id` 替换，否则追加）。
///
/// 注意：这是纯结构操作，不做 LWW 合并。LWW 合并在 [`merge_catalogs`] 里做。
/// 调用方应确保传入的 `record` 是该 `target_id` 的最新版本（已与现有记录做过 LWW 比较）。
pub fn upsert_record(catalog: &mut TargetLifecycleCatalog, record: TargetLifecycleRecord) {
    if let Some(existing) = catalog
        .records
        .iter_mut()
        .find(|r| r.target_id == record.target_id)
    {
        *existing = record;
    } else {
        catalog.records.push(record);
    }
}

/// 计算记录的 LWW 时间（delete 用 `deleted_at_ms`，upsert 用 `updated_at_ms`）。
///
/// 与 `sync::lww::manifest::lww_record_time` 同语义。
pub fn record_lww_time(record: &TargetLifecycleRecord) -> i64 {
    match record.op {
        TargetOp::Delete => record.deleted_at_ms.unwrap_or(record.updated_at_ms),
        TargetOp::Upsert => record.updated_at_ms,
    }
}

/// LWW 合并多个 catalog：按 `target_id` 分组，取 `(lww_time, device_id)` 最大的记录。
///
/// 与 `resolve_lww_path` 同规则：时间大的胜出，时间相同 `device_id` 字典序大的胜出。
/// 合并后 `records` 按 `target_id` 字典序排序，保证序列化稳定。
pub fn merge_catalogs(catalogs: &[TargetLifecycleCatalog]) -> TargetLifecycleCatalog {
    use std::collections::HashMap;
    let mut by_target: HashMap<String, TargetLifecycleRecord> = HashMap::new();
    for catalog in catalogs {
        for record in &catalog.records {
            // 保留 LWW 胜者：若现有记录更新则保留现有，否则用新记录覆盖。
            let winner = match by_target.get(&record.target_id) {
                Some(existing) if !lww_record_wins(record, existing) => existing.clone(),
                _ => record.clone(),
            };
            by_target.insert(record.target_id.clone(), winner);
        }
    }
    let mut records: Vec<TargetLifecycleRecord> = by_target.into_values().collect();
    records.sort_by(|a, b| a.target_id.cmp(&b.target_id));
    TargetLifecycleCatalog { records }
}

/// `candidate` 是否 LWW 胜过 `existing`（与 `resolve_lww_path` 同规则）。
///
/// 时间大的胜出；时间相同 `device_id` 字典序大的胜出。
fn lww_record_wins(candidate: &TargetLifecycleRecord, existing: &TargetLifecycleRecord) -> bool {
    let existing_time = record_lww_time(existing);
    let candidate_time = record_lww_time(candidate);
    if candidate_time > existing_time {
        true
    } else if candidate_time == existing_time {
        candidate.device_id > existing.device_id
    } else {
        false
    }
}

/// 查找指定 `target_id` 的记录。
pub fn find_record<'a>(
    catalog: &'a TargetLifecycleCatalog,
    target_id: &str,
) -> Option<&'a TargetLifecycleRecord> {
    catalog.records.iter().find(|r| r.target_id == target_id)
}

/// 判断 catalog 中该 target 是否有 upsert 记录（target 存在过/仍存在）。
pub fn catalog_has_upsert(catalog: &TargetLifecycleCatalog, target_id: &str) -> bool {
    find_record(catalog, target_id)
        .map(|r| r.op == TargetOp::Upsert)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::provider::memory::MemoryProvider;

    #[test]
    fn load_missing_returns_empty_snapshot() {
        let p = MemoryProvider::new();
        let snapshot = load_remote_catalog(&p).unwrap();
        assert!(snapshot.catalog.records.is_empty());
        assert_eq!(snapshot.version.as_str(), "__nonexistent__");
    }

    #[test]
    fn write_load_roundtrip_with_version() {
        let p = MemoryProvider::new();
        let mut catalog = TargetLifecycleCatalog::default();
        upsert_record(
            &mut catalog,
            TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 1000, "dev-1"),
        );
        upsert_record(
            &mut catalog,
            TargetLifecycleRecord::delete("projects/p2", "projects/p2", 2000, "dev-2"),
        );
        // 首次写入使用 CreateNew。
        let snapshot = RemoteTargetCatalogSnapshot {
            catalog: catalog.clone(),
            version: RemoteVersion::new("__nonexistent__"),
        };
        write_remote_catalog(&p, &snapshot).unwrap();

        let loaded = load_remote_catalog(&p).unwrap();
        assert_eq!(loaded.catalog, catalog);
        // 版本号不再是 sentinel。
        assert_ne!(loaded.version.as_str(), "__nonexistent__");
    }

    #[test]
    fn cas_write_uses_ifmatch_after_initial() {
        let p = MemoryProvider::new();
        let mut catalog = TargetLifecycleCatalog::default();
        upsert_record(
            &mut catalog,
            TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 1000, "dev-1"),
        );

        // 第一次写入（CreateNew）。
        let snap1 = RemoteTargetCatalogSnapshot {
            catalog: catalog.clone(),
            version: RemoteVersion::new("__nonexistent__"),
        };
        write_remote_catalog(&p, &snap1).unwrap();

        // 读取最新版本。
        let snap2 = load_remote_catalog(&p).unwrap();
        assert_ne!(snap2.version.as_str(), "__nonexistent__");

        // 用正确版本 CAS 写入应成功（CAS retry 会重读最新版本）。
        write_remote_catalog(&p, &snap2).unwrap();

        // 用错误版本 CAS 写入：CAS retry 会重读最新版本并重写，
        // 在 MemoryProvider 单线程环境下总会成功（无并发覆盖）。
        let snap_bad = RemoteTargetCatalogSnapshot {
            catalog: catalog.clone(),
            version: RemoteVersion::new("stale-version"),
        };
        let result = write_remote_catalog(&p, &snap_bad);
        // CAS retry 重读后写入成功（MemoryProvider 无并发冲突）。
        assert!(result.is_ok());
    }

    #[test]
    fn upsert_replaces_same_target_id() {
        let mut catalog = TargetLifecycleCatalog::default();
        upsert_record(
            &mut catalog,
            TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 1000, "dev-1"),
        );
        upsert_record(
            &mut catalog,
            TargetLifecycleRecord::delete("projects/p1", "projects/p1", 2000, "dev-2"),
        );
        assert_eq!(catalog.records.len(), 1);
        assert_eq!(catalog.records[0].op, TargetOp::Delete);
    }

    #[test]
    fn merge_picks_later_time() {
        let mut a = TargetLifecycleCatalog::default();
        upsert_record(
            &mut a,
            TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 1000, "dev-1"),
        );
        let mut b = TargetLifecycleCatalog::default();
        upsert_record(
            &mut b,
            TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 2000, "dev-2"),
        );
        let merged = merge_catalogs(&[a, b]);
        assert_eq!(merged.records.len(), 1);
        assert_eq!(merged.records[0].updated_at_ms, 2000);
    }

    #[test]
    fn merge_tie_break_by_device_id() {
        let mut a = TargetLifecycleCatalog::default();
        upsert_record(
            &mut a,
            TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 1000, "dev-a"),
        );
        let mut b = TargetLifecycleCatalog::default();
        upsert_record(
            &mut b,
            TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 1000, "dev-b"),
        );
        let merged = merge_catalogs(&[a, b]);
        assert_eq!(merged.records[0].device_id, "dev-b");
    }

    #[test]
    fn merge_delete_uses_deleted_at_ms() {
        let mut a = TargetLifecycleCatalog::default();
        upsert_record(
            &mut a,
            TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 1500, "dev-1"),
        );
        let mut b = TargetLifecycleCatalog::default();
        upsert_record(
            &mut b,
            TargetLifecycleRecord::delete("projects/p1", "projects/p1", 2000, "dev-2"),
        );
        let merged = merge_catalogs(&[a, b]);
        assert_eq!(merged.records[0].op, TargetOp::Delete);
    }

    #[test]
    fn catalog_has_upsert_works() {
        let mut catalog = TargetLifecycleCatalog::default();
        upsert_record(
            &mut catalog,
            TargetLifecycleRecord::delete("projects/p1", "projects/p1", 1000, "dev-1"),
        );
        upsert_record(
            &mut catalog,
            TargetLifecycleRecord::upsert("projects/p2", "projects/p2", 1000, "dev-1"),
        );
        assert!(!catalog_has_upsert(&catalog, "projects/p1"));
        assert!(catalog_has_upsert(&catalog, "projects/p2"));
        assert!(!catalog_has_upsert(&catalog, "projects/p3"));
    }

    #[test]
    fn load_parse_failure_returns_err() {
        let p = MemoryProvider::with_entries([(
            TARGET_CATALOG_REMOTE_PATH.to_string(),
            b"not json".to_vec(),
        )]);
        let result = load_remote_catalog(&p);
        assert!(result.is_err());
    }
}
