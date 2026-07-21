//! URL 解析与凭证脱敏 — 同步模块的 URL 处理工具。
//!
//! 核心职责：
//! - 从嵌入凭证的 URL（`https://user:token@host/path`）中提取并剥离 userinfo
//! - 检测传输方式（HTTPS token vs SSH deploy key）
//! - 日志和错误消息中的凭证脱敏（`mask_token_in_url` / `redact_secrets_from_message`）
//!
//! 安全约束：脱敏函数必须保证 token 不出现在日志、错误消息和 UI 展示中。
//! `redact_secrets_from_message` 是当前主链，`mask_token` 是遗留别名。

use crate::sync::types::BackendType;
use crate::sync::types::SyncConfig;
use crate::sync::types::SyncTransport;

/// URL 解析结果 — 剥离 userinfo 后的 URL 和提取的凭证。
pub struct ParsedRemoteUrl {
    pub sanitized_url: String,
    pub extracted_username: Option<String>,
    pub extracted_token: Option<String>,
}

/// 从嵌入凭证的 URL 中剥离 userinfo，返回脱敏 URL 和提取的 username/token。
///
/// 输入格式：`https://user:token@github.com/owner/repo.git`
/// 输出：`sanitized_url = "https://github.com/owner/repo.git"`, `extracted_username = Some("user")`, `extracted_token = Some("token")`
/// 无 userinfo 的 URL 原样返回。
pub fn sanitize_remote_url(url: &str) -> ParsedRemoteUrl {
    if url.contains("://") && url.contains('@') {
        if let Some(after_scheme) = url.split_once("://") {
            let scheme = after_scheme.0;
            let rest = after_scheme.1;
            if let Some(at_pos) = rest.find('@') {
                let userinfo = &rest[..at_pos];
                let host_and_path = &rest[at_pos + 1..];
                let sanitized = format!("{}://{}", scheme, host_and_path);
                let (username, token) = if let Some(colon_pos) = userinfo.find(':') {
                    (
                        Some(url_decode(userinfo[..colon_pos].to_string())),
                        Some(url_decode(userinfo[colon_pos + 1..].to_string())),
                    )
                } else {
                    (Some(url_decode(userinfo.to_string())), None)
                };
                return ParsedRemoteUrl {
                    sanitized_url: sanitized,
                    extracted_username: username,
                    extracted_token: token,
                };
            }
        }
    }
    ParsedRemoteUrl {
        sanitized_url: url.to_string(),
        extracted_username: None,
        extracted_token: None,
    }
}

fn url_decode(s: String) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next();
            let h2 = chars.next();
            if let (Some(h1), Some(h2)) = (h1, h2) {
                if let Ok(byte) = u8::from_str_radix(&format!("{}{}", h1, h2), 16) {
                    result.push(byte as char);
                } else {
                    result.push('%');
                    result.push(h1);
                    result.push(h2);
                }
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

/// 根据远程 URL 的协议前缀检测传输方式。
///
/// `git@` 或 `ssh://` 开头返回 `SshDeployKey`，其余返回 `HttpsToken`。
pub fn detect_transport(remote_url: &str) -> SyncTransport {
    let lower = remote_url.to_lowercase();
    if lower.starts_with("git@") || lower.starts_with("ssh://") {
        SyncTransport::SshDeployKey
    } else {
        SyncTransport::HttpsToken
    }
}

pub fn is_github_https_remote(remote_url: &str) -> bool {
    let sanitized = sanitize_remote_url(remote_url).sanitized_url;
    let lower = sanitized.to_lowercase();
    lower.starts_with("https://github.com/") || lower.starts_with("http://github.com/")
}

/// 解析最终使用的后端类型 — 若配置为 Git 但 URL 为 GitHub HTTPS，自动切换为 GithubApi。
///
/// GitHub HTTPS 远程仓库使用 REST API 更高效（无需 clone 整个仓库），
/// 因此当 `config.backend_type == Git` 且 URL 为 `https://github.com/` 时自动升级。
pub fn resolved_backend_type(config: &SyncConfig) -> BackendType {
    if config.backend_type == BackendType::Git && is_github_https_remote(&config.remote_url) {
        BackendType::GithubApi
    } else {
        config.backend_type.clone()
    }
}

/// 将 URL 中的 userinfo 部分替换为 `***`，用于日志和 UI 展示。
///
/// `https://user:pass@host/path` → `https://***@host/path`
/// 无 userinfo 的 URL 原样返回。
pub fn mask_token_in_url(url: &str) -> String {
    if url.contains('@') {
        if let Some(after_scheme) = url.split_once("://") {
            let scheme = after_scheme.0;
            let rest = after_scheme.1;
            if let Some(at_pos) = rest.find('@') {
                return format!("{}://***@{}", scheme, &rest[at_pos + 1..]);
            }
        } else {
            if let Some(at_pos) = url.find('@') {
                return format!("***@{}", &url[at_pos + 1..]);
            }
        }
    }
    url.to_string()
}

/// Redact known secrets (token, password) from a diagnostic/error message.
/// This is a SAFE replacement for `mask_token` which was destroying the entire error.
/// Strategy:
/// 1. Redact URL userinfo (https://user:token@host/path -> https://***@host/path)
/// 2. If a known token string is provided, redact every occurrence of it.
/// 3. Does NOT touch ordinary error text, git return codes, or libgit2 messages.
pub fn redact_secrets_from_message(
    msg: &str,
    known_token: Option<&str>,
    remote_url: Option<&str>,
) -> String {
    let mut result = msg.to_string();

    // 1. Always redact URL userinfo
    if let Some(url) = remote_url {
        if url.contains('@') {
            if let Some((prefix, _)) = url.split_once("://") {
                result = result.replace(url, &format!("{}://***@...", prefix));
            } else {
                result = result.replace(url, "***@...");
            }
        }
    } else {
        // Generic URL userinfo redaction if no specific URL given
        result = mask_token_in_url(&result);
    }

    // 2. Redact known token if provided
    if let Some(token) = known_token {
        if !token.is_empty() && token.len() >= 4 {
            result = result.replace(token, "***REDACTED***");
        }
    }

    // 3. Redact any remaining embedded URLs with userinfo
    // Pattern: https://something@...
    let mut found = true;
    while found {
        found = false;
        if let Some(start) = result.find("://") {
            let before = &result[..start];
            // Look backwards for start of scheme
            let scheme_start = before
                .rfind(|c: char| !c.is_alphanumeric() && c != '+' && c != '-' && c != '.')
                .map(|p| p + 1)
                .unwrap_or(0);
            let scheme = &result[scheme_start..start];
            if scheme == "http"
                || scheme == "https"
                || scheme == "ssh"
                || scheme == "git"
                || scheme == "socks5"
                || scheme == "socks5h"
            {
                let rest = &result[start + 3..];
                if let Some(at_pos) = rest.find('@') {
                    let before_at = &rest[..at_pos];
                    if before_at.contains(':') || before_at.contains('%') {
                        // Has userinfo (contains colon or percent-encoded chars)
                        let redacted = format!("{}://***@", scheme);
                        let after_at = &rest[at_pos + 1..];
                        // Find end (space, newline, comma, end-of-string)
                        let end = after_at
                            .find(|c: char| c.is_whitespace() || c == ',' || c == ')' || c == ']')
                            .unwrap_or(after_at.len());
                        let full = format!("{}://{}{}", scheme, before_at, &after_at[..end]);
                        let replacement = format!("{}***@{}", redacted, &after_at[..end]);
                        if let Some(pos) = result.find(&full) {
                            result.replace_range(pos..pos + full.len(), &replacement);
                            found = true;
                        }
                    }
                }
            }
        }
    }

    result
}

/// Legacy token masking function - now just an alias for redact_secrets_from_message
/// without known secrets. This prevents the old behavior of masking the entire error message.
pub fn mask_token(s: &str) -> String {
    redact_secrets_from_message(s, None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_token_in_url() {
        assert_eq!(
            mask_token_in_url("https://user:pass@github.com/repo.git"),
            "https://***@github.com/repo.git"
        );
        assert_eq!(
            mask_token_in_url("user:pass@github.com/repo.git"),
            "***@github.com/repo.git"
        );
        assert_eq!(
            mask_token_in_url("git@github.com:user/repo.git"),
            "***@github.com:user/repo.git"
        );
        assert_eq!(
            mask_token_in_url("https://github.com/repo.git"),
            "https://github.com/repo.git"
        );
        assert_eq!(mask_token_in_url("Just some text"), "Just some text");
    }
}
