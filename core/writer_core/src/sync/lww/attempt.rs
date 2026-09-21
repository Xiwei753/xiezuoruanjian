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

use super::merge::merge_remote_into_local_snapshot;
use super::transfer::{delete_remote_files, upload_local_files, upload_manifest};

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
    let outcome =
        merge_remote_into_local_snapshot(sync_root, provider, remote_prefix, scope, state)?;

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

    let has_doc_conflicts = !outcome.conflicts.is_empty();
    let has_changes = !outcome.remote_upload_paths.is_empty()
        || !outcome.downloaded_files.is_empty()
        || !outcome.local_deletes.is_empty()
        || !outcome.remote_delete_paths.is_empty();

    if has_doc_conflicts {
        result.status = SyncStatus::PartialConflict;
        result.conflicts = outcome.conflicts;
    } else if !outcome.pending_take_remote_failed.is_empty() {
        result.status = SyncStatus::RecoverableError(format!(
            "pending_take_remote_failed: {}",
            outcome.pending_take_remote_failed.join(", ")
        ));
        result.error = Some(format!(
            "pending_take_remote: remote file missing for paths: {}",
            outcome.pending_take_remote_failed.join(", ")
        ));
    } else if has_changes {
        result.status = SyncStatus::LatestWinsApplied;
    } else {
        result.status = SyncStatus::NoChanges;
    }

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
