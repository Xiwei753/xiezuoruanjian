//! GitHub Provider 配置 — 从通用 `SyncConfig` + `SyncSecrets` 解析 GitHub 特定参数。
//!
//! [`GitHubProviderConfig`] 持有 GitHub REST API 调用所需的全部信息：
//! API base URL、token、分支、原始 remote_url（用于诊断脱敏）、username。
//!
//! `from_sync_config` 把 token/URL/branch 解析集中在此处，
//! LWW engine 和 diagnose 都不再直接读 `SyncConfig`/`SyncSecrets`。

use crate::sync::provider::error::ProviderError;
use crate::sync::types::SyncConfig;
use crate::sync::types::SyncSecrets;
use crate::sync::url::sanitize_remote_url;

/// GitHub Provider 配置 — GitHub REST API 调用所需的全部参数。
///
/// 由 `from_sync_config` 从通用 `SyncConfig` + `SyncSecrets` 解析而来，
/// `GitHubProvider::new` 消费此结构体构造 Provider 实例。
#[derive(Debug, Clone)]
pub struct GitHubProviderConfig {
    /// GitHub REST API base URL，如 `https://api.github.com/repos/owner/repo`。
    pub api_base_url: String,
    /// GitHub personal access token。
    pub token: String,
    /// 远端分支名。
    pub branch: String,
    /// 原始 remote_url（未脱敏），用于诊断展示时走 `sanitize_remote_url` 脱敏。
    pub remote_url: String,
    /// GitHub username（HTTPS credential callback 用，默认 `x-access-token`）。
    pub username: String,
}

impl GitHubProviderConfig {
    /// 从通用同步配置和密钥解析 GitHub Provider 配置。
    ///
    /// - token 为空 → `ProviderError::AuthFailed`（调用方在创建 Provider 前就能拿到明确错误）。
    /// - remote_url 经 `sanitize_remote_url` 脱敏后推导 api_base_url。
    /// - branch 为空时回退到 `"main"`。
    /// - username 为空时回退到 `"x-access-token"`。
    ///
    /// 从 `SyncConfig::resolve_*()` 方法读取 GitHub 特定参数，
    /// 优先使用 `github` 嵌套配置，回退到顶层旧字段（向后兼容）。
    pub fn from_sync_config(
        config: &SyncConfig,
        secrets: &SyncSecrets,
    ) -> Result<Self, ProviderError> {
        let token = secrets.token.clone().unwrap_or_default();
        if token.is_empty() {
            return Err(ProviderError::AuthFailed {
                reason: "token is missing".to_string(),
            });
        }
        let branch = config.resolve_branch();
        let username = config
            .resolve_username()
            .unwrap_or_else(|| "x-access-token".to_string());
        let remote_url = config.resolve_remote_url().unwrap_or_default();
        let api_base_url = api_base_url_from_remote(&remote_url);
        Ok(Self {
            api_base_url,
            token,
            branch,
            remote_url,
            username,
        })
    }
}

/// 从远程 URL 推导 GitHub API base URL。
///
/// 输入格式约定：`https://github.com/owner/repo` 或 `https://github.com/owner/repo.git`。
/// `.git` 后缀会被自动剥离。非 GitHub 域名的 URL 原样返回（用于自托管 GitHub Enterprise）。
///
/// 此函数复刻原 `GitHubApiBackend::api_base_url` 的逻辑，Phase 7 删除旧后端后由本函数承接。
pub(crate) fn api_base_url_from_remote(remote_url: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_base_url_strips_git_suffix() {
        assert_eq!(
            api_base_url_from_remote("https://github.com/owner/repo.git"),
            "https://api.github.com/repos/owner/repo"
        );
        assert_eq!(
            api_base_url_from_remote("https://github.com/owner/repo"),
            "https://api.github.com/repos/owner/repo"
        );
    }

    #[test]
    fn api_base_url_preserves_enterprise_url() {
        assert_eq!(
            api_base_url_from_remote("https://ghe.example.com/api/v3/repos/owner/repo"),
            "https://ghe.example.com/api/v3/repos/owner/repo"
        );
    }
}
