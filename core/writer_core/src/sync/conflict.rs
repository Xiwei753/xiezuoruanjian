#[cfg(feature = "git-https")]
use crate::sync::service::SyncService;
#[cfg(feature = "git-https")]
use crate::sync::types::SettingConflictDetail;
use crate::sync::types::SyncConflict;
#[cfg(feature = "git-https")]
use crate::sync::types::SyncConflictSummary;
use std::path::Path;

/// 扫描工作区 Git 状态，返回 (是否有脏文件, 脏文件路径列表)。
/// 仅统计白名单内且非黑名单的路径——黑名单路径（如 app-meta 内部文件）
/// 不影响同步决策。
#[cfg(feature = "git-https")]
pub(crate) fn collect_git_status_summary(repo: &git2::Repository) -> (bool, Vec<String>) {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true);
    let mut local_dirty = false;
    let mut dirty_files = Vec::new();
    if let Ok(statuses) = repo.statuses(Some(&mut opts)) {
        for entry in statuses.iter() {
            if let Some(path) = entry.path() {
                if SyncService::is_blacklisted_path(path) || !SyncService::is_whitelisted_path(path)
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
pub(crate) fn collect_index_conflicts(repo: &git2::Repository) -> Vec<String> {
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
                        if !SyncService::is_blacklisted_path(&path)
                            && SyncService::is_whitelisted_path(&path)
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
/// 1. **dry-run checkout**：模拟 `git checkout` 检测远端与本地的工作区冲突
/// 2. **index conflicts**：`git merge` 后留在 index 中的未解决冲突
///
/// 两者合并去重后过滤掉黑名单路径和非白名单路径。
/// 若最终冲突列表为空但本地有脏文件，则用脏文件列表替代
/// （这种情况通常意味着本地修改与远端无直接冲突，但需要用户确认）。
#[cfg(feature = "git-https")]
pub(crate) fn build_conflict_summary(
    repo: &git2::Repository,
    fetch_commit_id: Option<git2::Oid>,
    blocked_reason: &str,
) -> SyncConflictSummary {
    let (local_dirty, dirty_files) = collect_git_status_summary(repo);

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

    let index_conflicts = collect_index_conflicts(repo);
    conflicted_files.extend(index_conflicts);

    conflicted_files.retain(|path| {
        !SyncService::is_blacklisted_path(path) && SyncService::is_whitelisted_path(path)
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
            "备份当前工作区。".to_string(),
            "运行诊断确认网络/认证没问题。".to_string(),
            "手动处理冲突后重新同步。".to_string(),
        ],
    }
}

#[cfg(feature = "git-https")]
impl crate::sync::SyncService {
    /// 逐键三路合并 JSON 设置对象。
    ///
    /// 以 base 为双方上次同步后的共识版本，对每个 key 独立判断：
    /// - local == remote：取任一方
    /// - 仅一方修改：取修改方
    /// - 双方都修改且值不同：记录冲突
    /// - 一方删除、另一方未修改 base：跟随删除
    /// - 一方删除、另一方修改了 base：记录冲突（删除 vs 修改）
    ///
    /// 返回合并后的 Map；若有任何 key 冲突则返回冲突列表。
    pub(crate) fn semantic_merge_json(
        base: &serde_json::Map<String, serde_json::Value>,
        local: &serde_json::Map<String, serde_json::Value>,
        remote: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<serde_json::Map<String, serde_json::Value>, Vec<SettingConflictDetail>> {
        let mut merged = serde_json::Map::new();
        let mut conflicts = Vec::new();

        // Collect all keys from all three maps
        let mut keys: std::collections::HashSet<&String> = base.keys().collect();
        keys.extend(local.keys());
        keys.extend(remote.keys());

        for key in keys {
            let base_val = base.get(key);
            let local_val = local.get(key);
            let remote_val = remote.get(key);

            match (base_val, local_val, remote_val) {
                (None, None, None) => {}
                (_, Some(l), None) => {
                    if base_val == Some(l) {
                        // Deleted in remote, unmodified in local
                    } else {
                        conflicts.push(SettingConflictDetail {
                            key: key.clone(),
                            local_value: l.clone(),
                            remote_value: serde_json::Value::Null,
                        });
                    }
                }
                (_, None, Some(r)) => {
                    if base_val == Some(r) {
                        // Deleted in local, unmodified in remote
                    } else {
                        conflicts.push(SettingConflictDetail {
                            key: key.clone(),
                            local_value: serde_json::Value::Null,
                            remote_value: r.clone(),
                        });
                    }
                }
                (Some(b), Some(l), Some(r)) => {
                    if l == r {
                        merged.insert(key.clone(), l.clone());
                    } else if l == b {
                        merged.insert(key.clone(), r.clone());
                    } else if r == b {
                        merged.insert(key.clone(), l.clone());
                    } else {
                        conflicts.push(SettingConflictDetail {
                            key: key.clone(),
                            local_value: l.clone(),
                            remote_value: r.clone(),
                        });
                    }
                }
                (None, Some(l), Some(r)) => {
                    if l == r {
                        merged.insert(key.clone(), l.clone());
                    } else {
                        conflicts.push(SettingConflictDetail {
                            key: key.clone(),
                            local_value: l.clone(),
                            remote_value: r.clone(),
                        });
                    }
                }
                (Some(_b), None, None) => {}
            }
        }

        if !conflicts.is_empty() {
            Err(conflicts)
        } else {
            Ok(merged)
        }
    }
}

impl crate::sync::SyncService {
    /// Remove conflict records for `path` from the `conflicts.json` file.
    /// This keeps the on-disk conflict list in sync with `state.conflicts`.
    fn remove_conflict_from_json(workspace_path: &Path, path: &str) {
        let conflicts_path = workspace_path.join("app-meta/sync/conflicts.json");
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
    pub fn record_sync_conflict(
        workspace_path: &Path,
        conflict: SyncConflict,
        local_content: Option<&str>,
    ) -> crate::Result<()> {
        if let Some(content) = local_content {
            let conflict_file_path = workspace_path.join(format!(
                "{}.conflict.{}",
                conflict.local_path, conflict.created_at
            ));
            if let Some(parent) = conflict_file_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&conflict_file_path, content)?;
        }

        let conflicts_path = workspace_path.join("app-meta/sync/conflicts.json");
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
    pub fn resolve_conflict_keep_local(workspace_path: &Path, path: &str) -> crate::Result<()> {
        let mut state = Self::load_sync_state(workspace_path)?;
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
            let full_path = workspace_path.join(path);
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
        Self::remove_conflict_from_json(workspace_path, path);
        Self::save_sync_state(workspace_path, &state)?;
        Ok(())
    }

    /// Resolve a conflict by taking the remote version.
    ///
    /// Removes the path from `conflicted_files` and `conflicts`, and adds it
    /// to `pending_take_remote`. On the next `perform_sync`, the engine will
    /// force-download the remote content to the local file, then update
    /// `known_files` to the final local hash.
    pub fn resolve_conflict_take_remote(workspace_path: &Path, path: &str) -> crate::Result<()> {
        let mut state = Self::load_sync_state(workspace_path)?;
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
        Self::remove_conflict_from_json(workspace_path, path);
        Self::save_sync_state(workspace_path, &state)?;
        Ok(())
    }

    /// Resolve a conflict by marking it as manually merged.
    ///
    /// Removes the path from `conflicted_files` and `conflicts`, and updates
    /// `known_files` to the remote hash so the next sync sees
    /// base=remote_hash, local≠base, remote=base → LocalChanged → uploads
    /// the merged version.
    pub fn resolve_conflict_mark_merged(workspace_path: &Path, path: &str) -> crate::Result<()> {
        let mut state = Self::load_sync_state(workspace_path)?;
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
            let full_path = workspace_path.join(path);
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
        Self::remove_conflict_from_json(workspace_path, path);
        Self::save_sync_state(workspace_path, &state)?;
        Ok(())
    }
}
