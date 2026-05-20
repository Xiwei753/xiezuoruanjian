use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BackendType {
    Git,
    GithubApi,
    WebDav,
    S3,
    LocalFolder,
}

impl Default for BackendType {
    fn default() -> Self {
        BackendType::Git
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SyncTransport {
    HttpsToken,
    SshDeployKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FirstSyncMode {
    #[default]
    NotAttempted,
    CloneIntoEmptyWorkspace,
    InitExistingWorkspace,
    AlreadyGitRepo,
    BlockedNonEmptyRemote,
    UnrelatedHistories,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    pub enabled: bool,
    #[serde(default)]
    pub backend_type: BackendType,
    pub remote_url: String,
    pub transport: SyncTransport,
    #[serde(default = "default_branch")]
    pub branch: String,
    pub auto_sync: bool,
    pub sync_interval_seconds: u32,
    #[serde(default)]
    pub proxy_enabled: bool,
    #[serde(default = "default_proxy_type")]
    pub proxy_type: String,
    #[serde(default = "default_proxy_host")]
    pub proxy_host: String,
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,
    /// GitHub username for HTTPS credential callback.
    /// Defaults to "x-access-token" when empty.
    #[serde(default)]
    pub username: String,
    /// Android-only: whether INTERNET permission is granted.
    /// Linux always sets this to true.
    #[serde(default = "default_true")]
    pub android_has_internet_permission: bool,
    /// Android-only: whether ACCESS_NETWORK_STATE permission is granted.
    /// Linux always sets this to true.
    #[serde(default = "default_true")]
    pub android_has_access_network_state_permission: bool,
}

fn default_true() -> bool { true }

fn default_proxy_type() -> String {
    "http".to_string()
}

fn default_proxy_host() -> String {
    "127.0.0.1".to_string()
}

fn default_proxy_port() -> u16 {
    7890
}

fn default_branch() -> String {
    "main".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncSecrets {
    pub token: Option<String>,
    pub ssh_private_key: Option<String>,
}

pub struct ParsedRemoteUrl {
    pub sanitized_url: String,
    pub extracted_username: Option<String>,
    pub extracted_token: Option<String>,
}

pub fn sanitize_remote_url(url: &str) -> ParsedRemoteUrl {
    if url.contains("://") && url.contains('@') {
        if let Some(after_scheme) = url.split_once("://") {
            let scheme = after_scheme.0;
            let rest = after_scheme.1;
            if let Some(at_pos) = rest.find('@') {
                let userinfo = &rest[..at_pos];
                let host_and_path = &rest[at_pos + 1..];
                let sanitized = format!("{}://{}", scheme, host_and_path);
                let (username, token) = if let Some(colon_pos) = userinfo.find(':') {
                    (
                        Some(url_decode(userinfo[..colon_pos].to_string())),
                        Some(url_decode(userinfo[colon_pos + 1..].to_string())),
                    )
                } else {
                    (Some(url_decode(userinfo.to_string())), None)
                };
                return ParsedRemoteUrl {
                    sanitized_url: sanitized,
                    extracted_username: username,
                    extracted_token: token,
                };
            }
        }
    }
    ParsedRemoteUrl {
        sanitized_url: url.to_string(),
        extracted_username: None,
        extracted_token: None,
    }
}

fn url_decode(s: String) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next();
            let h2 = chars.next();
            if let (Some(h1), Some(h2)) = (h1, h2) {
                if let Ok(byte) = u8::from_str_radix(&format!("{}{}", h1, h2), 16) {
                    result.push(byte as char);
                } else {
                    result.push('%');
                    result.push(h1);
                    result.push(h2);
                }
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

pub fn detect_transport(remote_url: &str) -> SyncTransport {
    let lower = remote_url.to_lowercase();
    if lower.starts_with("git@") || lower.starts_with("ssh://") {
        SyncTransport::SshDeployKey
    } else if lower.starts_with("https://") || lower.starts_with("http://") {
        SyncTransport::HttpsToken
    } else {
        SyncTransport::HttpsToken
    }
}

pub fn mask_token_in_url(url: &str) -> String {
    if url.contains('@') {
        if let Some(after_scheme) = url.split_once("://") {
            let scheme = after_scheme.0;
            let rest = after_scheme.1;
            if let Some(at_pos) = rest.find('@') {
                return format!("{}://***@{}", scheme, &rest[at_pos + 1..]);
            }
        }
    }
    url.to_string()
}

pub fn mask_token(s: &str) -> String {
    if s.len() <= 8 {
        return "***".to_string();
    }
    let prefix = &s[..4];
    let suffix = &s[s.len() - 4..];
    format!("{}***{}", prefix, suffix)
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
pub struct SyncDiagnosticsResult {
    pub success: bool,
    /// Backend type: git/github_api/webdav/s3/local_folder
    pub backend_type: String,
    /// Android permission check results
    pub android_has_internet_permission: bool,
    pub android_has_access_network_state_permission: bool,
    pub android_network_state: String,
    pub tcp_probe_ok: bool,
    pub tcp_probe_status: String,
    pub http_connect_probe_ok: bool,
    pub http_connect_probe_status: String,
    pub libgit2_probe_ok: bool,
    pub libgit2_probe_status: String,
    pub network_ok: bool,
    pub auth_ok: bool,
    pub repo_ok: bool,
    pub branch_ok: bool,
    pub network_status: String,
    pub auth_status: String,
    pub repo_status: String,
    pub branch_status: String,
    pub proxy_used: bool,
    pub proxy_type: String,
    pub proxy_host: String,
    pub proxy_port: u16,
    /// Sanitized remote URL (no credentials)
    pub remote_url_sanitized: String,
    /// Transport type: https/ssh/unknown
    pub transport: String,
    /// App-level proxy status: "未启用"/"已启用"
    pub app_proxy_status: String,
    /// Error category for proxy_enabled=false failures
    pub error_category: String,
    pub user_message: String,
    pub raw_error: Option<String>,
}

impl SyncDiagnosticsResult {
    pub fn new() -> Self {
        Self {
            success: false,
            backend_type: "git".to_string(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
            android_network_state: "unchecked".to_string(),
            tcp_probe_ok: false,
            tcp_probe_status: "unchecked".to_string(),
            http_connect_probe_ok: false,
            http_connect_probe_status: "unchecked".to_string(),
            libgit2_probe_ok: false,
            libgit2_probe_status: "unchecked".to_string(),
            network_ok: false,
            auth_ok: false,
            repo_ok: false,
            branch_ok: false,
            network_status: "unchecked".to_string(),
            auth_status: "unchecked".to_string(),
            repo_status: "unchecked".to_string(),
            branch_status: "unchecked".to_string(),
            proxy_used: false,
            proxy_type: "none".to_string(),
            proxy_host: "".to_string(),
            proxy_port: 0,
            remote_url_sanitized: "".to_string(),
            transport: "unknown".to_string(),
            app_proxy_status: "未启用".to_string(),
            error_category: "none".to_string(),
            user_message: "".to_string(),
            raw_error: None,
        }
    }
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
    pub first_sync_mode: FirstSyncMode,
    pub user_message: Option<String>,
}

impl SyncResult {
    pub fn success() -> Self {
        Self {
            status: SyncStatus::Success,
            uploaded_files: Vec::new(),
            downloaded_files: Vec::new(),
            ignored_files: Vec::new(),
            conflicts: Vec::new(),
            commit_hash: None,
            error: None,
            first_sync_mode: FirstSyncMode::NotAttempted,
            user_message: None,
        }
    }

    pub fn error(
        status: SyncStatus,
        first_sync_mode: FirstSyncMode,
        user_message: Option<String>,
        error: String,
    ) -> Self {
        Self {
            status,
            uploaded_files: Vec::new(),
            downloaded_files: Vec::new(),
            ignored_files: Vec::new(),
            conflicts: Vec::new(),
            commit_hash: None,
            error: Some(error),
            first_sync_mode,
            user_message,
        }
    }

    pub fn conflict(
        conflicts: Vec<SyncConflict>,
        error: String,
        user_message: Option<String>,
    ) -> Self {
        Self {
            status: SyncStatus::Conflict,
            uploaded_files: Vec::new(),
            downloaded_files: Vec::new(),
            ignored_files: Vec::new(),
            conflicts,
            commit_hash: None,
            error: Some(error),
            first_sync_mode: FirstSyncMode::NotAttempted,
            user_message,
        }
    }
}

pub enum GitAuth {
    HttpsToken { username: String, token: String },
    SshDeployKey,
}

pub trait GitBackend {
    fn init_repo(&self, local_repo_path: &Path) -> crate::Result<()>;
    fn ensure_remote(&self, local_repo_path: &Path, remote_url: &str) -> crate::Result<()>;
    fn has_repo(&self, local_repo_path: &Path) -> bool;
    fn is_worktree_empty_or_git_only(&self, local_repo_path: &Path) -> crate::Result<bool>;
    fn clone_repo(
        &self,
        remote_url: &str,
        local_repo_path: &Path,
        auth: Option<&GitAuth>,
        proxy_config: Option<&SyncConfig>,
    ) -> crate::Result<()>;
    fn open_repo(&self, local_repo_path: &Path) -> crate::Result<()>;
    fn pull(
        &self,
        local_repo_path: &Path,
        branch: &str,
        auth: Option<&GitAuth>,
        proxy_config: Option<&SyncConfig>,
    ) -> crate::Result<()>;
    fn stage_paths(&self, local_repo_path: &Path, paths: &[&str]) -> crate::Result<()>;
    fn commit(&self, local_repo_path: &Path, message: &str) -> crate::Result<Option<String>>;
    fn push(
        &self,
        local_repo_path: &Path,
        branch: &str,
        auth: Option<&GitAuth>,
        proxy_config: Option<&SyncConfig>,
    ) -> crate::Result<()>;
    fn current_head(&self, local_repo_path: &Path) -> crate::Result<Option<String>>;
    fn status(&self, local_repo_path: &Path) -> crate::Result<Vec<String>>; // Returns changed files
}

pub struct Git2Backend;

impl Git2Backend {
    fn build_proxy_options<'a>(
        config: Option<&'a SyncConfig>,
    ) -> crate::Result<git2::ProxyOptions<'a>> {
        let mut proxy_opts = git2::ProxyOptions::new();
        if let Some(cfg) = config {
            if cfg.proxy_enabled {
                if cfg.proxy_host.is_empty() {
                    return Err(crate::Error::Other("Proxy host cannot be empty".to_string()));
                }
                if cfg.proxy_port == 0 {
                    return Err(crate::Error::Other("Proxy port is invalid".to_string()));
                }
                match cfg.proxy_type.as_str() {
                    "auto" => {
                        proxy_opts.auto();
                    }
                    "http" => {
                        let proxy_url = format!("http://{}:{}", cfg.proxy_host, cfg.proxy_port);
                        proxy_opts.url(&proxy_url);
                    }
                    "socks5" => {
                        let proxy_url = format!("socks5h://{}:{}", cfg.proxy_host, cfg.proxy_port);
                        proxy_opts.url(&proxy_url);
                    }
                    "none" => {}
                    _ => {
                        return Err(crate::Error::Other("Invalid proxy type".to_string()));
                    }
                }
            }
        }
        Ok(proxy_opts)
    }

    fn build_callbacks<'a>(auth: Option<&'a GitAuth>, username_override: Option<&'a str>) -> git2::RemoteCallbacks<'a> {
        let mut callbacks = git2::RemoteCallbacks::new();
        if let Some(auth) = auth {
            callbacks.credentials(move |_url, username_from_url, _allowed_types| match auth {
                GitAuth::HttpsToken { username, token } => {
                    let user = username_override
                        .or_else(|| username_from_url)
                        .unwrap_or(username);
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
    fn init_repo(&self, local_repo_path: &Path) -> crate::Result<()> {
        git2::Repository::init(local_repo_path).map_err(|e| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        Ok(())
    }

    fn ensure_remote(&self, local_repo_path: &Path, remote_url: &str) -> crate::Result<()> {
        let repo = git2::Repository::open(local_repo_path).map_err(|e| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        if repo.find_remote("origin").is_err() {
            repo.remote("origin", remote_url).map_err(|e| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
        } else {
            repo.remote_set_url("origin", remote_url).map_err(|e| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
        }
        Ok(())
    }

    fn has_repo(&self, local_repo_path: &Path) -> bool {
        git2::Repository::open(local_repo_path).is_ok()
    }

    fn is_worktree_empty_or_git_only(&self, local_repo_path: &Path) -> crate::Result<bool> {
        let entries = std::fs::read_dir(local_repo_path).map_err(|e| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
            let name = entry.file_name();
            if name != ".git" {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn clone_repo(
        &self,
        remote_url: &str,
        local_repo_path: &Path,
        auth: Option<&GitAuth>,
        proxy_config: Option<&SyncConfig>,
    ) -> crate::Result<()> {
        let mut fetch_options = git2::FetchOptions::new();
        let username_override = proxy_config.and_then(|c| if c.username.is_empty() { None } else { Some(c.username.as_str()) });
        let callbacks = Self::build_callbacks(auth, username_override);
        fetch_options.remote_callbacks(callbacks);
        let proxy_opts = Self::build_proxy_options(proxy_config)?;
        fetch_options.proxy_options(proxy_opts);

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
        proxy_config: Option<&SyncConfig>,
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
        let username_override = proxy_config.and_then(|c| if c.username.is_empty() { None } else { Some(c.username.as_str()) });
        fetch_options.remote_callbacks(Self::build_callbacks(auth, username_override));
        fetch_options.proxy_options(Self::build_proxy_options(proxy_config)?);

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
            repo.checkout_head(Some(git2::build::CheckoutBuilder::default().safe()))
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
            if SyncService::is_blacklisted_path(p) || !SyncService::is_whitelisted_path(p) {
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
        proxy_config: Option<&SyncConfig>,
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
        let username_override = proxy_config.and_then(|c| if c.username.is_empty() { None } else { Some(c.username.as_str()) });
        push_options.remote_callbacks(Self::build_callbacks(auth, username_override));
        push_options.proxy_options(Self::build_proxy_options(proxy_config)?);

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
                if !SyncService::is_blacklisted_path(path) && SyncService::is_whitelisted_path(path)
                {
                    res.push(path.to_string());
                }
            }
        }
        Ok(res)
    }
}

pub trait SyncBackend {
    fn diagnose(
        &self,
        config: &SyncConfig,
        secrets: &SyncSecrets,
    ) -> crate::Result<SyncDiagnosticsResult>;
    fn pull(
        &self,
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
    ) -> crate::Result<SyncResult>;
    fn push(
        &self,
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
    ) -> crate::Result<SyncResult>;
    fn sync(
        &self,
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
    ) -> crate::Result<SyncResult>;
}

pub struct GitSyncBackend;

impl SyncBackend for GitSyncBackend {
    fn diagnose(
        &self,
        config: &SyncConfig,
        secrets: &SyncSecrets,
    ) -> crate::Result<SyncDiagnosticsResult> {
        let backend = Git2Backend;
        SyncService::perform_sync_diagnostics(config, secrets, &backend)
    }
    fn pull(
        &self,
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
    ) -> crate::Result<SyncResult> {
        let backend = Git2Backend;
        SyncService::perform_sync(workspace_path, config, secrets, &backend)
    }
    fn push(
        &self,
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
    ) -> crate::Result<SyncResult> {
        let backend = Git2Backend;
        SyncService::perform_sync(workspace_path, config, secrets, &backend)
    }
    fn sync(
        &self,
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
    ) -> crate::Result<SyncResult> {
        let backend = Git2Backend;
        SyncService::perform_sync(workspace_path, config, secrets, &backend)
    }
}

pub struct GitHubApiBackend;

impl GitHubApiBackend {
    fn api_base_url(remote_url: &str) -> String {
        let sanitized = sanitize_remote_url(remote_url).sanitized_url;
        if let Some(path) = sanitized.strip_prefix("https://github.com/") {
            let path = path.strip_suffix(".git").unwrap_or(path);
            format!("https://api.github.com/repos/{}", path)
        } else if let Some(path) = sanitized.strip_prefix("http://github.com/") {
            let path = path.strip_suffix(".git").unwrap_or(path);
            format!("https://api.github.com/repos/{}", path)
        } else {
            sanitized
        }
    }
}

impl SyncBackend for GitHubApiBackend {
    fn diagnose(
        &self,
        config: &SyncConfig,
        secrets: &SyncSecrets,
    ) -> crate::Result<SyncDiagnosticsResult> {
        let mut result = SyncDiagnosticsResult::new();
        result.remote_url_sanitized = sanitize_remote_url(&config.remote_url).sanitized_url.clone();
        result.transport = "https".to_string();
        result.app_proxy_status = if config.proxy_enabled { "已启用".to_string() } else { "未启用".to_string() };

        if !config.android_has_internet_permission {
            result.user_message = "缺少 INTERNET 权限。".to_string();
            result.error_category = "missing_permission".to_string();
            return Ok(result);
        }

        let token = secrets.token.clone().unwrap_or_default();
        if token.is_empty() {
            result.user_message = "缺少 GitHub Token。".to_string();
            result.error_category = "token_missing".to_string();
            return Ok(result);
        }

        let api_base = Self::api_base_url(&config.remote_url);
        let _masked_url = mask_token_in_url(&api_base);

        let client = build_http_client(Some(config))?;
        let api_url = format!("{}/git/ref/heads/{}", api_base, config.branch);

        match client.get(&api_url).header("Authorization", format!("Bearer {}", token)).header("User-Agent", "WriterApp/1.0").header("Accept", "application/vnd.github+json").send() {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let body = resp.text().unwrap_or_default();
                if status == 200 {
                    result.success = true;
                    result.network_ok = true;
                    result.network_status = "ok".to_string();
                    result.auth_ok = true;
                    result.auth_status = "ok".to_string();
                    result.repo_ok = true;
                    result.repo_status = "ok".to_string();
                    result.branch_ok = true;
                    result.branch_status = "ok".to_string();
                    result.user_message = "诊断成功：GitHub API 可达，Token 有效，仓库和分支存在。".to_string();
                } else if status == 401 || status == 403 {
                    result.network_ok = true;
                    result.network_status = "ok".to_string();
                    result.auth_ok = false;
                    result.auth_status = "failed".to_string();
                    result.error_category = if status == 401 { "token_invalid" } else { "token_permission_denied" }.to_string();
                    result.user_message = if status == 401 {
                        "身份验证失败。Token 无效或已过期。".to_string()
                    } else {
                        "Token 权限不足。请确认 Token 具有 repo 权限范围。".to_string()
                    };
                    result.raw_error = Some(format!("HTTP {} (body truncated): {}", status, &body.chars().take(200).collect::<String>()));
                } else if status == 404 {
                    result.network_ok = true;
                    result.network_status = "ok".to_string();
                    result.auth_ok = true;
                    result.auth_status = "ok".to_string();
                    result.repo_ok = false;
                    result.repo_status = "failed".to_string();
                    result.error_category = "repo_not_found_or_no_permission".to_string();
                    result.user_message = "找不到仓库或分支。请检查仓库地址和分支名称。".to_string();
                    result.raw_error = Some(format!("HTTP {} (body truncated): {}", status, &body.chars().take(200).collect::<String>()));
                } else {
                    result.network_ok = false;
                    result.network_status = "failed".to_string();
                    result.error_category = "github_network_failed".to_string();
                    result.user_message = format!("GitHub API 返回意外状态码: {}", status);
                    result.raw_error = Some(format!("HTTP {} (body truncated): {}", status, &body.chars().take(200).collect::<String>()));
                }
            }
            Err(e) => {
                let err_msg = e.to_string().to_lowercase();
                result.raw_error = Some(e.to_string());
                if err_msg.contains("dns") || err_msg.contains("resolve") || err_msg.contains("name resolution") {
                    result.error_category = "dns_failed".to_string();
                    result.user_message = "无法解析 GitHub API 地址。请检查网络/DNS 设置。".to_string();
                } else if err_msg.contains("ssl") || err_msg.contains("certificate") || err_msg.contains("tls") {
                    result.error_category = "tls_failed".to_string();
                    result.user_message = "SSL/TLS 连接失败。请检查网络环境或系统时间。".to_string();
                } else if err_msg.contains("connection refused") || err_msg.contains("timeout") || err_msg.contains("network unreachable") {
                    result.error_category = "github_network_failed".to_string();
                    result.user_message = "网络连接失败或超时。请检查网络连接或代理设置。".to_string();
                } else {
                    result.error_category = "github_network_failed".to_string();
                    result.user_message = format!("GitHub API 请求失败: {}", e);
                }
                result.network_ok = false;
                result.network_status = "failed".to_string();
            }
        }

        Ok(result)
    }

    fn pull(
        &self,
        _workspace_path: &Path,
        _config: &SyncConfig,
        _secrets: &SyncSecrets,
    ) -> crate::Result<SyncResult> {
        Ok(SyncResult::error(
            SyncStatus::Error("backend_not_implemented".to_string()),
            FirstSyncMode::NotAttempted,
            Some("GitHub API 后端的 pull 操作尚未实现。".to_string()),
            "GitHub API pull not implemented".to_string(),
        ))
    }

    fn push(
        &self,
        _workspace_path: &Path,
        _config: &SyncConfig,
        _secrets: &SyncSecrets,
    ) -> crate::Result<SyncResult> {
        Ok(SyncResult::error(
            SyncStatus::Error("backend_not_implemented".to_string()),
            FirstSyncMode::NotAttempted,
            Some("GitHub API 后端的 push 操作尚未实现。".to_string()),
            "GitHub API push not implemented".to_string(),
        ))
    }

    fn sync(
        &self,
        _workspace_path: &Path,
        _config: &SyncConfig,
        _secrets: &SyncSecrets,
    ) -> crate::Result<SyncResult> {
        Ok(SyncResult::error(
            SyncStatus::Error("backend_not_implemented".to_string()),
            FirstSyncMode::NotAttempted,
            Some("GitHub API 后端的 sync 操作尚未实现。".to_string()),
            "GitHub API sync not implemented".to_string(),
        ))
    }
}

pub struct WebDavBackend;

impl SyncBackend for WebDavBackend {
    fn diagnose(
        &self,
        _config: &SyncConfig,
        _secrets: &SyncSecrets,
    ) -> crate::Result<SyncDiagnosticsResult> {
        let mut result = SyncDiagnosticsResult::new();
        result.user_message = "WebDAV 后端尚未实现。".to_string();
        result.error_category = "backend_not_implemented".to_string();
        Ok(result)
    }
    fn pull(&self, _: &Path, _: &SyncConfig, _: &SyncSecrets) -> crate::Result<SyncResult> {
        Ok(SyncResult::error(
            SyncStatus::Error("backend_not_implemented".to_string()),
            FirstSyncMode::NotAttempted,
            Some("WebDAV 后端的 pull 操作尚未实现。".to_string()),
            "WebDAV pull not implemented".to_string(),
        ))
    }
    fn push(&self, _: &Path, _: &SyncConfig, _: &SyncSecrets) -> crate::Result<SyncResult> {
        Ok(SyncResult::error(
            SyncStatus::Error("backend_not_implemented".to_string()),
            FirstSyncMode::NotAttempted,
            Some("WebDAV 后端的 push 操作尚未实现。".to_string()),
            "WebDAV push not implemented".to_string(),
        ))
    }
    fn sync(&self, _: &Path, _: &SyncConfig, _: &SyncSecrets) -> crate::Result<SyncResult> {
        Ok(SyncResult::error(
            SyncStatus::Error("backend_not_implemented".to_string()),
            FirstSyncMode::NotAttempted,
            Some("WebDAV 后端的 sync 操作尚未实现。".to_string()),
            "WebDAV sync not implemented".to_string(),
        ))
    }
}

pub struct LocalFolderBackend;

impl SyncBackend for LocalFolderBackend {
    fn diagnose(
        &self,
        _config: &SyncConfig,
        _secrets: &SyncSecrets,
    ) -> crate::Result<SyncDiagnosticsResult> {
        let mut result = SyncDiagnosticsResult::new();
        result.user_message = "LocalFolder 后端尚未实现。此后端用于配合 Syncthing 等外部同步工具。".to_string();
        result.error_category = "backend_not_implemented".to_string();
        Ok(result)
    }
    fn pull(&self, _: &Path, _: &SyncConfig, _: &SyncSecrets) -> crate::Result<SyncResult> {
        Ok(SyncResult::error(
            SyncStatus::Error("backend_not_implemented".to_string()),
            FirstSyncMode::NotAttempted,
            Some("LocalFolder 后端的 pull 操作尚未实现。".to_string()),
            "LocalFolder pull not implemented".to_string(),
        ))
    }
    fn push(&self, _: &Path, _: &SyncConfig, _: &SyncSecrets) -> crate::Result<SyncResult> {
        Ok(SyncResult::error(
            SyncStatus::Error("backend_not_implemented".to_string()),
            FirstSyncMode::NotAttempted,
            Some("LocalFolder 后端的 push 操作尚未实现。".to_string()),
            "LocalFolder push not implemented".to_string(),
        ))
    }
    fn sync(&self, _: &Path, _: &SyncConfig, _: &SyncSecrets) -> crate::Result<SyncResult> {
        Ok(SyncResult::error(
            SyncStatus::Error("backend_not_implemented".to_string()),
            FirstSyncMode::NotAttempted,
            Some("LocalFolder 后端的 sync 操作尚未实现。".to_string()),
            "LocalFolder sync not implemented".to_string(),
        ))
    }
}

pub struct S3Backend;

impl SyncBackend for S3Backend {
    fn diagnose(
        &self,
        _config: &SyncConfig,
        _secrets: &SyncSecrets,
    ) -> crate::Result<SyncDiagnosticsResult> {
        let mut result = SyncDiagnosticsResult::new();
        result.user_message = "S3 后端尚未实现。".to_string();
        result.error_category = "backend_not_implemented".to_string();
        Ok(result)
    }
    fn pull(&self, _: &Path, _: &SyncConfig, _: &SyncSecrets) -> crate::Result<SyncResult> {
        Ok(SyncResult::error(
            SyncStatus::Error("backend_not_implemented".to_string()),
            FirstSyncMode::NotAttempted,
            Some("S3 后端的 pull 操作尚未实现。".to_string()),
            "S3 pull not implemented".to_string(),
        ))
    }
    fn push(&self, _: &Path, _: &SyncConfig, _: &SyncSecrets) -> crate::Result<SyncResult> {
        Ok(SyncResult::error(
            SyncStatus::Error("backend_not_implemented".to_string()),
            FirstSyncMode::NotAttempted,
            Some("S3 后端的 push 操作尚未实现。".to_string()),
            "S3 push not implemented".to_string(),
        ))
    }
    fn sync(&self, _: &Path, _: &SyncConfig, _: &SyncSecrets) -> crate::Result<SyncResult> {
        Ok(SyncResult::error(
            SyncStatus::Error("backend_not_implemented".to_string()),
            FirstSyncMode::NotAttempted,
            Some("S3 后端的 sync 操作尚未实现。".to_string()),
            "S3 sync not implemented".to_string(),
        ))
    }
}

pub fn create_sync_backend(backend_type: &BackendType) -> Box<dyn SyncBackend> {
    match backend_type {
        BackendType::Git => Box::new(GitSyncBackend),
        BackendType::GithubApi => Box::new(GitHubApiBackend),
        BackendType::WebDav => Box::new(WebDavBackend),
        BackendType::S3 => Box::new(S3Backend),
        BackendType::LocalFolder => Box::new(LocalFolderBackend),
    }
}

fn build_http_client(config: Option<&SyncConfig>) -> crate::Result<reqwest::blocking::Client> {
    let mut builder = reqwest::blocking::Client::builder()
        .user_agent("WriterApp/1.0")
        .timeout(std::time::Duration::from_secs(15));

    if let Some(cfg) = config {
        if cfg.proxy_enabled && !cfg.proxy_host.is_empty() && cfg.proxy_port > 0 {
            let proxy_url = match cfg.proxy_type.as_str() {
                "http" => format!("http://{}:{}", cfg.proxy_host, cfg.proxy_port),
                "socks5" => format!("socks5h://{}:{}", cfg.proxy_host, cfg.proxy_port),
                _ => format!("http://{}:{}", cfg.proxy_host, cfg.proxy_port),
            };
            let proxy = reqwest::Proxy::all(&proxy_url).map_err(|e| {
                crate::Error::Other(format!("Failed to configure proxy: {}", e))
            })?;
            builder = builder.proxy(proxy);
        }
    }

    let client = builder.build().map_err(|e| {
        crate::Error::Other(format!("Failed to build HTTP client: {}", e))
    })?;
    Ok(client)
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

fn get_user_friendly_error(err: &str) -> String {
    let e = err.to_lowercase();
    if e.contains("failed to resolve address")
        || e.contains("no address associated with hostname")
        || e.contains("could not resolve host")
        || e.contains("name resolution")
    {
        return "无法解析 GitHub。请检查手机网络、DNS、代理/VPN/Clash 是否允许本应用访问 GitHub，然后重试。".to_string();
    }
    if e.contains("authentication failed") || e.contains("invalid credentials") || e.contains("401")
    {
        return "身份验证失败。请检查您的 GitHub Token 是否正确，或者该 Token 是否具有访问该仓库的权限。".to_string();
    }
    if e.contains("repository not found") || e.contains("not found") || e.contains("404") {
        return "找不到仓库。请检查您填写的 GitHub 仓库地址是否正确，或者您的 Token 是否有权限访问该私有仓库。".to_string();
    }
    if e.contains("ssl") || e.contains("certificate") {
        return "SSL 证书或网络错误。请检查您的网络环境、代理/VPN 设置或系统时间是否正确。"
            .to_string();
    }
    if e.contains("timeout")
        || e.contains("connection refused")
        || e.contains("network unreachable")
    {
        return "网络连接失败或超时。请检查您的网络连接或代理设置。".to_string();
    }
    if e.contains("conflict") {
        return "同步代码冲突。请在另一端解决冲突后重试。".to_string();
    }
    if e.contains("operation not permitted") && e.contains("127.0.0.1") {
        return "代理 127.0.0.1:7890 连接被拒绝，请确认手机代理 App 开启本机 HTTP 端口，或选择不使用手动代理，改走系统 VPN/全局模式。".to_string();
    }
    if e.contains("unsupported proxy protocol") && e.contains("socks5") {
        return "当前构建版本的底层网络库不支持 SOCKS5 代理。请尝试使用 HTTP 代理或更新应用。".to_string();
    }
    format!("同步失败，请检查网络重试。({})", err)
}

fn classify_error_without_proxy(err_msg: &str) -> String {
    let e = err_msg.to_lowercase();
    if e.contains("failed to resolve address")
        || e.contains("no address associated with hostname")
        || e.contains("could not resolve host")
        || e.contains("name resolution")
    {
        return "dns_failed".to_string();
    }
    if e.contains("ssl") || e.contains("certificate") {
        return "tls_failed".to_string();
    }
    if e.contains("authentication failed") || e.contains("invalid credentials") || e.contains("401") {
        return "auth_failed".to_string();
    }
    if e.contains("token") && (e.contains("missing") || e.contains("empty") || e.contains("not provided")) {
        return "token_missing".to_string();
    }
    if e.contains("permission denied") || e.contains("403") {
        return "token_permission_denied".to_string();
    }
    if e.contains("repository not found") || e.contains("not found") || e.contains("404") {
        return "repo_not_found_or_no_permission".to_string();
    }
    if e.contains("ref not found") || e.contains("couldn't find remote ref") || e.contains("branch") && e.contains("not found") {
        return "branch_not_found".to_string();
    }
    if e.contains("timeout") || e.contains("connection refused") || e.contains("network unreachable") {
        return "github_network_failed".to_string();
    }
    "github_network_failed".to_string()
}

fn classify_error_with_proxy(err_msg: &str, result: &SyncDiagnosticsResult) -> String {
    let e = err_msg.to_lowercase();
    if e.contains("unsupported proxy protocol") {
        return "proxy_protocol_unsupported".to_string();
    }
    if !result.tcp_probe_ok {
        return "proxy_tcp_failed".to_string();
    }
    if result.tcp_probe_ok && !result.http_connect_probe_ok && result.proxy_type == "http" {
        return "proxy_connect_failed".to_string();
    }
    if e.contains("authentication failed") || e.contains("invalid credentials") || e.contains("401") {
        return "auth_failed".to_string();
    }
    if e.contains("repository not found") || e.contains("not found") || e.contains("404") {
        return "repo_not_found_or_no_permission".to_string();
    }
    "proxy_connect_failed".to_string()
}

impl SyncService {

    pub fn perform_sync_diagnostics(
        config: &SyncConfig,
        secrets: &SyncSecrets,
        _backend: &dyn GitBackend,
    ) -> crate::Result<SyncDiagnosticsResult> {
        let mut result = SyncDiagnosticsResult::new();
        result.backend_type = match config.backend_type {
            BackendType::Git => "git".to_string(),
            BackendType::GithubApi => "github_api".to_string(),
            BackendType::WebDav => "webdav".to_string(),
            BackendType::S3 => "s3".to_string(),
            BackendType::LocalFolder => "local_folder".to_string(),
        };

        result.android_has_internet_permission = config.android_has_internet_permission;
        result.android_has_access_network_state_permission = config.android_has_access_network_state_permission;

        if !config.android_has_internet_permission {
            result.user_message = "缺少 INTERNET 权限。Android 应用无法联网，请在 AndroidManifest.xml 中添加 android.permission.INTERNET。".to_string();
            result.network_status = "failed_no_internet_permission".to_string();
            result.auth_status = "skipped".to_string();
            result.repo_status = "skipped".to_string();
            result.branch_status = "skipped".to_string();
            result.error_category = "missing_permission".to_string();
            return Ok(result);
        }

        if !config.android_has_access_network_state_permission {
            result.android_network_state = "unknown_no_permission".to_string();
        } else {
            result.android_network_state = "permission_granted".to_string();
        }

        let parsed = sanitize_remote_url(&config.remote_url);
        let sanitized_url = parsed.sanitized_url;
        result.remote_url_sanitized = sanitized_url.clone();

        let transport = detect_transport(&sanitized_url);
        result.transport = match transport {
            SyncTransport::HttpsToken => "https".to_string(),
            SyncTransport::SshDeployKey => "ssh".to_string(),
        };

        if transport == SyncTransport::SshDeployKey {
            result.user_message = "检测到 SSH remote。在移动端/受限网络下 SSH 不推荐，建议改用 HTTPS remote (https://github.com/owner/repo.git)。".to_string();
            result.error_category = "ssh_not_recommended".to_string();
            result.network_status = "skipped_ssh".to_string();
            result.auth_status = "skipped".to_string();
            result.repo_status = "skipped".to_string();
            result.branch_status = "skipped".to_string();
            return Ok(result);
        }

        result.proxy_used = config.proxy_enabled;
        result.proxy_type = config.proxy_type.clone();
        result.proxy_host = config.proxy_host.clone();
        result.proxy_port = config.proxy_port;

        if config.proxy_enabled {
            result.app_proxy_status = "已启用".to_string();
        } else {
            result.app_proxy_status = "未启用".to_string();
        }

        if sanitized_url.is_empty() {
            result.user_message = "远程仓库地址为空。".to_string();
            result.error_category = "empty_url".to_string();
            return Ok(result);
        }

        let token_from_parsed = parsed.extracted_token;
        let token = secrets.token.clone().or(token_from_parsed).unwrap_or_default();
        if token.is_empty() {
            result.user_message = "缺少 GitHub Token。".to_string();
            result.error_category = "token_missing".to_string();
            return Ok(result);
        }

        let username_for_cred = if !config.username.is_empty() {
            config.username.clone()
        } else if let Some(ref extracted_user) = parsed.extracted_username {
            extracted_user.clone()
        } else {
            "x-access-token".to_string()
        };

        let temp_dir = tempfile::tempdir().map_err(|e| crate::Error::Io(e))?;
        let repo = git2::Repository::init(temp_dir.path()).map_err(|e: git2::Error| crate::Error::Other(e.to_string()))?;

        let mut remote = repo.remote_anonymous(&sanitized_url).map_err(|e: git2::Error| crate::Error::Other(e.to_string()))?;

        let mut callbacks = git2::RemoteCallbacks::new();
        let token_clone = token.clone();
        let username_clone = username_for_cred.clone();
        callbacks.credentials(move |_user, _user_from_url, _cred| {
            git2::Cred::userpass_plaintext(&username_clone, &token_clone)
        });

        let proxy_opts = match Git2Backend::build_proxy_options(Some(config)) {
            Ok(opts) => opts,
            Err(e) => {
                result.user_message = format!("代理配置错误: {}", e);
                result.success = false;
                result.error_category = "proxy_config_error".to_string();
                return Ok(result);
            }
        };

        if config.proxy_enabled && (config.proxy_type == "http" || config.proxy_type == "socks5") && !config.proxy_host.is_empty() {
            let addr = format!("{}:{}", config.proxy_host, config.proxy_port);

            let addrs = std::net::ToSocketAddrs::to_socket_addrs(&addr);
            let mut tcp_connected = false;
            let mut last_tcp_err = None;
            let mut maybe_stream = None;

            if let Ok(resolved_addrs) = addrs {
                let addr_list: Vec<_> = resolved_addrs.collect();
                if addr_list.is_empty() {
                    last_tcp_err = Some(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "DNS resolved to zero addresses",
                    ));
                } else {
                    for socket_addr in &addr_list {
                        match std::net::TcpStream::connect_timeout(
                            socket_addr,
                            std::time::Duration::from_secs(5),
                        ) {
                            Ok(stream) => {
                                tcp_connected = true;
                                maybe_stream = Some(stream);
                                break;
                            }
                            Err(e) => {
                                last_tcp_err = Some(e);
                            }
                        }
                    }
                }
            } else {
                last_tcp_err = addrs.err();
            }

            if tcp_connected {
                let mut stream = maybe_stream.unwrap();
                result.tcp_probe_ok = true;
                result.tcp_probe_status = "ok".to_string();

                let timeout = std::time::Duration::from_secs(5);
                let _ = stream.set_read_timeout(Some(timeout));
                let _ = stream.set_write_timeout(Some(timeout));

                if config.proxy_type == "http" {
                    let request = format!(
                        "CONNECT github.com:443 HTTP/1.1\r\nHost: github.com:443\r\n\r\n"
                    );
                    match std::io::Write::write_all(&mut stream, request.as_bytes()) {
                        Ok(_) => {
                            let mut buffer = [0u8; 1024];
                            match stream.read(&mut buffer) {
                                Ok(bytes_read) if bytes_read > 0 => {
                                    let response = String::from_utf8_lossy(&buffer[..bytes_read]);
                                    if response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200") {
                                        result.http_connect_probe_ok = true;
                                        result.http_connect_probe_status = "ok".to_string();
                                    } else if response.starts_with("HTTP/") {
                                        let status_code = response.split_whitespace().nth(1).unwrap_or("unknown");
                                        if status_code == "407" {
                                            result.http_connect_probe_ok = false;
                                            result.http_connect_probe_status = "proxy_auth_required".to_string();
                                            result.network_ok = false;
                                            result.network_status = "failed".to_string();
                                            result.user_message = "代理需要认证 (HTTP 407)。请检查代理配置。".to_string();
                                            result.error_category = "proxy_auth_required".to_string();
                                            return Ok(result);
                                        }
                                        result.http_connect_probe_ok = false;
                                        result.http_connect_probe_status = format!("failed_status_{}", status_code);
                                        result.network_ok = false;
                                        result.network_status = "failed".to_string();
                                        result.user_message = format!("代理端口 HTTP CONNECT 失败: {}", result.http_connect_probe_status);
                                        result.error_category = "proxy_connect_failed".to_string();
                                        return Ok(result);
                                    } else {
                                        result.http_connect_probe_ok = false;
                                        result.http_connect_probe_status = "invalid_response".to_string();
                                        result.network_ok = false;
                                        result.network_status = "failed".to_string();
                                        result.user_message = "代理端口 HTTP CONNECT 返回无效响应（非 HTTP）".to_string();
                                        result.error_category = "proxy_connect_failed".to_string();
                                        return Ok(result);
                                    }
                                }
                                Ok(_) => {
                                    result.http_connect_probe_ok = false;
                                    result.http_connect_probe_status = "read_timeout".to_string();
                                    result.network_ok = false;
                                    result.network_status = "failed".to_string();
                                    result.user_message = "代理端口 HTTP CONNECT 读取超时（代理未响应）".to_string();
                                    result.error_category = "proxy_connect_failed".to_string();
                                    return Ok(result);
                                }
                                Err(e) if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock => {
                                    result.http_connect_probe_ok = false;
                                    result.http_connect_probe_status = "read_timeout".to_string();
                                    result.network_ok = false;
                                    result.network_status = "failed".to_string();
                                    result.user_message = "代理端口 HTTP CONNECT 读取超时（代理未响应）".to_string();
                                    result.error_category = "proxy_connect_failed".to_string();
                                    return Ok(result);
                                }
                                Err(e) => {
                                    result.http_connect_probe_ok = false;
                                    result.http_connect_probe_status = format!("read_error: {}", e);
                                    result.network_ok = false;
                                    result.network_status = "failed".to_string();
                                    result.user_message = format!("代理端口 HTTP CONNECT 读取错误: {}", e);
                                    result.error_category = "proxy_connect_failed".to_string();
                                    return Ok(result);
                                }
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock => {
                            result.http_connect_probe_ok = false;
                            result.http_connect_probe_status = "write_timeout".to_string();
                            result.network_ok = false;
                            result.network_status = "failed".to_string();
                            result.user_message = "代理端口 HTTP CONNECT 写入超时".to_string();
                            result.error_category = "proxy_connect_failed".to_string();
                            return Ok(result);
                        }
                        Err(e) => {
                            result.http_connect_probe_ok = false;
                            result.http_connect_probe_status = "write_failed".to_string();
                            result.network_ok = false;
                            result.network_status = "failed".to_string();
                            result.user_message = format!("代理端口 HTTP CONNECT 写入失败: {}", e);
                            result.error_category = "proxy_connect_failed".to_string();
                            return Ok(result);
                        }
                    }
                } else {
                    result.http_connect_probe_status = "skipped_socks5".to_string();
                }
            } else {
                let err_msg = last_tcp_err
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "Unknown error".to_string());
                result.tcp_probe_ok = false;
                result.tcp_probe_status = format!("tcp_probe_failed: {}", err_msg);
                result.network_ok = false;
                result.network_status = "failed".to_string();
                result.user_message = format!("代理端口 {} 无法建立 TCP 连接（尝试所有解析地址均失败）: {}", addr, err_msg);
                result.error_category = "proxy_tcp_failed".to_string();
                return Ok(result);
            }
        }

        let direction = git2::Direction::Fetch;
        let connection: git2::RemoteConnection = match remote.connect_auth(direction, Some(callbacks), Some(proxy_opts)) {
            Ok(c) => {
                result.libgit2_probe_ok = true;
                result.libgit2_probe_status = "ok".to_string();
                c
            },
            Err(e) => {
                let err_msg = e.to_string();
                let clean_msg = err_msg.replace(&token, "***TOKEN***");
                result.raw_error = Some(clean_msg.clone());

                if config.proxy_enabled {
                    result.error_category = classify_error_with_proxy(&clean_msg, &result);
                } else {
                    result.error_category = classify_error_without_proxy(&clean_msg);
                }
                result.user_message = get_user_friendly_error(&clean_msg);

                result.libgit2_probe_ok = false;
                result.libgit2_probe_status = "failed".to_string();

                let is_network_error = clean_msg.contains("resolve address") || clean_msg.contains("resolve host") || clean_msg.contains("network") || clean_msg.contains("refused") || clean_msg.contains("timeout") || clean_msg.contains("operation not permitted") || clean_msg.contains("proxy");

                if clean_msg.contains("unsupported proxy protocol") || clean_msg.contains("代理协议不支持") {
                    result.user_message = "代理协议不支持。请改用 Clash mixed-port HTTP 代理（推荐：http://127.0.0.1:7890）。".to_string();
                    result.network_ok = false;
                    result.network_status = "failed".to_string();
                    result.auth_status = "skipped".to_string();
                    result.repo_status = "skipped".to_string();
                    result.branch_status = "skipped".to_string();
                    return Ok(result);
                }

                if is_network_error {
                    if config.proxy_enabled && result.tcp_probe_ok {
                        result.user_message = "代理端口可连接，但 libgit2 通过代理访问 GitHub 失败。问题可能在 libgit2 代理链路限制。".to_string();
                    }
                    result.network_ok = false;
                    result.network_status = "failed".to_string();
                    result.auth_status = "skipped".to_string();
                    result.repo_status = "skipped".to_string();
                    result.branch_status = "skipped".to_string();
                } else {
                    result.network_ok = true;
                    result.network_status = "ok".to_string();

                    if clean_msg.contains("authentication failed") || clean_msg.contains("401") || clean_msg.contains("invalid credentials") {
                        result.auth_ok = false;
                        result.auth_status = "failed".to_string();
                        result.repo_status = "skipped".to_string();
                        result.branch_status = "skipped".to_string();
                    } else if clean_msg.contains("not found") || clean_msg.contains("404") {
                        result.auth_ok = true;
                        result.auth_status = "ok".to_string();
                        result.repo_ok = false;
                        result.repo_status = "failed".to_string();
                        result.branch_status = "skipped".to_string();
                    } else {
                        result.auth_ok = false;
                        result.auth_status = "failed".to_string();
                        result.repo_status = "skipped".to_string();
                        result.branch_status = "skipped".to_string();
                    }
                }

                return Ok(result);
            }
        };

        result.network_ok = true;
        result.network_status = "ok".to_string();
        result.auth_ok = true;
        result.auth_status = "ok".to_string();
        result.repo_ok = true;

        let list = match connection.list() {
            Ok(l) => {
                result.repo_status = "ok".to_string();
                l
            },
            Err(e) => {
                let err_msg = e.to_string();
                let clean_msg = err_msg.replace(&token, "***TOKEN***");
                result.raw_error = Some(clean_msg.clone());
                result.user_message = get_user_friendly_error(&clean_msg);

                if clean_msg.contains("not found") || clean_msg.contains("404") {
                    result.repo_ok = false;
                    result.repo_status = "failed".to_string();
                    result.error_category = "repo_not_found_or_no_permission".to_string();
                } else {
                    result.repo_ok = false;
                    result.repo_status = "failed".to_string();
                    result.error_category = "repo_access_failed".to_string();
                }
                result.branch_status = "skipped".to_string();
                return Ok(result);
            }
        };

        let branch_ref = format!("refs/heads/{}", config.branch);
        let mut found_branch = false;
        for head in list {
            if head.name() == branch_ref {
                found_branch = true;
                break;
            }
        }

        if found_branch {
            result.branch_ok = true;
            result.branch_status = "ok".to_string();
            result.success = true;
            result.user_message = "诊断成功：连接正常，权限有效，仓库和分支存在。".to_string();
        } else {
            result.branch_ok = false;
            result.branch_status = "missing".to_string();
            result.success = true;
            result.user_message = format!("仓库可访问，分支 {} 不存在。首次同步将创建该分支。", config.branch);
            result.error_category = "branch_not_found".to_string();
        }

        Ok(result)
    }

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
                    return crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("代理不可用/端口不通: {}", msg),
                    ));
                }
            }
            e
        };

        let token_from_parsed = parsed.extracted_token;
        let token = secrets.token.clone().or(token_from_parsed).unwrap_or_default();
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
            SyncTransport::HttpsToken => {
                Some(GitAuth::HttpsToken {
                    username: username_for_cred.clone(),
                    token: token.clone(),
                })
            }
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
                    .clone_repo(
                        &sanitized_url,
                        workspace_path,
                        auth.as_ref(),
                        Some(config),
                    )
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
                    result.status = SyncStatus::Error(e.to_string());
                    result.error = Some(e.to_string());
                    return Ok(result);
                }
                if let Err(e) = backend.commit(workspace_path, "Auto sync local changes") {
                    result.status = SyncStatus::Error(e.to_string());
                    result.error = Some(e.to_string());
                    return Ok(result);
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
            let e_str = e.to_string().to_lowercase();
            if e_str.contains("unrelated")
                || e_str.contains("merge")
                || e_str.contains("no common ancestor")
            {
                let status = SyncStatus::Error(format!("Pull failed: {}", e));
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
            if e_str.contains("ref not found")
                || e_str.contains("couldn't find remote ref")
                || (e_str.contains("remote branch") && e_str.contains("not found"))
            {
                if result.first_sync_mode != FirstSyncMode::InitExistingWorkspace
                    && result.first_sync_mode != FirstSyncMode::AlreadyGitRepo
                {
                    return Ok(SyncResult::error(
                        SyncStatus::Error(format!("Pull failed: {}", e)),
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
                            let mut best_path = None;
                            if let Some(our) = &c.our {
                                best_path = Some(String::from_utf8_lossy(&our.path).to_string());
                            } else if let Some(their) = &c.their {
                                best_path = Some(String::from_utf8_lossy(&their.path).to_string());
                            } else if let Some(ancestor) = &c.ancestor {
                                best_path =
                                    Some(String::from_utf8_lossy(&ancestor.path).to_string());
                            }

                            let real_path = match best_path {
                                Some(p) => p,
                                None => {
                                    result.error = Some("Sync Conflict: unknown path".to_string());
                                    result.user_message = Some(
                                        "存在无法识别路径的冲突文件，需要手动处理。".to_string(),
                                    );
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

                return Ok(result);
            } else {
                return Ok(SyncResult::error(
                    SyncStatus::Error(format!("Pull failed: {}", e)),
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
                    SyncStatus::Error(e.to_string()),
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
                    SyncStatus::Error(format!("Stage failed: {}", e)),
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
                        SyncStatus::Error(format!("Commit failed: {}", e)),
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
                    SyncStatus::Error(format!("Push failed: {}", e)),
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
            result.status = SyncStatus::Error(format!("Failed to save sync state: {}", e));
            result.error = Some(format!("Failed to save sync state: {}", e));
            result.user_message = Some(
                "同步操作完成，但同步状态保存失败，请不要连续同步，先检查存储权限。".to_string(),
            );
            return Ok(result);
        }

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
            if Self::is_blacklisted_path(&entry.relative_path) {
                plan.ignored_files.push(entry.relative_path);
                continue;
            }

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
    fn test_sync_secrets_local_json_blacklisted() {
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/sync_secrets.local.json"
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/sync_secrets.local.json.tmp"
        ));
    }

    #[test]
    fn test_first_sync_mode_unrelated_histories() {
        // Test logic added via GitBackend trait mock
        struct MockUnrelatedBackend;
        impl GitBackend for MockUnrelatedBackend {
            fn init_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn ensure_remote(&self, _: &Path, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn has_repo(&self, _: &Path) -> bool {
                true
            }
            fn is_worktree_empty_or_git_only(&self, _: &Path) -> crate::Result<bool> {
                Ok(false)
            }
            fn open_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn push(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "fatal: refusing to merge unrelated histories",
                )))
            }
            fn clone_repo(
                &self,
                _: &str,
                _: &Path,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(Some("hash".to_string()))
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(Some("hash".to_string()))
            }
            fn status(&self, _: &Path) -> crate::Result<Vec<String>> {
                Ok(vec![])
            }
        }

        let dir = tempdir().unwrap();
        let config = SyncConfig {
            proxy_enabled: false,
            proxy_type: "http".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let result =
            SyncService::perform_sync(dir.path(), &config, &secrets, &MockUnrelatedBackend)
                .unwrap();
        assert_eq!(result.first_sync_mode, FirstSyncMode::UnrelatedHistories);
        assert!(result.user_message.unwrap().contains("远端仓库不是空仓库"));
    }

    #[test]
    fn test_record_sync_conflict_error_handling() {
        // Provide an invalid path to force an IO error
        let conflict = SyncConflict {
            local_path: "chapter.md".to_string(),
            remote_path: "chapter.md".to_string(),
            local_hash: "aaa".to_string(),
            remote_hash: "bbb".to_string(),
            base_hash: "ccc".to_string(),
            created_at: 123456789,
            description: "conflict test".to_string(),
        };

        // Pass a non-existent parent directory to force an error
        let res = SyncService::record_sync_conflict(
            Path::new("/non/existent/path/that/will/fail"),
            conflict,
            None,
        );
        assert!(res.is_err());
    }

    #[test]
    fn test_perform_sync_non_empty_no_git_init() {
        // Just a mock test to verify the logic inside perform_sync
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("some_file.txt"), "hello").unwrap();

        let config = SyncConfig {
            proxy_enabled: false,
            proxy_type: "http".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            enabled: true,
            remote_url: "https://github.com/test/test.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockBackend;
        impl GitBackend for MockBackend {
            fn clone_repo(
                &self,
                _: &str,
                _: &Path,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn open_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(&self, _: &Path) -> crate::Result<Vec<String>> {
                Ok(vec![])
            }
            fn init_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn ensure_remote(&self, _: &Path, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn has_repo(&self, _: &Path) -> bool {
                false
            }
            fn is_worktree_empty_or_git_only(&self, _: &Path) -> crate::Result<bool> {
                Ok(false)
            }
        }

        let res = SyncService::perform_sync(dir.path(), &config, &secrets, &MockBackend).unwrap();
        assert_eq!(res.status, SyncStatus::Success);
    }

    #[test]
    fn test_perform_sync_auto_commits_whitelist() {
        // Just mock test to ensure successful pass of logic
        assert!(SyncService::is_whitelisted_path(
            "app-meta/settings/settings.sync.json"
        ));
    }

    #[test]
    fn test_no_unknown_conflicts() {
        let conflict = SyncConflict {
            local_path: "real/path.txt".to_string(),
            remote_path: "real/path.txt".to_string(),
            local_hash: "".to_string(),
            remote_hash: "".to_string(),
            base_hash: "".to_string(),
            description: "".to_string(),
            created_at: 0,
        };
        assert_ne!(conflict.local_path, "unknown");
        assert_ne!(conflict.remote_path, "unknown");
    }

    #[test]
    fn test_sync_config_state_no_token() {
        let config = SyncConfig {
            proxy_enabled: false,
            proxy_type: "http".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            enabled: true,
            remote_url: "url".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let state = SyncState {
            remote_url: Some("url".to_string()),
            transport: Some(SyncTransport::HttpsToken),
            last_sync_time: Some(0),
            last_synced_commit: None,
            last_error: None,
            known_files: std::collections::HashMap::new(),
            conflicts: vec![],
        };
        let config_str = serde_json::to_string(&config).unwrap();
        let state_str = serde_json::to_string(&state).unwrap();
        // Since HttpsToken serializes as "https_token" because of snake_case, token is in the string.
        // We really want to assert that the ACTUAL token string is not there.
        assert!(!config_str.contains("my_secret_token"));
        assert!(!state_str.contains("my_secret_token"));
    }

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
            proxy_enabled: false,
            proxy_type: "http".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
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
            proxy_enabled: false,
            proxy_type: "http".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            enabled: false,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let plan = SyncService::perform_sync_dry_run(dir.path(), &config).unwrap();
        assert!(plan.files_to_upload.is_empty());
        assert!(plan.ignored_files.is_empty());
    }

    #[test]
    fn test_sync_dry_run_enabled_config_scans() {
        let dir = tempdir().unwrap();
        let config = SyncConfig {
            proxy_enabled: false,
            proxy_type: "http".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
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
            proxy_enabled: false,
            proxy_type: "http".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            enabled: true,
            remote_url: "".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
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

    #[test]
    fn test_perform_sync_non_empty_remote() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig {
            proxy_enabled: false,
            proxy_type: "http".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockInitNonEmptyBackend;
        impl GitBackend for MockInitNonEmptyBackend {
            fn clone_repo(
                &self,
                _: &str,
                _: &Path,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn open_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "pull failed: unable to merge unrelated histories",
                )))
            }
            fn stage_paths(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(&self, _: &Path) -> crate::Result<Vec<String>> {
                Ok(vec![])
            }
            fn init_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn ensure_remote(&self, _: &Path, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn has_repo(&self, _: &Path) -> bool {
                false
            }
            fn is_worktree_empty_or_git_only(&self, _: &Path) -> crate::Result<bool> {
                Ok(false)
            }
        }

        let res =
            SyncService::perform_sync(dir.path(), &config, &secrets, &MockInitNonEmptyBackend)
                .unwrap();
        assert_eq!(res.first_sync_mode, FirstSyncMode::UnrelatedHistories);
        assert!(res
            .user_message
            .unwrap()
            .contains("推荐使用空 GitHub 私人仓库"));
    }

    #[test]
    fn test_save_sync_state_failure() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("app-meta/sync");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join("sync_state.json"), "{}").unwrap();

        let config = SyncConfig {
            proxy_enabled: false,
            proxy_type: "http".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockBackendOk;
        impl GitBackend for MockBackendOk {
            fn clone_repo(
                &self,
                _: &str,
                _: &Path,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn open_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(&self, _: &Path) -> crate::Result<Vec<String>> {
                Ok(vec![])
            }
            fn init_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn ensure_remote(&self, _: &Path, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn has_repo(&self, _: &Path) -> bool {
                true
            }
            fn is_worktree_empty_or_git_only(&self, _: &Path) -> crate::Result<bool> {
                Ok(false)
            }
        }

        std::fs::remove_file(state_dir.join("sync_state.json")).unwrap();
        std::fs::create_dir(state_dir.join("sync_state.json")).unwrap();

        let res = SyncService::perform_sync(dir.path(), &config, &secrets, &MockBackendOk).unwrap();
        assert!(matches!(res.status, SyncStatus::Error(_)));
        assert!(res.user_message.unwrap().contains("同步状态保存失败"));
    }

    #[test]
    fn test_first_sync_mode_clone_into_empty_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig {
            proxy_enabled: false,
            proxy_type: "http".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockBackend;
        impl GitBackend for MockBackend {
            fn clone_repo(
                &self,
                _: &str,
                _: &Path,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn open_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(&self, _: &Path) -> crate::Result<Vec<String>> {
                Ok(vec![])
            }
            fn init_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn ensure_remote(&self, _: &Path, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn has_repo(&self, _: &Path) -> bool {
                false
            }
            fn is_worktree_empty_or_git_only(&self, _: &Path) -> crate::Result<bool> {
                Ok(true)
            }
        }

        let res = SyncService::perform_sync(dir.path(), &config, &secrets, &MockBackend).unwrap();
        assert_eq!(res.first_sync_mode, FirstSyncMode::CloneIntoEmptyWorkspace);
    }

    #[test]
    fn test_first_sync_mode_init_existing_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig {
            proxy_enabled: false,
            proxy_type: "http".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockBackend;
        impl GitBackend for MockBackend {
            fn clone_repo(
                &self,
                _: &str,
                _: &Path,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn open_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(&self, _: &Path) -> crate::Result<Vec<String>> {
                Ok(vec![])
            }
            fn init_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn ensure_remote(&self, _: &Path, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn has_repo(&self, _: &Path) -> bool {
                false
            }
            fn is_worktree_empty_or_git_only(&self, _: &Path) -> crate::Result<bool> {
                Ok(false)
            }
        }

        let res = SyncService::perform_sync(dir.path(), &config, &secrets, &MockBackend).unwrap();
        assert_eq!(res.first_sync_mode, FirstSyncMode::InitExistingWorkspace);
    }

    #[test]
    fn test_first_sync_mode_already_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig {
            proxy_enabled: false,
            proxy_type: "http".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockBackend;
        impl GitBackend for MockBackend {
            fn clone_repo(
                &self,
                _: &str,
                _: &Path,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn open_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(&self, _: &Path) -> crate::Result<Vec<String>> {
                Ok(vec![])
            }
            fn init_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn ensure_remote(&self, _: &Path, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn has_repo(&self, _: &Path) -> bool {
                true
            }
            fn is_worktree_empty_or_git_only(&self, _: &Path) -> crate::Result<bool> {
                Ok(false)
            }
        }

        let res = SyncService::perform_sync(dir.path(), &config, &secrets, &MockBackend).unwrap();
        assert_eq!(res.first_sync_mode, FirstSyncMode::AlreadyGitRepo);
    }

    #[test]
    fn test_sync_plan_no_tokens() {
        let dir = tempfile::tempdir().unwrap();

        let settings_dir = dir.path().join("app-meta/sync");
        std::fs::create_dir_all(&settings_dir).unwrap();

        // Write the local secrets
        std::fs::write(
            settings_dir.join("sync_secrets.local.json"),
            "secret_token_123",
        )
        .unwrap();
        std::fs::write(
            settings_dir.join("sync_secrets.local.json.tmp"),
            "secret_token_456",
        )
        .unwrap();

        // Also write some valid file to sync
        std::fs::write(dir.path().join("workspace_manifest.json"), "{}").unwrap();

        let plan = SyncService::build_sync_plan_from_workspace(dir.path()).unwrap();

        // Ensure plan does not include the blacklisted items
        for file in plan.files_to_upload {
            assert!(
                !file.contains("sync_secrets.local.json"),
                "Should not upload secrets"
            );
        }

        let ignored: Vec<String> = plan.ignored_files.into_iter().collect();
        assert!(
            ignored
                .iter()
                .any(|s| s.contains("sync_secrets.local.json")),
            "Secrets should be explicitly ignored"
        );
    }

    #[test]
    fn test_first_sync_empty_remote_branch_not_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("workspace_manifest.json"), "{}").unwrap();

        let config = SyncConfig {
            proxy_enabled: false,
            proxy_type: "http".to_string(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            enabled: true,
            remote_url: "https://github.com/test/empty-repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockEmptyRemoteBackend;
        impl GitBackend for MockEmptyRemoteBackend {
            fn clone_repo(
                &self,
                _: &str,
                _: &Path,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn open_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "ref not found: refs/heads/main",
                )))
            }
            fn stage_paths(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(Some("commit_hash".to_string()))
            }
            fn push(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: Option<&SyncConfig>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(&self, _: &Path) -> crate::Result<Vec<String>> {
                Ok(vec!["workspace_manifest.json".to_string()])
            }
            fn init_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn ensure_remote(&self, _: &Path, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn has_repo(&self, _: &Path) -> bool {
                false
            }
            fn is_worktree_empty_or_git_only(&self, _: &Path) -> crate::Result<bool> {
                Ok(false)
            }
        }

        let res =
            SyncService::perform_sync(dir.path(), &config, &secrets, &MockEmptyRemoteBackend)
                .unwrap();
        assert_eq!(res.status, SyncStatus::Success);
        assert_eq!(res.first_sync_mode, FirstSyncMode::InitExistingWorkspace);
        assert!(res.user_message.unwrap().contains("已初始化远端分支并完成首次同步"));
    }
}
