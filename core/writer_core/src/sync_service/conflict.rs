#![allow(unused_imports)]
use std::path::Path;
use serde::{Deserialize, Serialize};
use base64::Engine;
use std::collections::HashMap;
use crate::sync_service::*;

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

impl crate::sync_service::SyncService {
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

impl crate::sync_service::SyncService {
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

}
