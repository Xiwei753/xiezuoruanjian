use crate::sync_service::conflict::build_conflict_summary;
use crate::sync_service::types::SyncConfig;
use crate::sync_service::git_backend::GitBackend;
use crate::sync_service::git_backend::fetch_and_reset_local_repo;
use crate::sync_service::github_backend::GitHubApiBackend;
use crate::sync_service::conflict::collect_git_status_summary;
use crate::sync_service::types::SyncManifest;
use crate::sync_service::types::SyncResult;
use crate::sync_service::types::SyncPlan;
use crate::sync_service::types::ManifestFileRecord;
use crate::sync_service::types::SyncConflict;
use crate::sync_service::types::SyncStatus;
use crate::sync_service::types::SyncSecrets;
use crate::sync_service::types::SyncState;
use crate::sync_service::url::sanitize_remote_url;
use crate::sync_service::types::FirstSyncMode;
use crate::sync_service::types::SyncFileEntry;
use crate::sync_service::types::SettingConflictDetail;
use crate::sync_service::diagnostics::get_user_friendly_error;
use crate::sync_service::types::SyncTransport;
use crate::sync_service::types::SyncKind;
use crate::sync_service::git_backend::GitAuth;
use crate::sync_service::types::SyncConflictSummary;
use std::path::Path;
use base64::Engine;

pub struct SyncService {
    pub config: Option<SyncConfig>,
    pub status: SyncStatus,
}

impl crate::sync_service::SyncService {
    fn ensure_local_branch_exists(repo: &git2::Repository, branch: &str) -> crate::Result<()> {
        let branch_ref_name = format!("refs/heads/{}", branch);

        // 1. Clean up any leftover merge/rebase/etc. states first to ensure clean execution.
        let _ = repo.cleanup_state();

        if repo.find_reference(&branch_ref_name).is_ok() {
            // Branch exists, make sure HEAD points to it (symbolically or directly)
            let _ = repo.set_head(&branch_ref_name);
            return Ok(());
        }

        // Branch does not exist. Check if HEAD exists and points to a valid commit
        if let Ok(head_ref) = repo.head() {
            if let Ok(commit) = head_ref.peel_to_commit() {
                // Create branch pointing to this commit
                repo.branch(branch, &commit, false).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "Failed to create branch '{}': {}",
                        branch, e
                    )))
                })?;
                repo.set_head(&branch_ref_name).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "Failed to set HEAD to '{}': {}",
                        branch, e
                    )))
                })?;
                return Ok(());
            }
        }

        // HEAD is unborn/empty (no commits yet). Set HEAD symbolically.
        // The first commit will automatically create this branch.
        repo.set_head(&branch_ref_name).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "Failed to set symbolic HEAD to '{}': {}",
                branch, e
            )))
        })?;

        Ok(())
    }

}

impl crate::sync_service::SyncService {
    pub fn perform_sync_dry_run(
        workspace_path: &Path,
        config: &SyncConfig,
    ) -> crate::Result<SyncPlan> {
        if !config.enabled {
            return Ok(SyncPlan::new());
        }
        Self::build_sync_plan_from_workspace(workspace_path)
        // Note: Full dry-run combining remote diffs is not currently supported without
        // network access inside the dry-run invocation, so it operates locally for now.
    }

}

impl crate::sync_service::SyncService {
    pub fn perform_lww_sync(
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

        // Load or initialize local state
        let mut state = Self::load_sync_state(workspace_path)?;
        if state.device_id.is_empty() {
            state.device_id = uuid::Uuid::new_v4().to_string();
            Self::save_sync_state(workspace_path, &state)?;
        }

        // Build http client using build_auto_client
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

        // Perform the synchronization in a retry loop (OCC)
        let max_retries = 2;
        let mut attempt = 0;
        loop {
            match Self::execute_lww_sync_attempt(
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
                        result.status = SyncStatus::RecoverableError(e.to_string());
                        result.error = Some(e.to_string());
                        result.user_message =
                            Some(format!("同步失败，已重试 {} 次。错误: {}", max_retries, e));
                        return Ok(result);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
        }
    }

}

impl crate::sync_service::SyncService {
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
        let branch_url = format!("{}/git/ref/heads/{}", api_base, config.branch);
        let resp = client
            .get(&branch_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "WriterApp/1.0")
            .header("Accept", "application/vnd.github+json")
            .send()
            .map_err(|e| crate::Error::Other(e.to_string()))?;

        let mut latest_commit_sha = String::new();
        let mut base_tree_sha = String::new();
        let mut remote_tree_files = std::collections::HashMap::new();

        if resp.status().as_u16() == 200 {
            let json: serde_json::Value = resp
                .json()
                .map_err(|e| crate::Error::Other(e.to_string()))?;
            latest_commit_sha = json["object"]["sha"]
                .as_str()
                .unwrap_or_default()
                .to_string();

            // get commit
            let commit_url = format!("{}/git/commits/{}", api_base, latest_commit_sha);
            let resp = client
                .get(&commit_url)
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "WriterApp/1.0")
                .header("Accept", "application/vnd.github+json")
                .send()
                .map_err(|e| crate::Error::Other(e.to_string()))?;
            if resp.status().as_u16() == 200 {
                let json: serde_json::Value = resp
                    .json()
                    .map_err(|e| crate::Error::Other(e.to_string()))?;
                base_tree_sha = json["tree"]["sha"].as_str().unwrap_or_default().to_string();
            }

            // get tree recursively
            if !base_tree_sha.is_empty() {
                let tree_url = format!("{}/git/trees/{}?recursive=1", api_base, base_tree_sha);
                let resp = client
                    .get(&tree_url)
                    .header("Authorization", format!("Bearer {}", token))
                    .header("User-Agent", "WriterApp/1.0")
                    .header("Accept", "application/vnd.github+json")
                    .send()
                    .map_err(|e| crate::Error::Other(e.to_string()))?;
                if resp.status().as_u16() == 200 {
                    let json: serde_json::Value = resp
                        .json()
                        .map_err(|e| crate::Error::Other(e.to_string()))?;
                    if let Some(tree) = json["tree"].as_array() {
                        for item in tree {
                            if item["type"].as_str() == Some("blob") {
                                if let (Some(path), Some(sha)) =
                                    (item["path"].as_str(), item["sha"].as_str())
                                {
                                    remote_tree_files.insert(path.to_string(), sha.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut remote_manifest = SyncManifest::default();
        if remote_tree_files.contains_key("app-meta/sync/manifest.sync.json") {
            let manifest_url = format!(
                "{}/contents/app-meta/sync/manifest.sync.json?ref={}",
                api_base, config.branch
            );
            let resp = client
                .get(&manifest_url)
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "WriterApp/1.0")
                .header("Accept", "application/vnd.github+json")
                .send()
                .map_err(|e| crate::Error::Other(e.to_string()))?;
            if resp.status().is_success() {
                let json: serde_json::Value = resp.json().unwrap_or_default();
                if let Some(content_b64) = json["content"].as_str() {
                    let content_b64 = content_b64.replace("\n", "");
                    use base64::Engine;
                    if let Ok(content_bytes) =
                        base64::engine::general_purpose::STANDARD.decode(&content_b64)
                    {
                        if let Ok(manifest) = serde_json::from_slice::<SyncManifest>(&content_bytes)
                        {
                            remote_manifest = manifest;
                        }
                    }
                }
            }
        }

        let local_entries = Self::scan_workspace_for_sync(workspace_path)?;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut local_records = std::collections::HashMap::new();

        // 1. Existing local files
        for entry in &local_entries {
            if entry.sync_kind == SyncKind::Upload {
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
                        device_id: state.device_id.clone(),
                        op,
                        schema_version: 1,
                    },
                );
            }
        }

        // 2. Local deletions (in known_files but missing from workspace)
        for path in state.known_files.keys() {
            if !local_records.contains_key(path) {
                if !Self::is_whitelisted_path(path) || Self::is_blacklisted_path(path) {
                    continue;
                }
                if !workspace_path.join(path).exists() {
                    let mut updated_at_ms = now_ms;
                    if let Some(tombstone) =
                        state.tombstones.iter().find(|t| t.original_path == *path)
                    {
                        updated_at_ms = tombstone.deleted_at * 1000;
                    }

                    local_records.insert(
                        path.clone(),
                        ManifestFileRecord {
                            path: path.clone(),
                            content_hash: String::new(),
                            updated_at_ms,
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
            remote_records.insert(rec.path.clone(), rec);
        }

        for (path, sha) in &remote_tree_files {
            if !remote_records.contains_key(path) {
                if !Self::is_whitelisted_path(path) || Self::is_blacklisted_path(path) {
                    continue;
                }
                remote_records.insert(
                    path.clone(),
                    ManifestFileRecord {
                        path: path.clone(),
                        content_hash: sha.clone(),
                        updated_at_ms: 0,
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
                    let mut remote_wins = false;
                    if remote_rec.updated_at_ms > local_rec.updated_at_ms {
                        remote_wins = true;
                    } else if remote_rec.updated_at_ms == local_rec.updated_at_ms
                        && remote_rec.device_id > local_rec.device_id
                    {
                        remote_wins = true;
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

        // Delete local files
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

        // Download remote files
        for path in &to_download {
            let api_content_url = format!("{}/contents/{}?ref={}", api_base, path, config.branch);
            let dl_resp = client
                .get(&api_content_url)
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "WriterApp/1.0")
                .header("Accept", "application/vnd.github+json")
                .send()
                .map_err(|e| crate::Error::Other(e.to_string()))?;
            if dl_resp.status().is_success() {
                let json: serde_json::Value = dl_resp.json().unwrap_or_default();
                if let Some(content_b64) = json["content"].as_str() {
                    let content_b64 = content_b64.replace("\n", "");
                    use base64::Engine;
                    if let Ok(content) =
                        base64::engine::general_purpose::STANDARD.decode(&content_b64)
                    {
                        let full_path = workspace_path.join(path);
                        if let Some(parent) = full_path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        let tmp_path =
                            full_path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
                        if std::fs::write(&tmp_path, content).is_ok() {
                            let _ = std::fs::rename(tmp_path, &full_path);
                        }
                    }
                }
            } else {
                return Err(crate::Error::Other(format!(
                    "Failed to download file {}: {}",
                    path,
                    dl_resp.status()
                )));
            }
        }

        let mut tree_nodes = vec![];

        // Upload whitelisted files
        for path in &to_upload {
            let full_path = workspace_path.join(path);
            if !full_path.exists() {
                continue;
            }

            let content = std::fs::read(&full_path)
                .map_err(|e| crate::Error::Other(format!("读取文件失败 {}: {}", path, e)))?;

            let blob_url = format!("{}/git/blobs", api_base);
            let blob_payload = serde_json::json!({
                "content": base64::engine::general_purpose::STANDARD.encode(&content),
                "encoding": "base64"
            });
            let resp = client
                .post(&blob_url)
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "WriterApp/1.0")
                .header("Accept", "application/vnd.github+json")
                .json(&blob_payload)
                .send()
                .map_err(|e| crate::Error::Other(e.to_string()))?;

            if resp.status().as_u16() == 201 {
                let json: serde_json::Value = resp
                    .json()
                    .map_err(|e| crate::Error::Other(e.to_string()))?;
                let sha = json["sha"].as_str().unwrap_or_default().to_string();
                tree_nodes.push(serde_json::json!({
                    "path": path,
                    "mode": "100644",
                    "type": "blob",
                    "sha": sha
                }));
            } else {
                return Err(crate::Error::Other(format!(
                    "Failed to upload blob for {}: {}",
                    path,
                    resp.status()
                )));
            }
        }

        let purge_time = now_ms - 30 * 24 * 3600 * 1000;
        let mut manifest_files_vec: Vec<ManifestFileRecord> =
            merged_manifest_files.values().cloned().collect();
        manifest_files_vec.retain(|rec| rec.op != "delete" || rec.updated_at_ms > purge_time);

        let sync_manifest = SyncManifest {
            files: manifest_files_vec,
        };

        let manifest_json = serde_json::to_string_pretty(&sync_manifest).unwrap_or_default();
        let manifest_path = "app-meta/sync/manifest.sync.json";
        let full_manifest_path = workspace_path.join(manifest_path);
        if let Some(parent) = full_manifest_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&full_manifest_path, &manifest_json)
            .map_err(|e| crate::Error::Other(format!("Failed to write manifest locally: {}", e)))?;

        let blob_url = format!("{}/git/blobs", api_base);
        let blob_payload = serde_json::json!({
            "content": base64::engine::general_purpose::STANDARD.encode(manifest_json.as_bytes()),
            "encoding": "base64"
        });
        let resp = client
            .post(&blob_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "WriterApp/1.0")
            .header("Accept", "application/vnd.github+json")
            .json(&blob_payload)
            .send()
            .map_err(|e| crate::Error::Other(e.to_string()))?;
        let manifest_sha = if resp.status().as_u16() == 201 {
            let json: serde_json::Value = resp
                .json()
                .map_err(|e| crate::Error::Other(e.to_string()))?;
            json["sha"].as_str().unwrap_or_default().to_string()
        } else {
            return Err(crate::Error::Other(format!(
                "Failed to upload manifest blob: {}",
                resp.status()
            )));
        };

        tree_nodes.push(serde_json::json!({
            "path": manifest_path,
            "mode": "100644",
            "type": "blob",
            "sha": manifest_sha
        }));

        for (path, remote_rec) in &remote_records {
            if let Some(merged_rec) = merged_manifest_files.get(path) {
                if merged_rec.op == "delete" && remote_rec.op == "upsert" {
                    tree_nodes.push(serde_json::json!({
                        "path": path,
                        "mode": "100644",
                        "type": "blob",
                        "sha": serde_json::Value::Null
                    }));
                }
            }
        }

        let mut tree_payload = serde_json::json!({
            "tree": tree_nodes
        });
        if !base_tree_sha.is_empty() {
            tree_payload["base_tree"] = serde_json::json!(base_tree_sha);
        }

        let tree_url = format!("{}/git/trees", api_base);
        let resp = client
            .post(&tree_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "WriterApp/1.0")
            .header("Accept", "application/vnd.github+json")
            .json(&tree_payload)
            .send()
            .map_err(|e| crate::Error::Other(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(crate::Error::Other(format!(
                "Failed to create tree: {}",
                resp.status()
            )));
        }
        let json: serde_json::Value = resp
            .json()
            .map_err(|e| crate::Error::Other(e.to_string()))?;
        let new_tree_sha = json["sha"].as_str().unwrap_or_default().to_string();

        let commit_url = format!("{}/git/commits", api_base);
        let mut commit_payload = serde_json::json!({
            "message": format!("WriterApp LWW Sync: {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")),
            "tree": new_tree_sha
        });
        if !latest_commit_sha.is_empty() {
            commit_payload["parents"] = serde_json::json!(vec![latest_commit_sha.clone()]);
        }
        let resp = client
            .post(&commit_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "WriterApp/1.0")
            .header("Accept", "application/vnd.github+json")
            .json(&commit_payload)
            .send()
            .map_err(|e| crate::Error::Other(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(crate::Error::Other(format!(
                "Failed to create commit: {}",
                resp.status()
            )));
        }
        let json: serde_json::Value = resp
            .json()
            .map_err(|e| crate::Error::Other(e.to_string()))?;
        let new_commit_sha = json["sha"].as_str().unwrap_or_default().to_string();

        if !latest_commit_sha.is_empty() {
            let ref_url = format!("{}/git/refs/heads/{}", api_base, config.branch);
            let ref_payload = serde_json::json!({
                "sha": new_commit_sha,
                "force": false
            });
            let resp = client
                .patch(&ref_url)
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "WriterApp/1.0")
                .header("Accept", "application/vnd.github+json")
                .json(&ref_payload)
                .send()
                .map_err(|e| crate::Error::Other(e.to_string()))?;
            if resp.status().as_u16() == 422 || resp.status().as_u16() == 409 {
                return Err(crate::Error::Other("occ_conflict".to_string()));
            }
            if !resp.status().is_success() {
                return Err(crate::Error::Other(format!(
                    "Failed to update ref: {}",
                    resp.status()
                )));
            }
        } else {
            let ref_url = format!("{}/git/refs", api_base);
            let ref_payload = serde_json::json!({
                "ref": format!("refs/heads/{}", config.branch),
                "sha": new_commit_sha
            });
            let resp = client
                .post(&ref_url)
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "WriterApp/1.0")
                .header("Accept", "application/vnd.github+json")
                .json(&ref_payload)
                .send()
                .map_err(|e| crate::Error::Other(e.to_string()))?;
            if !resp.status().is_success() {
                return Err(crate::Error::Other(format!(
                    "Failed to create ref: {}",
                    resp.status()
                )));
            }
        }

        state.last_sync_time = Some(chrono::Utc::now().timestamp());
        state.last_synced_commit = Some(new_commit_sha.clone());
        state.last_error = None;
        state.last_successful_network_mode = Some(mode.to_string());

        let post_local_entries = Self::scan_workspace_for_sync(workspace_path)?;
        state.known_files.clear();
        state.known_files_updated_at.clear();
        for entry in post_local_entries {
            if entry.sync_kind == SyncKind::Upload {
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

        Self::save_sync_state(workspace_path, state)?;

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
        result.commit_hash = Some(new_commit_sha.clone());
        result.first_sync_mode = if latest_commit_sha.is_empty() {
            FirstSyncMode::InitExistingWorkspace
        } else {
            FirstSyncMode::AlreadyGitRepo
        };

        result.user_message = Some(format!(
            "双向同步完成。上传: {}, 下载: {}, 本地删除: {}, 远端删除: {}, 覆盖: {} (网络模式: {})。",
            result.uploaded_files.len(),
            result.downloaded_files.len(),
            result.local_deletes.len(),
            result.remote_deletes.len(),
            result.overwritten_files.len(),
            mode
        ));

        let _ = fetch_and_reset_local_repo(workspace_path, config, token, &new_commit_sha);

        Ok(result.clone())
    }

}

impl crate::sync_service::SyncService {
    pub fn perform_sync(
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
        backend: &dyn GitBackend,
    ) -> crate::Result<SyncResult> {
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

        let parsed = sanitize_remote_url(&config.remote_url);
        let sanitized_url = parsed.sanitized_url;

        let map_git_error = |e: crate::Error| -> crate::Error {
            if let crate::Error::Io(io_err) = &e {
                let msg = io_err.to_string();
                if msg.contains("unsupported proxy protocol")
                    || msg.contains("failed to resolve address")
                    || msg.contains("SOCKS5")
                {
                    return crate::Error::Io(std::io::Error::other(format!(
                        "代理不可用/端口不通: {}",
                        msg
                    )));
                }
            }
            e
        };

        let classify_error = |e_str: &str| -> SyncStatus {
            let lower = e_str.to_lowercase();
            if lower.contains("recoverable_error") {
                SyncStatus::RecoverableError(
                    e_str.replace("recoverable_error:", "").trim().to_string(),
                )
            } else if lower.contains("fatal_error") {
                SyncStatus::FatalError(e_str.replace("fatal_error:", "").trim().to_string())
            } else if lower.contains("auth")
                || lower.contains("token")
                || lower.contains("credential")
                || lower.contains("proxy")
                || lower.contains("resolve")
                || lower.contains("network")
                || lower.contains("unborn")
                || lower.contains("timeout")
                || lower.contains("connect")
                || lower.contains("could not resolve")
            {
                SyncStatus::RecoverableError(e_str.to_string())
            } else {
                SyncStatus::FatalError(e_str.to_string())
            }
        };

        let token_from_parsed = parsed.extracted_token;
        let token = secrets
            .token
            .clone()
            .or(token_from_parsed)
            .unwrap_or_default();
        if token.is_empty() {
            return Ok(SyncResult::error(
                SyncStatus::Error("No token provided".to_string()),
                FirstSyncMode::NotAttempted,
                Some("缺少 GitHub Token。".to_string()),
                "No token provided".to_string(),
            ));
        }

        let username_for_cred = if !config.username.is_empty() {
            config.username.clone()
        } else if let Some(ref extracted_user) = parsed.extracted_username {
            extracted_user.clone()
        } else {
            "x-access-token".to_string()
        };

        let auth = match &config.transport {
            SyncTransport::HttpsToken => Some(GitAuth::HttpsToken {
                username: username_for_cred.clone(),
                token: token.clone(),
            }),
            SyncTransport::SshDeployKey => {
                return Ok(SyncResult::error(
                    SyncStatus::Error("SshDeployKey is not implemented".to_string()),
                    FirstSyncMode::NotAttempted,
                    Some("当前不支持 SSH 同步方式。".to_string()),
                    "SshDeployKey is not implemented".to_string(),
                ));
            }
        };

        let has_repo = backend.has_repo(workspace_path);
        if !has_repo {
            let is_empty_or_git_only = match backend.is_worktree_empty_or_git_only(workspace_path) {
                Ok(val) => val,
                Err(e) => {
                    return Ok(SyncResult::error(
                        SyncStatus::Error(e.to_string()),
                        FirstSyncMode::NotAttempted,
                        Some("检查本地工作区失败。".to_string()),
                        e.to_string(),
                    ));
                }
            };

            if is_empty_or_git_only {
                result.first_sync_mode = FirstSyncMode::CloneIntoEmptyWorkspace;
                result.user_message = Some("已克隆远端仓库到空工作区。".to_string());
                if let Err(e) = backend
                    .clone_repo(&sanitized_url, workspace_path, auth.as_ref(), Some(config))
                    .map_err(map_git_error)
                {
                    return Ok(SyncResult::error(
                        SyncStatus::Error(e.to_string()),
                        FirstSyncMode::CloneIntoEmptyWorkspace,
                        Some("已克隆远端仓库到空工作区。".to_string()),
                        e.to_string(),
                    ));
                }
            } else {
                result.first_sync_mode = FirstSyncMode::InitExistingWorkspace;
                result.user_message =
                    Some("本地已有作品，已初始化为 Git 仓库并准备同步。".to_string());
                if let Err(e) = backend.init_repo(workspace_path) {
                    result.status = SyncStatus::Error(e.to_string());
                    result.error = Some(e.to_string());
                    return Ok(result);
                }
                if let Err(e) = backend.ensure_remote(workspace_path, &sanitized_url) {
                    result.status = SyncStatus::Error(e.to_string());
                    result.error = Some(e.to_string());
                    return Ok(result);
                }
            }
        } else {
            result.first_sync_mode = FirstSyncMode::AlreadyGitRepo;
            // Open and ensure remote
            if let Err(e) = backend.open_repo(workspace_path) {
                result.status = SyncStatus::Error(e.to_string());
                result.error = Some(e.to_string());
                return Ok(result);
            }
            if let Err(e) = backend.ensure_remote(workspace_path, &sanitized_url) {
                result.status = SyncStatus::Error(e.to_string());
                result.error = Some(e.to_string());
                return Ok(result);
            }
        }

        // Ensure local branch ref is initialized and clean states before staging/pulling
        let mut branch_recovered = false;
        if let Ok(repo) = git2::Repository::open(workspace_path) {
            let branch_ref_name = format!("refs/heads/{}", config.branch);
            let branch_exists = repo.find_reference(&branch_ref_name).is_ok();
            let head_commit = repo.head().ok().and_then(|r| r.peel_to_commit().ok());
            if !branch_exists && head_commit.is_some() {
                branch_recovered = true;
            }

            if let Err(e) = Self::ensure_local_branch_exists(&repo, &config.branch) {
                return Ok(SyncResult::error(
                    classify_error(&e.to_string()),
                    result.first_sync_mode,
                    Some("初始化本地分支失败。".to_string()),
                    e.to_string(),
                ));
            }
        }

        // Check for dirty non-whitelisted changes
        if let Ok(status_list) = backend.status(workspace_path) {
            let mut dirty_non_whitelisted = Vec::new();
            for p in &status_list {
                if !SyncService::is_blacklisted_path(p) && !SyncService::is_whitelisted_path(p) {
                    dirty_non_whitelisted.push(p.clone());
                }
            }
            if !dirty_non_whitelisted.is_empty() {
                return Ok(SyncResult::error(
                    SyncStatus::DirtyRepoBlocked,
                    result.first_sync_mode,
                    Some(format!(
                        "同步被阻止: 本地工作区存在未跟踪或未提交的修改，且这些修改不是同步安全文件:\n{}",
                        dirty_non_whitelisted.join("\n")
                    )),
                    "Dirty repo blocked: non-whitelisted files modified".to_string(),
                ));
            }
        }

        // Auto commit local whitelisted changes
        if let Ok(status_list) = backend.status(workspace_path) {
            let mut paths_to_stage = Vec::new();
            for p in &status_list {
                if SyncService::is_whitelisted_path(p) && !SyncService::is_blacklisted_path(p) {
                    paths_to_stage.push(p.as_str());
                }
            }
            if !paths_to_stage.is_empty() {
                if let Err(e) = backend.stage_paths(workspace_path, &paths_to_stage) {
                    return Ok(SyncResult::error(
                        classify_error(&e.to_string()),
                        result.first_sync_mode,
                        Some("暂存本地更改失败。".to_string()),
                        e.to_string(),
                    ));
                }
                if let Err(e) = backend.commit(workspace_path, "Auto sync local changes") {
                    return Ok(SyncResult::error(
                        classify_error(&e.to_string()),
                        result.first_sync_mode,
                        Some("提交本地更改失败。".to_string()),
                        e.to_string(),
                    ));
                }
            }
        }

        // Pull
        let mut pull_branch_missing = false;
        let pull_failed = backend
            .pull(workspace_path, &config.branch, auth.as_ref(), Some(config))
            .map_err(map_git_error)
            .err();
        if let Some(e) = pull_failed {
            let e_str = e.to_string(); // we should not to_lowercase before matching payload

            if e_str.contains("settings_conflict_payload:") {
                let payload_str = e_str
                    .split("settings_conflict_payload:")
                    .nth(1)
                    .unwrap_or("")
                    .trim();
                let details: Option<Vec<SettingConflictDetail>> =
                    serde_json::from_str(payload_str).ok();
                let mut res = SyncResult::error(
                    SyncStatus::Conflict,
                    result.first_sync_mode,
                    Some("同步冲突，已停止，未覆盖任何文件".to_string()),
                    "Settings semantic merge conflict".to_string(),
                );
                res.settings_conflicts = details;
                let summary = SyncConflictSummary {
                    status: "conflict".to_string(),
                    local_dirty: true,
                    remote_changed: true,
                    conflicted_files: vec!["app-meta/settings/settings.sync.json".to_string()],
                    blocked_reason: "本地和远端都修改了设置文件 settings.sync.json 且产生了冲突。"
                        .to_string(),
                    safe_next_steps: vec![
                        "手动检查本地与远端设置。".to_string(),
                        "重新保存设置以覆盖或重新同步。".to_string(),
                    ],
                };
                res.conflict_summary = Some(summary);
                return Ok(res);
            }

            if e_str.contains("checkout_conflict_payload:") {
                let payload_str = e_str
                    .split("checkout_conflict_payload:")
                    .nth(1)
                    .unwrap_or("")
                    .trim();
                let summary: Option<SyncConflictSummary> = serde_json::from_str(payload_str).ok();

                let mut res = SyncResult::error(
                    SyncStatus::Conflict,
                    result.first_sync_mode,
                    Some("本地工作区有文件会阻止远端更新，请先处理冲突文件后再同步。".to_string()),
                    "Pull failed due to conflict.".to_string(),
                );
                res.conflict_summary = summary;
                return Ok(res);
            }

            let e_str_lower = e_str.to_lowercase();
            // Checkout conflict / local blocking file (fallback)
            if e_str_lower.contains("checkout_conflict")
                || e_str_lower.contains("local_blocking_file")
                || e_str_lower.contains("conflict prevents checkout")
            {
                let mut res = SyncResult::error(
                    SyncStatus::Conflict,
                    result.first_sync_mode,
                    Some("本地工作区有文件会阻止远端更新，请先处理冲突文件后再同步。".to_string()),
                    format!("Pull failed: {}", e),
                );
                if let Ok(repo) = git2::Repository::open(workspace_path) {
                    let fetch_commit_id = repo
                        .find_reference("FETCH_HEAD")
                        .ok()
                        .and_then(|r| r.target());
                    let summary = build_conflict_summary(
                        &repo,
                        fetch_commit_id,
                        "本地工作区有文件会阻止远端更新，请先处理冲突文件后再同步。",
                    );
                    res.conflict_summary = Some(summary);
                }
                return Ok(res);
            }
            if e_str_lower.contains("unrelated")
                || e_str_lower.contains("merge")
                || e_str_lower.contains("no common ancestor")
            {
                let status = classify_error(&e.to_string());
                let user_msg = if result.first_sync_mode == FirstSyncMode::InitExistingWorkspace {
                    "远端仓库不是空仓库，且和本地作品历史不一致。推荐使用空 GitHub 私人仓库。"
                } else {
                    "远端仓库不是空仓库，且和本地作品历史不一致。请使用空 GitHub 私人仓库，或手动处理后再同步。"
                };

                return Ok(SyncResult::error(
                    status,
                    FirstSyncMode::UnrelatedHistories,
                    Some(user_msg.to_string()),
                    format!("Pull failed: {}", e),
                ));
            }
            if e_str_lower.contains("ref not found")
                || e_str_lower.contains("couldn't find remote ref")
                || (e_str_lower.contains("remote branch") && e_str_lower.contains("not found"))
            {
                if result.first_sync_mode != FirstSyncMode::InitExistingWorkspace
                    && result.first_sync_mode != FirstSyncMode::AlreadyGitRepo
                {
                    return Ok(SyncResult::error(
                        classify_error(&e.to_string()),
                        result.first_sync_mode,
                        Some("拉取失败，远程分支不存在。".to_string()),
                        format!("Pull failed: {}", e),
                    ));
                }
                pull_branch_missing = true;
                result.user_message = Some("远程分支不存在，首次同步将创建该分支。".to_string());
            } else if e.to_string().contains("SyncConflict_Detected") {
                result.status = SyncStatus::Conflict;
                result.error = Some("Sync Conflict: automatic merge failed".to_string());

                // Iterate through git index conflicts and record them
                let repo = match git2::Repository::open(workspace_path) {
                    Ok(r) => r,
                    Err(e) => {
                        result.status = classify_error(&e.to_string());
                        result.error = Some(e.to_string());
                        return Ok(result);
                    }
                };

                let index = match repo.index() {
                    Ok(i) => i,
                    Err(e) => {
                        result.status = classify_error(&e.to_string());
                        result.error = Some(e.to_string());
                        return Ok(result);
                    }
                };

                if index.has_conflicts() {
                    let conflicts = match index.conflicts() {
                        Ok(c) => c,
                        Err(e) => {
                            result.status = classify_error(&e.to_string());
                            result.error = Some(e.to_string());
                            return Ok(result);
                        }
                    };

                    for c in conflicts.flatten() {
                        let mut best_path = None;
                        if let Some(our) = &c.our {
                            best_path = Some(String::from_utf8_lossy(&our.path).to_string());
                        } else if let Some(their) = &c.their {
                            best_path = Some(String::from_utf8_lossy(&their.path).to_string());
                        } else if let Some(ancestor) = &c.ancestor {
                            best_path = Some(String::from_utf8_lossy(&ancestor.path).to_string());
                        }

                        let real_path = match best_path {
                            Some(p) => p,
                            None => {
                                result.status = SyncStatus::Conflict;
                                result.error = Some("Sync Conflict: unknown path".to_string());
                                result.user_message =
                                    Some("存在无法识别路径的冲突文件，需要手动处理。".to_string());
                                continue;
                            }
                        };

                        let local_path = real_path.clone();
                        let remote_path = real_path.clone();

                        let sync_conflict = SyncConflict {
                            local_path,
                            remote_path,
                            local_hash: c
                                .our
                                .as_ref()
                                .map(|o| o.id.to_string())
                                .unwrap_or_default(),
                            remote_hash: c
                                .their
                                .as_ref()
                                .map(|o| o.id.to_string())
                                .unwrap_or_default(),
                            base_hash: c
                                .ancestor
                                .as_ref()
                                .map(|o| o.id.to_string())
                                .unwrap_or_default(),
                            created_at: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs() as i64,
                            description: "Git pull resulted in merge conflicts.".to_string(),
                        };

                        let mut local_content = None;
                        if let Some(our) = c.our {
                            if let Ok(blob) = repo.find_blob(our.id) {
                                if let Ok(content_str) = std::str::from_utf8(blob.content()) {
                                    local_content = Some(content_str.to_string());
                                }
                            }
                        }

                        if !Self::is_blacklisted_path(&sync_conflict.local_path)
                            && Self::is_whitelisted_path(&sync_conflict.local_path)
                        {
                            if let Err(e) = Self::record_sync_conflict(
                                workspace_path,
                                sync_conflict.clone(),
                                local_content.as_deref(),
                            ) {
                                let err_msg = format!("Failed to record sync conflict: {}", e);
                                result.error = match result.error {
                                    Some(ref mut err) => {
                                        err.push_str(&format!(" | {}", err_msg));
                                        Some(err.clone())
                                    }
                                    None => Some(err_msg),
                                };
                            }
                            result.conflicts.push(sync_conflict);
                        }
                    }
                }

                // We must abort the merge and cleanup state
                if let Err(e) = repo.cleanup_state() {
                    let err_msg = format!("Cleanup state failed: {}", e);
                    result.error = match result.error {
                        Some(ref mut err) => {
                            err.push_str(&format!(" | {}", err_msg));
                            Some(err.clone())
                        }
                        None => Some(err_msg),
                    };
                }

                let (local_dirty, _) = collect_git_status_summary(&repo);
                let fetch_commit_id = repo
                    .find_reference("FETCH_HEAD")
                    .ok()
                    .and_then(|r| r.target());
                let mut remote_changed = false;
                if let Some(remote_oid) = fetch_commit_id {
                    if let Ok(head) = repo.head() {
                        if let Some(local_oid) = head.target() {
                            if local_oid != remote_oid {
                                remote_changed = true;
                            }
                        }
                    }
                }

                let conflicted_files = result
                    .conflicts
                    .iter()
                    .map(|c| c.local_path.clone())
                    .collect::<Vec<_>>();

                result.conflict_summary = Some(SyncConflictSummary {
                    status: "conflict".to_string(),
                    local_dirty,
                    remote_changed,
                    conflicted_files,
                    blocked_reason: "自动合并失败，本地和远端都修改了同一批同步文件。".to_string(),
                    safe_next_steps: vec![
                        "备份当前工作区。".to_string(),
                        "运行诊断确认网络/认证没问题。".to_string(),
                        "手动处理冲突后重新同步。".to_string(),
                    ],
                });

                return Ok(result);
            } else {
                return Ok(SyncResult::error(
                    classify_error(&e.to_string()),
                    result.first_sync_mode,
                    Some(get_user_friendly_error(
                        &(format!("Pull failed: {}", e)).to_string(),
                    )),
                    format!("Pull failed: {}", e),
                ));
            }
        }
        // Get Plan
        let plan = match Self::build_sync_plan_from_workspace(workspace_path) {
            Ok(p) => p,
            Err(e) => {
                return Ok(SyncResult::error(
                    classify_error(&e.to_string()),
                    result.first_sync_mode,
                    Some(get_user_friendly_error(&(e.to_string()).to_string())),
                    e.to_string(),
                ));
            }
        };

        result.ignored_files = plan.ignored_files.clone();

        // Stage paths
        let paths_to_stage: Vec<&str> = plan.files_to_upload.iter().map(|s| s.as_str()).collect();
        if !paths_to_stage.is_empty() {
            if let Err(e) = backend.stage_paths(workspace_path, &paths_to_stage) {
                return Ok(SyncResult::error(
                    classify_error(&e.to_string()),
                    result.first_sync_mode,
                    Some(get_user_friendly_error(
                        &(format!("Stage failed: {}", e)).to_string(),
                    )),
                    format!("Stage failed: {}", e),
                ));
            }
        }

        let changed_files = backend.status(workspace_path).unwrap_or_default();
        let mut actual_staged = Vec::new();
        for file in changed_files {
            if paths_to_stage.contains(&file.as_str()) {
                actual_staged.push(file);
            }
        }

        // Commit if there are changes
        if !actual_staged.is_empty() {
            match backend.commit(workspace_path, "Auto sync") {
                Ok(Some(hash)) => {
                    result.commit_hash = Some(hash.clone());
                    result.uploaded_files = actual_staged;
                }
                Ok(None) => {}
                Err(e) => {
                    return Ok(SyncResult::error(
                        classify_error(&e.to_string()),
                        result.first_sync_mode,
                        Some(get_user_friendly_error(
                            &(format!("Commit failed: {}", e)).to_string(),
                        )),
                        format!("Commit failed: {}", e),
                    ));
                }
            }

            // Push
            if let Err(e) = backend
                .push(workspace_path, &config.branch, auth.as_ref(), Some(config))
                .map_err(map_git_error)
            {
                return Ok(SyncResult::error(
                    classify_error(&e.to_string()),
                    result.first_sync_mode,
                    Some(get_user_friendly_error(
                        &(format!("Push failed: {}", e)).to_string(),
                    )),
                    format!("Push failed: {}", e),
                ));
            }
        }

        if pull_branch_missing {
            result.user_message = Some("已初始化远端分支并完成首次同步".to_string());
        }

        // Update state
        let mut state = Self::load_sync_state(workspace_path).unwrap_or_default();
        state.remote_url = Some(config.remote_url.clone());
        state.transport = Some(config.transport.clone());
        state.last_sync_time = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        );
        if let Some(hash) = &result.commit_hash {
            state.last_synced_commit = Some(hash.clone());
        }
        state.last_error = result.error.clone();

        if let Err(e) = Self::save_sync_state(workspace_path, &state) {
            result.status = classify_error(&e.to_string());
            result.error = Some(format!("Failed to save sync state: {}", e));
            result.user_message = Some(
                "同步操作完成，但同步状态保存失败，请不要连续同步，先检查存储权限。".to_string(),
            );
            return Ok(result);
        }

        result.status = if branch_recovered {
            SyncStatus::BranchMissingRecovered
        } else {
            SyncStatus::Success
        };
        Ok(result)
    }

}

impl crate::sync_service::SyncService {
    pub fn new() -> Self {
        Self {
            config: None,
            status: SyncStatus::Idle,
        }
    }

}

impl crate::sync_service::SyncService {
    pub fn scan_workspace_for_sync(workspace_path: &Path) -> crate::Result<Vec<SyncFileEntry>> {
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

            // Skip .git
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
                .as_secs() as i64;

            let file_hash = match Self::compute_file_hash(&absolute_path) {
                Ok(h) => h,
                Err(_) => String::new(),
            };

            let sync_kind = if Self::is_whitelisted_path(&rel_path) {
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

}

impl crate::sync_service::SyncService {
    pub fn build_sync_plan_from_workspace(workspace_path: &Path) -> crate::Result<SyncPlan> {
        let mut plan = SyncPlan::new();

        let entries = Self::scan_workspace_for_sync(workspace_path)?;
        let state = Self::load_sync_state(workspace_path).unwrap_or_default();
        let is_first_sync = state.known_files.is_empty();

        let mut local_files = std::collections::HashSet::new();

        for entry in entries {
            if Self::is_blacklisted_path(&entry.relative_path) {
                plan.ignored_files.push(entry.relative_path.clone());
                continue;
            }

            if entry.sync_kind == SyncKind::Upload || entry.sync_kind == SyncKind::ConflictCandidate
            {
                local_files.insert(entry.relative_path.clone());
                let known_hash_opt = state.known_files.get(&entry.relative_path);
                if is_first_sync {
                    plan.files_to_upload.push(entry.relative_path.clone());
                } else if let Some(kh) = known_hash_opt {
                    if *kh != entry.file_hash {
                        // local changed
                        plan.files_to_upload.push(entry.relative_path.clone());
                    }
                } else {
                    // local added
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

}

impl crate::sync_service::SyncService {
    pub fn sync(&self) -> crate::Result<()> {
        Err(crate::Error::NotImplemented)
    }

}

impl Default for SyncService {
    fn default() -> Self {
        Self::new()
    }
}
