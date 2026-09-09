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

///   snapshot_local_records_read_only 基本测试。
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
        snapshot_local_records_read_only(sync_root, SyncScope::Project, "current-device").unwrap();
    let rec = records.get("project.json").unwrap();
    // 保留原 winner 的 device_id，不写 currentA当前设备。
    assert_eq!(rec.device_id, "winner-device");
}
