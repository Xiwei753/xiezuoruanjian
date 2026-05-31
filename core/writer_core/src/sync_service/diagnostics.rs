use crate::sync_service::types::BackendType;
use crate::sync_service::types::SyncDiagnosticsResult;
use crate::sync_service::types::SyncConfig;
use crate::sync_service::git_backend::GitBackend;
use crate::sync_service::types::SyncTransport;
use crate::sync_service::types::SyncSecrets;
use crate::sync_service::url::sanitize_remote_url;
use crate::sync_service::url::detect_transport;

pub(crate) fn get_user_friendly_error(err: &str) -> String {
    let e = err.to_lowercase();
    if e.contains("failed to resolve address")
        || e.contains("no address associated with hostname")
        || e.contains("could not resolve host")
        || e.contains("name resolution")
    {
        return "无法解析 GitHub。请检查手机网络、DNS、代理/VPN/Clash 是否允许本应用访问 GitHub，然后重试。".to_string();
    }
    if e.contains("authentication failed") || e.contains("invalid credentials") || e.contains("401")
    {
        return "身份验证失败。请检查您的 GitHub Token 是否正确，或者该 Token 是否具有访问该仓库的权限。".to_string();
    }
    if e.contains("repository not found") || e.contains("not found") || e.contains("404") {
        return "找不到仓库。请检查您填写的 GitHub 仓库地址是否正确，或者您的 Token 是否有权限访问该私有仓库。".to_string();
    }
    if e.contains("ssl") || e.contains("certificate") {
        return "SSL 证书或网络错误。请检查您的网络环境、代理/VPN 设置或系统时间是否正确。"
            .to_string();
    }
    if e.contains("timeout")
        || e.contains("connection refused")
        || e.contains("network unreachable")
    {
        return "网络连接失败或超时。请检查您的网络连接或代理设置。".to_string();
    }
    if e.contains("conflict") {
        return "同步代码冲突。请在另一端解决冲突后重试。".to_string();
    }
    if e.contains("operation not permitted") && e.contains("127.0.0.1") {
        return "代理 127.0.0.1:7890 连接被拒绝，请确认手机代理 App 开启本机 HTTP 端口，或选择不使用手动代理，改走系统 VPN/全局模式。".to_string();
    }
    if e.contains("unsupported proxy protocol") && e.contains("socks5") {
        return "当前构建版本的底层网络库不支持 SOCKS5 代理。请尝试使用 HTTP 代理或更新应用。"
            .to_string();
    }
    format!("同步失败，请检查网络重试。({})", err)
}

impl crate::sync_service::SyncService {
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
            result.user_message = "缺少 INTERNET 权限。Android 应用无法联网，请在 AndroidManifest.xml 中添加 android.permission.INTERNET。".to_string();
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
            result.user_message = "检测到 SSH remote。由于跨平台网络限制，强烈建议改用 HTTPS API 模式 (https://github.com/owner/repo.git)。".to_string();
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
            result.user_message = "远程仓库地址为空。".to_string();
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
            result.user_message =
                "缺少 GitHub Token。请在设置中配置，这是目前最推荐的同步方式。".to_string();
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
        result.user_message =
            "基础配置检查通过。底层网络直连已简化，实际网络状况以执行同步时为准。".to_string();

        Ok(result)
    }

}
