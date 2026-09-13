//! 脱敏规则 — 合并 Android `DiagnosticsLogger.REDACT_RULES` 与 Linux `diagnostics.rs::redact`。
//!
//! 提供 [`redact`] 入口：把消息中可能包含的敏感信息替换为 `[REDACTED]`。
//! 为性能先做廉价前置判断 [`may_contain_sensitive_data`]，无敏感标记时直接返回原文。
//!
//! 规则覆盖（与 Android/Linux 现有实现对齐）：
//! - SSH private key blocks
//! - PEM private key blocks
//! - Bearer token (Authorization: Bearer xxx)
//! - Sensitive key=value (token, password, secret, access_token, refresh_token, private_key, …)
//! - Content/body/chapter key=value（用户内容）
//! - Bearer tokens standalone
//! - GitHub PAT patterns (ghp_, gho_, github_pat_)

use std::sync::OnceLock;

use regex::Regex;

/// 敏感字段名标记（大小写不敏感）— 与 REDACT_RULES 中的 key 保持一致。
///
/// 廉价前置判断用：消息中不包含任一标记时直接返回原文，避免每条结构事件都跑全部 Regex。
const SENSITIVE_MARKERS_CI: &[&str] = &[
    "token",
    "password",
    "passwd",
    "secret",
    "authorization",
    "private_key",
    "ssh_private_key",
    "access_token",
    "refresh_token",
    "bearer",
    "ghp_",
    "gho_",
    "github_pat_",
    "content",
    "chapter",
    "text",
    "body",
];

/// PEM 私钥结束标记（大小写敏感）。
const PEM_END_MARKER: &str = "PRIVATE KEY-----";

/// 廉价判断消息是否可能包含需要脱敏的敏感字段。
/// 只做子串/字符检查，不跑 Regex；结构事件（nav.destination 等）快速返回 false。
pub fn may_contain_sensitive_data(message: &str) -> bool {
    message.contains(PEM_END_MARKER)
        || SENSITIVE_MARKERS_CI
            .iter()
            .any(|marker| message.to_lowercase().contains(marker))
}

/// 预编译的脱敏规则集合。`OnceLock` 保证只编译一次。
struct RedactRules {
    ssh_key: Regex,
    pem: Regex,
    bearer_header: Regex,
    sensitive_kv: Regex,
    content_kv: Regex,
    bearer_json: Regex,
    sensitive_json: Regex,
    content_json: Regex,
    bearer_standalone: Regex,
    ghp: Regex,
    gho: Regex,
    github_pat: Regex,
}

impl RedactRules {
    fn new() -> Self {
        // Regex::new 在编译期固定模式，失败属于编程错误。用 `expect` 在初始化时
        // 暴露编程错误；局部 allow 因为这些模式是编译期常量，不是外部输入。
        #[allow(clippy::unwrap_used)]
        let p = |pat: &str| Regex::new(pat).unwrap();
        Self {
            ssh_key: p(
                r"(?i)ssh_private_key\s*[:=]\s*[\s\S]*?-----END[^\n]*PRIVATE KEY-----",
            ),
            pem: p(
                r"-----BEGIN[^\n]*PRIVATE KEY-----[\s\S]*?-----END[^\n]*PRIVATE KEY-----",
            ),
            bearer_header: p(r"(?i)\b(authorization)\s*[:=]\s*Bearer\s+\S+"),
            sensitive_kv: p(
                r#"(?i)\b(token|access_token|refresh_token|authorization|password|passwd|secret|private_key)\s*[:=]\s*(?:"[^"]*"|\S+)"#,
            ),
            content_kv: p(
                r#"(?i)\b(content|text|body|chapter|chapter_content|chapterContent)\s*[:=]\s*(?:"[^"]*"|[^,}\]\n]+)"#,
            ),
            bearer_json: p(
                r#"(?i)["'](authorization)["']\s*:\s*["']Bearer\s+[^"\\]*(?:\\.[^"\\]*)*["']"#,
            ),
            sensitive_json: p(
                r#"(?i)["'](token|access_token|refresh_token|authorization|password|passwd|secret|private_key|ssh_private_key)["']\s*:\s*["'][^"\\]*(?:\\.[^"\\]*)*["']"#,
            ),
            content_json: p(
                r#"(?i)["'](content|text|body|chapter|chapter_content|chapterContent)["']\s*:\s*["'][^"\\]*(?:\\.[^"\\]*)*["']"#,
            ),
            bearer_standalone: p(r"(?i)Bearer\s+[A-Za-z0-9\-._~+/]+=*"),
            ghp: p(r"ghp_[A-Za-z0-9]{36}"),
            gho: p(r"gho_[A-Za-z0-9]{36}"),
            github_pat: p(r"github_pat_[A-Za-z0-9_]{82}"),
        }
    }
}

static RULES: OnceLock<RedactRules> = OnceLock::new();

fn rules() -> &'static RedactRules {
    RULES.get_or_init(RedactRules::new)
}

/// 把消息中可能包含的敏感信息替换为 `[REDACTED]`。
///
/// 廉价前置判断：无敏感标记时直接返回原文，避免结构事件每条都跑全部 Regex。
/// 规则顺序与 Android `DiagnosticsLogger.redact` 对齐。
pub fn redact(message: &str) -> String {
    if !may_contain_sensitive_data(message) {
        return message.to_string();
    }
    let r = rules();
    let mut result = message.to_string();
    result = r.ssh_key.replace_all(&result, "ssh_private_key=[REDACTED]").into_owned();
    result = r.pem.replace_all(&result, "[REDACTED_PEM]").into_owned();
    result = r
        .bearer_header
        .replace_all(&result, "Authorization: Bearer [REDACTED]")
        .into_owned();
    result = r.sensitive_kv.replace_all(&result, "$1=[REDACTED]").into_owned();
    result = r.content_kv.replace_all(&result, "$1=[REDACTED]").into_owned();
    result = r
        .bearer_json
        .replace_all(&result, "\"authorization\": \"Bearer [REDACTED]\"")
        .into_owned();
    result = r
        .sensitive_json
        .replace_all(&result, "\"$1\": \"[REDACTED]\"")
        .into_owned();
    result = r
        .content_json
        .replace_all(&result, "\"$1\": \"[REDACTED]\"")
        .into_owned();
    result = r
        .bearer_standalone
        .replace_all(&result, "Bearer [REDACTED]")
        .into_owned();
    result = r.ghp.replace_all(&result, "[REDACTED]").into_owned();
    result = r.gho.replace_all(&result, "[REDACTED]").into_owned();
    result = r.github_pat.replace_all(&result, "[REDACTED]").into_owned();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_sensitive_data_returns_original() {
        let msg = "theme.appearance_select requested=dark";
        assert_eq!(redact(msg), msg);
        assert!(!may_contain_sensitive_data(msg));
    }

    #[test]
    fn redacts_bearer_token() {
        let msg = "Authorization: Bearer abc123secret";
        let redacted = redact(msg);
        assert!(redacted.contains("[REDACTED]"), "redacted: {redacted}");
        assert!(!redacted.contains("abc123secret"));
    }

    #[test]
    fn redacts_password_kv() {
        let msg = "password=hunter2";
        let redacted = redact(msg);
        assert!(redacted.contains("[REDACTED]"), "redacted: {redacted}");
        assert!(!redacted.contains("hunter2"));
    }

    #[test]
    fn redacts_token_kv() {
        let msg = "token=my-secret-token";
        let redacted = redact(msg);
        assert!(redacted.contains("[REDACTED]"), "redacted: {redacted}");
        assert!(!redacted.contains("my-secret-token"));
    }

    #[test]
    fn redacts_pem_block() {
        let msg = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA\n-----END RSA PRIVATE KEY-----";
        let redacted = redact(msg);
        assert!(redacted.contains("[REDACTED_PEM]"), "redacted: {redacted}");
        assert!(!redacted.contains("MIIEpAIBAAKCAQEA"));
    }

    #[test]
    fn redacts_github_pat() {
        let msg = "ghp_0123456789012345678901234567890123456";
        let redacted = redact(msg);
        assert!(redacted.contains("[REDACTED]"), "redacted: {redacted}");
    }

    #[test]
    fn redacts_content_kv() {
        let msg = "content=\"my secret chapter text\"";
        let redacted = redact(msg);
        assert!(redacted.contains("[REDACTED]"), "redacted: {redacted}");
        assert!(!redacted.contains("my secret chapter text"));
    }

    #[test]
    fn redacts_json_sensitive_field() {
        let msg = r#"{"token": "abc123", "other": "safe"}"#;
        let redacted = redact(msg);
        assert!(redacted.contains("[REDACTED]"), "redacted: {redacted}");
        assert!(!redacted.contains("abc123"));
    }
}
