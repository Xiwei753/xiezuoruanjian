use crate::sync_service::github_api_client::{
    github_delete_content_serial, github_get_content, github_put_content_serial,
};
use crate::sync_service::github_backend::GitHubApiBackend;
use crate::sync_service::scanner::scan_workspace_for_sync;
use crate::sync_service::types::{
    FirstSyncMode, ManifestFileRecord, SyncConfig, SyncKind, SyncManifest, SyncResult, SyncSecrets,
    SyncState, SyncStatus,
};
use crate::sync_service::SyncService;
use std::path::Path;

const SYNC_MANIFEST_PATH: &str = "app-meta/sync/manifest.sync.json";

fn lww_record_time(record: &ManifestFileRecord) -> i64 {
    if record.op == "delete" {
        record.deleted_at_ms.unwrap_or(record.updated_at_ms)
    } else {
        record.updated_at_ms
    }
}

pub(crate) fn perform_lww_sync(
    workspace_path: &Path,
    config: &SyncConfig,
    secrets: &SyncSecrets,
) -> crate::Result<SyncResult> {
    eprintln!(
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
            Some("远程仓库地址为空。".to_string()),
            "Remote URL is empty".to_string(),
        ));
    }

    let token = secrets.token.clone().unwrap_or_default();
    if token.is_empty() {
        return Ok(SyncResult::error(
            SyncStatus::Error("No token provided".to_string()),
            FirstSyncMode::NotAttempted,
            Some("缺少 GitHub Token。".to_string()),
            "No token provided".to_string(),
        ));
    }

    let mut state = crate::sync_service::SyncService::load_sync_state(workspace_path)?;
    if state.device_id.is_empty() {
        state.device_id = uuid::Uuid::new_v4().to_string();
        crate::sync_service::SyncService::save_sync_state(workspace_path, &state)?;
    }

    let api_base = GitHubApiBackend::api_base_url(&config.remote_url);
    let probed_res = GitHubApiBackend::build_auto_client(config, secrets, Some(workspace_path));
    let (client, mode, probe_summary) = match probed_res {
        Ok((p, summary)) => (p.client, p.mode, summary),
        Err(e) => {
            result.error = Some(e.to_string());
            result.user_message = Some(format!("网络探测失败: {}", e));
            result.status = SyncStatus::RecoverableError(e.to_string());
            return Ok(result);
        }
    };
    result.chosen_network_mode = Some(mode.clone());
    result.network_probe_summary = probe_summary;

    let max_retries = 2;
    let mut attempt = 0;
    loop {
        match execute_lww_sync_attempt(
            workspace_path,
            config,
            &token,
            &api_base,
            &client,
            &mode,
            &mut state,
            &mut result,
        ) {
            Ok(res) => return Ok(res),
            Err(e) => {
                attempt += 1;
                if attempt >= max_retries {
                    let err = e.to_string();
                    result.status = if err.contains("local_io_error") {
                        SyncStatus::Error("local_io_error".to_string())
                    } else if err.contains("auth_error") {
                        SyncStatus::Error("auth_error".to_string())
                    } else if err.contains("api_rate_limited") {
                        SyncStatus::RecoverableError("api_rate_limited".to_string())
                    } else if err.contains("network_error") {
                        SyncStatus::RecoverableError("network_error".to_string())
                    } else {
                        SyncStatus::RecoverableError("api_error".to_string())
                    };
                    result.error = Some(err.clone());
                    result.user_message = Some(format!(
                        "同步失败，已重试 {} 次。错误: {}",
                        max_retries, err
                    ));
                    return Ok(result);
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
}

fn execute_lww_sync_attempt(
    workspace_path: &Path,
    config: &SyncConfig,
    token: &str,
    api_base: &str,
    client: &reqwest::blocking::Client,
    mode: &str,
    state: &mut SyncState,
    result: &mut SyncResult,
) -> crate::Result<SyncResult> {
    eprintln!("[sync] github_api step=正在拉取远端清单");
    let tree_url = format!("{}/git/trees/{}?recursive=1", api_base, config.branch);
    let resp = client
        .get(&tree_url)
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "WriterApp/1.0")
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| crate::Error::Other(format!("network_error: {}", e)))?;

    let mut remote_tree_files = std::collections::HashMap::new();
    let tree_status = resp.status();
    let tree_body = resp
        .text()
        .map_err(|e| crate::Error::Other(format!("network_error: {}", e)))?;
    if tree_status.as_u16() == 200 {
        let json: serde_json::Value = serde_json::from_str(&tree_body)
            .map_err(|e| crate::Error::Other(format!("api_error: invalid tree json: {}", e)))?;
        if json["truncated"].as_bool().unwrap_or(false) {
            return Err(crate::Error::Other(
                "api_error: GitHub tree response truncated, repository is too large".to_string(),
            ));
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
    } else if tree_status.as_u16() != 404 {
        return Err(crate::sync_service::github_api_client::github_api_error(
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
                    crate::Error::Other(format!("api_error: invalid remote manifest: {}", e))
                })?;
        }
    }

    eprintln!("[sync] github_api step=正在比较本地和远端");
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

    let mut merged_manifest_files = std::collections::HashMap::new();
    let mut to_download = Vec::new();
    let mut to_upload = Vec::new();
    let mut to_delete_local = Vec::new();
    let mut local_deletes_count = Vec::new();
    let mut remote_deletes_count = Vec::new();
    let mut overwritten_files = Vec::new();

    let all_paths: std::collections::HashSet<String> = local_records
        .keys()
        .cloned()
        .chain(remote_records.keys().cloned())
        .collect();

    for path in all_paths {
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
                    eprintln!(
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

    eprintln!("[sync] github_api step=正在下载远端较新文件");
    for path in &to_download {
        let Some((content, _sha)) =
            github_get_content(client, api_base, token, &config.branch, path)?
        else {
            return Err(crate::Error::Other(format!(
                "api_error: remote file missing while downloading {}",
                path
            )));
        };
        let full_path = workspace_path.join(path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| crate::Error::Other(format!("local_io_error: {}: {}", path, e)))?;
        }
        let tmp_path = full_path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
        std::fs::write(&tmp_path, content)
            .map_err(|e| crate::Error::Other(format!("local_io_error: {}: {}", path, e)))?;
        std::fs::rename(tmp_path, &full_path)
            .map_err(|e| crate::Error::Other(format!("local_io_error: {}: {}", path, e)))?;
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
            .map_err(|e| crate::Error::Other(format!("local_io_error: manifest dir: {}", e)))?;
    }
    std::fs::write(&full_manifest_path, &manifest_json)
        .map_err(|e| crate::Error::Other(format!("local_io_error: write manifest: {}", e)))?;

    eprintln!("[sync] github_api step=正在上传本地较新文件");
    for path in &to_upload {
        let full_path = workspace_path.join(path);
        if !full_path.exists() {
            continue;
        }
        let content = std::fs::read(&full_path)
            .map_err(|e| crate::Error::Other(format!("local_io_error: read {}: {}", path, e)))?;
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

    state.last_sync_time = Some(chrono::Utc::now().timestamp());
    state.last_synced_commit = None;
    state.last_error = None;
    state.last_successful_network_mode = Some(mode.to_string());

    let post_local_entries = scan_workspace_for_sync(workspace_path)?;
    state.known_files.clear();
    state.known_files_updated_at.clear();
    for entry in post_local_entries {
        if entry.sync_kind == SyncKind::Upload && entry.relative_path != SYNC_MANIFEST_PATH {
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

    state
        .tombstones
        .retain(|t| t.purge_after > chrono::Utc::now().timestamp());

    crate::sync_service::SyncService::save_sync_state(workspace_path, state)?;

    let has_changes = !to_upload.is_empty()
        || !to_download.is_empty()
        || !local_deletes_count.is_empty()
        || !remote_deletes_count.is_empty();
    result.status = if has_changes {
        SyncStatus::LatestWinsApplied
    } else {
        SyncStatus::NoChanges
    };
    result.uploaded_files = to_upload;
    result.downloaded_files = to_download;
    result.local_deletes = local_deletes_count;
    result.remote_deletes = remote_deletes_count;
    result.overwritten_files = overwritten_files;
    result.commit_hash = None;
    result.first_sync_mode = FirstSyncMode::AlreadyGitRepo;

    result.user_message = Some(format!(
        "双向同步完成。上传: {}, 下载: {}, 本地删除: {}, 远端删除: {}, 覆盖: {} (网络模式: {})。",
        result.uploaded_files.len(),
        result.downloaded_files.len(),
        result.local_deletes.len(),
        result.remote_deletes.len(),
        result.overwritten_files.len(),
        mode
    ));

    eprintln!("[sync] github_api step=同步完成");
    Ok(result.clone())
}
