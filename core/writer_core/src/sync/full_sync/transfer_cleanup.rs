use crate::sync::provider::SyncProvider;
use crate::sync::types::SyncResult;

use super::transfer_helpers::{
    delete_all_remote_objects, resolve_current_target_lifecycle, sync_result_from_error,
};
use super::PlannedTarget;

#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub(super) fn transfer_remote_cleanup_project(
    provider: &dyn SyncProvider,
    planned: &PlannedTarget,
) -> (
    SyncResult,
    Option<crate::sync::types::DeletedTargetResolution>,
    Option<crate::sync::types::LocalLifecycleCommitAction>,
) {
    log::info!(
        "[sync] run_transfer: RemoteCleanupProject {} — CAS: re-confirming remote lifecycle",
        planned.target.remote_prefix
    );
    match resolve_current_target_lifecycle(provider, &planned.target.remote_prefix) {
        Ok(Some(current_rec)) => {
            use crate::sync::types::TargetOp;
            match current_rec.op {
                TargetOp::Upsert => {
                    log::info!("[sync] run_transfer: RemoteCleanupProject {} — CAS: remote changed to Upsert, pending expired", planned.target.remote_prefix);
                    (
                        SyncResult::no_changes(),
                        Some(crate::sync::types::DeletedTargetResolution::LocalDeleteWins),
                        None,
                    )
                }
                TargetOp::Delete => {
                    let current_time = crate::sync::target_lifecycle::record_lww_time(&current_rec);
                    let expected = planned.expected_delete_lww.as_ref();
                    let expected_time = expected.map(|e| e.deleted_at_ms).unwrap_or(0);
                    let expected_device = expected.map(|e| e.device_id.as_str()).unwrap_or("");
                    let same_delete =
                        current_time == expected_time && current_rec.device_id == expected_device;
                    let newer_delete = current_time >= expected_time;
                    if !same_delete && !newer_delete {
                        let msg = format!("RemoteCleanupProject {}: CAS: current Delete lww_time {} < expected {} — stale catalog, retrying", planned.target.remote_prefix, current_time, expected_time);
                        log::warn!("[sync] {msg}");
                        (
                            SyncResult::error(
                                crate::sync::SyncStatus::RecoverableError(msg.clone()),
                                msg,
                                None,
                            ),
                            Some(crate::sync::types::DeletedTargetResolution::Retry),
                            None,
                        )
                    } else {
                        let cleanup_result =
                            delete_all_remote_objects(provider, &planned.target.remote_prefix);
                        let cleanup_ok = matches!(
                            cleanup_result.status,
                            crate::sync::SyncStatus::Success | crate::sync::SyncStatus::NoChanges
                        );
                        if cleanup_ok {
                            (
                                cleanup_result,
                                Some(crate::sync::types::DeletedTargetResolution::LocalDeleteWins),
                                None,
                            )
                        } else {
                            (
                                cleanup_result,
                                Some(crate::sync::types::DeletedTargetResolution::Retry),
                                None,
                            )
                        }
                    }
                }
            }
        }
        Ok(None) => {
            let msg = format!(
                "RemoteCleanupProject {}: CAS: no remote record, retrying",
                planned.target.remote_prefix
            );
            log::warn!("[sync] {msg}");
            (
                SyncResult::error(
                    crate::sync::SyncStatus::RecoverableError(msg.clone()),
                    msg,
                    None,
                ),
                Some(crate::sync::types::DeletedTargetResolution::Retry),
                None,
            )
        }
        Err(e) => {
            log::warn!(
                "[sync] RemoteCleanupProject CAS failed for {}: {e}",
                planned.target.remote_prefix
            );
            (
                sync_result_from_error(e),
                Some(crate::sync::types::DeletedTargetResolution::Retry),
                None,
            )
        }
    }
}
