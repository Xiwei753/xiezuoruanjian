use std::fs;
use std::path::Path;

use super::model::*;
use crate::error::Result;

/// #644 评论 5488655439 问题2 + #644 评论 5488871385 问题1：
/// 在真正进入 rollback 路径之前，先检查所有 BackupEntry::RestoreFile 是否可读。
///
/// 必须在任何 Git/index/live 文件修改之前调用。
/// 一个不满足就不能碰 Git/index/live。
///
/// #644 评论 5488871385 问题1：用 `File::open()` 确认真正可读，不用 `metadata()` 冒充。
///
/// NotGitRepo 的 `RepoInstallCommitted` 是 commit-point，不需要 backup，
/// 所以不在本函数中做粗暴的无条件检查——由调用方根据 outcome 决定是否需要 preflight。
#[cfg(test)]
fn preflight_backup_entries(tx_dir: &Path, manifest: &TransactionManifest) -> Result<()> {
    let backup_dir = tx_dir.join("backup");
    for entry in &manifest.backup_entries {
        if let BackupEntry::RestoreFile {
            target_relative,
            backup_filename,
        } = entry
        {
            let backup_path = backup_dir.join(backup_filename);
            // 用 File::open() 确认真正可读，不用 metadata() 冒充。
            match std::fs::File::open(&backup_path) {
                Ok(file) => {
                    drop(file);
                }
                Err(e) => {
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "preflight_backup_entries: backup file not readable \
                         for RestoreFile {}: {}: {}",
                        target_relative,
                        backup_path.display(),
                        e
                    ))));
                }
            }
        }
    }
    Ok(())
}

/// #644 评论 5476546134 第3节：统一回滚 full-sync 事务入口。
///
/// 满足：
/// - Git rollback 成功 AND 文件 rollback 成功 → phase=RolledBack → cleanup
/// - 任意一步失败 → 返回 Err → 保留 manifest + backup + transaction 目录
///
/// 不吞错，给下次恢复留机会。
#[cfg(test)]
#[allow(clippy::too_many_lines, clippy::excessive_nesting)]
fn rollback_full_sync_transaction(
    tx_dir: &Path,
    target_root: &Path,
    manifest: &TransactionManifest,
) -> Result<()> {
    // #644 评论 5491531984 问题4：从 recovery record 解析 layout。
    // 旧 manifest 无 git_dir/worktree_root 时，使用 legacy target_root。
    let recovery_live_root = manifest
        .git_finalize
        .as_ref()
        .and_then(|g| g.worktree_root.as_deref())
        .unwrap_or(target_root);

    // 1. 回滚 Git metadata（如果有 recovery record）。
    //    #644 评论 5488871385 问题1：inspect-first 流程——
    //    先只读检查状态，再 preflight backup，最后才修改 Git/live。
    if let Some(ref git_rec) = manifest.git_finalize {
        let seed_state = git_rec.seed_state.to_seed_state().map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "rollback_full_sync_transaction: invalid seed state: {}",
                e
            )))
        })?;

        // #644 评论 5491531984 问题4：从 recovery record 解析 git_dir。
        let recovery_git_dir = git_rec.git_dir.as_deref();

        // #644 评论 5490206957 问题4：index_lock_owner + ref_tx_owner 迁移必须能在
        // 同一次恢复里顺序叠加，而不是二选一。用一个可变 effective plan 替代互斥的
        // Option<plan>。
        let mut effective_plan = git_rec.plan.clone();
        let mut plan_changed = false;

        // #644 评论 5490799656 问题3：ref_lock_names schema 迁移。
        // 旧 manifest 无 ref_lock_names 时（serde(default) 为空），从 seed_state
        // + ref_plans 重建完整的 forward lock 集合。必须在 owner migration 之前，
        // 因为 check_ref_tx_owner_migration 使用 plan.ref_lock_names 确定检查哪些 ref
        // 的 lock 状态。如果 ref_lock_names 为空，HEAD.lock 不会被检查、不会被清理。
        if effective_plan.ref_lock_names.is_empty() && !effective_plan.ref_plans.is_empty() {
            let mut rebuilt: Vec<String> = Vec::new();
            match &seed_state {
                crate::storage::workspace_git::seed::GitSeedState::NotGitRepo => {
                    // NotGitRepo：forward 不锁 live refs（live 还没有 .git），
                    // ref_lock_names 只包含 ref_plans 中的名称。
                }
                crate::storage::workspace_git::seed::GitSeedState::Unborn { head_ref }
                | crate::storage::workspace_git::seed::GitSeedState::Existing { head_ref, .. } => {
                    rebuilt.push("HEAD".to_string());
                    rebuilt.push(head_ref.clone());
                }
                crate::storage::workspace_git::seed::GitSeedState::Detached { .. } => {
                    rebuilt.push("HEAD".to_string());
                }
            }
            for (name, _, _) in &effective_plan.ref_plans {
                if !rebuilt.contains(name) {
                    rebuilt.push(name.clone());
                }
            }
            rebuilt.sort();
            rebuilt.dedup();
            effective_plan.ref_lock_names = rebuilt;
            plan_changed = true;
        }

        // index_lock_owner=None 旧 manifest 迁移。
        if effective_plan.new_index_sha256.is_some() && effective_plan.index_lock_owner.is_none() {
            let migration = crate::storage::workspace_git::check_index_lock_owner_migration(
                recovery_live_root,
                &git_rec.metadata_snapshot,
                &effective_plan,
                recovery_git_dir,
            )?;
            match migration {
                crate::storage::workspace_git::IndexLockOwnerMigration::LockExists => {
                    return Err(crate::Error::Io(std::io::Error::other(
                        "rollback_full_sync_transaction: index.lock exists but \
                         plan.index_lock_owner is None — cannot determine lock \
                         ownership, preserving transaction for next recovery",
                    )));
                }
                crate::storage::workspace_git::IndexLockOwnerMigration::AlreadyReverted => {}
                crate::storage::workspace_git::IndexLockOwnerMigration::MigrateToNewOwner(owner) => {
                    effective_plan.index_lock_owner = Some(owner);
                    plan_changed = true;
                }
                crate::storage::workspace_git::IndexLockOwnerMigration::ConcurrentModification => {
                    return Err(crate::Error::Io(std::io::Error::other(
                        "rollback_full_sync_transaction: index CAS miss during \
                         index_lock_owner migration — concurrent modification, \
                         preserving transaction for next recovery",
                    )));
                }
            }
        }

        // 评论 5489750244 问题5：ref_tx_owner=None 旧 manifest 迁移。
        // 与 index 迁移顺序叠加，不是互斥。
        if !effective_plan.ref_plans.is_empty() && effective_plan.ref_tx_owner.is_none() {
            let ref_migration = crate::storage::workspace_git::check_ref_tx_owner_migration(
                recovery_live_root,
                &git_rec.metadata_snapshot,
                &effective_plan,
                recovery_git_dir,
            )?;
            match ref_migration {
                crate::storage::workspace_git::RefTxOwnerMigration::LockExists => {
                    return Err(crate::Error::Io(std::io::Error::other(
                        "rollback_full_sync_transaction: ref lock exists but \
                         plan.ref_tx_owner is None — cannot determine lock \
                         ownership, preserving transaction for next recovery",
                    )));
                }
                crate::storage::workspace_git::RefTxOwnerMigration::ConcurrentModification => {
                    return Err(crate::Error::Io(std::io::Error::other(
                        "rollback_full_sync_transaction: ref CAS miss during \
                         ref_tx_owner migration — concurrent modification, \
                         preserving transaction for next recovery",
                    )));
                }
                crate::storage::workspace_git::RefTxOwnerMigration::MigrateToNewOwner(owner) => {
                    effective_plan.ref_tx_owner = Some(owner);
                    plan_changed = true;
                }
            }
        }

        // 一次原子写回完整 effective_plan（index + ref 可能在同一次恢复里都迁移了）。
        if plan_changed {
            let mut new_manifest = manifest.clone();
            if let Some(git_finalize) = new_manifest.git_finalize.as_mut() {
                git_finalize.plan = effective_plan.clone();
            }
            let manifest_path = tx_dir.join(MANIFEST_FILENAME);
            let json = serde_json::to_string_pretty(&new_manifest)?;
            crate::storage::atomic_write_string(&manifest_path, &json)?;
        }

        let plan_for_rollback: &crate::storage::workspace_git::GitFinalizePlan = &effective_plan;

        // #644 评论 5488871385 问题1：先只读 inspect，再 preflight，最后 rollback。
        let inspect_state = crate::storage::workspace_git::inspect_git_rollback_state(
            recovery_live_root,
            &git_rec.metadata_snapshot,
            plan_for_rollback,
            recovery_git_dir,
        )?;

        match inspect_state {
            crate::storage::workspace_git::GitRollbackState::NeedsRollback => {
                // 先 preflight backup（在任何 Git 修改之前）。
                preflight_backup_entries(tx_dir, manifest)?;
                // 再执行 Git rollback。
                let outcome = crate::storage::workspace_git::rollback_git_finalize(
                    recovery_live_root,
                    &git_rec.metadata_snapshot,
                    plan_for_rollback,
                    recovery_git_dir,
                )?;
                match outcome {
                    crate::storage::workspace_git::GitRollbackOutcome::Reverted => {
                        // Git metadata 已回滚，继续恢复文件 backup。
                    }
                    crate::storage::workspace_git::GitRollbackOutcome::ConcurrentChanged => {
                        return Err(crate::Error::Io(std::io::Error::other(
                            "rollback_full_sync_transaction: git rollback detected \
                             concurrent change, preserving transaction for next recovery",
                        )));
                    }
                    crate::storage::workspace_git::GitRollbackOutcome::RepoInstallCommitted => {
                        // inspect 已判 NeedsRollback 但 rollback 返回 RepoInstallCommitted
                        // （状态不一致），保留事务。
                        return Err(crate::Error::Io(std::io::Error::other(
                            "rollback_full_sync_transaction: inspect said NeedsRollback \
                             but rollback returned RepoInstallCommitted — state inconsistent, \
                             preserving transaction for next recovery",
                        )));
                    }
                }
            }
            crate::storage::workspace_git::GitRollbackState::RepoInstallCommitted => {
                // NotGitRepo 已完成 owner-matched .git rename。
                // 按 commit-point 逻辑收尾：不回滚文件，标记 Finished 并清理。
                let manifest_path = tx_dir.join(MANIFEST_FILENAME);
                write_manifest_phase_static(&manifest_path, TransactionPhase::Finished, manifest)?;
                fs::remove_dir_all(tx_dir)?;
                return Ok(());
            }
            crate::storage::workspace_git::GitRollbackState::ConcurrentChanged => {
                return Err(crate::Error::Io(std::io::Error::other(
                    "rollback_full_sync_transaction: inspect detected concurrent change \
                     (ownership mismatch or external repo), preserving transaction for next recovery",
                )));
            }
        }
    }

    // 2. 回滚 live 文件（用 backup_entries）。
    //    #644 评论 5491531984 问题4：使用 recovery_live_root（可能与 target_root 不同）。
    let backup_dir = tx_dir.join("backup");
    for entry in &manifest.backup_entries {
        match entry {
            BackupEntry::RestoreFile {
                target_relative,
                backup_filename,
            } => {
                let target_path = recovery_live_root.join(target_relative);
                let backup_path = backup_dir.join(backup_filename);
                if !backup_path.exists() {
                    // manifest 要求 RestoreFile 但 backup 文件缺失 → 恢复失败。
                    // 返回 Err，保留整份 transaction 给下次恢复机会。
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "rollback_full_sync_transaction: backup file missing for RestoreFile {}: {}",
                        target_relative,
                        backup_path.display()
                    ))));
                }
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                // #644 评论 5484539222 缺陷2：durable copy + fsync 父目录。
                crate::storage::durable_copy_file(&backup_path, &target_path)?;
            }
            BackupEntry::RemoveCreated { target_relative } => {
                // #648 评论 5509379360 问题1：与 RestoreFile 分支一致，使用 recovery_live_root
                // （外置 git_dir/worktree_root 布局下可能与 target_root 不同）。
                let target_path = recovery_live_root.join(target_relative);
                if target_path.exists() {
                    fs::remove_file(&target_path)?;
                    // #644 评论 5484539222 缺陷2：remove 后 fsync 父目录。
                    crate::storage::sync_parent(&target_path)?;
                }
            }
        }
    }

    // #644 评论 5483920624 问题1：不在 phase 落盘前单独删 backup_dir。
    // 正确顺序：1.Git rollback → 2.live 文件恢复 → 3.原子写 RolledBack phase
    // → 4.phase 成功后 cleanup 整个 tx_dir（含 backup）。
    // 若 phase 写盘失败，backup 仍在，下次启动重试 rollback 可用。

    // 3. 标记 RolledBack。
    let manifest_path = tx_dir.join(MANIFEST_FILENAME);
    write_manifest_phase_static(&manifest_path, TransactionPhase::RolledBack, manifest)?;

    // 4. 清理事务目录（含 backup）。
    fs::remove_dir_all(tx_dir)?;

    Ok(())
}

/// #644 评论 5476546134 第2节：静态版本的 manifest phase 更新，供恢复流程使用。
///
/// 恢复时没有 `SaveTransaction` 实例，需要直接操作 manifest 文件。
#[cfg(test)]
fn write_manifest_phase_static(
    manifest_path: &Path,
    phase: TransactionPhase,
    existing: &TransactionManifest,
) -> Result<()> {
    let mut manifest = existing.clone();
    manifest.phase = phase;
    let json = serde_json::to_string_pretty(&manifest)?;
    crate::storage::atomic_write_string(manifest_path, &json)?;
    Ok(())
}

/// 扫描事务目录，恢复未完成的事务。
///
/// #644 评论 5475805198 第1节：基于 manifest phase 的状态机恢复。
///
/// 判定逻辑：
/// - manifest 存在且 phase 为 `FilesCommitted`/`Finished`/`RolledBack` → 清理目录
/// - manifest 存在且 phase 为 `FilesCommittedPendingGit` → 返回 `PendingGitRecovery`
/// - manifest 存在且 phase 为 `Prepared` → 尝试将暂存文件 rename 到目标
/// - 旧格式：`committed` 标记存在 → 清理目录（向后兼容）
/// - 两者都不存在 → 无效目录，清理
///
/// 返回 `(常规恢复列表, 待 Git finalize 的恢复列表)`。
// TODO(#597): 既有代码可读性技术债，待后续重构拆分
#[cfg(test)]
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    deprecated
)]
pub fn recover_pending_transactions(
    target_root: &Path,
) -> (Vec<TransactionRecovery>, Vec<PendingGitTransactionRecovery>) {
    let tx_base = target_root.join(TRANSACTIONS_DIR);
    if !tx_base.exists() {
        return (Vec::new(), Vec::new());
    }

    let mut results = Vec::new();
    let mut pending_git = Vec::new();
    let entries = match fs::read_dir(&tx_base) {
        Ok(e) => e,
        Err(_) => return (Vec::new(), Vec::new()),
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let tx_dir = entry.path();
        if !tx_dir.is_dir() {
            continue;
        }

        let manifest_path = tx_dir.join(MANIFEST_FILENAME);

        // #644 评论 5475805198 第1节：优先读 manifest 获取 phase。
        if manifest_path.exists() {
            let manifest: TransactionManifest = match fs::read_to_string(&manifest_path) {
                Ok(s) => match serde_json::from_str(&s) {
                    Ok(m) => m,
                    Err(e) => {
                        // #644 评论 5483239422 问题4：manifest 解析失败时保留 tx_dir。
                        // transaction 目录含 backup_entries + GitMetadataSnapshot +
                        // GitFinalizePlan 崩溃恢复材料，删除即销毁恢复证据。
                        // 记录错误，等下次启动重试或显式修复入口。
                        log::warn!(
                            "[transaction] recover: manifest parse failed for tx_dir={}: {} \
                             — preserving tx_dir (contains backup_entries/git_finalize/plan \
                             recovery material), will retry next startup",
                            tx_dir.display(),
                            e
                        );
                        continue;
                    }
                },
                Err(e) => {
                    // #644 评论 5483239422 问题4：manifest 读取失败时保留 tx_dir。
                    log::warn!(
                        "[transaction] recover: manifest read failed for tx_dir={}: {} \
                         — preserving tx_dir (contains backup_entries/git_finalize/plan \
                         recovery material), will retry next startup",
                        tx_dir.display(),
                        e
                    );
                    continue;
                }
            };

            match manifest.phase {
                TransactionPhase::FilesCommitted
                | TransactionPhase::Finished
                | TransactionPhase::RolledBack => {
                    // 事务已完成，清理目录。
                    let _ = fs::remove_dir_all(&tx_dir);
                    continue;
                }
                TransactionPhase::FilesCommittedPendingGit => {
                    // #644 评论 5475805198 第1节：文件已 commit，Git finalize 未完成。
                    // #644 评论 5476546134 第2节：有 git_finalize recovery record 时，
                    // 直接回滚到同步前状态（Git metadata + live 文件），不尝试向前完成。
                    // 下一次正常 full sync 重新跑。
                    if manifest.git_finalize.is_some() {
                        log::warn!(
                            "[transaction] found FilesCommittedPendingGit tx={}, rolling back",
                            manifest.transaction_id
                        );
                        // #644 评论 5476546134 第3/4节：统一回滚入口，不吞错。
                        if let Err(e) =
                            rollback_full_sync_transaction(&tx_dir, target_root, &manifest)
                        {
                            log::warn!(
                                "[transaction] rollback_full_sync_transaction failed for tx={}: {}",
                                manifest.transaction_id,
                                e
                            );
                            // #644 评论 5476546134 第4节：失败保留事务目录，给下次恢复机会。
                            continue;
                        }
                        continue;
                    }

                    // 旧格式：无 git_finalize recovery record，返回给调用方处理。
                    log::warn!(
                        "[transaction] found FilesCommittedPendingGit tx={}, \
                         no git_finalize record, needs manual Git recovery",
                        manifest.transaction_id
                    );
                    pending_git.push(PendingGitTransactionRecovery {
                        transaction_id: manifest.transaction_id.clone(),
                        manifest,
                        target_root: target_root.to_path_buf(),
                        tx_dir: tx_dir.clone(),
                    });
                    continue;
                }
                TransactionPhase::Prepared => {
                    // #644 评论 5476546134 第2节：区分两类事务的 Prepared 阶段语义。
                    // - 有 git_finalize recovery record：backup_mode + Git finalize 事务，
                    //   应该回滚（不尝试向前完成），下一次 full-sync 重跑。
                    // - 无 git_finalize：普通保存事务，继续重放 rename。
                    if manifest.git_finalize.is_some() {
                        log::warn!(
                            "[transaction] found Prepared tx={} with git_finalize, rolling back",
                            manifest.transaction_id
                        );
                        // #644 评论 5476546134 第3/4节：统一回滚入口，不吞错。
                        if let Err(e) =
                            rollback_full_sync_transaction(&tx_dir, target_root, &manifest)
                        {
                            log::warn!(
                                "[transaction] rollback_full_sync_transaction failed for tx={}: {}",
                                manifest.transaction_id,
                                e
                            );
                            // #644 评论 5476546134 第4节：失败保留事务目录，给下次恢复机会。
                            continue;
                        }
                        continue;
                    }
                    // 事务中断在 Prepared 阶段（无 git_finalize），尝试恢复 rename。
                }
            }
        } else {
            // 无 manifest：检查旧格式 committed marker（向后兼容）。
            let commit_marker = tx_dir.join(COMMIT_MARKER);
            if commit_marker.exists() {
                let _ = fs::remove_dir_all(&tx_dir);
                continue;
            }
            // 无 manifest 也无 committed marker → 无效目录。
            let _ = fs::remove_dir_all(&tx_dir);
            continue;
        }

        // Prepared 阶段恢复：重放 rename。
        // #644 评论 5483239422 问题4：重读 manifest 失败时保留 tx_dir，不删恢复证据。
        let manifest: TransactionManifest = match fs::read_to_string(&manifest_path) {
            Ok(s) => match serde_json::from_str(&s) {
                Ok(m) => m,
                Err(e) => {
                    log::warn!(
                        "[transaction] recover: manifest re-read parse failed for tx_dir={}: {} \
                         — preserving tx_dir, will retry next startup",
                        tx_dir.display(),
                        e
                    );
                    continue;
                }
            },
            Err(e) => {
                log::warn!(
                    "[transaction] recover: manifest re-read failed for tx_dir={}: {} \
                     — preserving tx_dir, will retry next startup",
                    tx_dir.display(),
                    e
                );
                continue;
            }
        };

        let mut recovered_files = Vec::new();
        let mut missing_files = Vec::new();
        let mut created_dirs = std::collections::HashSet::new();

        for tx_entry in &manifest.entries {
            if tx_entry.is_delete {
                // #644 评论 5473401065 第3节：NotFound 记 recovered（幂等）；
                // 其它错误记 missing_files + 日志，不能假装 recovered。
                let target_path = target_root.join(&tx_entry.target_relative);
                match fs::remove_file(&target_path) {
                    Ok(()) => {
                        // #644 评论 5484539222 缺陷2：remove 后 fsync 父目录。
                        if let Err(e) = crate::storage::sync_parent(&target_path) {
                            log::warn!(
                                "[transaction] recovery delete sync_parent failed: {}: {}",
                                tx_entry.target_relative,
                                e
                            );
                            missing_files.push(tx_entry.target_relative.clone());
                            continue;
                        }
                        recovered_files.push(tx_entry.target_relative.clone());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        recovered_files.push(tx_entry.target_relative.clone());
                    }
                    Err(e) => {
                        log::warn!(
                            "[transaction] recovery delete failed: {}: {}",
                            tx_entry.target_relative,
                            e
                        );
                        missing_files.push(tx_entry.target_relative.clone());
                    }
                }
            } else {
                let staging_path = tx_dir.join(&tx_entry.staging_filename);
                if staging_path.exists() {
                    let target_path = target_root.join(&tx_entry.target_relative);
                    if let Some(parent) = target_path.parent() {
                        if !created_dirs.contains(parent) && fs::create_dir_all(parent).is_ok() {
                            created_dirs.insert(parent.to_path_buf());
                        }
                    }
                    match fs::rename(&staging_path, &target_path) {
                        Ok(()) => {
                            // #644 评论 5484539222 缺陷2：rename 后 fsync 目标父目录。
                            if let Err(e) = crate::storage::sync_parent(&target_path) {
                                log::warn!(
                                    "[transaction] recovery rename sync_parent failed: {} -> {}: {}",
                                    tx_entry.staging_filename,
                                    tx_entry.target_relative,
                                    e
                                );
                                missing_files.push(tx_entry.target_relative.clone());
                                continue;
                            }
                            recovered_files.push(tx_entry.target_relative.clone());
                        }
                        Err(e) => {
                            log::warn!(
                                "[transaction] recovery rename failed: {} -> {}: {}",
                                tx_entry.staging_filename,
                                tx_entry.target_relative,
                                e
                            );
                            missing_files.push(tx_entry.target_relative.clone());
                        }
                    }
                } else {
                    missing_files.push(tx_entry.target_relative.clone());
                }
            }
        }

        if recovered_files.is_empty() && !missing_files.is_empty() {
            log::warn!(
                "[transaction] all staging files lost for tx={}, dropping",
                manifest.transaction_id
            );
        }

        let _ = fs::remove_dir_all(&tx_dir);

        results.push(TransactionRecovery {
            transaction_id: manifest.transaction_id,
            recovered_files,
            missing_files,
        });
    }

    (results, pending_git)
}
