//! Issue #762 评论 5834136935 回归测试 — 跨 WriterCoreApi 实例的 state/conflict
//! mutation 串行化。
//!
//! 背景：后台同步线程和 UI resolve 线程各自新建独立的 `WriterCoreApi` 实例。
//! `core_instance: RwLock<WriterCore>` 是每个实例自己的字段，锁不住另一个实例。
//! 竞态窗口：
//! 1. live 里已有冲突 A；
//! 2. target Commit 开始，`record_staging_conflicts` load live（A unresolved）；
//! 3. 在 persist 写回之前，UI 用另一个 API 实例 resolve A；
//! 4. Commit 用第 2 步已 load 的 state 写回，A 复活。
//!
//! 修复（`sync/state_lock.rs`）：按规范化 sync_root 分桶的进程级锁，
//! `commit_target_after_transfer` 和三个 resolve API 都持同一把 root lock。
//!
//! 本测试用**两个独立 WriterCoreApi 实例**（`api_sync` + `api_ui`）验证：
//! 同步线程跑 `perform_full_sync_with_provider`，UI 线程并发 resolve 预存在冲突 A。
//! 有锁时无论谁先谁后 A 都保持已解决；无锁时间歇性 A 被 Commit 复活。
//! 循环 10 次提高竞态命中率，每次用新 TempDir 避免状态残留。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use writer_core::api::WriterCoreApi;
use writer_core::sync::full_sync::{FullSyncPlan, LiveTargetLww, PlannedTarget};
use writer_core::sync::provider::capabilities::SyncCapabilities;
use writer_core::sync::provider::error::ProviderError;
use writer_core::sync::provider::memory::MemoryProvider;
use writer_core::sync::provider::model::{
    DeletePrecondition, RemoteEntry, RemoteObject, RemoteVersion, WritePrecondition,
};
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
// 辅助函数（与 issue_762_comment_5832557522_integration.rs /
// issue_762_comment_5826175490_repro.rs 同构）
// ---------------------------------------------------------------------------

/// 构造 live project：本地正文 + manifest LWW。
fn build_live_project(
    projects_root: &Path,
    project_id: &str,
    chapter_content: &[u8],
    lww_time: i64,
    device_id: &str,
) -> PathBuf {
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

/// 单个 LiveProject `PlannedTarget`。
fn planned_live_project(
    project_id: &str,
    live_root: PathBuf,
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

/// 构造带预存在冲突 + remote snapshot 的 live project。
///
/// `chapter.md`（本轮会与远端冲突 → 新冲突 B，触发 `record_staging_conflicts`）
/// + `existing.md`（预存在冲突 A，带 `remote_snapshot_path`，让
///   `resolve_conflict_take_remote` 能真正 apply 远端正文到 live）。
fn build_live_project_with_preexisting_conflict_and_snapshot(
    projects_root: &Path,
    project_id: &str,
    chapter_content: &[u8],
    lww_time: i64,
    device_id: &str,
) -> PathBuf {
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

// ---------------------------------------------------------------------------
// SlowProvider — 包装 MemoryProvider，每次远端调用前 sleep，让 Transfer 慢下来，
// 增加 UI resolve 与 Commit 的交错概率。
// ---------------------------------------------------------------------------

struct SlowProvider {
    inner: MemoryProvider,
    delay: Duration,
}

impl SyncProvider for SlowProvider {
    fn capabilities(&self) -> SyncCapabilities {
        self.inner.capabilities()
    }

    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
        std::thread::sleep(self.delay);
        self.inner.list(prefix)
    }

    fn read(&self, path: &str) -> Result<Option<RemoteObject>, ProviderError> {
        std::thread::sleep(self.delay);
        self.inner.read(path)
    }

    fn write(
        &self,
        path: &str,
        content: &[u8],
        precondition: WritePrecondition,
    ) -> Result<RemoteVersion, ProviderError> {
        std::thread::sleep(self.delay);
        self.inner.write(path, content, precondition)
    }

    fn delete(&self, path: &str, precondition: DeletePrecondition) -> Result<(), ProviderError> {
        std::thread::sleep(self.delay);
        self.inner.delete(path, precondition)
    }
}

// ---------------------------------------------------------------------------
// 公共 setup：构造 plan + staging_runs + 两个独立 API 实例
// ---------------------------------------------------------------------------

/// 常量。
const CONFLICT_A: &str = "volumes/v1/chapters/existing.md";
const T: i64 = 10_000;
const DEVICE_REMOTE: &str = "device_remote";
const DEVICE_LOCAL: &str = "device_local";
const GEN_EXISTING: &str = "gen_existing";

/// 一轮竞态测试的公共 setup。
struct RoundSetup {
    // TempDir 必须存活到本轮检查结束，放在这里由调用方持有。
    _tmp: TempDir,
    p1_root: PathBuf,
    api_sync: Arc<WriterCoreApi>,
    api_ui: Arc<WriterCoreApi>,
    slow_provider: SlowProvider,
    plan: FullSyncPlan,
    staging_runs: Vec<writer_core::sync::staging::StagingRun>,
}

/// 构造一轮测试：live 预存在冲突 A + chapter.md（本轮新冲突 B）+ 远端 generation。
fn setup_round() -> RoundSetup {
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

    // 远端 generation：chapter.md 内容不同 → BothChanged 新冲突 B
    write_remote_generation_for(
        &provider,
        "p1",
        GEN_EXISTING,
        b"remote chapter content p1",
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
        targets: vec![planned_live_project("p1", p1_root.clone(), T, DEVICE_LOCAL)],
        app_data_root: app_data_root.clone(),
        remote_catalog_snapshot,
    };
    let staging_runs = prepare_staging_runs(&mut plan).unwrap();
    assert_eq!(staging_runs.len(), 1, "一个 target 一个 staging run");

    // 两个独立 API 实例 — 关键：指向同一 app/projects root，但 core_instance 独立。
    let api_sync = Arc::new(WriterCoreApi::new(&app_data_root, &projects_root));
    let api_ui = Arc::new(WriterCoreApi::new(&app_data_root, &projects_root));

    let slow_provider = SlowProvider {
        inner: provider,
        delay: Duration::from_millis(2),
    };

    RoundSetup {
        _tmp: tmp,
        p1_root,
        api_sync,
        api_ui,
        slow_provider,
        plan,
        staging_runs,
    }
}

/// UI 线程循环体：检查冲突 A，有就 resolve。返回 true 表示执行了一次 resolve。
fn ui_try_resolve(api_ui: &WriterCoreApi, keep_local: bool) -> bool {
    let conflicts = api_ui.list_sync_conflicts("p1").unwrap();
    if conflicts.iter().any(|c| c.local_path == CONFLICT_A) {
        if keep_local {
            let _ = api_ui.resolve_conflict_keep_local("p1", CONFLICT_A);
        } else {
            let _ = api_ui.resolve_conflict_take_remote("p1", CONFLICT_A);
        }
        return true;
    }
    false
}

/// 驱动一轮竞态测试：同步线程跑 full sync，UI 线程并发 resolve。
/// `keep_local` = true 用 keep_local，false 用 take_remote。
/// 返回 `(api_ui, p1_root, _tmp)`，`_tmp` 必须由调用方持有到本轮检查结束。
fn run_concurrency_round(keep_local: bool) -> (Arc<WriterCoreApi>, PathBuf, TempDir) {
    let RoundSetup {
        _tmp,
        p1_root,
        api_sync,
        api_ui,
        slow_provider,
        plan,
        staging_runs,
    } = setup_round();

    let sync_done = Arc::new(AtomicBool::new(false));

    // 同步线程：跑 full sync，结束后设 sync_done = true。
    let sync_done_for_sync = Arc::clone(&sync_done);
    let api_sync_for_thread = Arc::clone(&api_sync);
    let sync_thread = thread::spawn(move || {
        let result = api_sync_for_thread.perform_full_sync_with_provider(
            &slow_provider,
            &plan,
            staging_runs,
            None,
            None,
            None,
        );
        sync_done_for_sync.store(true, Ordering::SeqCst);
        result
    });

    // UI 线程：在同步运行期间持续 resolve 冲突 A。
    let api_ui_for_thread = Arc::clone(&api_ui);
    let sync_done_for_ui = Arc::clone(&sync_done);
    let ui_thread = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !sync_done_for_ui.load(Ordering::SeqCst) {
            ui_try_resolve(&api_ui_for_thread, keep_local);
            thread::sleep(Duration::from_millis(1));
            if Instant::now() > deadline {
                break;
            }
        }
    });

    // join 两个线程。
    sync_thread
        .join()
        .expect("sync thread panicked")
        .expect("perform_full_sync_with_provider 应成功");
    ui_thread.join().expect("ui thread panicked");

    (api_ui, p1_root, _tmp)
}

// ---------------------------------------------------------------------------
// 测试函数
// ---------------------------------------------------------------------------

/// 跨 API 实例：同步 Commit vs UI resolve_conflict_keep_local。
///
/// 有 state_lock 时：resolve 和 Commit 互斥，无论谁先谁后 A 都保持已解决。
/// 无锁时间歇性失败：resolve 被 Commit 的旧 merged state 覆盖，A 复活。
#[test]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn cross_api_lock_commit_vs_resolve_keep_local() {
    for round in 0..10 {
        let (api_ui, p1_root, _tmp) = run_concurrency_round(true);

        // 断言 1：A 已解决（没被 Commit 复活）。
        let conflicts = api_ui.list_sync_conflicts("p1").unwrap();
        assert!(
            conflicts.iter().all(|c| c.local_path != CONFLICT_A),
            "round {round}: keep_local — 已解决的冲突 A 不应被 Commit 复活\
             （list_sync_conflicts 仍含 A：{:?}）",
            conflicts
                .iter()
                .filter(|c| c.local_path == CONFLICT_A)
                .collect::<Vec<_>>()
        );

        // 断言 2：keep_local 保留本地正文。
        let existing_content = std::fs::read_to_string(p1_root.join(CONFLICT_A)).unwrap();
        assert_eq!(
            existing_content, "local existing content",
            "round {round}: keep_local 后 existing.md 应保留本地内容"
        );
    }
}

/// 跨 API 实例：同步 Commit vs UI resolve_conflict_take_remote。
///
/// 有 state_lock 时：resolve 和 Commit 互斥，无论谁先谁后 A 都保持已解决，
/// 且 take_remote 的正文 apply 不被 Commit 覆盖。
/// 无锁时间歇性失败：resolve 被 Commit 的旧 merged state 覆盖，A 复活。
#[test]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn cross_api_lock_commit_vs_resolve_take_remote() {
    for round in 0..10 {
        let (api_ui, p1_root, _tmp) = run_concurrency_round(false);

        // 断言 1：A 已解决（没被 Commit 复活）。
        let conflicts = api_ui.list_sync_conflicts("p1").unwrap();
        assert!(
            conflicts.iter().all(|c| c.local_path != CONFLICT_A),
            "round {round}: take_remote — 已解决的冲突 A 不应被 Commit 复活\
             （list_sync_conflicts 仍含 A：{:?}）",
            conflicts
                .iter()
                .filter(|c| c.local_path == CONFLICT_A)
                .collect::<Vec<_>>()
        );

        // 断言 2：take_remote 后正文应为远端 snapshot 内容。
        let existing_content = std::fs::read_to_string(p1_root.join(CONFLICT_A)).unwrap();
        assert_eq!(
            existing_content, "remote existing content",
            "round {round}: take_remote 后 existing.md 应为远端 snapshot 内容"
        );
    }
}
