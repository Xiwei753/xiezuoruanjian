//! Transfer phase — execute sync for each planned target and collect results.
//!
//! Contains `run_transfer`; all helpers live in `transfer_helpers.rs`.

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
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting
)]
pub fn run_transfer(provider: &dyn SyncProvider, plan: &FullSyncPlan) -> FullSyncTransferResult {
    use crate::sync::types::PlannedTargetKind;

    if !plan.sync_policy.enabled {
        log::debug!("[sync] run_transfer: sync disabled — returning no-op");
        return FullSyncTransferResult {
            targets: Vec::new(),
            generation_gc_result: None,
        };
    }

    let mut catalog_snapshot = plan.remote_catalog_snapshot.clone();

    let mut targets = Vec::with_capacity(plan.targets.len());
    for planned in &plan.targets {
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
                );
                (r, None, None)
            }
            PlannedTargetKind::LiveProject => {
                transfer_live_project(provider, planned, &mut catalog_snapshot, plan)
            }
            PlannedTargetKind::DeleteLocalProject => {
                transfer_delete_local_project(provider, planned, plan)
            }
            PlannedTargetKind::DeleteRemoteProject => {
                transfer_delete_remote_project(provider, planned, &mut catalog_snapshot)
            }
            PlannedTargetKind::RestoreProject => transfer_restore_project(provider, planned, plan),
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
                transfer_remote_cleanup_project(provider, planned)
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
    }

    // generation GC — 清理未引用 generation。
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut generation_gc_result: Option<Result<(), String>> = None;
    for planned in &plan.targets {
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
