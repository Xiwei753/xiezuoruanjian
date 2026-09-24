// =============================================================================
// app_backend.rs — Linux_qt 客户端全局底层状态与公共桥接后端
// =============================================================================
//
// 引用了什么：
// - qmetaobject：提供 Qt JSON 对象（QJsonObject, QJsonArray）与常规 QObject 属性机制。
// - rfd::FileDialog：调用桌面系统原生文件选择对话框（如新建/打开工作区）。
// - writer_core::api::WriterCoreApi：核心库对外的统一 API 入口。
// - super::json_utils：JSON 工具函数库，用于进行 DTO ↔ QJsonObject 转换。
// - crate::*：引入 starmap_bridge, sync_bridge, writing_bridge 以调用各个领域的桥接函数。
//
// 干什么的：
// - 定义主后端 AppBackend 结构体，维护工作区路径、调试日志级别、临时剪贴板交互等全局性跨模块属性。
// - 封装并对外提供 debug_log_static 等静态日志收集入口，规范化地将运行时关键链路节点记录到磁盘和控制台。
//
// 被什么引用：
// - 被 apps/Linux_qt/src/backend/mod.rs 引用，作为核心底层状态容器，被 AppRef (Rc<RefCell<AppBackend>>) 共享至各个分域后端。
// - 被 apps/Linux_qt/src/main.rs 注册为 QML 内命名空间 "SujianApp" 下的 "AppBackend"。
// =============================================================================

use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::{QJsonArray, QJsonObject, QString};
use rfd::FileDialog;
use std::collections::HashSet;
use std::sync::OnceLock;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use writer_core::api::WriterCoreApi;
// Issue #729：同步取消令牌 + workspace generation 用于切工作区后旧同步回调身份隔离。
use std::sync::Arc;
use writer_core::sync::SyncCancellationToken;

use super::json_utils::{
    bridge_error_object, bridge_success_object, qjson_array_data_from_json, qjson_object_from_json,
    serde_to_qjson_object,
};
use super::linux_qt_layout_plan_dto::LinuxQtLayoutPlanDto;
use crate::{starmap_bridge, sync_bridge, writing_bridge};

cpp! {{
    #include <QtGlobal>
}}

/// 调试级别，与 `log` crate level 映射：
/// Error=1→log::Error, Warn=2→log::Warn, Info=3→log::Info, Debug=4→log::Debug, Trace=5→log::Trace
#[derive(PartialEq, PartialOrd, Clone, Copy, Debug)]
enum DebugLevel {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

impl DebugLevel {
    fn from_str(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "error" => DebugLevel::Error,
            "warn" => DebugLevel::Warn,
            "info" => DebugLevel::Info,
            "debug" => DebugLevel::Debug,
            "trace" => DebugLevel::Trace,
            _ => DebugLevel::Info,
        }
    }
}

/// 调试配置。`all_modules=true` 时忽略 `modules` 集合，输出所有模块日志。
struct DebugConfig {
    enabled: bool,
    qml_enabled: bool,
    modules: HashSet<String>,
    all_modules: bool,
    level: DebugLevel,
}

/// 全局调试配置，使用 OnceLock 保证只初始化一次。
/// 选择 OnceLock 而非 LazyLock/OnceCell：标准库稳定、无需额外依赖。
static DEBUG_CONFIG: OnceLock<DebugConfig> = OnceLock::new();

static LINUX_SYNC_TRANSPORT_FACTORY: OnceLock<writer_platform_api::SyncTransportFactory> =
    OnceLock::new();
static LINUX_SECURE_STORAGE: OnceLock<std::sync::Arc<dyn writer_platform_api::SecureStorage>> =
    OnceLock::new();

pub fn set_linux_sync_transport_factory(factory: writer_platform_api::SyncTransportFactory) {
    LINUX_SYNC_TRANSPORT_FACTORY.set(factory).ok();
}

pub fn set_linux_secure_storage(storage: std::sync::Arc<dyn writer_platform_api::SecureStorage>) {
    LINUX_SECURE_STORAGE.set(storage).ok();
}

pub(crate) fn current_network_state() -> writer_platform_api::NetworkState {
    writer_platform_linux::get_cached_network_state()
}

pub(crate) fn create_core_api(
    app_data_root: &str,
    projects_root: &str,
) -> std::result::Result<WriterCoreApi, writer_core::api::WriterError> {
    let sync_transport = LINUX_SYNC_TRANSPORT_FACTORY.get().cloned();
    let secure_storage = LINUX_SECURE_STORAGE.get().cloned();
    // 统一走 Core workspace bootstrap：确保 .git 存在、恢复未完成删除事务、
    // 注入正确的 GitRepoLayout。不再裸构造未 bootstrap 的 WriterCoreApi。
    writer_core::api::bootstrap::bootstrap_core_api(
        app_data_root,
        projects_root,
        sync_transport,
        secure_storage,
    )
}

/// bootstrap workspace 并返回 (WriterCoreApi, GitRepoLayout)。
///
/// 只在打开/切换 workspace 时调用一次。返回的 layout 供保存到
/// AppBackend.current_workspace_git_layout，后续普通 core_api() getter
/// 和后台同步线程用此 layout 构造 API，不再重新 bootstrap。
pub(crate) fn create_core_api_with_layout(
    app_data_root: &str,
    projects_root: &str,
) -> std::result::Result<
    (
        WriterCoreApi,
        writer_core::storage::git_repo_layout::GitRepoLayout,
    ),
    writer_core::api::WriterError,
> {
    // 先 bootstrap workspace（ensure .git + recover），拿到 layout。
    let layout =
        writer_core::api::bootstrap::bootstrap_workspace(std::path::Path::new(app_data_root))?;
    // 用 layout 构造 API，不再重新 bootstrap。
    let api = with_layout_core_api(app_data_root, projects_root, &layout);
    Ok((api, layout))
}

/// 用已保存的 GitRepoLayout 构造 WriterCoreApi，不执行 bootstrap。
///
/// 供普通 core_api() getter 使用：不再每次调用都 ensure .git + recover journal，
/// 只用打开 workspace 时已经确定的 layout 快照构造 API。
pub(crate) fn with_layout_core_api(
    app_data_root: &str,
    projects_root: &str,
    layout: &writer_core::storage::git_repo_layout::GitRepoLayout,
) -> WriterCoreApi {
    let sync_transport = LINUX_SYNC_TRANSPORT_FACTORY.get().cloned();
    let secure_storage = LINUX_SECURE_STORAGE.get().cloned();
    writer_core::api::bootstrap::with_layout_core_api(
        app_data_root,
        projects_root,
        layout,
        sync_transport,
        secure_storage,
    )
}

fn get_debug_config() -> &'static DebugConfig {
    DEBUG_CONFIG.get_or_init(|| {
        let enabled = std::env::var("WRITER_DEBUG")
            .map(|v| v == "1")
            .unwrap_or(false);
        let qml_enabled = std::env::var("WRITER_DEBUG_QML")
            .map(|v| v == "1")
            .unwrap_or(false);
        let modules_env = std::env::var("WRITER_DEBUG_MODULES").unwrap_or_default();
        let level_env = std::env::var("WRITER_DEBUG_LEVEL").unwrap_or_else(|_| "info".to_string());
        let level = DebugLevel::from_str(&level_env);
        let mut modules = HashSet::new();
        let mut all_modules = false;
        if modules_env.eq_ignore_ascii_case("all") {
            all_modules = true;
        } else {
            for m in modules_env.split(',') {
                let trimmed = m.trim().to_lowercase();
                if !trimmed.is_empty() {
                    if trimmed == "all" {
                        all_modules = true;
                    } else {
                        modules.insert(trimmed);
                    }
                }
            }
        }
        DebugConfig {
            enabled,
            qml_enabled,
            modules,
            all_modules,
            level,
        }
    })
}

/// 检测是否为空内容覆盖阻止错误。业务场景：IME 异常清空正文时，
/// Core 拒绝空内容覆盖以防止用户数据丢失。
fn is_empty_overwrite_blocked(error: &writer_core::api::error::WriterError) -> bool {
    matches!(
        error,
        writer_core::api::error::WriterError::EmptyOverwriteBlocked { .. }
    )
}

fn blocked_empty_overwrite_user_message() -> &'static str {
    "已阻止空内容覆盖，原章节内容已保留"
}

fn blocked_empty_overwrite_error_code() -> &'static str {
    "EMPTY_OVERWRITE_BLOCKED"
}

fn debug_module_enabled(module: &str) -> bool {
    let cfg = get_debug_config();
    if !cfg.enabled {
        return false;
    }
    if cfg.all_modules {
        return true;
    }
    cfg.modules.contains(&module.to_lowercase())
}

fn debug_level_enabled(module: &str, level: DebugLevel) -> bool {
    if !debug_module_enabled(module) {
        return false;
    }
    level <= get_debug_config().level
}

/// Issue #707 评论 5723616999: 改 `pub` 让 bin (main.rs) 能访问。
pub fn debug_log_static(module: &str, event: &str, message: &str) {
    if debug_level_enabled(module, DebugLevel::Info) {
        println!(
            "[SujianDebug][static][module={}][event={}] {}",
            module, event, message
        );
    }
    // 文件日志由 writer_diagnostics 接管（log::* 已被 writer_diagnostics logger 接管）。
    log::info!(target: module, "{}: {}", event, message);
}

/// Issue #707 评论 5723616999: 改 `pub` 让 bin (main.rs) 能访问。
pub fn debug_warn_static(module: &str, event: &str, message: &str) {
    if debug_level_enabled(module, DebugLevel::Warn) {
        eprintln!(
            "[SujianDebug][WARN][static][module={}][event={}] {}",
            module, event, message
        );
    }
    // WARN 级别文件日志由 writer_diagnostics 接管。
    log::warn!(target: module, "{}: {}", event, message);
}

/// Issue #707 评论 5723616999: 改 `pub` 让 bin (main.rs) 能访问。
pub fn debug_error_static(module: &str, event: &str, message: &str) {
    if debug_level_enabled(module, DebugLevel::Error) {
        eprintln!(
            "[SujianDebug][ERROR][static][module={}][event={}] {}",
            module, event, message
        );
    }
    // ERROR 级别文件日志由 writer_diagnostics 接管。
    log::error!(target: module, "{}: {}", event, message);
}

/// 记录带 origin 的结构化诊断事件 — Issue #670 评论 5651060802 第 7 节。
///
/// 与 `debug_log_static` 不同，本函数允许调用点显式传入 `origin`
/// （User / System / App），用于主题链、导航、同步等需要区分触发源的事件。
/// 普通 `log::*` 默认 `origin=App`，不需要区分触发源的事件继续走 `debug_log_static`。
///
/// `fields` 是 `(key, value)` 对，value 会转成 JSON 字符串。
pub(crate) fn record_struct_event(
    origin: writer_diagnostics::DiagnosticOrigin,
    event: &str,
    target: &str,
    fields: &[(&str, &str)],
) {
    use std::collections::BTreeMap;
    let mut field_map = BTreeMap::new();
    for (k, v) in fields {
        field_map.insert(k.to_string(), serde_json::Value::String(v.to_string()));
    }
    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        sequence: 0,
        session_id: String::new(),
        level: writer_diagnostics::DiagnosticLevel::Info,
        origin,
        event: event.to_string(),
        target: target.to_string(),
        message: None,
        fields: field_map,
    });
}

use sync_bridge::SyncTaskOutcome;

#[path = "system_utils.rs"]
mod system_utils;

#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct AppBackend {
    #[allow(dead_code)]
    base: qt_base_class!(trait QObject),

    #[allow(dead_code)]
    system_color_scheme: qt_property!(QString; READ system_color_scheme NOTIFY system_color_scheme_changed),
    #[allow(dead_code)]
    system_color_scheme_changed: qt_signal!(),

    #[allow(dead_code)]
    ai_available: qt_property!(bool; READ ai_available NOTIFY ai_available_changed),
    #[allow(dead_code)] // SAFETY: qmetaobject macro field used by Qt meta-object system
    #[allow(dead_code)]
    ai_enabled: qt_property!(bool; READ ai_enabled WRITE set_ai_enabled NOTIFY ai_enabled_changed),
    #[allow(dead_code)]
    ai_enabled_changed: qt_signal!(),
    #[allow(dead_code)]
    ai_available_changed: qt_signal!(),

    #[allow(dead_code)]
    query_system_color_scheme: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    apply_window_dark_mode: qt_method!(fn(&mut self, is_dark: bool)),
    #[allow(dead_code)]
    copy_text_to_clipboard: qt_method!(fn(&mut self, text: QString) -> QString),
    #[allow(dead_code)]
    available_screen_geometry_json: qt_method!(fn(&self) -> QString),
    #[allow(dead_code)]
    debug_qml_enabled: qt_property!(bool; READ debug_qml_enabled),
    #[allow(dead_code)]
    debug_module_enabled_qml: qt_method!(fn(&self, module: QString) -> bool),
    log_qml:
        qt_method!(fn(&self, level: QString, module: QString, event: QString, message: QString)),

    current_data_root: String,
    current_projects_root: String,
    current_has_data_root: bool,
    /// 打开 workspace 时 bootstrap 得到的 GitRepoLayout 快照。
    ///
    /// 普通 core_api() getter 用此 layout 构造 WriterCoreApi，不再每次调用
    /// 都重新 bootstrap（ensure .git + recover_storage_transactions）。
    /// 后台同步线程也 clone 此 layout 快照，避免每次同步都完整 bootstrap。
    current_workspace_git_layout: Option<writer_core::storage::git_repo_layout::GitRepoLayout>,
    /// Issue #729：workspace 身份 generation。每次成功打开工作区或重置工作区时递增。
    /// 同步线程启动时捕获此值，回调时校验是否仍等于当前 generation，
    /// 不相等说明工作区已切换，旧同步结果必须丢弃，避免污染新工作区状态。
    /// 用 `wrapping_add` 递增，避免溢出 panic。
    current_workspace_generation: u64,
    /// Issue #729：当前同步操作的取消令牌。由平台层持有，切工作区时调用 `cancel()`
    /// 标记旧同步已取消。`Option` + `take()` 在 reset_workspace_state 中消费。
    /// `Arc<SyncCancellationToken>` 是 `Send + Sync`（由 `Arc` 自动推导），可在线程间共享。
    current_sync_cancel_token: Option<Arc<SyncCancellationToken>>,
    current_save_status: String,
    current_word_count: i32,
    current_error_message: String,

    selected_project_id: Option<String>,
    selected_volume_id: Option<String>,
    selected_chapter_id: Option<String>,

    cached_tree: QJsonArray,

    stats_device_id: String,
    stats_session_id: String,
    stats_last_event_ms: i64,

    current_sync_enabled: bool,
    current_sync_backend_type: String,
    current_sync_remote_url: String,
    current_sync_branch: String,
    current_sync_auto_sync: bool,
    current_sync_interval: u32,
    current_sync_username: String,
    current_sync_token: String,
    current_sync_operation_state: String,
    /// 当前同步操作的唯一 ID（由 Core 分配），用于跟踪操作生命周期
    current_sync_operation_id: String,
    /// 当前同步操作类型（如 "full_sync"、"lww_sync"、"git_sync"）
    current_sync_operation_kind: String,
    current_sync_status: String,
    current_sync_in_progress: bool,
    /// 手动同步 pending 标志。当手动同步请求到来时正在运行自动同步，
    /// 设为 true 排队等待当前同步完成后再执行一次 manual sync。
    /// 连续点击只保留一次 pending，不堆无限队列。
    manual_sync_pending: bool,
    current_last_sync_time: i64,

    current_system_color_scheme: String,
    current_pending_github_init_path: String,
    pub current_ai_enabled: bool,
    pub current_setting_desktop_sidebar_width: f64,
    pub current_setting_desktop_editor_width: f64,
    current_setting_font_size: f32,
    current_setting_line_spacing: f32,
    current_setting_auto_save_enabled: bool,
    current_setting_auto_save_delay_ms: u32,
    current_setting_auto_indent_enabled: bool,
    current_setting_auto_indent_width: f32,
    // Issue #705: current_setting_theme_mode 已删除。运行时只认
    // current_setting_appearance_mode,不再有第二套主题状态字段。
    current_setting_monet_color: String,
    current_setting_theme_palette_json: String,
    current_setting_color_source: String,
    current_setting_appearance_mode: String,
    current_setting_dynamic_color_enabled: bool,
    current_setting_selected_palette_id: String,
    current_setting_selected_builtin_theme_id: String,
    current_setting_typing_animation_enabled: bool,
    current_setting_smooth_cursor_enabled: bool,
    current_setting_typing_animation_duration_ms: u32,
    current_setting_smooth_cursor_duration_ms: u32,
    pub(crate) current_system_is_dark: bool,
    // alpha 阶段 diagnostics 默认 true（与 core settings 和 diagnostics 全局 AtomicBool 对齐）
    pub(crate) current_setting_diagnostics_enabled: bool,
    pub(crate) current_setting_diagnostics_verbose: bool,

    // ── Layout Contract（#610：Qt 侧先按本平台窗口系统算能力，再套 Core 契约） ──
    #[allow(dead_code)]
    resolve_layout: qt_method!(fn(&self, width_vp: f64, height_vp: f64) -> QJsonObject),

    // ── Screen Contract ──
    #[allow(dead_code)]
    resolve_screen_policy: qt_method!(fn(&self, screen_role: QString) -> QJsonObject),
}

impl AppBackend {
    /// 将 AppBackend 当前状态同步到 DomainSnapshot。
    ///
    /// 调用时机：QML 属性变更后、同步操作完成后等需要刷新 QML 绑定的场景。
    /// 线程安全：仅在 GUI 线程调用，DomainSnapshot 使用 Rc<RefCell> 非线程安全。
    pub(crate) fn update_snapshot(
        &self,
        snapshot: &std::rc::Rc<std::cell::RefCell<super::DomainSnapshot>>,
    ) {
        let mut s = snapshot.borrow_mut();
        s.save_status = self.current_save_status.clone();
        s.word_count = self.current_word_count;
        s.error_message = self.current_error_message.clone();
        s.selected_item_id = {
            if let Some(ref id) = self.selected_chapter_id {
                id.clone()
            } else if let Some(ref id) = self.selected_volume_id {
                id.clone()
            } else if let Some(ref id) = self.selected_project_id {
                id.clone()
            } else {
                String::new()
            }
        };
        s.has_selected_chapter_prop = self.selected_chapter_id.is_some();
        s.chapter_path = {
            if let (Some(api), Some(p), Some(v), Some(c)) = (
                self.core_api(),
                &self.selected_project_id,
                &self.selected_volume_id,
                &self.selected_chapter_id,
            ) {
                let mut path = String::new();
                if let Ok(projects) = api.list_projects() {
                    if let Some(proj) = projects.iter().find(|x| x.id == *p) {
                        path.push_str(&proj.title);
                    }
                }
                if let Ok(volumes) = api.list_volumes(p) {
                    if let Some(vol) = volumes.iter().find(|x| x.id == *v) {
                        path.push_str(" > ");
                        path.push_str(&vol.title);
                    }
                }
                if let Ok(chapters) = api.list_chapters(p, v) {
                    if let Some(chap) = chapters.iter().find(|x| x.id == *c) {
                        path.push_str(" > ");
                        path.push_str(&chap.title);
                    }
                }
                path
            } else {
                String::new()
            }
        };
        s.setting_font_size = self.current_setting_font_size;
        s.setting_line_spacing = self.current_setting_line_spacing;
        s.setting_auto_save_enabled = self.current_setting_auto_save_enabled;
        s.setting_auto_save_delay_ms = self.current_setting_auto_save_delay_ms;
        s.setting_auto_indent_enabled = self.current_setting_auto_indent_enabled;
        s.setting_auto_indent_width = self.current_setting_auto_indent_width;
        s.setting_smooth_cursor_enabled = self.current_setting_smooth_cursor_enabled;
        s.setting_typing_animation_enabled = self.current_setting_typing_animation_enabled;
        s.setting_smooth_cursor_duration_ms = self.current_setting_smooth_cursor_duration_ms;
        s.setting_typing_animation_duration_ms = self.current_setting_typing_animation_duration_ms;
        s.has_workspace = self.current_has_data_root;
        s.sync_enabled = self.current_sync_enabled;
        s.sync_auto_sync = self.current_sync_auto_sync;
        s.sync_interval = self.current_sync_interval;
        s.has_sync_token = !self.current_sync_token.is_empty();
        s.sync_in_progress = self.current_sync_in_progress;
        s.sync_can_run = self.current_has_data_root
            && self.current_sync_enabled
            && !self.current_sync_remote_url.is_empty()
            && !self.current_sync_token.is_empty();
        s.manual_sync_pending = self.manual_sync_pending;
        s.ai_available = cfg!(feature = "ai");
        s.ai_enabled = self.current_ai_enabled;
        s.setting_desktop_sidebar_width = self.current_setting_desktop_sidebar_width;
        s.setting_desktop_editor_width = self.current_setting_desktop_editor_width;
        s.setting_diagnostics_enabled = self.current_setting_diagnostics_enabled;
        s.setting_diagnostics_verbose = self.current_setting_diagnostics_verbose;
        s.setting_dynamic_color_enabled = self.current_setting_dynamic_color_enabled;
        s.system_is_dark = self.current_system_is_dark;
        s.appearance_mode = self.current_setting_appearance_mode.clone();
        s.color_source = self.current_setting_color_source.clone();
        s.selected_palette_id = self.current_setting_selected_palette_id.clone();
        s.selected_builtin_theme_id = self.current_setting_selected_builtin_theme_id.clone();
        s.has_selected_chapter = self.selected_chapter_id.is_some();
        s.selected_chapter_exists = {
            if let (Some(api), Some(p), Some(v), Some(c)) = (
                self.core_api(),
                &self.selected_project_id,
                &self.selected_volume_id,
                &self.selected_chapter_id,
            ) {
                if let Ok(chapters) = api.list_chapters(p, v) {
                    chapters.iter().any(|chap| chap.id == *c)
                } else {
                    false
                }
            } else {
                false
            }
        };
    }

    pub(crate) fn core_api(&self) -> Option<WriterCoreApi> {
        if self.current_has_data_root && !self.current_data_root.is_empty() {
            // 用打开 workspace 时保存的 layout 快照构造 API，不再重新 bootstrap。
            if let Some(ref layout) = self.current_workspace_git_layout {
                Some(with_layout_core_api(
                    &self.current_data_root,
                    &self.current_projects_root,
                    layout,
                ))
            } else {
                // layout 未保存（理论上不应发生，因为 internal_open_data_root 会设置）。
                // 回退到 bootstrap 以保证正确性。
                match create_core_api(&self.current_data_root, &self.current_projects_root) {
                    Ok(api) => Some(api),
                    Err(e) => {
                        log::error!(
                            "core_api: bootstrap_core_api fallback failed for {}: {}",
                            self.current_data_root,
                            e
                        );
                        None
                    }
                }
            }
        } else {
            None
        }
    }

    fn debug_qml_enabled(&self) -> bool {
        get_debug_config().qml_enabled
    }

    fn debug_module_enabled_qml(&self, module: QString) -> bool {
        debug_module_enabled(&module.to_string())
    }

    fn log_qml(&self, level: QString, module: QString, event: QString, message: QString) {
        let lvl = level.to_string();
        let m = module.to_string();
        let ev = event.to_string();
        let msg = message.to_string();
        let lvl_enum = match lvl.to_lowercase().as_str() {
            "error" => DebugLevel::Error,
            "warn" => DebugLevel::Warn,
            "info" => DebugLevel::Info,
            "debug" => DebugLevel::Debug,
            "trace" => DebugLevel::Trace,
            _ => DebugLevel::Info,
        };
        if debug_level_enabled(&m, lvl_enum) {
            let ws_exists = self.current_has_data_root;
            let proj = self.selected_project_id.as_deref().unwrap_or("none");
            let vol = self.selected_volume_id.as_deref().unwrap_or("none");
            let chap = self.selected_chapter_id.as_deref().unwrap_or("none");

            let prefix = format!("[SujianDebug][qml][module={}][event={}]", m, ev);
            let state = format!(
                "[data_root_exists={}][proj={}][vol={}][chap={}]",
                ws_exists, proj, vol, chap
            );
            if lvl_enum == DebugLevel::Warn {
                eprintln!("{}[WARN]{} {}", prefix, state, msg);
            } else if lvl_enum == DebugLevel::Error {
                eprintln!("{}[ERROR]{} {}", prefix, state, msg);
            } else {
                println!("{}{} {}", prefix, state, msg);
            }
        }
        // 文件日志：由 writer_diagnostics 接管（log::* 已被 writer_diagnostics logger 接管）。
        match lvl_enum {
            DebugLevel::Error => log::error!(target: &m, "{}: {}", ev, msg),
            DebugLevel::Warn => log::warn!(target: &m, "{}: {}", ev, msg),
            DebugLevel::Info => log::info!(target: &m, "{}: {}", ev, msg),
            DebugLevel::Debug => log::debug!(target: &m, "{}: {}", ev, msg),
            DebugLevel::Trace => log::trace!(target: &m, "{}: {}", ev, msg),
        }
    }

    fn debug_log(&self, module: &str, event: &str, message: &str) {
        if debug_level_enabled(module, DebugLevel::Info) {
            let ws_exists = self.current_has_data_root;
            let proj = self.selected_project_id.as_deref().unwrap_or("none");
            let vol = self.selected_volume_id.as_deref().unwrap_or("none");
            let chap = self.selected_chapter_id.as_deref().unwrap_or("none");
            println!(
                "[SujianDebug][module={}][event={}][data_root_exists={}][proj={}][vol={}][chap={}] {}",
                module, event, ws_exists, proj, vol, chap, message
            );
        }
        // 文件日志由 writer_diagnostics 接管。
        log::info!(target: module, "{}: {}", event, message);
    }

    fn debug_warn(&self, module: &str, event: &str, message: &str) {
        if debug_level_enabled(module, DebugLevel::Warn) {
            let ws_exists = self.current_has_data_root;
            let proj = self.selected_project_id.as_deref().unwrap_or("none");
            let vol = self.selected_volume_id.as_deref().unwrap_or("none");
            let chap = self.selected_chapter_id.as_deref().unwrap_or("none");
            eprintln!(
                "[SujianDebug][WARN][module={}][event={}][data_root_exists={}][proj={}][vol={}][chap={}] {}",
                module, event, ws_exists, proj, vol, chap, message
            );
        }
        // WARN 级别文件日志由 writer_diagnostics 接管。
        log::warn!(target: module, "{}: {}", event, message);
    }

    fn debug_error(&self, module: &str, event: &str, message: &str) {
        if debug_level_enabled(module, DebugLevel::Error) {
            let ws_exists = self.current_has_data_root;
            let proj = self.selected_project_id.as_deref().unwrap_or("none");
            let vol = self.selected_volume_id.as_deref().unwrap_or("none");
            let chap = self.selected_chapter_id.as_deref().unwrap_or("none");
            eprintln!(
                "[SujianDebug][ERROR][module={}][event={}][data_root_exists={}][proj={}][vol={}][chap={}] {}",
                module, event, ws_exists, proj, vol, chap, message
            );
        }
        // ERROR 级别文件日志由 writer_diagnostics 接管。
        log::error!(target: module, "{}: {}", event, message);
    }

    fn now_epoch_seconds() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn system_color_scheme(&self) -> QString {
        self.current_system_color_scheme.clone().into()
    }

    fn ai_available(&self) -> bool {
        cfg!(feature = "ai")
    }

    fn ai_enabled(&self) -> bool {
        self.current_ai_enabled
    }

    fn set_ai_enabled(&mut self, val: bool) {
        self.current_ai_enabled = val;
        self.ai_enabled_changed();
    }

    // ── Layout Contract ──
    //
    // ⚠️ 边界约束（#610）⚠️
    //
    // resolve_layout() 产出的布局 DTO 只决定壳层布局：
    //   - shellMode（单栏/双栏/三栏）
    //   - contentMaxWidthVp（编辑纸面最大宽度）
    //   - contentPaddingVp（页面内边距）
    //   - showPrimaryNavigation / visible pane roles
    //
    // 布局 DTO 绝对不干预编辑器底层渲染：
    //   - 不传递到 SujianEditorItem 的 QSG 渲染线程
    //   - 不影响光标位置、IME 输入、动画帧率
    //   - 不改变 QTextLayout 的排版计算
    //   - 不驱动 EditorAnimationOverlay 的动画属性
    //
    // 编辑器渲染由 EditorController + SujianEditorItem 独立管理，
    // 遵守 Qt QSG 线程边界，不受布局 DTO 影响。
    //
    // Qt 是桌面端：鼠标为主、无软键盘、无折叠屏，因此只把窗口宽高换算成
    // 可用栏数（Qt 平台断点），再调 Core 解析产品壳层契约。

    fn resolve_layout(&self, width_vp: f64, height_vp: f64) -> QJsonObject {
        use writer_core::presentation::layout::resolver::WindowViewport;

        let viewport = WindowViewport {
            width_dp: width_vp as f32,
            height_dp: height_vp as f32,
            // Qt 桌面端无折叠屏/系统遮挡（与上方"无折叠屏"注释一致，#628 验收点 5）。
            occlusions: Vec::new(),
        };

        let contract = writer_core::presentation::layout::resolve_layout(&viewport);
        // #628：show_primary_navigation 改由 ScreenPolicy 提供。
        // 桌面端默认显示一级导航；具体页面（Writing/Settings）的隐藏由
        // resolve_screen_policy 返回的 ScreenPolicy.show_primary_navigation 决定。
        let dto = LinuxQtLayoutPlanDto::from_contract(&contract, width_vp as f32, true);
        let json = serde_json::to_string(&dto).unwrap_or_else(|_| "{}".to_string());
        qjson_object_from_json(&json)
    }

    /// 根据页面角色解析动作槽位列表（#610 / #628：动作区域/顺序是产品语义，不随壳层变化；
    /// ScreenPolicy 含 show_primary_navigation 由 Rust 决定）
    ///
    /// QML 调用：backend.resolve_screen_policy("Writing")
    /// 返回：{ screenRole: "Writing", actionSlots: [...], showPrimaryNavigation: bool }
    fn resolve_screen_policy(&self, screen_role: QString) -> QJsonObject {
        use writer_core::presentation::screen::{resolve_screen_policy, ScreenRole};

        let role = match screen_role.to_string().as_str() {
            "Home" => ScreenRole::Home,
            "ProjectList" => ScreenRole::ProjectList,
            "ProjectWorkspace" => ScreenRole::ProjectWorkspace,
            "Writing" => ScreenRole::Writing,
            "StarMap" => ScreenRole::StarMap,
            "Stats" => ScreenRole::Stats,
            "Settings" => ScreenRole::Settings,
            "Sync" => ScreenRole::Sync,
            _ => ScreenRole::Home,
        };

        let policy = resolve_screen_policy(role);

        use writer_core::api::types::screen_policy::*;
        let dto = ScreenPolicyDto {
            screen_role: policy.screen_role.into(),
            action_slots: policy.action_slots.into_iter().map(Into::into).collect(),
            show_primary_navigation: policy.show_primary_navigation,
        };

        let json = serde_json::to_string(&dto).unwrap_or_else(|_| "{}".to_string());
        qjson_object_from_json(&json)
    }

    fn query_system_color_scheme(&mut self) {
        let scheme = system_utils::detect_system_theme_from_platform();
        self.current_system_color_scheme = scheme;
        self.system_color_scheme_changed();
    }

    fn apply_window_dark_mode(&mut self, _is_dark: bool) {
        // Linux Qt/QML route uses the native Linux window manager theme.
    }

    /// Issue #692 评论 5692612221: 查询当前窗口所属屏幕的可用几何
    /// （QScreen::availableGeometry，已扣任务栏/面板等窗口管理器保留区域）。
    /// 返回紧凑 JSON: {"valid":bool,"x":int,"y":int,"width":int,"height":int}，
    /// 单位为 Qt 6 设备无关逻辑像素，与 QML 坐标空间一致。
    /// 供 main.qml applyInitialWindowSize() 做一次性初始尺寸收口，
    /// 替代旧的 window.screen.width/height - 32 猜边距方案。
    /// 仅在 GUI 线程调用；纯平台查询，转发到 platform/linux_qt 平台封装层
    /// （cpp!(unsafe) FFI 边界只允许出现在平台封装目录，见 Rust 安全守卫）。
    fn available_screen_geometry_json(&self) -> QString {
        crate::platform::linux_qt::screen_geometry::available_screen_geometry_json()
    }

    fn copy_text_to_clipboard(&mut self, text: QString) -> QString {
        let result = system_utils::copy_text_to_clipboard_impl(&text.to_string());
        result.to_json().to_string().into()
    }

    fn workspace_path(&self) -> QString {
        self.current_data_root.clone().into()
    }

    // --- StarMap methods ---

    // Deprecated compatibility forwarding surface is split by domain.
}

#[path = "editor_backend.rs"]
pub mod editor_backend;
#[path = "project_backend.rs"]
pub mod project_backend;
#[path = "settings_backend.rs"]
pub mod settings_backend;
#[path = "starmap_backend.rs"]
pub mod starmap_backend;
#[path = "sync_backend.rs"]
pub mod sync_backend;
#[path = "workspace_backend.rs"]
pub mod workspace_backend;

pub use editor_backend::EditorBackend;
pub use project_backend::ProjectBackend;
pub use settings_backend::SettingsBackend;
pub use starmap_backend::StarMapBackend;
pub use sync_backend::SyncBackend;
pub use workspace_backend::WorkspaceBackend;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_create_project_success() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let ws_path = dir.path().to_str().ok_or("Invalid path")?.to_string();

        let mut backend = AppBackend::default();
        backend.current_data_root = ws_path.clone();
        backend.current_projects_root = ws_path.clone();
        backend.current_has_data_root = true;

        // Create 3 projects
        for i in 1..=3 {
            let res_json =
                backend.create_project_json(format!("Test Project {}", i).into(), "".into());
            let res: serde_json::Value = serde_json::from_str(&res_json.to_string())?;
            assert_eq!(res["success"], true);
        }

        // Check if tree size increased
        let tree_len_after = backend.cached_tree.len();
        assert!(tree_len_after >= 3);

        Ok(())
    }

    #[test]
    fn test_create_project_failure() -> Result<(), Box<dyn std::error::Error>> {
        use qmetaobject::QJsonValue;
        let mut backend = AppBackend::default();
        backend.current_data_root = "/invalid/path/that/does/not/exist".to_string();
        backend.current_projects_root = "/invalid/path/that/does/not/exist".to_string();
        backend.current_has_data_root = true;

        // Let's pretend the tree has some items
        let test_tree = serde_json::json!([
            { "id": "1", "title": "Old Project" }
        ]);

        let mut items = vec![];
        items.push(QJsonValue::from(QString::from(test_tree.to_string())));
        backend.cached_tree = QJsonArray::from(items);

        let res_json = backend.create_project_json("Test Project".into(), "".into());
        let res: serde_json::Value = serde_json::from_str(&res_json.to_string())?;

        assert_eq!(res["success"], false);
        // Ensure tree didn't wipe or change unexpectedly
        assert_eq!(backend.cached_tree.len(), 1);

        Ok(())
    }

    #[test]
    fn test_create_project_empty_title() -> Result<(), Box<dyn std::error::Error>> {
        let mut backend = AppBackend::default();
        backend.current_data_root = "/tmp".to_string();
        backend.current_projects_root = "/tmp".to_string();
        backend.current_has_data_root = true;

        let res_json = backend.create_project_json("   ".into(), "".into());
        let res: serde_json::Value = serde_json::from_str(&res_json.to_string())?;

        assert_eq!(res["success"], false);
        assert_eq!(res["errorCode"], "CORE_ERROR");
        assert_eq!(res["messageKey"], "error.empty_title");

        Ok(())
    }

    #[test]
    fn test_handle_sync_outcome_success_pending_path() {
        use tempfile::tempdir;
        let dir = tempdir().expect("tempdir creation failed");
        let path_str = dir.path().to_string_lossy().to_string();

        let mut backend = AppBackend::default();
        backend.current_pending_github_init_path = path_str.clone();
        backend.current_has_data_root = false;

        let outcome = SyncTaskOutcome {
            operation_id: "".to_string(),
            sync_status: "success".to_string(),
            action_result: "OK".to_string(),
            workspace_generation: 0,
            data_root: "".to_string(),
        };
        backend.handle_sync_outcome(outcome, None);

        // After sync success with pending path, internal_open_data_root is called.
        // The data root is opened successfully; pending path is cleared.
        // 工作区成功打开后，load_sync_config 走全局配置路径，没有同步配置时
        // refresh_sync_status_from_config 得到 "not_configured"。
        assert_eq!(backend.current_sync_status, "not_configured");
        assert_eq!(backend.current_pending_github_init_path, "");
        assert!(backend.current_has_data_root);
    }

    #[test]
    fn test_handle_sync_outcome_conflict_reloads_tree() {
        let mut backend = AppBackend::default();
        backend.current_has_data_root = true;

        let outcome = SyncTaskOutcome {
            operation_id: "".to_string(),
            sync_status: "conflict".to_string(),
            action_result: "Conflict".to_string(),
            workspace_generation: 0,
            data_root: "".to_string(),
        };
        backend.handle_sync_outcome(outcome, None);

        assert_eq!(backend.current_sync_status, "conflict");
    }

    #[test]
    fn test_handle_sync_outcome_error_does_not_clear_tree() {
        let mut backend = AppBackend::default();
        backend.current_has_data_root = true;

        let outcome = SyncTaskOutcome {
            operation_id: "".to_string(),
            sync_status: "error".to_string(),
            action_result: "Failed".to_string(),
            workspace_generation: 0,
            data_root: "".to_string(),
        };
        backend.handle_sync_outcome(outcome, None);

        assert_eq!(backend.current_sync_status, "error");
        assert_eq!(backend.current_has_data_root, true);
    }

    #[test]
    fn test_sync_dry_run_missing_config_returns_error() {
        let mut backend = AppBackend::default();
        backend.current_sync_remote_url = "".to_string();
        backend.current_sync_token = "".to_string();
        backend.current_data_root = "some_path".to_string();
        backend.current_projects_root = "some_path".to_string();

        backend.perform_sync_dry_run(None);

        assert_eq!(backend.current_sync_status, "error");
        assert!(backend
            .current_sync_operation_state
            .contains("sync.block.remote_url_missing"));
    }

    #[test]
    fn test_sync_dry_run_no_project_selected_still_validates_config() {
        let mut backend = AppBackend::default();
        backend.current_data_root = "some_path".to_string();
        backend.current_projects_root = "some_path".to_string();
        // 没有选作品时仍进入全局同步配置校验
        backend.perform_sync_dry_run(None);

        assert_eq!(backend.current_sync_status, "error");
        assert!(backend
            .current_sync_operation_state
            .contains("sync.block.remote_url_missing"));
    }

    #[test]
    fn test_load_sync_config_reads_global_config_without_selected_project() {
        use tempfile::tempdir;
        let dir = tempdir().expect("tempdir creation failed");
        let path_str = dir.path().to_string_lossy().to_string();

        let mut backend = AppBackend::default();
        backend.current_data_root = path_str.clone();
        backend.current_projects_root = path_str.clone();
        backend.current_has_data_root = true;
        // 不设置 selected_project_id，模拟首次安装/新设备恢复
        backend.selected_project_id = None;

        backend.load_sync_config();

        // 全局配置不存在时应得到 not_configured，而非 no_workspace
        assert_eq!(backend.current_sync_status, "not_configured");
        // 分支默认值应为 main
        assert_eq!(backend.current_sync_branch, "main");
    }

    #[test]
    fn test_save_sync_config_writes_global_config_without_selected_project() {
        use tempfile::tempdir;
        let dir = tempdir().expect("tempdir creation failed");
        let path_str = dir.path().to_string_lossy().to_string();

        let mut backend = AppBackend::default();
        backend.current_data_root = path_str.clone();
        backend.current_projects_root = path_str.clone();
        backend.current_has_data_root = true;
        // 不设置 selected_project_id，模拟首次安装/新设备恢复
        backend.selected_project_id = None;
        backend.current_sync_enabled = true;
        backend.current_sync_remote_url = "https://github.com/test/repo.git".to_string();
        backend.current_sync_branch = "main".to_string();
        backend.current_sync_token = "test_token".to_string();
        backend.current_sync_backend_type = "github_api".to_string();

        let result = backend.save_sync_config();
        // 没有选中作品也应能保存全局配置
        assert!(result);
    }

    // ── Issue #729：workspace generation 身份隔离测试 ──

    /// 过期 generation 的同步回调必须被丢弃，不更新同步状态。
    #[test]
    fn test_handle_sync_outcome_discards_stale_workspace_generation() {
        let mut backend = AppBackend::default();
        // 模拟同步进行中：generation=0，operation_id 已设
        backend.current_sync_operation_id = "op-stale".to_string();
        backend.current_sync_status = "syncing".to_string();
        backend.current_sync_in_progress = true;
        // current_workspace_generation 保持 default 0

        // 旧同步线程捕获的是 generation=0，但工作区已切换使 generation 变为 1。
        // 此处直接构造一个 generation=0 的 outcome 模拟「回调到达时工作区已前进」。
        // 为触发丢弃，先把 backend generation 推到 1（模拟切工作区后）。
        backend.current_workspace_generation = 1;

        let outcome = SyncTaskOutcome {
            operation_id: "op-stale".to_string(),
            sync_status: "success".to_string(),
            action_result: "OK".to_string(),
            workspace_generation: 0,
            data_root: "".to_string(),
        };
        backend.handle_sync_outcome(outcome, None);

        // 结果被丢弃：状态未被 "success" 覆盖，in_progress 未被清
        assert_eq!(
            backend.current_sync_status, "syncing",
            "过期 generation 的回调不应更新同步状态"
        );
        assert!(
            backend.current_sync_in_progress,
            "过期 generation 的回调不应清 in_progress"
        );
    }

    /// 匹配 generation 的同步回调正常接受并更新状态。
    #[test]
    fn test_handle_sync_outcome_accepts_matching_workspace_generation() {
        let mut backend = AppBackend::default();
        backend.current_sync_operation_id = "op-fresh".to_string();
        backend.current_sync_in_progress = true;
        // generation 保持 default 0

        let outcome = SyncTaskOutcome {
            operation_id: "op-fresh".to_string(),
            sync_status: "error".to_string(),
            action_result: "Failed".to_string(),
            workspace_generation: 0,
            data_root: "".to_string(),
        };
        backend.handle_sync_outcome(outcome, None);

        // 结果被接受：状态更新为 "error"，in_progress 清除
        assert_eq!(backend.current_sync_status, "error");
        assert!(!backend.current_sync_in_progress);
    }

    /// close_workspace（经 reset_workspace_state）必须取消当前同步令牌、
    /// 递增 workspace generation、清 in_progress。
    #[test]
    fn test_close_workspace_cancels_sync_token_and_increments_generation() {
        let mut backend = AppBackend::default();
        // 模拟一个正在运行的同步：创建令牌并存入 backend
        let token = Arc::new(SyncCancellationToken::new());
        let token_handle = token.clone();
        backend.current_sync_cancel_token = Some(token);
        backend.current_sync_in_progress = true;
        assert!(!token_handle.is_cancelled(), "新令牌初始未取消");

        backend.close_workspace();

        // a. 令牌被取消
        assert!(
            token_handle.is_cancelled(),
            "close_workspace 必须取消当前同步令牌"
        );
        // b. generation 递增
        assert_eq!(
            backend.current_workspace_generation, 1,
            "close_workspace 必须递增 workspace generation"
        );
        // c. in_progress 清除
        assert!(
            !backend.current_sync_in_progress,
            "close_workspace 必须清 current_sync_in_progress"
        );
        // d. 令牌被 take 掉
        assert!(
            backend.current_sync_cancel_token.is_none(),
            "close_workspace 后 current_sync_cancel_token 应为 None"
        );
    }

    /// 成功打开工作区必须递增 workspace generation。
    #[test]
    fn test_open_data_root_increments_workspace_generation() {
        let dir = tempdir().expect("tempdir creation failed");
        let path_str = dir.path().to_string_lossy().to_string();

        let mut backend = AppBackend::default();
        assert_eq!(backend.current_workspace_generation, 0, "初始 generation=0");

        backend.internal_open_data_root(&path_str);
        assert_eq!(
            backend.current_workspace_generation, 1,
            "打开工作区后 generation 应递增到 1"
        );

        // 关闭再打开应继续递增
        backend.close_workspace();
        assert_eq!(backend.current_workspace_generation, 2);

        backend.internal_open_data_root(&path_str);
        assert_eq!(backend.current_workspace_generation, 3);
    }

    /// 切工作区后，旧同步的 outcome（携带旧 generation）必须被丢弃，
    /// 不污染新工作区的同步状态。
    #[test]
    fn test_workspace_switch_discards_old_sync_outcome() {
        let dir = tempdir().expect("tempdir creation failed");
        let path_str = dir.path().to_string_lossy().to_string();

        let mut backend = AppBackend::default();
        backend.internal_open_data_root(&path_str);
        // 此时 generation=1。模拟启动了一个同步：
        backend.current_sync_operation_id = "op-old".to_string();
        backend.current_sync_in_progress = true;
        backend.current_sync_status = "syncing".to_string();
        let gen_at_sync_start = backend.current_workspace_generation; // 1

        // 用户切换工作区：close_workspace 取消旧同步并递增 generation
        backend.close_workspace();
        // reset 把 current_sync_status 设为 "no_workspace"
        assert_eq!(backend.current_sync_status, "no_workspace");
        assert_eq!(backend.current_workspace_generation, 2);

        // 旧同步线程完成，回调到达（携带启动时的旧 generation）
        let stale_outcome = SyncTaskOutcome {
            operation_id: "op-old".to_string(),
            sync_status: "success".to_string(),
            action_result: "OK".to_string(),
            workspace_generation: gen_at_sync_start, // 1，已过期
            data_root: path_str.clone(),
        };
        backend.handle_sync_outcome(stale_outcome, None);

        // 旧回调被丢弃：状态不被 "success" 覆盖，仍是 "no_workspace"
        assert_eq!(
            backend.current_sync_status, "no_workspace",
            "切工作区后旧同步回调不应污染新工作区状态"
        );
    }

    /// workspace generation 用 wrapping_add 递增，不会溢出 panic。
    #[test]
    fn test_workspace_generation_wraps_without_panic() {
        let mut backend = AppBackend::default();
        backend.current_workspace_generation = u64::MAX;

        // close_workspace 内部用 wrapping_add，不应 panic
        backend.close_workspace();
        assert_eq!(backend.current_workspace_generation, 0);
    }
}

#[cfg(test)]
mod workspace_flow_tests {
    use super::*;

    #[test]
    fn test_data_root_open_flow_updates_state() {
        use tempfile::tempdir;
        let mut app = AppBackend::default();
        assert!(!app.has_workspace());

        let dir = tempdir().expect("tempdir creation failed");
        let path_str = dir.path().to_string_lossy().to_string();

        // Test opening a data root directory
        app.internal_open_data_root(&path_str);

        assert!(
            app.has_workspace(),
            "AppBackend must have data root after opening"
        );
        assert_eq!(app.workspace_path().to_string(), path_str);

        // projects subdirectory must be created
        let projects_path = dir.path().join("projects");
        assert!(
            projects_path.exists(),
            "projects subdirectory must be created"
        );

        // Close workspace
        app.close_workspace();
        assert!(
            !app.has_workspace(),
            "AppBackend must not have data root after closing"
        );

        // Reopen existing data root
        app.internal_open_data_root(&path_str);
        assert!(
            app.has_workspace(),
            "AppBackend must have data root after reopening"
        );
    }
}
