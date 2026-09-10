//! Transfer helper functions — shared utilities used by transfer.rs and other submodules.
//!
//! Contains error conversion, remote object operations, staging downloads, single-target sync,
//! and per-kind transfer functions extracted from run_transfer.

use std::path::Path;

use crate::sync::provider::SyncProvider;
use crate::sync::types::{SyncPolicy, SyncResult, SyncTarget};

use super::{FullSyncPlan, PlannedTarget};

// ── Lifecycle CAS helper ──

pub(super) fn resolve_current_target_lifecycle(
    provider: &dyn SyncProvider,
    target_id: &str,
) -> crate::error::Result<Option<crate::sync::types::TargetLifecycleRecord>> {
    let snapshot = crate::sync::target_lifecycle::load_remote_catalog(provider)?;
    Ok(crate::sync::target_lifecycle::find_record(&snapshot.catalog, target_id).cloned())
}

// ── Error conversion ──

pub(super) fn sync_result_from_error(err: crate::Error) -> SyncResult {
    use crate::sync::SyncStatus;
    let msg = err.to_string();
    let category = err.sync_category();
    let status = if err.recoverable() {
        SyncStatus::RecoverableError(msg.clone())
    } else {
        SyncStatus::FatalError(msg.clone())
    };
    SyncResult::error(
        status,
        msg,
        (!category.is_empty()).then(|| category.to_string()),
    )
}

pub(super) fn sync_result_from_provider_error(
    err: crate::sync::provider::ProviderError,
) -> SyncResult {
    sync_result_from_error(crate::Error::from(err))
}

// ── Remote operations ──

pub(super) fn download_remote_to_staging(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    staging_root: Option<&Path>,
) -> SyncResult {
    let entries = match provider.list(remote_prefix) {
        Ok(e) => e,
        Err(e) => return sync_result_from_provider_error(e),
    };
    let mut downloaded: Vec<String> = Vec::new();
    for entry in &entries {
        let full_remote_path = format!("{}/{}", remote_prefix, entry.path);
        let obj = match provider.read(&full_remote_path) {
            Ok(Some(obj)) => obj,
            Ok(None) => continue,
            Err(e) => return sync_result_from_provider_error(e),
        };
        if let Some(staging) = staging_root {
            if let Err(e) =
                super::generation::write_staging_file(staging, &entry.path, &obj.content)
            {
                return sync_result_from_error(crate::Error::Io(e));
            }
        }
        downloaded.push(full_remote_path);
    }
    let mut result = SyncResult::success();
    result.downloaded_files = downloaded;
    result
}

pub(super) fn delete_all_remote_objects(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
) -> SyncResult {
    let remote_entries = match provider.list(remote_prefix) {
        Ok(entries) => entries,
        Err(err) => return sync_result_from_provider_error(err),
    };
    let mut remote_deletes: Vec<String> = Vec::new();
    for entry in &remote_entries {
        if super::generation::is_generation_path(&entry.path) {
            log::debug!(
                "[sync] delete_all_remote_objects: skipping generation path {} under {}",
                entry.path,
                remote_prefix
            );
            continue;
        }
        let full_remote_path = format!("{}/{}", remote_prefix, entry.path);
        match provider.delete(
            &full_remote_path,
            crate::sync::provider::model::DeletePrecondition::Unconditional,
        ) {
            Ok(()) => {
                remote_deletes.push(full_remote_path);
            }
            Err(err) => {
                let mut result = sync_result_from_provider_error(err);
                result.remote_deletes = remote_deletes;
                return result;
            }
        }
    }
    let mut result = SyncResult::success();
    result.remote_deletes = remote_deletes;
    result
}

pub(super) fn run_single_target(
    provider: &dyn SyncProvider,
    local_root: &Path,
    sync_policy: &SyncPolicy,
    target: &SyncTarget,
    force_sync: bool,
) -> SyncResult {
    match crate::sync::SyncService::perform_lww_sync(
        local_root,
        provider,
        sync_policy,
        target,
        force_sync,
    ) {
        Ok(result) => result,
        Err(err) => sync_result_from_error(err),
    }
}

// ── Per-kind transfer functions ──

#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub(super) fn transfer_live_project(
    provider: &dyn SyncProvider,
    planned: &PlannedTarget,
    catalog_snapshot: &mut crate::sync::types::RemoteTargetCatalogSnapshot,
    plan: &FullSyncPlan,
) -> (
    SyncResult,
    Option<crate::sync::types::DeletedTargetResolution>,
    Option<crate::sync::types::LocalLifecycleCommitAction>,
) {
    use crate::sync::types::TargetLifecycleApplyResult;
    use crate::sync::SyncStatus;

    if planned.live_lww.is_some() {
        let generation_id = uuid::Uuid::new_v4().to_string();
        let sync_root = planned
            .staging_root
            .as_deref()
            .unwrap_or(&planned.local_root);
        let merge_outcome: crate::error::Result<Option<crate::sync::lww::LwwMergeOutcome>> =
            (|| {
                if let Some(source_record) = crate::sync::target_lifecycle::find_record(
                    &catalog_snapshot.catalog,
                    &planned.target.remote_prefix,
                ) {
                    if let Some(source_prefix) =
                        crate::sync::target_lifecycle::resolve_visible_project_prefix(
                            source_record,
                            &planned.target.remote_prefix,
                        )?
                    {
                        log::info!(
                            "[sync] run_transfer: LiveProject {} — merging from visible source {}",
                            planned.target.remote_prefix,
                            source_prefix
                        );
                        let mut merge_state = crate::sync::SyncService::load_sync_state(sync_root)?;
                        let outcome = crate::sync::lww::merge_remote_into_local_snapshot(
                            sync_root,
                            provider,
                            &source_prefix,
                            planned.target.scope,
                            &mut merge_state,
                        )?;
                        return Ok(Some(outcome));
                    }
                }
                Ok(None)
            })();
        match super::generation::generation_remote_prefix(
            &planned.target.remote_prefix,
            &generation_id,
        ) {
            Ok(gen_remote_prefix) => {
                log::info!(
                    "[sync] run_transfer: LiveProject {} — uploading to generation prefix {}",
                    planned.target.remote_prefix,
                    gen_remote_prefix
                );
                let content_result = match merge_outcome {
                    Ok(Some(outcome)) => {
                        if !outcome.conflicts.is_empty() {
                            let mut r = SyncResult::success();
                            r.status = SyncStatus::PartialConflict;
                            r.conflicts = outcome.conflicts;
                            r.downloaded_files = outcome.downloaded_files;
                            r.local_deletes = outcome.remote_delete_paths;
                            r.remote_deletes = outcome.local_deletes;
                            r.overwritten_files = outcome.overwritten_files;
                            r.ignored_files = outcome.ignored_files;
                            r
                        } else {
                            super::generation::publish_generation(
                                provider,
                                sync_root,
                                &gen_remote_prefix,
                                &generation_id,
                                planned.project_id.as_deref().unwrap_or(""),
                                planned.target.scope,
                                &plan.sync_policy,
                                plan.force_sync,
                                Some(&outcome),
                            )
                        }
                    }
                    Ok(None) => super::generation::publish_generation(
                        provider,
                        sync_root,
                        &gen_remote_prefix,
                        &generation_id,
                        planned.project_id.as_deref().unwrap_or(""),
                        planned.target.scope,
                        &plan.sync_policy,
                        plan.force_sync,
                        None,
                    ),
                    Err(e) => sync_result_from_error(e),
                };
                let content_ok = matches!(
                    content_result.status,
                    SyncStatus::Success | SyncStatus::NoChanges | SyncStatus::LatestWinsApplied
                );
                if !content_ok {
                    (content_result, None, None)
                } else {
                    let post_transfer_root = planned
                        .staging_root
                        .as_deref()
                        .unwrap_or(&planned.local_root);
                    match super::plan::read_post_transfer_lww(post_transfer_root) {
                        Some(post_transfer_lww) => {
                            let candidate = crate::sync::types::TargetLifecycleRecord::upsert(
                                &planned.target.remote_prefix,
                                &planned.target.remote_prefix,
                                post_transfer_lww.lww_time_ms,
                                &post_transfer_lww.device_id,
                            )
                            .with_active_generation(&generation_id);
                            match crate::sync::target_lifecycle::apply_lifecycle_record(
                                provider,
                                catalog_snapshot,
                                candidate,
                            ) {
                                TargetLifecycleApplyResult::Applied(persisted) => {
                                    *catalog_snapshot = persisted;
                                    (content_result, None, None)
                                }
                                TargetLifecycleApplyResult::AlreadyCurrent(persisted) => {
                                    *catalog_snapshot = persisted;
                                    (content_result, None, None)
                                }
                                TargetLifecycleApplyResult::RemoteWinner {
                                    snapshot: persisted,
                                    record: winner,
                                } => {
                                    *catalog_snapshot = persisted;
                                    match winner.op {
                                        crate::sync::types::TargetOp::Upsert => {
                                            log::info!(
                                                "[sync] run_transfer: LiveProject RemoteWinner(Upsert) {} — retrying",
                                                planned.target.remote_prefix
                                            );
                                            let msg = "LiveProject: remote generation changed during merge, retrying".to_string();
                                            (
                                                SyncResult::error(
                                                    SyncStatus::RecoverableError(msg.clone()),
                                                    msg,
                                                    None,
                                                ),
                                                None,
                                                None,
                                            )
                                        }
                                        crate::sync::types::TargetOp::Delete => {
                                            log::info!(
                                                "[sync] run_transfer: LiveProject RemoteWinner(Delete) {} — cleaning remote + deferring to Commit",
                                                planned.target.remote_prefix
                                            );
                                            let cleanup_result = delete_all_remote_objects(
                                                provider,
                                                &planned.target.remote_prefix,
                                            );
                                            let cleanup_ok = matches!(
                                                cleanup_result.status,
                                                SyncStatus::Success | SyncStatus::NoChanges
                                            );
                                            if !cleanup_ok {
                                                let expected_time =
                                                    crate::sync::target_lifecycle::record_lww_time(
                                                        &winner,
                                                    );
                                                let expected_device = &winner.device_id;
                                                let record_result = crate::sync::pending_remote_cleanup::record_pending_remote_cleanup(
                                                    &plan.app_data_root, &planned.target.remote_prefix,
                                                    planned.project_id.as_deref().unwrap_or(""),
                                                    &format!("LiveProject RemoteWinner(Delete) cleanup failed: {:?}", cleanup_result.status),
                                                    expected_time, expected_device,
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
                                                            expected_local_lww: crate::sync::types::LiveTargetLwwSerde::from_lww(lww),
                                                        }
                                                    })
                                                });
                                                (content_result, None, action)
                                            }
                                        }
                                    }
                                }
                                TargetLifecycleApplyResult::Retry(e) => {
                                    (sync_result_from_error(e), None, None)
                                }
                            }
                        }
                        None => {
                            let msg = "post-transfer staging manifest unreadable".to_string();
                            (
                                SyncResult::error(
                                    SyncStatus::RecoverableError(msg.clone()),
                                    msg,
                                    None,
                                ),
                                None,
                                None,
                            )
                        }
                    }
                }
            }
            Err(e) => (sync_result_from_error(e), None, None),
        }
    } else {
        let msg = "live project missing lww (manifest unreadable)".to_string();
        (
            SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None),
            None,
            None,
        )
    }
}

#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub(super) fn transfer_restore_project(
    provider: &dyn SyncProvider,
    planned: &PlannedTarget,
    plan: &FullSyncPlan,
) -> (
    SyncResult,
    Option<crate::sync::types::DeletedTargetResolution>,
    Option<crate::sync::types::LocalLifecycleCommitAction>,
) {
    use crate::sync::SyncStatus;
    match resolve_current_target_lifecycle(provider, &planned.target.remote_prefix) {
        Ok(Some(current_rec)) => {
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
                        let cleanup_result =
                            delete_all_remote_objects(provider, &planned.target.remote_prefix);
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
                            super::plan::compute_local_project_lifecycle_candidate(
                                &planned.local_root,
                                planned
                                    .live_lww
                                    .as_ref()
                                    .map(|l| l.device_id.as_str())
                                    .unwrap_or(""),
                            );
                        match current_candidate {
                            super::LifecycleCandidate::Live { lww: current_lww } => {
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
                            super::LifecycleCandidate::Retry => {
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
                            let gen_prefix = super::generation::generation_remote_prefix(
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
pub(super) fn transfer_delete_local_project(
    provider: &dyn SyncProvider,
    planned: &PlannedTarget,
    plan: &FullSyncPlan,
) -> (
    SyncResult,
    Option<crate::sync::types::DeletedTargetResolution>,
    Option<crate::sync::types::LocalLifecycleCommitAction>,
) {
    match resolve_current_target_lifecycle(provider, &planned.target.remote_prefix) {
        Ok(Some(current_rec)) => {
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
                    let cleanup_result =
                        delete_all_remote_objects(provider, &planned.target.remote_prefix);
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
pub(super) fn transfer_delete_remote_project(
    provider: &dyn SyncProvider,
    planned: &PlannedTarget,
    catalog_snapshot: &mut crate::sync::types::RemoteTargetCatalogSnapshot,
) -> (
    SyncResult,
    Option<crate::sync::types::DeletedTargetResolution>,
    Option<crate::sync::types::LocalLifecycleCommitAction>,
) {
    use crate::sync::types::TargetLifecycleApplyResult;
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
            let del_result = delete_all_remote_objects(provider, &planned.target.remote_prefix);
            (
                del_result,
                Some(crate::sync::types::DeletedTargetResolution::LocalDeleteWins),
                None,
            )
        }
        TargetLifecycleApplyResult::AlreadyCurrent(persisted) => {
            *catalog_snapshot = persisted;
            log::info!("[sync] run_transfer: DeleteRemoteProject AlreadyCurrent(Delete) {} — continuing cleanup", planned.target.remote_prefix);
            let del_result = delete_all_remote_objects(provider, &planned.target.remote_prefix);
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
                    let del_result =
                        delete_all_remote_objects(provider, &planned.target.remote_prefix);
                    (
                        del_result,
                        Some(crate::sync::types::DeletedTargetResolution::LocalDeleteWins),
                        None,
                    )
                }
                crate::sync::types::TargetOp::Upsert => {
                    log::info!("[sync] run_transfer: DeleteRemoteProject RemoteWinner(Upsert) {} — switching to restore", planned.target.remote_prefix);
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
