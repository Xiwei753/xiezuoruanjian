//! Tests for aggregation/protocol error functions.
//!
//! 覆盖 #928 第 4 点：PartialConflict 的语义化摘要与“失败 target 不回滚
//! 其他 target 统计”的聚合语义。

use crate::sync::full_sync::aggregate::aggregate_full_sync_result;
use crate::sync::types::{
    LocalLifecycleCommitAction, SyncConflict, SyncConflictKind, SyncResult, SyncStatus,
    TargetSyncResult,
};

fn conflict(path: &str) -> SyncConflict {
    SyncConflict {
        local_path: path.to_string(),
        remote_path: path.to_string(),
        local_hash: "local-hash".to_string(),
        remote_hash: "remote-hash".to_string(),
        base_hash: "base-hash".to_string(),
        created_at: 0,
        description: "conflict".to_string(),
        kind: SyncConflictKind::BothChanged,
        remote_snapshot_path: None,
        remote_graph_snapshot_dir: None,
    }
}

fn target(target_kind: &str, remote_prefix: &str, result: SyncResult) -> TargetSyncResult {
    TargetSyncResult {
        target_kind: target_kind.to_string(),
        project_id: None,
        remote_prefix: remote_prefix.to_string(),
        result,
        deleted_resolution: None,
        local_lifecycle_action: LocalLifecycleCommitAction::None,
    }
}

/// #928 第 4 点：PartialConflict 且 dominant 无具体 error 时，合成
/// “非冲突文件已同步、N 个冲突待处理”的语义化摘要，并指出冲突 target。
#[test]
fn partial_conflict_without_error_gets_aggregate_summary() {
    let mut conflicted = SyncResult::success();
    conflicted.status = SyncStatus::PartialConflict;
    conflicted.uploaded_files = vec!["volumes/v1/chapter.md".to_string()];
    conflicted.conflicts = vec![
        conflict("volumes/v1/chapter.md"),
        conflict("characters/a.json"),
    ];

    let result = aggregate_full_sync_result(vec![
        target("live_project", "projects/p1", conflicted),
        target("app", "app", SyncResult::no_changes()),
    ]);

    assert_eq!(result.overall_status, SyncStatus::PartialConflict);
    assert_eq!(result.total_conflicts, 2);
    let error = result
        .error
        .expect("PartialConflict must synthesize a summary error");
    assert!(
        error.contains("non-conflicting files synced (1 target(s) completed)"),
        "摘要必须说明非冲突文件已同步，实际: {error}"
    );
    assert!(
        error.contains("2 conflict(s) pending on target(s): projects/p1"),
        "摘要必须说明冲突数量与冲突 target，实际: {error}"
    );
    assert_eq!(
        result.message_key.as_deref(),
        Some("sync.result.partial_conflict_summary")
    );
}

/// dominant target 自己带具体 error 时保留它，不被聚合摘要覆盖。
#[test]
fn partial_conflict_with_target_error_keeps_specific_error() {
    let mut conflicted = SyncResult::error(
        SyncStatus::PartialConflict,
        "remote API rate limited while publishing generation".to_string(),
        Some("rate_limited".to_string()),
    );
    conflicted.conflicts = vec![conflict("volumes/v1/chapter.md")];

    let result =
        aggregate_full_sync_result(vec![target("live_project", "projects/p1", conflicted)]);

    assert_eq!(result.overall_status, SyncStatus::PartialConflict);
    assert_eq!(
        result.error.as_deref(),
        Some("remote API rate limited while publishing generation")
    );
    assert_ne!(
        result.message_key.as_deref(),
        Some("sync.result.partial_conflict_summary"),
        "有具体 error 时不得替换为聚合摘要"
    );
}

/// #928/#931：一个 target 失败不回滚其他已完成 target 的传输统计；
/// 总体保留失败 target 的具体 error（含目标路径），不是笼统的“同步失败”。
#[test]
fn failed_target_does_not_roll_back_other_target_totals() {
    let mut failed = SyncResult::error(
        SyncStatus::RecoverableError("local_missing_without_tombstone".to_string()),
        "local_missing_without_tombstone: target=app paths=[starmaps/sm-1/nodes/06/n-1.json]"
            .to_string(),
        Some("local_io".to_string()),
    );
    failed.uploaded_files = vec!["settings.sync.json".to_string()];

    let mut completed = SyncResult::success();
    completed.status = SyncStatus::LatestWinsApplied;
    completed.uploaded_files = vec!["volumes/v1/chapter.md".to_string()];

    let result = aggregate_full_sync_result(vec![
        target("app", "app", failed),
        target("live_project", "projects/p1", completed),
    ]);

    assert_eq!(
        result.overall_status,
        SyncStatus::RecoverableError("one_or_more_targets_temporarily_failed".to_string())
    );
    assert_eq!(
        result.total_uploaded, 2,
        "已完成 target 的上传统计不得被失败 target 回滚"
    );
    let error = result.error.expect("overall must keep specific error");
    assert!(
        error.contains("target=app") && error.contains("starmaps/sm-1/nodes/06/n-1.json"),
        "总体错误必须带失败 target 与具体路径，实际: {error}"
    );
}
