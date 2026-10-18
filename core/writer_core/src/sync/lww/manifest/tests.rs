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

/// #931：known_files 中记录的文件磁盘缺失且无 tombstone → 宽松版不再整体 Err，
/// 而是收集到 missing_without_delete_fact；严格版保持旧 Err 语义与文案。
#[test]
fn test_snapshot_local_records_for_merge_collects_missing_known_file() {
    use crate::sync::types::{SyncScope, SyncState};
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let sync_root = tmp.path();

    // project.json 不在磁盘，但 state.known_files 与旧 manifest 都记录它。
    let known_hash = format!("{:x}", md5::compute(b"missing-content"));
    let mut state = SyncState::default();
    state.device_id = "current-device".to_string();
    state
        .known_files
        .insert("project.json".to_string(), known_hash.clone());
    state
        .known_files_updated_at
        .insert("project.json".to_string(), 1500);
    crate::sync::SyncService::save_sync_state(sync_root, &state).unwrap();

    let manifest = crate::sync::types::SyncManifest {
        files: vec![ManifestFileRecord {
            path: "project.json".to_string(),
            content_hash: known_hash.clone(),
            op: "upsert".to_string(),
            updated_at_ms: 1500,
            deleted_at_ms: None,
            device_id: "winner-device".to_string(),
            schema_version: 1,
        }],
    };
    std::fs::create_dir_all(sync_root.join("app-meta/sync")).unwrap();
    std::fs::write(
        sync_root.join(SYNC_MANIFEST_PATH),
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();

    // 宽松版：不 Err，收集缺失文件并保留恢复所需证据。
    let snapshot =
        snapshot_local_records_for_merge(sync_root, SyncScope::Project, "current-device", None)
            .unwrap();
    assert!(snapshot.records.is_empty());
    assert_eq!(snapshot.missing_without_delete_fact.len(), 1);
    let missing = &snapshot.missing_without_delete_fact[0];
    assert_eq!(missing.path, "project.json");
    assert_eq!(missing.known_hash, known_hash);
    assert_eq!(missing.cause, MissingCause::KnownFile);
    // 旧 manifest 的真实 LWW 元数据保留，供恢复时复用（不伪造新时间）。
    let old_rec = missing.manifest_record.as_ref().unwrap();
    assert_eq!(old_rec.updated_at_ms, 1500);
    assert_eq!(old_rec.device_id, "winner-device");

    // 严格版：仍返回 Err，且错误文案与旧行为一致。
    let err = snapshot_local_records_read_only(sync_root, SyncScope::Project, "current-device")
        .unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("known file project.json missing on disk without tombstone"),
        "严格版错误文案应保持不变，实际: {msg}"
    );
}

/// #931：仅旧 manifest 中 upsert 记录存在、磁盘缺失且无 tombstone →
/// 宽松版收集为 OldManifestUpsert；严格版保持旧 Err 文案。
#[test]
fn test_snapshot_local_records_for_merge_collects_missing_old_manifest_upsert() {
    use crate::sync::types::{SyncScope, SyncState};
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let sync_root = tmp.path();

    // state 无 known_files，仅旧 manifest 记录 project.json。
    let mut state = SyncState::default();
    state.device_id = "current-device".to_string();
    crate::sync::SyncService::save_sync_state(sync_root, &state).unwrap();

    let manifest = crate::sync::types::SyncManifest {
        files: vec![ManifestFileRecord {
            path: "project.json".to_string(),
            content_hash: format!("{:x}", md5::compute(b"old-manifest-content")),
            op: "upsert".to_string(),
            updated_at_ms: 1200,
            deleted_at_ms: None,
            device_id: "old-device".to_string(),
            schema_version: 1,
        }],
    };
    std::fs::create_dir_all(sync_root.join("app-meta/sync")).unwrap();
    std::fs::write(
        sync_root.join(SYNC_MANIFEST_PATH),
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let snapshot =
        snapshot_local_records_for_merge(sync_root, SyncScope::Project, "current-device", None)
            .unwrap();
    assert_eq!(snapshot.missing_without_delete_fact.len(), 1);
    assert_eq!(
        snapshot.missing_without_delete_fact[0].cause,
        MissingCause::OldManifestUpsert
    );
    assert_eq!(snapshot.records.len(), 0);

    let err = snapshot_local_records_read_only(sync_root, SyncScope::Project, "current-device")
        .unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("old manifest upsert for project.json but file missing without tombstone"),
        "严格版错误文案应保持不变，实际: {msg}"
    );
}

/// #931：真实 tombstone 覆盖的缺失文件仍生成 delete record（不进缺失列表），
/// 宽松版与严格版行为一致。
#[test]
fn test_snapshot_local_records_for_merge_tombstone_still_produces_delete_record() {
    use crate::sync::types::{SyncScope, SyncState, Tombstone};
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let sync_root = tmp.path();

    let known_hash = format!("{:x}", md5::compute(b"deleted-content"));
    let mut state = SyncState::default();
    state.device_id = "current-device".to_string();
    state
        .known_files
        .insert("project.json".to_string(), known_hash.clone());
    state
        .known_files_updated_at
        .insert("project.json".to_string(), 800);
    state.tombstones.push(Tombstone {
        original_path: "project.json".to_string(),
        trash_path: "app-meta/sync/trash/999_project.json".to_string(),
        deleted_at: 3_000,
        purge_after: i64::MAX,
        deleted_by: "delete-device".to_string(),
        original_hash: known_hash,
        kind: "local_delete".to_string(),
    });
    crate::sync::SyncService::save_sync_state(sync_root, &state).unwrap();

    let snapshot =
        snapshot_local_records_for_merge(sync_root, SyncScope::Project, "current-device", None)
            .unwrap();
    assert!(snapshot.missing_without_delete_fact.is_empty());
    let rec = snapshot.records.get("project.json").unwrap();
    assert_eq!(rec.op, "delete");
    // tombstone.deleted_at 是秒，record 用毫秒。
    assert_eq!(rec.deleted_at_ms, Some(3_000_000));
    assert_eq!(rec.device_id, "delete-device");

    // 严格版同样 Ok，不进任何缺失分支。
    let strict =
        snapshot_local_records_read_only(sync_root, SyncScope::Project, "current-device").unwrap();
    assert_eq!(strict.get("project.json").unwrap().op, "delete");
}
