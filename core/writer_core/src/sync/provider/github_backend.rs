//! GitHub REST API 同步后端 — 基于 LWW（Last Writer Wins）的文件级同步。
//!
//! 本模块仅在 `github-api` feature 启用时可用。

#[cfg(feature = "github-api")]
use crate::sync::provider::backend::SyncBackend;
#[cfg(feature = "github-api")]
use crate::sync::service::SyncService;
#[cfg(feature = "github-api")]
use crate::sync::types::FirstSyncMode;
#[cfg(feature = "github-api")]
use crate::sync::types::SyncConfig;
#[cfg(feature = "github-api")]
use crate::sync::types::SyncDiagnosticsResult;
#[cfg(feature = "github-api")]
use crate::sync::types::SyncResult;
#[cfg(feature = "github-api")]
use crate::sync::types::SyncSecrets;
#[cfg(feature = "github-api")]
use crate::sync::types::SyncStatus;
#[cfg(feature = "github-api")]
use crate::sync::url::mask_token_in_url;
#[cfg(feature = "github-api")]
use crate::sync::url::sanitize_remote_url;
#[cfg(feature = "github-api")]
use std::path::Path;
#[cfg(feature = "github-api")]
use writer_platform_api::{HttpRequest, HttpResponse, SyncTransport, TransportError};

#[cfg(feature = "github-api")]
pub struct GitHubApiBackend {
    transport: Option<Box<dyn SyncTransport>>,
}

#[cfg(not(feature = "github-api"))]
pub struct GitHubApiBackend {
    _marker: std::marker::PhantomData<()>,
}

#[cfg(feature = "github-api")]
impl GitHubApiBackend {
    pub fn new() -> Self {
        Self { transport: None }
    }

    pub fn with_transport(transport: Box<dyn SyncTransport>) -> Self {
        Self {
            transport: Some(transport),
        }
    }

    fn ensure_transport(&self) -> crate::Result<&dyn SyncTransport> {
        self.transport.as_ref().map(|t| t.as_ref()).ok_or_else(|| {
            crate::Error::SyncNetworkUnavailable {
                reason:
                    "No SyncTransport configured — platform must inject HTTP transport before sync"
                        .to_string(),
            }
        })
    }

    fn transport_err_to_core(e: TransportError) -> crate::Error {
        crate::Error::SyncNetworkUnavailable {
            reason: format!("{}: {}", e.category, e.message),
        }
    }

    fn execute_get(
        transport: &dyn SyncTransport,
        url: &str,
        token: &str,
    ) -> crate::Result<HttpResponse> {
        let request = HttpRequest {
            method: "GET".to_string(),
            url: url.to_string(),
            headers: vec![
                ("Authorization".to_string(), format!("Bearer {}", token)),
                ("User-Agent".to_string(), "WriterApp/1.0".to_string()),
                (
                    "Accept".to_string(),
                    "application/vnd.github+json".to_string(),
                ),
            ],
            body: None,
        };
        transport
            .execute(request)
            .map_err(Self::transport_err_to_core)
    }

    /// 从远程 URL 推导 GitHub API base URL。
    ///
    /// 输入格式约定：`https://github.com/owner/repo` 或 `https://github.com/owner/repo.git`。
    /// `.git` 后缀会被自动剥离。非 GitHub 域名的 URL 原样返回（用于自托管 GitHub Enterprise）。
    pub(crate) fn api_base_url(remote_url: &str) -> String {
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

#[cfg(feature = "github-api")]
impl Default for GitHubApiBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(feature = "github-api"))]
impl Default for GitHubApiBackend {
    fn default() -> Self {
        Self { _marker: std::marker::PhantomData }
    }
}

#[cfg(feature = "github-api")]
impl SyncBackend for GitHubApiBackend {
    // TODO(#597): 既有代码可读性技术债，待后续重构拆分
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        clippy::cast_lossless,
        deprecated
    )]
    fn diagnose(
        &self,
        config: &SyncConfig,
        secrets: &SyncSecrets,
    ) -> crate::Result<SyncDiagnosticsResult> {
        let mut result = SyncDiagnosticsResult::new();
        result.backend_type = "github_api".to_string();
        result.remote_url_sanitized = sanitize_remote_url(&config.remote_url)
            .sanitized_url
            .clone();
        result.transport = "https".to_string();

        if !config.has_network_permission {
            result.error_category = "missing_permission".to_string();
            return Ok(result);
        }

        let token = secrets.token.clone().unwrap_or_default();
        if token.is_empty() {
            result.error_category = "token_missing".to_string();
            return Ok(result);
        }

        let api_base = Self::api_base_url(&config.remote_url);
        let _masked_url = mask_token_in_url(&api_base);

        let transport = match self.ensure_transport() {
            Ok(t) => t,
            Err(e) => {
                result.error_category = "network_probe_failed".to_string();
                result.raw_error = Some(e.to_string());
                return Ok(result);
            }
        };

        let api_url = format!("{}/git/ref/heads/{}", api_base, config.branch);

        match Self::execute_get(transport, &api_url, &token) {
            Ok(resp) => {
                let status = resp.status;
                let body = String::from_utf8(resp.body).unwrap_or_default();
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
                } else if status == 401 || status == 403 {
                    result.network_ok = true;
                    result.network_status = "ok".to_string();
                    result.auth_ok = false;
                    result.auth_status = "failed".to_string();
                    result.error_category = if status == 401 {
                        "token_invalid"
                    } else {
                        "token_permission_denied"
                    }
                    .to_string();
                    result.raw_error = Some(format!(
                        "HTTP {} (body truncated): {}",
                        status,
                        body.chars().take(200).collect::<String>()
                    ));
                } else if status == 404 {
                    result.network_ok = true;
                    result.network_status = "ok".to_string();
                    result.auth_ok = true;
                    result.auth_status = "ok".to_string();
                    result.repo_ok = false;
                    result.repo_status = "failed".to_string();
                    result.error_category = "repo_not_found_or_no_permission".to_string();
                    result.raw_error = Some(format!(
                        "HTTP {} (body truncated): {}",
                        status,
                        body.chars().take(200).collect::<String>()
                    ));
                } else {
                    result.network_ok = false;
                    result.network_status = "failed".to_string();
                    result.error_category = "github_network_failed".to_string();
                    result.raw_error = Some(format!(
                        "HTTP {} (body truncated): {}",
                        status,
                        body.chars().take(200).collect::<String>()
                    ));
                }
            }
            Err(e) => {
                let err_str = e.to_string();
                result.raw_error = Some(err_str.clone());
                if err_str.contains("dns") || err_str.contains("connect") {
                    result.error_category = "dns_failed".to_string();
                } else {
                    result.error_category = "github_network_failed".to_string();
                }
                result.network_ok = false;
                result.network_status = "failed".to_string();
            }
        }

        Ok(result)
    }

    fn pull(
        &self,
        sync_root: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
        target: &crate::sync::types::SyncTarget,
        force_sync: bool,
    ) -> crate::Result<SyncResult> {
        self.sync(sync_root, config, secrets, target, force_sync)
    }

    fn push(
        &self,
        sync_root: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
        target: &crate::sync::types::SyncTarget,
        force_sync: bool,
    ) -> crate::Result<SyncResult> {
        self.sync(sync_root, config, secrets, target, force_sync)
    }

    fn sync(
        &self,
        sync_root: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
        target: &crate::sync::types::SyncTarget,
        force_sync: bool,
    ) -> crate::Result<SyncResult> {
        log::debug!(
            "[sync] backend_type=github_api sync_mode=lww_manifest force_sync={} entry=GitHubApiBackend::sync remote_url={} remote_prefix={}",
            force_sync,
            mask_token_in_url(&sanitize_remote_url(&config.remote_url).sanitized_url),
            target.remote_prefix
        );
        let transport = self.ensure_transport()?;
        // SAFETY: AssertUnwindSafe needed for catch_unwind at sync boundary; the closure only calls perform_lww_sync with borrowed data; on panic, the error is caught and returned as a SyncResult::Error.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SyncService::perform_lww_sync(sync_root, config, secrets, target, force_sync, transport)
        })) {
            Ok(result) => result,
            Err(err) => {
                let panic_msg = if let Some(s) = err.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = err.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                Ok(SyncResult::error(
                    SyncStatus::FatalError("backend_panic".to_string()),
                    FirstSyncMode::NotAttempted,
                    format!("backend_panic: {}", panic_msg),
                    None,
                ))
            }
        }
    }
}

// `ReqwestSyncTransport` 已迁移至各平台 crate。
//
// 平台端在自己的 crate 中提供基于 reqwest 的 `SyncTransport` 实现：
// - `writer-platform-linux`：Linux 桌面端
// - `writer-platform-android`：Android 端
//
// 平台端也可以提供自己的 `SyncTransport` 实现（如使用 Android 特定的 HTTP 客户端）。
// Core 只依赖 `writer_platform_api::SyncTransport` trait，不直接依赖 reqwest。
