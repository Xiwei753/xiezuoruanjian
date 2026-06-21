#![allow(deprecated)]
use crate::sync::conflict::build_conflict_summary;
use crate::sync::conflict::collect_git_status_summary;

use crate::sync::git_backend::GitAuth;
use crate::sync::git_backend::GitBackend;
use crate::sync::lww;
use crate::sync::scanner;
use crate::sync::types::FirstSyncMode;
use crate::sync::types::SettingConflictDetail;
use crate::sync::types::SyncConfig;
use crate::sync::types::SyncConflict;
use crate::sync::types::SyncConflictSummary;
use crate::sync::types::SyncPlan;
use crate::sync::types::SyncResult;
use crate::sync::types::SyncSecrets;
use crate::sync::types::SyncStatus;
use crate::sync::types::SyncTransport;
use crate::sync::url::sanitize_remote_url;
use std::path::Path;

fn map_git_error(e: crate::Error) -> crate::Error {
    if let crate::Error::Io(io_err) = &e {
        let msg = io_err.to_string();
        if msg.contains("failed to resolve address") {
            return crate::Error::Io(std::io::Error::other(format!(
                "DNS 解析失败: {}",
                msg
            )));
        }
    }
    e
}

fn classify_error(e_str: &str) -> SyncStatus {
    let lower = e_str.to_lowercase();
    if lower.contains("recoverable_error") {
        SyncStatus::RecoverableError(e_str.replace("recoverable_error:", "").trim().to_string())
    } else if lower.contains("fatal_error") {
        SyncStatus::FatalError(e_str.replace("fatal_error:", "").trim().to_string())
    } else if lower.contains("auth")
        || lower.contains("token")
        || lower.contains("credential")
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
}

pub struct SyncService {
    pub config: Option<SyncConfig>,
    pub status: SyncStatus,
}

impl SyncService {
    fn ensure_local_branch_exists(repo: &git2::Repository, branch: &str) -> crate::Result<()> {
        let branch_ref_name = format!("refs/heads/{}", branch);

        let _ = repo.cleanup_state();

        if repo.find_reference(&branch_ref_name).is_ok() {
            let _ = repo.set_head(&branch_ref_name);
            return Ok(());
        }

        if let Ok(head_ref) = repo.head() {
            if let Ok(commit) = head_ref.peel_to_commit() {
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

        repo.set_head(&branch_ref_name).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "Failed to set symbolic HEAD to '{}': {}",
                branch, e
            )))
        })?;

        Ok(())
    }
}

impl SyncService {
    pub fn perform_sync_dry_run(
        workspace_path: &Path,
        config: &SyncConfig,
    ) -> crate::Result<SyncPlan> {
        if !config.enabled {
            return Ok(SyncPlan::new());
        }
        scanner::build_sync_plan_from_workspace(workspace_path)
    }
}

impl SyncService {
    pub fn perform_lww_sync(
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
    ) -> crate::Result<SyncResult> {
        lww::perform_lww_sync(workspace_path, config, secrets)
    }
}

enum PullOutcome {
    Continue { pull_branch_missing: bool },
    Return(SyncResult),
}

fn handle_pull_error(
    e: crate::Error,
    workspace_path: &Path,
    result: &SyncResult,
    first_sync_mode: FirstSyncMode,
) -> PullOutcome {
    let e_str = e.to_string();

    if e_str.contains("settings_conflict_payload:") {
        let payload_str = e_str
            .split("settings_conflict_payload:")
            .nth(1)
            .unwrap_or("")
            .trim();
        let details: Option<Vec<SettingConflictDetail>> = serde_json::from_str(payload_str).ok();
        let mut res = SyncResult::error(
            SyncStatus::Conflict,
            first_sync_mode,
            None,
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
        return PullOutcome::Return(res);
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
            first_sync_mode,
            None,
            "Pull failed due to conflict.".to_string(),
        );
        res.conflict_summary = summary;
        return PullOutcome::Return(res);
    }

    let e_str_lower = e_str.to_lowercase();
    if e_str_lower.contains("checkout_conflict")
        || e_str_lower.contains("local_blocking_file")
        || e_str_lower.contains("conflict prevents checkout")
    {
        let mut res = SyncResult::error(
            SyncStatus::Conflict,
            first_sync_mode,
            None,
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
        return PullOutcome::Return(res);
    }
    if e_str_lower.contains("unrelated")
        || e_str_lower.contains("merge")
        || e_str_lower.contains("no common ancestor")
    {
        let status = classify_error(&e.to_string());
        return PullOutcome::Return(SyncResult::error(
            status,
            FirstSyncMode::UnrelatedHistories,
            None,
            format!("Pull failed: {}", e),
        ));
    }
    if e_str_lower.contains("ref not found")
        || e_str_lower.contains("couldn't find remote ref")
        || (e_str_lower.contains("remote branch") && e_str_lower.contains("not found"))
    {
        if first_sync_mode != FirstSyncMode::InitExistingWorkspace
            && first_sync_mode != FirstSyncMode::AlreadyGitRepo
        {
            return PullOutcome::Return(SyncResult::error(
                classify_error(&e.to_string()),
                first_sync_mode,
                None,
                format!("Pull failed: {}", e),
            ));
        }
        PullOutcome::Continue {
            pull_branch_missing: true,
        }
    } else if e.to_string().contains("SyncConflict_Detected") {
        handle_merge_conflict(workspace_path, result, first_sync_mode)
    } else {
        PullOutcome::Return(SyncResult::error(
            classify_error(&e.to_string()),
            first_sync_mode,
            None,
            format!("Pull failed: {}", e),
        ))
    }
}

fn handle_merge_conflict(
    workspace_path: &Path,
    result: &SyncResult,
    _first_sync_mode: FirstSyncMode,
) -> PullOutcome {
    let mut result = result.clone();
    result.status = SyncStatus::Conflict;
    result.error = Some("Sync Conflict: automatic merge failed".to_string());

    let repo = match git2::Repository::open(workspace_path) {
        Ok(r) => r,
        Err(e) => {
            result.status = classify_error(&e.to_string());
            result.error = Some(e.to_string());
            return PullOutcome::Return(result);
        }
    };

    let index = match repo.index() {
        Ok(i) => i,
        Err(e) => {
            result.status = classify_error(&e.to_string());
            result.error = Some(e.to_string());
            return PullOutcome::Return(result);
        }
    };

    if index.has_conflicts() {
        let conflicts = match index.conflicts() {
            Ok(c) => c,
            Err(e) => {
                result.status = classify_error(&e.to_string());
                result.error = Some(e.to_string());
                return PullOutcome::Return(result);
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
                    result.user_message = None;
                    continue;
                }
            };

            let local_path = real_path.clone();
            let remote_path = real_path.clone();

            let sync_conflict = SyncConflict {
                local_path,
                remote_path,
                local_hash: c.our.as_ref().map(|o| o.id.to_string()).unwrap_or_default(),
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

            if !SyncService::is_blacklisted_path(&sync_conflict.local_path)
                && SyncService::is_whitelisted_path(&sync_conflict.local_path)
            {
                if let Err(e) = SyncService::record_sync_conflict(
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

    PullOutcome::Return(result)
}

impl SyncService {
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
                None,
                "Remote URL is empty".to_string(),
            ));
        }

        let parsed = sanitize_remote_url(&config.remote_url);
        let sanitized_url = parsed.sanitized_url;

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
                None,
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
                    None,
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
                        None,
                        e.to_string(),
                    ));
                }
            };

            if is_empty_or_git_only {
                result.first_sync_mode = FirstSyncMode::CloneIntoEmptyWorkspace;
                result.user_message = None;
                if let Err(e) = backend
                    .clone_repo(&sanitized_url, workspace_path, auth.as_ref())
                    .map_err(map_git_error)
                {
                    return Ok(SyncResult::error(
                        SyncStatus::Error(e.to_string()),
                        FirstSyncMode::CloneIntoEmptyWorkspace,
                        None,
                        e.to_string(),
                    ));
                }
            } else {
                result.first_sync_mode = FirstSyncMode::InitExistingWorkspace;
                result.user_message = None;
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
                    None,
                    e.to_string(),
                ));
            }
        }

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
                    None,
                    "Dirty repo blocked: non-whitelisted files modified".to_string(),
                ));
            }
        }

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
                        None,
                        e.to_string(),
                    ));
                }
                if let Err(e) = backend.commit(workspace_path, "Auto sync local changes") {
                    return Ok(SyncResult::error(
                        classify_error(&e.to_string()),
                        result.first_sync_mode,
                        None,
                        e.to_string(),
                    ));
                }
            }
        }

        let mut pull_branch_missing = false;
        let pull_failed = backend
            .pull(workspace_path, &config.branch, auth.as_ref())
            .map_err(map_git_error)
            .err();
        if let Some(e) = pull_failed {
            match handle_pull_error(e, workspace_path, &result, result.first_sync_mode.clone()) {
                PullOutcome::Continue {
                    pull_branch_missing: missing,
                } => {
                    if missing {
                        pull_branch_missing = true;
                        result.user_message = None;
                    }
                }
                PullOutcome::Return(res) => return Ok(res),
            }
        }

        let plan = match scanner::build_sync_plan_from_workspace(workspace_path) {
            Ok(p) => p,
            Err(e) => {
                return Ok(SyncResult::error(
                    classify_error(&e.to_string()),
                    result.first_sync_mode,
                    None,
                    e.to_string(),
                ));
            }
        };

        result.ignored_files = plan.ignored_files.clone();

        let paths_to_stage: Vec<&str> = plan.files_to_upload.iter().map(|s| s.as_str()).collect();
        if !paths_to_stage.is_empty() {
            if let Err(e) = backend.stage_paths(workspace_path, &paths_to_stage) {
                return Ok(SyncResult::error(
                    classify_error(&e.to_string()),
                    result.first_sync_mode,
                    None,
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
                        None,
                        format!("Commit failed: {}", e),
                    ));
                }
            }

            if let Err(e) = backend
                .push(workspace_path, &config.branch, auth.as_ref())
                .map_err(map_git_error)
            {
                return Ok(SyncResult::error(
                    classify_error(&e.to_string()),
                    result.first_sync_mode,
                    None,
                    format!("Push failed: {}", e),
                ));
            }
        }

        if pull_branch_missing {
            result.user_message = None;
        }

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
            result.user_message = None;
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

impl SyncService {
    pub fn new() -> Self {
        Self {
            config: None,
            status: SyncStatus::Idle,
        }
    }

    pub fn scan_workspace_for_sync(
        workspace_path: &Path,
    ) -> crate::Result<Vec<crate::sync::types::SyncFileEntry>> {
        scanner::scan_workspace_for_sync(workspace_path)
    }

    pub fn build_sync_plan_from_workspace(workspace_path: &Path) -> crate::Result<SyncPlan> {
        scanner::build_sync_plan_from_workspace(workspace_path)
    }

    pub fn sync(&self) -> crate::Result<()> {
        Err(crate::Error::NotImplemented)
    }
}

impl Default for SyncService {
    fn default() -> Self {
        Self::new()
    }
}
