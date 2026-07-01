use crate::sync::github_backend::GitHubApiBackend;
#[cfg(feature = "git-https")]
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
        force_sync: bool,
    ) -> crate::Result<SyncResult>;
    fn push(
        &self,
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
        force_sync: bool,
    ) -> crate::Result<SyncResult>;
    fn sync(
        &self,
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
        force_sync: bool,
    ) -> crate::Result<SyncResult>;
}

#[cfg(feature = "git-https")]
pub struct GitSyncBackend;

#[cfg(not(feature = "git-https"))]
pub struct UnavailableGitBackend;

#[cfg(not(feature = "git-https"))]
impl SyncBackend for UnavailableGitBackend {
    fn diagnose(
        &self,
        _config: &SyncConfig,
        _secrets: &SyncSecrets,
    ) -> crate::Result<SyncDiagnosticsResult> {
        Err(crate::Error::Other(
            "git backend is unavailable in this build; use github_api".into(),
        ))
    }
    fn pull(
        &self,
        _workspace_path: &Path,
        _config: &SyncConfig,
        _secrets: &SyncSecrets,
        _force_sync: bool,
    ) -> crate::Result<SyncResult> {
        Err(crate::Error::Other(
            "git backend is unavailable in this build; use github_api".into(),
        ))
    }
    fn push(
        &self,
        _workspace_path: &Path,
        _config: &SyncConfig,
        _secrets: &SyncSecrets,
        _force_sync: bool,
    ) -> crate::Result<SyncResult> {
        Err(crate::Error::Other(
            "git backend is unavailable in this build; use github_api".into(),
        ))
    }
    fn sync(
        &self,
        _workspace_path: &Path,
        _config: &SyncConfig,
        _secrets: &SyncSecrets,
        _force_sync: bool,
    ) -> crate::Result<SyncResult> {
        Err(crate::Error::Other(
            "git backend is unavailable in this build; use github_api".into(),
        ))
    }
}

#[cfg(feature = "git-https")]
impl SyncBackend for GitSyncBackend {
    fn diagnose(
        &self,
        config: &SyncConfig,
        secrets: &SyncSecrets,
    ) -> crate::Result<SyncDiagnosticsResult> {
        SyncService::perform_sync_diagnostics(config, secrets)
    }
    fn pull(
        &self,
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
        _force_sync: bool,
    ) -> crate::Result<SyncResult> {
        use crate::sync::git_backend::Git2Backend;
        let backend = Git2Backend;
        SyncService::perform_sync(workspace_path, config, secrets, &backend)
    }
    fn push(
        &self,
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
        _force_sync: bool,
    ) -> crate::Result<SyncResult> {
        use crate::sync::git_backend::Git2Backend;
        let backend = Git2Backend;
        SyncService::perform_sync(workspace_path, config, secrets, &backend)
    }
    fn sync(
        &self,
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
        _force_sync: bool,
    ) -> crate::Result<SyncResult> {
        use crate::sync::git_backend::Git2Backend;
        let backend = Git2Backend;
        SyncService::perform_sync(workspace_path, config, secrets, &backend)
    }
}

pub fn create_sync_backend(backend_type: &BackendType) -> Box<dyn SyncBackend> {
    match backend_type {
        #[cfg(feature = "git-https")]
        BackendType::Git => Box::new(GitSyncBackend),
        #[cfg(not(feature = "git-https"))]
        BackendType::Git => Box::new(UnavailableGitBackend),
        BackendType::GithubApi => Box::new(GitHubApiBackend),
    }
}
