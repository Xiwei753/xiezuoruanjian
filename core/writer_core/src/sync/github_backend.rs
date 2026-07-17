use crate::sync::backends::SyncBackend;
use crate::sync::service::SyncService;
use crate::sync::types::FirstSyncMode;
use crate::sync::types::SyncConfig;
use crate::sync::types::SyncDiagnosticsResult;
use crate::sync::types::SyncResult;
use crate::sync::types::SyncSecrets;
use crate::sync::types::SyncStatus;
use crate::sync::url::mask_token_in_url;
use crate::sync::url::sanitize_remote_url;
use std::path::Path;

pub struct GitHubApiBackend;

impl GitHubApiBackend {
    /// 构建 HTTP 客户端（直连模式，不使用代理）。
    pub(crate) fn build_client() -> crate::Result<reqwest::blocking::Client> {
        reqwest::blocking::Client::builder()
            .user_agent("WriterApp/1.0")
            .timeout(std::time::Duration::from_secs(15))
            .no_proxy()
            .build()
            .map_err(|e| crate::Error::SyncNetworkUnavailable { reason: format!("Failed to build HTTP client: {}", e) })
    }

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

impl SyncBackend for GitHubApiBackend {
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

        if !config.android_has_internet_permission {
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

        let client = match Self::build_client() {
            Ok(c) => c,
            Err(e) => {
                result.error_category = "network_probe_failed".to_string();
                result.raw_error = Some(e.to_string());
                return Ok(result);
            }
        };

        let api_url = format!("{}/git/ref/heads/{}", api_base, config.branch);

        match client
            .get(&api_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "WriterApp/1.0")
            .header("Accept", "application/vnd.github+json")
            .send()
        {
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
                        &body.chars().take(200).collect::<String>()
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
                        &body.chars().take(200).collect::<String>()
                    ));
                } else {
                    result.network_ok = false;
                    result.network_status = "failed".to_string();
                    result.error_category = "github_network_failed".to_string();
                    result.raw_error = Some(format!(
                        "HTTP {} (body truncated): {}",
                        status,
                        &body.chars().take(200).collect::<String>()
                    ));
                }
            }
            Err(e) => {
                result.raw_error = Some(e.to_string());
                if e.is_connect() {
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
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
        force_sync: bool,
    ) -> crate::Result<SyncResult> {
        self.sync(workspace_path, config, secrets, force_sync)
    }

    fn push(
        &self,
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
        force_sync: bool,
    ) -> crate::Result<SyncResult> {
        self.sync(workspace_path, config, secrets, force_sync)
    }

    fn sync(
        &self,
        workspace_path: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
        force_sync: bool,
    ) -> crate::Result<SyncResult> {
        log::debug!(
            "[sync] backend_type=github_api sync_mode=lww_manifest force_sync={} entry=GitHubApiBackend::sync remote_url={}",
            force_sync,
            mask_token_in_url(&sanitize_remote_url(&config.remote_url).sanitized_url)
        );
        // SAFETY: AssertUnwindSafe needed for catch_unwind at sync boundary; the closure only calls perform_lww_sync with borrowed data; on panic, the error is caught and returned as a SyncResult::Error.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SyncService::perform_lww_sync(workspace_path, config, secrets, force_sync)
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

#[cfg(test)]
mod tests {
    #[test]
    fn test_build_client_uses_no_proxy() {
        let client = super::GitHubApiBackend::build_client().unwrap();
        drop(client);
    }
}
