//! `merge_remote_into_local_snapshot` 专项测试（#931：本地缺失恢复与文件级隔离）。
//!
//! 覆盖：
//! - known file 磁盘缺失 + 远端有可验证 blob → 从远端恢复并保留远端真实 LWW 元数据；
//! - 远端无可验证 blob → 显式 `unresolved_missing_paths`，不伪造删除、不上传；
//! - 远端 delete 事实 → 正常按删除收敛，不进缺失列表；
//! - 作品级校对结果记录（`correction/results/**.json`）远端删除 → 保留本地
//!   （不可变并集，绝不 trash/删除本地记录）。

use super::*;
use crate::sync::provider::memory::MemoryProvider;
use crate::sync::sparse_plan::SparseObjectPlan;
use crate::sync::types::{ManifestFileRecord, SyncManifest, SyncScope, SyncState};
use tempfile::TempDir;

fn md5_hex(content: &[u8]) -> String {
    crate::sync::hash::content_md5(content)
}

fn write_sync_file(sync_root: &Path, rel: &str, content: &[u8]) {
    let path = sync_root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// 按内容哈希定位某个路径的 object 文件（两代保留时同一路径可能有两个）。
fn manifest_object_with_hash(sync_root: &Path, path: &str, hash: &str) -> std::path::PathBuf {
    let objects_dir = sync_root.join("app-meta/sync/manifest/objects");
    let needle_path = format!("\"path\":\"{path}\"");
    let needle_hash = format!("\"content_hash\":\"{hash}\"");
    let matches: Vec<std::path::PathBuf> = std::fs::read_dir(&objects_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|object| {
            let text = std::fs::read_to_string(object).unwrap();
            text.contains(&needle_path) && text.contains(&needle_hash)
        })
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one manifest object for {path} with hash {hash}, got {matches:?}"
    );
    matches[0].clone()
}

/// root 记录的 namespace → index sha 对应的内容寻址索引路径。
fn manifest_index_for(sync_root: &Path, namespace: &str) -> std::path::PathBuf {
    let root: crate::sync::lww::ManifestRoot = serde_json::from_slice(
        &std::fs::read(sync_root.join("app-meta/sync/manifest/root.json")).unwrap(),
    )
    .unwrap();
    let sha = root
        .namespaces
        .get(namespace)
        .unwrap_or_else(|| panic!("namespace {namespace} missing from root"));
    sync_root.join(format!(
        "app-meta/sync/manifest/index/{namespace}/{sha}.json"
    ))
}

fn manifest_object_for(sync_root: &Path, path: &str) -> std::path::PathBuf {
    let needle = format!("\"path\":\"{path}\"");
    let objects_dir = sync_root.join("app-meta/sync/manifest/objects");
    let mut matches: Vec<std::path::PathBuf> = std::fs::read_dir(&objects_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|object| std::fs::read_to_string(object).unwrap().contains(&needle))
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one manifest object for {path}, got {matches:?}"
    );
    matches.pop().unwrap()
}

fn upsert_record(
    path: &str,
    content_hash: &str,
    updated_at_ms: i64,
    device_id: &str,
) -> ManifestFileRecord {
    ManifestFileRecord {
        path: path.to_string(),
        content_hash: content_hash.to_string(),
        op: "upsert".to_string(),
        updated_at_ms,
        deleted_at_ms: None,
        device_id: device_id.to_string(),
        schema_version: 1,
    }
}

fn write_old_manifest(sync_root: &Path, files: Vec<ManifestFileRecord>) {
    std::fs::create_dir_all(sync_root.join("app-meta/sync")).unwrap();
    std::fs::write(
        sync_root.join(SYNC_MANIFEST_PATH),
        serde_json::to_string(&SyncManifest { files }).unwrap(),
    )
    .unwrap();
}

/// #931：本地 known file 磁盘缺失、远端有可验证 blob →
/// merge 从远端 blob 恢复文件，merged manifest 保留远端真实 LWW 时间/设备，
/// 不伪造本地编辑时间，也不把消失当成本地删除。
#[test]
fn issue_931_recovers_missing_known_file_from_remote_blob() {
    let tmp = TempDir::new().unwrap();
    let sync_root = tmp.path();
    let content = br#"{"title":"recovered"}"#;
    let hash = md5_hex(content);

    let mut state = SyncState::default();
    state.device_id = "current-device".to_string();
    state
        .known_files
        .insert("project.json".to_string(), hash.clone());
    state
        .known_files_updated_at
        .insert("project.json".to_string(), 1000);
    crate::sync::SyncService::save_sync_state(sync_root, &state).unwrap();

    write_old_manifest(
        sync_root,
        vec![upsert_record("project.json", &hash, 1000, "winner-device")],
    );

    let remote_manifest = SyncManifest {
        files: vec![upsert_record("project.json", &hash, 2000, "remote-dev")],
    };
    let provider = MemoryProvider::with_entries([
        ("projects/p1/project.json".to_string(), content.to_vec()),
        (
            "projects/p1/app-meta/sync/manifest.sync.json".to_string(),
            serde_json::to_string(&remote_manifest)
                .unwrap()
                .into_bytes(),
        ),
    ]);

    let outcome = merge_remote_into_local_snapshot(
        sync_root,
        &provider,
        "projects/p1",
        SyncScope::Project,
        &mut state,
        None,
    )
    .unwrap();

    // 文件已从远端 blob 恢复。
    assert_eq!(
        std::fs::read(sync_root.join("project.json")).unwrap(),
        content
    );
    assert!(outcome.unresolved_missing_paths.is_empty());
    assert!(outcome
        .downloaded_files
        .contains(&"project.json".to_string()));

    // merged manifest 保留远端真实 LWW（不伪造本地编辑时间）。
    let rec = outcome
        .merged_manifest
        .files
        .iter()
        .find(|r| r.path == "project.json")
        .expect("merged manifest 应包含 project.json");
    assert_eq!(rec.op, "upsert");
    assert_eq!(rec.updated_at_ms, 2000);
    assert_eq!(rec.device_id, "remote-dev");

    // 恢复后本地内容与远端一致，无需上传。
    assert!(outcome.remote_upload_paths.is_empty());
}

/// #931：本地 known file 缺失且远端没有可验证 blob →
/// 显式 unresolved（不转成 delete、不上传、不发布引用缺失 blob 的新 generation），
/// 旧 manifest 记录与 known_files 基线保留，等待后续恢复。
#[test]
fn issue_931_missing_without_remote_blob_is_unresolved_not_deleted() {
    let tmp = TempDir::new().unwrap();
    let sync_root = tmp.path();
    let hash = md5_hex(b"vanished-content");

    let mut state = SyncState::default();
    state.device_id = "current-device".to_string();
    state
        .known_files
        .insert("project.json".to_string(), hash.clone());
    state
        .known_files_updated_at
        .insert("project.json".to_string(), 1000);
    crate::sync::SyncService::save_sync_state(sync_root, &state).unwrap();

    write_old_manifest(
        sync_root,
        vec![upsert_record("project.json", &hash, 1000, "winner-device")],
    );

    // 远端完全没有这个文件。
    let provider = MemoryProvider::new();
    let outcome = merge_remote_into_local_snapshot(
        sync_root,
        &provider,
        "projects/p1",
        SyncScope::Project,
        &mut state,
        None,
    )
    .unwrap();

    assert_eq!(
        outcome.unresolved_missing_paths,
        vec!["project.json".to_string()]
    );
    assert!(!outcome
        .remote_upload_paths
        .contains(&"project.json".to_string()));
    assert!(!outcome
        .remote_delete_paths
        .contains(&"project.json".to_string()));
    // 文件没有被伪造删除，也没有被恢复。
    assert!(!sync_root.join("project.json").exists());

    // merged manifest 保留旧记录的真实元数据，不伪造新时间/删除。
    let rec = outcome
        .merged_manifest
        .files
        .iter()
        .find(|r| r.path == "project.json")
        .expect("merged manifest 应保留旧记录供恢复");
    assert_eq!(rec.op, "upsert");
    assert_eq!(rec.updated_at_ms, 1000);
    assert_eq!(rec.device_id, "winner-device");

    // 显式错误 + known_files 基线保留。
    let err = state.last_error.as_deref().unwrap_or_default();
    assert!(
        err.contains("local_missing_without_tombstone"),
        "last_error 应显式记录 local_missing_without_tombstone，实际: {err}"
    );
    assert_eq!(
        state.known_files.get("project.json").map(String::as_str),
        Some(hash.as_str())
    );
}

/// #931：本地文件缺失但远端有真实 delete 事实（manifest tombstone）→
/// 走正常删除收敛，不进缺失列表；known_files 不再保留该路径。
#[test]
fn issue_931_missing_local_file_with_remote_tombstone_converges_to_delete() {
    let tmp = TempDir::new().unwrap();
    let sync_root = tmp.path();
    let hash = md5_hex(b"deleted-content");

    let mut state = SyncState::default();
    state.device_id = "current-device".to_string();
    state
        .known_files
        .insert("project.json".to_string(), hash.clone());
    state
        .known_files_updated_at
        .insert("project.json".to_string(), 1000);
    crate::sync::SyncService::save_sync_state(sync_root, &state).unwrap();

    write_old_manifest(
        sync_root,
        vec![upsert_record("project.json", &hash, 1000, "winner-device")],
    );

    // 远端 manifest 记录删除事实（近期时间，避免 30 天清理）。
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut delete_rec = upsert_record("project.json", &hash, now_ms, "remote-dev");
    delete_rec.op = "delete".to_string();
    delete_rec.deleted_at_ms = Some(now_ms);
    let remote_manifest = SyncManifest {
        files: vec![delete_rec],
    };
    let provider = MemoryProvider::with_entries([(
        "projects/p1/app-meta/sync/manifest.sync.json".to_string(),
        serde_json::to_string(&remote_manifest)
            .unwrap()
            .into_bytes(),
    )]);

    let outcome = merge_remote_into_local_snapshot(
        sync_root,
        &provider,
        "projects/p1",
        SyncScope::Project,
        &mut state,
        None,
    )
    .unwrap();

    // 远端 delete 是真实删除事实：正常收敛，不算 unresolved。
    assert!(outcome.unresolved_missing_paths.is_empty());
    // Issue #993：本地文件已缺失时远端 delete 无需再 move_to_trash，
    // 不伪造本地删除动作，也不能反向变成本地 delete 上传。
    assert!(!outcome.local_deletes.contains(&"project.json".to_string()));
    assert!(!outcome
        .remote_delete_paths
        .contains(&"project.json".to_string()));

    let rec = outcome
        .merged_manifest
        .files
        .iter()
        .find(|r| r.path == "project.json")
        .expect("近期 delete record 应保留在 manifest");
    assert_eq!(rec.op, "delete");

    // 删除事实收敛后基线不再保留（不是待恢复的缺失）。
    assert!(!state.known_files.contains_key("project.json"));
}

/// #1064：作品级校对结果记录是不可变并集对象——远端 delete 事实不能删除
/// 本地记录，本地记录保留并重新发布，绝不进入 local_deletes / trash。
#[test]
fn correction_result_remote_delete_keeps_local_record() {
    let tmp = TempDir::new().unwrap();
    let sync_root = tmp.path();
    let rel = concat!(
        "correction/results/c1/",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ".json",
    );
    let content = br#"{"schemaVersion":1,"checked":true}"#;
    let hash = md5_hex(content);
    let record_dir = sync_root.join("correction/results/c1");
    std::fs::create_dir_all(&record_dir).unwrap();
    std::fs::write(sync_root.join(rel), content).unwrap();

    let mut state = SyncState::default();
    state.device_id = "current-device".to_string();
    state.known_files.insert(rel.to_string(), hash.clone());
    state.known_files_updated_at.insert(rel.to_string(), 1000);
    crate::sync::SyncService::save_sync_state(sync_root, &state).unwrap();

    write_old_manifest(
        sync_root,
        vec![upsert_record(rel, &hash, 1000, "local-device")],
    );

    // 远端只留下一条 delete 记录（另一台设备同步时的删除事实）。
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut delete_rec = upsert_record(rel, &hash, now_ms, "remote-dev");
    delete_rec.op = "delete".to_string();
    delete_rec.deleted_at_ms = Some(now_ms);
    let remote_manifest = SyncManifest {
        files: vec![delete_rec],
    };
    let provider = MemoryProvider::with_entries([(
        "projects/p1/app-meta/sync/manifest.sync.json".to_string(),
        serde_json::to_string(&remote_manifest)
            .unwrap()
            .into_bytes(),
    )]);

    let outcome = merge_remote_into_local_snapshot(
        sync_root,
        &provider,
        "projects/p1",
        SyncScope::Project,
        &mut state,
        None,
    )
    .unwrap();

    // 本地记录既不进 trash，也不作为删除事实上报给远端。
    assert!(
        sync_root.join(rel).exists(),
        "remote delete must not remove a local correction result"
    );
    assert!(!outcome.local_deletes.contains(&rel.to_string()));
    assert!(!outcome.remote_delete_paths.contains(&rel.to_string()));
    assert!(outcome.unresolved_missing_paths.is_empty());
    assert!(outcome.conflicts.is_empty());

    // 本地记录保留在 manifest 中并重新发布，别台设备仍需要的记录不会丢。
    let rec = outcome
        .merged_manifest
        .files
        .iter()
        .find(|r| r.path == rel)
        .expect("merged manifest 应保留本地校对结果记录");
    assert_eq!(rec.op, "upsert");
    assert_eq!(rec.content_hash, hash);
    assert!(outcome.remote_upload_paths.contains(&rel.to_string()));
    assert_eq!(
        state.known_files.get(rel).map(String::as_str),
        Some(hash.as_str())
    );
}

/// #1062 第 4 项：稀疏快照只读取 affected 路径。
///
/// 未受影响文件即使已经从 staging 消失，也不能进入 records 或
/// `missing_without_delete_fact`；它的已确认记录由基线 manifest 引用。
#[test]
fn sparse_snapshot_records_only_affected_paths() {
    let tmp = TempDir::new().unwrap();
    let sync_root = tmp.path();
    let affected = "volumes/a/chapters/c1/chapter.md";
    let untouched = "volumes/b/chapters/c2/chapter.md";
    write_sync_file(sync_root, affected, b"chapter one");
    write_sync_file(sync_root, untouched, b"chapter two");

    let affected_hash = md5_hex(b"chapter one");
    let untouched_hash = md5_hex(b"chapter two");
    let mut state = SyncState::default();
    state.device_id = "current-device".to_string();
    state
        .known_files
        .insert(affected.to_string(), affected_hash.clone());
    state
        .known_files
        .insert(untouched.to_string(), untouched_hash.clone());
    state
        .known_files_updated_at
        .insert(affected.to_string(), 1000);
    state
        .known_files_updated_at
        .insert(untouched.to_string(), 1000);
    crate::sync::SyncService::save_sync_state(sync_root, &state).unwrap();
    write_old_manifest(
        sync_root,
        vec![
            upsert_record(affected, &affected_hash, 1000, "local-dev"),
            upsert_record(untouched, &untouched_hash, 1000, "local-dev"),
        ],
    );

    // staging 里未受影响对象没有字节，但这不等于本地删除/缺失。
    std::fs::remove_file(sync_root.join(untouched)).unwrap();

    let plan = SparseObjectPlan::new(
        Some("head".to_string()),
        Vec::<String>::new(),
        vec![affected.to_string()],
    );
    let snapshot = snapshot_local_records_for_merge(
        sync_root,
        SyncScope::Project,
        "current-device",
        None,
        Some(&plan),
    )
    .unwrap();

    assert_eq!(
        snapshot.records.keys().cloned().collect::<Vec<_>>(),
        vec![affected.to_string()],
        "sparse snapshot must not project unaffected records"
    );
    assert!(
        snapshot.missing_without_delete_fact.is_empty(),
        "unaffected missing staging object must not be treated as a local delete"
    );
    assert_eq!(
        snapshot.records[affected].content_hash, affected_hash,
        "affected path must still be hashed from the real snapshot"
    );
}

/// #1062 第 4 项：稀疏 merge 只持久化 affected object/index/root，
/// 未受影响的对象与其它 namespace 索引保持逐字节不变；known_files 未涉及键
/// 也不被改写。
#[test]
fn sparse_merge_persists_only_affected_objects() {
    let tmp = TempDir::new().unwrap();
    let sync_root = tmp.path();
    let affected = "volumes/a/chapters/c1/chapter.md";
    let untouched = "volumes/b/chapters/c2/chapter.md";
    let project = "project.json";
    let project_content = br#"{"title":"p1"}"#;
    let old_affected_content = b"chapter one v1";
    let new_affected_content = b"chapter one v2";
    let untouched_content = b"chapter two v1";

    write_sync_file(sync_root, affected, old_affected_content);
    write_sync_file(sync_root, untouched, untouched_content);
    write_sync_file(sync_root, project, project_content);

    let old_affected_hash = md5_hex(old_affected_content);
    let new_affected_hash = md5_hex(new_affected_content);
    let untouched_hash = md5_hex(untouched_content);
    let project_hash = md5_hex(project_content);

    let mut state = SyncState::default();
    state.device_id = "current-device".to_string();
    for (path, hash) in [
        (affected, &old_affected_hash),
        (untouched, &untouched_hash),
        (project, &project_hash),
    ] {
        state.known_files.insert(path.to_string(), hash.clone());
        state.known_files_updated_at.insert(path.to_string(), 1000);
    }
    crate::sync::SyncService::save_sync_state(sync_root, &state).unwrap();

    let baseline = SyncManifest {
        files: vec![
            upsert_record(affected, &old_affected_hash, 1000, "local-dev"),
            upsert_record(untouched, &untouched_hash, 1000, "local-dev"),
            upsert_record(project, &project_hash, 1000, "local-dev"),
        ],
    };
    crate::sync::conflict::persist_sync_merge_result(
        sync_root,
        SYNC_MANIFEST_PATH,
        &baseline,
        &state,
        &[],
    )
    .unwrap();

    let untouched_object = manifest_object_for(sync_root, untouched);
    let untouched_object_before = std::fs::read(&untouched_object).unwrap();
    let old_affected_object = manifest_object_for(sync_root, affected);
    let root_index = manifest_index_for(sync_root, "_root");
    let root_index_before = std::fs::read(&root_index).unwrap();
    let volumes_index = manifest_index_for(sync_root, "volumes");
    let volumes_index_before = std::fs::read(&volumes_index).unwrap();

    // 本地修改 affected 文件；远端仍是基线内容，走三路 LocalChanged → 上传。
    std::fs::write(sync_root.join(affected), new_affected_content).unwrap();

    let remote_manifest = baseline.clone();
    let provider = MemoryProvider::with_entries([(
        format!("projects/p1/{SYNC_MANIFEST_PATH}"),
        serde_json::to_string(&remote_manifest)
            .unwrap()
            .into_bytes(),
    )]);
    let plan = SparseObjectPlan::new(
        Some("head".to_string()),
        Vec::<String>::new(),
        vec![affected.to_string()],
    );
    let outcome = merge_remote_into_local_snapshot(
        sync_root,
        &provider,
        "projects/p1",
        SyncScope::Project,
        &mut state,
        Some(&plan),
    )
    .unwrap();

    // 返回给 full_sync/发布方的仍是完整清单：基线 + affected 覆盖。
    assert_eq!(outcome.merged_manifest.files.len(), 3);
    let merged_affected = outcome
        .merged_manifest
        .files
        .iter()
        .find(|record| record.path == affected)
        .expect("merged manifest must contain affected record");
    assert_eq!(merged_affected.content_hash, new_affected_hash);
    assert!(outcome.remote_upload_paths.contains(&affected.to_string()));

    // 持久化清单同样完整，且只有 affected 记录变化。
    let stored = ManifestStore::new(sync_root, SYNC_MANIFEST_PATH)
        .load_all()
        .unwrap()
        .expect("manifest root must exist");
    assert_eq!(stored.files.len(), 3);
    let stored_affected = stored
        .files
        .iter()
        .find(|record| record.path == affected)
        .expect("stored manifest must contain affected record");
    assert_eq!(stored_affected.content_hash, new_affected_hash);

    // 未受影响 object 字节不变；affected 旧 object 被回收、新 object 存在。
    assert_eq!(
        std::fs::read(&untouched_object).unwrap(),
        untouched_object_before,
        "untouched object must not be rewritten"
    );
    // 内容寻址对象保留一代：上一代已发布根仍可能被读者引用，
    // 不能在新根刚提交后立刻删除。
    assert!(
        old_affected_object.exists(),
        "old affected object must be retained for the previous published root"
    );
    let new_affected_object = manifest_object_with_hash(sync_root, affected, &new_affected_hash);
    assert_ne!(new_affected_object, old_affected_object);

    // 其它 namespace 的 index 逐字节不变；affected namespace 的 index 必须变化。
    assert_eq!(
        std::fs::read(&root_index).unwrap(),
        root_index_before,
        "unaffected namespace index must keep its bytes"
    );
    // 上一代索引仍按哈希可寻址；新根指向新 sha 的新索引路径。
    assert_eq!(
        std::fs::read(&volumes_index).unwrap(),
        volumes_index_before,
        "previous generation index must stay readable by its sha"
    );
    let new_volumes_index = manifest_index_for(sync_root, "volumes");
    assert_ne!(new_volumes_index, volumes_index);
    assert!(
        new_volumes_index.is_file(),
        "new index must be content-addressed"
    );

    // known_files：只结算 affected；未涉及键原值保留。
    assert_eq!(
        state.known_files.get(affected).map(String::as_str),
        Some(new_affected_hash.as_str())
    );
    for (path, hash) in [(untouched, &untouched_hash), (project, &project_hash)] {
        assert_eq!(
            state.known_files.get(path).map(String::as_str),
            Some(hash.as_str()),
            "known_files for unaffected {path} must stay unchanged"
        );
    }

    // 再发布一代（空增量）后，第一代不再处于“新根 ∪ 上一代”保留边界内，
    // 旧 affected object 才可以被回收。
    crate::sync::conflict::persist_sync_merge_delta(
        sync_root,
        SYNC_MANIFEST_PATH,
        &crate::sync::manifest_delta::ManifestDelta::default(),
        &state,
        &[],
    )
    .unwrap();
    assert!(
        !old_affected_object.exists(),
        "object unreachable from both current and previous root must be reclaimed"
    );
}

/// 稀疏 merge 的 delete 墓碑仍写入清单，但本地 known_files 基线必须移除。
#[test]
fn sparse_merge_keeps_delete_tombstone_without_known_file() {
    let tmp = TempDir::new().unwrap();
    let sync_root = tmp.path();
    let path = "project.json";
    let old_hash = md5_hex(b"deleted content");

    let mut state = SyncState::default();
    state.device_id = "current-device".to_string();
    state.known_files.insert(path.to_string(), old_hash.clone());
    state.known_files_updated_at.insert(path.to_string(), 1000);
    crate::sync::SyncService::save_sync_state(sync_root, &state).unwrap();

    let baseline = SyncManifest {
        files: vec![upsert_record(path, &old_hash, 1000, "local-dev")],
    };
    crate::sync::conflict::persist_sync_merge_result(
        sync_root,
        SYNC_MANIFEST_PATH,
        &baseline,
        &state,
        &[],
    )
    .unwrap();

    // 本地文件已缺失，远端有近期 delete 事实：按删除收敛，不伪造缺失。
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut delete_record = upsert_record(path, &old_hash, now_ms, "remote-dev");
    delete_record.op = "delete".to_string();
    delete_record.deleted_at_ms = Some(now_ms);
    let remote_manifest = SyncManifest {
        files: vec![delete_record],
    };
    let provider = MemoryProvider::with_entries([(
        format!("projects/p1/{SYNC_MANIFEST_PATH}"),
        serde_json::to_string(&remote_manifest)
            .unwrap()
            .into_bytes(),
    )]);
    let plan = SparseObjectPlan::new(
        Some("head".to_string()),
        Vec::<String>::new(),
        vec![path.to_string()],
    );
    let outcome = merge_remote_into_local_snapshot(
        sync_root,
        &provider,
        "projects/p1",
        SyncScope::Project,
        &mut state,
        Some(&plan),
    )
    .unwrap();

    assert!(outcome.unresolved_missing_paths.is_empty());
    let merged = outcome
        .merged_manifest
        .files
        .iter()
        .find(|record| record.path == path)
        .expect("delete tombstone must stay in the merged manifest");
    assert_eq!(merged.op, "delete");
    assert!(
        !state.known_files.contains_key(path),
        "delete tombstone must not leave a content hash in known_files"
    );

    let stored = ManifestStore::new(sync_root, SYNC_MANIFEST_PATH)
        .load_all()
        .unwrap()
        .expect("manifest root must exist");
    let stored_record = stored
        .files
        .iter()
        .find(|record| record.path == path)
        .expect("delete tombstone must be persisted");
    assert_eq!(stored_record.op, "delete");
}

/// 非稀疏路径仍产出完整清单（不因为稀疏优化丢掉未涉及记录）。
#[test]
fn non_sparse_merge_still_returns_complete_manifest() {
    let tmp = TempDir::new().unwrap();
    let sync_root = tmp.path();
    let first = "volumes/a/chapters/c1/chapter.md";
    let second = "volumes/b/chapters/c2/chapter.md";
    write_sync_file(sync_root, first, b"one");
    write_sync_file(sync_root, second, b"two");

    let first_hash = md5_hex(b"one");
    let second_hash = md5_hex(b"two");
    let mut state = SyncState::default();
    state.device_id = "current-device".to_string();
    state
        .known_files
        .insert(first.to_string(), first_hash.clone());
    state
        .known_files
        .insert(second.to_string(), second_hash.clone());
    state.known_files_updated_at.insert(first.to_string(), 1000);
    state
        .known_files_updated_at
        .insert(second.to_string(), 1000);
    crate::sync::SyncService::save_sync_state(sync_root, &state).unwrap();
    write_old_manifest(
        sync_root,
        vec![
            upsert_record(first, &first_hash, 1000, "local-dev"),
            upsert_record(second, &second_hash, 1000, "local-dev"),
        ],
    );

    let remote_manifest = SyncManifest {
        files: vec![
            upsert_record(first, &first_hash, 1000, "local-dev"),
            upsert_record(second, &second_hash, 1000, "local-dev"),
        ],
    };
    let provider = MemoryProvider::with_entries([(
        format!("projects/p1/{SYNC_MANIFEST_PATH}"),
        serde_json::to_string(&remote_manifest)
            .unwrap()
            .into_bytes(),
    )]);

    let outcome = merge_remote_into_local_snapshot(
        sync_root,
        &provider,
        "projects/p1",
        SyncScope::Project,
        &mut state,
        None,
    )
    .unwrap();

    assert_eq!(outcome.merged_manifest.files.len(), 2);
    for path in [first, second] {
        assert!(
            outcome
                .merged_manifest
                .files
                .iter()
                .any(|record| record.path == path),
            "non-sparse merged manifest must contain {path}"
        );
    }
    let stored = ManifestStore::new(sync_root, SYNC_MANIFEST_PATH)
        .load_all()
        .unwrap()
        .expect("manifest root must exist");
    assert_eq!(stored.files.len(), 2);
}
