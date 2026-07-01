// =============================================================================
// sync_backend.rs — 网络同步与远程诊断领域 QObject 后端适配层
// =============================================================================
//
// 引用了什么：
// - super::*：引入 AppBackend 核心后端的全部方法与结构体。
// - crate::backend::SafeAppPtr：用于安全访问全局 AppBackend 指针以读取/更新网络同步状态。
//
// 干什么的：
// - 实现 SyncBackend 结构体，作为 QML 中 "syncBackend" 对象的桥梁。
// - 负责同步密钥安全存取与网络同步配置的管理（URL、分支、令牌、代理等）。
// - 执行手动同步、运行网络诊断、和同步计划预热，通过 UUID 并行安全锁机制（operation_id & operation_kind），杜绝多个异步流的输出竞态冲突。
// - 接收多线程同步结果 outcomes，提取并序列化为包含 { operation_id, operation_kind, status, summary, details } 的结构化 JSON 并通过 sync_operation_state 属性单向通知 QML 渲染，严格守卫逻辑与显示文案分离边界。
//
// 被什么引用：
// - 被 apps/desktop/src/backend/mod.rs 引用，用于实例化同步后端并绑定为 QML 全局上下文属性。
// =============================================================================

mod sync_operations;

use super::*;
use crate::backend::SafeAppPtr;
use crate::sync_bridge::{
    determine_diagnostics_status, format_diagnostics_message, mask_sync_error, sync_error_category,
    SyncTaskOutcome,
};
use writer_core::api::WriterCoreApi;

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
    sync_username: qt_property!(QString; READ sync_username WRITE set_sync_username NOTIFY sync_config_changed),
    has_sync_token: qt_property!(bool; READ has_sync_token NOTIFY sync_config_changed),
    sync_operation_state: qt_property!(QString; READ sync_operation_state NOTIFY sync_action_completed),
    sync_status: qt_property!(QString; READ sync_status WRITE set_sync_status NOTIFY sync_status_changed),
    sync_in_progress: qt_property!(bool; READ sync_in_progress NOTIFY sync_status_changed),
    has_workspace: qt_property!(bool; READ has_workspace NOTIFY workspace_state_changed),
    sync_config_changed: qt_signal!(),
    sync_action_completed: qt_signal!(),
    sync_status_changed: qt_signal!(),
    workspace_state_changed: qt_signal!(),
    set_sync_token: qt_method!(fn(&mut self, token: QString)),
    load_sync_config: qt_method!(fn(&mut self)),
    save_sync_config: qt_method!(fn(&mut self) -> bool),
    perform_sync_dry_run: qt_method!(fn(&mut self) -> QString),
    perform_sync: qt_method!(fn(&mut self) -> QString),
    perform_sync_diagnostics: qt_method!(fn(&mut self) -> QString),
    request_auto_sync: qt_method!(fn(&mut self, reason: QString)),
    maybe_auto_sync_on_foreground: qt_method!(fn(&mut self)),
    open_workspace_dir: qt_method!(fn(&mut self)),
    copy_text_to_clipboard: qt_method!(fn(&mut self, text: QString) -> QString),
    app: SafeAppPtr,
}

impl SyncBackend {
    pub fn new(app: SafeAppPtr) -> Self {
        Self {
            app,
            ..Default::default()
        }
    }
    fn with_app<R>(&self, default: R, f: impl FnOnce(&AppBackend) -> R) -> R {
        if let Some(app) = self.app.get() {
            // SAFETY: pointer was set from QObjectBox-pinned AppBackend in
            // BackendRuntime::new; null-guarded above; single-threaded (Rc).
            unsafe { f(&*app) }
        } else {
            crate::backend::app_backend::debug_error_static(
                "sync",
                "BACKEND_LINK_BROKEN",
                "app pointer is null",
            );
            default
        }
    }
    fn with_app_mut<R>(&mut self, default: R, f: impl FnOnce(&mut AppBackend) -> R) -> R {
        if let Some(app) = self.app.get() {
            // SAFETY: same as with_app; &mut is safe because callers hold
            // &mut self, preventing aliasing within this backend.
            unsafe { f(&mut *app) }
        } else {
            crate::backend::app_backend::debug_error_static(
                "sync",
                "BACKEND_LINK_BROKEN",
                "app pointer is null",
            );
            default
        }
    }
    fn sync_enabled(&self) -> bool {
        self.with_app(false, |app| app.sync_enabled())
    }
    fn set_sync_enabled(&mut self, val: bool) {
        self.with_app_mut((), |app| app.set_sync_enabled(val));
        self.sync_config_changed();
    }
    fn sync_backend_type(&self) -> QString {
        self.with_app("".into(), |app| app.sync_backend_type())
    }
    fn set_sync_backend_type(&mut self, val: QString) {
        self.with_app_mut((), |app| app.set_sync_backend_type(val));
        self.sync_config_changed();
    }
    fn sync_remote_url(&self) -> QString {
        self.with_app("".into(), |app| app.sync_remote_url())
    }
    fn set_sync_remote_url(&mut self, val: QString) {
        self.with_app_mut((), |app| app.set_sync_remote_url(val));
        self.sync_config_changed();
    }
    fn sync_branch(&self) -> QString {
        self.with_app("main".into(), |app| app.sync_branch())
    }
    fn set_sync_branch(&mut self, val: QString) {
        self.with_app_mut((), |app| app.set_sync_branch(val));
        self.sync_config_changed();
    }
    fn sync_auto_sync(&self) -> bool {
        self.with_app(false, |app| app.sync_auto_sync())
    }
    fn set_sync_auto_sync(&mut self, val: bool) {
        self.with_app_mut((), |app| app.set_sync_auto_sync(val));
        self.sync_config_changed();
    }
    fn sync_interval(&self) -> u32 {
        self.with_app(300, |app| app.sync_interval())
    }
    fn set_sync_interval(&mut self, val: u32) {
        self.with_app_mut((), |app| app.set_sync_interval(val));
        self.sync_config_changed();
    }
    fn sync_username(&self) -> QString {
        self.with_app("".into(), |app| app.sync_username())
    }
    fn set_sync_username(&mut self, val: QString) {
        self.with_app_mut((), |app| app.set_sync_username(val));
        self.sync_config_changed();
    }
    fn has_sync_token(&self) -> bool {
        self.with_app(false, |app| app.has_sync_token())
    }
    fn sync_operation_state(&self) -> QString {
        self.with_app("".into(), |app| app.sync_operation_state())
    }
    fn sync_status(&self) -> QString {
        self.with_app("not_configured".into(), |app| app.sync_status())
    }
    fn sync_in_progress(&self) -> bool {
        self.with_app(false, |app| app.sync_in_progress())
    }
    fn set_sync_status(&mut self, val: QString) {
        self.with_app_mut((), |app| app.set_sync_status(val));
        self.sync_status_changed();
    }
    fn has_workspace(&self) -> bool {
        self.with_app(false, |app| app.has_workspace())
    }
    fn set_sync_token(&mut self, token: QString) {
        self.with_app_mut((), |app| app.set_sync_token(token));
        self.sync_config_changed();
    }
    fn load_sync_config(&mut self) {
        self.with_app_mut((), |app| app.load_sync_config());
        self.sync_config_changed();
        self.sync_status_changed();
    }
    fn save_sync_config(&mut self) -> bool {
        let ok = self.with_app_mut(false, |app| app.save_sync_config());
        self.sync_config_changed();
        ok
    }
    fn perform_sync_dry_run(&mut self) -> QString {
        let id = self.with_app_mut("".into(), |app| app.perform_sync_dry_run());
        self.sync_status_changed();
        self.sync_action_completed();
        id
    }
    fn perform_sync(&mut self) -> QString {
        let id = self.with_app_mut("".into(), |app| app.perform_sync());
        self.sync_status_changed();
        id
    }
    fn perform_sync_diagnostics(&mut self) -> QString {
        let id = self.with_app_mut("".into(), |app| app.perform_sync_diagnostics());
        self.sync_status_changed();
        self.sync_action_completed();
        id
    }
    fn request_auto_sync(&mut self, reason: QString) {
        self.with_app_mut((), |app| app.request_auto_sync(reason));
        self.sync_status_changed();
    }
    fn maybe_auto_sync_on_foreground(&mut self) {
        self.with_app_mut((), |app| app.maybe_auto_sync_on_foreground());
        self.sync_status_changed();
    }
    fn open_workspace_dir(&mut self) {
        self.with_app_mut((), |app| app.open_workspace_dir());
    }
    fn copy_text_to_clipboard(&mut self, text: QString) -> QString {
        self.with_app_mut("{}".into(), |app| app.copy_text_to_clipboard(text))
    }
}

impl AppBackend {
    // AppBackend::sync_status
    pub(crate) fn sync_status(&self) -> QString {
        self.current_sync_status.clone().into()
    }

    // AppBackend::sync_in_progress
    pub(crate) fn sync_in_progress(&self) -> bool {
        self.current_sync_in_progress
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
            self.current_sync_status = "configured_not_tested".to_string();
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
    pub(crate) fn sync_operation_state(&self) -> QString {
        #[derive(serde::Serialize)]
        struct SyncOperationState {
            operation_id: String,
            operation_kind: String,
            status: String,
            summary: String,
            details: String,
        }

        let state = SyncOperationState {
            operation_id: self.current_sync_operation_id.clone(),
            operation_kind: self.current_sync_operation_kind.clone(),
            status: self.current_sync_status.clone(),
            summary: self.current_sync_operation_state.clone(),
            details: String::new(),
        };

        serde_json::to_string(&state).unwrap_or_default().into()
    }

    // AppBackend::perform_sync_diagnostics
    pub(crate) fn perform_sync_diagnostics(&mut self) -> QString {
        self.debug_log("sync", "perform_sync_diagnostics_start", "");
        let workspace_path = self.current_workspace.clone();
        if workspace_path.is_empty() {
            self.current_sync_operation_state = "请先打开工作区".to_string();
            self.sync_action_completed();
            self.debug_error("sync", "perform_sync_diagnostics_failed", "workspace_empty");
            return "".into();
        }

        let op_id = uuid::Uuid::new_v4().to_string();
        self.current_sync_operation_id = op_id.clone();
        self.current_sync_operation_kind = "diagnose".to_string();

        self.current_sync_status = "syncing".to_string();
        self.sync_status_changed();
        self.current_sync_operation_state = "正在诊断...".to_string();

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
                            operation_kind: "diagnose".to_string(),
                            sync_status: "error".to_string(),
                            action_result: format!("无法加载同步配置: {}", e),
                        };
                    }
                };
                config.android_has_internet_permission = true;
                config.android_has_access_network_state_permission = true;

                match api.perform_sync_diagnostics(config) {
                    Ok(result) => {
                        let status = determine_diagnostics_status(&result);
                        let msg = format_diagnostics_message(&result);

                        SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "diagnose".to_string(),
                            sync_status: status.to_string(),
                            action_result: msg,
                        }
                    }
                    Err(e) => SyncTaskOutcome {
                        operation_id: op_id_capture.clone(),
                        operation_kind: "diagnose".to_string(),
                        sync_status: sync_error_category(&e.to_string()),
                        action_result: format!(
                            "诊断过程发生错误:\n{}",
                            mask_sync_error(&e.to_string())
                        ),
                    },
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
                        operation_kind: "diagnose".to_string(),
                        sync_status: "fatal_error".to_string(),
                        action_result: format!("同步诊断发生致命错误 (Panic):\n{}", panic_msg),
                    });
                }
            }
        });

        op_id.into()
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
                self.current_sync_branch = if config.branch.is_empty() {
                    "main".to_string()
                } else {
                    config.branch.clone()
                };
                self.current_sync_auto_sync = config.auto_sync;
                self.current_sync_interval = config.sync_interval_seconds;
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
                &format!(
                    "enabled={}, remote_url={}, branch={}, token_present={}",
                    self.current_sync_enabled, masked_url, self.current_sync_branch, token_present
                ),
            );
        } else {
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
                    username: "".to_string(),
                    android_has_internet_permission: false,
                    android_has_access_network_state_permission: false,
                });

            let raw_url = self.current_sync_remote_url.clone();
            let parsed = writer_core::sync::sanitize_remote_url(&raw_url);

            c.enabled = self.current_sync_enabled;
            c.backend_type = match self.current_sync_backend_type.as_str() {
                "webdav" | "s3" | "local_folder" | "git" | "github_api" => {
                    self.current_sync_backend_type.clone()
                }
                _ => "github_api".to_string(),
            };
            c.remote_url = parsed.sanitized_url.clone();
            c.branch = if self.current_sync_branch.is_empty() {
                "main".to_string()
            } else {
                self.current_sync_branch.clone()
            };
            c.auto_sync = self.current_sync_auto_sync;
            c.sync_interval_seconds = self.current_sync_interval;
            c.username = self.current_sync_username.clone();

            if let Some(ref extracted_user) = parsed.extracted_username {
                if c.username.is_empty() {
                    c.username = extracted_user.clone();
                }
            }

            let mut s = api
                .load_sync_secrets()
                .unwrap_or(writer_core::api::types::SyncSecretsDto { token: None });
            if let Some(ref extracted_token) = parsed.extracted_token {
                s.token = Some(extracted_token.clone());
            } else if self.current_sync_token.is_empty() {
                s.token = None;
            } else {
                s.token = Some(self.current_sync_token.clone());
            }

            let config_result = api.save_sync_config(c);
            let config_json = match config_result {
                Ok(data) => writer_core::api::ResultEnvelope::success_with_changes(
                    data,
                    vec!["sync_config.json".to_string()],
                    vec![writer_core::api::ChangedEntityDto {
                        entity_type: "SyncConfigSaved".to_string(),
                        entity_id: None,
                    }],
                ),
                Err(error) => writer_core::api::ResultEnvelope::<bool>::error(error),
            }
            .to_json_string();
            let config_envelope: serde_json::Value = serde_json::from_str(&config_json)
                .unwrap_or(serde_json::json!({"success": false, "errorCode": "JSON_ERROR"}));
            if config_envelope["success"] != true {
                let error_code = config_envelope["errorCode"].as_str().unwrap_or("UNKNOWN");
                let message_key = config_envelope["messageKey"].as_str().unwrap_or("");
                let resolved_key = if !message_key.is_empty() {
                    crate::backend::message_key_mapper::resolve_message_key(message_key).to_string()
                } else {
                    "error.other".to_string()
                };
                error_msg = Some(format!("{} ({})", resolved_key, error_code));
            } else {
                let secrets_result = api.save_sync_secrets(s);
                let secrets_json = match secrets_result {
                    Ok(data) => writer_core::api::ResultEnvelope::success_with_changes(
                        data,
                        vec!["sync_secrets.local.json".to_string()],
                        vec![writer_core::api::ChangedEntityDto {
                            entity_type: "SyncConfigSaved".to_string(),
                            entity_id: None,
                        }],
                    ),
                    Err(error) => writer_core::api::ResultEnvelope::<bool>::error(error),
                }
                .to_json_string();
                let secrets_envelope: serde_json::Value = serde_json::from_str(&secrets_json)
                    .unwrap_or(serde_json::json!({"success": false, "errorCode": "JSON_ERROR"}));
                if secrets_envelope["success"] != true {
                    let error_code = secrets_envelope["errorCode"].as_str().unwrap_or("UNKNOWN");
                    let message_key = secrets_envelope["messageKey"].as_str().unwrap_or("");
                    let resolved_key = if !message_key.is_empty() {
                        crate::backend::message_key_mapper::resolve_message_key(message_key)
                            .to_string()
                    } else {
                        "error.other".to_string()
                    };
                    error_msg = Some(format!("{} ({})", resolved_key, error_code));
                }
            }
        } else {
            error_msg = Some("Core 未初始化".to_string());
        }

        if let Some(msg) = error_msg {
            self.set_error(&msg);
            self.current_sync_operation_state = msg.clone();
            self.sync_action_completed();
            self.debug_error("sync", "save_sync_config_failed", &msg);
            return false;
        }

        self.refresh_sync_status_from_config();
        self.current_sync_operation_state = "配置保存成功".to_string();
        self.sync_action_completed();
        let token_present = !self.current_sync_token.is_empty();
        let masked_url = mask_sync_error(&self.current_sync_remote_url);
        self.debug_log(
            "sync",
            "save_sync_config_success",
            &format!(
                "enabled={}, remote_url={}, branch={}, token_present={}",
                self.current_sync_enabled, masked_url, self.current_sync_branch, token_present
            ),
        );
        true
    }
}

// Sync execution methods (perform_sync, auto_sync, handle_sync_outcome, etc.)
// are defined in sync_operations.rs (submodule of sync_backend).
