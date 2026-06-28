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
// - 被 apps/desktop/src/backend/app_backend/sync_backend.rs 中的 SyncBackend QObject 间接调用。
// =============================================================================

use super::*;
use crate::sync_bridge::{
    mask_sync_error, sync_error_category, sync_error_category_from_code, SyncTaskOutcome,
};

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
                self.internal_open_workspace(&pending_path, false);
                self.load_sync_config();
                return;
            }
        }

        let sync_success = matches!(status_str, "success" | "branch_missing_recovered");
        if sync_success && self.has_workspace() {
            self.handle_successful_sync_refresh();
        } else if (status_str == "conflict" || status_str == "unrelated_histories")
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
            self.current_save_status = "当前章节已在其他设备删除，已刷新列表。".to_string();
            self.save_status_changed();
            self.current_sync_operation_state
                .push_str("\n\n当前章节已在其他设备删除，已刷新列表。");
            self.sync_action_completed();
        }

        self.debug_log("sync", "sync_refresh_applied", "tree_reloaded=true");
    }

    pub(crate) fn can_start_auto_sync(&self, reason: &str, min_gap_secs: i64) -> bool {
        if self.current_sync_in_progress {
            return false;
        }
        if !self.current_has_workspace || !self.current_sync_enabled || !self.current_sync_auto_sync
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
        let workspace_path = self.current_workspace.clone();
        if workspace_path.is_empty() {
            self.current_sync_operation_state = "请先打开工作区".to_string();
            self.sync_action_completed();
            return "".into();
        }

        if self.current_sync_remote_url.is_empty() {
            self.current_sync_status = "error".to_string();
            self.current_sync_operation_state = "同步检查失败: 未配置远程仓库 URL".to_string();
            self.sync_status_changed();
            self.sync_action_completed();
            return "".into();
        }

        if self.current_sync_token.is_empty() {
            self.current_sync_status = "error".to_string();
            self.current_sync_operation_state =
                "同步检查失败: 未配置 GitHub 访问令牌 (Token)".to_string();
            self.sync_status_changed();
            self.sync_action_completed();
            return "".into();
        }

        if self.current_sync_branch.is_empty() {
            self.current_sync_branch = "main".to_string();
            self.sync_config_changed();
        }

        let op_id = uuid::Uuid::new_v4().to_string();
        self.current_sync_operation_id = op_id.clone();
        self.current_sync_operation_kind = "dry_run".to_string();

        self.current_sync_status = "syncing".to_string();
        self.sync_status_changed();
        self.current_sync_operation_state = "正在检查同步计划...".to_string();

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |outcome: SyncTaskOutcome| {
            qptr.as_pinned().map(|this| {
                let mut this = this.borrow_mut();
                this.handle_sync_outcome(outcome);
            });
        });

        let op_id_capture = op_id.clone();
        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let api = WriterCoreApi::new(&workspace_path);
                let mut config = match api.load_sync_config() {
                    Ok(c) => c,
                    Err(e) => {
                        return SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "dry_run".to_string(),
                            sync_status: "error".to_string(),
                            action_result: format!("无法加载同步配置: {}", e),
                        };
                    }
                };
                config.android_has_internet_permission = true;
                config.android_has_access_network_state_permission = true;

                match api.perform_sync_dry_run(config) {
                    Ok(plan) => {
                        let mut msg = String::new();
                        msg.push_str("同步计划检查完成\n");
                        msg.push_str(&format!(
                            "需要上传的文件数: {}\n",
                            plan.files_to_upload.len()
                        ));
                        msg.push_str(&format!(
                            "需要下载的文件数: {}\n",
                            plan.files_to_download.len()
                        ));
                        msg.push_str(&format!(
                            "本地待删除的文件数: {}\n",
                            plan.files_to_delete_local.len()
                        ));
                        msg.push_str(&format!(
                            "远程待删除的文件数: {}\n",
                            plan.files_to_delete_remote.len()
                        ));

                        if !plan.files_to_upload.is_empty() {
                            msg.push_str("\n将要上传的文件:\n");
                            for f in plan.files_to_upload.iter().take(10) {
                                msg.push_str(&format!("  - {}\n", f));
                            }
                            if plan.files_to_upload.len() > 10 {
                                msg.push_str("  ... 更多文件省略\n");
                            }
                        }
                        SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "dry_run".to_string(),
                            sync_status: "dry_run_success".to_string(),
                            action_result: msg,
                        }
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        let cat = sync_error_category_from_code(None, &err_str);
                        SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "dry_run".to_string(),
                            sync_status: cat,
                            action_result: format!(
                                "检查同步计划失败: {}",
                                mask_sync_error(&err_str)
                            ),
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
                        "未知 Panic".to_string()
                    };
                    callback(SyncTaskOutcome {
                        operation_id: op_id_capture,
                        operation_kind: "dry_run".to_string(),
                        sync_status: "fatal_error".to_string(),
                        action_result: format!("检查同步计划发生致命错误 (Panic):\n{}", panic_msg),
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
        if !self.current_has_workspace
            || !self.current_sync_auto_sync
            || self.current_sync_in_progress
        {
            return;
        }
        let interval_secs = self.current_sync_interval.max(60) as i64;
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
        if self.current_sync_in_progress {
            self.debug_log("sync", "perform_sync_skipped", "sync already running");
            if trigger == "manual" {
                self.current_sync_operation_state = "同步正在进行中，请稍候。".to_string();
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
        let workspace_path = self.current_workspace.clone();
        if workspace_path.is_empty() {
            self.current_sync_operation_state = "请先打开工作区".to_string();
            self.sync_action_completed();
            self.debug_error("sync", "perform_sync_failed", "workspace_empty");
            return "".into();
        }

        if self.current_sync_remote_url.is_empty() {
            self.current_sync_status = "error".to_string();
            self.current_sync_operation_state =
                "同步失败: 未配置远程仓库 URL，请先填写并保存配置。".to_string();
            self.sync_status_changed();
            self.sync_action_completed();
            self.debug_error("sync", "perform_sync_failed", "remote_url_empty");
            return "".into();
        }

        if self.current_sync_token.is_empty() {
            self.current_sync_status = "error".to_string();
            self.current_sync_operation_state =
                "同步失败: 未配置 GitHub 访问令牌 (Token)，请先填写并保存配置。".to_string();
            self.sync_status_changed();
            self.sync_action_completed();
            self.debug_error("sync", "perform_sync_failed", "token_empty");
            return "".into();
        }

        if self.current_sync_branch.is_empty() {
            self.current_sync_branch = "main".to_string();
            self.sync_config_changed();
        }

        let op_id = uuid::Uuid::new_v4().to_string();
        self.current_sync_operation_id = op_id.clone();
        self.current_sync_operation_kind = "sync".to_string();

        self.flush_writing_stats();
        self.current_sync_operation_state = if silent_success {
            "后台同步中...\n正在拉取远端清单\n正在比较本地和远端\n正在下载远端较新文件\n正在上传本地较新文件".to_string()
        } else {
            "正在同步...\n正在拉取远端清单\n正在比较本地和远端\n正在下载远端较新文件\n正在上传本地较新文件".to_string()
        };
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
        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let api = WriterCoreApi::new(&workspace_path);
                let mut config = match api.load_sync_config() {
                    Ok(c) => c,
                    Err(e) => {
                        return SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "sync".to_string(),
                            sync_status: "error".to_string(),
                            action_result: format!("无法读取同步配置: {}", e),
                        };
                    }
                };
                config.android_has_internet_permission = true;
                config.android_has_access_network_state_permission = true;

                let backend_label = config.backend_type.clone();
                debug_log_static(
                    "sync",
                    "perform_sync_backend",
                    &format!("backend_type={}, sync_mode=lww_manifest", backend_label),
                );

                match api.perform_sync(config) {
                    Ok(result) => {
                        let (status, msg) = match result.status.as_str() {
                            "success" => {
                                let mut m = format!(
                                    "同步成功\n上传: {} 个文件\n下载: {} 个文件\n本地删除: {} 个文件\n远端删除: {} 个文件\n覆盖: {} 个文件\n跳过: {} 个文件",
                                    result.uploaded_files.len(),
                                    result.downloaded_files.len(),
                                    result.local_deletes.len(),
                                    result.remote_deletes.len(),
                                    result.overwritten_files.len(),
                                    result.ignored_files.len()
                                );
                                ("success".to_string(), m)
                            }
                            "latest_wins_applied" => {
                                let mut m = format!(
                                    "同步完成 (已自动按最新时间选择版本)\n\n上传: {} 个文件\n下载: {} 个文件\n本地删除: {} 个文件\n远端删除: {} 个文件\n覆盖: {} 个文件\n跳过: {} 个文件",
                                    result.uploaded_files.len(),
                                    result.downloaded_files.len(),
                                    result.local_deletes.len(),
                                    result.remote_deletes.len(),
                                    result.overwritten_files.len(),
                                    result.ignored_files.len()
                                );
                                ("success".to_string(), m)
                            }
                            "no_changes" => {
                                let mut m = format!(
                                    "同步完成：本地和远端均已是最新状态，无须更新。\n\n上传: {} 个文件\n下载: {} 个文件\n本地删除: {} 个文件\n远端删除: {} 个文件\n覆盖: {} 个文件\n跳过: {} 个文件",
                                    result.uploaded_files.len(),
                                    result.downloaded_files.len(),
                                    result.local_deletes.len(),
                                    result.remote_deletes.len(),
                                    result.overwritten_files.len(),
                                    result.ignored_files.len()
                                );
                                ("success".to_string(), m)
                            }
                            "configured_untested" => {
                                ("configured_untested".to_string(), "同步配置已加载，尚未测试或执行同步。".to_string())
                            }
                            "conflict" => {
                                let mut files = result.conflicts.iter().map(|c| c.local_path.clone()).collect::<Vec<_>>();
                                files.sort();
                                files.dedup();

                                let file_str = if files.is_empty() {
                                    "未能列出具体冲突文件".to_string()
                                } else {
                                    let display_files = if files.len() > 100 {
                                        let mut subset = files[0..100].to_vec();
                                        subset.push(format!("...等共 {} 个文件", files.len()));
                                        subset
                                    } else {
                                        files.clone()
                                    };
                                    display_files.join("\n  - ")
                                };

                                let masked_err = result.error.as_deref().map(mask_sync_error).unwrap_or_else(|| "None".to_string());
                                debug_log_static("sync", "conflict_detected", &format!("conflicted file count={}, masked error={}", files.len(), masked_err));

                                let m = format!(
                                    "同步冲突，已停止，未覆盖任何文件\n\n原因:\n本地和远端都修改了同一批同步文件，Git 无法安全自动合并。\n\n冲突文件:\n  - {}\n\n下一步建议:\n1. 先备份当前工作区\n2. 运行诊断确认网络认证正常\n3. 手动处理冲突后重新同步",
                                    file_str
                                );
                                ("conflict".to_string(), m)
                            }
                            "recoverable_error" => {
                                let e = result.error.as_deref().unwrap_or("未知错误");
                                ("recoverable_error".to_string(), format!("可恢复的同步错误:\n{}\n请检查后重试。", mask_sync_error(e)))
                            }
                            "fatal_error" => {
                                let e = result.error.as_deref().unwrap_or("未知错误");
                                ("fatal_error".to_string(), format!("严重同步错误:\n{}\n建议备份数据并重新配置。", mask_sync_error(e)))
                            }
                            "dirty_repo_blocked" => {
                                ("dirty_repo_blocked".to_string(), "同步被阻止: 本地工作区存在未跟踪或未提交的修改，且这些修改不是同步安全文件。".to_string())
                            }
                            "branch_missing_recovered" => {
                                ("branch_missing_recovered".to_string(), "同步成功 (分支已恢复)\n已自动恢复并关联本地与远端分支。".to_string())
                            }
                            "error" => {
                                let e = result.error.as_deref().unwrap_or("未知错误");
                                let cat = sync_error_category_from_code(result.error_category.as_deref(), e);
                                let m = if cat == "conflict" {
                                    debug_log_static("sync", "conflict_detected", &format!("conflicted file count=unknown, masked error={}", mask_sync_error(e)));
                                    format!(
                                        "同步冲突，已停止，未覆盖任何文件\n\n原因:\n本地和远端都修改了同一批同步文件，Git 无法安全自动合并。\n\n冲突文件:\n  - 未能列出具体冲突文件\n\n下一步建议:\n1. 先备份当前工作区\n2. 运行诊断确认网络认证正常\n3. 手动处理冲突后重新同步\n\n(原始错误: {})",
                                        mask_sync_error(e)
                                    )
                                } else {
                                    format!("同步失败:\n{}", mask_sync_error(e))
                                };
                                (cat, m)
                            }
                            "idle" => {
                                ("configured_untested".to_string(), "同步未执行".to_string())
                            }
                            other => {
                                ("error".to_string(), format!("同步状态: {}", other))
                            }
                        };

                        SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "sync".to_string(),
                            sync_status: status.to_string(),
                            action_result: msg,
                        }
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        let err_code = e.code();
                        let cat = match err_code {
                            "SYNC_CONFLICT" => "conflict".to_string(),
                            "SYNC_FAILED" => sync_error_category_from_code(None, &err_str),
                            _ => sync_error_category(&err_str),
                        };
                        let action_result = if cat == "conflict" {
                            debug_log_static(
                                "sync",
                                "conflict_detected",
                                &format!(
                                    "conflicted file count=unknown, masked error={}",
                                    mask_sync_error(&err_str)
                                ),
                            );
                            format!(
                                "同步冲突，已停止，未覆盖任何文件\n\n原因:\n本地和远端都修改了同一批同步文件，Git 无法安全自动合并。\n\n冲突文件:\n  - 未能列出具体冲突文件\n\n下一步建议:\n1. 先备份当前工作区\n2. 运行诊断确认网络认证正常\n3. 手动处理冲突后重新同步\n\n(原始错误: {})",
                                mask_sync_error(&err_str)
                            )
                        } else {
                            format!("同步操作失败:\n{}", mask_sync_error(&err_str))
                        };
                        SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "sync".to_string(),
                            sync_status: cat,
                            action_result,
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
                        "未知 Panic".to_string()
                    };
                    callback(SyncTaskOutcome {
                        operation_id: op_id_capture,
                        operation_kind: "sync".to_string(),
                        sync_status: "fatal_error".to_string(),
                        action_result: format!("同步执行发生致命错误 (Panic):\n{}", panic_msg),
                    });
                }
            }
        });

        op_id.into()
    }
}
