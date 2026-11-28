//! journal 投影确认入口：Core 统计投影与平台草稿 ack 分开确认。
//!
//! Issue #1062 评论 7191076411：草稿 ack 是 Core durable 决议的一部分，
//! 只有 Core 统计投影和草稿确认都清空后事务才 Finished 并清理。
//!
//! Issue #1069 评论 3：确认入口不是单文件原子写，而是“读取 manifest → 修改
//! ack 集合 → Decided 阶段发布 live 投影 → 写回 manifest → Finished 清理”的
//! 完整读改写。调用方必须持有与正文提交/恢复相同的工作区写锁，且同一批
//! eventId 只读一次、写回一次，避免两个并发 ack 互相覆盖。

use super::super::publish::publish_decided;
use super::*;

/// 确认一个投影完成。所有投影确认后 journal 进入 Finished 并清理。
///
/// 当前编辑保存路径在 receipt 返回后补写统计/证明；平台端重发同一批事件时
/// 可调用本函数推进 journal。调用方应在工作区写锁内调用；已有外层 guard 时
/// 本入口复用同一把锁，保证整个读改写过程受锁保护。
pub fn mark_chapter_commit_projection(
    project_root: &Path,
    key: &ChapterKey,
    transaction_id: &str,
    projection_id: &str,
) -> Result<bool> {
    confirm_pending_projections_in_workspace(project_root, key, transaction_id, &[projection_id])
}

/// 一次确认同一事务的多个 Core 统计投影，只读一次 manifest、只写一次。
///
/// 编辑保存/恢复轮里同一批证明事件按因果顺序批量落盘后，用这个入口批量
/// 回执，避免每个事件都重新读写 manifest。
pub fn mark_chapter_commit_projections(
    project_root: &Path,
    key: &ChapterKey,
    transaction_id: &str,
    projection_ids: &[String],
) -> Result<bool> {
    let ids: Vec<&str> = projection_ids.iter().map(String::as_str).collect();
    confirm_pending_projections_in_workspace(project_root, key, transaction_id, &ids)
}

/// 在调用方已持有的工作区写锁内确认 Core 统计投影。
///
/// journal ack 不能从 `project_root` 反推 app data root 自己再取一把锁：那会
/// 形成第二把分支锁，和正文提交/恢复的锁顺序不一致。已有外层 guard 时按同一
/// root 嵌套获取（`WorkspaceWriteLock` 支持同线程同根重入），让本入口的完整
/// 过程都落在锁内；没有外层 guard 时只允许推进已 Applied 的事务，Decided 的
/// live 发布一律返回错误，由持锁入口完成。
fn confirm_pending_projections_in_workspace(
    project_root: &Path,
    key: &ChapterKey,
    transaction_id: &str,
    projection_ids: &[&str],
) -> Result<bool> {
    if let Some(root) = crate::storage::WorkspaceWriteLock::current_root() {
        let guard = crate::storage::WorkspaceWriteLock::acquire(&root)?;
        return confirm_pending_projections(
            project_root,
            key,
            transaction_id,
            projection_ids,
            Some(&guard),
        );
    }
    confirm_pending_projections(project_root, key, transaction_id, projection_ids, None)
}

fn confirm_pending_projections(
    project_root: &Path,
    key: &ChapterKey,
    transaction_id: &str,
    projection_ids: &[&str],
    workspace_lock: Option<&crate::storage::WorkspaceWriteLock>,
) -> Result<bool> {
    let tx_dir = chapter_transaction_dir(project_root, key, transaction_id);
    let manifest_path = tx_dir.join("manifest.json");
    if !manifest_path.exists() {
        return Ok(false);
    }
    let mut manifest: ChapterCommitManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    if manifest.phase == ChapterCommitPhase::Decided && workspace_lock.is_none() {
        // Decided 尚未发布 live 投影；没有工作区锁时不允许在这里触发发布，
        // 保留 journal 交给持锁的保存/恢复入口。
        return Err(Error::Other(format!(
            "chapter commit ack cannot publish Decided journal without workspace write lock: \
             tx={}",
            manifest.transaction_id
        )));
    }
    manifest
        .pending_projections
        .retain(|id| !projection_ids.contains(&id.as_str()));
    settle_manifest_acks(project_root, &tx_dir, &mut manifest)
}

/// 平台草稿完成一个原始证明事件的精确 ack。
///
/// Issue #1062：Core 只记录“草稿仍需确认这些事件”；草稿侧完成 ack 后
/// （pending-ack 重放写入 meta 成功）调用本函数清除。所有草稿 ack 和 Core
/// 统计投影都清空后 journal 才 Finished。
///
/// `guard` 必须覆盖整个读改写过程；本函数不重新取锁。
pub fn mark_chapter_commit_draft_ack(
    project_root: &Path,
    guard: &crate::storage::WorkspaceWriteLock,
    key: &ChapterKey,
    transaction_id: &str,
    projection_id: &str,
) -> Result<bool> {
    let ids = [projection_id];
    let (_, finished) =
        mark_chapter_commit_draft_acks(project_root, guard, key, transaction_id, &ids)?;
    Ok(finished)
}

/// 一次确认同一事务的多个平台草稿 ack，只读一次 manifest、只写一次。
///
/// 返回本次真实清除的 eventId 数量。同一调用里多个 ID 共用同一次读取和写回，
/// 不会为每个 ID 各做一遍 read-modify-write。传入空列表时不清除任何 ID，
/// 但照样执行一次收口（清空后的 journal 会被 Finished/清理），返回 0。
pub fn mark_chapter_commit_draft_acks(
    project_root: &Path,
    _guard: &crate::storage::WorkspaceWriteLock,
    key: &ChapterKey,
    transaction_id: &str,
    projection_ids: &[&str],
) -> Result<u32> {
    let tx_dir = chapter_transaction_dir(project_root, key, transaction_id);
    let manifest_path = tx_dir.join("manifest.json");
    if !manifest_path.exists() {
        return Ok(0);
    }
    let mut manifest: ChapterCommitManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    let before = manifest.draft_ack_projections.len();
    manifest
        .draft_ack_projections
        .retain(|id| !projection_ids.contains(&id.as_str()));
    let acknowledged = before.saturating_sub(manifest.draft_ack_projections.len());
    settle_manifest_acks(project_root, &tx_dir, &mut manifest)?;
    Ok(u32::try_from(acknowledged).unwrap_or(u32::MAX))
}

/// 所有 ack 清空后的统一收口：Decided 先在同一 guard 内发布 live 投影，
/// 再写回 manifest，Finished 才清理事务目录。
///
/// 返回 journal 是否已 Finished。`publish_decided` 发现更新版本时返回 `false`
/// （已归档 Superseded），不写回旧 manifest。
fn settle_manifest_acks(
    project_root: &Path,
    tx_dir: &Path,
    manifest: &mut ChapterCommitManifest,
) -> Result<bool> {
    if manifest.pending_projections.is_empty()
        && manifest.draft_ack_projections.is_empty()
        && matches!(
            manifest.phase,
            ChapterCommitPhase::Applied | ChapterCommitPhase::Decided
        )
    {
        if manifest.phase == ChapterCommitPhase::Decided {
            let published = publish_decided(project_root, tx_dir, manifest)?;
            if !published {
                // publish_decided 发现更新版本时已把本事务归档为 Superseded 并
                // 写出它自己的 manifest/回执；这里不能用内存里的旧 manifest
                // 覆盖归档结果，因此直接返回，事务目录留给冲突归档路径。
                return Ok(false);
            }
        }
        // 同步账本尚未 durable 时保留 Applied，等重放入口补投影；不能因为
        // 统计/草稿确认清空就删掉唯一待同步决议。
        if manifest.sync_projection_done {
            manifest.phase = ChapterCommitPhase::Finished;
        }
    }
    write_manifest(tx_dir, manifest)?;
    if manifest.phase == ChapterCommitPhase::Finished {
        let _ = remove_transaction_dir(project_root, &manifest.key, &manifest.transaction_id);
        return Ok(true);
    }
    Ok(false)
}
