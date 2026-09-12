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

// =========================================================================
// Issue #659: workspace 总 manifest fallback 测试
//
// 旧远端作品可能只有 workspace 总 manifest (app/app-meta/sync/manifest.sync.json)
// 而无 project 级 manifest (projects/<id>/app-meta/sync/manifest.sync.json)。
// discover_legacy_remote_catalog 在 project 级 manifest 缺失时应 fallback 到
// workspace 总 manifest 中 projects/<id>/... 的 records 推断 LWW。
// =========================================================================

/// 构造 workspace 总 manifest JSON（records 的 path 为 `projects/<id>/...` 绝对路径）。
fn make_workspace_manifest_json(files: &[(&str, i64, &str, &str)]) -> Vec<u8> {
    // (path, updated_at_ms, device_id, op)
    let files_json: Vec<_> = files
        .iter()
        .map(|(path, ts, dev, op)| {
            serde_json::json!({
                "path": path,
                "content_hash": "dummy-md5",
                "updated_at_ms": ts,
                "device_id": dev,
                "op": op,
                "schema_version": 1
            })
        })
        .collect();
    serde_json::json!({ "files": files_json })
        .to_string()
        .into_bytes()
}

/// 构造 project 级 manifest JSON（records 的 path 为相对路径如 `project.json`）。
fn make_project_manifest_json(files: &[(&str, i64, &str, &str)]) -> Vec<u8> {
    make_workspace_manifest_json(files)
}

/// 测试 1: workspace fallback 成功。
///
/// 远端有 `projects/<id>/project.json` + workspace manifest（含 `projects/<id>/...` records），
/// 无 project 级 manifest，无 targets.sync.json →
/// `discover_legacy_remote_catalog` 返回 Ok，record 是 Upsert，LWW 来自 workspace manifest。
#[test]
fn issue_659_workspace_fallback_succeeds() {
    let project_id = "02d3aa5e-576e-490f-9840-3dbccaab3b3a";
    let project_json_path = format!("projects/{project_id}/project.json");

    // workspace manifest 含两条 projects/<id>/... records，LWW 应取最大 (ts, device_id)。
    let rec1_path = format!("projects/{project_id}/project.json");
    let rec2_path = format!("projects/{project_id}/volumes/v1.json");
    let workspace_manifest = make_workspace_manifest_json(&[
        (rec1_path.as_str(), 1700000000000_i64, "device-a", "upsert"),
        (rec2_path.as_str(), 1700000005000_i64, "device-b", "upsert"),
    ]);

    let provider = MemoryProvider::with_entries([
        (project_json_path, br#"{"title":"legacy"}"#.to_vec()),
        (
            WORKSPACE_MANIFEST_REMOTE_PATH.to_string(),
            workspace_manifest,
        ),
    ]);

    let result = discover_legacy_remote_catalog(&provider);
    let snapshot = result.expect("workspace fallback 应成功，但返回 Err");

    assert_eq!(snapshot.version.as_str(), "__nonexistent__");
    assert_eq!(
        snapshot.catalog.records.len(),
        1,
        "应合成 1 条 Upsert record"
    );
    let record = &snapshot.catalog.records[0];
    assert_eq!(record.target_id, format!("projects/{project_id}"));
    assert_eq!(record.op, TargetOp::Upsert);
    // LWW 应来自 workspace manifest 中 ts=1700000005000, device=device-b。
    assert_eq!(record.updated_at_ms, 1700000005000);
    assert_eq!(record.device_id, "device-b");
}

/// 测试 2: project manifest 优先。
///
/// 远端有 project 级 manifest + workspace manifest，两者 LWW 不同 →
/// 用 project 级 manifest 的 LWW（project manifest 优先）。
#[test]
fn issue_659_project_manifest_takes_priority() {
    let project_id = "02d3aa5e-576e-490f-9840-3dbccaab3b3a";
    let project_json_path = format!("projects/{project_id}/project.json");
    let project_manifest_path = format!("projects/{project_id}/app-meta/sync/manifest.sync.json");

    // project 级 manifest LWW: ts=1800000000000, device=project-dev。
    let project_manifest =
        make_project_manifest_json(&[("project.json", 1800000000000_i64, "project-dev", "upsert")]);
    // workspace manifest LWW: ts=1700000000000, device=workspace-dev（更旧，不应被采用）。
    let ws_rec_path = format!("projects/{project_id}/project.json");
    let workspace_manifest = make_workspace_manifest_json(&[(
        ws_rec_path.as_str(),
        1700000000000_i64,
        "workspace-dev",
        "upsert",
    )]);

    let provider = MemoryProvider::with_entries([
        (project_json_path, br#"{"title":"legacy"}"#.to_vec()),
        (project_manifest_path, project_manifest),
        (
            WORKSPACE_MANIFEST_REMOTE_PATH.to_string(),
            workspace_manifest,
        ),
    ]);

    let result = discover_legacy_remote_catalog(&provider);
    let snapshot = result.expect("project manifest 优先应成功");

    assert_eq!(snapshot.catalog.records.len(), 1);
    let record = &snapshot.catalog.records[0];
    // LWW 应来自 project 级 manifest（ts=1800000000000, device=project-dev）。
    assert_eq!(record.updated_at_ms, 1800000000000);
    assert_eq!(record.device_id, "project-dev");
}

/// 测试 3: 两边都失败才 Err。
///
/// 远端有 `projects/<id>/project.json`，无 project 级 manifest，
/// 无 workspace manifest（或 workspace manifest 无匹配 records）→
/// 返回 Err（现有行为）。
#[test]
fn issue_659_both_fail_returns_err() {
    let project_id = "02d3aa5e-576e-490f-9840-3dbccaab3b3a";
    let project_json_path = format!("projects/{project_id}/project.json");

    // 无 workspace manifest，无 project 级 manifest。
    let provider =
        MemoryProvider::with_entries([(project_json_path, br#"{"title":"legacy"}"#.to_vec())]);

    let result = discover_legacy_remote_catalog(&provider);
    assert!(result.is_err(), "两边都失败应返回 Err，但返回了 Ok");
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("read_legacy_project_lww"),
        "错误信息应包含 'read_legacy_project_lww'，实际: {err_msg}"
    );
    assert!(
        err_msg.contains("manifest not found for project"),
        "错误信息应包含 'manifest not found for project'，实际: {err_msg}"
    );
}

/// 测试 3b: workspace manifest 存在但无匹配 records 也应返回 Err。
#[test]
fn issue_659_workspace_manifest_no_matching_records_returns_err() {
    let project_id = "02d3aa5e-576e-490f-9840-3dbccaab3b3a";
    let project_json_path = format!("projects/{project_id}/project.json");

    // workspace manifest 存在但只含其他 project 的 records。
    let other_project_id = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    let other_rec_path = format!("projects/{other_project_id}/project.json");
    let workspace_manifest = make_workspace_manifest_json(&[(
        other_rec_path.as_str(),
        1700000000000_i64,
        "device-a",
        "upsert",
    )]);

    let provider = MemoryProvider::with_entries([
        (project_json_path, br#"{"title":"legacy"}"#.to_vec()),
        (
            WORKSPACE_MANIFEST_REMOTE_PATH.to_string(),
            workspace_manifest,
        ),
    ]);

    let result = discover_legacy_remote_catalog(&provider);
    assert!(
        result.is_err(),
        "workspace manifest 无匹配 records 应返回 Err"
    );
}

/// 测试 4: workspace manifest 损坏 → Err。
///
/// workspace manifest 存在但 JSON 非法 → 返回 Err（不静默隐藏）。
#[test]
fn issue_659_corrupted_workspace_manifest_returns_err() {
    let project_id = "02d3aa5e-576e-490f-9840-3dbccaab3b3a";
    let project_json_path = format!("projects/{project_id}/project.json");

    let provider = MemoryProvider::with_entries([
        (project_json_path, br#"{"title":"legacy"}"#.to_vec()),
        (
            WORKSPACE_MANIFEST_REMOTE_PATH.to_string(),
            b"not valid json".to_vec(),
        ),
    ]);

    let result = discover_legacy_remote_catalog(&provider);
    assert!(
        result.is_err(),
        "损坏的 workspace manifest 应返回 Err，不应静默隐藏"
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("load_legacy_workspace_manifest"),
        "错误信息应包含 'load_legacy_workspace_manifest'，实际: {err_msg}"
    );
}

/// 测试 5: workspace manifest 不存在 + project manifest 存在 → Ok。
///
/// 现有行为不变（workspace manifest 不存在是正常的）。
#[test]
fn issue_659_workspace_absent_project_present_succeeds() {
    let project_id = "02d3aa5e-576e-490f-9840-3dbccaab3b3a";
    let project_json_path = format!("projects/{project_id}/project.json");
    let project_manifest_path = format!("projects/{project_id}/app-meta/sync/manifest.sync.json");

    // 只有 project 级 manifest，无 workspace manifest。
    let project_manifest = make_project_manifest_json(&[(
        "project.json",
        1700000000000_i64,
        "legacy-device",
        "upsert",
    )]);

    let provider = MemoryProvider::with_entries([
        (project_json_path, br#"{"title":"legacy"}"#.to_vec()),
        (project_manifest_path, project_manifest),
    ]);

    let result = discover_legacy_remote_catalog(&provider);
    let snapshot = result.expect("project manifest 存在时应成功（现有行为不变）");

    assert_eq!(snapshot.catalog.records.len(), 1);
    let record = &snapshot.catalog.records[0];
    assert_eq!(record.updated_at_ms, 1700000000000);
    assert_eq!(record.device_id, "legacy-device");
}

/// 测试 6: 多个 project 混合。
///
/// project A 有 project 级 manifest，project B 只有 workspace manifest records →
/// 两者都成功，各自 LWW 正确。
#[test]
fn issue_659_mixed_projects_both_succeed() {
    let project_a = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    let project_b = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";

    let project_a_json_path = format!("projects/{project_a}/project.json");
    let project_b_json_path = format!("projects/{project_b}/project.json");

    // project A 有 project 级 manifest，LWW: ts=2000, device=dev-a。
    let project_a_manifest_path = format!("projects/{project_a}/app-meta/sync/manifest.sync.json");
    let project_a_manifest =
        make_project_manifest_json(&[("project.json", 2000_i64, "dev-a", "upsert")]);

    // project B 无 project 级 manifest，只在 workspace manifest 中有 records。
    // workspace manifest 同时含 project A 和 project B 的 records（project A 的应被忽略，
    // 因为 project A 有自己的 project 级 manifest）。
    let ws_a_path = format!("projects/{project_a}/project.json");
    let ws_b1_path = format!("projects/{project_b}/project.json");
    let ws_b2_path = format!("projects/{project_b}/volumes/v1.json");
    let workspace_manifest = make_workspace_manifest_json(&[
        (ws_a_path.as_str(), 9999_i64, "should-be-ignored", "upsert"),
        (ws_b1_path.as_str(), 3000_i64, "dev-b", "upsert"),
        (ws_b2_path.as_str(), 4000_i64, "dev-b2", "upsert"),
    ]);

    let provider = MemoryProvider::with_entries([
        (project_a_json_path, br#"{"title":"a"}"#.to_vec()),
        (project_b_json_path, br#"{"title":"b"}"#.to_vec()),
        (project_a_manifest_path, project_a_manifest),
        (
            WORKSPACE_MANIFEST_REMOTE_PATH.to_string(),
            workspace_manifest,
        ),
    ]);

    let result = discover_legacy_remote_catalog(&provider);
    let snapshot = result.expect("混合场景两个 project 都应成功");

    assert_eq!(snapshot.catalog.records.len(), 2);

    // records 按 target_id 字典序排序（project_a < project_b）。
    let record_a = &snapshot.catalog.records[0];
    assert_eq!(record_a.target_id, format!("projects/{project_a}"));
    // project A 用自己的 project 级 manifest（ts=2000, dev=dev-a），
    // 不用 workspace manifest 中 ts=9999 的 record。
    assert_eq!(record_a.updated_at_ms, 2000);
    assert_eq!(record_a.device_id, "dev-a");

    let record_b = &snapshot.catalog.records[1];
    assert_eq!(record_b.target_id, format!("projects/{project_b}"));
    // project B 用 workspace manifest 的 LWW（ts=4000, dev=dev-b2）。
    assert_eq!(record_b.updated_at_ms, 4000);
    assert_eq!(record_b.device_id, "dev-b2");
}
