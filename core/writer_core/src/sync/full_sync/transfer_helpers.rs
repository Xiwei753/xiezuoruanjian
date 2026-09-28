//! Transfer helper functions — shared utilities used by transfer.rs and other submodules.
//!
//! Contains error conversion, remote object operations, staging downloads, single-target sync,
//! and per-kind transfer functions extracted from run_transfer.

use std::path::Path;

use crate::sync::cancellation_token::SyncCancellationToken;
use crate::sync::provider::SyncProvider;
use crate::sync::types::{SyncPolicy, SyncResult, SyncTarget};

use super::{FullSyncPlan, LiveTargetLww, PlannedTarget};

// 单 target 的传输实现按目标类别拆分：
//   - transfer_live.rs：正常上传 live 项目
//   - transfer_project_ops.rs：恢复 / 删除本地 / 删除远端
// 两个子模块在这里整体重导出，`use super::transfer_helpers::*` 的调用方不受影响。
mod transfer_live;
mod transfer_project_ops;

pub(super) use transfer_live::transfer_live_project;
pub(super) use transfer_project_ops::{
    transfer_delete_local_project, transfer_delete_remote_project, transfer_restore_project,
};

// ── Lifecycle CAS helper ──

pub(super) fn resolve_current_target_lifecycle(
    provider: &dyn SyncProvider,
    target_id: &str,
) -> crate::error::Result<Option<crate::sync::types::TargetLifecycleRecord>> {
    let snapshot = crate::sync::target_lifecycle::load_remote_catalog(provider)?;
    Ok(crate::sync::target_lifecycle::find_record(&snapshot.catalog, target_id).cloned())
}

// ── Error conversion ──

pub(super) fn sync_result_from_error(err: crate::Error) -> SyncResult {
    use crate::sync::SyncStatus;
    let msg = err.to_string();
    let category = err.sync_category();
    let status = if err.recoverable() {
        SyncStatus::RecoverableError(msg.clone())
    } else {
        SyncStatus::FatalError(msg.clone())
    };
    SyncResult::error(
        status,
        msg,
        (!category.is_empty()).then(|| category.to_string()),
    )
}

pub(super) fn sync_result_from_provider_error(
    err: crate::sync::provider::ProviderError,
) -> SyncResult {
    sync_result_from_error(crate::Error::from(err))
}

// ── Remote operations ──

pub(super) fn download_remote_to_staging(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    staging_root: Option<&Path>,
) -> SyncResult {
    let entries = match provider.list(remote_prefix) {
        Ok(e) => e,
        Err(e) => return sync_result_from_provider_error(e),
    };
    let mut downloaded: Vec<String> = Vec::new();
    for entry in &entries {
        let full_remote_path = format!("{}/{}", remote_prefix, entry.path);
        let obj = match provider.read(&full_remote_path) {
            Ok(Some(obj)) => obj,
            Ok(None) => continue,
            Err(e) => return sync_result_from_provider_error(e),
        };
        if let Some(staging) = staging_root {
            if let Err(e) =
                super::generation::write_staging_file(staging, &entry.path, &obj.content)
            {
                return sync_result_from_error(crate::Error::Io(e));
            }
        }
        downloaded.push(full_remote_path);
    }
    let mut result = SyncResult::success();
    result.downloaded_files = downloaded;
    result
}

pub(super) fn delete_all_remote_objects(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    cancellation_token: Option<&SyncCancellationToken>,
) -> SyncResult {
    // Issue #729 评论 5765979275：provider.list 前先检查取消令牌。
    // 已取消则不开启新的 list 操作。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] delete_all_remote_objects: cancellation requested before list {} — returning success",
                remote_prefix
            );
            return SyncResult::success();
        }
    }
    let remote_entries = match provider.list(remote_prefix) {
        Ok(entries) => entries,
        Err(err) => return sync_result_from_provider_error(err),
    };
    // Issue #729 评论 5765306162 问题5：provider.list 返回后检查取消令牌。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] delete_all_remote_objects: cancellation requested after list {} — returning success",
                remote_prefix
            );
            return SyncResult::success();
        }
    }
    let mut remote_deletes: Vec<String> = Vec::new();
    for entry in &remote_entries {
        if super::generation::is_generation_path(&entry.path) {
            log::debug!(
                "[sync] delete_all_remote_objects: skipping generation path {} under {}",
                entry.path,
                remote_prefix
            );
            continue;
        }
        let full_remote_path = format!("{}/{}", remote_prefix, entry.path);
        match provider.delete(
            &full_remote_path,
            crate::sync::provider::model::DeletePrecondition::Unconditional,
        ) {
            Ok(()) => {
                remote_deletes.push(full_remote_path.clone());
            }
            Err(err) => {
                let mut result = sync_result_from_provider_error(err);
                result.remote_deletes = remote_deletes;
                return result;
            }
        }
        // Issue #729 评论 5765306162 问题5：每次 provider.delete 返回后检查取消令牌。
        // 取消则 break 并返回已完成的结果（已删除的保留，未完成的不继续）。
        if let Some(token) = cancellation_token {
            if token.is_cancelled() {
                log::info!(
                    "[sync] delete_all_remote_objects: cancellation requested after delete {} — breaking with {} deletes done",
                    full_remote_path,
                    remote_deletes.len()
                );
                let mut result = SyncResult::success();
                result.remote_deletes = remote_deletes;
                return result;
            }
        }
    }
    let mut result = SyncResult::success();
    result.remote_deletes = remote_deletes;
    result
}

pub(super) fn run_single_target(
    provider: &dyn SyncProvider,
    local_root: &Path,
    sync_policy: &SyncPolicy,
    target: &SyncTarget,
    force_sync: bool,
    cancellation_token: Option<&SyncCancellationToken>,
) -> SyncResult {
    // Issue #729：关键写入操作前检查取消令牌。
    // 如果已取消，提前返回，不继续写入远端。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!("[sync] run_single_target: cancellation requested — skipping target");
            return SyncResult::success();
        }
    }
    // Issue #729 评论 5765306162 问题4：把 cancellation_token 传进 perform_lww_sync，
    // 不再只在入口检查一次。perform_lww_sync 内部会在 debounce 后、重试循环每次
    // 迭代前、execute_lww_sync_attempt 内部 merge/upload/delete 后各检查一次。
    match crate::sync::SyncService::perform_lww_sync(
        local_root,
        provider,
        sync_policy,
        target,
        force_sync,
        cancellation_token,
    ) {
        Ok(result) => result,
        Err(err) => sync_result_from_error(err),
    }
}

// ── Per-kind transfer functions ──

/// 合并累计的 local merge effects 到最终结果。
///
/// Issue #716 评论 5742849844 问题 2：CAS 重试时，前几轮的本地变化
/// （downloaded_files/local_trashed/overwritten/ignored）不能被后续
/// NoOp 轮次丢弃。任何 attempt 发生过真实本地变化，最终成功类结果
/// 不能再降回 `NoChanges`。
///
/// 只合并本地效果（downloaded/remote_deletes=本地trashed/overwritten/ignored），
/// 不碰 `local_deletes`（远端侧动作），避免覆盖 publish 路径已正确设置的远端删除。
fn merge_accumulated_local_effects(
    result: &mut SyncResult,
    downloaded: &[String],
    trashed: &[String],
    overwritten: &[String],
    ignored: &[String],
) {
    use crate::sync::SyncStatus;
    for f in downloaded {
        if !result.downloaded_files.contains(f) {
            result.downloaded_files.push(f.clone());
        }
    }
    for f in trashed {
        if !result.remote_deletes.contains(f) {
            result.remote_deletes.push(f.clone());
        }
    }
    for f in overwritten {
        if !result.overwritten_files.contains(f) {
            result.overwritten_files.push(f.clone());
        }
    }
    for f in ignored {
        if !result.ignored_files.contains(f) {
            result.ignored_files.push(f.clone());
        }
    }
    // 如果累计有本地变化，状态不能降回 NoChanges。
    let has_accumulated_changes = !downloaded.is_empty() || !trashed.is_empty();
    if has_accumulated_changes && matches!(result.status, SyncStatus::NoChanges) {
        result.status = SyncStatus::LatestWinsApplied;
    }
}
