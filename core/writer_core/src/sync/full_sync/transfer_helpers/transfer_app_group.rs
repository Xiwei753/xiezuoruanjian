//! Independent immutable-generation transfer for one App data group.

use super::*;

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
pub(in crate::sync::full_sync) fn transfer_app_data_group(
    provider: &dyn SyncProvider,
    sync_root: &Path,
    sync_policy: &SyncPolicy,
    target: &SyncTarget,
    force_sync: bool,
    cancellation_token: Option<&SyncCancellationToken>,
    starmap_group_completion: &std::collections::HashMap<String, bool>,
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
            Some(_) => match crate::sync::app_data_group_catalog::resolve_group_source_prefix(
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

        if matches!(group, crate::sync::types::AppDataGroup::StarmapIndex)
            && sync_root.join("starmaps/index.json").is_file()
        {
            let referenced_ids = match read_final_starmap_index_ids(sync_root) {
                Ok(ids) => ids,
                Err(reason) => {
                    let message = format!(
                        "dependency_check: failed to read final starmaps/index.json path=starmaps/index.json reason={reason}"
                    );
                    let mut result = SyncResult::error(
                        SyncStatus::RecoverableError(message.clone()),
                        message,
                        None,
                    );
                    result.error_category = Some("dependency_check".to_string());
                    return result;
                }
            };
            let mut blocked: Vec<String> = referenced_ids
                .into_iter()
                .filter(|starmap_id| {
                    starmap_group_completion
                        .get(starmap_id)
                        .is_some_and(|completed| !completed)
                })
                .collect();
            blocked.sort();
            if !blocked.is_empty() {
                let message = format!(
                    "dependency_check: starmap index publication blocked because referenced graph groups did not complete: {} path=starmaps/index.json",
                    blocked.join(", ")
                );
                let mut result =
                    SyncResult::error(SyncStatus::RecoverableError(message.clone()), message, None);
                result.error_category = Some("dependency_check".to_string());
                return result;
            }
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

        // Issue #990：Git HTTPS 在一轮完整同步内固定远端快照，同一个 provider
        // 无法读到别的设备刚推进的 HEAD。generation 发布遇到 precondition_failed
        // 时保留分类直接返回，由完整同步入口新建 provider、重新读取 catalog/latest
        // source 并重新做 LWW；不要在本轮循环里重放旧快照。
        if content_result.error_category.as_deref() == Some("precondition_failed") {
            retry_failure_phase = "generation_upload";
            log::info!(
                "[sync] transfer_app_data_group: generation precondition failed group={} attempt={} — returning to full-sync replan",
                group.id(),
                attempt + 1
            );
            return mark_app_group_failure(content_result, retry_failure_phase);
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
                    "[sync] transfer_app_data_group: catalog precondition failed group={} attempt={} — returning to full-sync replan",
                    group.id(),
                    attempt + 1
                );
                return mark_app_group_failure(sync_result_from_error(error), retry_failure_phase);
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

fn read_final_starmap_index_ids(
    sync_root: &Path,
) -> Result<std::collections::HashSet<String>, String> {
    let index_path = sync_root.join("starmaps/index.json");
    let bytes = std::fs::read(&index_path).map_err(|error| error.to_string())?;
    let index: crate::starmap::StarMapIndexRecord =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let mut ids: std::collections::HashSet<String> = index.starmap_ids.into_iter().collect();
    ids.extend(index.root_starmap_ids);
    ids.extend(index.main_starmap_by_project.into_values());
    Ok(ids)
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
