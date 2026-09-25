//! Issue #762 评论 5830266600：project sync runtime state 的三方语义合并。
//!
//! `compute_commit_plan` 把 `state.local.json` 和 `conflicts.json` 当成
//! `EngineState` 直接 apply incoming（staging 的值），无三方比较。当用户在
//! 同步运行期间（target1 Transfer 后、Commit 前）从已可见的冲突入口解决冲突 A
//! 时，target1 Commit 会把 staging 里的旧 state/conflicts 直接覆盖回 live，
//! 导致用户解决的冲突"复活"。
//!
//! 本模块对 `state.local.json` + `conflicts.json` 做三方语义合并：
//! - base = Seed 时的状态（staging run 的 `base_root` 下）
//! - live = Commit 当下最新状态（`live_root` 下）
//! - staging = Transfer 后状态（`staging_root` 下）
//!
//! 合并规则（对每个冲突路径）：
//! - base 有、live 已删、staging 仍有 → 用户在同步期间已解决，以 live 的解决结果为准，
//!   不得复活。
//! - base 无、staging 新增 → 本轮新冲突，写入 live。
//! - base/live/staging 都有 → 保留当前 unresolved。
//!
//! `manifest.sync.json` 不走本模块，继续按同步结果直接写回（由 `EngineState` 处理）。

use std::collections::HashSet;
use std::path::Path;

use crate::error::Result;
use crate::sync::conflict::load_conflicts_json;
use crate::sync::types::{SyncConflict, SyncState};

/// 三方语义合并 `SyncState` + `conflicts`。
///
/// - `base_root` = Seed 时的冲突/SyncState（staging run 的 `base_root` 下）
/// - `live_root` = Commit 当下最新状态（`live_root` 下）
/// - `staging_root` = Transfer 后状态（`staging_root` 下）
///
/// 返回合并后的 `(SyncState, Vec<SyncConflict>)`，由调用方在同一
/// `SaveTransaction` 里提交。
///
/// # 合并规则
///
/// 对每个冲突路径 `p`：
/// - **base 有、live 已删（不在 `live.conflicted_files`）、staging 仍有**
///   → 用户在同步期间已解决，**以 live 的解决结果为准，不得复活**。
/// - **base 无、staging 新增** → 本轮新冲突，写入 live。
/// - **base/live/staging 都有** → 保留当前 unresolved。
///
/// 对被用户解决的路径，`known_files` / `known_files_updated_at` /
/// `pending_take_remote` / `conflicted_files` / `conflicts` 必须保留 live
/// 当前值，不能再拿 staging 的旧基线覆盖。
///
/// # 三方读取失败回退
///
/// - base 不存在 → 视为空状态（所有 staging 冲突都是"新增"）。
/// - live 不存在 → 视为空状态。
/// - staging 不存在 → 直接返回 live 状态（无变更）。
pub(crate) fn merge_sync_state_three_way(
    base_root: &Path,
    live_root: &Path,
    staging_root: &Path,
) -> Result<(SyncState, Vec<SyncConflict>)> {
    // 读取三方 SyncState。文件不存在/损坏时回退 SyncState::default()。
    let base_state = read_sync_state_or_default(base_root);
    let live_state = read_sync_state_or_default(live_root);

    // staging 的 state.local.json 不存在 → Transfer 没产生新状态，
    // 直接返回 live 状态（无变更）。
    let staging_state_path = staging_root.join("app-meta/sync/state.local.json");
    if !staging_state_path.exists() {
        let live_conflicts = load_conflicts_json(live_root).unwrap_or_default();
        return Ok((live_state, live_conflicts));
    }
    let staging_state = read_sync_state_or_default(staging_root);

    // 读取三方 conflicts.json。文件不存在/损坏时回退空 Vec。
    // base/live 的 conflicts.json 不需要读取——合并只依赖 base_state.conflicted_files
    // 和 live_state.conflicted_files（SyncState 内的 HashSet）来判断哪些路径
    // 被用户解决，以及 staging_conflicts 作为合并基准。
    let staging_conflicts = load_conflicts_json(staging_root).unwrap_or_default();

    // 计算"用户在同步期间已解决"的路径集合：
    // `p in base.conflicted_files && p not in live.conflicted_files`
    // 这些路径的解决结果必须保留 live 值，不能被 staging 旧基线覆盖。
    let user_resolved_paths: HashSet<String> = base_state
        .conflicted_files
        .iter()
        .filter(|p| !live_state.conflicted_files.contains(*p))
        .cloned()
        .collect();

    // 合并 conflicted_files：以 staging 为基准（同步结果），
    // 移除"base 有、live 已删"的路径（用户已解决）。
    let merged_conflicted_files: HashSet<String> = staging_state
        .conflicted_files
        .iter()
        .filter(|p| !user_resolved_paths.contains(*p))
        .cloned()
        .collect();

    // 合并 conflicts.json（Vec<SyncConflict>）：同上按 local_path 合并。
    // staging 的冲突列表，移除"base 有、live 已删"的路径。
    let merged_conflicts_json: Vec<SyncConflict> = staging_conflicts
        .iter()
        .filter(|c| !user_resolved_paths.contains(&c.local_path))
        .cloned()
        .collect();

    // 合并 known_files：以 staging 为基准（同步结果更新了基线），
    // 但对"用户已解决冲突"的路径保留 live 值（用户 resolve 后的 known_files）。
    let mut merged_known_files = staging_state.known_files.clone();
    for p in &user_resolved_paths {
        if let Some(v) = live_state.known_files.get(p) {
            merged_known_files.insert(p.clone(), v.clone());
        } else {
            merged_known_files.remove(p);
        }
    }

    // 合并 known_files_updated_at：同 known_files 逻辑。
    let mut merged_known_files_updated_at = staging_state.known_files_updated_at.clone();
    for p in &user_resolved_paths {
        if let Some(t) = live_state.known_files_updated_at.get(p) {
            merged_known_files_updated_at.insert(p.clone(), *t);
        } else {
            merged_known_files_updated_at.remove(p);
        }
    }

    // pending_take_remote：保留 live 值（用户操作），不拿 staging 覆盖。
    // 用户的 take_remote 排队不能被 staging 覆盖。
    let merged_pending_take_remote = live_state.pending_take_remote.clone();

    // device_id：保留 live 值（设备身份不能被 staging 覆盖）。
    let merged_device_id = live_state.device_id.clone();

    // tombstones / deleted_files：按 staging 同步结果合并（同步引擎管理的）。
    let merged_tombstones = staging_state.tombstones.clone();
    let merged_deleted_files = staging_state.deleted_files.clone();

    // last_sync_time / last_error：用 staging 值（同步结果）。
    let merged_last_sync_time = staging_state.last_sync_time;
    let merged_last_error = staging_state.last_error.clone();

    // 合并 state.conflicts（Vec<SyncConflict>）：同 conflicts.json 逻辑。
    let merged_state_conflicts: Vec<SyncConflict> = staging_state
        .conflicts
        .iter()
        .filter(|c| !user_resolved_paths.contains(&c.local_path))
        .cloned()
        .collect();

    let merged_state = SyncState {
        last_sync_time: merged_last_sync_time,
        last_error: merged_last_error,
        known_files: merged_known_files,
        conflicts: merged_state_conflicts,
        tombstones: merged_tombstones,
        deleted_files: merged_deleted_files,
        device_id: merged_device_id,
        known_files_updated_at: merged_known_files_updated_at,
        conflicted_files: merged_conflicted_files,
        pending_take_remote: merged_pending_take_remote,
    };

    Ok((merged_state, merged_conflicts_json))
}

/// 读取 `app-meta/sync/state.local.json`，文件不存在/损坏时回退 `SyncState::default()`。
///
/// 只读，不做旧格式迁移、不写文件（与 `SyncService::load_sync_state` 不同，
/// 后者可能落盘迁移结果）。合并阶段不应有副作用。
fn read_sync_state_or_default(root: &Path) -> SyncState {
    let state_path = root.join("app-meta/sync/state.local.json");
    if !state_path.exists() {
        return SyncState::default();
    }
    let content = match std::fs::read_to_string(&state_path) {
        Ok(c) => c,
        Err(_) => return SyncState::default(),
    };
    serde_json::from_str(&content).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::types::{SyncConflict, SyncConflictKind, SyncState};
    use tempfile::TempDir;

    /// 写入 SyncState 到 `root/app-meta/sync/state.local.json`。
    fn write_state(root: &Path, state: &SyncState) {
        let dir = root.join("app-meta/sync");
        std::fs::create_dir_all(&dir).unwrap();
        let json = serde_json::to_string_pretty(state).unwrap();
        std::fs::write(dir.join("state.local.json"), json).unwrap();
    }

    /// 写入 conflicts 到 `root/app-meta/sync/conflicts.json`。
    fn write_conflicts(root: &Path, conflicts: &[SyncConflict]) {
        let dir = root.join("app-meta/sync");
        std::fs::create_dir_all(&dir).unwrap();
        let json = serde_json::to_string_pretty(conflicts).unwrap();
        std::fs::write(dir.join("conflicts.json"), json).unwrap();
    }

    fn make_conflict(path: &str) -> SyncConflict {
        SyncConflict {
            local_path: path.to_string(),
            remote_path: path.to_string(),
            kind: SyncConflictKind::BothChanged,
            local_hash: "local".to_string(),
            remote_hash: "remote".to_string(),
            base_hash: "base".to_string(),
            created_at: 1,
            description: "test".to_string(),
            remote_snapshot_path: None,
        }
    }

    fn make_state(conflicted: &[&str], known: &[(&str, &str)]) -> SyncState {
        let mut state = SyncState::default();
        for p in conflicted {
            state.conflicted_files.insert(p.to_string());
        }
        for (p, h) in known {
            state.known_files.insert(p.to_string(), h.to_string());
        }
        state
    }

    /// base 有冲突 A，live 已解决（无 A），staging 仍有 A → 不复活。
    #[test]
    fn user_resolved_conflict_is_not_revived() {
        let tmp = TempDir::new().unwrap();
        let base_root = tmp.path().join("base");
        let live_root = tmp.path().join("live");
        let staging_root = tmp.path().join("staging");

        let conflict_a = make_conflict("a.md");
        write_state(&base_root, &make_state(&["a.md"], &[("a.md", "base-hash")]));
        write_conflicts(&base_root, std::slice::from_ref(&conflict_a));

        // live: 用户已解决 A，known_files 更新为 remote_hash
        write_state(&live_root, &make_state(&[], &[("a.md", "remote-hash")]));
        write_conflicts(&live_root, &[]);

        // staging: 仍有旧冲突 A（seed 时复制）
        write_state(
            &staging_root,
            &make_state(&["a.md"], &[("a.md", "base-hash")]),
        );
        write_conflicts(&staging_root, std::slice::from_ref(&conflict_a));

        let (merged_state, merged_conflicts) =
            merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

        assert!(
            !merged_state.conflicted_files.contains("a.md"),
            "用户已解决的冲突不应复活"
        );
        assert!(
            merged_conflicts.iter().all(|c| c.local_path != "a.md"),
            "conflicts.json 中不应有已解决的冲突"
        );
        assert_eq!(
            merged_state.known_files.get("a.md").map(String::as_str),
            Some("remote-hash"),
            "known_files 应保留 live 的解决结果，不被 staging 旧值覆盖"
        );
    }

    /// base 无冲突，staging 新增冲突 B → 写入 live。
    #[test]
    fn new_conflict_from_staging_is_written() {
        let tmp = TempDir::new().unwrap();
        let base_root = tmp.path().join("base");
        let live_root = tmp.path().join("live");
        let staging_root = tmp.path().join("staging");

        let conflict_b = make_conflict("b.md");
        write_state(&base_root, &SyncState::default());
        write_conflicts(&base_root, &[]);

        write_state(&live_root, &SyncState::default());
        write_conflicts(&live_root, &[]);

        write_state(
            &staging_root,
            &make_state(&["b.md"], &[("b.md", "staging-hash")]),
        );
        write_conflicts(&staging_root, std::slice::from_ref(&conflict_b));

        let (merged_state, merged_conflicts) =
            merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

        assert!(
            merged_state.conflicted_files.contains("b.md"),
            "staging 新增冲突应写入 live"
        );
        assert!(
            merged_conflicts.iter().any(|c| c.local_path == "b.md"),
            "conflicts.json 应包含新冲突"
        );
    }

    /// base/live/staging 都有冲突 → 保留 unresolved。
    #[test]
    fn unresolved_conflict_is_preserved() {
        let tmp = TempDir::new().unwrap();
        let base_root = tmp.path().join("base");
        let live_root = tmp.path().join("live");
        let staging_root = tmp.path().join("staging");

        let conflict_c = make_conflict("c.md");
        write_state(&base_root, &make_state(&["c.md"], &[("c.md", "base-hash")]));
        write_conflicts(&base_root, std::slice::from_ref(&conflict_c));

        write_state(&live_root, &make_state(&["c.md"], &[("c.md", "base-hash")]));
        write_conflicts(&live_root, std::slice::from_ref(&conflict_c));

        write_state(
            &staging_root,
            &make_state(&["c.md"], &[("c.md", "base-hash")]),
        );
        write_conflicts(&staging_root, std::slice::from_ref(&conflict_c));

        let (merged_state, merged_conflicts) =
            merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

        assert!(
            merged_state.conflicted_files.contains("c.md"),
            "未解决的冲突应保留"
        );
        assert!(
            merged_conflicts.iter().any(|c| c.local_path == "c.md"),
            "conflicts.json 应保留未解决冲突"
        );
    }

    /// staging 不存在 → 返回 live 状态。
    #[test]
    fn staging_missing_returns_live() {
        let tmp = TempDir::new().unwrap();
        let base_root = tmp.path().join("base");
        let live_root = tmp.path().join("live");
        let staging_root = tmp.path().join("staging");

        write_state(&base_root, &SyncState::default());
        write_conflicts(&base_root, &[]);

        let live_state = make_state(&["x.md"], &[("x.md", "live-hash")]);
        write_state(&live_root, &live_state);
        write_conflicts(&live_root, &[]);

        // staging 目录存在但无 state.local.json
        std::fs::create_dir_all(staging_root.join("app-meta/sync")).unwrap();

        let (merged_state, merged_conflicts) =
            merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

        assert!(
            merged_state.conflicted_files.contains("x.md"),
            "staging 不存在时应返回 live 状态"
        );
        assert!(merged_conflicts.is_empty());
    }

    /// pending_take_remote 保留 live 值，不被 staging 覆盖。
    #[test]
    fn pending_take_remote_preserves_live() {
        let tmp = TempDir::new().unwrap();
        let base_root = tmp.path().join("base");
        let live_root = tmp.path().join("live");
        let staging_root = tmp.path().join("staging");

        write_state(&base_root, &SyncState::default());
        write_conflicts(&base_root, &[]);

        let mut live_state = SyncState::default();
        live_state
            .pending_take_remote
            .insert("pending.md".to_string());
        write_state(&live_root, &live_state);
        write_conflicts(&live_root, &[]);

        let staging_state = SyncState::default();
        // staging 不应有 pending_take_remote
        write_state(&staging_root, &staging_state);
        write_conflicts(&staging_root, &[]);

        let (merged_state, _) =
            merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

        assert!(
            merged_state.pending_take_remote.contains("pending.md"),
            "pending_take_remote 应保留 live 值"
        );
    }

    /// device_id 保留 live 值，不被 staging 覆盖。
    #[test]
    fn device_id_preserves_live() {
        let tmp = TempDir::new().unwrap();
        let base_root = tmp.path().join("base");
        let live_root = tmp.path().join("live");
        let staging_root = tmp.path().join("staging");

        write_state(&base_root, &SyncState::default());
        write_conflicts(&base_root, &[]);

        let mut live_state = SyncState::default();
        live_state.device_id = "live-device".to_string();
        write_state(&live_root, &live_state);
        write_conflicts(&live_root, &[]);

        let mut staging_state = SyncState::default();
        staging_state.device_id = "staging-device".to_string();
        write_state(&staging_root, &staging_state);
        write_conflicts(&staging_root, &[]);

        let (merged_state, _) =
            merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

        assert_eq!(
            merged_state.device_id, "live-device",
            "device_id 应保留 live 值"
        );
    }
}
