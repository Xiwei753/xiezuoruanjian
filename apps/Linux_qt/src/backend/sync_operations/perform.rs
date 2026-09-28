//! 同步真实执行（`perform_sync_internal`）。
//!
//! 从 `sync_operations.rs` 拆出：这是同步链路上最长的一个方法，独占近 480 行。

use super::*;

impl AppBackend {
    pub(crate) fn perform_sync_internal(
        &mut self,
        trigger: &str,
        silent_success: bool,
        sync_qptr: Option<QPointer<SyncBackend>>,
    ) -> QString {
        let op_id = uuid::Uuid::new_v4().to_string();
        if self.current_sync_in_progress {
            // 手动同步请求到来时正在运行同步：排队等待，不丢点击，不并行启动第二个同步。
            // 连续点击只保留一次 pending（已经是 true 就不再重复设），不堆无限队列。
            if trigger == "manual" {
                if !self.manual_sync_pending {
                    self.manual_sync_pending = true;
                    self.debug_log(
                        "sync",
                        "manual_sync_pending_set",
                        "sync already running, manual sync queued",
                    );
                } else {
                    self.debug_log(
                        "sync",
                        "manual_sync_pending_already_set",
                        "sync already running and pending already queued",
                    );
                }
                let state = writer_core::api::SyncOperationStateDto {
                    operation_id: op_id.clone(),
                    operation_kind: "sync".to_string(),
                    status_code: "syncing".to_string(),
                    phase_key: None,
                    summary_key: Some("sync.status.already_running".to_string()),
                    summary_args: std::collections::HashMap::new(),
                    current_target: None,
                    finished_targets: 0,
                    total_targets: 0,
                    counts: writer_core::api::SyncOperationCountsDto::default(),
                    raw_error: None,
                };
                self.current_sync_operation_state =
                    serde_json::to_string(&state).unwrap_or_default();
            } else {
                self.debug_log(
                    "sync",
                    "perform_sync_skipped",
                    &format!("sync already running (trigger={})", trigger),
                );
            }
            return self.current_sync_operation_id.clone().into();
        }
        let token_present = !self.current_sync_token.is_empty();
        let masked_url = mask_sync_error(&self.current_sync_remote_url);
        self.debug_log(
            "sync",
            "perform_sync_start",
            &format!(
                "trigger={}, remote_url={}, branch={}, token_present={}",
                trigger, masked_url, self.current_sync_branch, token_present
            ),
        );
        let data_root = self.current_data_root.clone();
        let projects_root = self.current_projects_root.clone();
        if data_root.is_empty() {
            let state = writer_core::api::SyncOperationStateDto {
                operation_id: op_id.clone(),
                operation_kind: "sync".to_string(),
                status_code: "error".to_string(),
                phase_key: None,
                summary_key: Some("sync.block.no_workspace".to_string()),
                summary_args: std::collections::HashMap::new(),
                current_target: None,
                finished_targets: 0,
                total_targets: 0,
                counts: writer_core::api::SyncOperationCountsDto::default(),
                raw_error: None,
            };
            self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();
            self.debug_error("sync", "perform_sync_failed", "workspace_empty");
            return op_id.into();
        }

        if self.current_sync_remote_url.is_empty() {
            self.current_sync_status = "error".to_string();
            let state = writer_core::api::SyncOperationStateDto {
                operation_id: op_id.clone(),
                operation_kind: "sync".to_string(),
                status_code: "error".to_string(),
                phase_key: None,
                summary_key: Some("sync.block.remote_url_missing".to_string()),
                summary_args: std::collections::HashMap::new(),
                current_target: None,
                finished_targets: 0,
                total_targets: 0,
                counts: writer_core::api::SyncOperationCountsDto::default(),
                raw_error: None,
            };
            self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();
            self.debug_error("sync", "perform_sync_failed", "remote_url_empty");
            return op_id.into();
        }

        if self.current_sync_token.is_empty() {
            self.current_sync_status = "error".to_string();
            let state = writer_core::api::SyncOperationStateDto {
                operation_id: op_id.clone(),
                operation_kind: "sync".to_string(),
                status_code: "error".to_string(),
                phase_key: None,
                summary_key: Some("sync.block.token_missing".to_string()),
                summary_args: std::collections::HashMap::new(),
                current_target: None,
                finished_targets: 0,
                total_targets: 0,
                counts: writer_core::api::SyncOperationCountsDto::default(),
                raw_error: None,
            };
            self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();
            self.debug_error("sync", "perform_sync_failed", "token_empty");
            return op_id.into();
        }

        if self.current_sync_branch.is_empty() {
            self.current_sync_branch = "main".to_string();
        }

        self.current_sync_operation_id = op_id.clone();
        self.current_sync_operation_kind = "sync".to_string();

        self.flush_writing_stats();

        let state = writer_core::api::SyncOperationStateDto {
            operation_id: op_id.clone(),
            operation_kind: "sync".to_string(),
            status_code: "syncing".to_string(),
            phase_key: Some(if silent_success {
                "sync.phase.background_syncing".to_string()
            } else {
                "sync.phase.syncing".to_string()
            }),
            summary_key: None,
            summary_args: std::collections::HashMap::new(),
            current_target: None,
            finished_targets: 0,
            total_targets: 0,
            counts: writer_core::api::SyncOperationCountsDto::default(),
            raw_error: None,
        };
        self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();

        self.current_sync_status = "syncing".to_string();
        self.current_sync_in_progress = true;

        // Issue #779 评论 5854082763：新用户同步开始时，取消已有 GC maintenance。
        // GC maintenance 会尽快停止发起新的远端操作（token 取消后 run_generation_gc
        // 在下次检查时退出）。不立即清 current_gc_maintenance_cancel_token —
        // GC 线程的 done callback 会清。
        if let Some(gc_token) = self.current_gc_maintenance_cancel_token.as_ref() {
            gc_token.cancel();
            // Issue #779 评论 5854734343：cancel 了正在跑的 GC，标记 pending，
            // 确保本轮用户同步完成后能补启动一次 GC（不靠"下次用户再同步"兜底）。
            // 不清 token — GC 线程的 done callback 会清，并在清后检查 pending 补启动。
            self.gc_maintenance_pending = true;
            self.debug_log(
                "sync",
                "gc_maintenance_cancelled_for_user_sync",
                "cancelled existing GC maintenance before starting user sync",
            );
        }

        // 获取 workspace git layout 快照，供后台线程用 with_layout_core_api 构造 API。
        // 不在线程里重新 bootstrap（ensure .git + recover_storage_transactions）。
        // 无 layout 说明 workspace 未正确打开，直接返回状态错误。
        let layout = match self.current_workspace_git_layout.clone() {
            Some(l) => l,
            None => {
                self.current_sync_in_progress = false;
                self.current_sync_status = "error".to_string();
                let state = writer_core::api::SyncOperationStateDto {
                    operation_id: op_id.clone(),
                    operation_kind: "sync".to_string(),
                    status_code: "error".to_string(),
                    phase_key: None,
                    summary_key: Some("sync.block.no_workspace_layout".to_string()),
                    summary_args: std::collections::HashMap::new(),
                    current_target: None,
                    finished_targets: 0,
                    total_targets: 0,
                    counts: writer_core::api::SyncOperationCountsDto::default(),
                    raw_error: None,
                };
                self.current_sync_operation_state =
                    serde_json::to_string(&state).unwrap_or_default();
                self.debug_error("sync", "perform_sync_failed", "no_workspace_git_layout");
                return op_id.into();
            }
        };

        // Issue #729：为新同步创建取消令牌并捕获当前 workspace generation。
        self.current_sync_cancel_token = Some(std::sync::Arc::new(
            writer_core::sync::SyncCancellationToken::new(),
        ));
        let workspace_generation = self.current_workspace_generation;
        // Issue #729：clone token 传入后台线程，再传入 Core API perform_full_sync。
        // SyncCancellationToken 是 Clone 的（内部 Arc<AtomicBool>），可廉价克隆。
        let cancel_token = self
            .current_sync_cancel_token
            .as_ref()
            .map(|arc| (**arc).clone());
        // Issue #763：创建同步进度共享 sink。total 初始 0，Core run_transfer 会在
        // 各 target 开始/结束时更新 finished/total。存到 self 供诊断导出主线程读，
        // clone 一份 move 进后台线程传给 perform_full_sync。SyncProgressSink 是
        // Clone（廉价 Arc）且 Send + Sync，可安全跨线程共享。
        let progress_sink = writer_core::sync::SyncProgressSink::new(0);
        self.current_sync_progress = Some(progress_sink.clone());
        // Issue #729 评论 5763441474：捕获 data_root 用于回调身份校验。
        let data_root_capture = data_root.clone();

        let app_qptr = QPointer::from(&*self);
        // Issue #762 评论 5826175490 第 5 点：target progress 回调通道。
        // 通道主体在 sync_bridge（SyncTargetProgressOutcome + make_target_progress_callback），
        // 这里只提供"回到主线程做什么"：queued_callback 包 QPointer<SyncBackend>，
        // 每个 target 完成后投递回主线程调 handle_sync_target_progress 刷新全局冲突数。
        // 平台同步线程不再等最终 FullSyncResult 才刷新冲突。
        let progress_qptr = sync_qptr.clone();
        let progress_callback: Option<writer_core::sync::full_sync::SyncProgressCallback> =
            progress_qptr.as_ref().map(|sq| {
                let sq = sq.clone();
                let main_thread_cb = qmetaobject::queued_callback(move |_progress| {
                    sq.as_pinned().map(|this| {
                        let mut this = this.borrow_mut();
                        this.handle_sync_target_progress();
                    });
                });
                crate::sync_bridge::make_target_progress_callback(move |progress| {
                    main_thread_cb(progress);
                })
            });
        let callback = make_outcome_callback(app_qptr, sync_qptr);

        let op_id_capture = op_id.clone();
        let trigger = trigger.to_string();
        thread::spawn(move || {
            // SAFETY: catch_unwind requires the closure to be UnwindSafe. The closure captures
            // owned String data (data_root, projects_root, op_id_capture), a GitRepoLayout
            // snapshot, and a `&SyncProgressSink` (for `.clone()`). All of these auto-implement
            // UnwindSafe/RefUnwindSafe: String/GitRepoLayout are plain data, and
            // SyncProgressSink is `Arc<Mutex<SyncTargetProgressDto>>` where the inner type is
            // plain data — std impls `RefUnwindSafe for Mutex<T>` and `RefUnwindSafe for Arc<T>`
            // when `T: RefUnwindSafe`. No hand-rolled `unsafe impl` is needed.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let api = crate::backend::app_backend::with_layout_core_api(
                    &data_root,
                    &projects_root,
                    &layout,
                );
                let mut config = match prepare_sync_profile(&api) {
                    Ok(c) => c,
                    Err(e) => {
                        let err_str = e.raw_error().to_string();
                        let summary_key = e.summary_key().to_string();
                        let state = writer_core::api::SyncOperationStateDto {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "sync".to_string(),
                            status_code: "error".to_string(),
                            phase_key: None,
                            summary_key: Some(summary_key),
                            summary_args: std::collections::HashMap::new(),
                            current_target: None,
                            finished_targets: 0,
                            total_targets: 0,
                            counts: writer_core::api::SyncOperationCountsDto::default(),
                            raw_error: Some(mask_sync_error(&err_str)),
                        };
                        return SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            sync_status: "error".to_string(),
                            action_result: serde_json::to_string(&state).unwrap_or_default(),
                            workspace_generation,
                            data_root: data_root_capture.clone(),
                        };
                    }
                };
                let net = crate::backend::app_backend::current_network_state();
                config.has_network_permission = net.is_connected;
                config.has_network_state_permission = true;

                let backend_label = config.active_provider.clone();
                debug_log_static(
                    "sync",
                    "perform_sync_backend",
                    &format!("backend_type={}, sync_mode=lww_manifest", backend_label),
                );

                match api.perform_full_sync(
                    config,
                    trigger == "manual",
                    cancel_token.clone(),
                    Some(progress_sink.clone()),
                    progress_callback.as_ref(),
                ) {
                    Ok(result) => {
                        let status_code = result.overall_status.clone();
                        let summary_key = match status_code.as_str() {
                            "success" => Some("sync.result.success_summary".to_string()),
                            "latest_wins_applied" => {
                                Some("sync.result.latest_wins_summary".to_string())
                            }
                            "no_changes" => Some("sync.result.no_changes_summary".to_string()),
                            "configured_not_tested" => {
                                Some("sync.result.configured_not_tested".to_string())
                            }
                            "conflict" => Some("sync.result.conflict_summary".to_string()),
                            "partial_conflict" => {
                                Some("sync.result.partial_conflict_summary".to_string())
                            }
                            "dirty_repo_blocked" => {
                                Some("sync.result.dirty_repo_blocked".to_string())
                            }
                            "branch_missing_recovered" => {
                                Some("sync.result.branch_recovered_summary".to_string())
                            }
                            "token_missing" => Some("sync.result.token_missing".to_string()),
                            "token_invalid" => Some("sync.result.token_invalid".to_string()),
                            "token_permission_denied" => {
                                Some("sync.result.token_permission_denied".to_string())
                            }
                            "repo_not_found_or_no_permission" => {
                                Some("sync.result.repo_not_found_or_no_permission".to_string())
                            }
                            "branch_missing" | "remote_branch_missing" => {
                                Some("sync.result.branch_missing".to_string())
                            }
                            "network_failed"
                            | "dns_failed"
                            | "tls_failed"
                            | "github_network_failed" => {
                                Some("sync.result.network_failed".to_string())
                            }
                            "auth_failed" => Some("sync.result.auth_failed".to_string()),
                            "non_fast_forward" => Some("sync.result.non_fast_forward".to_string()),
                            "unrelated_histories" => {
                                Some("sync.result.unrelated_histories".to_string())
                            }
                            _ => Some("sync.result.generic_error".to_string()),
                        };

                        let counts = writer_core::api::SyncOperationCountsDto {
                            uploaded: result.total_uploaded,
                            downloaded: result.total_downloaded,
                            local_deleted: result.total_local_deletes,
                            remote_deleted: result.total_remote_deletes,
                            overwritten: result.total_overwritten,
                            ignored: result.total_ignored,
                            conflicts: result.total_conflicts,
                            conflict_count: 0,
                        };

                        let mut summary_args = std::collections::HashMap::new();
                        let all_conflicts: Vec<_> = result
                            .targets
                            .iter()
                            .flat_map(|t| t.result.conflicts.iter())
                            .collect();
                        if !all_conflicts.is_empty() {
                            let mut files = all_conflicts
                                .iter()
                                .map(|c| c.local_path.clone())
                                .collect::<Vec<_>>();
                            files.sort();
                            files.dedup();
                            summary_args.insert(
                                "conflict_files".to_string(),
                                format_conflict_files(&files),
                            );
                        }

                        let state = writer_core::api::SyncOperationStateDto {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "sync".to_string(),
                            status_code: status_code.clone(),
                            phase_key: None,
                            summary_key,
                            summary_args,
                            current_target: None,
                            finished_targets: 0,
                            total_targets: 0,
                            counts,
                            raw_error: result.error.as_ref().map(|e| mask_sync_error(e)),
                        };

                        SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            sync_status: status_code,
                            action_result: serde_json::to_string(&state).unwrap_or_default(),
                            workspace_generation,
                            data_root: data_root_capture.clone(),
                        }
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        let cat = sync_error_category_from_code(None, &err_str);

                        let summary_key = match cat.as_str() {
                            "token_missing" => "sync.result.token_missing",
                            "token_invalid" => "sync.result.token_invalid",
                            "token_permission_denied" => "sync.result.token_permission_denied",
                            "repo_not_found_or_no_permission" => {
                                "sync.result.repo_not_found_or_no_permission"
                            }
                            "branch_missing" => "sync.result.branch_missing",
                            "network_failed" => "sync.result.network_failed",
                            "auth_failed" => "sync.result.auth_failed",
                            "non_fast_forward" => "sync.result.non_fast_forward",
                            "unrelated_histories" => "sync.result.unrelated_histories",
                            "conflict" => "sync.result.conflict_summary",
                            _ => "sync.result.generic_error",
                        };

                        let state = writer_core::api::SyncOperationStateDto {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "sync".to_string(),
                            status_code: cat.clone(),
                            phase_key: None,
                            summary_key: Some(summary_key.to_string()),
                            summary_args: std::collections::HashMap::new(),
                            current_target: None,
                            finished_targets: 0,
                            total_targets: 0,
                            counts: writer_core::api::SyncOperationCountsDto::default(),
                            raw_error: Some(mask_sync_error(&err_str)),
                        };
                        SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            sync_status: cat,
                            action_result: serde_json::to_string(&state).unwrap_or_default(),
                            workspace_generation,
                            data_root: data_root_capture.clone(),
                        }
                    }
                }
            }));

            match result {
                Ok(outcome) => callback(outcome),
                Err(err) => {
                    let panic_msg = if let Some(s) = err.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = err.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "panic.unknown".to_string()
                    };
                    let state = writer_core::api::SyncOperationStateDto {
                        operation_id: op_id_capture.clone(),
                        operation_kind: "sync".to_string(),
                        status_code: "fatal_error".to_string(),
                        phase_key: None,
                        summary_key: Some("error.sync_panic".to_string()),
                        summary_args: [("panic_msg".to_string(), panic_msg)].into_iter().collect(),
                        current_target: None,
                        finished_targets: 0,
                        total_targets: 0,
                        counts: writer_core::api::SyncOperationCountsDto::default(),
                        raw_error: None,
                    };
                    callback(SyncTaskOutcome {
                        operation_id: op_id_capture,
                        sync_status: "fatal_error".to_string(),
                        action_result: serde_json::to_string(&state).unwrap_or_default(),
                        workspace_generation,
                        data_root: data_root_capture,
                    });
                }
            }
        });

        op_id.into()
    }
}
