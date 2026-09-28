//! 同步预演（dry run）：只算不落盘，返回将要发生的变更。
//!
//! 从 `sync_operations.rs` 拆出：预演与真实执行共享 Core 侧接口但分支判断
//! 完全不同，合在一个函数群里会互相淹没。

use super::*;

impl AppBackend {
    pub(crate) fn perform_sync_dry_run(
        &mut self,
        sync_qptr: Option<QPointer<SyncBackend>>,
    ) -> QString {
        let data_root = self.current_data_root.clone();
        let projects_root = self.current_projects_root.clone();

        let op_id = uuid::Uuid::new_v4().to_string();
        // single-flight 拦截：busy 时拒绝 dry-run，不覆盖正在运行的操作的 operation_id。
        if self.current_sync_in_progress {
            let state = writer_core::api::SyncOperationStateDto {
                operation_id: op_id.clone(),
                operation_kind: "dry_run".to_string(),
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
            self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();
            return op_id.into();
        }
        self.current_sync_operation_id = op_id.clone();
        self.current_sync_operation_kind = "dry_run".to_string();

        if data_root.is_empty() {
            let state = writer_core::api::SyncOperationStateDto {
                operation_id: op_id.clone(),
                operation_kind: "dry_run".to_string(),
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
            return op_id.into();
        }

        if self.current_sync_remote_url.is_empty() {
            self.current_sync_status = "error".to_string();
            let state = writer_core::api::SyncOperationStateDto {
                operation_id: op_id.clone(),
                operation_kind: "dry_run".to_string(),
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
            return op_id.into();
        }

        if self.current_sync_token.is_empty() {
            self.current_sync_status = "error".to_string();
            let state = writer_core::api::SyncOperationStateDto {
                operation_id: op_id.clone(),
                operation_kind: "dry_run".to_string(),
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
            return op_id.into();
        }

        if self.current_sync_branch.is_empty() {
            self.current_sync_branch = "main".to_string();
        }

        self.current_sync_status = "syncing".to_string();
        self.current_sync_in_progress = true;
        let state = writer_core::api::SyncOperationStateDto {
            operation_id: op_id.clone(),
            operation_kind: "dry_run".to_string(),
            status_code: "syncing".to_string(),
            phase_key: Some("sync.phase.dry_run".to_string()),
            summary_key: None,
            summary_args: std::collections::HashMap::new(),
            current_target: None,
            finished_targets: 0,
            total_targets: 0,
            counts: writer_core::api::SyncOperationCountsDto::default(),
            raw_error: None,
        };
        self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();

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
                    operation_kind: "dry_run".to_string(),
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
                self.debug_error(
                    "sync",
                    "perform_sync_dry_run_failed",
                    "no_workspace_git_layout",
                );
                return op_id.into();
            }
        };

        // Issue #729：为新同步创建取消令牌并捕获当前 workspace generation。
        // 令牌存入 AppBackend，切工作区时由 reset_workspace_state 取消。
        // generation 副本随线程捕获，回调时与最新 generation 比对丢弃过期结果。
        self.current_sync_cancel_token = Some(std::sync::Arc::new(
            writer_core::sync::SyncCancellationToken::new(),
        ));
        let workspace_generation = self.current_workspace_generation;
        // Issue #729 评论 5763441474：捕获 data_root 用于回调身份校验。
        let data_root_capture = data_root.clone();

        let app_qptr = QPointer::from(&*self);
        let callback = make_outcome_callback(app_qptr, sync_qptr);

        let op_id_capture = op_id.clone();
        thread::spawn(move || {
            // SAFETY: catch_unwind requires the closure to be UnwindSafe. The closure only captures
            // owned String data (data_root, projects_root, op_id_capture) and a GitRepoLayout
            // snapshot which auto-implement UnwindSafe. No shared mutable state or borrows are
            // captured, so the closure is UnwindSafe by auto-impl without needing
            // AssertUnwindSafe.
            let result = std::panic::catch_unwind(|| {
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
                            operation_kind: "dry_run".to_string(),
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

                match api.perform_full_sync_dry_run(config) {
                    Ok(plan) => {
                        let counts = writer_core::api::SyncOperationCountsDto {
                            uploaded: plan.total_to_upload,
                            downloaded: plan.total_to_download,
                            local_deleted: plan.total_to_delete_local,
                            remote_deleted: plan.total_to_delete_remote,
                            ignored: plan.total_ignored,
                            conflicts: plan.total_conflicts,
                            conflict_count: 0,
                            overwritten: 0,
                        };

                        let state = writer_core::api::SyncOperationStateDto {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "dry_run".to_string(),
                            status_code: "dry_run_success".to_string(),
                            phase_key: None,
                            summary_key: Some("sync.result.dry_run_summary".to_string()),
                            summary_args: std::collections::HashMap::new(),
                            current_target: None,
                            finished_targets: 0,
                            total_targets: 0,
                            counts,
                            raw_error: None,
                        };

                        SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            sync_status: "dry_run_success".to_string(),
                            action_result: serde_json::to_string(&state).unwrap_or_default(),
                            workspace_generation,
                            data_root: data_root_capture.clone(),
                        }
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        let cat = sync_error_category_from_code(None, &err_str);

                        let state = writer_core::api::SyncOperationStateDto {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "dry_run".to_string(),
                            status_code: cat.clone(),
                            phase_key: None,
                            summary_key: Some("sync.result.dry_run_failed".to_string()),
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
            });

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
                        operation_kind: "dry_run".to_string(),
                        status_code: "fatal_error".to_string(),
                        phase_key: None,
                        summary_key: Some("error.sync_dry_run_panic".to_string()),
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
