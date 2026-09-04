//! Issue #644 评论 5474166587 — full-sync Commit 语义问题复现测试。
//!
//! 修复后所有 6 个测试都应通过。原 3 个"期望行为"测试断言修复后的正确行为；
//! 原 3 个"当前 bug 行为锁定"测试已更新为断言修复后的正确行为（不再锁定 bug）。
//!
//! 三个问题的修复（见 issue 描述）：
//! 1. `app-meta/sync/manifest.sync.json` 与 `app-meta/sync/state.local.json` 作为
//!    EngineState 写回 live（`StagingCommitClass::EngineState`），不再被
//!    `ContentClass::LocalOnly` 跳过。`CommitPlan` 拆 `content_actions` +
//!    `engine_state_actions`。
//! 2. `PartialConflict`/`Conflict` 终态走 `TargetCommitMode::ConflictMetadataOnly`，
//!    冲突元数据 + 已安全完成的非冲突文件写回 live，不再整体丢弃 staging。
//! 3. `Metadata`/`GeneratedCache` 双方都改时走真正 LWW（时间戳 + device_id 决胜），
//!    不再固定 remote-wins。
//!
//! 这些测试只调用 `writer_core` 的公开 API（`StagingRun::compute_commit_plan`、
//! `WriterCore::commit_full_sync`），不修改任何业务代码。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use writer_core::facade::WriterCore;
use writer_core::sync::full_sync::FullSyncTransferResult;
use writer_core::sync::staging::{CommitAction, StagingRun};
use writer_core::sync::types::{SyncResult, SyncStatus, TargetSyncResult};

// ── helpers ──

/// 在 `root` 下写入 `rel` 文件，自动创建父目录。
fn write_rel(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

/// 判断 commit plan 的 `content_actions` 或 `engine_state_actions` 里是否包含
/// 给定 `rel_path` 的 `Apply` 动作。
fn plan_applies_contains(plan: &writer_core::sync::staging::CommitPlan, rel: &str) -> bool {
    plan.content_actions
        .iter()
        .chain(plan.engine_state_actions.iter())
        .any(|action| match action {
            CommitAction::Apply { rel_path, .. } => rel_path.to_string_lossy() == rel,
            CommitAction::Delete { .. } => false,
        })
}

/// 判断 commit plan 的 `engine_state_actions` 里是否包含给定 `rel_path` 的 `Apply` 动作。
fn plan_engine_state_applies_contains(
    plan: &writer_core::sync::staging::CommitPlan,
    rel: &str,
) -> bool {
    plan.engine_state_actions.iter().any(|action| match action {
        CommitAction::Apply { rel_path, .. } => rel_path.to_string_lossy() == rel,
        CommitAction::Delete { .. } => false,
    })
}

/// 取 commit plan 中 `Apply` 动作的内容（若存在，搜 content + engine_state）。
fn plan_applied_content(
    plan: &writer_core::sync::staging::CommitPlan,
    rel: &str,
) -> Option<Vec<u8>> {
    plan.content_actions
        .iter()
        .chain(plan.engine_state_actions.iter())
        .find_map(|action| match action {
            CommitAction::Apply { rel_path, content } if rel_path.to_string_lossy() == rel => {
                Some(content.clone())
            }
            _ => None,
        })
}

/// 构造一个只填 `status` 的最小 `SyncResult`。
fn sync_result_with_status(status: SyncStatus) -> SyncResult {
    SyncResult {
        status,
        uploaded_files: Vec::new(),
        downloaded_files: Vec::new(),
        ignored_files: Vec::new(),
        conflicts: Vec::new(),
        error: None,
        error_category: None,
        message_key: None,
        conflict_summary: None,
        local_deletes: Vec::new(),
        remote_deletes: Vec::new(),
        overwritten_files: Vec::new(),
        search_index_rebuild_error: None,
    }
}

// ════════════════════════════════════════════════════════════════════════════
// 问题1：app-meta/ EngineState 在外层 Commit 里被整体当 LocalOnly 跳过
// ════════════════════════════════════════════════════════════════════════════

/// 期望行为（修复后 **通过**）：
/// `app-meta/sync/manifest.sync.json` 与 `app-meta/sync/state.local.json` 是
/// EngineState，Transfer 在 staging 里真实更新了它们，Commit 应把它们写回 live。
///
/// 修复后：`classify_staging_commit_path` 把这两个文件归为 `EngineState`，
/// 出现在 `plan.engine_state_actions` 里。
#[test]
fn repro_issue644_p1_app_meta_engine_state_expected_to_be_committed() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let manifest = "app-meta/sync/manifest.sync.json";
    let state = "app-meta/sync/state.local.json";

    // live = base（同步前）
    write_rel(&live, manifest, "base-manifest");
    write_rel(&live, state, "base-state");

    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(manifest), PathBuf::from(state)])
        .unwrap();

    // Transfer 在 staging 里写入新内容（incoming 改了，local==base 没动）
    write_rel(&run.staging_root(), manifest, "incoming-manifest");
    write_rel(&run.staging_root(), state, "incoming-state");

    let plan = run.compute_commit_plan(&live).unwrap();

    // 期望：两个 EngineState 文件应出现在 plan.engine_state_actions 里（写回 live）。
    assert!(
        plan_engine_state_applies_contains(&plan, manifest),
        "期望 manifest.sync.json 作为 EngineState 被提交写回 live"
    );
    assert!(
        plan_engine_state_applies_contains(&plan, state),
        "期望 state.local.json 作为 EngineState 被提交写回 live"
    );
}

/// 修复后行为（修复后 **通过**）：
/// 两个 EngineState 文件现在出现在 `plan.engine_state_actions` 里（不再被跳过）。
#[test]
fn repro_issue644_p1_app_meta_engine_state_now_committed() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let manifest = "app-meta/sync/manifest.sync.json";
    let state = "app-meta/sync/state.local.json";

    write_rel(&live, manifest, "base-manifest");
    write_rel(&live, state, "base-state");

    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(manifest), PathBuf::from(state)])
        .unwrap();

    write_rel(&run.staging_root(), manifest, "incoming-manifest");
    write_rel(&run.staging_root(), state, "incoming-state");

    let plan = run.compute_commit_plan(&live).unwrap();

    // 修复后：两个文件作为 EngineState 写回 live，出现在 engine_state_actions 里。
    assert!(
        plan_engine_state_applies_contains(&plan, manifest),
        "修复后：manifest.sync.json 作为 EngineState 写回 live"
    );
    assert!(
        plan_engine_state_applies_contains(&plan, state),
        "修复后：state.local.json 作为 EngineState 写回 live"
    );
    // 且内容是 incoming（staging 里的最新权威值）。
    let manifest_content = plan_applied_content(&plan, manifest).unwrap();
    assert_eq!(
        String::from_utf8(manifest_content).unwrap(),
        "incoming-manifest",
        "修复后：engine_state 写回的是 staging 里的 incoming 内容"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// 问题2：PartialConflict 终态把整个 staging 丢弃，已安全的非冲突文件不落 live
// ════════════════════════════════════════════════════════════════════════════

/// 期望行为（修复后 **通过**）：
/// `PartialConflict` 表示部分文件冲突，但已安全完成的非冲突文件（如 project.json
/// 的远端更新）应继续提交到 live，冲突元数据也应落到 live。
///
/// 修复后：`target_commit_mode` 对 `PartialConflict` 返回 `ConflictMetadataOnly`，
/// 已安全完成的非冲突文件写回 live。
#[test]
fn repro_issue644_p2_partial_conflict_expected_to_commit_non_conflict_files() {
    let tmp = TempDir::new().unwrap();
    let app_data = tmp.path().join("app-data");
    let projects = tmp.path().join("projects");
    let project_live = projects.join("p1");
    fs::create_dir_all(&project_live).unwrap();

    // live: project.json = base（同步前）
    let project_json = "project.json";
    write_rel(&project_live, project_json, "base-content");

    // 创建 staging run，base 快照从 live 建
    let run = StagingRun::create(&app_data, project_live.clone()).unwrap();
    run.build_base_snapshot_from_live(&project_live, &[PathBuf::from(project_json)])
        .unwrap();

    // Transfer 在 staging 里写入 project.json 的远端更新（非冲突文件，已安全下载）
    write_rel(&run.staging_root(), project_json, "incoming-updated");
    // staging 里也写了冲突元数据（正文冲突产生的）
    write_rel(
        &run.staging_root(),
        "app-meta/sync/conflicts.json",
        "new-conflicts",
    );

    // 构造 transfer 结果：该 target 状态为 PartialConflict（某个正文冲突，
    // 但 project.json 是已安全完成的非冲突文件）
    let transfer_result = FullSyncTransferResult {
        targets: vec![TargetSyncResult {
            target_kind: "project".to_string(),
            project_id: Some("p1".to_string()),
            remote_prefix: "projects/p1".to_string(),
            result: sync_result_with_status(SyncStatus::PartialConflict),
        }],
    };

    let core = WriterCore::new(&app_data, &projects);
    let _ = core.commit_full_sync(transfer_result, vec![run]);

    // 期望：project.json 的远端更新应落到 live（PartialConflict 中已安全完成的非冲突文件）
    let live_content = fs::read_to_string(project_live.join(project_json)).unwrap();
    assert_eq!(
        live_content, "incoming-updated",
        "期望 PartialConflict 中已安全完成的非冲突文件 project.json 被提交到 live"
    );
}

/// 修复后行为（修复后 **通过**）：
/// PartialConflict 时 live 的 project.json 被更新为 incoming（staging 不再被整体丢弃）。
#[test]
fn repro_issue644_p2_partial_conflict_now_commits_non_conflict_files() {
    let tmp = TempDir::new().unwrap();
    let app_data = tmp.path().join("app-data");
    let projects = tmp.path().join("projects");
    let project_live = projects.join("p1");
    fs::create_dir_all(&project_live).unwrap();

    let project_json = "project.json";
    write_rel(&project_live, project_json, "base-content");

    let run = StagingRun::create(&app_data, project_live.clone()).unwrap();
    run.build_base_snapshot_from_live(&project_live, &[PathBuf::from(project_json)])
        .unwrap();
    write_rel(&run.staging_root(), project_json, "incoming-updated");
    write_rel(
        &run.staging_root(),
        "app-meta/sync/conflicts.json",
        "new-conflicts",
    );

    let transfer_result = FullSyncTransferResult {
        targets: vec![TargetSyncResult {
            target_kind: "project".to_string(),
            project_id: Some("p1".to_string()),
            remote_prefix: "projects/p1".to_string(),
            result: sync_result_with_status(SyncStatus::PartialConflict),
        }],
    };

    let core = WriterCore::new(&app_data, &projects);
    let _ = core.commit_full_sync(transfer_result, vec![run]);

    // 修复后：PartialConflict → ConflictMetadataOnly → project.json 被写回 live。
    let live_content = fs::read_to_string(project_live.join(project_json)).unwrap();
    assert_eq!(
        live_content, "incoming-updated",
        "修复后：PartialConflict 时已安全完成的非冲突文件被提交到 live"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// 问题3：Metadata 双方都改时固定 remote-wins，不是真正 LWW
// ════════════════════════════════════════════════════════════════════════════

/// 修复后行为（修复后 **通过**）：
/// `Metadata`（如 project.json）双方都改时，`compute_commit_plan` 走真正 LWW
/// （时间戳较大方获胜；同时间 device_id 字典序决胜）。
/// 当本地版本更新（mtime 更新）时，应 `keep_local`，不应盲目 `apply_incoming`。
#[test]
fn repro_issue644_p3_metadata_both_changed_now_true_lww() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let project_json = "project.json";

    // base = "base"
    write_rel(&live, project_json, "base");
    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(project_json)])
        .unwrap();

    // incoming 先写（远端版本，假设比 local 旧）
    write_rel(&run.staging_root(), project_json, "incoming-changed");
    // local 后写（用户在同步过程中刚改过 project.json，mtime 更新）
    std::thread::sleep(std::time::Duration::from_millis(10));
    writer_core::storage::atomic_write_string(&live.join(project_json), "local-newer").unwrap();

    let plan = run.compute_commit_plan(&live).unwrap();

    // 修复后：真正 LWW。local mtime 更新 → keep_local，不盲目 apply incoming。
    assert!(
        !plan_applies_contains(&plan, project_json),
        "修复后：真正 LWW——local 更新时不应盲目 apply incoming"
    );
    assert!(
        plan.keep_local
            .iter()
            .any(|p| p.to_string_lossy() == project_json),
        "修复后：local 更新时走 keep_local"
    );
}

/// 期望行为（修复后 **通过**）：
/// `Metadata` 双方都改时应走真正 LWW（时间戳较大方获胜；同时间 device_id 决胜），
/// 而非固定 remote-wins。当本地版本更新时，应 `keep_local`，不应盲目 `apply_incoming`。
#[test]
fn repro_issue644_p3_metadata_both_changed_expected_true_lww_not_fixed_remote_wins() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let project_json = "project.json";

    write_rel(&live, project_json, "base");
    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(project_json)])
        .unwrap();

    // incoming 先写（远端版本，假设比 local 旧）
    write_rel(&run.staging_root(), project_json, "incoming-changed");
    // local 后写（用户刚改过，应被视为较新版本）
    std::thread::sleep(std::time::Duration::from_millis(10));
    writer_core::storage::atomic_write_string(&live.join(project_json), "local-newer").unwrap();

    let plan = run.compute_commit_plan(&live).unwrap();

    // 期望：真正 LWW 在 local 更新时不应盲目 apply incoming（应 keep_local）。
    assert!(
        !plan_applies_contains(&plan, project_json),
        "期望真正 LWW：local 更新时不应盲目 apply incoming（固定 remote-wins 会\
         静默覆盖用户刚改过的 project.json/volume.json/settings.sync.json）"
    );
}
