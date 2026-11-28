//! journal 投影确认入口：Core 统计投影与平台草稿 ack 分开确认。
//!
//! Issue #1062 评论 7191076411：草稿 ack 是 Core durable 决议的一部分，
//! 只有 Core 统计投影和草稿确认都清空后事务才 Finished 并清理。

use super::super::publish::publish_decided;
use super::*;

/// 确认一个投影完成。所有投影确认后 journal 进入 Finished 并清理。
///
/// 当前编辑保存路径在 receipt 返回后补写统计/证明；平台端重发同一批事件时
/// 可调用本函数推进 journal。
pub fn mark_chapter_commit_projection(
    project_root: &Path,
    transaction_id: &str,
    projection_id: &str,
) -> Result<bool> {
    confirm_pending_projections(project_root, transaction_id, &[projection_id])
}

/// 一次确认同一事务的多个 Core 统计投影，只写一次 manifest。
///
/// 编辑保存/恢复轮里同一批证明事件按因果顺序批量落盘后，用这个入口批量
/// 回执，避免每个事件都重新读写 manifest。
pub fn mark_chapter_commit_projections(
    project_root: &Path,
    transaction_id: &str,
    projection_ids: &[String],
) -> Result<bool> {
    let ids: Vec<&str> = projection_ids.iter().map(String::as_str).collect();
    confirm_pending_projections(project_root, transaction_id, &ids)
}

fn confirm_pending_projections(
    project_root: &Path,
    transaction_id: &str,
    projection_ids: &[&str],
) -> Result<bool> {
    let tx_dir = find_transaction_dir(project_root, transaction_id)?;
    let manifest_path = tx_dir.join("manifest.json");
    if !manifest_path.exists() {
        return Ok(false);
    }
    let mut manifest: ChapterCommitManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    manifest
        .pending_projections
        .retain(|id| !projection_ids.contains(&id.as_str()));
    if manifest.pending_projections.is_empty()
        && manifest.draft_ack_projections.is_empty()
        && matches!(
            manifest.phase,
            ChapterCommitPhase::Applied | ChapterCommitPhase::Decided
        )
    {
        if manifest.phase == ChapterCommitPhase::Decided {
            let published = publish_decided(project_root, &tx_dir, &mut manifest)?;
            if !published {
                return Ok(false);
            }
        }
        manifest.phase = ChapterCommitPhase::Finished;
    }
    write_manifest(&tx_dir, &manifest)?;
    if manifest.phase == ChapterCommitPhase::Finished {
        let _ = remove_transaction_dir(&tx_dir);
        return Ok(true);
    }
    Ok(false)
}

/// 平台草稿完成一个原始证明事件的精确 ack。
///
/// Issue #1062：Core 只记录“草稿仍需确认这些事件”；草稿侧完成 ack 后
/// （pending-ack 重放写入 meta 成功）调用本函数清除。所有草稿 ack 和 Core
/// 统计投影都清空后 journal 才 Finished。
pub fn mark_chapter_commit_draft_ack(
    project_root: &Path,
    transaction_id: &str,
    projection_id: &str,
) -> Result<bool> {
    let tx_dir = find_transaction_dir(project_root, transaction_id)?;
    let manifest_path = tx_dir.join("manifest.json");
    if !manifest_path.exists() {
        return Ok(false);
    }
    let mut manifest: ChapterCommitManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    manifest
        .draft_ack_projections
        .retain(|id| id != projection_id);
    if manifest.pending_projections.is_empty()
        && manifest.draft_ack_projections.is_empty()
        && matches!(
            manifest.phase,
            ChapterCommitPhase::Applied | ChapterCommitPhase::Decided
        )
    {
        if manifest.phase == ChapterCommitPhase::Decided {
            let published = publish_decided(project_root, &tx_dir, &mut manifest)?;
            if !published {
                return Ok(false);
            }
        }
        manifest.phase = ChapterCommitPhase::Finished;
    }
    write_manifest(&tx_dir, &manifest)?;
    if manifest.phase == ChapterCommitPhase::Finished {
        let _ = remove_transaction_dir(&tx_dir);
        return Ok(true);
    }
    Ok(false)
}
