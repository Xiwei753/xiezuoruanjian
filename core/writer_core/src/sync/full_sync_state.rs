//! 全量同步持久状态。
//!
//! 写在 `<app_data_root>/app-meta/sync/full_state.local.json`，与 per-target 的
//! `state.local.json` 分层：per-target state 记录每个 target 自己的 manifest/LWW 状态，
//! `FullSyncState` 只记录"这一次全量事务整体是什么结果"。
//!
//! 生命周期：
//! - 正式事务开始 → [`FullSyncState::started`]（Syncing + 本次 attempt，保留旧
//!   `last_success_time`；进程中断后重启读到 Syncing 而不是旧绿灯）；
//! - target 开始执行前失败（transport 初始化 / `list_projects` / 平台预处理）→
//!   [`FullSyncState::failed_before_targets`]（失败状态 + `"preflight"`/`"global"`）；
//! - target 全部执行、聚合完成后 → [`FullSyncState::from_result_and_previous`]
//!   （覆盖 Syncing 为终态）。
//!
//! 冷启动恢复：新 Core/WriterAppService 实例
//! 启动时调用 [`FullSyncState::recover_interrupted`] 一次，把磁盘上残留的 `Syncing`
//! 原子改成 `RecoverableError("previous_full_sync_interrupted")`，避免顶部永久假黄灯。
//! 该动作只在启动时执行一次，不塞进 `load_full_sync_state()`。
use super::types::{FullSyncResult, SyncStatus};
use serde::{Deserialize, Serialize};

/// Version checkpoint for the last successful, fully converged sync.
///
/// Repository identity is stored as a digest so private remote URLs are not copied
/// into the checkpoint file. The revision and local fingerprint are content
/// versions, not timestamps.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FullSyncCheckpoint {
    pub repository_identity: String,
    pub remote_revision: String,
    pub local_object_fingerprint: String,
    /// Independent versions let a stable remote branch process only locally changed
    /// App groups or projects on the next run.
    #[serde(default)]
    pub target_fingerprints: std::collections::BTreeMap<String, String>,
}

impl FullSyncCheckpoint {
    const FILE_NAME: &'static str = "checkpoint.local.json";

    pub fn load(app_data_root: &std::path::Path) -> crate::error::Result<Option<Self>> {
        let path = app_data_root.join("app-meta/sync").join(Self::FILE_NAME);
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(crate::Error::Io(error)),
        };
        match serde_json::from_str(&content) {
            Ok(checkpoint) => Ok(Some(checkpoint)),
            Err(error) => {
                log::warn!("sync checkpoint is unreadable; forcing a full sync: {error}");
                Ok(None)
            }
        }
    }

    pub fn save(&self, app_data_root: &std::path::Path) -> crate::error::Result<()> {
        let path = app_data_root.join("app-meta/sync").join(Self::FILE_NAME);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|error| crate::Error::Io(std::io::Error::other(error.to_string())))?;
        crate::storage::atomic_write_string(&path, &content)
    }
}

/// 最近一次全量同步报告，单独保存在 `last_report.local.json`。
///
/// `FullSyncState` 保持顶部状态的轻量契约；这份报告保存每个独立数据组的
/// 完成计数和失败根因，供 UI 与诊断附件跨重启读取。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct FullSyncReport {
    pub attempted_at: i64,
    pub completed_groups: u32,
    pub failed_groups: u32,
    pub pending_groups: u32,
    pub failures: Vec<FullSyncReportFailure>,
}

/// 一个失败数据组的可诊断信息。`path` 使用相对路径或远端组路径。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FullSyncReportFailure {
    pub target_kind: String,
    pub group_id: String,
    pub path: String,
    pub phase: String,
    pub error_category: String,
    pub message_key: String,
    pub detail: String,
    pub occurred_at: i64,
}

impl FullSyncReport {
    /// 从一轮已聚合的同步结果生成持久报告。
    pub fn from_result(
        result: &FullSyncResult,
        now_epoch_seconds: i64,
        app_data_root: &std::path::Path,
        projects_root: &std::path::Path,
    ) -> Self {
        use super::types::SyncStatus;

        let mut report = Self {
            attempted_at: now_epoch_seconds,
            ..Self::default()
        };

        for target in &result.targets {
            if matches!(target.result.status, SyncStatus::Syncing) {
                report.pending_groups = report.pending_groups.saturating_add(1);
                continue;
            }
            if matches!(
                target.result.status,
                SyncStatus::Success | SyncStatus::NoChanges | SyncStatus::LatestWinsApplied
            ) {
                report.completed_groups = report.completed_groups.saturating_add(1);
                continue;
            }

            report.failed_groups = report.failed_groups.saturating_add(1);
            let raw_detail = target
                .result
                .error
                .as_deref()
                .or_else(|| status_detail(&target.result.status))
                .unwrap_or("sync group did not complete");
            let detail = sanitize_report_detail(raw_detail, app_data_root, projects_root);
            let error_category = target
                .result
                .error_category
                .clone()
                .or_else(|| inferred_error_category(&target.result.status, raw_detail))
                .unwrap_or_else(|| "other".to_string());
            let message_key = target
                .result
                .message_key
                .clone()
                .unwrap_or_else(|| message_key_for_category(&error_category));

            report.failures.push(FullSyncReportFailure {
                target_kind: target.target_kind.clone(),
                group_id: target
                    .group_id
                    .clone()
                    .or_else(|| target.project_id.clone())
                    .unwrap_or_else(|| target.remote_prefix.clone()),
                path: target
                    .failure_path
                    .clone()
                    .unwrap_or_else(|| target.remote_prefix.clone()),
                phase: target
                    .failure_phase
                    .clone()
                    .unwrap_or_else(|| "transfer".to_string()),
                error_category,
                message_key,
                detail,
                occurred_at: now_epoch_seconds,
            });
        }

        report
    }

    /// 为尚未执行 target 的全局或预处理失败生成一份持久报告。
    pub fn early_failure(
        status: &SyncStatus,
        failed_target: &str,
        now_epoch_seconds: i64,
        app_data_root: &std::path::Path,
        projects_root: &std::path::Path,
    ) -> Self {
        let raw_detail = status_detail(status).unwrap_or("sync failed before data groups ran");
        let error_category =
            inferred_error_category(status, raw_detail).unwrap_or_else(|| match status {
                SyncStatus::FatalError(_) => "auth_failed".to_string(),
                SyncStatus::RecoverableError(_) => "network".to_string(),
                _ => "other".to_string(),
            });
        Self::early_failure_typed(
            status,
            failed_target,
            &error_category,
            "preflight",
            "",
            now_epoch_seconds,
            app_data_root,
            projects_root,
        )
    }

    /// 使用调用方已有的真实错误分类、失败阶段与脱敏前的 detail 生成早期报告，
    /// 避免仅凭 Recoverable/Fatal 二值误判为 network/auth_failed，也避免报告
    /// 只剩下 `preflight` 而丢失真实的 connect_auth/list 子阶段。
    pub fn early_failure_typed(
        status: &SyncStatus,
        failed_target: &str,
        error_category: &str,
        phase: &str,
        detail: &str,
        now_epoch_seconds: i64,
        app_data_root: &std::path::Path,
        projects_root: &std::path::Path,
    ) -> Self {
        let raw_detail = if detail.is_empty() {
            status_detail(status).unwrap_or("sync failed before data groups ran")
        } else {
            detail
        };
        Self {
            attempted_at: now_epoch_seconds,
            completed_groups: 0,
            failed_groups: 1,
            pending_groups: 0,
            failures: vec![FullSyncReportFailure {
                target_kind: "global".to_string(),
                group_id: failed_target.to_string(),
                path: failed_target.to_string(),
                phase: phase.to_string(),
                message_key: message_key_for_category(error_category),
                error_category: error_category.to_string(),
                detail: sanitize_report_detail(raw_detail, app_data_root, projects_root),
                occurred_at: now_epoch_seconds,
            }],
        }
    }
}

fn status_detail(status: &SyncStatus) -> Option<&str> {
    match status {
        SyncStatus::RecoverableError(s) | SyncStatus::FatalError(s) | SyncStatus::Error(s) => {
            Some(s)
        }
        _ => None,
    }
}

fn inferred_error_category(status: &SyncStatus, detail: &str) -> Option<String> {
    match status {
        SyncStatus::Conflict | SyncStatus::PartialConflict => Some("conflict".to_string()),
        _ if detail.contains("local_missing_without_tombstone") => {
            Some("local_missing".to_string())
        }
        _ => None,
    }
}

fn message_key_for_category(category: &str) -> String {
    match category {
        "conflict" => "sync.result.conflict_summary".to_string(),
        "network" | "temporary_unavailable" => "sync.result.network_failed".to_string(),
        "permission_denied" => "sync.result.token_permission_denied".to_string(),
        "local_missing" => "sync.result.local_missing_without_tombstone".to_string(),
        "auth_failed" => "sync.result.auth_failed".to_string(),
        _ => "sync.result.generic_error".to_string(),
    }
}

fn sanitize_report_detail(
    detail: &str,
    app_data_root: &std::path::Path,
    projects_root: &std::path::Path,
) -> String {
    let redacted = crate::sync::url::redact_secrets_from_message(detail, None, None);
    let redacted = writer_diagnostics::redact::redact(&redacted);
    let redacted = redacted.replace(
        &app_data_root.to_string_lossy().to_string(),
        "<app_data_root>",
    );
    redacted.replace(
        &projects_root.to_string_lossy().to_string(),
        "<projects_root>",
    )
}
/// 全量同步持久状态 — 一次全量同步事务的总体结果。
///
/// - `overall_status`：总体状态（Success/NoChanges/LatestWinsApplied
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
            SyncStatus::Success | SyncStatus::NoChanges | SyncStatus::LatestWinsApplied
        )
    }
    /// 正式事务开始时的状态。
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
    /// target 开始执行前就失败的状态。
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

    /// 冷启动恢复中断的 `Syncing` 状态。
    ///
    /// 仅当旧状态是 `Syncing` 时生成一份中断终态：
    /// - `overall_status = RecoverableError("previous_full_sync_interrupted")`
    /// - `failed_targets = ["global"]`
    /// - `last_attempt_time` 保留旧 attempt，不伪造一次新尝试
    /// - `last_success_time` 原样保留
    ///
    /// 旧状态不是 `Syncing`（包括 `None`）时返回 `None`，调用方不应落盘。
    /// 该方法只描述"应该恢复成什么"，不自己读写磁盘；落盘由 facade 层
    /// `recover_interrupted_full_sync_state` 原子完成，且只在新 Core 实例启动时
    /// 调用一次，不能塞进 `load_full_sync_state()`。
    pub fn recover_interrupted(previous: Option<&Self>) -> Option<Self> {
        let prev = previous?;
        if !matches!(prev.overall_status, SyncStatus::Syncing) {
            return None;
        }
        Some(Self {
            overall_status: SyncStatus::RecoverableError(
                "previous_full_sync_interrupted".to_string(),
            ),
            last_attempt_time: prev.last_attempt_time,
            last_success_time: prev.last_success_time,
            failed_targets: vec!["global".to_string()],
        })
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
