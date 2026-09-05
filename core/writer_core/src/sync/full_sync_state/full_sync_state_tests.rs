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
                deleted_resolution: None,
                local_lifecycle_action: crate::sync::types::LocalLifecycleCommitAction::None,
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
                    deleted_resolution: None,
                    local_lifecycle_action: crate::sync::types::LocalLifecycleCommitAction::None,
                },
                crate::sync::types::TargetSyncResult {
                    target_kind: "project".to_string(),
                    project_id: Some("p1".to_string()),
                    remote_prefix: "projects/p1".to_string(),
                    result: crate::sync::types::SyncResult::error(
                        SyncStatus::FatalError("boom".to_string()),
                        "boom".to_string(),
                        None,
                    ),
                    deleted_resolution: None,
                    local_lifecycle_action: crate::sync::types::LocalLifecycleCommitAction::None,
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

    // ── Issue #630 评论 5308439467 Part 1：recover_interrupted ──

    fn make_recover_state(
        status: SyncStatus,
        attempt: Option<i64>,
        success: Option<i64>,
    ) -> FullSyncState {
        FullSyncState {
            overall_status: status,
            last_attempt_time: attempt,
            last_success_time: success,
            failed_targets: Vec::new(),
        }
    }

    /// `recover_interrupted(None)` 返回 None：没有旧状态不需要恢复。
    #[test]
    fn recover_interrupted_none_returns_none() {
        assert!(FullSyncState::recover_interrupted(None).is_none());
    }

    /// `recover_interrupted(Some(Success state))` 返回 None：Success 是终态，不需要恢复。
    #[test]
    fn recover_interrupted_success_returns_none() {
        let state = make_recover_state(SyncStatus::Success, Some(1000), Some(999));
        assert!(FullSyncState::recover_interrupted(Some(&state)).is_none());
    }

    /// `recover_interrupted(Some(FatalError state))` 返回 None：FatalError 是终态。
    #[test]
    fn recover_interrupted_fatal_error_returns_none() {
        let state = make_recover_state(
            SyncStatus::FatalError("auth failed".to_string()),
            Some(1000),
            Some(999),
        );
        assert!(FullSyncState::recover_interrupted(Some(&state)).is_none());
    }

    /// `recover_interrupted(Some(RecoverableError state))` 返回 None：RecoverableError 是终态。
    #[test]
    fn recover_interrupted_recoverable_error_returns_none() {
        let state = make_recover_state(
            SyncStatus::RecoverableError("network hiccup".to_string()),
            Some(1000),
            Some(999),
        );
        assert!(FullSyncState::recover_interrupted(Some(&state)).is_none());
    }

    /// `recover_interrupted(Some(Syncing state))` 返回中断终态：
    /// - `overall_status = RecoverableError("previous_full_sync_interrupted")`
    /// - `failed_targets = ["global"]`
    /// - `last_attempt_time` 保留旧 attempt（不伪造新尝试）
    /// - `last_success_time` 原样保留
    #[test]
    fn recover_interrupted_syncing_returns_recoverable_interrupted() {
        let state = make_recover_state(SyncStatus::Syncing, Some(1234), Some(999));
        let recovered = FullSyncState::recover_interrupted(Some(&state))
            .expect("Syncing state must be recovered");

        assert!(
            matches!(
                recovered.overall_status,
                SyncStatus::RecoverableError(ref msg) if msg == "previous_full_sync_interrupted"
            ),
            "recovered status must be RecoverableError(\"previous_full_sync_interrupted\"), got {:?}",
            recovered.overall_status
        );
        assert_eq!(
            recovered.failed_targets,
            vec!["global".to_string()],
            "failed_targets must be [\"global\"]"
        );
        assert_eq!(
            recovered.last_attempt_time,
            Some(1234),
            "last_attempt_time must preserve old attempt, not fabricate a new one"
        );
        assert_eq!(
            recovered.last_success_time,
            Some(999),
            "last_success_time must be preserved as-is"
        );
    }

    /// `recover_interrupted(Some(Syncing state))` 当旧 `last_success_time` 为 None 时
    /// 仍返回 None 的 last_success_time（原样保留，不伪造）。
    #[test]
    fn recover_interrupted_syncing_preserves_none_last_success() {
        let state = make_recover_state(SyncStatus::Syncing, Some(1234), None);
        let recovered = FullSyncState::recover_interrupted(Some(&state))
            .expect("Syncing state must be recovered");
        assert!(
            recovered.last_success_time.is_none(),
            "last_success_time must remain None when old was None"
        );
        assert_eq!(recovered.last_attempt_time, Some(1234));
    }

    /// `recover_interrupted(Some(Syncing state))` 当旧 `last_attempt_time` 为 None 时
    /// 保留 None（不伪造新尝试时间）。
    #[test]
    fn recover_interrupted_syncing_preserves_none_last_attempt() {
        let state = make_recover_state(SyncStatus::Syncing, None, Some(999));
        let recovered = FullSyncState::recover_interrupted(Some(&state))
            .expect("Syncing state must be recovered");
        assert!(
            recovered.last_attempt_time.is_none(),
            "last_attempt_time must remain None when old was None, not fabricate a new attempt"
        );
        assert_eq!(recovered.last_success_time, Some(999));
    }
}
