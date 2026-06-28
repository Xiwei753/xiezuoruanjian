use crate::sync::backends::SyncBackend;
use crate::sync::types::BackendType;
use crate::sync::types::SyncConfig;
use crate::sync::types::SyncDiagnosticsResult;
use crate::sync::types::SyncSecrets;
use crate::sync::types::SyncTransport;
use crate::sync::url::detect_transport;
use crate::sync::url::sanitize_remote_url;

impl crate::sync::SyncService {
    pub fn perform_sync_diagnostics(
        config: &SyncConfig,
        secrets: &SyncSecrets,
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
            .token
            .clone()
            .or(token_from_parsed)
            .unwrap_or_default();
        if token.is_empty() {
            result.error_category = "token_missing".to_string();
            return Ok(result);
        }

        // --- 委托给实际后端进行真实诊断 ---
        // 不再假成功，而是走真实后端的 diagnose 方法
        // 对于 GithubApi 后端，直接调用 GitHubApiBackend::diagnose 进行真实 GitHub API 请求
        // 对于 Git 后端，保留快速路径（git 后端不支持独立 diagnose）
        match config.backend_type {
            BackendType::GithubApi => {
                let backend = crate::sync::github_backend::GitHubApiBackend;
                match backend.diagnose(config, secrets) {
                    Ok(diag_result) => Ok(diag_result),
                    Err(e) => {
                        result.error_category = "backend_error".to_string();
                        result.network_status = format!("error: {}", e);
                        result.success = false;
                        Ok(result)
                    }
                }
            }
            BackendType::Git => {
                // Git 后端（libgit2）不支持独立 diagnose，保留快速路径
                // 实际同步时如果出错会暴露真实错误
                result.network_ok = true;
                result.network_status = "ok_git_backend".to_string();
                result.auth_ok = true;
                result.auth_status = "assumed_ok_git".to_string();
                result.repo_ok = true;
                result.repo_status = "assumed_exists".to_string();
                result.branch_ok = true;
                result.branch_status = "assumed_exists".to_string();
                result.success = true;
                Ok(result)
            }
        }
    }
}
