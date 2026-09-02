use std::path::Path;

/// 将 commit plan 中的 Apply/Delete 变更通过 SaveTransaction 写回 live root。
pub(crate) fn apply_commit_plan_to_live(
    live_root: &Path,
    content_actions: &[crate::sync::staging::CommitAction],
    engine_state_actions: &[crate::sync::staging::CommitAction],
    backup_mode: bool,
    git_finalize_recovery: Option<crate::sync::git::GitFinalizeRecoveryRecord>,
) -> crate::error::Result<crate::storage::transaction::SaveTransaction> {
    if content_actions.is_empty()
        && engine_state_actions.is_empty()
        && !backup_mode
        && git_finalize_recovery.is_none()
    {
        return Ok(crate::storage::transaction::SaveTransaction::new(live_root));
    }
    let mut tx = crate::storage::transaction::SaveTransaction::new(live_root);
    if backup_mode {
        tx.enable_backup_mode();
    }
    if let Some(record) = git_finalize_recovery {
        tx.set_git_finalize_recovery(record);
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

/// 统一 rollback coordinator — commit_git_finalize 失败后调用。
pub(crate) fn coordinate_rollback_after_finalize_failure(
    live_root: &Path,
    snapshot: &crate::sync::git::GitMetadataSnapshot,
    plan: &crate::sync::git::GitFinalizePlan,
    _seed_state: &crate::sync::git::seed::GitSeedState,
    tx: &mut crate::storage::transaction::SaveTransaction,
    explicit_git_dir: Option<&Path>,
) -> crate::error::Result<()> {
    let inspect_state =
        crate::sync::git::inspect_git_rollback_state(live_root, snapshot, plan, explicit_git_dir)?;
    match inspect_state {
        crate::sync::git::GitRollbackState::NeedsRollback => {
            let outcome = crate::sync::git::rollback_git_finalize(
                live_root,
                snapshot,
                plan,
                explicit_git_dir,
            )?;
            match outcome {
                crate::sync::git::GitRollbackOutcome::Reverted => tx.rollback(),
                crate::sync::git::GitRollbackOutcome::ConcurrentChanged => {
                    Err(crate::Error::Io(std::io::Error::other(
                        "coordinate_rollback: rollback detected concurrent change",
                    )))
                }
                crate::sync::git::GitRollbackOutcome::RepoInstallCommitted => Ok(()),
            }
        }
        crate::sync::git::GitRollbackState::RepoInstallCommitted => Ok(()),
        crate::sync::git::GitRollbackState::ConcurrentChanged => {
            Err(crate::Error::Io(std::io::Error::other(
                "coordinate_rollback: inspect_git_rollback_state returned \
                 ConcurrentChanged — cannot safely prove Git metadata unchanged, \
                 preserving transaction for next recovery",
            )))
        }
    }
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
                let needs_git_finalize = run.git_seed_state().is_some();
                let (git_snapshot, git_plan) = if needs_git_finalize {
                    let seed_state = match run.git_seed_state() {
                        Some(s) => s,
                        None => {
                            let msg = "git_seed_state missing";
                            log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                            target_results.push(TargetCommitResult::Failed(msg.to_string()));
                            target_conflicts.push(Vec::new());
                            run.cleanup();
                            continue;
                        }
                    };
                    match crate::sync::git::prepare_git_finalize(
                        live_root,
                        seed_state,
                        &run.staging_root(),
                        run.git_layout().map(|l| l.git_dir.as_path()),
                    ) {
                        Ok((snap, plan)) => (snap, plan),
                        Err(e) => {
                            let msg = format!("prepare_git_finalize failed: {}", e);
                            log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                            target_results.push(TargetCommitResult::Failed(msg));
                            target_conflicts.push(Vec::new());
                            run.cleanup();
                            continue;
                        }
                    }
                } else {
                    (
                        crate::sync::git::GitMetadataSnapshot {
                            head: crate::sync::git::RefSnapshot::DidNotExist,
                            refs: std::collections::BTreeMap::new(),
                            index: crate::sync::git::IndexSnapshot::Missing,
                            repo_existed: false,
                        },
                        crate::sync::git::GitFinalizePlan::default(),
                    )
                };

                let git_finalize_recovery = if let (Some(snap), Some(seed_state), Some(plan)) =
                    (Some(&git_snapshot), run.git_seed_state(), Some(&git_plan))
                {
                    Some(crate::sync::git::GitFinalizeRecoveryRecord {
                        seed_state: crate::sync::git::SerializableGitSeedState::from_seed_state(
                            seed_state,
                        ),
                        metadata_snapshot: snap.clone(),
                        plan: plan.clone(),
                        mutation_log: crate::sync::git::GitFinalizeMutationLog::default(),
                        git_dir: run.git_layout().map(|l| l.git_dir.clone()),
                        worktree_root: run.git_layout().map(|l| l.worktree_root.clone()),
                    })
                } else {
                    None
                };

                let mut tx = match apply_commit_plan_to_live(
                    live_root,
                    &plan.content_actions,
                    &plan.engine_state_actions,
                    needs_git_finalize,
                    git_finalize_recovery,
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

                use crate::sync::git::GitFinalizeError;
                let git_plan_ref = if needs_git_finalize {
                    Some(&git_plan)
                } else {
                    None
                };

                if needs_git_finalize {
                    if let Err(e) = tx.preflight_rollback_material() {
                        let msg = format!("preflight_rollback_material failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                }

                match crate::sync::git::try_commit_git_finalize(
                    live_root,
                    &run.staging_root(),
                    run.git_seed_state(),
                    Some(&git_snapshot),
                    git_plan_ref,
                    run.git_layout().map(|l| l.git_dir.as_path()),
                ) {
                    Ok(()) => match tx.finish() {
                        Ok(()) => {
                            if let Some(plan) = git_plan_ref {
                                crate::sync::git::cleanup_repo_create_owner_marker(
                                    live_root,
                                    plan,
                                    run.git_layout().map(|l| l.git_dir.as_path()),
                                );
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
                    },
                    Err(GitFinalizeError::FinalizeFailed(e)) => {
                        log::warn!("Staging commit: git finalize failed: {}", e);
                        let rollback_err = if let (Some(seed_state), Some(plan)) =
                            (run.git_seed_state(), git_plan_ref)
                        {
                            coordinate_rollback_after_finalize_failure(
                                live_root,
                                &git_snapshot,
                                plan,
                                seed_state,
                                &mut tx,
                                run.git_layout().map(|l| l.git_dir.as_path()),
                            )
                        } else {
                            tx.rollback()
                        };
                        if let Err(rb_err) = rollback_err {
                            log::warn!("Staging commit: rollback also failed: {}", rb_err);
                        }
                        let msg = format!("git repo-metadata finalize failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                    }
                    Err(GitFinalizeError::ConcurrentMetadataChanged { reason }) => {
                        log::warn!(
                            "Staging commit: concurrent git metadata changed: {}",
                            reason
                        );
                        if let Err(rb_err) = tx.rollback() {
                            log::warn!("Staging commit: rollback also failed: {}", rb_err);
                        }
                        let msg = format!("git finalize aborted: concurrent change: {}", reason);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                    }
                    Err(GitFinalizeError::RollbackFailed { finalize, rollback }) => {
                        let msg = format!("finalize+rollback failed: {}+{}", finalize, rollback);
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
                    None,
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
