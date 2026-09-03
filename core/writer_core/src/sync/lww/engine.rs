//! LWW 同步入口：debounce、重试与错误分类。
//!
//! 本文件只负责同步生命周期编排（前置检查 → debounce → 重试 → 错误分类），
//! 一次完整同步尝试的编排见 [`super::attempt::execute_lww_sync_attempt`]。
//!
//! 通过 [`crate::sync::provider::SyncProvider`] trait 与具体后端解耦，
//! 不直接依赖 `SyncConfig`/`SyncSecrets`/`SyncTransport`。
//! 认证/URL/分支解析在创建 Provider 时完成，engine 只接收 [`SyncPolicy`]。
//! Provider 能力（`SyncCapabilities`）影响传输策略：`conditional_write` 决定
//! 前置条件类型，`max_parallel_downloads` 控制并行下载线程数。

use crate::sync::provider::SyncProvider;
use crate::sync::types::{SyncPolicy, SyncResult, SyncStatus};
use std::path::Path;

use super::attempt::execute_lww_sync_attempt;

/// 执行 LWW 同步 — 入口函数。
///
/// 整体流程：前置检查 → debounce → 重试循环（最多 2 次）→ 错误分类。
///
/// 重试策略：最多重试 2 次，间隔 500ms。仅对可恢复错误（网络/限流）重试；
/// 认证/权限等不可恢复错误直接返回，不重试。
///
/// 错误分类（`SyncErrorCategory`，Issue #645 评论 5504296097 第1点起 provider-neutral）：
/// - `LocalIo` → Error（不可恢复）
/// - `AuthFailed` / `PermissionDenied` → Error（不可恢复）
/// - `NotFound` → Error("not_found")（不可恢复）
/// - `PreconditionFailed` → Conflict（不可恢复，需用户决策或拉取远端最新后重试）
/// - `RateLimited` → RecoverableError（可恢复，下次同步自动重试）
/// - `Network` / `TemporaryUnavailable` → RecoverableError
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
    provider: &dyn SyncProvider,
    sync_policy: &SyncPolicy,
    target: &crate::sync::types::SyncTarget,
    force_sync: bool,
) -> crate::Result<SyncResult> {
    let remote_prefix = &target.remote_prefix;
    log::debug!(
        "[sync] sync_mode=lww_manifest entry=perform_lww_sync sync_root={} remote_prefix={}",
        sync_root.display(),
        remote_prefix
    );
    let mut result = SyncResult::success();
    result.status = SyncStatus::Idle;

    if !sync_policy.enabled {
        result.status = SyncStatus::Success;
        return Ok(result);
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
        let min_interval = i64::from(sync_policy.sync_interval_seconds.max(60));
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

    let max_retries = 2;
    let mut attempt = 0;
    loop {
        match execute_lww_sync_attempt(sync_root, provider, target, &mut state, &mut result) {
            Ok(res) => return Ok(res),
            Err(e) => {
                // 不可恢复错误（认证/权限/precondition conflict/file_not_found 等）
                // 直接分类返回，不 sleep 不重试——重试也不会成功，反而拖延用户感知。
                // 可恢复性判断依赖 Error::recoverable() 的结构化实现：
                // SyncRemoteError 按 category 区分（precondition_failed/file_not_found 不可恢复），
                // 其他变体（SyncAuthFailed 不可恢复、SyncNetworkUnavailable/SyncRateLimited 可恢复等）。
                if !e.recoverable() {
                    let err = e.to_string();
                    result.status = classify_sync_error(&e);
                    result.error = Some(err);
                    return Ok(result);
                }
                // 可恢复错误（网络/限流等）才进入退避重试
                attempt += 1;
                if attempt >= max_retries {
                    let err = e.to_string();
                    result.status = classify_sync_error(&e);
                    result.error = Some(err);
                    return Ok(result);
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
}

/// 将同步错误分类为终端 [`SyncStatus`]。
///
/// 供 [`perform_lww_sync`] 重试循环在不可恢复错误或达到最大重试次数时复用，
/// 避免错误分类逻辑在两个分支重复。
///
/// Issue #645 评论 5504296097 第1点：分类规则与新的 provider-neutral
/// `SyncErrorCategory` 对齐：
/// - `LocalIo` → `Error("local_io")`
/// - `AuthFailed` / `PermissionDenied` → `Error(to_ui_status)`（不可恢复）
/// - `NotFound` → `Error("not_found")`（不可恢复）
/// - `PreconditionFailed` → `Conflict`（不可恢复，需用户决策或拉取远端最新后重试）
/// - `RateLimited` → `RecoverableError("rate_limited")`
/// - `Network` / `TemporaryUnavailable` → `RecoverableError("network")`
/// - 其他 → `RecoverableError("api_error")`（保守处理，避免误报不可恢复）
fn classify_sync_error(e: &crate::Error) -> SyncStatus {
    let category = crate::sync::types::SyncErrorCategory::from_code(e.sync_category(), "");
    match category {
        crate::sync::types::SyncErrorCategory::LocalIo => SyncStatus::Error("local_io".to_string()),
        crate::sync::types::SyncErrorCategory::AuthFailed
        | crate::sync::types::SyncErrorCategory::PermissionDenied => {
            SyncStatus::Error(category.to_ui_status().to_string())
        }
        crate::sync::types::SyncErrorCategory::NotFound => {
            SyncStatus::Error("not_found".to_string())
        }
        crate::sync::types::SyncErrorCategory::PreconditionFailed => SyncStatus::Conflict,
        crate::sync::types::SyncErrorCategory::RateLimited => {
            SyncStatus::RecoverableError("rate_limited".to_string())
        }
        crate::sync::types::SyncErrorCategory::Network
        | crate::sync::types::SyncErrorCategory::TemporaryUnavailable => {
            SyncStatus::RecoverableError("network".to_string())
        }
        _ => SyncStatus::RecoverableError("api_error".to_string()),
    }
}
