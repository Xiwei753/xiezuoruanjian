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
    // 读取三方 SyncState。严格区分"文件不存在"（首次同步，回退 default 空状态
    // 参与合并）与"文件存在但损坏"（必须返回 Err 让该 target Commit 失败，
    // 不能伪装成随机 device_id 的默认状态，参见 Issue #762 评论 5831990584）。
    let base_state = read_sync_state(base_root)?.unwrap_or_default();
    let live_state = read_sync_state(live_root)?.unwrap_or_default();

    // staging 的 state.local.json 不存在 → Transfer 没产生新状态，
    // 直接返回 live 状态（无变更）。
    let staging_state_path = staging_root.join("app-meta/sync/state.local.json");
    if !staging_state_path.exists() {
        let live_conflicts = load_conflicts_json(live_root).unwrap_or_default();
        return Ok((live_state, live_conflicts));
    }
    // staging_state_path 已确认存在；read_sync_state 返回 Err 则传播（staging
    // 损坏必须让该 target Commit 失败）。理论上 None 不会到达（已确认 exists），
    // 但为防御 TOCTOU 竞态（文件在 exists() 与 read 之间被删），回退 default
    // 空状态——等价于"staging 不存在"语义，不引入随机 device_id。
    let staging_state = read_sync_state(staging_root)?.unwrap_or_default();

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

    // pending_take_remote：按 path 做三方集合合并，不能整套 HashSet 直接复制 live。
    // 对每个 path：live 相对 base 的 membership 发生变化 → 取 live（用户在同步期间改了）；
    // 否则取 staging（让 Transfer 对旧 pending 的成功消费/失败保留正常生效）。
    // 即 live_has != base_has ? live_has : staging_has。
    let merged_pending_take_remote: HashSet<String> = {
        let all_paths: HashSet<&String> = base_state
            .pending_take_remote
            .iter()
            .chain(live_state.pending_take_remote.iter())
            .chain(staging_state.pending_take_remote.iter())
            .collect();
        let mut merged = HashSet::new();
        for p in all_paths {
            let base_has = base_state.pending_take_remote.contains(p);
            let live_has = live_state.pending_take_remote.contains(p);
            let staging_has = staging_state.pending_take_remote.contains(p);
            let keep = if live_has != base_has {
                // live 相对 base 改了 → 取 live 的 membership
                live_has
            } else {
                // live 没改 → 取 staging 的 membership（Transfer 消费结果）
                staging_has
            };
            if keep {
                merged.insert(p.clone());
            }
        }
        merged
    };

    let merged_device_id = merge_device_id_three_way(
        base_root,
        live_root,
        &base_state,
        &live_state,
        &staging_state,
    )?;

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

/// device_id 三方合并：按"state.local.json 存在且 device_id 字段非空"选择，
/// 不能只看文件是否存在，也不能只看字段是否非空。
/// - 只看文件存在：漏掉"文件存在但 device_id 为空"的合法旧数据/迁移输入
///   （config_store.rs::load_sync_state_with_preferred_device_id 明确支持这种补齐），
///   staging 已被 Transfer 用 preferred device_id 修成稳定值时会被 live 的空值覆盖
///   （评论 5831349330 的场景）。
/// - 只看字段非空：漏掉"文件不存在时 read_sync_state 返回 None，调用方回退
///   default()，其 device_id 是随机 UUID（非空但不是真实设备身份）"，会把首次同步
///   时 staging Transfer 生成的稳定 device_id 丢掉（#761 回归）。
///
/// 有效 device_id = state.local.json 存在 且 device_id 字段非空。
/// - live 有效 → 用 live（真实用户状态）
/// - live 无效、staging 有效 → 用 staging（Transfer 生成/修复的稳定值）
/// - live/staging 都无效 → 看 base
/// - 三方都拿不到有效 device_id → 返回错误，让正式同步路径处理，
///   不静默制造另一套设备身份。
fn merge_device_id_three_way(
    base_root: &Path,
    live_root: &Path,
    base_state: &SyncState,
    live_state: &SyncState,
    staging_state: &SyncState,
) -> Result<String> {
    let live_state_path = live_root.join("app-meta/sync/state.local.json");
    let base_state_path = base_root.join("app-meta/sync/state.local.json");
    if live_state_path.exists() && !live_state.device_id.is_empty() {
        Ok(live_state.device_id.clone())
    } else if !staging_state.device_id.is_empty() {
        // staging_state_path 在调用方已确认存在（不存在则早期返回），
        // 此处只需检查字段非空。
        Ok(staging_state.device_id.clone())
    } else if base_state_path.exists() && !base_state.device_id.is_empty() {
        Ok(base_state.device_id.clone())
    } else {
        Err(crate::Error::Io(std::io::Error::other(
            "merge_sync_state_three_way: no valid device_id in base/live/staging \
             (all empty or missing); cannot silently fabricate a new device identity"
                .to_string(),
        )))
    }
}

/// 读取 `app-meta/sync/state.local.json`，严格区分三态。
///
/// - 文件不存在 → `Ok(None)`（首次同步语义，调用方按需回退 `SyncState::default()`）
/// - 文件存在且合法 → `Ok(Some(state))`
/// - read 失败 / JSON parse 失败 → `Err`（带清晰错误信息，损坏必须让该 target
///   Commit 失败，不能像旧 `read_sync_state_or_default` 那样静默回退
///   `SyncState::default()`——后者的 device_id 是随机 UUID，会把损坏文件
///   伪装成一份新的随机设备状态参与三方判断，参见 Issue #762 评论 5831990584）
///
/// 只读，不做旧格式迁移、不写文件（与 `SyncService::load_sync_state` 不同，
/// 后者可能落盘迁移结果）。合并阶段不应有副作用。
///
/// 错误格式参考 `config_store.rs::load_sync_state`（行 335-343）。
fn read_sync_state(root: &Path) -> Result<Option<SyncState>> {
    let state_path = root.join("app-meta/sync/state.local.json");
    if !state_path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&state_path).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "read_sync_state: state.local.json read failed at {}: {e}",
            state_path.display()
        )))
    })?;
    let state: SyncState = serde_json::from_str(&content).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "read_sync_state: state.local.json parse failed at {}: {e}",
            state_path.display()
        )))
    })?;
    Ok(Some(state))
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

    /// 真实首次同步：live/base 无 state（device_id 空），staging Transfer 生成稳定
    /// device_id → 合并结果必须使用 staging 的 device_id，不能因为 live 优先而丢掉。
    ///
    /// 生产顺序：live 还没有 state.local.json → seed 后 base/live 都没 state →
    /// Transfer 在 staging 生成 state.local.json（#761 保证用平台稳定 device_id）→
    /// Commit 三方 merge。修复前合并器无条件选 live（default 的随机 UUID）把 staging
    /// 的稳定 device_id 丢掉，把 #761 的首次同步 device identity 修复打回去。
    #[test]
    fn device_id_first_sync_uses_staging_when_live_empty() {
        let tmp = TempDir::new().unwrap();
        let base_root = tmp.path().join("base");
        let live_root = tmp.path().join("live");
        let staging_root = tmp.path().join("staging");

        // base/live 都没有 state.local.json（首次同步）
        // read_sync_state_or_default 返回 default（device_id 是随机 UUID，非空但无意义）
        std::fs::create_dir_all(base_root.join("app-meta/sync")).unwrap();
        std::fs::create_dir_all(live_root.join("app-meta/sync")).unwrap();

        // staging: Transfer 生成的稳定 device_id
        let mut staging_state = SyncState::default();
        staging_state.device_id = "stable-device-from-transfer".to_string();
        write_state(&staging_root, &staging_state);
        write_conflicts(&staging_root, &[]);

        let (merged_state, _) =
            merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

        assert_eq!(
            merged_state.device_id, "stable-device-from-transfer",
            "首次同步时 live 无 state，合并结果必须使用 staging 生成的稳定 device_id"
        );
    }

    /// live 有 state.local.json 但 device_id 为空（合法旧数据/迁移输入），
    /// staging 已被 Transfer 修成稳定 device_id → 合并结果必须用 staging 的稳定值，
    /// 不能因为 live state 文件"存在"就选 live 的空 device_id。
    ///
    /// 生产顺序：live 已有旧 state.local.json（device_id=""）；seed 复制到 base/staging；
    /// Transfer 在 staging 用 preferred device_id 修成稳定值；live 仍为空；
    /// Commit 进入 merge。修复前因 live state 文件存在直接选 live_state.device_id=""，
    /// 把 staging 修好的稳定 device_id 覆盖成空。
    #[test]
    fn device_id_uses_staging_when_live_file_exists_but_field_empty() {
        let tmp = TempDir::new().unwrap();
        let base_root = tmp.path().join("base");
        let live_root = tmp.path().join("live");
        let staging_root = tmp.path().join("staging");

        // base: 旧数据，state.local.json 存在但 device_id 为空
        let mut base_state = SyncState::default();
        base_state.device_id = "".to_string();
        write_state(&base_root, &base_state);
        write_conflicts(&base_root, &[]);

        // live: 旧数据，state.local.json 存在但 device_id 为空
        let mut live_state = SyncState::default();
        live_state.device_id = "".to_string();
        write_state(&live_root, &live_state);
        write_conflicts(&live_root, &[]);

        // staging: Transfer 已用 preferred device_id 修成稳定值
        let mut staging_state = SyncState::default();
        staging_state.device_id = "stable-device".to_string();
        write_state(&staging_root, &staging_state);
        write_conflicts(&staging_root, &[]);

        // 确认 base/live 的 state.local.json 都真实存在（测试前提）
        assert!(
            base_root.join("app-meta/sync/state.local.json").exists(),
            "测试前提：base 应有 state.local.json"
        );
        assert!(
            live_root.join("app-meta/sync/state.local.json").exists(),
            "测试前提：live 应有 state.local.json"
        );

        let (merged_state, _) =
            merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

        assert_eq!(
            merged_state.device_id, "stable-device",
            "live state 文件存在但 device_id 为空时，合并结果必须使用 staging 的稳定 device_id，\
             不能因为文件存在就选 live 的空值"
        );
    }

    /// Transfer 成功消费 pending 后不能复活：base={A}, live={A}, staging={} → merged 必须空。
    ///
    /// 生产顺序：同步前 live 已有旧兼容数据 pending_take_remote={A}；seed 后 base/staging
    /// 也有 A；Transfer 在 staging 成功下载 A 并按 LWW 把 A 从 staging pending 移除；
    /// live 没有并发用户改动仍为 {A}。修复前合并器无条件复制 live={A}，A 被重新写回
    /// pending，下一轮重复执行 take_remote。
    #[test]
    fn pending_take_remote_consumed_by_transfer_not_revived() {
        let tmp = TempDir::new().unwrap();
        let base_root = tmp.path().join("base");
        let live_root = tmp.path().join("live");
        let staging_root = tmp.path().join("staging");

        // base = {A}
        let mut base_state = SyncState::default();
        base_state.pending_take_remote.insert("a.md".to_string());
        write_state(&base_root, &base_state);
        write_conflicts(&base_root, &[]);

        // live = {A}（用户没并发改动，与 base 一致）
        let mut live_state = SyncState::default();
        live_state.pending_take_remote.insert("a.md".to_string());
        write_state(&live_root, &live_state);
        write_conflicts(&live_root, &[]);

        // staging = {}（Transfer 成功消费 A，从 pending 移除）
        write_state(&staging_root, &SyncState::default());
        write_conflicts(&staging_root, &[]);

        let (merged_state, _) =
            merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

        assert!(
            merged_state.pending_take_remote.is_empty(),
            "Transfer 成功消费的 pending 不应被 live 无条件复制复活"
        );
    }

    /// Issue #762 评论 5831990584 问题 1 复现：
    /// live 的 state.local.json 真实存在但写入非法 JSON 时，
    /// `read_sync_state_or_default()` 会通过 `unwrap_or_default()` 回退到
    /// `SyncState::default()`，其 device_id 是随机 UUID（非空但无意义）。
    /// `merge_device_id_three_way()` 看到 live 文件存在且 device_id 非空，
    /// 会把这个随机 UUID 当成"有效 live device_id"返回，导致
    /// `merge_sync_state_three_way()` 错误地返回 Ok（伪装的随机设备状态）。
    ///
    /// 正式加载逻辑 `config_store.rs::load_sync_state_with_preferred_device_id`
    /// 对"文件存在但解析失败"明确返回 Err。合并阶段也应返回 Err，而不是
    /// 静默把损坏文件伪装成一份新的随机设备状态参与三方判断。
    ///
    /// 修复前：本测试在 `assert!(result.is_err())` 处失败——
    /// `merge_sync_state_three_way` 返回 Ok，device_id 是随机 UUID。
    #[test]
    fn corrupt_live_state_json_must_return_err_not_fabricate_random_device_id() {
        let tmp = TempDir::new().unwrap();
        let base_root = tmp.path().join("base");
        let live_root = tmp.path().join("live");
        let staging_root = tmp.path().join("staging");

        // base: 合法的 state（device_id 稳定）
        let mut base_state = SyncState::default();
        base_state.device_id = "base-device".to_string();
        write_state(&base_root, &base_state);
        write_conflicts(&base_root, &[]);

        // live: state.local.json 真实存在但内容是非法 JSON
        let live_sync_dir = live_root.join("app-meta/sync");
        std::fs::create_dir_all(&live_sync_dir).unwrap();
        std::fs::write(live_sync_dir.join("state.local.json"), "{not valid json").unwrap();
        // 确认测试前提：live 的 state.local.json 真实存在
        assert!(
            live_root.join("app-meta/sync/state.local.json").exists(),
            "测试前提：live 应有损坏的 state.local.json"
        );

        // staging: 合法的 state（device_id 稳定），避免 staging 不存在时 early return
        let mut staging_state = SyncState::default();
        staging_state.device_id = "staging-device".to_string();
        write_state(&staging_root, &staging_state);
        write_conflicts(&staging_root, &[]);

        let result = merge_sync_state_three_way(&base_root, &live_root, &staging_root);

        // 期望：live 的 state.local.json 存在但解析失败 → 返回 Err
        // 当前（未修复）代码：返回 Ok，device_id 是随机 UUID → 此断言失败
        assert!(
            result.is_err(),
            "live 的 state.local.json 存在但 JSON 损坏时，merge_sync_state_three_way 必须返回 Err，\
             不能把损坏文件伪装成一份随机 device_id 的默认状态参与三方合并。\
             实际得到：{:?}",
            result.as_ref().err().map(|e| e.to_string())
        );

        // 若修复后返回 Err，进一步验证错误信息提及解析失败（可选，不强制措辞）。
        if let Err(e) = result {
            let msg = e.to_string();
            assert!(
                msg.contains("parse") || msg.contains("json") || msg.contains("state.local.json"),
                "错误信息应提及 state.local.json 解析失败，实际：{msg}"
            );
        }
    }
}
