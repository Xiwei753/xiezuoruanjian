//! Transfer helper functions — shared utilities used by transfer.rs and other submodules.
//!
//! Contains error conversion, remote object operations, staging downloads,
//! and per-kind transfer functions.

use std::path::Path;

use crate::sync::provider::SyncProvider;
use crate::sync::cancellation_token::SyncCancellationToken;
use crate::sync::types::{SyncResult, SyncTarget};

use super::{FullSyncPlan, LiveTargetLww, PlannedTarget};

// 生命周期 target（恢复 / 删除本地 / 删除远端）的传输实现。
// 普通 App 组与作品不再有各自的传输/合并实现：统一走 `transfer_object_scope`。
mod transfer_project_ops;

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

/// 统一对象传输（Issue #1066）：只把远端变化对象下载进 staging。
///
/// 不做业务合并、不做 LWW 裁决、不复制本地目录。远端索引记录与下载结果都写进
/// staging，由提交阶段用同一套对象比较做唯一一次裁决。
pub(super) fn transfer_object_scope(
    provider: &dyn SyncProvider,
    target: &SyncTarget,
    live_root: &Path,
    staging_root: Option<&Path>,
    cancellation_token: Option<&SyncCancellationToken>,
) -> SyncResult {
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] transfer_object_scope: cancellation requested — skipping {}",
                target.remote_prefix
            );
            return SyncResult::success();
        }
    }
    let Some(staging_root) = staging_root else {
        return SyncResult::error(
            crate::sync::SyncStatus::FatalError("missing staging root".to_string()),
            "transfer_object_scope called without a staging root".to_string(),
            None,
        );
    };
    match crate::sync::staging::download::stage_remote_changes(
        provider,
        target,
        live_root,
        staging_root,
    ) {
        Ok(outcome) => {
            let mut result = SyncResult::success();
            result.downloaded_files = outcome.downloaded;
            result.remote_deletes = outcome.remote_deletes;
            result
        }
        Err(error) => sync_result_from_error(error),
    }
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

// ── Per-kind transfer functions ──
