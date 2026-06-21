use crate::sync::git_backend::Git2Backend;
use crate::sync::github_backend::GitHubApiBackend;
use crate::sync::service::SyncService;
use crate::sync::types::BackendType;
use crate::sync::types::SyncConfig;
use crate::sync::types::SyncDiagnosticsResult;
use crate::sync::types::SyncResult;
use crate::sync::types::SyncSecrets;
use std::path::Path;

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

pub fn create_sync_backend(backend_type: &BackendType) -> Box<dyn SyncBackend> {
    match backend_type {
        BackendType::Git => Box::new(GitSyncBackend),
        BackendType::GithubApi => Box::new(GitHubApiBackend),
    }
}

pub(crate) fn build_http_client(
    config: Option<&SyncConfig>,
) -> crate::Result<reqwest::blocking::Client> {
    let mut builder = reqwest::blocking::Client::builder()
        .user_agent("WriterApp/1.0")
        .timeout(std::time::Duration::from_secs(15));

    if let Some(cfg) = config {
        if cfg.proxy_enabled && !cfg.proxy_host.is_empty() && cfg.proxy_port > 0 {
            let proxy_url = match cfg.proxy_type.as_str() {
                "http" => format!("http://{}:{}", cfg.proxy_host, cfg.proxy_port),
                "https" => format!("https://{}:{}", cfg.proxy_host, cfg.proxy_port),
                "socks5" => format!("socks5h://{}:{}", cfg.proxy_host, cfg.proxy_port),
                _ => format!("https://{}:{}", cfg.proxy_host, cfg.proxy_port),
            };
            let proxy = reqwest::Proxy::all(&proxy_url)
                .map_err(|e| crate::Error::Other(format!("Failed to configure proxy: {}", e)))?;
            builder = builder.proxy(proxy);
        }
    }

    let client = builder
        .build()
        .map_err(|e| crate::Error::Other(format!("Failed to build HTTP client: {}", e)))?;
    Ok(client)
}
