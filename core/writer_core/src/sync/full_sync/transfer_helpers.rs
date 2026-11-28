//! Transfer helper functions — shared utilities used by `transfer.rs`.
//!
//! 普通作品、App 组、恢复、删除都只做同一件事：把远端对象事实搬进 staging；
//! 业务裁决只发生在提交阶段。

use std::path::Path;

use crate::sync::cancellation_token::SyncCancellationToken;
use crate::sync::provider::SyncProvider;
use crate::sync::types::{SyncResult, SyncTarget};

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

/// 统一对象传输（Issue #1066）：只把远端变化对象下载进 staging。
///
/// 远端索引记录与下载结果都写进 staging，由提交阶段用同一套对象比较做唯一一次裁决。
pub(super) fn transfer_object_scope(
    provider: &dyn SyncProvider,
    target: &SyncTarget,
    live_root: &Path,
    known_root: &Path,
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
        known_root,
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
