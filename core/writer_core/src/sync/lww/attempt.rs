//! LWW 同步单次尝试编排。
//!
//! 本文件实现 [`execute_lww_sync_attempt`]：调用统一的 merge 核心
//! [`super::merge::merge_remote_into_local_snapshot`] 做文件级 LWW 合并，
//! 然后根据 [`super::merge::LwwMergeOutcome`] 做远端写（upload / delete / upload_manifest）。
//!
//!   ：不再自己维护第二套合并逻辑，统一复用
//! `merge_remote_into_local_snapshot`，与 `full_sync.rs` LiveProject 同源。

use crate::sync::cancellation_token::SyncCancellationToken;
use crate::sync::provider::SyncProvider;
use crate::sync::types::{SyncResult, SyncState, SyncStatus, SyncTarget};
use std::path::Path;

use super::merge::{
    merge_remote_into_local_app_group, merge_remote_into_local_snapshot, LwwMergeOutcome,
};
use super::transfer::{delete_remote_files, upload_local_files, upload_manifest};

/// 按 merge 结果决定单次尝试的终态。
///
/// #931：本地缺失且无墓碑、远端也无可验证 blob 时优先显式报
/// `local_missing_without_tombstone`（不能当删除传播，也不能假装同步完整）；
/// 其余分支沿用既有优先级：文档冲突 > pending_take_remote 失败 > 有变更 >
/// 无变更。其他路径的 Put/Download/Delete 已按文件级隔离完成，这里只决定状态。
fn apply_outcome_status(result: &mut SyncResult, outcome: &LwwMergeOutcome, remote_prefix: &str) {
    let has_doc_conflicts = !outcome.conflicts.is_empty();
    let has_changes = !outcome.remote_upload_paths.is_empty()
        || !outcome.downloaded_files.is_empty()
        || !outcome.local_deletes.is_empty()
        || !outcome.remote_delete_paths.is_empty();
    let has_auto_adjudication = !outcome.auto_adjudicated_files.is_empty();

    if !outcome.unresolved_missing_paths.is_empty() {
        // 显式报出目标与路径，保留 known_files 基线待后续恢复。
        result.status = SyncStatus::RecoverableError(format!(
            "local_missing_without_tombstone: {}",
            outcome.unresolved_missing_paths.join(", ")
        ));
        result.error = Some(format!(
            "local_missing_without_tombstone: target={} paths=[{}] — local file(s) missing on \
             disk without tombstone and no verifiable remote blob to recover; keeping sync state \
             for later recovery",
            remote_prefix,
            outcome.unresolved_missing_paths.join(", ")
        ));
        result.conflicts = outcome.conflicts.clone();
    } else if has_doc_conflicts {
        result.status = SyncStatus::PartialConflict;
        result.conflicts = outcome.conflicts.clone();
    } else if !outcome.pending_take_remote_failed.is_empty() {
        result.status = SyncStatus::RecoverableError(format!(
            "pending_take_remote_failed: {}",
            outcome.pending_take_remote_failed.join(", ")
        ));
        result.error = Some(format!(
            "pending_take_remote: remote file missing for paths: {}",
            outcome.pending_take_remote_failed.join(", ")
        ));
    } else if has_changes || has_auto_adjudication {
        // 普通单侧变更只是 Success；只有双方相对基线都变化、由 LWW
        // 自动裁决时才标 LatestWinsApplied。
        result.status = if has_auto_adjudication {
            SyncStatus::LatestWinsApplied
        } else {
            SyncStatus::Success
        };
    } else {
        result.status = SyncStatus::NoChanges;
    }
}

/// 执行一次 LWW 同步尝试。
///
/// 调用 [`merge_remote_into_local_snapshot`] 做文件级合并（本地 IO + SyncState 保存），
/// 然后根据 [`LwwMergeOutcome`] 做远端写（upload / delete / upload_manifest）。
///
/// 调用方 `perform_lww_sync` 负责重试（最多 2 次）和错误分类。
///
/// Issue #729 评论 5765306162 问题4：`cancellation_token` 在 merge 后、
/// upload_local_files 后、delete_remote_files 后各检查一次。取消时返回
/// `Ok(SyncResult::success())`（空成功，不继续下一项），与 perform_lww_sync
/// 和 run_single_target 入口取消的语义一致。
pub(crate) fn execute_lww_sync_attempt(
    sync_root: &Path,
    provider: &dyn SyncProvider,
    target: &SyncTarget,
    state: &mut SyncState,
    result: &mut SyncResult,
    cancellation_token: Option<&SyncCancellationToken>,
) -> crate::Result<SyncResult> {
    let remote_prefix = &target.remote_prefix;
    let scope = target.scope;
    log::debug!("[sync] lww step=正在合并 remote_prefix={}", remote_prefix);

    // 调用统一 merge 核心。
    let outcome = match &target.app_group {
        Some(group) => {
            merge_remote_into_local_app_group(sync_root, provider, remote_prefix, group, state)?
        }
        None => merge_remote_into_local_snapshot(sync_root, provider, remote_prefix, scope, state)?,
    };

    // Issue #729 评论 5765306162 问题4：merge 后检查取消令牌。
    // 取消则返回空成功，不继续 upload/delete/upload_manifest。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] lww step=cancelled after merge remote_prefix={} — returning success",
                remote_prefix
            );
            result.status = SyncStatus::Success;
            return Ok(result.clone());
        }
    }

    // #931 review：merge 已把"本地缺失且远端无可验证 blob"的路径收进
    // unresolved_missing_paths，且 merged_manifest 仍保留其旧 upsert 记录。
    // 此时 upload / delete_remote / upload_manifest 任一远端写都会发布引用
    // 缺失 blob 的清单快照，违反"不能发布引用缺失 blob 的完整快照"的规则
    // （LiveProject 在 transfer_live.rs publish 前同样拦截）。所以在首次
    // upload_local_files 之前直接返回可恢复错误：只报告本轮 merge 已实际产生的
    // 本地效果（下载/本地移入 trash 等已生效），远端发布留待缺失事实恢复后
    // 按已有增量规则进行。
    if !outcome.unresolved_missing_paths.is_empty() {
        apply_outcome_status(result, &outcome, remote_prefix);
        // 本轮没有执行任何远端写：uploaded_files / local_deletes 保持为空；
        // downloaded_files / remote_deletes / overwritten_files / ignored_files
        // 与正常路径同义（merge 内已执行的本地效果）。
        result.uploaded_files = Vec::new();
        result.downloaded_files = outcome.downloaded_files;
        result.local_deletes = Vec::new();
        result.remote_deletes = outcome.local_deletes;
        result.overwritten_files = outcome.overwritten_files;
        result.ignored_files = outcome.ignored_files;
        log::warn!(
            "[sync] lww step=skip remote writes for unresolved missing paths remote_prefix={} \
             paths=[{}]",
            remote_prefix,
            outcome.unresolved_missing_paths.join(", ")
        );
        return Ok(result.clone());
    }

    log::debug!("[sync] lww step=正在上传本地较新文件");
    upload_local_files(
        sync_root,
        provider,
        remote_prefix,
        &outcome.remote_upload_paths,
        &outcome.remote_tree_files,
        cancellation_token,
    )?;

    // Issue #729 评论 5765306162 问题4：upload_local_files 后检查取消令牌。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] lww step=cancelled after upload_local_files remote_prefix={} — returning success",
                remote_prefix
            );
            result.status = SyncStatus::Success;
            return Ok(result.clone());
        }
    }

    delete_remote_files(
        provider,
        remote_prefix,
        &outcome.remote_delete_paths,
        &outcome.remote_tree_files,
        cancellation_token,
    )?;

    // Issue #729 评论 5765306162 问题4：delete_remote_files 后检查取消令牌。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] lww step=cancelled after delete_remote_files remote_prefix={} — returning success",
                remote_prefix
            );
            result.status = SyncStatus::Success;
            return Ok(result.clone());
        }
    }

    upload_manifest(
        provider,
        &outcome.remote_manifest_path,
        &outcome.manifest_json,
        &outcome.remote_tree_files,
        cancellation_token,
    )?;

    apply_outcome_status(result, &outcome, remote_prefix);

    result.uploaded_files = outcome.remote_upload_paths;
    result.downloaded_files = outcome.downloaded_files;
    //   result.local_deletes = 本地发起的删除（删了远端），
    // result.remote_deletes = 远端发起的删除（删了本地，已 move_to_trash）。
    result.local_deletes = outcome.remote_delete_paths;
    result.remote_deletes = outcome.local_deletes;
    result.overwritten_files = outcome.overwritten_files;
    result.ignored_files = outcome.ignored_files;

    log::debug!("[sync] lww step=同步完成");
    Ok(result.clone())
}
