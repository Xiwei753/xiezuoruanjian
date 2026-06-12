// =============================================================================
// app_backend.rs — Desktop 客户端全局底层状态与公共桥接后端
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
// - 被 apps/desktop/src/backend/mod.rs 引用，作为核心底层指针底座，被 SafeAppPtr 传递至各个分域后端。
// - 被 apps/desktop/src/main.rs 注册为 QML 内命名空间 "SujianApp" 下的 "AppBackend"。
// =============================================================================

use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::{QJsonArray, QJsonObject, QJsonValue, QString};
use rfd::FileDialog;
use std::collections::HashSet;
use std::sync::OnceLock;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use writer_core::api::WriterCoreApi;

use super::json_utils::{
    bridge_error_object, bridge_success_object, qjson_array_data_from_json, qjson_object_from_json,
    serde_to_qjson_object, serde_value_to_qjson,
};
use crate::{starmap_bridge, sync_bridge, writing_bridge};

cpp! {{
    #include <QtGlobal>
}}

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

struct DebugConfig {
    enabled: bool,
    qml_enabled: bool,
    modules: HashSet<String>,
    all_modules: bool,
    level: DebugLevel,
}

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

#[allow(dead_code)]
fn debug_enabled() -> bool {
    get_debug_config().enabled
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

#[allow(dead_code)]
pub(crate) fn debug_log_static(module: &str, event: &str, message: &str) {
    if debug_level_enabled(module, DebugLevel::Info) {
        println!(
            "[SujianDebug][static][module={}][event={}] {}",
            module, event, message
        );
    }
}

#[allow(dead_code)]
pub(crate) fn debug_warn_static(module: &str, event: &str, message: &str) {
    if debug_level_enabled(module, DebugLevel::Warn) {
        eprintln!(
            "[SujianDebug][WARN][static][module={}][event={}] {}",
            module, event, message
        );
    }
}

#[allow(dead_code)]
pub(crate) fn debug_error_static(module: &str, event: &str, message: &str) {
    if debug_level_enabled(module, DebugLevel::Error) {
        eprintln!(
            "[SujianDebug][ERROR][static][module={}][event={}] {}",
            module, event, message
        );
    }
}

use sync_bridge::SyncTaskOutcome;

#[path = "system_utils.rs"]
mod system_utils;

#[allow(dead_code)]
#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct AppBackend {
    base: qt_base_class!(trait QObject),

    workspace_opened: qt_signal!(),
    workspace_content_changed: qt_signal!(),
    workspace_state_changed: qt_signal!(),
    projects_reloaded: qt_signal!(),
    #[allow(non_snake_case)]
    projectsReloaded: qt_signal!(),
    save_status_changed: qt_signal!(),
    word_count_changed: qt_signal!(),
    error_occurred: qt_signal!(),
    selected_item_changed: qt_signal!(),
    chapter_path_changed: qt_signal!(),
    clear_editor: qt_signal!(),

    sync_config_changed: qt_signal!(),
    sync_action_completed: qt_signal!(),
    sync_status_changed: qt_signal!(),

    settings_changed: qt_signal!(),

    system_color_scheme: qt_property!(QString; READ system_color_scheme NOTIFY system_color_scheme_changed),
    system_color_scheme_changed: qt_signal!(),

    ai_available: qt_property!(bool; READ ai_available NOTIFY ai_available_changed),
    ai_enabled: qt_property!(bool; READ ai_enabled WRITE set_ai_enabled NOTIFY ai_enabled_changed),
    ai_enabled_changed: qt_signal!(),
    ai_available_changed: qt_signal!(),

    pending_github_init_path_changed: qt_signal!(),
    query_system_color_scheme: qt_method!(fn(&mut self)),
    copy_text_to_clipboard: qt_method!(fn(&mut self, text: QString) -> QString),
    debug_qml_enabled: qt_property!(bool; READ debug_qml_enabled),
    sujian_editor_item_enabled: qt_property!(bool; READ sujian_editor_item_enabled),
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
    stats_previous_text: String,

    current_sync_enabled: bool,
    current_sync_backend_type: String,
    current_sync_remote_url: String,
    current_sync_branch: String,
    current_sync_auto_sync: bool,
    current_sync_interval: u32,
    current_sync_proxy_enabled: bool,
    current_sync_proxy_type: String,
    current_sync_proxy_host: String,
    current_sync_proxy_port: u16,
    current_sync_username: String,
    current_sync_token: String,
    current_sync_operation_state: String,
    current_sync_operation_id: String,
    current_sync_operation_kind: String,
    current_sync_status: String,
    current_sync_in_progress: bool,
    current_last_sync_time: i64,
    current_last_auto_sync_reason: String,
    current_last_auto_sync_started_at: i64,

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
    current_setting_theme_mode: String,
    current_setting_monet_color: String,
    current_setting_typing_animation_enabled: bool,
    current_setting_smooth_cursor_enabled: bool,
    current_setting_typing_animation_duration_ms: u32,
    current_setting_smooth_cursor_duration_ms: u32,
}

impl AppBackend {
    fn core_api(&self) -> Option<WriterCoreApi> {
        if self.current_has_workspace && !self.current_workspace.is_empty() {
            Some(WriterCoreApi::new(&self.current_workspace))
        } else {
            None
        }
    }

    fn debug_qml_enabled(&self) -> bool {
        get_debug_config().qml_enabled
    }

    fn sujian_editor_item_enabled(&self) -> bool {
        match std::env::var("SUJIAN_DESKTOP_USE_SUJIAN_EDITOR") {
            Ok(v) => !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off"),
            Err(_) => true,
        }
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

    fn query_system_color_scheme(&mut self) {
        let scheme = system_utils::detect_system_theme_from_platform();
        self.current_system_color_scheme = scheme;
        self.system_color_scheme_changed();
    }

    fn copy_text_to_clipboard(&mut self, text: QString) -> QString {
        let result = system_utils::copy_text_to_clipboard_impl(&text.to_string());
        result.to_string().into()
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
    #[cfg(not(windows))]
    fn test_create_project_failure() -> Result<(), Box<dyn std::error::Error>> {
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
        assert!(res["userMessage"].as_str().ok_or("No userMessage")?.contains("不能为空"));

        Ok(())
    }

    #[test]
    fn test_handle_sync_outcome_success_pending_path() {
        let mut backend = AppBackend::default();
        backend.current_pending_github_init_path = "/tmp/test_workspace".to_string();
        backend.current_has_workspace = false;

        let outcome = SyncTaskOutcome {
            operation_id: "".to_string(),
            operation_kind: "sync".to_string(),
            sync_status: "success".to_string(),
            action_result: "OK".to_string(),
        };
        backend.handle_sync_outcome(outcome);

        assert_eq!(backend.current_sync_status, "success");
        assert_eq!(backend.current_pending_github_init_path, "");
    }

    #[test]
    fn test_handle_sync_outcome_conflict_reloads_tree() {
        let mut backend = AppBackend::default();
        backend.current_has_workspace = true;

        let outcome = SyncTaskOutcome {
            operation_id: "".to_string(),
            operation_kind: "sync".to_string(),
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
            operation_kind: "sync".to_string(),
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
            .contains("未配置远程仓库 URL"));
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

        let dir = tempdir().unwrap();
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
