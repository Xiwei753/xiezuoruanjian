//! redact.rs 的单元测试。
//!
//! 按仓库结构门禁（`tools/check_source_structure.py` 的
//! production-test-bloat 规则）从生产文件拆出，测试逻辑保持不变。
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
    let reparsed: serde_json::Value = serde_json::from_str(&out).expect("redacted must reparse");
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
    assert_eq!(
        obj.get("layoutTextLength").and_then(|v| v.as_i64()),
        Some(46)
    );
}

/// content / token 命中敏感 key，value 整体替换成 [REDACTED]。
#[test]
fn redact_json_redacts_sensitive_values() {
    let input = r#"{"content":"正文","token":"secret"}"#;
    let mut value: serde_json::Value =
        serde_json::from_str(input).expect("input must be valid JSON");
    redact_json(&mut value);
    let obj = value.as_object().expect("top level must be object");
    assert_eq!(
        obj.get("content").and_then(|v| v.as_str()),
        Some("[REDACTED]")
    );
    assert_eq!(
        obj.get("token").and_then(|v| v.as_str()),
        Some("[REDACTED]")
    );
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
    assert_eq!(
        outer.get("token").and_then(|v| v.as_str()),
        Some("[REDACTED]")
    );
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
    let reparsed: serde_json::Value = serde_json::from_str(&out).expect("redacted must reparse");
    let obj = reparsed.as_object().expect("top level must be object");
    let note = obj
        .get("note")
        .and_then(|v| v.as_str())
        .expect("note present");
    assert!(note.contains("[REDACTED]"), "note: {note}");
    assert!(!note.contains("abc123secret"));
    let pat = obj
        .get("pat")
        .and_then(|v| v.as_str())
        .expect("pat present");
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
