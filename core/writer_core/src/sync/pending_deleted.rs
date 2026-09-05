//! 待删除同步 target 的持久化 — provider-neutral（Issue #645 评论 5504296097 问题1）。
//!
//! `PendingDeletedTarget` 列表存到
//! `<app_data_root>/app-meta/sync/pending_deleted_targets.json`，用 atomic write。
//!
//! ## 生命周期
//!
//! 1. `delete_project_with_changes` 时调 [`record_pending_deleted_target`] 记录；
//! 2. `prepare_full_sync` 调 [`load_pending_deleted_targets`] 加载，加入 plan；
//! 3. `run_transfer` 对 deleted target 走 target-delete 计划；
//! 4. 全部远端删除成功后调 [`remove_pending_deleted_target`] 移除该条目。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::sync::types::PendingDeletedTarget;

const PENDING_DELETED_FILE: &str = "pending_deleted_targets.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PendingDeletedTargetList {
    #[serde(default)]
    targets: Vec<PendingDeletedTarget>,
}

fn pending_deleted_path(app_data_root: &Path) -> std::path::PathBuf {
    app_data_root
        .join("app-meta")
        .join("sync")
        .join(PENDING_DELETED_FILE)
}

/// 加载所有待删除的同步 target。
///
/// 文件不存在时返回空 Vec。文件损坏时返回 Err（不吞错误）。
pub fn load_pending_deleted_targets(
    app_data_root: &Path,
) -> crate::error::Result<Vec<PendingDeletedTarget>> {
    let path = pending_deleted_path(app_data_root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)?;
    let list: PendingDeletedTargetList = serde_json::from_str(&content).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "load_pending_deleted_targets: parse {}: {e}",
            path.display()
        )))
    })?;
    Ok(list.targets)
}

/// 记录一个待删除的同步 target（追加到持久化列表）。
///
/// 用 read-modify-write + atomic write。幂等：相同 `journal_token` 不重复追加。
pub fn record_pending_deleted_target(
    app_data_root: &Path,
    target: PendingDeletedTarget,
) -> crate::error::Result<()> {
    let path = pending_deleted_path(app_data_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut current = load_pending_deleted_targets(app_data_root).unwrap_or_default();
    if current
        .iter()
        .any(|t| t.journal_token == target.journal_token)
    {
        return Ok(());
    }
    current.push(target);
    let list = PendingDeletedTargetList { targets: current };
    let content = serde_json::to_string_pretty(&list).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "record_pending_deleted_target: serialize: {e}"
        )))
    })?;
    crate::storage::atomic_write_string(&path, &content)?;
    Ok(())
}

/// 移除一个已完成的待删除 target（按 `journal_token` 匹配）。
///
/// 用 read-modify-write + atomic write。未找到时返回 Ok(())（幂等）。
pub fn remove_pending_deleted_target(
    app_data_root: &Path,
    journal_token: &str,
) -> crate::error::Result<()> {
    let path = pending_deleted_path(app_data_root);
    if !path.exists() {
        return Ok(());
    }
    let mut current = load_pending_deleted_targets(app_data_root)?;
    let before = current.len();
    current.retain(|t| t.journal_token != journal_token);
    if current.len() == before {
        return Ok(());
    }
    let list = PendingDeletedTargetList { targets: current };
    let content = serde_json::to_string_pretty(&list).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "remove_pending_deleted_target: serialize: {e}"
        )))
    })?;
    crate::storage::atomic_write_string(&path, &content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn record_load_remove_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let target = PendingDeletedTarget::for_project("p1", 1000, "token_a");
        record_pending_deleted_target(root, target.clone()).unwrap();
        let loaded = load_pending_deleted_targets(root).unwrap();
        assert_eq!(loaded, vec![target.clone()]);

        // 幂等：相同 journal_token 不重复追加。
        record_pending_deleted_target(root, target).unwrap();
        let loaded = load_pending_deleted_targets(root).unwrap();
        assert_eq!(loaded.len(), 1);

        // 移除后为空。
        remove_pending_deleted_target(root, "token_a").unwrap();
        let loaded = load_pending_deleted_targets(root).unwrap();
        assert!(loaded.is_empty());

        // 幂等移除。
        remove_pending_deleted_target(root, "token_a").unwrap();
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let loaded = load_pending_deleted_targets(tmp.path()).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn multiple_targets_preserved() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        record_pending_deleted_target(root, PendingDeletedTarget::for_project("p1", 1000, "t1"))
            .unwrap();
        record_pending_deleted_target(root, PendingDeletedTarget::for_project("p2", 2000, "t2"))
            .unwrap();
        let loaded = load_pending_deleted_targets(root).unwrap();
        assert_eq!(loaded.len(), 2);

        remove_pending_deleted_target(root, "t1").unwrap();
        let loaded = load_pending_deleted_targets(root).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].journal_token, "t2");
    }
}
