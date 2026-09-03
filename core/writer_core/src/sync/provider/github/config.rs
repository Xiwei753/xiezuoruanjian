//! GitHub Provider 配置 — 持久化配置 + 运行时配置（Issue #645 评论第 2 点）。
//!
//! 持久化配置 [`GitHubProviderConfig`] 只含可安全写入 config.json 的字段
//! （remote_url / branch / username / transport），不含 token。
//! 运行时配置 [`GitHubRuntimeConfig`] 在构造 Provider 时由持久化配置 + secrets 推导，
//! 含 token 和从 remote_url 推导的 api_base_url。
//!
//! `GitHubProvider::new` 接收 `GitHubRuntimeConfig` + `Arc<dyn SyncTransport>`。

use crate::sync::provider::error::ProviderError;
use crate::sync::provider::ProviderSecrets;
use crate::sync::types::SyncProtocol;
use crate::sync::url::sanitize_remote_url;

/// GitHub Provider 持久化配置 — 存储在 `SyncConfig.provider_config` 中。
///
/// 不含 token（token 由 `SyncSecrets.provider_secrets` 注入）。
/// 可安全序列化到 `<app_data_root>/app-meta/sync/config.local.json`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GitHubProviderConfig {
    /// GitHub 远端仓库 URL（HTTPS 或 SSH），未脱敏。
    pub remote_url: String,
    /// 远端分支名。
    pub branch: String,
    /// GitHub username（HTTPS credential callback 用，默认 `x-access-token`）。
    pub username: String,
    /// 传输方式（HTTPS token 或 SSH deploy key）。
    pub transport: SyncProtocol,
}

impl GitHubProviderConfig {
    /// 默认值 — 空 URL、`main` 分支、`x-access-token` 用户、HTTPS token。
    pub fn defaults() -> Self {
        Self {
            remote_url: String::new(),
            branch: "main".to_string(),
            username: "x-access-token".to_string(),
            transport: SyncProtocol::HttpsToken,
        }
    }

    /// 从旧 `SyncConfig` 顶层 GitHub 字段迁移（用于 load 时旧 JSON 一次性转换）。
    ///
    /// 旧格式中 `remote_url`/`branch`/`username`/`transport` 在 `SyncConfig` 顶层，
    /// 新格式统一收进 `ProviderConfig::GitHub(...)`。本方法把旧顶层字段聚合成持久化配置。
    /// 空字段回退到默认值：branch → "main"，username → "x-access-token"，
    /// transport → HttpsToken。
    pub fn from_legacy_fields(
        remote_url: Option<String>,
        branch: Option<String>,
        username: Option<String>,
        transport: Option<SyncProtocol>,
    ) -> Self {
        Self {
            remote_url: remote_url.unwrap_or_default(),
            branch: branch
                .filter(|b| !b.is_empty())
                .unwrap_or_else(|| "main".to_string()),
            username: username
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| "x-access-token".to_string()),
            transport: transport.unwrap_or(SyncProtocol::HttpsToken),
        }
    }
}

impl Default for GitHubProviderConfig {
    fn default() -> Self {
        Self::defaults()
    }
}

/// GitHub Provider 运行时配置 — 构造 `GitHubProvider` 时使用。
///
/// 由 `GitHubRuntimeConfig::from_persisted` 从持久化配置 + secrets 推导：
/// - `api_base_url` 从 `remote_url` 推导（剥离 `.git`、转 API 路径）。
/// - `token` 从 `ProviderSecrets::GitHub { token }` 注入。
///
/// 不实现 `Serialize`/`Deserialize` — 含 token，不持久化。
#[derive(Debug, Clone)]
pub struct GitHubRuntimeConfig {
    /// GitHub REST API base URL，如 `https://api.github.com/repos/owner/repo`。
    pub api_base_url: String,
    /// GitHub personal access token。
    pub token: String,
    /// 远端分支名。
    pub branch: String,
    /// 原始 remote_url（未脱敏），用于诊断展示时走 `sanitize_remote_url` 脱敏。
    pub remote_url: String,
    /// GitHub username（HTTPS credential callback 用）。
    pub username: String,
}

impl GitHubRuntimeConfig {
    /// 从持久化配置 + secrets 构造运行时配置。
    ///
    /// - `provider_secrets` 为 None 或非 GitHub 变体 → `ProviderError::AuthFailed`（token 缺失）。
    /// - token 为空 → `ProviderError::AuthFailed`。
    /// - `api_base_url` 从 `remote_url` 经 `sanitize_remote_url` 脱敏后推导。
    pub fn from_persisted(
        config: &GitHubProviderConfig,
        secrets: Option<&ProviderSecrets>,
    ) -> Result<Self, ProviderError> {
        let token = secrets
            .and_then(|s| s.github_token())
            .unwrap_or_default()
            .to_string();
        if token.is_empty() {
            return Err(ProviderError::AuthFailed {
                reason: "token is missing".to_string(),
            });
        }
        let api_base_url = api_base_url_from_remote(&config.remote_url);
        Ok(Self {
            api_base_url,
            token,
            branch: config.branch.clone(),
            remote_url: config.remote_url.clone(),
            username: config.username.clone(),
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

    #[test]
    fn from_legacy_fields_applies_defaults_for_empty() {
        let cfg = GitHubProviderConfig::from_legacy_fields(None, None, None, None);
        assert_eq!(cfg.branch, "main");
        assert_eq!(cfg.username, "x-access-token");
        assert_eq!(cfg.transport, SyncProtocol::HttpsToken);
        assert!(cfg.remote_url.is_empty());
    }

    #[test]
    fn from_legacy_fields_preserves_non_empty() {
        let cfg = GitHubProviderConfig::from_legacy_fields(
            Some("https://github.com/o/r.git".to_string()),
            Some("dev".to_string()),
            Some("alice".to_string()),
            Some(SyncProtocol::SshDeployKey),
        );
        assert_eq!(cfg.remote_url, "https://github.com/o/r.git");
        assert_eq!(cfg.branch, "dev");
        assert_eq!(cfg.username, "alice");
        assert_eq!(cfg.transport, SyncProtocol::SshDeployKey);
    }

    #[test]
    fn from_persisted_missing_token_yields_auth_failed() {
        let cfg = GitHubProviderConfig::defaults();
        let err = GitHubRuntimeConfig::from_persisted(&cfg, None).expect_err("missing token");
        assert!(matches!(err, ProviderError::AuthFailed { .. }));
    }

    #[test]
    fn from_persisted_with_token_succeeds() {
        let cfg = GitHubProviderConfig {
            remote_url: "https://github.com/owner/repo.git".to_string(),
            branch: "main".to_string(),
            username: "x-access-token".to_string(),
            transport: SyncProtocol::HttpsToken,
        };
        let secrets = ProviderSecrets::GitHub {
            token: "tok".to_string(),
        };
        let runtime = GitHubRuntimeConfig::from_persisted(&cfg, Some(&secrets))
            .expect("valid config + token");
        assert_eq!(
            runtime.api_base_url,
            "https://api.github.com/repos/owner/repo"
        );
        assert_eq!(runtime.token, "tok");
        assert_eq!(runtime.branch, "main");
    }
}
