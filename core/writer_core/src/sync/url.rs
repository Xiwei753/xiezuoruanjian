use crate::sync::types::BackendType;
use crate::sync::types::SyncConfig;
use crate::sync::types::SyncTransport;

pub struct ParsedRemoteUrl {
    pub sanitized_url: String,
    pub extracted_username: Option<String>,
    pub extracted_token: Option<String>,
}

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

pub fn detect_transport(remote_url: &str) -> SyncTransport {
    let lower = remote_url.to_lowercase();
    if lower.starts_with("git@") || lower.starts_with("ssh://") {
        SyncTransport::SshDeployKey
    } else if lower.starts_with("https://") || lower.starts_with("http://") {
        SyncTransport::HttpsToken
    } else {
        SyncTransport::HttpsToken
    }
}

pub fn is_github_https_remote(remote_url: &str) -> bool {
    let sanitized = sanitize_remote_url(remote_url).sanitized_url;
    let lower = sanitized.to_lowercase();
    lower.starts_with("https://github.com/") || lower.starts_with("http://github.com/")
}

pub fn resolved_backend_type(config: &SyncConfig) -> BackendType {
    if config.backend_type == BackendType::Git && is_github_https_remote(&config.remote_url) {
        BackendType::GithubApi
    } else {
        config.backend_type.clone()
    }
}

pub fn mask_token_in_url(url: &str) -> String {
    if url.contains('@') {
        if let Some(after_scheme) = url.split_once("://") {
            let scheme = after_scheme.0;
            let rest = after_scheme.1;
            if let Some(at_pos) = rest.find('@') {
                return format!("{}://***@{}", scheme, &rest[at_pos + 1..]);
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
            if let Some(prefix) = url.split("://").next() {
                result = result.replace(url, &format!("{}://***@...", prefix));
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
