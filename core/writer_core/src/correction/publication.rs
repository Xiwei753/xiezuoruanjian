//! 校对结果的独立待发布事实（Issue #1064 复审 #7209819808 / #7210343338）。
//!
//! 每个 `record_key` 是一个独立、不可变的 pending 文件：
//!
//! ```text
//! <projects_root>/<project_id>/correction/pending/<chapter_id>/<record_key>.json
//! ```
//!
//! 写入只创建/覆盖自己的 key，不与其它 scanner 共享 read-modify-write；发布
//! 先冻结目录列表，成功后只删除这一批实际发布的文件，发布期间新追加的 key
//! 不受影响。读目录缺失才表示无待办；坏 JSON、身份不符或读失败都保留原件并
//! 返回错误，绝不把无法读取的内容当成空集删除。
//!
//! 待发布文件不参与同步，也不依赖当前正文、revision、scanner 代际或建议赢家。
//! 本地 result cache 只负责显示/段落索引。

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::synced_results::{SyncedCorrectionRecord, SCHEMA_VERSION};

pub(crate) const PENDING_DIR: &str = "correction/pending";

/// 旧版单文件格式（`<chapter_id>.json`）的只读兼容结构；发布成功后一并删除。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyPendingFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    project_id: String,
    #[serde(default)]
    chapter_id: String,
    #[serde(default)]
    records: Vec<SyncedCorrectionRecord>,
}

/// 一个已经通过身份校验、等待发布的记录及其来源文件。
#[derive(Debug, Clone)]
pub(crate) struct PendingRecord {
    pub(crate) path: PathBuf,
    pub(crate) record: SyncedCorrectionRecord,
}

fn valid_chapter_id(chapter_id: &str) -> bool {
    !chapter_id.is_empty()
        && chapter_id != "."
        && chapter_id != ".."
        && !chapter_id.contains('/')
        && !chapter_id.contains('\\')
}

fn chapter_dir(project_root: &Path, chapter_id: &str) -> Option<PathBuf> {
    valid_chapter_id(chapter_id)
        .then(|| project_root.join(PENDING_DIR).join(chapter_id))
}

fn legacy_path(project_root: &Path, chapter_id: &str) -> Option<PathBuf> {
    valid_chapter_id(chapter_id).then(|| {
        project_root
            .join(PENDING_DIR)
            .join(format!("{chapter_id}.json"))
    })
}

fn invalid_record(project_id: &str, chapter_id: &str, reason: &str) -> crate::Error {
    crate::Error::Other(format!(
        "invalid correction pending record project={project_id} chapter={chapter_id}: {reason}"
    ))
}

/// 强校验记录身份；返回它应落在哪个不可变文件。
fn record_key_for(
    project_id: &str,
    chapter_id: &str,
    record: &SyncedCorrectionRecord,
) -> crate::Result<String> {
    if record.schema_version != SCHEMA_VERSION {
        return Err(invalid_record(project_id, chapter_id, "schema version"));
    }
    if record.project_id != project_id {
        return Err(invalid_record(project_id, chapter_id, "project identity"));
    }
    if record.chapter_id != chapter_id {
        return Err(invalid_record(project_id, chapter_id, "chapter identity"));
    }
    if !record.checked {
        return Err(invalid_record(project_id, chapter_id, "unchecked result"));
    }
    Ok(record.record_key())
}

fn read_record_file(
    project_id: &str,
    chapter_id: &str,
    path: &Path,
) -> crate::Result<SyncedCorrectionRecord> {
    let content = fs::read_to_string(path).map_err(|error| {
        crate::Error::Other(format!(
            "read correction pending {} failed: {error}",
            path.display()
        ))
    })?;
    let record: SyncedCorrectionRecord = serde_json::from_str(&content).map_err(|error| {
        crate::Error::Other(format!(
            "parse correction pending {} failed: {error}",
            path.display()
        ))
    })?;
    let key = record_key_for(project_id, chapter_id, &record)?;
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| {
            crate::Error::Other(format!(
                "correction pending file has no utf-8 stem: {}",
                path.display()
            ))
        })?;
    if stem != key {
        return Err(crate::Error::Other(format!(
            "correction pending key mismatch at {}: file={stem} record={key}",
            path.display()
        )));
    }
    Ok(record)
}

/// 读取一章的全部待发布记录。
///
/// 目录/旧文件缺失 = 没有待办；其他错误（读失败、坏 JSON、身份或键不符）都
/// 返回 `Err` 并保留原件，由下一轮重试而不是静默丢弃。
pub(crate) fn load_pending(
    project_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> crate::Result<Vec<PendingRecord>> {
    let mut pending = Vec::new();
    if let Some(dir) = chapter_dir(project_root, chapter_id) {
        match fs::read_dir(&dir) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry.map_err(|error| {
                        crate::Error::Other(format!(
                            "read correction pending dir {} failed: {error}",
                            dir.display()
                        ))
                    })?;
                    let path = entry.path();
                    if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                        continue;
                    }
                    let record = read_record_file(project_id, chapter_id, &path)?;
                    pending.push(PendingRecord { path, record });
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(crate::Error::Other(format!(
                    "open correction pending dir {} failed: {error}",
                    dir.display()
                )));
            }
        }
    }
    // 兼容旧版单文件；只有读到完整有效记录才加入待办，否则保留原件报错。
    if let Some(path) = legacy_path(project_root, chapter_id) {
        match fs::read_to_string(&path) {
            Ok(content) => {
                let file: LegacyPendingFile = serde_json::from_str(&content).map_err(|error| {
                    crate::Error::Other(format!(
                        "parse legacy correction pending {} failed: {error}",
                        path.display()
                    ))
                })?;
                if file.schema_version != 0
                    && (file.project_id != project_id || file.chapter_id != chapter_id)
                {
                    return Err(crate::Error::Other(format!(
                        "legacy correction pending identity mismatch at {}: project={} chapter={}",
                        path.display(),
                        file.project_id,
                        file.chapter_id
                    )));
                }
                for record in file.records {
                    record_key_for(project_id, chapter_id, &record)?;
                    pending.push(PendingRecord {
                        path: path.clone(),
                        record,
                    });
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(crate::Error::Other(format!(
                    "read legacy correction pending {} failed: {error}",
                    path.display()
                )));
            }
        }
    }
    Ok(pending)
}

/// 写入本批新结果；每个 key 独立文件，不读旧集合、不覆盖其它 key。
pub(crate) fn write_pending(
    project_root: &Path,
    project_id: &str,
    chapter_id: &str,
    records: &[SyncedCorrectionRecord],
) -> crate::Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    let Some(dir) = chapter_dir(project_root, chapter_id) else {
        return Err(invalid_record(project_id, chapter_id, "chapter id"));
    };
    fs::create_dir_all(&dir)?;
    for record in records {
        let key = record_key_for(project_id, chapter_id, record)?;
        let path = dir.join(format!("{key}.json"));
        let json = serde_json::to_string(record)?;
        match fs::read_to_string(&path) {
            Ok(existing) if existing == json => continue,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        crate::storage::atomic_write_string(&path, &json)?;
    }
    Ok(())
}

/// 发布本章待发布事实；只删除实际发布成功的文件，新追加的文件不受影响。
/// 返回本轮确认的记录数（0 表示没有待办）。
pub(crate) fn publish_pending(
    project_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> crate::Result<usize> {
    let pending = load_pending(project_root, project_id, chapter_id)?;
    if pending.is_empty() {
        return Ok(0);
    }
    let records: Vec<SyncedCorrectionRecord> =
        pending.iter().map(|item| item.record.clone()).collect();
    let (written, _) = super::synced_results::publish_records(project_root, &records)?;
    for item in &pending {
        match fs::remove_file(&item.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(crate::Error::Other(format!(
                    "remove published correction pending {} failed: {error}",
                    item.path.display()
                )));
            }
        }
    }
    if let Some(dir) = chapter_dir(project_root, chapter_id) {
        let _ = fs::remove_dir(&dir);
    }
    if let Some(legacy) = legacy_path(project_root, chapter_id) {
        match fs::remove_file(&legacy) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(crate::Error::Other(format!(
                    "remove legacy correction pending {} failed: {error}",
                    legacy.display()
                )));
            }
        }
    }
    // 只有真正写出/更新结果文件才需要失效章节索引；只是确认已存在的
    // 记录并清理 pending 不应触发重新加载整个目录。
    Ok(written)
}

/// Core 同步前恢复入口：遍历实际待办目录，而不是等用户重新打开失败章节。
///
/// 整个扫描/发布持有 #1065 的工作区写锁；发布时 `append_batch` 复用同一把锁。
/// 单个章节失败保留其文件并继续其它章节，最终返回首个错误。
pub(crate) fn retry_all_pending(
    app_data_root: &Path,
    projects_root: &Path,
) -> crate::Result<usize> {
    let _workspace_lock = crate::storage::WorkspaceWriteLock::acquire(app_data_root)?;
    let project_entries = match fs::read_dir(projects_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => {
            return Err(crate::Error::Other(format!(
                "open projects root {} failed: {error}",
                projects_root.display()
            )));
        }
    };
    let mut published = 0usize;
    let mut first_error: Option<crate::Error> = None;
    for project_entry in project_entries {
        let project_entry = match project_entry {
            Ok(entry) => entry,
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(crate::Error::Other(format!(
                        "read projects root {} failed: {error}",
                        projects_root.display()
                    )));
                }
                continue;
            }
        };
        let Some(project_id) = project_entry
            .file_name()
            .to_str()
            .map(|id| id.to_string())
        else {
            continue;
        };
        let project_root = project_entry.path();
        if !project_root.is_dir() {
            continue;
        }
        let pending_root = project_root.join(PENDING_DIR);
        let entries = match fs::read_dir(&pending_root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(crate::Error::Other(format!(
                        "open correction pending root {} failed: {error}",
                        pending_root.display()
                    )));
                }
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(crate::Error::Other(format!(
                            "read correction pending root {} failed: {error}",
                            pending_root.display()
                        )));
                    }
                    continue;
                }
            };
            let path = entry.path();
            let chapter_id = if path.is_dir() {
                path.file_name().and_then(|name| name.to_str())
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
                path.file_stem().and_then(|stem| stem.to_str())
            } else {
                None
            };
            let Some(chapter_id) = chapter_id else {
                continue;
            };
            match publish_pending(&project_root, &project_id, chapter_id) {
                Ok(count) => published = published.saturating_add(count),
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(published),
    }
}
