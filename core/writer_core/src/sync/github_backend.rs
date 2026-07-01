#![allow(deprecated)]
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
    /// 构建 HTTP 客户端，根据 network_mode 选择是否使用代理。
    /// - "direct" 或 ""：不使用代理（.no_proxy()）
    /// - "system"：使用系统代理设置（reqwest 默认行为）
    /// - 其他：默认不使用代理
    pub(crate) fn build_client(network_mode: &str) -> crate::Result<reqwest::blocking::Client> {
        let mut builder = reqwest::blocking::Client::builder()
            .user_agent("WriterApp/1.0")
            .timeout(std::time::Duration::from_secs(15));

        match network_mode {
            "direct" | "" => {
                builder = builder.no_proxy();
            }
            "system" => {
                // 使用系统代理设置（reqwest 默认行为）
            }
            _ => {
                // 未知模式，默认不使用代理
                builder = builder.no_proxy();
            }
        }

        builder
            .build()
            .map_err(|e| crate::Error::Other(format!("Failed to build HTTP client: {}", e)))
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

    /// 决定网络模式：优先使用 last_successful_network_mode，否则默认 "direct"
    pub(crate) fn resolve_network_mode(_config: &SyncConfig) -> String {
        // 未来可根据 config 中的用户偏好选择网络模式
        // 当前默认 "direct"，如果直连失败可在 diagnose 中尝试 "system"
        "direct".to_string()
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

        let network_mode = Self::resolve_network_mode(config);
        result.chosen_network_mode = Some(network_mode.clone());

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

        // 先尝试首选网络模式
        let client = match Self::build_client(&network_mode) {
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
                let err_msg = e.to_string().to_lowercase();
                result.raw_error = Some(e.to_string());
                if err_msg.contains("dns")
                    || err_msg.contains("resolve")
                    || err_msg.contains("name resolution")
                {
                    result.error_category = "dns_failed".to_string();
                } else if err_msg.contains("ssl")
                    || err_msg.contains("certificate")
                    || err_msg.contains("tls")
                {
                    result.error_category = "tls_failed".to_string();
                } else if err_msg.contains("connection refused")
                    || err_msg.contains("timeout")
                    || err_msg.contains("network unreachable")
                {
                    result.error_category = "github_network_failed".to_string();
                } else {
                    result.error_category = "github_network_failed".to_string();
                }
                result.network_ok = false;
                result.network_status = "failed".to_string();

                // direct 模式失败时，尝试 system 代理模式
                if network_mode == "direct" {
                    let system_client = match Self::build_client("system") {
                        Ok(c) => c,
                        Err(_) => return Ok(result),
                    };
                    match system_client
                        .get(&api_url)
                        .header("Authorization", format!("Bearer {}", token))
                        .header("User-Agent", "WriterApp/1.0")
                        .header("Accept", "application/vnd.github+json")
                        .send()
                    {
                        Ok(resp) => {
                            let status = resp.status().as_u16();
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
                                result.chosen_network_mode = Some("system".to_string());
                            }
                            // 其他状态码保持 direct 模式的错误信息
                        }
                        Err(_) => {
                            // system 代理也失败，保持 direct 模式的错误信息
                        }
                    }
                }
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
        let network_mode = Self::resolve_network_mode(config);
        eprintln!(
            "[sync] backend_type=github_api sync_mode=lww_manifest chosen_network_mode={} force_sync={} entry=GitHubApiBackend::sync remote_url={}",
            network_mode,
            force_sync,
            mask_token_in_url(&sanitize_remote_url(&config.remote_url).sanitized_url)
        );
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
    fn test_build_client_direct_uses_no_proxy() {
        let source = include_str!("github_backend.rs");
        // build_client("direct") 分支应调用 .no_proxy()
        assert!(
            source.contains("\"direct\" | \"\" => {\n                builder = builder.no_proxy();"),
            "build_client must call .no_proxy() for direct mode"
        );
    }

    #[test]
    fn test_build_client_system_does_not_use_no_proxy() {
        let source = include_str!("github_backend.rs");
        // "system" 分支不应调用 .no_proxy()
        let system_idx = source.find("\"system\" =>").expect("must have system branch");
        let wildcard_idx = source[system_idx..].find("_ =>").expect("must have wildcard arm after system");
        let system_section = &source[system_idx..system_idx + wildcard_idx];
        assert!(
            !system_section.contains(".no_proxy()"),
            "build_client must NOT call .no_proxy() for system mode"
        );
    }

    #[test]
    fn test_github_api_backend_pull_push_not_backend_not_implemented() {
        let source = include_str!("github_backend.rs");
        let impl_start = source
            .find("impl SyncBackend for GitHubApiBackend")
            .expect("must have SyncBackend impl");
        let impl_block = &source[impl_start..];
        let impl_end = impl_block.find("\n}").unwrap_or(impl_block.len());
        let impl_body = &impl_block[..impl_end];
        assert!(
            !impl_body.contains("backend_not_implemented"),
            "GitHubApiBackend pull/push must not return backend_not_implemented — they should delegate to sync()"
        );
    }
}
