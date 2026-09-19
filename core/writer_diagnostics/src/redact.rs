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
            ssh_key: p(r"(?i)ssh_private_key\s*[:=]\s*[\s\S]*?-----END[^\n]*PRIVATE KEY-----"),
            pem: p(r"-----BEGIN[^\n]*PRIVATE KEY-----[\s\S]*?-----END[^\n]*PRIVATE KEY-----"),
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
///
/// 注意：本函数面向**自由文本日志**（logcat、thread dump、panic message 等），
/// 会用 KV/JSON 文本正则匹配 `key=value` / `"key":"value"` 模式。**不要**把它
/// 用在合法 JSON 附件上——`chapter-body:...` 里的 `body:` 会被误认成正文键，
/// 进而吃掉字符串结束引号，产出非法 JSON（Issue #717 评论 5741567193）。
/// JSON 附件请用 [`redact_json`] 做结构化脱敏。
pub fn redact(message: &str) -> String {
    if !may_contain_sensitive_data(message) {
        return message.to_string();
    }
    let r = rules();
    let mut result = message.to_string();
    result = r
        .ssh_key
        .replace_all(&result, "ssh_private_key=[REDACTED]")
        .into_owned();
    result = r.pem.replace_all(&result, "[REDACTED_PEM]").into_owned();
    result = r
        .bearer_header
        .replace_all(&result, "Authorization: Bearer [REDACTED]")
        .into_owned();
    result = r
        .sensitive_kv
        .replace_all(&result, "$1=[REDACTED]")
        .into_owned();
    result = r
        .content_kv
        .replace_all(&result, "$1=[REDACTED]")
        .into_owned();
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

// ── JSON 结构化脱敏 — Issue #717 评论 5741567193 ──────────────────────────
//
// 自由文本 `redact()` 的 KV/JSON 正则会把 `chapter-body:...` 里的 `body:` 误认
// 成正文键，吃掉字符串结束引号，产出非法 JSON。JSON 附件必须走结构化脱敏：
// 按 serde_json::Value 递归，命中敏感 key 直接替换 value，普通字符串值只扫
// bearer/PEM/PAT 这类字符串内部秘密，不跑 KV 文本正则。

/// JSON object key 命中即视为敏感字段的列表（大小写不敏感匹配 key）。
const SENSITIVE_KEYS_CI: &[&str] = &[
    "token",
    "access_token",
    "refresh_token",
    "authorization",
    "password",
    "passwd",
    "secret",
    "private_key",
    "ssh_private_key",
    "content",
    "text",
    "body",
    "chapter",
    "chapter_content",
    "chapterContent",
];

/// 判断 JSON object key 是否命中敏感字段（大小写不敏感）。
///
/// 敏感 key 都是 ASCII，用 `eq_ignore_ascii_case` 做大小写不敏感比较，
/// 不分配、不依赖 Unicode 折叠。
pub fn is_sensitive_key(key: &str) -> bool {
    SENSITIVE_KEYS_CI
        .iter()
        .any(|s| s.eq_ignore_ascii_case(key))
}

/// 对字符串值做轻量秘密扫描：只处理 bearer / PEM / GitHub PAT 这类**字符串内部**
/// 模式，不调用完整 KV/JSON 文本正则。
///
/// 这避免把 `chapter-body:p:v:c` 里的 `body:` 误认成正文键。普通字符串值
/// （targetId、event 名等）大多不含秘密标记，前置判断直接返回原文。
pub fn redact_string_secrets(s: &str) -> String {
    // 廉价前置判断：不含任何秘密标记时直接返回，避免每个字符串值都跑 Regex。
    let lower = s.to_ascii_lowercase();
    if !s.contains(PEM_END_MARKER)
        && !lower.contains("bearer")
        && !s.contains("ghp_")
        && !s.contains("gho_")
        && !s.contains("github_pat_")
    {
        return s.to_string();
    }
    let r = rules();
    let mut result = s.to_string();
    result = r.pem.replace_all(&result, "[REDACTED_PEM]").into_owned();
    result = r
        .bearer_standalone
        .replace_all(&result, "Bearer [REDACTED]")
        .into_owned();
    result = r.ghp.replace_all(&result, "[REDACTED]").into_owned();
    result = r.gho.replace_all(&result, "[REDACTED]").into_owned();
    result = r.github_pat.replace_all(&result, "[REDACTED]").into_owned();
    result
}

/// 递归对 JSON value 做结构化脱敏 — Issue #717 评论 5741567193。
///
/// - Object 的 key 命中敏感字段（[`is_sensitive_key`]）时，直接把 value 替换
///   成 `"[REDACTED]"`，不再递归进该 value。
/// - 普通字符串值（非敏感 key 下的）只做 bearer / PEM / GitHub PAT 字符串内部
///   秘密扫描（[`redact_string_secrets`]），不调用完整 KV 文本正则。
/// - 递归进入 Array 元素和非敏感 key 的 Object value。
///
/// 这保证合法 JSON 在脱敏后仍是合法 JSON：只整体替换 value 或原地改字符串，
/// 不触碰结构字符（引号、逗号、括号）。
pub fn redact_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            // 先收集 key 列表，避免在循环中一边读 key 一边 get_mut 借用冲突。
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                match (is_sensitive_key(&key), map.get_mut(&key)) {
                    (true, Some(v)) => {
                        *v = serde_json::Value::String("[REDACTED]".to_string());
                    }
                    (false, Some(v)) => {
                        redact_json(v);
                    }
                    _ => {}
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                redact_json(v);
            }
        }
        serde_json::Value::String(s) => {
            *s = redact_string_secrets(s);
        }
        _ => {}
    }
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
        let msg =
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA\n-----END RSA PRIVATE KEY-----";
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

    // ── redact_json 结构化脱敏测试 — Issue #717 评论 5741567193 ──────────

    /// 真实 editor 事件导出后 JSON 结构不被破坏，targetId 中的 `body` 是
    /// 字符串值的一部分（不是 key），不应被脱敏。
    #[test]
    fn redact_json_preserves_valid_editor_event() {
        let input = r#"[{"event":"editor.layout.presented","targetId":"chapter-body:p:v:c","layoutTextLength":46}]"#;
        let mut value: serde_json::Value =
            serde_json::from_str(input).expect("input must be valid JSON");
        redact_json(&mut value);
        // 重新序列化再解析，验证结构不被破坏。
        let out = serde_json::to_string(&value).expect("redacted must serialize");
        let reparsed: serde_json::Value =
            serde_json::from_str(&out).expect("redacted must reparse");
        let arr = reparsed.as_array().expect("top level must be array");
        assert_eq!(arr.len(), 1);
        let obj = arr[0].as_object().expect("element must be object");
        assert_eq!(
            obj.get("event").and_then(|v| v.as_str()),
            Some("editor.layout.presented"),
        );
        // targetId 里的 body 是字符串值的一部分，不是 key，必须保留原值。
        assert_eq!(
            obj.get("targetId").and_then(|v| v.as_str()),
            Some("chapter-body:p:v:c"),
        );
        assert_eq!(obj.get("layoutTextLength").and_then(|v| v.as_i64()), Some(46));
    }

    /// content / token 命中敏感 key，value 整体替换成 [REDACTED]。
    #[test]
    fn redact_json_redacts_sensitive_values() {
        let input = r#"{"content":"正文","token":"secret"}"#;
        let mut value: serde_json::Value =
            serde_json::from_str(input).expect("input must be valid JSON");
        redact_json(&mut value);
        let obj = value.as_object().expect("top level must be object");
        assert_eq!(obj.get("content").and_then(|v| v.as_str()), Some("[REDACTED]"));
        assert_eq!(obj.get("token").and_then(|v| v.as_str()), Some("[REDACTED]"));
    }

    /// 嵌套 Object / Array 中的敏感字段也要递归脱敏。
    #[test]
    fn redact_json_handles_nested() {
        let input = r#"{"outer":{"token":"secret","safe":"keep"},"arr":[{"password":"p","note":"n"}]}"#;
        let mut value: serde_json::Value =
            serde_json::from_str(input).expect("input must be valid JSON");
        redact_json(&mut value);
        let outer = value
            .get("outer")
            .and_then(|v| v.as_object())
            .expect("outer must be object");
        assert_eq!(outer.get("token").and_then(|v| v.as_str()), Some("[REDACTED]"));
        assert_eq!(outer.get("safe").and_then(|v| v.as_str()), Some("keep"));
        let arr = value
            .get("arr")
            .and_then(|v| v.as_array())
            .expect("arr must be array");
        assert_eq!(arr.len(), 1);
        let elem = arr[0].as_object().expect("element must be object");
        assert_eq!(
            elem.get("password").and_then(|v| v.as_str()),
            Some("[REDACTED]"),
        );
        assert_eq!(elem.get("note").and_then(|v| v.as_str()), Some("n"));
    }

    /// 普通字符串值中的 bearer / PEM / GitHub PAT 被脱敏，但 KV 正则不触发，
    /// 不会破坏字符串结构。
    #[test]
    fn redact_json_redacts_string_secrets() {
        let input = r#"{"note":"Authorization: Bearer abc123secret","pat":"ghp_0123456789012345678901234567890123456"}"#;
        let mut value: serde_json::Value =
            serde_json::from_str(input).expect("input must be valid JSON");
        redact_json(&mut value);
        let out = serde_json::to_string(&value).expect("redacted must serialize");
        // 重新解析成功 → JSON 结构完整。
        let reparsed: serde_json::Value =
            serde_json::from_str(&out).expect("redacted must reparse");
        let obj = reparsed.as_object().expect("top level must be object");
        let note = obj.get("note").and_then(|v| v.as_str()).expect("note present");
        assert!(note.contains("[REDACTED]"), "note: {note}");
        assert!(!note.contains("abc123secret"));
        let pat = obj.get("pat").and_then(|v| v.as_str()).expect("pat present");
        assert!(pat.contains("[REDACTED]"), "pat: {pat}");
    }

    /// is_sensitive_key 大小写不敏感匹配。
    #[test]
    fn is_sensitive_key_case_insensitive() {
        assert!(is_sensitive_key("token"));
        assert!(is_sensitive_key("Token"));
        assert!(is_sensitive_key("TOKEN"));
        assert!(is_sensitive_key("chapterContent"));
        assert!(is_sensitive_key("chaptercontent"));
        assert!(is_sensitive_key("CHAPTER_CONTENT"));
        assert!(!is_sensitive_key("targetId"));
        assert!(!is_sensitive_key("event"));
    }
}
