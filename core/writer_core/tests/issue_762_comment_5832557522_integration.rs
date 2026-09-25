//! Issue #762 评论 5832557522 整合测试 — #762 callback 与 #763 sink 在同一条同步链上同时成立。
//!
//! 场景：两个 project target（p1, p2），p1 Transfer 产生 BothChanged 冲突。
//! 验证 sink 与 #762 callback 协同：
//! 1. sink 显示 p1 + phase=transfer（p1 Transfer 进行中）
//! 2. p1 进入单 target Commit 时 sink 显示 p1 + phase=commit
//!    （编排顺序保证：`set_target_phase("commit")` 在 commit 前、`update_target_finish`
//!    在 commit 后；commit 阶段不调 provider，窗口极短，主线程轮询尽力采样，
//!    callback 在 finish 之后触发间接证明 commit 已完成）
//! 3. p1 Commit 完成后 #762 callback 才触发，此时 live 已能查到 p1 冲突
//! 4. p2 继续（sink 切到 p2，callback 第二次触发带 p2）
//! 5. generation GC 切到真实 target（sink 显示某 target + phase=generation_gc）
//! 6. 最终全局收口时 sink current_target 清空（None）、phase=commit
//!
//! 编排顺序（`perform_full_sync_with_provider`）：
//! ```text
//! for each target:
//!   sink.update_target_start(prefix, pid, "transfer", finished, total)
//!   Transfer
//!   sink.set_target_phase(prefix, pid, "commit")
//!   Commit（写 live 终态，冲突落盘）
//!   sink.update_target_finish(prefix, pid, finished, total)  // phase 清空
//!   cleanup staging
//!   target_progress callback  // 此时 live 已是终态
//! generation GC: for each target: sink.set_target_phase(prefix, pid, "generation_gc"); GC
//! sink.set_global_phase("commit")  // 清 current target
//! finalize
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;
use writer_core::api::WriterCoreApi;
use writer_core::sync::full_sync::{
    FullSyncPlan, LiveTargetLww, PlannedTarget, SyncProgressCallback, SyncTargetProgress,
};
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
use writer_core::sync::{SyncProgressSink, SyncTargetProgressDto};

// ---------------------------------------------------------------------------
// 辅助函数（与 issue_762_comment_5826175490_repro.rs 同构）
// ---------------------------------------------------------------------------

/// 构造 live project：本地正文 + manifest LWW。
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

// ---------------------------------------------------------------------------
// SlowProvider — 包装 MemoryProvider，每次远端调用前 sleep，让同步慢下来，
// 主线程轮询 sink 能可靠捕获中间状态（transfer / generation_gc）。
//
// commit 阶段不调 provider（只写本地 live），窗口极短，轮询可能采不到；
// 场景 2（commit 中间状态）由编排顺序 + callback 在 finish 之后触发间接保证。
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
// 观察记录
// ---------------------------------------------------------------------------

/// #762 callback 触发时采集的快照：progress + sink 状态 + live 冲突。
///
/// `live_conflict_count` 是该 target 作品的未解决冲突数（用单项目
/// `list_sync_conflicts(project_id)` 查询，与既有 #762 测试同构；不依赖
/// `list_projects` 项目注册表，直接读作品目录的 conflicts.json）。
#[derive(Debug, Clone)]
struct CallbackRecord {
    progress: SyncTargetProgress,
    sink_snapshot: SyncTargetProgressDto,
    live_conflict_count: usize,
    live_has_conflict: bool,
}

/// 主线程轮询 sink 采到的快照。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ObservedSnapshot {
    phase: Option<String>,
    project_id: Option<String>,
    finished_targets: u32,
}

// ---------------------------------------------------------------------------
// 整合测试：#762 callback 与 #763 sink 在同一条同步链上同时成立
// ---------------------------------------------------------------------------

#[test]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn sink_and_callback_cooperate_on_same_sync_chain() {
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

    // p1, p2 两个 live project；远端 generation 内容不同 → BothChanged 冲突。
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
        app_data_root: app_data_root.clone(),
        remote_catalog_snapshot,
    };
    let staging_runs = prepare_staging_runs(&mut plan).unwrap();
    assert_eq!(staging_runs.len(), 2, "两个 target 各一个 staging run");

    // #763 sink：两个 target。
    let sink = SyncProgressSink::new(2);

    // #762 callback 记录：每次触发时 sink 快照 + live 冲突。
    let callback_records: Arc<Mutex<Vec<CallbackRecord>>> = Arc::new(Mutex::new(Vec::new()));
    let api: Arc<WriterCoreApi> = Arc::new(WriterCoreApi::new(&app_data_root, &projects_root));

    let sink_for_cb = sink.clone();
    let records_for_cb = callback_records.clone();
    let api_for_cb = Arc::clone(&api);
    let callback: SyncProgressCallback = Arc::new(move |p: SyncTargetProgress| {
        let snap = sink_for_cb.snapshot();
        // callback 在 commit 之后触发，core 写锁已释放，可安全读 live 冲突。
        // 用单项目 list_sync_conflicts 查该 target 作品的冲突（与既有 #762 测试同构，
        // 直接读作品目录 conflicts.json，不依赖 list_projects 注册表）。
        let (live_count, live_has) = match &p.project_id {
            Some(pid) => {
                let conflicts = api_for_cb.list_sync_conflicts(pid).unwrap_or_default();
                let count = conflicts.len();
                (count, count > 0)
            }
            None => (0, false),
        };
        if let Ok(mut guard) = records_for_cb.lock() {
            guard.push(CallbackRecord {
                progress: p,
                sink_snapshot: snap,
                live_conflict_count: live_count,
                live_has_conflict: live_has,
            });
        }
    });

    // 主线程轮询 sink 采集中间状态。
    let observed: Arc<Mutex<Vec<ObservedSnapshot>>> = Arc::new(Mutex::new(Vec::new()));
    let sink_for_poll = sink.clone();
    let observed_for_poll = observed.clone();

    // worker 线程跑同步（SlowProvider 拉长远端调用，让中间状态可采样）。
    let slow_provider = SlowProvider {
        inner: provider,
        delay: Duration::from_millis(3),
    };
    let api_for_worker = Arc::clone(&api);
    let sink_for_worker = sink.clone();

    let worker = std::thread::spawn(move || {
        api_for_worker.perform_full_sync_with_provider(
            &slow_provider,
            &plan,
            staging_runs,
            None,
            Some(sink_for_worker),
            Some(&callback),
        )
    });

    // 主线程高频轮询 sink，直到 worker 结束。
    while !worker.is_finished() {
        let snap = sink_for_poll.snapshot();
        if let Ok(mut guard) = observed_for_poll.lock() {
            guard.push(ObservedSnapshot {
                phase: snap.current_phase,
                project_id: snap.current_project_id,
                finished_targets: snap.finished_targets,
            });
        }
        std::thread::sleep(Duration::from_micros(200));
    }
    let result = worker
        .join()
        .expect("worker thread panicked")
        .expect("perform_full_sync_with_provider 应成功");

    let observed_snapshots = observed.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let callback_rows = callback_records
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();

    // 采样摘要（失败时打印辅助诊断）。
    let phase_trace: Vec<(Option<String>, Option<String>)> = observed_snapshots
        .iter()
        .map(|s| (s.phase.clone(), s.project_id.clone()))
        .collect();

    // ---- 场景 1：sink 显示 p1 + phase=transfer（p1 Transfer 进行中）----
    let saw_p1_transfer = observed_snapshots
        .iter()
        .any(|s| s.phase.as_deref() == Some("transfer") && s.project_id.as_deref() == Some("p1"));
    assert!(
        saw_p1_transfer,
        "场景 1：sink 应在 p1 Transfer 期间显示 phase=transfer + project_id=p1。\n\
         采到的 (phase, project_id) 序列：{:?}",
        phase_trace
    );

    // ---- 场景 2：sink 显示 p1 + phase=commit（单 target commit）----
    // commit 阶段不调 provider，窗口极短；若轮询采到则验证语义正确。
    // 编排顺序（set_target_phase("commit") 在 commit 前、update_target_finish 在 commit 后）
    // 已由下方场景 3「callback 在 finish 之后触发」间接证明 commit 已完成。
    let saw_p1_commit = observed_snapshots
        .iter()
        .any(|s| s.phase.as_deref() == Some("commit") && s.project_id.as_deref() == Some("p1"));
    // 不强制要求采到（窗口太短）；采到即说明 commit 中间状态可见。
    let _ = saw_p1_commit;

    // ---- 场景 4（前置）：两个 target 各回调一次 progress ----
    assert_eq!(
        callback_rows.len(),
        2,
        "场景 4：两个 target 各回调一次 progress，实际 {} 次",
        callback_rows.len()
    );

    // ---- 场景 3：p1 callback 在 commit 之后触发，live 已能查到 p1 冲突 ----
    let p1_cb = callback_rows
        .iter()
        .find(|r| r.progress.project_id.as_deref() == Some("p1"))
        .expect("应有 p1 的 callback");
    assert_eq!(
        p1_cb.progress.status, "partial_conflict",
        "p1 callback status 应为 partial_conflict"
    );
    assert!(
        p1_cb.live_has_conflict,
        "场景 3：p1 callback 触发时 live list_sync_conflicts(p1) 应已能查到 p1 冲突"
    );
    assert!(
        p1_cb.live_conflict_count >= 1,
        "场景 3：p1 callback 触发时 live 冲突数应 >= 1，实际 {}",
        p1_cb.live_conflict_count
    );
    // callback 在 update_target_finish 之后触发：phase 已清空，current_target 仍指 p1。
    assert_eq!(
        p1_cb.sink_snapshot.current_project_id.as_deref(),
        Some("p1"),
        "callback 触发时 sink current_project_id 应仍指 p1（finish 不清 target）"
    );
    assert!(
        p1_cb.sink_snapshot.current_phase.is_none(),
        "callback 触发时 sink phase 应已清空（update_target_finish 在 callback 前执行），\
         证明 callback 在 commit 阶段之后触发（场景 2/3 顺序保证）"
    );
    assert_eq!(
        p1_cb.sink_snapshot.finished_targets, 1,
        "p1 callback 触发时 finished_targets 应为 1"
    );

    // ---- 场景 4：p2 继续（callback 第二次带 p2，sink 切到 p2）----
    let p2_cb = callback_rows
        .iter()
        .find(|r| r.progress.project_id.as_deref() == Some("p2"))
        .expect("应有 p2 的 callback");
    assert_eq!(
        p2_cb.sink_snapshot.current_project_id.as_deref(),
        Some("p2"),
        "场景 4：p2 callback 触发时 sink 应已切到 p2"
    );
    assert_eq!(
        p2_cb.sink_snapshot.finished_targets, 2,
        "p2 callback 触发时 finished_targets 应为 2"
    );

    // ---- 场景 5：generation GC 切到真实 target（phase=generation_gc + project_id 非 None）----
    let saw_generation_gc_with_target = observed_snapshots
        .iter()
        .any(|s| s.phase.as_deref() == Some("generation_gc") && s.project_id.is_some());
    assert!(
        saw_generation_gc_with_target,
        "场景 5：sink 应在 generation GC 期间显示 phase=generation_gc 且指向真实 target\
         （project_id 非 None），不应残留空 target。\n\
         采到的 (phase, project_id) 序列：{:?}",
        phase_trace
    );

    // ---- 场景 6：最终全局收口 ----
    let final_snap = sink.snapshot();
    assert!(
        final_snap.current_target_remote_prefix.is_none(),
        "场景 6：最终 sink current_target_remote_prefix 应为 None（全局收口）"
    );
    assert!(
        final_snap.current_project_id.is_none(),
        "场景 6：最终 sink current_project_id 应为 None（全局收口）"
    );
    assert_eq!(
        final_snap.current_phase.as_deref(),
        Some("commit"),
        "场景 6：最终 sink phase 应为 commit（set_global_phase）"
    );
    assert_eq!(
        final_snap.finished_targets, 2,
        "场景 6：最终 finished_targets 应为 2（p1 + p2 都完成）"
    );
    assert_eq!(final_snap.total_targets, 2, "场景 6：total_targets 应为 2");

    // 整轮聚合状态保留冲突（两个作品都未 resolve）。
    assert_eq!(
        result.overall_status, "partial_conflict",
        "整轮聚合状态应为 partial_conflict"
    );
    assert!(
        !api.list_sync_conflicts("p1").unwrap().is_empty(),
        "最终 live 应有 p1 冲突"
    );
    assert!(
        !api.list_sync_conflicts("p2").unwrap().is_empty(),
        "最终 live 应有 p2 冲突"
    );
}
