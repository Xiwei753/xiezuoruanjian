//! 同步诊断 — 探测网络、认证、仓库和分支可用性。
//!
//! 诊断流程按层级递进：权限 → URL 格式 → 网络连通 → 认证 → 仓库 → 分支。
//! 任一层失败则跳过后续层级，返回对应 `error_category` 供平台端 i18n 映射。
//!
//! SSH 传输方式当前跳过诊断（`ssh_not_recommended`），因为 LWW 后端仅支持 HTTPS。
//!
//! Issue #645 评论第 2 点：`config.backend_type` 改为 `config.active_provider` 字符串判断，
//! GitHub `remote_url`/`transport` 从 `provider_config: ProviderConfig::GitHub` 读取。

#[cfg(feature = "github-api")]
use crate::sync::provider::github::config::GitHubProviderConfig;
use crate::sync::types::SyncConfig;
use crate::sync::types::SyncDiagnosticsResult;
use crate::sync::types::SyncProtocol;
use crate::sync::types::SyncSecrets;
use crate::sync::url::detect_transport;
use crate::sync::url::sanitize_remote_url;

impl crate::sync::SyncService {
    #[allow(clippy::too_many_lines)]
    pub fn perform_sync_diagnostics(
        config: &SyncConfig,
        secrets: &SyncSecrets,
    ) -> crate::Result<SyncDiagnosticsResult> {
        let mut result = SyncDiagnosticsResult::new();
        result.backend_type = config.active_provider.clone();

        result.has_network_permission = config.has_network_permission;
        result.has_network_state_permission = config.has_network_state_permission;

        if !config.has_network_permission {
            result.network_status = "failed_no_internet_permission".to_string();
            result.auth_status = "skipped".to_string();
            result.repo_status = "skipped".to_string();
            result.branch_status = "skipped".to_string();
            result.error_category = "missing_permission".to_string();
            return Ok(result);
        }

        if !config.has_network_state_permission {
            result.network_state = "unknown_no_permission".to_string();
        } else {
            result.network_state = "permission_granted".to_string();
        }

        // 从 provider_config 读取 GitHub remote_url（若为 GitHub provider）。
        let (remote_url, transport_opt): (String, Option<SyncProtocol>) =
            match &config.provider_config {
                #[cfg(feature = "github-api")]
                Some(crate::sync::provider::ProviderConfig::GitHub(gh)) => {
                    (gh.remote_url.clone(), Some(gh.transport.clone()))
                }
                _ => (String::new(), None),
            };

        match config.active_provider.as_str() {
            #[cfg(feature = "github-api")]
            "github_api" => {
                let parsed = sanitize_remote_url(&remote_url);
                let sanitized_url = parsed.sanitized_url;
                result.remote_url_sanitized = sanitized_url.clone();

                let transport = transport_opt.unwrap_or_else(|| detect_transport(&sanitized_url));
                result.transport = match transport {
                    SyncProtocol::HttpsToken => "https".to_string(),
                    SyncProtocol::SshDeployKey => "ssh".to_string(),
                };

                if transport == SyncProtocol::SshDeployKey {
                    result.error_category = "ssh_not_recommended".to_string();
                    result.network_status = "skipped_ssh".to_string();
                    result.auth_status = "skipped".to_string();
                    result.repo_status = "skipped".to_string();
                    result.branch_status = "skipped".to_string();
                    return Ok(result);
                }

                if sanitized_url.is_empty() {
                    result.error_category = "empty_url".to_string();
                    return Ok(result);
                }

                let token_from_parsed = parsed.extracted_token;
                let token = secrets
                    .github_token()
                    .or(token_from_parsed)
                    .unwrap_or_default();
                if token.is_empty() {
                    result.error_category = "token_missing".to_string();
                    return Ok(result);
                }

                // perform_sync_diagnostics 是静态方法，不持有 SyncTransport。
                // 真正的网络探测由 facade 的 run_sync_diagnostics（持有 transport）完成。
                // 此处仅做 config 级校验：GitHubRuntimeConfig::from_persisted 验证 token/URL，
                // 通过后返回 network_probe_failed（与旧 GitHubApiBackend::new() 无 transport 行为一致）。
                let github_config = match &config.provider_config {
                    Some(crate::sync::provider::ProviderConfig::GitHub(gh)) => gh.clone(),
                    None => GitHubProviderConfig::defaults(),
                };

                match crate::sync::provider::github::config::GitHubRuntimeConfig::from_persisted(
                    &github_config,
                    secrets.provider_secrets.as_ref(),
                ) {
                    Ok(_runtime) => {
                        result.error_category = "network_probe_failed".to_string();
                        result.raw_error = Some(
                            "No SyncTransport — call run_sync_diagnostics via AppService for network probe"
                                .to_string(),
                        );
                        result.network_ok = false;
                        result.network_status = "failed".to_string();
                        Ok(result)
                    }
                    Err(e) => {
                        result.error_category = "backend_error".to_string();
                        result.network_status = format!("error: {}", e);
                        result.success = false;
                        Ok(result)
                    }
                }
            }
            #[cfg(not(feature = "github-api"))]
            "github_api" => {
                result.error_category = "github_api_unavailable".to_string();
                result.network_status = "unavailable".to_string();
                result.success = false;
                Ok(result)
            }
            "git" => {
                // Git 后端（libgit2）是 legacy 后端，不支持独立 diagnose。
                // 不再假成功，返回明确的"不支持"状态，建议用户使用 GitHubApi 后端。
                // 实际同步时如果出错会暴露真实错误。
                result.network_ok = false;
                result.network_status = "unsupported_git_backend".to_string();
                result.auth_ok = false;
                result.auth_status = "not_checked_git_backend".to_string();
                result.repo_ok = false;
                result.repo_status = "not_checked_git_backend".to_string();
                result.branch_ok = false;
                result.branch_status = "not_checked_git_backend".to_string();
                result.success = false;
                Ok(result)
            }
            _ => {
                result.error_category = "other".to_string();
                result.network_status = "unknown_provider".to_string();
                result.success = false;
                Ok(result)
            }
        }
    }
}
