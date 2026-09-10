//! Tests for build_plan_* and planner-related functions.

use super::*;
use crate::sync::types::{ManifestFileRecord, SyncManifest, TargetLifecycleCatalog};
use tempfile::TempDir;

///   build_full_sync_target_plan 包含 pending deleted target。
#[test]
fn build_plan_includes_pending_deleted_targets() {
    use crate::sync::types::{PendingDeletedTarget, PlannedTargetKind};

    let tmp = TempDir::new().unwrap();
    let app_root = tmp.path().join("app");
    let projects_root = tmp.path().join("projects");
    std::fs::create_dir_all(&app_root).unwrap();
    std::fs::create_dir_all(&projects_root).unwrap();

    let pending = vec![PendingDeletedTarget::for_project(
        "p-deleted",
        1000,
        "token-1",
        "dev-1",
    )];
    let sync_policy = crate::sync::types::SyncPolicy::default();

    let planned = crate::sync::full_sync::build_full_sync_target_plan(
        &app_root,
        &projects_root,
        &[],
        &pending,
        &TargetLifecycleCatalog::default(),
        &sync_policy,
        false,
        "dev-1",
        &[],
    );

    // App target + 1 deleted target。
    assert_eq!(planned.len(), 2);
    assert_eq!(planned[0].target_kind, PlannedTargetKind::App);
    // 远端无 catalog 记录 → DeleteRemoteProject。
    assert_eq!(
        planned[1].target_kind,
        PlannedTargetKind::DeleteRemoteProject
    );
    assert_eq!(planned[1].target.remote_prefix, "projects/p-deleted");
    assert_eq!(planned[1].deleted_journal_token.as_deref(), Some("token-1"));
    assert!(planned[1].deleted_lww.is_some());
    //   target_live_root 应指向 projects_root/<id>。
    assert_eq!(planned[1].target_live_root, projects_root.join("p-deleted"));
}

///   build_full_sync_target_plan 包含 live project targets。
#[test]
fn build_plan_includes_live_projects() {
    use crate::project::Project;
    use crate::sync::types::PlannedTargetKind;

    let tmp = TempDir::new().unwrap();
    let app_root = tmp.path().join("app");
    let projects_root = tmp.path().join("projects");
    std::fs::create_dir_all(&app_root).unwrap();
    std::fs::create_dir_all(&projects_root).unwrap();

    let projects = vec![Project {
        id: "p1".to_string(),
        title: "T1".to_string(),
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
        order: 0,
    }];
    //   首次同步 manifest 缺失，需要 project.json
    // 元数据建立初始 LWW。写一份合法 project.json。
    let project_root = projects_root.join("p1");
    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::write(
        project_root.join("project.json"),
        serde_json::to_vec(&projects[0]).unwrap(),
    )
    .unwrap();
    let sync_policy = crate::sync::types::SyncPolicy::default();

    let planned = crate::sync::full_sync::build_full_sync_target_plan(
        &app_root,
        &projects_root,
        &projects,
        &[],
        &TargetLifecycleCatalog::default(),
        &sync_policy,
        false,
        "dev-1",
        &[],
    );

    // App target + 1 project target。
    assert_eq!(planned.len(), 2);
    assert_eq!(planned[0].target_kind, PlannedTargetKind::App);
    // 远端无 catalog 记录 → LiveProject（首次同步建立初始 manifest）。
    assert_eq!(planned[1].target_kind, PlannedTargetKind::LiveProject);
    assert_eq!(planned[1].target.remote_prefix, "projects/p1");
    assert_eq!(planned[1].local_root, projects_root.join("p1"));
}

///   远端 catalog 有 delete tombstone 且本地 live 更晚 → LiveProject。
#[test]
fn build_plan_live_project_with_remote_delete_local_wins() {
    use crate::project::Project;
    use crate::sync::types::PlannedTargetKind;

    let tmp = TempDir::new().unwrap();
    let app_root = tmp.path().join("app");
    let projects_root = tmp.path().join("projects");
    std::fs::create_dir_all(&app_root).unwrap();
    let project_root = projects_root.join("p1");
    std::fs::create_dir_all(&project_root).unwrap();

    // 写本地 manifest，lww_time = 12000。
    std::fs::create_dir_all(project_root.join("app-meta/sync")).unwrap();
    let manifest = SyncManifest {
        files: vec![ManifestFileRecord {
            path: "volumes/v1/chapter.md".to_string(),
            content_hash: "9a0364b9e99bb480dd25e1f0284c8555".to_string(),
            updated_at_ms: 12000,
            deleted_at_ms: None,
            device_id: "dev-1".to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        }],
    };
    std::fs::write(
        project_root.join("app-meta/sync/manifest.sync.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    std::fs::create_dir_all(project_root.join("volumes/v1")).unwrap();
    std::fs::write(project_root.join("volumes/v1/chapter.md"), b"content").unwrap();

    let projects = vec![Project {
        id: "p1".to_string(),
        title: "T1".to_string(),
        created_at: "2024-01-01".to_string(),
        updated_at: "2024-01-01".to_string(),
        order: 0,
    }];

    let mut catalog = TargetLifecycleCatalog::default();
    crate::sync::target_lifecycle::upsert_record(
        &mut catalog,
        crate::sync::types::TargetLifecycleRecord::delete(
            "projects/p1",
            "projects/p1",
            11000,
            "dev-2",
        ),
    );

    let planned = crate::sync::full_sync::build_full_sync_target_plan(
        &app_root,
        &projects_root,
        &projects,
        &[],
        &catalog,
        &crate::sync::types::SyncPolicy::default(),
        false,
        "dev-1",
        &[],
    );

    assert_eq!(planned[1].target_kind, PlannedTargetKind::LiveProject);
}

///   远端 catalog 有 delete tombstone 且远端更晚 → DeleteLocalProject。
#[test]
fn build_plan_live_project_with_remote_delete_remote_wins() {
    use crate::project::Project;
    use crate::sync::types::PlannedTargetKind;

    let tmp = TempDir::new().unwrap();
    let app_root = tmp.path().join("app");
    let projects_root = tmp.path().join("projects");
    std::fs::create_dir_all(&app_root).unwrap();
    let project_root = projects_root.join("p1");
    std::fs::create_dir_all(&project_root).unwrap();

    std::fs::create_dir_all(project_root.join("app-meta/sync")).unwrap();
    let manifest = SyncManifest {
        files: vec![ManifestFileRecord {
            path: "volumes/v1/chapter.md".to_string(),
            content_hash: "9a0364b9e99bb480dd25e1f0284c8555".to_string(),
            updated_at_ms: 11000,
            deleted_at_ms: None,
            device_id: "dev-1".to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        }],
    };
    std::fs::write(
        project_root.join("app-meta/sync/manifest.sync.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    std::fs::create_dir_all(project_root.join("volumes/v1")).unwrap();
    std::fs::write(project_root.join("volumes/v1/chapter.md"), b"content").unwrap();

    let projects = vec![Project {
        id: "p1".to_string(),
        title: "T1".to_string(),
        created_at: "2024-01-01".to_string(),
        updated_at: "2024-01-01".to_string(),
        order: 0,
    }];

    let mut catalog = TargetLifecycleCatalog::default();
    crate::sync::target_lifecycle::upsert_record(
        &mut catalog,
        crate::sync::types::TargetLifecycleRecord::delete(
            "projects/p1",
            "projects/p1",
            12000,
            "dev-2",
        ),
    );

    let planned = crate::sync::full_sync::build_full_sync_target_plan(
        &app_root,
        &projects_root,
        &projects,
        &[],
        &catalog,
        &crate::sync::types::SyncPolicy::default(),
        false,
        "dev-1",
        &[],
    );

    assert_eq!(
        planned[1].target_kind,
        PlannedTargetKind::DeleteLocalProject
    );
}

///   pending delete 远端 upsert 且本地 tombstone 胜出 → DeleteRemoteProject。
#[test]
fn build_plan_pending_delete_local_tombstone_wins() {
    use crate::sync::types::{PendingDeletedTarget, PlannedTargetKind};

    let tmp = TempDir::new().unwrap();
    let app_root = tmp.path().join("app");
    let projects_root = tmp.path().join("projects");
    std::fs::create_dir_all(&app_root).unwrap();
    std::fs::create_dir_all(&projects_root).unwrap();

    let pending = vec![PendingDeletedTarget::for_project(
        "p1", 12000, "token-1", "dev-1",
    )];

    let mut catalog = TargetLifecycleCatalog::default();
    crate::sync::target_lifecycle::upsert_record(
        &mut catalog,
        crate::sync::types::TargetLifecycleRecord::upsert(
            "projects/p1",
            "projects/p1",
            11000,
            "dev-2",
        ),
    );

    let planned = crate::sync::full_sync::build_full_sync_target_plan(
        &app_root,
        &projects_root,
        &[],
        &pending,
        &catalog,
        &crate::sync::types::SyncPolicy::default(),
        false,
        "dev-1",
        &[],
    );

    assert_eq!(
        planned[1].target_kind,
        PlannedTargetKind::DeleteRemoteProject
    );
}

///   pending delete 远端 upsert 且远端胜出 → RestoreProject。
#[test]
fn build_plan_pending_delete_remote_upsert_wins() {
    use crate::sync::types::{PendingDeletedTarget, PlannedTargetKind};

    let tmp = TempDir::new().unwrap();
    let app_root = tmp.path().join("app");
    let projects_root = tmp.path().join("projects");
    std::fs::create_dir_all(&app_root).unwrap();
    std::fs::create_dir_all(&projects_root).unwrap();

    let pending = vec![PendingDeletedTarget::for_project(
        "p1", 11000, "token-1", "dev-1",
    )];

    let mut catalog = TargetLifecycleCatalog::default();
    crate::sync::target_lifecycle::upsert_record(
        &mut catalog,
        crate::sync::types::TargetLifecycleRecord::upsert(
            "projects/p1",
            "projects/p1",
            12000,
            "dev-2",
        ),
    );

    let planned = crate::sync::full_sync::build_full_sync_target_plan(
        &app_root,
        &projects_root,
        &[],
        &pending,
        &catalog,
        &crate::sync::types::SyncPolicy::default(),
        false,
        "dev-1",
        &[],
    );

    assert_eq!(planned[1].target_kind, PlannedTargetKind::RestoreProject);
}

// ─────────────────────────────────────────────────────────────────────
//   复现测试 — 6 个实质问题
// ─────────────────────────────────────────────────────────────────────

/// 问题 1 复现：remote-only Project 不进入 plan。
#[test]
fn repro_issue_645_q1_remote_only_project_not_in_plan() {
    use crate::sync::types::PlannedTargetKind;

    let tmp = TempDir::new().unwrap();
    let app_root = tmp.path().join("app");
    let projects_root = tmp.path().join("projects");
    std::fs::create_dir_all(&app_root).unwrap();
    std::fs::create_dir_all(&projects_root).unwrap();

    let mut remote_catalog = TargetLifecycleCatalog::default();
    crate::sync::target_lifecycle::upsert_record(
        &mut remote_catalog,
        crate::sync::types::TargetLifecycleRecord::upsert(
            "projects/remote-only",
            "projects/remote-only",
            10_000,
            "dev-A",
        ),
    );

    let planned = crate::sync::full_sync::build_full_sync_target_plan(
        &app_root,
        &projects_root,
        &[],
        &[],
        &remote_catalog,
        &crate::sync::types::SyncPolicy::default(),
        false,
        "dev-B",
        &[],
    );

    let has_remote_only = planned
        .iter()
        .any(|t| t.target.remote_prefix == "projects/remote-only");
    assert!(
        has_remote_only,
        "问题1: remote-only project 未进入 plan。\
         当前 plan targets: {:?}",
        planned
            .iter()
            .map(|t| (t.target.remote_prefix.clone(), t.target_kind))
            .collect::<Vec<_>>()
    );

    let remote_only_target = planned
        .iter()
        .find(|t| t.target.remote_prefix == "projects/remote-only")
        .expect("remote-only target should exist");
    assert_eq!(
        remote_only_target.target_kind,
        PlannedTargetKind::RestoreProject,
        "问题1: remote-only project 应为 RestoreProject"
    );
}

/// 问题 2 复现：本地 manifest 缺失时伪造 now() 作为 LWW。
#[test]
fn repro_issue_645_q2_manifest_missing_fakes_lww_now() {
    use crate::project::Project;
    use crate::sync::types::PlannedTargetKind;
    use crate::sync::types::SyncPolicy;

    let tmp = TempDir::new().unwrap();
    let app_root = tmp.path().join("app");
    let projects_root = tmp.path().join("projects");
    std::fs::create_dir_all(&app_root).unwrap();
    let project_root = projects_root.join("P");
    std::fs::create_dir_all(&project_root).unwrap();

    let projects = vec![Project {
        id: "P".to_string(),
        title: "T".to_string(),
        created_at: "2024-01-01".to_string(),
        updated_at: "2024-01-01".to_string(),
        order: 0,
    }];

    let mut remote_catalog = TargetLifecycleCatalog::default();
    crate::sync::target_lifecycle::upsert_record(
        &mut remote_catalog,
        crate::sync::types::TargetLifecycleRecord::delete(
            "projects/P",
            "projects/P",
            12_000,
            "dev-A",
        ),
    );

    let planned = crate::sync::full_sync::build_full_sync_target_plan(
        &app_root,
        &projects_root,
        &projects,
        &[],
        &remote_catalog,
        &crate::sync::types::SyncPolicy::default(),
        false,
        "dev-B",
        &[],
    );

    let p_target = planned
        .iter()
        .find(|t| t.target.remote_prefix == "projects/P")
        .expect("P target should exist");

    assert!(
        !matches!(p_target.target_kind, PlannedTargetKind::LiveProject),
        "问题2: manifest 缺失时不应判为 LiveProject（会用 now() 伪造 upsert 复活远端 delete）。\
         当前 target_kind={:?}, live_lww={:?}",
        p_target.target_kind,
        p_target.live_lww
    );

    let provider = crate::sync::provider::memory::MemoryProvider::new();
    {
        let mut cat = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut cat,
            crate::sync::types::TargetLifecycleRecord::delete(
                "projects/P",
                "projects/P",
                12_000,
                "dev-A",
            ),
        );
        let snap = crate::sync::types::RemoteTargetCatalogSnapshot {
            catalog: cat,
            version: crate::sync::provider::model::RemoteVersion::new("__nonexistent__"),
        };
        crate::sync::target_lifecycle::write_remote_catalog(&provider, &snap).unwrap();
    }

    let plan = super::super::FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: false,
        targets: planned.clone(),
        app_data_root: app_root.clone(),
        remote_catalog_snapshot: test_empty_catalog_snapshot(),
    };
    let transfer = super::super::run_transfer(&provider, &plan);

    let final_snapshot = crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
    let rec = crate::sync::target_lifecycle::find_record(&final_snapshot.catalog, "projects/P");

    let catalog_has_upsert = rec
        .map(|r| r.op == crate::sync::types::TargetOp::Upsert)
        .unwrap_or(false);
    assert!(
        !catalog_has_upsert,
        "问题2: manifest 缺失时伪造 now() upsert 复活了远端 delete。\
         catalog 记录={:?}, transfer status={:?}",
        rec.map(|r| (r.op, r.updated_at_ms)),
        transfer
            .targets
            .iter()
            .map(|t| &t.result.status)
            .collect::<Vec<_>>()
    );
}

///   parse_project_target_id 合法 target_id 正确解析。
#[test]
fn q6_parse_project_target_id_valid() {
    assert_eq!(
        crate::sync::target_lifecycle::parse_project_target_id("projects/p1").unwrap(),
        "p1"
    );
    assert_eq!(
        crate::sync::target_lifecycle::parse_project_target_id("projects/abc-123").unwrap(),
        "abc-123"
    );
}

///   非法 target_id 被拒绝。
#[test]
fn q6_parse_project_target_id_invalid() {
    assert!(crate::sync::target_lifecycle::parse_project_target_id("p1").is_err());
    assert!(crate::sync::target_lifecycle::parse_project_target_id("apps/p1").is_err());
    assert!(crate::sync::target_lifecycle::parse_project_target_id("projects/").is_err());
    assert!(crate::sync::target_lifecycle::parse_project_target_id("projects/../app").is_err());
    assert!(crate::sync::target_lifecycle::parse_project_target_id("projects/a/b").is_err());
    assert!(crate::sync::target_lifecycle::parse_project_target_id("projects/.").is_err());
    assert!(crate::sync::target_lifecycle::parse_project_target_id("projects/..").is_err());
    assert!(crate::sync::target_lifecycle::parse_project_target_id("projects/a\\b").is_err());
}

///   remote-only discovery 遇到非法 target_id 跳过。
#[test]
fn q6_build_plan_skips_invalid_remote_target_id() {
    let tmp = TempDir::new().unwrap();
    let app_root = tmp.path().join("app");
    let projects_root = tmp.path().join("projects");
    std::fs::create_dir_all(&app_root).unwrap();
    std::fs::create_dir_all(&projects_root).unwrap();

    let mut remote_catalog = TargetLifecycleCatalog::default();
    crate::sync::target_lifecycle::upsert_record(
        &mut remote_catalog,
        crate::sync::types::TargetLifecycleRecord::upsert(
            "projects/../app",
            "projects/../app",
            10_000,
            "dev-A",
        ),
    );
    crate::sync::target_lifecycle::upsert_record(
        &mut remote_catalog,
        crate::sync::types::TargetLifecycleRecord::upsert(
            "projects/legit",
            "projects/legit",
            10_000,
            "dev-A",
        ),
    );

    let planned = crate::sync::full_sync::build_full_sync_target_plan(
        &app_root,
        &projects_root,
        &[],
        &[],
        &remote_catalog,
        &crate::sync::types::SyncPolicy::default(),
        false,
        "dev-B",
        &[],
    );

    let has_traversal = planned
        .iter()
        .any(|t| t.target.remote_prefix.contains(".."));
    assert!(
        !has_traversal,
        "问题6: 非法 target_id 不应进入 plan。planned={:?}",
        planned
            .iter()
            .map(|t| &t.target.remote_prefix)
            .collect::<Vec<_>>()
    );
    let has_legit = planned
        .iter()
        .any(|t| t.target.remote_prefix == "projects/legit");
    assert!(has_legit, "问题6: 合法 target_id 应进入 plan");
}

///   pending deleted 非法 target_id 跳过。
#[test]
fn q6_build_plan_skips_invalid_pending_deleted_target_id() {
    use crate::sync::types::PendingDeletedTarget;

    let tmp = TempDir::new().unwrap();
    let app_root = tmp.path().join("app");
    let projects_root = tmp.path().join("projects");
    std::fs::create_dir_all(&app_root).unwrap();
    std::fs::create_dir_all(&projects_root).unwrap();

    let pending = vec![PendingDeletedTarget {
        target: crate::sync::types::SyncTarget {
            scope: crate::sync::types::SyncScope::Project,
            remote_prefix: "projects/../app".to_string(),
        },
        deleted_at_ms: 1000,
        journal_token: "tok-1".to_string(),
        device_id: "dev-1".to_string(),
        paths: None,
    }];

    let planned = crate::sync::full_sync::build_full_sync_target_plan(
        &app_root,
        &projects_root,
        &[],
        &pending,
        &TargetLifecycleCatalog::default(),
        &crate::sync::types::SyncPolicy::default(),
        false,
        "dev-1",
        &[],
    );

    assert_eq!(
        planned.len(),
        1,
        "问题6: 非法 pending deleted target_id 应被跳过，plan 只含 App target"
    );
    assert_eq!(
        planned[0].target_kind,
        crate::sync::types::PlannedTargetKind::App
    );
}

///   manifest 损坏 → Retry，不 DeleteLocalProject。
#[test]
fn q2_manifest_corrupt_returns_retry() {
    use crate::project::Project;
    use crate::sync::types::PlannedTargetKind;

    let tmp = TempDir::new().unwrap();
    let app_root = tmp.path().join("app");
    let projects_root = tmp.path().join("projects");
    std::fs::create_dir_all(&app_root).unwrap();
    let project_root = projects_root.join("P");
    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::create_dir_all(project_root.join("app-meta/sync")).unwrap();
    std::fs::write(
        project_root.join("app-meta/sync/manifest.sync.json"),
        b"{not valid json",
    )
    .unwrap();

    let projects = vec![Project {
        id: "P".to_string(),
        title: "T".to_string(),
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
        order: 0,
    }];

    let mut remote_catalog = TargetLifecycleCatalog::default();
    crate::sync::target_lifecycle::upsert_record(
        &mut remote_catalog,
        crate::sync::types::TargetLifecycleRecord::delete(
            "projects/P",
            "projects/P",
            12_000,
            "dev-A",
        ),
    );

    let planned = crate::sync::full_sync::build_full_sync_target_plan(
        &app_root,
        &projects_root,
        &projects,
        &[],
        &remote_catalog,
        &crate::sync::types::SyncPolicy::default(),
        false,
        "dev-B",
        &[],
    );

    let p_target = planned
        .iter()
        .find(|t| t.target.remote_prefix == "projects/P")
        .expect("P target should exist");
    assert_eq!(
        p_target.target_kind,
        PlannedTargetKind::Retry,
        "问题2: manifest 损坏应返回 Retry，不 DeleteLocalProject"
    );
}

///   首次同步 manifest 不存在 project.json 合法 → LiveProject。
#[test]
fn q2_first_sync_establishes_initial_manifest() {
    use crate::project::Project;
    use crate::sync::types::PlannedTargetKind;

    let tmp = TempDir::new().unwrap();
    let app_root = tmp.path().join("app");
    let projects_root = tmp.path().join("projects");
    std::fs::create_dir_all(&app_root).unwrap();
    let project_root = projects_root.join("P");
    std::fs::create_dir_all(&project_root).unwrap();
    let project = Project {
        id: "P".to_string(),
        title: "T".to_string(),
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
        order: 0,
    };
    std::fs::write(
        project_root.join("project.json"),
        serde_json::to_vec(&project).unwrap(),
    )
    .unwrap();

    let planned = crate::sync::full_sync::build_full_sync_target_plan(
        &app_root,
        &projects_root,
        &[project],
        &[],
        &TargetLifecycleCatalog::default(),
        &crate::sync::types::SyncPolicy::default(),
        false,
        "dev-B",
        &[],
    );

    let p_target = planned
        .iter()
        .find(|t| t.target.remote_prefix == "projects/P")
        .expect("P target should exist");
    assert_eq!(
        p_target.target_kind,
        PlannedTargetKind::LiveProject,
        "问题2: 首次同步应建立初始 manifest 并判为 LiveProject"
    );
    assert!(
        p_target.live_lww.is_some(),
        "问题2: 首次同步应产出初始 live_lww"
    );
    assert!(
        !project_root
            .join("app-meta/sync/manifest.sync.json")
            .exists(),
        "问题4: planner 不应落盘 manifest（由 staging 阶段写入）"
    );
}

///   manifest 不存在 project.json 损坏 → scan_sync_file 用 mtime fallback。
#[test]
fn q2_no_manifest_and_corrupt_project_json_returns_retry() {
    use crate::project::Project;
    use crate::sync::types::PlannedTargetKind;

    let tmp = TempDir::new().unwrap();
    let app_root = tmp.path().join("app");
    let projects_root = tmp.path().join("projects");
    std::fs::create_dir_all(&app_root).unwrap();
    let project_root = projects_root.join("P");
    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::write(project_root.join("project.json"), b"not json").unwrap();

    let projects = vec![Project {
        id: "P".to_string(),
        title: "T".to_string(),
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
        order: 0,
    }];

    let planned = crate::sync::full_sync::build_full_sync_target_plan(
        &app_root,
        &projects_root,
        &projects,
        &[],
        &TargetLifecycleCatalog::default(),
        &crate::sync::types::SyncPolicy::default(),
        false,
        "dev-B",
        &[],
    );

    let p_target = planned
        .iter()
        .find(|t| t.target.remote_prefix == "projects/P")
        .expect("P target should exist");
    assert_eq!(
        p_target.target_kind,
        PlannedTargetKind::LiveProject,
        "问题4: 损坏 project.json 用 mtime fallback 仍应建立 LWW → LiveProject"
    );
}

///   dry-run 应使用真实 catalog 做 target 决策。
#[test]
fn q5_dry_run_uses_real_catalog_for_target_decision() {
    use crate::sync::types::PlannedTargetKind;

    let tmp = TempDir::new().unwrap();
    let projects_root = tmp.path().join("projects");
    let project_root = projects_root.join("P");
    std::fs::create_dir_all(&project_root).unwrap();
    let project = crate::project::Project {
        id: "P".to_string(),
        title: "P".to_string(),
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
        order: 0,
    };
    std::fs::write(
        project_root.join("project.json"),
        serde_json::to_vec(&project).unwrap(),
    )
    .unwrap();
    std::fs::create_dir_all(project_root.join("app-meta/sync")).unwrap();
    let manifest = SyncManifest {
        files: vec![ManifestFileRecord {
            path: "project.json".to_string(),
            content_hash: "9a0364b9e99bb480dd25e1f0284c8555".to_string(),
            updated_at_ms: 1000,
            deleted_at_ms: None,
            device_id: "device-1".to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        }],
    };
    std::fs::write(
        project_root.join("app-meta/sync/manifest.sync.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();

    let projects = vec![project];

    let mut catalog = TargetLifecycleCatalog::default();
    crate::sync::target_lifecycle::upsert_record(
        &mut catalog,
        crate::sync::types::TargetLifecycleRecord::upsert(
            "projects/P",
            "projects/P",
            2000,
            "device-2",
        ),
    );

    let planned = crate::sync::full_sync::build_full_sync_target_plan(
        tmp.path(),
        &projects_root,
        &projects,
        &[],
        &catalog,
        &crate::sync::types::SyncPolicy::default(),
        false,
        "device-1",
        &[],
    );

    let p_target = planned
        .iter()
        .find(|t| t.project_id.as_deref() == Some("P"))
        .expect("P should be in plan");
    assert!(
        matches!(p_target.target_kind, PlannedTargetKind::LiveProject),
        "P 应为 LiveProject（本地有 + catalog 有 upsert），实际: {:?}",
        p_target.target_kind
    );
}

///   dry-run 用空 catalog 时，首次同步 → LiveProject。
#[test]
fn q5_dry_run_empty_catalog_first_sync() {
    use crate::sync::types::{PlannedTargetKind, TargetLifecycleCatalog};

    let tmp = TempDir::new().unwrap();
    let projects_root = tmp.path().join("projects");
    let project_root = projects_root.join("P");
    std::fs::create_dir_all(&project_root).unwrap();
    let project = crate::project::Project {
        id: "P".to_string(),
        title: "P".to_string(),
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
        order: 0,
    };
    std::fs::write(
        project_root.join("project.json"),
        serde_json::to_vec(&project).unwrap(),
    )
    .unwrap();

    let projects = vec![project];

    let planned = crate::sync::full_sync::build_full_sync_target_plan(
        tmp.path(),
        &projects_root,
        &projects,
        &[],
        &TargetLifecycleCatalog::default(),
        &crate::sync::types::SyncPolicy::default(),
        false,
        "device-1",
        &[],
    );

    let p_target = planned
        .iter()
        .find(|t| t.project_id.as_deref() == Some("P"))
        .expect("P should be in plan");
    assert!(
        matches!(p_target.target_kind, PlannedTargetKind::LiveProject),
        "首次同步 P 应为 LiveProject，实际: {:?}",
        p_target.target_kind
    );
}

///   remote-only Delete 直接生成 RemoteCleanupProject。
#[test]
fn test_remote_only_delete_generates_cleanup_target() {
    use crate::sync::types::{PlannedTargetKind, TargetLifecycleCatalog, TargetLifecycleRecord};

    let tmp = TempDir::new().unwrap();
    let projects_root = tmp.path().join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();

    let mut catalog = TargetLifecycleCatalog::default();
    crate::sync::target_lifecycle::upsert_record(
        &mut catalog,
        TargetLifecycleRecord::delete("projects/P", "projects/P", 20_000, "dev-A"),
    );

    let planned = crate::sync::full_sync::build_full_sync_target_plan(
        tmp.path(),
        &projects_root,
        &[],
        &[],
        &catalog,
        &crate::sync::types::SyncPolicy::default(),
        false,
        "device-local",
        &[],
    );

    let p_target = planned
        .iter()
        .find(|t| t.project_id.as_deref() == Some("P"))
        .expect("remote-only Delete should generate a target for P");
    assert!(
        matches!(
            p_target.target_kind,
            PlannedTargetKind::RemoteCleanupProject
        ),
        "remote-only Delete 应生成 RemoteCleanupProject，实际: {:?}",
        p_target.target_kind
    );
    let expected = p_target
        .expected_delete_lww
        .as_ref()
        .expect("RemoteCleanupProject should carry expected_delete_lww");
    assert_eq!(expected.deleted_at_ms, 20_000);
    assert_eq!(expected.device_id, "dev-A");
}
