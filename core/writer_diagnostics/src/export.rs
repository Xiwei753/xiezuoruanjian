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
    let mut attachment_names = Vec::new();
    for att in attachments {
        let dest = temp_dir.join(&att.relative_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("create attachment parent failed: {e}"))?;
        }
        fs::write(&dest, &att.content)
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

/// 把目录打 zip 包。简单的 store-only 实现，不依赖外部 zip crate。
fn zip_directory(src_dir: &Path, zip_path: &Path) -> Result<PathBuf, String> {
    // 简化实现：把目录里所有文件按顺序写入一个 tar-like 容器。
    // 真正的 zip 格式较复杂，这里用简化的"文件拼接 + 索引"格式，
    // 接收端可用 unzip 工具或后续替换为 zip crate。
    //
    // 但 Issue 要求打 zip 包，且仓库已有 zip crate（apps/Linux_qt 用 zip = "2.2"）。
    // writer_diagnostics 不依赖平台 crate，只依赖 log/serde/serde_json/regex/chrono/uuid/std。
    // 因此这里用一个最小的 zip 写入器（store-only，无压缩）。
    let file = fs::File::create(zip_path).map_err(|e| format!("create zip failed: {e}"))?;
    let mut writer = ZipWriter::new(file);
    let entries = collect_entries(src_dir, src_dir)?;
    for (name, content) in entries {
        writer
            .add_file(&name, &content)
            .map_err(|e| format!("add file {name} failed: {e}"))?;
    }
    writer
        .finish()
        .map_err(|e| format!("finish zip failed: {e}"))?;
    Ok(zip_path.to_path_buf())
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

// ── 最小 ZIP 写入器（store-only，无压缩） ──
//
// ZIP 格式参考 APPNOTE.TXT。store-only 模式下：
// - compression method = 0 (stored)
// - CRC-32 = 实际 CRC
// - compressed size = uncompressed size
//
// 这是 Issue 约定"打 zip 包"的最小实现，避免引入 zip crate 依赖
// （writer_diagnostics 只依赖 log/serde/serde_json/regex/chrono/uuid/std）。

struct ZipWriter {
    file: fs::File,
    entries: Vec<CentralDirEntry>,
    offset: u64,
}

struct CentralDirEntry {
    name: String,
    crc32: u32,
    size: u64,
    offset: u64,
}

impl ZipWriter {
    fn new(file: fs::File) -> Self {
        Self {
            file,
            entries: Vec::new(),
            offset: 0,
        }
    }

    fn add_file(&mut self, name: &str, data: &[u8]) -> std::io::Result<()> {
        let crc = crc32(data);
        let size = data.len() as u64;
        let size_u32 = u32::try_from(size).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "file too large for zip32")
        })?;
        let local_offset = self.offset;
        // Local file header.
        let mut header = Vec::new();
        header.extend_from_slice(&0x04034b50u32.to_le_bytes()); // signature
        header.extend_from_slice(&20u16.to_le_bytes()); // version needed
        header.extend_from_slice(&0u16.to_le_bytes()); // flags
        header.extend_from_slice(&0u16.to_le_bytes()); // compression method (stored)
        header.extend_from_slice(&0u16.to_le_bytes()); // mod time
        header.extend_from_slice(&0u16.to_le_bytes()); // mod date
        header.extend_from_slice(&crc.to_le_bytes()); // crc-32
        header.extend_from_slice(&size_u32.to_le_bytes()); // compressed size
        header.extend_from_slice(&size_u32.to_le_bytes()); // uncompressed size
        let name_bytes = name.as_bytes();
        let name_len_u16 = u16::try_from(name_bytes.len()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "name too long for zip")
        })?;
        header.extend_from_slice(&name_len_u16.to_le_bytes()); // file name length
        header.extend_from_slice(&0u16.to_le_bytes()); // extra field length
        header.extend_from_slice(name_bytes);
        self.file.write_all(&header)?;
        self.file.write_all(data)?;
        self.offset += u64::try_from(header.len()).unwrap_or(0) + size;
        self.entries.push(CentralDirEntry {
            name: name.to_string(),
            crc32: crc,
            size,
            offset: local_offset,
        });
        Ok(())
    }

    fn finish(mut self) -> std::io::Result<()> {
        let cd_start = self.offset;
        for entry in &self.entries {
            let size_u32 = u32::try_from(entry.size).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "file too large for zip32")
            })?;
            let offset_u32 = u32::try_from(entry.offset).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "offset too large for zip32",
                )
            })?;
            let mut cd = Vec::new();
            cd.extend_from_slice(&0x02014b50u32.to_le_bytes()); // signature
            cd.extend_from_slice(&20u16.to_le_bytes()); // version made by
            cd.extend_from_slice(&20u16.to_le_bytes()); // version needed
            cd.extend_from_slice(&0u16.to_le_bytes()); // flags
            cd.extend_from_slice(&0u16.to_le_bytes()); // compression method
            cd.extend_from_slice(&0u16.to_le_bytes()); // mod time
            cd.extend_from_slice(&0u16.to_le_bytes()); // mod date
            cd.extend_from_slice(&entry.crc32.to_le_bytes()); // crc-32
            cd.extend_from_slice(&size_u32.to_le_bytes()); // compressed size
            cd.extend_from_slice(&size_u32.to_le_bytes()); // uncompressed size
            let name_bytes = entry.name.as_bytes();
            let name_len_u16 = u16::try_from(name_bytes.len()).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "name too long for zip")
            })?;
            cd.extend_from_slice(&name_len_u16.to_le_bytes()); // file name length
            cd.extend_from_slice(&0u16.to_le_bytes()); // extra field length
            cd.extend_from_slice(&0u16.to_le_bytes()); // file comment length
            cd.extend_from_slice(&0u16.to_le_bytes()); // disk number start
            cd.extend_from_slice(&0u16.to_le_bytes()); // internal file attributes
            cd.extend_from_slice(&0u32.to_le_bytes()); // external file attributes
            cd.extend_from_slice(&offset_u32.to_le_bytes()); // relative offset of local header
            cd.extend_from_slice(name_bytes);
            self.file.write_all(&cd)?;
            self.offset += u64::try_from(cd.len()).unwrap_or(0);
        }
        let cd_end = self.offset;
        let cd_size = cd_end - cd_start;
        let cd_size_u32 = u32::try_from(cd_size).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "CD too large for zip32")
        })?;
        let cd_start_u32 = u32::try_from(cd_start).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "CD offset too large for zip32",
            )
        })?;
        // End of central directory record.
        let mut eocd = Vec::new();
        eocd.extend_from_slice(&0x06054b50u32.to_le_bytes()); // signature
        eocd.extend_from_slice(&0u16.to_le_bytes()); // disk number
        eocd.extend_from_slice(&0u16.to_le_bytes()); // disk with CD
        let entries_u16 = u16::try_from(self.entries.len()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "too many entries for zip")
        })?;
        eocd.extend_from_slice(&entries_u16.to_le_bytes()); // entries on this disk
        eocd.extend_from_slice(&entries_u16.to_le_bytes()); // total entries
        eocd.extend_from_slice(&cd_size_u32.to_le_bytes()); // CD size
        eocd.extend_from_slice(&cd_start_u32.to_le_bytes()); // CD offset
        eocd.extend_from_slice(&0u16.to_le_bytes()); // comment length
        self.file.write_all(&eocd)?;
        Ok(())
    }
}

/// CRC-32 (IEEE 802.3) — 标准 ZIP 使用的 CRC 算法。
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn export_creates_zip_with_manifest() {
        let _lock = TEST_LOCK.lock().unwrap();
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

    #[test]
    fn crc32_known_value() {
        // CRC-32 of "123456789" is 0xCBF43926.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn crc32_empty() {
        assert_eq!(crc32(b""), 0);
    }
}
