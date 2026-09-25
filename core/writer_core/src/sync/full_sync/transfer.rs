//! Transfer phase — execute sync for each planned target and collect results.
//!
//! Contains `run_transfer`; all helpers live in `transfer_helpers.rs`.

use crate::sync::cancellation_token::{SyncCancellationToken, SyncProgressSink};
use crate::sync::provider::SyncProvider;
use crate::sync::types::{SyncResult, TargetSyncResult};

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
/// `progress`：可选的进度 sink（Issue #763）。在 target 开始/结束时写入当前
/// target 的 remote_prefix / project_id / phase / finished / total，供诊断包导出时
/// 读取实时进度。`None` 时不产生任何进度更新。
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting
)]
pub fn run_transfer(
    provider: &dyn SyncProvider,
    plan: &FullSyncPlan,
    cancellation_token: Option<&SyncCancellationToken>,
    progress: Option<&SyncProgressSink>,
) -> FullSyncTransferResult {
    use crate::sync::types::PlannedTargetKind;

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
    for (idx, planned) in plan.targets.iter().enumerate() {
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

        // Issue #763：target 开始时写入进度 sink。
        if let Some(sink) = progress {
            let finished = u32::try_from(idx).unwrap_or(u32::MAX);
            sink.update_target_start(
                &planned.target.remote_prefix,
                planned.project_id.as_deref(),
                planned.target_kind.as_target_kind_str(),
                finished,
                total_targets,
            );
        }

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
                &mut catalog_snapshot,
                plan,
                cancellation_token,
            ),
            PlannedTargetKind::DeleteLocalProject => {
                transfer_delete_local_project(provider, planned, plan, cancellation_token)
            }
            PlannedTargetKind::DeleteRemoteProject => transfer_delete_remote_project(
                provider,
                planned,
                &mut catalog_snapshot,
                cancellation_token,
            ),
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
                    Some(crate::sync::types::DeletedTargetResolution::Retry),
                    None,
                )
            }
            PlannedTargetKind::RemoteCleanupProject => {
                transfer_remote_cleanup_project(provider, planned, cancellation_token)
            }
        };

        targets.push(TargetSyncResult {
            target_kind: planned.target_kind.as_target_kind_str().to_string(),
            project_id: planned.project_id.clone(),
            remote_prefix: planned.target.remote_prefix.clone(),
            result,
            deleted_resolution: resolution,
            local_lifecycle_action: action.unwrap_or_default(),
        });

        // Issue #763：target 结束时写入进度 sink（phase 清空表示该 target 已完成）。
        if let Some(sink) = progress {
            let finished = u32::try_from(idx + 1).unwrap_or(u32::MAX);
            sink.update_target_finish(
                &planned.target.remote_prefix,
                planned.project_id.as_deref(),
                finished,
                total_targets,
            );
        }
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
