//! Transfer phase — execute sync for each planned target and collect results.
//!
//! `run_single_target_transfer`：每个 target 独立执行（Transfer 只下载，不持锁、
//! 不写 live）。
//! 所有 helpers live in `transfer_helpers.rs`.

use crate::sync::cancellation_token::{SyncCancellationToken, SyncProgressSink};
use crate::sync::provider::SyncProvider;
use crate::sync::types::{
    DeletedTargetResolution, LocalLifecycleCommitAction, SyncResult, TargetSyncResult,
};

use super::transfer_cleanup::transfer_remote_cleanup_project;
use super::transfer_helpers::*;
use super::FullSyncPlan;

/// 执行单个 target 的 Transfer — 新编排入口。
///
/// 与 `run_transfer` 的区别：
/// - **不调用 `persist_unresolved_conflicts_early`**：新编排中 Commit 在 progress 之前完成，
///   冲突状态由 Commit 阶段写入 live，不需要提前落盘。
/// - **不调用 progress 回调**：progress 由调用方在 Commit 之后自行发送。
/// - **不执行 generation GC**：generation GC 在整轮结束后统一执行。
/// - **接收 `catalog_snapshot: &mut`**：调用方维护 catalog snapshot 跨 target 传递。
///
/// 返回 `None` 表示 target 未执行（取消或索引越界）；
/// 返回 `Some((target_sync_result, resolution, action))` 表示已执行完毕。
///
/// `TargetSyncResult` 中已包含 `deleted_resolution` 和 `local_lifecycle_action` 字段，
/// 同时作为独立元组元素返回是为了方便调用方在 Commit 阶段直接使用，无需再从结构体提取。
pub fn run_single_target_transfer(
    provider: &dyn SyncProvider,
    plan: &FullSyncPlan,
    target_index: usize,
    catalog_snapshot: &mut crate::sync::types::RemoteTargetCatalogSnapshot,
    cancellation_token: Option<&SyncCancellationToken>,
    completed_targets: &[TargetSyncResult],
    diagnostic_operation_id: Option<&str>,
) -> Option<(
    TargetSyncResult,
    Option<DeletedTargetResolution>,
    Option<LocalLifecycleCommitAction>,
)> {
    use crate::sync::types::PlannedTargetKind;

    if !plan.sync_policy.enabled {
        log::debug!("[sync] run_single_target_transfer: sync disabled — returning no-op");
        return None;
    }

    if target_index >= plan.targets.len() {
        log::warn!(
            "[sync] run_single_target_transfer: target_index {} out of bounds (len={})",
            target_index,
            plan.targets.len()
        );
        return None;
    }

    // Issue #729：target 执行前检查取消令牌。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] run_single_target_transfer: cancellation requested — skipping target {}",
                target_index
            );
            return None;
        }
    }

    let planned = &plan.targets[target_index];

    let (result, resolution, action) = match planned.target_kind {
        // 普通 App 数据组与作品都走同一条对象增量传输：远端固定 HEAD 的对象索引
        // 里与本机基线/内容不同的对象才下载，业务裁决统一留给提交阶段。
        PlannedTargetKind::App | PlannedTargetKind::LiveProject => (
            transfer_object_scope(
                provider,
                &planned.target,
                &planned.local_root,
                planned.staging_root.as_deref(),
                cancellation_token,
            ),
            None,
            None,
        ),
        PlannedTargetKind::DeleteLocalProject => {
            transfer_delete_local_project(provider, planned, plan, cancellation_token)
        }
        PlannedTargetKind::DeleteRemoteProject => {
            transfer_delete_remote_project(provider, planned, catalog_snapshot, cancellation_token)
        }
        PlannedTargetKind::RestoreProject => {
            transfer_restore_project(provider, planned, plan, cancellation_token)
        }
        PlannedTargetKind::Retry => {
            let msg = "target lifecycle decision retry".to_string();
            (
                SyncResult::error(
                    crate::sync::SyncStatus::RecoverableError(msg.clone()),
                    msg,
                    None,
                ),
                Some(DeletedTargetResolution::Retry),
                None,
            )
        }
        PlannedTargetKind::RemoteCleanupProject => {
            transfer_remote_cleanup_project(provider, planned, cancellation_token)
        }
    };

    let target_sync_result = TargetSyncResult {
        target_kind: planned.target_kind.as_target_kind_str().to_string(),
        project_id: planned.project_id.clone(),
        remote_prefix: planned.target.remote_prefix.clone(),
        group_id: planned.target.app_group.as_ref().map(|group| group.id()),
        failure_path: app_group_failure_path(&planned.target, &result),
        failure_phase: app_group_failure_phase(&planned.target, &result),
        result,
        deleted_resolution: resolution,
        local_lifecycle_action: action.clone().unwrap_or_default(),
    };

    Some((target_sync_result, resolution, action))
}

/// Prepare 阶段已失败的 target 结果（#955：本机统计事件库迁移失败）。
///
/// 调用方跳过该 target 的 Transfer / Commit，但仍继续同步其它独立组；
/// 失败沿用既有 `failure_phase` / `failure_path` 反馈，不新建同步协议。
pub(crate) fn prepare_failed_target_result(
    plan: &FullSyncPlan,
    target_index: usize,
    error: &str,
) -> TargetSyncResult {
    let planned = &plan.targets[target_index];
    let failure_path = match planned.target.app_group.as_ref() {
        Some(crate::sync::types::AppDataGroup::StatsDevice(device_id)) => {
            Some(format!("stats/events/{device_id}"))
        }
        _ => None,
    };
    TargetSyncResult {
        target_kind: planned.target_kind.as_target_kind_str().to_string(),
        project_id: planned.project_id.clone(),
        remote_prefix: planned.target.remote_prefix.clone(),
        group_id: planned.target.app_group.as_ref().map(|group| group.id()),
        failure_path,
        failure_phase: Some("migration".to_string()),
        result: SyncResult::error(
            crate::sync::SyncStatus::RecoverableError(error.to_string()),
            error.to_string(),
            None,
        ),
        deleted_resolution: None,
        local_lifecycle_action: Default::default(),
    }
}

fn app_group_failure_path(
    target: &crate::sync::types::SyncTarget,
    result: &SyncResult,
) -> Option<String> {
    target.app_group.as_ref()?;
    let error = result.error.as_deref()?;
    if error.contains("local_missing_without_tombstone") {
        if let Some((_, encoded_path)) = error.split_once("failure_path_json=") {
            let mut deserializer = serde_json::Deserializer::from_str(encoded_path);
            if let Ok(path) = <String as serde::Deserialize>::deserialize(&mut deserializer) {
                if !path.is_empty() {
                    return Some(path);
                }
            }
        }
        let paths = error.split_once("paths=[")?.1.split_once(']')?.0;
        return paths
            .split(',')
            .next()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_string);
    }

    // Provider errors include the exact remote object path in forms such as
    // `not found: path=...` and `precondition failed: path=..., reason=...`.
    // Preserve that evidence when present; otherwise identify the group prefix.
    let path = error
        .split_once("path=")
        .map(|(_, provider_path)| {
            provider_path
                .split(|character: char| {
                    character == ',' || character == ')' || character.is_whitespace()
                })
                .next()
                .unwrap_or_default()
        })
        .unwrap_or_default();
    Some(if path.is_empty() {
        target.remote_prefix.clone()
    } else {
        path.to_string()
    })
}

fn app_group_failure_phase(
    target: &crate::sync::types::SyncTarget,
    result: &SyncResult,
) -> Option<String> {
    target.app_group.as_ref()?;
    let error = result.error.as_deref()?;
    if error.contains("local_missing_without_tombstone") {
        return Some("merge".to_string());
    }
    [
        "dependency_check",
        "catalog_read",
        "merge",
        "tree_publish",
        "catalog_publish",
    ]
    .iter()
    .find(|phase| error.starts_with(&format!("{phase}:")))
    .map_or_else(
        || Some("transfer".to_string()),
        |phase| Some((*phase).to_string()),
    )
}
