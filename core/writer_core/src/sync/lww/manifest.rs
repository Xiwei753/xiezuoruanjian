//! #644 评论 5462823517 第3节：LWW manifest/state/local record 构造。
//!
//! 从 lww.rs 抽出的 manifest 相关：SYNC_MANIFEST_PATH 常量、lww_record_time。

use crate::sync::types::ManifestFileRecord;

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
