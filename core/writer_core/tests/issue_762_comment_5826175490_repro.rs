//! Issue #762 评论 5826175490 / 5828791004 — 同步冲突状态回归测试。
//!
//! 覆盖四件事：
//! 1. Core 全局冲突查询 `list_all_sync_conflicts()` 枚举所有作品，只返回未解决冲突，
//!    并带上 `project_id + project_title`——平台层不需要自己扫目录。
//! 2. 该查询在 UDL/`WriterAppService` 边界暴露为 `ProjectSyncConflictDto`，
//!    单个作品的 `list_sync_conflicts(project_id)` 行为保持不变。
//! 3. full-sync 每个 target 的 `Transfer → Commit` 完成后立即回调 progress，
//!    且此时该作品的持久冲突状态已经落盘——平台不用等最终 `FullSyncResult`
//!    才能让用户处理冲突。
//! 4. 用户在同步运行期间（收到 progress 后）解决的冲突不会被整轮收口重新写回：
//!    每个 target 的 Commit 只在自己 progress 之前做一次，之后不再提交它的 staging。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use tempfile::TempDir;
use writer_core::api::WriterCoreApi;
use writer_core::facade::WriterCore;
use writer_core::sync::full_sync::{
    FullSyncPlan, LiveTargetLww, PlannedTarget, SyncProgressCallback, SyncTargetProgress,
};
use writer_core::sync::provider::memory::MemoryProvider;
use writer_core::sync::provider::model::{RemoteVersion, WritePrecondition};
use writer_core::sync::provider::SyncProvider;
use writer_core::sync::staging::prepare_staging_runs;
use writer_core::sync::target_lifecycle::{
    load_remote_catalog, upsert_record, write_remote_catalog,
};
use writer_core::sync::types::{
    ManifestFileRecord, PlannedTargetKind, RemoteTargetCatalogSnapshot, SyncManifest, SyncPolicy,
    SyncTarget, TargetLifecycleCatalog, TargetLifecycleRecord,
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
// 3. full-sync target progress + 同步中解决冲突不被整轮收口覆盖
// ---------------------------------------------------------------------------

/// 构造真实 live 作品目录（本机已有作品）：本地正文 + manifest LWW。
///
/// `prepare_staging_runs` 从这里 seed `base/` 与 `staging/`（与生产 Phase 2 一致），
/// 测试不手写 staging。
fn build_live_project(
    projects_root: &std::path::Path,
    project_id: &str,
    chapter_content: &[u8],
    lww_time: i64,
    device_id: &str,
) -> std::path::PathBuf {
    let live_root = projects_root.join(project_id);
    std::fs::create_dir_all(live_root.join("volumes").join("v1").join("chapters")).unwrap();
    std::fs::write(
        live_root
            .join("volumes")
            .join("v1")
            .join("chapters")
            .join("chapter.md"),
        chapter_content,
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
    std::fs::create_dir_all(live_root.join("app-meta").join("sync")).unwrap();
    std::fs::write(
        live_root
            .join("app-meta")
            .join("sync")
            .join("manifest.sync.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    live_root
}

/// 远端 generation：chapter 正文 + manifest（内容与本地不同 → BothChanged 冲突）。
fn write_remote_generation_for(
    provider: &MemoryProvider,
    project_id: &str,
    generation_id: &str,
    chapter_content: &[u8],
    lww_time: i64,
    device_id: &str,
) {
    let gen_prefix = format!("projects/{project_id}/__generations__/{generation_id}");
    let chapter_path = format!("{gen_prefix}/volumes/v1/chapters/chapter.md");
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
    let manifest_path = format!("{gen_prefix}/app-meta/sync/manifest.sync.json");
    provider
        .write(
            &manifest_path,
            &serde_json::to_vec(&manifest).unwrap(),
            WritePrecondition::Unconditional,
        )
        .unwrap();
}

/// 远端 catalog：每个作品一条 `Upsert(lww_time, device_id)` + active_generation。
///
/// 入参 `(project_id, generation_id, lww_time, device_id)` 中 device_id 必须大于本地
/// device_id，让 remote record 严格赢，走 `RemoteWins` 分支。
fn write_remote_catalog_for_projects(
    provider: &MemoryProvider,
    projects: &[(&str, &str, i64, &str)],
) -> RemoteTargetCatalogSnapshot {
    let mut catalog = TargetLifecycleCatalog::default();
    for (project_id, generation_id, lww_time, device_id) in projects {
        let prefix = format!("projects/{project_id}");
        upsert_record(
            &mut catalog,
            TargetLifecycleRecord::upsert(&prefix, &prefix, *lww_time, device_id)
                .with_active_generation(*generation_id),
        );
    }
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("v1"),
    };
    write_remote_catalog(provider, &snapshot).unwrap();
    load_remote_catalog(provider).unwrap()
}

/// 单个 LiveProject `PlannedTarget`：`live_lww` 与 live manifest 一致。
fn planned_live_project(
    project_id: &str,
    live_root: std::path::PathBuf,
    lww_time: i64,
    device_id: &str,
) -> PlannedTarget {
    PlannedTarget {
        target: SyncTarget::project(project_id),
        local_root: live_root.clone(),
        staging_root: None,
        target_kind: PlannedTargetKind::LiveProject,
        project_id: Some(project_id.to_string()),
        target_live_root: live_root,
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: Some(LiveTargetLww {
            lww_time_ms: lww_time,
            device_id: device_id.to_string(),
        }),
        expected_delete_lww: None,
    }
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

/// target Commit 完成后必须立即回调 progress，且此时持久冲突状态已落盘可读——
/// "同步中"和"等待用户解决冲突"可以同时成立。
///
/// Issue #762 评论 5828791004 起顺序是 `Transfer → Commit（写 live 终态）→ progress`：
/// 冲突不再由 Transfer 提前落盘，但"progress 回调时冲突已可读"这条不变量必须保持，
/// 否则平台收到 progress 立刻让用户处理时会读到空列表。
#[test]
fn target_progress_exposes_conflict_before_full_sync_finishes() {
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";
    const GEN_EXISTING: &str = "gen_existing";
    assert!(DEVICE_LOCAL < DEVICE_REMOTE);

    let provider = MemoryProvider::new();
    let tmp = TempDir::new().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();

    let live_root = build_live_project(
        &projects_root,
        "p1",
        b"local chapter content",
        T,
        DEVICE_LOCAL,
    );
    write_remote_generation_for(
        &provider,
        "p1",
        GEN_EXISTING,
        b"remote chapter content",
        T,
        DEVICE_REMOTE,
    );
    let remote_catalog_snapshot =
        write_remote_catalog_for_projects(&provider, &[("p1", GEN_EXISTING, T, DEVICE_REMOTE)]);

    let mut plan = FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: true,
        targets: vec![planned_live_project(
            "p1",
            live_root.clone(),
            T,
            DEVICE_LOCAL,
        )],
        app_data_root: app_data_root.clone(),
        remote_catalog_snapshot,
    };
    // 与生产 Phase 2 一致：从 live seed staging（base + staging 克隆）。
    let staging_runs = prepare_staging_runs(&mut plan).unwrap();
    assert_eq!(staging_runs.len(), 1);

    let recorder = ProgressRecorder::new(live_root);
    let progress = recorder.callback();
    let api = WriterCoreApi::new(&app_data_root, &projects_root);

    let result = api
        .perform_full_sync_with_provider(&provider, &plan, staging_runs, None, Some(&progress))
        .unwrap();

    let events = recorder.events.lock().unwrap();
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

    let persisted = recorder.conflicts_on_callback.lock().unwrap();
    assert_eq!(
        persisted[0], 1,
        "progress 回调发生时，该作品的持久冲突状态必须已经落盘可读"
    );
    assert_eq!(
        persisted[0], events[0].conflict_count as usize,
        "progress 的 conflict_count 必须等于已落盘的未解决冲突数"
    );
    drop(events);
    assert_eq!(
        result.overall_status, "partial_conflict",
        "整轮聚合状态必须保留该 target 的未解决冲突"
    );
}

/// 两个作品（p1/p2）在同一轮同步里都产生 `BothChanged` 冲突的完整 fixture。
///
/// live：本地已改正文 + manifest；remote：同路径远端也改了 + catalog record。
/// `prepare_staging_runs` 已按生产 Phase 2 建好 staging。
struct TwoConflictFixture {
    tmp: TempDir,
    provider: MemoryProvider,
    plan: FullSyncPlan,
    staging_runs: Vec<writer_core::sync::staging::StagingRun>,
    /// 每个 target 的 staging run 根目录（断言 target commit 后已清理）。
    staging_run_roots: Vec<std::path::PathBuf>,
    /// target1（p1）的 live 根目录。
    p1_root: std::path::PathBuf,
}

fn two_conflicting_projects_fixture() -> TwoConflictFixture {
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";
    const GEN_EXISTING: &str = "gen_existing";
    assert!(DEVICE_LOCAL < DEVICE_REMOTE);

    let provider = MemoryProvider::new();
    let tmp = TempDir::new().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();

    let p1_root = build_live_project(
        &projects_root,
        "p1",
        b"local chapter content p1",
        T,
        DEVICE_LOCAL,
    );
    let p2_root = build_live_project(
        &projects_root,
        "p2",
        b"local chapter content p2",
        T,
        DEVICE_LOCAL,
    );
    write_remote_generation_for(
        &provider,
        "p1",
        GEN_EXISTING,
        b"remote chapter content p1",
        T,
        DEVICE_REMOTE,
    );
    write_remote_generation_for(
        &provider,
        "p2",
        GEN_EXISTING,
        b"remote chapter content p2",
        T,
        DEVICE_REMOTE,
    );
    let remote_catalog_snapshot = write_remote_catalog_for_projects(
        &provider,
        &[
            ("p1", GEN_EXISTING, T, DEVICE_REMOTE),
            ("p2", GEN_EXISTING, T, DEVICE_REMOTE),
        ],
    );

    let mut plan = FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: true,
        targets: vec![
            planned_live_project("p1", p1_root.clone(), T, DEVICE_LOCAL),
            planned_live_project("p2", p2_root, T, DEVICE_LOCAL),
        ],
        app_data_root,
        remote_catalog_snapshot,
    };
    let staging_runs = prepare_staging_runs(&mut plan).unwrap();
    assert_eq!(staging_runs.len(), 2, "两个 target 各有一个 staging run");
    let staging_run_roots = staging_runs
        .iter()
        .map(|run| run.run_root().to_path_buf())
        .collect();

    TwoConflictFixture {
        tmp,
        provider,
        plan,
        staging_runs,
        staging_run_roots,
        p1_root,
    }
}

/// Issue #762 评论 5828791004 第 1 点：同步过程中解决的冲突不能被整轮收口写回。
///
/// 真实并发顺序（走生产编排 `perform_full_sync_with_provider`）：
/// `target1 Transfer → target1 Commit（冲突写进 live）→ progress`
/// `→ 用户在回调里 resolve_conflict_keep_local("p1")`
/// `→ target2 Transfer + Commit → 整轮聚合收口`。
///
/// 修复前：Transfer 提前写 live，用户 resolve 后整轮结束的 Commit 又把 staging 里
/// 旧的 `state.local.json / conflicts.json` 覆盖回 live，冲突"复活"，
/// 连 keep_local 调整过的 known_files 基线也被覆盖。修复后：每个 target 的 Commit
/// 在它自己的 progress 之前完成，之后不再碰它的 staging。
#[test]
fn resolved_conflict_during_full_sync_is_not_overwritten_at_round_end() {
    const CONFLICT_PATH: &str = "volumes/v1/chapters/chapter.md";

    let fixture = two_conflicting_projects_fixture();
    let app_data_root = fixture.tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    let p1_root = fixture.p1_root.clone();
    let api = Arc::new(WriterCoreApi::new(&app_data_root, &projects_root));

    // progress 回调 = 平台主线程。这里的顺序与平台一致：
    // 收到 progress → 刷新冲突列表 → 用户在同步运行期间立刻处理冲突。
    // resolve 走 API 写锁（`resolve_conflict_keep_local`），因此 progress 必须在
    // 该 target 的 Commit 释放写锁之后发出，否则这里会自锁。
    let observed_at_progress = Arc::new(Mutex::new(Vec::<(String, usize)>::new()));
    let observed = observed_at_progress.clone();
    let api_for_callback = Arc::clone(&api);
    let progress: SyncProgressCallback = Arc::new(move |p: SyncTargetProgress| {
        let Some(project_id) = p.project_id.clone() else {
            return;
        };
        let conflicts = api_for_callback.list_sync_conflicts(&project_id).unwrap();
        observed
            .lock()
            .unwrap()
            .push((project_id.clone(), conflicts.len()));
        if project_id == "p1" {
            assert!(
                api_for_callback
                    .resolve_conflict_keep_local("p1", CONFLICT_PATH)
                    .unwrap(),
                "progress 回调时冲突必须已经可处理"
            );
        }
    });

    let result = api
        .perform_full_sync_with_provider(
            &fixture.provider,
            &fixture.plan,
            fixture.staging_runs,
            None,
            Some(&progress),
        )
        .unwrap();

    // 1) 两个 target 的 progress 发出时，各自的冲突都已经落盘可读。
    let observed_rows = observed_at_progress
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    assert_eq!(
        observed_rows,
        vec![("p1".to_string(), 1), ("p2".to_string(), 1)],
        "每个 target 的 progress 必须在它的 Commit 写进 live 之后才发"
    );

    // 2) 整轮结束（target2 跑完 + 聚合收口）后，target1 仍是用户 keep_local 后的状态。
    assert!(
        api.list_sync_conflicts("p1").unwrap().is_empty(),
        "同步运行期间已解决的冲突不能被整轮收口重新写回"
    );
    let p1_state = SyncService::load_sync_state(&p1_root).unwrap();
    assert!(
        p1_state.conflicted_files.is_empty(),
        "conflicted_files 不能复活为 target1 的旧路径"
    );
    assert_eq!(
        p1_state.known_files.get(CONFLICT_PATH).map(String::as_str),
        Some(format!("{:x}", md5::compute(b"remote chapter content p1")).as_str()),
        "keep_local 调整的 known_files 基线不能被旧 staging state 覆盖"
    );

    // 3) 对照：同一轮里 target2 的冲突仍然在（证明整轮真的跑到了后面，
    //    且 target2 的结果没有被 target1 的 resolve 影响）。
    assert_eq!(
        api.list_sync_conflicts("p2").unwrap().len(),
        1,
        "target2 自己的冲突必须仍然存在"
    );
    assert_eq!(
        result.overall_status, "partial_conflict",
        "整轮聚合状态反映仍存在的 target2 冲突"
    );

    // 4) 结构保证：每个 target commit 后 staging run 已立即清理，
    //    整轮收口没有任何可以"再提交一次"的 staging 残留。
    for run_root in &fixture.staging_run_roots {
        assert!(
            !run_root.exists(),
            "target commit 后 staging run 必须立即清理，整轮结束不能再提交它: {}",
            run_root.display()
        );
    }
}

// ---------------------------------------------------------------------------
// 4. Issue #762 评论 5830266600：已有冲突在 Target Commit 前被解决，仍会复活
// ---------------------------------------------------------------------------

/// 构造带预存在冲突的 live project：chapter.md（本轮新冲突 B）+ existing.md（预存在冲突 A）。
///
/// `prepare_staging_runs` 从 live seed staging，staging 会带上旧的 conflict A 状态。
/// 测试在 seed 后、perform_full_sync 前对 live 调 resolve_conflict_*，模拟用户
/// 在同步开始后、target Commit 前解决冲突。
fn build_live_project_with_preexisting_conflict(
    projects_root: &std::path::Path,
    project_id: &str,
    chapter_content: &[u8],
    lww_time: i64,
    device_id: &str,
) -> std::path::PathBuf {
    let live_root = build_live_project(
        projects_root,
        project_id,
        chapter_content,
        lww_time,
        device_id,
    );
    // 写 existing.md（预存在冲突 A 的本地文件）
    let existing_path = live_root.join("volumes/v1/chapters/existing.md");
    std::fs::write(&existing_path, b"local existing content").unwrap();
    // 记算 local_hash
    let local_hash = format!("{:x}", md5::compute(b"local existing content"));
    // 记算 remote_hash（模拟远端版本）
    let remote_hash = format!("{:x}", md5::compute(b"remote existing content"));
    // 落盘一条未解决冲突 A
    let conflict = SyncConflict {
        local_path: "volumes/v1/chapters/existing.md".to_string(),
        remote_path: "volumes/v1/chapters/existing.md".to_string(),
        kind: SyncConflictKind::BothChanged,
        local_hash,
        remote_hash,
        base_hash: "base-existing-hash".to_string(),
        created_at: 1_000,
        description: "pre-existing conflict".to_string(),
        remote_snapshot_path: None,
    };
    SyncService::record_sync_conflict(&live_root, conflict, Some("local existing content"))
        .unwrap();
    live_root
}

/// 构造带预存在冲突 + remote snapshot 的 live project（take_remote 测试用）。
///
/// 与 [`build_live_project_with_preexisting_conflict`] 的区别：冲突 A 带有
/// `remote_snapshot_path`，让 `resolve_conflict_take_remote` 能真正 apply
/// 远端正文到 live（而非走 pending fallback）。
fn build_live_project_with_preexisting_conflict_and_snapshot(
    projects_root: &std::path::Path,
    project_id: &str,
    chapter_content: &[u8],
    lww_time: i64,
    device_id: &str,
) -> std::path::PathBuf {
    let live_root = build_live_project(
        projects_root,
        project_id,
        chapter_content,
        lww_time,
        device_id,
    );
    // 写 existing.md（预存在冲突 A 的本地文件）
    let existing_path = live_root.join("volumes/v1/chapters/existing.md");
    std::fs::write(&existing_path, b"local existing content").unwrap();
    // 写 remote snapshot 文件（take_remote 会读这个文件替换本地正文）
    let snapshot_rel = "volumes/v1/chapters/existing.md.remote-snapshot";
    let snapshot_path = live_root.join(snapshot_rel);
    std::fs::write(&snapshot_path, b"remote existing content").unwrap();
    // 计算 hashes
    let local_hash = format!("{:x}", md5::compute(b"local existing content"));
    let remote_hash = format!("{:x}", md5::compute(b"remote existing content"));
    // 落盘一条未解决冲突 A（带 remote_snapshot_path）
    let conflict = SyncConflict {
        local_path: "volumes/v1/chapters/existing.md".to_string(),
        remote_path: "volumes/v1/chapters/existing.md".to_string(),
        kind: SyncConflictKind::BothChanged,
        local_hash,
        remote_hash,
        base_hash: "base-existing-hash".to_string(),
        created_at: 1_000,
        description: "pre-existing conflict with snapshot".to_string(),
        remote_snapshot_path: Some(snapshot_rel.to_string()),
    };
    SyncService::record_sync_conflict(&live_root, conflict, Some("local existing content"))
        .unwrap();
    live_root
}

/// Issue #762 评论 5830266600：已有冲突在 Target Commit 前被解决，仍会复活。
///
/// 真实顺序：
/// 1. live 里原本已有冲突 A（p1）
/// 2. prepare_staging_runs 把旧 state/conflicts 复制进 p1 staging
/// 3. 在 p1 Commit 前对 live 调 resolve_conflict_keep_local(A)
/// 4. p1 Transfer/Commit（三方合并应保留 live 的解决结果）
/// 5. p2 继续同步
/// 6. 整轮结束
///
/// 断言：A 仍已解决、conflicted_files 无 A、known_files[A] 未被 staging 旧值覆盖、
/// p1 本轮新冲突 B 仍进入 live。
#[test]
#[allow(clippy::too_many_lines)]
fn preexisting_conflict_resolved_before_target_commit_keep_local_is_not_revived() {
    const CONFLICT_A: &str = "volumes/v1/chapters/existing.md";
    const CONFLICT_B: &str = "volumes/v1/chapters/chapter.md";
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";
    const GEN_EXISTING: &str = "gen_existing";
    assert!(DEVICE_LOCAL < DEVICE_REMOTE);

    let provider = MemoryProvider::new();
    let tmp = TempDir::new().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();

    // p1: 预存在冲突 A + chapter.md（本轮会与远端冲突 → 新冲突 B）
    let p1_root = build_live_project_with_preexisting_conflict(
        &projects_root,
        "p1",
        b"local chapter content p1",
        T,
        DEVICE_LOCAL,
    );
    // p2: 普通冲突（对照）
    let p2_root = build_live_project(
        &projects_root,
        "p2",
        b"local chapter content p2",
        T,
        DEVICE_LOCAL,
    );

    // 远端 generation：chapter.md 内容不同 → BothChanged
    write_remote_generation_for(
        &provider,
        "p1",
        GEN_EXISTING,
        b"remote chapter content p1",
        T,
        DEVICE_REMOTE,
    );
    write_remote_generation_for(
        &provider,
        "p2",
        GEN_EXISTING,
        b"remote chapter content p2",
        T,
        DEVICE_REMOTE,
    );
    let remote_catalog_snapshot = write_remote_catalog_for_projects(
        &provider,
        &[
            ("p1", GEN_EXISTING, T, DEVICE_REMOTE),
            ("p2", GEN_EXISTING, T, DEVICE_REMOTE),
        ],
    );

    let mut plan = FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: true,
        targets: vec![
            planned_live_project("p1", p1_root.clone(), T, DEVICE_LOCAL),
            planned_live_project("p2", p2_root, T, DEVICE_LOCAL),
        ],
        app_data_root: app_data_root.clone(),
        remote_catalog_snapshot,
    };
    // 与生产 Phase 2 一致：从 live seed staging（base + staging 克隆）。
    // 此时 staging 里有旧的 conflict A 状态（从 live 复制）。
    let staging_runs = prepare_staging_runs(&mut plan).unwrap();
    assert_eq!(staging_runs.len(), 2);

    // 在 p1 Commit 前对 live 调 resolve_conflict_keep_local(A)。
    // 模拟用户在同步开始后、target1 Commit 前解决冲突。
    // 此时 live 已无冲突 A，但 staging 里有旧的。
    SyncService::resolve_conflict_keep_local(&p1_root, CONFLICT_A).unwrap();

    // 验证 resolve 确实生效了
    let p1_state_before = SyncService::load_sync_state(&p1_root).unwrap();
    assert!(
        !p1_state_before.conflicted_files.contains(CONFLICT_A),
        "resolve 后 live 应无冲突 A"
    );
    let keep_local_hash = format!("{:x}", md5::compute(b"remote existing content"));
    assert_eq!(
        p1_state_before
            .known_files
            .get(CONFLICT_A)
            .map(String::as_str),
        Some(keep_local_hash.as_str()),
        "resolve 后 known_files[A] 应为 remote_hash"
    );

    let api = WriterCoreApi::new(&app_data_root, &projects_root);
    let result = api
        .perform_full_sync_with_provider(&provider, &plan, staging_runs, None, None)
        .unwrap();

    // 断言 1：A 仍已解决（不被 staging 旧状态复活）
    let p1_conflicts = api.list_sync_conflicts("p1").unwrap();
    assert!(
        p1_conflicts.iter().all(|c| c.local_path != CONFLICT_A),
        "已解决的冲突 A 不应被 staging 旧状态复活"
    );

    // 断言 2：conflicted_files 无 A
    let p1_state = SyncService::load_sync_state(&p1_root).unwrap();
    assert!(
        !p1_state.conflicted_files.contains(CONFLICT_A),
        "conflicted_files 不能复活冲突 A"
    );

    // 断言 3：known_files[A] 未被 staging 旧值覆盖
    assert_eq!(
        p1_state.known_files.get(CONFLICT_A).map(String::as_str),
        Some(keep_local_hash.as_str()),
        "keep_local 调整的 known_files[A] 不能被旧 staging state 覆盖"
    );

    // 断言 4：p1 本轮新冲突 B 仍进入 live
    assert!(
        p1_conflicts.iter().any(|c| c.local_path == CONFLICT_B),
        "本轮新冲突 B 应进入 live"
    );
    assert!(
        p1_state.conflicted_files.contains(CONFLICT_B),
        "conflicted_files 应包含新冲突 B"
    );

    // 断言 5：p2 的冲突仍然在（对照）
    let p2_conflicts = api.list_sync_conflicts("p2").unwrap();
    assert_eq!(p2_conflicts.len(), 1, "p2 自己的冲突必须仍然存在");
    assert_eq!(
        result.overall_status, "partial_conflict",
        "整轮聚合状态反映仍存在的冲突"
    );
}

/// Issue #762 评论 5830266600：take_remote 路径——已有冲突在 Target Commit 前被解决，
/// 仍会复活。
///
/// 与 keep_local 测试的区别：用 `resolve_conflict_take_remote` 解决冲突 A。
/// take_remote 除了 conflict metadata 还会改 live 正文/known_files，
/// 最容易暴露旧 staging 覆盖问题。
#[test]
#[allow(clippy::too_many_lines)]
fn preexisting_conflict_resolved_before_target_commit_take_remote_is_not_revived() {
    const CONFLICT_A: &str = "volumes/v1/chapters/existing.md";
    const CONFLICT_B: &str = "volumes/v1/chapters/chapter.md";
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";
    const GEN_EXISTING: &str = "gen_existing";
    assert!(DEVICE_LOCAL < DEVICE_REMOTE);

    let provider = MemoryProvider::new();
    let tmp = TempDir::new().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();

    // p1: 预存在冲突 A（带 remote snapshot）+ chapter.md（本轮新冲突 B）
    let p1_root = build_live_project_with_preexisting_conflict_and_snapshot(
        &projects_root,
        "p1",
        b"local chapter content p1",
        T,
        DEVICE_LOCAL,
    );
    // p2: 普通冲突（对照）
    let p2_root = build_live_project(
        &projects_root,
        "p2",
        b"local chapter content p2",
        T,
        DEVICE_LOCAL,
    );

    write_remote_generation_for(
        &provider,
        "p1",
        GEN_EXISTING,
        b"remote chapter content p1",
        T,
        DEVICE_REMOTE,
    );
    write_remote_generation_for(
        &provider,
        "p2",
        GEN_EXISTING,
        b"remote chapter content p2",
        T,
        DEVICE_REMOTE,
    );
    let remote_catalog_snapshot = write_remote_catalog_for_projects(
        &provider,
        &[
            ("p1", GEN_EXISTING, T, DEVICE_REMOTE),
            ("p2", GEN_EXISTING, T, DEVICE_REMOTE),
        ],
    );

    let mut plan = FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: true,
        targets: vec![
            planned_live_project("p1", p1_root.clone(), T, DEVICE_LOCAL),
            planned_live_project("p2", p2_root, T, DEVICE_LOCAL),
        ],
        app_data_root: app_data_root.clone(),
        remote_catalog_snapshot,
    };
    let staging_runs = prepare_staging_runs(&mut plan).unwrap();
    assert_eq!(staging_runs.len(), 2);

    // 在 p1 Commit 前对 live 调 resolve_conflict_take_remote(A)。
    // take_remote 会用 remote snapshot 替换本地正文，并更新 known_files。
    let applied = SyncService::resolve_conflict_take_remote(&p1_root, CONFLICT_A).unwrap();
    assert!(applied, "take_remote 应已立即应用（有 remote snapshot）");

    // 验证 resolve 确实生效了
    let p1_state_before = SyncService::load_sync_state(&p1_root).unwrap();
    assert!(
        !p1_state_before.conflicted_files.contains(CONFLICT_A),
        "resolve 后 live 应无冲突 A"
    );
    let take_remote_hash = format!("{:x}", md5::compute(b"remote existing content"));
    assert_eq!(
        p1_state_before
            .known_files
            .get(CONFLICT_A)
            .map(String::as_str),
        Some(take_remote_hash.as_str()),
        "resolve 后 known_files[A] 应为 remote_hash"
    );
    // 验证本地正文已被替换为远端版本
    let existing_content = std::fs::read_to_string(p1_root.join(CONFLICT_A)).unwrap();
    assert_eq!(
        existing_content, "remote existing content",
        "take_remote 后本地正文应替换为远端版本"
    );

    let api = WriterCoreApi::new(&app_data_root, &projects_root);
    let result = api
        .perform_full_sync_with_provider(&provider, &plan, staging_runs, None, None)
        .unwrap();

    // 断言 1：A 仍已解决（不被 staging 旧状态复活）
    let p1_conflicts = api.list_sync_conflicts("p1").unwrap();
    assert!(
        p1_conflicts.iter().all(|c| c.local_path != CONFLICT_A),
        "已解决的冲突 A 不应被 staging 旧状态复活（take_remote 路径）"
    );

    // 断言 2：conflicted_files 无 A
    let p1_state = SyncService::load_sync_state(&p1_root).unwrap();
    assert!(
        !p1_state.conflicted_files.contains(CONFLICT_A),
        "conflicted_files 不能复活冲突 A（take_remote 路径）"
    );

    // 断言 3：known_files[A] 未被 staging 旧值覆盖
    assert_eq!(
        p1_state.known_files.get(CONFLICT_A).map(String::as_str),
        Some(take_remote_hash.as_str()),
        "take_remote 调整的 known_files[A] 不能被旧 staging state 覆盖"
    );

    // 断言 4：本地正文未被 staging 旧值覆盖
    let existing_after = std::fs::read_to_string(p1_root.join(CONFLICT_A)).unwrap();
    assert_eq!(
        existing_after, "remote existing content",
        "take_remote 替换的本地正文不能被旧 staging 覆盖"
    );

    // 断言 5：p1 本轮新冲突 B 仍进入 live
    assert!(
        p1_conflicts.iter().any(|c| c.local_path == CONFLICT_B),
        "本轮新冲突 B 应进入 live"
    );
    assert!(
        p1_state.conflicted_files.contains(CONFLICT_B),
        "conflicted_files 应包含新冲突 B"
    );

    // 断言 6：p2 的冲突仍然在（对照）
    let p2_conflicts = api.list_sync_conflicts("p2").unwrap();
    assert_eq!(p2_conflicts.len(), 1, "p2 自己的冲突必须仍然存在");
    assert_eq!(
        result.overall_status, "partial_conflict",
        "整轮聚合状态反映仍存在的冲突"
    );
}
