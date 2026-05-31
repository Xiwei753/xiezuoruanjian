// =============================================================================
// workspace_backend.rs — 工作区领域 QObject 后端适配层
// =============================================================================
//
// 引用了什么：
// - super::*：引入 AppBackend 核心后端的全部方法与结构体。
// - crate::backend::SafeAppPtr：用于安全访问全局 AppBackend 指针以读取/更新工作区状态。
//
// 干什么的：
// - 实现 WorkspaceBackend 结构体，作为 QML 中 "workspaceBackend" 对象的桥梁。
// - 提供工作区的全生命周期交互，包含自动恢复上次工作区、唤起 RFD 文件夹框选择路径新建/打开工作区等。
// - 支持从 GitHub 克隆（init_workspace_from_github & execute_github_init）拉取已有工作区至本地，并在异步线程中进行安全性操作。
// - 负责向上层 QML 主页提供当前工作区路径（workspace_path）和是否已加载工作区（has_workspace）属性，以驱动界面渲染。
//
// 被什么引用：
// - 被 apps/linux/src/backend/mod.rs 引用，用于实例化工作区后端并绑定为 QML 全局上下文属性。
// =============================================================================

use super::*;
use crate::backend::SafeAppPtr;

fn backend_link_broken_json() -> QString {
    WriterCoreApi::envelope_json::<String>(Err(writer_core::api::WriterError::Other(
        "底层链接断开，请重启应用".to_string(),
    )))
    .into()
}

fn workspace_success_json(data: &str) -> QString {
    writer_core::api::ResultEnvelope::success(data.to_string())
        .to_json_string()
        .into()
}

#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct WorkspaceBackend {
    base: qt_base_class!(trait QObject),
    workspace_path: qt_property!(QString; READ workspace_path NOTIFY workspace_opened),
    has_workspace: qt_property!(bool; READ has_workspace NOTIFY workspace_state_changed),
    pending_github_init_path: qt_property!(QString; READ pending_github_init_path NOTIFY pending_github_init_path_changed),
    workspace_opened: qt_signal!(),
    workspace_content_changed: qt_signal!(),
    workspace_state_changed: qt_signal!(),
    pending_github_init_path_changed: qt_signal!(),
    try_restore_last_workspace: qt_method!(fn(&mut self)),
    create_new_workspace: qt_method!(fn(&mut self) -> QString),
    open_existing_workspace: qt_method!(fn(&mut self) -> QString),
    create_workspace_with_path: qt_method!(fn(&mut self, path: QString) -> QString),
    open_workspace_with_path: qt_method!(fn(&mut self, path: QString) -> QString),
    close_workspace: qt_method!(fn(&mut self)),
    clear_last_workspace: qt_method!(fn(&mut self)),
    switch_workspace: qt_method!(fn(&mut self)),
    init_workspace_from_github: qt_method!(fn(&mut self)),
    execute_github_init: qt_method!(fn(&mut self, path: QString, remote_url: QString, branch: QString, token: QString, proxy_type: QString, proxy_host: QString, proxy_port: u16)),
    get_workspace_diagnostics: qt_method!(fn(&self) -> QString),
    open_workspace_dir: qt_method!(fn(&mut self)),
    app: SafeAppPtr,
}

impl WorkspaceBackend {
    pub fn new(app: SafeAppPtr) -> Self { Self { app, ..Default::default() } }
    fn with_app<R>(&self, default: R, f: impl FnOnce(&AppBackend) -> R) -> R {
        if let Some(app) = self.app.get() {
            unsafe { f(&*app) }
        } else {
            crate::backend::app_backend::debug_error_static("workspace", "BACKEND_LINK_BROKEN", "app pointer is null");
            default
        }
    }
    fn with_app_mut<R>(&mut self, default: R, f: impl FnOnce(&mut AppBackend) -> R) -> R {
        if let Some(app) = self.app.get() {
            unsafe { f(&mut *app) }
        } else {
            crate::backend::app_backend::debug_error_static("workspace", "BACKEND_LINK_BROKEN", "app pointer is null");
            default
        }
    }
    fn emit_workspace_changed(&mut self) { self.workspace_opened(); self.workspace_content_changed(); self.workspace_state_changed(); }
    fn workspace_path(&self) -> QString { self.with_app("".into(), |app| app.workspace_path()) }
    fn has_workspace(&self) -> bool { self.with_app(false, |app| app.has_workspace()) }
    fn pending_github_init_path(&self) -> QString { self.with_app("".into(), |app| app.pending_github_init_path()) }
    fn try_restore_last_workspace(&mut self) { self.with_app_mut((), |app| app.try_restore_last_workspace()); self.emit_workspace_changed(); }
    fn create_new_workspace(&mut self) -> QString { 
        let res = self.with_app_mut(
            backend_link_broken_json(),
            |app| app.create_new_workspace()
        );
        self.emit_workspace_changed(); 
        res
    }
    fn open_existing_workspace(&mut self) -> QString { 
        let res = self.with_app_mut(
            backend_link_broken_json(),
            |app| app.open_existing_workspace()
        );
        self.emit_workspace_changed(); 
        res
    }
    fn create_workspace_with_path(&mut self, path: QString) -> QString { 
        let path_str = path.to_string();
        crate::backend::app_backend::debug_log_static("workspace", "qml_click_create_workspace", &format!("path={}", path_str));
        crate::backend::app_backend::debug_log_static("workspace", "workspace_backend_create_workspace_called", &format!("path={}", path_str));
        let res = self.with_app_mut(
            backend_link_broken_json(),
            |app| app.internal_open_workspace(&path_str, true)
        );
        let has = self.has_workspace();
        crate::backend::app_backend::debug_log_static("workspace", "writer_core_create_workspace_result", &format!("success={}", has));
        self.emit_workspace_changed(); 
        crate::backend::app_backend::debug_log_static("workspace", "workspace_state_updated", &format!("has_workspace={}", has));
        res
    }
    fn open_workspace_with_path(&mut self, path: QString) -> QString { 
        let path_str = path.to_string();
        crate::backend::app_backend::debug_log_static("workspace", "qml_click_open_workspace", &format!("path={}", path_str));
        crate::backend::app_backend::debug_log_static("workspace", "workspace_backend_open_workspace_called", &format!("path={}", path_str));
        let res = self.with_app_mut(
            backend_link_broken_json(),
            |app| app.internal_open_workspace(&path_str, false)
        );
        let has = self.has_workspace();
        crate::backend::app_backend::debug_log_static("workspace", "writer_core_open_workspace_result", &format!("success={}", has));
        self.emit_workspace_changed(); 
        crate::backend::app_backend::debug_log_static("workspace", "workspace_state_updated", &format!("has_workspace={}", has));
        res
    }
    fn close_workspace(&mut self) { self.with_app_mut((), |app| app.close_workspace()); self.emit_workspace_changed(); }
    fn clear_last_workspace(&mut self) { self.with_app_mut((), |app| app.clear_last_workspace()); self.workspace_state_changed(); }
    fn switch_workspace(&mut self) { self.with_app_mut((), |app| app.switch_workspace()); self.emit_workspace_changed(); }
    fn init_workspace_from_github(&mut self) { self.with_app_mut((), |app| app.init_workspace_from_github()); self.pending_github_init_path_changed(); }
    fn execute_github_init(&mut self, path: QString, remote_url: QString, branch: QString, token: QString, proxy_type: QString, proxy_host: QString, proxy_port: u16) {
        crate::backend::app_backend::debug_log_static("workspace", "qml_click_import_workspace", &format!("path={}, url={}", path.to_string(), remote_url.to_string()));
        crate::backend::app_backend::debug_log_static("workspace", "workspace_backend_import_workspace_called", &format!("path={}", path.to_string()));
        self.with_app_mut((), |app| app.execute_github_init(path, remote_url, branch, token, proxy_type, proxy_host, proxy_port));
        self.emit_workspace_changed();
        let has = self.has_workspace();
        crate::backend::app_backend::debug_log_static("workspace", "workspace_state_updated", &format!("has_workspace={}", has));
        self.pending_github_init_path_changed();
    }
    fn get_workspace_diagnostics(&self) -> QString { self.with_app(backend_link_broken_json(), |app| app.get_workspace_diagnostics()) }
    fn open_workspace_dir(&mut self) { self.with_app_mut((), |app| app.open_workspace_dir()); }
}

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
        WriterCoreApi::new(&self.current_workspace)
            .get_workspace_diagnostics_envelope_json(
                self.current_has_workspace,
                self.cached_tree.len() as u64,
            )
            .into()
    }

// AppBackend::try_restore_last_workspace
    pub(crate) fn try_restore_last_workspace(&mut self) {
        self.debug_log("workspace", "try_restore_last_workspace_start", "");
        if let Some(path) = writer_core::app_config::get_last_workspace_path() {
            self.debug_log("workspace", "try_restore_last_workspace_path_found", &format!("path={}", path));
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
    pub(crate) fn internal_open_workspace(&mut self, path: &str, initialize: bool) -> QString {
        self.debug_log("workspace", "internal_open_workspace_start", &format!("path={}, initialize={}", path, initialize));
        let api = WriterCoreApi::new(path);
        let is_valid = api.validate_workspace().unwrap_or(false);
        self.debug_log("workspace", "internal_open_workspace_validate", &format!("path={}, is_valid={}", path, is_valid));

        if !is_valid && !initialize {
            let err_msg = "不是有效工作区。请选择其他目录，或使用「新建工作区」初始化该目录。";
            self.set_error(err_msg);
            self.debug_error("workspace", "internal_open_workspace_failed", err_msg);
            return WriterCoreApi::envelope_json::<String>(Err(writer_core::api::WriterError::InvalidWorkspace)).into();
        }

        if !is_valid && initialize {
            self.debug_log("workspace", "internal_open_workspace_creating", path);
            if let Err(e) = api.create_workspace_if_needed() {
                let err_msg = format!("无法创建工作区: {}", e);
                self.set_error(&err_msg);
                self.debug_error("workspace", "internal_open_workspace_failed", &err_msg);
                return WriterCoreApi::envelope_json::<String>(Err(e)).into();
            }
        }

        let val_res = api.validate_workspace().unwrap_or(false);
        self.debug_log("workspace", "internal_open_workspace_revalidate", &format!("path={}, is_valid={}", path, val_res));
        if !val_res {
            let err_msg = "工作区验证失败";
            self.set_error(err_msg);
            self.debug_error("workspace", "internal_open_workspace_failed", err_msg);
            return WriterCoreApi::envelope_json::<String>(Err(writer_core::api::WriterError::InvalidWorkspace)).into();
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

        workspace_success_json("OK")
    }

// AppBackend::create_new_workspace
    pub(crate) fn create_new_workspace(&mut self) -> QString {
        self.debug_log("workspace", "create_new_workspace_clicked", "");
        if let Some(path) = FileDialog::new().pick_folder() {
            self.internal_open_workspace(&path.to_string_lossy(), true)
        } else {
            self.debug_log("workspace", "create_new_workspace_cancelled", "");
            workspace_success_json("CANCELLED")
        }
    }

// AppBackend::open_existing_workspace
    pub(crate) fn open_existing_workspace(&mut self) -> QString {
        self.debug_log("workspace", "open_existing_workspace_clicked", "");
        if let Some(path) = FileDialog::new().pick_folder() {
            self.internal_open_workspace(&path.to_string_lossy(), false)
        } else {
            self.debug_log("workspace", "open_existing_workspace_cancelled", "");
            workspace_success_json("CANCELLED")
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

        let branch_str = if branch.to_string().is_empty() { "main".to_string() } else { branch.to_string() };
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
                &path_str, &remote_url_str, &branch_str, &token_str,
                &proxy_type_str, &proxy_host_str, proxy_port_val,
            );
            callback(result);
        });
    }

// AppBackend::do_github_init
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
                                    operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: "error".to_string(),
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
                        // Save config + secrets to disk
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
                            operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: sync_error_category_from_code(result.error_category.as_deref(), &err),
                            action_result: format!("克隆失败: {}", mask_sync_error(&err)),
                        }
                    }
                }
                Err(e) => SyncTaskOutcome {
                    operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: sync_error_category(&e.to_string()),
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
                                operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: "success".to_string(),
                                action_result: "远程仓库已配置并同步成功".to_string(),
                            },
                            Err(e) => SyncTaskOutcome {
                                operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: "error".to_string(),
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
                            operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: "conflict".to_string(),
                            action_result: m,
                        }
                    } else {
                        let err = result.error.unwrap_or_default();
                        let cat = sync_error_category_from_code(result.error_category.as_deref(), &err);
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
                            operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: cat,
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
                        operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: cat,
                        action_result,
                    }
                }
            }
        } else if is_git_repo() {
            // Has git repo but no workspace — error, user should open as workspace
            SyncTaskOutcome {
                operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: "error".to_string(),
                action_result: "目录包含 Git 仓库但不是 Writer 工作区。请先新建本地工作区（新建作品后保存），再配置同步。".to_string(),
            }
        } else {
            // Non-empty, no workspace, no git — blocked
            SyncTaskOutcome {
                operation_id: operation_id.to_string(), operation_kind: "sync".to_string(), sync_status: "error".to_string(),
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
