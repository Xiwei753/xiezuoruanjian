use super::*;

impl AppBackend {
// Included inside impl AppBackend from app_backend.rs.
// Deprecated compatibility methods for this Linux backend domain.

// AppBackend::has_workspace
    pub(crate) fn has_workspace(&self) -> bool {
        self.current_has_workspace
    }

// AppBackend::pending_github_init_path
    pub(crate) fn pending_github_init_path(&self) -> QString {
        self.current_pending_github_init_path.clone().into()
    }

// AppBackend::get_workspace_diagnostics
    pub(crate) fn get_workspace_diagnostics(&self) -> QString {
        use std::path::Path;
        let ws_path = &self.current_workspace;
        let path_obj = Path::new(ws_path);
        let path_exists = path_obj.exists();
        let is_dir = path_obj.is_dir();
        let manifest_path = path_obj.join("workspace_manifest.json");
        let manifest_exists = manifest_path.exists();
        let projects_path = path_obj.join("projects");
        let projects_dir_exists = projects_path.exists();
        let app_meta_exists = path_obj.join("app-meta").exists();
        let validate_workspace = if path_exists && path_obj.is_dir() {
            WriterCoreApi::new(ws_path).validate_workspace().unwrap_or(false)
        } else {
            false
        };
        let core_initialized = self.current_has_workspace && !self.current_workspace.is_empty();
        let last_workspace = writer_core::app_config::get_last_workspace_path().unwrap_or_default();
        // Real writable test: try to create and delete a temp file
        let (writable, writable_error) = if path_exists && path_obj.is_dir() {
            let test_file = path_obj.join(".writer_write_test");
            match std::fs::write(&test_file, b"test") {
                Ok(()) => {
                    let _ = std::fs::remove_file(&test_file);
                    (true, String::new())
                }
                Err(e) => (false, format!("{}", e)),
            }
        } else {
            (false, "path does not exist or is not a directory".to_string())
        };
        let create_project_available = self.current_has_workspace
            && core_initialized
            && validate_workspace
            && path_exists
            && is_dir
            && manifest_exists
            && projects_dir_exists
            && writable;
        let diag = serde_json::json!({
            "hasWorkspace": self.current_has_workspace,
            "workspacePath": ws_path,
            "coreInitialized": core_initialized,
            "pathExists": path_exists,
            "isDir": is_dir,
            "manifestPath": manifest_path.to_string_lossy(),
            "manifestExists": manifest_exists,
            "projectsPath": projects_path.to_string_lossy(),
            "projectsDirExists": projects_dir_exists,
            "appMetaExists": app_meta_exists,
            "writable": writable,
            "writableError": writable_error,
            "validateWorkspace": validate_workspace,
            "treeCount": self.cached_tree.len(),
            "lastWorkspacePath": last_workspace,
            "createProjectAvailable": create_project_available,
        });
        diag.to_string().into()
    }

// AppBackend::try_restore_last_workspace
    pub(crate) fn try_restore_last_workspace(&mut self) {
        self.debug_log("workspace", "try_restore_last_workspace_start", "");
        if let Some(path) = writer_core::app_config::get_last_workspace_path() {
            self.debug_log("workspace", "try_restore_last_workspace_path_found", &format!("path={}", path));
            let path_obj = std::path::Path::new(&path);
            if path_obj.exists() && path_obj.is_dir() {
                let api = WriterCoreApi::new(&path);
                let val_res = api.validate_workspace().unwrap_or(false);
                self.debug_log("workspace", "try_restore_last_workspace_validate", &format!("path={}, is_valid={}", path, val_res));
                if val_res {
                    self.current_workspace = path.clone();
                    self.current_has_workspace = true;
                    self.current_save_status = "已保存".to_string();
                    self.save_status_changed();
                    self.reload_tree();
                    self.load_sync_config();
                    self.load_local_settings();
                    self.ai_available_changed();
                    self.workspace_opened();
                    self.workspace_content_changed();
                    self.workspace_state_changed();
                    self.debug_log("workspace", "try_restore_last_workspace_success", &format!("path={}", path));
                    return;
                }
            }
            // Restore failed: clear the invalid lastWorkspacePath to avoid being stuck
            self.debug_warn("workspace", "try_restore_last_workspace_failed_clearing", &format!("path={}", path));
            let _ = writer_core::app_config::clear_last_workspace_path();
        } else {
            self.debug_log("workspace", "try_restore_last_workspace_no_path", "");
        }
        // No valid workspace to restore
        self.current_has_workspace = false;
        self.current_sync_status = "not_configured".to_string();
        self.sync_status_changed();
        self.workspace_state_changed();
        // Load app-level theme mode even without workspace
        self.load_app_theme_mode();
        self.ai_available_changed();
    }

// AppBackend::internal_open_workspace
    pub(crate) fn internal_open_workspace(&mut self, path: &str, initialize: bool) {
        self.debug_log("workspace", "internal_open_workspace_start", &format!("path={}, initialize={}", path, initialize));
        let path_obj = std::path::Path::new(path);
        if !path_obj.exists() || !path_obj.is_dir() {
            let err_msg = format!("路径不存在或不是目录: {}", path);
            self.set_error(&err_msg);
            self.debug_error("workspace", "internal_open_workspace_failed", &err_msg);
            return;
        }

        let api = WriterCoreApi::new(path);
        let is_valid = api.validate_workspace().unwrap_or(false);
        self.debug_log("workspace", "internal_open_workspace_validate", &format!("path={}, is_valid={}", path, is_valid));

        if !is_valid && !initialize {
            let err_msg = "不是有效工作区。请选择其他目录，或使用「新建工作区」初始化该目录。";
            self.set_error(err_msg);
            self.debug_error("workspace", "internal_open_workspace_failed", err_msg);
            return;
        }

        if !is_valid && initialize {
            self.debug_log("workspace", "internal_open_workspace_creating", path);
            if let Err(e) = api.create_workspace_if_needed() {
                let err_msg = format!("无法创建工作区: {}", e);
                self.set_error(&err_msg);
                self.debug_error("workspace", "internal_open_workspace_failed", &err_msg);
                return;
            }
        }

        let val_res = api.validate_workspace().unwrap_or(false);
        self.debug_log("workspace", "internal_open_workspace_revalidate", &format!("path={}, is_valid={}", path, val_res));
        if !val_res {
            let err_msg = "工作区验证失败";
            self.set_error(err_msg);
            self.debug_error("workspace", "internal_open_workspace_failed", err_msg);
            return;
        }

        self.current_workspace = path.to_string();
        self.current_has_workspace = true;
        self.current_save_status = "已保存".to_string();
        self.save_status_changed();
        self.reload_tree();
        self.load_sync_config();
        self.load_local_settings();
        self.workspace_opened();
        self.workspace_content_changed();
        self.workspace_state_changed();

        let _ = writer_core::app_config::set_last_workspace_path(path);
        self.debug_log("workspace", "internal_open_workspace_success", &format!("path={}", path));
    }

// AppBackend::create_new_workspace
    pub(crate) fn create_new_workspace(&mut self) {
        self.debug_log("workspace", "create_new_workspace_clicked", "");
        if let Some(path) = FileDialog::new().pick_folder() {
            self.internal_open_workspace(&path.to_string_lossy(), true);
        } else {
            self.debug_log("workspace", "create_new_workspace_cancelled", "");
        }
    }

// AppBackend::open_existing_workspace
    pub(crate) fn open_existing_workspace(&mut self) {
        self.debug_log("workspace", "open_existing_workspace_clicked", "");
        if let Some(path) = FileDialog::new().pick_folder() {
            self.internal_open_workspace(&path.to_string_lossy(), false);
        } else {
            self.debug_log("workspace", "open_existing_workspace_cancelled", "");
        }
    }

// AppBackend::close_workspace
    pub(crate) fn close_workspace(&mut self) {
        self.debug_log("workspace", "close_workspace_start", "");
        self.flush_writing_stats();
        // Clear workspace state
        self.current_workspace = "".to_string();
        self.current_has_workspace = false;
        // Clear selection state
        self.selected_project_id = None;
        self.selected_volume_id = None;
        self.selected_chapter_id = None;
        // Clear tree
        self.cached_tree = QJsonArray::default();
        // Reset sync status
        self.current_sync_status = "not_configured".to_string();
        // Reset save status
        self.current_save_status = "未打开工作区".to_string();
        self.save_status_changed();
        // Clear editor
        self.clear_editor();
        // Emit signals
        self.workspace_state_changed();
        self.trigger_projects_reloaded();
        self.sync_status_changed();
        self.debug_log("workspace", "close_workspace_success", "");
    }

// AppBackend::clear_last_workspace
    pub(crate) fn clear_last_workspace(&mut self) {
        let _ = writer_core::app_config::clear_last_workspace_path();
    }

// AppBackend::switch_workspace
    pub(crate) fn switch_workspace(&mut self) {
        self.close_workspace();
        self.clear_last_workspace();
    }

// AppBackend::init_workspace_from_github
    pub(crate) fn init_workspace_from_github(&mut self) {
        if let Some(path) = FileDialog::new().pick_folder() {
            self.current_pending_github_init_path = path.to_string_lossy().to_string();
            self.pending_github_init_path_changed();
        }
    }

// AppBackend::execute_github_init
    pub(crate) fn execute_github_init(&mut self, path: QString, remote_url: QString, branch: QString, token: QString, proxy_type: QString, proxy_host: QString, proxy_port: u16) {
        let path_str = path.to_string();
        let path_obj = std::path::Path::new(&path_str);
        if !path_obj.exists() || !path_obj.is_dir() {
            self.set_error("所选目录不存在或不是目录");
            self.current_sync_action_result = "所选目录不存在或不是目录".to_string();
            self.sync_action_completed();
            return;
        }

        let remote_url_str = remote_url.to_string();
        if remote_url_str.is_empty() {
            self.set_error("远程仓库地址不能为空");
            self.current_sync_action_result = "远程仓库地址不能为空".to_string();
            self.sync_action_completed();
            return;
        }

        let branch_str = if branch.to_string().is_empty() { "main".to_string() } else { branch.to_string() };
        let token_str = token.to_string();
        let proxy_type_str = proxy_type.to_string();
        let proxy_host_str = proxy_host.to_string();
        let proxy_port_val = proxy_port;

        self.current_sync_status = "syncing".to_string();
        self.sync_status_changed();
        self.current_sync_action_result = "正在初始化...".to_string();

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |outcome: SyncTaskOutcome| {
            qptr.as_pinned().map(|this| {
                let mut this = this.borrow_mut();
                this.handle_sync_outcome(outcome);
            });
        });

        thread::spawn(move || {
            let result = Self::do_github_init(
                &path_str, &remote_url_str, &branch_str, &token_str,
                &proxy_type_str, &proxy_host_str, proxy_port_val,
            );
            callback(result);
        });
    }

// AppBackend::do_github_init
    pub(crate) fn do_github_init(
        path: &str,
        remote_url: &str,
        branch: &str,
        token: &str,
        proxy_type: &str,
        proxy_host: &str,
        proxy_port: u16,
    ) -> SyncTaskOutcome {
        use writer_core::sync_service::{sanitize_remote_url, SyncConfig, BackendType, SyncSecrets};

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
            WriterCoreApi::new(path).validate_workspace().unwrap_or(false)
        };

        let is_git_repo = || -> bool {
            path_obj.join(".git").exists()
        };

        let effective_token = if token.is_empty() {
            parsed.extracted_token.clone()
        } else {
            Some(token.to_string())
        };

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: sanitized_url.clone(),
            transport: writer_core::sync_service::SyncTransport::HttpsToken,
            branch: branch.to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            proxy_enabled: !proxy_type.is_empty() && proxy_type != "none" && !proxy_host.is_empty() && proxy_port > 0,
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
            // Empty directory - clone remote first
            let backend = writer_core::sync_service::create_sync_backend(&config.backend_type);
            match backend.sync(path_obj, &config, &secrets) {
                Ok(result) => {
                    if result.status == writer_core::sync_service::SyncStatus::Success {
                        let api = WriterCoreApi::new(path);
                        if !api.validate_workspace().unwrap_or(false) {
                            // Remote is empty or not a valid workspace — create workspace locally
                            if let Err(e) = api.create_workspace_if_needed() {
                                return SyncTaskOutcome {
                                    sync_status: "error".to_string(),
                                    action_result: format!("克隆成功但工作区初始化失败: {}", e),
                                };
                            }
                            // Push the newly created workspace files
                            let push_backend = writer_core::sync_service::create_sync_backend(&config.backend_type);
                            let push_result = push_backend.sync(path_obj, &config, &secrets);
                            let save_first = match &push_result {
                                Ok(r) if r.status != writer_core::sync_service::SyncStatus::Success => true,
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
                                        let err = push_res.error.unwrap_or_default();
                                        if let Some(se) = save_outcome {
                                            return SyncTaskOutcome {
                                                sync_status: "error".to_string(),
                                                action_result: format!("工作区已创建，但推送到远端失败: {}，且同步配置保存失败: {}. 请检查权限/磁盘。", mask_sync_error(&err), se),
                                            };
                                        }
                                        return SyncTaskOutcome {
                                            sync_status: sync_error_category(&err),
                                            action_result: format!("本地工作区已初始化但推送到远端失败: {}. 可在配置同步后手动同步。", mask_sync_error(&err)),
                                        };
                                    }
                                    Err(e) => {
                                        if let Some(se) = save_outcome {
                                            return SyncTaskOutcome {
                                                sync_status: "error".to_string(),
                                                action_result: format!("工作区已创建，但推送到远端失败: {}，且同步配置保存失败: {}. 请检查权限/磁盘。", mask_sync_error(&e.to_string()), se),
                                            };
                                        }
                                        return SyncTaskOutcome {
                                            sync_status: sync_error_category(&e.to_string()),
                                            action_result: format!("本地工作区已初始化但推送到远端失败: {}. 可在配置同步后手动同步。", mask_sync_error(&e.to_string())),
                                        };
                                    }
                                }
                            }
                        }
                        // Save config + secrets to disk
                        match save_sync_configs(path, cfg_ref, sec_ref) {
                            Ok(()) => SyncTaskOutcome {
                                sync_status: "success".to_string(),
                                action_result: "克隆并初始化工作区成功".to_string(),
                            },
                            Err(e) => SyncTaskOutcome {
                                sync_status: "error".to_string(),
                                action_result: format!("工作区已创建/同步可能完成，但同步配置保存失败: {}. 请检查权限/磁盘，不要继续同步。", e),
                            },
                        }
                    } else {
                        let err = result.error.unwrap_or_default();
                        SyncTaskOutcome {
                            sync_status: sync_error_category(&err),
                            action_result: format!("克隆失败: {}", mask_sync_error(&err)),
                        }
                    }
                }
                Err(e) => SyncTaskOutcome {
                    sync_status: sync_error_category(&e.to_string()),
                    action_result: format!("克隆失败: {}", mask_sync_error(&e.to_string())),
                },
            }
        } else if has_workspace() {
            // Existing workspace — sync and save config
            let backend = writer_core::sync_service::create_sync_backend(&config.backend_type);
            match backend.sync(path_obj, &config, &secrets) {
                Ok(result) => {
                    if result.status == writer_core::sync_service::SyncStatus::Success {
                        match save_sync_configs(path, cfg_ref, sec_ref) {
                            Ok(()) => SyncTaskOutcome {
                                sync_status: "success".to_string(),
                                action_result: "远程仓库已配置并同步成功".to_string(),
                            },
                            Err(e) => SyncTaskOutcome {
                                sync_status: "error".to_string(),
                                action_result: format!("同步成功但配置保存失败: {}. 请检查权限/磁盘，不要继续同步。", e),
                            },
                        }
                    } else if result.status == writer_core::sync_service::SyncStatus::Conflict {
                        let mut files = result.conflicts.iter().map(|c| c.local_path.clone()).collect::<Vec<_>>();
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
                        
                        let masked_err = result.error.as_deref().map(mask_sync_error).unwrap_or_else(|| "None".to_string());
                        debug_log_static("sync", "conflict_detected", &format!("conflicted file count={}, masked error={}", files.len(), masked_err));
                        
                        let m = format!(
                            "同步冲突，已停止，未覆盖任何文件\n\n原因:\n本地和远端都修改了同一批同步文件，Git 无法安全自动合并。\n\n冲突文件:\n  - {}\n\n下一步建议:\n1. 先备份当前工作区\n2. 运行诊断确认网络认证正常\n3. 手动处理冲突后重新同步",
                            file_str
                        );
                        SyncTaskOutcome {
                            sync_status: "conflict".to_string(),
                            action_result: m,
                        }
                    } else {
                        let err = result.error.unwrap_or_default();
                        let cat = sync_error_category(&err);
                        let action_result = if cat == "conflict" {
                            debug_log_static("sync", "conflict_detected", &format!("conflicted file count=unknown, masked error={}", mask_sync_error(&err)));
                            format!(
                                "同步冲突，已停止，未覆盖任何文件\n\n原因:\n本地和远端都修改了同一批同步文件，Git 无法安全自动合并。\n\n冲突文件:\n  - 未能列出具体冲突文件\n\n下一步建议:\n1. 先备份当前工作区\n2. 运行诊断确认网络认证正常\n3. 手动处理冲突后重新同步\n\n(原始错误: {})",
                                mask_sync_error(&err)
                            )
                        } else {
                            format!("同步失败: {}", mask_sync_error(&err))
                        };
                        SyncTaskOutcome {
                            sync_status: cat,
                            action_result,
                        }
                    }
                }
                Err(e) => {
                    let err_str = e.to_string();
                    let cat = sync_error_category(&err_str);
                    let action_result = if cat == "conflict" {
                        debug_log_static("sync", "conflict_detected", &format!("conflicted file count=unknown, masked error={}", mask_sync_error(&err_str)));
                        format!(
                            "同步冲突，已停止，未覆盖任何文件\n\n原因:\n本地和远端都修改了同一批同步文件，Git 无法安全自动合并。\n\n冲突文件:\n  - 未能列出具体冲突文件\n\n下一步建议:\n1. 先备份当前工作区\n2. 运行诊断确认网络认证正常\n3. 手动处理冲突后重新同步\n\n(原始错误: {})",
                            mask_sync_error(&err_str)
                        )
                    } else {
                        format!("同步失败: {}", mask_sync_error(&err_str))
                    };
                    SyncTaskOutcome {
                        sync_status: cat,
                        action_result,
                    }
                }
            }
        } else if is_git_repo() {
            // Has git repo but no workspace — error, user should open as workspace
            SyncTaskOutcome {
                sync_status: "error".to_string(),
                action_result: "目录包含 Git 仓库但不是 Writer 工作区。请先新建本地工作区（新建作品后保存），再配置同步。".to_string(),
            }
        } else {
            // Non-empty, no workspace, no git — blocked
            SyncTaskOutcome {
                sync_status: "error".to_string(),
                action_result: "目录非空且不是 Writer 工作区。请选择空目录，或先新建本地工作区。".to_string(),
            }
        }
    }

// AppBackend::open_workspace_dir
    pub(crate) fn open_workspace_dir(&mut self) {
        let path = self.current_workspace.clone();
        if !path.is_empty() {
            let _ = std::process::Command::new("xdg-open")
                .arg(&path)
                .spawn();
        }
    }

}
