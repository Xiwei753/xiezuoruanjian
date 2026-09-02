use std::fs;
use std::path::Path;

use super::super::model::*;
use super::temp::*;
use crate::error::Result;

/// #644 评论 5480360027 修复点 3 + #644 评论 5481496190 问题2 +
/// #644 评论 5486167472 问题1 + #644 评论 5486852142 问题1：index 原生锁边界 + 持久 ownership。
pub(crate) fn install_index_with_lock(
    live_root: &Path,
    _live_repo: &git2::Repository,
    staging_repo: &git2::Repository,
    new_oid: git2::Oid,
    snapshot: &GitMetadataSnapshot,
    owner: &str,
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    let target_index_bytes = generate_target_index_bytes(staging_repo, new_oid)?;

    let git_dir = explicit_git_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| live_root.join(".git"));
    let index_path = git_dir.join("index");

    let mut lock =
        match super::super::locks::OwnedIndexLock::acquire(&git_dir, owner, &target_index_bytes)? {
            super::super::locks::AcquireOutcome::NewlyAcquired(lock) => lock,
            super::super::locks::AcquireOutcome::AlreadyCommitted => {
                return Ok(());
            }
        };

    let current_index = if index_path.exists() {
        let bytes = fs::read(&index_path)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        IndexSnapshot::Bytes(bytes)
    } else {
        IndexSnapshot::Missing
    };
    if !index_snapshot_eq(&current_index, &snapshot.index) {
        return Err(GitFinalizeError::ConcurrentMetadataChanged {
            reason: "index changed between verify and acquire of index.lock".to_string(),
        });
    }

    lock.commit_rename(&index_path)?;

    Ok(())
}

fn generate_target_index_bytes(
    staging_repo: &git2::Repository,
    new_oid: git2::Oid,
) -> std::result::Result<Vec<u8>, GitFinalizeError> {
    let new_commit = staging_repo.find_commit(new_oid).map_err(|e| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
            "generate_target_index_bytes: find commit: {e}"
        ))))
    })?;
    let new_tree = new_commit.tree().map_err(|e| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
            "generate_target_index_bytes: find tree: {e}"
        ))))
    })?;

    let staging_git_dir = staging_repo.path().to_path_buf();
    let tmp_id = uuid::Uuid::new_v4().to_string();
    let tmp_index_path = staging_git_dir.join(format!("index.sujian-tmp-{}", tmp_id));
    let tmp_index_guard = TmpFileGuard::new(tmp_index_path.clone());

    let mut index = git2::Index::open(&tmp_index_path).map_err(|e| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
            "generate_target_index_bytes: open tmp index: {e}"
        ))))
    })?;
    index.read_tree(&new_tree).map_err(|e| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
            "generate_target_index_bytes: read_tree: {e}"
        ))))
    })?;
    index.write().map_err(|e| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
            "generate_target_index_bytes: write tmp index: {e}"
        ))))
    })?;

    let bytes = fs::read(&tmp_index_path)
        .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
    drop(tmp_index_guard);

    Ok(bytes)
}

pub(crate) fn import_missing_objects(staging_odb: &git2::Odb, live_odb: &git2::Odb) -> Result<()> {
    let mut missing_oids: Vec<git2::Oid> = Vec::new();
    staging_odb
        .foreach(|oid| {
            if !live_odb.exists(*oid) {
                missing_oids.push(*oid);
            }
            true
        })
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "import_missing_objects: foreach: {e}"
            )))
        })?;

    for oid in &missing_oids {
        let obj = staging_odb.read(*oid).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "import_missing_objects: read {}: {}",
                oid, e
            )))
        })?;
        live_odb.write(obj.kind(), obj.data()).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "import_missing_objects: write {}: {}",
                oid, e
            )))
        })?;
    }

    Ok(())
}

pub(crate) fn verify_git_metadata_unchanged(
    live_root: &Path,
    snapshot: &GitMetadataSnapshot,
    head_ref: &str,
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    let live_repo = super::prepare::open_live_repo(live_root, explicit_git_dir)
        .map_err(GitFinalizeError::FinalizeFailed)?;

    let current_head = read_ref_snapshot(&live_repo, "HEAD")?;
    if !ref_snapshot_eq(&current_head, &snapshot.head) {
        return Err(GitFinalizeError::ConcurrentMetadataChanged {
            reason: format!(
                "HEAD changed: snapshot={:?} current={:?}",
                snapshot.head, current_head
            ),
        });
    }

    if !head_ref.is_empty() && head_ref != "HEAD" {
        let current_branch = read_ref_snapshot(&live_repo, head_ref)?;
        let snapshot_branch = snapshot
            .refs
            .get(head_ref)
            .cloned()
            .unwrap_or(RefSnapshot::DidNotExist);
        if !ref_snapshot_eq(&current_branch, &snapshot_branch) {
            return Err(GitFinalizeError::ConcurrentMetadataChanged {
                reason: format!(
                    "branch {} changed: snapshot={:?} current={:?}",
                    head_ref, snapshot_branch, current_branch
                ),
            });
        }
    }

    let index_path = live_repo.path().join("index");
    let current_index = if index_path.exists() {
        let bytes = fs::read(&index_path)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        IndexSnapshot::Bytes(bytes)
    } else {
        IndexSnapshot::Missing
    };
    if !index_snapshot_eq(&current_index, &snapshot.index) {
        return Err(GitFinalizeError::ConcurrentMetadataChanged {
            reason: "index changed before finalize wrote anything".to_string(),
        });
    }

    for (ref_name, ref_snapshot) in &snapshot.refs {
        if !ref_name.starts_with("refs/remotes/") {
            continue;
        }
        let current = read_ref_snapshot(&live_repo, ref_name)?;
        if !ref_snapshot_eq(&current, ref_snapshot) {
            return Err(GitFinalizeError::ConcurrentMetadataChanged {
                reason: format!(
                    "remote ref {} changed: snapshot={:?} current={:?}",
                    ref_name, ref_snapshot, current
                ),
            });
        }
    }

    Ok(())
}

pub(crate) fn read_ref_snapshot(
    repo: &git2::Repository,
    ref_name: &str,
) -> std::result::Result<RefSnapshot, crate::Error> {
    match repo.find_reference(ref_name) {
        Ok(r) => Ok(super::prepare::snapshot_ref_from_repo_ref(&r)),
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(RefSnapshot::DidNotExist),
        Err(e) => Err(crate::Error::Io(std::io::Error::other(format!(
            "read_ref_snapshot: failed to read reference {ref_name}: {e}"
        )))),
    }
}

pub(crate) fn update_live_index(
    live_repo: &git2::Repository,
    staging_repo: &git2::Repository,
    new_oid: git2::Oid,
) -> Result<()> {
    let new_commit = staging_repo.find_commit(new_oid).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "update_live_index: find commit: {e}"
        )))
    })?;
    let new_tree = new_commit.tree().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "update_live_index: find tree: {e}"
        )))
    })?;

    let mut live_index = live_repo.index().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "update_live_index: get index: {e}"
        )))
    })?;
    live_index.read_tree(&new_tree).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "update_live_index: read_tree: {e}"
        )))
    })?;
    live_index.write().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "update_live_index: write: {e}"
        )))
    })?;

    let index_path = live_repo.path().join("index");
    if index_path.exists() {
        let index_file = std::fs::File::open(&index_path)?;
        index_file.sync_all()?;
        drop(index_file);
        crate::storage::sync_parent(&index_path)?;
    }

    Ok(())
}

pub(crate) fn execute_plan_refs_under_lock(
    ref_tx: &mut super::super::tx::RefTransaction<'_>,
    plan: &GitFinalizePlan,
    exclude_ref: Option<&str>,
) -> std::result::Result<(), GitFinalizeError> {
    for (ref_name, old_oid_str, new_oid_str) in &plan.ref_plans {
        if Some(ref_name.as_str()) == exclude_ref {
            continue;
        }

        let new_oid = git2::Oid::from_str(new_oid_str).map_err(|e| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
                "execute_plan_refs_under_lock: invalid new_oid for {}: {e}",
                ref_name
            ))))
        })?;

        let current = ref_tx.find_reference(ref_name);

        match (old_oid_str, current) {
            (None, Ok(current_ref)) => {
                if current_ref.target() == Some(new_oid) {
                } else {
                    return Err(GitFinalizeError::ConcurrentMetadataChanged {
                        reason: format!(
                            "execute_plan_refs_under_lock: ref {} has unexpected value \
                             (expected absent or new_oid {}) — concurrent modification",
                            ref_name, new_oid
                        ),
                    });
                }
            }
            (None, Err(e)) if e.code() == git2::ErrorCode::NotFound => {
                ref_tx.set_target(ref_name, new_oid, "sync: finalize plan ref (create)")?;
            }
            (None, Err(e)) => {
                return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                    std::io::Error::other(format!(
                        "execute_plan_refs_under_lock: lookup {}: {e}",
                        ref_name
                    )),
                )));
            }

            (Some(old_oid_str), Ok(current_ref)) => {
                let old_oid = git2::Oid::from_str(old_oid_str).map_err(|e| {
                    GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(
                        format!(
                            "execute_plan_refs_under_lock: invalid old_oid for {}: {e}",
                            ref_name
                        ),
                    )))
                })?;
                if current_ref.target() == Some(old_oid) {
                    ref_tx.set_target(ref_name, new_oid, "sync: finalize plan ref (update)")?;
                } else if current_ref.target() == Some(new_oid) {
                } else {
                    return Err(GitFinalizeError::ConcurrentMetadataChanged {
                        reason: format!(
                            "execute_plan_refs_under_lock: ref {} changed from {} to unexpected \
                             value (expected old_oid {} or new_oid {})",
                            ref_name,
                            current_ref
                                .target()
                                .map_or_else(|| "none".to_string(), |o| o.to_string()),
                            old_oid,
                            new_oid
                        ),
                    });
                }
            }
            (Some(_), Err(e)) if e.code() == git2::ErrorCode::NotFound => {
                return Err(GitFinalizeError::ConcurrentMetadataChanged {
                    reason: format!(
                        "execute_plan_refs_under_lock: ref {} not found (expected old or new)",
                        ref_name
                    ),
                });
            }
            (Some(_), Err(e)) => {
                return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                    std::io::Error::other(format!(
                        "execute_plan_refs_under_lock: lookup {}: {e}",
                        ref_name
                    )),
                )));
            }
        }
    }
    Ok(())
}

pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            crate::storage::durable_copy_file(&src_path, &dst_path)?;
        }
    }
    crate::storage::sync_dir(dst)?;
    Ok(())
}
