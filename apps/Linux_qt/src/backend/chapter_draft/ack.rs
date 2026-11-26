// Core durable receipt 与草稿 ack 之间的可重放桥。
//
// Issue #1062 复审：Core journal 已经确认正文/证明批次后，如果本地
// mark_committed 因 meta/事实写盘错误失败，旧事件会留在 facts/ 里；下一次
// 保存重新冻结它们时会与新 base 的 before digest 不匹配，永久
// PROOF_BATCH_MISMATCH。因此 Core 回执一返回就先持久化 pending-ack
// （transaction id + 已确认 event IDs + 新版本），下次 freeze/恢复前幂等
// 重放，成功后才删除 ack 文件、推进草稿 base 并回收已确认前缀。

use std::path::{Path, PathBuf};

use super::store::DraftLocation;

pub(super) const ACK_DIR: &str = "pending-acks";

/// 一笔已由 Core durable journal 确认、但草稿侧尚未完成 ack 的提交。
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct PendingAck {
    pub transaction_id: String,
    pub event_ids: Vec<String>,
    pub base_generation: String,
    pub base_revision: u64,
    pub base_sha256: String,
}

fn ack_dir(location: &DraftLocation) -> PathBuf {
    location.dir.join(ACK_DIR)
}

fn ack_path(location: &DraftLocation, transaction_id: &str) -> PathBuf {
    let safe: String = transaction_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    ack_dir(location).join(format!("{safe}.json"))
}

/// 持久化一笔待补做的 ack；必须在 mark_committed 之前调用。
pub(crate) fn record_pending_ack(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    ack: &PendingAck,
) -> Result<(), String> {
    if ack.transaction_id.is_empty() {
        return Err(format!(
            "pending ack without transaction id: {project_id}/{chapter_id}"
        ));
    }
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Err(format!("invalid draft identity: {project_id}/{chapter_id}"));
    };
    let dir = ack_dir(&location);
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("create pending ack dir {}: {error}", dir.display()))?;
    let json = serde_json::to_string(ack)
        .map_err(|error| format!("encode pending ack {}: {error}", ack.transaction_id))?;
    let path = ack_path(&location, &ack.transaction_id);
    writer_core::storage::atomic_write_string(&path, &json)
        .map_err(|error| format!("write pending ack {}: {error}", path.display()))
}

/// ack 已经完成（meta 落盘、前缀删除）后删除 ack 文件。
pub(crate) fn complete_pending_ack(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    transaction_id: &str,
) {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return;
    };
    let path = ack_path(&location, transaction_id);
    if let Err(error) = std::fs::remove_file(&path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            log::warn!("remove pending ack {}: {error}", path.display());
        }
    }
}

/// 重放所有待补做的 ack（按文件名排序，幂等）。
///
/// 任何不可读/损坏的 ack 文件都保留并返回 Err：保存/恢复必须继续被挡住，
/// 不能把“ack 未完成”当成没有未提交证明。
pub(crate) fn replay_pending_acks(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Result<usize, String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Ok(0);
    };
    let dir = ack_dir(&location);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(format!("read pending acks dir {}: {error}", dir.display())),
    };
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("read pending acks dir {}: {error}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            paths.push(path);
        }
    }
    paths.sort();
    let mut replayed = 0usize;
    for path in paths {
        let content = std::fs::read_to_string(&path)
            .map_err(|error| format!("read pending ack {}: {error}", path.display()))?;
        let ack: PendingAck = serde_json::from_str(&content).map_err(|error| {
            format!("pending ack corrupted (kept): {}: {error}", path.display())
        })?;
        if ack.transaction_id.is_empty() || ack.event_ids.is_empty() {
            return Err(format!("pending ack incomplete (kept): {}", path.display()));
        }
        super::mark_committed(
            projects_root,
            project_id,
            chapter_id,
            &ack.event_ids,
            &ack.base_generation,
            ack.base_revision,
            &ack.base_sha256,
        )
        .map_err(|error| {
            format!(
                "pending ack {} replay failed (kept): {error}",
                ack.transaction_id
            )
        })?;
        if let Err(error) = std::fs::remove_file(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!("remove replayed pending ack {}: {error}", path.display());
            }
        }
        replayed = replayed.saturating_add(1);
    }
    Ok(replayed)
}

/// 当前待补做的 ack 数量（诊断/测试）。
pub(crate) fn pending_ack_count(projects_root: &Path, project_id: &str, chapter_id: &str) -> usize {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return 0;
    };
    std::fs::read_dir(ack_dir(&location))
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| {
                    entry.path().extension().and_then(|ext| ext.to_str()) == Some("json")
                })
                .count()
        })
        .unwrap_or(0)
}
