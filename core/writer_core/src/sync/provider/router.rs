//! 统一同步路由 — 通用 Git HTTPS 优先，GitHub 不可达时自动转 REST API。
//!
//! 用户不选择传输方式；只配置仓库地址、分支、用户名与密码/令牌。
//! - 非 `github.com` 仓库始终使用标准 Git Smart HTTP。
//! - 官方 `github.com` 仓库先探测 Git HTTPS；只有连接/传输不可达
//!   （DNS、超时、连接重置、代理拒绝）才改用 `api.github.com`。
//! - 认证失败、权限不足、TLS 证书错误、non-fast-forward 不做 API 后备。
//! - 一次完整同步只使用一种传输；后备时重新读取远端状态并重新规划。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[cfg(feature = "github-api")]
use crate::sync::provider::error::ProviderError;
use crate::sync::provider::git_https::{
    GitHttpsProvider, GitHttpsProviderConfig, GitHttpsRuntimeConfig,
};
use crate::sync::provider::{ProviderConfig, SyncProvider};
use crate::sync::types::{SyncConfig, SyncSecrets};
use crate::sync::url::sanitize_remote_url;

/// Git 连接不可达的缓存窗口。网络环境变化或重连后重新探测。
const UNREACHABLE_TTL: Duration = Duration::from_secs(10 * 60);
const GIT_CACHE_DIR: &str = "app-meta/sync/git-cache";

static GIT_UNREACHABLE_UNTIL: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

fn unreachable_cache() -> &'static Mutex<Option<Instant>> {
    GIT_UNREACHABLE_UNTIL.get_or_init(|| Mutex::new(None))
}

pub(crate) fn is_git_known_unreachable() -> bool {
    let Ok(guard) = unreachable_cache().lock() else {
        return false;
    };
    match *guard {
        Some(until) if Instant::now() < until => true,
        Some(_) => false,
        None => false,
    }
}

pub(crate) fn mark_git_unreachable() {
    if let Ok(mut guard) = unreachable_cache().lock() {
        *guard = Some(Instant::now() + UNREACHABLE_TTL);
    }
}

pub(crate) fn clear_git_unreachable() {
    if let Ok(mut guard) = unreachable_cache().lock() {
        *guard = None;
    }
}

/// 判断 URL 是否为官方 `github.com` 的 HTTPS 仓库（按主机精确匹配）。
pub fn is_github_https_remote(remote_url: &str) -> bool {
    let sanitized = sanitize_remote_url(remote_url).sanitized_url;
    let Some((scheme, rest)) = sanitized.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let authority = rest.split('/').next().unwrap_or_default();
    let host = authority
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default();
    host.eq_ignore_ascii_case("github.com")
        && rest
            .split_once('/')
            .map(|(_, path)| !path.trim_matches('/').is_empty())
            .unwrap_or(false)
}

fn is_https(remote_url: &str) -> bool {
    sanitize_remote_url(remote_url)
        .sanitized_url
        .to_ascii_lowercase()
        .starts_with("https://")
}

fn cache_dir(app_data_root: &Path, remote_url: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let sanitized = sanitize_remote_url(remote_url).sanitized_url;
    let mut hasher = Sha256::new();
    hasher.update(sanitized.as_bytes());
    let digest = hasher.finalize();
    let key: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    app_data_root.join(GIT_CACHE_DIR).join(key)
}

/// 从持久化配置构造通用 Git HTTPS Provider。
fn git_provider(
    app_data_root: &Path,
    config: &GitHttpsProviderConfig,
    secrets: &SyncSecrets,
) -> crate::error::Result<GitHttpsProvider> {
    let runtime = GitHttpsRuntimeConfig::from_persisted(config, secrets.provider_secrets.as_ref())
        .map_err(crate::Error::from)?;
    let dir = cache_dir(app_data_root, &runtime.remote_url);
    Ok(GitHttpsProvider::new(runtime, dir))
}

/// 官方 GitHub HTTPS 不可达时，把通用配置转换为 REST 配置。
#[cfg(feature = "github-api")]
fn github_api_config(
    config: &GitHttpsProviderConfig,
) -> crate::sync::provider::github::config::GitHubProviderConfig {
    crate::sync::provider::github::config::GitHubProviderConfig {
        remote_url: sanitize_remote_url(&config.remote_url).sanitized_url,
        branch: config.branch.clone(),
        username: if config.username.is_empty() {
            "x-access-token".to_string()
        } else {
            config.username.clone()
        },
        transport: crate::sync::provider::github::config::GitHubTransport::HttpsToken,
    }
}

/// 统一 provider 工厂。
///
/// `create_github_api` 由 facade 注入：完成 transport 初始化并构造 GitHubProvider。
/// 只有 Git HTTPS 因传输不可达失败时才调用，且每次完整同步最多调用一次。
pub fn create_provider<F>(
    config: &SyncConfig,
    secrets: &SyncSecrets,
    app_data_root: &Path,
    create_github_api: F,
) -> crate::error::Result<Box<dyn SyncProvider>>
where
    F: FnOnce(
        &crate::sync::provider::ProviderConfig,
    ) -> crate::error::Result<Box<dyn SyncProvider>>,
{
    match config.provider_config.as_ref() {
        Some(ProviderConfig::GitHttps(git_config)) => create_git_or_fallback(
            git_config,
            secrets,
            app_data_root,
            config,
            create_github_api,
        ),
        #[cfg(feature = "github-api")]
        Some(ProviderConfig::GitHub(github_config)) => {
            // 旧 GitHub 配置：HTTPS 仓库优先走 Git，其余（SSH/Enterprise）保持 API。
            if is_https(&github_config.remote_url) {
                let git_config = GitHttpsProviderConfig {
                    remote_url: github_config.remote_url.clone(),
                    branch: github_config.branch.clone(),
                    username: github_config.username.clone(),
                };
                create_git_or_fallback(
                    &git_config,
                    secrets,
                    app_data_root,
                    config,
                    create_github_api,
                )
            } else {
                create_github_api(&ProviderConfig::GitHub(github_config.clone()))
            }
        }
        None if config.active_provider == "git_https" => Err(crate::Error::Other(
            "missing git https provider config".into(),
        )),
        None => Err(crate::Error::NotImplemented),
        #[cfg(not(feature = "github-api"))]
        Some(_) => Err(crate::Error::NotImplemented),
    }
}

fn create_git_or_fallback<F>(
    git_config: &GitHttpsProviderConfig,
    secrets: &SyncSecrets,
    app_data_root: &Path,
    _config: &SyncConfig,
    create_github_api: F,
) -> crate::error::Result<Box<dyn SyncProvider>>
where
    F: FnOnce(&ProviderConfig) -> crate::error::Result<Box<dyn SyncProvider>>,
{
    let provider = git_provider(app_data_root, git_config, secrets)?;

    if !is_github_https_remote(&provider.config().remote_url) {
        // 通用 Git 平台（GitLab/Gitee/Gitea/Forgejo/Bitbucket/自建）直接使用。
        log::info!(
            "sync.route: provider=git_https host_class=other fallback=false phase=git_probe"
        );
        return Ok(Box::new(provider));
    }

    #[cfg(feature = "github-api")]
    {
        if is_git_known_unreachable() {
            log::warn!(
                "sync.route: provider=github_api host_class=github fallback=true phase=git_probe reason_code=network_cached"
            );
            return fallback_to_github_api(git_config, create_github_api, None);
        }
        match provider.probe() {
            Ok(()) => {
                clear_git_unreachable();
                log::info!(
                    "sync.route: provider=git_https host_class=github fallback=false phase=git_probe"
                );
                Ok(Box::new(provider))
            }
            // 仅底层传输不可达（DNS/连接/超时/握手中断）允许 REST 后备。
            // TLS 证书、CA、认证、本地缓存与 remote 配置错误必须原样上报。
            Err(ProviderError::Network { reason }) => {
                mark_git_unreachable();
                log::warn!(
                    "sync.route: provider=github_api host_class=github fallback=true phase=git_probe reason_code=network"
                );
                fallback_to_github_api(git_config, create_github_api, Some(reason))
            }
            Err(error) => {
                log::warn!(
                    "sync.route: provider=git_https host_class=github fallback=false phase=git_probe reason_code={}",
                    error.stable_code()
                );
                Err(crate::Error::from(error))
            }
        }
    }

    #[cfg(not(feature = "github-api"))]
    {
        let _ = create_github_api;
        match provider.probe() {
            Ok(()) => {
                log::info!(
                    "sync.route: provider=git_https host_class=github fallback=false phase=git_probe"
                );
                Ok(Box::new(provider))
            }
            Err(error) => {
                log::warn!(
                    "sync.route: provider=git_https host_class=github fallback=false phase=git_probe reason_code={}",
                    error.stable_code()
                );
                Err(crate::Error::from(error))
            }
        }
    }
}

#[cfg(feature = "github-api")]
fn fallback_to_github_api<F>(
    git_config: &GitHttpsProviderConfig,
    create_github_api: F,
    git_error: Option<String>,
) -> crate::error::Result<Box<dyn SyncProvider>>
where
    F: FnOnce(&ProviderConfig) -> crate::error::Result<Box<dyn SyncProvider>>,
{
    let api_config = github_api_config(git_config);
    log::info!(
        "sync.route: provider=github_api host_class=github fallback=true phase=github_api_fallback_init"
    );
    match create_github_api(&ProviderConfig::GitHub(api_config)) {
        Ok(provider) => {
            log::info!(
                "sync.route: provider=github_api host_class=github fallback=true phase=github_api_fallback_init result=ok"
            );
            Ok(provider)
        }
        Err(api_error) => {
            let category = api_error.sync_category();
            log::warn!(
                "sync.route: provider=github_api host_class=github fallback=true phase=github_api_fallback_init result=error reason_code={}",
                if category.is_empty() { "other" } else { category }
            );
            match git_error {
                // 保留 API 自身的错误分类（例如令牌缺失/无效仍是认证错误），
                // 同时附带 Git HTTPS 不可达原因，避免把认证失败误报为网络被墙。
                Some(git_reason) => match api_error {
                    crate::Error::SyncAuthFailed { reason } => Err(crate::Error::SyncAuthFailed {
                        reason: format!("{reason} (Git HTTPS also unavailable: {git_reason})"),
                    }),
                    crate::Error::SyncNetworkUnavailable { reason } => {
                        Err(crate::Error::SyncNetworkUnavailable {
                            reason: format!("{reason} (Git HTTPS also unavailable: {git_reason})"),
                        })
                    }
                    other => Err(other),
                },
                None => Err(api_error),
            }
        }
    }
}
