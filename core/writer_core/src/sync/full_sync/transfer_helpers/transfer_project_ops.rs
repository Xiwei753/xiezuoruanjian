//! 非常规项目的传输实现：恢复远端项目、删除本地项目、删除远端项目。
//!
//! 三个函数共用同一套「先 CAS 再搬运」的骨架，放在一起便于对照；与
//! `transfer_live.rs` 的正常上传路径关注点不同，故独立成文件。

use super::*;

#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub(in crate::sync::full_sync) fn transfer_restore_project(
    provider: &dyn SyncProvider,
    planned: &PlannedTarget,
    plan: &FullSyncPlan,
    cancellation_token: Option<&SyncCancellationToken>,
) -> (
    SyncResult,
    Option<crate::sync::types::DeletedTargetResolution>,
    Option<crate::sync::types::LocalLifecycleCommitAction>,
) {
    use crate::sync::SyncStatus;

    // Issue #729：关键写/delete 操作前检查取消令牌。取消则跳过整个 helper。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] transfer_restore_project: cancellation requested — skipping target {}",
                planned.target.remote_prefix
            );
            return (SyncResult::success(), None, None);
        }
    }

    match resolve_current_target_lifecycle(provider, &planned.target.remote_prefix) {
        Ok(Some(current_rec)) => {
            // Issue #729 评论 5765979275：CAS 返回后、开始下一次 provider 操作前检查取消令牌。
            if let Some(token) = cancellation_token {
                if token.is_cancelled() {
                    log::info!(
                        "[sync] transfer_restore_project: cancellation requested after CAS — skipping {}",
                        planned.target.remote_prefix
                    );
                    return (SyncResult::success(), None, None);
                }
            }
            use crate::sync::types::TargetOp;
            match current_rec.op {
                TargetOp::Delete => {
                    let local_project_exists = planned.local_root.exists() && {
                        std::fs::read_dir(&planned.local_root)
                            .map(|mut it| it.next().is_some())
                            .unwrap_or(false)
                    };
                    if !local_project_exists {
                        log::info!("[sync] run_transfer: RestoreProject {} — CAS: remote changed to Delete, local project absent — cleaning remote residue", planned.target.remote_prefix);
                        let cleanup_result = delete_all_remote_objects(
                            provider,
                            &planned.target.remote_prefix,
                            cancellation_token,
                        );
                        let cleanup_ok = matches!(
                            cleanup_result.status,
                            SyncStatus::Success | SyncStatus::NoChanges
                        );
                        if !cleanup_ok {
                            let expected_time =
                                crate::sync::target_lifecycle::record_lww_time(&current_rec);
                            let expected_device = &current_rec.device_id;
                            let record_result =
                                crate::sync::pending_remote_cleanup::record_pending_remote_cleanup(
                                    &plan.app_data_root,
                                    &planned.target.remote_prefix,
                                    planned.project_id.as_deref().unwrap_or(""),
                                    &format!(
                                        "RestoreProject remote-only cleanup failed: {:?}",
                                        cleanup_result.status
                                    ),
                                    expected_time,
                                    expected_device,
                                );
                            if let Err(e) = record_result {
                                (sync_result_from_error(e), None, None)
                            } else {
                                (cleanup_result, None, None)
                            }
                        } else {
                            (cleanup_result, None, None)
                        }
                    } else {
                        log::info!("[sync] run_transfer: RestoreProject {} — CAS: remote changed to Delete, local project exists — re-evaluating", planned.target.remote_prefix);
                        let current_candidate =
                            super::super::plan::compute_local_project_lifecycle_candidate(
                                &planned.local_root,
                                planned
                                    .live_lww
                                    .as_ref()
                                    .map(|l| l.device_id.as_str())
                                    .unwrap_or(""),
                            );
                        match current_candidate {
                            super::super::LifecycleCandidate::Live { lww: current_lww } => {
                                let remote_time =
                                    crate::sync::target_lifecycle::record_lww_time(&current_rec);
                                let local_wins = current_lww.lww_time_ms > remote_time
                                    || (current_lww.lww_time_ms == remote_time
                                        && current_lww.device_id > current_rec.device_id);
                                if !local_wins {
                                    let action = planned.project_id.as_ref().map(|pid| {
                                        crate::sync::types::LocalLifecycleCommitAction::DeleteProject {
                                            project_id: pid.clone(),
                                            expected_local_lww: crate::sync::types::LiveTargetLwwSerde::from_lww(&current_lww),
                                        }
                                    });
                                    (SyncResult::no_changes(), None, action)
                                } else {
                                    log::info!("[sync] run_transfer: RestoreProject {} — CAS: remote Delete but local LWW wins", planned.target.remote_prefix);
                                    (SyncResult::no_changes(), None, None)
                                }
                            }
                            super::super::LifecycleCandidate::Retry => {
                                log::info!("[sync] run_transfer: RestoreProject {} — CAS: remote Delete but local snapshot failed — Retry", planned.target.remote_prefix);
                                (
                                    SyncResult::error(
                                        SyncStatus::RecoverableError(
                                            "RestoreProject: local snapshot failed".to_string(),
                                        ),
                                        "RestoreProject: local snapshot failed".to_string(),
                                        None,
                                    ),
                                    Some(crate::sync::types::DeletedTargetResolution::Retry),
                                    None,
                                )
                            }
                        }
                    }
                }
                TargetOp::Upsert => {
                    let download_prefix: crate::error::Result<String> = match &current_rec
                        .active_generation
                    {
                        Some(gen_id) => {
                            let gen_prefix = super::super::generation::generation_remote_prefix(
                                &planned.target.remote_prefix,
                                gen_id,
                            );
                            if let Ok(ref p) = gen_prefix {
                                log::info!("[sync] run_transfer: RestoreProject {} — CAS confirmed Upsert with active_generation={}, downloading from {}", planned.target.remote_prefix, gen_id, p);
                            }
                            gen_prefix
                        }
                        None => {
                            log::info!("[sync] run_transfer: RestoreProject {} — CAS confirmed Upsert (no active_generation), downloading from legacy", planned.target.remote_prefix);
                            Ok(planned.target.remote_prefix.clone())
                        }
                    };
                    match download_prefix {
                        Ok(download_prefix) => {
                            let result = download_remote_to_staging(
                                provider,
                                &download_prefix,
                                planned.staging_root.as_deref(),
                            );
                            (
                                result,
                                Some(crate::sync::types::DeletedTargetResolution::RemoteTargetWins),
                                None,
                            )
                        }
                        Err(e) => (sync_result_from_error(e), None, None),
                    }
                }
            }
        }
        Ok(None) => {
            log::info!(
                "[sync] run_transfer: RestoreProject {} — CAS: no remote record, returning Retry",
                planned.target.remote_prefix
            );
            (
                SyncResult::error(
                    SyncStatus::RecoverableError(
                        "RestoreProject: no remote record, retrying".to_string(),
                    ),
                    "RestoreProject: no remote record to confirm upsert winner".to_string(),
                    None,
                ),
                Some(crate::sync::types::DeletedTargetResolution::Retry),
                None,
            )
        }
        Err(e) => {
            log::warn!(
                "[sync] RestoreProject CAS failed for {}: {e}",
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

#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub(in crate::sync::full_sync) fn transfer_delete_local_project(
    provider: &dyn SyncProvider,
    planned: &PlannedTarget,
    plan: &FullSyncPlan,
    cancellation_token: Option<&SyncCancellationToken>,
) -> (
    SyncResult,
    Option<crate::sync::types::DeletedTargetResolution>,
    Option<crate::sync::types::LocalLifecycleCommitAction>,
) {
    // Issue #729：关键写/delete 操作前检查取消令牌。取消则跳过整个 helper。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] transfer_delete_local_project: cancellation requested — skipping target {}",
                planned.target.remote_prefix
            );
            return (SyncResult::success(), None, None);
        }
    }

    match resolve_current_target_lifecycle(provider, &planned.target.remote_prefix) {
        Ok(Some(current_rec)) => {
            // Issue #729 评论 5765979275：CAS 返回后、开始下一次 provider 操作前检查取消令牌。
            if let Some(token) = cancellation_token {
                if token.is_cancelled() {
                    log::info!(
                        "[sync] transfer_delete_local_project: cancellation requested after CAS — skipping {}",
                        planned.target.remote_prefix
                    );
                    return (SyncResult::success(), None, None);
                }
            }
            use crate::sync::types::TargetOp;
            match current_rec.op {
                TargetOp::Upsert => {
                    log::info!("[sync] run_transfer: DeleteLocalProject {} — CAS: remote changed to Upsert, switching to ReplaceProject", planned.target.remote_prefix);
                    let result = download_remote_to_staging(
                        provider,
                        &planned.target.remote_prefix,
                        planned.staging_root.as_deref(),
                    );
                    let action = planned.project_id.as_ref().and_then(|pid| {
                        planned.live_lww.as_ref().map(|lww| {
                            crate::sync::types::LocalLifecycleCommitAction::ReplaceProject {
                                project_id: pid.clone(),
                                expected_local_lww:
                                    crate::sync::types::LiveTargetLwwSerde::from_lww(lww),
                            }
                        })
                    });
                    (result, None, action)
                }
                TargetOp::Delete => {
                    log::info!("[sync] run_transfer: DeleteLocalProject {} — CAS confirmed remote Delete, cleaning remote residue", planned.target.remote_prefix);
                    let cleanup_result = delete_all_remote_objects(
                        provider,
                        &planned.target.remote_prefix,
                        cancellation_token,
                    );
                    let cleanup_ok = matches!(
                        cleanup_result.status,
                        crate::sync::SyncStatus::Success | crate::sync::SyncStatus::NoChanges
                    );
                    if !cleanup_ok {
                        let expected_time =
                            crate::sync::target_lifecycle::record_lww_time(&current_rec);
                        let expected_device = &current_rec.device_id;
                        let record_result =
                            crate::sync::pending_remote_cleanup::record_pending_remote_cleanup(
                                &plan.app_data_root,
                                &planned.target.remote_prefix,
                                planned.project_id.as_deref().unwrap_or(""),
                                &format!(
                                    "DeleteLocalProject current Delete cleanup failed: {:?}",
                                    cleanup_result.status
                                ),
                                expected_time,
                                expected_device,
                            );
                        if let Err(e) = record_result {
                            (sync_result_from_error(e), None, None)
                        } else {
                            (cleanup_result, None, None)
                        }
                    } else {
                        let action = planned.project_id.as_ref().and_then(|pid| {
                            planned.live_lww.as_ref().map(|lww| {
                                crate::sync::types::LocalLifecycleCommitAction::DeleteProject {
                                    project_id: pid.clone(),
                                    expected_local_lww:
                                        crate::sync::types::LiveTargetLwwSerde::from_lww(lww),
                                }
                            })
                        });
                        (SyncResult::no_changes(), None, action)
                    }
                }
            }
        }
        Ok(None) => {
            log::info!("[sync] run_transfer: DeleteLocalProject {} — CAS: no remote record, returning Retry", planned.target.remote_prefix);
            (
                SyncResult::error(
                    crate::sync::SyncStatus::RecoverableError(
                        "DeleteLocalProject: no remote record, retrying".to_string(),
                    ),
                    "DeleteLocalProject: no remote record to confirm delete winner".to_string(),
                    None,
                ),
                Some(crate::sync::types::DeletedTargetResolution::Retry),
                None,
            )
        }
        Err(e) => {
            log::warn!(
                "[sync] DeleteLocalProject CAS failed for {}: {e}",
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

#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub(in crate::sync::full_sync) fn transfer_delete_remote_project(
    provider: &dyn SyncProvider,
    planned: &PlannedTarget,
    catalog_snapshot: &mut crate::sync::types::RemoteTargetCatalogSnapshot,
    cancellation_token: Option<&SyncCancellationToken>,
) -> (
    SyncResult,
    Option<crate::sync::types::DeletedTargetResolution>,
    Option<crate::sync::types::LocalLifecycleCommitAction>,
) {
    use crate::sync::types::TargetLifecycleApplyResult;

    // Issue #729：关键写/delete 操作前检查取消令牌。取消则跳过整个 helper。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] transfer_delete_remote_project: cancellation requested — skipping target {}",
                planned.target.remote_prefix
            );
            return (SyncResult::success(), None, None);
        }
    }

    let deleted_at_ms = planned
        .deleted_lww
        .as_ref()
        .map(|l| l.deleted_at_ms)
        .unwrap_or_else(|| crate::sync::full_sync_utils::now_epoch_seconds() * 1000);
    let device_id = planned
        .deleted_lww
        .as_ref()
        .map(|l| l.device_id.as_str())
        .unwrap_or("");
    let candidate = crate::sync::types::TargetLifecycleRecord::delete(
        &planned.target.remote_prefix,
        &planned.target.remote_prefix,
        deleted_at_ms,
        device_id,
    );
    match crate::sync::target_lifecycle::apply_lifecycle_record(
        provider,
        catalog_snapshot,
        candidate,
    ) {
        TargetLifecycleApplyResult::Applied(persisted) => {
            *catalog_snapshot = persisted;
            // Issue #729 评论 5765979275：CAS 返回后、开始下一次 provider 操作前检查取消令牌。
            if let Some(token) = cancellation_token {
                if token.is_cancelled() {
                    log::info!(
                        "[sync] transfer_delete_remote_project: cancellation requested after apply_lifecycle_record — skipping {}",
                        planned.target.remote_prefix
                    );
                    return (SyncResult::success(), None, None);
                }
            }
            let del_result = delete_all_remote_objects(
                provider,
                &planned.target.remote_prefix,
                cancellation_token,
            );
            (
                del_result,
                Some(crate::sync::types::DeletedTargetResolution::LocalDeleteWins),
                None,
            )
        }
        TargetLifecycleApplyResult::AlreadyCurrent(persisted) => {
            *catalog_snapshot = persisted;
            // Issue #729 评论 5765979275：CAS 返回后、开始下一次 provider 操作前检查取消令牌。
            if let Some(token) = cancellation_token {
                if token.is_cancelled() {
                    log::info!(
                        "[sync] transfer_delete_remote_project: cancellation requested after apply_lifecycle_record — skipping {}",
                        planned.target.remote_prefix
                    );
                    return (SyncResult::success(), None, None);
                }
            }
            log::info!("[sync] run_transfer: DeleteRemoteProject AlreadyCurrent(Delete) {} — continuing cleanup", planned.target.remote_prefix);
            let del_result = delete_all_remote_objects(
                provider,
                &planned.target.remote_prefix,
                cancellation_token,
            );
            (
                del_result,
                Some(crate::sync::types::DeletedTargetResolution::LocalDeleteWins),
                None,
            )
        }
        TargetLifecycleApplyResult::RemoteWinner {
            snapshot: persisted,
            record: winner,
        } => {
            *catalog_snapshot = persisted;
            match winner.op {
                crate::sync::types::TargetOp::Delete => {
                    log::info!("[sync] run_transfer: DeleteRemoteProject RemoteWinner(Delete) {} — continuing cleanup", planned.target.remote_prefix);
                    // Issue #729 评论 5765979275：CAS 返回后、开始下一次 provider 操作前检查取消令牌。
                    if let Some(token) = cancellation_token {
                        if token.is_cancelled() {
                            log::info!(
                                "[sync] transfer_delete_remote_project: cancellation requested after apply_lifecycle_record — skipping {}",
                                planned.target.remote_prefix
                            );
                            return (SyncResult::success(), None, None);
                        }
                    }
                    let del_result = delete_all_remote_objects(
                        provider,
                        &planned.target.remote_prefix,
                        cancellation_token,
                    );
                    (
                        del_result,
                        Some(crate::sync::types::DeletedTargetResolution::LocalDeleteWins),
                        None,
                    )
                }
                crate::sync::types::TargetOp::Upsert => {
                    log::info!("[sync] run_transfer: DeleteRemoteProject RemoteWinner(Upsert) {} — switching to restore", planned.target.remote_prefix);
                    // Issue #729 评论 5765979275：CAS 返回后、开始下一次 provider 操作前检查取消令牌。
                    if let Some(token) = cancellation_token {
                        if token.is_cancelled() {
                            log::info!(
                                "[sync] transfer_delete_remote_project: cancellation requested after apply_lifecycle_record — skipping {}",
                                planned.target.remote_prefix
                            );
                            return (SyncResult::success(), None, None);
                        }
                    }
                    let restore_result = download_remote_to_staging(
                        provider,
                        &planned.target.remote_prefix,
                        planned.staging_root.as_deref(),
                    );
                    (
                        restore_result,
                        Some(crate::sync::types::DeletedTargetResolution::RemoteTargetWins),
                        None,
                    )
                }
            }
        }
        TargetLifecycleApplyResult::Retry(e) => (
            sync_result_from_error(e),
            Some(crate::sync::types::DeletedTargetResolution::Retry),
            None,
        ),
    }
}
