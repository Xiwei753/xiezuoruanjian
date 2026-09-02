use std::fs;
use std::path::Path;

use crate::error::Result;

use super::model::*;
use super::finalize::*;
use super::locks::*;

/// #644 评论 5488871385 问题1 + 评论 5489750244 问题4：纯只读检查 Git rollback 状态。
///
/// 不修改任何 Git/live 文件（不删 lock、不删 marker）。调用方根据返回的
/// `GitRollbackState` 决定：
/// - `NeedsRollback`：先 `preflight_backup_entries()`，再 `rollback_git_finalize()`
///   （stale lock / orphan marker 清理在 rollback_git_finalize 中），
///   再恢复文件 backup。
/// - `RepoInstallCommitted`：直接 Finished。
/// - `ConcurrentChanged`：保留事务。
#[allow(clippy::too_many_lines, clippy::excessive_nesting)]
pub fn inspect_git_rollback_state(
    live_root: &Path,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) -> Result<GitRollbackState> {
    crate::storage::git_runtime::ensure_initialized()?;

    // #644 评论 5491531984 问题4：使用 recovery record 中的 git_dir，
    // 不再硬编码 live_root.join(".git")。
    let effective_git_dir = explicit_git_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| live_root.join(".git"));

    // 1. repo_create=true 时判 ownership。
    if plan.repo_create {
        let live_git = effective_git_dir.clone();
        if !live_git.exists() {
            // repo install 没发生。需要 rollback（清理 tmp repo + 恢复文件）。
            return Ok(GitRollbackState::NeedsRollback);
        }
        let marker_path = live_git.join(".sujian-sync-owner");
        let marker_matches = match (&plan.repo_create_owner, marker_path.exists()) {
            (Some(expected), true) => match fs::read_to_string(&marker_path) {
                Ok(content) => content == *expected,
                Err(_) => false,
            },
            _ => false,
        };
        if !marker_matches {
            return Ok(GitRollbackState::ConcurrentChanged);
        }
        // marker 匹配 → rename 已发生 → RepoInstallCommitted。
        return Ok(GitRollbackState::RepoInstallCommitted);
    }

    // 2. repo_create=false：检查 index + refs 状态。
    //    只读检查，不修改任何文件。

    // 2a. index 状态。
    if let Some(expected_hash) = plan.new_index_sha256 {
        let index_path = effective_git_dir.join("index");
        let lock_path = effective_git_dir.join("index.lock");

        // 检查 stale lock。
        if let Some(owner) = &plan.index_lock_owner {
            match lock_dir_belongs_to_owner(&lock_path, owner) {
                LockOwner::Ours | LockOwner::IncompleteSujianLock => {
                    // 我们的 stale lock，需要 rollback（会清理 lock）。
                }
                LockOwner::External => {
                    return Ok(GitRollbackState::ConcurrentChanged);
                }
                LockOwner::Unknown => {
                    return Err(crate::Error::Io(std::io::Error::other(
                        "inspect_git_rollback_state: index.lock directory exists but \
                         owner file read failed (IO error)",
                    )));
                }
                LockOwner::Absent => {}
            }
        } else if lock_path.exists() {
            return Err(crate::Error::Io(std::io::Error::other(
                "inspect_git_rollback_state: plan.index_lock_owner is None but \
                 index.lock exists — cannot determine lock ownership",
            )));
        }

        // index 三态。
        let current_index = if index_path.exists() {
            let bytes = fs::read(&index_path)?;
            IndexSnapshot::Bytes(bytes)
        } else {
            IndexSnapshot::Missing
        };

        if index_snapshot_eq(&current_index, &snapshot.index) {
            // current == old → AlreadyReverted for index, check refs.
        } else {
            let current_is_new = match &current_index {
                IndexSnapshot::Bytes(b) => sha256_bytes(b) == expected_hash,
                IndexSnapshot::Missing => false,
            };
            if !current_is_new {
                return Err(crate::Error::Io(std::io::Error::other(
                    "inspect_git_rollback_state: index CAS miss (concurrent modification)",
                )));
            }
            // current == new → needs rollback.
            return Ok(GitRollbackState::NeedsRollback);
        }
    }

    // 2b. refs 状态（只读，不修改任何文件）。
    // #644 评论 5490206957 问题3：用 plan.ref_lock_names（完整的 forward lock 集合）
    // 而不是 plan.ref_plans（只包含 head_ref + remote refs，不含 HEAD）来检查 lock 状态。
    // 向后兼容：旧 manifest 无 ref_lock_names 时从 ref_plans 取名称。
    let ref_lock_check_names: Vec<String> = if !plan.ref_lock_names.is_empty() {
        plan.ref_lock_names.clone()
    } else {
        plan.ref_plans
            .iter()
            .map(|(name, _, _)| name.clone())
            .collect()
    };
    if !ref_lock_check_names.is_empty() {
        let git_dir_ref = effective_git_dir.clone();

        // 评论 5489750244 问题4：inspect_git_rollback_state 恢复成纯只读函数。
        // 不再执行 clean_stale_ref_lock / clean_orphan_owner_marker，只做分类。
        // stale lock / orphan marker 的清理移到 rollback_git_finalize 中，
        // 在 backup preflight 成功后才执行，保证 inspect → backup preflight → rollback
        // 的严格顺序不被破坏。
        if let Some(owner) = &plan.ref_tx_owner {
            use super::tx::{inspect_ref_lock_owner, RefLockOwner};
            for ref_name in &ref_lock_check_names {
                match inspect_ref_lock_owner(&git_dir_ref, ref_name, owner) {
                    RefLockOwner::Ours => {
                        // 本事务的 stale lock：classify 为 NeedsRollback，
                        // 真正的清理在 rollback_git_finalize 中。
                        return Ok(GitRollbackState::NeedsRollback);
                    }
                    RefLockOwner::Absent => {
                        // lock 不存在，可能有 orphan owner marker。只读 inspect 不清理，
                        // 但无 lock → ref 值不受阻塞，继续检查 ref 值。
                    }
                    RefLockOwner::OtherSujian => {
                        // 别的素笺事务的 lock：不碰，返回 ConcurrentChanged。
                        return Ok(GitRollbackState::ConcurrentChanged);
                    }
                    RefLockOwner::External => {
                        // 外部 Git 的 lock：不碰，返回 ConcurrentChanged。
                        return Ok(GitRollbackState::ConcurrentChanged);
                    }
                    RefLockOwner::Unknown => {
                        // owner marker 读取失败：不碰，返回 ConcurrentChanged。
                        return Ok(GitRollbackState::ConcurrentChanged);
                    }
                }
            }
        }

        // #644 评论 5492740265 问题3：用 open_live_repo 统一入口，
        // 外部 git_dir 布局下 Repository::open(live_root) 会失败。
        let repo = open_live_repo(live_root, explicit_git_dir)?;

        for (ref_name, old_oid_str, new_oid_str) in &plan.ref_plans {
            let new_oid = git2::Oid::from_str(new_oid_str).map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "inspect_git_rollback_state: invalid new_oid for {}: {e}",
                    ref_name
                )))
            })?;

            match (&old_oid_str, repo.find_reference(ref_name)) {
                (None, Ok(current_ref)) => {
                    if current_ref.target() == Some(new_oid) {
                        // current == new → needs rollback (Remove).
                        return Ok(GitRollbackState::NeedsRollback);
                    }
                    // 第三值。
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "inspect_git_rollback_state: ref {} has unexpected value \
                         (expected absent or new_oid {})",
                        ref_name, new_oid
                    ))));
                }
                (None, Err(e)) if e.code() == git2::ErrorCode::NotFound => {
                    // Already gone → no-op for this ref.
                }
                (None, Err(e)) => {
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "inspect_git_rollback_state: lookup {}: {e}",
                        ref_name
                    ))));
                }
                (Some(old_oid_str), Ok(current_ref)) => {
                    let old_oid = git2::Oid::from_str(old_oid_str).map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "inspect_git_rollback_state: invalid old_oid for {}: {e}",
                            ref_name
                        )))
                    })?;
                    if current_ref.target() == Some(old_oid) {
                        // current == old → no-op for this ref.
                    } else if current_ref.target() == Some(new_oid) {
                        // current == new → needs rollback (SetTarget).
                        return Ok(GitRollbackState::NeedsRollback);
                    } else {
                        return Err(crate::Error::Io(std::io::Error::other(format!(
                            "inspect_git_rollback_state: ref {} has unexpected value \
                             (expected old_oid {} or new_oid {})",
                            ref_name, old_oid, new_oid
                        ))));
                    }
                }
                (Some(_), Err(e)) if e.code() == git2::ErrorCode::NotFound => {
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "inspect_git_rollback_state: ref {} not found (expected old or new)",
                        ref_name
                    ))));
                }
                (Some(_), Err(e)) => {
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "inspect_git_rollback_state: lookup {}: {e}",
                        ref_name
                    ))));
                }
            }
        }
    }

    // 所有 index + refs 都已是 old 状态 → AlreadyReverted，但仍然返回 NeedsRollback
    // 让调用方走完整流程（rollback_git_finalize 内部会 no-op）。
    // 这样 preflight + file rollback 仍会执行。
    Ok(GitRollbackState::NeedsRollback)
}

/// #644 评论 5488100307 问题1：在 Transaction 锁保护下分类 ref 的回滚动作。
///
/// 在所有 ref 锁仍持有时验证当前值，决定对每个 ref 执行什么操作：
/// - `Noop`：当前值等于 old（AlreadyReverted），无需操作
/// - `SetTarget`：当前值等于 new，需要反向恢复到 old_oid
/// - `Remove`：当前值等于 new（old=None 表示新建），需要删除 ref
///
/// 第三值/NotFound 立即返回 Err，不进入后续 index/refs 修改。
enum LockedRollbackRefAction {
    Noop,
    SetTarget {
        ref_name: String,
        old_oid: git2::Oid,
    },
    Remove {
        ref_name: String,
    },
}

/// #644 评论 5488100307 问题1：在 Transaction 锁保护下验证所有 refs 并分类回滚动作。
///
/// 必须在所有 `lock_ref()` 成功后调用，锁仍持有时完成验证。
/// 验证必须发生在任何 index mutation 之前。
///
/// 规则沿用现有三态：
/// - `old=None`：只接受 `Absent` 或 `current==new`；
/// - `old=Some`：只接受 `current==old` 或 `current==new`；
/// - 第三值、`old=Some` 但 NotFound、其它 refdb 错误：立即 Err。
fn classify_locked_ref_rollback(
    repo: &git2::Repository,
    plan: &GitFinalizePlan,
) -> Result<Vec<LockedRollbackRefAction>> {
    let mut actions = Vec::new();

    for (ref_name, old_oid_str, new_oid_str) in &plan.ref_plans {
        let new_oid = git2::Oid::from_str(new_oid_str).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "classify_locked_ref_rollback: invalid new_oid for {}: {e}",
                ref_name
            )))
        })?;

        let current = repo.find_reference(ref_name);

        match (&old_oid_str, current) {
            // ref(old=None): finalize 新建了 ref。
            // - Absent → Noop（AlreadyReverted 或从未创建）
            // - current == new → Remove（反向删除）
            // - current == 其它 → 第三值，Err
            (None, Ok(current_ref)) => {
                if current_ref.target() == Some(new_oid) {
                    actions.push(LockedRollbackRefAction::Remove {
                        ref_name: ref_name.clone(),
                    });
                } else {
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "classify_locked_ref_rollback: ref {} has unexpected value {} \
                         (expected absent or new_oid {}) under Transaction lock — \
                         concurrent modification detected, preserving transaction",
                        ref_name,
                        current_ref
                            .target()
                            .map_or_else(|| "none".to_string(), |o| o.to_string()),
                        new_oid
                    ))));
                }
            }
            (None, Err(e)) if e.code() == git2::ErrorCode::NotFound => {
                // Already gone — nothing to undo.
                actions.push(LockedRollbackRefAction::Noop);
            }
            (None, Err(e)) => {
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "classify_locked_ref_rollback: lookup {}: {e}",
                    ref_name
                ))));
            }

            // ref(old=Some): finalize 更新了 ref old_oid -> new_oid。
            // - current == old → Noop（AlreadyReverted 或本轮 finalize 未执行）
            // - current == new → SetTarget(old_oid)（反向恢复）
            // - current == 其它 / NotFound → 第三值，Err
            (Some(old_oid_str), Ok(current_ref)) => {
                let old_oid = git2::Oid::from_str(old_oid_str).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "classify_locked_ref_rollback: invalid old_oid for {}: {e}",
                        ref_name
                    )))
                })?;
                if current_ref.target() == Some(old_oid) {
                    actions.push(LockedRollbackRefAction::Noop);
                } else if current_ref.target() == Some(new_oid) {
                    actions.push(LockedRollbackRefAction::SetTarget {
                        ref_name: ref_name.clone(),
                        old_oid,
                    });
                } else {
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "classify_locked_ref_rollback: ref {} has unexpected value \
                         (expected old_oid {} or new_oid {}) under Transaction lock — \
                         concurrent modification detected, preserving transaction",
                        ref_name, old_oid, new_oid
                    ))));
                }
            }
            (Some(old_oid_str), Err(e)) if e.code() == git2::ErrorCode::NotFound => {
                let old_oid = git2::Oid::from_str(old_oid_str).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "classify_locked_ref_rollback: invalid old_oid for {}: {e}",
                        ref_name
                    )))
                })?;
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "classify_locked_ref_rollback: ref {} not found under Transaction lock \
                     (expected old_oid {} or new_oid {}) — concurrent modification detected, \
                     preserving transaction",
                    ref_name, old_oid, new_oid
                ))));
            }
            (Some(_), Err(e)) => {
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "classify_locked_ref_rollback: lookup {}: {e}",
                    ref_name
                ))));
            }
        }
    }

    Ok(actions)
}

/// #644 评论 5480360027：CAS-based rollback，根据 plan 的 old_oid/new_oid 做反向 CAS。
///
/// 恢复时根据当前值判断某一步是否真的发生：
/// - current == new_oid → 这步已执行，反向 CAS 回 old_oid；
/// - current == old_oid → 这步没执行，无需 rollback；
/// - 两者都不是 → 有并发新状态，不能覆盖。
///
/// #644 评论 5480360027 修复点 4：更新型 ref rollback 用 `reference_matching`
/// 反向 CAS，不再用 `force=true` 的 `reference()`。
///
/// #644 评论 5482310913 问题2/3：
/// - 问题2：`plan.repo_create=true` 时，**第一件事**先判 live `.git` ownership，
///   marker 不匹配 → 返回 `ConcurrentChanged`，不碰 index/lock/refs。
/// - 问题3：返回 `Result<GitRollbackOutcome>`。index/ref CAS miss（真正并发新状态）
///   返回 `Err`，让上层 `rollback_full_sync_transaction` 保留 transaction 不回滚文件。
#[allow(clippy::too_many_lines, clippy::excessive_nesting)]
pub fn rollback_git_finalize(
    live_root: &Path,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) -> Result<GitRollbackOutcome> {
    crate::storage::git_runtime::ensure_initialized()?;

    // #644 评论 5491531984 问题4：使用 recovery record 中的 git_dir。
    let effective_git_dir = explicit_git_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| live_root.join(".git"));

    // 1. #644 评论 5482310913 问题2 + #644 评论 5487751293 问题1：
    //    repo_create=true 时先判 ownership，再碰 index/refs。
    //    外部仓库的 index/lock/refs 一字节都不能碰。
    if plan.repo_create {
        let live_git = effective_git_dir.clone();
        if !live_git.exists() {
            // 本轮 repo install 没发生（rename 前崩溃）。清理本轮对应的 tmp_git
            //（基于 repo_create_owner 命名，无需扫猜）。然后回滚文件。
            // #644 评论 5487751293 问题1：durable cleanup — remove_dir_all 失败时
            // 返回 Err 保留 transaction，不能 terminalize 后把没人再知道 owner 的
            // tmp repo 留在磁盘上。
            if let Some(owner) = &plan.repo_create_owner {
                // #644 评论 5492740265 问题2：tmp 建在 live_git 父目录，
                // 保证 tmp -> live_git 同一文件系统原子 rename。
                let tmp_git = repo_install_tmp_path(live_root, explicit_git_dir, owner)?;
                if tmp_git.exists() {
                    fs::remove_dir_all(&tmp_git)?;
                    crate::storage::sync_parent(&tmp_git)?;
                }
            }
            return Ok(GitRollbackOutcome::Reverted);
        }
        let marker_path = live_git.join(".sujian-sync-owner");
        let marker_matches = match (&plan.repo_create_owner, marker_path.exists()) {
            (Some(expected), true) => match fs::read_to_string(&marker_path) {
                Ok(content) => content == *expected,
                Err(_) => false,
            },
            _ => false,
        };
        if !marker_matches {
            // #644 评论 5482310913 问题2：marker 不匹配 → 外部创建的仓库。
            // 不碰 index/lock/refs，不继续回滚 live 文件，保留 transaction 给下次恢复。
            log::warn!(
                "rollback_git_finalize: repo_create=true but live .git owner marker \
                 missing or mismatched — treating as externally created, NOT touching \
                 index/lock/refs"
            );
            return Ok(GitRollbackOutcome::ConcurrentChanged);
        }
        // #644 评论 5487751293 问题1：marker 匹配说明 rename 已发生（.git 已是 live）。
        // owner marker 只能证明"这个 repo 最初是本轮装进去的"，不能证明 rename 以后
        // 没人改过它。真实场景：rename 成功 → 进程死在 tx.finish() 前 → 用户/别的
        // Git 进程做了一次 commit/建分支/改 config → 恢复看到 marker 仍匹配。
        // 此时不能 remove_dir_all(.git)，否则会把后来的 Git 操作一起删掉。
        // 直接返回 RepoInstallCommitted，让上层按 commit-point 逻辑收尾
        //（写 Finished，不恢复旧业务文件）。
        //
        // 清理 rename 前可能残留的 tmp repo（crash 在 rename 前的其它 owner 的 tmp）。
        if let Some(owner) = &plan.repo_create_owner {
            let tmp_git = live_root.join(format!(".git.sujian-tmp-{}", owner));
            if tmp_git.exists() {
                fs::remove_dir_all(&tmp_git)?;
                crate::storage::sync_parent(&tmp_git)?;
            }
        }
        return Ok(GitRollbackOutcome::RepoInstallCommitted);
    }

    // 2. repo_create=false：index + refs rollback。
    //    #644 评论 5483239422 问题2/3 + #644 评论 5484539222 缺陷1：
    //    三态幂等 + lockfile 反向提交边界 + 持久 ownership。
    //    #644 评论 5489192105 问题1+2+3：用 RefTransaction 统一 ref transaction：
    //    - 问题1：先 acquire 全部 refs 的 writer exclusion，锁内 classify，再改。
    //      消除 read→lock TOCTOU。
    //    - 问题2：owner marker 文件做持久 ownership，区分本事务 stale lock、
    //      别的素笺事务 lock、外部 Git regular lock。
    //    - 问题3：set/delete 通过 git2::Transaction（libgit2 refdb），
    //      不直接碰 loose ref 文件，正确处理 packed refs。
    //
    //    流程：
    //    A. acquire 全部 refs 的 lock（RefTransaction，先写 owner marker，再 lock_ref）。
    //    B. 锁内 classify（读取每个 ref 的当前值，与 plan 的 old/new 比较）。
    //       任一第三值 → 释放全部锁，返回 ConcurrentChanged（index 一字节不动）。
    //    C. index rollback（在 ref lock 保护下执行）。
    //    D. ref update（用 tx.set_target / tx.remove，通过 libgit2 refdb）。
    //    E. commit（释放全部 ref lock）。

    use super::tx::{
        clean_orphan_owner_marker, clean_stale_ref_lock, inspect_ref_lock_owner, RefLockOwner,
        RefTransaction,
    };

    // A. acquire 全部 refs 的 lock（如果 ref_lock_names 非空）。
    //    #644 评论 5490206957 问题3：使用 plan.ref_lock_names（完整的 forward lock 集合）
    //    而不是 plan.ref_plans（只包含 head_ref + remote refs，不含 HEAD）。
    //    #644 评论 5489192105 问题1：先拿齐 writer exclusion，再判断，再改。
    //    #644 评论 5489192105 问题2：用 owner marker 做持久 ownership。
    //    向后兼容：旧 manifest 无 ref_lock_names 时从 ref_plans 取名称。
    let ref_names: Vec<String> = if !plan.ref_lock_names.is_empty() {
        plan.ref_lock_names.clone()
    } else {
        plan.ref_plans
            .iter()
            .map(|(name, _, _)| name.clone())
            .collect()
    };

    // repo 需要定义在 ref_tx 之前，且生命周期要覆盖 ref_tx。
    let repo: Option<git2::Repository> = if !ref_names.is_empty() {
        // #644 评论 5492740265 问题3：用 open_live_repo 统一入口，
        // 外部 git_dir 布局下 Repository::open(live_root) 会失败。
        Some(open_live_repo(live_root, explicit_git_dir)?)
    } else {
        None
    };

    let git_dir = repo
        .as_ref()
        .map(|r| r.path().to_path_buf())
        .unwrap_or_else(|| effective_git_dir.clone());

    let mut ref_tx: Option<RefTransaction<'_>> = None;
    if let Some(repo) = &repo {
        let owner = plan.ref_tx_owner.as_deref().ok_or_else(|| {
            crate::Error::Io(std::io::Error::other(
                "rollback_git_finalize: plan.ref_tx_owner is None but ref_plans is non-empty \
                 — cannot acquire RefTransaction without owner (plan was generated by older \
                 code without persistent ref transaction ownership)",
            ))
        })?;

        // 先清理可能残留的 stale lock / orphan owner marker（本事务的）。
        // #644 评论 5489192105 问题2：崩溃恢复时区分 lock 归属。
        for ref_name in &ref_names {
            match inspect_ref_lock_owner(&git_dir, ref_name, owner) {
                RefLockOwner::Ours => {
                    // 本事务的 stale lock（SIGKILL 后残留）：清理后继续 acquire。
                    clean_stale_ref_lock(&git_dir, ref_name)?;
                }
                RefLockOwner::Absent => {
                    // lock 不存在，清理可能残留的 orphan owner marker。
                    clean_orphan_owner_marker(&git_dir, ref_name)?;
                }
                RefLockOwner::OtherSujian => {
                    // 别的素笺事务的 lock：不碰，返回 ConcurrentChanged。
                    log::warn!(
                        "rollback_git_finalize: ref {} lock belongs to another Sujian \
                         transaction — preserving transaction for next recovery",
                        ref_name
                    );
                    return Ok(GitRollbackOutcome::ConcurrentChanged);
                }
                RefLockOwner::External => {
                    // 外部 Git 的 lock：不碰，返回 ConcurrentChanged。
                    log::warn!(
                        "rollback_git_finalize: ref {} lock is external Git regular lock \
                         — preserving transaction for next recovery",
                        ref_name
                    );
                    return Ok(GitRollbackOutcome::ConcurrentChanged);
                }
                RefLockOwner::Unknown => {
                    // owner marker 读取失败（EIO 等）：不碰，返回 Err 保留事务。
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "rollback_git_finalize: ref {} lock owner marker read failed \
                         (IO error) — cannot determine lock ownership, refusing to delete, \
                         preserving transaction for next recovery",
                        ref_name
                    ))));
                }
            }
        }

        ref_tx = Some(RefTransaction::acquire_all_refs(repo, &ref_names, owner)?);
    }

    // B. 锁内 classify（在 ref lock 保护下读取并分类）。
    //    #644 评论 5489192105 问题1：classify 在锁保护下完成，消除 read→lock TOCTOU。
    let ref_actions: Vec<LockedRollbackRefAction> = if let Some(tx) = &ref_tx {
        classify_locked_ref_rollback(tx.repo(), plan)?
    } else {
        Vec::new()
    };

    // C. index rollback（在 ref lock 保护下执行）。
    //    #644 评论 5489192105 问题1：index rollback 在 ref lock 保护下执行，
    //    消除 "verify → index write → ref update" 之间的并发窗口。
    if let Some(expected_hash) = plan.new_index_sha256 {
        let index_path = git_dir.join("index");
        let lock_path = git_dir.join("index.lock");

        // 读取当前 index 字节（不存在视为 Missing）。
        let current_index = if index_path.exists() {
            let bytes = fs::read(&index_path)?;
            IndexSnapshot::Bytes(bytes)
        } else {
            IndexSnapshot::Missing
        };

        // 三态判断：先判 old（AlreadyReverted），再判 new（需反向恢复），其它并发失败。
        if index_snapshot_eq(&current_index, &snapshot.index) {
            // current == old(snapshot.index) → 第一次 rollback 已恢复，no-op。
            // #644 评论 5486167472 问题1 + #644 评论 5486852142 问题1+问题2：
            // 目录锁模型下检测并清理本轮的 stale lock。
            // forward install 中途被 SIGKILL（acquire 后、commit_rename 前）会残留
            // .git/index.lock 目录（含 owner + prepared 文件）。
            // - lock_path 是目录且归属本轮（owner 匹配或 owner 文件不存在/为空）
            //   → 清理 lock 目录，继续 no-op。
            // - lock_path 是 regular file（外部 Git 进程的 lock）
            //   → #644 评论 5486852142 问题2：返回 ConcurrentChanged，保留 transaction。
            // - lock_path 是目录但 owner 不匹配（不同事务的 lock）
            //   → #644 评论 5486852142 问题2：返回 ConcurrentChanged，保留 transaction。
            // #644 评论 5485518160 修改点 2：迁移入口已保证进入 rollback_git_finalize 时
            // 若 new_index_sha256.is_some() 则 owner 必为 Some（除非 current==old 且无
            // lock 的安全 no-op 路径）。owner=None 且有 stale lock → 未知归属，返回 Err
            // 保留事务（不能留永久 lock）。
            if let Some(owner) = &plan.index_lock_owner {
                match lock_dir_belongs_to_owner(&lock_path, owner) {
                    LockOwner::Ours | LockOwner::IncompleteSujianLock => {
                        // #644 评论 5488100307 问题2：lock 目录属于本轮（owner 匹配或
                        // owner 未完成写入）：清理 lock 目录，fsync .git。
                        // remove_dir_all 或 sync_dir 失败时返回 Err 保留 transaction，
                        // 不能吞错后返回 Reverted（磁盘可能留着没人负责的 index.lock）。
                        remove_lock_dir_if_exists(&lock_path, &git_dir)?;
                    }
                    LockOwner::External => {
                        // #644 评论 5486852142 问题2：lock 不属于本轮
                        //（regular file 外部 Git lock 或目录锁但 owner 不匹配）。
                        // 返回 ConcurrentChanged，保留 transaction 不继续 rollback refs。
                        return Ok(GitRollbackOutcome::ConcurrentChanged);
                    }
                    LockOwner::Unknown => {
                        // #644 评论 5487751293 问题4：owner 文件读取失败（EIO 等）。
                        // 不能降级成"owner 为空 = ours"，返回 Err 保留事务。
                        return Err(crate::Error::Io(std::io::Error::other(
                            "rollback_git_finalize: index.lock directory exists but \
                             owner file read failed (IO error) — cannot determine lock \
                             ownership, refusing to delete, preserving transaction for \
                             next recovery",
                        )));
                    }
                    LockOwner::Absent => {
                        // lock 不存在：no-op，无需清理。
                    }
                }
            } else if lock_path.exists() {
                // #644 评论 5485518160 修改点 2：owner=None 但 stale lock 存在。
                // 迁移入口应已处理 owner=None 情况，到达这里说明 lock 是未知的
                //（可能外部 Git 进程），不能留永久 lock 也不能删别人的 lock，
                // 返回 Err 保留事务给下次恢复。
                return Err(crate::Error::Io(std::io::Error::other(
                    "rollback_git_finalize: current == snapshot.index but \
                     plan.index_lock_owner is None and stale index.lock exists — \
                     cannot determine lock ownership, refusing to delete external \
                     lock or leave permanent lock, preserving transaction for next \
                     recovery",
                )));
            }
        } else {
            // current != old，检查是否 == new（plan.new_index_sha256）。
            let current_is_new = match &current_index {
                IndexSnapshot::Bytes(b) => sha256_bytes(b) == expected_hash,
                IndexSnapshot::Missing => false,
            };
            if !current_is_new {
                // current 既不是 old 也不是 new → 真正并发修改。
                return Err(crate::Error::Io(std::io::Error::other(
                    "rollback_git_finalize: index CAS miss (concurrent modification, \
                     current matches neither snapshot.index nor plan.new_index_sha256) — \
                     refusing to continue rollback to preserve transaction for next recovery",
                )));
            }
            // #644 评论 5489192105 问题1：refs 已通过 RefTransaction 锁住
            //（writer exclusion），在锁保护下不可能被并发修改。
            // index rollback 在 ref lock 保护下执行，消除了
            // "verify → index write → ref update" 之间的并发窗口。
            //
            // current == new(plan) → 需要反向恢复到 snapshot.index。
            // 走 OwnedIndexLock 反向提交边界（与 forward install_index_with_lock 同语义）：
            // 1. acquire（create_dir + owner metadata）自己获取 .git/index.lock 目录；
            //    lock 已存在且不属于本轮 → Err，绝不删。
            // 2. 拿到锁后重新读 index 确认仍 == new。
            // 3. Bytes 路径 commit_rename（rename prepared_file → index）；Missing 路径 commit_delete。
            // #644 评论 5486167472 问题1：用 plan.index_lock_owner 做持久 ownership。
            let owner = plan.index_lock_owner.as_deref().ok_or_else(|| {
                crate::Error::Io(std::io::Error::other(
                    "rollback_git_finalize: plan.index_lock_owner is None but \
                     new_index_sha256 is Some — cannot acquire OwnedIndexLock without owner \
                     (plan was generated by older code without persistent index lock ownership)",
                ))
            })?;
            rollback_index_via_lockfile(
                &index_path,
                &lock_path,
                expected_hash,
                &snapshot.index,
                owner,
            )?;
        }
    }

    // D. ref update（用 tx.set_target / tx.remove，通过 libgit2 refdb）。
    //    #644 评论 5489192105 问题3：set/delete 通过 git2::Transaction，
    //    不直接碰 loose ref 文件，正确处理 packed refs。
    //    #644 评论 5489192105 问题2：forward 和 rollback 共用同一套 ref transaction。
    if let Some(mut tx) = ref_tx {
        for action in &ref_actions {
            match action {
                LockedRollbackRefAction::Noop => {
                    // current == old → AlreadyReverted，无需操作。
                }
                LockedRollbackRefAction::SetTarget { ref_name, old_oid } => {
                    // current == new → 反向恢复到 old_oid。
                    // 通过 git2::Transaction::set_target（libgit2 refdb），
                    // 正确处理 loose ref 和 packed refs。
                    tx.set_target(
                        ref_name,
                        *old_oid,
                        "sync: rollback ref after finalize failure",
                    )?;
                }
                LockedRollbackRefAction::Remove { ref_name } => {
                    // current == new → 删除 ref（反向恢复）。
                    // 通过 git2::Transaction::remove（libgit2 refdb），
                    // 正确处理 loose ref 和 packed refs。
                    tx.remove(ref_name)?;
                }
            }
        }
        // E. commit（提交所有 set/remove 操作，释放全部 ref lock）。
        tx.commit()?;
    }

    Ok(GitRollbackOutcome::Reverted)
}

/// #644 评论 5485518160 修改点 2：检查 `index_lock_owner=None` 旧 manifest 的迁移判定。
///
/// 读 live `.git/index`、`.git/index.lock`，根据磁盘状态返回迁移决策。
/// 调用方（`rollback_full_sync_transaction`）负责持久化新 owner 到 manifest。
pub fn check_index_lock_owner_migration(
    live_root: &Path,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) -> Result<IndexLockOwnerMigration> {
    // #644 评论 5491531984 问题4：使用 recovery record 中的 git_dir。
    let effective_git_dir = explicit_git_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| live_root.join(".git"));
    let lock_path = effective_git_dir.join("index.lock");
    let index_path = effective_git_dir.join("index");

    // canonical index.lock 已存在：不知道是谁的，不能 terminalize。
    if lock_path.exists() {
        return Ok(IndexLockOwnerMigration::LockExists);
    }

    // 没有 index.lock，读 current index。
    let current_index = if index_path.exists() {
        let bytes = fs::read(&index_path)?;
        IndexSnapshot::Bytes(bytes)
    } else {
        IndexSnapshot::Missing
    };

    // 先判 current == old（snapshot.index）。
    if index_snapshot_eq(&current_index, &snapshot.index) {
        return Ok(IndexLockOwnerMigration::AlreadyReverted);
    }

    // 再判 current == new（plan.new_index_sha256）。
    if let Some(expected_hash) = plan.new_index_sha256 {
        let current_is_new = match &current_index {
            IndexSnapshot::Bytes(b) => sha256_bytes(b) == expected_hash,
            IndexSnapshot::Missing => false,
        };
        if current_is_new {
            // 生成新的 owner UUID，调用方负责持久化到 manifest。
            return Ok(IndexLockOwnerMigration::MigrateToNewOwner(
                uuid::Uuid::new_v4().to_string(),
            ));
        }
    }

    // current 既不是 old 也不是 new → 并发修改。
    Ok(IndexLockOwnerMigration::ConcurrentModification)
}

/// 评论 5489750244 问题5：检查 `ref_tx_owner=None` 旧 manifest 的迁移判定。
///
/// 读 live `.git` 下每个 ref_plan 的 lock file 状态和 ref 值，返回迁移决策。
/// 调用方（`rollback_full_sync_transaction`）负责持久化新 owner 到 manifest。
pub fn check_ref_tx_owner_migration(
    live_root: &Path,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) -> Result<RefTxOwnerMigration> {
    let _ = snapshot;
    // #644 评论 5491531984 问题4：使用 recovery record 中的 git_dir。
    let effective_git_dir = explicit_git_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| live_root.join(".git"));

    crate::storage::git_runtime::ensure_initialized()?;

    // #644 评论 5492740265 问题3：用 open_live_repo 统一入口，
    // 外部 git_dir 布局下 Repository::open(live_root) 会失败。
    let repo = open_live_repo(live_root, explicit_git_dir)?;

    // #644 评论 5490206957 问题3：用 plan.ref_lock_names（完整的 forward lock 集合）
    // 而不是 plan.ref_plans（只包含 head_ref + remote refs，不含 HEAD）。
    // 向后兼容：旧 manifest 无 ref_lock_names 时从 ref_plans 取名称。
    let ref_lock_check_names: Vec<String> = if !plan.ref_lock_names.is_empty() {
        plan.ref_lock_names.clone()
    } else {
        plan.ref_plans
            .iter()
            .map(|(name, _, _)| name.clone())
            .collect()
    };
    for ref_name in &ref_lock_check_names {
        // 1. 检查 canonical ref lock 是否存在。
        let lock_path = effective_git_dir.join(format!("{}.lock", ref_name));
        if lock_path.exists() {
            return Ok(RefTxOwnerMigration::LockExists);
        }
    }

    // 2. 检查每个 ref_plan 的 ref 值是否在三态允许范围内。
    for (ref_name, old_oid_str, new_oid_str) in &plan.ref_plans {
        let new_oid = git2::Oid::from_str(new_oid_str).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "check_ref_tx_owner_migration: invalid new_oid for {}: {e}",
                ref_name
            )))
        })?;

        let current_ref_result = repo.find_reference(ref_name);

        match (old_oid_str, current_ref_result) {
            (None, Ok(current_ref)) => {
                if current_ref.target() != Some(new_oid) {
                    return Ok(RefTxOwnerMigration::ConcurrentModification);
                }
                // current == new → allowed, will need rollback.
            }
            (None, Err(e)) if e.code() == git2::ErrorCode::NotFound => {
                // Absent → allowed, no-op for this ref.
            }
            (None, Err(e)) => {
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "check_ref_tx_owner_migration: lookup {}: {e}",
                    ref_name
                ))));
            }
            (Some(old_oid_str), Ok(current_ref)) => {
                let old_oid = git2::Oid::from_str(old_oid_str).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "check_ref_tx_owner_migration: invalid old_oid for {}: {e}",
                        ref_name
                    )))
                })?;
                if current_ref.target() != Some(old_oid) && current_ref.target() != Some(new_oid) {
                    return Ok(RefTxOwnerMigration::ConcurrentModification);
                }
                // current == old (no-op) or current == new (rollback) → allowed.
            }
            (Some(_old_oid_str), Err(e)) if e.code() == git2::ErrorCode::NotFound => {
                // ref 不存在但 old_oid 是 Some → unexpected, concurrent modification.
                return Ok(RefTxOwnerMigration::ConcurrentModification);
            }
            (Some(_), Err(e)) => {
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "check_ref_tx_owner_migration: lookup {}: {e}",
                    ref_name
                ))));
            }
        }
    }

    // 所有 ref 都在三态允许范围内，无 canonical lock → 可迁移。
    Ok(RefTxOwnerMigration::MigrateToNewOwner(
        uuid::Uuid::new_v4().to_string(),
    ))
}

/// #644 评论 5483239422 问题3 + #644 评论 5483920624 问题2/3 +
/// #644 评论 5486167472 问题1 + #644 评论 5486852142 问题1：rollback index 的 lockfile 反向提交边界 + 持久 ownership。
///
/// 与 forward `install_index_with_lock` 同语义（共用 `OwnedIndexLock`），但方向相反：
/// 把 snapshot.index 的旧字节写回 live index。关键安全约束：
/// 1. 用 `OwnedIndexLock::acquire` 把 `.git/index.lock` 创建为**目录**，
///    目录内 `owner` 文件写 owner metadata，`prepared` 文件写 snapshot.index 旧字节。
///    lock 已存在（目录或 regular file）→ `ConcurrentMetadataChanged`，绝不删别人的 lock
///    （外部 Git 进程可能正在用）。
/// 2. 拿到锁后重新读 index 确认仍等于 `expected_new_sha256`（调用方已确认
///    current==new，但拿锁期间可能被并发改掉，需复验）。不等则返回 Err
///    （并发变化，保留 transaction；lock 由 OwnedIndexLock::drop 清理）。
/// 3. Bytes 路径：`commit_rename`（rename prepared_file → index，然后删 lock 目录，原子恢复 + 释放锁）。
///    Missing 路径：`commit_delete`（删 index → 删 prepared_file → 删 lock 目录，每步 fsync 父目录）。
/// 4. ownership 是磁盘事实（lock 目录存在性 + owner 文件内容），即使 SIGKILL
///    后 Drop 不执行，恢复时也能通过目录类型 + owner 文件判断 lock 归属。
///    `create_dir` 成功就是原子的 ownership 证明，不再有 create-to-write 窗口。
///
/// 调用方已保证进入此函数前 `current_index == new`（plan.new_index_sha256 命中）。
fn rollback_index_via_lockfile(
    index_path: &Path,
    lock_path: &Path,
    expected_new_sha256: [u8; 32],
    snapshot_index: &IndexSnapshot,
    owner: &str,
) -> Result<()> {
    // 1. OwnedIndexLock::acquire：O_EXCL 创建 lock + 写 owner metadata + 写 prepared file
    //   （snapshot.index 旧字节）。lock 已存在 → ConcurrentMetadataChanged（绝不删别人的 lock）。
    //    #644 评论 5485518160 修改点 1：acquire 返回 AcquireOutcome。
    //    - NewlyAcquired：拿到新锁，继续 CAS 复验 + commit。
    //    - AlreadyCommitted：上次反向 commit_rename/commit_delete 已完成
    //      （index 已恢复到 snapshot.index），跳过 commit，直接返回 Ok。
    let git_dir = index_path.parent().ok_or_else(|| {
        crate::Error::Io(std::io::Error::other(
            "rollback_git_finalize: index_path has no parent (.git dir)",
        ))
    })?;
    let prepared_bytes: &[u8] = match snapshot_index {
        IndexSnapshot::Bytes(b) => b,
        IndexSnapshot::Missing => &[],
    };
    let mut lock =
        match OwnedIndexLock::acquire(git_dir, owner, prepared_bytes).map_err(|e| match e {
            GitFinalizeError::ConcurrentMetadataChanged { reason } => {
                crate::Error::Io(std::io::Error::other(format!(
                    "rollback_git_finalize: index.lock exists — refusing to delete external \
                     git process lock; concurrent git operation in progress ({}), preserving \
                     transaction for next recovery",
                    reason
                )))
            }
            GitFinalizeError::FinalizeFailed(inner) => inner,
            GitFinalizeError::RollbackFailed { .. } => crate::Error::Io(std::io::Error::other(
                "rollback_git_finalize: unexpected RollbackFailed from OwnedIndexLock::acquire",
            )),
        })? {
            AcquireOutcome::NewlyAcquired(lock) => lock,
            AcquireOutcome::AlreadyCommitted => {
                // 上次反向 commit 已完成（index 已恢复到 snapshot.index），无需再 commit。
                return Ok(());
            }
        };
    // lock_path 仅用于诊断/一致性检查，确认 OwnedIndexLock 用的 lock 路径与调用方一致。
    debug_assert_eq!(lock_path, &git_dir.join("index.lock"));

    // 2. 拿到锁后重新读 live index，计算 SHA-256，确认仍等于 expected_new_sha256。
    //    若被并发改掉（current != expected_new），返回 Err
    //    （lock 由 OwnedIndexLock::drop 清理：disarmed=false → 清理 lock + owner_file）。
    let current_index = if index_path.exists() {
        let bytes = fs::read(index_path)?;
        IndexSnapshot::Bytes(bytes)
    } else {
        IndexSnapshot::Missing
    };
    let current_is_new = match &current_index {
        IndexSnapshot::Bytes(b) => sha256_bytes(b) == expected_new_sha256,
        IndexSnapshot::Missing => false,
    };
    if !current_is_new {
        return Err(crate::Error::Io(std::io::Error::other(
            "rollback_git_finalize: index hash changed after acquiring lock (concurrent \
             modification) — cleaned our lock, preserving transaction for next recovery",
        )));
    }

    // 3. 提交：Bytes 路径 commit_rename，Missing 路径 commit_delete。
    match snapshot_index {
        IndexSnapshot::Bytes(_) => {
            lock.commit_rename(index_path).map_err(|e| match e {
                GitFinalizeError::FinalizeFailed(inner) => inner,
                GitFinalizeError::ConcurrentMetadataChanged { reason } => {
                    crate::Error::Io(std::io::Error::other(reason))
                }
                GitFinalizeError::RollbackFailed { .. } => crate::Error::Io(std::io::Error::other(
                    "rollback_git_finalize: unexpected RollbackFailed from commit_rename",
                )),
            })?;
        }
        IndexSnapshot::Missing => {
            // #644 评论 5483920624 问题3：Missing 路径在持有自己 lock 期间完成：
            // 重新验证 current==expected new（上方已完成）→ 删 index → 删 lock → fsync 父目录。
            lock.commit_delete(index_path).map_err(|e| match e {
                GitFinalizeError::FinalizeFailed(inner) => inner,
                GitFinalizeError::ConcurrentMetadataChanged { reason } => {
                    crate::Error::Io(std::io::Error::other(reason))
                }
                GitFinalizeError::RollbackFailed { .. } => crate::Error::Io(std::io::Error::other(
                    "rollback_git_finalize: unexpected RollbackFailed from commit_delete",
                )),
            })?;
        }
    }

    Ok(())
}
