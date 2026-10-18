//! Independent immutable-generation transfer for one App data group.

use super::*;

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
pub(super) fn transfer_app_data_group(
    provider: &dyn SyncProvider,
    sync_root: &Path,
    sync_policy: &SyncPolicy,
    target: &SyncTarget,
    force_sync: bool,
    cancellation_token: Option<&SyncCancellationToken>,
) -> SyncResult {
    use crate::sync::types::SyncStatus;

    const MAX_CAS_RETRIES: usize = 5;
    let Some(group) = target.app_group.as_ref() else {
        return sync_result_from_error(crate::Error::Other(
            "transfer_app_data_group called for an ungrouped target".to_string(),
        ));
    };

    let mut retry_failure_phase = "catalog_publish";
    for attempt in 0..MAX_CAS_RETRIES {
        if cancellation_token.is_some_and(SyncCancellationToken::is_cancelled) {
            return cancelled_app_group_result();
        }

        let mut catalog_snapshot =
            match crate::sync::app_data_group_catalog::load_remote_app_data_group_catalog(provider)
            {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    return mark_app_group_failure(sync_result_from_error(error), "catalog_read")
                }
            };

        // Bootstrap catalog records from the legacy manifest as `None` (legacy app/ source).
        // A local-only group not present in the catalog is new and starts from an empty grouped
        // prefix once v2 is already active.
        let source_prefix = match catalog_snapshot.catalog.record(group) {
            Some(record) => match crate::sync::app_data_group_catalog::resolve_group_source_prefix(
                Some(&catalog_snapshot.catalog),
                group,
            ) {
                Ok(prefix) if !prefix.is_empty() => prefix,
                Ok(_) => target.remote_prefix.clone(),
                Err(error) => {
                    return mark_app_group_failure(sync_result_from_error(error), "catalog_read")
                }
            },
            None if !catalog_snapshot.catalog_present => "app".to_string(),
            // A v2 catalog is authoritative. The group's base prefix may contain
            // incomplete or orphaned generations, so query a never-published child
            // as the empty remote source for a newly introduced group.
            None => format!("{}/__empty_source__", target.remote_prefix),
        };

        let mut state =
            match crate::sync::SyncService::load_sync_state_for_target(sync_root, target) {
                Ok(state) => state,
                Err(error) => {
                    return mark_app_group_failure(sync_result_from_error(error), "merge")
                }
            };
        if state.device_id.is_empty() {
            state.device_id = uuid::Uuid::new_v4().to_string();
            if let Err(error) =
                crate::sync::SyncService::save_sync_state_for_target(sync_root, target, &state)
            {
                return mark_app_group_failure(sync_result_from_error(error), "merge");
            }
        }

        let outcome = match crate::sync::lww::merge_remote_into_local_app_group(
            sync_root,
            provider,
            &source_prefix,
            group,
            &mut state,
        ) {
            Ok(outcome) => outcome,
            Err(error) => return mark_app_group_failure(sync_result_from_error(error), "merge"),
        };
        if !outcome.unresolved_missing_paths.is_empty() {
            let paths = outcome.unresolved_missing_paths.join(", ");
            let failure_path_json = outcome
                .unresolved_missing_paths
                .first()
                .and_then(|path| serde_json::to_string(path).ok())
                .unwrap_or_else(|| "\"\"".to_string());
            let message = format!(
                "local_missing_without_tombstone: target={} failure_path_json={} paths=[{}] — cannot publish this App group",
                group.id(), failure_path_json, paths
            );
            let mut result =
                SyncResult::error(SyncStatus::RecoverableError(message.clone()), message, None);
            result.downloaded_files = outcome.downloaded_files;
            result.remote_deletes = outcome.local_deletes;
            return result;
        }
        if matches!(group, crate::sync::types::AppDataGroup::StarmapIndex)
            && !sync_root.join("starmaps/index.json").is_file()
            && !outcome
                .merged_manifest
                .files
                .iter()
                .any(|record| record.path == "starmaps/index.json" && record.op == "delete")
        {
            let message = format!(
                "local_missing_without_tombstone: target={} paths=[starmaps/index.json] — index is absent and no remote source or tombstone restored it",
                group.id()
            );
            return SyncResult::error(SyncStatus::RecoverableError(message.clone()), message, None);
        }

        let generation_id = uuid::Uuid::new_v4().to_string();
        let generation_prefix = match super::super::generation::generation_remote_prefix(
            &target.remote_prefix,
            &generation_id,
        ) {
            Ok(prefix) => prefix,
            Err(error) => {
                return mark_app_group_failure(sync_result_from_error(error), "generation_upload")
            }
        };
        let content_result = super::super::generation::publish_generation(
            provider,
            sync_root,
            &generation_prefix,
            &generation_id,
            &group.id(),
            target.scope,
            Some(group),
            sync_policy,
            force_sync,
            Some(&outcome),
            cancellation_token,
        );

        // A Git branch CAS can fail because another target/device committed while this
        // generation was uploading. Keep the unreferenced generation and re-merge from the
        // latest selected source on retry.
        if content_result.error_category.as_deref() == Some("precondition_failed") {
            retry_failure_phase = "generation_upload";
            log::info!(
                "[sync] transfer_app_data_group: generation CAS conflict group={} attempt={}",
                group.id(),
                attempt + 1
            );
            continue;
        }
        let content_ok = matches!(
            &content_result.status,
            SyncStatus::Success | SyncStatus::NoChanges | SyncStatus::LatestWinsApplied
        );
        if !content_ok {
            return mark_app_group_failure(content_result, "generation_upload");
        }

        catalog_snapshot
            .catalog
            .upsert(group, Some(generation_id.clone()));
        match crate::sync::app_data_group_catalog::write_remote_app_data_group_catalog_once(
            provider,
            &catalog_snapshot,
        ) {
            Ok(_persisted) => return content_result,
            Err(error) if error.sync_category() == "precondition_failed" => {
                retry_failure_phase = "catalog_publish";
                log::info!(
                    "[sync] transfer_app_data_group: catalog CAS conflict group={} attempt={}",
                    group.id(),
                    attempt + 1
                );
                continue;
            }
            Err(error) => {
                return mark_app_group_failure(sync_result_from_error(error), "catalog_publish")
            }
        }
    }

    let message = format!(
        "app data group catalog changed during {} retries: {}",
        MAX_CAS_RETRIES,
        group.id()
    );
    mark_app_group_failure(
        SyncResult::error(
            SyncStatus::RecoverableError(message.clone()),
            message,
            Some("precondition_failed".to_string()),
        ),
        retry_failure_phase,
    )
}

fn mark_app_group_failure(mut result: SyncResult, phase: &str) -> SyncResult {
    if let Some(error) = result.error.take() {
        result.error = Some(format!("{phase}: {error}"));
    }
    result
}

fn cancelled_app_group_result() -> SyncResult {
    mark_app_group_failure(
        sync_result_from_error(crate::Error::Other(
            "sync cancelled during App group transfer".to_string(),
        )),
        "transfer",
    )
}
