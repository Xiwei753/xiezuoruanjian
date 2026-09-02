//! LWW 同步入口：debounce、重试与错误分类。
//!
//! 本文件只负责同步生命周期编排（前置检查 → debounce → 重试 → 错误分类），
//! 一次完整同步尝试的编排见 [`super::attempt::execute_lww_sync_attempt`]。

use crate::sync::types::{FirstSyncMode, SyncConfig, SyncResult, SyncSecrets, SyncStatus};
use std::path::Path;
use writer_platform_api::SyncTransport;

use super::attempt::execute_lww_sync_attempt;

/// 执行 LWW 同步 — 入口函数。
///
/// 整体流程：前置检查 → debounce → 重试循环（最多 2 次）→ 错误分类。
///
/// 重试策略：最多重试 2 次，间隔 500ms。仅对可恢复错误（网络/限流）重试；
/// 认证/权限等不可恢复错误直接返回，不重试。
///
/// 错误分类（`SyncErrorCategory`）：
/// - `LocalIoError` → Error（不可恢复）
/// - `TokenMissing/TokenInvalid/TokenPermissionDenied/AuthError` → Error（不可恢复）
/// - `ApiRateLimited` → RecoverableError（可恢复，下次同步自动重试）
/// - `GithubNetworkFailed/DnsFailed/TlsFailed/NetworkProbeFailed` → RecoverableError
/// - 其他 → RecoverableError（保守处理，避免误报不可恢复）
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub(crate) fn perform_lww_sync(
    sync_root: &Path,
    config: &SyncConfig,
    secrets: &SyncSecrets,
    target: &crate::sync::types::SyncTarget,
    force_sync: bool,
    transport: &dyn SyncTransport,
) -> crate::Result<SyncResult> {
    let remote_prefix = &target.remote_prefix;
    log::debug!(
        "[sync] backend_type=github_api sync_mode=lww_manifest entry=perform_lww_sync sync_root={} remote_prefix={}",
        sync_root.display(),
        remote_prefix
    );
    let mut result = SyncResult::success();
    result.status = SyncStatus::Idle;

    if !config.enabled {
        result.status = SyncStatus::Success;
        return Ok(result);
    }

    if config.remote_url.is_empty() {
        return Ok(SyncResult::error(
            SyncStatus::Error("Remote URL is empty".to_string()),
            FirstSyncMode::NotAttempted,
            "Remote URL is empty".to_string(),
            Some("empty_url".to_string()),
        ));
    }

    let token = secrets.token.clone().unwrap_or_default();
    if token.is_empty() {
        return Ok(SyncResult::error(
            SyncStatus::Error("No token provided".to_string()),
            FirstSyncMode::NotAttempted,
            "No token provided".to_string(),
            Some("token_missing".to_string()),
        ));
    }

    let mut state = crate::sync::SyncService::load_sync_state(sync_root)?;
    if state.device_id.is_empty() {
        state.device_id = uuid::Uuid::new_v4().to_string();
        crate::sync::SyncService::save_sync_state(sync_root, &state)?;
    }

    // P1-4: Core-level debounce. Even if clients call sync too often,
    // the core enforces a minimum interval to prevent network I/O flood.
    // This is a safety net; clients should also debounce.
    // However, force_sync=true bypasses this debounce for manual sync,
    // conflict resolution, and first configuration.
    if !force_sync {
        let min_interval = i64::from(config.sync_interval_seconds.max(60));
        if let Some(last_sync) = state.last_sync_time {
            let now = chrono::Utc::now().timestamp();
            let elapsed = now - last_sync;
            if elapsed >= 0 && elapsed < min_interval {
                // 冲突解决后（pending_take_remote 非空）必须绕过 debounce，
                // 否则用户解决冲突后可能要等 60 秒才能同步到远端内容
                if !state.pending_take_remote.is_empty() {
                    log::debug!(
                        "[sync] debounce bypassed: pending_take_remote has {} entries",
                        state.pending_take_remote.len()
                    );
                } else {
                    log::debug!(
                        "[sync] debounce: last_sync={}s ago, min_interval={}s, skipping",
                        elapsed,
                        min_interval
                    );
                    result.status = SyncStatus::Success;
                    return Ok(result);
                }
            }
        }
    }

    let api_base =
        crate::sync::provider::github_backend::GitHubApiBackend::api_base_url(&config.remote_url);

    let max_retries = 2;
    let mut attempt = 0;
    loop {
        match execute_lww_sync_attempt(
            sync_root,
            config,
            &token,
            &api_base,
            target,
            transport,
            &mut state,
            &mut result,
        ) {
            Ok(res) => return Ok(res),
            Err(e) => {
                attempt += 1;
                if attempt >= max_retries {
                    let err = e.to_string();
                    let category =
                        crate::sync::types::SyncErrorCategory::from_code(e.sync_category(), &err);
                    result.status = match category {
                        crate::sync::types::SyncErrorCategory::LocalIoError => {
                            SyncStatus::Error("local_io_error".to_string())
                        }
                        crate::sync::types::SyncErrorCategory::TokenMissing
                        | crate::sync::types::SyncErrorCategory::TokenInvalid
                        | crate::sync::types::SyncErrorCategory::TokenPermissionDenied
                        | crate::sync::types::SyncErrorCategory::AuthError => {
                            SyncStatus::Error(category.to_ui_status().to_string())
                        }
                        crate::sync::types::SyncErrorCategory::ApiRateLimited => {
                            SyncStatus::RecoverableError("api_rate_limited".to_string())
                        }
                        crate::sync::types::SyncErrorCategory::GithubNetworkFailed
                        | crate::sync::types::SyncErrorCategory::DnsFailed
                        | crate::sync::types::SyncErrorCategory::TlsFailed
                        | crate::sync::types::SyncErrorCategory::NetworkProbeFailed => {
                            SyncStatus::RecoverableError("network_error".to_string())
                        }
                        _ => SyncStatus::RecoverableError("api_error".to_string()),
                    };
                    result.error = Some(err.clone());
                    return Ok(result);
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
}
