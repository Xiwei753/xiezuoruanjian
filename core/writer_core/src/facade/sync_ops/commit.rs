use crate::sync::full_sync_utils::now_epoch_seconds;

/// 本轮级别的阶段失败，需要和 target 结果一起进入持久终态。
#[derive(Debug, Clone)]
pub struct RoundPhaseFailure {
    /// `remote_publish_failed` / `cancelled`。
    pub category: String,
    pub message: String,
    /// 平台展示用的消息 key（可选）。
    pub message_key: Option<String>,
}

impl crate::facade::WriterCore {
    /// 聚合收口 — 新编排入口（Issue #762 评论 5828791004）。
    ///
    /// 每个 target 的 Transfer + Commit 已在 `perform_full_sync` 中逐 target 完成，
    /// 此函数只负责整轮结束后的聚合收口：
    /// - `aggregate_full_sync_result`：聚合总体状态
    /// - generation GC 失败传播
    /// - `cleanup_completed_deleted_targets`：清理 pending deleted
    /// - `FullSyncState` 持久化
    /// - `rebuild_search_index`
    ///
    /// 返回本轮的聚合结果；本地对象提交已在逐 target 阶段完成。
    pub fn finalize_full_sync(
        &self,
        targets: Vec<crate::sync::types::TargetSyncResult>,
        round_phase_failure: Option<RoundPhaseFailure>,
        deletions_confirmed: bool,
    ) -> crate::sync::types::FullSyncResult {
        let mut result = crate::sync::full_sync::aggregate_full_sync_result(targets);

        // 本轮级别的阶段失败（远端发布失败 / 取消）必须在**持久化终态之前**
        // 汇入结果：先 finalize 写成功、再把内存 DTO 改成失败会让 Core 的
        // last_success_time 与 UI 永久不一致（复审 #4）。
        if let Some(failure) = round_phase_failure {
            if matches!(
                result.overall_status,
                crate::sync::SyncStatus::Success
                    | crate::sync::SyncStatus::NoChanges
                    | crate::sync::SyncStatus::LatestWinsApplied
            ) {
                result.overall_status =
                    crate::sync::SyncStatus::RecoverableError(failure.category.clone());
                result.error = Some(failure.message);
                result.error_category = Some(failure.category);
                if failure.message_key.is_some() {
                    result.message_key = failure.message_key;
                }
            }
        }

        // 只有远端提交成功且删除回执都已结算时，才消费 pending 删除记录；
        // 发布失败/回执未结算的删除必须保留到下一轮重试。
        if deletions_confirmed {
            self.cleanup_completed_deleted_targets(&result);
        }

        let report_time = now_epoch_seconds();
        let previous_state = self.load_full_sync_state().unwrap_or(None);
        let new_state = crate::sync::full_sync_state::FullSyncState::from_result_and_previous(
            &result,
            previous_state.as_ref(),
            report_time,
        );
        if let Err(e) = self.save_full_sync_state(&new_state) {
            log::warn!("Failed to persist full sync state: {e}");
        }
        if let Err(e) = self.record_latest_full_sync_report(&result, report_time) {
            log::warn!(
                "Failed to persist latest full sync report to {}: {e}",
                self.app_data_root
                    .join("app-meta/sync/last_report.local.json")
                    .display()
            );
        }

        if matches!(
            result.overall_status,
            crate::sync::SyncStatus::Success | crate::sync::SyncStatus::LatestWinsApplied
        ) {
            if let Err(e) = self.rebuild_search_index(None) {
                log::warn!("Failed to rebuild search index after full sync: {e}");
            }
        }

        result
    }

    /// 删除作品的远端对象全部发布成功后，移除本地 pending 记录。
    ///
    /// 只有 target 状态是成功类才移除；发布失败保留 pending，下一轮重新比较重试。
    fn cleanup_completed_deleted_targets(&self, result: &crate::sync::types::FullSyncResult) {
        for target in &result.targets {
            if target.target_kind != "deleted_project" {
                continue;
            }
            if matches!(
                target.result.status,
                crate::sync::SyncStatus::Success | crate::sync::SyncStatus::LatestWinsApplied
            ) {
                self.remove_pending_deleted_by_prefix(&target.remote_prefix);
            }
        }
    }

    /// 平台端预处理失败写同一份 Core FullSyncState 的窄接口。
    ///
    /// 只负责更新 `<app_data_root>/app-meta/sync/full_state.local.json`（与
    /// `perform_full_sync` 同一份），不新建平台第二份状态、不恢复旧双同步 API。
    /// 覆盖 Android 正文 flush / app data barrier / credentials override 等
    /// Core 根本没进入 full sync 的失败路径。
    ///
    /// - `status`：按失败类型给定（通常 `FatalError`）；
    /// - `failed_target`：传 `"preflight"`（或 `"global"`），不要伪造某个 project id。
    ///
    /// 保留旧 `last_success_time`，保证重启后顶部不会出现旧绿灯。
    pub fn record_full_sync_preflight_failure(
        &self,
        status: crate::sync::SyncStatus,
        failed_target: &str,
    ) -> crate::error::Result<()> {
        self.persist_full_sync_early_failure(status, failed_target);
        Ok(())
    }

    /// 保留 Core 原始错误分类与失败阶段的预检失败落盘入口。
    pub fn record_full_sync_preflight_failure_typed(
        &self,
        status: crate::sync::SyncStatus,
        failed_target: &str,
        error_category: &str,
        phase: &str,
        detail: &str,
    ) -> crate::error::Result<()> {
        self.persist_full_sync_early_failure_typed(
            status,
            failed_target,
            error_category,
            phase,
            detail,
        );
        Ok(())
    }

    /// 正式事务开始：原子写 `Syncing` + 本次 attempt 时间，保留旧 last_success_time。
    /// 写失败只记录警告（同步本身继续，状态持久化是副作用）。
    ///   回退问题：改为 `pub(crate)` 供 API 层在短锁内调用。
    pub(crate) fn persist_full_sync_started(&self) {
        let previous = self.load_full_sync_state().unwrap_or(None);
        let state = crate::sync::full_sync_state::FullSyncState::started(
            previous.as_ref(),
            now_epoch_seconds(),
        );
        if let Err(e) = self.save_full_sync_state(&state) {
            log::warn!("Failed to persist full sync started state: {e}");
        }
    }

    /// target 开始执行前失败：原子写失败状态 + failed_target（"global"/"preflight"），
    /// 保留旧 last_success_time。写失败只记录警告。
    /// `pub(in crate::facade)`：仅供 facade 内部与 `sync_ops_tests` 验证提前失败语义。
    pub(in crate::facade) fn persist_full_sync_early_failure(
        &self,
        status: crate::sync::SyncStatus,
        failed_target: &str,
    ) {
        let report_time = now_epoch_seconds();
        let report_status = status.clone();
        let previous = self.load_full_sync_state().unwrap_or(None);
        let state = crate::sync::full_sync_state::FullSyncState::failed_before_targets(
            previous.as_ref(),
            status,
            report_time,
            failed_target,
        );
        if let Err(e) = self.save_full_sync_state(&state) {
            log::warn!("Failed to persist full sync early failure state: {e}");
        }
        if let Err(e) =
            self.record_full_sync_early_failure_report(&report_status, failed_target, report_time)
        {
            log::warn!("Failed to persist early full sync report: {e}");
        }
    }

    /// 带真实分类/阶段的预检失败持久化。状态机仍使用 Recoverable/Fatal 线格式，
    /// 但报告保留 `provider_init`、TLS/CA、本地 Git 缓存等可诊断原因。
    pub(in crate::facade) fn persist_full_sync_early_failure_typed(
        &self,
        status: crate::sync::SyncStatus,
        failed_target: &str,
        error_category: &str,
        phase: &str,
        detail: &str,
    ) {
        let report_time = now_epoch_seconds();
        let report_status = status.clone();
        let previous = self.load_full_sync_state().unwrap_or(None);
        let state = crate::sync::full_sync_state::FullSyncState::failed_before_targets(
            previous.as_ref(),
            status,
            report_time,
            failed_target,
        );
        if let Err(e) = self.save_full_sync_state(&state) {
            log::warn!("Failed to persist full sync early failure state: {e}");
        }
        if let Err(e) = self.record_full_sync_early_failure_report_typed(
            &report_status,
            failed_target,
            error_category,
            phase,
            detail,
            report_time,
        ) {
            log::warn!("Failed to persist typed early full sync report: {e}");
        }
    }

    /// Issue #729 评论 5765306162 问题6：persist_full_sync_started 之后取消时，
    /// 持久化一个取消终态，避免 full_state.local.json 停在 Syncing 直到下次启动
    /// 被当成"上次同步中断"。
    ///
    /// 用 `SyncStatus::Error("cancelled")` 表示取消（不新增 SyncStatus::Cancelled
    /// 变体，避免补全 260+ 处 match 分支）。复用 failed_before_targets 模式，
    /// failed_target 标 "cancelled"。写失败只记录警告。
    pub(crate) fn persist_full_sync_cancelled(&self) {
        let report_time = now_epoch_seconds();
        let status = crate::sync::SyncStatus::Error("cancelled".to_string());
        let previous = self.load_full_sync_state().unwrap_or(None);
        let state = crate::sync::full_sync_state::FullSyncState::failed_before_targets(
            previous.as_ref(),
            status.clone(),
            report_time,
            "cancelled",
        );
        if let Err(e) = self.save_full_sync_state(&state) {
            log::warn!("Failed to persist full sync cancelled state: {e}");
        }
        if let Err(e) =
            self.record_full_sync_early_failure_report(&status, "cancelled", report_time)
        {
            log::warn!("Failed to persist cancelled full sync report: {e}");
        }
    }
}
