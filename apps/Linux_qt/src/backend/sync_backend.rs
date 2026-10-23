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

fn normalize_manual_sync_source(source: &str) -> String {
    match source.trim() {
        "starmap" | "hub" | "writing" | "settings" => source.trim().to_string(),
        _ => "unknown".to_string(),
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
    last_sync_report: qt_property!(QString; READ last_sync_report NOTIFY sync_status_changed),
    // Issue #948：Core 持久化的最近一次全量同步历史状态（FullSyncStateDto JSON，只读）。
    // 与 last_sync_report 相互独立：旧格式失败可能没有分组报告。无历史时为空串。
    // 这是"持久化的最近一次历史结果"，不是"本轮刚失败"。
    #[allow(dead_code)]
    last_full_sync_state: qt_property!(QString; READ last_full_sync_state NOTIFY sync_status_changed),
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
    // Issue #762 评论 5826175490：全局冲突数 + 通知 signal。
    // sync_conflict_count 是持久状态（跨所有作品的未解决冲突数），
    // 与"当前有没有正在跑一轮同步"是两回事。不依赖 sync_operation_state 的最终 status。
    #[allow(dead_code)]
    sync_conflict_count: qt_property!(u32; READ sync_conflict_count NOTIFY sync_conflicts_changed),
    #[allow(dead_code)]
    sync_conflicts_changed: qt_signal!(),
    #[allow(dead_code)]
    set_sync_token: qt_method!(fn(&mut self, token: QString)),
    #[allow(dead_code)]
    load_sync_config: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    save_sync_config: qt_method!(fn(&mut self) -> bool),
    #[allow(dead_code)]
    perform_sync_dry_run: qt_method!(fn(&mut self) -> QString),
    #[allow(dead_code)]
    record_manual_sync_request: qt_method!(fn(&mut self, source: QString) -> QString),
    #[allow(dead_code)]
    perform_sync: qt_method!(fn(&mut self, source: QString, request_id: QString) -> QString),
    // Issue #983：自动同步的 QML 边界。QML 只负责定时唤醒；到期判定、工作区身份
    // 校验和真正启动都在 AppBackend 内完成，不把自动触发冒充成手动请求。
    #[allow(dead_code)]
    auto_sync_due: qt_method!(fn(&mut self) -> bool),
    #[allow(dead_code)]
    start_auto_sync: qt_method!(fn(&mut self) -> QString),
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
    list_sync_conflicts:
        qt_method!(fn(&mut self, target_kind: QString, project_id: QString) -> QString),
    // Issue #762 评论 5826175490：跨作品全局冲突列表入口。
    #[allow(dead_code)]
    list_all_sync_conflicts: qt_method!(fn(&mut self) -> QString),
    #[allow(dead_code)]
    load_sync_conflict_preview: qt_method!(
        fn(&mut self, target_kind: QString, project_id: QString, path: QString) -> QString
    ),
    #[allow(dead_code)]
    resolve_conflict_keep_local: qt_method!(
        fn(&mut self, target_kind: QString, project_id: QString, path: QString) -> QString
    ),
    #[allow(dead_code)]
    resolve_conflict_take_remote: qt_method!(
        fn(&mut self, target_kind: QString, project_id: QString, path: QString) -> QString
    ),
    #[allow(dead_code)]
    resolve_conflict_mark_merged: qt_method!(
        fn(&mut self, target_kind: QString, project_id: QString, path: QString) -> QString
    ),
    // Issue #762 评论 5826175490：全局冲突数缓存，u32 Default 为 0，符合 #[derive(Default)]。
    current_sync_conflict_count: u32,
    // Issue #762 评论 5828791004：冲突列表 fingerprint 缓存。
    // count 不变但列表内容变了（如 A 冲突解决、B 新产生冲突）时也能检测到变化并通知 UI。
    current_sync_conflict_fingerprint: String,
    app: AppRef,
}

impl SyncBackend {
    pub fn new(app: AppRef) -> Self {
        Self {
            app,
            ..Default::default()
        }
    }

    pub(crate) fn mark_core_attempt_started(&mut self, operation_id: &str) {
        if self
            .with_app_mut(|app| app.mark_sync_core_attempt_started(operation_id))
            .is_ok()
        {
            self.sync_action_completed();
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
        // Issue #948 第 3 点：终态快照在 move 进 AppBackend 前留下，结束后核对
        // Core 是否真的产出了最新 FullSyncReport。
        let outcome_status = outcome.sync_status.clone();
        let outcome_operation_id = outcome.operation_id.clone();
        let outcome_action_result = outcome.action_result.clone();
        let outcome_source = self
            .with_app(|app| app.sync_source_for_operation_id(&outcome_operation_id))
            .ok()
            .flatten()
            .unwrap_or_else(|| "unknown".to_string());
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
        // Issue #948 第 3 点：完成时只发通知，QML 的 getter（sync_operation_state /
        // last_sync_report / last_full_sync_state）重新直读 Core。运行终态与持久历史
        // 只有一个状态源，不在 Qt 侧另存一套同步历史。
        self.sync_status_changed();
        self.sync_action_completed();
        self.log_sync_operation_terminal(
            &outcome_operation_id,
            &outcome_source,
            &outcome_action_result,
        );
        if effect == sync_operations::SyncOutcomeEffect::ContentChanged {
            self.sync_content_applied();
        }
        // Issue #762 评论 5826175490 / 5828791004：同步结束后刷新全局冲突状态。
        // 冲突是持久状态，无论同步成功还是冲突都需刷新。
        self.refresh_sync_conflict_state();
        self.log_full_sync_report_gap(&outcome_status, &outcome_action_result);
    }

    /// Issue #948 第 3 点：本轮同步确实结束但 Core 没有产出最新 FullSyncReport 时，
    /// 记录操作 ID / 阶段 / 失败目标 / 错误码，便于沿当前分组报告写入链路追真正原因。
    ///
    /// 只做诊断记录：空报告既不等于本轮成功也不等于本轮失败，不改任何状态、
    /// 不让 UI 猜测失败原因（UI 仍按本轮 operation_id 的终态展示）。
    fn log_full_sync_report_gap(&self, outcome_status: &str, action_result: &str) {
        // 只有真实同步操作才会写 FullSyncReport；诊断 / 保存配置等终态跳过。
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(action_result) else {
            return;
        };
        if parsed.get("operation_kind").and_then(|v| v.as_str()) != Some("sync") {
            return;
        }
        // 成功类终态本来就没有失败分组可写，不要求报告存在。
        if matches!(
            outcome_status,
            "success"
                | "no_changes"
                | "latest_wins_applied"
                | "branch_missing_recovered"
                | "configured_not_tested"
        ) {
            return;
        }
        // 只有"明确读到没有报告"才算缺口；读取失败（借用冲突 / IO 错误）属于
        // 基础设施问题，不在这里误报成报告缺失。
        let report_missing = matches!(
            self.with_app(|app| app.core_api().map(|api| api.load_latest_full_sync_report())),
            Ok(Some(Ok(None)))
        );
        if !report_missing {
            return;
        }
        let phase = parsed
            .get("phase_key")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let failed_targets = parsed
            .get("summary_args")
            .and_then(|v| v.get("failed_targets"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let raw_error = parsed
            .get("raw_error")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        crate::backend::app_backend::debug_error_static(
            "sync",
            "full_sync_report_missing_after_sync",
            &format!(
                "operation_id={}, status={}, phase={}, failed_targets={}, raw_error={}",
                parsed
                    .get("operation_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
                outcome_status,
                phase,
                failed_targets,
                raw_error
            ),
        );
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

    fn log_sync_operation_terminal(&self, operation_id: &str, source: &str, action_result: &str) {
        let Ok(state) = serde_json::from_str::<serde_json::Value>(action_result) else {
            return;
        };
        if state.get("operation_kind").and_then(|value| value.as_str()) != Some("sync") {
            return;
        }
        let status = state
            .get("status_code")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        let phase = state
            .get("phase_key")
            .and_then(|value| value.as_str())
            .unwrap_or("remote_sync");
        let error_category = crate::sync_bridge::sync_error_category_from_code(Some(status), "");
        let raw_error = state
            .get("raw_error")
            .and_then(|value| value.as_str())
            .map(mask_sync_error)
            .unwrap_or_default();
        let failed_targets = state
            .get("summary_args")
            .and_then(|value| value.get("failed_targets"))
            .and_then(|value| value.as_str())
            .map(mask_sync_error)
            .unwrap_or_default();

        let persisted_report = self
            .with_app(|app| {
                app.core_api().map(|api| {
                    let last_attempt = api
                        .load_full_sync_state()
                        .ok()
                        .flatten()
                        .and_then(|full_state| full_state.last_attempt_time);
                    let report = api.load_latest_full_sync_report().ok().flatten();
                    (last_attempt, report)
                })
            })
            .ok()
            .flatten();
        let report_failures = persisted_report
            .and_then(|(last_attempt, report)| {
                report.filter(|report| Some(report.attempted_at) == last_attempt)
            })
            .map(|report| {
                report
                    .failures
                    .into_iter()
                    .map(|failure| {
                        format!(
                            "{}:{} phase={} category={} detail={}",
                            failure.target_kind,
                            mask_sync_error(&failure.path),
                            failure.phase,
                            failure.error_category,
                            mask_sync_error(&failure.detail)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .unwrap_or_default();

        crate::backend::app_backend::debug_log_static(
            "sync",
            "perform_sync_terminal",
            &format!(
                "operation_id={}, source={}, started=true, queued=false, status={}, phase={}, error_category={}, raw_error={}, failed_targets={}, report_failures={}",
                operation_id,
                source,
                status,
                phase,
                error_category,
                raw_error,
                failed_targets,
                report_failures
            ),
        );
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
    fn last_sync_report(&self) -> QString {
        self.with_app(|app| app.last_sync_report_json())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    /// Issue #948：Core 持久化的全量同步历史状态（只读 JSON 快照）。
    ///
    /// getter 每次经 `with_app` 直读 Core 的 FullSyncState（overall_status /
    /// last_attempt_time / last_success_time / failed_targets），与诊断包导出的
    /// `full_sync_state.json` 同源，不另存一套 Qt 侧同步历史。
    /// Core 无历史时返回空串，不从 AppSyncState.status 猜整轮结果。
    fn last_full_sync_state(&self) -> QString {
        self.with_app(|app| app.last_full_sync_state_json())
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
        // Issue #782 评论 5855709706：重新加载配置只刷新配置字段（sync_config_changed），
        // 不把"重新读取同一份配置"等同于"同步状态发生变化"。只有当 restore_sync_status_from_runtime
        // 真的修正了状态时才发 sync_status_changed。
        let prev_status = self
            .with_app(|app| app.sync_status().to_string())
            .unwrap_or_default();
        if self.with_app_mut(|app| app.load_sync_config()).is_ok() {
            self.sync_config_changed();
            let new_status = self
                .with_app(|app| app.sync_status().to_string())
                .unwrap_or_default();
            if new_status != prev_status {
                self.sync_status_changed();
            }
        }
        // Issue #762 评论 5828791004：工作区打开 / load sync config 后也刷新一次冲突状态，
        // 不能让应用刚启动时 sync_conflict_count 永远先是 0，直到用户手动同步或 resolve 才更新。
        self.refresh_sync_conflict_state();
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
    fn record_manual_sync_request(&mut self, source: QString) -> QString {
        let source = normalize_manual_sync_source(&source.to_string());
        let request_id = uuid::Uuid::new_v4().to_string();
        crate::backend::app_backend::debug_log_static(
            "sync",
            "manual_sync_request_received",
            &format!(
                "request_id={}, source={}, stage=clicked",
                request_id, source
            ),
        );
        if self
            .with_app_mut(|app| app.record_manual_sync_request(&source, &request_id))
            .is_err()
        {
            crate::backend::app_backend::debug_error_static(
                "sync_backend",
                "manual_sync_request_record_failed",
                &format!(
                    "request_id={}, source={}, phase=request_record, category=BORROW_CONFLICT",
                    request_id, source
                ),
            );
        }
        request_id.into()
    }

    fn perform_sync(&mut self, source: QString, request_id: QString) -> QString {
        // Issue #762 评论 5826175490 / 5828791004：同步开始前刷新全局冲突状态，
        // 让 QML 在同步运行期间就显示当前持久冲突状态。
        self.refresh_sync_conflict_state();
        let source = normalize_manual_sync_source(&source.to_string());
        let request_id = request_id.to_string();
        let qptr = QPointer::from(&*self);
        crate::backend::app_backend::debug_log_static(
            "sync",
            "perform_sync_request_received",
            &format!("request_id={}, source={}", request_id, source),
        );
        let result =
            self.with_app_mut(|app| app.perform_sync_for_request(Some(qptr), &source, &request_id));
        match result {
            Ok((operation_id, started, queued, status)) => {
                crate::backend::app_backend::debug_log_static(
                    "sync",
                    "perform_sync_request_dispatched",
                    &format!(
                        "request_id={}, operation_id={}, source={}, started={}, queued={}, status={}",
                        request_id, operation_id, source, started, queued, status
                    ),
                );
                self.sync_status_changed();
                if !started && !queued {
                    // Preflight failures update the operation state synchronously and need an
                    // immediate notification; active operations notify on their terminal callback.
                    self.sync_action_completed();
                }
                operation_id
            }
            Err(_) => {
                crate::backend::app_backend::debug_error_static(
                    "sync_backend",
                    "perform_sync_request_failed",
                    &format!(
                        "request_id={}, source={}, operation_id=, phase=app_backend_dispatch, category=BORROW_CONFLICT",
                        request_id, source
                    ),
                );
                self.sync_status_changed();
                self.sync_action_completed();
                crate::backend::json_utils::borrow_conflict_error_json().into()
            }
        }
    }
    fn auto_sync_due(&mut self) -> bool {
        match self.with_app_mut(|app| app.auto_sync_due()) {
            Ok(due) => due,
            Err(_) => {
                crate::backend::app_backend::debug_error_static(
                    "sync_backend",
                    "auto_sync_due_failed",
                    "auto sync due check skipped due to AppBackend borrow conflict",
                );
                false
            }
        }
    }

    fn start_auto_sync(&mut self) -> QString {
        let qptr = QPointer::from(&*self);
        match self.with_app_mut(|app| app.start_auto_sync(Some(qptr))) {
            Ok((operation_id, started)) => {
                if started {
                    self.sync_status_changed();
                } else if !operation_id.is_empty() {
                    // 到期检查通过但真正启动被前置条件拒绝：同步更新操作终态。
                    self.sync_status_changed();
                    self.sync_action_completed();
                }
                operation_id.into()
            }
            Err(_) => {
                crate::backend::app_backend::debug_error_static(
                    "sync_backend",
                    "auto_sync_dispatch_failed",
                    "auto sync dispatch skipped due to AppBackend borrow conflict",
                );
                QString::from("")
            }
        }
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
    fn list_sync_conflicts(&mut self, target_kind: QString, project_id: QString) -> QString {
        let tk = target_kind.to_string();
        let pid = project_id.to_string();
        let pid_opt = if tk == "app" {
            None
        } else {
            Some(pid.as_str())
        };
        let result = self.with_app(|app| {
            app.core_api()
                .map(|api| api.list_sync_conflicts(&tk, pid_opt))
        });
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

    /// 列出所有作品的所有未解决冲突（全局冲突入口）。
    ///
    /// 返回 `ResultEnvelope<{ conflicts: AllSyncConflictEntryDto[] }>` JSON 字符串。
    /// QML 端 JSON.parse 后按 projectId 分组展示。
    fn list_all_sync_conflicts(&mut self) -> QString {
        let result = self.with_app(|app| app.core_api().map(|api| api.list_all_sync_conflicts()));
        match result {
            Ok(Some(Ok(all))) => {
                let data = serde_json::json!({ "conflicts": all });
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
    fn load_sync_conflict_preview(
        &mut self,
        target_kind: QString,
        project_id: QString,
        path: QString,
    ) -> QString {
        let tk = target_kind.to_string();
        let pid = project_id.to_string();
        let pid_opt = if tk == "app" {
            None
        } else {
            Some(pid.as_str())
        };
        let conflict_path = path.to_string();
        let result = self.with_app(|app| {
            app.core_api()
                .map(|api| api.load_sync_conflict_preview(&tk, pid_opt, &conflict_path))
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
        target_kind: QString,
        project_id: QString,
        path: QString,
        action: &str,
    ) -> QString {
        let tk = target_kind.to_string();
        let pid = project_id.to_string();
        let pid_opt = if tk == "app" {
            None
        } else {
            Some(pid.as_str())
        };
        let conflict_path = path.to_string();
        let result = self.with_app(|app| {
            app.core_api().map(|api| match action {
                "keep_local" => api.resolve_conflict_keep_local(&tk, pid_opt, &conflict_path),
                "take_remote" => api.resolve_conflict_take_remote(&tk, pid_opt, &conflict_path),
                _ => api.resolve_conflict_mark_merged(&tk, pid_opt, &conflict_path),
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
            // Issue #762 评论 5826175490 / 5828791004：resolve 成功后冲突状态变化，刷新全局冲突状态。
            self.refresh_sync_conflict_state();
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

    fn resolve_conflict_keep_local(
        &mut self,
        target_kind: QString,
        project_id: QString,
        path: QString,
    ) -> QString {
        self.resolve_conflict_dispatch(target_kind, project_id, path, "keep_local")
    }

    fn resolve_conflict_take_remote(
        &mut self,
        target_kind: QString,
        project_id: QString,
        path: QString,
    ) -> QString {
        self.resolve_conflict_dispatch(target_kind, project_id, path, "take_remote")
    }

    fn resolve_conflict_mark_merged(
        &mut self,
        target_kind: QString,
        project_id: QString,
        path: QString,
    ) -> QString {
        self.resolve_conflict_dispatch(target_kind, project_id, path, "mark_merged")
    }

    // ── Issue #762 评论 5826175490 / 5828791004：全局冲突状态刷新 ──

    /// 刷新全局冲突状态。一次调用 `list_all_sync_conflicts()`，同时算 count 和 fingerprint。
    ///
    /// - count 变化 → 更新 `current_sync_conflict_count`
    /// - fingerprint 变化 → 发 `sync_conflicts_changed()` signal（哪怕 count 一样）
    /// - count 和 fingerprint 都没变 → 不发 signal
    ///
    /// 查询失败（core 未就绪 / borrow conflict）时不更新缓存、不发 signal：
    /// 保留上一份已知状态，避免把"暂时读不到"误报成"冲突已清空"。
    ///
    /// 冲突是持久状态，与同步运行状态无关。同步开始前/结束后/resolve 后/target progress 时都调。
    fn refresh_sync_conflict_state(&mut self) {
        let Some(conflicts) = self
            .with_app(|app| {
                app.core_api()
                    .and_then(|api| api.list_all_sync_conflicts().ok())
            })
            .ok()
            .flatten()
        else {
            return;
        };
        let new_count = u32::try_from(conflicts.len()).unwrap_or(u32::MAX);
        let new_fingerprint = sync_conflict_fingerprint(&conflicts);
        let count_changed = new_count != self.current_sync_conflict_count;
        let fingerprint_changed = new_fingerprint != self.current_sync_conflict_fingerprint;
        if count_changed {
            self.current_sync_conflict_count = new_count;
        }
        if fingerprint_changed {
            self.current_sync_conflict_fingerprint = new_fingerprint;
        }
        if count_changed || fingerprint_changed {
            self.sync_conflicts_changed();
        }
    }

    fn sync_conflict_count(&self) -> u32 {
        self.current_sync_conflict_count
    }

    /// progress 回调入口 — 后台线程 target 完成后通过 queued_callback 回主线程调此方法。
    /// 只刷新冲突状态，不传 progress 数据到 QML（QML 端收到 sync_conflicts_changed 后自己刷新）。
    pub(crate) fn handle_sync_target_progress(&mut self) {
        self.refresh_sync_conflict_state();
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

    /// 恢复同步运行状态（Issue #782 评论 5855709706 / 5855913889）。
    ///
    /// 替代旧的 `refresh_sync_status_from_config` 在 `load_sync_config` / `save_sync_config`
    /// 中的无条件覆盖。优先级（配置检查提前到 full_sync_state 读取之前）：
    /// 1. `current_sync_in_progress == true` → `"syncing"`
    /// 2. 没有工作区（`!current_has_data_root || current_data_root.is_empty()`）→ `"no_workspace"`
    /// 3. 当前配置 `!current_sync_enabled` 或远端地址为空 → `"not_configured"`
    /// 4. 当前配置仍有效，再读取 `core_api().load_full_sync_state()`，有历史就恢复真实 `overall_status`
    /// 5. 没有历史才 → `"configured_not_tested"`
    ///
    /// 这样"以前同步成功过、之后用户关闭同步或清空远端、再重启应用"不会从旧 `full_sync_state`
    /// 恢复成 `success`，而是正确落到 `not_configured`。重新读取同一份有效配置也不会覆盖
    /// 真实同步结果；只有完全没有同步历史且配置有效时才落到 `configured_not_tested`。
    pub(crate) fn restore_sync_status_from_runtime(&mut self) {
        if self.current_sync_in_progress {
            self.current_sync_status = "syncing".to_string();
            return;
        }
        // 第 2 步：没有工作区 → no_workspace。条件与 core_api() 门禁一致，
        // 保证第 4 步走到时 core_api() 一定返回 Some。
        if !self.current_has_data_root || self.current_data_root.is_empty() {
            self.current_sync_status = "no_workspace".to_string();
            return;
        }
        // 第 3 步：当前配置关闭同步或远端地址为空 → not_configured。
        // 提前到 full_sync_state 读取之前，避免旧 success 历史覆盖"已关闭同步"的事实。
        if !self.current_sync_enabled || self.current_sync_remote_url.is_empty() {
            self.current_sync_status = "not_configured".to_string();
            return;
        }
        // 第 4 步：当前配置仍有效，再读取 full_sync_state，有历史就恢复真实 overall_status。
        // Issue #948：这里恢复的是"持久化的最近一次历史结果"，不代表本轮刚刚失败。
        // 同步运行互斥与操作结果处理保持不变；QML 通过只读的 last_full_sync_state
        // 单独展示这次历史的最后尝试/成功时间与失败目标。
        if let Some(api) = self.core_api() {
            if let Ok(Some(state)) = api.load_full_sync_state() {
                self.current_sync_status = state.overall_status;
                return;
            }
        }
        // 第 5 步：配置有效但没有同步历史 → configured_not_tested。
        self.current_sync_status = "configured_not_tested".to_string();
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
        let Ok(mut state) =
            serde_json::from_str::<serde_json::Value>(&self.current_sync_operation_state)
        else {
            return self.current_sync_operation_state.clone().into();
        };
        let Some(object) = state.as_object_mut() else {
            return self.current_sync_operation_state.clone().into();
        };
        let operation_id = object
            .get("operation_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        object.insert(
            "core_attempt_started".to_string(),
            serde_json::Value::Bool(self.sync_core_attempt_started_for_operation_id(operation_id)),
        );
        serde_json::to_string(&state)
            .unwrap_or_else(|_| self.current_sync_operation_state.clone())
            .into()
    }

    // AppBackend::last_sync_report_json
    pub(crate) fn last_sync_report_json(&self) -> QString {
        let Some(api) = self.core_api() else {
            return QString::from("");
        };
        match api.load_latest_full_sync_report() {
            Ok(Some(report)) => serde_json::to_string(&report).unwrap_or_default().into(),
            Ok(None) | Err(_) => QString::from(""),
        }
    }

    // AppBackend::last_full_sync_state_json
    /// Issue #948：Core 持久化的最近一次全量同步历史状态（FullSyncStateDto JSON）。
    ///
    /// 只陈述 Core 已有事实：`overall_status` / `last_attempt_time` /
    /// `last_success_time` / `failed_targets`，供 QML 与诊断包共用同一状态源
    /// （`full_sync_state.json` 也直接来自本接口）。没有工作区或没有历史时返回空串，
    /// 由调用方按"无历史"处理，不回退到其它字段猜整轮结果。
    pub(crate) fn last_full_sync_state_json(&self) -> QString {
        let Some(api) = self.core_api() else {
            return QString::from("");
        };
        match api.load_full_sync_state() {
            Ok(Some(state)) => serde_json::to_string(&state).unwrap_or_default().into(),
            Ok(None) | Err(_) => QString::from(""),
        }
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
        self.current_sync_operation_kind = "diagnose".to_string();

        if data_root.is_empty() {
            let state = writer_core::api::SyncOperationStateDto {
                operation_id: op_id.clone(),
                operation_kind: "diagnose".to_string(),
                status_code: "error".to_string(),
                phase_key: None,
                summary_key: Some("sync.block.no_workspace".to_string()),
                summary_args: std::collections::HashMap::new(),
                current_target: None,
                finished_targets: 0,
                total_targets: 0,
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
                    operation_kind: "diagnose".to_string(),
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
                            current_target: None,
                            finished_targets: 0,
                            total_targets: 0,
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
                            current_target: None,
                            finished_targets: 0,
                            total_targets: 0,
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
                    writer_core::api::types::ProviderSecretsDto::GitHttps { secret } => secret,
                });
            if let Some(config) = config_opt {
                self.current_sync_enabled = config.enabled;
                self.current_sync_backend_type = config.active_provider.clone();
                // Issue #990：兼容旧 GitHub 配置与新的通用 Git HTTPS 配置。
                let (sync_remote_url, sync_branch, sync_username) = match &config.provider_config {
                    Some(writer_core::api::types::ProviderConfigDto::GitHub {
                        remote_url,
                        branch,
                        username,
                        ..
                    }) => (remote_url.clone(), branch.clone(), username.clone()),
                    Some(writer_core::api::types::ProviderConfigDto::GitHttps {
                        remote_url,
                        branch,
                        username,
                    }) => (remote_url.clone(), branch.clone(), username.clone()),
                    None => (String::new(), String::new(), String::new()),
                };
                self.current_sync_remote_url = sync_remote_url;
                self.current_sync_branch = if sync_branch.is_empty() {
                    "main".to_string()
                } else {
                    sync_branch
                };
                self.current_sync_auto_sync = config.auto_sync;
                self.current_sync_interval = config.sync_interval_seconds;
                self.current_sync_username = sync_username;
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
            self.restore_sync_status_from_runtime();
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
            // Issue #990：通用 Git HTTPS 配置；GitHub 由 Core 自动选择 Git/API。
            let mut c = api
                .load_sync_config()
                .unwrap_or(writer_core::api::types::SyncConfigDto {
                    enabled: false,
                    active_provider: "git_https".to_string(),
                    provider_config: Some(writer_core::api::types::ProviderConfigDto::GitHttps {
                        remote_url: "".to_string(),
                        branch: "main".to_string(),
                        username: "".to_string(),
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
                "webdav" | "s3" | "local_folder" | "git" | "git_https" => {
                    self.current_sync_backend_type.clone()
                }
                _ => "git_https".to_string(),
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
            // Issue #990：远端地址、分支、用户名统一写入通用 Git HTTPS 配置。
            c.provider_config = Some(writer_core::api::types::ProviderConfigDto::GitHttps {
                remote_url: parsed.sanitized_url.clone(),
                branch: new_branch,
                username: new_username,
            });
            c.auto_sync = self.current_sync_auto_sync;
            c.sync_interval_seconds = self.current_sync_interval;

            // Issue #990：密码/访问令牌统一写入通用 Git HTTPS 密钥。
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
                .map(|secret| writer_core::api::types::ProviderSecretsDto::GitHttps { secret });

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
                    current_target: None,
                    finished_targets: 0,
                    total_targets: 0,
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
                        current_target: None,
                        finished_targets: 0,
                        total_targets: 0,
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
                current_target: None,
                finished_targets: 0,
                total_targets: 0,
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

        // Issue #782 评论 5855913889：保存成功后不再调用旧的 refresh_sync_status_from_config
        // 无条件把状态写成 configured_not_tested。改走同一套 restore_sync_status_from_runtime
        // 规则：如果本次保存明确关闭同步或清空远端，落到 not_configured；配置仍有效且有
        // 同步历史则恢复真实 overall_status；没有历史才 configured_not_tested。
        self.restore_sync_status_from_runtime();
        let state = writer_core::api::SyncOperationStateDto {
            operation_id: String::new(),
            operation_kind: "save_config".to_string(),
            status_code: "success".to_string(),
            phase_key: None,
            summary_key: Some("sync.result.save_config_success".to_string()),
            summary_args: std::collections::HashMap::new(),
            current_target: None,
            finished_targets: 0,
            total_targets: 0,
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

/// 冲突列表 fingerprint —— 检测"列表内容变了但总数没变"。
///
/// Issue #762 评论 5828791004 第 2 点：只比较 `sync_conflict_count` 不够。
/// 例如作品 A 的冲突刚解决、同一轮同步作品 B 新产生一个冲突，总数一直是 1，
/// count 不变但 WritingWorkspace / SyncPage 必须刷新。
///
/// 收集每个冲突的 `target_kind + project_id + path + kind + created_at`，排序后拼接：
/// 内容集合一致（含顺序不同）时 fingerprint 相同，集合一变就不同。
/// Issue #902 第二十轮：纳入 `target_kind`，区分 app 与 project 同路径冲突。
fn sync_conflict_fingerprint(
    conflicts: &[writer_core::api::types::AllSyncConflictEntryDto],
) -> String {
    let mut entries: Vec<String> = conflicts
        .iter()
        .map(|c| {
            format!(
                "{}|{}|{}|{}|{}",
                c.target_kind, c.project_id, c.path, c.kind, c.created_at
            )
        })
        .collect();
    entries.sort();
    entries.join(";")
}

#[cfg(test)]
mod sync_conflict_fingerprint_tests {
    use super::sync_conflict_fingerprint;
    use writer_core::api::types::AllSyncConflictEntryDto;

    fn conflict(project_id: &str, path: &str, created_at: i64) -> AllSyncConflictEntryDto {
        AllSyncConflictEntryDto {
            project_id: project_id.to_string(),
            project_title: format!("{project_id}-title"),
            path: path.to_string(),
            kind: "both_changed".to_string(),
            created_at,
            target_kind: "project".to_string(),
        }
    }

    /// A 的冲突解决、B 新产生冲突：总数都是 1，fingerprint 必须变化。
    #[test]
    fn fingerprint_changes_when_list_changes_with_same_count() {
        let before = vec![conflict("A", "volumes/v1/chapters/a.md", 1_000)];
        let after = vec![conflict("B", "volumes/v1/chapters/b.md", 2_000)];
        assert_eq!(before.len(), after.len());
        assert_ne!(
            sync_conflict_fingerprint(&before),
            sync_conflict_fingerprint(&after),
            "总数相同但冲突列表换了，fingerprint 必须变化"
        );
    }

    /// 同一集合、顺序不同：fingerprint 必须相同（先排序再拼接）。
    #[test]
    fn fingerprint_is_stable_across_ordering() {
        let a = conflict("A", "volumes/v1/chapters/a.md", 1_000);
        let b = conflict("B", "volumes/v1/chapters/b.md", 2_000);
        assert_eq!(
            sync_conflict_fingerprint(&[a.clone(), b.clone()]),
            sync_conflict_fingerprint(&[b, a])
        );
    }

    /// 冲突被解决（列表变空）必须与"初始为空"区分：空列表 fingerprint 为空串。
    #[test]
    fn fingerprint_distinguishes_empty_from_non_empty() {
        assert_eq!(sync_conflict_fingerprint(&[]), "");
        assert_ne!(
            sync_conflict_fingerprint(&[conflict("A", "volumes/v1/chapters/a.md", 1_000)]),
            ""
        );
    }
}
