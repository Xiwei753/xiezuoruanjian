use crate::sync_service::types::SyncDiagnosticsResult;
use crate::sync_service::types::SyncConfig;
use crate::sync_service::github_backend::GitHubApiBackend;
use crate::sync_service::types::SyncStatus;
use crate::sync_service::service::SyncService;
use crate::sync_service::git_backend::Git2Backend;
use crate::sync_service::types::SyncSecrets;
use crate::sync_service::types::BackendType;
use crate::sync_service::types::FirstSyncMode;
use crate::sync_service::types::SyncResult;
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
        result.user_message =
            "LocalFolder 后端尚未实现。此后端用于配合 Syncthing 等外部同步工具。".to_string();
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

pub(crate) fn build_http_client(config: Option<&SyncConfig>) -> crate::Result<reqwest::blocking::Client> {
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
