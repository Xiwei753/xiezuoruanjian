//! commit_helpers.rs 的单元测试。
//!
//! 按仓库结构门禁（`tools/check_source_structure.py` 的
//! production-test-bloat 规则）从生产文件拆出，测试逻辑保持不变。
use super::*;
use crate::sync::staging::CommitAction;
use crate::sync::types::{SyncConflict, SyncConflictKind, SyncState};
use tempfile::TempDir;

/// Issue #762 评论 5831990584 问题 2 复现：
/// `apply_commit_plan_with_sync_state_merge()` 开头的 early return：
/// ```ignore
/// if content_actions.is_empty() && engine_state_actions.is_empty() && !backup_mode {
///     return Ok(SaveTransaction::new(live_root));
/// }
/// ```
/// 会把已经算好的 `merged_state` / `merged_conflicts` 静默丢掉。
///
/// 进入这个函数本身就说明 `plan.needs_sync_state_merge == true`，
/// merged_state / merged_conflicts 是需要提交的动作。即使没有正文 Apply/Delete、
/// manifest 也没动作，只要 state.local.json / conflicts.json 的三方结果
/// 发生了变化，这个 early return 会跳过写回，让 live 的 state/conflicts
/// 停留在旧值。
///
/// 修复前：本测试在读取 live state.local.json 的断言处失败——
/// 文件内容仍是 live 旧值，merged state 被丢掉。
#[test]
fn empty_actions_must_still_persist_merged_sync_state() {
    let tmp = TempDir::new().unwrap();
    let live_root = tmp.path().join("live");
    std::fs::create_dir_all(live_root.join("app-meta/sync")).unwrap();

    // live 旧状态：device_id = "live-device"，known_files 空。
    let mut live_state = SyncState::default();
    live_state.device_id = "live-device".to_string();
    let live_state_json = serde_json::to_string_pretty(&live_state).unwrap();
    std::fs::write(
        live_root.join("app-meta/sync/state.local.json"),
        live_state_json,
    )
    .unwrap();
    std::fs::write(live_root.join("app-meta/sync/conflicts.json"), "[]").unwrap();

    // merged_state 与 live 不同：device_id = "merged-device"。
    let mut merged_state = SyncState::default();
    merged_state.device_id = "merged-device".to_string();
    merged_state
        .known_files
        .insert("a.md".to_string(), "hash-a".to_string());

    // merged_conflicts 非空，和 live 的空 conflicts 不同。
    let merged_conflict = SyncConflict {
        local_path: "a.md".to_string(),
        remote_path: "a.md".to_string(),
        kind: SyncConflictKind::BothChanged,
        local_hash: "local".to_string(),
        remote_hash: "remote".to_string(),
        base_hash: "base".to_string(),
        created_at: 1,
        description: "merged".to_string(),
        remote_snapshot_path: None,
    };
    let merged_conflicts = vec![merged_conflict];

    // content_actions=[]、engine_state_actions=[]、backup_mode=false
    // → 命中当前代码的 early return，merged state 被丢掉。
    let content_actions: &[CommitAction] = &[];
    let engine_state_actions: &[CommitAction] = &[];
    let mut tx = apply_commit_plan_with_sync_state_merge(
        &live_root,
        content_actions,
        engine_state_actions,
        &merged_state,
        &merged_conflicts,
        false,
    )
    .expect("apply_commit_plan_with_sync_state_merge 不应返回 Err");
    tx.finish().expect("tx.finish 不应失败");

    // 验证 live 的 state.local.json 已更新为 merged_state。
    let live_state_path = live_root.join("app-meta/sync/state.local.json");
    assert!(
        live_state_path.exists(),
        "state.local.json 应被写入（merged state 必须落盘）"
    );
    let content = std::fs::read_to_string(&live_state_path).unwrap();
    let persisted: SyncState = serde_json::from_str(&content).unwrap();
    assert_eq!(
        persisted.device_id, "merged-device",
        "live 的 state.local.json 必须更新为 merged_state 的 device_id；\
         当前（未修复）代码因 early return 仍是 live 旧值 'live-device'"
    );
    assert_eq!(
        persisted.known_files.get("a.md").map(String::as_str),
        Some("hash-a"),
        "live 的 state.local.json 必须包含 merged_state 的 known_files"
    );

    // 验证 live 的 conflicts.json 已更新为 merged_conflicts。
    let conflicts_path = live_root.join("app-meta/sync/conflicts.json");
    assert!(
        conflicts_path.exists(),
        "conflicts.json 应被写入（merged conflicts 必须落盘）"
    );
    let conflicts_content = std::fs::read_to_string(&conflicts_path).unwrap();
    let persisted_conflicts: Vec<SyncConflict> = serde_json::from_str(&conflicts_content).unwrap();
    assert!(
        persisted_conflicts.iter().any(|c| c.local_path == "a.md"),
        "live 的 conflicts.json 必须更新为 merged_conflicts；\
         当前（未修复）代码因 early return 仍是 live 旧值（空）"
    );
}
