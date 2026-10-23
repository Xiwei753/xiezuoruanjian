//! Git HTTPS Provider 配置 — 适用于所有标准 Git Smart HTTP 远端。
//!
//! 持久化配置只包含仓库地址、分支与可选用户名；密码/访问令牌由
//! `SyncSecrets.provider_secrets` 注入，绝不写入 config.json，也不拼进 URL。
//! 客户端不识别 GitHub/GitLab/Gitee 等厂商的令牌格式，凭据由远端自行判定。

use crate::sync::provider::error::ProviderError;
use crate::sync::provider::ProviderSecrets;
use crate::sync::url::sanitize_remote_url;

/// Git HTTPS 持久化配置（安全写入 config.local.json）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GitHttpsProviderConfig {
    /// 仓库 HTTPS 地址，不含明文凭据。
    pub remote_url: String,
    /// 远端分支名，默认 `main`。
    pub branch: String,
    /// HTTPS Basic 认证用户名；允许为空，表示由凭据回调自动选择。
    #[serde(default)]
    pub username: String,
}

impl GitHttpsProviderConfig {
    pub fn defaults() -> Self {
        Self {
            remote_url: String::new(),
            branch: "main".to_string(),
            username: String::new(),
        }
    }

    /// 从旧 `SyncConfig` 顶层字段迁移。
    pub fn from_legacy_fields(
        remote_url: Option<String>,
        branch: Option<String>,
        username: Option<String>,
    ) -> Self {
        Self {
            remote_url: remote_url.unwrap_or_default(),
            branch: branch
                .filter(|b| !b.is_empty())
                .unwrap_or_else(|| "main".to_string()),
            username: username.unwrap_or_default(),
        }
    }
}

impl Default for GitHttpsProviderConfig {
    fn default() -> Self {
        Self::defaults()
    }
}

/// Git HTTPS 运行时配置（含密钥，不持久化）。
#[derive(Debug, Clone)]
pub struct GitHttpsRuntimeConfig {
    /// 脱敏后的仓库 HTTPS 地址。
    pub remote_url: String,
    pub branch: String,
    /// 用户显式填写的用户名；为空表示自动。
    pub username: String,
    /// 密码 / PAT / API token；`None` 或空串表示匿名读取。
    pub secret: Option<String>,
}

impl GitHttpsRuntimeConfig {
    pub fn from_persisted(
        config: &GitHttpsProviderConfig,
        secrets: Option<&ProviderSecrets>,
    ) -> Result<Self, ProviderError> {
        let parsed = sanitize_remote_url(&config.remote_url);
        if parsed.sanitized_url.trim().is_empty() {
            return Err(ProviderError::Other {
                reason: "repository URL is empty".to_string(),
            });
        }
        let lower = parsed.sanitized_url.to_ascii_lowercase();
        if !lower.starts_with("https://") {
            return Err(ProviderError::AuthFailed {
                reason: "Git HTTPS provider requires an https:// repository URL".to_string(),
            });
        }

        // 兼容旧配置把 user:token 写在 URL 里的情况：提取后作为回退，
        // 但对外始终使用脱敏 URL。
        let username = if config.username.trim().is_empty() {
            parsed.extracted_username.unwrap_or_default()
        } else {
            config.username.clone()
        };
        let secret = secrets
            .and_then(|s| s.any_secret())
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .or(parsed.extracted_token)
            .filter(|s| !s.is_empty());

        Ok(Self {
            remote_url: parsed.sanitized_url,
            branch: if config.branch.trim().is_empty() {
                "main".to_string()
            } else {
                config.branch.clone()
            },
            username,
            secret,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_use_main_and_auto_username() {
        let cfg = GitHttpsProviderConfig::defaults();
        assert_eq!(cfg.branch, "main");
        assert!(cfg.username.is_empty());
    }

    #[test]
    fn runtime_strips_embedded_credentials() {
        let cfg = GitHttpsProviderConfig {
            remote_url: "https://alice:secret@example.com/o/r.git".to_string(),
            branch: "".to_string(),
            username: "".to_string(),
        };
        let runtime = GitHttpsRuntimeConfig::from_persisted(&cfg, None).unwrap();
        assert_eq!(runtime.remote_url, "https://example.com/o/r.git");
        assert_eq!(runtime.username, "alice");
        assert_eq!(runtime.secret.as_deref(), Some("secret"));
        assert_eq!(runtime.branch, "main");
    }

    #[test]
    fn explicit_username_wins_over_url_userinfo() {
        let cfg = GitHttpsProviderConfig {
            remote_url: "https://alice:secret@example.com/o/r.git".to_string(),
            branch: "dev".to_string(),
            username: "bob".to_string(),
        };
        let runtime = GitHttpsRuntimeConfig::from_persisted(&cfg, None).unwrap();
        assert_eq!(runtime.username, "bob");
    }

    #[test]
    fn rejects_non_https_url() {
        let cfg = GitHttpsProviderConfig {
            remote_url: "git@github.com:o/r.git".to_string(),
            ..GitHttpsProviderConfig::defaults()
        };
        assert!(matches!(
            GitHttpsRuntimeConfig::from_persisted(&cfg, None),
            Err(ProviderError::AuthFailed { .. })
        ));
    }
}
