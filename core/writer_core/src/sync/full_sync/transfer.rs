//! Transfer phase — 只把远端对象变化搬进 staging，不做业务裁决。
//!
//! 每个 target 的 Transfer 都不持锁、不写 live：
//! - 普通作品/App 组：下载与本机基线、本机内容都不同的远端 blob；
//! - 用户已删除作品的远端删除：只读远端索引对象，不下载内容，提交阶段把
//!   所有这些对象发布为删除。

use crate::sync::cancellation_token::SyncCancellationToken;
use crate::sync::provider::SyncProvider;
use crate::sync::types::{SyncResult, TargetSyncResult};

use super::transfer_helpers::*;
use super::FullSyncPlan;

/// 执行单个 target 的 Transfer。
///
/// 返回 `None` 表示 target 未执行（取消、索引越界或同步关闭）；返回
/// `Some(..)` 表示 Transfer 已完成，业务裁决留给 Commit。
#[allow(clippy::type_complexity)]
pub fn run_single_target_transfer(
    provider: &dyn SyncProvider,
    plan: &FullSyncPlan,
    target_index: usize,
    cancellation_token: Option<&SyncCancellationToken>,
    _diagnostic_operation_id: Option<&str>,
) -> Option<TargetSyncResult> {
    if !plan.sync_policy.enabled {
        log::debug!("[sync] run_single_target_transfer: sync disabled — returning no-op");
        return None;
    }
    if target_index >= plan.targets.len() {
        log::warn!(
            "[sync] run_single_target_transfer: target_index {target_index} out of bounds (len={})",
            plan.targets.len()
        );
        return None;
    }
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] run_single_target_transfer: cancellation requested — skipping target {target_index}"
            );
            return None;
        }
    }

    let planned = &plan.targets[target_index];
    let staging_root = planned.staging_root.as_deref()?;

    // 删除作品与其它 target 走同一条对象传输：真正需要的远端冲突内容
    // 也会下载到 staging，供用户预览与选择。
    // 删除作品的回收区基线已在 Prepare 阶段写成 live 的初始索引，之后所有
    // 下载判断统一读 live 权威状态，不再双根切换。
    let result = transfer_object_scope(
        provider,
        &planned.target,
        &planned.local_root,
        &planned.local_root,
        Some(staging_root),
        cancellation_token,
    );

    let target_sync_result = TargetSyncResult {
        target_kind: planned.target_kind.as_target_kind_str().to_string(),
        project_id: planned.project_id.clone(),
        remote_prefix: planned.target.remote_prefix.clone(),
        group_id: planned.target.app_group.as_ref().map(|group| group.id()),
        failure_path: app_group_failure_path(&planned.target, &result),
        failure_phase: app_group_failure_phase(&planned.target, &result),
        result,
    };

    Some(target_sync_result)
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
    ["catalog_read", "merge", "tree_publish", "catalog_publish"]
        .iter()
        .find(|phase| error.starts_with(&format!("{phase}:")))
        .map_or_else(
            || Some("transfer".to_string()),
            |phase| Some((*phase).to_string()),
        )
}
