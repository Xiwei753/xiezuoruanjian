//! 同步服务层 — 编排 Git 和 LWW 两种同步策略的统一入口。
//!
//! `SyncService` 是同步功能的业务编排层，提供：
//! - Git 同步（`perform_sync`）：基于 libgit2 的三路合并，需 `git-https` feature
//! - LWW 同步（`perform_lww_sync`）：基于 GitHub Contents API 的文件级 Last Writer Wins
//! - 诊断（`perform_sync_diagnostics`）：探测网络、认证、仓库和分支可用性
//! - 路径过滤（`is_blacklisted_path`/`is_whitelisted_path`）：见 `config_store` 模块
//!
//! ## 线程安全
//!
//! `SyncService` 的方法均为关联函数（无 `&self`），所有状态通过参数传递。
//! 调用方（`WriterAppService`）通过 `Mutex` 保证线程安全。

#[cfg(feature = "git-https")]
use crate::sync::conflict::collect_git_status_summary;

#[cfg(feature = "git-https")]
use crate::sync::git_backend::GitAuth;
#[cfg(feature = "git-https")]
use crate::sync::git_backend::GitBackend;
#[cfg(feature = "github-api")]
use crate::sync::lww;
use crate::sync::scanner;
#[cfg(feature = "git-https")]
use crate::sync::types::FirstSyncMode;
use crate::sync::types::SyncConfig;
#[cfg(feature = "git-https")]
use crate::sync::types::SyncConflict;
#[cfg(feature = "git-https")]
use crate::sync::types::SyncConflictSummary;
use crate::sync::types::SyncPlan;
#[cfg(feature = "git-https")]
use crate::sync::types::SyncProtocol;
#[cfg(any(feature = "git-https", feature = "github-api"))]
use crate::sync::types::SyncResult;
#[cfg(feature = "git-https")]
use crate::sync::types::SyncScope;
#[cfg(any(feature = "git-https", feature = "github-api"))]
use crate::sync::types::SyncSecrets;
use crate::sync::types::SyncStatus;
#[cfg(feature = "git-https")]
use crate::sync::url::sanitize_remote_url;
use std::path::Path;

#[cfg(feature = "git-https")]
/// 将 IO 错误中的网络类错误映射为 `SyncNetworkUnavailable`。
///
/// 仅处理 `AddrNotAvailable`（DNS 解析失败）和 `ConnectionAborted`（连接中断），
/// 其他 IO 错误（如 PermissionDenied、NotFound）不映射，保持原样传递。
fn map_git_error(e: crate::Error) -> crate::Error {
    if let crate::Error::Io(io_err) = &e {
        if io_err.kind() == std::io::ErrorKind::AddrNotAvailable
            || io_err.kind() == std::io::ErrorKind::ConnectionAborted
        {
            return crate::Error::SyncNetworkUnavailable {
                reason: format!("DNS 解析失败: {}", io_err),
            };
        }
    }
    e
}

#[cfg(feature = "git-https")]
/// 错误分类——将 `Error` 映射为 `SyncStatus` 供平台端展示。
///
/// 分类原则：
/// - `RecoverableError`：网络/认证临时故障，可自动重试
/// - `Conflict`：需要用户介入解决（checkout 冲突、设置冲突、文档冲突）
/// - `FatalError`：不可恢复的错误（IO 错误、未知错误）
fn classify_error(e: &crate::Error) -> SyncStatus {
    match e {
        crate::Error::SyncAuthFailed { .. } | crate::Error::SyncRateLimited { .. } => {
            SyncStatus::RecoverableError(e.to_string())
        }
        crate::Error::SyncNetworkUnavailable { .. } => SyncStatus::RecoverableError(e.to_string()),
        crate::Error::SyncCheckoutConflict { .. }
        | crate::Error::SyncConflictDetected
        | crate::Error::SyncDocumentConflict { .. } => SyncStatus::Conflict,
        crate::Error::SyncNonFastForward { .. } => SyncStatus::RecoverableError(e.to_string()),
        crate::Error::SyncUnrelatedHistories { .. } => SyncStatus::RecoverableError(e.to_string()),
        crate::Error::SyncRemoteBranchNotFound { .. } => {
            SyncStatus::RecoverableError(e.to_string())
        }
        crate::Error::SyncGithubApiError { category, .. } => {
            let cat = crate::sync::types::SyncErrorCategory::from_code(category, "");
            match cat {
                crate::sync::types::SyncErrorCategory::AuthError
                | crate::sync::types::SyncErrorCategory::TokenMissing
                | crate::sync::types::SyncErrorCategory::TokenInvalid
                | crate::sync::types::SyncErrorCategory::TokenPermissionDenied
                | crate::sync::types::SyncErrorCategory::GithubNetworkFailed
                | crate::sync::types::SyncErrorCategory::DnsFailed
                | crate::sync::types::SyncErrorCategory::TlsFailed
                | crate::sync::types::SyncErrorCategory::NetworkProbeFailed
                | crate::sync::types::SyncErrorCategory::ApiRateLimited => {
                    SyncStatus::RecoverableError(e.to_string())
                }
                _ => SyncStatus::FatalError(e.to_string()),
            }
        }
        crate::Error::Io(io_err) => SyncStatus::FatalError(io_err.to_string()),
        _ => SyncStatus::FatalError(e.to_string()),
    }
}

/// 同步服务。
///
/// 封装 Git 同步的完整生命周期：配置加载 → dry-run → 执行 → 冲突处理。
/// `config` 为 `None` 表示未配置同步；`status` 跟踪最近一次同步结果。
///
/// ## 错误分类
///
/// `classify_error` 将 `Error` 映射为 `SyncStatus`：
/// - `RecoverableError`：网络/认证临时故障，可重试
/// - `Conflict`：需要用户介入解决
/// - `FatalError`：不可恢复的错误
pub struct SyncService {
    pub config: Option<SyncConfig>,
    pub status: SyncStatus,
}

impl SyncService {
    #[cfg(feature = "git-https")]
    /// 确保本地 Git 分支存在。先清理 merge state（防止上次中断的 merge 残留），
    /// 再尝试查找或创建分支。`set_head` 失败时仍继续——HEAD 可能已指向正确引用。
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
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
    /// 干运行——构建同步计划但不执行文件传输。config.enabled=false 时返回空计划。
    pub fn perform_sync_dry_run(sync_root: &Path, config: &SyncConfig) -> crate::Result<SyncPlan> {
        if !config.enabled {
            return Ok(SyncPlan::new());
        }
        scanner::build_sync_plan(sync_root, config.scope)
    }
}

impl SyncService {
    #[cfg(feature = "github-api")]
    /// LWW 同步主入口——基于 Last-Writer-Wins 策略执行文件级同步。
    /// `force_sync=true` 跳过脏仓库保护等安全检查。
    /// `transport` 提供平台 HTTP 客户端实现，Core 不直接依赖 reqwest。
    pub fn perform_lww_sync(
        sync_root: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
        force_sync: bool,
        transport: &dyn writer_platform_api::SyncTransport,
    ) -> crate::Result<SyncResult> {
        lww::perform_lww_sync(sync_root, config, secrets, force_sync, transport)
    }
}

/// Git pull 操作的流程控制枚举。
///
/// - `Continue`：pull 成功或可忽略，继续后续步骤（stage → commit → push）
/// - `Return`：遇到冲突或不可恢复错误，立即返回同步结果
#[cfg(feature = "git-https")]
#[allow(clippy::large_enum_variant)]
enum PullOutcome {
    Continue,
    Return(SyncResult),
}

#[cfg(feature = "git-https")]
fn handle_pull_error(
    e: crate::Error,
    sync_root: &Path,
    result: &SyncResult,
    first_sync_mode: FirstSyncMode,
    scope: SyncScope,
) -> PullOutcome {
    match &e {
        crate::Error::SyncCheckoutConflict { summary_json } => {
            let summary: Option<SyncConflictSummary> = serde_json::from_str(summary_json).ok();
            let mut res = SyncResult::error(
                SyncStatus::Conflict,
                first_sync_mode,
                "Pull failed due to conflict.".to_string(),
                Some("conflict".to_string()),
            );
            res.conflict_summary = summary;
            PullOutcome::Return(res)
        }
        crate::Error::SyncConflictDetected => {
            handle_merge_conflict(sync_root, result, first_sync_mode, scope)
        }
        crate::Error::SyncUnrelatedHistories { .. } => PullOutcome::Return(SyncResult::error(
            classify_error(&e),
            FirstSyncMode::UnrelatedHistories,
            format!("Pull failed: {}", e),
            None,
        )),
        crate::Error::SyncRemoteBranchNotFound { .. } => {
            if first_sync_mode != FirstSyncMode::InitExistingProject
                && first_sync_mode != FirstSyncMode::AlreadyGitRepo
            {
                return PullOutcome::Return(SyncResult::error(
                    classify_error(&e),
                    first_sync_mode,
                    format!("Pull failed: {}", e),
                    Some("remote_branch_missing".to_string()),
                ));
            }
            PullOutcome::Continue
        }
        _ => PullOutcome::Return(SyncResult::error(
            classify_error(&e),
            first_sync_mode,
            format!("Pull failed: {}", e),
            None,
        )),
    }
}

#[cfg(feature = "git-https")]
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
fn handle_merge_conflict(
    sync_root: &Path,
    result: &SyncResult,
    _first_sync_mode: FirstSyncMode,
    scope: SyncScope,
) -> PullOutcome {
    let mut result = result.clone();
    result.status = SyncStatus::Conflict;
    result.error = Some("Sync Conflict: automatic merge failed".to_string());

    let repo = match git2::Repository::open(sync_root) {
        Ok(r) => r,
        Err(e) => {
            let err = crate::Error::Io(std::io::Error::other(e.to_string()));
            result.status = classify_error(&err);
            result.error = Some(e.to_string());
            return PullOutcome::Return(result);
        }
    };

    let index = match repo.index() {
        Ok(i) => i,
        Err(e) => {
            let err = crate::Error::Io(std::io::Error::other(e.to_string()));
            result.status = classify_error(&err);
            result.error = Some(e.to_string());
            return PullOutcome::Return(result);
        }
    };

    if index.has_conflicts() {
        let conflicts = match index.conflicts() {
            Ok(c) => c,
            Err(e) => {
                let err = crate::Error::Io(std::io::Error::other(e.to_string()));
                result.status = classify_error(&err);
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
                #[allow(clippy::cast_possible_wrap)]
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

            if !SyncService::is_blacklisted_path(&sync_conflict.local_path, scope)
                && SyncService::is_whitelisted_path(&sync_conflict.local_path, scope)
            {
                if let Err(e) = SyncService::record_sync_conflict(
                    sync_root,
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

    let (local_dirty, _) = collect_git_status_summary(&repo, scope);
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
            "备份当前作品目录。".to_string(),
            "运行诊断确认网络/认证没问题。".to_string(),
            "手动处理冲突后重新同步。".to_string(),
        ],
    });

    PullOutcome::Return(result)
}

impl SyncService {
    #[cfg(feature = "git-https")]
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn perform_sync(
        sync_root: &Path,
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
                "Remote URL is empty".to_string(),
                Some("empty_url".to_string()),
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
                "No token provided".to_string(),
                Some("token_missing".to_string()),
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
            SyncProtocol::HttpsToken => Some(GitAuth::HttpsToken {
                username: username_for_cred.clone(),
                token: token.clone(),
            }),
            SyncProtocol::SshDeployKey => {
                return Ok(SyncResult::error(
                    SyncStatus::Error("SshDeployKey is not implemented".to_string()),
                    FirstSyncMode::NotAttempted,
                    "SshDeployKey is not implemented".to_string(),
                    None,
                ));
            }
        };

        let has_repo = backend.has_repo(sync_root);
        if !has_repo {
            let is_empty_or_git_only = match backend.is_worktree_empty_or_git_only(sync_root) {
                Ok(val) => val,
                Err(e) => {
                    return Ok(SyncResult::error(
                        SyncStatus::Error(e.to_string()),
                        FirstSyncMode::NotAttempted,
                        e.to_string(),
                        None,
                    ));
                }
            };

            if is_empty_or_git_only {
                result.first_sync_mode = FirstSyncMode::CloneIntoEmptyProject;
                if let Err(e) = backend
                    .clone_repo(&sanitized_url, sync_root, auth.as_ref())
                    .map_err(map_git_error)
                {
                    return Ok(SyncResult::error(
                        SyncStatus::Error(e.to_string()),
                        FirstSyncMode::CloneIntoEmptyProject,
                        e.to_string(),
                        None,
                    ));
                }
            } else {
                result.first_sync_mode = FirstSyncMode::InitExistingProject;
                if let Err(e) = backend.init_repo(sync_root) {
                    result.status = SyncStatus::Error(e.to_string());
                    result.error = Some(e.to_string());
                    return Ok(result);
                }
                if let Err(e) = backend.ensure_remote(sync_root, &sanitized_url) {
                    result.status = SyncStatus::Error(e.to_string());
                    result.error = Some(e.to_string());
                    return Ok(result);
                }
            }
        } else {
            result.first_sync_mode = FirstSyncMode::AlreadyGitRepo;
            if let Err(e) = backend.open_repo(sync_root) {
                result.status = SyncStatus::Error(e.to_string());
                result.error = Some(e.to_string());
                return Ok(result);
            }
            if let Err(e) = backend.ensure_remote(sync_root, &sanitized_url) {
                result.status = SyncStatus::Error(e.to_string());
                result.error = Some(e.to_string());
                return Ok(result);
            }
        }

        let mut branch_recovered = false;
        if let Ok(repo) = git2::Repository::open(sync_root) {
            let branch_ref_name = format!("refs/heads/{}", config.branch);
            let branch_exists = repo.find_reference(&branch_ref_name).is_ok();
            let head_commit = repo.head().ok().and_then(|r| r.peel_to_commit().ok());
            if !branch_exists && head_commit.is_some() {
                branch_recovered = true;
            }

            if let Err(e) = Self::ensure_local_branch_exists(&repo, &config.branch) {
                return Ok(SyncResult::error(
                    classify_error(&e),
                    result.first_sync_mode,
                    e.to_string(),
                    None,
                ));
            }
        }

        if let Ok(status_list) = backend.status(sync_root, config.scope) {
            let mut dirty_non_whitelisted = Vec::new();
            for p in &status_list {
                if !SyncService::is_blacklisted_path(p, config.scope)
                    && !SyncService::is_whitelisted_path(p, config.scope)
                {
                    dirty_non_whitelisted.push(p.clone());
                }
            }
            if !dirty_non_whitelisted.is_empty() {
                return Ok(SyncResult::error(
                    SyncStatus::DirtyRepoBlocked,
                    result.first_sync_mode,
                    "Dirty repo blocked: non-whitelisted files modified".to_string(),
                    None,
                ));
            }
        }

        if let Ok(status_list) = backend.status(sync_root, config.scope) {
            let mut paths_to_stage = Vec::new();
            for p in &status_list {
                if SyncService::is_whitelisted_path(p, config.scope)
                    && !SyncService::is_blacklisted_path(p, config.scope)
                {
                    paths_to_stage.push(p.as_str());
                }
            }
            if !paths_to_stage.is_empty() {
                if let Err(e) = backend.stage_paths(sync_root, &paths_to_stage, config.scope) {
                    return Ok(SyncResult::error(
                        classify_error(&e),
                        result.first_sync_mode,
                        e.to_string(),
                        None,
                    ));
                }
                if let Err(e) = backend.commit(sync_root, "Auto sync local changes") {
                    return Ok(SyncResult::error(
                        classify_error(&e),
                        result.first_sync_mode,
                        e.to_string(),
                        None,
                    ));
                }
            }
        }

        let pull_failed = backend
            .pull(sync_root, &config.branch, auth.as_ref(), config.scope)
            .map_err(map_git_error)
            .err();
        if let Some(e) = pull_failed {
            match handle_pull_error(
                e,
                sync_root,
                &result,
                result.first_sync_mode.clone(),
                config.scope,
            ) {
                PullOutcome::Continue => {}
                PullOutcome::Return(res) => return Ok(res),
            }
        }

        // ── Process pending_take_remote ──
        // For each path in pending_take_remote, force-checkout the file from the
        // remote branch so the local file matches the remote version.
        let mut state_for_pending = Self::load_sync_state(sync_root).unwrap_or_default();
        if !state_for_pending.pending_take_remote.is_empty() {
            log::debug!(
                "[sync] processing pending_take_remote count={}",
                state_for_pending.pending_take_remote.len()
            );
            let mut succeeded: std::collections::HashSet<String> = std::collections::HashSet::new();
            if let Ok(repo) = git2::Repository::open(sync_root) {
                let remote_branch_ref = format!("refs/remotes/origin/{}", config.branch);
                if let Ok(remote_commit) = repo
                    .find_reference(&remote_branch_ref)
                    .and_then(|r| r.peel_to_commit())
                {
                    let remote_tree = remote_commit.tree();
                    for path in state_for_pending.pending_take_remote.iter() {
                        if let Ok(ref tree) = remote_tree {
                            if let Ok(entry) = tree.get_path(std::path::Path::new(path)) {
                                if let Ok(blob) = repo.find_blob(entry.id()) {
                                    let full_path = sync_root.join(path);
                                    if let Some(parent) = full_path.parent() {
                                        let _ = std::fs::create_dir_all(parent);
                                    }
                                    let content = blob.content();
                                    let tmp_path = full_path
                                        .with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
                                    if let Err(e) = std::fs::write(&tmp_path, content) {
                                        log::warn!(
                                            "[sync] pending_take_remote: write failed path={} err={}",
                                            path, e
                                        );
                                        continue;
                                    }
                                    if let Err(e) = std::fs::rename(&tmp_path, &full_path) {
                                        log::warn!(
                                            "[sync] pending_take_remote: rename failed path={} err={}",
                                            path, e
                                        );
                                        continue;
                                    }
                                    // Update known_files to the hash of the newly written content
                                    let hash = format!("{:x}", md5::compute(content));
                                    state_for_pending.known_files.insert(path.clone(), hash);
                                    let now_ts = chrono::Utc::now().timestamp_millis();
                                    state_for_pending
                                        .known_files_updated_at
                                        .insert(path.clone(), now_ts);
                                    result.downloaded_files.push(path.clone());
                                    succeeded.insert(path.clone());
                                    log::debug!(
                                        "[sync] pending_take_remote checked out path={}",
                                        path
                                    );
                                }
                            } else {
                                log::debug!(
                                    "[sync] pending_take_remote: remote file missing for path={}, keeping in pending",
                                    path
                                );
                            }
                        } else {
                            log::debug!(
                                "[sync] pending_take_remote: could not get remote tree for path={}, keeping in pending",
                                path
                            );
                        }
                    }
                } else {
                    log::warn!(
                        "[sync] pending_take_remote: could not resolve remote branch ref={}, keeping all in pending",
                        remote_branch_ref
                    );
                }
            }
            // Only clear paths that were successfully downloaded;
            // failed/missing paths remain in pending_take_remote so the user
            // is not silently left with stale local content.
            state_for_pending
                .pending_take_remote
                .retain(|p| !succeeded.contains(p));
            let _ = Self::save_sync_state(sync_root, &state_for_pending);
        }

        let plan = match scanner::build_sync_plan(sync_root, config.scope) {
            Ok(p) => p,
            Err(e) => {
                return Ok(SyncResult::error(
                    classify_error(&e),
                    result.first_sync_mode,
                    e.to_string(),
                    None,
                ));
            }
        };

        result.ignored_files = plan.ignored_files.clone();

        let paths_to_stage: Vec<&str> = plan.files_to_upload.iter().map(|s| s.as_str()).collect();
        if !paths_to_stage.is_empty() {
            if let Err(e) = backend.stage_paths(sync_root, &paths_to_stage, config.scope) {
                return Ok(SyncResult::error(
                    classify_error(&e),
                    result.first_sync_mode,
                    e.to_string(),
                    None,
                ));
            }
        }

        let changed_files = backend.status(sync_root, config.scope).unwrap_or_default();
        let mut actual_staged = Vec::new();
        for file in changed_files {
            if paths_to_stage.contains(&file.as_str()) {
                actual_staged.push(file);
            }
        }

        if !actual_staged.is_empty() {
            match backend.commit(sync_root, "Auto sync") {
                Ok(Some(hash)) => {
                    result.commit_hash = Some(hash.clone());
                    result.uploaded_files = actual_staged;
                }
                Ok(None) => {}
                Err(e) => {
                    return Ok(SyncResult::error(
                        classify_error(&e),
                        result.first_sync_mode,
                        format!("Commit failed: {}", e),
                        None,
                    ));
                }
            }

            if let Err(e) = backend
                .push(sync_root, &config.branch, auth.as_ref())
                .map_err(map_git_error)
            {
                return Ok(SyncResult::error(
                    classify_error(&e),
                    result.first_sync_mode,
                    format!("Push failed: {}", e),
                    None,
                ));
            }
        }

        let mut state = Self::load_sync_state(sync_root).unwrap_or_default();
        state.remote_url = Some(config.remote_url.clone());
        state.transport = Some(config.transport.clone());
        state.last_sync_time = Some({
            #[allow(clippy::cast_possible_wrap)]
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            ts
        });
        if let Some(hash) = &result.commit_hash {
            state.last_synced_commit = Some(hash.clone());
        }
        state.last_error = result.error.clone();

        if let Err(e) = Self::save_sync_state(sync_root, &state) {
            result.status = classify_error(&e);
            result.error = Some(format!("Failed to save sync state: {}", e));
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

    /// 扫描作品目录中所有可同步文件。
    pub fn scan_for_sync(
        sync_root: &Path,
        scope: crate::sync::types::SyncScope,
    ) -> crate::Result<Vec<crate::sync::types::SyncFileEntry>> {
        scanner::scan_for_sync(sync_root, scope)
    }

    /// 从作品目录构建同步计划（上传/下载/删除/冲突文件列表）。
    pub fn build_sync_plan(
        sync_root: &Path,
        scope: crate::sync::types::SyncScope,
    ) -> crate::Result<SyncPlan> {
        scanner::build_sync_plan(sync_root, scope)
    }

    /// 占位同步方法——当前返回 NotImplemented，实际同步通过 perform_lww_sync 执行。
    pub fn sync(&self) -> crate::Result<()> {
        Err(crate::Error::NotImplemented)
    }
}

impl Default for SyncService {
    fn default() -> Self {
        Self::new()
    }
}
