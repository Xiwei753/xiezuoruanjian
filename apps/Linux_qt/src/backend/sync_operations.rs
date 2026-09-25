// =============================================================================
// sync_operations.rs — 同步执行逻辑
// =============================================================================
//
// 引用了什么：
// - super::*：引入 AppBackend 核心后端的全部方法与结构体。
// - crate::sync_bridge：引入 SyncTaskOutcome、mask_sync_error 等同步工具函数。
// - writer_core::api::WriterCoreApi：核心库对外的统一 API 入口。
//
// 干什么的：
// - 实现 AppBackend 上的同步执行方法：perform_sync、perform_sync_dry_run、perform_sync_internal。
// - 实现同步结果处理：handle_sync_outcome、handle_sync_content_refresh。
// - 所有同步操作通过 UUID operation_id 机制保证并发安全，通过 QPointer + queued_callback 实现线程安全回调。
//
// 被什么引用：
// - 被 apps/Linux_qt/src/backend/app_backend/sync_backend.rs 中的 SyncBackend QObject 间接调用。
// =============================================================================

use super::SyncBackend;
use super::*;
use crate::sync_bridge::{mask_sync_error, sync_error_category_from_code, SyncTaskOutcome};

/// 同步结果对当前工作区内容的影响。
///
/// `handle_sync_outcome` 返回此枚举，调用方（SyncBackend::handle_outcome）
/// 据此决定是否发 `sync_content_applied` signal。
/// - `ContentChanged`：同步确实修改/重新加载了工作区内容（success、
///   branch_missing_recovered、冲突后 tree reload），QML 需刷新正文/树。
/// - `StatusOnly`：只更新同步状态，不刷新工作区内容（过期回调、diagnostics、
///   dry_run、配置保存、error 等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SyncOutcomeEffect {
    StatusOnly,
    ContentChanged,
}

/// 构造异步同步结果的 callback。
///
/// 如果提供 `sync_qptr`（来自 SyncBackend），callback 通过 SyncBackend::handle_outcome
/// 进入，走 with_app_mut 刷新 DomainSnapshot 并发 SyncBackend signal。
/// 否则（测试场景）回退到 QPointer<AppBackend> 直接 handle_sync_outcome。
pub(super) fn make_outcome_callback(
    app_qptr: QPointer<AppBackend>,
    sync_qptr: Option<QPointer<SyncBackend>>,
) -> Box<dyn FnOnce(SyncTaskOutcome) + Send> {
    if let Some(sq) = sync_qptr {
        Box::new(qmetaobject::queued_callback(
            move |outcome: SyncTaskOutcome| {
                sq.as_pinned().map(|this| {
                    let mut this = this.borrow_mut();
                    this.handle_outcome(outcome);
                });
            },
        ))
    } else {
        Box::new(qmetaobject::queued_callback(
            move |outcome: SyncTaskOutcome| {
                app_qptr.as_pinned().map(|this| {
                    let mut this = this.borrow_mut();
                    // 测试回退路径：无 SyncBackend，effect 无消费者，忽略返回值。
                    let _ = this.handle_sync_outcome(outcome, None);
                });
            },
        ))
    }
}

impl AppBackend {
    pub(crate) fn handle_sync_outcome(
        &mut self,
        outcome: SyncTaskOutcome,
        sync_qptr: Option<QPointer<SyncBackend>>,
    ) -> SyncOutcomeEffect {
        if outcome.operation_id != self.current_sync_operation_id {
            self.debug_log(
                "sync",
                "sync_outcome_discarded",
                &format!(
                    "Discarded outdated outcome. Expected: {}, got: {}",
                    self.current_sync_operation_id, outcome.operation_id
                ),
            );
            return SyncOutcomeEffect::StatusOnly;
        }

        // Issue #729：workspace generation 身份校验。
        // 同步线程启动时捕获当时的 generation 并填入 outcome。若回调到达时
        // current_workspace_generation 已变（切工作区/reset_workspace_state 递增），
        // 说明此结果属于旧工作区，必须丢弃，避免旧同步污染新工作区状态。
        if outcome.workspace_generation != self.current_workspace_generation {
            self.debug_log(
                "sync",
                "sync_outcome_discarded_workspace_changed",
                &format!(
                    "Discarded outcome from stale workspace. expected_gen={}, got_gen={}",
                    self.current_workspace_generation, outcome.workspace_generation
                ),
            );
            // 旧工作区的回调不应清新工作区的 in_progress（reset_workspace_state 已清）。
            return SyncOutcomeEffect::StatusOnly;
        }

        // Issue #729 评论 5763441474：data_root 身份校验。
        // 同步线程启动时捕获当时的 data_root 并填入 outcome。若回调到达时
        // current_data_root 已变（切工作区），说明此结果属于旧工作区，必须丢弃。
        // operation_id + workspace_generation + data_root 三者同时匹配才接受结果。
        if outcome.data_root != self.current_data_root {
            self.debug_log(
                "sync",
                "sync_outcome_discarded_data_root_changed",
                &format!(
                    "Discarded outcome from stale data_root. expected={}, got={}",
                    self.current_data_root, outcome.data_root
                ),
            );
            return SyncOutcomeEffect::StatusOnly;
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
        let status_str = outcome.sync_status.as_str();

        // Issue #754 评论 5814866116 改动2: handle_sync_outcome 返回 SyncOutcomeEffect，
        // 由 SyncBackend::handle_outcome 据此决定是否发 sync_content_applied。
        // 改动3: github init 旧路径已删除，不再有 pending_github_init_path 特判。
        let sync_success = matches!(status_str, "success" | "branch_missing_recovered");
        // Issue #754 评论 5816573119: 所有"内容已变化"的同步结果统一走同一个刷新入口，
        // 确保 reload_tree + reconcile_selection_after_tree_reload 成对执行。
        // conflict/partial_conflict/unrelated_histories 也会真实修改本地工作区内容
        // （Core issue_644: PartialConflict 时安全完成的非冲突文件继续提交到 live），
        // 若包含远端删除/结构变化，selection 必须同步 reconcile，否则
        // selected_project_id/selected_volume_id/selected_chapter_id 可能指向已删除对象。
        let content_changed = self.has_workspace()
            && (sync_success
                || status_str == "conflict"
                || status_str == "partial_conflict"
                || status_str == "unrelated_histories");

        let effect = if content_changed {
            self.handle_sync_content_refresh();
            SyncOutcomeEffect::ContentChanged
        } else {
            SyncOutcomeEffect::StatusOnly
        };

        // 当前同步任务真正结束并把 busy 清掉后，检查 manual_sync_pending。
        // 为 true 时先清 flag，再启动一次 manual sync。
        // 排队的同步还没执行，当前 outcome 的 effect 才是返回值。
        if self.manual_sync_pending {
            self.manual_sync_pending = false;
            self.debug_log(
                "sync",
                "manual_sync_pending_triggered",
                "starting queued manual sync after previous sync completed",
            );
            self.perform_sync_internal("manual", false, sync_qptr);
        }
        effect
    }

    pub(crate) fn handle_sync_content_refresh(&mut self) {
        self.reload_tree();
        let chapter_deleted = self.reconcile_selection_after_tree_reload();
        // Issue #754 评论 5815901258: trigger_projects_reloaded 已删除，
        // 领域通知由 ProjectBackend::emit_changed() 负责。
        if chapter_deleted {
            self.current_save_status = "chapter.deleted_remotely_refreshed".to_string();
        }

        self.debug_log("sync", "sync_refresh_applied", "tree_reloaded=true");
    }

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

    pub(crate) fn perform_sync(&mut self, sync_qptr: Option<QPointer<SyncBackend>>) -> QString {
        self.perform_sync_internal("manual", false, sync_qptr)
    }

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
            counts: writer_core::api::SyncOperationCountsDto::default(),
            raw_error: None,
        };
        self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();

        self.current_sync_status = "syncing".to_string();
        self.current_sync_in_progress = true;
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
            // snapshot, and progress_callback (Option<Arc<dyn Fn + Send + Sync>>). Arc<dyn Fn>
            // is not RefUnwindSafe (dyn Fn lacks RefUnwindSafe bound), so the closure is wrapped
            // in AssertUnwindSafe to satisfy catch_unwind's UnwindSafe bound. AssertUnwindSafe
            // is std's safe wrapper (not unsafe impl), no hand-written unsafe.
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

                // Issue #762 评论 5826175490 第 5 点：progress callback 在主线程构造，
                // 闭包用 AssertUnwindSafe 包装，progress_callback.as_ref() 直接传入。
                match api.perform_full_sync(
                    config,
                    trigger == "manual",
                    cancel_token.clone(),
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
