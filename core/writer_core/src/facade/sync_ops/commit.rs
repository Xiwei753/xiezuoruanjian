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
    /// - `apply_local_lifecycle_deletes`：处理 DeleteProject action
    /// - `aggregate_full_sync_result`：聚合总体状态
    /// - generation GC 失败传播
    /// - `cleanup_completed_deleted_targets`：清理 pending deleted
    /// - `FullSyncState` 持久化
    /// - `rebuild_search_index`
    ///
    /// 返回 `(FullSyncResult, committed_paths, lifecycle_receipts)`。
    /// `committed_paths` 只包含 lifecycle 删除产生的 paths（staging commit 的 paths
    /// 已在 `perform_full_sync` 中逐 target 收集）。
    pub fn finalize_full_sync(
        &self,
        mut targets: Vec<crate::sync::types::TargetSyncResult>,
        round_phase_failure: Option<RoundPhaseFailure>,
    ) -> (
        crate::sync::types::FullSyncResult,
        Vec<std::path::PathBuf>,
        Vec<crate::sync::types::LocalLifecycleCommitReceipt>,
    ) {
        // 处理 local_lifecycle_action（DeleteProject）。
        let (lifecycle_committed_paths, lifecycle_receipts) =
            self.apply_local_lifecycle_deletes(&mut targets);

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

        // deleted target 远端清理成功后，从 pending_deleted_targets.json 移除该条目。
        self.cleanup_completed_deleted_targets(&result);

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

        (result, lifecycle_committed_paths, lifecycle_receipts)
    }

    fn apply_local_lifecycle_deletes(
        &self,
        targets: &mut [crate::sync::types::TargetSyncResult],
    ) -> (
        Vec<std::path::PathBuf>,
        Vec<crate::sync::types::LocalLifecycleCommitReceipt>,
    ) {
        let lifecycle_committed_paths: Vec<std::path::PathBuf> = Vec::new();
        let mut lifecycle_receipts: Vec<crate::sync::types::LocalLifecycleCommitReceipt> =
            Vec::new();
        for target in targets {
            if let crate::sync::types::LocalLifecycleCommitAction::DeleteProject {
                project_id,
                expected_local_lww,
            } = &target.local_lifecycle_action
            {
                // DeleteProject guard —
                // 用 snapshot_local_records_read_only 重新计算当前 local target LWW，
                // 与 expected_local_lww 严格比较。current_local == expected 才放行，
                // 其他任何情况都拒绝。
                // expected_local_lww 非 Option —
                // 破坏性 action 必须携带 guard。
                let expected_lww = crate::sync::full_sync::LiveTargetLww {
                    lww_time_ms: expected_local_lww.lww_time_ms,
                    device_id: expected_local_lww.device_id.clone(),
                };
                let project_root = self.projects_root.join(project_id);
                match crate::sync::staging::replace::check_replace_project_guard(
                    &project_root,
                    &expected_lww,
                ) {
                    crate::sync::staging::replace::ReplaceProjectGuardResult::Ok => {
                        // guard 通过，继续执行删除。
                    }
                    crate::sync::staging::replace::ReplaceProjectGuardResult::Err(e) => {
                        let msg = match e {
                            crate::sync::staging::replace::ReplaceProjectGuardError::LocalAdvanced {
                                expected,
                                current,
                            } => format!(
                                "DeleteProject guard failed: local diverged \
                                 (expected lww_time={} device_id={}, current lww_time={} device_id={}) \
                                 — not touching live",
                                expected.lww_time_ms,
                                expected.device_id,
                                current.lww_time_ms,
                                current.device_id
                            ),
                            crate::sync::staging::replace::ReplaceProjectGuardError::SnapshotFailed(err) => {
                                format!("DeleteProject guard snapshot failed: {err}")
                            }
                        };
                        log::warn!(
                            "[sync] commit_full_sync: DeleteProject {} guard failed: {}",
                            project_id,
                            msg
                        );
                        target.result.status =
                            crate::sync::SyncStatus::RecoverableError(msg.clone());
                        target.result.error = Some(msg);
                        continue;
                    }
                }

                //   RemoteLifecycle origin — 不生成
                // PendingDeletedTarget（远端已删，不反向要求删远端）。
                log::info!(
                    "[sync] commit_full_sync: DeleteProject (remote lifecycle) project_id={}",
                    project_id
                );
                let device_id = crate::settings::load_device_info(&self.app_data_root)
                    .map(|info| info.device_id)
                    .unwrap_or_default();
                match crate::project::delete_project_with_changes(
                    &self.projects_root,
                    project_id,
                    &self.app_data_root,
                    &device_id,
                    crate::project::ProjectDeleteOrigin::RemoteLifecycle,
                ) {
                    Ok(outcome) => {
                        // RemoteLifecycle delete 走单一
                        // durable 路线 — 不把 outcome.changes.to_flat_paths() 塞进
                        // lifecycle_committed_paths（避免与 receipt.change_set 双重记 history）。
                        // API 层用 receipt.change_set 调 record_workspace_change_set_history，
                        // 成功后调 ack_project_delete_history。
                        lifecycle_receipts.push(crate::sync::types::LocalLifecycleCommitReceipt {
                            journal_token: outcome.journal_token.clone(),
                            change_set: outcome.changes.clone(),
                            unbound_starmap_ids: outcome.unbound_starmap_ids.clone(),
                            origin: crate::project::ProjectDeleteOrigin::RemoteLifecycle,
                        });
                        // 真实删除是实际变更，
                        // 用 Success 触发 rebuild_search_index（NoChanges 不触发）。
                        target.result = crate::sync::types::SyncResult::success();
                    }
                    Err(e) => {
                        let msg = format!("remote lifecycle delete failed: {e}");
                        target.result.status =
                            crate::sync::SyncStatus::RecoverableError(msg.clone());
                        target.result.error = Some(msg);
                    }
                }
            }
        }
        (lifecycle_committed_paths, lifecycle_receipts)
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
