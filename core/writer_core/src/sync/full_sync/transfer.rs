//! Transfer phase — execute sync for each planned target and collect results.
//!
//! Contains `run_transfer`（兼容入口，给旧测试用）和 `run_single_target_transfer`
//! （新编排入口，每个 target 独立执行，不调用 `persist_unresolved_conflicts_early`）。
//! 所有 helpers live in `transfer_helpers.rs`.

use crate::sync::cancellation_token::{SyncCancellationToken, SyncProgressSink};
use crate::sync::provider::SyncProvider;
use crate::sync::types::{
    DeletedTargetResolution, LocalLifecycleCommitAction, SyncResult, TargetSyncResult,
};

use super::transfer_cleanup::transfer_remote_cleanup_project;
use super::transfer_helpers::*;
use super::{FullSyncPlan, FullSyncTransferResult};

/// 执行 Transfer 阶段：对 plan 中每个 target 调对应同步函数，收集结果。
///
///   CAS — 执行破坏性动作前重新读远端 catalog 确认 winner。
///
/// 本函数是纯函数 — 不接触 `WriterCore`、不持锁、不写 `FullSyncState`。
///
/// `cancellation_token`：平台层持有的取消令牌。在每次 target 迭代开头检查
/// `is_cancelled()`，如果已取消则 break 并返回已收集的结果（已完成的 targets +
/// 剩余的标记为 cancelled/skipped）。
///
/// `progress_sink`：可选的进度 sink（Issue #763）。在 target 开始/结束时写入当前
/// target 的 remote_prefix / project_id / phase / finished / total，供诊断包导出时
/// 读取实时进度。`None` 时不产生任何进度更新。
///
/// `target_progress`：可选的 #762 callback。每个 target Transfer 完成后回调一次，
/// 平台层据此实时刷新冲突状态。生产路径（`perform_full_sync_with_provider`）的
/// callback 在 Commit 之后调，不在此处调；本参数仅供兼容入口 `run_transfer` 的
/// 旧测试用。
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting
)]
pub fn run_transfer(
    provider: &dyn SyncProvider,
    plan: &FullSyncPlan,
    cancellation_token: Option<&SyncCancellationToken>,
    progress_sink: Option<&SyncProgressSink>,
    target_progress: Option<&super::SyncProgressCallback>,
) -> FullSyncTransferResult {
    if !plan.sync_policy.enabled {
        log::debug!("[sync] run_transfer: sync disabled — returning no-op");
        return FullSyncTransferResult {
            targets: Vec::new(),
            generation_gc_result: None,
        };
    }

    let mut catalog_snapshot = plan.remote_catalog_snapshot.clone();

    let total_targets = u32::try_from(plan.targets.len()).unwrap_or(u32::MAX);
    let mut targets = Vec::with_capacity(plan.targets.len());
    for target_index in 0..plan.targets.len() {
        // Issue #729：每次 target 迭代开头检查取消令牌。
        // 已取消则 break，已完成的 targets 保留，剩余的不执行。
        if let Some(token) = cancellation_token {
            if token.is_cancelled() {
                log::info!(
                    "[sync] run_transfer: cancellation requested — breaking after {} targets",
                    targets.len()
                );
                break;
            }
        }

        let planned = &plan.targets[target_index];

        // Issue #763：target 开始时写入进度 sink。
        if let Some(sink) = progress_sink {
            let finished = u32::try_from(target_index).unwrap_or(u32::MAX);
            sink.update_target_start(
                &planned.target.remote_prefix,
                planned.project_id.as_deref(),
                "transfer",
                finished,
                total_targets,
            );
        }

        // 与生产编排共用同一个单 target 入口，不复制 dispatch。
        let Some((target_result, _resolution, _action)) = run_single_target_transfer(
            provider,
            plan,
            target_index,
            &mut catalog_snapshot,
            cancellation_token,
        ) else {
            // 取消（或索引越界）→ 停止本轮，已完成的 targets 保留。
            break;
        };

        // Issue #763：target 结束时写入进度 sink（phase 清空表示该 target 已完成）。
        if let Some(sink) = progress_sink {
            let finished = u32::try_from(target_index + 1).unwrap_or(u32::MAX);
            sink.update_target_finish(
                &planned.target.remote_prefix,
                planned.project_id.as_deref(),
                finished,
                total_targets,
            );
        }

        // 每个 target 完成后立即回调 progress，平台层据此实时刷新冲突状态，
        // 不必等最终 FullSyncResult。
        // 注意：本回调是"Transfer 后"而非"Commit 后"——`run_transfer` 是兼容入口
        // 不做 Commit。生产路径的 callback 在 `perform_full_sync_with_provider` 的
        // Commit 之后调，不经过此处。
        if let Some(cb) = target_progress {
            let progress_status =
                crate::api::types::sync_status_to_wire(&target_result.result.status);
            let progress_conflict_count =
                u32::try_from(target_result.result.conflicts.len()).unwrap_or(u32::MAX);
            cb(super::SyncTargetProgress {
                project_id: target_result.project_id.clone(),
                target_kind: target_result.target_kind.clone(),
                status: progress_status,
                conflict_count: progress_conflict_count,
            });
        }

        targets.push(target_result);
    }

    // generation GC — 清理未引用 generation。
    // Issue #729：generation GC 循环前检查取消令牌，取消则跳过整个 GC。
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut generation_gc_result: Option<Result<(), String>> = None;
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!("[sync] run_transfer: cancellation requested — skipping generation GC");
            return FullSyncTransferResult {
                targets,
                generation_gc_result: None,
            };
        }
    }
    // Issue #763 评论 5831610228：generation GC 阶段不再在循环前用 set_phase 写死 phase，
    // 改为在循环内每处理一个 planned target 时用 set_target_phase 更新 remote_prefix /
    // project_id / phase，避免残留最后一个 Transfer target 导致诊断包指错作品。
    for planned in &plan.targets {
        // Issue #729：generation GC 循环内每个 target 前检查取消令牌。
        if let Some(token) = cancellation_token {
            if token.is_cancelled() {
                log::info!(
                    "[sync] run_transfer: cancellation requested during generation GC — breaking"
                );
                break;
            }
        }
        // Issue #763 评论 5831610228：generation GC 循环里每处理一个 planned，
        // 都更新 sink 的 target 信息，避免残留最后一个 Transfer target 指错作品。
        if let Some(sink) = progress_sink {
            sink.set_target_phase(
                &planned.target.remote_prefix,
                planned.project_id.as_deref(),
                "generation_gc",
            );
        }
        if planned.target.remote_prefix.starts_with("projects/") {
            let active_generation = crate::sync::target_lifecycle::find_record(
                &catalog_snapshot.catalog,
                &planned.target.remote_prefix,
            )
            .and_then(|r| r.active_generation.as_deref());
            match crate::sync::generation_gc::run_generation_gc(
                provider,
                &planned.target.remote_prefix,
                active_generation,
                now_ms,
                crate::sync::generation_gc::GENERATION_RETENTION_MS,
                cancellation_token,
            ) {
                Ok(()) => {}
                Err(e) => {
                    log::warn!(
                        "[sync] run_transfer: generation GC failed for {}: {e}",
                        planned.target.remote_prefix
                    );
                    generation_gc_result = Some(Err(e.to_string()));
                }
            }
        }
    }

    FullSyncTransferResult {
        targets,
        generation_gc_result,
    }
}

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
        PlannedTargetKind::App => {
            let sync_root = planned
                .staging_root
                .as_deref()
                .unwrap_or(&planned.local_root);
            let r = run_single_target(
                provider,
                sync_root,
                &plan.sync_policy,
                &planned.target,
                plan.force_sync,
                cancellation_token,
            );
            (r, None, None)
        }
        PlannedTargetKind::LiveProject => transfer_live_project(
            provider,
            planned,
            catalog_snapshot,
            plan,
            cancellation_token,
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
        result,
        deleted_resolution: resolution,
        local_lifecycle_action: action.clone().unwrap_or_default(),
    };

    Some((target_sync_result, resolution, action))
}
