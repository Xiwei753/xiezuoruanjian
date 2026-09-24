// =============================================================================
// sync_backend.rs — 网络同步与远程诊断领域 QObject 后端适配层
// =============================================================================
//
// 引用了什么：
// - super::*：引入 AppBackend 核心后端的全部方法与结构体。
// - crate::backend::AppRef：用于安全访问全局 AppBackend 指针以读取/更新网络同步状态。
//
// 干什么的：
// - 实现 SyncBackend 结构体，作为 QML 中 "syncBackend" 对象的桥梁。
// - 负责同步密钥安全存取与网络同步配置的管理（URL、分支、令牌、代理等）。
// - 执行手动同步、运行网络诊断、和同步计划预热，通过 UUID 并行安全锁机制（operation_id & operation_kind），杜绝多个异步流的输出竞态冲突。
// - 接收多线程同步结果 outcomes，提取并序列化为包含 { operation_id, operation_kind, status, summary, details } 的结构化 JSON 并通过 sync_operation_state 属性单向通知 QML 渲染，严格守卫逻辑与显示文案分离边界。
//
// 被什么引用：
// - 被 apps/Linux_qt/src/backend/mod.rs 引用，用于实例化同步后端并绑定为 QML 全局上下文属性。
// =============================================================================

mod sync_operations;

use super::*;
use crate::backend::AppRef;
use crate::backend::DomainSnapshot;
use crate::sync_bridge::{
    determine_diagnostics_status, mask_sync_error, sync_error_category_from_code, SyncTaskOutcome,
};

/// Linux 同步操作共用的 profile 准备失败。
///
/// 三步任一步失败都携带原始错误字符串。`summary_key` 给 QML i18n，
/// `raw_error` 给 raw_error 字段（调用方负责 mask_sync_error）。
pub(crate) enum SyncProfileError {
    LoadConfig(String),
    LoadSecrets(String),
    SetOverride(String),
}

impl SyncProfileError {
    pub(crate) fn summary_key(&self) -> &'static str {
        match self {
            Self::LoadConfig(_) => "error.load_sync_config_failed",
            Self::LoadSecrets(_) => "error.load_sync_secrets_failed",
            Self::SetOverride(_) => "error.set_sync_secrets_override_failed",
        }
    }
    pub(crate) fn raw_error(&self) -> &str {
        match self {
            Self::LoadConfig(s) | Self::LoadSecrets(s) | Self::SetOverride(s) => s,
        }
    }
}

/// Linux 同步操作共用的 profile 准备。
///
/// 职责（严格只做这三步，不要复制到三个入口）：
/// 1. 用本次新建的 WriterCoreApi 调 load_sync_config()；
/// 2. 调 load_sync_secrets()（从 Linux SecureStorage / 本地兼容存储拿已提交凭据）；
/// 3. 立刻 set_sync_secrets_override(secrets)（本次操作级快照）；
/// 4. 返回 config。
///
/// `api` 是每次后台操作临时新建、操作结束即 drop 的 WriterCoreApi，
/// 所以 override 生命周期天然就是单次操作，不需要做成全局状态。
///
/// 协议级单测见 tests/sync_profile_override.rs。
pub(crate) fn prepare_sync_profile(
    api: &writer_core::api::WriterCoreApi,
) -> Result<writer_core::api::types::SyncConfigDto, SyncProfileError> {
    let config = api
        .load_sync_config()
        .map_err(|e| SyncProfileError::LoadConfig(e.to_string()))?;
    let secrets = api
        .load_sync_secrets()
        .map_err(|e| SyncProfileError::LoadSecrets(e.to_string()))?;
    api.set_sync_secrets_override(secrets)
        .map_err(|e| SyncProfileError::SetOverride(e.to_string()))?;
    Ok(config)
}

#[allow(non_snake_case)] // Qt QML naming convention
#[derive(QObject, Default)]
pub struct SyncBackend {
    #[allow(dead_code)]
    base: qt_base_class!(trait QObject),
    #[allow(dead_code)]
    sync_enabled: qt_property!(bool; READ sync_enabled WRITE set_sync_enabled NOTIFY sync_config_changed),
    #[allow(dead_code)]
    sync_backend_type: qt_property!(QString; READ sync_backend_type WRITE set_sync_backend_type NOTIFY sync_config_changed),
    #[allow(dead_code)]
    sync_remote_url: qt_property!(QString; READ sync_remote_url WRITE set_sync_remote_url NOTIFY sync_config_changed),
    #[allow(dead_code)]
    sync_branch: qt_property!(QString; READ sync_branch WRITE set_sync_branch NOTIFY sync_config_changed),
    #[allow(dead_code)]
    sync_auto_sync: qt_property!(bool; READ sync_auto_sync WRITE set_sync_auto_sync NOTIFY sync_config_changed),
    #[allow(dead_code)]
    sync_interval: qt_property!(u32; READ sync_interval WRITE set_sync_interval NOTIFY sync_config_changed),
    #[allow(dead_code)]
    sync_username: qt_property!(QString; READ sync_username WRITE set_sync_username NOTIFY sync_config_changed),
    #[allow(dead_code)]
    has_sync_token: qt_property!(bool; READ has_sync_token NOTIFY sync_config_changed),
    #[allow(dead_code)]
    sync_operation_state: qt_property!(QString; READ sync_operation_state NOTIFY sync_action_completed),
    #[allow(dead_code)]
    sync_status: qt_property!(QString; READ sync_status WRITE set_sync_status NOTIFY sync_status_changed),
    #[allow(dead_code)]
    sync_in_progress: qt_property!(bool; READ sync_in_progress NOTIFY sync_status_changed),
    #[allow(dead_code)]
    sync_can_run: qt_property!(bool; READ sync_can_run NOTIFY sync_status_changed),
    manual_sync_pending: qt_property!(bool; READ manual_sync_pending NOTIFY sync_status_changed),
    #[allow(dead_code)]
    sync_block_reason: qt_property!(QString; READ sync_block_reason NOTIFY sync_status_changed),
    #[allow(dead_code)]
    sync_config_changed: qt_signal!(),
    #[allow(dead_code)]
    sync_action_completed: qt_signal!(),
    #[allow(dead_code)]
    sync_status_changed: qt_signal!(),
    // Issue #754 评论 5814866116 改动2: 同步真正应用到当前工作区内容时发出，
    // 由 QML onSync_content_applied 刷新正文/树。只表示"真实同步已应用"，
    // 不承载 workspace 状态属性。
    #[allow(dead_code)]
    sync_content_applied: qt_signal!(),
    #[allow(dead_code)]
    set_sync_token: qt_method!(fn(&mut self, token: QString)),
    #[allow(dead_code)]
    load_sync_config: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    save_sync_config: qt_method!(fn(&mut self) -> bool),
    #[allow(dead_code)]
    perform_sync_dry_run: qt_method!(fn(&mut self) -> QString),
    #[allow(dead_code)]
    perform_sync: qt_method!(fn(&mut self) -> QString),
    #[allow(dead_code)]
    perform_sync_diagnostics: qt_method!(fn(&mut self) -> QString),
    #[allow(dead_code)]
    open_workspace_dir: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    copy_text_to_clipboard: qt_method!(fn(&mut self, text: QString) -> QString),
    // Issue #757 评论 5818193510 第 4 点：冲突列表/预览/解决动作暴露给 QML。
    // 全部返回结构化 JSON envelope（ResultEnvelope<serde_json::Value>），
    // QML 端 JSON.parse 后按 success 分支取 data.conflicts / data.preview / data.resolved。
    #[allow(dead_code)]
    list_sync_conflicts: qt_method!(fn(&mut self, project_id: QString) -> QString),
    #[allow(dead_code)]
    load_sync_conflict_preview:
        qt_method!(fn(&mut self, project_id: QString, path: QString) -> QString),
    #[allow(dead_code)]
    resolve_conflict_keep_local:
        qt_method!(fn(&mut self, project_id: QString, path: QString) -> QString),
    #[allow(dead_code)]
    resolve_conflict_take_remote:
        qt_method!(fn(&mut self, project_id: QString, path: QString) -> QString),
    #[allow(dead_code)]
    resolve_conflict_mark_merged:
        qt_method!(fn(&mut self, project_id: QString, path: QString) -> QString),
    app: AppRef,
}

impl SyncBackend {
    pub fn new(app: AppRef) -> Self {
        Self {
            app,
            ..Default::default()
        }
    }

    /// 异步同步结果的统一处理入口。
    ///
    /// 后台线程的 queued callback 通过 QPointer<SyncBackend> 进入此方法，
    /// 而不是直接 QPointer<AppBackend> borrow_mut。这样：
    /// 1. with_app_mut 在 mutation 完成后自动刷新 DomainSnapshot；
    /// 2. SyncBackend 自己发 sync_status_changed / sync_action_completed，
    ///    QML 监听的 SyncBackend signal 能正确触发。
    pub(crate) fn handle_outcome(&mut self, outcome: SyncTaskOutcome) {
        let qptr = QPointer::from(&*self);
        // Issue #754 评论 5816335613: 明确区分成功与 AppBackend 借用失败。
        // 借用失败直接 return，不发任何"已完成"类 signal——
        // StatusOnly 只表示 outcome 已成功进入 AppBackend 并完成处理、但无工作区内容变化，
        // 不能拿来表示基础设施失败。这与 main 原行为一致（仅 is_ok() 才发信号），
        // 避免把"handle_sync_outcome 根本没执行 / DomainSnapshot 没刷新"伪装成同步完成。
        let effect = match self.with_app_mut(|app| app.handle_sync_outcome(outcome, Some(qptr))) {
            Ok(effect) => effect,
            Err(_) => {
                crate::backend::app_backend::debug_error_static(
                    "sync_backend",
                    "BORROW_CONFLICT",
                    "sync outcome skipped due to AppBackend borrow conflict",
                );
                return;
            }
        };
        self.sync_status_changed();
        self.sync_action_completed();
        if effect == sync_operations::SyncOutcomeEffect::ContentChanged {
            self.sync_content_applied();
        }
    }

    fn with_app<R>(
        &self,
        f: impl FnOnce(&AppBackend) -> R,
    ) -> Result<R, crate::backend::AppBorrowError> {
        self.app.with_app(f)
    }
    fn with_app_mut<R>(
        &self,
        f: impl FnOnce(&mut AppBackend) -> R,
    ) -> Result<R, crate::backend::AppBorrowError> {
        self.app.with_app_mut(f)
    }
    fn snap(&self) -> std::cell::Ref<'_, DomainSnapshot> {
        self.app.snapshot().borrow()
    }
    fn sync_enabled(&self) -> bool {
        self.snap().sync_enabled
    }
    fn set_sync_enabled(&mut self, val: bool) {
        if self.with_app_mut(|app| app.set_sync_enabled(val)).is_ok() {
            self.sync_config_changed();
        }
    }
    fn sync_backend_type(&self) -> QString {
        self.with_app(|app| app.sync_backend_type())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn set_sync_backend_type(&mut self, val: QString) {
        if self
            .with_app_mut(|app| app.set_sync_backend_type(val))
            .is_ok()
        {
            self.sync_config_changed();
        }
    }
    fn sync_remote_url(&self) -> QString {
        self.with_app(|app| app.sync_remote_url())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn set_sync_remote_url(&mut self, val: QString) {
        if self
            .with_app_mut(|app| app.set_sync_remote_url(val))
            .is_ok()
        {
            self.sync_config_changed();
        }
    }
    fn sync_branch(&self) -> QString {
        self.with_app(|app| app.sync_branch())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn set_sync_branch(&mut self, val: QString) {
        if self.with_app_mut(|app| app.set_sync_branch(val)).is_ok() {
            self.sync_config_changed();
        }
    }
    fn sync_auto_sync(&self) -> bool {
        self.snap().sync_auto_sync
    }
    fn set_sync_auto_sync(&mut self, val: bool) {
        if self.with_app_mut(|app| app.set_sync_auto_sync(val)).is_ok() {
            self.sync_config_changed();
        }
    }
    fn sync_interval(&self) -> u32 {
        self.snap().sync_interval
    }
    fn set_sync_interval(&mut self, val: u32) {
        if self.with_app_mut(|app| app.set_sync_interval(val)).is_ok() {
            self.sync_config_changed();
        }
    }
    fn sync_username(&self) -> QString {
        self.with_app(|app| app.sync_username())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn set_sync_username(&mut self, val: QString) {
        if self.with_app_mut(|app| app.set_sync_username(val)).is_ok() {
            self.sync_config_changed();
        }
    }
    fn has_sync_token(&self) -> bool {
        self.snap().has_sync_token
    }
    fn sync_operation_state(&self) -> QString {
        self.with_app(|app| app.sync_operation_state())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn sync_status(&self) -> QString {
        self.with_app(|app| app.sync_status())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn sync_in_progress(&self) -> bool {
        self.snap().sync_in_progress
    }
    fn set_sync_status(&mut self, val: QString) {
        if self.with_app_mut(|app| app.set_sync_status(val)).is_ok() {
            self.sync_status_changed();
        }
    }
    fn sync_can_run(&self) -> bool {
        self.snap().sync_can_run
    }
    fn manual_sync_pending(&self) -> bool {
        self.snap().manual_sync_pending
    }
    fn sync_block_reason(&self) -> QString {
        self.with_app(|app| app.sync_block_reason())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn set_sync_token(&mut self, token: QString) {
        if self.with_app_mut(|app| app.set_sync_token(token)).is_ok() {
            self.sync_config_changed();
        }
    }
    fn load_sync_config(&mut self) {
        if self.with_app_mut(|app| app.load_sync_config()).is_ok() {
            self.sync_config_changed();
            self.sync_status_changed();
        }
    }
    fn save_sync_config(&mut self) -> bool {
        let result = self.with_app_mut(|app| app.save_sync_config());
        if result.is_ok() {
            self.sync_config_changed();
        }
        match result {
            Ok(r) => r,
            Err(_) => {
                crate::backend::app_backend::debug_error_static(
                    "sync_backend",
                    "BORROW_CONFLICT",
                    "save_sync_config skipped due to borrow conflict",
                );
                false
            }
        }
    }
    fn perform_sync_dry_run(&mut self) -> QString {
        let qptr = QPointer::from(&*self);
        let result = self.with_app_mut(|app| app.perform_sync_dry_run(Some(qptr)));
        if result.is_ok() {
            self.sync_status_changed();
            self.sync_action_completed();
        }
        result.unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn perform_sync(&mut self) -> QString {
        let qptr = QPointer::from(&*self);
        let result = self.with_app_mut(|app| app.perform_sync(Some(qptr)));
        if result.is_ok() {
            self.sync_status_changed();
        }
        result.unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn perform_sync_diagnostics(&mut self) -> QString {
        let qptr = QPointer::from(&*self);
        let result = self.with_app_mut(|app| app.perform_sync_diagnostics(Some(qptr)));
        if result.is_ok() {
            self.sync_status_changed();
            self.sync_action_completed();
        }
        result.unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn open_workspace_dir(&mut self) {
        if self.with_app_mut(|app| app.open_workspace_dir()).is_err() {
            crate::backend::app_backend::debug_error_static(
                "sync_backend",
                "BORROW_CONFLICT",
                "open_workspace_dir skipped due to borrow conflict",
            );
        }
    }
    fn copy_text_to_clipboard(&mut self, text: QString) -> QString {
        self.with_app_mut(|app| app.copy_text_to_clipboard(text))
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }

    // ── Issue #757 评论 5818193510 第 4 点：冲突列表/预览/解决动作 ──
    //
    // 平台层只做 Core API 调用 + JSON envelope 序列化，不复制同步状态机。
    // QML 端不自己拼磁盘路径读 conflicts.json，全部通过这些方法拿数据。
    // envelope 线格式：{ success, data: { conflicts | preview | resolved }, errorCode, ... }。

    /// 列出当前项目的所有未解决冲突。
    ///
    /// 返回 `ResultEnvelope<{ conflicts: SyncConflictDto[] }>` JSON 字符串。
    fn list_sync_conflicts(&mut self, project_id: QString) -> QString {
        let pid = project_id.to_string();
        let result = self.with_app(|app| app.core_api().map(|api| api.list_sync_conflicts(&pid)));
        match result {
            Ok(Some(Ok(conflicts))) => {
                let data = serde_json::json!({ "conflicts": conflicts });
                writer_core::api::ResultEnvelope::success(data)
                    .to_json_string()
                    .into()
            }
            Ok(Some(Err(error))) => {
                writer_core::api::ResultEnvelope::<serde_json::Value>::error(error)
                    .to_json_string()
                    .into()
            }
            Ok(None) => crate::backend::json_utils::envelope_error_json(
                writer_core::api::WriterError::Other("workspace not initialized".to_string()),
            )
            .into(),
            Err(_) => crate::backend::json_utils::borrow_conflict_error_json().into(),
        }
    }

    /// 加载单个冲突的本地/远端预览。
    ///
    /// 返回 `ResultEnvelope<{ preview: SyncConflictPreviewDto }>` JSON 字符串。
    fn load_sync_conflict_preview(&mut self, project_id: QString, path: QString) -> QString {
        let pid = project_id.to_string();
        let conflict_path = path.to_string();
        let result = self.with_app(|app| {
            app.core_api()
                .map(|api| api.load_sync_conflict_preview(&pid, &conflict_path))
        });
        match result {
            Ok(Some(Ok(preview))) => {
                let data = serde_json::json!({ "preview": preview });
                writer_core::api::ResultEnvelope::success(data)
                    .to_json_string()
                    .into()
            }
            Ok(Some(Err(error))) => {
                writer_core::api::ResultEnvelope::<serde_json::Value>::error(error)
                    .to_json_string()
                    .into()
            }
            Ok(None) => crate::backend::json_utils::envelope_error_json(
                writer_core::api::WriterError::Other("workspace not initialized".to_string()),
            )
            .into(),
            Err(_) => crate::backend::json_utils::borrow_conflict_error_json().into(),
        }
    }

    /// 冲突解决动作分派 — 三个公开 resolve 方法共用此实现。
    ///
    /// 成功后发 `sync_status_changed` + `sync_action_completed`，通知 QML 刷新冲突列表。
    /// `action`：`"keep_local"` / `"take_remote"` / `"mark_merged"`。
    fn resolve_conflict_dispatch(
        &mut self,
        project_id: QString,
        path: QString,
        action: &str,
    ) -> QString {
        let pid = project_id.to_string();
        let conflict_path = path.to_string();
        let result = self.with_app(|app| {
            app.core_api().map(|api| match action {
                "keep_local" => api.resolve_conflict_keep_local(&pid, &conflict_path),
                "take_remote" => api.resolve_conflict_take_remote(&pid, &conflict_path),
                _ => api.resolve_conflict_mark_merged(&pid, &conflict_path),
            })
        });
        // Issue #757 评论 5820327136 第 1 点：take_remote 的 applied_live bool 透传到
        // 此处。只在 take_remote 且 Core 确实立即修改了 live 正文时触发编辑器重载
        // （sync_content_applied）。老数据 fallback（pending_take_remote）此时 live 正文
        // 未变，不应触发。keep_local / mark_merged 不修改 live 正文，applied_live 固定 false。
        let (envelope, applied_live) = match result {
            Ok(Some(Ok(ok))) => (
                writer_core::api::ResultEnvelope::success(serde_json::json!({ "resolved": ok })),
                ok,
            ),
            Ok(Some(Err(error))) => (
                writer_core::api::ResultEnvelope::<serde_json::Value>::error(error),
                false,
            ),
            Ok(None) => (
                writer_core::api::ResultEnvelope::<serde_json::Value>::error(
                    writer_core::api::WriterError::Other("workspace not initialized".to_string()),
                ),
                false,
            ),
            Err(_) => return crate::backend::json_utils::borrow_conflict_error_json().into(),
        };
        let json: QString = envelope.to_json_string().into();
        if envelope.success {
            self.sync_status_changed();
            self.sync_action_completed();
            // Issue #757 评论 5819894306 第 2 点：take_remote 的 BothChanged 原子替换
            // 正文、RemoteDeleted 移走正文，当前编辑器仍持有旧正文，需触发
            // onSync_content_applied 走现有 refreshStateImmediate + reconcileActiveChapter
            // 链路重载。keep_local / mark_merged 不替换/删除 live 正文，不需要此信号。
            // 只在 take_remote 且 applied_live=true 时发（#757 评论 5820327136 第 1 点）。
            if action == "take_remote" && applied_live {
                self.sync_content_applied();
            }
        }
        json
    }

    fn resolve_conflict_keep_local(&mut self, project_id: QString, path: QString) -> QString {
        self.resolve_conflict_dispatch(project_id, path, "keep_local")
    }

    fn resolve_conflict_take_remote(&mut self, project_id: QString, path: QString) -> QString {
        self.resolve_conflict_dispatch(project_id, path, "take_remote")
    }

    fn resolve_conflict_mark_merged(&mut self, project_id: QString, path: QString) -> QString {
        self.resolve_conflict_dispatch(project_id, path, "mark_merged")
    }
}

impl AppBackend {
    // AppBackend::sync_status
    pub(crate) fn sync_status(&self) -> QString {
        self.current_sync_status.clone().into()
    }

    // AppBackend::set_sync_status
    pub(crate) fn set_sync_status(&mut self, val: QString) {
        self.current_sync_status = val.to_string();
    }

    // AppBackend::refresh_sync_status_from_config
    pub(crate) fn refresh_sync_status_from_config(&mut self) {
        if !self.current_has_data_root {
            self.current_sync_status = "no_workspace".to_string();
            return;
        }
        let has_remote = !self.current_sync_remote_url.is_empty();
        if !has_remote || !self.current_sync_enabled {
            self.current_sync_status = "not_configured".to_string();
        } else {
            self.current_sync_status = "configured_not_tested".to_string();
        }
    }

    // AppBackend::set_sync_enabled
    pub(crate) fn set_sync_enabled(&mut self, val: bool) {
        self.current_sync_enabled = val;
    }

    // AppBackend::sync_backend_type
    pub(crate) fn sync_backend_type(&self) -> QString {
        self.current_sync_backend_type.clone().into()
    }

    // AppBackend::set_sync_backend_type
    pub(crate) fn set_sync_backend_type(&mut self, val: QString) {
        self.current_sync_backend_type = val.to_string();
    }

    // AppBackend::sync_remote_url
    pub(crate) fn sync_remote_url(&self) -> QString {
        self.current_sync_remote_url.clone().into()
    }

    // AppBackend::set_sync_remote_url
    pub(crate) fn set_sync_remote_url(&mut self, val: QString) {
        self.current_sync_remote_url = val.to_string();
    }

    // AppBackend::sync_branch
    pub(crate) fn sync_branch(&self) -> QString {
        self.current_sync_branch.clone().into()
    }

    // AppBackend::set_sync_branch
    pub(crate) fn set_sync_branch(&mut self, val: QString) {
        self.current_sync_branch = val.to_string();
    }

    // AppBackend::set_sync_auto_sync
    pub(crate) fn set_sync_auto_sync(&mut self, val: bool) {
        self.current_sync_auto_sync = val;
    }

    // AppBackend::set_sync_interval
    pub(crate) fn set_sync_interval(&mut self, val: u32) {
        self.current_sync_interval = val;
    }

    // AppBackend::sync_username
    pub(crate) fn sync_username(&self) -> QString {
        self.current_sync_username.clone().into()
    }

    // AppBackend::set_sync_username
    pub(crate) fn set_sync_username(&mut self, val: QString) {
        self.current_sync_username = val.to_string();
    }

    // AppBackend::sync_block_reason
    pub(crate) fn sync_block_reason(&self) -> QString {
        if !self.current_has_data_root || self.current_data_root.is_empty() {
            return "sync.block.no_workspace".into();
        }

        if let Some(api) = self.core_api() {
            if let Ok(cap) = api.get_sync_capability() {
                if !cap.can_run {
                    return cap
                        .block_message_key
                        .unwrap_or_else(|| "sync.block.unknown".to_string())
                        .into();
                }
            }
        }

        "".into()
    }

    // AppBackend::set_sync_token
    pub(crate) fn set_sync_token(&mut self, val: QString) {
        self.current_sync_token = val.to_string();
    }

    // AppBackend::sync_action_result
    pub(crate) fn sync_operation_state(&self) -> QString {
        self.current_sync_operation_state.clone().into()
    }

    // AppBackend::perform_sync_diagnostics
    pub(crate) fn perform_sync_diagnostics(
        &mut self,
        sync_qptr: Option<QPointer<SyncBackend>>,
    ) -> QString {
        self.debug_log("sync", "perform_sync_diagnostics_start", "");
        let data_root = self.current_data_root.clone();
        let projects_root = self.current_projects_root.clone();

        let op_id = uuid::Uuid::new_v4().to_string();
        // single-flight 拦截：busy 时拒绝诊断，不覆盖正在运行的操作的 operation_id。
        if self.current_sync_in_progress {
            let state = writer_core::api::SyncOperationStateDto {
                operation_id: op_id.clone(),
                operation_kind: "diagnose".to_string(),
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
        self.current_sync_operation_kind = "diagnose".to_string();

        if data_root.is_empty() {
            let state = writer_core::api::SyncOperationStateDto {
                operation_id: op_id.clone(),
                operation_kind: "diagnose".to_string(),
                status_code: "error".to_string(),
                phase_key: None,
                summary_key: Some("sync.block.no_workspace".to_string()),
                summary_args: std::collections::HashMap::new(),
                counts: writer_core::api::SyncOperationCountsDto::default(),
                raw_error: Some("workspace_empty".to_string()),
            };
            self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();
            self.debug_error("sync", "perform_sync_diagnostics_failed", "workspace_empty");
            return op_id.into();
        }

        self.current_sync_status = "syncing".to_string();
        self.current_sync_in_progress = true;

        let state = writer_core::api::SyncOperationStateDto {
            operation_id: op_id.clone(),
            operation_kind: "diagnose".to_string(),
            status_code: "syncing".to_string(),
            phase_key: Some("sync.phase.diagnose".to_string()),
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
                    operation_kind: "diagnose".to_string(),
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
                    "perform_sync_diagnostics_failed",
                    "no_workspace_git_layout",
                );
                return op_id.into();
            }
        };

        // Issue #729：为新同步创建取消令牌并捕获当前 workspace generation。
        self.current_sync_cancel_token = Some(std::sync::Arc::new(
            writer_core::sync::SyncCancellationToken::new(),
        ));
        let workspace_generation = self.current_workspace_generation;
        // Issue #729 评论 5763441474：捕获 data_root 用于回调身份校验。
        let data_root_capture = data_root.clone();

        let app_qptr = QPointer::from(&*self);
        let callback = sync_operations::make_outcome_callback(app_qptr, sync_qptr);

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
                            operation_kind: "diagnose".to_string(),
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

                match api.perform_full_sync_diagnostics(config) {
                    Ok(result) => {
                        let status = determine_diagnostics_status(&result.diagnostics);

                        let state = writer_core::api::SyncOperationStateDto {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "diagnose".to_string(),
                            status_code: status.to_string(),
                            phase_key: None,
                            summary_key: if result.diagnostics.success {
                                Some("sync.result.diagnose_success".to_string())
                            } else {
                                Some("sync.result.diagnose_failed".to_string())
                            },
                            summary_args: std::collections::HashMap::new(),
                            counts: writer_core::api::SyncOperationCountsDto::default(),
                            raw_error: result.diagnostics.raw_error.clone(),
                        };

                        SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            sync_status: status.to_string(),
                            action_result: serde_json::to_string(&state).unwrap_or_default(),
                            workspace_generation,
                            data_root: data_root_capture.clone(),
                        }
                    }
                    Err(e) => {
                        let status = sync_error_category_from_code(None, &e.to_string());
                        let state = writer_core::api::SyncOperationStateDto {
                            operation_id: op_id_capture.clone(),
                            operation_kind: "diagnose".to_string(),
                            status_code: status.to_string(),
                            phase_key: None,
                            summary_key: Some("sync.result.diagnose_failed".to_string()),
                            summary_args: std::collections::HashMap::new(),
                            counts: writer_core::api::SyncOperationCountsDto::default(),
                            raw_error: Some(mask_sync_error(&e.to_string())),
                        };
                        SyncTaskOutcome {
                            operation_id: op_id_capture.clone(),
                            sync_status: status,
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
                        operation_kind: "diagnose".to_string(),
                        status_code: "fatal_error".to_string(),
                        phase_key: None,
                        summary_key: Some("error.sync_diagnose_panic".to_string()),
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

    // AppBackend::load_sync_config
    pub(crate) fn load_sync_config(&mut self) {
        self.debug_log("sync", "load_sync_config_start", "");
        if let Some(api) = self.core_api() {
            let config_opt = api.load_sync_config().ok();
            // Issue #645：token 从 provider_secrets → ProviderSecretsDto::GitHub 提取。
            let token_opt = api
                .load_sync_secrets()
                .ok()
                .and_then(|s| s.provider_secrets)
                .map(|ps| match ps {
                    writer_core::api::types::ProviderSecretsDto::GitHub { token } => token,
                });
            if let Some(config) = config_opt {
                self.current_sync_enabled = config.enabled;
                self.current_sync_backend_type = config.active_provider.clone();
                // Issue #645：GitHub 字段从 provider_config → ProviderConfigDto::GitHub 提取。
                let (gh_remote_url, gh_branch, gh_username) = match &config.provider_config {
                    Some(writer_core::api::types::ProviderConfigDto::GitHub {
                        remote_url,
                        branch,
                        username,
                        ..
                    }) => (remote_url.clone(), branch.clone(), username.clone()),
                    None => (String::new(), String::new(), String::new()),
                };
                self.current_sync_remote_url = gh_remote_url;
                self.current_sync_branch = if gh_branch.is_empty() {
                    "main".to_string()
                } else {
                    gh_branch
                };
                self.current_sync_auto_sync = config.auto_sync;
                self.current_sync_interval = config.sync_interval_seconds;
                self.current_sync_username = gh_username;
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
            self.current_sync_status = "no_workspace".to_string();
            self.debug_warn("sync", "load_sync_config_failed", "core_not_initialized");
        }
    }

    // AppBackend::save_sync_config
    pub(crate) fn save_sync_config(&mut self) -> bool {
        self.debug_log("sync", "save_sync_config_start", "");
        let mut error_state: Option<writer_core::api::SyncOperationStateDto> = None;
        if let Some(api) = self.core_api() {
            let net = crate::backend::app_backend::current_network_state();
            // Issue #645：SyncConfigDto 改为 provider-neutral 结构，
            // GitHub 字段通过 provider_config: Option<ProviderConfigDto::GitHub> 携带。
            let mut c = api
                .load_sync_config()
                .unwrap_or(writer_core::api::types::SyncConfigDto {
                    enabled: false,
                    active_provider: "github_api".to_string(),
                    provider_config: Some(writer_core::api::types::ProviderConfigDto::GitHub {
                        remote_url: "".to_string(),
                        branch: "main".to_string(),
                        username: "".to_string(),
                        transport: "https_token".to_string(),
                    }),
                    auto_sync: false,
                    sync_interval_seconds: 300,
                    has_network_permission: net.is_connected,
                    has_network_state_permission: true,
                });

            let raw_url = self.current_sync_remote_url.clone();
            let parsed = writer_core::sync::sanitize_remote_url(&raw_url);

            c.enabled = self.current_sync_enabled;
            c.active_provider = match self.current_sync_backend_type.as_str() {
                "webdav" | "s3" | "local_folder" | "git" | "github_api" => {
                    self.current_sync_backend_type.clone()
                }
                _ => "github_api".to_string(),
            };
            let new_branch = if self.current_sync_branch.is_empty() {
                "main".to_string()
            } else {
                self.current_sync_branch.clone()
            };
            let mut new_username = self.current_sync_username.clone();
            if let Some(ref extracted_user) = parsed.extracted_username {
                if new_username.is_empty() {
                    new_username = extracted_user.clone();
                }
            }
            // GitHub 字段统一写入 provider_config（线格式 transport 为 "https_token"）。
            c.provider_config = Some(writer_core::api::types::ProviderConfigDto::GitHub {
                remote_url: parsed.sanitized_url.clone(),
                branch: new_branch,
                username: new_username,
                transport: "https_token".to_string(),
            });
            c.auto_sync = self.current_sync_auto_sync;
            c.sync_interval_seconds = self.current_sync_interval;

            // Issue #645：SyncSecretsDto 改为 provider_secrets → ProviderSecretsDto::GitHub { token }。
            let mut s =
                api.load_sync_secrets()
                    .unwrap_or(writer_core::api::types::SyncSecretsDto {
                        provider_secrets: None,
                    });
            let new_token = if let Some(ref extracted_token) = parsed.extracted_token {
                Some(extracted_token.clone())
            } else if self.current_sync_token.is_empty() {
                None
            } else {
                Some(self.current_sync_token.clone())
            };
            s.provider_secrets = new_token
                .map(|token| writer_core::api::types::ProviderSecretsDto::GitHub { token });

            let config_result = api.save_sync_config(c);
            let config_envelope = match config_result {
                Ok(data) => writer_core::api::ResultEnvelope::success_with_changes(
                    data,
                    vec!["sync_config.json".to_string()],
                    vec![writer_core::api::ChangedEntityDto {
                        entity_type: "SyncConfigSaved".to_string(),
                        entity_id: None,
                    }],
                ),
                Err(error) => writer_core::api::ResultEnvelope::<bool>::error(error),
            };
            if !config_envelope.success {
                let error_code = config_envelope.error_code.as_deref().unwrap_or("UNKNOWN");
                let message_key = config_envelope.message_key.as_deref().unwrap_or("");
                let resolved_key = if !message_key.is_empty() {
                    crate::backend::message_key_mapper::resolve_message_key(message_key).to_string()
                } else {
                    "error.other".to_string()
                };
                error_state = Some(writer_core::api::SyncOperationStateDto {
                    operation_id: String::new(),
                    operation_kind: "save_config".to_string(),
                    status_code: "error".to_string(),
                    phase_key: None,
                    summary_key: Some("error.save_sync_config_failed".to_string()),
                    summary_args: std::collections::HashMap::new(),
                    counts: writer_core::api::SyncOperationCountsDto::default(),
                    raw_error: Some(format!("{} ({})", resolved_key, error_code)),
                });
            } else {
                let secrets_result = api.save_sync_secrets(s);
                let secrets_envelope = match secrets_result {
                    Ok(data) => writer_core::api::ResultEnvelope::success_with_changes(
                        data,
                        vec!["sync_secrets.local.json".to_string()],
                        vec![writer_core::api::ChangedEntityDto {
                            entity_type: "SyncConfigSaved".to_string(),
                            entity_id: None,
                        }],
                    ),
                    Err(error) => writer_core::api::ResultEnvelope::<bool>::error(error),
                };
                if !secrets_envelope.success {
                    let error_code = secrets_envelope.error_code.as_deref().unwrap_or("UNKNOWN");
                    let message_key = secrets_envelope.message_key.as_deref().unwrap_or("");
                    let resolved_key = if !message_key.is_empty() {
                        crate::backend::message_key_mapper::resolve_message_key(message_key)
                            .to_string()
                    } else {
                        "error.other".to_string()
                    };
                    error_state = Some(writer_core::api::SyncOperationStateDto {
                        operation_id: String::new(),
                        operation_kind: "save_config".to_string(),
                        status_code: "error".to_string(),
                        phase_key: None,
                        summary_key: Some("error.save_sync_secrets_failed".to_string()),
                        summary_args: std::collections::HashMap::new(),
                        counts: writer_core::api::SyncOperationCountsDto::default(),
                        raw_error: Some(format!("{} ({})", resolved_key, error_code)),
                    });
                }
            }
        } else {
            error_state = Some(writer_core::api::SyncOperationStateDto {
                operation_id: String::new(),
                operation_kind: "save_config".to_string(),
                status_code: "error".to_string(),
                phase_key: None,
                summary_key: Some("error.core_not_initialized".to_string()),
                summary_args: std::collections::HashMap::new(),
                counts: writer_core::api::SyncOperationCountsDto::default(),
                raw_error: None,
            });
        }

        if let Some(state) = error_state {
            let msg = format!(
                "{}: {}",
                state.summary_key.as_deref().unwrap_or("error.other"),
                state.raw_error.as_deref().unwrap_or("")
            );
            self.set_error(&msg);
            self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();
            self.debug_error("sync", "save_sync_config_failed", &msg);
            return false;
        }

        self.refresh_sync_status_from_config();
        let state = writer_core::api::SyncOperationStateDto {
            operation_id: String::new(),
            operation_kind: "save_config".to_string(),
            status_code: "success".to_string(),
            phase_key: None,
            summary_key: Some("sync.result.save_config_success".to_string()),
            summary_args: std::collections::HashMap::new(),
            counts: writer_core::api::SyncOperationCountsDto::default(),
            raw_error: None,
        };
        self.current_sync_operation_state = serde_json::to_string(&state).unwrap_or_default();
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
