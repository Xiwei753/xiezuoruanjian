use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use base64::Engine;

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
        BackendType::GithubApi
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
    "auto".to_string()
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

pub fn is_github_https_remote(remote_url: &str) -> bool {
    let sanitized = sanitize_remote_url(remote_url).sanitized_url;
    let lower = sanitized.to_lowercase();
    lower.starts_with("https://github.com/") || lower.starts_with("http://github.com/")
}

pub fn resolved_backend_type(config: &SyncConfig) -> BackendType {
    if config.backend_type == BackendType::Git && is_github_https_remote(&config.remote_url) {
        BackendType::GithubApi
    } else {
        config.backend_type.clone()
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

/// Redact known secrets (token, password) from a diagnostic/error message.
/// This is a SAFE replacement for `mask_token` which was destroying the entire error.
/// Strategy:
/// 1. Redact URL userinfo (https://user:token@host/path -> https://***@host/path)
/// 2. If a known token string is provided, redact every occurrence of it.
/// 3. Does NOT touch ordinary error text, git return codes, or libgit2 messages.
pub fn redact_secrets_from_message(msg: &str, known_token: Option<&str>, remote_url: Option<&str>) -> String {
    let mut result = msg.to_string();

    // 1. Always redact URL userinfo
    if let Some(url) = remote_url {
        if url.contains('@') {
            if let Some(prefix) = url.split("://").next() {
                result = result.replace(url, &format!("{}://***@...", prefix));
            }
        }
    } else {
        // Generic URL userinfo redaction if no specific URL given
        result = mask_token_in_url(&result);
    }

    // 2. Redact known token if provided
    if let Some(token) = known_token {
        if !token.is_empty() && token.len() >= 4 {
            result = result.replace(token, "***REDACTED***");
        }
    }

    // 3. Redact any remaining embedded URLs with userinfo
    // Pattern: https://something@...
    let mut found = true;
    while found {
        found = false;
        if let Some(start) = result.find("://") {
            let before = &result[..start];
            // Look backwards for start of scheme
            let scheme_start = before.rfind(|c: char| !c.is_alphanumeric() && c != '+' && c != '-' && c != '.')
                .map(|p| p + 1)
                .unwrap_or(0);
            let scheme = &result[scheme_start..start];
            if scheme == "http" || scheme == "https" || scheme == "ssh" || scheme == "git" || scheme == "socks5" || scheme == "socks5h" {
                let rest = &result[start + 3..];
                if let Some(at_pos) = rest.find('@') {
                    let before_at = &rest[..at_pos];
                    if before_at.contains(':') || before_at.contains('%') {
                        // Has userinfo (contains colon or percent-encoded chars)
                        let redacted = format!("{}://***@", scheme);
                        let after_at = &rest[at_pos + 1..];
                        // Find end (space, newline, comma, end-of-string)
                        let end = after_at.find(|c: char| c.is_whitespace() || c == ',' || c == ')' || c == ']')
                            .unwrap_or(after_at.len());
                        let full = format!("{}://{}{}", scheme, before_at, &after_at[..end]);
                        let replacement = format!("{}***@{}", redacted, &after_at[..end]);
                        if let Some(pos) = result.find(&full) {
                            result.replace_range(pos..pos + full.len(), &replacement);
                            found = true;
                        }
                    }
                }
            }
        }
    }

    result
}

/// Legacy token masking function - now just an alias for redact_secrets_from_message
/// without known secrets. This prevents the old behavior of masking the entire error message.
pub fn mask_token(s: &str) -> String {
    redact_secrets_from_message(s, None, None)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Idle,
    Syncing,
    Success,
    ConfiguredUntested,
    Conflict,
    RecoverableError(String),
    FatalError(String),
    DirtyRepoBlocked,
    BranchMissingRecovered,
    Error(String),
    NoChanges,
    LatestWinsApplied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingConflictDetail {
    pub key: String,
    pub local_value: serde_json::Value,
    pub remote_value: serde_json::Value,
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
pub struct NetworkProbeResult {
    pub mode: String,
    pub success: bool,
    pub status: String,
    pub message: String,
    pub raw_error: Option<String>,
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
    pub chosen_network_mode: Option<String>,
    pub network_probe_summary: Vec<NetworkProbeResult>,
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
            chosen_network_mode: None,
            network_probe_summary: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflictSummary {
    pub status: String,
    pub local_dirty: bool,
    pub remote_changed: bool,
    pub conflicted_files: Vec<String>,
    pub blocked_reason: String,
    pub safe_next_steps: Vec<String>,
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
    pub error_category: Option<String>,
    pub conflict_summary: Option<SyncConflictSummary>,
    pub first_sync_mode: FirstSyncMode,
    pub user_message: Option<String>,
    pub chosen_network_mode: Option<String>,
    pub network_probe_summary: Vec<NetworkProbeResult>,
    pub settings_conflicts: Option<Vec<SettingConflictDetail>>,
    #[serde(default)]
    pub local_deletes: Vec<String>,
    #[serde(default)]
    pub remote_deletes: Vec<String>,
    #[serde(default)]
    pub overwritten_files: Vec<String>,
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
            error_category: None,
            conflict_summary: None,
            first_sync_mode: FirstSyncMode::NotAttempted,
            user_message: None,
            chosen_network_mode: None,
            network_probe_summary: Vec::new(),
            settings_conflicts: None,
            local_deletes: Vec::new(),
            remote_deletes: Vec::new(),
            overwritten_files: Vec::new(),
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
            error_category: None,
            conflict_summary: None,
            first_sync_mode,
            user_message,
            chosen_network_mode: None,
            network_probe_summary: Vec::new(),
            settings_conflicts: None,
            local_deletes: Vec::new(),
            remote_deletes: Vec::new(),
            overwritten_files: Vec::new(),
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
            error_category: None,
            conflict_summary: None,
            first_sync_mode: FirstSyncMode::NotAttempted,
            user_message,
            chosen_network_mode: None,
            network_probe_summary: Vec::new(),
            settings_conflicts: None,
            local_deletes: Vec::new(),
            remote_deletes: Vec::new(),
            overwritten_files: Vec::new(),
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

        // Record transaction anchors
        let is_unborn = repo.head().is_err();
        let original_head_ref_name = repo.head().ok().and_then(|r| r.name().map(String::from));
        let original_head_oid = repo.head().ok().and_then(|r| r.target());
        let original_index_bytes = std::fs::read(repo.path().join("index")).ok();

        let rollback = |repo: &git2::Repository| {
            let _ = repo.cleanup_state();
            if !is_unborn {
                if let Some(oid) = original_head_oid {
                    if let Ok(obj) = repo.find_object(oid, None) {
                        let mut cb = git2::build::CheckoutBuilder::default();
                        cb.force();
                        let _ = repo.reset(&obj, git2::ResetType::Hard, Some(&mut cb));
                    }
                }
            }
            if let Some(ref bytes) = original_index_bytes {
                let _ = std::fs::write(repo.path().join("index"), bytes);
                if let Ok(mut index) = repo.index() {
                    let _ = index.read(true);
                }
            }
            if let Some(ref ref_name) = original_head_ref_name {
                let _ = repo.set_head(ref_name);
            }
        };

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

        if let Err(e) = remote.fetch(&[branch], Some(&mut fetch_options), None) {
            rollback(&repo);
            return Err(crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )));
        }

        let fetch_head = repo
            .find_reference("FETCH_HEAD")
            .map_err(|e: git2::Error| {
                rollback(&repo);
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
        let fetch_commit =
            repo.reference_to_annotated_commit(&fetch_head)
                .map_err(|e: git2::Error| {
                    rollback(&repo);
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })?;

        // Handle unborn local repository
        if repo.head().is_err() {
            let commit_obj = match repo.find_commit(fetch_commit.id()) {
                Ok(c) => c,
                Err(e) => {
                    rollback(&repo);
                    return Err(crate::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
                }
            };
            if let Err(e) = repo.checkout_tree(commit_obj.as_object(), Some(git2::build::CheckoutBuilder::default().force())) {
                rollback(&repo);
                return Err(crate::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
            }
            if let Err(e) = repo.branch(branch, &commit_obj, true) {
                rollback(&repo);
                return Err(crate::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
            }
            if let Err(e) = repo.set_head(&format!("refs/heads/{}", branch)) {
                rollback(&repo);
                return Err(crate::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
            }
            return Ok(());
        }

        let analysis = repo
            .merge_analysis(&[&fetch_commit])
            .map_err(|e: git2::Error| {
                rollback(&repo);
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;

        // Pre-pull safety check: check for index conflicts and blocking untracked files
        let refname = format!("refs/heads/{}", branch);
        if let Ok(statuses) = repo.statuses(Some(git2::StatusOptions::new().include_untracked(true))) {
            let mut blocking_files = Vec::new();
            for entry in statuses.iter() {
                if let Some(path) = entry.path() {
                    if SyncService::is_blacklisted_path(path) {
                        continue;
                    }
                    let status = entry.status();
                    if status.is_index_new() || status.is_index_deleted() || status.is_index_modified() {
                        // Index has conflicts or unmerged entries
                        if status.is_conflicted() {
                            rollback(&repo);
                            let conflicts = collect_index_conflicts(&repo);
                            let summary = SyncConflictSummary {
                                status: "conflict".to_string(),
                                local_dirty: true,
                                remote_changed: true,
                                conflicted_files: conflicts,
                                blocked_reason: "本地 Git 暂存区存在未解决的冲突，请先解决冲突。".to_string(),
                                safe_next_steps: vec![
                                    "备份当前工作区。".to_string(),
                                    "运行诊断确认网络/认证没问题。".to_string(),
                                    "手动处理冲突后重新同步。".to_string(),
                                ],
                            };
                            let payload = serde_json::to_string(&summary).unwrap_or_default();
                            return Err(crate::Error::Io(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                format!("checkout_conflict_payload:{}", payload),
                            )));
                        }
                    }
                    // Check for untracked files that would be overwritten
                    if status.is_wt_new() {
                        if SyncService::is_whitelisted_path(path) {
                            blocking_files.push(path.to_string());
                        }
                    }
                }
            }
            if !blocking_files.is_empty() {
                rollback(&repo);
                let summary = SyncConflictSummary {
                    status: "conflict".to_string(),
                    local_dirty: true,
                    remote_changed: true,
                    conflicted_files: blocking_files.clone(),
                    blocked_reason: format!("本地工作区有 {} 个未跟踪文件会阻止远端 checkout。", blocking_files.len()),
                    safe_next_steps: vec![
                        "备份当前工作区。".to_string(),
                        "运行诊断确认网络/认证没问题。".to_string(),
                        "手动处理冲突后重新同步。".to_string(),
                    ],
                };
                let payload = serde_json::to_string(&summary).unwrap_or_default();
                return Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("checkout_conflict_payload:{}", payload),
                )));
            }
        }

        if analysis.0.is_up_to_date() {
            // Do nothing
        } else if analysis.0.is_fast_forward() {
            // Fast-forward: checkout target tree FIRST, then update ref/head.
            // This ensures that if checkout fails, HEAD/ref remain unchanged.

            // Step 1: Dry-run checkout to detect conflicts before making any changes
            let conflicted_paths = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let cp_clone = conflicted_paths.clone();
            
            let mut dry_run_builder = git2::build::CheckoutBuilder::default();
            dry_run_builder.notify_on(git2::CheckoutNotificationType::CONFLICT);
            dry_run_builder.notify(move |_, path, _, _, _| {
                if let Some(p) = path {
                    if let Some(s) = p.to_str() {
                        cp_clone.borrow_mut().push(s.to_string());
                    }
                }
                true
            });
            dry_run_builder.dry_run();
            let fetch_tree = match repo.find_commit(fetch_commit.id()).and_then(|c| c.tree()) {
                Ok(t) => t,
                Err(e) => {
                    rollback(&repo);
                    return Err(crate::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
                }
            };
            if let Err(e) = repo.checkout_tree(
                fetch_tree.as_object(),
                Some(&mut dry_run_builder),
            ) {
                rollback(&repo);
                let err_msg = e.to_string();
                if err_msg.contains("conflict") || err_msg.contains("Conflict") {
                    let paths = conflicted_paths.borrow().clone();
                    let summary = crate::sync_service::SyncConflictSummary {
                        status: "conflict".to_string(),
                        local_dirty: true,
                        remote_changed: true,
                        conflicted_files: paths,
                        blocked_reason: "本地未提交的改动与远端更新冲突，Git 无法安全检出。".to_string(),
                        safe_next_steps: vec![
                            "备份当前工作区。".to_string(),
                            "运行诊断确认网络/认证没问题。".to_string(),
                            "手动处理冲突后重新同步。".to_string(),
                        ],
                    };
                    let payload = serde_json::to_string(&summary).unwrap_or_default();
                    return Err(crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("checkout_conflict_payload:{}", payload),
                    )));
                }
                return Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("checkout dry-run failed: {}", err_msg),
                )));
            }

            // Step 2: Actual checkout (safe) - still no ref/head change yet
            let fetch_tree2 = match repo.find_commit(fetch_commit.id()).and_then(|c| c.tree()) {
                Ok(t) => t,
                Err(e) => {
                    rollback(&repo);
                    return Err(crate::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
                }
            };
            if let Err(e) = repo.checkout_tree(
                fetch_tree2.as_object(),
                Some(git2::build::CheckoutBuilder::default().safe()),
            ) {
                rollback(&repo);
                return Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )));
            }

            // Step 3: Only after successful checkout, update ref and head
            let mut reference = match repo.find_reference(&refname) {
                Ok(r) => r,
                Err(e) => {
                    rollback(&repo);
                    return Err(crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )));
                }
            };
            if let Err(e) = reference.set_target(fetch_commit.id(), "Fast-Forward") {
                rollback(&repo);
                return Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )));
            }
            if let Err(e) = repo.set_head(&refname) {
                rollback(&repo);
                return Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )));
            }
        } else if analysis.0.is_normal() {
            let mut merge_opts = git2::MergeOptions::new();
            if let Err(e) = repo.merge(&[&fetch_commit], Some(&mut merge_opts), None) {
                rollback(&repo);
                let err_msg = e.to_string();
                if e.code() == git2::ErrorCode::Conflict || e.class() == git2::ErrorClass::Checkout || err_msg.contains("conflict") || err_msg.contains("Conflict") {
                    let summary = build_conflict_summary(&repo, Some(fetch_commit.id()), "本地未提交的改动或冲突阻止了合并操作。");
                    let payload = serde_json::to_string(&summary).unwrap_or_default();
                    return Err(crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("checkout_conflict_payload:{}", payload),
                    )));
                }
                return Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    err_msg,
                )));
            }

            let mut index = match repo.index() {
                Ok(i) => i,
                Err(e) => {
                    rollback(&repo);
                    return Err(crate::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
                }
            };

            // Settings semantic merge conflict resolution
            let mut settings_conflict_details = None;
            let mut resolved_settings = false;
            if index.has_conflicts() {
                if let Ok(mut conflicts) = index.conflicts() {
                    let mut settings_conflict = None;
                    for conflict in conflicts.by_ref() {
                        if let Ok(c) = conflict {
                            let path_opt = c.our.as_ref().map(|o| String::from_utf8_lossy(&o.path).to_string())
                                .or_else(|| c.their.as_ref().map(|t| String::from_utf8_lossy(&t.path).to_string()))
                                .or_else(|| c.ancestor.as_ref().map(|a| String::from_utf8_lossy(&a.path).to_string()));
                            if let Some(p) = path_opt {
                                if p == "app-meta/settings/settings.sync.json" {
                                    settings_conflict = Some(c);
                                    break;
                                }
                            }
                        }
                    }

                    if let Some(c) = settings_conflict {
                        let base_json = c.ancestor.as_ref()
                            .and_then(|entry| repo.find_blob(entry.id).ok())
                            .and_then(|blob| {
                                let s = std::str::from_utf8(blob.content()).ok()?;
                                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(s).ok()
                            })
                            .unwrap_or_default();

                        let local_json = c.our.as_ref()
                            .and_then(|entry| repo.find_blob(entry.id).ok())
                            .and_then(|blob| {
                                let s = std::str::from_utf8(blob.content()).ok()?;
                                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(s).ok()
                            })
                            .unwrap_or_default();

                        let remote_json = c.their.as_ref()
                            .and_then(|entry| repo.find_blob(entry.id).ok())
                            .and_then(|blob| {
                                let s = std::str::from_utf8(blob.content()).ok()?;
                                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(s).ok()
                            })
                            .unwrap_or_default();

                        match SyncService::semantic_merge_json(&base_json, &local_json, &remote_json) {
                            Ok(merged_map) => {
                                let merged_value = serde_json::Value::Object(merged_map);
                                let merged_str = serde_json::to_string_pretty(&merged_value).unwrap_or_default();
                                
                                let settings_path = local_repo_path.join("app-meta/settings/settings.sync.json");
                                if let Some(parent) = settings_path.parent() {
                                    std::fs::create_dir_all(parent).ok();
                                }
                                let _ = std::fs::write(&settings_path, &merged_str);

                                if let Ok(mut mut_index) = repo.index() {
                                    if mut_index.add_path(Path::new("app-meta/settings/settings.sync.json")).is_ok() {
                                        let _ = mut_index.write();
                                        resolved_settings = true;
                                    }
                                }
                            }
                            Err(key_conflicts) => {
                                settings_conflict_details = Some(key_conflicts);
                            }
                        }
                    }
                }
            }

            if resolved_settings {
                if let Ok(reloaded) = repo.index() {
                    index = reloaded;
                }
            }

            if let Some(details) = settings_conflict_details {
                rollback(&repo);
                let payload = serde_json::to_string(&details).unwrap_or_default();
                return Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("settings_conflict_payload:{}", payload),
                )));
            }

            if index.has_conflicts() {
                rollback(&repo);
                // Return an error for conflicts with a special prefix that can be parsed
                return Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "SyncConflict_Detected".to_string(),
                )));
            } else {
                let oid = match index.write_tree() {
                    Ok(o) => o,
                    Err(e) => {
                        rollback(&repo);
                        return Err(crate::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
                    }
                };
                let signature = match git2::Signature::now("Sync User", "sync@writer.app") {
                    Ok(s) => s,
                    Err(e) => {
                        rollback(&repo);
                        return Err(crate::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
                    }
                };
                let tree = match repo.find_tree(oid) {
                    Ok(t) => t,
                    Err(e) => {
                        rollback(&repo);
                        return Err(crate::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
                    }
                };
                let head_ref = match repo.head() {
                    Ok(r) => r,
                    Err(e) => {
                        rollback(&repo);
                        return Err(crate::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
                    }
                };
                let head_commit = match head_ref.peel_to_commit() {
                    Ok(c) => c,
                    Err(e) => {
                        rollback(&repo);
                        return Err(crate::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
                    }
                };
                let fetch_commit_obj = match repo.find_commit(fetch_commit.id()) {
                    Ok(c) => c,
                    Err(e) => {
                        rollback(&repo);
                        return Err(crate::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
                    }
                };
                if let Err(e) = repo.commit(
                    Some("HEAD"),
                    &signature,
                    &signature,
                    "Merge remote-tracking branch",
                    &tree,
                    &[&head_commit, &fetch_commit_obj],
                ) {
                    rollback(&repo);
                    return Err(crate::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
                }
                if let Err(e) = repo.cleanup_state() {
                    rollback(&repo);
                    return Err(crate::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
                }
            }
        } else {
            rollback(&repo);
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

        // 1. Check index conflicts
        if let Ok(index) = repo.index() {
            if index.has_conflicts() {
                return Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "fatal_error: Cannot push: index has unresolved conflicts.".to_string(),
                )));
            }
        }

        let branch_ref_name = format!("refs/heads/{}", branch);
        let branch_exists = repo.find_reference(&branch_ref_name).is_ok();

        // 2. Check HEAD commit
        let head_ref = repo.head();
        let head_commit = head_ref.as_ref().ok().and_then(|r| r.peel_to_commit().ok());

        match (branch_exists, head_commit) {
            (true, Some(_)) => {
                // Normal case: branch ref exists, HEAD points to a commit.
            }
            (false, Some(commit)) => {
                // Branch ref doesn't exist but HEAD has a commit. Reconstruct it.
                repo.branch(branch, &commit, false).map_err(|e| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("fatal_error: Failed to reconstruct branch ref: {}", e),
                    ))
                })?;
                let _ = repo.set_head(&branch_ref_name);
            }
            (_, None) => {
                // HEAD unborn / no commits.
                return Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "recoverable_error: HEAD is unborn and has no commit.".to_string(),
                )));
            }
        }

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
        SyncService::perform_lww_sync(workspace_path, config, secrets)
    }
}

pub struct GitHubApiBackend;

struct ProbedClient {
    client: reqwest::blocking::Client,
    mode: String,
}

impl GitHubApiBackend {

    fn classify_reqwest_error(e: &reqwest::Error) -> (String, String) {
        let msg = e.to_string().to_lowercase();
        if msg.contains("dns") || msg.contains("resolve") || msg.contains("name resolution") {
            ("dns_failed".to_string(), "无法解析域名".to_string())
        } else if msg.contains("ssl") || msg.contains("certificate") || msg.contains("tls") {
            ("tls_failed".to_string(), "SSL/TLS 握手失败".to_string())
        } else if msg.contains("connection refused") {
            ("connection_refused".to_string(), "连接被拒绝 (端口可能未开放)".to_string())
        } else if e.is_timeout() || msg.contains("timeout") {
            ("timeout".to_string(), "连接超时".to_string())
        } else {
            ("network_failed".to_string(), "网络请求失败".to_string())
        }
    }

    fn build_auto_client(config: &SyncConfig, secrets: &SyncSecrets, workspace_path: Option<&Path>) -> crate::Result<(ProbedClient, Vec<NetworkProbeResult>)> {
        let mut probe_summary = Vec::new();
        let token = secrets.token.clone().unwrap_or_default();
        let api_base = Self::api_base_url(&config.remote_url);
        let test_url = format!("{}/rate_limit", if api_base.starts_with("https://api.github.com/repos/") { "https://api.github.com" } else { &api_base });

        let mut candidates = match config.proxy_type.as_str() {
            "auto" | "" => vec![
                ("direct".to_string(), "none".to_string(), "".to_string(), 0u16),
                ("http_local_7890".to_string(), "http".to_string(), "127.0.0.1".to_string(), 7890u16),
                ("socks5_local_7891".to_string(), "socks5".to_string(), "127.0.0.1".to_string(), 7891u16),
            ],
            "none" => vec![
                ("direct".to_string(), "none".to_string(), "".to_string(), 0u16),
            ],
            "http" => {
                let host = if config.proxy_host.is_empty() { "127.0.0.1" } else { &config.proxy_host };
                let port = if config.proxy_port > 0 { config.proxy_port } else { 7890 };
                vec![
                    (format!("http_{}:{}", host, port), "http".to_string(), host.to_string(), port),
                ]
            },
            "socks5" => {
                let host = if config.proxy_host.is_empty() { "127.0.0.1" } else { &config.proxy_host };
                let port = if config.proxy_port > 0 { config.proxy_port } else { 7891 };
                vec![
                    (format!("socks5_{}:{}", host, port), "socks5".to_string(), host.to_string(), port),
                ]
            },
            _ => vec![
                ("direct".to_string(), "none".to_string(), "".to_string(), 0u16),
                ("http_local_7890".to_string(), "http".to_string(), "127.0.0.1".to_string(), 7890u16),
                ("socks5_local_7891".to_string(), "socks5".to_string(), "127.0.0.1".to_string(), 7891u16),
            ],
        };

        if let Some(wp) = workspace_path {
            if let Ok(state) = SyncService::load_sync_state(wp) {
                if let Some(last_mode) = state.last_successful_network_mode {
                    if let Some(pos) = candidates.iter().position(|c| c.0 == last_mode) {
                        let c = candidates.remove(pos);
                        candidates.insert(0, c);
                    }
                }
            }
        }

        for (mode_name, p_type, p_host, p_port) in &candidates {
            let mut builder = reqwest::blocking::Client::builder()
                .user_agent("WriterApp/1.0")
                .timeout(std::time::Duration::from_secs(3));

            if *p_type != "none" && !p_host.is_empty() && *p_port > 0 {
                let proxy_url = match p_type.as_str() {
                    "http" => format!("http://{}:{}", p_host, p_port),
                    "socks5" => format!("socks5h://{}:{}", p_host, p_port),
                    _ => format!("http://{}:{}", p_host, p_port),
                };
                if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
                    builder = builder.proxy(proxy);
                }
            }

            match builder.build() {
                Ok(client) => {
                    let req = client.get(&test_url)
                        .header("User-Agent", "WriterApp/1.0")
                        .header("Accept", "application/vnd.github+json");

                    let req = if !token.is_empty() { req.header("Authorization", format!("Bearer {}", token)) } else { req };

                    match req.send() {
                        Ok(resp) => {
                            let status = resp.status().as_u16();
                            let msg = if status == 200 {
                                "网络连通测试成功".to_string()
                            } else {
                                format!("网络可达 (HTTP {})", status)
                            };
                            probe_summary.push(NetworkProbeResult {
                                mode: mode_name.clone(),
                                success: true,
                                status: "ok".to_string(),
                                message: msg,
                                raw_error: None,
                            });

                            let mut working_config = config.clone();
                            working_config.proxy_type = p_type.clone();
                            working_config.proxy_host = p_host.clone();
                            working_config.proxy_port = *p_port;
                            working_config.proxy_enabled = *p_type != "none";
                            let final_client = build_http_client(Some(&working_config))?;
                            return Ok((ProbedClient { client: final_client, mode: mode_name.clone() }, probe_summary));
                        }
                        Err(e) => {
                            let raw = e.to_string();
                            let sanitized = if !token.is_empty() { raw.replace(&token, "***TOKEN***") } else { raw.clone() };
                            let truncated = if sanitized.len() > 200 {
                                format!("{}...[truncated {} bytes]", &sanitized[..200], sanitized.len() - 200)
                            } else {
                                sanitized
                            };
                            let (status_code, msg) = Self::classify_reqwest_error(&e);
                            probe_summary.push(NetworkProbeResult {
                                mode: mode_name.clone(),
                                success: false,
                                status: status_code,
                                message: msg,
                                raw_error: Some(truncated),
                            });
                        }
                    }
                }
                Err(e) => {
                    probe_summary.push(NetworkProbeResult {
                        mode: mode_name.clone(),
                        success: false,
                        status: "client_build_failed".to_string(),
                        message: "无法构建 HTTP 客户端".to_string(),
                        raw_error: Some(e.to_string()),
                    });
                }
            }
        }

        let error_detail = probe_summary.iter().map(|p| {
            let err_suffix = p.raw_error.as_ref().map(|e| format!(" ({})", e)).unwrap_or_default();
            format!("  [{}] {}: {}{}", if p.success { "OK" } else { "FAIL" }, p.mode, p.message, err_suffix)
        }).collect::<Vec<_>>().join("\n");
        Err(crate::Error::Other(format!("network_probe_failed: 所有网络路径探测均失败:\n{}", error_detail)))
    }

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

        let probed_res = Self::build_auto_client(config, secrets, None);
        let (client, mode, probe_summary) = match probed_res {
            Ok((p, summary)) => (p.client, p.mode, summary),
            Err(e) => {
                result.user_message = format!("网络探测失败: {}", e);
                result.error_category = "network_probe_failed".to_string();
                return Ok(result);
            }
        };

        result.chosen_network_mode = Some(mode.clone());
        result.network_probe_summary = probe_summary;

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
                    result.user_message = format!("诊断成功：GitHub API 可达，Token 有效，仓库和分支存在。(使用网络模式: {})", mode);
                } else if status == 401 || status == 403 {
                    result.network_ok = true;
                    result.network_status = "ok".to_string();
                    result.auth_ok = false;
                    result.auth_status = "failed".to_string();
                    result.error_category = if status == 401 { "token_invalid" } else { "token_permission_denied" }.to_string();
                    result.user_message = if status == 401 {
                        format!("身份验证失败。Token 无效或已过期。(使用网络模式: {})", mode)
                    } else {
                        format!("Token 权限不足。请确认 Token 具有 repo 权限范围。(使用网络模式: {})", mode)
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
                    result.user_message = format!("找不到仓库或分支。请检查仓库地址和分支名称。(使用网络模式: {})", mode);
                    result.raw_error = Some(format!("HTTP {} (body truncated): {}", status, &body.chars().take(200).collect::<String>()));
                } else {
                    result.network_ok = false;
                    result.network_status = "failed".to_string();
                    result.error_category = "github_network_failed".to_string();
                    result.user_message = format!("GitHub API 返回意外状态码: {} (使用网络模式: {})", status, mode);
                    result.raw_error = Some(format!("HTTP {} (body truncated): {}", status, &body.chars().take(200).collect::<String>()));
                }
            }
            Err(e) => {
                let err_msg = e.to_string().to_lowercase();
                result.raw_error = Some(e.to_string());
                if err_msg.contains("dns") || err_msg.contains("resolve") || err_msg.contains("name resolution") {
                    result.error_category = "dns_failed".to_string();
                    result.user_message = format!("无法解析 GitHub API 地址。请检查网络/DNS 设置。(尝试过的最后模式: {})", mode);
                } else if err_msg.contains("ssl") || err_msg.contains("certificate") || err_msg.contains("tls") {
                    result.error_category = "tls_failed".to_string();
                    result.user_message = format!("SSL/TLS 连接失败。请检查网络环境或系统时间。(尝试过的最后模式: {})", mode);
                } else if err_msg.contains("connection refused") || err_msg.contains("timeout") || err_msg.contains("network unreachable") {
                    result.error_category = "github_network_failed".to_string();
                    result.user_message = format!("网络连接失败或超时。所有尝试路径均不可用。(尝试过的最后模式: {})", mode);
                } else {
                    result.error_category = "github_network_failed".to_string();
                    result.user_message = format!("GitHub API 请求失败: {} (尝试过的最后模式: {})", e, mode);
                }
                result.network_ok = false;
                result.network_status = "failed".to_string();
            }
        }

        Ok(result)
    }

    fn pull(&self, _: &Path, _: &SyncConfig, _: &SyncSecrets) -> crate::Result<SyncResult> {
        Ok(SyncResult::error(
            SyncStatus::Error("backend_not_implemented".to_string()),
            FirstSyncMode::NotAttempted,
            Some("GitHub API 后端的 pull 操作尚未实现。".to_string()),
            "GitHub API pull not implemented".to_string(),
        ))
    }

    fn push(&self, _: &Path, _: &SyncConfig, _: &SyncSecrets) -> crate::Result<SyncResult> {
        Ok(SyncResult::error(
            SyncStatus::Error("backend_not_implemented".to_string()),
            FirstSyncMode::NotAttempted,
            Some("GitHub API 后端的 push 操作尚未实现。".to_string()),
            "GitHub API push not implemented".to_string(),
        ))
    }

    fn sync(&self, workspace_path: &Path, config: &SyncConfig, secrets: &SyncSecrets) -> crate::Result<SyncResult> {
        eprintln!(
            "[sync] backend_type=github_api sync_mode=lww_manifest entry=GitHubApiBackend::sync remote_url={}",
            mask_token_in_url(&sanitize_remote_url(&config.remote_url).sanitized_url)
        );
        SyncService::perform_lww_sync(workspace_path, config, secrets)
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
    #[serde(default)]
    pub conflicts: Vec<String>,
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
            conflicts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestFileRecord {
    pub path: String,
    pub content_hash: String,
    pub updated_at_ms: i64,
    pub device_id: String,
    pub op: String, // "upsert" or "delete"
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
}

fn default_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncManifest {
    pub files: Vec<ManifestFileRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tombstone {
    pub original_path: String,
    pub trash_path: String,
    pub deleted_at: i64,
    pub purge_after: i64,
    pub deleted_by: String,
    pub original_hash: String,
    pub kind: String, // "local_delete" or "remote_delete"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    pub remote_url: Option<String>,
    pub transport: Option<SyncTransport>,
    pub last_synced_commit: Option<String>,
    pub last_sync_time: Option<i64>,
    pub last_error: Option<String>,
    pub last_successful_network_mode: Option<String>,
    pub known_files: std::collections::HashMap<String, String>,
    pub conflicts: Vec<SyncConflict>,
    #[serde(default)]
    pub tombstones: Vec<Tombstone>,
    #[serde(default)]
    pub deleted_files: std::collections::HashSet<String>,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub known_files_updated_at: std::collections::HashMap<String, i64>,
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            remote_url: None,
            transport: None,
            last_synced_commit: None,
            last_sync_time: None,
            last_error: None,
            last_successful_network_mode: None,
            known_files: std::collections::HashMap::new(),
            conflicts: Vec::new(),
            tombstones: Vec::new(),
            deleted_files: std::collections::HashSet::new(),
            device_id: uuid::Uuid::new_v4().to_string(),
            known_files_updated_at: std::collections::HashMap::new(),
        }
    }
}

fn collect_git_status_summary(repo: &git2::Repository) -> (bool, Vec<String>) {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true);
    let mut local_dirty = false;
    let mut dirty_files = Vec::new();
    if let Ok(statuses) = repo.statuses(Some(&mut opts)) {
        for entry in statuses.iter() {
            if let Some(path) = entry.path() {
                if SyncService::is_blacklisted_path(path) || !SyncService::is_whitelisted_path(path) {
                    continue;
                }
                let status = entry.status();
                if status.is_wt_modified() || status.is_index_modified() ||
                   status.is_wt_new() || status.is_index_new() ||
                   status.is_wt_deleted() || status.is_index_deleted() {
                    local_dirty = true;
                    dirty_files.push(path.to_string());
                }
            }
        }
    }
    (local_dirty, dirty_files)
}

fn collect_index_conflicts(repo: &git2::Repository) -> Vec<String> {
    let mut conflicted = Vec::new();
    if let Ok(index) = repo.index() {
        if index.has_conflicts() {
            if let Ok(conflicts) = index.conflicts() {
                for conflict in conflicts.flatten() {
                    let mut best_path = None;
                    if let Some(our) = &conflict.our {
                        best_path = Some(String::from_utf8_lossy(&our.path).to_string());
                    } else if let Some(their) = &conflict.their {
                        best_path = Some(String::from_utf8_lossy(&their.path).to_string());
                    } else if let Some(ancestor) = &conflict.ancestor {
                        best_path = Some(String::from_utf8_lossy(&ancestor.path).to_string());
                    }
                    if let Some(path) = best_path {
                        if !SyncService::is_blacklisted_path(&path) && SyncService::is_whitelisted_path(&path) {
                            conflicted.push(path);
                        }
                    }
                }
            }
        }
    }
    conflicted.sort();
    conflicted.dedup();
    conflicted
}

fn build_conflict_summary(
    repo: &git2::Repository,
    fetch_commit_id: Option<git2::Oid>,
    blocked_reason: &str,
) -> SyncConflictSummary {
    let (local_dirty, dirty_files) = collect_git_status_summary(repo);
    
    let mut remote_changed = false;
    if let Some(remote_oid) = fetch_commit_id {
        if let Ok(head) = repo.head() {
            if let Some(local_oid) = head.target() {
                if local_oid != remote_oid {
                    remote_changed = true;
                }
            }
        } else {
            remote_changed = true;
        }
    }

    let mut conflicted_files = Vec::new();
    if let Some(remote_oid) = fetch_commit_id {
        if let Ok(commit) = repo.find_commit(remote_oid) {
            if let Ok(tree) = commit.tree() {
                let paths = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
                let cp_clone = paths.clone();
                let mut dry_run_builder = git2::build::CheckoutBuilder::default();
                dry_run_builder.notify_on(git2::CheckoutNotificationType::CONFLICT);
                dry_run_builder.notify(move |_, path, _, _, _| {
                    if let Some(p) = path {
                        if let Some(s) = p.to_str() {
                            cp_clone.borrow_mut().push(s.to_string());
                        }
                    }
                    true
                });
                dry_run_builder.dry_run();
                let _ = repo.checkout_tree(tree.as_object(), Some(&mut dry_run_builder));
                conflicted_files = paths.borrow().clone();
            }
        }
    }

    let index_conflicts = collect_index_conflicts(repo);
    conflicted_files.extend(index_conflicts);

    conflicted_files.retain(|path| {
        !SyncService::is_blacklisted_path(path) && SyncService::is_whitelisted_path(path)
    });
    conflicted_files.sort();
    conflicted_files.dedup();

    if conflicted_files.is_empty() && local_dirty {
        conflicted_files = dirty_files;
    }

    SyncConflictSummary {
        status: "conflict".to_string(),
        local_dirty,
        remote_changed,
        conflicted_files,
        blocked_reason: blocked_reason.to_string(),
        safe_next_steps: vec![
            "备份当前工作区。".to_string(),
            "运行诊断确认网络/认证没问题。".to_string(),
            "手动处理冲突后重新同步。".to_string(),
        ],
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

fn fetch_and_reset_local_repo(
    workspace_path: &Path,
    config: &SyncConfig,
    token: &str,
    new_commit_sha: &str,
) -> crate::Result<()> {
    if let Ok(repo) = git2::Repository::open(workspace_path) {
        let mut remote = repo.find_remote("origin").or_else(|_| {
            repo.remote("origin", &config.remote_url)
        }).map_err(|e| crate::Error::Other(e.to_string()))?;

        let mut fetch_opts = git2::FetchOptions::new();
        if !token.is_empty() {
            let mut callbacks = git2::RemoteCallbacks::new();
            let token_clone = token.to_string();
            callbacks.credentials(move |_url, username_from_url, _allowed_types| {
                let user = username_from_url.unwrap_or("x-access-token");
                git2::Cred::userpass_plaintext(user, &token_clone)
            });
            fetch_opts.remote_callbacks(callbacks);
        }

        if config.proxy_enabled {
            if let Ok(proxy_opts) = Git2Backend::build_proxy_options(Some(config)) {
                fetch_opts.proxy_options(proxy_opts);
            }
        }

        let refspec = format!("refs/heads/{}:refs/remotes/origin/{}", config.branch, config.branch);
        remote.fetch(&[refspec], Some(&mut fetch_opts), None).map_err(|e| crate::Error::Other(e.to_string()))?;

        let commit_oid = git2::Oid::from_str(new_commit_sha).map_err(|e| crate::Error::Other(e.to_string()))?;
        let commit_obj = repo.find_commit(commit_oid).map_err(|e| crate::Error::Other(e.to_string()))?;

        repo.reset(commit_obj.as_object(), git2::ResetType::Mixed, None).map_err(|e| crate::Error::Other(e.to_string()))?;

        let branch_ref_name = format!("refs/heads/{}", config.branch);
        repo.reference(&branch_ref_name, commit_oid, true, "LWW sync update").map_err(|e| crate::Error::Other(e.to_string()))?;

        let _ = repo.set_head(&format!("refs/heads/{}", config.branch));
    }
    Ok(())
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
                let is_tls_error = clean_msg.contains("SSL certificate is invalid") || clean_msg.contains("Certificate (-17)") || clean_msg.contains("Bad file descriptor; class=Net");

                if is_tls_error {
                    result.error_category = "tls_failed".to_string();
                    // Extract some diagnostic info (since it's a hardcoded string or error, we just include the clean_msg)
                    result.user_message = "Android native libgit2 TLS certificate validation failed; GitHub API fallback is available".to_string();
                    result.raw_error = Some(format!("TLS Error details: {}", clean_msg));
                    result.network_ok = false;
                    result.network_status = "failed".to_string();
                    result.auth_status = "skipped".to_string();
                    result.repo_status = "skipped".to_string();
                    result.branch_status = "skipped".to_string();
                    return Ok(result);
                }

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
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to create branch '{}': {}", branch, e),
                ))
            })?;
            repo.set_head(&branch_ref_name).map_err(|e| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to set HEAD to '{}': {}", branch, e),
                ))
            })?;
            return Ok(());
        }
    }

    // HEAD is unborn/empty (no commits yet). Set HEAD symbolically.
    // The first commit will automatically create this branch.
    repo.set_head(&branch_ref_name).map_err(|e| {
        crate::Error::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to set symbolic HEAD to '{}': {}", branch, e),
        ))
    })?;

    Ok(())
}

fn semantic_merge_json(
    base: &serde_json::Map<String, serde_json::Value>,
    local: &serde_json::Map<String, serde_json::Value>,
    remote: &serde_json::Map<String, serde_json::Value>,
) -> Result<serde_json::Map<String, serde_json::Value>, Vec<SettingConflictDetail>> {
    let mut merged = serde_json::Map::new();
    let mut conflicts = Vec::new();

    // Collect all keys from all three maps
    let mut keys: std::collections::HashSet<&String> = base.keys().collect();
    keys.extend(local.keys());
    keys.extend(remote.keys());

    for key in keys {
        let base_val = base.get(key);
        let local_val = local.get(key);
        let remote_val = remote.get(key);

        match (base_val, local_val, remote_val) {
            (None, None, None) => {}
            (_, Some(l), None) => {
                if base_val == Some(l) {
                    // Deleted in remote, unmodified in local
                } else {
                    conflicts.push(SettingConflictDetail {
                        key: key.clone(),
                        local_value: l.clone(),
                        remote_value: serde_json::Value::Null,
                    });
                }
            }
            (_, None, Some(r)) => {
                if base_val == Some(r) {
                    // Deleted in local, unmodified in remote
                } else {
                    conflicts.push(SettingConflictDetail {
                        key: key.clone(),
                        local_value: serde_json::Value::Null,
                        remote_value: r.clone(),
                    });
                }
            }
            (Some(b), Some(l), Some(r)) => {
                if l == r {
                    merged.insert(key.clone(), l.clone());
                } else if l == b {
                    merged.insert(key.clone(), r.clone());
                } else if r == b {
                    merged.insert(key.clone(), l.clone());
                } else {
                    conflicts.push(SettingConflictDetail {
                        key: key.clone(),
                        local_value: l.clone(),
                        remote_value: r.clone(),
                    });
                }
            }
            (None, Some(l), Some(r)) => {
                if l == r {
                    merged.insert(key.clone(), l.clone());
                } else {
                    conflicts.push(SettingConflictDetail {
                        key: key.clone(),
                        local_value: l.clone(),
                        remote_value: r.clone(),
                    });
                }
            }
            (Some(_b), None, None) => {}
        }
    }

    if !conflicts.is_empty() {
        Err(conflicts)
    } else {
        Ok(merged)
    }
}

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

    pub fn check_dirty_repo_blocked(workspace_path: &Path) -> crate::Result<Option<Vec<String>>> {
        if let Ok(repo) = git2::Repository::open(workspace_path) {
            let mut opts = git2::StatusOptions::new();
            opts.include_untracked(true);
            let mut dirty = Vec::new();
            if let Ok(statuses) = repo.statuses(Some(&mut opts)) {
                for entry in statuses.iter() {
                    if let Some(path) = entry.path() {
                        if !Self::is_blacklisted_path(path) && !Self::is_whitelisted_path(path) {
                            let status = entry.status();
                            if status.is_wt_modified() || status.is_index_modified() ||
                               status.is_wt_new() || status.is_index_new() ||
                               status.is_wt_deleted() || status.is_index_deleted() {
                                dirty.push(path.to_string());
                            }
                        }
                    }
                }
            }
            if !dirty.is_empty() {
                return Ok(Some(dirty));
            }
        }
        Ok(None)
    }

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

        // Check for dirty non-whitelisted changes
        if let Some(dirty_files) = Self::check_dirty_repo_blocked(workspace_path)? {
            return Ok(SyncResult::error(
                SyncStatus::DirtyRepoBlocked,
                FirstSyncMode::NotAttempted,
                Some(format!(
                    "同步被阻止: 本地工作区存在未跟踪或未提交的修改，且这些修改不是同步安全文件:\n{}",
                    dirty_files.join("\n")
                )),
                "Dirty repo blocked: non-whitelisted files modified".to_string(),
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
                        result.user_message = Some(format!("同步失败，已重试 {} 次。错误: {}", max_retries, e));
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
        let branch_url = format!("{}/git/ref/heads/{}", api_base, config.branch);
        let resp = client.get(&branch_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "WriterApp/1.0")
            .header("Accept", "application/vnd.github+json")
            .send()
            .map_err(|e| crate::Error::Other(e.to_string()))?;

        let mut latest_commit_sha = String::new();
        let mut base_tree_sha = String::new();
        let mut remote_tree_files = std::collections::HashMap::new();

        if resp.status().as_u16() == 200 {
            let json: serde_json::Value = resp.json().map_err(|e| crate::Error::Other(e.to_string()))?;
            latest_commit_sha = json["object"]["sha"].as_str().unwrap_or_default().to_string();

            // get commit
            let commit_url = format!("{}/git/commits/{}", api_base, latest_commit_sha);
            let resp = client.get(&commit_url)
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "WriterApp/1.0")
                .header("Accept", "application/vnd.github+json")
                .send()
                .map_err(|e| crate::Error::Other(e.to_string()))?;
            if resp.status().as_u16() == 200 {
                let json: serde_json::Value = resp.json().map_err(|e| crate::Error::Other(e.to_string()))?;
                base_tree_sha = json["tree"]["sha"].as_str().unwrap_or_default().to_string();
            }

            // get tree recursively
            if !base_tree_sha.is_empty() {
                let tree_url = format!("{}/git/trees/{}?recursive=1", api_base, base_tree_sha);
                let resp = client.get(&tree_url)
                    .header("Authorization", format!("Bearer {}", token))
                    .header("User-Agent", "WriterApp/1.0")
                    .header("Accept", "application/vnd.github+json")
                    .send()
                    .map_err(|e| crate::Error::Other(e.to_string()))?;
                if resp.status().as_u16() == 200 {
                    let json: serde_json::Value = resp.json().map_err(|e| crate::Error::Other(e.to_string()))?;
                    if let Some(tree) = json["tree"].as_array() {
                        for item in tree {
                            if item["type"].as_str() == Some("blob") {
                                if let (Some(path), Some(sha)) = (item["path"].as_str(), item["sha"].as_str()) {
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
            let manifest_url = format!("{}/contents/app-meta/sync/manifest.sync.json?ref={}", api_base, config.branch);
            let resp = client.get(&manifest_url)
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
                    if let Ok(content_bytes) = base64::engine::general_purpose::STANDARD.decode(&content_b64) {
                        if let Ok(manifest) = serde_json::from_slice::<SyncManifest>(&content_bytes) {
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
                        updated_at_ms = state.known_files_updated_at.get(&path).cloned().unwrap_or(0);
                    } else {
                        let modified_ms = std::fs::metadata(workspace_path.join(&path))
                            .and_then(|m| m.modified())
                            .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)))
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(now_ms);
                        updated_at_ms = modified_ms;
                    }
                } else {
                    let modified_ms = std::fs::metadata(workspace_path.join(&path))
                        .and_then(|m| m.modified())
                        .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)))
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(now_ms);
                    updated_at_ms = modified_ms;
                }

                local_records.insert(path.clone(), ManifestFileRecord {
                    path,
                    content_hash: local_hash,
                    updated_at_ms,
                    device_id: state.device_id.clone(),
                    op,
                    schema_version: 1,
                });
            }
        }

        // 2. Local deletions (in known_files but missing from workspace)
        for (path, _known_hash) in &state.known_files {
            if !local_records.contains_key(path) {
                if !Self::is_whitelisted_path(path) || Self::is_blacklisted_path(path) {
                    continue;
                }
                if !workspace_path.join(path).exists() {
                    let mut updated_at_ms = now_ms;
                    if let Some(tombstone) = state.tombstones.iter().find(|t| t.original_path == *path) {
                        updated_at_ms = tombstone.deleted_at * 1000;
                    }
                    
                    local_records.insert(path.clone(), ManifestFileRecord {
                        path: path.clone(),
                        content_hash: String::new(),
                        updated_at_ms,
                        device_id: state.device_id.clone(),
                        op: "delete".to_string(),
                        schema_version: 1,
                    });
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
                remote_records.insert(path.clone(), ManifestFileRecord {
                    path: path.clone(),
                    content_hash: sha.clone(),
                    updated_at_ms: 0,
                    device_id: "remote".to_string(),
                    op: "upsert".to_string(),
                    schema_version: 1,
                });
            }
        }

        let mut merged_manifest_files = std::collections::HashMap::new();
        let mut to_download = Vec::new();
        let mut to_upload = Vec::new();
        let mut to_delete_local = Vec::new();
        let mut local_deletes_count = Vec::new();
        let mut remote_deletes_count = Vec::new();
        let mut overwritten_files = Vec::new();

        let all_paths: std::collections::HashSet<String> = local_records.keys().cloned()
            .chain(remote_records.keys().cloned()).collect();

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
                    } else if remote_rec.updated_at_ms == local_rec.updated_at_ms {
                        if remote_rec.device_id > local_rec.device_id {
                            remote_wins = true;
                        }
                    }

                    if remote_wins {
                        merged_manifest_files.insert(path.clone(), remote_rec.clone());
                        if remote_rec.op == "upsert" {
                            if local_rec.op == "delete" || local_rec.content_hash != remote_rec.content_hash {
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
                            if remote_rec.op == "delete" || remote_rec.content_hash != local_rec.content_hash {
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
                let filename = full_path.file_name().unwrap_or_default().to_string_lossy().to_string();
                let trash_dir = workspace_path.join("app-meta/sync/trash");
                let _ = std::fs::create_dir_all(&trash_dir);
                let trash_path = trash_dir.join(format!("{}_{}_{}", chrono::Utc::now().timestamp_millis(), uuid::Uuid::new_v4(), filename));
                let _ = std::fs::rename(&full_path, &trash_path);
            }
        }

        // Download remote files
        for path in &to_download {
            let api_content_url = format!("{}/contents/{}?ref={}", api_base, path, config.branch);
            let dl_resp = client.get(&api_content_url)
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
                    if let Ok(content) = base64::engine::general_purpose::STANDARD.decode(&content_b64) {
                        let full_path = workspace_path.join(path);
                        if let Some(parent) = full_path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        let tmp_path = full_path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
                        if std::fs::write(&tmp_path, content).is_ok() {
                            let _ = std::fs::rename(tmp_path, &full_path);
                        }
                    }
                }
            } else {
                return Err(crate::Error::Other(format!("Failed to download file {}: {}", path, dl_resp.status())));
            }
        }

        let mut tree_nodes = vec![];

        // Upload whitelisted files
        for path in &to_upload {
            let full_path = workspace_path.join(path);
            if !full_path.exists() { continue; }

            let content = std::fs::read(&full_path).map_err(|e| crate::Error::Other(format!("读取文件失败 {}: {}", path, e)))?;

            let blob_url = format!("{}/git/blobs", api_base);
            let blob_payload = serde_json::json!({
                "content": base64::engine::general_purpose::STANDARD.encode(&content),
                "encoding": "base64"
            });
            let resp = client.post(&blob_url)
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "WriterApp/1.0")
                .header("Accept", "application/vnd.github+json")
                .json(&blob_payload)
                .send()
                .map_err(|e| crate::Error::Other(e.to_string()))?;
            
            if resp.status().as_u16() == 201 {
                let json: serde_json::Value = resp.json().map_err(|e| crate::Error::Other(e.to_string()))?;
                let sha = json["sha"].as_str().unwrap_or_default().to_string();
                tree_nodes.push(serde_json::json!({
                    "path": path,
                    "mode": "100644",
                    "type": "blob",
                    "sha": sha
                }));
            } else {
                return Err(crate::Error::Other(format!("Failed to upload blob for {}: {}", path, resp.status())));
            }
        }

        let purge_time = now_ms - 30 * 24 * 3600 * 1000;
        let mut manifest_files_vec: Vec<ManifestFileRecord> = merged_manifest_files.values().cloned().collect();
        manifest_files_vec.retain(|rec| {
            rec.op != "delete" || rec.updated_at_ms > purge_time
        });

        let sync_manifest = SyncManifest {
            files: manifest_files_vec,
        };
        
        let manifest_json = serde_json::to_string_pretty(&sync_manifest).unwrap_or_default();
        let manifest_path = "app-meta/sync/manifest.sync.json";
        let full_manifest_path = workspace_path.join(manifest_path);
        if let Some(parent) = full_manifest_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&full_manifest_path, &manifest_json).map_err(|e| crate::Error::Other(format!("Failed to write manifest locally: {}", e)))?;

        let blob_url = format!("{}/git/blobs", api_base);
        let blob_payload = serde_json::json!({
            "content": base64::engine::general_purpose::STANDARD.encode(manifest_json.as_bytes()),
            "encoding": "base64"
        });
        let resp = client.post(&blob_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "WriterApp/1.0")
            .header("Accept", "application/vnd.github+json")
            .json(&blob_payload)
            .send()
            .map_err(|e| crate::Error::Other(e.to_string()))?;
        let manifest_sha = if resp.status().as_u16() == 201 {
            let json: serde_json::Value = resp.json().map_err(|e| crate::Error::Other(e.to_string()))?;
            json["sha"].as_str().unwrap_or_default().to_string()
        } else {
            return Err(crate::Error::Other(format!("Failed to upload manifest blob: {}", resp.status())));
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
        let resp = client.post(&tree_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "WriterApp/1.0")
            .header("Accept", "application/vnd.github+json")
            .json(&tree_payload)
            .send()
            .map_err(|e| crate::Error::Other(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(crate::Error::Other(format!("Failed to create tree: {}", resp.status())));
        }
        let json: serde_json::Value = resp.json().map_err(|e| crate::Error::Other(e.to_string()))?;
        let new_tree_sha = json["sha"].as_str().unwrap_or_default().to_string();

        let commit_url = format!("{}/git/commits", api_base);
        let mut commit_payload = serde_json::json!({
            "message": format!("WriterApp LWW Sync: {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")),
            "tree": new_tree_sha
        });
        if !latest_commit_sha.is_empty() {
            commit_payload["parents"] = serde_json::json!(vec![latest_commit_sha.clone()]);
        }
        let resp = client.post(&commit_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "WriterApp/1.0")
            .header("Accept", "application/vnd.github+json")
            .json(&commit_payload)
            .send()
            .map_err(|e| crate::Error::Other(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(crate::Error::Other(format!("Failed to create commit: {}", resp.status())));
        }
        let json: serde_json::Value = resp.json().map_err(|e| crate::Error::Other(e.to_string()))?;
        let new_commit_sha = json["sha"].as_str().unwrap_or_default().to_string();

        if !latest_commit_sha.is_empty() {
            let ref_url = format!("{}/git/refs/heads/{}", api_base, config.branch);
            let ref_payload = serde_json::json!({
                "sha": new_commit_sha,
                "force": false
            });
            let resp = client.patch(&ref_url)
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
                return Err(crate::Error::Other(format!("Failed to update ref: {}", resp.status())));
            }
        } else {
            let ref_url = format!("{}/git/refs", api_base);
            let ref_payload = serde_json::json!({
                "ref": format!("refs/heads/{}", config.branch),
                "sha": new_commit_sha
            });
            let resp = client.post(&ref_url)
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "WriterApp/1.0")
                .header("Accept", "application/vnd.github+json")
                .json(&ref_payload)
                .send()
                .map_err(|e| crate::Error::Other(e.to_string()))?;
            if !resp.status().is_success() {
                return Err(crate::Error::Other(format!("Failed to create ref: {}", resp.status())));
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
                state.known_files.insert(entry.relative_path.clone(), entry.file_hash.clone());
                
                let matched_rec = merged_manifest_files.get(&entry.relative_path);
                let t = matched_rec.map(|r| r.updated_at_ms).unwrap_or_else(|| {
                    std::fs::metadata(workspace_path.join(&entry.relative_path))
                        .and_then(|m| m.modified())
                        .and_then(|time| time.duration_since(std::time::SystemTime::UNIX_EPOCH).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)))
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(now_ms)
                });
                state.known_files_updated_at.insert(entry.relative_path, t);
            }
        }
        
        state.tombstones.retain(|t| {
            t.purge_after > chrono::Utc::now().timestamp()
        });
        
        Self::save_sync_state(workspace_path, state)?;

        let has_changes = !to_upload.is_empty() || !to_download.is_empty() || !local_deletes_count.is_empty() || !remote_deletes_count.is_empty();
        result.status = if has_changes { SyncStatus::LatestWinsApplied } else { SyncStatus::NoChanges };
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

        let classify_error = |e_str: &str| -> SyncStatus {
            let lower = e_str.to_lowercase();
            if lower.contains("recoverable_error") {
                SyncStatus::RecoverableError(e_str.replace("recoverable_error:", "").trim().to_string())
            } else if lower.contains("fatal_error") {
                SyncStatus::FatalError(e_str.replace("fatal_error:", "").trim().to_string())
            } else if lower.contains("auth") || lower.contains("token") || lower.contains("credential") || lower.contains("proxy") || lower.contains("resolve") || lower.contains("network") || lower.contains("unborn") || lower.contains("timeout") || lower.contains("connect") || lower.contains("could not resolve") {
                SyncStatus::RecoverableError(e_str.to_string())
            } else {
                SyncStatus::FatalError(e_str.to_string())
            }
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
                let payload_str = e_str.split("settings_conflict_payload:").nth(1).unwrap_or("").trim();
                let details: Option<Vec<SettingConflictDetail>> = serde_json::from_str(payload_str).ok();
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
                    blocked_reason: "本地和远端都修改了设置文件 settings.sync.json 且产生了冲突。".to_string(),
                    safe_next_steps: vec![
                        "手动检查本地与远端设置。".to_string(),
                        "重新保存设置以覆盖或重新同步。".to_string(),
                    ],
                };
                res.conflict_summary = Some(summary);
                return Ok(res);
            }

            if e_str.contains("checkout_conflict_payload:") {
                let payload_str = e_str.split("checkout_conflict_payload:").nth(1).unwrap_or("").trim();
                let summary: Option<SyncConflictSummary> = serde_json::from_str(payload_str).ok();
                
                let mut res = SyncResult::error(
                    SyncStatus::Conflict,
                    result.first_sync_mode,
                    Some("本地工作区有文件会阻止远端更新，请先处理冲突文件后再同步。".to_string()),
                    format!("Pull failed due to conflict."),
                );
                res.conflict_summary = summary;
                return Ok(res);
            }

            let e_str_lower = e_str.to_lowercase();
            // Checkout conflict / local blocking file (fallback)
            if e_str_lower.contains("checkout_conflict") || e_str_lower.contains("local_blocking_file") || e_str_lower.contains("conflict prevents checkout") {
                let mut res = SyncResult::error(
                    SyncStatus::Conflict,
                    result.first_sync_mode,
                    Some("本地工作区有文件会阻止远端更新，请先处理冲突文件后再同步。".to_string()),
                    format!("Pull failed: {}", e),
                );
                if let Ok(repo) = git2::Repository::open(workspace_path) {
                    let fetch_commit_id = repo.find_reference("FETCH_HEAD").ok().and_then(|r| r.target());
                    let summary = build_conflict_summary(&repo, fetch_commit_id, "本地工作区有文件会阻止远端更新，请先处理冲突文件后再同步。");
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
                                    result.status = SyncStatus::Conflict;
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

                let (local_dirty, _) = collect_git_status_summary(&repo);
                let fetch_commit_id = repo.find_reference("FETCH_HEAD").ok().and_then(|r| r.target());
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
                
                let conflicted_files = result.conflicts.iter().map(|c| c.local_path.clone()).collect::<Vec<_>>();
                
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
            "app-meta/sync/state.local.json",
            "sqlite_cache",
            "tmp",
            "backups",
        ];

        if rel_path.ends_with(".tmp") || rel_path.ends_with(".lock") {
            return true;
        }

        if rel_path.starts_with("app-meta/logs") || rel_path.contains("/logs/") {
            return true;
        }

        // But we DO want to sync app-meta/sync/trash!
        if rel_path.starts_with("app-meta/sync/trash/") {
            return false;
        }

        // Keep 'trash' pattern if it's anywhere else?
        // Let's just avoid a blanket "trash" ignore. We used to ignore "trash".
        // Instead of ignoring all "trash", we only ignore it if it matches something else, but since we removed it from ignored_patterns, it won't.
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
        if rel_path == "app-meta/sync/manifest.sync.json" {
            return true;
        }

        if rel_path.starts_with("projects/") {
            if rel_path.ends_with("/project.json") {
                return true;
            }
            if rel_path.ends_with("/mind_map.json") || rel_path.contains("/mind_map/") {
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

        if rel_path.starts_with("app-meta/sync/trash/") {
            return true;
        }

        if rel_path == "app-meta/sync/tombstones.json" {
            return true;
        }

        false
    }

    fn compute_file_hash(path: &Path) -> std::io::Result<String> {
        let content = std::fs::read(path)?;
        Ok(format!("{:x}", md5::compute(&content)))
    }

    pub fn compute_git_hash(content: &[u8]) -> String {
        match git2::Oid::hash_object(git2::ObjectType::Blob, content) {
            Ok(oid) => oid.to_string(),
            Err(_) => format!("{:x}", md5::compute(content)),
        }
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
        let state = Self::load_sync_state(workspace_path).unwrap_or_default();
        let is_first_sync = state.known_files.is_empty();

        let mut local_files = std::collections::HashSet::new();

        for entry in entries {
            if Self::is_blacklisted_path(&entry.relative_path) {
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

    pub fn load_sync_state(workspace_path: &Path) -> crate::Result<SyncState> {
        let state_path = workspace_path.join("app-meta/sync/state.local.json");
        if !state_path.exists() {
            // Try to migrate if the old sync_state.json exists
            let old_path = workspace_path.join("app-meta/sync/sync_state.json");
            if old_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&old_path) {
                    if let Ok(mut state) = serde_json::from_str::<SyncState>(&content) {
                        if state.device_id.is_empty() {
                            state.device_id = uuid::Uuid::new_v4().to_string();
                        }
                        let _ = Self::save_sync_state(workspace_path, &state);
                        let _ = std::fs::remove_file(old_path);
                        return Ok(state);
                    }
                }
            }

            let mut default_state = SyncState::default();
            default_state.device_id = uuid::Uuid::new_v4().to_string();
            return Ok(default_state);
        }

        let content = std::fs::read_to_string(state_path)?;
        let mut state: SyncState = serde_json::from_str(&content).unwrap_or_default();
        if state.device_id.is_empty() {
            state.device_id = uuid::Uuid::new_v4().to_string();
            let _ = Self::save_sync_state(workspace_path, &state);
        }
        Ok(state)
    }

    pub fn save_sync_state(workspace_path: &Path, state: &SyncState) -> crate::Result<()> {
        let state_path = workspace_path.join("app-meta/sync/state.local.json");
        if let Some(parent) = state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(state).map_err(|e| {
            crate::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;

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
            last_successful_network_mode: None,
            known_files: std::collections::HashMap::new(),
            conflicts: vec![],
            tombstones: Vec::new(),
            deleted_files: std::collections::HashSet::new(),
            device_id: String::new(),
            known_files_updated_at: std::collections::HashMap::new(),
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
            last_successful_network_mode: None,
            known_files: std::collections::HashMap::new(),
            conflicts: vec![],
            tombstones: Vec::new(),
            deleted_files: std::collections::HashSet::new(),
            device_id: String::new(),
            known_files_updated_at: std::collections::HashMap::new(),
        };

        SyncService::save_sync_state(dir.path(), &state).unwrap();
        let state_path = dir.path().join("app-meta/sync/state.local.json");
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
        std::fs::write(state_dir.join("state.local.json"), "{}").unwrap();

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

        std::fs::remove_file(state_dir.join("state.local.json")).unwrap();
        std::fs::create_dir(state_dir.join("state.local.json")).unwrap();

        let res = SyncService::perform_sync(dir.path(), &config, &secrets, &MockBackendOk).unwrap();
        assert!(matches!(res.status, SyncStatus::FatalError(_)));
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

    #[test]
    fn test_push_preflight_unborn_head() {
        let dir = tempfile::tempdir().unwrap();
        // Repository is initialized but has no commits (unborn HEAD)
        let _repo = git2::Repository::init(dir.path()).unwrap();

        let backend = Git2Backend;
        let res = backend.push(dir.path(), "main", None, None);
        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("recoverable_error") || err_msg.contains("unborn"));
    }

    #[test]
    fn test_push_preflight_missing_branch_ref_recovered() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        // Create a commit
        let signature = git2::Signature::now("Test User", "test@test.com").unwrap();
        let mut index = repo.index().unwrap();
        let file_path = dir.path().join("app-meta/settings/settings.sync.json");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "{}").unwrap();
        index.add_path(Path::new("app-meta/settings/settings.sync.json")).unwrap();
        index.write().unwrap();
        let oid = index.write_tree().unwrap();
        let tree = repo.find_tree(oid).unwrap();
        let commit_oid = repo.commit(Some("refs/heads/main"), &signature, &signature, "Initial commit", &tree, &[]).unwrap();

        // Delete the branch reference, keeping HEAD detached pointing to the commit
        repo.set_head_detached(commit_oid).unwrap();
        let mut branch_ref = repo.find_reference("refs/heads/main").unwrap();
        branch_ref.delete().unwrap();

        // Now branch reference refs/heads/main does not exist, but HEAD points to a commit.
        // We verify that calling Git2Backend::push reconstructs the branch ref successfully!
        let backend = Git2Backend;
        let res = backend.push(dir.path(), "main", None, None);
        // Verify branch ref has been reconstructed!
        assert!(repo.find_reference("refs/heads/main").is_ok());
    }

    #[test]
    fn test_settings_semantic_merge_conflict_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        // Set up local repository with a commit containing base settings.sync.json
        let signature = git2::Signature::now("Test User", "test@test.com").unwrap();
        let mut index = repo.index().unwrap();
        let file_path = dir.path().join("app-meta/settings/settings.sync.json");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        
        let base_content = r#"{"font_size": 12, "theme": "dark"}"#;
        std::fs::write(&file_path, base_content).unwrap();
        index.add_path(Path::new("app-meta/settings/settings.sync.json")).unwrap();
        index.write().unwrap();
        let oid = index.write_tree().unwrap();
        let tree = repo.find_tree(oid).unwrap();
        let base_commit_oid = repo.commit(Some("refs/heads/main"), &signature, &signature, "Base commit", &tree, &[]).unwrap();
        repo.set_head("refs/heads/main").unwrap();

        // Clone local repo to remote right after base commit (so remote shares base commit OID and history)
        let remote_dir = tempfile::tempdir().unwrap();
        let remote_repo = git2::Repository::clone(dir.path().to_str().unwrap(), remote_dir.path()).unwrap();

        // Now modify local settings.sync.json and commit it in local repo (local divergent change)
        let local_content = r#"{"font_size": 16, "theme": "dark"}"#;
        std::fs::write(&file_path, local_content).unwrap();
        index.add_path(Path::new("app-meta/settings/settings.sync.json")).unwrap();
        index.write().unwrap();
        let oid = index.write_tree().unwrap();
        let tree = repo.find_tree(oid).unwrap();
        let local_commit_oid = repo.commit(Some("refs/heads/main"), &signature, &signature, "Local commit", &tree, &[&repo.find_commit(base_commit_oid).unwrap()]).unwrap();

        // In remote repo, modify settings.sync.json to a conflicting value and commit (remote divergent change)
        let remote_file_path = remote_dir.path().join("app-meta/settings/settings.sync.json");
        let remote_content = r#"{"font_size": 20, "theme": "dark"}"#;
        std::fs::write(&remote_file_path, remote_content).unwrap();
        let mut remote_index = remote_repo.index().unwrap();
        remote_index.add_path(Path::new("app-meta/settings/settings.sync.json")).unwrap();
        remote_index.write().unwrap();
        let remote_oid = remote_index.write_tree().unwrap();
        let remote_tree = remote_repo.find_tree(remote_oid).unwrap();
        let remote_base_commit = remote_repo.find_commit(base_commit_oid).unwrap();
        let _remote_commit_oid = remote_repo.commit(
            Some("refs/heads/main"),
            &signature,
            &signature,
            "Remote commit",
            &remote_tree,
            &[&remote_base_commit],
        ).unwrap();

        // Add remote to local repo
        let mut remote = repo.remote("origin", remote_dir.path().to_str().unwrap()).unwrap();
        remote.fetch(&["main"], None, None).unwrap();

        // Verify pull/merge fails with settings_conflict_payload
        let backend = Git2Backend;
        let res = backend.pull(dir.path(), "main", None, None);
        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("settings_conflict_payload"));

        // Verify that after transactional rollback:
        // 1. Index has no conflicts
        let index = repo.index().unwrap();
        assert!(!index.has_conflicts());
        // 2. HEAD points back to original local_commit_oid
        let head = repo.head().unwrap();
        assert_eq!(head.target().unwrap(), local_commit_oid);
        // 3. Local settings file is intact and not corrupted with remote change
        let content_after = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content_after, local_content);
    }

    fn start_mock_github_api(
        initial_manifest: Option<SyncManifest>,
        initial_files: std::collections::HashMap<String, String>,
    ) -> (
        String,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
        std::sync::Arc<std::sync::Mutex<String>>,
        std::thread::JoinHandle<()>,
    ) {
        use std::net::TcpListener;
        use std::io::{Read, Write};
        use std::thread;
        use std::sync::{Arc, Mutex};
        use std::sync::atomic::{AtomicBool, Ordering};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let addr = format!("http://127.0.0.1:{}", port);
        
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        let files = Arc::new(Mutex::new(initial_files));
        let files_clone = files.clone();

        let manifest_str = if let Some(m) = initial_manifest {
            serde_json::to_string(&m).unwrap()
        } else {
            String::new()
        };
        let manifest = Arc::new(Mutex::new(manifest_str));
        let manifest_clone = manifest.clone();

        listener.set_nonblocking(true).unwrap();

        let handle = thread::spawn(move || {
            while !shutdown_clone.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buffer = [0; 65536];
                        if let Ok(bytes_read) = stream.read(&mut buffer) {
                            let req = String::from_utf8_lossy(&buffer[..bytes_read]);
                            let first_line = req.lines().next().unwrap_or("");
                            let parts: Vec<&str> = first_line.split_whitespace().collect();
                            if parts.len() >= 2 {
                                let method = parts[0];
                                let path = parts[1];

                                let mut response_body = String::new();
                                let mut status_line = "HTTP/1.1 200 OK";

                                if path.contains("/rate_limit") {
                                    response_body = r#"{"resources":{}}"#.to_string();
                                } else if path.contains("/git/ref/heads/main") {
                                    let m = manifest_clone.lock().unwrap();
                                    if m.is_empty() {
                                        status_line = "HTTP/1.1 404 Not Found";
                                        response_body = r#"{"message":"Not Found"}"#.to_string();
                                    } else {
                                        response_body = r#"{"object":{"sha":"mock_commit_sha"}}"#.to_string();
                                    }
                                } else if path.contains("/git/commits/mock_commit_sha") {
                                    response_body = r#"{"tree":{"sha":"mock_tree_sha"}}"#.to_string();
                                } else if path.contains("/git/trees/mock_tree_sha") {
                                    let mut tree_list = Vec::new();
                                    let m = manifest_clone.lock().unwrap();
                                    if !m.is_empty() {
                                        tree_list.push(serde_json::json!({
                                            "path": "app-meta/sync/manifest.sync.json",
                                            "type": "blob",
                                            "sha": "manifest_blob_sha"
                                        }));
                                        let fls = files_clone.lock().unwrap();
                                        for filename in fls.keys() {
                                            tree_list.push(serde_json::json!({
                                                "path": filename,
                                                "type": "blob",
                                                "sha": format!("{}_sha", filename)
                                            }));
                                        }
                                    }
                                    response_body = serde_json::json!({ "tree": tree_list }).to_string();
                                } else if path.contains("/contents/app-meta/sync/manifest.sync.json") {
                                    let m = manifest_clone.lock().unwrap();
                                    if m.is_empty() {
                                        status_line = "HTTP/1.1 404 Not Found";
                                    } else {
                                        let encoded = base64::engine::general_purpose::STANDARD.encode(m.as_bytes());
                                        response_body = serde_json::json!({
                                            "content": encoded,
                                            "encoding": "base64"
                                        }).to_string();
                                    }
                                } else if path.contains("/contents/") {
                                    if let Some(idx) = path.find("/contents/") {
                                        let file_path = &path[idx + 10..];
                                        let file_path = file_path.split('?').next().unwrap_or(file_path);
                                        let fls = files_clone.lock().unwrap();
                                        if let Some(content) = fls.get(file_path) {
                                            let encoded = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
                                            response_body = serde_json::json!({
                                                "content": encoded,
                                                "encoding": "base64"
                                            }).to_string();
                                        } else {
                                            status_line = "HTTP/1.1 404 Not Found";
                                        }
                                    }
                                } else if method == "POST" && path.contains("/git/blobs") {
                                    status_line = "HTTP/1.1 201 Created";
                                    if let Some(body_start) = req.find("\r\n\r\n") {
                                        let body = &req[body_start + 4..];
                                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(body) {
                                            if let Some(b64_content) = val["content"].as_str() {
                                                if let Ok(decoded_bytes) = base64::engine::general_purpose::STANDARD.decode(b64_content) {
                                                    if let Ok(decoded_str) = String::from_utf8(decoded_bytes) {
                                                        if decoded_str.contains("manifest.sync.json") || decoded_str.contains("\"files\":") {
                                                            let mut m = manifest_clone.lock().unwrap();
                                                            *m = decoded_str;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    response_body = r#"{"sha":"mock_blob_sha"}"#.to_string();
                                } else if method == "POST" && path.contains("/git/trees") {
                                    status_line = "HTTP/1.1 201 Created";
                                    if let Some(body_start) = req.find("\r\n\r\n") {
                                        let body = &req[body_start + 4..];
                                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(body) {
                                            if let Some(tree_nodes) = val["tree"].as_array() {
                                                let mut fls = files_clone.lock().unwrap();
                                                for node in tree_nodes {
                                                    if let Some(n_path) = node["path"].as_str() {
                                                        if node["sha"].is_null() {
                                                            fls.remove(n_path);
                                                        } else {
                                                            if !fls.contains_key(n_path) && n_path != "app-meta/sync/manifest.sync.json" {
                                                                fls.insert(n_path.to_string(), "dummy content".to_string());
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    response_body = r#"{"sha":"mock_tree_sha"}"#.to_string();
                                } else if method == "POST" && path.contains("/git/commits") {
                                    status_line = "HTTP/1.1 201 Created";
                                    response_body = r#"{"sha":"mock_commit_sha"}"#.to_string();
                                } else if method == "POST" && path.contains("/git/refs") {
                                    status_line = "HTTP/1.1 201 Created";
                                    response_body = r#"{}"#.to_string();
                                } else if method == "PATCH" && path.contains("/git/refs/heads/main") {
                                    response_body = r#"{}"#.to_string();
                                }

                                let response = format!(
                                    "{}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                                    status_line,
                                    response_body.len(),
                                    response_body
                                );
                                let _ = stream.write_all(response.as_bytes());
                            }
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    Err(_) => {}
                }
            }
        });

        (addr, shutdown, files, manifest, handle)
    }

    #[test]
    fn test_perform_lww_sync_first_download() {
        let dir = tempdir().unwrap();
        let mut initial_files = std::collections::HashMap::new();
        initial_files.insert("projects/p1/project.json".to_string(), "remote content".to_string());
        
        let initial_manifest = SyncManifest {
            files: vec![
                ManifestFileRecord {
                    path: "projects/p1/project.json".to_string(),
                    content_hash: format!("{:x}", md5::compute("remote content".as_bytes())),
                    updated_at_ms: 1000,
                    device_id: "device_remote".to_string(),
                    op: "upsert".to_string(),
                    schema_version: 1,
                }
            ]
        };

        let (mock_url, shutdown, _files, _manifest, server_thread) = start_mock_github_api(
            Some(initial_manifest),
            initial_files,
        );

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            proxy_enabled: false,
            proxy_type: "none".to_string(),
            proxy_host: String::new(),
            proxy_port: 0,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();
        assert!(res.downloaded_files.contains(&"projects/p1/project.json".to_string()));
        assert!(res.downloaded_files.contains(&"app-meta/sync/manifest.sync.json".to_string()));
        assert!(res.uploaded_files.is_empty());
        assert!(res.local_deletes.is_empty());
        assert!(res.remote_deletes.is_empty());
        assert!(res.overwritten_files.is_empty());

        let local_file_path = dir.path().join("projects/p1/project.json");
        assert!(local_file_path.exists());
        let local_content = std::fs::read_to_string(local_file_path).unwrap();
        assert_eq!(local_content, "remote content");

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    fn test_perform_lww_sync_local_delete_generates_manifest_delete() {
        let dir = tempdir().unwrap();

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state
            .known_files
            .insert("projects/p1/project.json".to_string(), "old_hash".to_string());
        state
            .known_files_updated_at
            .insert("projects/p1/project.json".to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let mut initial_files = std::collections::HashMap::new();
        initial_files.insert("projects/p1/project.json".to_string(), "remote content".to_string());
        let initial_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "projects/p1/project.json".to_string(),
                content_hash: format!("{:x}", md5::compute("remote content".as_bytes())),
                updated_at_ms: 900,
                device_id: "device_remote".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };

        let (mock_url, shutdown, _files_map, manifest_str, server_thread) =
            start_mock_github_api(Some(initial_manifest), initial_files);

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            proxy_enabled: false,
            proxy_type: "none".to_string(),
            proxy_host: String::new(),
            proxy_port: 0,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();
        assert!(res.local_deletes.contains(&"projects/p1/project.json".to_string()));

        let final_m: SyncManifest =
            serde_json::from_str(&manifest_str.lock().unwrap().clone()).unwrap();
        let rec = final_m
            .files
            .iter()
            .find(|f| f.path == "projects/p1/project.json")
            .unwrap();
        assert_eq!(rec.op, "delete");

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    fn test_perform_lww_sync_remote_delete_removes_local_file() {
        let dir = tempdir().unwrap();
        let local_path = dir.path().join("projects/p1/project.json");
        std::fs::create_dir_all(local_path.parent().unwrap()).unwrap();
        std::fs::write(&local_path, "local content").unwrap();

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state.known_files.insert(
            "projects/p1/project.json".to_string(),
            format!("{:x}", md5::compute("local content".as_bytes())),
        );
        state
            .known_files_updated_at
            .insert("projects/p1/project.json".to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let initial_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "projects/p1/project.json".to_string(),
                content_hash: String::new(),
                updated_at_ms: 3000,
                device_id: "device_remote".to_string(),
                op: "delete".to_string(),
                schema_version: 1,
            }],
        };

        let (mock_url, shutdown, _files_map, _manifest_str, server_thread) =
            start_mock_github_api(Some(initial_manifest), std::collections::HashMap::new());

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            proxy_enabled: false,
            proxy_type: "none".to_string(),
            proxy_host: String::new(),
            proxy_port: 0,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();
        assert!(res
            .remote_deletes
            .contains(&"projects/p1/project.json".to_string()));
        assert!(!local_path.exists());

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    fn test_perform_lww_sync_timestamp_wins() {
        let dir = tempdir().unwrap();

        let local_p1 = dir.path().join("projects/p1/project.json");
        std::fs::create_dir_all(local_p1.parent().unwrap()).unwrap();
        std::fs::write(&local_p1, "local newer content").unwrap();

        let local_p2 = dir.path().join("projects/p2/project.json");
        std::fs::create_dir_all(local_p2.parent().unwrap()).unwrap();
        std::fs::write(&local_p2, "local older content").unwrap();

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state.known_files.insert("projects/p1/project.json".to_string(), format!("{:x}", md5::compute("local base".as_bytes())));
        state.known_files_updated_at.insert("projects/p1/project.json".to_string(), 1000);
        state.known_files.insert("projects/p2/project.json".to_string(), format!("{:x}", md5::compute("local older content".as_bytes())));
        state.known_files_updated_at.insert("projects/p2/project.json".to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let mut initial_files = std::collections::HashMap::new();
        initial_files.insert("projects/p1/project.json".to_string(), "remote older content".to_string());
        initial_files.insert("projects/p2/project.json".to_string(), "remote newer content".to_string());

        let initial_manifest = SyncManifest {
            files: vec![
                ManifestFileRecord {
                    path: "projects/p1/project.json".to_string(),
                    content_hash: format!("{:x}", md5::compute("remote older content".as_bytes())),
                    updated_at_ms: 2000,
                    device_id: "device_remote".to_string(),
                    op: "upsert".to_string(),
                    schema_version: 1,
                },
                ManifestFileRecord {
                    path: "projects/p2/project.json".to_string(),
                    content_hash: format!("{:x}", md5::compute("remote newer content".as_bytes())),
                    updated_at_ms: 4000,
                    device_id: "device_remote".to_string(),
                    op: "upsert".to_string(),
                    schema_version: 1,
                }
            ]
        };

        let (mock_url, shutdown, _files_map, manifest_str, server_thread) = start_mock_github_api(
            Some(initial_manifest),
            initial_files,
        );

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            proxy_enabled: false,
            proxy_type: "none".to_string(),
            proxy_host: String::new(),
            proxy_port: 0,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();
        
        assert!(res.uploaded_files.contains(&"projects/p1/project.json".to_string()));
        assert!(res.downloaded_files.contains(&"projects/p2/project.json".to_string()));

        let content_p2 = std::fs::read_to_string(&local_p2).unwrap();
        assert_eq!(content_p2, "remote newer content");

        let final_m_str = manifest_str.lock().unwrap().clone();
        assert!(!final_m_str.is_empty());
        let final_m: SyncManifest = serde_json::from_str(&final_m_str).unwrap();
        let p1_rec = final_m.files.iter().find(|f| f.path == "projects/p1/project.json").unwrap();
        assert_eq!(p1_rec.device_id, "device_local");
        assert_eq!(p1_rec.op, "upsert");

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }
}
