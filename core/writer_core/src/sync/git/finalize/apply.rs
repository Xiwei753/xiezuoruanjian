use std::fs;
use std::path::Path;

use super::super::seed::GitSeedState;
use super::super::model::*;
use super::prepare::open_live_repo;
use super::index_ops::*;

// ── 公共 API ──

/// #644 评论 5475805198 第2节：应用 Git metadata 变更到 live。
pub fn commit_git_finalize(
    live_root: &Path,
    staging_root: &Path,
    seed_state: &GitSeedState,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    finalize_git_repo_metadata_inner(
        live_root,
        staging_root,
        seed_state,
        snapshot,
        plan,
        explicit_git_dir,
    )
}

/// #644 评论 5475805198 第2节：崩溃恢复。
pub fn recover_git_finalize(
    live_root: &Path,
    staging_root: &Path,
    seed_state: &GitSeedState,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    match finalize_git_repo_metadata_inner(
        live_root,
        staging_root,
        seed_state,
        snapshot,
        plan,
        explicit_git_dir,
    ) {
        Ok(()) => {
            log::info!("recover_git_finalize: successfully completed pending Git finalize");
            Ok(())
        }
        Err(e) => match &e {
            GitFinalizeError::ConcurrentMetadataChanged { .. } => {
                log::warn!(
                    "recover_git_finalize: {} (not rolling back Git metadata)",
                    e
                );
                Err(e)
            }
            _ => {
                log::warn!("recover_git_finalize: finalize failed ({}), rolling back", e);
                match super::super::rollback::rollback_git_finalize(live_root, snapshot, plan, None) {
                    Ok(GitRollbackOutcome::Reverted) => Err(e),
                    Ok(GitRollbackOutcome::ConcurrentChanged) => {
                        Err(GitFinalizeError::ConcurrentMetadataChanged {
                            reason: format!(
                                "rollback detected concurrent change during recovery: {}",
                                e
                            ),
                        })
                    }
                    Ok(GitRollbackOutcome::RepoInstallCommitted) => {
                        log::info!(
                            "recover_git_finalize: rollback saw repo install committed, \
                             treating tx as successful"
                        );
                        Ok(())
                    }
                    Err(rb_err) => Err(GitFinalizeError::RollbackFailed {
                        finalize: e.to_string(),
                        rollback: rb_err.to_string(),
                    }),
                }
            }
        },
    }
}

/// 包装函数，供 `sync_ops.rs` 调用。
pub fn try_commit_git_finalize(
    live_root: &Path,
    staging_root: &Path,
    seed_state: Option<&GitSeedState>,
    snapshot: Option<&GitMetadataSnapshot>,
    plan: Option<&GitFinalizePlan>,
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    let Some(state) = seed_state else {
        return Ok(());
    };
    let Some(snap) = snapshot else {
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other("missing GitMetadataSnapshot for Git backend"),
        )));
    };
    let Some(plan) = plan else {
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other("missing GitFinalizePlan for Git backend"),
        )));
    };
    commit_git_finalize(live_root, staging_root, state, snap, plan, explicit_git_dir)
}

/// #644 评论 5482310913 问题1：成功收尾时清理 owner marker。
pub fn cleanup_repo_create_owner_marker(
    live_root: &Path,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) {
    if plan.repo_create_owner.is_some() {
        let default_git_dir = live_root.join(".git");
        let git_dir_ref = explicit_git_dir.unwrap_or(&default_git_dir);
        let marker_path = git_dir_ref.join(".sujian-sync-owner");
        let _ = fs::remove_file(&marker_path);
    }
}

// ── 内部 finalize 实现 ──

#[allow(clippy::too_many_lines)]
fn finalize_git_repo_metadata_inner(
    live_root: &Path,
    staging_root: &Path,
    seed_state: &GitSeedState,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    crate::storage::git_runtime::ensure_initialized()?;

    let staging_git_dir = staging_root.join(".git");
    if !staging_git_dir.exists() {
        return Ok(());
    }

    let staging_repo = git2::Repository::open(staging_root).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize: failed to open staging repo: {e}"
        )))
    })?;

    let staging_head = staging_repo.head().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize: staging HEAD missing: {e}"
        )))
    })?;
    let new_oid = staging_head.target().ok_or_else(|| {
        crate::Error::Io(std::io::Error::other(
            "finalize: staging HEAD is unborn (no target OID)",
        ))
    })?;

    let staging_odb = staging_repo.odb().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!("finalize: staging odb: {e}")))
    })?;

    let branch_name = staging_head
        .shorthand()
        .ok_or_else(|| {
            crate::Error::Io(std::io::Error::other(
                "finalize: staging HEAD shorthand() returned None",
            ))
        })?
        .to_string();

    match seed_state {
        GitSeedState::NotGitRepo => finalize_not_git_repo(
            live_root, staging_root, &staging_repo, &staging_odb,
            new_oid, &branch_name, plan, explicit_git_dir,
        ),
        GitSeedState::Unborn { head_ref } => finalize_unborn(
            live_root, &staging_repo, &staging_odb, new_oid,
            head_ref, snapshot, plan, explicit_git_dir,
        ),
        GitSeedState::Existing { head_ref, head_oid } => finalize_existing(
            live_root, &staging_repo, &staging_odb, new_oid,
            head_ref, *head_oid, snapshot, plan, explicit_git_dir,
        ),
        GitSeedState::Detached { head_oid } => finalize_detached(
            live_root, &staging_repo, &staging_odb, new_oid,
            *head_oid, snapshot, plan, explicit_git_dir,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn finalize_not_git_repo(
    live_root: &Path,
    staging_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    branch_name: &str,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    use super::temp::*;

    let staging_git = staging_root.join(".git");
    let live_git = explicit_git_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| live_root.join(".git"));

    let tmp_id = plan
        .repo_create_owner
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let tmp_git = repo_install_tmp_path(live_root, explicit_git_dir, &tmp_id)
        .map_err(GitFinalizeError::FinalizeFailed)?;
    let mut _guard = TmpDirGuard::new(tmp_git.clone());

    copy_dir_recursive(&staging_git, _guard.path())?;

    let tmp_repo = git2::Repository::open(_guard.path()).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize_not_git_repo: open tmp repo: {e}"
        )))
    })?;
    let tmp_odb = tmp_repo.odb().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize_not_git_repo: tmp odb: {e}"
        )))
    })?;

    import_missing_objects(staging_odb, &tmp_odb)?;

    if !tmp_odb.exists(new_oid) {
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other(format!(
                "finalize_not_git_repo: new_oid {} not found in tmp after import",
                new_oid
            )),
        )));
    }

    let ref_name = format!("refs/heads/{}", branch_name);
    tmp_repo
        .reference(&ref_name, new_oid, true, "sync: init branch from staging")
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "finalize_not_git_repo: update-ref {} failed: {}",
                ref_name, e
            )))
        })?;

    update_live_index(&tmp_repo, staging_repo, new_oid)?;

    if let Some(owner) = &plan.repo_create_owner {
        let marker_path = _guard.path().join(".sujian-sync-owner");
        crate::storage::atomic_write_bytes(&marker_path, owner.as_bytes())
            .map_err(GitFinalizeError::FinalizeFailed)?;
    }

    if live_git.exists() {
        return Err(GitFinalizeError::ConcurrentMetadataChanged {
            reason: "live .git appeared during finalize (concurrent git init)".to_string(),
        });
    }

    let guard_path = _guard.disarm();
    if let Err(e) = fs::rename(&guard_path, &live_git) {
        let _ = fs::remove_dir_all(&guard_path);
        if live_git.exists() {
            return Err(GitFinalizeError::ConcurrentMetadataChanged {
                reason: "live .git appeared during rename (concurrent git init)".to_string(),
            });
        }
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other(format!(
                "finalize_not_git_repo: rename tmp -> .git failed: {e}"
            )),
        )));
    }

    crate::storage::sync_parent(&live_git).map_err(GitFinalizeError::FinalizeFailed)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn finalize_unborn(
    live_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    head_ref: &str,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    let live_repo =
        open_live_repo(live_root, explicit_git_dir).map_err(GitFinalizeError::FinalizeFailed)?;
    let live_odb = live_repo.odb().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!("finalize_unborn: live odb: {e}")))
    })?;

    import_missing_objects(staging_odb, &live_odb)?;

    if !live_odb.exists(new_oid) {
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other(format!(
                "finalize_unborn: new_oid {} not found in live after import",
                new_oid
            )),
        )));
    }

    verify_git_metadata_unchanged(live_root, snapshot, head_ref, explicit_git_dir)?;

    let index_lock_owner = plan.index_lock_owner.as_deref().ok_or_else(|| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(
            "finalize_unborn: plan.index_lock_owner is None",
        )))
    })?;
    install_index_with_lock(
        live_root, &live_repo, staging_repo, new_oid,
        snapshot, index_lock_owner, explicit_git_dir,
    )?;

    {
        use super::super::tx::RefTransaction;

        let ref_tx_owner = plan.ref_tx_owner.as_deref().ok_or_else(|| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(
                "finalize_unborn: plan.ref_tx_owner is None",
            )))
        })?;

        let ref_names = &plan.ref_lock_names;
        let mut ref_tx = RefTransaction::acquire_all_refs(&live_repo, ref_names, ref_tx_owner)
            .map_err(GitFinalizeError::FinalizeFailed)?;

        let raw_head = ref_tx.find_reference("HEAD").map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "finalize_unborn: HEAD reference not found: {e}"
            )))
        })?;
        if let Some(sym_target) = raw_head.symbolic_target() {
            if sym_target != head_ref {
                return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                    std::io::Error::other(format!(
                        "finalize_unborn: HEAD changed from {} to {}", head_ref, sym_target
                    )),
                )));
            }
        } else {
            return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                std::io::Error::other("finalize_unborn: HEAD is no longer symbolic"),
            )));
        }

        match ref_tx.find_reference(head_ref) {
            Ok(_) => {
                return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                    std::io::Error::other(format!(
                        "finalize_unborn: branch ref {} already exists", head_ref
                    )),
                )));
            }
            Err(e) if e.code() == git2::ErrorCode::NotFound => {}
            Err(e) => {
                return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                    std::io::Error::other(format!(
                        "finalize_unborn: find_reference({}) failed: {}", head_ref, e
                    )),
                )));
            }
        }

        ref_tx.set_target(head_ref, new_oid, "sync: create branch from staging")?;
        execute_plan_refs_under_lock(&mut ref_tx, plan, Some(head_ref))?;
        ref_tx.commit().map_err(GitFinalizeError::FinalizeFailed)?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn finalize_existing(
    live_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    head_ref: &str,
    base_oid: git2::Oid,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    let live_repo =
        open_live_repo(live_root, explicit_git_dir).map_err(GitFinalizeError::FinalizeFailed)?;
    let live_odb = live_repo.odb().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!("finalize_existing: live odb: {e}")))
    })?;

    import_missing_objects(staging_odb, &live_odb)?;

    if !live_odb.exists(new_oid) {
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other(format!(
                "finalize_existing: new_oid {} not found in live", new_oid
            )),
        )));
    }

    verify_git_metadata_unchanged(live_root, snapshot, head_ref, explicit_git_dir)?;

    let index_lock_owner = plan.index_lock_owner.as_deref().ok_or_else(|| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(
            "finalize_existing: plan.index_lock_owner is None",
        )))
    })?;
    install_index_with_lock(
        live_root, &live_repo, staging_repo, new_oid,
        snapshot, index_lock_owner, explicit_git_dir,
    )?;

    {
        use super::super::tx::RefTransaction;

        let ref_tx_owner = plan.ref_tx_owner.as_deref().ok_or_else(|| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(
                "finalize_existing: plan.ref_tx_owner is None",
            )))
        })?;

        let ref_names = &plan.ref_lock_names;
        let mut ref_tx = RefTransaction::acquire_all_refs(&live_repo, ref_names, ref_tx_owner)
            .map_err(GitFinalizeError::FinalizeFailed)?;

        let raw_head = ref_tx.find_reference("HEAD").map_err(|e| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
                "finalize_existing: HEAD reference not found: {e}"
            ))))
        })?;
        match raw_head.symbolic_target() {
            Some(sym_target) if sym_target == head_ref => {}
            Some(other) => {
                return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                    std::io::Error::other(format!(
                        "finalize_existing: HEAD now points to {} but seed was {}", other, head_ref
                    )),
                )));
            }
            None => {
                return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                    std::io::Error::other(
                        "finalize_existing: HEAD detached after preflight",
                    ),
                )));
            }
        }
        drop(raw_head);

        let branch_ref = ref_tx.find_reference(head_ref).map_err(|e| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
                "finalize_existing: branch ref {} not found: {e}", head_ref
            ))))
        })?;
        let current_branch_oid = branch_ref.target().ok_or_else(|| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
                "finalize_existing: branch ref {} has no target", head_ref
            ))))
        })?;
        if current_branch_oid != base_oid {
            return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                std::io::Error::other(format!(
                    "finalize_existing: branch {} changed from {} to {}",
                    head_ref, base_oid, current_branch_oid
                )),
            )));
        }
        drop(branch_ref);

        ref_tx.set_target(head_ref, new_oid, "sync: finalize git repo metadata")?;
        execute_plan_refs_under_lock(&mut ref_tx, plan, Some(head_ref))?;
        ref_tx.commit().map_err(GitFinalizeError::FinalizeFailed)?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn finalize_detached(
    live_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    base_oid: git2::Oid,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    let live_repo =
        open_live_repo(live_root, explicit_git_dir).map_err(GitFinalizeError::FinalizeFailed)?;
    let live_odb = live_repo.odb().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!("finalize_detached: live odb: {e}")))
    })?;

    import_missing_objects(staging_odb, &live_odb)?;

    if !live_odb.exists(new_oid) {
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other(format!(
                "finalize_detached: new_oid {} not found in live", new_oid
            )),
        )));
    }

    verify_git_metadata_unchanged(live_root, snapshot, "HEAD", explicit_git_dir)?;

    let index_lock_owner = plan.index_lock_owner.as_deref().ok_or_else(|| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(
            "finalize_detached: plan.index_lock_owner is None",
        )))
    })?;
    install_index_with_lock(
        live_root, &live_repo, staging_repo, new_oid,
        snapshot, index_lock_owner, explicit_git_dir,
    )?;

    {
        use super::super::tx::RefTransaction;

        let ref_tx_owner = plan.ref_tx_owner.as_deref().ok_or_else(|| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(
                "finalize_detached: plan.ref_tx_owner is None",
            )))
        })?;

        let ref_names = &plan.ref_lock_names;
        let mut ref_tx = RefTransaction::acquire_all_refs(&live_repo, ref_names, ref_tx_owner)
            .map_err(GitFinalizeError::FinalizeFailed)?;

        let raw_head = ref_tx.find_reference("HEAD").map_err(|e| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
                "finalize_detached: HEAD reference not found: {e}"
            ))))
        })?;
        if raw_head.symbolic_target().is_some() {
            return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                std::io::Error::other("finalize_detached: HEAD is now symbolic"),
            )));
        }
        let current_head_oid = raw_head.target().ok_or_else(|| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(
                "finalize_detached: HEAD has no target",
            )))
        })?;
        if current_head_oid != base_oid {
            return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                std::io::Error::other(format!(
                    "finalize_detached: HEAD changed from {} to {}", base_oid, current_head_oid
                )),
            )));
        }
        drop(raw_head);

        ref_tx.set_target("HEAD", new_oid, "sync: finalize detached HEAD")?;
        execute_plan_refs_under_lock(&mut ref_tx, plan, Some("HEAD"))?;
        ref_tx.commit().map_err(GitFinalizeError::FinalizeFailed)?;
    }

    Ok(())
}
