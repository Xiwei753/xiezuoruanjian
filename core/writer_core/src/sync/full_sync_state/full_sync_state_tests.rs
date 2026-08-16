//! 全量同步持久状态行为测试（Issue #630 评论 5307423953 Part B / 5308040939 Part 1）。

use super::*;

mod tests {
    use super::*;

    fn success_result() -> FullSyncResult {
        FullSyncResult {
            overall_status: SyncStatus::Success,
            targets: vec![crate::sync::types::TargetSyncResult {
                target_kind: "app".to_string(),
                project_id: None,
                remote_prefix: "app".to_string(),
                result: crate::sync::types::SyncResult::success(),
            }],
            total_uploaded: 0,
            total_downloaded: 0,
            total_local_deletes: 0,
            total_remote_deletes: 0,
            total_overwritten: 0,
            total_ignored: 0,
            total_conflicts: 0,
            error: None,
            error_category: None,
            message_key: None,
        }
    }

    fn partial_failure_result() -> FullSyncResult {
        FullSyncResult {
            overall_status: SyncStatus::Error("one_or_more_targets_failed".to_string()),
            targets: vec![
                crate::sync::types::TargetSyncResult {
                    target_kind: "app".to_string(),
                    project_id: None,
                    remote_prefix: "app".to_string(),
                    result: crate::sync::types::SyncResult::success(),
                },
                crate::sync::types::TargetSyncResult {
                    target_kind: "project".to_string(),
                    project_id: Some("p1".to_string()),
                    remote_prefix: "projects/p1".to_string(),
                    result: crate::sync::types::SyncResult::error(
                        SyncStatus::FatalError("boom".to_string()),
                        crate::sync::types::FirstSyncMode::NotAttempted,
                        "boom".to_string(),
                        None,
                    ),
                },
            ],
            total_uploaded: 0,
            total_downloaded: 0,
            total_local_deletes: 0,
            total_remote_deletes: 0,
            total_overwritten: 0,
            total_ignored: 0,
            total_conflicts: 0,
            error: None,
            error_category: None,
            message_key: None,
        }
    }

    #[test]
    fn overall_success_updates_last_success_time() {
        let result = success_result();
        let state = FullSyncState::from_result_and_previous(&result, None, 1000);
        assert_eq!(state.overall_status, SyncStatus::Success);
        assert_eq!(state.last_attempt_time, Some(1000));
        assert_eq!(state.last_success_time, Some(1000));
        assert!(state.failed_targets.is_empty());
    }

    #[test]
    fn partial_failure_preserves_previous_last_success_time() {
        let previous = FullSyncState {
            overall_status: SyncStatus::Success,
            last_attempt_time: Some(500),
            last_success_time: Some(500),
            failed_targets: vec![],
        };
        let result = partial_failure_result();
        let state = FullSyncState::from_result_and_previous(&result, Some(&previous), 1000);
        assert_eq!(
            state.overall_status,
            SyncStatus::Error("one_or_more_targets_failed".to_string())
        );
        assert_eq!(state.last_attempt_time, Some(1000));
        // 部分失败保留旧 last_success_time
        assert_eq!(state.last_success_time, Some(500));
        // 失败 target 被记录
        assert_eq!(state.failed_targets, vec!["project:p1".to_string()]);
    }

    #[test]
    fn partial_failure_without_previous_has_no_last_success_time() {
        let result = partial_failure_result();
        let state = FullSyncState::from_result_and_previous(&result, None, 1000);
        assert_eq!(state.last_success_time, None);
        assert_eq!(state.failed_targets, vec!["project:p1".to_string()]);
    }

    #[test]
    fn no_changes_is_overall_success() {
        let mut result = success_result();
        result.overall_status = SyncStatus::NoChanges;
        let state = FullSyncState::from_result_and_previous(&result, None, 2000);
        assert_eq!(state.last_success_time, Some(2000));
    }

    #[test]
    fn branch_missing_recovered_is_overall_success() {
        let mut result = success_result();
        result.overall_status = SyncStatus::BranchMissingRecovered;
        let state = FullSyncState::from_result_and_previous(&result, None, 3000);
        assert_eq!(state.last_success_time, Some(3000));
    }

    #[test]
    fn latest_wins_applied_is_overall_success() {
        let mut result = success_result();
        result.overall_status = SyncStatus::LatestWinsApplied;
        let state = FullSyncState::from_result_and_previous(&result, None, 4000);
        assert_eq!(state.last_success_time, Some(4000));
    }

    #[test]
    fn conflict_is_not_overall_success() {
        let mut result = success_result();
        result.overall_status = SyncStatus::PartialConflict;
        let previous = FullSyncState {
            overall_status: SyncStatus::Success,
            last_attempt_time: Some(100),
            last_success_time: Some(100),
            failed_targets: vec![],
        };
        let state = FullSyncState::from_result_and_previous(&result, Some(&previous), 5000);
        assert_eq!(state.last_success_time, Some(100));
    }

    // ── Issue #630 评论 5308040939 Part 1：started / failed_before_targets ──

    #[test]
    fn started_writes_syncing_and_preserves_previous_last_success() {
        let previous = FullSyncState {
            overall_status: SyncStatus::Success,
            last_attempt_time: Some(100),
            last_success_time: Some(300),
            failed_targets: vec![],
        };
        let state = FullSyncState::started(Some(&previous), 400);
        assert_eq!(state.overall_status, SyncStatus::Syncing);
        assert_eq!(state.last_attempt_time, Some(400));
        // 旧绿灯的 last_success_time 必须保留，部分失败/中断时仍知道上次整体成功时刻。
        assert_eq!(state.last_success_time, Some(300));
        assert!(state.failed_targets.is_empty());
    }

    #[test]
    fn started_without_previous_has_no_last_success() {
        let state = FullSyncState::started(None, 400);
        assert_eq!(state.overall_status, SyncStatus::Syncing);
        assert_eq!(state.last_attempt_time, Some(400));
        assert_eq!(state.last_success_time, None);
    }

    #[test]
    fn failed_before_targets_writes_status_and_failed_target() {
        let previous = FullSyncState {
            overall_status: SyncStatus::Success,
            last_attempt_time: Some(100),
            last_success_time: Some(300),
            failed_targets: vec![],
        };
        let state = FullSyncState::failed_before_targets(
            Some(&previous),
            SyncStatus::FatalError("preflight".to_string()),
            500,
            "preflight",
        );
        assert!(matches!(state.overall_status, SyncStatus::FatalError(_)));
        assert_eq!(state.last_attempt_time, Some(500));
        // 提前失败同样保留旧 last_success_time，不把旧绿灯抹掉。
        assert_eq!(state.last_success_time, Some(300));
        assert_eq!(state.failed_targets, vec!["preflight".to_string()]);
    }

    #[test]
    fn failed_before_targets_allows_global_target() {
        let state = FullSyncState::failed_before_targets(
            None,
            SyncStatus::RecoverableError("list_projects".to_string()),
            600,
            "global",
        );
        assert!(matches!(
            state.overall_status,
            SyncStatus::RecoverableError(_)
        ));
        assert_eq!(state.last_success_time, None);
        assert_eq!(state.failed_targets, vec!["global".to_string()]);
    }
}
