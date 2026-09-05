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

use crate::sync::provider::model::WritePrecondition;
use crate::sync::provider::SyncProvider;
use crate::sync::types::{TargetLifecycleCatalog, TargetLifecycleRecord, TargetOp};

/// catalog 在远端的固定路径：app target 的 remote_prefix（`"app"`）下。
///
/// 这个位置不会随 `projects/<id>/` 一起被删除，保证 delete tombstone 持久存在。
pub const TARGET_CATALOG_REMOTE_PATH: &str = "app/app-meta/sync/targets.sync.json";

/// 加载远端 catalog。
///
/// - 远端不存在 catalog → 返回空 catalog（首次同步或所有 target 都未记录）；
/// - 解析失败 → 返回 `Err`（不吞错误，调用方决定 Retry）。
pub fn load_remote_catalog(
    provider: &dyn SyncProvider,
) -> crate::error::Result<TargetLifecycleCatalog> {
    let obj = provider
        .read(TARGET_CATALOG_REMOTE_PATH)
        .map_err(crate::Error::from)?;
    let Some(obj) = obj else {
        return Ok(TargetLifecycleCatalog::default());
    };
    let catalog: TargetLifecycleCatalog = serde_json::from_slice(&obj.content).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "load_remote_catalog: parse {}: {e}",
            TARGET_CATALOG_REMOTE_PATH
        )))
    })?;
    Ok(catalog)
}

/// 写远端 catalog（Unconditional 覆盖）。
///
/// 序列化失败或 provider.write 失败 → `Err`。
pub fn write_remote_catalog(
    provider: &dyn SyncProvider,
    catalog: &TargetLifecycleCatalog,
) -> crate::error::Result<()> {
    let content = serde_json::to_vec(catalog).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "write_remote_catalog: serialize: {e}"
        )))
    })?;
    provider
        .write(
            TARGET_CATALOG_REMOTE_PATH,
            &content,
            WritePrecondition::Unconditional,
        )
        .map_err(crate::Error::from)?;
    Ok(())
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
    fn load_missing_returns_empty() {
        let p = MemoryProvider::new();
        let catalog = load_remote_catalog(&p).unwrap();
        assert!(catalog.records.is_empty());
    }

    #[test]
    fn write_load_roundtrip() {
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
        write_remote_catalog(&p, &catalog).unwrap();

        let loaded = load_remote_catalog(&p).unwrap();
        assert_eq!(loaded, catalog);
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
        // dev-b > dev-a 字典序
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
        // delete 的 lww_time = 2000 > upsert 的 1500
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
