//! 全量同步持久状态（Issue #630 评论 5307423953 Part B）。
//!
//! 写在 `<app_data_root>/app-meta/sync/full_state.local.json`，与 per-target 的
//! `state.local.json` 分层：per-target state 记录每个 target 自己的 manifest/LWW 状态，
//! `FullSyncState` 只记录"这一次全量事务整体是什么结果"。
//!
//! 生命周期（Issue #630 评论 5308040939 Part 1）：
//! - 正式事务开始 → [`FullSyncState::started`]（Syncing + 本次 attempt，保留旧
//!   `last_success_time`；进程中断后重启读到 Syncing 而不是旧绿灯）；
//! - target 开始执行前失败（transport 初始化 / `list_projects` / 平台预处理）→
//!   [`FullSyncState::failed_before_targets`]（失败状态 + `"preflight"`/`"global"`）；
//! - target 全部执行、聚合完成后 → [`FullSyncState::from_result_and_previous`]
//!   （覆盖 Syncing 为终态）。
use super::types::{FullSyncResult, SyncStatus};
use serde::{Deserialize, Serialize};
/// 全量同步持久状态 — 一次全量同步事务的总体结果。
///
/// - `overall_status`：总体状态（Success/NoChanges/LatestWinsApplied/BranchMissingRecovered
///   视为整体成功；Error/PartialConflict/RecoverableError 等为失败）。
/// - `last_attempt_time`：上次全量同步尝试时间（Unix 秒），每次尝试都更新。
/// - `last_success_time`：上次全量同步整体成功时间（Unix 秒），仅当 `overall_status`
///   为整体成功类时才更新；部分失败时保留旧值。
/// - `failed_targets`：本次尝试中失败的 target 标识（`"app"`、`"project:<id>"`、
///   或提前失败时的 `"global"` / `"preflight"`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FullSyncState {
    pub overall_status: SyncStatus,
    pub last_attempt_time: Option<i64>,
    pub last_success_time: Option<i64>,
    pub failed_targets: Vec<String>,
}
impl FullSyncState {
    /// 判定 `overall_status` 是否属于整体成功类（应更新 `last_success_time`）。
    pub fn is_overall_success(status: &SyncStatus) -> bool {
        matches!(
            status,
            SyncStatus::Success
                | SyncStatus::NoChanges
                | SyncStatus::LatestWinsApplied
                | SyncStatus::BranchMissingRecovered
        )
    }
    /// 正式事务开始时的状态（Issue #630 评论 5308040939 Part 1）。
    ///
    /// 同步一开始就原子写入 `Syncing` + 本次 attempt 时间；进程中断/被杀后
    /// 磁盘上至少留下 `Syncing` 而不是上一次的绿灯。保留旧 `last_success_time`：
    /// 部分失败/中断时依然知道上次真正整体成功的时刻。
    pub fn started(previous: Option<&Self>, now: i64) -> Self {
        Self {
            overall_status: SyncStatus::Syncing,
            last_attempt_time: Some(now),
            last_success_time: previous.and_then(|s| s.last_success_time),
            failed_targets: Vec::new(),
        }
    }
    /// target 开始执行前就失败的状态（Issue #630 评论 5308040939 Part 1）。
    ///
    /// 覆盖 transport 初始化失败、`list_projects` 失败和 Android 预处理
    /// （正文 flush / app data barrier / credentials override）失败。
    /// `failed_target` 允许 `"global"` / `"preflight"`，不要伪造某个 project id。
    pub fn failed_before_targets(
        previous: Option<&Self>,
        status: SyncStatus,
        now: i64,
        failed_target: &str,
    ) -> Self {
        Self {
            overall_status: status,
            last_attempt_time: Some(now),
            last_success_time: previous.and_then(|s| s.last_success_time),
            failed_targets: vec![failed_target.to_string()],
        }
    }
    /// 从本次 `FullSyncResult` + 当前时间构造下一份 `FullSyncState`，合并旧 state
    /// 的 `last_success_time`（部分失败时保留旧成功时间）。
    pub fn from_result_and_previous(
        result: &FullSyncResult,
        previous: Option<&FullSyncState>,
        now_epoch_seconds: i64,
    ) -> Self {
        let failed_targets: Vec<String> = result
            .targets
            .iter()
            .filter(|t| {
                matches!(
                    t.result.status,
                    SyncStatus::FatalError(_)
                        | SyncStatus::Error(_)
                        | SyncStatus::RecoverableError(_)
                        | SyncStatus::DirtyRepoBlocked
                        | SyncStatus::Conflict
                        | SyncStatus::PartialConflict
                )
            })
            .map(|t| {
                if t.target_kind == "app" {
                    "app".to_string()
                } else {
                    format!("project:{}", t.project_id.as_deref().unwrap_or(""))
                }
            })
            .collect();
        let last_success_time = if Self::is_overall_success(&result.overall_status) {
            Some(now_epoch_seconds)
        } else {
            previous.and_then(|p| p.last_success_time)
        };
        Self {
            overall_status: result.overall_status.clone(),
            last_attempt_time: Some(now_epoch_seconds),
            last_success_time,
            failed_targets,
        }
    }
}

#[cfg(test)]
mod full_sync_state_tests;
