//! 作品级校对结果的同步记录（Issue #1064 评论 #7194523322 第 2、3 条）。
//!
//! 记录只保存**派生结果**：segment/上下文的 SHA-256 + 相对 segment 起点的建议，
//! 不含章节正文、不写正文版本/证明/CAS。布局是固定契约：
//!
//! ```text
//! <projects_root>/<project_id>/correction/results/<chapter_id>/<record_key>.json
//! ```
//!
//! 记录身份不依赖绝对路径、mtime、设备或时间戳，因此同一作品在不同设备上对
//! 同一段文本、同一 scanner 代际算出同一个 `record_key`，同步后自然去重。
//! 同步语义是不可变并集：远端删除不删除本地记录，同 identity 不同内容保留本地。

use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::result_cache::CachedSuggestion;

/// 记录文件格式版本。加载时版本不符直接跳过，不猜测。
pub(crate) const SCHEMA_VERSION: u32 = 1;
/// 记录根目录（相对作品根）。
pub(crate) const RESULTS_DIR: &str = "correction/results";
/// 记录身份前缀。
const RECORD_IDENTITY_PREFIX: &str = "correction-synced-v1";
/// scanner 稳定身份前缀（与 legacy 指纹区分）。
const SCANNER_STABLE_PREFIX: &str = "correction-scanner-sync-v1";
/// 分段/上下文窗口算法的版本。算法变化会让旧记录不再复用，必须显式改版本。
const SEGMENT_IDENTITY_VERSION: &str = "canonical-segment-v2";

/// 本次扫描实际构建/使用了模型引擎。
pub(crate) const MODE_MODEL: &str = "model";
/// 纯规则扫描（没有模型引擎参与）。
pub(crate) const MODE_RULES: &str = "rules";

/// 一台设备、一个 scanner 代际对一个 segment 的校对结果。
///
/// `suggestions` 的坐标相对 segment 起点，与 [`CachedSuggestion`] 的既有语义
/// 一致；空列表表示「已检查且无建议」，不是「未检查」。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncedCorrectionRecord {
    pub(crate) schema_version: u32,
    pub(crate) project_id: String,
    pub(crate) chapter_id: String,
    /// [`MODE_MODEL`] 或 [`MODE_RULES`]。
    pub(crate) mode: String,
    pub(crate) scanner_stable: String,
    pub(crate) model_identity: String,
    pub(crate) lexicon_identity: String,
    pub(crate) rules_identity: String,
    /// segment 文本的 SHA-256（强摘要，跨设备稳定）。
    pub(crate) text_sha256: String,
    /// segment 前后上下文的 SHA-256（强摘要）。
    pub(crate) context_sha256: String,
    #[serde(default)]
    pub(crate) suggestions: Vec<CachedSuggestion>,
    /// 写入时恒为 true；加载时 false 的记录视为无效。
    #[serde(default)]
    pub(crate) checked: bool,
}

impl SyncedCorrectionRecord {
    /// 固定布局下的记录键（小写 hex SHA-256）。
    pub(crate) fn record_key(&self) -> String {
        record_key(
            &self.project_id,
            &self.chapter_id,
            &self.mode,
            &self.scanner_stable,
            &self.text_sha256,
            &self.context_sha256,
        )
    }
}

/// scanner 稳定身份 = SHA-256(`correction-scanner-sync-v1|model|lexicon|rules`)。
///
/// 只由内容/配置事实组成：模型四个 sha、词典内容指纹、排序后的禁用规则。
pub(crate) fn scanner_stable_identity(
    model_identity: &str,
    lexicon_identity: &str,
    rules_identity: &str,
) -> String {
    let material = format!(
        "{SCANNER_STABLE_PREFIX}|{SEGMENT_IDENTITY_VERSION}|{model_identity}|\
         {lexicon_identity}|{rules_identity}"
    );
    sha256_hex(material.as_bytes())
}

/// record_key = 小写 hex SHA-256(identity 字符串)。
pub(crate) fn record_key(
    project_id: &str,
    chapter_id: &str,
    mode: &str,
    scanner_stable: &str,
    text_sha256: &str,
    context_sha256: &str,
) -> String {
    let identity = format!(
        "{RECORD_IDENTITY_PREFIX}|{SEGMENT_IDENTITY_VERSION}|{project_id}|{chapter_id}|\
         {mode}|{scanner_stable}|{text_sha256}|{context_sha256}"
    );
    sha256_hex(identity.as_bytes())
}

/// 纯文本 SHA-256 摘要（强摘要，供同步记录身份使用）。
pub(crate) fn text_sha256(text: &str) -> String {
    sha256_hex(text.as_bytes())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// `correction/results/<chapter_id>/<record_key>.json` 的路径判定。
///
/// 只承认这两层结构和 `.json` 扩展名；`correction/results/` 下的其他扩展名
/// 不进入同步。
pub(crate) fn is_synced_result_path(rel_path: &str) -> bool {
    // Windows 传来的路径可能带反斜杠；与同步分类器一样先归一化。
    let path = if rel_path.contains('\\') {
        std::borrow::Cow::Owned(rel_path.replace('\\', "/"))
    } else {
        std::borrow::Cow::Borrowed(rel_path)
    };
    let Some(rest) = path.strip_prefix("correction/results/") else {
        return false;
    };
    let mut parts = rest.split('/');
    let (Some(chapter_id), Some(filename), None) = (parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    !chapter_id.is_empty() && filename.len() > ".json".len() && filename.ends_with(".json")
}

fn is_valid_path_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
}

fn chapter_records_dir(project_root: &Path, chapter_id: &str) -> Option<PathBuf> {
    if !is_valid_path_segment(chapter_id) {
        return None;
    }
    Some(project_root.join(RESULTS_DIR).join(chapter_id))
}

fn normalize(record: &SyncedCorrectionRecord) -> SyncedCorrectionRecord {
    let mut record = record.clone();
    record.checked = true;
    record
}

/// 幂等写入一条记录。
///
/// 文件已存在且内容等价时返回 `Ok(false)`，不重写（避免没有变化的同步提交）；
/// 目录不存在时创建；写入走仓库统一的原子写入。
pub(crate) fn write_record(
    project_root: &Path,
    record: &SyncedCorrectionRecord,
) -> crate::Result<bool> {
    if record.schema_version != SCHEMA_VERSION {
        return Err(crate::Error::Other(
            "unsupported correction synced result schema version".into(),
        ));
    }
    let Some(dir) = chapter_records_dir(project_root, &record.chapter_id) else {
        return Err(crate::Error::Other(
            "invalid correction synced result chapter id".into(),
        ));
    };
    let record = normalize(record);
    let path = dir.join(format!("{}.json", record.record_key()));
    if let Ok(existing) = std::fs::read_to_string(&path) {
        if serde_json::from_str::<SyncedCorrectionRecord>(&existing).is_ok_and(|existing| {
            existing.schema_version == SCHEMA_VERSION && existing == record
        }) {
            return Ok(false);
        }
    }
    let json = serde_json::to_string(&record)
        .map_err(|error| crate::Error::Other(format!("correction result encode failed: {error}")))?;
    crate::storage::atomic_write_string(&path, &json)?;
    Ok(true)
}

/// 加载一个章节在当前 mode + scanner 稳定身份下的全部记录。
///
/// 目录或文件缺失、JSON 损坏、版本不符、身份不匹配都跳过；本函数不返回错误，
/// 让恢复路径保持只读且可降级。
pub(crate) fn load_records(
    project_root: &Path,
    chapter_id: &str,
    mode: &str,
    scanner_stable: &str,
) -> Vec<SyncedCorrectionRecord> {
    let Some(dir) = chapter_records_dir(project_root, chapter_id) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut paths = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
        .collect::<Vec<_>>();
    paths.sort();
    let mut records = Vec::new();
    for path in paths {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(record) = serde_json::from_str::<SyncedCorrectionRecord>(&content) else {
            continue;
        };
        if record.schema_version != SCHEMA_VERSION
            || !record.checked
            || record.chapter_id != chapter_id
            || record.mode != mode
            || record.scanner_stable != scanner_stable
        {
            continue;
        }
        records.push(record);
    }
    records
}

/// 显式作品级回收：删除一个章节的全部同步记录文件。
///
/// 不接入每台设备的章节删除路径；同步对象删除语义由同步并集分支决定。
pub(crate) fn remove_chapter_records(
    project_root: &Path,
    chapter_id: &str,
) -> crate::Result<usize> {
    let Some(dir) = chapter_records_dir(project_root, chapter_id) else {
        return Err(crate::Error::Other(
            "invalid correction synced result chapter id".into(),
        ));
    };
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    let mut removed = 0;
    for entry in entries {
        let path = entry?.path();
        if path.is_file() {
            std::fs::remove_file(&path)?;
            removed += 1;
        }
    }
    // 目录已空才可能成功；仍有其他文件时保留，不递归删除。
    let _ = std::fs::remove_dir(&dir);
    Ok(removed)
}

/// 显式作品级回收：只保留 `keep_keys` 中的记录文件。
pub(crate) fn prune_records(
    project_root: &Path,
    chapter_id: &str,
    keep_keys: &HashSet<String>,
) -> crate::Result<usize> {
    let Some(dir) = chapter_records_dir(project_root, chapter_id) else {
        return Err(crate::Error::Other(
            "invalid correction synced result chapter id".into(),
        ));
    };
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    let mut removed = 0;
    for entry in entries {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let Some(key) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if keep_keys.contains(key) {
            continue;
        }
        std::fs::remove_file(&path)?;
        removed += 1;
    }
    Ok(removed)
}

#[cfg(test)]
#[path = "synced_results_tests.rs"]
mod tests;
