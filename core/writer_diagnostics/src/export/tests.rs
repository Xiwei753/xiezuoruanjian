//! export.rs 的单元测试。
//!
//! 按仓库结构门禁（`tools/check_source_structure.py` 的
//! production-test-bloat 规则）从生产文件拆出，测试逻辑保持不变。
use super::*;

// 与 writer/logger 测试共用 crate 级 TEST_LOCK：本测试也直接触碰全局 writer 单例
// （init/enqueue/flush/reset_for_test），必须串行，否则会改写全局 config 污染断言。
#[test]
fn export_creates_zip_with_manifest() {
    let _lock = crate::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    // 先 init writer 写一条日志。
    super::super::writer::init(tmp.path().join("log"), "export-test".to_string(), true);
    super::super::writer::set_enabled(true);
    super::super::writer::enqueue(
        r#"{"ts":0,"seq":1,"level":"INFO","origin":"app","event":"test","target":"t","session":"s"}"#
            .to_string(),
    );
    assert!(super::super::writer::flush());

    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();
    let attachments = vec![PlatformAttachment {
        relative_path: "device_info.json".to_string(),
        content: b"{\"platform\":\"test\"}".to_vec(),
    }];
    let zip_path = export_diagnostics(&out_dir, "test", "export-test", &attachments)
        .expect("export should succeed");
    assert!(zip_path.exists(), "zip should exist at {zip_path:?}");
    assert!(
        zip_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("sujian-diagnostics-"),
        "zip name: {zip_path:?}"
    );
    // 验证 zip 不是空文件。
    let meta = fs::metadata(&zip_path).unwrap();
    assert!(meta.len() > 0, "zip should not be empty");
    super::super::writer::reset_for_test();
}

/// Issue #670 评论 5651816143 修改 4：文本附件写入前应被 Rust `redact()` 脱敏。
#[test]
fn redact_attachment_content_redacts_text() {
    let content = b"token=my-secret-token\nother=safe";
    let redacted = redact_attachment_content(content, "logcat.txt");
    let text = std::str::from_utf8(&redacted).unwrap();
    assert!(text.contains("[REDACTED]"), "redacted: {text}");
    assert!(!text.contains("my-secret-token"));
}

/// 非 UTF-8 内容（二进制附件）应原样返回，不做处理。
#[test]
fn redact_attachment_content_passes_through_non_utf8() {
    let binary = vec![0u8, 159, 146, 150, 255];
    let redacted = redact_attachment_content(&binary, "screenshot.png");
    assert_eq!(redacted, binary);
}

/// Issue #670 评论 5651816143 修改 4：导出时附件应被脱敏。
#[test]
fn export_redacts_attachment_content() {
    let _lock = crate::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    super::super::writer::init(
        tmp.path().join("log"),
        "export-redact-test".to_string(),
        true,
    );
    super::super::writer::set_enabled(true);
    super::super::writer::enqueue(
        r#"{"ts":0,"seq":1,"level":"INFO","origin":"app","event":"test","target":"t","session":"s"}"#
            .to_string(),
    );
    assert!(super::super::writer::flush());

    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();
    let attachments = vec![PlatformAttachment {
        relative_path: "logcat.txt".to_string(),
        content: b"Authorization: Bearer secret123".to_vec(),
    }];
    let zip_path = export_diagnostics(&out_dir, "test", "export-redact-test", &attachments)
        .expect("export should succeed");
    assert!(zip_path.exists());
    super::super::writer::reset_for_test();
}

/// 验证 ZIP 包中日志文件使用 Deflate 压缩而非 Stored。
#[test]
fn export_zip_uses_deflate_compression() {
    let _lock = crate::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    super::super::writer::init(tmp.path().join("log"), "deflate-test".to_string(), true);
    super::super::writer::set_enabled(true);
    super::super::writer::enqueue(
        r#"{"ts":0,"seq":1,"level":"INFO","origin":"app","event":"test","target":"t","session":"s"}"#
            .to_string(),
    );
    assert!(super::super::writer::flush());

    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();
    let attachments = vec![];
    let zip_path = export_diagnostics(&out_dir, "test", "deflate-test", &attachments)
        .expect("export should succeed");

    // 用 ZipArchive 打开验证压缩方法
    let zip_file = fs::File::open(&zip_path).unwrap();
    let mut archive = zip::ZipArchive::new(zip_file).expect("zip archive should be valid");

    let mut found_log_entry = false;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).unwrap();
        let name = entry.name().to_string();
        if name.ends_with(".log") {
            found_log_entry = true;
            assert_eq!(
                entry.compression(),
                zip::CompressionMethod::Deflated,
                "log entry '{}' should use Deflated compression, got {:?}",
                name,
                entry.compression()
            );
        }
    }
    assert!(
        found_log_entry,
        "zip should contain at least one .log entry"
    );

    super::super::writer::reset_for_test();
}

// ── JSON 附件结构化脱敏测试 — Issue #717 评论 5741567193 ──────────────

/// 从 zip 中读回指定名称的文件内容。
fn read_zip_entry(zip_path: &Path, name: &str) -> Vec<u8> {
    let zip_file = fs::File::open(zip_path).unwrap();
    let mut archive = zip::ZipArchive::new(zip_file).expect("zip archive should be valid");
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        if entry.name() == name {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut buf).unwrap();
            return buf;
        }
    }
    panic!("zip entry {name:?} not found");
}

/// 导出含 `targetId=chapter-body:p:v:c` 的 JSON 附件，读回后必须能被
/// serde_json 重新解析，且 targetId 不被破坏（body 在这里是字符串值的一部分）。
#[test]
fn redact_json_attachment_preserves_structure() {
    let _lock = crate::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    super::super::writer::init(tmp.path().join("log"), "json-struct-test".to_string(), true);
    super::super::writer::set_enabled(true);
    super::super::writer::enqueue(
        r#"{"ts":0,"seq":1,"level":"INFO","origin":"app","event":"test","target":"t","session":"s"}"#
            .to_string(),
    );
    assert!(super::super::writer::flush());

    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();
    let attachments = vec![PlatformAttachment {
        relative_path: "editor_snapshot.json".to_string(),
        content: br#"{"targetId":"chapter-body:p:v:c"}"#.to_vec(),
    }];
    let zip_path = export_diagnostics(&out_dir, "test", "json-struct-test", &attachments)
        .expect("export should succeed");

    let bytes = read_zip_entry(&zip_path, "editor_snapshot.json");
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).expect("redacted json must parse");
    assert_eq!(
        value.get("targetId").and_then(|v| v.as_str()),
        Some("chapter-body:p:v:c"),
        "targetId must not be redacted or corrupted",
    );
    super::super::writer::reset_for_test();
}

/// JSON 附件中敏感 key 的 value 必须变成 [REDACTED]。
#[test]
fn redact_json_attachment_redacts_sensitive() {
    let _lock = crate::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    super::super::writer::init(tmp.path().join("log"), "json-redact-test".to_string(), true);
    super::super::writer::set_enabled(true);
    super::super::writer::enqueue(
        r#"{"ts":0,"seq":1,"level":"INFO","origin":"app","event":"test","target":"t","session":"s"}"#
            .to_string(),
    );
    assert!(super::super::writer::flush());

    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();
    let attachments = vec![PlatformAttachment {
        relative_path: "secret.json".to_string(),
        content: br#"{"token":"secret"}"#.to_vec(),
    }];
    let zip_path = export_diagnostics(&out_dir, "test", "json-redact-test", &attachments)
        .expect("export should succeed");

    let bytes = read_zip_entry(&zip_path, "secret.json");
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).expect("redacted json must parse");
    assert_eq!(
        value.get("token").and_then(|v| v.as_str()),
        Some("[REDACTED]"),
    );
    super::super::writer::reset_for_test();
}

/// JSON 附件内容非法时，导出产出一个合法的错误 JSON 附件（能被 serde_json 解析）。
#[test]
fn redact_json_attachment_parse_failure_emits_valid_json() {
    let _lock = crate::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    super::super::writer::init(tmp.path().join("log"), "json-err-test".to_string(), true);
    super::super::writer::set_enabled(true);
    super::super::writer::enqueue(
        r#"{"ts":0,"seq":1,"level":"INFO","origin":"app","event":"test","target":"t","session":"s"}"#
            .to_string(),
    );
    assert!(super::super::writer::flush());

    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();
    // 非法 JSON：引号被吃掉的真实破坏样本。
    let broken = br#"{"targetId": "chapter-body=[REDACTED],"#;
    let attachments = vec![PlatformAttachment {
        relative_path: "broken.json".to_string(),
        content: broken.to_vec(),
    }];
    let zip_path = export_diagnostics(&out_dir, "test", "json-err-test", &attachments)
        .expect("export should succeed");

    let bytes = read_zip_entry(&zip_path, "broken.json");
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).expect("error json must be valid JSON");
    assert!(
        value.get("error").is_some(),
        "error json must contain 'error' field, got: {value}",
    );
    super::super::writer::reset_for_test();
}
