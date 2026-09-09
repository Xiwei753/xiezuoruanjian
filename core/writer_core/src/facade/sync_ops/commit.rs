use crate::sync::full_sync_utils::now_epoch_seconds;

impl crate::facade::WriterCore {
    /// 三段式全量同步 — Commit 阶段（短写锁内调用）。
    ///
    /// 聚合 [`crate::sync::full_sync::FullSyncTransferResult`] → `FullSyncResult`，
    /// 原子写终态 `FullSyncState`，成功类重建搜索索引。
    ///
    /// staging run 的三方 commit 逻辑正式接入。
    /// 逐 target 判断 — 只有该 target 的 Transfer 结果
    /// 属于允许提交的终态，才计算/应用它的 commit plan；失败 target 直接丢弃 staging。
    ///
    /// 三方冲突不再只改 overall_status。
    /// 冲突按 target 保留完整元数据（rel_path + base/local/incoming hash），
    /// 映射成 `SyncConflict` 写入对应 target 的 `SyncResult.conflicts`，
    /// 同时持久化到该 target live root 的 `SyncState.conflicts/conflicted_files`。
    ///
    /// `staging_runs` 来自 Prepare 阶段，与 `transfer_result.targets` 按索引对应；
    /// commit 完成后显式 cleanup（`Drop` 也会兜底）。
    ///
    /// 返回 `(FullSyncResult, committed_paths, lifecycle_receipts)`，
    /// `committed_paths` 是本次 commit 真正落盘的 workspace-relative paths，
    /// `lifecycle_receipts` 是 RemoteLifecycle 删除事务的完整 receipt，
    /// 供 API 层调 `record_workspace_change_set` + `ack_project_delete_history`。
    #[allow(clippy::excessive_nesting)]
    pub fn commit_full_sync(
        &self,
        transfer_result: crate::sync::full_sync::FullSyncTransferResult,
        staging_runs: Vec<crate::sync::staging::StagingRun>,
    ) -> (
        crate::sync::types::FullSyncResult,
        Vec<std::path::PathBuf>,
        Vec<crate::sync::types::LocalLifecycleCommitReceipt>,
    ) {
        //   先处理 local_lifecycle_action（DeleteProject）。
        // 对有 DeleteProject action 的 target，执行完整 Project 本地删除事务
        // （move worktree / unbind starmaps / history），不生成 PendingDeletedTarget
        // （远端已删，不反向要求删远端）。staging commit 会跳过这些 target。
        // 收集 LocalLifecycleCommitReceipt，
        // API 层负责记 history + ack（facade 没有 workspace_git_layout）。
        let mut targets = transfer_result.targets;
        let (lifecycle_committed_paths, lifecycle_receipts) =
            self.apply_local_lifecycle_deletes(&mut targets);

        //   第3/4节：逐 target 判断 transfer 结果，
        // 只对成功终态的 target 做 staging commit；commit IO 失败向上传播。
        let commit_outcome =
            crate::sync::commit_helpers::apply_staging_commits_for_targets(&staging_runs, &targets);

        // commit 失败的 target 需要把失败信息
        // 注入到对应的 TargetSyncResult 中，让聚合逻辑产生 Recoverable/Fatal 状态。
        //   targets 已在上方 lifecycle 循环中声明并修改，
        // 不再重新从 transfer_result.targets 取值（否则会丢失 lifecycle 修改）。
        for (idx, commit_result) in commit_outcome.target_results.iter().enumerate() {
            if let crate::sync::commit_helpers::TargetCommitResult::Failed(msg) = commit_result {
                if let Some(target) = targets.get_mut(idx) {
                    // commit 失败视为 RecoverableError（下次同步可重试）
                    target.result.status = crate::sync::SyncStatus::RecoverableError(format!(
                        "staging_commit_failed: {}",
                        msg
                    ));
                    target.result.error = Some(format!("staging commit failed: {}", msg));
                }
            }
        }

        //
        // 三方冲突按 target 映射成 SyncConflict，复用 conflict.rs 的
        // record_staging_conflicts() 统一写 conflicts.json + SyncState。
        // 持久化失败必须传播到对应 target 的错误状态，不能只打日志。
        //
        // 不再用 `=` 覆盖 target.result.conflicts，
        // 而是把 Transfer 阶段已有的冲突（如 GitHub LWW 发现的正文冲突）
        // 传给 record_staging_conflicts 做合并，保留两层冲突。
        for (idx, target_conflicts) in commit_outcome.target_conflicts.iter().enumerate() {
            if target_conflicts.is_empty() {
                continue;
            }
            if let Some(target) = targets.get_mut(idx) {
                let live_root = staging_runs[idx].target_live_root();
                // 保留 Transfer 阶段已有的冲突（如 GitHub LWW 发现的正文冲突）。
                let existing_conflicts = target.result.conflicts.clone();
                match crate::sync::conflict::record_staging_conflicts(
                    live_root,
                    &target.remote_prefix,
                    target_conflicts,
                    &existing_conflicts,
                ) {
                    Ok(merged_conflicts) => {
                        // 合并后的完整冲突列表
                        // （Transfer + staging），不再覆盖。
                        target.result.conflicts = merged_conflicts;
                        target.result.status = crate::sync::SyncStatus::Conflict;
                    }
                    Err(e) => {
                        // 持久化失败：target 进入 RecoverableError，下次同步可重试
                        target.result.status = crate::sync::SyncStatus::RecoverableError(format!(
                            "staging_conflict_persist_failed: {}",
                            e
                        ));
                        target.result.error =
                            Some(format!("failed to persist staging conflicts: {}", e));
                    }
                }
            }
        }

        let mut result = crate::sync::full_sync::aggregate_full_sync_result(targets);

        // generation GC 失败 → 聚合进 FullSyncResult。
        // GC 出错是 RecoverableError（下一轮 full-sync 自然再次执行 GC）。
        // 只在当前 overall_status 是成功类时升级，避免覆盖更严重的 FatalError/Conflict。
        if let Some(Err(gc_err)) = &transfer_result.generation_gc_result {
            let gc_msg = format!("generation_gc failed: {gc_err}");
            log::warn!("[sync] commit_full_sync: {gc_msg}");
            if matches!(
                result.overall_status,
                crate::sync::SyncStatus::Success
                    | crate::sync::SyncStatus::NoChanges
                    | crate::sync::SyncStatus::LatestWinsApplied
            ) {
                result.overall_status =
                    crate::sync::SyncStatus::RecoverableError("generation_gc_failed".to_string());
                result.error = Some(gc_msg);
            }
        }

        //   deleted target 远端清理成功后，
        // 从 pending_deleted_targets.json 移除该条目。
        self.cleanup_completed_deleted_targets(&result);

        let previous_state = self.load_full_sync_state().unwrap_or(None);
        let new_state = crate::sync::full_sync_state::FullSyncState::from_result_and_previous(
            &result,
            previous_state.as_ref(),
            now_epoch_seconds(),
        );
        if let Err(e) = self.save_full_sync_state(&new_state) {
            log::warn!("Failed to persist full sync state: {e}");
        }

        if matches!(
            result.overall_status,
            crate::sync::SyncStatus::Success | crate::sync::SyncStatus::LatestWinsApplied
        ) {
            if let Err(e) = self.rebuild_search_index(None) {
                log::warn!("Failed to rebuild search index after full sync: {e}");
            }
        }

        //   合并 lifecycle 删除产生的 committed_paths
        // 与 staging commit 产生的 committed_paths。
        let mut all_committed_paths = lifecycle_committed_paths;
        all_committed_paths.extend(commit_outcome.committed_paths);
        (result, all_committed_paths, lifecycle_receipts)
    }

    ///    修复：对有 `DeleteProject` lifecycle action 的 target
    /// 执行完整 Project 本地删除事务（move worktree / unbind starmaps / history），
    /// 不生成 PendingDeletedTarget（远端已删，不反向要求删远端）。staging commit 会
    /// 跳过这些 target。
    ///
    /// DeleteProject 在执行删除前先用
    /// `snapshot_local_records_read_only` 重新计算当前 local target LWW，与
    /// `expected_local_lww` guard 比较。current_local > expected → 不动 live →
    /// target 进入 RecoverableError（下次同步重试）。ReplaceProject 的 guard
    /// 在 `apply_staging_commits_for_targets` 里检查（走 replace plan）。
    ///
    /// 返回 `(lifecycle_committed_paths, lifecycle_receipts)`：
    /// - `lifecycle_committed_paths`：本次删除真正落盘的 workspace-relative paths；
    /// - `lifecycle_receipts`：`LocalLifecycleCommitReceipt`，供 API 层记 history + ack。
    #[allow(clippy::excessive_nesting)]
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

    ///   deleted target 远端清理/恢复成功后，
    /// 从 pending_deleted_targets.json 移除该条目。
    ///
    /// 按 typed `DeletedTargetResolution` 精确确认（不再按 `SyncStatus` 猜）：
    /// - `LocalDeleteWins` 且 `result.status` 成功类 → 移除 pending（远端删除+catalog tombstone 写入成功）；
    /// - `RemoteTargetWins` 且 `result.status` 成功类 → 移除 pending（本地恢复成功）；
    /// - `Retry` 或 `None` → 保留 pending（下次同步重试）。
    fn cleanup_completed_deleted_targets(&self, result: &crate::sync::types::FullSyncResult) {
        use crate::sync::types::DeletedTargetResolution;

        for t in &result.targets {
            if t.target_kind != "deleted_project" {
                continue;
            }
            let should_remove = match t.deleted_resolution {
                Some(DeletedTargetResolution::LocalDeleteWins)
                | Some(DeletedTargetResolution::RemoteTargetWins) => {
                    // 且 result.status 是成功类（远端删除/恢复真正落盘成功）。
                    matches!(
                        t.result.status,
                        crate::sync::SyncStatus::Success
                            | crate::sync::SyncStatus::LatestWinsApplied
                    )
                }
                // Retry 或 None（未走 deleted target 决策路径）→ 保留 pending。
                Some(DeletedTargetResolution::Retry) | None => false,
            };
            if should_remove {
                self.remove_pending_deleted_by_prefix(&t.remote_prefix);
                // 同时移除 pending_remote_cleanup
                // （RemoteCleanupProject 成功时）。如果不存在则是幂等 no-op。
                self.remove_pending_remote_cleanup_by_prefix(&t.remote_prefix);
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
        let previous = self.load_full_sync_state().unwrap_or(None);
        let state = crate::sync::full_sync_state::FullSyncState::failed_before_targets(
            previous.as_ref(),
            status,
            now_epoch_seconds(),
            failed_target,
        );
        if let Err(e) = self.save_full_sync_state(&state) {
            log::warn!("Failed to persist full sync early failure state: {e}");
        }
    }
}
