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
//! 结果 payload 也纳入身份：同一输入的不同有效结果成为不同不可变对象并集，
//! 读端按文件键排序选确定性赢家；远端删除不删除本地记录。

use std::collections::{BTreeMap, HashSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::storage::transaction::SaveTransaction;
use crate::sync::hash::content_md5;
use crate::sync::local_mutations::{self, LocalMutationRecord};
use crate::sync::SyncService;

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
/// 校对引擎/规则/词典构建算法的版本；与分析结果语义绑定。
const ENGINE_IDENTITY_VERSION: &str = "correction-engine-v11";

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
    ///
    /// 身份包含规范化结果 payload 的摘要：同一输入身份下不同有效结果会落到
    /// 不同不可变文件，而不是互相覆盖；读端按文件键排序取确定性赢家。
    pub(crate) fn record_key(&self) -> String {
        record_key(
            &self.project_id,
            &self.chapter_id,
            &self.mode,
            &self.scanner_stable,
            &self.text_sha256,
            &self.context_sha256,
            &payload_digest(&self.suggestions),
        )
    }
}

/// 规范化结果 payload 的 SHA-256：建议按字段全排序后逐字段长度前缀拼接，
/// 不受 HashMap/JSON 键顺序影响，也不含设备/时间字段。
pub(crate) fn payload_digest(suggestions: &[CachedSuggestion]) -> String {
    let mut sorted: Vec<&CachedSuggestion> = suggestions.iter().collect();
    sorted.sort_by(|left, right| {
        (
            left.start_byte,
            left.end_byte,
            left.original.as_str(),
            left.replacement.as_str(),
            left.source.as_str(),
            left.rule_id.as_str(),
            left.severity.as_str(),
        )
            .cmp(&(
                right.start_byte,
                right.end_byte,
                right.original.as_str(),
                right.replacement.as_str(),
                right.source.as_str(),
                right.rule_id.as_str(),
                right.severity.as_str(),
            ))
    });
    let mut canonical = String::new();
    for suggestion in sorted {
        push_digest_field(&mut canonical, &suggestion.start_byte.to_string());
        push_digest_field(&mut canonical, &suggestion.end_byte.to_string());
        push_digest_field(&mut canonical, &suggestion.original);
        push_digest_field(&mut canonical, &suggestion.replacement);
        push_digest_field(&mut canonical, &suggestion.source);
        push_digest_field(&mut canonical, &suggestion.rule_id);
        push_digest_field(&mut canonical, &suggestion.severity);
    }
    sha256_hex(canonical.as_bytes())
}

fn push_digest_field(buffer: &mut String, value: &str) {
    buffer.push_str(&value.len().to_string());
    buffer.push(':');
    buffer.push_str(value);
    buffer.push('\u{1f}');
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
        "{SCANNER_STABLE_PREFIX}|{SEGMENT_IDENTITY_VERSION}|{ENGINE_IDENTITY_VERSION}|\
         {model_identity}|{lexicon_identity}|{rules_identity}"
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
    payload_sha256: &str,
) -> String {
    let identity = format!(
        "{RECORD_IDENTITY_PREFIX}|{SEGMENT_IDENTITY_VERSION}|{project_id}|{chapter_id}|\
         {mode}|{scanner_stable}|{text_sha256}|{context_sha256}|{payload_sha256}"
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

pub(crate) fn chapter_records_dir(project_root: &Path, chapter_id: &str) -> Option<PathBuf> {
    if !is_valid_path_segment(chapter_id) {
        return None;
    }
    Some(project_root.join(RESULTS_DIR).join(chapter_id))
}

fn normalize(record: &SyncedCorrectionRecord) -> SyncedCorrectionRecord {
    let mut record = record.clone();
    record.checked = true;
    record.suggestions.sort_by(|left, right| {
        (
            left.start_byte,
            left.end_byte,
            left.original.as_str(),
            left.replacement.as_str(),
            left.source.as_str(),
            left.rule_id.as_str(),
            left.severity.as_str(),
        )
            .cmp(&(
                right.start_byte,
                right.end_byte,
                right.original.as_str(),
                right.replacement.as_str(),
                right.source.as_str(),
                right.rule_id.as_str(),
                right.severity.as_str(),
            ))
    });
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
        if serde_json::from_str::<SyncedCorrectionRecord>(&existing)
            .is_ok_and(|existing| existing.schema_version == SCHEMA_VERSION && existing == record)
        {
            return Ok(false);
        }
    }
    let json = serde_json::to_string(&record).map_err(|error| {
        crate::Error::Other(format!("correction result encode failed: {error}"))
    })?;
    crate::storage::atomic_write_string(&path, &json)?;
    Ok(true)
}

/// 通过 Core 的业务对象持久事务发布一批记录，并把新增对象写入 mutation/outbox。
///
/// - 文件内容与 mutation 账本都未变时不写、不产生新的同步代次；
/// - 文件已存在但账本缺事实时只补账本（重启后可重试未完成的发布）；
/// - 同一路径只提交一次，批次成功后才追加 mutation，失败保留待发布责任。
pub(crate) fn publish_records(
    project_root: &Path,
    records: &[SyncedCorrectionRecord],
) -> crate::Result<(usize, usize)> {
    if records.is_empty() {
        return Ok((0, 0));
    }
    // 同步账本/状态损坏不能阻断校对结果的 durable 落盘：缺省只用空账本/空基线
    // 继续写文件与 mutation，下一次同步会通过完整核对重建基线。
    let latest = match local_mutations::load_latest(project_root) {
        Ok(latest) => latest,
        Err(error) => {
            log::warn!("[correction] local mutation ledger read failed: {error}");
            std::collections::HashMap::new()
        }
    };
    let state = match SyncService::load_sync_state_read_only(project_root, None) {
        Ok(state) => Some(state),
        Err(error) => {
            log::warn!("[correction] sync state read failed; publish without base hash: {error}");
            None
        }
    };
    let now_ms = chrono::Utc::now().timestamp_millis();
    let device_id = state
        .as_ref()
        .map(|state| state.device_id.clone())
        .filter(|device_id| !device_id.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let mut mutation_paths: HashSet<String> = HashSet::new();
    let mut mutations: Vec<LocalMutationRecord> = Vec::new();
    for record in records {
        if record.schema_version != SCHEMA_VERSION {
            continue;
        }
        let normalized = normalize(record);
        let Some(dir) = chapter_records_dir(project_root, &normalized.chapter_id) else {
            continue;
        };
        let key = normalized.record_key();
        let rel_path = format!("{RESULTS_DIR}/{}/{key}.json", normalized.chapter_id);
        let json = serde_json::to_string(&normalized).map_err(|error| {
            crate::Error::Other(format!("correction result encode failed: {error}"))
        })?;
        let content_hash = content_md5(json.as_bytes());
        let path = dir.join(format!("{key}.json"));
        let file_needs_write = std::fs::read(&path)
            .map(|existing| existing != json.as_bytes())
            .unwrap_or(true);
        if file_needs_write {
            files.insert(rel_path.clone(), json);
        }
        // 已由同步基线确认（known_files 同 hash）或账本已有同内容未确认
        // 事实时不再登记本地 mutation：恢复/下载显示不能制造新的本地业务变更。
        let confirmed = state
            .as_ref()
            .and_then(|state| state.known_files.get(&rel_path))
            .is_some_and(|known_hash| known_hash == &content_hash);
        let mutation_needed = !confirmed
            && latest.get(&rel_path).map_or(true, |previous| {
                !previous.is_upsert() || previous.content_hash != content_hash
            });
        if mutation_needed && mutation_paths.insert(rel_path.clone()) {
            let (object_type, object_id) = local_mutations::object_identity(&rel_path);
            mutations.push(LocalMutationRecord {
                schema_version: 1,
                transaction_id: String::new(),
                sync_root: project_root.to_string_lossy().into_owned(),
                path: rel_path.clone(),
                object_type,
                object_id,
                op: "upsert".to_string(),
                base_hash: state
                    .as_ref()
                    .and_then(|state| state.known_files.get(&rel_path).cloned()),
                content_hash,
                updated_at_ms: now_ms,
                deleted_at_ms: None,
                device_id: device_id.clone(),
                object_revision: None,
                seq: 0,
            });
        }
    }
    let mut transaction = if files.is_empty() {
        None
    } else {
        Some(SaveTransaction::new(project_root))
    };
    if let Some(transaction) = transaction.as_mut() {
        for (rel_path, json) in &files {
            transaction.add_file(rel_path, json)?;
        }
        transaction.commit()?;
    }
    let transaction_id = match transaction.as_ref() {
        Some(transaction) => transaction.transaction_id().to_string(),
        None => uuid::Uuid::new_v4().to_string(),
    };
    for mutation in &mut mutations {
        mutation.transaction_id = transaction_id.clone();
    }
    local_mutations::append_batch(project_root, &mutations)?;
    Ok((files.len(), mutations.len()))
}

/// 本章同步结果的不可变索引：按 (text_sha256, context_sha256) 预排序。
///
/// 由 scanner 在章节 slot 锁外构建，restore 只做内容键查找，不再每次复制
/// 全部历史建议或重建/排序 HashMap。
#[derive(Debug, Default)]
pub(crate) struct SyncedResultIndex {
    by_content: std::collections::HashMap<
        String,
        std::collections::HashMap<String, Vec<(String, Vec<CachedSuggestion>)>>,
    >,
    /// 已加载索引的实际内容占用估算（键 + 建议字符串字节），供诊断区分
    /// “有几个索引”和“占多少字节”。
    estimated_bytes: usize,
}

impl SyncedResultIndex {
    pub(crate) fn build(records: &[SyncedCorrectionRecord]) -> Self {
        let mut estimated_bytes = 0usize;
        let mut by_content: std::collections::HashMap<
            String,
            std::collections::HashMap<String, Vec<(String, Vec<CachedSuggestion>)>>,
        > = std::collections::HashMap::new();
        for record in records {
            by_content
                .entry(record.text_sha256.clone())
                .or_default()
                .entry(record.context_sha256.clone())
                .or_default()
                .push((record.record_key(), record.suggestions.clone()));
        }
        for (text_sha256, contexts) in by_content.iter_mut() {
            estimated_bytes = estimated_bytes.saturating_add(text_sha256.len());
            for (context_sha256, candidates) in contexts.iter_mut() {
                estimated_bytes = estimated_bytes.saturating_add(context_sha256.len());
                candidates.sort_by(|left, right| left.0.cmp(&right.0));
                for (record_key, suggestions) in candidates.iter() {
                    estimated_bytes = estimated_bytes.saturating_add(record_key.len());
                    for suggestion in suggestions {
                        estimated_bytes = estimated_bytes
                            .saturating_add(suggestion.original.len())
                            .saturating_add(suggestion.replacement.len())
                            .saturating_add(suggestion.source.len())
                            .saturating_add(suggestion.rule_id.len())
                            .saturating_add(suggestion.severity.len())
                            .saturating_add(32);
                    }
                }
            }
        }
        Self {
            by_content,
            estimated_bytes,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.by_content.is_empty()
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.estimated_bytes
    }

    pub(crate) fn candidates(
        &self,
        text_sha256: &str,
        context_sha256: &str,
    ) -> Option<&[(String, Vec<CachedSuggestion>)]> {
        self.by_content
            .get(text_sha256)
            .and_then(|contexts| contexts.get(context_sha256))
            .map(Vec::as_slice)
    }
}

/// 加载一个章节在当前 mode + scanner 稳定身份下的全部记录。
///
/// 目录不存在、版本不符、身份不匹配都按“没有可用记录”略过；真实的目录/
/// 文件读取或 JSON 损坏返回带路径错误，由调用方按现有缓存失效入口重建，
/// 不能静默当成“未检查”重扫整个章节。
pub(crate) fn load_records(
    project_root: &Path,
    project_id: &str,
    chapter_id: &str,
    mode: &str,
    scanner_stable: &str,
) -> crate::Result<Vec<SyncedCorrectionRecord>> {
    let Some(dir) = chapter_records_dir(project_root, chapter_id) else {
        return Ok(Vec::new());
    };
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|error| {
        crate::Error::Other(format!(
            "failed to read synced correction records dir {}: {error}",
            dir.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            crate::Error::Other(format!(
                "failed to read synced correction record entry in {}: {error}",
                dir.display()
            ))
        })?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(path);
        }
    }
    paths.sort();
    let mut records = Vec::new();
    for path in paths {
        let content = std::fs::read_to_string(&path).map_err(|error| {
            crate::Error::Other(format!(
                "failed to read synced correction record {}: {error}",
                path.display()
            ))
        })?;
        let record: SyncedCorrectionRecord = serde_json::from_str(&content).map_err(|error| {
            crate::Error::Other(format!(
                "corrupt synced correction record {}: {error}",
                path.display()
            ))
        })?;
        let Some(file_key) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        // 版本/项目/章节/模式/扫描器身份不匹配都是正常的缓存失效，略过。
        if record.schema_version != SCHEMA_VERSION
            || !record.checked
            || record.project_id != project_id
            || record.chapter_id != chapter_id
            || record.mode != mode
            || record.scanner_stable != scanner_stable
            || file_key != record.record_key()
        {
            continue;
        }
        records.push(record);
    }
    Ok(records)
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
