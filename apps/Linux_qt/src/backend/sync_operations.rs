// =============================================================================
// sync_operations.rs — 同步执行与自动同步调度逻辑
// =============================================================================
//
// 引用了什么：
// - super::*：引入 AppBackend 核心后端的全部方法与结构体。
// - crate::sync_bridge：引入 SyncTaskOutcome、mask_sync_error 等同步工具函数。
// - writer_core::api::WriterCoreApi：核心库对外的统一 API 入口。
//
// 干什么的：
// - 实现 AppBackend 上的同步执行方法：perform_sync、perform_sync_dry_run、perform_sync_internal。
// - 实现自动同步调度：request_auto_sync、maybe_auto_sync_on_foreground、trigger_auto_sync、can_start_auto_sync。
// - 实现同步结果处理：handle_sync_outcome、handle_successful_sync_refresh。
// - 所有同步操作通过 UUID operation_id 机制保证并发安全，通过 QPointer + queued_callback 实现线程安全回调。
//
// 被什么引用：
// - 被 apps/Linux_qt/src/backend/app_backend/sync_backend.rs 中的 SyncBackend QObject 间接调用。
// =============================================================================

use super::*;
use crate::sync_bridge::{mask_sync_error, sync_error_category_from_code, SyncTaskOutcome};

use writer_core::api::WriterCoreApi;

impl AppBackend {
    pub(crate) fn handle_sync_outcome(&mut self, outcome: SyncTaskOutcome) {
        if outcome.operation_id != self.current_sync_operation_id {
            self.debug_log(
                "sync",
                "sync_outcome_discarded",
                &format!(
                    "Discarded outdated outcome. Expected: {}, got: {}",
                    self.current_sync_operation_id, outcome.operation_id
                ),
            );
            return;
        }

        let status = outcome.sync_status.clone();
        let result_trunc = if outcome.action_result.chars().count() > 1000 {
            outcome.action_result.chars().take(1000).collect::<String>() + "..."
        } else {
            outcome.action_result.clone()
        };
        let sanitized_result = mask_sync_error(&result_trunc);
        self.debug_log(
            "sync",
            "sync_outcome_received",
            &format!("status={}, result={}", status, sanitized_result),
        );

        self.current_sync_status = outcome.sync_status.clone();
        self.current_sync_in_progress = false;
        self.current_last_sync_time = Self::now_epoch_seconds();
        self.current_sync_operation_state = outcome.action_result.clone();
        self.sync_status_changed();
        self.sync_action_completed();

        let status_str = outcome.sync_status.as_str();

        if status_str == "success" {
            let pending_path = self.current_pending_github_init_path.clone();
            if !pending_path.is_empty() {
                self.current_pending_github_init_path.clear();
                self.pending_github_init_path_changed();
                self.internal_open_data_root(&pending_path);
                self.load_sync_config();
                return;
            }
        }

        let sync_success = matches!(status_str, "success" | "branch_missing_recovered");
        if sync_success && self.has_workspace() {
            self.handle_successful_sync_refresh();
        } else if (status_str == "conflict"
            || status_str == "partial_conflict"
            || status_str == "unrelated_histories")
            && self.has_workspace()
        {
            self.reload_tree();
            self.trigger_projects_reloaded();
        }
    }

    pub(crate) fn handle_successful_sync_refresh(&mut self) {
        self.reload_tree();
        let chapter_deleted = self.reconcile_selection_after_tree_reload();
        self.trigger_projects_reloaded();
        self.workspace_content_changed();
        self.workspace_state_changed();

        if chapter_deleted {
            self.current_save_status = "chapter.deleted_remotely_refreshed".to_string();
            self.save_status_changed();
        }

        self.debug_log("sync", "sync_refresh_applied", "tree_reloaded=true");
    }

    pub(crate) fn can_start_auto_sync(&self, reason: &str, min_gap_secs: i64) -> bool {
        if self.current_sync_in_progress {
            return false;
        }
        if !self.current_has_data_root || !self.current_sync_enabled || !self.current_sync_auto_sync
        {
            return false;
        }
        if self.current_sync_remote_url.is_empty() || self.current_sync_token.is_empty() {
            return false;
        }
        let now = Self::now_epoch_seconds();
        if self.current_last_auto_sync_reason == reason {
            let elapsed = now.saturating_sub(self.current_last_auto_sync_started_at);
            if elapsed < min_gap_secs {
                return false;
            }
        }
        true
    }

    pub(crate) fn perform_sync_dry_run(&mut self) -> QString {
        let data_root = self.current_data_root.clone();
        let projects_root = self.current_projects_root.clone();

        let op_id = uuid::Uuid::new_v4().to_string();
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
                counts: writer_core::api::SyncOperationCountsDto::default(),
                raw_error: None,
            };
            self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();
            self.sync_action_completed();
            return op_id.into();
        }

        // per-project sync：每个作品目录是独立 Git 仓库，必须指定作品。
        let project_id = match self.selected_project_id.clone() {
            Some(id) if !id.is_empty() => id,
            _ => {
                self.current_sync_status = "error".to_string();
                let state = writer_core::api::SyncOperationStateDto {
                    operation_id: op_id.clone(),
                    operation_kind: "dry_run".to_string(),
                    status_code: "error".to_string(),
                    phase_key: None,
                    summary_key: Some("sync.block.no_project_selected".to_string()),
                    summary_args: std::collections::HashMap::new(),
                    counts: writer_core::api::SyncOperationCountsDto::default(),
                    raw_error: None,
                };
                self.current_sync_operation_state =
                    serde_json::to_string(&state).unwrap_or_default();
                self.sync_status_changed();
                self.sync_action_completed();
                self.debug_error("sync", "perform_sync_dry_run_failed", "no_project_selected");
                return op_id.into();
            }
        };

        if self.current_sync_remote_url.is_empty() {
            self.current_sync_status = "error".to_string();
            let state = writer_core::api::SyncOperationStateDto {
                operation_id: op_id.clone(),
                operation_kind: "dry_run".to_string(),
                status_code: "error".to_string(),
                phase_key: None,
                summary_key: Some("sync.block.remote_url_missing".to_string()),
                summary_args: std::collections::HashMap::new(),
                counts: writer_core::api::SyncOperationCountsDto::default(),
                raw_error: None,
            };
            self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();
            self.sync_status_changed();
            self.sync_action_completed();
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
                counts: writer_core::api::SyncOperationCountsDto::default(),
                raw_error: None,
            };
            self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();
            self.sync_status_changed();
            self.sync_action_completed();
            return op_id.into();
        }

        if self.current_sync_branch.is_empty() {
            self.current_sync_branch = "main".to_string();
            self.sync_config_changed();
        }

        self.current_sync_status = "syncing".to_string();
        self.sync_status_changed();

        let state = writer_core::api::SyncOperationStateDto {
            operation_id: op_id.clone(),
            operation_kind: "dry_run".to_string(),
            status_code: "syncing".to_string(),
            phase_key: Some("sync.phase.dry_run".to_string()),
            summary_key: None,
            summary_args: std::collections::HashMap::new(),
            counts: writer_core::api::SyncOperationCountsDto::default(),
            raw_error: None,
        };
        self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |outcome: SyncTaskOutcome| {
            qptr.as_pinned().map(|this| {
                let mut this = this.borrow_mut();
                this.handle_sync_outcome(outcome);
            });
        });

        let op_id_capture = op_id.clone();
        let project_id_capture = project_id.clone();
        thread::spawn(move || {
            // SAFETY: catch_unwind requires the closure to be UnwindSafe. The closure only captures
            // owned String data (data_root, projects_root, op_id_capture, project_id_capture) which
            // auto-implement UnwindSafe. No shared mutable state or borrows are captured, so the
            // closure is UnwindSafe by auto-impl without needing AssertUnwindSafe.
            let result = std::panic::catch_unwind(|| {
                let api = crate::backend::app_backend::create_core_api(&data_root, &projects_root);
                let mut config = match api.load_sync_config() {
                    Ok(c) => c,
                    Err(e) => {
                        let err_str = e.to_string();
                        let state = writer_core::api::SyncOperationStateDto {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "dry_run".to_string(),
                            status_code: "error".to_string(),
                            phase_key: None,
                            summary_key: Some("error.load_sync_config_failed".to_string()),
                            summary_args: std::collections::HashMap::new(),
                            counts: writer_core::api::SyncOperationCountsDto::default(),
                            raw_error: Some(mask_sync_error(&err_str)),
                        };
                        return SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            sync_status: "error".to_string(),
                            action_result: serde_json::to_string(&state).unwrap_or_default(),
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
                            overwritten: 0,
                        };

                        let state = writer_core::api::SyncOperationStateDto {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "dry_run".to_string(),
                            status_code: "dry_run_success".to_string(),
                            phase_key: None,
                            summary_key: Some("sync.result.dry_run_summary".to_string()),
                            summary_args: std::collections::HashMap::new(),
                            counts,
                            raw_error: None,
                        };

                        SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            sync_status: "dry_run_success".to_string(),
                            action_result: serde_json::to_string(&state).unwrap_or_default(),
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
                            counts: writer_core::api::SyncOperationCountsDto::default(),
                            raw_error: Some(mask_sync_error(&err_str)),
                        };

                        SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            sync_status: cat,
                            action_result: serde_json::to_string(&state).unwrap_or_default(),
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
                        counts: writer_core::api::SyncOperationCountsDto::default(),
                        raw_error: None,
                    };
                    callback(SyncTaskOutcome {
                        operation_id: op_id_capture,
                        sync_status: "fatal_error".to_string(),
                        action_result: serde_json::to_string(&state).unwrap_or_default(),
                    });
                }
            }
        });

        op_id.into()
    }

    pub(crate) fn perform_sync(&mut self) -> QString {
        self.perform_sync_internal("manual", false)
    }

    pub(crate) fn request_auto_sync(&mut self, reason: QString) {
        let reason_str = reason.to_string();
        self.trigger_auto_sync(&reason_str);
    }

    pub(crate) fn maybe_auto_sync_on_foreground(&mut self) {
        if !self.current_has_data_root
            || !self.current_sync_auto_sync
            || self.current_sync_in_progress
        {
            return;
        }
        let interval_secs = i64::from(self.current_sync_interval.max(60));
        let now = Self::now_epoch_seconds();
        let elapsed = now.saturating_sub(self.current_last_sync_time);
        if self.current_last_sync_time > 0 && elapsed < interval_secs {
            self.debug_log(
                "sync",
                "auto_sync_skipped_foreground",
                &format!("elapsed={}s, min={}s", elapsed, interval_secs),
            );
            return;
        }
        self.trigger_auto_sync("auto_sync_on_foreground");
    }

    pub(crate) fn trigger_auto_sync(&mut self, reason: &str) {
        if !self.can_start_auto_sync(reason, 60) {
            self.debug_log("sync", "auto_sync_skipped", &format!("reason={}", reason));
            return;
        }
        self.current_last_auto_sync_reason = reason.to_string();
        self.current_last_auto_sync_started_at = Self::now_epoch_seconds();
        self.debug_log("sync", reason, "triggered");
        self.perform_sync_internal(reason, true);
    }

    pub(crate) fn perform_sync_internal(&mut self, trigger: &str, silent_success: bool) -> QString {
        let op_id = uuid::Uuid::new_v4().to_string();
        if self.current_sync_in_progress {
            self.debug_log("sync", "perform_sync_skipped", "sync already running");
            if trigger == "manual" {
                let state = writer_core::api::SyncOperationStateDto {
                    operation_id: op_id.clone(),
                    operation_kind: "sync".to_string(),
                    status_code: "syncing".to_string(),
                    phase_key: None,
                    summary_key: Some("sync.status.already_running".to_string()),
                    summary_args: std::collections::HashMap::new(),
                    counts: writer_core::api::SyncOperationCountsDto::default(),
                    raw_error: None,
                };
                self.current_sync_operation_state =
                    serde_json::to_string(&state).unwrap_or_default();
                self.sync_action_completed();
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
                counts: writer_core::api::SyncOperationCountsDto::default(),
                raw_error: None,
            };
            self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();
            self.sync_action_completed();
            self.debug_error("sync", "perform_sync_failed", "workspace_empty");
            return op_id.into();
        }

        // per-project sync：每个作品目录是独立 Git 仓库，必须指定作品。
        let project_id = match self.selected_project_id.clone() {
            Some(id) if !id.is_empty() => id,
            _ => {
                self.current_sync_status = "error".to_string();
                let state = writer_core::api::SyncOperationStateDto {
                    operation_id: op_id.clone(),
                    operation_kind: "sync".to_string(),
                    status_code: "error".to_string(),
                    phase_key: None,
                    summary_key: Some("sync.block.no_project_selected".to_string()),
                    summary_args: std::collections::HashMap::new(),
                    counts: writer_core::api::SyncOperationCountsDto::default(),
                    raw_error: None,
                };
                self.current_sync_operation_state =
                    serde_json::to_string(&state).unwrap_or_default();
                self.sync_status_changed();
                self.sync_action_completed();
                self.debug_error("sync", "perform_sync_failed", "no_project_selected");
                return op_id.into();
            }
        };

        if self.current_sync_remote_url.is_empty() {
            self.current_sync_status = "error".to_string();
            let state = writer_core::api::SyncOperationStateDto {
                operation_id: op_id.clone(),
                operation_kind: "sync".to_string(),
                status_code: "error".to_string(),
                phase_key: None,
                summary_key: Some("sync.block.remote_url_missing".to_string()),
                summary_args: std::collections::HashMap::new(),
                counts: writer_core::api::SyncOperationCountsDto::default(),
                raw_error: None,
            };
            self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();
            self.sync_status_changed();
            self.sync_action_completed();
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
                counts: writer_core::api::SyncOperationCountsDto::default(),
                raw_error: None,
            };
            self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();
            self.sync_status_changed();
            self.sync_action_completed();
            self.debug_error("sync", "perform_sync_failed", "token_empty");
            return op_id.into();
        }

        if self.current_sync_branch.is_empty() {
            self.current_sync_branch = "main".to_string();
            self.sync_config_changed();
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
            counts: writer_core::api::SyncOperationCountsDto::default(),
            raw_error: None,
        };
        self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();

        self.current_sync_status = "syncing".to_string();
        self.current_sync_in_progress = true;
        self.sync_status_changed();

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |outcome: SyncTaskOutcome| {
            qptr.as_pinned().map(|this| {
                let mut this = this.borrow_mut();
                this.handle_sync_outcome(outcome);
            });
        });

        let op_id_capture = op_id.clone();
        let project_id_capture = project_id.clone();
        let trigger = trigger.to_string();
        thread::spawn(move || {
            // SAFETY: catch_unwind requires the closure to be UnwindSafe. The closure only captures
            // owned String data (data_root, projects_root, op_id_capture, project_id_capture) which
            // auto-implement UnwindSafe. No shared mutable state or borrows are captured, so the
            // closure is UnwindSafe by auto-impl without needing AssertUnwindSafe.
            let result = std::panic::catch_unwind(|| {
                let api = crate::backend::app_backend::create_core_api(&data_root, &projects_root);
                let mut config = match api.load_sync_config() {
                    Ok(c) => c,
                    Err(e) => {
                        let err_str = e.to_string();
                        let state = writer_core::api::SyncOperationStateDto {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "sync".to_string(),
                            status_code: "error".to_string(),
                            phase_key: None,
                            summary_key: Some("error.load_sync_config_failed".to_string()),
                            summary_args: std::collections::HashMap::new(),
                            counts: writer_core::api::SyncOperationCountsDto::default(),
                            raw_error: Some(mask_sync_error(&err_str)),
                        };
                        return SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            sync_status: "error".to_string(),
                            action_result: serde_json::to_string(&state).unwrap_or_default(),
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

                match api.perform_full_sync(config, trigger == "manual") {
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
                            counts,
                            raw_error: result.error.as_ref().map(|e| mask_sync_error(e)),
                        };

                        SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            sync_status: status_code,
                            action_result: serde_json::to_string(&state).unwrap_or_default(),
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
                            counts: writer_core::api::SyncOperationCountsDto::default(),
                            raw_error: Some(mask_sync_error(&err_str)),
                        };
                        SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            sync_status: cat,
                            action_result: serde_json::to_string(&state).unwrap_or_default(),
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
                        operation_kind: "sync".to_string(),
                        status_code: "fatal_error".to_string(),
                        phase_key: None,
                        summary_key: Some("error.sync_panic".to_string()),
                        summary_args: [("panic_msg".to_string(), panic_msg)].into_iter().collect(),
                        counts: writer_core::api::SyncOperationCountsDto::default(),
                        raw_error: None,
                    };
                    callback(SyncTaskOutcome {
                        operation_id: op_id_capture,
                        sync_status: "fatal_error".to_string(),
                        action_result: serde_json::to_string(&state).unwrap_or_default(),
                    });
                }
            }
        });

        op_id.into()
    }
}

fn format_conflict_files(files: &[String]) -> String {
    if files.is_empty() {
        "sync.result.no_conflict_files".to_string()
    } else {
        let display_files = if files.len() > 100 {
            let mut subset = files[0..100].to_vec();
            subset.push(format!("sync.result.more_files_count: {}", files.len()));
            subset
        } else {
            files.to_vec()
        };
        display_files.join("\n  - ")
    }
}
