use crate::sync::types::{SyncFileEntry, SyncKind, SyncPlan};
use crate::sync::SyncService;
use std::path::Path;

#[allow(clippy::cast_possible_wrap)]
pub(crate) fn scan_workspace_for_sync(workspace_path: &Path) -> crate::Result<Vec<SyncFileEntry>> {
    let mut entries = Vec::new();

    for entry in walkdir::WalkDir::new(workspace_path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let absolute_path = entry.path().to_path_buf();

        let rel_path = match absolute_path.strip_prefix(workspace_path) {
            Ok(p) => p.to_string_lossy().replace("\\", "/"),
            Err(_) => continue,
        };

        if rel_path.starts_with(".git/") || rel_path == ".git" {
            continue;
        }

        let modified_time = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::SystemTime::now())
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let modified_time = modified_time as i64;

        let file_hash = SyncService::compute_file_hash(&absolute_path).unwrap_or_default();

        let sync_kind = if SyncService::is_whitelisted_path(&rel_path) {
            SyncKind::Upload
        } else {
            SyncKind::Ignore
        };

        entries.push(SyncFileEntry {
            relative_path: rel_path,
            absolute_path: absolute_path.to_string_lossy().into_owned(),
            file_hash,
            modified_time,
            sync_kind,
        });
    }

    Ok(entries)
}

pub(crate) fn build_sync_plan_from_workspace(workspace_path: &Path) -> crate::Result<SyncPlan> {
    let mut plan = SyncPlan::new();

    let entries = scan_workspace_for_sync(workspace_path)?;
    let state = SyncService::load_sync_state(workspace_path).unwrap_or_default();
    let is_first_sync = state.known_files.is_empty();

    let mut local_files = std::collections::HashSet::new();

    for entry in entries {
        if SyncService::is_blacklisted_path(&entry.relative_path) {
            plan.ignored_files.push(entry.relative_path.clone());
            continue;
        }

        if entry.sync_kind == SyncKind::Upload || entry.sync_kind == SyncKind::ConflictCandidate {
            local_files.insert(entry.relative_path.clone());
            let known_hash_opt = state.known_files.get(&entry.relative_path);
            if is_first_sync {
                plan.files_to_upload.push(entry.relative_path.clone());
            } else if let Some(kh) = known_hash_opt {
                if *kh != entry.file_hash {
                    plan.files_to_upload.push(entry.relative_path.clone());
                }
            } else {
                plan.files_to_upload.push(entry.relative_path.clone());
            }
        } else {
            plan.ignored_files.push(entry.relative_path.clone());
        }
    }

    if !is_first_sync {
        for known_path in state.known_files.keys() {
            if !local_files.contains(known_path) {
                plan.files_to_delete_remote.push(known_path.clone());
            }
        }
    }

    let now = chrono::Utc::now().timestamp();
    for t in &state.tombstones {
        if t.purge_after <= now {
            plan.files_to_delete_local.push(t.trash_path.clone());
        }
    }

    Ok(plan)
}
