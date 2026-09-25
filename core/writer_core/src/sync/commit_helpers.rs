use std::path::{Path, PathBuf};

///    把 target-relative `rel_path` 转成
/// workspace-relative path，供 `record_workspace_history` 精确 stage。
///
/// - App target：`live_root = app_data_root`，`rel_path` 已是 workspace-relative。
/// - Project target：`live_root = projects_root/<project_id>`，
///   workspace-relative = `projects/<project_id>/<rel_path>`。
/// - DeletedProject target with `RemoteTargetWins`：`target_kind` 保持为
///   `"deleted_project"`，但 `target_live_root` 已设为 `projects_root/<project_id>`，
///   所以 workspace-relative 也是 `projects/<project_id>/<rel_path>`。
///
/// `target_kind` / `project_id` 来自 `TargetSyncResult`，与 staging_runs
/// 按索引对应。未知 target_kind 时退化为直接返回 `rel_path`（保守不丢路径）。
fn to_workspace_rel_path(target_kind: &str, project_id: Option<&str>, rel_path: &Path) -> PathBuf {
    //   deleted_project 的 RemoteTargetWins 恢复路径
    // 与普通 project 共用同一分支，不再走 rel-only 路径。
    if target_kind == "project" || target_kind == "deleted_project" {
        if let Some(pid) = project_id {
            return PathBuf::from("projects").join(pid).join(rel_path);
        }
    }
    PathBuf::from(rel_path)
}

/// 收集一批 `CommitAction` 的 rel_path，转成 workspace-relative 后追加到 `out`。
fn collect_action_paths(
    target_kind: &str,
    project_id: Option<&str>,
    actions: &[crate::sync::staging::CommitAction],
    out: &mut Vec<PathBuf>,
) {
    for action in actions {
        let rel = match action {
            crate::sync::staging::CommitAction::Apply { rel_path, .. } => rel_path,
            crate::sync::staging::CommitAction::Delete { rel_path } => rel_path,
        };
        out.push(to_workspace_rel_path(target_kind, project_id, rel));
    }
}

/// 将 commit plan 中的 Apply/Delete 变更通过 SaveTransaction 写回 live root。
///
/// 删除 `git_finalize_recovery` 参数。
/// staging commit 不再承担 Git repo metadata finalize 职责；
/// workspace 本地 Git 如果要参与本地版本历史，放在 commit 完成后的 workspace
/// Git 层统一处理，不作为某个 remote provider 的 staging 模式。
pub(crate) fn apply_commit_plan_to_live(
    live_root: &Path,
    content_actions: &[crate::sync::staging::CommitAction],
    engine_state_actions: &[crate::sync::staging::CommitAction],
    backup_mode: bool,
) -> crate::error::Result<crate::storage::transaction::SaveTransaction> {
    if content_actions.is_empty() && engine_state_actions.is_empty() && !backup_mode {
        return Ok(crate::storage::transaction::SaveTransaction::new(live_root));
    }
    let mut tx = crate::storage::transaction::SaveTransaction::new(live_root);
    if backup_mode {
        tx.enable_backup_mode();
    }
    for action in engine_state_actions {
        match action {
            crate::sync::staging::CommitAction::Apply { rel_path, content } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_bytes(&rel_str, content)?;
            }
            crate::sync::staging::CommitAction::Delete { rel_path } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_delete(&rel_str);
            }
        }
    }
    for action in content_actions {
        match action {
            crate::sync::staging::CommitAction::Apply { rel_path, content } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_bytes(&rel_str, content)?;
            }
            crate::sync::staging::CommitAction::Delete { rel_path } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_delete(&rel_str);
            }
        }
    }
    tx.commit()?;
    Ok(tx)
}

pub(crate) enum TargetCommitResult {
    Ok,
    Skipped,
    Failed(String),
}

/// 在 staging cleanup 前，对 plan.conflict 的每个 StagingConflict 读取 incoming
/// 正文（staging_root/rel_path），用 save_conflict_copy 保存到 live target 的冲突快照，
/// 填入 StagingConflict.remote_snapshot_path。这样 record_staging_conflicts 映射成
/// SyncConflict 时能带上远端 snapshot，#757 冲突侧栏"用户看到什么就选择什么"。
///
/// 必须在 `run.cleanup()` 之前调用——cleanup 会删除 staging_root，之后读不到 incoming。
/// 已填快照（`remote_snapshot_path.is_some()`）的条目跳过，避免重复保存。
fn save_conflict_snapshots(
    live_root: &Path,
    staging_root: &Path,
    conflicts: &mut [crate::sync::staging::StagingConflict],
) -> crate::error::Result<()> {
    for sc in conflicts.iter_mut() {
        if sc.remote_snapshot_path.is_some() {
            continue;
        }
        // 只有 BothChanged 才保存 incoming snapshot。
        // RemoteDeleted 远端已删除，无 incoming 正文，不保存快照，
        // remote_snapshot_path 保持 None。
        if sc.kind != crate::sync::types::SyncConflictKind::BothChanged {
            continue;
        }
        let rel_str = sc.rel_path.to_string_lossy().to_string();
        let incoming_path = staging_root.join(&sc.rel_path);
        if !incoming_path.exists() {
            // incoming 不存在（理论上 BothChanged 应有 incoming），跳过，保留 None 兜底。
            continue;
        }
        let incoming = std::fs::read(&incoming_path)?;
        let snapshot_rel = crate::sync::lww::save_conflict_copy(live_root, &rel_str, &incoming)?;
        sc.remote_snapshot_path = Some(snapshot_rel);
    }
    Ok(())
}

pub(crate) struct StagingCommitOutcome {
    pub(crate) target_results: Vec<TargetCommitResult>,
    pub(crate) target_conflicts: Vec<Vec<crate::sync::staging::StagingConflict>>,
    /// 本次 commit 真正落盘（Apply/Delete）的
    /// workspace-relative paths。供 `record_workspace_history` 精确 stage，
    /// 替代全量 `&[]` 扫描。
    pub(crate) committed_paths: Vec<PathBuf>,
}

pub(crate) enum TargetCommitMode {
    Full,
    ConflictMetadataOnly,
    Skip,
    /// ReplaceProject 走专用 replace plan
    /// （staging 有 → Apply；live 有但 staging 没有 → Delete），不走普通三方合并。
    ReplaceProject,
}

pub(crate) fn target_commit_mode(status: &crate::sync::SyncStatus) -> TargetCommitMode {
    use crate::sync::SyncStatus;
    match status {
        SyncStatus::Success | SyncStatus::NoChanges | SyncStatus::LatestWinsApplied => {
            TargetCommitMode::Full
        }
        SyncStatus::Conflict | SyncStatus::PartialConflict => {
            TargetCommitMode::ConflictMetadataOnly
        }
        _ => TargetCommitMode::Skip,
    }
}

/// staging commit 简化为纯文件级 commit。
///
/// 旧 Git finalize 逻辑（`prepare_git_finalize` / `try_commit_git_finalize` /
/// `cleanup_repo_create_owner_marker` / `coordinate_rollback_after_finalize_failure`）
/// 已删除。workspace 本地 Git 如果要参与本地版本历史，放在 commit 完成后的
/// workspace Git 层统一处理，不作为某个 remote provider 的 staging 模式。
#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub(crate) fn apply_staging_commits_for_targets(
    staging_runs: &[crate::sync::staging::StagingRun],
    transfer_targets: &[crate::sync::types::TargetSyncResult],
) -> StagingCommitOutcome {
    let mut target_conflicts: Vec<Vec<crate::sync::staging::StagingConflict>> = Vec::new();
    let mut target_results: Vec<TargetCommitResult> = Vec::new();
    let mut committed_paths: Vec<PathBuf> = Vec::new();

    for (idx, run) in staging_runs.iter().enumerate() {
        //   有 DeleteProject lifecycle action 的 target
        // 跳过 staging commit（本地删除由 commit_full_sync 的 lifecycle action 处理）。
        // 否则 staging commit 会把 staging 里的旧作品内容写回 live，复活刚删掉的作品。
        // ReplaceProject 走整树替换 commit
        // （staging 有 → Apply；live 有但 staging 没有 → Delete），不再走普通
        // compute_commit_plan。在真正 Apply/Delete 之前用 snapshot_local_records_read_only
        // 重新计算当前 local target LWW，与 expected_local_lww guard 比较。
        let has_delete_action = transfer_targets
            .get(idx)
            .map(|t| {
                matches!(
                    t.local_lifecycle_action,
                    crate::sync::types::LocalLifecycleCommitAction::DeleteProject { .. }
                )
            })
            .unwrap_or(false);
        let has_replace_action = transfer_targets
            .get(idx)
            .map(|t| {
                matches!(
                    t.local_lifecycle_action,
                    crate::sync::types::LocalLifecycleCommitAction::ReplaceProject { .. }
                )
            })
            .unwrap_or(false);
        let mode = if has_delete_action {
            TargetCommitMode::Skip
        } else if has_replace_action {
            // ReplaceProject 走专用 replace plan。
            TargetCommitMode::ReplaceProject
        } else if let Some(target) = transfer_targets.get(idx) {
            target_commit_mode(&target.result.status)
        } else {
            TargetCommitMode::Skip
        };

        match mode {
            TargetCommitMode::Skip => {
                log::warn!(
                    "Staging commit: skipping target {} (run_id={})",
                    idx,
                    run.run_id()
                );
                target_results.push(TargetCommitResult::Skipped);
                target_conflicts.push(Vec::new());
                run.cleanup();
                continue;
            }
            TargetCommitMode::Full => {
                let live_root = run.target_live_root();
                let mut plan = match run.compute_commit_plan(live_root) {
                    Ok(plan) => plan,
                    Err(e) => {
                        let msg = format!("compute_commit_plan failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                };

                let mut tx = match apply_commit_plan_to_live(
                    live_root,
                    &plan.content_actions,
                    &plan.engine_state_actions,
                    false,
                ) {
                    Ok(tx) => tx,
                    Err(e) => {
                        let msg = format!("apply_commit_plan_to_live failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                };

                match tx.finish() {
                    Ok(()) => {
                        // 收集本 target 真正
                        // Apply/Delete 的 rel_path，转成 workspace-relative。
                        //   committed_paths 只收集
                        // content_actions，不收集 engine_state_actions——
                        // sync engine state（manifest.sync.json/state.local.json/
                        // conflicts.json 等）不进入本地 Git history。
                        let (kind, pid) = transfer_targets
                            .get(idx)
                            .map(|t| (t.target_kind.as_str(), t.project_id.as_deref()))
                            .unwrap_or(("", None));
                        collect_action_paths(
                            kind,
                            pid,
                            &plan.content_actions,
                            &mut committed_paths,
                        );
                        // #757：在 cleanup 前保存远端快照到 plan.conflict，
                        // 让 record_staging_conflicts 映射成 SyncConflict 时带上
                        // remote_snapshot_path。cleanup 后 staging_root 已删，无法再读。
                        let staging_root = run.staging_root();
                        if let Err(e) =
                            save_conflict_snapshots(live_root, &staging_root, &mut plan.conflict)
                        {
                            let msg = format!("save_conflict_snapshots failed: {}", e);
                            log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                            target_results.push(TargetCommitResult::Failed(msg));
                            target_conflicts.push(Vec::new());
                            run.cleanup();
                            continue;
                        }
                        target_conflicts.push(plan.conflict);
                        target_results.push(TargetCommitResult::Ok);
                        run.cleanup();
                    }
                    Err(e) => {
                        let msg = format!("tx.finish() failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                    }
                }
            }
            TargetCommitMode::ConflictMetadataOnly => {
                let live_root = run.target_live_root();
                let mut plan = match run.compute_commit_plan(live_root) {
                    Ok(plan) => plan,
                    Err(e) => {
                        let msg = format!("compute_commit_plan failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                };
                let transfer_conflict_paths: std::collections::HashSet<String> = transfer_targets
                    .get(idx)
                    .map(|t| {
                        t.result
                            .conflicts
                            .iter()
                            .map(|c| c.local_path.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                let safe_content_actions: Vec<_> = plan
                    .content_actions
                    .iter()
                    .filter(|action| {
                        let rel = match action {
                            crate::sync::staging::CommitAction::Apply { rel_path, .. } => {
                                rel_path.to_string_lossy().to_string()
                            }
                            crate::sync::staging::CommitAction::Delete { rel_path } => {
                                rel_path.to_string_lossy().to_string()
                            }
                        };
                        !transfer_conflict_paths.contains(&rel)
                    })
                    .cloned()
                    .collect();
                if let Err(e) = apply_commit_plan_to_live(
                    live_root,
                    &safe_content_actions,
                    &plan.engine_state_actions,
                    false,
                )
                .map(|_tx| ())
                {
                    let msg = format!("apply_commit_plan_to_live failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    target_results.push(TargetCommitResult::Failed(msg));
                    target_conflicts.push(Vec::new());
                    run.cleanup();
                    continue;
                }
                // ConflictMetadataOnly 也落盘了
                // safe_content_actions + engine_state_actions，收集它们的 rel_path。
                //   committed_paths 只收集
                // safe_content_actions，不收集 engine_state_actions。
                let (kind, pid) = transfer_targets
                    .get(idx)
                    .map(|t| (t.target_kind.as_str(), t.project_id.as_deref()))
                    .unwrap_or(("", None));
                collect_action_paths(kind, pid, &safe_content_actions, &mut committed_paths);
                // #757：在 cleanup 前保存远端快照到 plan.conflict（同 Full 模式）。
                let staging_root = run.staging_root();
                if let Err(e) =
                    save_conflict_snapshots(live_root, &staging_root, &mut plan.conflict)
                {
                    let msg = format!("save_conflict_snapshots failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    target_results.push(TargetCommitResult::Failed(msg));
                    target_conflicts.push(Vec::new());
                    run.cleanup();
                    continue;
                }
                target_conflicts.push(plan.conflict);
                target_results.push(TargetCommitResult::Ok);
                run.cleanup();
            }
            TargetCommitMode::ReplaceProject => {
                // ReplaceProject 走专用 replace plan。
                let live_root = run.target_live_root();
                let staging_root = run.staging_root();

                // 1. guard 检查：用 snapshot_local_records_read_only 重新计算当前
                //    local target LWW，与 expected_local_lww 严格比较。
                // expected_local_lww 非 Option —
                // 破坏性 action 必须携带 guard。
                let expected_lww =
                    transfer_targets
                        .get(idx)
                        .and_then(|t| match &t.local_lifecycle_action {
                            crate::sync::types::LocalLifecycleCommitAction::ReplaceProject {
                                expected_local_lww,
                                ..
                            } => Some(crate::sync::full_sync::LiveTargetLww {
                                lww_time_ms: expected_local_lww.lww_time_ms,
                                device_id: expected_local_lww.device_id.clone(),
                            }),
                            _ => None,
                        });
                let expected_lww = match expected_lww {
                    Some(lww) => lww,
                    None => {
                        // 不应发生：TargetCommitMode::ReplaceProject 只对 ReplaceProject action 设置。
                        let msg =
                            "ReplaceProject commit mode but no ReplaceProject action with guard"
                                .to_string();
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                };
                match crate::sync::staging::replace::check_replace_project_guard(
                    live_root,
                    &expected_lww,
                ) {
                    crate::sync::staging::replace::ReplaceProjectGuardResult::Ok => {
                        // guard 通过，继续执行 replace plan。
                    }
                    crate::sync::staging::replace::ReplaceProjectGuardResult::Err(e) => {
                        let msg = match e {
                            crate::sync::staging::replace::ReplaceProjectGuardError::LocalAdvanced {
                                expected,
                                current,
                            } => format!(
                                "ReplaceProject guard failed: local advanced \
                                 (expected lww_time={} device_id={}, current lww_time={} device_id={}) \
                                 — not touching live",
                                expected.lww_time_ms,
                                expected.device_id,
                                current.lww_time_ms,
                                current.device_id
                            ),
                            crate::sync::staging::replace::ReplaceProjectGuardError::SnapshotFailed(err) => {
                                format!("ReplaceProject guard snapshot failed: {err}")
                            }
                        };
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                }

                // 2. 构建 replace plan（staging 有 → Apply；live 有但 staging 没有 → Delete）。
                let mut plan = match crate::sync::staging::replace::build_replace_project_plan(
                    live_root,
                    &staging_root,
                ) {
                    Ok(plan) => plan,
                    Err(e) => {
                        let msg = format!("build_replace_project_plan failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                };

                // 3. 应用 replace plan 到 live。
                let mut tx = match apply_commit_plan_to_live(
                    live_root,
                    &plan.content_actions,
                    &plan.engine_state_actions,
                    false,
                ) {
                    Ok(tx) => tx,
                    Err(e) => {
                        let msg = format!("apply_commit_plan_to_live failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                };

                match tx.finish() {
                    Ok(()) => {
                        let (kind, pid) = transfer_targets
                            .get(idx)
                            .map(|t| (t.target_kind.as_str(), t.project_id.as_deref()))
                            .unwrap_or(("", None));
                        collect_action_paths(
                            kind,
                            pid,
                            &plan.content_actions,
                            &mut committed_paths,
                        );
                        // #757：在 cleanup 前保存远端快照（防御性——ReplaceProject
                        // 通常无冲突，但保持与其他分支一致）。
                        if let Err(e) =
                            save_conflict_snapshots(live_root, &staging_root, &mut plan.conflict)
                        {
                            let msg = format!("save_conflict_snapshots failed: {}", e);
                            log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                            target_results.push(TargetCommitResult::Failed(msg));
                            target_conflicts.push(Vec::new());
                            run.cleanup();
                            continue;
                        }
                        target_conflicts.push(plan.conflict);
                        target_results.push(TargetCommitResult::Ok);
                        run.cleanup();
                    }
                    Err(e) => {
                        let msg = format!("tx.finish() failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                    }
                }
            }
        }
    }

    StagingCommitOutcome {
        target_results,
        target_conflicts,
        committed_paths,
    }
}

/// 单 target staging commit — 新编排入口。
///
/// 与 `apply_staging_commits_for_targets` 中对单个 target 的处理逻辑一致，
/// 但有以下区别：
/// - **不调用 `run.cleanup()`**：由调用方负责 cleanup，便于在 commit 后、cleanup 前
///   插入 `record_staging_conflicts` 等操作。
/// - **输入是单个 `&StagingRun` 和 `&TargetSyncResult`**，不是批量切片。
/// - **输出是 `(TargetCommitResult, Vec<StagingConflict>, Vec<PathBuf>)`**：
///   commit result, conflicts, committed_paths。
///
/// 复用现有 Full / ConflictMetadataOnly / ReplaceProject / Skip 规则。
#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub(crate) fn commit_single_target_staging(
    run: &crate::sync::staging::StagingRun,
    target: &crate::sync::types::TargetSyncResult,
) -> (
    TargetCommitResult,
    Vec<crate::sync::staging::StagingConflict>,
    Vec<PathBuf>,
) {
    let has_delete_action = matches!(
        target.local_lifecycle_action,
        crate::sync::types::LocalLifecycleCommitAction::DeleteProject { .. }
    );
    let has_replace_action = matches!(
        target.local_lifecycle_action,
        crate::sync::types::LocalLifecycleCommitAction::ReplaceProject { .. }
    );
    let mode = if has_delete_action {
        TargetCommitMode::Skip
    } else if has_replace_action {
        TargetCommitMode::ReplaceProject
    } else {
        target_commit_mode(&target.result.status)
    };

    match mode {
        TargetCommitMode::Skip => {
            log::warn!("Staging commit: skipping target (run_id={})", run.run_id());
            (TargetCommitResult::Skipped, Vec::new(), Vec::new())
        }
        TargetCommitMode::Full => {
            let live_root = run.target_live_root();
            let mut plan = match run.compute_commit_plan(live_root) {
                Ok(plan) => plan,
                Err(e) => {
                    let msg = format!("compute_commit_plan failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            };

            let mut tx = match apply_commit_plan_to_live(
                live_root,
                &plan.content_actions,
                &plan.engine_state_actions,
                false,
            ) {
                Ok(tx) => tx,
                Err(e) => {
                    let msg = format!("apply_commit_plan_to_live failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            };

            match tx.finish() {
                Ok(()) => {
                    let mut committed_paths = Vec::new();
                    collect_action_paths(
                        target.target_kind.as_str(),
                        target.project_id.as_deref(),
                        &plan.content_actions,
                        &mut committed_paths,
                    );
                    let staging_root = run.staging_root();
                    if let Err(e) =
                        save_conflict_snapshots(live_root, &staging_root, &mut plan.conflict)
                    {
                        let msg = format!("save_conflict_snapshots failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        return (TargetCommitResult::Failed(msg), Vec::new(), committed_paths);
                    }
                    (TargetCommitResult::Ok, plan.conflict, committed_paths)
                }
                Err(e) => {
                    let msg = format!("tx.finish() failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    (TargetCommitResult::Failed(msg), Vec::new(), Vec::new())
                }
            }
        }
        TargetCommitMode::ConflictMetadataOnly => {
            let live_root = run.target_live_root();
            let mut plan = match run.compute_commit_plan(live_root) {
                Ok(plan) => plan,
                Err(e) => {
                    let msg = format!("compute_commit_plan failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            };
            let transfer_conflict_paths: std::collections::HashSet<String> = target
                .result
                .conflicts
                .iter()
                .map(|c| c.local_path.clone())
                .collect();
            let safe_content_actions: Vec<_> = plan
                .content_actions
                .iter()
                .filter(|action| {
                    let rel = match action {
                        crate::sync::staging::CommitAction::Apply { rel_path, .. } => {
                            rel_path.to_string_lossy().to_string()
                        }
                        crate::sync::staging::CommitAction::Delete { rel_path } => {
                            rel_path.to_string_lossy().to_string()
                        }
                    };
                    !transfer_conflict_paths.contains(&rel)
                })
                .cloned()
                .collect();
            if let Err(e) = apply_commit_plan_to_live(
                live_root,
                &safe_content_actions,
                &plan.engine_state_actions,
                false,
            )
            .map(|_tx| ())
            {
                let msg = format!("apply_commit_plan_to_live failed: {}", e);
                log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
            }
            let mut committed_paths = Vec::new();
            collect_action_paths(
                target.target_kind.as_str(),
                target.project_id.as_deref(),
                &safe_content_actions,
                &mut committed_paths,
            );
            let staging_root = run.staging_root();
            if let Err(e) = save_conflict_snapshots(live_root, &staging_root, &mut plan.conflict) {
                let msg = format!("save_conflict_snapshots failed: {}", e);
                log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                return (TargetCommitResult::Failed(msg), Vec::new(), committed_paths);
            }
            (TargetCommitResult::Ok, plan.conflict, committed_paths)
        }
        TargetCommitMode::ReplaceProject => {
            let live_root = run.target_live_root();
            let staging_root = run.staging_root();

            let expected_lww = match &target.local_lifecycle_action {
                crate::sync::types::LocalLifecycleCommitAction::ReplaceProject {
                    expected_local_lww,
                    ..
                } => Some(crate::sync::full_sync::LiveTargetLww {
                    lww_time_ms: expected_local_lww.lww_time_ms,
                    device_id: expected_local_lww.device_id.clone(),
                }),
                _ => None,
            };
            let expected_lww = match expected_lww {
                Some(lww) => lww,
                None => {
                    let msg = "ReplaceProject commit mode but no ReplaceProject action with guard"
                        .to_string();
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            };
            match crate::sync::staging::replace::check_replace_project_guard(
                live_root,
                &expected_lww,
            ) {
                crate::sync::staging::replace::ReplaceProjectGuardResult::Ok => {}
                crate::sync::staging::replace::ReplaceProjectGuardResult::Err(e) => {
                    let msg = match e {
                        crate::sync::staging::replace::ReplaceProjectGuardError::LocalAdvanced {
                            expected,
                            current,
                        } => format!(
                            "ReplaceProject guard failed: local advanced \
                             (expected lww_time={} device_id={}, current lww_time={} device_id={}) \
                             — not touching live",
                            expected.lww_time_ms,
                            expected.device_id,
                            current.lww_time_ms,
                            current.device_id
                        ),
                        crate::sync::staging::replace::ReplaceProjectGuardError::SnapshotFailed(err) => {
                            format!("ReplaceProject guard snapshot failed: {err}")
                        }
                    };
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            }

            let mut plan = match crate::sync::staging::replace::build_replace_project_plan(
                live_root,
                &staging_root,
            ) {
                Ok(plan) => plan,
                Err(e) => {
                    let msg = format!("build_replace_project_plan failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            };

            let mut tx = match apply_commit_plan_to_live(
                live_root,
                &plan.content_actions,
                &plan.engine_state_actions,
                false,
            ) {
                Ok(tx) => tx,
                Err(e) => {
                    let msg = format!("apply_commit_plan_to_live failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            };

            match tx.finish() {
                Ok(()) => {
                    let mut committed_paths = Vec::new();
                    collect_action_paths(
                        target.target_kind.as_str(),
                        target.project_id.as_deref(),
                        &plan.content_actions,
                        &mut committed_paths,
                    );
                    if let Err(e) =
                        save_conflict_snapshots(live_root, &staging_root, &mut plan.conflict)
                    {
                        let msg = format!("save_conflict_snapshots failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        return (TargetCommitResult::Failed(msg), Vec::new(), committed_paths);
                    }
                    (TargetCommitResult::Ok, plan.conflict, committed_paths)
                }
                Err(e) => {
                    let msg = format!("tx.finish() failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    (TargetCommitResult::Failed(msg), Vec::new(), Vec::new())
                }
            }
        }
    }
}
