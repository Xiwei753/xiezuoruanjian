//! `merge_remote_into_local_snapshot` 专项测试（#931：本地缺失恢复与文件级隔离）。
//!
//! 覆盖：
//! - known file 磁盘缺失 + 远端有可验证 blob → 从远端恢复并保留远端真实 LWW 元数据；
//! - 远端无可验证 blob → 显式 `unresolved_missing_paths`，不伪造删除、不上传；
//! - 远端 delete 事实 → 正常按删除收敛，不进缺失列表。

use super::*;
use crate::sync::provider::memory::MemoryProvider;
use crate::sync::types::{ManifestFileRecord, SyncManifest, SyncScope, SyncState};
use tempfile::TempDir;

fn md5_hex(content: &[u8]) -> String {
    crate::sync::hash::content_md5(content)
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
    )
    .unwrap();

    // 远端 delete 是真实删除事实：正常收敛，不算 unresolved。
    assert!(outcome.unresolved_missing_paths.is_empty());
    assert!(outcome.local_deletes.contains(&"project.json".to_string()));

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
