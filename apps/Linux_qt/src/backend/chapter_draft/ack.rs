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
    /// 本次提交快照的编辑器会话与 revision；重放时用于精确收口正文。
    pub editor_session_id: u64,
    pub snapshot_revision: u64,
    pub snapshot_digest: String,
    /// Core journal 的项目级提交序号；重放顺序的唯一依据。
    #[serde(default)]
    pub commit_sequence: u64,
    /// 本地草稿已完成 ack（mark_committed 成功），但 Core journal 的
    /// 草稿确认尚未回写。只有回写成功后才删除文件。
    #[serde(default)]
    pub reconciled: bool,
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
    let path = ack_path(&location, &ack.transaction_id);
    // 同一 transaction 已存在本地 ack 时保留它携带的编辑器快照坐标与
    // reconciled 决议：Core journal 对账写回的 ack 只有章节版本坐标，直接
    // 覆盖会丢失“精确收口已提交正文”的能力（Issue #1062 评论 7193847871）。
    let mut ack = ack.clone();
    if let Ok(existing) = std::fs::read_to_string(&path) {
        if let Ok(existing) = serde_json::from_str::<PendingAck>(&existing) {
            if ack.snapshot_digest.is_empty() && !existing.snapshot_digest.is_empty() {
                ack.editor_session_id = existing.editor_session_id;
                ack.snapshot_revision = existing.snapshot_revision;
                ack.snapshot_digest = existing.snapshot_digest;
            }
            if existing.reconciled && existing.event_ids == ack.event_ids {
                ack.reconciled = true;
            }
        }
    }
    let json = serde_json::to_string(&ack)
        .map_err(|error| format!("encode pending ack {}: {error}", ack.transaction_id))?;
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

/// 重放所有待补做的 ack（幂等，按对象版本单调顺序）。
///
/// Issue #1062 复审：ack 文件名是 UUID，不能按文件名执行；多笔 ack 必须按
/// `base_revision` 递增顺序推进，否则旧事务会把已推进的 base 降回去。
/// 本地 mark_committed 成功后只把 ack 标为 `reconciled`，文件保留到 Core
/// journal 的草稿确认回写成功（见 `complete_pending_ack`）。
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
    let mut acks: Vec<(PendingAck, PathBuf)> = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("read pending acks dir {}: {error}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|error| format!("read pending ack {}: {error}", path.display()))?;
        let ack: PendingAck = serde_json::from_str(&content).map_err(|error| {
            format!("pending ack corrupted (kept): {}: {error}", path.display())
        })?;
        if ack.transaction_id.is_empty() || ack.event_ids.is_empty() {
            return Err(format!("pending ack incomplete (kept): {}", path.display()));
        }
        acks.push((ack, path));
    }
    // Issue #1062 评论 7194373427 第 3 项：重放顺序以 Core journal 的项目级
    // commit_sequence 为第一依据（缺失/旧文件为 0，排在最前），generation
    // 是 UUID，绝不能当排序键；同一序号再按 base revision 与事务 ID 稳定化。
    acks.sort_by(|(left, _), (right, _)| {
        (
            left.commit_sequence,
            left.base_revision,
            &left.transaction_id,
        )
            .cmp(&(
                right.commit_sequence,
                right.base_revision,
                &right.transaction_id,
            ))
    });
    let mut replayed = 0usize;
    for (mut ack, path) in acks {
        // 带编辑器快照坐标的 ack 可以顺带收口已提交正文；只来自 Core
        // journal 的兼容 ack（旧文件/无快照信息）只结算事实与 base。
        let settlement =
            (!ack.snapshot_digest.is_empty() || ack.snapshot_revision > 0).then(|| {
                super::TextSettlement {
                    session_id: ack.editor_session_id,
                    snapshot_revision: ack.snapshot_revision,
                    snapshot_digest: ack.snapshot_digest.clone(),
                }
            });
        super::mark_committed_with_settlement(
            projects_root,
            project_id,
            chapter_id,
            &ack.transaction_id,
            &ack.event_ids,
            &ack.base_generation,
            ack.base_revision,
            &ack.base_sha256,
            settlement.as_ref(),
            ack.commit_sequence,
        )
        .map_err(|error| {
            format!(
                "pending ack {} replay failed (kept): {error}",
                ack.transaction_id
            )
        })?;
        if !ack.reconciled {
            ack.reconciled = true;
            let json = serde_json::to_string(&ack).map_err(|error| {
                format!(
                    "encode reconciled pending ack {}: {error}",
                    ack.transaction_id
                )
            })?;
            writer_core::storage::atomic_write_string(&path, &json).map_err(|error| {
                format!("write reconciled pending ack {}: {error}", path.display())
            })?;
        }
        replayed = replayed.saturating_add(1);
    }
    Ok(replayed)
}

/// 把某笔 ack 标为已本地完成（mark_committed 成功后由 worker 调用）。
pub(crate) fn reconciled_ack(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    transaction_id: &str,
) {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return;
    };
    let path = ack_path(&location, transaction_id);
    let Ok(content) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(mut ack) = serde_json::from_str::<PendingAck>(&content) else {
        return;
    };
    if ack.reconciled {
        return;
    }
    ack.reconciled = true;
    if let Ok(json) = serde_json::to_string(&ack) {
        if let Err(error) = writer_core::storage::atomic_write_string(&path, &json) {
            log::warn!("write reconciled pending ack {}: {error}", path.display());
        }
    }
}

/// 本地已完成、等待回写 Core journal 的 ack（按 commit_sequence 递增）。
pub(crate) fn reconciled_acks(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Result<Vec<(PendingAck, PathBuf)>, String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Ok(Vec::new());
    };
    let dir = ack_dir(&location);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("read pending acks dir {}: {error}", dir.display())),
    };
    let mut acks = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("read pending acks dir {}: {error}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(ack) = serde_json::from_str::<PendingAck>(&content) else {
            continue;
        };
        if ack.reconciled {
            acks.push((ack, path));
        }
    }
    acks.sort_by(|(left, _), (right, _)| {
        (
            left.commit_sequence,
            left.base_revision,
            &left.transaction_id,
        )
            .cmp(&(
                right.commit_sequence,
                right.base_revision,
                &right.transaction_id,
            ))
    });
    Ok(acks)
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
