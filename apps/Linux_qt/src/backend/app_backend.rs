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

use super::linux_qt_layout_plan_dto::LinuxQtLayoutPlanDto;
use super::json_utils::{
    bridge_error_object, bridge_success_object, qjson_array_data_from_json, qjson_object_from_json,
    serde_to_qjson_object,
};
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

pub(crate) fn debug_log_static(module: &str, event: &str, message: &str) {
    if debug_level_enabled(module, DebugLevel::Info) {
        println!(
            "[SujianDebug][static][module={}][event={}] {}",
            module, event, message
        );
    }
    // 文件日志始终写入（受 diagnostics_verbose 控制）
    crate::backend::diagnostics::log_to_file("INFO", module, event, message);
}

pub(crate) fn debug_warn_static(module: &str, event: &str, message: &str) {
    if debug_level_enabled(module, DebugLevel::Warn) {
        eprintln!(
            "[SujianDebug][WARN][static][module={}][event={}] {}",
            module, event, message
        );
    }
    // WARN 级别文件日志永远写入
    crate::backend::diagnostics::log_to_file("WARN", module, event, message);
}

pub(crate) fn debug_error_static(module: &str, event: &str, message: &str) {
    if debug_level_enabled(module, DebugLevel::Error) {
        eprintln!(
            "[SujianDebug][ERROR][static][module={}][event={}] {}",
            module, event, message
        );
    }
    // ERROR 级别文件日志永远写入
    crate::backend::diagnostics::log_to_file("ERROR", module, event, message);
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
    workspace_opened: qt_signal!(),
    #[allow(dead_code)]
    workspace_content_changed: qt_signal!(),
    #[allow(dead_code)]
    workspace_state_changed: qt_signal!(),
    #[allow(dead_code)]
    projects_reloaded: qt_signal!(),
    #[allow(non_snake_case)]
    #[allow(dead_code)]
    projectsReloaded: qt_signal!(),
    #[allow(dead_code)]
    save_status_changed: qt_signal!(),
    #[allow(dead_code)]
    word_count_changed: qt_signal!(),
    #[allow(dead_code)]
    error_occurred: qt_signal!(),
    #[allow(dead_code)]
    selected_item_changed: qt_signal!(),
    #[allow(dead_code)]
    chapter_path_changed: qt_signal!(),
    #[allow(dead_code)]
    clear_editor: qt_signal!(),

    #[allow(dead_code)]
    sync_config_changed: qt_signal!(),
    #[allow(dead_code)]
    sync_action_completed: qt_signal!(),
    #[allow(dead_code)]
    sync_status_changed: qt_signal!(),

    #[allow(dead_code)]
    settings_changed: qt_signal!(),

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
    pending_github_init_path_changed: qt_signal!(),
    #[allow(dead_code)]
    query_system_color_scheme: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    apply_window_dark_mode: qt_method!(fn(&mut self, is_dark: bool)),
    #[allow(dead_code)]
    copy_text_to_clipboard: qt_method!(fn(&mut self, text: QString) -> QString),
    #[allow(dead_code)]
    debug_qml_enabled: qt_property!(bool; READ debug_qml_enabled),
    #[allow(dead_code)]
    debug_module_enabled_qml: qt_method!(fn(&self, module: QString) -> bool),
    log_qml:
        qt_method!(fn(&self, level: QString, module: QString, event: QString, message: QString)),

    current_workspace: String,
    current_has_workspace: bool,
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
    current_last_sync_time: i64,
    current_last_auto_sync_reason: String,
    current_last_auto_sync_started_at: i64,

    current_system_color_scheme: String,
    current_pending_github_init_path: String,
    pub current_ai_enabled: bool,
    pub current_setting_linux_qt_sidebar_width: f64,
    pub current_setting_linux_qt_editor_width: f64,
    current_setting_font_size: f32,
    current_setting_line_spacing: f32,
    current_setting_auto_save_enabled: bool,
    current_setting_auto_save_delay_ms: u32,
    current_setting_auto_indent_enabled: bool,
    current_setting_auto_indent_width: f32,
    current_setting_theme_mode: String,
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
    current_setting_coordinated_text_cursor_animation_enabled: bool,
    pub(crate) current_system_is_dark: bool,
    // alpha 阶段 diagnostics 默认 true（与 core settings 和 diagnostics 全局 AtomicBool 对齐）
    pub(crate) current_setting_diagnostics_enabled: bool,
    pub(crate) current_setting_diagnostics_verbose: bool,

    // ── Layout Policy ──
    #[allow(dead_code)]
    resolve_layout: qt_method!(fn(&self, width_dp: f64, height_dp: f64, safe_top_dp: f64, safe_bottom_dp: f64, keyboard_visible: bool, fold_state: QString, fold_orientation: QString, fold_is_separating: bool, fold_occlusion: QString, fold_bounds_left: f64, fold_bounds_top: f64, fold_bounds_right: f64, fold_bounds_bottom: f64, orientation: QString, pointer: QString) -> QJsonObject),

    // ── Screen Policy ──
    #[allow(dead_code)]
    resolve_screen_policy: qt_method!(fn(&self, screen_role: QString, shell_mode: QString) -> QJsonObject),
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
        s.setting_coordinated_text_cursor_animation_enabled = self.current_setting_coordinated_text_cursor_animation_enabled;
        s.has_workspace = self.current_has_workspace;
        s.sync_enabled = self.current_sync_enabled;
        s.sync_auto_sync = self.current_sync_auto_sync;
        s.sync_interval = self.current_sync_interval;
        s.has_sync_token = !self.current_sync_token.is_empty();
        s.sync_in_progress = self.current_sync_in_progress;
        s.sync_can_run = self.current_has_workspace && self.current_sync_enabled && !self.current_sync_in_progress;
        s.ai_available = cfg!(feature = "ai");
        s.ai_enabled = self.current_ai_enabled;
        s.setting_linux_qt_sidebar_width = self.current_setting_linux_qt_sidebar_width;
        s.setting_linux_qt_editor_width = self.current_setting_linux_qt_editor_width;
        s.setting_diagnostics_enabled = self.current_setting_diagnostics_enabled;
        s.setting_diagnostics_verbose = self.current_setting_diagnostics_verbose;
        s.setting_dynamic_color_enabled = self.current_setting_dynamic_color_enabled;
        s.system_is_dark = self.current_system_is_dark;
        s.appearance_mode = self.current_setting_appearance_mode.clone();
        s.selected_palette_id = self.current_setting_selected_palette_id.clone();
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
        if self.current_has_workspace && !self.current_workspace.is_empty() {
            Some(WriterCoreApi::new(&self.current_workspace))
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
            let ws_exists = self.current_has_workspace;
            let proj = self.selected_project_id.as_deref().unwrap_or("none");
            let vol = self.selected_volume_id.as_deref().unwrap_or("none");
            let chap = self.selected_chapter_id.as_deref().unwrap_or("none");

            let prefix = format!("[SujianDebug][qml][module={}][event={}]", m, ev);
            let state = format!(
                "[workspace_exists={}][proj={}][vol={}][chap={}]",
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
        // 文件日志：ERROR/WARN 永远写入，其他受 verbose 控制
        let level_str = match lvl_enum {
            DebugLevel::Error => "ERROR",
            DebugLevel::Warn => "WARN",
            DebugLevel::Info => "INFO",
            DebugLevel::Debug => "DEBUG",
            DebugLevel::Trace => "TRACE",
        };
        crate::backend::diagnostics::log_to_file(level_str, &m, &ev, &msg);
    }

    fn debug_log(&self, module: &str, event: &str, message: &str) {
        if debug_level_enabled(module, DebugLevel::Info) {
            let ws_exists = self.current_has_workspace;
            let proj = self.selected_project_id.as_deref().unwrap_or("none");
            let vol = self.selected_volume_id.as_deref().unwrap_or("none");
            let chap = self.selected_chapter_id.as_deref().unwrap_or("none");
            println!(
                "[SujianDebug][module={}][event={}][workspace_exists={}][proj={}][vol={}][chap={}] {}",
                module, event, ws_exists, proj, vol, chap, message
            );
        }
        // 文件日志：INFO 级别受 verbose 控制
        crate::backend::diagnostics::log_to_file("INFO", module, event, message);
    }

    fn debug_warn(&self, module: &str, event: &str, message: &str) {
        if debug_level_enabled(module, DebugLevel::Warn) {
            let ws_exists = self.current_has_workspace;
            let proj = self.selected_project_id.as_deref().unwrap_or("none");
            let vol = self.selected_volume_id.as_deref().unwrap_or("none");
            let chap = self.selected_chapter_id.as_deref().unwrap_or("none");
            eprintln!(
                "[SujianDebug][WARN][module={}][event={}][workspace_exists={}][proj={}][vol={}][chap={}] {}",
                module, event, ws_exists, proj, vol, chap, message
            );
        }
        // WARN 级别文件日志永远写入
        crate::backend::diagnostics::log_to_file("WARN", module, event, message);
    }

    fn debug_error(&self, module: &str, event: &str, message: &str) {
        if debug_level_enabled(module, DebugLevel::Error) {
            let ws_exists = self.current_has_workspace;
            let proj = self.selected_project_id.as_deref().unwrap_or("none");
            let vol = self.selected_volume_id.as_deref().unwrap_or("none");
            let chap = self.selected_chapter_id.as_deref().unwrap_or("none");
            eprintln!(
                "[SujianDebug][ERROR][module={}][event={}][workspace_exists={}][proj={}][vol={}][chap={}] {}",
                module, event, ws_exists, proj, vol, chap, message
            );
        }
        // ERROR 级别文件日志永远写入
        crate::backend::diagnostics::log_to_file("ERROR", module, event, message);
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

    // ── Layout Policy ──
    //
    // ⚠️ LayoutPlan 边界约束 ⚠️
    //
    // resolve_layout() 产出的 LayoutPlan 只决定壳层布局：
    //   - shellMode（导航模式：compact/medium/expanded）
    //   - contentMaxWidthVp（内容区域最大宽度）
    //   - contentPaddingVp（页面内边距）
    //   - sidebarVisible / sidebarWidth
    //
    // LayoutPlan 绝对不干预编辑器底层渲染：
    //   - 不传递到 SujianEditorItem 的 QSG 渲染线程
    //   - 不影响光标位置、IME 输入、动画帧率
    //   - 不改变 QTextLayout 的排版计算
    //   - 不驱动 EditorAnimationOverlay 的动画属性
    //
    // 编辑器渲染由 EditorController + SujianEditorItem 独立管理，
    // 遵守 Qt QSG 线程边界，不受 LayoutPlan 影响。

    fn resolve_layout(
        &self,
        width_dp: f64,
        height_dp: f64,
        safe_top_dp: f64,
        safe_bottom_dp: f64,
        keyboard_visible: bool,
        fold_state: QString,
        fold_orientation: QString,
        fold_is_separating: bool,
        fold_occlusion: QString,
        fold_bounds_left: f64,
        fold_bounds_top: f64,
        fold_bounds_right: f64,
        fold_bounds_bottom: f64,
        orientation: QString,
        pointer: QString,
    ) -> QJsonObject {
        use writer_core::layout_policy::{
            FoldState, FoldOrientation, FoldOcclusion, FoldFeatureInfo,
            Orientation, PointerKind, WindowMetrics, resolve_layout,
        };

        let fs = match fold_state.to_string().as_str() {
            "Flat" => FoldState::Flat,
            "HalfOpened" => FoldState::HalfOpened,
            _ => FoldState::None,
        };
        let fo = match fold_orientation.to_string().as_str() {
            "Horizontal" => FoldOrientation::Horizontal,
            _ => FoldOrientation::Vertical,
        };
        let foc = match fold_occlusion.to_string().as_str() {
            "Full" => FoldOcclusion::Full,
            _ => FoldOcclusion::None,
        };
        let fold_feature = FoldFeatureInfo {
            state: fs,
            orientation: fo,
            is_separating: fold_is_separating,
            occlusion: foc,
            bounds_left_vp: fold_bounds_left as f32,
            bounds_top_vp: fold_bounds_top as f32,
            bounds_right_vp: fold_bounds_right as f32,
            bounds_bottom_vp: fold_bounds_bottom as f32,
        };
        let orient = match orientation.to_string().as_str() {
            "Portrait" => Orientation::Portrait,
            "Landscape" => Orientation::Landscape,
            _ => Orientation::Unknown,
        };
        let ptr = match pointer.to_string().as_str() {
            "Touch" => PointerKind::Touch,
            "Stylus" => PointerKind::Stylus,
            "Mouse" => PointerKind::Mouse,
            _ => PointerKind::Unknown,
        };

        let metrics = WindowMetrics {
            width_dp: width_dp as f32,
            height_dp: height_dp as f32,
            safe_top_dp: safe_top_dp as f32,
            safe_bottom_dp: safe_bottom_dp as f32,
            keyboard_visible,
            fold_feature,
            orientation: orient,
            pointer: ptr,
        };

        let plan = resolve_layout(&metrics);
        let dto = LinuxQtLayoutPlanDto::from_layout_plan(&plan);
        let json = serde_json::to_string(&dto).unwrap_or_else(|_| "{}".to_string());
        qjson_object_from_json(&json)
    }

    /// 根据页面角色和壳层模式解析动作槽位列表
    ///
    /// QML 调用：backend.resolve_screen_policy("Writing", "SinglePane")
    /// 返回：{ screenRole: "Writing", actionSlots: [...] }
    fn resolve_screen_policy(
        &self,
        screen_role: QString,
        shell_mode: QString,
    ) -> QJsonObject {
        use writer_core::screen_policy::{ScreenRole, resolve_screen_policy};
        use writer_core::layout_policy::ShellMode;

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
        let mode = match shell_mode.to_string().as_str() {
            "SinglePane" => ShellMode::SinglePane,
            "SupportingPane" => ShellMode::SupportingPane,
            "TwoPane" => ShellMode::TwoPane,
            "ThreePane" => ShellMode::ThreePane,
            _ => ShellMode::SinglePane,
        };

        let action_slots = resolve_screen_policy(role, mode);

        use writer_core::api::types::screen_policy::*;
        let dto = ScreenPolicyDto {
            screen_role: role.into(),
            action_slots: action_slots.into_iter().map(Into::into).collect(),
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

    fn copy_text_to_clipboard(&mut self, text: QString) -> QString {
        let result = system_utils::copy_text_to_clipboard_impl(&text.to_string());
        result.to_json().to_string().into()
    }

    fn workspace_path(&self) -> QString {
        self.current_workspace.clone().into()
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
        backend.current_workspace = ws_path.clone();
        backend.current_has_workspace = true;

        WriterCoreApi::new(&ws_path)
            .create_workspace_if_needed()?;

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
        backend.current_workspace = "/invalid/path/that/does/not/exist".to_string();
        backend.current_has_workspace = true;

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
        backend.current_workspace = "/tmp".to_string();
        backend.current_has_workspace = true;

        let res_json = backend.create_project_json("   ".into(), "".into());
        let res: serde_json::Value = serde_json::from_str(&res_json.to_string())?;

        assert_eq!(res["success"], false);
        assert_eq!(res["errorCode"], "CORE_ERROR");
        assert_eq!(res["messageKey"], "error.empty_title");

        Ok(())
    }

    #[test]
    fn test_handle_sync_outcome_success_pending_path() {
        let mut backend = AppBackend::default();
        backend.current_pending_github_init_path = "/tmp/test_workspace".to_string();
        backend.current_has_workspace = false;

        let outcome = SyncTaskOutcome {
            operation_id: "".to_string(),
            sync_status: "success".to_string(),
            action_result: "OK".to_string(),
        };
        backend.handle_sync_outcome(outcome);

        // After sync success with pending path, internal_open_workspace is called.
        // If the path is invalid, load_sync_config sets status to "no_workspace"
        // (not "success") because core is not initialized.
        assert_eq!(backend.current_sync_status, "no_workspace");
        assert_eq!(backend.current_pending_github_init_path, "");
    }

    #[test]
    fn test_handle_sync_outcome_conflict_reloads_tree() {
        let mut backend = AppBackend::default();
        backend.current_has_workspace = true;

        let outcome = SyncTaskOutcome {
            operation_id: "".to_string(),
            sync_status: "conflict".to_string(),
            action_result: "Conflict".to_string(),
        };
        backend.handle_sync_outcome(outcome);

        assert_eq!(backend.current_sync_status, "conflict");
    }

    #[test]
    fn test_handle_sync_outcome_error_does_not_clear_tree() {
        let mut backend = AppBackend::default();
        backend.current_has_workspace = true;

        let outcome = SyncTaskOutcome {
            operation_id: "".to_string(),
            sync_status: "error".to_string(),
            action_result: "Failed".to_string(),
        };
        backend.handle_sync_outcome(outcome);

        assert_eq!(backend.current_sync_status, "error");
        assert_eq!(backend.current_has_workspace, true);
    }

    #[test]
    fn test_sync_dry_run_missing_config_returns_error() {
        let mut backend = AppBackend::default();
        backend.current_sync_remote_url = "".to_string();
        backend.current_sync_token = "".to_string();
        backend.current_workspace = "some_path".to_string();

        backend.perform_sync_dry_run();

        assert_eq!(backend.current_sync_status, "error");
        assert!(backend
            .current_sync_operation_state
            .contains("sync.block.remote_url_missing"));
    }
}

#[cfg(test)]
mod workspace_flow_tests {
    use super::*;

    #[test]
    fn test_workspace_creation_flow_updates_state() {
        use tempfile::tempdir;
        let mut app = AppBackend::default();
        assert!(!app.has_workspace());

        let dir = tempdir().expect("tempdir creation failed");
        let path_str = dir.path().to_string_lossy().to_string();

        // Test creating a new workspace
        app.internal_open_workspace(&path_str, true);

        assert!(
            app.has_workspace(),
            "AppBackend must have workspace after creation"
        );
        assert_eq!(app.workspace_path().to_string(), path_str);

        let manifest_path = dir.path().join("workspace_manifest.json");
        assert!(manifest_path.exists(), "Workspace manifest must be created");

        // Close workspace
        app.close_workspace();
        assert!(
            !app.has_workspace(),
            "AppBackend must not have workspace after closing"
        );

        // Reopen existing workspace
        app.internal_open_workspace(&path_str, false);
        assert!(
            app.has_workspace(),
            "AppBackend must have workspace after opening existing"
        );
    }
}