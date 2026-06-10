// =============================================================================
// github_init_operations.rs — GitHub 克隆/导入工作区（从 workspace_backend.rs 拆分）
// =============================================================================

use super::*;
use crate::sync_bridge::{
    mask_sync_error, save_sync_configs, sync_error_category, sync_error_category_from_code,
};

impl AppBackend {
    pub(crate) fn execute_github_init(
        &mut self,
        path: QString,
        remote_url: QString,
        branch: QString,
        token: QString,
        proxy_type: QString,
        proxy_host: QString,
        proxy_port: u16,
    ) {
        let path_str = path.to_string();
        let path_obj = std::path::Path::new(&path_str);
        if !path_obj.exists() || !path_obj.is_dir() {
            self.set_error("所选目录不存在或不是目录");
            self.current_sync_operation_state = "所选目录不存在或不是目录".to_string();
            self.sync_action_completed();
            return;
        }

        let remote_url_str = remote_url.to_string();
        if remote_url_str.is_empty() {
            self.set_error("远程仓库地址不能为空");
            self.current_sync_operation_state = "远程仓库地址不能为空".to_string();
            self.sync_action_completed();
            return;
        }

        let branch_str = if branch.to_string().is_empty() {
            "main".to_string()
        } else {
            branch.to_string()
        };
        let token_str = token.to_string();
        let proxy_type_str = proxy_type.to_string();
        let proxy_host_str = proxy_host.to_string();
        let proxy_port_val = proxy_port;

        let op_id = uuid::Uuid::new_v4().to_string();
        self.current_sync_operation_id = op_id.clone();
        self.current_sync_operation_kind = "sync".to_string();

        self.current_sync_status = "syncing".to_string();
        self.sync_status_changed();
        self.current_sync_operation_state = "正在初始化...".to_string();

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |outcome: SyncTaskOutcome| {
            qptr.as_pinned().map(|this| {
                let mut this = this.borrow_mut();
                this.handle_sync_outcome(outcome);
            });
        });

        let op_id_capture = op_id.clone();
        thread::spawn(move || {
            let result = Self::do_github_init(
                &op_id_capture,
                &path_str,
                &remote_url_str,
                &branch_str,
                &token_str,
                &proxy_type_str,
                &proxy_host_str,
                proxy_port_val,
            );
            callback(result);
        });
    }

    pub(crate) fn do_github_init(
        operation_id: &str,
        path: &str,
        remote_url: &str,
        branch: &str,
        token: &str,
        proxy_type: &str,
        proxy_host: &str,
        proxy_port: u16,
    ) -> SyncTaskOutcome {
        use writer_core::sync::{
            sanitize_remote_url, BackendType, SyncConfig, SyncSecrets,
        };

        let parsed = sanitize_remote_url(remote_url);
        let sanitized_url = parsed.sanitized_url;

        let path_obj = std::path::Path::new(path);

        let has_content = || -> bool {
            if let Ok(mut entries) = std::fs::read_dir(path_obj) {
                entries.next().is_some()
            } else {
                false
            }
        };

        let has_workspace = || -> bool {
            WriterCoreApi::new(path)
                .validate_workspace()
                .unwrap_or(false)
        };

        let is_git_repo = || -> bool { path_obj.join(".git").exists() };

        let effective_token = if token.is_empty() {
            parsed.extracted_token.clone()
        } else {
            Some(token.to_string())
        };

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: sanitized_url.clone(),
            transport: writer_core::sync::SyncTransport::HttpsToken,
            branch: branch.to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            proxy_enabled: !proxy_type.is_empty()
                && proxy_type != "none"
                && !proxy_host.is_empty()
                && proxy_port > 0,
            proxy_type: proxy_type.to_string(),
            proxy_host: proxy_host.to_string(),
            proxy_port,
            username: parsed.extracted_username.clone().unwrap_or_default(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let secrets = SyncSecrets {
            token: effective_token,
            ssh_private_key: None,
        };

        let cfg_ref = &config;
        let sec_ref = &secrets;

        if !has_content() {
            let backend = writer_core::sync::create_sync_backend(&config.backend_type);
            match backend.sync(path_obj, &config, &secrets) {
                Ok(result) => {
                    if result.status == writer_core::sync::SyncStatus::Success {
                        let api = WriterCoreApi::new(path);
                        if !api.validate_workspace().unwrap_or(false) {
                            if let Err(e) = api.create_workspace_if_needed() {
                                return SyncTaskOutcome {
                                    operation_id: operation_id.to_string(),
                                    operation_kind: "sync".to_string(),
                                    sync_status: "error".to_string(),
                                    action_result: format!("克隆成功但工作区初始化失败: {}", e),
                                };
                            }
                            let push_backend = writer_core::sync::create_sync_backend(
                                &config.backend_type,
                            );
                            let push_result = push_backend.sync(path_obj, &config, &secrets);
                            let save_first = match &push_result {
                                Ok(r)
                                    if r.status
                                        != writer_core::sync::SyncStatus::Success =>
                                {
                                    true
                                }
                                Err(_) => true,
                                _ => false,
                            };
                            if save_first {
                                let save_outcome = match save_sync_configs(path, cfg_ref, sec_ref) {
                                    Ok(()) => None,
                                    Err(e) => Some(e),
                                };
                                match push_result {
                                    Ok(push_res) => {
                                        let category = push_res.error_category.clone();
                                        let err = push_res.error.unwrap_or_default();
                                        if let Some(se) = save_outcome {
                                            return SyncTaskOutcome {
                                                operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: "error".to_string(),
                                                action_result: format!("工作区已创建，但推送到远端失败: {}，且同步配置保存失败: {}. 请检查权限/磁盘。", mask_sync_error(&err), se),
                                            };
                                        }
                                        return SyncTaskOutcome {
                                            operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: sync_error_category_from_code(category.as_deref(), &err),
                                            action_result: format!("本地工作区已初始化但推送到远端失败: {}. 可在配置同步后手动同步。", mask_sync_error(&err)),
                                        };
                                    }
                                    Err(e) => {
                                        if let Some(se) = save_outcome {
                                            return SyncTaskOutcome {
                                                operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: "error".to_string(),
                                                action_result: format!("工作区已创建，但推送到远端失败: {}，且同步配置保存失败: {}. 请检查权限/磁盘。", mask_sync_error(&e.to_string()), se),
                                            };
                                        }
                                        return SyncTaskOutcome {
                                            operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: sync_error_category(&e.to_string()),
                                            action_result: format!("本地工作区已初始化但推送到远端失败: {}. 可在配置同步后手动同步。", mask_sync_error(&e.to_string())),
                                        };
                                    }
                                }
                            }
                        }
                        match save_sync_configs(path, cfg_ref, sec_ref) {
                            Ok(()) => SyncTaskOutcome {
                                operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: "success".to_string(),
                                action_result: "克隆并初始化工作区成功".to_string(),
                            },
                            Err(e) => SyncTaskOutcome {
                                operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: "error".to_string(),
                                action_result: format!("工作区已创建/同步可能完成，但同步配置保存失败: {}. 请检查权限/磁盘，不要继续同步。", e),
                            },
                        }
                    } else {
                        let err = result.error.unwrap_or_default();
                        SyncTaskOutcome {
                            operation_id: operation_id.to_string(),
                            operation_kind: "sync".to_string(),
                            sync_status: sync_error_category_from_code(
                                result.error_category.as_deref(),
                                &err,
                            ),
                            action_result: format!("克隆失败: {}", mask_sync_error(&err)),
                        }
                    }
                }
                Err(e) => SyncTaskOutcome {
                    operation_id: operation_id.to_string(),
                    operation_kind: "sync".to_string(),
                    sync_status: sync_error_category(&e.to_string()),
                    action_result: format!("克隆失败: {}", mask_sync_error(&e.to_string())),
                },
            }
        } else if has_workspace() {
            let backend = writer_core::sync::create_sync_backend(&config.backend_type);
            match backend.sync(path_obj, &config, &secrets) {
                Ok(result) => {
                    if result.status == writer_core::sync::SyncStatus::Success {
                        match save_sync_configs(path, cfg_ref, sec_ref) {
                            Ok(()) => SyncTaskOutcome {
                                operation_id: operation_id.to_string(),
                                operation_kind: "sync".to_string(),
                                sync_status: "success".to_string(),
                                action_result: "远程仓库已配置并同步成功".to_string(),
                            },
                            Err(e) => SyncTaskOutcome {
                                operation_id: operation_id.to_string(),
                                operation_kind: "sync".to_string(),
                                sync_status: "error".to_string(),
                                action_result: format!(
                                    "同步成功但配置保存失败: {}. 请检查权限/磁盘，不要继续同步。",
                                    e
                                ),
                            },
                        }
                    } else if result.status == writer_core::sync::SyncStatus::Conflict {
                        let mut files = result
                            .conflicts
                            .iter()
                            .map(|c| c.local_path.clone())
                            .collect::<Vec<_>>();
                        if let Some(summary) = &result.conflict_summary {
                            for f in &summary.conflicted_files {
                                files.push(f.clone());
                            }
                        }
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

                        let masked_err = result
                            .error
                            .as_deref()
                            .map(mask_sync_error)
                            .unwrap_or_else(|| "None".to_string());
                        debug_log_static(
                            "sync",
                            "conflict_detected",
                            &format!(
                                "conflicted file count={}, masked error={}",
                                files.len(),
                                masked_err
                            ),
                        );

                        let m = format!(
                            "同步冲突，已停止，未覆盖任何文件\n\n原因:\n本地和远端都修改了同一批同步文件，Git 无法安全自动合并。\n\n冲突文件:\n  - {}\n\n下一步建议:\n1. 先备份当前工作区\n2. 运行诊断确认网络认证正常\n3. 手动处理冲突后重新同步",
                            file_str
                        );
                        SyncTaskOutcome {
                            operation_id: operation_id.to_string(),
                            operation_kind: "sync".to_string(),
                            sync_status: "conflict".to_string(),
                            action_result: m,
                        }
                    } else {
                        let err = result.error.unwrap_or_default();
                        let cat =
                            sync_error_category_from_code(result.error_category.as_deref(), &err);
                        let action_result = if cat == "conflict" {
                            debug_log_static(
                                "sync",
                                "conflict_detected",
                                &format!(
                                    "conflicted file count=unknown, masked error={}",
                                    mask_sync_error(&err)
                                ),
                            );
                            format!(
                                "同步冲突，已停止，未覆盖任何文件\n\n原因:\n本地和远端都修改了同一批同步文件，Git 无法安全自动合并。\n\n冲突文件:\n  - 未能列出具体冲突文件\n\n下一步建议:\n1. 先备份当前工作区\n2. 运行诊断确认网络认证正常\n3. 手动处理冲突后重新同步\n\n(原始错误: {})",
                                mask_sync_error(&err)
                            )
                        } else {
                            format!("同步失败: {}", mask_sync_error(&err))
                        };
                        SyncTaskOutcome {
                            operation_id: operation_id.to_string(),
                            operation_kind: "sync".to_string(),
                            sync_status: cat,
                            action_result,
                        }
                    }
                }
                Err(e) => {
                    let err_str = e.to_string();
                    let cat = sync_error_category(&err_str);
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
                        format!("同步失败: {}", mask_sync_error(&err_str))
                    };
                    SyncTaskOutcome {
                        operation_id: operation_id.to_string(),
                        operation_kind: "sync".to_string(),
                        sync_status: cat,
                        action_result,
                    }
                }
            }
        } else if is_git_repo() {
            SyncTaskOutcome {
                operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: "error".to_string(),
                action_result: "目录包含 Git 仓库但不是素笺写作工作区。请先新建本地工作区（新建作品后保存），再配置同步。".to_string(),
            }
        } else {
            SyncTaskOutcome {
                operation_id: operation_id.to_string(),
                operation_kind: "sync".to_string(),
                sync_status: "error".to_string(),
                action_result: "目录非空且不是素笺写作工作区。请选择空目录，或先新建本地工作区。"
                    .to_string(),
            }
        }
    }
}
