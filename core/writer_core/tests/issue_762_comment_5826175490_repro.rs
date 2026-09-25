//! Issue #762 评论 5826175490 — 已有冲突被藏到整轮同步结束的回归测试。
//!
//! 覆盖三件事：
//! 1. Core 全局冲突查询 `list_all_sync_conflicts()` 枚举所有作品，只返回未解决冲突，
//!    并带上 `project_id + project_title`——平台层不需要自己扫目录。
//! 2. 该查询在 UDL/`WriterAppService` 边界暴露为 `ProjectSyncConflictDto`，
//!    单个作品的 `list_sync_conflicts(project_id)` 行为保持不变。
//! 3. full-sync 每个 target 结束时立即回调 progress，且此时该作品的持久冲突状态
//!    已经落盘——平台不用等最终 `FullSyncResult` 才能让用户处理冲突。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use tempfile::TempDir;
use writer_core::facade::WriterCore;
use writer_core::sync::full_sync::{
    run_transfer, FullSyncPlan, LiveTargetLww, PlannedTarget, SyncProgressCallback,
    SyncTargetProgress,
};
use writer_core::sync::provider::memory::MemoryProvider;
use writer_core::sync::provider::model::{RemoteVersion, WritePrecondition};
use writer_core::sync::provider::SyncProvider;
use writer_core::sync::target_lifecycle::{
    load_remote_catalog, upsert_record, write_remote_catalog,
};
use writer_core::sync::types::{
    ManifestFileRecord, PlannedTargetKind, RemoteTargetCatalogSnapshot, SyncManifest, SyncPolicy,
    SyncStatus, SyncTarget, TargetLifecycleCatalog, TargetLifecycleRecord,
};
use writer_core::sync::{SyncConflict, SyncConflictKind, SyncService};

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

/// 构造测试用 WriterCore 实例（facade 层，不需要 git layout）。
fn make_core() -> (TempDir, WriterCore) {
    let temp_dir = tempfile::tempdir().unwrap();
    let app_data_root = temp_dir.path();
    std::fs::create_dir_all(app_data_root.join("projects")).unwrap();
    let core = WriterCore::new(app_data_root, app_data_root.join("projects"));
    (temp_dir, core)
}

/// 造一条最小可用的未解决冲突记录。
fn make_conflict(local_path: &str) -> SyncConflict {
    SyncConflict {
        local_path: local_path.to_string(),
        remote_path: local_path.to_string(),
        kind: SyncConflictKind::BothChanged,
        local_hash: "local-hash".to_string(),
        remote_hash: "remote-hash".to_string(),
        base_hash: "base-hash".to_string(),
        created_at: 1_000,
        description: "both changed".to_string(),
        remote_snapshot_path: None,
    }
}

/// 用真实的 conflict loader 落盘一条未解决冲突到指定作品。
fn record_conflict(project_root: &std::path::Path, local_path: &str) {
    SyncService::record_sync_conflict(project_root, make_conflict(local_path), Some("local body"))
        .unwrap();
}

// ---------------------------------------------------------------------------
// 1. Core 全局冲突查询
// ---------------------------------------------------------------------------

/// 全局查询必须覆盖所有作品，只返回 unresolved，并带 project_id + project_title。
///
/// 修复前：平台只能按 workspaceProjectId 查当前作品，日志里其他作品的 unresolved
/// conflict 完全没有入口。
#[test]
fn list_all_sync_conflicts_enumerates_only_unresolved_across_projects() {
    let (temp_dir, core) = make_core();
    let projects_root = temp_dir.path().join("projects");

    let alpha = core.create_project("作品甲").unwrap();
    let beta = core.create_project("作品乙").unwrap();
    let gamma = core.create_project("作品丙").unwrap();

    record_conflict(&projects_root.join(&alpha.id), "volumes/v1/chapters/a.md");
    record_conflict(&projects_root.join(&gamma.id), "volumes/v1/chapters/g1.md");
    record_conflict(&projects_root.join(&gamma.id), "volumes/v1/chapters/g2.md");

    let all = core.list_all_sync_conflicts().unwrap();

    let mut project_ids: Vec<String> = all.iter().map(|c| c.project_id.clone()).collect();
    project_ids.sort();
    project_ids.dedup();
    let mut expected = vec![alpha.id.clone(), gamma.id.clone()];
    expected.sort();
    assert_eq!(
        project_ids, expected,
        "只应返回有未解决冲突的作品，作品乙（无冲突）不应出现"
    );
    assert!(
        all.iter().all(|c| c.project_id != beta.id),
        "作品乙没有冲突，不应出现在全局入口"
    );

    assert_eq!(all.len(), 3, "作品甲 1 处 + 作品丙 2 处");
    let alpha_entries: Vec<_> = all.iter().filter(|c| c.project_id == alpha.id).collect();
    assert_eq!(alpha_entries.len(), 1);
    assert_eq!(alpha_entries[0].project_title, "作品甲");
    assert_eq!(
        alpha_entries[0].conflict.local_path,
        "volumes/v1/chapters/a.md"
    );

    let gamma_entries: Vec<_> = all.iter().filter(|c| c.project_id == gamma.id).collect();
    assert_eq!(gamma_entries.len(), 2);
    assert!(gamma_entries.iter().all(|c| c.project_title == "作品丙"));
}

/// 冲突被解决后必须从全局列表中消失——"只返回 unresolved"不能只靠文件里有没有记录。
#[test]
fn list_all_sync_conflicts_drops_resolved_conflict() {
    let (temp_dir, core) = make_core();
    let projects_root = temp_dir.path().join("projects");

    let project = core.create_project("作品甲").unwrap();
    let project_root = projects_root.join(&project.id);
    record_conflict(&project_root, "volumes/v1/chapters/a.md");
    assert_eq!(core.list_all_sync_conflicts().unwrap().len(), 1);

    SyncService::resolve_conflict_mark_merged(&project_root, "volumes/v1/chapters/a.md").unwrap();

    assert!(
        core.list_all_sync_conflicts().unwrap().is_empty(),
        "已解决冲突不应继续出现在全局冲突入口"
    );
}

/// 单个作品查询保留给具体作品侧栏使用，与全局查询看到同一份数据。
#[test]
fn single_project_query_still_works_alongside_global_query() {
    let (temp_dir, core) = make_core();
    let projects_root = temp_dir.path().join("projects");

    let alpha = core.create_project("作品甲").unwrap();
    let beta = core.create_project("作品乙").unwrap();
    record_conflict(&projects_root.join(&alpha.id), "volumes/v1/chapters/a.md");
    record_conflict(&projects_root.join(&beta.id), "volumes/v1/chapters/b.md");

    assert_eq!(core.list_sync_conflicts(&alpha.id).unwrap().len(), 1);
    assert_eq!(core.list_sync_conflicts(&beta.id).unwrap().len(), 1);

    let all = core.list_all_sync_conflicts().unwrap();
    assert_eq!(all.len(), 2);
}

// ---------------------------------------------------------------------------
// 2. UDL / WriterAppService 边界
// ---------------------------------------------------------------------------

/// `ProjectSyncConflictDto` 必须带 camelCase 的 projectId/projectTitle/conflict，
/// 平台层（Linux QML）按 projectId 分组、按 conflict.localPath 打开目标冲突。
#[test]
fn app_service_exposes_project_sync_conflict_dto() {
    let (temp_dir, _core) = make_core();
    let app_data_root = temp_dir.path();
    let projects_root = app_data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();

    let api = writer_core::api::bootstrap::bootstrap_core_api(
        app_data_root,
        projects_root.clone(),
        None,
        None,
    )
    .expect("bootstrap_core_api 应成功");

    let project = api.create_project("作品甲").unwrap();
    let project_id = project.id.clone();
    record_conflict(&projects_root.join(&project_id), "volumes/v1/chapters/a.md");

    let all = api.list_all_sync_conflicts().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].project_id, project_id);
    assert_eq!(all[0].project_title, "作品甲");
    assert_eq!(all[0].conflict.local_path, "volumes/v1/chapters/a.md");

    let json = serde_json::to_value(&all[0]).unwrap();
    assert!(json.get("projectId").is_some(), "线格式必须是 projectId");
    assert!(
        json.get("projectTitle").is_some(),
        "线格式必须是 projectTitle"
    );
    assert!(json.get("conflict").is_some());
    assert_eq!(
        json["conflict"]["localPath"],
        serde_json::Value::String("volumes/v1/chapters/a.md".to_string())
    );

    // 单项目接口保留，且与全局接口一致。
    let single = api.list_sync_conflicts(&project_id).unwrap();
    assert_eq!(single.len(), 1);
    assert_eq!(single[0].local_path, "volumes/v1/chapters/a.md");
}

// ---------------------------------------------------------------------------
// 3. full-sync target progress
// ---------------------------------------------------------------------------

/// 构造本地 staging：manifest LWW = (lww_time, device_id)，一个 chapter 文件。
fn build_staging_doc_conflict(
    tmp: &TempDir,
    lww_time: i64,
    device_id: &str,
    chapter_content: &[u8],
) -> std::path::PathBuf {
    let staging_root = tmp.path().join("staging-p1");
    std::fs::create_dir_all(staging_root.join("volumes").join("v1").join("chapters")).unwrap();
    std::fs::write(
        staging_root
            .join("volumes")
            .join("v1")
            .join("chapters")
            .join("chapter.md"),
        chapter_content,
    )
    .unwrap();
    std::fs::create_dir_all(staging_root.join("app-meta").join("sync")).unwrap();
    let staging_manifest = SyncManifest {
        files: vec![ManifestFileRecord {
            path: "volumes/v1/chapters/chapter.md".to_string(),
            content_hash: format!("{:x}", md5::compute(chapter_content)),
            updated_at_ms: lww_time,
            deleted_at_ms: None,
            device_id: device_id.to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        }],
    };
    std::fs::write(
        staging_root
            .join("app-meta")
            .join("sync")
            .join("manifest.sync.json"),
        serde_json::to_vec(&staging_manifest).unwrap(),
    )
    .unwrap();
    staging_root
}

/// 远端 generation 里放一份与本地不同的 chapter，触发 BothChanged 冲突。
fn write_remote_generation(
    provider: &MemoryProvider,
    gen_prefix: &str,
    chapter_content: &[u8],
    lww_time: i64,
    device_id: &str,
) {
    let chapter_path = format!("{}/volumes/v1/chapters/chapter.md", gen_prefix);
    provider
        .write(
            &chapter_path,
            chapter_content,
            WritePrecondition::Unconditional,
        )
        .unwrap();
    let manifest = SyncManifest {
        files: vec![ManifestFileRecord {
            path: "volumes/v1/chapters/chapter.md".to_string(),
            content_hash: format!("{:x}", md5::compute(chapter_content)),
            updated_at_ms: lww_time,
            deleted_at_ms: None,
            device_id: device_id.to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        }],
    };
    let manifest_path = format!("{}/app-meta/sync/manifest.sync.json", gen_prefix);
    provider
        .write(
            &manifest_path,
            &serde_json::to_vec(&manifest).unwrap(),
            WritePrecondition::Unconditional,
        )
        .unwrap();
}

/// 记录每次 progress 回调时该作品磁盘上的 conflicts.json 快照。
///
/// 用来证明"立即写持久 conflict state → 立即回调 progress"的顺序：
/// 平台收到 progress 时冲突已经可读，不需要等最终 `FullSyncResult`。
struct ProgressRecorder {
    events: Arc<Mutex<Vec<SyncTargetProgress>>>,
    conflicts_on_callback: Arc<Mutex<Vec<usize>>>,
    project_root: std::path::PathBuf,
}

impl ProgressRecorder {
    fn new(project_root: std::path::PathBuf) -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            conflicts_on_callback: Arc::new(Mutex::new(Vec::new())),
            project_root,
        }
    }

    fn callback(&self) -> SyncProgressCallback {
        let events = self.events.clone();
        let seen_counts = self.conflicts_on_callback.clone();
        let project_root = self.project_root.clone();
        Arc::new(move |progress: SyncTargetProgress| {
            let persisted = SyncService::list_conflicts(&project_root)
                .map(|c| c.len())
                .unwrap_or(0);
            if let Ok(mut guard) = seen_counts.lock() {
                guard.push(persisted);
            }
            if let Ok(mut guard) = events.lock() {
                guard.push(progress);
            }
        })
    }
}

/// target 完成 merge、确认 unresolved_conflicts 后必须立即回调 progress，
/// 且此时持久冲突状态已落盘——"同步中"和"等待用户解决冲突"可以同时成立。
#[test]
fn target_progress_exposes_conflict_before_full_sync_finishes() {
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";
    const GEN_EXISTING: &str = "gen_existing";
    assert!(DEVICE_LOCAL < DEVICE_REMOTE);

    let provider_inner = MemoryProvider::new();
    let remote_gen_prefix = format!("projects/p1/__generations__/{}", GEN_EXISTING);
    write_remote_generation(
        &provider_inner,
        &remote_gen_prefix,
        b"remote chapter content",
        T,
        DEVICE_REMOTE,
    );

    let remote_record =
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", T, DEVICE_REMOTE)
            .with_active_generation(GEN_EXISTING);
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut catalog, remote_record);
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("v1"),
    };
    write_remote_catalog(&provider_inner, &snapshot).unwrap();
    let remote_catalog_snapshot = load_remote_catalog(&provider_inner).unwrap();

    let tmp = TempDir::new().unwrap();
    let staging_root = build_staging_doc_conflict(&tmp, T, DEVICE_LOCAL, b"local chapter content");
    let local_root = tmp.path().join("projects").join("p1");
    std::fs::create_dir_all(&local_root).unwrap();
    let plan = FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: false,
        targets: vec![PlannedTarget {
            target: SyncTarget::project("p1"),
            local_root: local_root.clone(),
            staging_root: Some(staging_root),
            target_kind: PlannedTargetKind::LiveProject,
            project_id: Some("p1".to_string()),
            target_live_root: local_root.clone(),
            deleted_journal_token: None,
            deleted_lww: None,
            live_lww: Some(LiveTargetLww {
                lww_time_ms: T,
                device_id: DEVICE_LOCAL.to_string(),
            }),
            expected_delete_lww: None,
        }],
        app_data_root: tmp.path().to_path_buf(),
        remote_catalog_snapshot,
    };

    let recorder = ProgressRecorder::new(local_root);
    let progress = recorder.callback();

    let transfer = run_transfer(&provider_inner, &plan, None, Some(&progress));

    // 冲突确实产生了。
    assert!(
        matches!(
            transfer.targets[0].result.status,
            SyncStatus::PartialConflict
        ),
        "应进入 PartialConflict，实际 {:?}",
        transfer.targets[0].result.status
    );
    assert!(
        !transfer.targets[0].result.conflicts.is_empty(),
        "target 结果里应有未解决冲突"
    );

    let events = recorder.events.lock().unwrap();
    let persisted = recorder.conflicts_on_callback.lock().unwrap();
    assert_eq!(events.len(), 1, "每个 target 结束回调一次 progress");
    assert_eq!(
        events[0].project_id.as_deref(),
        Some("p1"),
        "progress 必须带 project_id，平台据此定位作品"
    );
    assert_eq!(events[0].target_kind, "project");
    assert_eq!(
        events[0].status, "partial_conflict",
        "progress 的 status 必须是线格式状态码，平台据此判断 target 终态"
    );
    assert_eq!(
        events[0].conflict_count as usize,
        transfer.targets[0].result.conflicts.len(),
        "progress 的 conflict_count 必须等于该 target 的未解决冲突数"
    );
    assert_eq!(
        persisted[0], events[0].conflict_count as usize,
        "progress 回调发生时，该作品的持久冲突状态必须已经落盘可读"
    );
}
