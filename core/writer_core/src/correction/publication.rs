//! 校对结果的独立待发布事实（Issue #1064 复审 #7209819808）。
//!
//! 待发布记录拥有完整 [`SyncedCorrectionRecord`] 与 record_key，存放在
//! `correction/pending/<chapter_id>.json`，不参与同步、不依赖当前正文、
//! revision、scanner 代际或建议赢家。扫描先写这里，成功后删除；发布失败
//! 或进程重启后由章节目录版本无关的重试路径继续。
//!
//! 本地 result cache 只负责显示/段落索引，不再承载待发布事务。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::synced_results::{SyncedCorrectionRecord, SCHEMA_VERSION};

pub(crate) const PENDING_DIR: &str = "correction/pending";
const PENDING_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingPublicationFile {
    schema_version: u32,
    project_id: String,
    chapter_id: String,
    #[serde(default)]
    records: Vec<SyncedCorrectionRecord>,
}

pub(crate) fn pending_path(project_root: &Path, chapter_id: &str) -> Option<PathBuf> {
    if chapter_id.is_empty()
        || chapter_id == "."
        || chapter_id == ".."
        || chapter_id.contains('/')
        || chapter_id.contains('\\')
    {
        return None;
    }
    Some(
        project_root
            .join(PENDING_DIR)
            .join(format!("{chapter_id}.json")),
    )
}

pub(crate) fn load_pending(
    project_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Vec<SyncedCorrectionRecord> {
    let Some(path) = pending_path(project_root, chapter_id) else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(file) = serde_json::from_str::<PendingPublicationFile>(&content) else {
        log::warn!("[correction] pending publication file is malformed; ignored");
        return Vec::new();
    };
    if file.schema_version != PENDING_SCHEMA_VERSION
        || file.project_id != project_id
        || file.chapter_id != chapter_id
    {
        return Vec::new();
    }
    let mut records: BTreeMap<String, SyncedCorrectionRecord> = BTreeMap::new();
    for record in file.records {
        if record.schema_version != SCHEMA_VERSION
            || record.project_id != project_id
            || record.chapter_id != chapter_id
            || !record.checked
        {
            continue;
        }
        records.insert(record.record_key(), record);
    }
    records.into_values().collect()
}

/// 把本批新结果并入本章待发布文件；同 record_key 覆盖为最新。
pub(crate) fn write_pending(
    project_root: &Path,
    project_id: &str,
    chapter_id: &str,
    records: &[SyncedCorrectionRecord],
) -> crate::Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    let Some(path) = pending_path(project_root, chapter_id) else {
        return Err(crate::Error::Other(
            "invalid correction pending chapter id".into(),
        ));
    };
    let mut merged: BTreeMap<String, SyncedCorrectionRecord> = load_pending(
        project_root,
        project_id,
        chapter_id,
    )
    .into_iter()
    .map(|record| (record.record_key(), record))
    .collect();
    for record in records {
        merged.insert(record.record_key(), record.clone());
    }
    let file = PendingPublicationFile {
        schema_version: PENDING_SCHEMA_VERSION,
        project_id: project_id.to_string(),
        chapter_id: chapter_id.to_string(),
        records: merged.into_values().collect(),
    };
    let json = serde_json::to_string(&file)
        .map_err(|error| crate::Error::Other(format!("correction pending encode failed: {error}")))?;
    crate::storage::atomic_write_string(&path, &json)?;
    Ok(())
}

/// 发布本章待发布事实；成功才删除文件。失败保留完整 payload 供下次重试。
pub(crate) fn publish_pending(
    project_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> crate::Result<bool> {
    let records = load_pending(project_root, project_id, chapter_id);
    if records.is_empty() {
        if let Some(path) = pending_path(project_root, chapter_id) {
            let _ = std::fs::remove_file(path);
        }
        return Ok(false);
    }
    super::synced_results::publish_records(project_root, &records)?;
    if let Some(path) = pending_path(project_root, chapter_id) {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(true)
}
