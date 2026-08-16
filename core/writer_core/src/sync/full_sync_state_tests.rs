//! FullSyncState 单元测试（Issue #630 评论 5308439467 Part 1）。
//!
//! 覆盖 `recover_interrupted` 的三种输入：None / 非 Syncing 终态 / Syncing。

use super::full_sync_state::FullSyncState;
use super::types::SyncStatus;

fn make_state(status: SyncStatus, attempt: Option<i64>, success: Option<i64>) -> FullSyncState {
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
    let state = make_state(SyncStatus::Success, Some(1000), Some(999));
    assert!(FullSyncState::recover_interrupted(Some(&state)).is_none());
}

/// `recover_interrupted(Some(FatalError state))` 返回 None：FatalError 是终态。
#[test]
fn recover_interrupted_fatal_error_returns_none() {
    let state = make_state(
        SyncStatus::FatalError("auth failed".to_string()),
        Some(1000),
        Some(999),
    );
    assert!(FullSyncState::recover_interrupted(Some(&state)).is_none());
}

/// `recover_interrupted(Some(RecoverableError state))` 返回 None：RecoverableError 是终态。
#[test]
fn recover_interrupted_recoverable_error_returns_none() {
    let state = make_state(
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
    let state = make_state(SyncStatus::Syncing, Some(1234), Some(999));
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
        recovered.last_success_time, Some(999),
        "last_success_time must be preserved as-is"
    );
}

/// `recover_interrupted(Some(Syncing state))` 当旧 `last_success_time` 为 None 时
/// 仍返回 None 的 last_success_time（原样保留，不伪造）。
#[test]
fn recover_interrupted_syncing_preserves_none_last_success() {
    let state = make_state(SyncStatus::Syncing, Some(1234), None);
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
    let state = make_state(SyncStatus::Syncing, None, Some(999));
    let recovered = FullSyncState::recover_interrupted(Some(&state))
        .expect("Syncing state must be recovered");
    assert!(
        recovered.last_attempt_time.is_none(),
        "last_attempt_time must remain None when old was None, not fabricate a new attempt"
    );
    assert_eq!(recovered.last_success_time, Some(999));
}
