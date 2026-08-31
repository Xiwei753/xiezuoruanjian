//! 同步冲突检测与解决。
//!
//! 本模块提供两种冲突检测路径：
//! - **Git 路径**（需 `git-https` feature）：通过 dry-run checkout + index diff 检测冲突，
//!   依赖 git2 crate 的合并基线计算。
//! - **LWW 路径**（始终可用）：由 `lww.rs` 中的三路比较检测，本模块不参与。
//!
//! 冲突解决策略：
//! - `resolve_conflict_keep_local`：保留本地版本，丢弃远端变更
//! - `resolve_conflict_take_remote`：接受远端版本，丢弃本地变更
//! - `resolve_conflict_mark_merged`：标记为已合并（用户手动解决后调用）

#[cfg(feature = "git-https")]
use crate::sync::service::SyncService;
use crate::sync::types::SyncConflict;
#[cfg(feature = "git-https")]
use crate::sync::types::SyncConflictSummary;
use std::path::Path;

/// 扫描作品目录 Git 状态，返回 (是否有脏文件, 脏文件路径列表)。
/// 仅统计白名单内且非黑名单的路径——黑名单路径（如 app-meta 内部文件）
/// 不影响同步决策。
#[cfg(feature = "git-https")]
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub(crate) fn collect_git_status_summary(
    repo: &git2::Repository,
    scope: crate::sync::types::SyncScope,
) -> (bool, Vec<String>) {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true);
    let mut local_dirty = false;
    let mut dirty_files = Vec::new();
    if let Ok(statuses) = repo.statuses(Some(&mut opts)) {
        for entry in statuses.iter() {
            if let Some(path) = entry.path() {
                if SyncService::is_blacklisted_path(path, scope)
                    || !SyncService::is_whitelisted_path(path, scope)
                {
                    continue;
                }
                let status = entry.status();
                if status.is_wt_modified()
                    || status.is_index_modified()
                    || status.is_wt_new()
                    || status.is_index_new()
                    || status.is_wt_deleted()
                    || status.is_index_deleted()
                {
                    local_dirty = true;
                    dirty_files.push(path.to_string());
                }
            }
        }
    }
    (local_dirty, dirty_files)
}

/// 从 Git index 中收集未解决的合并冲突路径。
/// 路径优先取 `our`（本地）侧，回退到 `their`（远端）或 `ancestor`（共同祖先）。
/// 仅返回白名单内且非黑名单的路径，去重排序。
#[cfg(feature = "git-https")]
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub(crate) fn collect_index_conflicts(
    repo: &git2::Repository,
    scope: crate::sync::types::SyncScope,
) -> Vec<String> {
    let mut conflicted = Vec::new();
    if let Ok(index) = repo.index() {
        if index.has_conflicts() {
            if let Ok(conflicts) = index.conflicts() {
                for conflict in conflicts.flatten() {
                    let mut best_path = None;
                    if let Some(our) = &conflict.our {
                        best_path = Some(String::from_utf8_lossy(&our.path).to_string());
                    } else if let Some(their) = &conflict.their {
                        best_path = Some(String::from_utf8_lossy(&their.path).to_string());
                    } else if let Some(ancestor) = &conflict.ancestor {
                        best_path = Some(String::from_utf8_lossy(&ancestor.path).to_string());
                    }
                    if let Some(path) = best_path {
                        if !SyncService::is_blacklisted_path(&path, scope)
                            && SyncService::is_whitelisted_path(&path, scope)
                        {
                            conflicted.push(path);
                        }
                    }
                }
            }
        }
    }
    conflicted.sort();
    conflicted.dedup();
    conflicted
}

/// 构建同步冲突摘要——诊断当前同步状态并收集冲突文件列表。
///
/// 冲突文件来源有两个：
/// 1. **dry-run checkout**：模拟 `git checkout` 检测远端与本地的作品目录冲突
/// 2. **index conflicts**：`git merge` 后留在 index 中的未解决冲突
///
/// 两者合并去重后过滤掉黑名单路径和非白名单路径。
/// 若最终冲突列表为空但本地有脏文件，则用脏文件列表替代
/// （这种情况通常意味着本地修改与远端无直接冲突，但需要用户确认）。
#[cfg(feature = "git-https")]
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub(crate) fn build_conflict_summary(
    repo: &git2::Repository,
    fetch_commit_id: Option<git2::Oid>,
    blocked_reason: &str,
    scope: crate::sync::types::SyncScope,
) -> SyncConflictSummary {
    let (local_dirty, dirty_files) = collect_git_status_summary(repo, scope);

    let mut remote_changed = false;
    if let Some(remote_oid) = fetch_commit_id {
        if let Ok(head) = repo.head() {
            if let Some(local_oid) = head.target() {
                if local_oid != remote_oid {
                    remote_changed = true;
                }
            }
        } else {
            remote_changed = true;
        }
    }

    let mut conflicted_files = Vec::new();
    if let Some(remote_oid) = fetch_commit_id {
        if let Ok(commit) = repo.find_commit(remote_oid) {
            if let Ok(tree) = commit.tree() {
                let paths = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
                // SAFETY: git2 操作非线程安全，dry-run checkout 在单线程上下文中执行。
                // Rc<RefCell> 仅在此闭包和下方 borrow 之间共享，不跨线程。
                let cp_clone = paths.clone();
                let mut dry_run_builder = git2::build::CheckoutBuilder::default();
                dry_run_builder.notify_on(git2::CheckoutNotificationType::CONFLICT);
                dry_run_builder.notify(move |_, path, _, _, _| {
                    if let Some(p) = path {
                        if let Some(s) = p.to_str() {
                            cp_clone.borrow_mut().push(s.to_string());
                        }
                    }
                    true
                });
                dry_run_builder.dry_run();
                let _ = repo.checkout_tree(tree.as_object(), Some(&mut dry_run_builder));
                conflicted_files = paths.borrow().clone();
            }
        }
    }

    let index_conflicts = collect_index_conflicts(repo, scope);
    conflicted_files.extend(index_conflicts);

    conflicted_files.retain(|path| {
        !SyncService::is_blacklisted_path(path, scope)
            && SyncService::is_whitelisted_path(path, scope)
    });
    conflicted_files.sort();
    conflicted_files.dedup();

    if conflicted_files.is_empty() && local_dirty {
        conflicted_files = dirty_files;
    }

    SyncConflictSummary {
        status: "conflict".to_string(),
        local_dirty,
        remote_changed,
        conflicted_files,
        blocked_reason: blocked_reason.to_string(),
        safe_next_steps: vec![
            "备份当前作品目录。".to_string(),
            "运行诊断确认网络/认证没问题。".to_string(),
            "手动处理冲突后重新同步。".to_string(),
        ],
    }
}

/// #644 评论 5473789298 第4节：读取 `app-meta/sync/conflicts.json`。
///
/// 文件不存在或内容损坏（半写/无效 JSON）时回退为空列表——丢失冲突记录比
/// 阻塞后续同步更可接受，与 [`crate::sync::SyncService::remove_conflict_from_json`]
/// 的容错策略一致。
fn load_conflicts_json(sync_root: &Path) -> crate::Result<Vec<SyncConflict>> {
    let conflicts_path = sync_root.join("app-meta/sync/conflicts.json");
    if !conflicts_path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&conflicts_path)?;
    let conflicts: Vec<SyncConflict> = serde_json::from_str(&content).unwrap_or_default();
    Ok(conflicts)
}

/// #644 评论 5473789298 第4节：一次事务写入 `state.local.json` + `conflicts.json`。
///
/// 用 [`crate::storage::transaction::SaveTransaction`] 保证两个文件原子提交，
/// 不会出现"state 写了但 conflicts.json 没写"的中间不一致状态。
/// `record_staging_conflicts` 和 [`crate::sync::SyncService::record_sync_conflict`]
/// 共用本函数，不要两套写法。
fn persist_conflict_state(
    sync_root: &Path,
    state: &crate::sync::types::SyncState,
    conflicts: &[SyncConflict],
) -> crate::Result<()> {
    let state_json = serde_json::to_string_pretty(state)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
    let conflicts_json = serde_json::to_string_pretty(conflicts)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;

    let mut tx = crate::storage::transaction::SaveTransaction::new(sync_root);
    tx.add_bytes("app-meta/sync/state.local.json", state_json.as_bytes())?;
    tx.add_bytes("app-meta/sync/conflicts.json", conflicts_json.as_bytes())?;
    tx.commit()?;
    Ok(())
}

/// #644 评论 5473789298 第4节：按 `local_path` 去重/替换加入冲突。
///
/// 同一路径重复写入时替换已有记录，不无限 append。`state_conflicts` 和
/// `conflicts_json` 都做同样的去重，保持两者一致。
fn upsert_conflict(
    conflicts_json: &mut Vec<SyncConflict>,
    state_conflicts: &mut Vec<SyncConflict>,
    conflicted_files: &mut std::collections::HashSet<String>,
    conflict: SyncConflict,
) {
    let path = conflict.local_path.clone();
    conflicted_files.insert(path.clone());
    if let Some(existing) = conflicts_json.iter_mut().find(|c| c.local_path == path) {
        *existing = conflict.clone();
    } else {
        conflicts_json.push(conflict.clone());
    }
    if let Some(existing) = state_conflicts.iter_mut().find(|c| c.local_path == path) {
        *existing = conflict.clone();
    } else {
        state_conflicts.push(conflict);
    }
}

/// #644 评论 5473551127 第3节：staging 三方冲突 → `SyncConflict` 映射 + 持久化。
///
/// #644 评论 5473789298 第4节：改成完整事务——先在内存里构造新的 `SyncState` 和
/// 完整 `Vec<SyncConflict>`，用 [`persist_conflict_state`] 一次提交
/// `app-meta/sync/state.local.json` + `app-meta/sync/conflicts.json`。
/// 不再循环调用 `record_sync_conflict` 一条一条落盘，中间写失败不会留下不一致。
/// 同一路径重复写入时按 `local_path` 去重/替换。
///
/// 返回映射后的 `Vec<SyncConflict>`，供调用方填入 `SyncResult.conflicts`。
/// 持久化失败必须返回 Err（不能只打日志），让对应 target 进入错误状态。
pub fn record_staging_conflicts(
    sync_root: &Path,
    remote_prefix: &str,
    staging_conflicts: &[crate::sync::staging::StagingConflict],
) -> crate::Result<Vec<SyncConflict>> {
    if staging_conflicts.is_empty() {
        return Ok(Vec::new());
    }

    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0);

    // 先在内存里构造完整的新状态。
    let mut state = crate::sync::SyncService::load_sync_state(sync_root)?;
    let mut conflicts_json = load_conflicts_json(sync_root)?;
    let mut result = Vec::with_capacity(staging_conflicts.len());

    for sc in staging_conflicts {
        let rel_str = sc.rel_path.to_string_lossy().to_string();
        let rel_unix = rel_str.replace('\\', "/");
        let sync_conflict = SyncConflict {
            local_path: rel_str.clone(),
            remote_path: format!("{}/{}", remote_prefix, rel_unix),
            local_hash: sc.local_hash.clone(),
            remote_hash: sc.incoming_hash.clone(),
            base_hash: sc.base_hash.clone(),
            created_at: now_ts,
            description: format!(
                "three-way conflict: both local and remote changed {}",
                sc.rel_path.display()
            ),
        };

        upsert_conflict(
            &mut conflicts_json,
            &mut state.conflicts,
            &mut state.conflicted_files,
            sync_conflict.clone(),
        );
        result.push(sync_conflict);
    }

    // 一次事务写 state + conflicts.json。
    persist_conflict_state(sync_root, &state, &conflicts_json)?;
    Ok(result)
}

impl crate::sync::SyncService {
    /// Remove conflict records for `path` from the `conflicts.json` file.
    /// This keeps the on-disk conflict list in sync with `state.conflicts`.
    ///
    /// 如果 `conflicts.json` 不存在或内容损坏（半写/无效 JSON），
    /// 回退为空列表——丢失冲突记录比阻塞后续同步更可接受。
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    fn remove_conflict_from_json(sync_root: &Path, path: &str) {
        let conflicts_path = sync_root.join("app-meta/sync/conflicts.json");
        if !conflicts_path.exists() {
            return;
        }
        if let Ok(content) = std::fs::read_to_string(&conflicts_path) {
            let mut conflicts: Vec<SyncConflict> =
                serde_json::from_str(&content).unwrap_or_default();
            let before = conflicts.len();
            conflicts.retain(|c| c.local_path != path && c.remote_path != path);
            if conflicts.len() != before {
                if let Ok(json) = serde_json::to_string_pretty(&conflicts) {
                    let tmp_path = conflicts_path.with_extension("tmp");
                    if std::fs::write(&tmp_path, &json).is_ok() {
                        let _ = std::fs::rename(tmp_path, conflicts_path);
                    }
                }
            }
        }
    }

    /// 记录同步冲突——将冲突元数据追加到 `app-meta/sync/conflicts.json`，
    /// 并将本地内容备份为 `{path}.conflict.{timestamp}` 文件。
    ///
    /// #644 评论 5473789298 第4节：改成完整事务——先在内存里构造新的 `SyncState`
    /// 和完整 `Vec<SyncConflict>`，用 [`persist_conflict_state`] 一次提交
    /// `state.local.json` + `conflicts.json`。同时更新 `conflicted_files` 和
    /// `state.conflicts`，修复原来"调用前路径已加入 conflicted_files"的注释违反。
    /// 同一路径重复写入时按 `local_path` 去重/替换，不无限 append。
    ///
    /// 备份文件（`{path}.conflict.{timestamp}`）是辅助文件，不进事务（事务只保证
    /// state + conflicts.json 一致，备份文件丢失不影响同步语义）。
    pub fn record_sync_conflict(
        sync_root: &Path,
        conflict: SyncConflict,
        local_content: Option<&str>,
    ) -> crate::Result<()> {
        // 备份本地内容（事务外，辅助文件）。
        if let Some(content) = local_content {
            let conflict_file_path = sync_root.join(format!(
                "{}.conflict.{}",
                conflict.local_path, conflict.created_at
            ));
            if let Some(parent) = conflict_file_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&conflict_file_path, content)?;
        }

        // 先在内存里构造完整的新状态。
        let mut state = Self::load_sync_state(sync_root)?;
        let mut conflicts_json = load_conflicts_json(sync_root)?;

        upsert_conflict(
            &mut conflicts_json,
            &mut state.conflicts,
            &mut state.conflicted_files,
            conflict,
        );

        // 一次事务写 state + conflicts.json。
        persist_conflict_state(sync_root, &state, &conflicts_json)?;
        Ok(())
    }

    /// Resolve a conflict by keeping the local version.
    ///
    /// Removes the path from `conflicted_files` and `conflicts`, and updates
    /// `known_files` to the remote hash so the next sync sees
    /// base=remote_hash, local≠base, remote=base → LocalChanged → uploads
    /// the local version.
    ///
    /// 不变量：known_files 必须设为 remote_hash（而非 local_hash）。
    /// 若设为 local_hash，三路比较会看到 base=local_hash, remote≠base → RemoteChanged，
    /// 导致下次同步下载远端版本覆盖本地——与"保留本地"的意图相反。
    /// 设为 remote_hash 后，三路比较看到 base=remote_hash, local≠base, remote=base
    /// → LocalChanged → 上传本地版本，符合预期。
    pub fn resolve_conflict_keep_local(sync_root: &Path, path: &str) -> crate::Result<()> {
        let mut state = Self::load_sync_state(sync_root)?;
        if !state.conflicted_files.remove(path) {
            return Err(crate::Error::Other(format!(
                "resolve_conflict_keep_local: path '{}' is not in conflicted_files",
                path
            )));
        }
        // Set known_files to the remote_hash so that three-way comparison on the
        // next sync sees: base=remote_hash, local≠base, remote=base → LocalChanged → upload.
        // If we set known_files to local_hash instead, three-way would see
        // RemoteChanged and download the remote version over local — the opposite of
        // what "keep local" means.
        if let Some(conflict) = state.conflicts.iter().find(|c| c.local_path == path) {
            state
                .known_files
                .insert(path.to_string(), conflict.remote_hash.clone());
            if let Some(t) = state
                .known_files_updated_at
                .get(&conflict.remote_path)
                .cloned()
            {
                state.known_files_updated_at.insert(path.to_string(), t);
            }
        } else {
            // Fallback: if no conflict record, use the current local file hash.
            let full_path = sync_root.join(path);
            if full_path.exists() {
                let content = std::fs::read(&full_path)?;
                let hash = format!("{:x}", md5::compute(&content));
                state.known_files.insert(path.to_string(), hash);
            }
        }
        // Remove the conflict record from state.conflicts
        state
            .conflicts
            .retain(|c| c.local_path != path && c.remote_path != path);
        Self::remove_conflict_from_json(sync_root, path);
        Self::save_sync_state(sync_root, &state)?;
        Ok(())
    }

    /// Resolve a conflict by taking the remote version.
    ///
    /// Removes the path from `conflicted_files` and `conflicts`, and adds it
    /// to `pending_take_remote`. On the next `perform_sync`, the engine will
    /// force-download the remote content to the local file, then update
    /// `known_files` to the final local hash.
    ///
    /// 不变量：不直接下载远端内容（可能在本函数调用时网络不可用），
    /// 而是标记为 pending_take_remote，下次 perform_sync 时在正常三路比较之前
    /// 强制下载。这保证"采用远端"意图不会因网络临时故障而丢失。
    pub fn resolve_conflict_take_remote(sync_root: &Path, path: &str) -> crate::Result<()> {
        let mut state = Self::load_sync_state(sync_root)?;
        if !state.conflicted_files.remove(path) {
            return Err(crate::Error::Other(format!(
                "resolve_conflict_take_remote: path '{}' is not in conflicted_files",
                path
            )));
        }
        // Mark as pending_take_remote so the next perform_sync force-downloads
        // the remote content to the local file.
        state.pending_take_remote.insert(path.to_string());
        // Remove the conflict record from state.conflicts
        state
            .conflicts
            .retain(|c| c.local_path != path && c.remote_path != path);
        Self::remove_conflict_from_json(sync_root, path);
        Self::save_sync_state(sync_root, &state)?;
        Ok(())
    }

    /// Resolve a conflict by marking it as manually merged.
    ///
    /// Removes the path from `conflicted_files` and `conflicts`, and updates
    /// `known_files` to the remote hash so the next sync sees
    /// base=remote_hash, local≠base, remote=base → LocalChanged → uploads
    /// the merged version.
    pub fn resolve_conflict_mark_merged(sync_root: &Path, path: &str) -> crate::Result<()> {
        let mut state = Self::load_sync_state(sync_root)?;
        if !state.conflicted_files.remove(path) {
            return Err(crate::Error::Other(format!(
                "resolve_conflict_mark_merged: path '{}' is not in conflicted_files",
                path
            )));
        }
        // Set known_files to the remote_hash so that three-way comparison on the
        // next sync sees: base=remote_hash, local≠base, remote=base → LocalChanged → upload.
        // This ensures the merged local version gets uploaded to the remote.
        if let Some(conflict) = state.conflicts.iter().find(|c| c.local_path == path) {
            state
                .known_files
                .insert(path.to_string(), conflict.remote_hash.clone());
            if let Some(t) = state
                .known_files_updated_at
                .get(&conflict.remote_path)
                .cloned()
            {
                state.known_files_updated_at.insert(path.to_string(), t);
            }
        } else {
            // Fallback: if no conflict record, use the current local file hash.
            let full_path = sync_root.join(path);
            if full_path.exists() {
                let content = std::fs::read(&full_path)?;
                let hash = format!("{:x}", md5::compute(&content));
                state.known_files.insert(path.to_string(), hash);
            }
        }
        // Remove the conflict record from state.conflicts
        state
            .conflicts
            .retain(|c| c.local_path != path && c.remote_path != path);
        Self::remove_conflict_from_json(sync_root, path);
        Self::save_sync_state(sync_root, &state)?;
        Ok(())
    }
}
