//! 待清理远端残留的持久化 — provider-neutral（#645 评论 5504296097 问题3 修复）。
//!
//! 当 authoritative Delete 清 prefix（`delete_all_remote_objects(projects/P)`）失败时，
//! 记录 `PendingRemoteTargetCleanup` 到
//! `<app_data_root>/app-meta/sync/pending_remote_cleanups.json`，用 atomic write。
//!
//! ## 为什么需要这个文件
//!
//! `RestoreProject` 的 remote-only 场景：本地没有 P，Prepare 看见 remote Upsert →
//! `RestoreProject`，Transfer 前 remote 又变 Delete → `delete_all_remote_objects(projects/P)`。
//! 如果 cleanup 失败：本地仍然没有 P，remote catalog 已经是 Delete(P)。下一轮 planner
//! 对"remote Delete + 本地无 live + 无 pending delete"会直接跳过，再也没有 target
//! 会清这个 prefix。`PendingRemoteTargetCleanup` 让下一轮 Prepare 即使本地没有 Project，
//! 也生成 `RemoteCleanupProject` target 重试清理。
//!
//! ## 生命周期
//!
//! 1. Transfer 阶段 `delete_all_remote_objects` 失败时调 [`record_pending_remote_cleanup`]；
//! 2. `prepare_full_sync` 调 [`load_pending_remote_cleanups`] 加载，加入 plan
//!    （`PlannedTargetKind::RemoteCleanupProject`）；
//! 3. `run_transfer` 对 `RemoteCleanupProject` target 走 `delete_all_remote_objects`；
//! 4. 全部远端删除成功后调 [`remove_pending_remote_cleanup`] 移除该条目。

use std::path::Path;

use serde::{Deserialize, Serialize};

/// 一条待清理的远端残留记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRemoteTargetCleanup {
    /// 远端前缀，如 `projects/P`。
    pub remote_prefix: String,
    /// 对应的 project id（用于构造 SyncTarget 和本地路径）。
    pub project_id: String,
    /// 上次 cleanup 失败的错误描述（供诊断）。
    pub last_error: String,
    /// 记录创建时间（Unix 毫秒）。
    pub created_at_ms: i64,
    /// #645 评论 5504296097 问题2 修复：绑定产生该 cleanup 的 Delete lifecycle identity。
    ///
    /// Transfer 前重新确认远端 catalog 时，用这两个字段校验当前 winner record
    /// 仍是同一条/更新的 Delete（lww_time 和 device_id 匹配，或当前 Delete 的
    /// lww_time >= expected）。当前是 Upsert → pending 过期，不删 prefix。
    pub expected_delete_lww_time_ms: i64,
    /// #645 评论 5504296097 问题2 修复：绑定 Delete 的 device_id（tie-break）。
    pub expected_delete_device_id: String,
}

const PENDING_REMOTE_CLEANUP_FILE: &str = "pending_remote_cleanups.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PendingRemoteTargetCleanupList {
    #[serde(default)]
    cleanups: Vec<PendingRemoteTargetCleanup>,
}

fn pending_remote_cleanup_path(app_data_root: &Path) -> std::path::PathBuf {
    app_data_root
        .join("app-meta")
        .join("sync")
        .join(PENDING_REMOTE_CLEANUP_FILE)
}

/// 加载所有待清理的远端残留记录。
///
/// 文件不存在时返回空 Vec。文件损坏时返回 Err（不吞错误）。
pub fn load_pending_remote_cleanups(
    app_data_root: &Path,
) -> crate::error::Result<Vec<PendingRemoteTargetCleanup>> {
    let path = pending_remote_cleanup_path(app_data_root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)?;
    let list: PendingRemoteTargetCleanupList = serde_json::from_str(&content).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "load_pending_remote_cleanups: parse {}: {e}",
            path.display()
        )))
    })?;
    Ok(list.cleanups)
}

/// 记录一条待清理的远端残留（追加到持久化列表）。
///
/// 用 read-modify-write + atomic write。幂等：相同 `remote_prefix` 不重复追加，
/// 只更新 `last_error`、`created_at_ms` 和 `expected_delete_*` 字段。
///
/// 文件损坏（解析失败）时返回 Err，不吞错误。
///
/// #645 评论 5504296097 问题2 修复：`expected_delete_lww_time_ms` /
/// `expected_delete_device_id` 绑定产生该 cleanup 的 Delete lifecycle identity，
/// Transfer 前重新确认远端 catalog 时校验当前 winner 仍是同一条/更新的 Delete。
pub fn record_pending_remote_cleanup(
    app_data_root: &Path,
    remote_prefix: &str,
    project_id: &str,
    last_error: &str,
    expected_delete_lww_time_ms: i64,
    expected_delete_device_id: &str,
) -> crate::error::Result<()> {
    let path = pending_remote_cleanup_path(app_data_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut current = load_pending_remote_cleanups(app_data_root)?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    if let Some(existing) = current
        .iter_mut()
        .find(|c| c.remote_prefix == remote_prefix)
    {
        // 幂等：相同 remote_prefix 只更新错误、时间和 lifecycle identity。
        existing.last_error = last_error.to_string();
        existing.created_at_ms = now_ms;
        existing.expected_delete_lww_time_ms = expected_delete_lww_time_ms;
        existing.expected_delete_device_id = expected_delete_device_id.to_string();
    } else {
        current.push(PendingRemoteTargetCleanup {
            remote_prefix: remote_prefix.to_string(),
            project_id: project_id.to_string(),
            last_error: last_error.to_string(),
            created_at_ms: now_ms,
            expected_delete_lww_time_ms,
            expected_delete_device_id: expected_delete_device_id.to_string(),
        });
    }
    let list = PendingRemoteTargetCleanupList { cleanups: current };
    let content = serde_json::to_string_pretty(&list).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "record_pending_remote_cleanup: serialize: {e}"
        )))
    })?;
    crate::storage::atomic_write_string(&path, &content)?;
    Ok(())
}

/// 移除一个已完成的待清理记录（按 `remote_prefix` 匹配）。
///
/// 用 read-modify-write + atomic write。未找到时返回 Ok(())（幂等）。
pub fn remove_pending_remote_cleanup(
    app_data_root: &Path,
    remote_prefix: &str,
) -> crate::error::Result<()> {
    let path = pending_remote_cleanup_path(app_data_root);
    if !path.exists() {
        return Ok(());
    }
    let mut current = load_pending_remote_cleanups(app_data_root)?;
    let before = current.len();
    current.retain(|c| c.remote_prefix != remote_prefix);
    if current.len() == before {
        return Ok(());
    }
    let list = PendingRemoteTargetCleanupList { cleanups: current };
    let content = serde_json::to_string_pretty(&list).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "remove_pending_remote_cleanup: serialize: {e}"
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
        record_pending_remote_cleanup(
            root,
            "projects/p1",
            "p1",
            "cleanup failed: network",
            1000,
            "dev1",
        )
        .unwrap();
        let loaded = load_pending_remote_cleanups(root).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].remote_prefix, "projects/p1");
        assert_eq!(loaded[0].project_id, "p1");
        assert_eq!(loaded[0].last_error, "cleanup failed: network");
        assert_eq!(loaded[0].expected_delete_lww_time_ms, 1000);
        assert_eq!(loaded[0].expected_delete_device_id, "dev1");

        // 幂等：相同 remote_prefix 只更新错误。
        record_pending_remote_cleanup(
            root,
            "projects/p1",
            "p1",
            "cleanup failed: timeout",
            2000,
            "dev2",
        )
        .unwrap();
        let loaded = load_pending_remote_cleanups(root).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].last_error, "cleanup failed: timeout");
        assert_eq!(loaded[0].expected_delete_lww_time_ms, 2000);
        assert_eq!(loaded[0].expected_delete_device_id, "dev2");

        // 移除后为空。
        remove_pending_remote_cleanup(root, "projects/p1").unwrap();
        let loaded = load_pending_remote_cleanups(root).unwrap();
        assert!(loaded.is_empty());

        // 幂等移除。
        remove_pending_remote_cleanup(root, "projects/p1").unwrap();
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let loaded = load_pending_remote_cleanups(tmp.path()).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn multiple_cleanups_preserved() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        record_pending_remote_cleanup(root, "projects/p1", "p1", "err1", 1000, "dev1").unwrap();
        record_pending_remote_cleanup(root, "projects/p2", "p2", "err2", 2000, "dev2").unwrap();
        let loaded = load_pending_remote_cleanups(root).unwrap();
        assert_eq!(loaded.len(), 2);

        remove_pending_remote_cleanup(root, "projects/p1").unwrap();
        let loaded = load_pending_remote_cleanups(root).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].remote_prefix, "projects/p2");
    }

    #[test]
    fn corrupt_file_returns_err() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let path = pending_remote_cleanup_path(root);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not json").unwrap();
        let result = load_pending_remote_cleanups(root);
        assert!(result.is_err());
    }
}
