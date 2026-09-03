use crate::sync::types::SyncErrorCategory;

/// 当前 Unix 秒 — 全量同步持久状态统一时间源。
pub(crate) fn now_epoch_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// transport 初始化失败的类型化 Error 转换。
pub(crate) fn transport_init_failure_error(category: &str, message: &str) -> crate::Error {
    let reason = format!("Transport init failed: {} - {}", category, message);
    match SyncErrorCategory::from_code(category, "") {
        SyncErrorCategory::TokenMissing
        | SyncErrorCategory::TokenInvalid
        | SyncErrorCategory::TokenPermissionDenied
        | SyncErrorCategory::AuthError
        | SyncErrorCategory::RepoNotFoundOrNoPermission => crate::Error::SyncAuthFailed { reason },
        SyncErrorCategory::NetworkFailed
        | SyncErrorCategory::DnsFailed
        | SyncErrorCategory::TlsFailed
        | SyncErrorCategory::NetworkProbeFailed => crate::Error::SyncNetworkUnavailable { reason },
        SyncErrorCategory::ApiRateLimited => crate::Error::SyncRateLimited {
            retry_after_secs: 0,
        },
        _ => crate::Error::SyncAuthFailed { reason },
    }
}

/// 判断 target 状态是否为协议错误。
pub(crate) fn is_protocol_error_status(status: &crate::sync::SyncStatus) -> bool {
    matches!(
        status,
        crate::sync::SyncStatus::Syncing
            | crate::sync::SyncStatus::Idle
            | crate::sync::SyncStatus::ConfiguredNotTested
    )
}

pub(crate) type ProtocolErrorFields = (
    crate::sync::SyncStatus,
    Option<String>,
    Option<String>,
    Option<String>,
);

pub(crate) fn build_protocol_error_fields(
    targets: &[crate::sync::types::TargetSyncResult],
) -> Option<ProtocolErrorFields> {
    if !targets
        .iter()
        .any(|t| is_protocol_error_status(&t.result.status))
    {
        return None;
    }
    let overall_status =
        crate::sync::SyncStatus::FatalError("invalid_target_status_for_aggregation".to_string());
    let dominant = targets
        .iter()
        .find(|t| is_protocol_error_status(&t.result.status));
    let error = dominant.and_then(|t| t.result.error.clone());
    let error_category = dominant.and_then(|t| t.result.error_category.clone());
    let message_key = dominant
        .and_then(|t| t.result.message_key.clone())
        .or_else(|| {
            error_category
                .as_deref()
                .map(sync_error_category_to_message_key_string)
        });
    Some((overall_status, error, error_category, message_key))
}

pub(crate) fn sync_error_category_to_message_key_string(code: &str) -> String {
    SyncErrorCategory::from_code(code, "")
        .to_message_key()
        .to_string()
}

/// 聚合成功类终态。
pub(crate) fn aggregate_success_status(
    targets: &[crate::sync::types::TargetSyncResult],
) -> crate::sync::SyncStatus {
    use crate::sync::SyncStatus;

    if targets
        .iter()
        .any(|t| matches!(t.result.status, SyncStatus::BranchMissingRecovered))
    {
        return SyncStatus::BranchMissingRecovered;
    }
    if targets
        .iter()
        .any(|t| matches!(t.result.status, SyncStatus::LatestWinsApplied))
    {
        return SyncStatus::LatestWinsApplied;
    }
    if !targets.is_empty()
        && targets
            .iter()
            .all(|t| matches!(t.result.status, SyncStatus::NoChanges))
    {
        return SyncStatus::NoChanges;
    }
    SyncStatus::Success
}

/// 单个 target 状态在聚合中的优先级。
pub(crate) fn full_sync_status_priority(status: &crate::sync::SyncStatus) -> u8 {
    match status {
        crate::sync::SyncStatus::FatalError(_) | crate::sync::SyncStatus::Error(_) => 4,
        crate::sync::SyncStatus::DirtyRepoBlocked => 3,
        crate::sync::SyncStatus::Conflict | crate::sync::SyncStatus::PartialConflict => 2,
        crate::sync::SyncStatus::RecoverableError(_) => 1,
        _ => 0,
    }
}

/// 执行单个 target 的同步，把 `Err` 转为该 target 的 `SyncResult::error(...)`。
#[cfg(feature = "github-api")]
pub(crate) fn run_full_sync_target(
    provider: &dyn crate::sync::provider::SyncProvider,
    local_root: &std::path::Path,
    sync_policy: &crate::sync::types::SyncPolicy,
    target: &crate::sync::types::SyncTarget,
    force_sync: bool,
) -> crate::sync::types::SyncResult {
    match crate::sync::lww::perform_lww_sync(local_root, provider, sync_policy, target, force_sync)
    {
        Ok(result) => result,
        Err(err) => {
            let msg = err.to_string();
            let category = err.sync_category();
            let error_category = if category.is_empty() {
                None
            } else {
                Some(category.to_string())
            };
            let status = if err.recoverable() {
                crate::sync::SyncStatus::RecoverableError(msg.clone())
            } else {
                crate::sync::SyncStatus::FatalError(msg.clone())
            };
            crate::sync::types::SyncResult::error(
                status,
                crate::sync::types::FirstSyncMode::NotAttempted,
                msg,
                error_category,
            )
        }
    }
}
