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
    use super::super::{
        is_manifest_rel_path, is_manifest_storage_path, object_key, sha256_hex, ManifestRoot,
        ManifestStore,
    };
    use crate::sync::manifest_delta::ManifestDelta;
    use crate::sync::types::{ManifestFileRecord, SyncManifest};
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    const GROUP_MANIFEST: &str = "app-meta/sync/groups/app/manifest.sync.json";
    const STORAGE: &str = "app-meta/sync/manifest";
    const GROUP_STORAGE: &str = "app-meta/sync/groups/app/manifest";

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
        let root_written = plan.writes.iter().any(|(rel, _)| rel == &plan.root_rel);
        if root_written {
            assert_eq!(
                plan.writes.last().map(|(rel, _)| rel.as_str()),
                Some(plan.root_rel.as_str()),
                "写根时 root 必须是 writes 的最后一项"
            );
        } else {
            assert!(
                !plan.writes.iter().any(|(rel, _)| rel == &plan.root_rel),
                "清单语义未变化时不得写 root"
            );
        }
    }

    fn storage_dir(dir: &Path) -> PathBuf {
        dir.join(STORAGE)
    }

    fn root_bytes(dir: &Path, storage: &str) -> Vec<u8> {
        std::fs::read(dir.join(format!("{storage}/root.json"))).unwrap()
    }

    /// root 记录的 namespace → index sha256。
    fn root_namespaces(dir: &Path, storage: &str) -> BTreeMap<String, String> {
        let root: ManifestRoot = serde_json::from_slice(&root_bytes(dir, storage)).unwrap();
        root.namespaces
    }

    /// 内容寻址索引相对路径：`<storage>/index/<ns>/<sha256>.json`。
    fn index_rel(dir: &Path, storage: &str, namespace: &str) -> String {
        let sha = root_namespaces(dir, storage)
            .remove(namespace)
            .unwrap_or_else(|| panic!("root 缺少 namespace {namespace}"));
        format!("{storage}/index/{namespace}/{sha}.json")
    }

    fn index_path(dir: &Path, namespace: &str) -> PathBuf {
        dir.join(index_rel(dir, STORAGE, namespace))
    }

    fn legacy_index_path(dir: &Path, storage: &str, namespace: &str) -> PathBuf {
        dir.join(format!("{storage}/index/{namespace}.json"))
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

        // 迁移后 root 是权威：索引按 namespace 落到内容寻址路径，旧单文件保留。
        assert!(storage_dir(root).join("root.json").is_file());
        let volumes_index = index_path(root, "volumes");
        let root_index = index_path(root, "_root");
        assert!(volumes_index.is_file());
        assert!(root_index.is_file());
        assert!(!legacy_index_path(root, STORAGE, "volumes").exists());
        assert!(!legacy_index_path(root, STORAGE, "_root").exists());
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

        // 索引缺失（内容寻址路径与旧固定路径都不存在）
        let index_path = index_path(root, "volumes");
        let index_bytes = std::fs::read(&index_path).unwrap();
        std::fs::remove_file(&index_path).unwrap();
        assert!(store.load_all().is_err(), "索引缺失必须报错");
        std::fs::write(&index_path, &index_bytes).unwrap();

        // 索引 sha256 与 root 不符
        std::fs::write(&index_path, b"{\"schemaVersion\":2}").unwrap();
        let err = store.load_all().unwrap_err();
        assert!(
            format!("{err}").contains("sha256 mismatch"),
            "索引 hash 不符必须报错，实际 {err}"
        );
        std::fs::write(&index_path, &index_bytes).unwrap();

        // 内容寻址缺失时回退旧固定路径；固定路径内容 sha 不符同样报错。
        std::fs::rename(&index_path, legacy_index_path(root, STORAGE, "volumes")).unwrap();
        assert!(store.load_all().is_ok(), "旧固定路径索引应可迁移读取");
        let legacy_path = legacy_index_path(root, STORAGE, "volumes");
        std::fs::write(&legacy_path, b"{\"schemaVersion\":2}").unwrap();
        let err = store.load_all().unwrap_err();
        let message = format!("{err}");
        assert!(
            message.contains("sha256 mismatch") && message.contains("volumes"),
            "固定路径 sha 不符应报错并带 namespace，实际 {message}"
        );
        assert!(
            message.contains(&legacy_path.display().to_string()),
            "错误消息应带读取路径，实际 {message}"
        );
        std::fs::write(&legacy_path, &index_bytes).unwrap();

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

        // root 记录的 sha 在两种布局都找不到：错误消息带 namespace、sha、路径。
        std::fs::remove_file(&legacy_path).unwrap();
        let err = store.load_all().unwrap_err();
        let message = format!("{err}");
        let root_ns = root_namespaces(root, STORAGE);
        let expected_sha = root_ns.get("volumes").unwrap();
        assert!(
            message.contains("missing")
                && message.contains("volumes")
                && message.contains(expected_sha),
            "索引缺失错误应带 namespace 与 sha，实际 {message}"
        );
        assert!(
            message.contains(&format!("index/volumes/{expected_sha}.json")),
            "索引缺失错误应带内容寻址路径，实际 {message}"
        );
        std::fs::write(&index_path, &index_bytes).unwrap();

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
    fn index_is_content_addressed_and_immutable() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let manifest = SyncManifest {
            files: vec![
                record("volumes/v1/chapters/c1/chapter.md", "h1"),
                record("project.json", "h2"),
            ],
        };
        let store = ManifestStore::new(root, SYNC_MANIFEST_PATH);
        commit_plan(root, &store.plan_write_all(&manifest, "g1").unwrap());

        for namespace in ["volumes", "_root"] {
            let rel = index_rel(root, STORAGE, namespace);
            let bytes = std::fs::read(root.join(&rel)).unwrap();
            let file_sha = rel
                .strip_suffix(".json")
                .unwrap()
                .rsplit('/')
                .next()
                .unwrap();
            assert_eq!(
                sha256_hex(&bytes),
                file_sha,
                "索引文件名必须等于内容 canonical bytes 的 sha256"
            );
            assert_eq!(
                root_namespaces(root, STORAGE)
                    .get(namespace)
                    .map(String::as_str),
                Some(file_sha),
                "root 记录的 sha 必须等于索引文件名"
            );
            assert!(
                !legacy_index_path(root, STORAGE, namespace).exists(),
                "新布局不再写旧固定路径"
            );
        }

        // 相同清单再计划：根映射未变化 → 不写根，索引/对象同 sha 复用。
        let root_before = root_bytes(root, STORAGE);
        let plan = store
            .plan_write_all(&manifest, "ignored-generation")
            .unwrap();
        assert!(plan.writes.is_empty(), "语义未变化时不得重写任何清单文件");
        assert!(plan.deletes.is_empty(), "未推进版本时不得回收文件");
        commit_plan(root, &plan);
        assert_eq!(
            root_bytes(root, STORAGE),
            root_before,
            "空计划不得改变 root 字节与 generation"
        );
        assert!(store
            .load_all()
            .unwrap()
            .unwrap()
            .semantically_equals(&manifest));
    }

    #[test]
    fn legacy_fixed_index_is_read_then_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let manifest = SyncManifest {
            files: vec![record("volumes/v1/chapters/c1/chapter.md", "h1")],
        };
        let store = ManifestStore::new(root, SYNC_MANIFEST_PATH);
        commit_plan(root, &store.plan_write_all(&manifest, "g1").unwrap());

        // 模拟 87b15f2 旧布局：索引只存在于固定路径，内容寻址路径缺失。
        let hashed_rel = index_rel(root, STORAGE, "volumes");
        let hashed_path = root.join(&hashed_rel);
        let legacy_path = legacy_index_path(root, STORAGE, "volumes");
        std::fs::rename(&hashed_path, &legacy_path).unwrap();
        assert!(!hashed_path.exists());
        assert!(
            store
                .load_all()
                .unwrap()
                .unwrap()
                .semantically_equals(&manifest),
            "迁移前必须能回退读取旧固定路径索引"
        );

        // 下一次写入计划把同一 sha 补写到内容寻址路径：这是幂等修复事实，
        // 不推进根版本，也不回收旧固定路径（deletes 为空）。
        let root_before = root_bytes(root, STORAGE);
        let mut delta = ManifestDelta::default();
        delta.upserts.insert(
            "volumes/v1/chapters/c1/chapter.md".to_string(),
            record("volumes/v1/chapters/c1/chapter.md", "h1"),
        );
        let plan = store.apply_delta(&delta, "g2").unwrap();
        assert!(
            plan.writes.iter().any(|(rel, _)| rel == &hashed_rel),
            "迁移计划必须写内容寻址路径: {:?}",
            plan.writes.iter().map(|(rel, _)| rel).collect::<Vec<_>>()
        );
        assert!(
            !plan.writes.iter().any(|(rel, _)| rel == &plan.root_rel),
            "迁移旧固定路径索引不推进根版本"
        );
        assert!(plan.deletes.is_empty(), "不推进版本时不得回收任何文件");
        commit_plan(root, &plan);

        assert!(hashed_path.is_file(), "迁移后新路径必须存在");
        assert!(legacy_path.exists(), "无版本推进的幂等迁移不回收旧固定路径");
        assert_eq!(
            root_bytes(root, STORAGE),
            root_before,
            "迁移不得改变 root 字节与 generation"
        );
        assert_eq!(index_rel(root, STORAGE, "volumes"), hashed_rel);
        assert!(store
            .load_all()
            .unwrap()
            .unwrap()
            .semantically_equals(&manifest));

        // 真实变化推进根版本后，旧固定路径才进入上一代保留边界结算。
        let mut delta = ManifestDelta::default();
        delta.upserts.insert(
            "volumes/v1/chapters/c1/chapter.md".to_string(),
            record("volumes/v1/chapters/c1/chapter.md", "h2"),
        );
        let plan = store.apply_delta(&delta, "ignored").unwrap();
        assert!(
            plan.writes.iter().any(|(rel, _)| rel == &plan.root_rel),
            "真实变化必须推进根版本"
        );
        commit_plan(root, &plan);
        assert_ne!(
            root_bytes(root, STORAGE),
            root_before,
            "真实变化必须改写 root"
        );
    }

    #[test]
    fn legacy_fixed_index_with_changed_sha_is_retained_one_generation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let manifest = SyncManifest {
            files: vec![record("volumes/v1/chapters/c1/chapter.md", "h1")],
        };
        let store = ManifestStore::new(root, SYNC_MANIFEST_PATH);
        commit_plan(root, &store.plan_write_all(&manifest, "g1").unwrap());

        // 模拟旧布局：R1 的 volumes 索引只存在于固定路径（sha = S1）。
        let fixed = legacy_index_path(root, STORAGE, "volumes");
        std::fs::rename(index_path(root, "volumes"), &fixed).unwrap();

        // R2 索引换成 S2：固定路径仍是上一代根的唯一副本，必须保留一代。
        let mut delta = ManifestDelta::default();
        delta.upserts.insert(
            "volumes/v1/chapters/c1/chapter.md".to_string(),
            record("volumes/v1/chapters/c1/chapter.md", "h2"),
        );
        let plan = store.apply_delta(&delta, "g2").unwrap();
        let fixed_rel = format!("{STORAGE}/index/volumes.json");
        let new_index_rel = plan
            .writes
            .iter()
            .map(|(rel, _)| rel.clone())
            .find(|rel| rel.starts_with(&format!("{STORAGE}/index/volumes/")))
            .expect("R2 应写新的内容寻址索引");
        assert!(
            !plan.deletes.contains(&fixed_rel),
            "索引换 sha 时旧固定路径仍是上一代根依赖，不能立即回收: {:?}",
            plan.deletes
        );
        commit_plan(root, &plan);
        assert!(fixed.is_file(), "R2 发布后旧固定路径索引必须保留");
        assert!(root.join(&new_index_rel).is_file());
        assert!(store
            .load_all()
            .unwrap()
            .unwrap()
            .files
            .iter()
            .any(|r| r.path == "volumes/v1/chapters/c1/chapter.md" && r.content_hash == "h2"));

        // R3：真实变化把上一代根换成 S2，固定路径的 S1 不再被需要，可以回收。
        let mut delta = ManifestDelta::default();
        delta.upserts.insert(
            "volumes/v1/chapters/c1/chapter.md".to_string(),
            record("volumes/v1/chapters/c1/chapter.md", "h3"),
        );
        let plan = store.apply_delta(&delta, "g3").unwrap();
        assert!(
            plan.writes.iter().any(|(rel, _)| rel == &plan.root_rel),
            "R3 必须推进根版本"
        );
        assert!(
            plan.deletes.contains(&fixed_rel),
            "R3 后旧固定路径可回收: {:?}",
            plan.deletes
        );
        commit_plan(root, &plan);
        assert!(!fixed.exists());
    }

    #[test]
    fn corrupt_index_json_is_an_error_with_namespace_and_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let corrupt = b"{not json";
        let corrupt_sha = sha256_hex(corrupt);
        let mut namespaces = BTreeMap::new();
        namespaces.insert("volumes".to_string(), corrupt_sha.clone());
        let root_value = ManifestRoot::local(namespaces, None);
        std::fs::create_dir_all(storage_dir(root).join("index/volumes")).unwrap();
        std::fs::write(
            storage_dir(root)
                .join("index/volumes")
                .join(format!("{corrupt_sha}.json")),
            corrupt,
        )
        .unwrap();
        std::fs::write(
            storage_dir(root).join("root.json"),
            root_value.canonical_bytes().unwrap(),
        )
        .unwrap();

        let store = ManifestStore::new(root, SYNC_MANIFEST_PATH);
        let err = store.load_all().unwrap_err();
        let message = format!("{err}");
        assert!(
            message.contains("corrupt") && message.contains("volumes"),
            "损坏索引应报错并带 namespace，实际 {message}"
        );
        assert!(
            message.contains(&corrupt_sha),
            "损坏索引错误应带 sha，实际 {message}"
        );
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
        let root_index = index_path(root, "_root");
        let volumes_index_r1 = index_path(root, "volumes");
        let c1_before = std::fs::read(&c1_object).unwrap();
        let c2_rel = c2_object
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        assert!(c2_object.is_file());
        let root_index_before = std::fs::read(&root_index).unwrap();
        let volumes_index_r1_bytes = std::fs::read(&volumes_index_r1).unwrap();
        let root_before = std::fs::read(storage_dir(root).join("root.json")).unwrap();

        let mut delta = ManifestDelta::default();
        delta.upserts.insert(
            "volumes/v1/chapters/c2/chapter.md".to_string(),
            record("volumes/v1/chapters/c2/chapter.md", "h2-new"),
        );
        let plan = store.apply_delta(&delta, "g2").unwrap();
        assert_eq!(plan.writes.len(), 3, "只写受影响对象 + 索引 + root");
        assert!(
            plan.deletes.is_empty(),
            "上一代根仍可达的对象/索引必须保留一代: {:?}",
            plan.deletes
        );
        commit_plan(root, &plan);

        // 旧 c2 对象/索引仍被上一代根引用 → 根切换后仍存在。
        assert_eq!(std::fs::read(&c1_object).unwrap(), c1_before);
        assert!(c2_object.is_file(), "R1 独有对象在 R2 发布后必须保留");
        assert!(
            volumes_index_r1.is_file(),
            "R1 独有索引在 R2 发布后必须保留"
        );
        assert_eq!(
            std::fs::read(&volumes_index_r1).unwrap(),
            volumes_index_r1_bytes
        );
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

        // R3：真实变化推进根版本，上一代边界推进到 R2，R1 独有对象/索引才可回收。
        let mut delta = ManifestDelta::default();
        delta.upserts.insert(
            "volumes/v1/chapters/c1/chapter.md".to_string(),
            record("volumes/v1/chapters/c1/chapter.md", "h1-new"),
        );
        let plan_r3 = store.apply_delta(&delta, "g3").unwrap();
        assert!(
            plan_r3.deletes.contains(&c2_rel),
            "R3 后 R1 独有对象可回收: {:?}",
            plan_r3.deletes
        );
        let volumes_index_r1_rel = volumes_index_r1
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        assert!(
            plan_r3.deletes.contains(&volumes_index_r1_rel),
            "R3 后 R1 独有索引可回收: {:?}",
            plan_r3.deletes
        );
        commit_plan(root, &plan_r3);
        assert!(!c2_object.exists());
        assert!(!volumes_index_r1.exists());
        assert_eq!(std::fs::read(&c1_object).unwrap(), c1_before);
    }

    #[test]
    fn deleting_last_record_reclaims_object_one_generation_later() {
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
        let volumes_index = index_path(root, "volumes");

        let mut delta = ManifestDelta::default();
        delta
            .deletes
            .insert("volumes/v1/chapters/c1/chapter.md".to_string());
        let plan = store.apply_delta(&delta, "g2").unwrap();
        assert!(
            plan.deletes.is_empty(),
            "R1 可达对象/索引在 R2 发布后仍保留一代: {:?}",
            plan.deletes
        );
        assert!(
            !plan.writes.iter().any(|(rel, _)| rel.contains("/objects/")),
            "空对象不写入"
        );
        commit_plan(root, &plan);

        assert!(object_path.is_file(), "R1 独有对象在 R2 后必须保留");
        assert!(volumes_index.is_file(), "R1 独有索引在 R2 后必须保留");
        assert!(store.load_all().unwrap().unwrap().files.is_empty());

        // R3：真实变化推进根版本；R1 独有且 R2 不引用的文件才可回收。
        let mut delta = ManifestDelta::default();
        delta.upserts.insert(
            "project.json".to_string(),
            record("project.json", "h-project"),
        );
        let plan_r3 = store.apply_delta(&delta, "g3").unwrap();
        assert!(
            plan_r3
                .writes
                .iter()
                .any(|(rel, _)| rel == &plan_r3.root_rel),
            "R3 必须推进根版本"
        );
        assert!(plan_r3.deletes.contains(&object_rel), "R3 回收旧对象");
        let volumes_index_rel = volumes_index
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        assert!(
            plan_r3.deletes.contains(&volumes_index_rel),
            "R3 回收空 namespace 的旧索引"
        );
        commit_plan(root, &plan_r3);

        assert!(!object_path.exists());
        assert!(!volumes_index.exists());
        let loaded = store.load_all().unwrap().unwrap();
        assert_eq!(
            loaded
                .files
                .iter()
                .map(|record| record.path.as_str())
                .collect::<Vec<_>>(),
            vec!["project.json"],
            "R3 只保留本轮真实变化的记录"
        );
    }

    #[test]
    fn empty_delta_keeps_root_bytes_and_generation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let manifest = SyncManifest {
            files: vec![
                record("volumes/v1/chapters/c1/chapter.md", "h1"),
                record("project.json", "h2"),
            ],
        };
        let store = ManifestStore::new(root, SYNC_MANIFEST_PATH);
        commit_plan(root, &store.plan_write_all(&manifest, "g1").unwrap());
        let root_before = root_bytes(root, STORAGE);
        let generation_before: ManifestRoot = serde_json::from_slice(&root_before).unwrap();

        let plan = store
            .apply_delta(&ManifestDelta::default(), "ignored")
            .unwrap();
        assert!(plan.writes.is_empty(), "空增量不得产生任何清单写入");
        assert!(plan.deletes.is_empty(), "空增量不得回收任何文件");
        commit_plan(root, &plan);

        let root_after = root_bytes(root, STORAGE);
        let generation_after: ManifestRoot = serde_json::from_slice(&root_after).unwrap();
        assert_eq!(root_after, root_before, "空增量不得改变 root 字节");
        assert_eq!(
            generation_after.generation, generation_before.generation,
            "空增量不得改变 generation"
        );
    }

    #[test]
    fn semantically_identical_delta_does_not_rewrite_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let manifest = SyncManifest {
            files: vec![record("volumes/v1/chapters/c1/chapter.md", "h1")],
        };
        let store = ManifestStore::new(root, SYNC_MANIFEST_PATH);
        commit_plan(root, &store.plan_write_all(&manifest, "g1").unwrap());
        let root_before = root_bytes(root, STORAGE);

        // 相同 records 重新提交：索引 sha 不变，不写根。
        let mut delta = ManifestDelta::default();
        delta.upserts.insert(
            "volumes/v1/chapters/c1/chapter.md".to_string(),
            record("volumes/v1/chapters/c1/chapter.md", "h1"),
        );
        let plan = store.apply_delta(&delta, "ignored").unwrap();
        assert!(plan.writes.is_empty(), "语义相同 delta 不得重写清单");
        assert!(plan.deletes.is_empty());
        commit_plan(root, &plan);
        assert_eq!(root_bytes(root, STORAGE), root_before);
    }

    #[test]
    fn local_root_lineage_uses_local_previous_root_not_previous_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let store = ManifestStore::new(root, SYNC_MANIFEST_PATH);
        let manifest = SyncManifest {
            files: vec![record("volumes/v1/chapters/c1/chapter.md", "h1")],
        };
        commit_plan(root, &store.plan_write_all(&manifest, "g1").unwrap());
        let r1_bytes = root_bytes(root, STORAGE);
        let r1: ManifestRoot = serde_json::from_slice(&r1_bytes).unwrap();
        assert!(r1.previous_root.is_none(), "本地根不得自称已发布上一代");
        assert!(r1.local_previous_root.is_none(), "首次本地根没有本地前代");
        let r1_sha = sha256_hex(&r1_bytes);

        let mut delta = ManifestDelta::default();
        delta.upserts.insert(
            "volumes/v1/chapters/c1/chapter.md".to_string(),
            record("volumes/v1/chapters/c1/chapter.md", "h2"),
        );
        let plan = store.apply_delta(&delta, "g2").unwrap();
        let root_write = plan
            .writes
            .iter()
            .find(|(rel, _)| rel == &plan.root_rel)
            .map(|(_, bytes)| bytes.clone())
            .expect("真实变化必须写根");
        let planned: ManifestRoot = serde_json::from_slice(&root_write).unwrap();
        assert!(planned.previous_root.is_none());
        assert_eq!(
            planned.local_previous_root.as_deref(),
            Some(r1_sha.as_str()),
            "localPreviousRoot 必须是写盘前当前本地根的 canonical sha256"
        );
        assert_eq!(
            planned.generation,
            ManifestRoot::content_generation(&planned.namespaces, Some(&r1_sha)),
            "generation 必须由 (namespaces, localPreviousRoot) 内容派生"
        );
        let planned_json: serde_json::Value = serde_json::from_slice(&root_write).unwrap();
        assert!(
            planned_json.get("previousRoot").is_none(),
            "本地根序列化不得包含 previousRoot"
        );
        assert_eq!(
            planned_json["localPreviousRoot"].as_str(),
            Some(r1_sha.as_str()),
            "本地前代字段名必须是 camelCase 的 localPreviousRoot"
        );

        // 同一 base + 同一变化重复计算：逐字节一致（连续两次得到同一 generation）。
        let plan_again = store.apply_delta(&delta, "g3").unwrap();
        let root_write_again = plan_again
            .writes
            .iter()
            .find(|(rel, _)| rel == &plan_again.root_rel)
            .map(|(_, bytes)| bytes.clone())
            .expect("真实变化必须写根");
        assert_eq!(
            root_write_again, root_write,
            "相同内容变化必须得到逐字节一致的 root/generation"
        );

        commit_plan(root, &plan);
        let r2_bytes = root_bytes(root, STORAGE);
        assert_ne!(r2_bytes, r1_bytes);
    }

    #[test]
    fn content_generation_depends_on_namespaces_and_local_previous_root() {
        let namespaces = BTreeMap::from([
            ("_root".to_string(), sha256_hex(b"root-index")),
            ("volumes".to_string(), sha256_hex(b"volumes-index")),
        ]);
        let generation = ManifestRoot::content_generation(&namespaces, None);
        assert!(generation.starts_with("manifest-"));
        assert_eq!(
            generation,
            ManifestRoot::content_generation(&namespaces, None),
            "同内容必须得到同一 generation"
        );

        let local_previous = sha256_hex(b"previous-local-root");
        assert_ne!(
            generation,
            ManifestRoot::content_generation(&namespaces, Some(&local_previous)),
            "本地前代不同则 generation 不同"
        );

        let mut other = namespaces.clone();
        other.insert("settings".to_string(), sha256_hex(b"settings-index"));
        assert_ne!(
            generation,
            ManifestRoot::content_generation(&other, None),
            "不同 namespaces 必须得到不同 generation"
        );
    }

    #[test]
    fn published_root_records_remote_base_and_strips_local_lineage() {
        let namespaces = BTreeMap::from([("volumes".to_string(), sha256_hex(b"volumes-index"))]);
        let base_sha = sha256_hex(b"remote-base-root");
        let published = ManifestRoot::published(namespaces.clone(), Some(&base_sha));
        assert_eq!(
            published.previous_root.as_deref(),
            Some(base_sha.as_str()),
            "previousRoot 必须记录本轮远端 base 根 canonical sha256"
        );
        assert!(
            published.local_previous_root.is_none(),
            "发布视图不得携带本地血缘"
        );
        assert_eq!(
            published.generation,
            ManifestRoot::content_generation(&namespaces, None)
        );
        let json: serde_json::Value = serde_json::to_value(&published).unwrap();
        assert_eq!(json["previousRoot"].as_str(), Some(base_sha.as_str()));
        assert!(json.get("localPreviousRoot").is_none());

        let mut invalid = published.clone();
        invalid.local_previous_root = Some("not-a-sha256".to_string());
        let err = invalid.validate("test-root").unwrap_err();
        assert!(
            format!("{err}").contains("localPreviousRoot"),
            "validate 必须校验 localPreviousRoot，实际 {err}"
        );
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
        let settings_index = index_rel(root, GROUP_STORAGE, "settings");
        assert!(root.join(&settings_index).is_file());
        assert!(!legacy_index_path(root, GROUP_STORAGE, "settings").exists());
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
