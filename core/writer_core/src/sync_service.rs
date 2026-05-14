use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SyncTransport {
    HttpsToken,
    SshDeployKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    pub enabled: bool,
    pub remote_url: String,
    pub transport: SyncTransport,
    #[serde(default = "default_branch")]
    pub branch: String,
    pub auto_sync: bool,
    pub sync_interval_seconds: u32,
}

fn default_branch() -> String {
    "main".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncSecrets {
    pub token: Option<String>,
    pub ssh_private_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Idle,
    Syncing,
    Error(String),
    Conflict,
    Success,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SyncKind {
    Upload,
    Ignore,
    ConflictCandidate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFileEntry {
    pub relative_path: String,
    pub absolute_path: String,
    pub file_hash: String,
    pub modified_time: i64,
    pub sync_kind: SyncKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflict {
    pub local_path: String,
    pub remote_path: String,
    pub local_hash: String,
    pub remote_hash: String,
    pub base_hash: String,
    pub created_at: i64,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub status: SyncStatus,
    pub uploaded_files: Vec<String>,
    pub downloaded_files: Vec<String>,
    pub ignored_files: Vec<String>,
    pub conflicts: Vec<SyncConflict>,
    pub commit_hash: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum GitAuth {
    HttpsToken { username: String, token: String },
    SshDeployKey,
}

pub trait GitBackend {
    fn clone_repo(
        &self,
        remote_url: &str,
        local_repo_path: &Path,
        auth: Option<&GitAuth>,
    ) -> crate::Result<()>;
    fn open_repo(&self, local_repo_path: &Path) -> crate::Result<()>;
    fn pull(
        &self,
        local_repo_path: &Path,
        branch: &str,
        auth: Option<&GitAuth>,
    ) -> crate::Result<()>;
    fn stage_paths(&self, local_repo_path: &Path, paths: &[&str]) -> crate::Result<()>;
    fn commit(&self, local_repo_path: &Path, message: &str) -> crate::Result<Option<String>>;
    fn push(
        &self,
        local_repo_path: &Path,
        branch: &str,
        auth: Option<&GitAuth>,
    ) -> crate::Result<()>;
    fn current_head(&self, local_repo_path: &Path) -> crate::Result<Option<String>>;
    fn status(&self, local_repo_path: &Path) -> crate::Result<Vec<String>>; // Returns changed files
}

pub struct Git2Backend;

impl Git2Backend {
    fn build_callbacks<'a>(auth: Option<&'a GitAuth>) -> git2::RemoteCallbacks<'a> {
        let mut callbacks = git2::RemoteCallbacks::new();
        if let Some(auth) = auth {
            callbacks.credentials(move |_url, username_from_url, _allowed_types| match auth {
                GitAuth::HttpsToken { username, token } => {
                    let user = username_from_url.unwrap_or(username);
                    git2::Cred::userpass_plaintext(user, token)
                }
                GitAuth::SshDeployKey => {
                    Err(git2::Error::from_str("SshDeployKey is NotImplemented"))
                }
            });
        }
        callbacks
    }
}

impl GitBackend for Git2Backend {
    fn clone_repo(
        &self,
        remote_url: &str,
        local_repo_path: &Path,
        auth: Option<&GitAuth>,
    ) -> crate::Result<()> {
        let mut fetch_options = git2::FetchOptions::new();
        let callbacks = Self::build_callbacks(auth);
        fetch_options.remote_callbacks(callbacks);

        let mut builder = git2::build::RepoBuilder::new();
        builder.fetch_options(fetch_options);

        builder
            .clone(remote_url, local_repo_path)
            .map_err(|e: git2::Error| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
        Ok(())
    }

    fn open_repo(&self, local_repo_path: &Path) -> crate::Result<()> {
        git2::Repository::open(local_repo_path).map_err(|e: git2::Error| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        Ok(())
    }

    fn pull(
        &self,
        local_repo_path: &Path,
        branch: &str,
        auth: Option<&GitAuth>,
    ) -> crate::Result<()> {
        let repo = git2::Repository::open(local_repo_path).map_err(|e: git2::Error| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        let mut remote = repo.find_remote("origin").map_err(|e: git2::Error| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;

        let mut fetch_options = git2::FetchOptions::new();
        fetch_options.remote_callbacks(Self::build_callbacks(auth));

        remote
            .fetch(&[branch], Some(&mut fetch_options), None)
            .map_err(|e: git2::Error| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;

        let fetch_head = repo
            .find_reference("FETCH_HEAD")
            .map_err(|e: git2::Error| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
        let fetch_commit =
            repo.reference_to_annotated_commit(&fetch_head)
                .map_err(|e: git2::Error| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })?;

        let mut statuses = git2::StatusOptions::new();
        statuses.include_untracked(true);
        let has_uncommitted_changes = repo
            .statuses(Some(&mut statuses))
            .map_err(|e| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?
            .iter()
            .any(|s| {
                if let Some(path) = s.path() {
                    SyncService::is_whitelisted_path(path)
                } else {
                    false
                }
            });

        if has_uncommitted_changes {
            return Err(crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Uncommitted changes exist. Commit before pulling.",
            )));
        }

        let analysis = repo
            .merge_analysis(&[&fetch_commit])
            .map_err(|e: git2::Error| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
        if analysis.0.is_up_to_date() {
            // Do nothing
        } else if analysis.0.is_fast_forward() {
            let refname = format!("refs/heads/{}", branch);
            let mut reference = repo.find_reference(&refname).map_err(|e: git2::Error| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
            reference
                .set_target(fetch_commit.id(), "Fast-Forward")
                .map_err(|e: git2::Error| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })?;
            repo.set_head(&refname).map_err(|e: git2::Error| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
            // Safe to checkout since we checked for uncommitted changes
            repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
                .map_err(|e: git2::Error| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })?;
        } else if analysis.0.is_normal() {
            let mut merge_opts = git2::MergeOptions::new();
            repo.merge(&[&fetch_commit], Some(&mut merge_opts), None)
                .map_err(|e: git2::Error| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })?;

            let mut index = repo.index().map_err(|e: git2::Error| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
            if index.has_conflicts() {
                // Return an error for conflicts with a special prefix that can be parsed
                return Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "SyncConflict_Detected".to_string(),
                )));
            } else {
                let oid = index.write_tree().map_err(|e: git2::Error| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })?;
                let signature = git2::Signature::now("Sync User", "sync@writer.app").map_err(
                    |e: git2::Error| {
                        crate::Error::Io(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            e.to_string(),
                        ))
                    },
                )?;
                let tree = repo.find_tree(oid).map_err(|e: git2::Error| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })?;
                let head_ref = repo.head().map_err(|e| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })?;
                let head_commit = head_ref.peel_to_commit().map_err(|e| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })?;
                let fetch_commit_obj = repo.find_commit(fetch_commit.id()).map_err(|e| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })?;
                repo.commit(
                    Some("HEAD"),
                    &signature,
                    &signature,
                    "Merge remote-tracking branch",
                    &tree,
                    &[&head_commit, &fetch_commit_obj],
                )
                .map_err(|e: git2::Error| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })?;
                repo.cleanup_state().map_err(|e| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })?;
            }
        } else {
            return Err(crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Unable to pull: remote branch is unrelated or unable to merge",
            )));
        }

        Ok(())
    }

    fn stage_paths(&self, local_repo_path: &Path, paths: &[&str]) -> crate::Result<()> {
        let repo = git2::Repository::open(local_repo_path).map_err(|e: git2::Error| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        let mut index = repo.index().map_err(|e: git2::Error| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        for p in paths {
            if SyncService::is_blacklisted_path(p) {
                continue;
            }
            index.add_path(Path::new(p)).map_err(|e: git2::Error| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
        }
        index.write().map_err(|e: git2::Error| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        Ok(())
    }

    fn commit(&self, local_repo_path: &Path, message: &str) -> crate::Result<Option<String>> {
        let repo = git2::Repository::open(local_repo_path).map_err(|e: git2::Error| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        let mut index = repo.index().map_err(|e: git2::Error| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        let oid = index.write_tree().map_err(|e: git2::Error| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        let signature =
            git2::Signature::now("Sync User", "sync@writer.app").map_err(|e: git2::Error| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;

        let tree = repo.find_tree(oid).map_err(|e: git2::Error| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;

        let head_commit = match repo.head() {
            Ok(head) => {
                let target = head.target().ok_or_else(|| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "HEAD target not found",
                    ))
                })?;
                Some(repo.find_commit(target).map_err(|e| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })?)
            }
            Err(_) => None,
        };

        let parents = if let Some(ref c) = head_commit {
            vec![c]
        } else {
            vec![]
        };

        let mut parent_refs = vec![];
        for p in &parents {
            parent_refs.push(*p);
        }

        let commit_id = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &parent_refs,
            )
            .map_err(|e: git2::Error| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;

        Ok(Some(commit_id.to_string()))
    }

    fn push(
        &self,
        local_repo_path: &Path,
        branch: &str,
        auth: Option<&GitAuth>,
    ) -> crate::Result<()> {
        let repo = git2::Repository::open(local_repo_path).map_err(|e: git2::Error| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        let mut remote = repo.find_remote("origin").map_err(|e: git2::Error| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;

        let mut push_options = git2::PushOptions::new();
        push_options.remote_callbacks(Self::build_callbacks(auth));

        let refspec = format!("refs/heads/{}:refs/heads/{}", branch, branch);
        remote
            .push(&[&refspec], Some(&mut push_options))
            .map_err(|e: git2::Error| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
        Ok(())
    }

    fn current_head(&self, local_repo_path: &Path) -> crate::Result<Option<String>> {
        let repo = git2::Repository::open(local_repo_path).map_err(|e: git2::Error| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        if let Ok(head) = repo.head() {
            if let Some(target) = head.target() {
                return Ok(Some(target.to_string()));
            }
        }
        Ok(None)
    }

    fn status(&self, local_repo_path: &Path) -> crate::Result<Vec<String>> {
        let repo = git2::Repository::open(local_repo_path).map_err(|e: git2::Error| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true);
        let statuses = repo.statuses(Some(&mut opts)).map_err(|e: git2::Error| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        let mut res = Vec::new();
        for entry in statuses.iter() {
            if let Some(path) = entry.path() {
                res.push(path.to_string());
            }
        }
        Ok(res)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPlan {
    pub files_to_upload: Vec<String>,
    pub files_to_download: Vec<String>,
    pub files_to_delete_local: Vec<String>,
    pub files_to_delete_remote: Vec<String>,
    pub ignored_files: Vec<String>,
}

impl Default for SyncPlan {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncPlan {
    pub fn new() -> Self {
        Self {
            files_to_upload: Vec::new(),
            files_to_download: Vec::new(),
            files_to_delete_local: Vec::new(),
            files_to_delete_remote: Vec::new(),
            ignored_files: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    pub remote_url: Option<String>,
    pub transport: Option<SyncTransport>,
    pub last_synced_commit: Option<String>,
    pub last_sync_time: Option<i64>,
    pub last_error: Option<String>,
    pub known_files: std::collections::HashMap<String, String>,
    pub conflicts: Vec<SyncConflict>,
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            remote_url: None,
            transport: None,
            last_synced_commit: None,
            last_sync_time: None,
            last_error: None,
            known_files: std::collections::HashMap::new(),
            conflicts: Vec::new(),
        }
    }
}

pub struct SyncService {
    pub config: Option<SyncConfig>,
    pub status: SyncStatus,
}

impl SyncService {
    pub fn perform_sync_dry_run(
        workspace_path: &Path,
        config: &SyncConfig,
    ) -> crate::Result<SyncPlan> {
        if !config.enabled {
            return Ok(SyncPlan::new());
        }
        Self::build_sync_plan_from_workspace(workspace_path)
    }

    pub fn perform_sync(
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
        backend: &dyn GitBackend,
    ) -> crate::Result<SyncResult> {
        let mut result = SyncResult {
            status: SyncStatus::Idle,
            uploaded_files: Vec::new(),
            downloaded_files: Vec::new(),
            ignored_files: Vec::new(),
            conflicts: Vec::new(),
            commit_hash: None,
            error: None,
        };

        if !config.enabled {
            result.status = SyncStatus::Success;
            return Ok(result);
        }

        if config.remote_url.is_empty() {
            result.status = SyncStatus::Error("Remote URL is empty".to_string());
            result.error = Some("Remote URL is empty".to_string());
            return Ok(result);
        }

        let auth = match &config.transport {
            SyncTransport::HttpsToken => {
                if let Some(token) = &secrets.token {
                    Some(GitAuth::HttpsToken {
                        username: "sync_user".to_string(), // In GitHub, token is the password, username can be anything, but usually we use token as password or username
                        token: token.clone(),
                    })
                } else {
                    result.status = SyncStatus::Error("No token provided".to_string());
                    result.error = Some("No token provided".to_string());
                    return Ok(result);
                }
            }
            SyncTransport::SshDeployKey => {
                result.status = SyncStatus::Error("SshDeployKey is not implemented".to_string());
                result.error = Some("SshDeployKey is not implemented".to_string());
                return Ok(result);
            }
        };

        let git_dir = workspace_path.join(".git");
        if !git_dir.exists() {
            // Clone
            if let Err(e) = backend.clone_repo(&config.remote_url, workspace_path, auth.as_ref()) {
                result.status = SyncStatus::Error(e.to_string());
                result.error = Some(e.to_string());
                return Ok(result);
            }
        } else {
            // Open
            if let Err(e) = backend.open_repo(workspace_path) {
                result.status = SyncStatus::Error(e.to_string());
                result.error = Some(e.to_string());
                return Ok(result);
            }
        }

        // Pull
        if let Err(e) = backend.pull(workspace_path, &config.branch, auth.as_ref()) {
            if e.to_string().contains("SyncConflict_Detected") {
                result.status = SyncStatus::Conflict;
                result.error = Some("Sync Conflict: automatic merge failed".to_string());

                // Iterate through git index conflicts and record them
                let repo = match git2::Repository::open(workspace_path) {
                    Ok(r) => r,
                    Err(e) => {
                        result.error = Some(e.to_string());
                        return Ok(result);
                    }
                };

                let index = match repo.index() {
                    Ok(i) => i,
                    Err(e) => {
                        result.error = Some(e.to_string());
                        return Ok(result);
                    }
                };

                if index.has_conflicts() {
                    let conflicts = match index.conflicts() {
                        Ok(c) => c,
                        Err(e) => {
                            result.error = Some(e.to_string());
                            return Ok(result);
                        }
                    };

                    for conflict in conflicts {
                        if let Ok(c) = conflict {
                            let local_path = c
                                .our
                                .as_ref()
                                .map(|o| String::from_utf8_lossy(&o.path).to_string())
                                .unwrap_or_else(|| "unknown".to_string());
                            let remote_path = c
                                .their
                                .as_ref()
                                .map(|o| String::from_utf8_lossy(&o.path).to_string())
                                .unwrap_or_else(|| "unknown".to_string());

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

                            let _ = Self::record_sync_conflict(
                                workspace_path,
                                sync_conflict.clone(),
                                local_content.as_deref(),
                            );
                            result.conflicts.push(sync_conflict);
                        }
                    }
                }

                // We must abort the merge and cleanup state
                let _ = repo.cleanup_state();

                return Ok(result);
            }

            result.status = SyncStatus::Error(format!("Pull failed: {}", e));
            result.error = Some(format!("Pull failed: {}", e));
            return Ok(result);
        }
        // Get Plan
        let plan = match Self::build_sync_plan_from_workspace(workspace_path) {
            Ok(p) => p,
            Err(e) => {
                result.status = SyncStatus::Error(e.to_string());
                result.error = Some(e.to_string());
                return Ok(result);
            }
        };

        result.ignored_files = plan.ignored_files.clone();

        // Stage paths
        let paths_to_stage: Vec<&str> = plan.files_to_upload.iter().map(|s| s.as_str()).collect();
        if !paths_to_stage.is_empty() {
            if let Err(e) = backend.stage_paths(workspace_path, &paths_to_stage) {
                result.status = SyncStatus::Error(format!("Stage failed: {}", e));
                result.error = Some(format!("Stage failed: {}", e));
                return Ok(result);
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
                    result.status = SyncStatus::Error(format!("Commit failed: {}", e));
                    result.error = Some(format!("Commit failed: {}", e));
                    return Ok(result);
                }
            }

            // Push
            if let Err(e) = backend.push(workspace_path, &config.branch, auth.as_ref()) {
                result.status = SyncStatus::Error(format!("Push failed: {}", e));
                result.error = Some(format!("Push failed: {}", e));
                return Ok(result);
            }
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

        let _ = Self::save_sync_state(workspace_path, &state);

        result.status = SyncStatus::Success;
        Ok(result)
    }

    pub fn new() -> Self {
        Self {
            config: None,
            status: SyncStatus::Idle,
        }
    }

    pub fn is_blacklisted_path(rel_path: &str) -> bool {
        let ignored_patterns = [
            "app-meta/settings/settings.local.json",
            "app-meta/sync/sync_secrets.local.json",
            "sqlite_cache",
            "tmp",
            "backups",
            "trash",
        ];

        if rel_path.ends_with(".tmp") || rel_path.ends_with(".lock") {
            return true;
        }

        if rel_path.starts_with("app-meta/logs") || rel_path.contains("/logs/") {
            return true;
        }

        for pattern in ignored_patterns {
            if rel_path.contains(pattern) {
                return true;
            }
        }

        false
    }

    pub fn is_whitelisted_path(rel_path: &str) -> bool {
        if Self::is_blacklisted_path(rel_path) {
            return false;
        }

        if rel_path == "workspace_manifest.json" {
            return true;
        }
        if rel_path == "app-meta/settings/settings.sync.json" {
            return true;
        }

        if rel_path.starts_with("projects/") {
            if rel_path.ends_with("/project.json") {
                return true;
            }
            if rel_path.contains("/volumes/") && rel_path.ends_with("/volume.json") {
                return true;
            }
            if rel_path.contains("/chapters/") && rel_path.ends_with("/chapter.md") {
                return true;
            }
            if rel_path.contains("/chapters/") && rel_path.ends_with("/chapter.meta.json") {
                return true;
            }
            if rel_path.contains("/characters/") {
                return true;
            }
            if rel_path.contains("/outline/") {
                return true;
            }
            if rel_path.contains("/graphs/") {
                return true;
            }
            return false;
        }

        if rel_path.starts_with("app-meta/graphs/") {
            return true;
        }

        if rel_path.starts_with("app-meta/ai/") {
            return true;
        }

        if rel_path.starts_with("app-meta/proofreading/") {
            return true;
        }

        false
    }

    fn compute_file_hash(path: &Path) -> std::io::Result<String> {
        let content = std::fs::read(path)?;
        Ok(format!("{:x}", md5::compute(content)))
    }

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

    pub fn build_sync_plan_from_workspace(workspace_path: &Path) -> crate::Result<SyncPlan> {
        let mut plan = SyncPlan::new();

        let entries = Self::scan_workspace_for_sync(workspace_path)?;

        for entry in entries {
            match entry.sync_kind {
                SyncKind::Upload | SyncKind::ConflictCandidate => {
                    plan.files_to_upload.push(entry.relative_path);
                }
                SyncKind::Ignore => {
                    plan.ignored_files.push(entry.relative_path);
                }
            }
        }

        Ok(plan)
    }

    pub fn load_sync_state(workspace_path: &Path) -> crate::Result<SyncState> {
        let state_path = workspace_path.join("app-meta/sync/sync_state.json");
        if !state_path.exists() {
            return Ok(SyncState::default());
        }

        let content = std::fs::read_to_string(state_path)?;
        let state: SyncState = serde_json::from_str(&content).unwrap_or_default();
        Ok(state)
    }

    pub fn save_sync_state(workspace_path: &Path, state: &SyncState) -> crate::Result<()> {
        let state_path = workspace_path.join("app-meta/sync/sync_state.json");
        if let Some(parent) = state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Ensure we don't save token or private keys! Wait, the SyncState model doesn't even have token/private key fields
        // which prevents accidental leakage natively.

        let content = serde_json::to_string_pretty(state).map_err(|e| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;

        // Atomic save wrapper or direct save since it's just sync state
        let tmp_path = state_path.with_extension("tmp");
        std::fs::write(&tmp_path, content)?;
        std::fs::rename(tmp_path, state_path)?;

        Ok(())
    }

    pub fn record_sync_conflict(
        workspace_path: &Path,
        conflict: SyncConflict,
        local_content: Option<&str>,
    ) -> crate::Result<()> {
        if let Some(content) = local_content {
            let conflict_file_path = workspace_path.join(format!(
                "{}.conflict.{}",
                conflict.local_path, conflict.created_at
            ));
            if let Some(parent) = conflict_file_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&conflict_file_path, content)?;
        }

        let conflicts_path = workspace_path.join("app-meta/sync/conflicts.json");
        if let Some(parent) = conflicts_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut conflicts: Vec<SyncConflict> = if conflicts_path.exists() {
            let content = std::fs::read_to_string(&conflicts_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        };

        conflicts.push(conflict);

        let content = serde_json::to_string_pretty(&conflicts).map_err(|e| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;

        let tmp_path = conflicts_path.with_extension("tmp");
        std::fs::write(&tmp_path, content)?;
        std::fs::rename(tmp_path, conflicts_path)?;

        Ok(())
    }

    pub fn get_sync_ignored_paths(workspace_path: &Path) -> crate::Result<Vec<String>> {
        let plan = Self::build_sync_plan_from_workspace(workspace_path)?;
        Ok(plan.ignored_files)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_whitelist_includes_sync_json() {
        assert!(SyncService::is_whitelisted_path(
            "app-meta/settings/settings.sync.json"
        ));
        assert!(!SyncService::is_whitelisted_path(
            "app-meta/settings/settings.local.json"
        ));
    }

    #[test]
    fn test_blacklist_ignores_local_json() {
        assert!(SyncService::is_blacklisted_path(
            "app-meta/settings/settings.local.json"
        ));
    }

    #[test]
    fn test_sync_secrets_blacklisted() {
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/sync_secrets.local.json"
        ));
    }

    #[test]
    fn test_sync_config_no_token() {
        let config = SyncConfig {
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
        };
        let content = serde_json::to_string(&config).unwrap();
        // token might be there if some other struct is serialized, but we want to ensure
        // the word "token" isn't a key. Actually since token was removed from SyncConfig, it should literally not be there.
        // Wait, why did it fail? Oh, HttpsToken transport is present! It serializes to "https_token".
        // Let's assert it doesn't contain `"token":`
        assert!(!content.contains("\"token\":"));
    }
    #[test]
    fn test_blacklist_ignores_tmp_and_lock_files() {
        assert!(SyncService::is_blacklisted_path(
            "projects/v1/chapters/c1.tmp"
        ));
        assert!(SyncService::is_blacklisted_path(
            "workspace_manifest.json.lock"
        ));
        assert!(SyncService::is_blacklisted_path("app-meta/logs/sync.log"));
    }

    #[test]
    fn test_record_sync_conflict_writes_correctly() {
        let dir = tempdir().unwrap();
        let conflict = SyncConflict {
            local_path: "chapter.md".to_string(),
            remote_path: "chapter.md".to_string(),
            local_hash: "aaa".to_string(),
            remote_hash: "bbb".to_string(),
            base_hash: "ccc".to_string(),
            created_at: 123456789,
            description: "conflict test".to_string(),
        };

        SyncService::record_sync_conflict(dir.path(), conflict, Some("my local conflict")).unwrap();
        let conflicts_path = dir.path().join("app-meta/sync/conflicts.json");
        assert!(conflicts_path.exists());
        let content = std::fs::read_to_string(conflicts_path).unwrap();
        assert!(content.contains("conflict test"));

        let file_conflict = dir.path().join("chapter.md.conflict.123456789");
        assert!(file_conflict.exists());
        let content2 = std::fs::read_to_string(file_conflict).unwrap();
        assert_eq!(content2, "my local conflict");
    }

    #[test]
    fn test_sync_state_does_not_leak_tokens() {
        let dir = tempdir().unwrap();
        let state = SyncState {
            remote_url: Some("https://example.com/repo.git".to_string()),
            transport: Some(SyncTransport::HttpsToken),
            last_synced_commit: None,
            last_sync_time: None,
            last_error: None,
            known_files: std::collections::HashMap::new(),
            conflicts: vec![],
        };

        SyncService::save_sync_state(dir.path(), &state).unwrap();
        let state_path = dir.path().join("app-meta/sync/sync_state.json");
        let state_content = std::fs::read_to_string(state_path).unwrap();

        assert!(state_content.contains("https://example.com/repo.git"));
        assert!(!state_content.contains("\"token\":"));
    }

    #[test]
    fn test_stage_blacklisted_files() {
        let dir = tempdir().unwrap();

        // Initialize git repo manually or use SyncService
        let repo = git2::Repository::init(dir.path()).unwrap();

        let file_path = dir.path().join("app-meta/sync/sync_secrets.local.json");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "secret_content").unwrap();

        let backend = Git2Backend;
        let paths = vec!["app-meta/sync/sync_secrets.local.json"];
        backend.stage_paths(dir.path(), &paths).unwrap();

        // Ensure it's not staged
        let index = repo.index().unwrap();
        assert!(index
            .get_path(
                std::path::Path::new("app-meta/sync/sync_secrets.local.json"),
                0
            )
            .is_none());
    }

    #[test]
    fn test_sync_dry_run_disabled_config() {
        let dir = tempdir().unwrap();
        let config = SyncConfig {
            enabled: false,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
        };

        let plan = SyncService::perform_sync_dry_run(dir.path(), &config).unwrap();
        assert!(plan.files_to_upload.is_empty());
        assert!(plan.ignored_files.is_empty());
    }

    #[test]
    fn test_sync_dry_run_enabled_config_scans() {
        let dir = tempdir().unwrap();
        let config = SyncConfig {
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
        };

        // Create some whitelisted and blacklisted files
        let settings_path = dir.path().join("app-meta/settings");
        std::fs::create_dir_all(&settings_path).unwrap();
        std::fs::write(settings_path.join("settings.sync.json"), "{}").unwrap();
        std::fs::write(settings_path.join("settings.local.json"), "{}").unwrap();

        let plan = SyncService::perform_sync_dry_run(dir.path(), &config).unwrap();
        assert!(plan
            .files_to_upload
            .contains(&"app-meta/settings/settings.sync.json".to_string()));
        assert!(plan
            .ignored_files
            .contains(&"app-meta/settings/settings.local.json".to_string()));
    }

    #[test]
    fn test_perform_sync_empty_remote_url() {
        let dir = tempdir().unwrap();
        let config = SyncConfig {
            enabled: true,
            remote_url: "".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
        };

        let secrets = SyncSecrets {
            token: Some("secret_token".to_string()),
            ssh_private_key: None,
        };

        // For this test we can use Git2Backend as it won't be called due to early return
        let backend = Git2Backend;
        let result = SyncService::perform_sync(dir.path(), &config, &secrets, &backend).unwrap();
        assert_eq!(
            result.status,
            SyncStatus::Error("Remote URL is empty".to_string())
        );
    }
}
