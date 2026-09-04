//! Issue #644 评论 5474166587 — 对抗式语义验证测试。
//!
//! 这些测试由 ResultVerify Agent 独立构造，不依赖复现测试，目的是
//! "尝试打破补丁"——从补丁描述的语义边界构造场景，验证修复真正生效。
//!
//! 三个问题的对抗式验证：
//! 1. 问题1：app-meta/sync/manifest.sync.json 进 engine_state_actions；
//!    config.local.json 不出现在任何 actions 里。
//! 2. 问题2：PartialConflict 时非冲突文件 + 冲突元数据都落 live；
//!    FatalError 时整体跳过。
//! 3. 问题3：真正 LWW——local mtime 新 → keep_local；
//!    incoming mtime 新 → apply incoming；mtime 相同 + device_id 决胜。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use tempfile::TempDir;
use writer_core::facade::WriterCore;
use writer_core::sync::full_sync::FullSyncTransferResult;
use writer_core::sync::staging::{CommitAction, StagingRun};
use writer_core::sync::types::{SyncResult, SyncStatus, TargetSyncResult};

// ── helpers ──

fn write_rel(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

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

fn plan_all_applies(plan: &writer_core::sync::staging::CommitPlan) -> Vec<String> {
    plan.content_actions
        .iter()
        .chain(plan.engine_state_actions.iter())
        .filter_map(|a| match a {
            CommitAction::Apply { rel_path, .. } => Some(rel_path.to_string_lossy().to_string()),
            CommitAction::Delete { .. } => None,
        })
        .collect()
}

fn plan_all_deletes(plan: &writer_core::sync::staging::CommitPlan) -> Vec<String> {
    plan.content_actions
        .iter()
        .chain(plan.engine_state_actions.iter())
        .filter_map(|a| match a {
            CommitAction::Delete { rel_path } => Some(rel_path.to_string_lossy().to_string()),
            CommitAction::Apply { .. } => None,
        })
        .collect()
}

/// 设置文件 mtime 为指定的 Unix 时间戳（秒）。
fn set_mtime(path: &Path, unix_secs: i64) {
    let time = UNIX_EPOCH + Duration::from_secs(unix_secs.cast_unsigned());
    let file = fs::File::open(path).unwrap();
    file.set_modified(time).unwrap();
}

// ════════════════════════════════════════════════════════════════════════════
// 问题1 对抗式验证：app-meta EngineState 写回 live + config.local.json 不被覆盖
// ════════════════════════════════════════════════════════════════════════════

/// 对抗式：staging 含 manifest.sync.json 新内容，调 compute_commit_plan，
/// 断言该文件出现在 engine_state_actions 里（写回 live）。
#[test]
fn adv_p1_manifest_sync_json_goes_to_engine_state_actions() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let manifest = "app-meta/sync/manifest.sync.json";

    write_rel(&live, manifest, "base-manifest");
    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(manifest)])
        .unwrap();
    // Transfer 在 staging 写入新 manifest（incoming 改了）
    write_rel(&run.staging_root(), manifest, "incoming-manifest-v2");

    let plan = run.compute_commit_plan(&live).unwrap();

    // 对抗式断言：必须在 engine_state_actions 里（不能在 content_actions，也不能被跳过）
    let in_engine_state = plan
        .engine_state_actions
        .iter()
        .any(|a| matches!(a, CommitAction::Apply { rel_path, .. } if rel_path.to_string_lossy() == manifest));
    assert!(
        in_engine_state,
        "对抗式失败：manifest.sync.json 必须在 engine_state_actions 里，实际 engine_state={:?}, content={:?}",
        plan.engine_state_actions, plan.content_actions
    );
    // 且不能在 content_actions 里（必须分类到 engine_state，不能混到 content）
    let in_content = plan
        .content_actions
        .iter()
        .any(|a| matches!(a, CommitAction::Apply { rel_path, .. } if rel_path.to_string_lossy() == manifest));
    assert!(
        !in_content,
        "对抗式失败：manifest.sync.json 不应在 content_actions 里（应分类到 engine_state）"
    );
}

/// 对抗式：state.local.json 同样必须进 engine_state_actions。
#[test]
fn adv_p1_state_local_json_goes_to_engine_state_actions() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let state = "app-meta/sync/state.local.json";

    write_rel(&live, state, "base-state");
    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(state)])
        .unwrap();
    write_rel(&run.staging_root(), state, "incoming-state-v2");

    let plan = run.compute_commit_plan(&live).unwrap();

    let in_engine_state = plan
        .engine_state_actions
        .iter()
        .any(|a| matches!(a, CommitAction::Apply { rel_path, .. } if rel_path.to_string_lossy() == state));
    assert!(
        in_engine_state,
        "对抗式失败：state.local.json 必须在 engine_state_actions 里"
    );
}

/// 对抗式：config.local.json 不出现在任何 actions 里（不从 staging 覆盖 live）。
#[test]
fn adv_p1_config_local_json_never_in_any_actions() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let config_local = "app-meta/sync/config.local.json";

    // live 有设备专属 config.local.json
    write_rel(&live, config_local, "live-device-config");
    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(config_local)])
        .unwrap();
    // staging 里也有一个不同的 config.local.json（远端来的，但这是设备专属的，不应覆盖 live）
    write_rel(&run.staging_root(), config_local, "remote-different-config");

    let plan = run.compute_commit_plan(&live).unwrap();

    let all_applies = plan_all_applies(&plan);
    let all_deletes = plan_all_deletes(&plan);
    assert!(
        !all_applies.iter().any(|p| p == config_local),
        "对抗式失败：config.local.json 不应出现在 apply actions 里（设备专属，不从 staging 覆盖 live），实际 applies={:?}",
        all_applies
    );
    assert!(
        !all_deletes.iter().any(|p| p == config_local),
        "对抗式失败：config.local.json 不应出现在 delete actions 里"
    );
    assert!(
        !plan
            .keep_local
            .iter()
            .any(|p| p.to_string_lossy() == config_local),
        "对抗式失败：config.local.json 不应出现在 keep_local 里（应整体 Skip）"
    );
    assert!(
        !plan
            .noop
            .iter()
            .any(|p| p.to_string_lossy() == config_local),
        "对抗式失败：config.local.json 不应出现在 noop 里（应整体 Skip）"
    );
}

/// 对抗式：secrets 文件不进 commit。
#[test]
fn adv_p1_secrets_never_in_any_actions() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let secrets = "app-meta/sync/secrets/github.json";

    write_rel(&live, secrets, "live-secrets");
    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(secrets)])
        .unwrap();
    write_rel(&run.staging_root(), secrets, "remote-secrets-different");

    let plan = run.compute_commit_plan(&live).unwrap();

    let all_applies = plan_all_applies(&plan);
    assert!(
        !all_applies.iter().any(|p| p == secrets),
        "对抗式失败：secrets 文件不应出现在 apply actions 里（凭证不进 commit）"
    );
}

/// 对抗式：app-meta/transactions/ 永不进 commit。
#[test]
fn adv_p1_transactions_never_in_any_actions() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let tx_file = "app-meta/transactions/tx1/staged";

    write_rel(&live, tx_file, "tmp-staged");
    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(tx_file)])
        .unwrap();
    write_rel(&run.staging_root(), tx_file, "tmp-staged-v2");

    let plan = run.compute_commit_plan(&live).unwrap();

    let all_applies = plan_all_applies(&plan);
    assert!(
        !all_applies.iter().any(|p| p == tx_file),
        "对抗式失败：app-meta/transactions/ 不应出现在 apply actions 里"
    );
}

/// 对抗式：.git/ 元数据永不进 commit。
#[test]
fn adv_p1_git_metadata_never_in_any_actions() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let git_head = ".git/HEAD";

    write_rel(&live, git_head, "ref: refs/heads/main");
    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(git_head)])
        .unwrap();
    write_rel(&run.staging_root(), git_head, "ref: refs/heads/feature");

    let plan = run.compute_commit_plan(&live).unwrap();

    let all_applies = plan_all_applies(&plan);
    assert!(
        !all_applies.iter().any(|p| p == git_head),
        "对抗式失败：.git/ 元数据不应出现在 apply actions 里"
    );
}

/// 对抗式：full-sync-staging/ 永不进 commit（避免递归）。
#[test]
fn adv_p1_full_sync_staging_never_in_any_actions() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let staging_file = "full-sync-staging/run1/staged.txt";

    write_rel(&live, staging_file, "staging-tmp");
    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(staging_file)])
        .unwrap();
    write_rel(&run.staging_root(), staging_file, "staging-tmp-v2");

    let plan = run.compute_commit_plan(&live).unwrap();

    let all_applies = plan_all_applies(&plan);
    assert!(
        !all_applies.iter().any(|p| p == staging_file),
        "对抗式失败：full-sync-staging/ 不应出现在 apply actions 里"
    );
}

/// 对抗式：EngineState 写回的内容是 staging 里的 incoming（不是 base/local）。
#[test]
fn adv_p1_engine_state_writes_incoming_content_not_base() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let manifest = "app-meta/sync/manifest.sync.json";

    write_rel(&live, manifest, "base-manifest");
    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(manifest)])
        .unwrap();
    write_rel(&run.staging_root(), manifest, "incoming-authoritative");

    let plan = run.compute_commit_plan(&live).unwrap();

    let applied = plan.engine_state_actions.iter().find_map(|a| match a {
        CommitAction::Apply { rel_path, content } if rel_path.to_string_lossy() == manifest => {
            Some(content.clone())
        }
        _ => None,
    });
    let content = applied.expect("应有 manifest 的 Apply 动作");
    assert_eq!(
        String::from_utf8(content).unwrap(),
        "incoming-authoritative",
        "对抗式失败：engine_state 写回的应是 staging 里的 incoming 内容（最新权威值）"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// 问题2 对抗式验证：PartialConflict 保留非冲突文件 + 冲突元数据
// ════════════════════════════════════════════════════════════════════════════

/// 对抗式：PartialConflict 时已安全完成的非冲突文件 project.json 落到 live，
/// 冲突元数据 conflicts.json 也落到 live。
#[test]
fn adv_p2_partial_conflict_commits_non_conflict_files_and_conflict_metadata() {
    let tmp = TempDir::new().unwrap();
    let app_data = tmp.path().join("app-data");
    let projects = tmp.path().join("projects");
    let project_live = projects.join("p1");
    fs::create_dir_all(&project_live).unwrap();

    let project_json = "project.json";
    let conflicts_json = "app-meta/sync/conflicts.json";

    // live = base
    write_rel(&project_live, project_json, "base-content");
    let run = StagingRun::create(&app_data, project_live.clone()).unwrap();
    run.build_base_snapshot_from_live(&project_live, &[PathBuf::from(project_json)])
        .unwrap();
    // Transfer 在 staging 写入 project.json 远端更新（非冲突文件，已安全下载）
    write_rel(&run.staging_root(), project_json, "incoming-updated");
    // staging 里也写了冲突元数据
    write_rel(&run.staging_root(), conflicts_json, "new-conflicts-meta");

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

    // 对抗式断言1：非冲突文件 project.json 落到 live
    let live_project = fs::read_to_string(project_live.join(project_json)).unwrap();
    assert_eq!(
        live_project, "incoming-updated",
        "对抗式失败：PartialConflict 时已安全完成的非冲突文件 project.json 应落 live"
    );
    // 对抗式断言2：冲突元数据 conflicts.json 落到 live
    let live_conflicts = fs::read_to_string(project_live.join(conflicts_json)).unwrap();
    assert_eq!(
        live_conflicts, "new-conflicts-meta",
        "对抗式失败：PartialConflict 时冲突元数据 conflicts.json 应落 live"
    );
}

/// 对抗式：FatalError 时整体跳过 staging，live 不被修改。
#[test]
fn adv_p2_fatal_error_skips_staging_commit() {
    let tmp = TempDir::new().unwrap();
    let app_data = tmp.path().join("app-data");
    let projects = tmp.path().join("projects");
    let project_live = projects.join("p1");
    fs::create_dir_all(&project_live).unwrap();

    let project_json = "project.json";
    write_rel(&project_live, project_json, "live-original");
    let run = StagingRun::create(&app_data, project_live.clone()).unwrap();
    run.build_base_snapshot_from_live(&project_live, &[PathBuf::from(project_json)])
        .unwrap();
    write_rel(
        &run.staging_root(),
        project_json,
        "incoming-should-not-apply",
    );

    let transfer_result = FullSyncTransferResult {
        targets: vec![TargetSyncResult {
            target_kind: "project".to_string(),
            project_id: Some("p1".to_string()),
            remote_prefix: "projects/p1".to_string(),
            result: sync_result_with_status(SyncStatus::FatalError("auth failed".to_string())),
        }],
    };

    let core = WriterCore::new(&app_data, &projects);
    let _ = core.commit_full_sync(transfer_result, vec![run]);

    // 对抗式断言：FatalError → Skip，live 保持原样
    let live_content = fs::read_to_string(project_live.join(project_json)).unwrap();
    assert_eq!(
        live_content, "live-original",
        "对抗式失败：FatalError 时 staging 应被整体跳过，live 不应被修改"
    );
}

/// 对抗式：RecoverableError 时整体跳过 staging。
#[test]
fn adv_p2_recoverable_error_skips_staging_commit() {
    let tmp = TempDir::new().unwrap();
    let app_data = tmp.path().join("app-data");
    let projects = tmp.path().join("projects");
    let project_live = projects.join("p1");
    fs::create_dir_all(&project_live).unwrap();

    let project_json = "project.json";
    write_rel(&project_live, project_json, "live-original");
    let run = StagingRun::create(&app_data, project_live.clone()).unwrap();
    run.build_base_snapshot_from_live(&project_live, &[PathBuf::from(project_json)])
        .unwrap();
    write_rel(
        &run.staging_root(),
        project_json,
        "incoming-should-not-apply",
    );

    let transfer_result = FullSyncTransferResult {
        targets: vec![TargetSyncResult {
            target_kind: "project".to_string(),
            project_id: Some("p1".to_string()),
            remote_prefix: "projects/p1".to_string(),
            result: sync_result_with_status(SyncStatus::RecoverableError("network".to_string())),
        }],
    };

    let core = WriterCore::new(&app_data, &projects);
    let _ = core.commit_full_sync(transfer_result, vec![run]);

    let live_content = fs::read_to_string(project_live.join(project_json)).unwrap();
    assert_eq!(
        live_content, "live-original",
        "对抗式失败：RecoverableError 时 staging 应被整体跳过"
    );
}

/// 对抗式：Success 时正常 Full 模式，content + engine_state 全部写回。
#[test]
fn adv_p2_success_full_commit_writes_both_content_and_engine_state() {
    let tmp = TempDir::new().unwrap();
    let app_data = tmp.path().join("app-data");
    let projects = tmp.path().join("projects");
    let project_live = projects.join("p1");
    fs::create_dir_all(&project_live).unwrap();

    let project_json = "project.json";
    let manifest = "app-meta/sync/manifest.sync.json";

    write_rel(&project_live, project_json, "base-content");
    write_rel(&project_live, manifest, "base-manifest");
    let run = StagingRun::create(&app_data, project_live.clone()).unwrap();
    run.build_base_snapshot_from_live(
        &project_live,
        &[PathBuf::from(project_json), PathBuf::from(manifest)],
    )
    .unwrap();
    write_rel(&run.staging_root(), project_json, "incoming-content");
    write_rel(&run.staging_root(), manifest, "incoming-manifest");

    let transfer_result = FullSyncTransferResult {
        targets: vec![TargetSyncResult {
            target_kind: "project".to_string(),
            project_id: Some("p1".to_string()),
            remote_prefix: "projects/p1".to_string(),
            result: sync_result_with_status(SyncStatus::Success),
        }],
    };

    let core = WriterCore::new(&app_data, &projects);
    let _ = core.commit_full_sync(transfer_result, vec![run]);

    // 对抗式：Success → Full，content 和 engine_state 都写回 live
    let live_project = fs::read_to_string(project_live.join(project_json)).unwrap();
    assert_eq!(live_project, "incoming-content");
    let live_manifest = fs::read_to_string(project_live.join(manifest)).unwrap();
    assert_eq!(live_manifest, "incoming-manifest");
}

/// 对抗式：PartialConflict 时 transfer_result.conflicts 里的路径不写回 live
/// （即使 staging 里有该路径的 incoming 内容）。
#[test]
fn adv_p2_partial_conflict_excludes_conflict_paths_from_content_actions() {
    let tmp = TempDir::new().unwrap();
    let app_data = tmp.path().join("app-data");
    let projects = tmp.path().join("projects");
    let project_live = projects.join("p1");
    fs::create_dir_all(&project_live).unwrap();

    let conflict_file = "volumes/v1/chapters/c1/chapter.md";
    let safe_file = "project.json";

    // live = base
    write_rel(&project_live, conflict_file, "base-chapter");
    write_rel(&project_live, safe_file, "base-project");
    let run = StagingRun::create(&app_data, project_live.clone()).unwrap();
    run.build_base_snapshot_from_live(
        &project_live,
        &[PathBuf::from(conflict_file), PathBuf::from(safe_file)],
    )
    .unwrap();
    // staging: conflict_file 有 incoming（但 transfer 标记为冲突，不应写回）
    write_rel(&run.staging_root(), conflict_file, "incoming-chapter");
    // staging: safe_file 有 incoming（非冲突，应写回）
    write_rel(&run.staging_root(), safe_file, "incoming-project");

    // transfer 结果：PartialConflict + conflicts 列表含 conflict_file
    let mut sync_result = sync_result_with_status(SyncStatus::PartialConflict);
    sync_result.conflicts = vec![writer_core::sync::types::SyncConflict {
        local_path: conflict_file.to_string(),
        remote_path: format!("projects/p1/{}", conflict_file),
        local_hash: "local-hash".to_string(),
        remote_hash: "remote-hash".to_string(),
        base_hash: "base-hash".to_string(),
        created_at: 0,
        description: "both changed".to_string(),
    }];
    let transfer_result = FullSyncTransferResult {
        targets: vec![TargetSyncResult {
            target_kind: "project".to_string(),
            project_id: Some("p1".to_string()),
            remote_prefix: "projects/p1".to_string(),
            result: sync_result,
        }],
    };

    let core = WriterCore::new(&app_data, &projects);
    let _ = core.commit_full_sync(transfer_result, vec![run]);

    // 对抗式断言1：safe_file 落 live
    let live_safe = fs::read_to_string(project_live.join(safe_file)).unwrap();
    assert_eq!(
        live_safe, "incoming-project",
        "对抗式失败：PartialConflict 时非冲突文件应落 live"
    );
    // 对抗式断言2：conflict_file 不被 incoming 覆盖（保持 base 或 local）
    // 注意：compute_commit_plan 对正文双方都改会判 Conflict，不会进 content_actions，
    // 所以 live 保持 base-chapter（local 没改）。
    let live_conflict = fs::read_to_string(project_live.join(conflict_file)).unwrap();
    assert_eq!(
        live_conflict, "base-chapter",
        "对抗式失败：PartialConflict 时冲突路径不应被 incoming 覆盖，实际 live={:?}",
        live_conflict
    );
}

// ════════════════════════════════════════════════════════════════════════════
// 问题3 对抗式验证：真正 LWW（时间戳 + device_id 决胜）
// ════════════════════════════════════════════════════════════════════════════

/// 对抗式：Metadata 双方都改，local mtime 比 incoming 新 → keep_local（本地获胜）。
#[test]
fn adv_p3_metadata_local_newer_wins() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let project_json = "project.json";

    write_rel(&live, project_json, "base");
    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(project_json)])
        .unwrap();

    // incoming 先写（旧）
    write_rel(&run.staging_root(), project_json, "incoming-older");
    // local 后写（新）—— sleep 保证 mtime 严格大于
    thread::sleep(Duration::from_millis(20));
    writer_core::storage::atomic_write_string(&live.join(project_json), "local-newer").unwrap();

    let plan = run.compute_commit_plan(&live).unwrap();

    // 对抗式断言：local mtime 新 → keep_local，不 apply incoming
    assert!(
        !plan_all_applies(&plan).iter().any(|p| p == project_json),
        "对抗式失败：local mtime 更新时不应 apply incoming（真正 LWW 应 keep_local）"
    );
    assert!(
        plan.keep_local
            .iter()
            .any(|p| p.to_string_lossy() == project_json),
        "对抗式失败：local mtime 更新时应 keep_local"
    );
}

/// 对抗式：Metadata 双方都改，incoming mtime 比 local 新 → apply incoming（远端获胜）。
#[test]
fn adv_p3_metadata_incoming_newer_wins() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let project_json = "project.json";

    // local 先写（旧）
    write_rel(&live, project_json, "base");
    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(project_json)])
        .unwrap();

    // local 改成 local-older（先改，旧 mtime）
    writer_core::storage::atomic_write_string(&live.join(project_json), "local-older").unwrap();
    // incoming 后写（新 mtime）—— sleep 保证 incoming mtime 严格大于 local
    thread::sleep(Duration::from_millis(20));
    write_rel(&run.staging_root(), project_json, "incoming-newer");

    let plan = run.compute_commit_plan(&live).unwrap();

    // 对抗式断言：incoming mtime 新 → apply incoming
    let applies = plan_all_applies(&plan);
    assert!(
        applies.iter().any(|p| p == project_json),
        "对抗式失败：incoming mtime 更新时应 apply incoming，实际 applies={:?}, keep_local={:?}",
        applies,
        plan.keep_local
    );
    // 且 apply 的内容是 incoming
    let applied_content = plan
        .content_actions
        .iter()
        .find_map(|a| match a {
            CommitAction::Apply { rel_path, content }
                if rel_path.to_string_lossy() == project_json =>
            {
                Some(content.clone())
            }
            _ => None,
        })
        .expect("应有 Apply 动作");
    assert_eq!(
        String::from_utf8(applied_content).unwrap(),
        "incoming-newer",
        "对抗式失败：apply 的内容应是 incoming-newer"
    );
}

/// 对抗式：Metadata 双方都改，local mtime 与 incoming mtime 完全相同（用 set_mtime 构造 tie），
/// live device_id 字典序 > "remote"（staging 里 remote 侧固定 device_id）→ keep_local。
///
/// 这验证了 LWW 在时间戳相同时走 device_id 字典序决胜，且 live device_id 读取路径生效。
#[test]
fn adv_p3_metadata_tie_local_device_id_greater_wins() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let project_json = "project.json";
    let state_local = "app-meta/sync/state.local.json";

    // live 写入 project.json + state.local.json
    // device_id = "zzz-top-device"（字典序 > "remote"）
    write_rel(&live, project_json, "base");
    write_rel(
        &live,
        state_local,
        r#"{"last_sync_time":null,"last_error":null,"known_files":{},"conflicts":[],"device_id":"zzz-top-device","conflicted_files":[]}"#,
    );

    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(
        &live,
        &[PathBuf::from(project_json), PathBuf::from(state_local)],
    )
    .unwrap();

    // 双方都改 project.json
    write_rel(&run.staging_root(), project_json, "incoming-changed");
    writer_core::storage::atomic_write_string(&live.join(project_json), "local-changed").unwrap();

    // 用 set_mtime 把 local 和 incoming 的 mtime 设成完全相同 → 构造 tie
    let tie_time = 1_700_000_000; // 固定 Unix 时间戳
    set_mtime(&live.join(project_json), tie_time);
    set_mtime(&run.staging_root().join(project_json), tie_time);

    let plan = run.compute_commit_plan(&live).unwrap();

    // 对抗式断言：时间戳相同 + live device_id "zzz-top-device" > "remote" → keep_local
    assert!(
        !plan_all_applies(&plan).iter().any(|p| p == project_json),
        "对抗式失败：时间戳相同 + live device_id 字典序 > \"remote\" 时应 keep_local（不应 apply incoming）"
    );
    assert!(
        plan.keep_local
            .iter()
            .any(|p| p.to_string_lossy() == project_json),
        "对抗式失败：时间戳相同 + live device_id 字典序更大时应 keep_local"
    );
}

/// 对抗式：Metadata 双方都改，local mtime 与 incoming mtime 完全相同（tie），
/// live device_id 字典序 < "remote"（staging 里 remote 侧固定 device_id）→ apply incoming。
#[test]
fn adv_p3_metadata_tie_remote_device_id_greater_wins() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let project_json = "project.json";
    let state_local = "app-meta/sync/state.local.json";

    write_rel(&live, project_json, "base");
    // device_id = "aaa-low-device"（字典序 < "remote"）
    write_rel(
        &live,
        state_local,
        r#"{"last_sync_time":null,"last_error":null,"known_files":{},"conflicts":[],"device_id":"aaa-low-device","conflicted_files":[]}"#,
    );

    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(
        &live,
        &[PathBuf::from(project_json), PathBuf::from(state_local)],
    )
    .unwrap();

    write_rel(&run.staging_root(), project_json, "incoming-changed");
    writer_core::storage::atomic_write_string(&live.join(project_json), "local-changed").unwrap();

    // 构造 tie
    let tie_time = 1_700_000_000;
    set_mtime(&live.join(project_json), tie_time);
    set_mtime(&run.staging_root().join(project_json), tie_time);

    let plan = run.compute_commit_plan(&live).unwrap();

    // 对抗式断言：时间戳相同 + live device_id "aaa-low-device" < "remote" → apply incoming
    let applies = plan_all_applies(&plan);
    assert!(
        applies.iter().any(|p| p == project_json),
        "对抗式失败：时间戳相同 + live device_id 字典序 < \"remote\" 时应 apply incoming，实际 applies={:?}, keep_local={:?}",
        applies, plan.keep_local
    );
    // 且 apply 的内容是 incoming
    let applied_content = plan
        .content_actions
        .iter()
        .find_map(|a| match a {
            CommitAction::Apply { rel_path, content }
                if rel_path.to_string_lossy() == project_json =>
            {
                Some(content.clone())
            }
            _ => None,
        })
        .expect("应有 Apply 动作");
    assert_eq!(
        String::from_utf8(applied_content).unwrap(),
        "incoming-changed",
        "对抗式失败：apply 的内容应是 incoming-changed"
    );
}

/// 对抗式：Metadata 双方都改，local mtime 与 incoming mtime 完全相同（tie），
/// live 没有 state.local.json（device_id 读取失败回退空字符串）。
/// 空字符串 < "remote" → apply incoming。
/// 这验证 device_id 读取失败的回退路径不 panic 且退化为纯时间戳比较。
#[test]
fn adv_p3_metadata_tie_no_state_local_falls_back_to_empty_device_id() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let project_json = "project.json";

    // live 没有 state.local.json
    write_rel(&live, project_json, "base");
    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(project_json)])
        .unwrap();

    write_rel(&run.staging_root(), project_json, "incoming-changed");
    writer_core::storage::atomic_write_string(&live.join(project_json), "local-changed").unwrap();

    // 构造 tie
    let tie_time = 1_700_000_000;
    set_mtime(&live.join(project_json), tie_time);
    set_mtime(&run.staging_root().join(project_json), tie_time);

    let plan = run.compute_commit_plan(&live).unwrap();

    // device_id 回退空字符串 "" < "remote" → apply incoming
    let applies = plan_all_applies(&plan);
    assert!(
        applies.iter().any(|p| p == project_json),
        "对抗式失败：无 state.local.json 时 device_id 回退空字符串，应 apply incoming（不 panic）"
    );
}

/// 对抗式：GeneratedCache（如 .txt 文件）双方都改也走 LWW，不冲突。
#[test]
fn adv_p3_generated_cache_both_changed_uses_lww_not_conflict() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    let cache_file = "some-cache.txt"; // .txt → GeneratedCache

    write_rel(&live, cache_file, "base");
    let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
    run.build_base_snapshot_from_live(&live, &[PathBuf::from(cache_file)])
        .unwrap();

    // 双方都改
    write_rel(&run.staging_root(), cache_file, "incoming-cache");
    writer_core::storage::atomic_write_string(&live.join(cache_file), "local-cache").unwrap();
    // local mtime 更新
    thread::sleep(Duration::from_millis(20));
    writer_core::storage::atomic_write_string(&live.join(cache_file), "local-cache-newer").unwrap();

    let plan = run.compute_commit_plan(&live).unwrap();

    // GeneratedCache 双方都改 → LWW（local 更新 → keep_local），不进 conflict
    assert!(
        plan.conflict.is_empty(),
        "对抗式失败：GeneratedCache 双方都改应走 LWW 不冲突"
    );
    assert!(
        plan.keep_local
            .iter()
            .any(|p| p.to_string_lossy() == cache_file),
        "对抗式失败：GeneratedCache local 更新时应 keep_local"
    );
}
