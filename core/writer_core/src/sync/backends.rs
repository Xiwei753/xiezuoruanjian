//! 同步后端抽象 — 跨平台同步契约的统一入口。
//!
//! `SyncBackend` trait 定义了 diagnose / pull / push / sync 四个核心方法，
//! 所有同步后端（Git、GitHub API）必须实现此 trait。
//!
//! ## 调用时序
//!
//! 平台端应先调用 `diagnose` 验证连通性和权限，再调用 `sync`/`pull`/`push`。
//! trait 层不强制时序——diagnose 失败后调用 sync 会返回错误，不会产生副作用。
//!
//! ## 线程安全
//!
//! trait 方法接受 `&self`（不可变引用），实现必须是线程安全的。
//! 调用方可以在多线程环境中共享 `&SyncBackend` 引用。
//! 长时间阻塞的网络操作由实现内部处理，调用方无需额外同步。
//!
//! ## `force_sync` 语义
//!
//! `force_sync = true` 时跳过本地脏检查，即使本地无变更也执行完整同步流程。
//! 用于平台端"立即同步"按钮等场景。`force_sync = false` 时后端可跳过不必要的网络请求。
//!
//! ## Feature 门控
//!
//! - `git-https`：启用 Git 同步后端（依赖 `git2`）
//! - `github-api`：启用 GitHub API 同步后端（依赖 `reqwest`）
//!
//! 未启用 feature 时对应后端返回不可用错误，编译通过且运行时行为明确。

#[cfg(feature = "github-api")]
use crate::sync::github_backend::GitHubApiBackend;
#[cfg(feature = "git-https")]
use crate::sync::service::SyncService;
use crate::sync::types::BackendType;
use crate::sync::types::SyncConfig;
use crate::sync::types::SyncDiagnosticsResult;
use crate::sync::types::SyncResult;
use crate::sync::types::SyncSecrets;
use std::path::Path;

/// 同步后端契约 — 所有同步实现必须满足此接口。
///
/// 平台端通过 `create_sync_backend` 获取具体实现，不直接依赖后端类型。
/// 返回 `SyncResult` 携带同步状态、冲突信息和错误分类，平台端按状态走不同恢复路径。
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

/// Git 同步后端 — 仅在 `git-https` feature 启用时可用。
///
/// 使用 libgit2 执行完整的 Git 同步流程（clone/fetch/push），
/// 通过 `SyncService::perform_sync` 实现。
#[cfg(feature = "git-https")]
pub struct GitSyncBackend;

/// Git 不可用占位后端 — 在 `git-https` feature 未启用时替代 `GitSyncBackend`。
///
/// 所有方法返回错误，提示用户使用 `github_api` 后端。
/// 这保证编译通过且运行时行为明确，不会静默失败。
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

/// GitHub API 不可用占位后端 — 在 `github-api` feature 未启用时替代 `GitHubApiBackend`。
///
/// 所有方法返回错误，提示用户启用 `github-api` feature。
#[cfg(not(feature = "github-api"))]
pub struct UnavailableGithubApiBackend;

#[cfg(not(feature = "github-api"))]
impl SyncBackend for UnavailableGithubApiBackend {
    fn diagnose(
        &self,
        _config: &SyncConfig,
        _secrets: &SyncSecrets,
    ) -> crate::Result<SyncDiagnosticsResult> {
        Err(crate::Error::Other(
            "github_api backend is unavailable in this build; enable 'github-api' feature".into(),
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
            "github_api backend is unavailable in this build; enable 'github-api' feature".into(),
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
            "github_api backend is unavailable in this build; enable 'github-api' feature".into(),
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
            "github_api backend is unavailable in this build; enable 'github-api' feature".into(),
        ))
    }
}

/// 同步后端工厂 — 根据 `BackendType` 创建对应的后端实例。
///
/// 返回 `Box<dyn SyncBackend>`，调用方不关心具体实现类型。
/// 未启用 feature 的后端类型返回占位实现（所有操作失败）。
pub fn create_sync_backend(backend_type: &BackendType) -> Box<dyn SyncBackend> {
    match backend_type {
        #[cfg(feature = "git-https")]
        BackendType::Git => Box::new(GitSyncBackend),
        #[cfg(not(feature = "git-https"))]
        BackendType::Git => Box::new(UnavailableGitBackend),
        #[cfg(feature = "github-api")]
        BackendType::GithubApi => Box::new(GitHubApiBackend),
        #[cfg(not(feature = "github-api"))]
        BackendType::GithubApi => Box::new(UnavailableGithubApiBackend),
    }
}
