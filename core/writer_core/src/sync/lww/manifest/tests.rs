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
    let snapshot = snapshot_local_records_for_merge(
        sync_root,
        SyncScope::Project,
        "current-device",
        None,
        None,
    )
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

    let snapshot = snapshot_local_records_for_merge(
        sync_root,
        SyncScope::Project,
        "current-device",
        None,
        None,
    )
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

    let snapshot = snapshot_local_records_for_merge(
        sync_root,
        SyncScope::Project,
        "current-device",
        None,
        None,
    )
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

// ── v2 版本化存储（root + 两级索引 + 内容寻址对象）────────────────────────

mod manifest_store {
    use super::super::store::ManifestWritePlan;
    use super::super::SYNC_MANIFEST_PATH;
    use super::super::{is_manifest_rel_path, is_manifest_storage_path, object_key, ManifestStore};
    use crate::sync::manifest_delta::ManifestDelta;
    use crate::sync::types::{ManifestFileRecord, SyncManifest};
    use std::path::{Path, PathBuf};

    const GROUP_MANIFEST: &str = "app-meta/sync/groups/app/manifest.sync.json";

    fn record(path: &str, hash: &str) -> ManifestFileRecord {
        ManifestFileRecord {
            path: path.to_string(),
            content_hash: hash.to_string(),
            updated_at_ms: 1,
            deleted_at_ms: None,
            device_id: "dev".to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        }
    }

    fn commit_plan(dir: &Path, plan: &ManifestWritePlan) {
        let mut tx = crate::storage::transaction::SaveTransaction::new(dir);
        for (rel, bytes) in &plan.writes {
            tx.add_bytes(rel, bytes).unwrap();
        }
        for rel in &plan.deletes {
            tx.add_delete(rel).unwrap();
        }
        tx.commit().unwrap();
        assert_eq!(
            plan.writes.last().map(|(rel, _)| rel.as_str()),
            Some(plan.root_rel.as_str()),
            "root 必须是 writes 的最后一项"
        );
    }

    fn storage_dir(dir: &Path) -> PathBuf {
        dir.join("app-meta/sync/manifest")
    }

    fn find_object(dir: &Path, key: &str) -> PathBuf {
        let objects = storage_dir(dir).join("objects");
        let needle = format!("\"key\":\"{key}\"");
        for entry in std::fs::read_dir(&objects).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            if text.contains(&needle) {
                return path;
            }
        }
        panic!("object for key {key} not found");
    }

    fn object_files(dir: &Path) -> Vec<PathBuf> {
        let objects = storage_dir(dir).join("objects");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&objects)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        files.sort();
        files
    }

    #[test]
    fn object_key_groups_by_business_object() {
        assert_eq!(
            object_key("volumes/v1/chapters/c1/chapter.md"),
            "volumes/v1/chapters/c1"
        );
        assert_eq!(
            object_key("volumes/v1/chapters/c1/notes.md"),
            "volumes/v1/chapters/c1"
        );
        assert_eq!(object_key("volumes/v1/volume.json"), "volumes/v1");
        assert_eq!(object_key("volumes/v1"), "volumes/v1");
        assert_eq!(object_key("starmaps/sm1/graph.json"), "starmaps/sm1");
        assert_eq!(object_key("starmaps/index.json"), "starmaps/index.json");
        assert_eq!(
            object_key("annotations/links/l1.json"),
            "annotations/links/l1.json"
        );
        assert_eq!(object_key("project.json"), "project.json");
        assert_eq!(object_key("settings"), "settings");
    }

    #[test]
    fn manifest_storage_paths_are_recognized() {
        assert!(is_manifest_rel_path(SYNC_MANIFEST_PATH));
        assert!(is_manifest_rel_path(GROUP_MANIFEST));
        assert!(!is_manifest_rel_path("app-meta/sync/manifest/root.json"));
        assert!(is_manifest_storage_path(SYNC_MANIFEST_PATH));
        assert!(is_manifest_storage_path("app-meta/sync/manifest/root.json"));
        assert!(is_manifest_storage_path(
            "app-meta/sync/groups/app/manifest/objects/abc.json"
        ));
        assert!(is_manifest_storage_path(
            "app-meta/sync/manifest-shards/volumes.json"
        ));
        assert!(!is_manifest_storage_path(
            "volumes/v1/chapters/c1/chapter.md"
        ));
    }

    #[test]
    fn legacy_single_file_migrates_to_v2_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let manifest = SyncManifest {
            files: vec![
                record("volumes/v1/chapters/c1/chapter.md", "h1"),
                record("project.json", "h2"),
            ],
        };
        std::fs::create_dir_all(root.join("app-meta/sync")).unwrap();
        std::fs::write(
            root.join(SYNC_MANIFEST_PATH),
            manifest.canonical_json().unwrap(),
        )
        .unwrap();

        let store = ManifestStore::new(root, SYNC_MANIFEST_PATH);
        assert!(store.load_root().unwrap().is_none(), "迁移前无 v2 root");
        let loaded = store.load_or_legacy().unwrap();
        assert!(loaded.semantically_equals(&manifest));

        // 迁移后 root 是权威：索引按 namespace 落盘，旧单文件保留。
        assert!(storage_dir(root).join("root.json").is_file());
        assert!(storage_dir(root).join("index/volumes.json").is_file());
        assert!(storage_dir(root).join("index/_root.json").is_file());
        assert!(root.join(SYNC_MANIFEST_PATH).is_file());
        assert!(store
            .load_all()
            .unwrap()
            .unwrap()
            .semantically_equals(&manifest));

        // root 存在即权威：旧单文件即使被改也不再参与读取。
        let stale = SyncManifest {
            files: vec![record("project.json", "stale")],
        };
        std::fs::write(
            root.join(SYNC_MANIFEST_PATH),
            stale.canonical_json().unwrap(),
        )
        .unwrap();
        let loaded = store.load_or_legacy().unwrap();
        assert!(loaded.semantically_equals(&manifest));
    }

    #[test]
    fn legacy_shards_dir_migrates_to_v2_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let shards = root.join("app-meta/sync/manifest-shards");
        std::fs::create_dir_all(&shards).unwrap();
        let volumes = SyncManifest {
            files: vec![record("volumes/v1/chapters/c1/chapter.md", "h1")],
        };
        let root_shard = SyncManifest {
            files: vec![record("project.json", "h2")],
        };
        std::fs::write(
            shards.join("volumes.json"),
            volumes.canonical_json().unwrap(),
        )
        .unwrap();
        std::fs::write(
            shards.join("_root.json"),
            root_shard.canonical_json().unwrap(),
        )
        .unwrap();

        let store = ManifestStore::new(root, SYNC_MANIFEST_PATH);
        let loaded = store.load_or_legacy().unwrap();
        assert_eq!(loaded.files.len(), 2);
        assert!(store.load_all().unwrap().is_some());
        // 旧分片目录保留，由后续发布端回收。
        assert!(shards.is_dir());
    }

    #[test]
    fn legacy_shards_win_over_stale_single_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("app-meta/sync/manifest-shards")).unwrap();
        // 旧单文件停留在更早一代（只有 project.json 旧 hash）。
        let stale = SyncManifest {
            files: vec![record("project.json", "stale")],
        };
        std::fs::write(
            root.join(SYNC_MANIFEST_PATH),
            stale.canonical_json().unwrap(),
        )
        .unwrap();
        // 分片目录是上一代权威，包含更新的章节记录。
        let latest = SyncManifest {
            files: vec![
                record("project.json", "fresh"),
                record("volumes/v1/chapters/c1/chapter.md", "h1"),
            ],
        };
        std::fs::write(
            root.join("app-meta/sync/manifest-shards/volumes.json"),
            SyncManifest {
                files: vec![record("volumes/v1/chapters/c1/chapter.md", "h1")],
            }
            .canonical_json()
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.join("app-meta/sync/manifest-shards/_root.json"),
            SyncManifest {
                files: vec![record("project.json", "fresh")],
            }
            .canonical_json()
            .unwrap(),
        )
        .unwrap();

        let store = ManifestStore::new(root, SYNC_MANIFEST_PATH);
        let loaded = store.load_or_legacy().unwrap();
        assert!(
            loaded.semantically_equals(&latest),
            "迁移必须优先读取旧分片（上一代权威），而不是更早的旧单文件"
        );
    }

    #[test]
    fn missing_or_corrupt_storage_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let manifest = SyncManifest {
            files: vec![record("volumes/v1/chapters/c1/chapter.md", "h1")],
        };
        let store = ManifestStore::new(root, SYNC_MANIFEST_PATH);
        let plan = store.plan_write_all(&manifest, "g1").unwrap();
        commit_plan(root, &plan);

        // 索引缺失
        let index_path = storage_dir(root).join("index/volumes.json");
        let index_bytes = std::fs::read(&index_path).unwrap();
        std::fs::remove_file(&index_path).unwrap();
        assert!(store.load_all().is_err(), "索引缺失必须报错");
        std::fs::write(&index_path, &index_bytes).unwrap();

        // 索引 sha256 与 root 不符
        std::fs::write(&index_path, b"{\"schemaVersion\":2}").unwrap();
        assert!(store.load_all().is_err(), "索引 hash 不符必须报错");
        std::fs::write(&index_path, &index_bytes).unwrap();

        // 对象缺失
        let objects = object_files(root);
        assert_eq!(objects.len(), 1);
        let object_path = objects[0].clone();
        let object_bytes = std::fs::read(&object_path).unwrap();
        std::fs::remove_file(&object_path).unwrap();
        assert!(store.load_all().is_err(), "对象缺失必须报错");
        std::fs::write(&object_path, &object_bytes).unwrap();

        // 对象内容与其 sha256 文件名不符
        std::fs::write(&object_path, b"{}").unwrap();
        assert!(store.load_all().is_err(), "对象 hash 不符必须报错");
        std::fs::write(&object_path, &object_bytes).unwrap();

        // root 损坏
        let root_path = storage_dir(root).join("root.json");
        let root_bytes = std::fs::read(&root_path).unwrap();
        std::fs::write(&root_path, b"{not json").unwrap();
        assert!(store.load_root().is_err(), "root 损坏必须报错");
        std::fs::write(&root_path, &root_bytes).unwrap();

        // root 缺失 → 不是错误，只是没有权威清单。
        std::fs::remove_file(&root_path).unwrap();
        assert!(store.load_all().unwrap().is_none());
    }

    #[test]
    fn apply_delta_only_rewrites_affected_object_index_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let manifest = SyncManifest {
            files: vec![
                record("volumes/v1/chapters/c1/chapter.md", "h1"),
                record("volumes/v1/chapters/c2/chapter.md", "h2"),
                record("project.json", "h3"),
            ],
        };
        let store = ManifestStore::new(root, SYNC_MANIFEST_PATH);
        commit_plan(root, &store.plan_write_all(&manifest, "g1").unwrap());

        let c1_object = find_object(root, "volumes/v1/chapters/c1");
        let c2_object = find_object(root, "volumes/v1/chapters/c2");
        let root_index = storage_dir(root).join("index/_root.json");
        let c1_before = std::fs::read(&c1_object).unwrap();
        let c2_rel = c2_object
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        assert!(c2_object.is_file());
        let root_index_before = std::fs::read(&root_index).unwrap();
        let root_before = std::fs::read(storage_dir(root).join("root.json")).unwrap();

        let mut delta = ManifestDelta::default();
        delta.upserts.insert(
            "volumes/v1/chapters/c2/chapter.md".to_string(),
            record("volumes/v1/chapters/c2/chapter.md", "h2-new"),
        );
        let plan = store.apply_delta(&delta, "g2").unwrap();
        assert_eq!(plan.writes.len(), 3, "只写受影响对象 + 索引 + root");
        assert!(plan.deletes.contains(&c2_rel), "旧 c2 对象不可达要回收");
        commit_plan(root, &plan);

        assert_eq!(std::fs::read(&c1_object).unwrap(), c1_before);
        assert!(!std::path::Path::new(&c2_object).exists());
        assert_eq!(std::fs::read(&root_index).unwrap(), root_index_before);
        assert_ne!(
            std::fs::read(storage_dir(root).join("root.json")).unwrap(),
            root_before
        );

        let loaded = store.load_all().unwrap().unwrap();
        let c1 = loaded
            .files
            .iter()
            .find(|r| r.path == "volumes/v1/chapters/c1/chapter.md")
            .unwrap();
        assert_eq!(c1.content_hash, "h1");
        let c2 = loaded
            .files
            .iter()
            .find(|r| r.path == "volumes/v1/chapters/c2/chapter.md")
            .unwrap();
        assert_eq!(c2.content_hash, "h2-new");
        assert!(plan.reachable.contains(&plan.root_rel));
    }

    #[test]
    fn deleting_last_record_reclaims_object() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let manifest = SyncManifest {
            files: vec![record("volumes/v1/chapters/c1/chapter.md", "h1")],
        };
        let store = ManifestStore::new(root, SYNC_MANIFEST_PATH);
        commit_plan(root, &store.plan_write_all(&manifest, "g1").unwrap());
        let object_path = find_object(root, "volumes/v1/chapters/c1");
        let object_rel = object_path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");

        let mut delta = ManifestDelta::default();
        delta
            .deletes
            .insert("volumes/v1/chapters/c1/chapter.md".to_string());
        let plan = store.apply_delta(&delta, "g2").unwrap();
        assert!(plan.deletes.contains(&object_rel), "空 objects 文件要回收");
        assert!(
            plan.deletes
                .contains(&"app-meta/sync/manifest/index/volumes.json".to_string()),
            "空 namespace 的索引要回收"
        );
        assert!(
            !plan.writes.iter().any(|(rel, _)| rel.contains("/objects/")),
            "空对象不写入"
        );
        commit_plan(root, &plan);

        assert!(!object_path.exists());
        assert!(!storage_dir(root).join("index/volumes.json").exists());
        let loaded = store.load_all().unwrap().unwrap();
        assert!(loaded.files.is_empty());
    }

    #[test]
    fn empty_manifest_round_trips_through_root_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let store = ManifestStore::new(root, SYNC_MANIFEST_PATH);
        assert!(store.load_all().unwrap().is_none());

        let plan = store
            .plan_write_all(&SyncManifest::default(), "empty")
            .unwrap();
        assert_eq!(plan.writes.len(), 1);
        assert_eq!(plan.writes[0].0, plan.root_rel);
        assert!(plan.reachable.contains(&plan.root_rel));
        commit_plan(root, &plan);

        let loaded = store.load_all().unwrap().unwrap();
        assert!(loaded.files.is_empty());
        let (root_bytes, reachable) = store.load_reachable().unwrap().unwrap();
        assert!(!root_bytes.is_empty());
        assert_eq!(reachable.len(), 1);
    }

    #[test]
    fn app_group_manifest_uses_group_scoped_store() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let store = ManifestStore::new(root, GROUP_MANIFEST);
        let manifest = SyncManifest {
            files: vec![record("settings/app.json", "h1")],
        };
        commit_plan(root, &store.plan_write_all(&manifest, "g1").unwrap());

        assert!(root
            .join("app-meta/sync/groups/app/manifest/root.json")
            .is_file());
        assert!(root
            .join("app-meta/sync/groups/app/manifest/index/settings.json")
            .is_file());
        // 项目级存储不受影响。
        assert!(!root.join("app-meta/sync/manifest").exists());
        assert!(store
            .load_all()
            .unwrap()
            .unwrap()
            .semantically_equals(&manifest));
    }
}

/// v2：App group 还没有自己的权威 root 时，仍允许只读回退聚合旧单文件
/// （v1 迁移输入），且回退不创建 group 自己的 v2 存储。
#[test]
fn app_group_read_only_falls_back_to_aggregate_legacy_manifest() {
    use crate::sync::types::{AppDataGroup, SyncScope};
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let sync_root = tmp.path();
    let content = b"{\"theme\":\"x\"}";
    std::fs::write(sync_root.join("settings.sync.json"), content).unwrap();
    let content_hash = format!("{:x}", md5::compute(content));
    let aggregate = SyncManifest {
        files: vec![ManifestFileRecord {
            path: "settings.sync.json".to_string(),
            content_hash: content_hash.clone(),
            updated_at_ms: 1000,
            deleted_at_ms: None,
            device_id: "aggregate-device".to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        }],
    };
    std::fs::create_dir_all(sync_root.join("app-meta/sync")).unwrap();
    std::fs::write(
        sync_root.join(SYNC_MANIFEST_PATH),
        aggregate.canonical_json().unwrap(),
    )
    .unwrap();

    let group = AppDataGroup::Global;
    assert!(!sync_root.join(group.local_manifest_path()).exists());
    let records =
        snapshot_app_group_records_read_only(sync_root, &group, "current-device").unwrap();
    let record = records
        .get("settings.sync.json")
        .expect("聚合旧清单记录应被只读回退采用");
    assert_eq!(record.content_hash, content_hash);
    assert_eq!(record.device_id, "aggregate-device");
    // 只读回退不写 group root；聚合旧单文件也保留。
    assert!(!sync_root
        .join("app-meta/sync/groups/global/manifest/root.json")
        .exists());
    assert!(sync_root.join(SYNC_MANIFEST_PATH).is_file());

    // 项目级读取同一份聚合旧清单时会迁移成 v2 root（root 成为权威）。
    snapshot_local_records_read_only(sync_root, SyncScope::Project, "current-device").unwrap();
    assert!(sync_root.join("app-meta/sync/manifest/root.json").is_file());
}
