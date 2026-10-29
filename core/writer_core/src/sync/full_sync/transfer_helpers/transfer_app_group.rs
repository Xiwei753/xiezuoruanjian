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

        // No persisted catalog yet: every discovered group still lives in the
        // legacy flat `app/` tree, even though the bootstrap catalog itself is
        // already schema 3.
        let source_prefix = if !catalog_snapshot.catalog_present {
            "app".to_string()
        } else {
            match catalog_snapshot.catalog.record(group) {
                Some(_) => {
                    match crate::sync::app_data_group_catalog::resolve_group_source_prefix(
                        Some(&catalog_snapshot.catalog),
                        group,
                    ) {
                        Ok(prefix) if !prefix.is_empty() => prefix,
                        Ok(_) => target.remote_prefix.clone(),
                        Err(error) => {
                            return mark_app_group_failure(
                                sync_result_from_error(error),
                                "catalog_read",
                            )
                        }
                    }
                }
                // A persisted catalog is authoritative. The group's base prefix
                // may contain incomplete/orphaned generations, so query a
                // never-published child as the empty remote source.
                None => format!("{}/__empty_source__", target.remote_prefix),
            }
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

        // A schema-2 catalog written by the intermediate sidecar build may
        // point at the old generation while the stable tree already contains
        // newer objects.  Merge both sources through the normal object-level
        // LWW/tombstone path, then migrate the catalog to schema 3 below.
        let transitional_stable = catalog_snapshot.is_transitional_stable(group);
        let mut merge_prefixes: Vec<&str> = Vec::new();
        if transitional_stable && target.remote_prefix != source_prefix {
            merge_prefixes.push(target.remote_prefix.as_str());
        }
        merge_prefixes.push(source_prefix.as_str());

        let mut outcome = match merge_app_group_sources(
            provider,
            sync_root,
            &merge_prefixes,
            group,
            &mut state,
        ) {
            Ok(outcome) => outcome,
            Err(error) => return mark_app_group_failure(sync_result_from_error(error), "merge"),
        };
        if transitional_stable {
            // The merged manifest may contain blobs from either source.  Force
            // a complete upload for this one migration commit instead of
            // reusing a version from only one of the two source trees.
            outcome.remote_upload_paths = outcome
                .merged_manifest
                .files
                .iter()
                .filter(|record| record.op == "upsert")
                .map(|record| record.path.clone())
                .collect();
            outcome.remote_tree_files.clear();
        }
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

        let content_result = super::super::generation::publish_tree_snapshot(
            provider,
            sync_root,
            &target.remote_prefix,
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
            retry_failure_phase = "tree_publish";
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
            return mark_app_group_failure(content_result, "tree_publish");
        }

        catalog_snapshot.catalog.upsert_tree(group);
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

fn merge_app_group_sources(
    provider: &dyn SyncProvider,
    sync_root: &Path,
    prefixes: &[&str],
    group: &crate::sync::types::AppDataGroup,
    state: &mut crate::sync::types::SyncState,
) -> crate::error::Result<crate::sync::lww::LwwMergeOutcome> {
    let mut combined: Option<crate::sync::lww::LwwMergeOutcome> = None;
    for prefix in prefixes {
        let outcome = crate::sync::lww::merge_remote_into_local_app_group(
            sync_root, provider, prefix, group, state,
        )?;
        combined = Some(match combined {
            None => outcome,
            Some(mut aggregate) => {
                extend_unique(&mut aggregate.downloaded_files, outcome.downloaded_files);
                extend_unique(&mut aggregate.local_deletes, outcome.local_deletes);
                extend_unique(
                    &mut aggregate.remote_upload_paths,
                    outcome.remote_upload_paths,
                );
                extend_unique(
                    &mut aggregate.remote_delete_paths,
                    outcome.remote_delete_paths,
                );
                extend_unique(&mut aggregate.overwritten_files, outcome.overwritten_files);
                extend_unique(&mut aggregate.ignored_files, outcome.ignored_files);
                extend_unique(
                    &mut aggregate.pending_take_remote_failed,
                    outcome.pending_take_remote_failed,
                );
                extend_unique(
                    &mut aggregate.unresolved_missing_paths,
                    outcome.unresolved_missing_paths,
                );
                for conflict in outcome.conflicts {
                    if !aggregate
                        .conflicts
                        .iter()
                        .any(|existing| existing.local_path == conflict.local_path)
                    {
                        aggregate.conflicts.push(conflict);
                    }
                }
                // The last merge sees the local state updated by every
                // previous source, so its complete manifest is authoritative.
                aggregate.remote_tree_files = outcome.remote_tree_files;
                aggregate.remote_manifest_path = outcome.remote_manifest_path;
                aggregate.manifest_json = outcome.manifest_json;
                aggregate.merged_manifest = outcome.merged_manifest;
                aggregate
            }
        });
    }
    combined.ok_or_else(|| {
        crate::Error::Other("merge_app_group_sources: no source prefixes".to_string())
    })
}

fn extend_unique(target: &mut Vec<String>, source: Vec<String>) {
    for item in source {
        if !target.contains(&item) {
            target.push(item);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::lww::LwwMergeOutcome;
    use crate::sync::provider::memory::MemoryProvider;
    use crate::sync::types::{AppDataGroup, ManifestFileRecord, SyncManifest, SyncState};

    const EVENT_PATH: &str = "stats/events/device-abc/2024-05-01.events.jsonl";

    fn stats_device_group() -> AppDataGroup {
        AppDataGroup::StatsDevice("device-abc".to_string())
    }

    fn prefixes() -> (String, String) {
        let stable = format!("app/__groups__/{}", stats_device_group().storage_key());
        let generation = format!("{stable}/__generations__/{}", "a".repeat(40));
        (stable, generation)
    }

    fn record(
        content_hash: &str,
        updated_at_ms: i64,
        op: &str,
        deleted_at_ms: Option<i64>,
    ) -> ManifestFileRecord {
        ManifestFileRecord {
            path: EVENT_PATH.to_string(),
            content_hash: content_hash.to_string(),
            updated_at_ms,
            deleted_at_ms,
            device_id: "remote-device".to_string(),
            op: op.to_string(),
            schema_version: 1,
        }
    }

    fn add_source(
        entries: &mut Vec<(String, Vec<u8>)>,
        prefix: &str,
        records: Vec<ManifestFileRecord>,
        blobs: &[(&str, &[u8])],
    ) {
        let manifest = SyncManifest { files: records };
        entries.push((
            format!("{prefix}/app-meta/sync/manifest.sync.json"),
            serde_json::to_vec(&manifest).unwrap(),
        ));
        for (path, content) in blobs {
            entries.push((format!("{prefix}/{path}"), content.to_vec()));
        }
    }

    fn merge_both(provider: &MemoryProvider, sync_root: &Path) -> LwwMergeOutcome {
        let (stable, generation) = prefixes();
        let mut state = SyncState::default();
        state.device_id = "local-device".to_string();
        merge_app_group_sources(
            provider,
            sync_root,
            &[stable.as_str(), generation.as_str()],
            &stats_device_group(),
            &mut state,
        )
        .unwrap()
    }

    fn merged_record<'a>(outcome: &'a LwwMergeOutcome, path: &str) -> &'a ManifestFileRecord {
        outcome
            .merged_manifest
            .files
            .iter()
            .find(|record| record.path == path)
            .expect("path must exist in merged manifest")
    }

    #[test]
    fn test_transitional_merge_keeps_newer_stable_object() {
        let now = chrono::Utc::now().timestamp_millis();
        let older = now - 60_000;
        let newer = now - 1_000;
        let stable_content = b"stable-new";
        let generation_content = b"generation-old";
        let (stable, generation) = prefixes();
        let mut entries = Vec::new();
        add_source(
            &mut entries,
            &stable,
            vec![record(
                &crate::sync::hash::content_md5(stable_content),
                newer,
                "upsert",
                None,
            )],
            &[(EVENT_PATH, stable_content)],
        );
        add_source(
            &mut entries,
            &generation,
            vec![record(
                &crate::sync::hash::content_md5(generation_content),
                older,
                "upsert",
                None,
            )],
            &[(EVENT_PATH, generation_content)],
        );
        let provider = MemoryProvider::with_entries(entries);
        let root = tempfile::tempdir().unwrap();

        let outcome = merge_both(&provider, root.path());

        assert_eq!(
            std::fs::read(root.path().join(EVENT_PATH)).unwrap(),
            stable_content
        );
        assert_eq!(merged_record(&outcome, EVENT_PATH).updated_at_ms, newer);
    }

    #[test]
    fn test_transitional_merge_keeps_newer_generation_object() {
        let now = chrono::Utc::now().timestamp_millis();
        let older = now - 60_000;
        let newer = now - 1_000;
        let stable_content = b"stable-old";
        let generation_content = b"generation-new";
        let (stable, generation) = prefixes();
        let mut entries = Vec::new();
        add_source(
            &mut entries,
            &stable,
            vec![record(
                &crate::sync::hash::content_md5(stable_content),
                older,
                "upsert",
                None,
            )],
            &[(EVENT_PATH, stable_content)],
        );
        add_source(
            &mut entries,
            &generation,
            vec![record(
                &crate::sync::hash::content_md5(generation_content),
                newer,
                "upsert",
                None,
            )],
            &[(EVENT_PATH, generation_content)],
        );
        let provider = MemoryProvider::with_entries(entries);
        let root = tempfile::tempdir().unwrap();

        let outcome = merge_both(&provider, root.path());

        assert_eq!(
            std::fs::read(root.path().join(EVENT_PATH)).unwrap(),
            generation_content
        );
        assert_eq!(merged_record(&outcome, EVENT_PATH).updated_at_ms, newer);
    }

    #[test]
    fn test_transitional_merge_keeps_newer_stable_delete() {
        let now = chrono::Utc::now().timestamp_millis();
        let older = now - 60_000;
        let newer = now - 1_000;
        let (stable, generation) = prefixes();
        let mut entries = Vec::new();
        add_source(
            &mut entries,
            &stable,
            vec![record("", newer, "delete", Some(newer))],
            &[],
        );
        add_source(
            &mut entries,
            &generation,
            vec![record(
                &crate::sync::hash::content_md5(b"generation-old"),
                older,
                "upsert",
                None,
            )],
            &[(EVENT_PATH, b"generation-old")],
        );
        let provider = MemoryProvider::with_entries(entries);
        let root = tempfile::tempdir().unwrap();

        let outcome = merge_both(&provider, root.path());

        assert!(!root.path().join(EVENT_PATH).exists());
        let record = merged_record(&outcome, EVENT_PATH);
        assert_eq!(record.op, "delete");
        assert_eq!(record.deleted_at_ms, Some(newer));
    }

    #[test]
    fn test_transitional_merge_keeps_newer_generation_delete() {
        let now = chrono::Utc::now().timestamp_millis();
        let older = now - 60_000;
        let newer = now - 1_000;
        let (stable, generation) = prefixes();
        let mut entries = Vec::new();
        add_source(
            &mut entries,
            &stable,
            vec![record(
                &crate::sync::hash::content_md5(b"stable-old"),
                older,
                "upsert",
                None,
            )],
            &[(EVENT_PATH, b"stable-old")],
        );
        add_source(
            &mut entries,
            &generation,
            vec![record("", newer, "delete", Some(newer))],
            &[],
        );
        let provider = MemoryProvider::with_entries(entries);
        let root = tempfile::tempdir().unwrap();

        let outcome = merge_both(&provider, root.path());

        assert!(!root.path().join(EVENT_PATH).exists());
        let record = merged_record(&outcome, EVENT_PATH);
        assert_eq!(record.op, "delete");
        assert_eq!(record.deleted_at_ms, Some(newer));
    }
}
