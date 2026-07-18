use crate::sync::github_api_client::{
    github_delete_content_serial, github_get_content, github_put_content_serial,
};
use crate::sync::github_backend::GitHubApiBackend;
use crate::sync::scanner::scan_workspace_for_sync;
use crate::sync::types::{
    FirstSyncMode, ManifestFileRecord, SyncConfig, SyncConflict, SyncKind, SyncManifest,
    SyncResult, SyncSecrets, SyncState, SyncStatus,
};
use crate::sync::SyncService;
use rayon::prelude::*;
use std::path::Path;

const SYNC_MANIFEST_PATH: &str = "app-meta/sync/manifest.sync.json";
const MAX_PARALLEL_DOWNLOADS: usize = 4;

fn sync_download_pool(task_count: usize) -> crate::Result<rayon::ThreadPool> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(task_count.clamp(1, MAX_PARALLEL_DOWNLOADS))
        .build()
        .map_err(|e| crate::Error::Io(std::io::Error::other(format!("sync_parallel_pool_error: {}", e))))
}

fn save_conflict_copy(
    workspace_path: &Path,
    path: &str,
    remote_content: &[u8],
) -> crate::Result<String> {
    let full_path = workspace_path.join(path);
    let filename = full_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let conflict_filename = format!("{}.remote-conflict-{}", filename, timestamp);

    let conflict_path = full_path
        .parent()
        .unwrap_or(&full_path)
        .join(&conflict_filename);

    if let Some(parent) = conflict_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    std::fs::write(&conflict_path, remote_content).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "write conflict copy {}: {}",
            path, e
        )))
    })?;

    Ok(conflict_filename)
}

fn lww_record_time(record: &ManifestFileRecord) -> i64 {
    if record.op == "delete" {
        record.deleted_at_ms.unwrap_or(record.updated_at_ms)
    } else {
        record.updated_at_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::types::ManifestFileRecord;

    #[test]
    fn test_lww_record_time_non_delete() {
        let record = ManifestFileRecord {
            path: "test.md".to_string(),
            content_hash: "hash".to_string(),
            op: "upsert".to_string(),
            updated_at_ms: 1000,
            deleted_at_ms: Some(2000), // Should be ignored
            device_id: "dev1".to_string(),
            schema_version: 1,
        };
        assert_eq!(lww_record_time(&record), 1000);
    }

    #[test]
    fn test_lww_record_time_delete_with_deleted_at_ms() {
        let record = ManifestFileRecord {
            path: "test.md".to_string(),
            content_hash: "hash".to_string(),
            op: "delete".to_string(),
            updated_at_ms: 1000,
            deleted_at_ms: Some(2000),
            device_id: "dev1".to_string(),
            schema_version: 1,
        };
        assert_eq!(lww_record_time(&record), 2000);
    }

    #[test]
    fn test_lww_record_time_delete_without_deleted_at_ms() {
        let record = ManifestFileRecord {
            path: "test.md".to_string(),
            content_hash: "hash".to_string(),
            op: "delete".to_string(),
            updated_at_ms: 1000,
            deleted_at_ms: None,
            device_id: "dev1".to_string(),
            schema_version: 1,
        };
        assert_eq!(lww_record_time(&record), 1000); // Fallback to updated_at_ms
    }

    #[test]
    fn test_lww_record_time_tie_breaker_with_deleted_at_ms() {
        // Local is a newer edit based on updated_at_ms
        let local_rec = ManifestFileRecord {
            path: "a.txt".to_string(),
            content_hash: "hash1".to_string(),
            op: "upsert".to_string(),
            updated_at_ms: 1500,
            deleted_at_ms: None,
            device_id: "dev1".to_string(),
            schema_version: 1,
        };
        // Remote is a delete. updated_at_ms is older, but deleted_at_ms is newer
        let remote_rec = ManifestFileRecord {
            path: "a.txt".to_string(),
            content_hash: "hash1".to_string(),
            op: "delete".to_string(),
            updated_at_ms: 1000,
            deleted_at_ms: Some(2000),
            device_id: "dev2".to_string(),
            schema_version: 1,
        };

        let local_time = lww_record_time(&local_rec);
        let remote_time = lww_record_time(&remote_rec);

        // Remote time should be 2000 (from deleted_at_ms) and win against local's 1500
        assert!(remote_time > local_time);
        assert_eq!(remote_time, 2000);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentClass {
    /// User-authored text: chapter.md, note.md, outline.md, scene.md, etc.
    /// Three-way merge on sync; never silently overwritten by LWW.
    UserTextDocument,
    /// Project/volume/chapter metadata JSON. LWW or semantic merge.
    Metadata,
    /// Local-only data (backups, app-meta internals). Never synced.
    LocalOnly,
    /// Generated or cache data. LWW is acceptable.
    GeneratedCache,
}

/// Classify a workspace-relative path into a content category.
///
/// Uses suffix-based rules so it works for any project/volume/chapter ID.
/// The path is normalized to forward slashes before matching to ensure
/// correct behavior on Windows where local paths may contain backslashes.
pub(crate) fn classify_content_path(raw_path: &str) -> ContentClass {
    // Normalize backslashes to forward slashes for consistent matching on Windows
    let path = if raw_path.contains('\\') {
        std::borrow::Cow::Owned(raw_path.replace('\\', "/"))
    } else {
        std::borrow::Cow::Borrowed(raw_path)
    };

    // Local-only directories
    if path.starts_with("backups/") || path.starts_with("app-meta/") {
        return ContentClass::LocalOnly;
    }

    // User text documents: any .md file under /chapters/, plus
    // note.md, outline.md, scene.md, character_notes.md, timeline_notes.md
    // anywhere in the workspace
    if path.ends_with(".md") {
        if path.contains("/chapters/") {
            return ContentClass::UserTextDocument;
        }
        let filename = path.rsplit('/').next().unwrap_or(&path);
        if matches!(
            filename,
            "note.md"
                | "outline.md"
                | "scene.md"
                | "character_notes.md"
                | "timeline_notes.md"
                | "draft.md"
        ) {
            return ContentClass::UserTextDocument;
        }
        return ContentClass::GeneratedCache;
    }

    // Metadata JSON files
    if path.ends_with(".json") {
        let filename = path.rsplit('/').next().unwrap_or(&path);
        if matches!(
            filename,
            "project.json"
                | "volume.json"
                | "chapter.meta.json"
                | "settings.sync.json"
                | "starmap.json"
                | "writing_stats.json"
        ) {
            return ContentClass::Metadata;
        }
    }

    ContentClass::GeneratedCache
}

pub(crate) fn is_document_content_path(path: &str) -> bool {
    classify_content_path(path) == ContentClass::UserTextDocument
}

fn three_way_resolve(base_hash: &str, local_hash: &str, remote_hash: &str) -> ThreeWayResult {
    if local_hash == remote_hash {
        return ThreeWayResult::NoConflict;
    }
    if local_hash == base_hash && remote_hash != base_hash {
        return ThreeWayResult::RemoteChanged;
    }
    if local_hash != base_hash && remote_hash == base_hash {
        return ThreeWayResult::LocalChanged;
    }
    if local_hash != base_hash && remote_hash != base_hash {
        return ThreeWayResult::BothChanged;
    }
    ThreeWayResult::NoConflict
}

enum ThreeWayResult {
    NoConflict,
    LocalChanged,
    RemoteChanged,
    BothChanged,
}

pub(crate) fn perform_lww_sync(
    workspace_path: &Path,
    config: &SyncConfig,
    secrets: &SyncSecrets,
    force_sync: bool,
) -> crate::Result<SyncResult> {
    log::debug!(
        "[sync] backend_type=github_api sync_mode=lww_manifest entry=perform_lww_sync workspace={}",
        workspace_path.display()
    );
    let mut result = SyncResult::success();
    result.status = SyncStatus::Idle;

    if !config.enabled {
        result.status = SyncStatus::Success;
        return Ok(result);
    }

    if config.remote_url.is_empty() {
        return Ok(SyncResult::error(
            SyncStatus::Error("Remote URL is empty".to_string()),
            FirstSyncMode::NotAttempted,
            "Remote URL is empty".to_string(),
            Some("empty_url".to_string()),
        ));
    }

    let token = secrets.token.clone().unwrap_or_default();
    if token.is_empty() {
        return Ok(SyncResult::error(
            SyncStatus::Error("No token provided".to_string()),
            FirstSyncMode::NotAttempted,
            "No token provided".to_string(),
            Some("token_missing".to_string()),
        ));
    }

    let mut state = crate::sync::SyncService::load_sync_state(workspace_path)?;
    if state.device_id.is_empty() {
        state.device_id = uuid::Uuid::new_v4().to_string();
        crate::sync::SyncService::save_sync_state(workspace_path, &state)?;
    }

    // P1-4: Core-level debounce. Even if clients call sync too often,
    // the core enforces a minimum interval to prevent network I/O flood.
    // This is a safety net; clients should also debounce.
    // However, force_sync=true bypasses this debounce for manual sync,
    // conflict resolution, and first configuration.
    if !force_sync {
        let min_interval = i64::from(config.sync_interval_seconds.max(60));
        if let Some(last_sync) = state.last_sync_time {
            let now = chrono::Utc::now().timestamp();
            let elapsed = now - last_sync;
            if elapsed >= 0 && elapsed < min_interval {
                // 冲突解决后（pending_take_remote 非空）必须绕过 debounce，
                // 否则用户解决冲突后可能要等 60 秒才能同步到远端内容
                if !state.pending_take_remote.is_empty() {
                    log::debug!(
                        "[sync] debounce bypassed: pending_take_remote has {} entries",
                        state.pending_take_remote.len()
                    );
                } else {
                    log::debug!(
                        "[sync] debounce: last_sync={}s ago, min_interval={}s, skipping",
                        elapsed, min_interval
                    );
                    result.status = SyncStatus::Success;
                    return Ok(result);
                }
            }
        }
    }

    let api_base = GitHubApiBackend::api_base_url(&config.remote_url);
    let client = match GitHubApiBackend::build_client() {
        Ok(c) => c,
        Err(e) => {
            result.error = Some(e.to_string());
            result.status = SyncStatus::RecoverableError(e.to_string());
            return Ok(result);
        }
    };

    let max_retries = 2;
    let mut attempt = 0;
    loop {
        match execute_lww_sync_attempt(
            workspace_path,
            config,
            &token,
            &api_base,
            &client,
            &mut state,
            &mut result,
        ) {
            Ok(res) => return Ok(res),
            Err(e) => {
                attempt += 1;
                if attempt >= max_retries {
                    let err = e.to_string();
                    let category = crate::sync::types::SyncErrorCategory::from_code(
                        e.sync_category(),
                        &err,
                    );
                    result.status = match category {
                        crate::sync::types::SyncErrorCategory::LocalIoError => {
                            SyncStatus::Error("local_io_error".to_string())
                        }
                        crate::sync::types::SyncErrorCategory::TokenMissing
                        | crate::sync::types::SyncErrorCategory::TokenInvalid
                        | crate::sync::types::SyncErrorCategory::TokenPermissionDenied
                        | crate::sync::types::SyncErrorCategory::AuthError => {
                            SyncStatus::Error(category.to_ui_status().to_string())
                        }
                        crate::sync::types::SyncErrorCategory::ApiRateLimited => {
                            SyncStatus::RecoverableError("api_rate_limited".to_string())
                        }
                        crate::sync::types::SyncErrorCategory::GithubNetworkFailed
                        | crate::sync::types::SyncErrorCategory::DnsFailed
                        | crate::sync::types::SyncErrorCategory::TlsFailed
                        | crate::sync::types::SyncErrorCategory::NetworkProbeFailed => {
                            SyncStatus::RecoverableError("network_error".to_string())
                        }
                        _ => SyncStatus::RecoverableError("api_error".to_string()),
                    };
                    result.error = Some(err.clone());
                    return Ok(result);
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
fn execute_lww_sync_attempt(
    workspace_path: &Path,
    config: &SyncConfig,
    token: &str,
    api_base: &str,
    client: &reqwest::blocking::Client,
    state: &mut SyncState,
    result: &mut SyncResult,
) -> crate::Result<SyncResult> {
    log::debug!("[sync] github_api step=正在拉取远端清单");
    let tree_url = format!("{}/git/trees/{}?recursive=1", api_base, config.branch);
    let resp = client
        .get(&tree_url)
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "WriterApp/1.0")
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| crate::Error::SyncNetworkUnavailable { reason: e.to_string() })?;

    let mut remote_tree_files = std::collections::HashMap::new();
    let tree_status = resp.status();
    let tree_body = resp
        .text()
        .map_err(|e| crate::Error::SyncNetworkUnavailable { reason: e.to_string() })?;
    if tree_status.as_u16() == 200 {
        let json: serde_json::Value = serde_json::from_str(&tree_body)
            .map_err(|e| crate::Error::SyncGithubApiError {
                category: "api_error".to_string(),
                context: format!("invalid tree json: {}", e),
                status: 0,
                body_preview: String::new(),
            })?;
        if json["truncated"].as_bool().unwrap_or(false) {
            return Err(crate::Error::SyncGithubApiError {
                category: "api_error".to_string(),
                context: "GitHub tree response truncated, repository is too large".to_string(),
                status: 0,
                body_preview: String::new(),
            });
        }
        if let Some(tree) = json["tree"].as_array() {
            for item in tree {
                if item["type"].as_str() == Some("blob") {
                    if let (Some(path), Some(sha)) = (item["path"].as_str(), item["sha"].as_str()) {
                        remote_tree_files.insert(path.to_string(), sha.to_string());
                    }
                }
            }
        }
    } else if tree_status.as_u16() == 404 {
        // tree 404 不能直接当空远端：可能是仓库不存在、Token 无权限、或分支不存在。
        // 先调用 /git/ref/heads/{branch} 诊断接口来区分。
        let ref_url = format!("{}/git/ref/heads/{}", api_base, config.branch);
        let ref_resp = client
            .get(&ref_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "WriterApp/1.0")
            .header("Accept", "application/vnd.github+json")
            .send()
            .map_err(|e| crate::Error::SyncNetworkUnavailable { reason: e.to_string() })?;
        let ref_status = ref_resp.status().as_u16();
        if ref_status == 200 {
            // 仓库和分支都存在，tree 404 说明是空仓库，remote_tree_files 保持为空
            log::debug!(
                "[sync] tree 404 but ref 200: branch {} exists with empty tree, treating as empty remote",
                config.branch
            );
        } else if ref_status == 404 {
            // ref 也 404：可能是仓库不存在或 Token 无权限，也可能是分支不存在
            // 再尝试访问仓库本身来区分
            let repo_resp = client
                .get(api_base)
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "WriterApp/1.0")
                .header("Accept", "application/vnd.github+json")
                .send()
                .map_err(|e| crate::Error::SyncNetworkUnavailable { reason: e.to_string() })?;
            let repo_status = repo_resp.status().as_u16();
            if repo_status == 200 {
                // 仓库可访问但分支不存在
                return Err(crate::Error::SyncRemoteBranchNotFound {
                    detail: format!("branch '{}' not found in repository", config.branch),
                });
            } else if repo_status == 401 || repo_status == 403 {
                return Err(crate::Error::SyncGithubApiError {
                    category: "repo_not_found_or_no_permission".to_string(),
                    context: "token lacks access to repository".to_string(),
                    status: repo_status,
                    body_preview: String::new(),
                });
            } else {
                return Err(crate::Error::SyncGithubApiError {
                    category: "repo_not_found_or_no_permission".to_string(),
                    context: "repository not found or inaccessible".to_string(),
                    status: repo_status,
                    body_preview: String::new(),
                });
            }
        } else if ref_status == 401 || ref_status == 403 {
            return Err(crate::Error::SyncGithubApiError {
                category: "repo_not_found_or_no_permission".to_string(),
                context: "authentication failed or token lacks permission".to_string(),
                status: ref_status,
                body_preview: String::new(),
            });
        } else {
            return Err(crate::Error::SyncGithubApiError {
                category: "repo_not_found_or_no_permission".to_string(),
                context: format!("unexpected HTTP {} when checking ref", ref_status),
                status: ref_status,
                body_preview: String::new(),
            });
        }
    } else {
        return Err(crate::sync::github_api_client::github_api_error(
            "get recursive tree",
            tree_status,
            tree_body,
        ));
    }

    let mut remote_manifest = SyncManifest::default();
    if remote_tree_files.contains_key(SYNC_MANIFEST_PATH) {
        if let Some((content_bytes, _)) =
            github_get_content(client, api_base, token, &config.branch, SYNC_MANIFEST_PATH)?
        {
            remote_manifest =
                serde_json::from_slice::<SyncManifest>(&content_bytes).map_err(|e| {
                    crate::Error::SyncGithubApiError {
                        category: "api_error".to_string(),
                        context: format!("invalid remote manifest: {}", e),
                        status: 0,
                        body_preview: String::new(),
                    }
                })?;
        }
    }

    log::debug!("[sync] github_api step=正在比较本地和远端");
    let local_entries = scan_workspace_for_sync(workspace_path)?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut local_records = std::collections::HashMap::new();

    for entry in &local_entries {
        if entry.sync_kind == SyncKind::Upload && entry.relative_path != SYNC_MANIFEST_PATH {
            let path = entry.relative_path.clone();
            let local_hash = entry.file_hash.clone();

            let updated_at_ms;
            let op = "upsert".to_string();

            if let Some(known_hash) = state.known_files.get(&path) {
                if *known_hash == local_hash {
                    updated_at_ms = state
                        .known_files_updated_at
                        .get(&path)
                        .cloned()
                        .unwrap_or(0);
                } else {
                    let modified_ms = std::fs::metadata(workspace_path.join(&path))
                        .and_then(|m| m.modified())
                        .and_then(|t| {
                            t.duration_since(std::time::SystemTime::UNIX_EPOCH)
                                .map_err(std::io::Error::other)
                        })
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(now_ms);
                    updated_at_ms = modified_ms;
                }
            } else {
                let modified_ms = std::fs::metadata(workspace_path.join(&path))
                    .and_then(|m| m.modified())
                    .and_then(|t| {
                        t.duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .map_err(std::io::Error::other)
                    })
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(now_ms);
                updated_at_ms = modified_ms;
            }

            local_records.insert(
                path.clone(),
                ManifestFileRecord {
                    path,
                    content_hash: local_hash,
                    updated_at_ms,
                    deleted_at_ms: None,
                    device_id: state.device_id.clone(),
                    op,
                    schema_version: 1,
                },
            );
        }
    }

    for path in state.known_files.keys() {
        if !local_records.contains_key(path) {
            if !SyncService::is_whitelisted_path(path) || SyncService::is_blacklisted_path(path) {
                continue;
            }
            if !workspace_path.join(path).exists() {
                let mut updated_at_ms = now_ms;
                if let Some(tombstone) = state.tombstones.iter().find(|t| t.original_path == *path)
                {
                    updated_at_ms = tombstone.deleted_at * 1000;
                }

                local_records.insert(
                    path.clone(),
                    ManifestFileRecord {
                        path: path.clone(),
                        content_hash: String::new(),
                        updated_at_ms,
                        deleted_at_ms: Some(updated_at_ms),
                        device_id: state.device_id.clone(),
                        op: "delete".to_string(),
                        schema_version: 1,
                    },
                );
            }
        }
    }

    let mut remote_records = std::collections::HashMap::new();
    for rec in remote_manifest.files {
        if rec.path != SYNC_MANIFEST_PATH {
            remote_records.insert(rec.path.clone(), rec);
        }
    }

    for (path, sha) in &remote_tree_files {
        if path != SYNC_MANIFEST_PATH && !remote_records.contains_key(path) {
            if !SyncService::is_whitelisted_path(path) || SyncService::is_blacklisted_path(path) {
                continue;
            }
            remote_records.insert(
                path.clone(),
                ManifestFileRecord {
                    path: path.clone(),
                    content_hash: sha.clone(),
                    updated_at_ms: 0,
                    deleted_at_ms: None,
                    device_id: "remote".to_string(),
                    op: "upsert".to_string(),
                    schema_version: 1,
                },
            );
        }
    }

    // Build a quick-lookup set of unresolved conflict paths from the persisted state.
    // While a path remains in this set, the sync engine must not auto-upload,
    // auto-download, or apply LWW/three-way resolution to it.
    let unresolved_conflict_paths: std::collections::HashSet<String> =
        state.conflicted_files.clone();

    // ── Process pending_take_remote ──
    // For each path in pending_take_remote, force-download the remote content
    // to the local file, then update known_files to the new local hash.
    // This must happen BEFORE the three-way comparison loop so that the
    // downloaded content becomes the local version for the sync plan.
    //
    // CRITICAL: Regardless of whether the download succeeds or fails, the path
    // must NOT enter the normal three-way/LWW comparison loop. If it did, a
    // "local has, remote missing" scenario could cause the old local content to
    // be uploaded back, violating the "take remote" intent.
    let pending_take_remote_all_set: std::collections::HashSet<String> =
        state.pending_take_remote.clone();
    let mut pending_take_remote_downloaded: Vec<String> = Vec::new();
    let mut pending_take_remote_failed: Vec<String> = Vec::new();
    if !state.pending_take_remote.is_empty() {
        log::debug!(
            "[sync] processing pending_take_remote count={}",
            state.pending_take_remote.len()
        );
        let pending_paths: Vec<String> = state.pending_take_remote.iter().cloned().collect();
        let download_pool = sync_download_pool(pending_paths.len())?;
        let pending_results: crate::Result<Vec<_>> = download_pool.install(|| {
            pending_paths
                .par_iter()
                .map(|path| {
                    let remote =
                        github_get_content(client, api_base, token, &config.branch, path)?;
                    let Some((content, _sha)) = remote else {
                        return Ok((path.clone(), None));
                    };

                    let full_path = workspace_path.join(path);
                    if let Some(parent) = full_path.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| {
                            crate::Error::Io(std::io::Error::other(format!(
                                "create pending_take_remote dir {}: {}",
                                path, e
                            )))
                        })?;
                    }
                    let tmp_path =
                        full_path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
                    std::fs::write(&tmp_path, &content).map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "write pending_take_remote {}: {}",
                            path, e
                        )))
                    })?;
                    std::fs::rename(&tmp_path, &full_path).map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "rename pending_take_remote {}: {}",
                            path, e
                        )))
                    })?;
                    Ok((path.clone(), Some(content)))
                })
                .collect()
        });

        for (path, content) in pending_results? {
            if let Some(content) = content {
                let hash = format!("{:x}", md5::compute(&content));
                state.known_files.insert(path.clone(), hash);
                let now_ts = chrono::Utc::now().timestamp_millis();
                state.known_files_updated_at.insert(path.clone(), now_ts);
                pending_take_remote_downloaded.push(path.clone());
                log::debug!("[sync] pending_take_remote downloaded path={}", path);
            } else {
                log::debug!(
                    "[sync] pending_take_remote: remote file missing for path={}, keeping in pending",
                    path
                );
                pending_take_remote_failed.push(path);
            }
        }
        // Only clear paths that were successfully downloaded;
        // failed/missing paths remain in pending_take_remote so the user
        // is not silently left with stale local content.
        state
            .pending_take_remote
            .retain(|p| pending_take_remote_failed.contains(p));
    }

    let mut merged_manifest_files = std::collections::HashMap::new();
    let mut to_download = Vec::new();
    let mut to_upload = Vec::new();
    let mut to_delete_local = Vec::new();
    let mut local_deletes_count = Vec::new();
    let mut remote_deletes_count = Vec::new();
    let mut overwritten_files = Vec::new();
    let mut doc_conflicts: Vec<SyncConflict> = Vec::new();

    let all_paths: std::collections::HashSet<String> = local_records
        .keys()
        .cloned()
        .chain(remote_records.keys().cloned())
        .collect();

    for path in all_paths {
        // Skip ALL paths that were in pending_take_remote — both successfully
        // downloaded and failed/missing ones. A failed download must NOT fall
        // through to the normal three-way/LWW logic, which could upload the
        // old local content back (violating "take remote" intent).
        if pending_take_remote_all_set.contains(&path) {
            if pending_take_remote_downloaded.contains(&path) {
                // Successfully downloaded: use remote_rec in merged manifest
                if let Some(remote_rec) = remote_records.get(&path) {
                    merged_manifest_files.insert(path.clone(), remote_rec.clone());
                }
                result.ignored_files.push(path);
            } else {
                // Failed/missing: keep whichever record exists, but do NOT
                // schedule any upload/download/delete. The path remains in
                // pending_take_remote for the next sync attempt.
                if let Some(remote_rec) = remote_records.get(&path) {
                    merged_manifest_files.insert(path.clone(), remote_rec.clone());
                } else if let Some(local_rec) = local_records.get(&path) {
                    merged_manifest_files.insert(path.clone(), local_rec.clone());
                }
            }
            continue;
        }
        // Skip paths that have unresolved conflicts — do not auto-upload,
        // auto-download, or apply LWW/three-way resolution.
        if unresolved_conflict_paths.contains(&path) {
            log::debug!(
                "[sync] skipping unresolved_conflict path={} (awaiting user resolution)",
                path
            );
            // Keep the remote record in the merged manifest so the remote side
            // stays consistent, but do NOT schedule any upload/download/delete.
            if let Some(remote_rec) = remote_records.get(&path) {
                merged_manifest_files.insert(path.clone(), remote_rec.clone());
            } else if let Some(local_rec) = local_records.get(&path) {
                merged_manifest_files.insert(path.clone(), local_rec.clone());
            }
            continue;
        }

        let local_opt = local_records.get(&path);
        let remote_opt = remote_records.get(&path);

        match (local_opt, remote_opt) {
            (Some(local_rec), None) => {
                merged_manifest_files.insert(path.clone(), local_rec.clone());
                if local_rec.op == "upsert" {
                    to_upload.push(path);
                }
            }
            (None, Some(remote_rec)) => {
                merged_manifest_files.insert(path.clone(), remote_rec.clone());
                if remote_rec.op == "upsert" {
                    to_download.push(path);
                } else if remote_rec.op == "delete" {
                    to_delete_local.push(path.clone());
                    remote_deletes_count.push(path);
                }
            }
            (Some(local_rec), Some(remote_rec)) => {
                if is_document_content_path(&path) {
                    let base_hash = state
                        .known_files
                        .get(&path)
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    let local_hash = &local_rec.content_hash;
                    let remote_hash = &remote_rec.content_hash;

                    match three_way_resolve(base_hash, local_hash, remote_hash) {
                        ThreeWayResult::NoConflict => {
                            merged_manifest_files.insert(path.clone(), local_rec.clone());
                            result.ignored_files.push(path);
                        }
                        ThreeWayResult::LocalChanged => {
                            merged_manifest_files.insert(path.clone(), local_rec.clone());
                            to_upload.push(path);
                        }
                        ThreeWayResult::RemoteChanged => {
                            merged_manifest_files.insert(path.clone(), remote_rec.clone());
                            if remote_rec.op == "upsert" {
                                to_download.push(path);
                            } else if remote_rec.op == "delete" {
                                to_delete_local.push(path.clone());
                                remote_deletes_count.push(path);
                            }
                        }
                        ThreeWayResult::BothChanged => {
                            log::warn!(
                                "[sync] document_conflict path={} local_hash={} remote_hash={} base_hash={}",
                                path, local_hash, remote_hash, base_hash
                            );

                            let conflict = if remote_rec.op == "upsert" {
                                if let Some((remote_content, _)) = github_get_content(
                                    client,
                                    api_base,
                                    token,
                                    &config.branch,
                                    &path,
                                )? {
                                    let conflict_filename =
                                        save_conflict_copy(workspace_path, &path, &remote_content)?;

                                    Some(SyncConflict {
                                        local_path: path.clone(),
                                        remote_path: path.clone(),
                                        local_hash: local_hash.clone(),
                                        remote_hash: remote_hash.clone(),
                                        base_hash: base_hash.to_string(),
                                        created_at: chrono::Utc::now().timestamp(),
                                        description: format!(
                                            "正文文件双端修改冲突。本地修改和远端修改均保留。远端副本: {}",
                                            conflict_filename
                                        ),
                                    })
                                } else {
                                    None
                                }
                            } else if remote_rec.op == "delete" {
                                Some(SyncConflict {
                                    local_path: path.clone(),
                                    remote_path: path.clone(),
                                    local_hash: local_hash.clone(),
                                    remote_hash: remote_hash.clone(),
                                    base_hash: base_hash.to_string(),
                                    created_at: chrono::Utc::now().timestamp(),
                                    description:
                                        "正文文件冲突：本地已修改，远端已删除。保留本地文件。"
                                            .to_string(),
                                })
                            } else {
                                None
                            };

                            if let Some(conflict) = &conflict {
                                doc_conflicts.push(conflict.clone());
                                // Record the path as having an unresolved conflict so that
                                // subsequent syncs skip it until the user explicitly resolves.
                                state.conflicted_files.insert(path.clone());
                                // Also persist the conflict record in state.conflicts so that
                                // resolve_conflict_keep_local / take_remote / mark_merged can
                                // look up the remote_hash without needing a separate query.
                                state.conflicts.push(conflict.clone());
                            }
                            // Keep remote_rec in manifest so the remote side stays consistent.
                            // Do NOT update known_files[path] — it must remain at base_hash
                            // so that three-way comparison on the next sync still sees
                            // base=base, local≠base, remote≠base → BothChanged (or the
                            // unresolved_conflict_paths guard catches it first).
                            merged_manifest_files.insert(path.clone(), remote_rec.clone());
                        }
                    }
                } else {
                    let local_time = lww_record_time(local_rec);
                    let remote_time = lww_record_time(remote_rec);
                    let mut remote_wins = false;
                    if remote_time > local_time {
                        remote_wins = true;
                    } else if remote_time == local_time {
                        if remote_rec.content_hash == local_rec.content_hash
                            && remote_rec.op == local_rec.op
                        {
                            merged_manifest_files.insert(path.clone(), local_rec.clone());
                            result.ignored_files.push(path);
                            continue;
                        }
                        remote_wins = remote_rec.device_id > local_rec.device_id;
                        log::debug!(
                            "[sync] lww_tie_breaker path={} winner={} local_device={} remote_device={}",
                            path,
                            if remote_wins { "remote" } else { "local" },
                            local_rec.device_id,
                            remote_rec.device_id
                        );
                    }

                    if remote_wins {
                        merged_manifest_files.insert(path.clone(), remote_rec.clone());
                        if remote_rec.op == "upsert" {
                            if local_rec.op == "delete"
                                || local_rec.content_hash != remote_rec.content_hash
                            {
                                overwritten_files.push(path.clone());
                                to_download.push(path);
                            }
                        } else if remote_rec.op == "delete" {
                            if local_rec.op == "upsert" {
                                overwritten_files.push(path.clone());
                            }
                            to_delete_local.push(path.clone());
                            remote_deletes_count.push(path);
                        }
                    } else {
                        merged_manifest_files.insert(path.clone(), local_rec.clone());
                        if local_rec.op == "upsert" {
                            if remote_rec.op == "delete"
                                || remote_rec.content_hash != local_rec.content_hash
                            {
                                overwritten_files.push(path.clone());
                                to_upload.push(path);
                            }
                        } else if local_rec.op == "delete" {
                            if remote_rec.op == "upsert" {
                                overwritten_files.push(path.clone());
                            }
                            local_deletes_count.push(path);
                        }
                    }
                }
            }
            (None, None) => {}
        }
    }

    for path in &to_delete_local {
        let full_path = workspace_path.join(path);
        if full_path.exists() {
            let filename = full_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let trash_dir = workspace_path.join("app-meta/sync/trash");
            let _ = std::fs::create_dir_all(&trash_dir);
            let trash_path = trash_dir.join(format!(
                "{}_{}_{}",
                chrono::Utc::now().timestamp_millis(),
                uuid::Uuid::new_v4(),
                filename
            ));
            let _ = std::fs::rename(&full_path, &trash_path);
        }
    }

    log::debug!("[sync] github_api step=download newer remote files");
    if !to_download.is_empty() {
        let download_pool = sync_download_pool(to_download.len())?;
        let download_result: crate::Result<()> = download_pool.install(|| {
            to_download.par_iter().try_for_each(|path| {
                let Some((content, _sha)) =
                    github_get_content(client, api_base, token, &config.branch, path)?
                else {
                    return Err(crate::Error::SyncGithubApiError {
                        category: "api_error".to_string(),
                        context: format!("remote file missing while downloading {}", path),
                        status: 0,
                        body_preview: String::new(),
                    });
                };
                let full_path = workspace_path.join(path);
                if let Some(parent) = full_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!("{}: {}", path, e)))
                    })?;
                }
                let tmp_path =
                    full_path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
                std::fs::write(&tmp_path, content).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!("{}: {}", path, e)))
                })?;
                std::fs::rename(tmp_path, &full_path).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!("{}: {}", path, e)))
                })?;
                Ok(())
            })
        });
        download_result?;
    }

    let purge_time = now_ms - 30 * 24 * 3600 * 1000;
    let mut manifest_files_vec: Vec<ManifestFileRecord> =
        merged_manifest_files.values().cloned().collect();
    manifest_files_vec.retain(|rec| rec.op != "delete" || lww_record_time(rec) > purge_time);
    manifest_files_vec.sort_by(|a, b| a.path.cmp(&b.path));

    let sync_manifest = SyncManifest {
        files: manifest_files_vec,
    };

    let manifest_json = serde_json::to_string_pretty(&sync_manifest).unwrap_or_default();
    let full_manifest_path = workspace_path.join(SYNC_MANIFEST_PATH);
    if let Some(parent) = full_manifest_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| crate::Error::Io(std::io::Error::other(format!("manifest dir: {}", e))))?;
    }
    std::fs::write(&full_manifest_path, &manifest_json)
        .map_err(|e| crate::Error::Io(std::io::Error::other(format!("write manifest: {}", e))))?;

    log::debug!("[sync] github_api step=正在上传本地较新文件");
    for path in &to_upload {
        let full_path = workspace_path.join(path);
        if !full_path.exists() {
            continue;
        }
        let content = std::fs::read(&full_path)
            .map_err(|e| crate::Error::Io(std::io::Error::other(format!("read {}: {}", path, e))))?;
        github_put_content_serial(
            client,
            api_base,
            token,
            &config.branch,
            path,
            &content,
            remote_tree_files.get(path).cloned(),
        )?;
    }

    for path in &local_deletes_count {
        github_delete_content_serial(
            client,
            api_base,
            token,
            &config.branch,
            path,
            remote_tree_files.get(path).cloned(),
        )?;
    }

    github_put_content_serial(
        client,
        api_base,
        token,
        &config.branch,
        SYNC_MANIFEST_PATH,
        manifest_json.as_bytes(),
        remote_tree_files.get(SYNC_MANIFEST_PATH).cloned(),
    )?;

    for conflict in &doc_conflicts {
        let _ = SyncService::record_sync_conflict(workspace_path, conflict.clone(), None);
    }

    state.last_sync_time = Some(chrono::Utc::now().timestamp());
    state.last_synced_commit = None;
    state.last_error = None;

    let post_local_entries = scan_workspace_for_sync(workspace_path)?;

    // Before clearing known_files, save the base_hash values for conflicted
    // paths so we can restore them after the scan. The scan would otherwise
    // overwrite them with the current local file hash, which would break the
    // three-way comparison on the next sync.
    let conflicted_known_files: std::collections::HashMap<String, String> = state
        .conflicted_files
        .iter()
        .filter_map(|p| state.known_files.get(p).map(|v| (p.clone(), v.clone())))
        .collect();
    let conflicted_known_files_updated_at: std::collections::HashMap<String, i64> = state
        .conflicted_files
        .iter()
        .filter_map(|p| {
            state
                .known_files_updated_at
                .get(p)
                .map(|v| (p.clone(), *v))
        })
        .collect();

    state.known_files.clear();
    state.known_files_updated_at.clear();
    for entry in post_local_entries {
        if entry.sync_kind == SyncKind::Upload && entry.relative_path != SYNC_MANIFEST_PATH {
            // Do not let post-sync scan overwrite known_files for paths that
            // have unresolved conflicts — their known_files must stay at the
            // base_hash so three-way comparison keeps detecting BothChanged.
            if state.conflicted_files.contains(&entry.relative_path) {
                continue;
            }

            state
                .known_files
                .insert(entry.relative_path.clone(), entry.file_hash.clone());

            let matched_rec = merged_manifest_files.get(&entry.relative_path);
            let t = matched_rec.map(|r| r.updated_at_ms).unwrap_or_else(|| {
                std::fs::metadata(workspace_path.join(&entry.relative_path))
                    .and_then(|m| m.modified())
                    .and_then(|time| {
                        time.duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .map_err(std::io::Error::other)
                    })
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(now_ms)
            });
            state.known_files_updated_at.insert(entry.relative_path, t);
        }
    }

    // Restore the base_hash values for conflicted paths.
    for (path, hash) in conflicted_known_files {
        state.known_files.insert(path, hash);
    }
    for (path, t) in conflicted_known_files_updated_at {
        state.known_files_updated_at.insert(path, t);
    }

    // NOTE: The old logic that set known_files[path] = remote_hash for
    // conflicted files has been removed. Conflicted paths keep their
    // known_files at base_hash, and the unresolved_conflict_paths guard
    // at the top of the sync loop prevents any auto-resolution.

    state
        .tombstones
        .retain(|t| t.purge_after > chrono::Utc::now().timestamp());

    crate::sync::SyncService::save_sync_state(workspace_path, state)?;

    let has_doc_conflicts = !doc_conflicts.is_empty();
    let has_changes = !to_upload.is_empty()
        || !to_download.is_empty()
        || !pending_take_remote_downloaded.is_empty()
        || !local_deletes_count.is_empty()
        || !remote_deletes_count.is_empty();

    if has_doc_conflicts {
        result.status = SyncStatus::PartialConflict;
        result.conflicts = doc_conflicts;
    } else if !pending_take_remote_failed.is_empty() {
        result.status = SyncStatus::RecoverableError(format!(
            "pending_take_remote_failed: {}",
            pending_take_remote_failed.join(", ")
        ));
        result.error = Some(format!(
            "pending_take_remote: remote file missing for paths: {}",
            pending_take_remote_failed.join(", ")
        ));
    } else if has_changes {
        result.status = SyncStatus::LatestWinsApplied;
    } else {
        result.status = SyncStatus::NoChanges;
    }

    result.uploaded_files = to_upload;
    // Merge pending_take_remote downloads with regular downloads
    let mut all_downloaded = pending_take_remote_downloaded;
    all_downloaded.extend(to_download);
    result.downloaded_files = all_downloaded;
    result.local_deletes = local_deletes_count;
    result.remote_deletes = remote_deletes_count;
    result.overwritten_files = overwritten_files;
    result.commit_hash = None;
    result.first_sync_mode = FirstSyncMode::AlreadyGitRepo;

    log::debug!("[sync] github_api step=同步完成");
    Ok(result.clone())
}
