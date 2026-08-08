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
    /// 不变量：调用此方法前，路径必须已加入 `SyncState.conflicted_files`；
    /// 调用后 `conflicts.json` 与 `state.conflicts` 保持一致。
    /// 备份文件仅用于用户手动对比，不参与自动合并或同步逻辑。
    /// 写入使用 atomic rename（先写 .tmp 再 rename），避免半写状态。
    pub fn record_sync_conflict(
        sync_root: &Path,
        conflict: SyncConflict,
        local_content: Option<&str>,
    ) -> crate::Result<()> {
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

        let conflicts_path = sync_root.join("app-meta/sync/conflicts.json");
        if let Some(parent) = conflicts_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut conflicts: Vec<SyncConflict> = if conflicts_path.exists() {
            let content = std::fs::read_to_string(&conflicts_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        };

        conflicts.push(conflict);

        let content = serde_json::to_string_pretty(&conflicts)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;

        let tmp_path = conflicts_path.with_extension("tmp");
        std::fs::write(&tmp_path, content)?;
        std::fs::rename(tmp_path, conflicts_path)?;

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
