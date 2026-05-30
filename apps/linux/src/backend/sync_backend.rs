use super::*;

#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct SyncBackend {
    base: qt_base_class!(trait QObject),
    sync_enabled: qt_property!(bool; READ sync_enabled WRITE set_sync_enabled NOTIFY sync_config_changed),
    sync_backend_type: qt_property!(QString; READ sync_backend_type WRITE set_sync_backend_type NOTIFY sync_config_changed),
    sync_remote_url: qt_property!(QString; READ sync_remote_url WRITE set_sync_remote_url NOTIFY sync_config_changed),
    sync_branch: qt_property!(QString; READ sync_branch WRITE set_sync_branch NOTIFY sync_config_changed),
    sync_auto_sync: qt_property!(bool; READ sync_auto_sync WRITE set_sync_auto_sync NOTIFY sync_config_changed),
    sync_interval: qt_property!(u32; READ sync_interval WRITE set_sync_interval NOTIFY sync_config_changed),
    sync_proxy_enabled: qt_property!(bool; READ sync_proxy_enabled WRITE set_sync_proxy_enabled NOTIFY sync_config_changed),
    sync_proxy_type: qt_property!(QString; READ sync_proxy_type WRITE set_sync_proxy_type NOTIFY sync_config_changed),
    sync_proxy_host: qt_property!(QString; READ sync_proxy_host WRITE set_sync_proxy_host NOTIFY sync_config_changed),
    sync_proxy_port: qt_property!(u16; READ sync_proxy_port WRITE set_sync_proxy_port NOTIFY sync_config_changed),
    sync_username: qt_property!(QString; READ sync_username WRITE set_sync_username NOTIFY sync_config_changed),
    has_sync_token: qt_property!(bool; READ has_sync_token NOTIFY sync_config_changed),
    sync_action_result: qt_property!(QString; READ sync_action_result NOTIFY sync_action_completed),
    sync_status: qt_property!(QString; READ sync_status WRITE set_sync_status NOTIFY sync_status_changed),
    has_workspace: qt_property!(bool; READ has_workspace NOTIFY workspace_state_changed),
    sync_config_changed: qt_signal!(),
    sync_action_completed: qt_signal!(),
    sync_status_changed: qt_signal!(),
    workspace_state_changed: qt_signal!(),
    set_sync_token: qt_method!(fn(&mut self, token: QString)),
    load_sync_config: qt_method!(fn(&mut self)),
    save_sync_config: qt_method!(fn(&mut self) -> bool),
    perform_sync_dry_run: qt_method!(fn(&mut self)),
    perform_sync: qt_method!(fn(&mut self)),
    perform_sync_diagnostics: qt_method!(fn(&mut self)),
    request_auto_sync: qt_method!(fn(&mut self, reason: QString)),
    maybe_auto_sync_on_foreground: qt_method!(fn(&mut self)),
    open_workspace_dir: qt_method!(fn(&mut self)),
    copy_text_to_clipboard: qt_method!(fn(&mut self, text: QString) -> QString),
    app: QPointer<AppBackend>,
}

impl SyncBackend {
    pub fn new(app: QPointer<AppBackend>) -> Self { Self { app, ..Default::default() } }
    fn with_app<R>(&self, default: R, f: impl FnOnce(&AppBackend) -> R) -> R { self.app.as_pinned().map(|app| f(&app.borrow())).unwrap_or(default) }
    fn with_app_mut<R>(&mut self, default: R, f: impl FnOnce(&mut AppBackend) -> R) -> R { self.app.as_pinned().map(|app| f(&mut app.borrow_mut())).unwrap_or(default) }
    fn sync_enabled(&self) -> bool { self.with_app(false, |app| app.sync_enabled()) }
    fn set_sync_enabled(&mut self, val: bool) { self.with_app_mut((), |app| app.set_sync_enabled(val)); self.sync_config_changed(); }
    fn sync_backend_type(&self) -> QString { self.with_app("".into(), |app| app.sync_backend_type()) }
    fn set_sync_backend_type(&mut self, val: QString) { self.with_app_mut((), |app| app.set_sync_backend_type(val)); self.sync_config_changed(); }
    fn sync_remote_url(&self) -> QString { self.with_app("".into(), |app| app.sync_remote_url()) }
    fn set_sync_remote_url(&mut self, val: QString) { self.with_app_mut((), |app| app.set_sync_remote_url(val)); self.sync_config_changed(); }
    fn sync_branch(&self) -> QString { self.with_app("main".into(), |app| app.sync_branch()) }
    fn set_sync_branch(&mut self, val: QString) { self.with_app_mut((), |app| app.set_sync_branch(val)); self.sync_config_changed(); }
    fn sync_auto_sync(&self) -> bool { self.with_app(false, |app| app.sync_auto_sync()) }
    fn set_sync_auto_sync(&mut self, val: bool) { self.with_app_mut((), |app| app.set_sync_auto_sync(val)); self.sync_config_changed(); }
    fn sync_interval(&self) -> u32 { self.with_app(300, |app| app.sync_interval()) }
    fn set_sync_interval(&mut self, val: u32) { self.with_app_mut((), |app| app.set_sync_interval(val)); self.sync_config_changed(); }
    fn sync_proxy_enabled(&self) -> bool { self.with_app(false, |app| app.sync_proxy_enabled()) }
    fn set_sync_proxy_enabled(&mut self, val: bool) { self.with_app_mut((), |app| app.set_sync_proxy_enabled(val)); self.sync_config_changed(); }
    fn sync_proxy_type(&self) -> QString { self.with_app("".into(), |app| app.sync_proxy_type()) }
    fn set_sync_proxy_type(&mut self, val: QString) { self.with_app_mut((), |app| app.set_sync_proxy_type(val)); self.sync_config_changed(); }
    fn sync_proxy_host(&self) -> QString { self.with_app("".into(), |app| app.sync_proxy_host()) }
    fn set_sync_proxy_host(&mut self, val: QString) { self.with_app_mut((), |app| app.set_sync_proxy_host(val)); self.sync_config_changed(); }
    fn sync_proxy_port(&self) -> u16 { self.with_app(0, |app| app.sync_proxy_port()) }
    fn set_sync_proxy_port(&mut self, val: u16) { self.with_app_mut((), |app| app.set_sync_proxy_port(val)); self.sync_config_changed(); }
    fn sync_username(&self) -> QString { self.with_app("".into(), |app| app.sync_username()) }
    fn set_sync_username(&mut self, val: QString) { self.with_app_mut((), |app| app.set_sync_username(val)); self.sync_config_changed(); }
    fn has_sync_token(&self) -> bool { self.with_app(false, |app| app.has_sync_token()) }
    fn sync_action_result(&self) -> QString { self.with_app("".into(), |app| app.sync_action_result()) }
    fn sync_status(&self) -> QString { self.with_app("not_configured".into(), |app| app.sync_status()) }
    fn set_sync_status(&mut self, val: QString) { self.with_app_mut((), |app| app.set_sync_status(val)); self.sync_status_changed(); }
    fn has_workspace(&self) -> bool { self.with_app(false, |app| app.has_workspace()) }
    fn set_sync_token(&mut self, token: QString) { self.with_app_mut((), |app| app.set_sync_token(token)); self.sync_config_changed(); }
    fn load_sync_config(&mut self) { self.with_app_mut((), |app| app.load_sync_config()); self.sync_config_changed(); self.sync_status_changed(); }
    fn save_sync_config(&mut self) -> bool { let ok = self.with_app_mut(false, |app| app.save_sync_config()); self.sync_config_changed(); ok }
    fn perform_sync_dry_run(&mut self) { self.with_app_mut((), |app| app.perform_sync_dry_run()); self.sync_status_changed(); self.sync_action_completed(); }
    fn perform_sync(&mut self) { self.with_app_mut((), |app| app.perform_sync()); self.sync_status_changed(); }
    fn perform_sync_diagnostics(&mut self) { self.with_app_mut((), |app| app.perform_sync_diagnostics()); self.sync_status_changed(); self.sync_action_completed(); }
    fn request_auto_sync(&mut self, reason: QString) { self.with_app_mut((), |app| app.request_auto_sync(reason)); self.sync_status_changed(); }
    fn maybe_auto_sync_on_foreground(&mut self) { self.with_app_mut((), |app| app.maybe_auto_sync_on_foreground()); self.sync_status_changed(); }
    fn open_workspace_dir(&mut self) { self.with_app_mut((), |app| app.open_workspace_dir()); }
    fn copy_text_to_clipboard(&mut self, text: QString) -> QString { self.with_app_mut("{}".into(), |app| app.copy_text_to_clipboard(text)) }
}

impl AppBackend {
// Included inside impl AppBackend from app_backend.rs.
// Deprecated compatibility methods for this Linux backend domain.

// AppBackend::handle_sync_outcome
    pub(crate) fn handle_sync_outcome(&mut self, outcome: SyncTaskOutcome) {
        let status = outcome.sync_status.clone();
        let result_trunc = if outcome.action_result.chars().count() > 1000 {
            outcome.action_result.chars().take(1000).collect::<String>() + "..."
        } else {
            outcome.action_result.clone()
        };
        let sanitized_result = mask_sync_error(&result_trunc);
        self.debug_log("sync", "sync_outcome_received", &format!("status={}, result={}", status, sanitized_result));

        self.current_sync_status = outcome.sync_status.clone();
        self.current_sync_in_progress = false;
        self.current_last_sync_time = Self::now_epoch_seconds();
        self.current_sync_action_result = outcome.action_result.clone();
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
        } else if (status_str == "conflict" || status_str == "unrelated_histories") && self.has_workspace() {
            self.reload_tree();
            self.trigger_projects_reloaded();
        }
    }

// AppBackend::handle_successful_sync_refresh
    pub(crate) fn handle_successful_sync_refresh(&mut self) {
        self.reload_tree();
        let chapter_deleted = self.reconcile_selection_after_tree_reload();
        self.trigger_projects_reloaded();
        self.workspace_content_changed();
        self.workspace_state_changed();

        if chapter_deleted {
            self.current_save_status = "当前章节已在其他设备删除，已刷新列表。".to_string();
            self.save_status_changed();
            self.current_sync_action_result.push_str("\n\n当前章节已在其他设备删除，已刷新列表。");
            self.sync_action_completed();
        }

        self.debug_log("sync", "sync_refresh_applied", "tree_reloaded=true");
    }

// AppBackend::can_start_auto_sync
    pub(crate) fn can_start_auto_sync(&self, reason: &str, min_gap_secs: i64) -> bool {
        if self.current_sync_in_progress {
            return false;
        }
        if !self.current_has_workspace || !self.current_sync_enabled || !self.current_sync_auto_sync {
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

// AppBackend::sync_status
    pub(crate) fn sync_status(&self) -> QString {
        self.current_sync_status.clone().into()
    }

// AppBackend::set_sync_status
    pub(crate) fn set_sync_status(&mut self, val: QString) {
        self.current_sync_status = val.to_string();
        self.sync_status_changed();
    }

// AppBackend::refresh_sync_status_from_config
    pub(crate) fn refresh_sync_status_from_config(&mut self) {
        if !self.current_has_workspace {
            self.current_sync_status = "not_configured".to_string();
            self.sync_status_changed();
            return;
        }
        let has_remote = !self.current_sync_remote_url.is_empty();
        if !has_remote || !self.current_sync_enabled {
            self.current_sync_status = "not_configured".to_string();
        } else {
            // remote_url exists — configured_untested regardless of token state
            self.current_sync_status = "configured_untested".to_string();
        }
        self.sync_status_changed();
    }

// AppBackend::sync_enabled
    pub(crate) fn sync_enabled(&self) -> bool {
        self.current_sync_enabled
    }

// AppBackend::set_sync_enabled
    pub(crate) fn set_sync_enabled(&mut self, val: bool) {
        self.current_sync_enabled = val;
        self.sync_config_changed();
    }

// AppBackend::sync_backend_type
    pub(crate) fn sync_backend_type(&self) -> QString {
        self.current_sync_backend_type.clone().into()
    }

// AppBackend::set_sync_backend_type
    pub(crate) fn set_sync_backend_type(&mut self, val: QString) {
        self.current_sync_backend_type = val.to_string();
        self.sync_config_changed();
    }

// AppBackend::sync_remote_url
    pub(crate) fn sync_remote_url(&self) -> QString {
        self.current_sync_remote_url.clone().into()
    }

// AppBackend::set_sync_remote_url
    pub(crate) fn set_sync_remote_url(&mut self, val: QString) {
        self.current_sync_remote_url = val.to_string();
        self.sync_config_changed();
    }

// AppBackend::sync_branch
    pub(crate) fn sync_branch(&self) -> QString {
        self.current_sync_branch.clone().into()
    }

// AppBackend::set_sync_branch
    pub(crate) fn set_sync_branch(&mut self, val: QString) {
        self.current_sync_branch = val.to_string();
        self.sync_config_changed();
    }

// AppBackend::sync_auto_sync
    pub(crate) fn sync_auto_sync(&self) -> bool {
        self.current_sync_auto_sync
    }

// AppBackend::set_sync_auto_sync
    pub(crate) fn set_sync_auto_sync(&mut self, val: bool) {
        self.current_sync_auto_sync = val;
        self.sync_config_changed();
    }

// AppBackend::sync_interval
    pub(crate) fn sync_interval(&self) -> u32 {
        self.current_sync_interval
    }

// AppBackend::set_sync_interval
    pub(crate) fn set_sync_interval(&mut self, val: u32) {
        self.current_sync_interval = val;
        self.sync_config_changed();
    }

// AppBackend::sync_proxy_type
    pub(crate) fn sync_proxy_type(&self) -> QString {
        self.current_sync_proxy_type.clone().into()
    }

// AppBackend::set_sync_proxy_type
    pub(crate) fn set_sync_proxy_type(&mut self, val: QString) {
        self.current_sync_proxy_type = val.to_string();
        self.sync_config_changed();
    }

// AppBackend::sync_proxy_host
    pub(crate) fn sync_proxy_host(&self) -> QString {
        self.current_sync_proxy_host.clone().into()
    }

// AppBackend::set_sync_proxy_host
    pub(crate) fn set_sync_proxy_host(&mut self, val: QString) {
        self.current_sync_proxy_host = val.to_string();
        self.sync_config_changed();
    }

// AppBackend::sync_proxy_port
    pub(crate) fn sync_proxy_port(&self) -> u16 {
        self.current_sync_proxy_port
    }

// AppBackend::set_sync_proxy_port
    pub(crate) fn set_sync_proxy_port(&mut self, val: u16) {
        self.current_sync_proxy_port = val;
        self.sync_config_changed();
    }

// AppBackend::sync_proxy_enabled
    pub(crate) fn sync_proxy_enabled(&self) -> bool {
        self.current_sync_proxy_enabled
    }

// AppBackend::set_sync_proxy_enabled
    pub(crate) fn set_sync_proxy_enabled(&mut self, val: bool) {
        self.current_sync_proxy_enabled = val;
        self.sync_config_changed();
    }

// AppBackend::sync_username
    pub(crate) fn sync_username(&self) -> QString {
        self.current_sync_username.clone().into()
    }

// AppBackend::set_sync_username
    pub(crate) fn set_sync_username(&mut self, val: QString) {
        self.current_sync_username = val.to_string();
        self.sync_config_changed();
    }

// AppBackend::has_sync_token
    pub(crate) fn has_sync_token(&self) -> bool {
        !self.current_sync_token.is_empty()
    }

// AppBackend::set_sync_token
    pub(crate) fn set_sync_token(&mut self, val: QString) {
        self.current_sync_token = val.to_string();
        self.sync_config_changed();
    }

// AppBackend::sync_action_result
    pub(crate) fn sync_action_result(&self) -> QString {
        self.current_sync_action_result.clone().into()
    }

// AppBackend::perform_sync_diagnostics
    pub(crate) fn perform_sync_diagnostics(&mut self) {
        self.debug_log("sync", "perform_sync_diagnostics_start", "");
        let workspace_path = self.current_workspace.clone();
        if workspace_path.is_empty() {
            self.current_sync_action_result = "请先打开工作区".to_string();
            self.sync_action_completed();
            self.debug_error("sync", "perform_sync_diagnostics_failed", "workspace_empty");
            return;
        }

        self.current_sync_status = "syncing".to_string();
        self.sync_status_changed();
        self.current_sync_action_result = "正在诊断...".to_string();

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |outcome: SyncTaskOutcome| {
            qptr.as_pinned().map(|this| {
                let mut this = this.borrow_mut();
                this.handle_sync_outcome(outcome);
            });
        });

        thread::spawn(move || {
            let api = WriterCoreApi::new(&workspace_path);
            let config = match api.load_sync_config() {
                Ok(c) => c,
                Err(e) => {
                    callback(SyncTaskOutcome {
                        sync_status: "error".to_string(),
                        action_result: format!("无法加载同步配置: {}", e),
                    });
                    return;
                }
            };

            match api.perform_sync_diagnostics(config) {
                Ok(result) => {
                    let status = determine_diagnostics_status(&result);
                    let msg = format_diagnostics_message(&result);

                    callback(SyncTaskOutcome {
                        sync_status: status.to_string(),
                        action_result: msg,
                    });
                }
                Err(e) => {
                    callback(SyncTaskOutcome {
                        sync_status: sync_error_category(&e.to_string()),
                        action_result: format!("诊断过程发生错误:\n{}", mask_sync_error(&e.to_string())),
                    });
                }
            }
        });
    }

// AppBackend::load_sync_config
    pub(crate) fn load_sync_config(&mut self) {
        self.debug_log("sync", "load_sync_config_start", "");
        if let Some(api) = self.core_api() {
            let config_opt = api.load_sync_config().ok();
            let token_opt = api.load_sync_secrets().ok().and_then(|s| s.token);
            if let Some(config) = config_opt {
                self.current_sync_enabled = config.enabled;
                self.current_sync_backend_type = config.backend_type.clone();
                self.current_sync_remote_url = config.remote_url.clone();
                self.current_sync_branch = if config.branch.is_empty() { "main".to_string() } else { config.branch.clone() };
                self.current_sync_auto_sync = config.auto_sync;
                self.current_sync_interval = config.sync_interval_seconds;
                self.current_sync_proxy_enabled = config.proxy_enabled;
                self.current_sync_proxy_type = config.proxy_type.clone();
                self.current_sync_proxy_host = config.proxy_host.clone();
                self.current_sync_proxy_port = config.proxy_port;
                self.current_sync_username = config.username.clone();
            } else {
                self.current_sync_enabled = false;
                self.current_sync_remote_url = "".to_string();
                self.current_sync_branch = "main".to_string();
                self.current_sync_token = "".to_string();
            }
            if let Some(t) = token_opt {
                self.current_sync_token = t;
            } else {
                self.current_sync_token = "".to_string();
            }
            self.refresh_sync_status_from_config();
            self.sync_config_changed();
            let token_present = !self.current_sync_token.is_empty();
            let masked_url = mask_sync_error(&self.current_sync_remote_url);
            self.debug_log(
                "sync",
                "load_sync_config_success",
                &format!("enabled={}, remote_url={}, branch={}, token_present={}", self.current_sync_enabled, masked_url, self.current_sync_branch, token_present)
            );
        } else {
            // No workspace open - ensure branch defaults to main
            self.current_sync_branch = "main".to_string();
            self.debug_warn("sync", "load_sync_config_failed", "core_not_initialized");
        }
    }

// AppBackend::save_sync_config
    pub(crate) fn save_sync_config(&mut self) -> bool {
        self.debug_log("sync", "save_sync_config_start", "");
        let mut error_msg: Option<String> = None;
        if let Some(api) = self.core_api() {

            let mut c = api
                .load_sync_config()
                .unwrap_or(writer_core::api::types::SyncConfigDto {
                    enabled: false,
                    backend_type: "github_api".to_string(),
                    remote_url: "".to_string(),
                    transport: "https_token".to_string(),
                    branch: "main".to_string(),
                    auto_sync: false,
                    sync_interval_seconds: 300,
                    proxy_enabled: false,
                    proxy_type: "none".to_string(),
                    proxy_host: "".to_string(),
                    proxy_port: 0,
                    username: "".to_string(),
                });

            let raw_url = self.current_sync_remote_url.clone();
            let parsed = writer_core::sync_service::sanitize_remote_url(&raw_url);

            c.enabled = self.current_sync_enabled;
            c.backend_type = match self.current_sync_backend_type.as_str() {
                "webdav" | "s3" | "local_folder" | "git" | "github_api" => self.current_sync_backend_type.clone(),
                _ => "github_api".to_string(),
            };
            c.remote_url = parsed.sanitized_url.clone();
            c.branch = if self.current_sync_branch.is_empty() { "main".to_string() } else { self.current_sync_branch.clone() };
            c.auto_sync = self.current_sync_auto_sync;
            c.sync_interval_seconds = self.current_sync_interval;
            c.proxy_enabled = self.current_sync_proxy_enabled;
            c.proxy_type = self.current_sync_proxy_type.clone();
            c.proxy_host = self.current_sync_proxy_host.clone();
            c.proxy_port = self.current_sync_proxy_port;
            c.username = self.current_sync_username.clone();

            if let Some(ref extracted_user) = parsed.extracted_username {
                if c.username.is_empty() {
                    c.username = extracted_user.clone();
                }
            }

            let mut s = api.load_sync_secrets().unwrap_or(writer_core::api::types::SyncSecretsDto { token: None });
            if let Some(ref extracted_token) = parsed.extracted_token {
                s.token = Some(extracted_token.clone());
            } else if self.current_sync_token.is_empty() {
                s.token = None;
            } else {
                s.token = Some(self.current_sync_token.clone());
            }

            if let Err(e) = api.save_sync_config(c) {
                error_msg = Some(format!("保存同步配置失败: {}", e));
            } else if let Err(e) = api.save_sync_secrets(s) {
                error_msg = Some(format!("保存同步凭证失败: {}", e));
            }
        } else {
            error_msg = Some("Core 未初始化".to_string());
        }

        if let Some(msg) = error_msg {
            self.set_error(&msg);
            self.current_sync_action_result = msg.clone();
            self.sync_action_completed();
            self.debug_error("sync", "save_sync_config_failed", &msg);
            return false;
        }

        self.refresh_sync_status_from_config();
        self.current_sync_action_result = "配置保存成功".to_string();
        self.sync_action_completed();
        let token_present = !self.current_sync_token.is_empty();
        let masked_url = mask_sync_error(&self.current_sync_remote_url);
        self.debug_log(
            "sync",
            "save_sync_config_success",
            &format!("enabled={}, remote_url={}, branch={}, token_present={}", self.current_sync_enabled, masked_url, self.current_sync_branch, token_present)
        );
        true
    }

// AppBackend::perform_sync_dry_run
    pub(crate) fn perform_sync_dry_run(&mut self) {
        let workspace_path = self.current_workspace.clone();
        if workspace_path.is_empty() {
            self.current_sync_action_result = "请先打开工作区".to_string();
            self.sync_action_completed();
            return;
        }

        if self.current_sync_remote_url.is_empty() {
            self.current_sync_status = "error".to_string();
            self.current_sync_action_result = "同步检查失败: 未配置远程仓库 URL".to_string();
            self.sync_status_changed();
            self.sync_action_completed();
            return;
        }

        if self.current_sync_token.is_empty() {
            self.current_sync_status = "error".to_string();
            self.current_sync_action_result = "同步检查失败: 未配置 GitHub 访问令牌 (Token)".to_string();
            self.sync_status_changed();
            self.sync_action_completed();
            return;
        }

        if self.current_sync_branch.is_empty() {
            self.current_sync_branch = "main".to_string();
            self.sync_config_changed();
        }

        self.current_sync_status = "syncing".to_string();
        self.sync_status_changed();
        self.current_sync_action_result = "正在检查同步计划...".to_string();

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |outcome: SyncTaskOutcome| {
            qptr.as_pinned().map(|this| {
                let mut this = this.borrow_mut();
                this.handle_sync_outcome(outcome);
            });
        });

        thread::spawn(move || {
            let api = WriterCoreApi::new(&workspace_path);
            let config = match api.load_sync_config() {
                Ok(c) => c,
                Err(e) => {
                    callback(SyncTaskOutcome {
                        sync_status: "configured_untested".to_string(),
                        action_result: format!("无法加载同步配置: {}", e),
                    });
                    return;
                }
            };

            match api.perform_sync_dry_run(config) {
                Ok(plan) => {
                    let mut msg = String::new();
                    msg.push_str("同步计划检查完成\n");
                    msg.push_str(&format!("需要上传的文件数: {}\n", plan.files_to_upload.len()));
                    msg.push_str(&format!("需要下载的文件数: {}\n", plan.files_to_download.len()));
                    msg.push_str(&format!("本地待删除的文件数: {}\n", plan.files_to_delete_local.len()));
                    msg.push_str(&format!("远程待删除的文件数: {}\n", plan.files_to_delete_remote.len()));

                    if !plan.files_to_upload.is_empty() {
                        msg.push_str("\n将要上传的文件:\n");
                        for f in plan.files_to_upload.iter().take(10) {
                            msg.push_str(&format!("  - {}\n", f));
                        }
                        if plan.files_to_upload.len() > 10 {
                            msg.push_str("  ... 更多文件省略\n");
                        }
                    }
                    callback(SyncTaskOutcome {
                        sync_status: "configured_untested".to_string(),
                        action_result: msg,
                    });
                }
                Err(e) => {
                    callback(SyncTaskOutcome {
                        sync_status: "configured_untested".to_string(),
                        action_result: format!("检查同步计划失败: {}", mask_sync_error(&e.to_string())),
                    });
                }
            }
        });
    }

// AppBackend::perform_sync
    pub(crate) fn perform_sync(&mut self) {
        self.perform_sync_internal("manual", false);
    }

// AppBackend::request_auto_sync
    pub(crate) fn request_auto_sync(&mut self, reason: QString) {
        let reason_str = reason.to_string();
        self.trigger_auto_sync(&reason_str);
    }

// AppBackend::maybe_auto_sync_on_foreground
    pub(crate) fn maybe_auto_sync_on_foreground(&mut self) {
        if !self.current_has_workspace || !self.current_sync_auto_sync || self.current_sync_in_progress {
            return;
        }
        let interval_secs = self.current_sync_interval.max(60) as i64;
        let now = Self::now_epoch_seconds();
        let elapsed = now.saturating_sub(self.current_last_sync_time);
        if self.current_last_sync_time > 0 && elapsed < interval_secs {
            self.debug_log("sync", "auto_sync_skipped_foreground", &format!("elapsed={}s, min={}s", elapsed, interval_secs));
            return;
        }
        self.trigger_auto_sync("auto_sync_on_foreground");
    }

// AppBackend::trigger_auto_sync
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

// AppBackend::perform_sync_internal
    pub(crate) fn perform_sync_internal(&mut self, trigger: &str, silent_success: bool) {
        if self.current_sync_in_progress {
            self.debug_log("sync", "perform_sync_skipped", "sync already running");
            if trigger == "manual" {
                self.current_sync_action_result = "同步正在进行中，请稍候。".to_string();
                self.sync_action_completed();
            }
            return;
        }
        let token_present = !self.current_sync_token.is_empty();
        let masked_url = mask_sync_error(&self.current_sync_remote_url);
        self.debug_log(
            "sync",
            "perform_sync_start",
            &format!("trigger={}, remote_url={}, branch={}, token_present={}", trigger, masked_url, self.current_sync_branch, token_present)
        );
        let workspace_path = self.current_workspace.clone();
        if workspace_path.is_empty() {
            self.current_sync_action_result = "请先打开工作区".to_string();
            self.sync_action_completed();
            self.debug_error("sync", "perform_sync_failed", "workspace_empty");
            return;
        }

        if self.current_sync_remote_url.is_empty() {
            self.current_sync_status = "error".to_string();
            self.current_sync_action_result = "同步失败: 未配置远程仓库 URL，请先填写并保存配置。".to_string();
            self.sync_status_changed();
            self.sync_action_completed();
            self.debug_error("sync", "perform_sync_failed", "remote_url_empty");
            return;
        }

        if self.current_sync_token.is_empty() {
            self.current_sync_status = "error".to_string();
            self.current_sync_action_result = "同步失败: 未配置 GitHub 访问令牌 (Token)，请先填写并保存配置。".to_string();
            self.sync_status_changed();
            self.sync_action_completed();
            self.debug_error("sync", "perform_sync_failed", "token_empty");
            return;
        }

        if self.current_sync_branch.is_empty() {
            self.current_sync_branch = "main".to_string();
            self.sync_config_changed();
        }

        self.current_sync_status = "syncing".to_string();
        self.current_sync_in_progress = true;
        self.sync_status_changed();

        self.flush_writing_stats();
        self.current_sync_action_result = if silent_success { "后台同步中...".to_string() } else { "正在同步...".to_string() };

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |outcome: SyncTaskOutcome| {
            qptr.as_pinned().map(|this| {
                let mut this = this.borrow_mut();
                this.handle_sync_outcome(outcome);
            });
        });

        thread::spawn(move || {
            let api = WriterCoreApi::new(&workspace_path);
            let config = match api.load_sync_config() {
                Ok(c) => c,
                Err(e) => {
                    callback(SyncTaskOutcome {
                        sync_status: "error".to_string(),
                        action_result: format!("无法读取同步配置: {}", e),
                    });
                    return;
                }
            };
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
                            let m = format!(
                                "同步成功\n上传: {} 个文件\n下载: {} 个文件",
                                result.uploaded_files.len(),
                                result.downloaded_files.len()
                            );
                            ("success".to_string(), m)
                        }
                        "latest_wins_applied" => {
                            let m = format!(
                                "同步完成 (已自动按最新时间选择版本)\n\n上传: {} 个文件\n下载: {} 个文件\n本地删除: {} 个文件\n远端删除: {} 个文件\n覆盖: {} 个文件",
                                result.uploaded_files.len(),
                                result.downloaded_files.len(),
                                result.local_deletes.len(),
                                result.remote_deletes.len(),
                                result.overwritten_files.len()
                            );
                            ("success".to_string(), m)
                        }
                        "no_changes" => {
                            ("success".to_string(), "同步完成：本地和远端均已是最新状态，无须更新。".to_string())
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
                            let cat = sync_error_category(e);
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

                    callback(SyncTaskOutcome {
                        sync_status: status.to_string(),
                        action_result: msg,
                    });
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
                        format!("同步操作失败:\n{}", mask_sync_error(&err_str))
                    };
                    callback(SyncTaskOutcome {
                        sync_status: cat,
                        action_result,
                    });
                }
            }
        });
    }

}
