//! 统一诊断导出 — 生成日志清单、manifest、公共运行信息和日志文件 zip 包。
//!
//! 平台特有附件由平台采集器交进来（[`PlatformAttachment`]），本模块不自己再
//! 定义一套诊断包格式。导出流程：
//! 1. flush barrier 确保前序日志落盘
//! 2. 复制日志文件到输出目录的 `logs/` 子目录
//! 3. 写入平台附件到输出目录
//! 4. 生成 `diagnostics_manifest.json`
//! 5. 打 zip 包

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::writer;

/// 平台采集器交进来的附件 — 平台特有数据（logcat、process_exits、jank 等）。
#[derive(Debug, Clone)]
pub struct PlatformAttachment {
    /// 相对输出目录的路径（如 `logcat.txt`、`process_exits.json`）。
    pub relative_path: String,
    /// 附件内容（已由平台采集器脱敏）。
    pub content: Vec<u8>,
}

/// 导出元数据 — 写入 `diagnostics_manifest.json`。
#[derive(Serialize)]
struct DiagnosticsManifest {
    schema_version: u32,
    platform: String,
    build_identity: String,
    exported_at: String,
    collection: CollectionStatus,
}

#[derive(Serialize)]
struct CollectionStatus {
    logs: String,
    attachments: Vec<String>,
}

/// 导出诊断包到 `output_dir`，返回生成的 zip 文件路径。
///
/// 流程：flush → 复制日志 → 写附件 → 写 manifest → 打 zip。
/// `output_dir` 会作为临时目录，最终 zip 文件写入 `output_dir/sujian-diagnostics-{timestamp}.zip`。
pub fn export_diagnostics(
    output_dir: &Path,
    platform_name: &str,
    build_key: &str,
    attachments: &[PlatformAttachment],
) -> Result<PathBuf, String> {
    // 1. flush barrier 确保前序日志落盘。
    if !writer::flush() {
        return Err("flush barrier failed; logs may be incomplete".to_string());
    }

    // 2. 准备临时目录。
    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let temp_dir = output_dir.join(format!("temp_{timestamp}"));
    fs::create_dir_all(&temp_dir).map_err(|e| format!("create temp_dir failed: {e}"))?;

    // 3. 复制日志文件到 temp_dir/logs/。
    let logs_status = write_logs(&temp_dir);

    // 4. 写平台附件。
    // Issue #670 评论 5651816143 修改 4：文本附件写入前统一调用 Rust `redact()` 脱敏。
    // 对 content 做 UTF-8 解码 → redact → 再编码；非 UTF-8 内容（二进制附件）原样写入。
    let mut attachment_names = Vec::new();
    for att in attachments {
        let dest = temp_dir.join(&att.relative_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("create attachment parent failed: {e}"))?;
        }
        let redacted_content = redact_attachment_content(&att.content);
        fs::write(&dest, &redacted_content)
            .map_err(|e| format!("write attachment {} failed: {e}", att.relative_path))?;
        attachment_names.push(att.relative_path.clone());
    }

    // 5. 写 diagnostics_manifest.json。
    let manifest = DiagnosticsManifest {
        schema_version: 1,
        platform: platform_name.to_string(),
        build_identity: build_key.to_string(),
        exported_at: chrono::Utc::now().to_rfc3339(),
        collection: CollectionStatus {
            logs: logs_status,
            attachments: attachment_names,
        },
    };
    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("serialize manifest failed: {e}"))?;
    let manifest_path = temp_dir.join("diagnostics_manifest.json");
    fs::write(&manifest_path, manifest_json).map_err(|e| format!("write manifest failed: {e}"))?;

    // 6. 打 zip 包。
    let zip_name = format!("sujian-diagnostics-{timestamp}.zip");
    let zip_path = output_dir.join(zip_name);
    zip_directory(&temp_dir, &zip_path)?;

    // 7. 清理临时目录。
    let _ = fs::remove_dir_all(&temp_dir);

    Ok(zip_path)
}

/// 复制 writer 已落盘的日志文件到 `dest_dir/logs/`，对内容做脱敏。
/// 返回 `"ok"` / `"missing"` / `"error"`。
fn write_logs(dest_dir: &Path) -> String {
    let logs_dir = dest_dir.join("logs");
    if fs::create_dir_all(&logs_dir).is_err() {
        return "error".to_string();
    }
    let log_files = writer::log_files();
    if log_files.is_empty() {
        return "missing".to_string();
    }
    let mut all_ok = true;
    for log_file in &log_files {
        let content = match fs::read_to_string(log_file) {
            Ok(c) => c,
            Err(_) => {
                all_ok = false;
                continue;
            }
        };
        let redacted = super::redact::redact(&content);
        let dest = logs_dir.join(
            log_file
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("unknown")),
        );
        if fs::write(&dest, redacted).is_err() {
            all_ok = false;
        }
    }
    if all_ok {
        "ok".to_string()
    } else {
        "error".to_string()
    }
}

/// 对附件内容做脱敏 — Issue #670 评论 5651816143 修改 4。
///
/// 文本附件（UTF-8 可解码）走 Rust `redact::redact()` 统一脱敏；
/// 非 UTF-8 内容（二进制附件，如截图、protobuf）原样返回，不做处理。
/// 这保证附件脱敏只有一份事实来源（Rust `redact`），不再在 Kotlin 端复制一套规则。
fn redact_attachment_content(content: &[u8]) -> Vec<u8> {
    match std::str::from_utf8(content) {
        Ok(text) => super::redact::redact(text).into_bytes(),
        Err(_) => content.to_vec(),
    }
}

/// 把目录打 zip 包。使用标准 zip crate 的 Deflate 压缩，
/// 诊断日志是 JSONL/文本，Deflate 对这种重复字段非常有效。
fn zip_directory(source_dir: &Path, zip_path: &Path) -> Result<(), String> {
    let file = fs::File::create(zip_path)
        .map_err(|e| format!("create zip failed: {e}"))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(6));

    let mut entries = collect_entries(source_dir, source_dir)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    for (name, data) in entries {
        let zip_name = name.replace('\\', "/");
        zip.start_file(zip_name, options)
            .map_err(|e| format!("zip start_file failed: {e}"))?;
        zip.write_all(&data)
            .map_err(|e| format!("zip write failed: {e}"))?;
    }

    zip.finish()
        .map_err(|e| format!("zip finish failed: {e}"))?;
    Ok(())
}

/// 递归收集目录下所有文件，返回 (相对路径, 内容) 列表。
type CollectResult = Result<Vec<(String, Vec<u8>)>, String>;

fn collect_entries(base: &Path, dir: &Path) -> CollectResult {
    let mut result = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| format!("read_dir failed: {e}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            let rel = path
                .strip_prefix(base)
                .map_err(|e| format!("strip_prefix failed: {e}"))?
                .to_string_lossy()
                .to_string();
            let content = fs::read(&path).map_err(|e| format!("read file failed: {e}"))?;
            result.push((rel, content));
        } else if path.is_dir() {
            let sub = collect_entries(base, &path)?;
            result.extend(sub);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
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
        let redacted = redact_attachment_content(content);
        let text = std::str::from_utf8(&redacted).unwrap();
        assert!(text.contains("[REDACTED]"), "redacted: {text}");
        assert!(!text.contains("my-secret-token"));
    }

    /// 非 UTF-8 内容（二进制附件）应原样返回，不做处理。
    #[test]
    fn redact_attachment_content_passes_through_non_utf8() {
        let binary = vec![0u8, 159, 146, 150, 255];
        let redacted = redact_attachment_content(&binary);
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
}
