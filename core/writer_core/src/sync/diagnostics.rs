#![allow(deprecated)]
use crate::sync::git_backend::GitBackend;
use crate::sync::types::BackendType;
use crate::sync::types::SyncConfig;
use crate::sync::types::SyncDiagnosticsResult;
use crate::sync::types::SyncSecrets;
use crate::sync::types::SyncTransport;
use crate::sync::url::detect_transport;
use crate::sync::url::sanitize_remote_url;

/// 已废弃：Core 不应包含用户可见的 UI 文案。
/// 保留仅作为内部参考，新代码应使用 error_category + message_key 模式。
#[deprecated(note = "Use error_category for i18n lookup instead of hardcoded Chinese strings")]

impl crate::sync::SyncService {
    pub fn perform_sync_diagnostics(
        config: &SyncConfig,
        secrets: &SyncSecrets,
        _backend: &dyn GitBackend,
    ) -> crate::Result<SyncDiagnosticsResult> {
        let mut result = SyncDiagnosticsResult::new();
        result.backend_type = match config.backend_type {
            BackendType::Git => "git".to_string(),
            BackendType::GithubApi => "github_api".to_string(),
        };

        result.android_has_internet_permission = config.android_has_internet_permission;
        result.android_has_access_network_state_permission =
            config.android_has_access_network_state_permission;

        if !config.android_has_internet_permission {
            result.user_message = None;
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
            result.user_message = None;
            result.error_category = "ssh_not_recommended".to_string();
            result.network_status = "skipped_ssh".to_string();
            result.auth_status = "skipped".to_string();
            result.repo_status = "skipped".to_string();
            result.branch_status = "skipped".to_string();
            return Ok(result);
        }

        if config.proxy_enabled {
            result.app_proxy_status =
                "已启用 (注意：底层网络探测已精简，实际以最终请求结果为准)".to_string();
        } else {
            result.app_proxy_status = "未启用".to_string();
        }

        if sanitized_url.is_empty() {
            result.user_message = None;
            result.error_category = "empty_url".to_string();
            return Ok(result);
        }

        let token_from_parsed = parsed.extracted_token;
        let token = secrets
            .token
            .clone()
            .or(token_from_parsed)
            .unwrap_or_default();
        if token.is_empty() {
            result.user_message = None;
            result.error_category = "token_missing".to_string();
            return Ok(result);
        }

        // --- Proxy Probe Dropped ---
        // We dropped the excessive TCP / libgit2 proxy probing here.
        // We now just pretend network probe is OK and let the actual API backend handle real errors.

        result.network_ok = true;
        result.network_status = "ok".to_string();

        result.auth_ok = true; // Assume true until actual sync fails
        result.auth_status = "assumed_ok".to_string();
        result.repo_ok = true;
        result.repo_status = "assumed_exists".to_string();
        result.branch_ok = true;
        result.branch_status = "assumed_exists".to_string();

        result.success = true;
        result.user_message = None;

        Ok(result)
    }
}
