use std::path::Path;

/// 将 commit plan 中的 Apply/Delete 变更通过 SaveTransaction 写回 live root。
///
/// #645 评论 5504296097 第2点：删除 `git_finalize_recovery` 参数。
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

pub(crate) struct StagingCommitOutcome {
    pub(crate) target_results: Vec<TargetCommitResult>,
    pub(crate) target_conflicts: Vec<Vec<crate::sync::staging::StagingConflict>>,
}

pub(crate) enum TargetCommitMode {
    Full,
    ConflictMetadataOnly,
    Skip,
}

pub(crate) fn target_commit_mode(status: &crate::sync::SyncStatus) -> TargetCommitMode {
    use crate::sync::SyncStatus;
    match status {
        SyncStatus::Success
        | SyncStatus::NoChanges
        | SyncStatus::LatestWinsApplied
        | SyncStatus::BranchMissingRecovered => TargetCommitMode::Full,
        SyncStatus::Conflict | SyncStatus::PartialConflict => {
            TargetCommitMode::ConflictMetadataOnly
        }
        _ => TargetCommitMode::Skip,
    }
}

/// #645 评论 5504296097 第2点：staging commit 简化为纯文件级 commit。
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

    for (idx, run) in staging_runs.iter().enumerate() {
        let mode = if let Some(target) = transfer_targets.get(idx) {
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
                let plan = match run.compute_commit_plan(live_root) {
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
                let plan = match run.compute_commit_plan(live_root) {
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
                target_conflicts.push(plan.conflict);
                target_results.push(TargetCommitResult::Ok);
                run.cleanup();
            }
        }
    }

    StagingCommitOutcome {
        target_results,
        target_conflicts,
    }
}
