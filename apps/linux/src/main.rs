#![recursion_limit = "256"]
//! # Linux 客户端入口（Linux UI 层 - Backend Adapter）
//!
//! 这是 Linux 桌面客户端的入口文件，包含 AppBackend QObject。
//!
//! ## 架构定位
//!
//! ```text
//! QML UI → AppBackend/Linux adapter → WriterCoreApi → facade::WriterCore → Core domain
//! ```
//!
//! ## 职责边界
//!
//! - **做**：将 WriterCore API 暴露为 QML 可调用的 QObject 方法
//! - **不做**：业务逻辑（全部委托给 WriterCore）
//! - **不做**：文件 I/O（由 WriterCore 负责）
//! - **不做**：排版格式化（由 DocumentHandler 负责）
//!
//! ## 设计原则
//!
//! - AppBackend 是薄适配层，只做 QML ↔ Rust 类型转换
//! - 所有业务逻辑都在 Core 层
//! - QML 只绑定 AppBackend 暴露的属性和方法
//!
//! ## 调用链示例
//!
//! ```text
//! QML: AppBackend.create_project_json("My Book")
//!   → Rust: WriterCoreApi::create_project("My Book")
//!     → Core facade: WriterCore::create_project()
//!       → Core domain: project::create_project()
//! ```

use qmetaobject::log::{install_message_handler, QMessageLogContext, QtMsgType};
use qmetaobject::prelude::*;

use qmetaobject::{QJsonArray, QJsonObject, QJsonValue, QString};
use rfd::FileDialog;
use std::cell::RefCell;
use std::ffi::CStr;
use std::io::Write;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::sync::OnceLock;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use writer_core::api::WriterCoreApi;
use writer_core::facade::WriterCore;

mod document_handler;
mod starmap_bridge;
mod writing_bridge;
mod sync_bridge;

fn serde_value_to_qjson(value: serde_json::Value) -> QJsonValue {
    match value {
        serde_json::Value::Null => QJsonValue::default(),
        serde_json::Value::Bool(v) => QJsonValue::from(v),
        serde_json::Value::Number(v) => QJsonValue::from(v.as_f64().unwrap_or_default()),
        serde_json::Value::String(v) => QJsonValue::from(QString::from(v)),
        serde_json::Value::Array(values) => {
            let mut arr = QJsonArray::default();
            for item in values {
                arr.push(serde_value_to_qjson(item));
            }
            QJsonValue::from(arr)
        }
        serde_json::Value::Object(values) => {
            let mut obj = QJsonObject::default();
            for (key, item) in values {
                obj.insert(&key, serde_value_to_qjson(item));
            }
            QJsonValue::from(obj)
        }
    }
}

fn serde_to_qjson_object(value: serde_json::Value) -> QJsonObject {
    if let serde_json::Value::Object(values) = value {
        let mut obj = QJsonObject::default();
        for (key, item) in values {
            obj.insert(&key, serde_value_to_qjson(item));
        }
        obj
    } else {
        QJsonObject::default()
    }
}

fn serde_to_qjson_array(value: serde_json::Value) -> QJsonArray {
    if let serde_json::Value::Array(values) = value {
        let mut arr = QJsonArray::default();
        for item in values {
            arr.push(serde_value_to_qjson(item));
        }
        arr
    } else {
        QJsonArray::default()
    }
}

fn bridge_error_object(message: &str, code: &str) -> QJsonObject {
    serde_to_qjson_object(serde_json::json!({
        "success": false,
        "code": code,
        "error": message,
        "message": message
    }))
}

fn bridge_success_object(data: serde_json::Value) -> QJsonObject {
    serde_to_qjson_object(serde_json::json!({
        "success": true,
        "data": data
    }))
}

fn qjson_object_from_json(raw: &str) -> QJsonObject {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(value) => serde_to_qjson_object(value),
        Err(e) => bridge_error_object(&format!("无效 Bridge 返回: {}", e), "JSON_ERROR"),
    }
}

fn qjson_array_data_from_json(raw: &str) -> QJsonArray {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(value) => serde_to_qjson_array(value.get("data").cloned().unwrap_or(serde_json::Value::Array(vec![]))),
        Err(_) => QJsonArray::default(),
    }
}

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
        let enabled = std::env::var("WRITER_DEBUG").map(|v| v == "1").unwrap_or(false);
        let qml_enabled = std::env::var("WRITER_DEBUG_QML").map(|v| v == "1").unwrap_or(false);
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
    matches!(error, writer_core::api::error::WriterError::EmptyOverwriteBlocked { .. })
}

fn blocked_empty_overwrite_user_message() -> &'static str {
    "已阻止空内容覆盖，原章节内容已保留"
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
fn debug_log_static(module: &str, event: &str, message: &str) {
    if debug_level_enabled(module, DebugLevel::Info) {
        println!("[WriterDebug][static][module={}][event={}] {}", module, event, message);
    }
}

#[allow(dead_code)]
fn debug_warn_static(module: &str, event: &str, message: &str) {
    if debug_level_enabled(module, DebugLevel::Warn) {
        eprintln!("[WriterDebug][WARN][static][module={}][event={}] {}", module, event, message);
    }
}

#[allow(dead_code)]
fn debug_error_static(module: &str, event: &str, message: &str) {
    if debug_level_enabled(module, DebugLevel::Error) {
        eprintln!("[WriterDebug][ERROR][static][module={}][event={}] {}", module, event, message);
    }
}

qmetaobject::qrc!(qml_resources, "/" {
    // Pages
    "qml/main.qml" as "main.qml",
    "qml/DesignTokens.qml" as "DesignTokens.qml",
    "qml/ProjectCard.qml" as "ProjectCard.qml",
    "qml/ProjectHomePage.qml" as "ProjectHomePage.qml",
    "qml/HubPageFrame.qml" as "HubPageFrame.qml",
    "qml/HubPageHeader.qml" as "HubPageHeader.qml",
    "qml/HubContentGrid.qml" as "HubContentGrid.qml",
    "qml/CardCollectionPage.qml" as "CardCollectionPage.qml",
    "qml/StarMapPreviewPage.qml" as "StarMapPreviewPage.qml",
    "qml/StarMapCard.qml" as "StarMapCard.qml",
    "qml/StarMapPage.qml" as "StarMapPage.qml",
    "qml/StarMapWorkspace.qml" as "StarMapWorkspace.qml",
    "qml/StarMapCanvas.qml" as "StarMapCanvas.qml",
    "qml/StarMapGraphController.qml" as "StarMapGraphController.qml",
    "qml/StarMapNode.qml" as "StarMapNode.qml",
    "qml/StarMapInspector.qml" as "StarMapInspector.qml",
    "qml/StatsPreviewPage.qml" as "StatsPreviewPage.qml",
    "qml/StatCard.qml" as "StatCard.qml",
    "qml/CreativeHub.qml" as "CreativeHub.qml",
    "qml/AppController.qml" as "AppController.qml",
    "qml/WritingWorkspace.qml" as "WritingWorkspace.qml",
    "qml/WritingTreeController.qml" as "WritingTreeController.qml",
    "qml/EditorController.qml" as "EditorController.qml",
    "qml/SmoothCursor.qml" as "SmoothCursor.qml",
    "qml/TopWritingToolbar.qml" as "TopWritingToolbar.qml",
    "qml/RightDrawer.qml" as "RightDrawer.qml",
    "qml/SettingsDialog.qml" as "SettingsDialog.qml",
    "qml/SettingsSection.qml" as "SettingsSection.qml",
    "qml/SettingCard.qml" as "SettingCard.qml",
    "qml/ModernSwitch.qml" as "ModernSwitch.qml",
    "qml/ModernComboBox.qml" as "ModernComboBox.qml",
    "qml/DashboardGrid.qml" as "DashboardGrid.qml",
    "qml/DashboardSection.qml" as "DashboardSection.qml",
    "qml/EditorPage.qml" as "EditorPage.qml",
    "qml/ActionRegistryPage.qml" as "ActionRegistryPage.qml",
    "qml/SyncPage.qml" as "SyncPage.qml",
    "qml/EmptyWorkspace.qml" as "EmptyWorkspace.qml",
    // Components
    "qml/AppButton.qml" as "AppButton.qml",
    "qml/AppCard.qml" as "AppCard.qml",
    "qml/AppTextField.qml" as "AppTextField.qml",
    "qml/AppSwitch.qml" as "AppSwitch.qml",
    "qml/AppSlider.qml" as "AppSlider.qml",
    "qml/AppComboBox.qml" as "AppComboBox.qml",
    "qml/AppText.qml" as "AppText.qml",
    "qml/SectionHeader.qml" as "SectionHeader.qml",
    "qml/SettingsRow.qml" as "SettingsRow.qml",
    "qml/SidebarItem.qml" as "SidebarItem.qml",
    "qml/WorkspaceTree.qml" as "WorkspaceTree.qml",
    "qml/CreateProjectDialog.qml" as "CreateProjectDialog.qml",
    "qml/StatusPill.qml" as "StatusPill.qml",
    "qml/ToolbarButton.qml" as "ToolbarButton.qml",
});

use sync_bridge::{SyncTaskOutcome, mask_sync_error, sync_error_category, determine_diagnostics_status, format_diagnostics_message, save_sync_configs};

fn try_kreadconfig(cmd: &str) -> Option<String> {
    let output = std::process::Command::new(cmd)
        .args(["--file", "kdeglobals", "--group", "General", "--key", "ColorScheme"])
        .output().ok()?;
    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_lowercase();
        if value.contains("dark") {
            return Some("dark".to_string());
        }
    }
    None
}

#[allow(dead_code)]
#[allow(non_snake_case)]
#[derive(QObject, Default)]
struct AppBackend {
    base: qt_base_class!(trait QObject),

    workspace_path: qt_property!(QString; READ workspace_path NOTIFY workspace_opened),
    has_workspace: qt_property!(bool; READ has_workspace NOTIFY workspace_state_changed),
    save_status: qt_property!(QString; READ save_status WRITE set_save_status NOTIFY save_status_changed),
    word_count: qt_property!(i32; READ word_count WRITE set_word_count NOTIFY word_count_changed),
    error_message: qt_property!(QString; READ error_message NOTIFY error_occurred),
    selected_item_id: qt_property!(QString; READ selected_item_id NOTIFY selected_item_changed),
    has_selected_chapter_prop: qt_property!(bool; READ has_selected_chapter_prop NOTIFY selected_item_changed),
    chapter_path: qt_property!(QString; READ chapter_path NOTIFY chapter_path_changed),

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
    set_sync_token: qt_method!(fn(&mut self, token: QString)),

    sync_config_changed: qt_signal!(),
    sync_action_result: qt_property!(QString; READ sync_action_result NOTIFY sync_action_completed),
    sync_action_completed: qt_signal!(),
    sync_status: qt_property!(QString; READ sync_status WRITE set_sync_status NOTIFY sync_status_changed),
    sync_status_changed: qt_signal!(),

    setting_font_size: qt_property!(f32; READ setting_font_size WRITE set_setting_font_size NOTIFY settings_changed),
    setting_line_spacing: qt_property!(f32; READ setting_line_spacing WRITE set_setting_line_spacing NOTIFY settings_changed),
    setting_auto_save_enabled: qt_property!(bool; READ setting_auto_save_enabled WRITE set_setting_auto_save_enabled NOTIFY settings_changed),
    setting_auto_save_delay_ms: qt_property!(u32; READ setting_auto_save_delay_ms WRITE set_setting_auto_save_delay_ms NOTIFY settings_changed),
    setting_auto_indent_enabled: qt_property!(bool; READ setting_auto_indent_enabled WRITE set_setting_auto_indent_enabled NOTIFY settings_changed),
    setting_auto_indent_width: qt_property!(f32; READ setting_auto_indent_width WRITE set_setting_auto_indent_width NOTIFY settings_changed),
    setting_theme_mode: qt_property!(QString; READ setting_theme_mode WRITE set_setting_theme_mode NOTIFY settings_changed),
    setting_monet_color: qt_property!(QString; READ setting_monet_color WRITE set_setting_monet_color NOTIFY settings_changed),

    setting_typing_animation_enabled: qt_property!(bool; READ setting_typing_animation_enabled WRITE set_setting_typing_animation_enabled NOTIFY settings_changed),
    setting_smooth_cursor_enabled: qt_property!(bool; READ setting_smooth_cursor_enabled WRITE set_setting_smooth_cursor_enabled NOTIFY settings_changed),
    setting_typing_animation_duration_ms: qt_property!(u32; READ setting_typing_animation_duration_ms WRITE set_setting_typing_animation_duration_ms NOTIFY settings_changed),
    setting_smooth_cursor_duration_ms: qt_property!(u32; READ setting_smooth_cursor_duration_ms WRITE set_setting_smooth_cursor_duration_ms NOTIFY settings_changed),

    settings_changed: qt_signal!(),

    system_color_scheme: qt_property!(QString; READ system_color_scheme NOTIFY system_color_scheme_changed),
    system_color_scheme_changed: qt_signal!(),

    ai_available: qt_property!(bool; READ ai_available NOTIFY ai_available_changed),
    ai_enabled: qt_property!(bool; READ ai_enabled WRITE set_ai_enabled NOTIFY ai_enabled_changed),
    ai_enabled_changed: qt_signal!(),
    ai_available_changed: qt_signal!(),

    load_local_settings: qt_method!(fn(&mut self)),
    save_local_settings: qt_method!(fn(&mut self) -> bool),
    perform_sync_diagnostics: qt_method!(fn(&mut self)),

    load_sync_config: qt_method!(fn(&mut self)),
    save_sync_config: qt_method!(fn(&mut self) -> bool),
    perform_sync_dry_run: qt_method!(fn(&mut self)),
    perform_sync: qt_method!(fn(&mut self)),
    request_auto_sync: qt_method!(fn(&mut self, reason: QString)),
    maybe_auto_sync_on_foreground: qt_method!(fn(&mut self)),
    open_workspace_dir: qt_method!(fn(&mut self)),

    try_restore_last_workspace: qt_method!(fn(&mut self)),
    create_new_workspace: qt_method!(fn(&mut self)),
    open_existing_workspace: qt_method!(fn(&mut self)),
    close_workspace: qt_method!(fn(&mut self)),
    clear_last_workspace: qt_method!(fn(&mut self)),
    switch_workspace: qt_method!(fn(&mut self)),
    init_workspace_from_github: qt_method!(fn(&mut self)),
    pending_github_init_path: qt_property!(QString; READ pending_github_init_path NOTIFY pending_github_init_path_changed),
    pending_github_init_path_changed: qt_signal!(),
    execute_github_init: qt_method!(fn(&mut self, path: QString, remote_url: QString, branch: QString, token: QString, proxy_type: QString, proxy_host: QString, proxy_port: u16)),
    query_system_color_scheme: qt_method!(fn(&mut self)),
    get_workspace_diagnostics: qt_method!(fn(&self) -> QString),
    copy_text_to_clipboard: qt_method!(fn(&mut self, text: QString) -> QString),
    debug_qml_enabled: qt_property!(bool; READ debug_qml_enabled),
    debug_module_enabled_qml: qt_method!(fn(&self, module: QString) -> bool),
    log_qml: qt_method!(fn(&self, level: QString, module: QString, event: QString, message: QString)),

    create_new_volume: qt_method!(fn(&mut self, project_id: QString, title: QString)),
    create_new_chapter:
        qt_method!(fn(&mut self, project_id: QString, volume_id: QString, title: QString)),

    rename_project: qt_method!(fn(&mut self, project_id: QString, new_title: QString)),
    delete_project: qt_method!(fn(&mut self, project_id: QString)),
    reorder_projects: qt_method!(fn(&mut self, ordered_ids_joined: QString)),

    rename_volume:
        qt_method!(fn(&mut self, project_id: QString, volume_id: QString, new_title: QString)),
    delete_volume: qt_method!(fn(&mut self, project_id: QString, volume_id: QString)),
    reorder_volumes: qt_method!(fn(&mut self, project_id: QString, ordered_ids_joined: QString)),

    rename_chapter: qt_method!(
        fn(
            &mut self,
            project_id: QString,
            volume_id: QString,
            chapter_id: QString,
            new_title: QString,
        )
    ),
    delete_chapter:
        qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString)),
    reorder_chapters: qt_method!(
        fn(&mut self, project_id: QString, volume_id: QString, ordered_ids_joined: QString)
    ),

    get_tree_model: qt_method!(fn(&self) -> QJsonArray),
    get_tree_model_json: qt_method!(fn(&self) -> QString),
    get_mind_map_snapshot_json: qt_method!(fn(&self, project_id: QString) -> QString),
    create_mind_map_graph_json: qt_method!(fn(&mut self, project_id: QString, title: QString) -> QString),
    list_mind_map_graphs_json: qt_method!(fn(&self, project_id: QString) -> QString),
    set_default_mind_map_graph_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString) -> QString),
    create_mind_map_node_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, node_json: QString) -> QString),
    update_mind_map_node_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, node_id: QString, patch_json: QString) -> QString),
    delete_mind_map_node_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, node_id: QString, cascade: bool) -> QString),
    create_mind_map_edge_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, edge_json: QString) -> QString),
    update_mind_map_edge_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, edge_id: QString, patch_json: QString) -> QString),
    delete_mind_map_edge_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, edge_id: QString) -> QString),
    create_mind_map_anchor_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, anchor_json: QString) -> QString),
    bind_mind_map_anchor_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, node_id: QString, anchor_id: QString, link_kind: QString) -> QString),
    save_mind_map_layout_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, layout_json: QString) -> QString),

    refresh_app_state_json: qt_method!(fn(&mut self) -> QString),
    create_project_json: qt_method!(fn(&mut self, title: QString, action_id: QString) -> QString),
    create_volume_json: qt_method!(fn(&mut self, project_id: QString, title: QString, action_id: QString) -> QString),
    create_chapter_json: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, title: QString, action_id: QString) -> QString),
    select_tree_item_json: qt_method!(fn(&mut self, item_type: QString, project_id: QString, volume_id: QString, chapter_id: QString, action_id: QString) -> QString),
    delete_project_json: qt_method!(fn(&mut self, project_id: QString, action_id: QString) -> QString),
    delete_volume_json: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, action_id: QString) -> QString),
    delete_chapter_json: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, action_id: QString) -> QString),

    refresh_tree_model_json: qt_method!(fn(&mut self) -> QString),
    calculate_word_count: qt_method!(fn(&mut self, text: QString)),

    select_project: qt_method!(fn(&mut self, project_id: QString)),
    select_volume: qt_method!(fn(&mut self, project_id: QString, volume_id: QString)),
    select_chapter:
        qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString)),

    open_chapter_json: qt_method!(
        fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString
    ),
    open_chapter: qt_method!(
        fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QJsonObject
    ),
    get_chapter_content: qt_method!(
        fn(&self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString
    ),
    save_chapter: qt_method!(
        fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, content: QString) -> QJsonObject
    ),
    clear_chapter_content: qt_method!(
        fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QJsonObject
    ),
    report_writing_event: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, source: QString, inserted_chars: u32, deleted_chars: u32, pasted_chars: u32)),
    process_writing_event_from_text: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, old_text: QString, new_text: QString)),
    get_writing_stats_summary: qt_method!(fn(&self, start_date: QString, end_date: QString) -> QString),
    get_writing_stats_summary_object: qt_method!(fn(&self, start_date: QString, end_date: QString) -> QJsonObject),
    get_writing_stats_by_project: qt_method!(fn(&self, start_date: QString, end_date: QString) -> QString),
    get_writing_stats_by_project_object: qt_method!(fn(&self, start_date: QString, end_date: QString) -> QJsonObject),
    get_writing_stats_by_chapter: qt_method!(fn(&self, start_date: QString, end_date: QString) -> QString),
    get_writing_stats_by_chapter_object: qt_method!(fn(&self, start_date: QString, end_date: QString) -> QJsonObject),
    get_writing_stats_by_device: qt_method!(fn(&self, start_date: QString, end_date: QString) -> QString),
    get_writing_stats_by_device_object: qt_method!(fn(&self, start_date: QString, end_date: QString) -> QJsonObject),
    get_writing_speed_curve: qt_method!(fn(&self, start_date: QString, end_date: QString, bucket_minutes: u32) -> QString),
    get_writing_speed_curve_object: qt_method!(fn(&self, start_date: QString, end_date: QString, bucket_minutes: u32) -> QJsonObject),
    flush_writing_stats: qt_method!(fn(&self)),
    flush_recent_edits: qt_method!(fn(&self)),

    list_starmaps_json: qt_method!(fn(&self) -> QString),
    list_starmaps: qt_method!(fn(&self) -> QJsonArray),
    list_starmaps_for_project_json: qt_method!(fn(&self, project_id: QString) -> QString),
    get_starmap_json: qt_method!(fn(&self, starmap_id: QString) -> QString),
    create_starmap_json: qt_method!(fn(&mut self, title: QString, description: QString, accent_color: QString) -> QString),
    create_starmap: qt_method!(fn(&mut self, title: QString, description: QString, accent_color: QString) -> QJsonObject),
    create_child_starmap_json: qt_method!(fn(&mut self, parent_id: QString, title: QString, description: QString, accent_color: QString) -> QString),
    rename_starmap_json: qt_method!(fn(&mut self, starmap_id: QString, new_title: QString) -> QString),
    delete_starmap_json: qt_method!(fn(&mut self, starmap_id: QString) -> QString),
    bind_starmap_to_project_json: qt_method!(fn(&mut self, starmap_id: QString, project_id: QString) -> QString),
    set_main_starmap_json: qt_method!(fn(&mut self, starmap_id: QString, project_id: QString) -> QString),
    get_main_starmap_json: qt_method!(fn(&self, project_id: QString) -> QString),
    unbind_starmap_json: qt_method!(fn(&mut self, starmap_id: QString) -> QString),
    get_starmap_graph_json: qt_method!(fn(&self, starmap_id: QString) -> QString),
    get_starmap_graph: qt_method!(fn(&self, starmap_id: QString) -> QJsonObject),
    create_starmap_node_json: qt_method!(fn(&mut self, starmap_id: QString, title: QString, kind: QString, x: f64, y: f64) -> QString),
    create_starmap_node: qt_method!(fn(&mut self, starmap_id: QString, title: QString, kind: QString, x: f64, y: f64) -> QJsonObject),
    update_starmap_node_json: qt_method!(fn(&mut self, starmap_id: QString, node_id: QString, patch_json: QString) -> QString),
    update_starmap_node: qt_method!(fn(&mut self, starmap_id: QString, node_id: QString, patch_json: QString) -> QJsonObject),
    delete_starmap_node_json: qt_method!(fn(&mut self, starmap_id: QString, node_id: QString) -> QString),
    delete_starmap_node: qt_method!(fn(&mut self, starmap_id: QString, node_id: QString) -> QJsonObject),
    create_starmap_edge_json: qt_method!(fn(&mut self, starmap_id: QString, from_node_id: QString, to_node_id: QString, kind: QString, label: QString) -> QString),
    create_starmap_edge: qt_method!(fn(&mut self, starmap_id: QString, from_node_id: QString, to_node_id: QString, kind: QString, label: QString) -> QJsonObject),
    update_starmap_edge_json: qt_method!(fn(&mut self, starmap_id: QString, edge_id: QString, patch_json: QString) -> QString),
    update_starmap_edge: qt_method!(fn(&mut self, starmap_id: QString, edge_id: QString, patch_json: QString) -> QJsonObject),
    delete_starmap_edge_json: qt_method!(fn(&mut self, starmap_id: QString, edge_id: QString) -> QString),
    delete_starmap_edge: qt_method!(fn(&mut self, starmap_id: QString, edge_id: QString) -> QJsonObject),
    save_starmap_layout_json: qt_method!(fn(&mut self, starmap_id: QString, layout_json: QString) -> QString),
    save_starmap_layout: qt_method!(fn(&mut self, starmap_id: QString, layout_json: QString) -> QJsonObject),

    has_selected_chapter: qt_method!(fn(&self) -> bool),
    selected_chapter_exists: qt_method!(fn(&self) -> bool),
    clear_editor_state: qt_method!(fn(&mut self)),

    list_registered_actions: qt_method!(fn(&mut self) -> QString),
    execute_action: qt_method!(fn(&mut self, action_id: QString, args_json: QString, context_json: QString) -> QString),


    core: Option<Rc<RefCell<WriterCore>>>,
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
    current_sync_action_result: String,
    current_sync_status: String,
    current_sync_in_progress: bool,
    current_last_sync_time: i64,
    current_last_auto_sync_reason: String,
    current_last_auto_sync_started_at: i64,

    current_system_color_scheme: String,
    current_pending_github_init_path: String,
    current_ai_enabled: bool,
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

    // TODO(api): migrate when WriterCoreApi exposes this capability
    fn core_facade(&self) -> Option<WriterCore> {
        if self.current_has_workspace && !self.current_workspace.is_empty() {
            Some(WriterCore::new(&self.current_workspace))
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
            
            let prefix = format!("[WriterDebug][qml][module={}][event={}]", m, ev);
            let state = format!("[workspace_exists={}][proj={}][vol={}][chap={}]", ws_exists, proj, vol, chap);
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
                "[WriterDebug][module={}][event={}][workspace_exists={}][proj={}][vol={}][chap={}] {}",
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
                "[WriterDebug][WARN][module={}][event={}][workspace_exists={}][proj={}][vol={}][chap={}] {}",
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
                "[WriterDebug][ERROR][module={}][event={}][workspace_exists={}][proj={}][vol={}][chap={}] {}",
                module, event, ws_exists, proj, vol, chap, message
            );
        }
    }

    fn handle_sync_outcome(&mut self, outcome: SyncTaskOutcome) {
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

    fn now_epoch_seconds() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn handle_successful_sync_refresh(&mut self) {
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

    fn can_start_auto_sync(&self, reason: &str, min_gap_secs: i64) -> bool {
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

    fn reconcile_selection_after_tree_reload(&mut self) -> bool {
        let mut had_chapter_deleted = false;
        let Some(core_ref) = &self.core else {
            return false;
        };

        if let Some(project_id) = self.selected_project_id.clone() {
            let project_exists = {
                let core = core_ref.borrow();
                let projects = core.list_projects().unwrap_or_default();
                projects.iter().any(|p| p.id == project_id)
            };
            if !project_exists {
                self.selected_project_id = None;
                self.selected_volume_id = None;
                self.clear_editor_state();
                self.selected_item_changed();
                self.chapter_path_changed();
                return false;
            }
        }

        if let (Some(project_id), Some(volume_id)) = (self.selected_project_id.clone(), self.selected_volume_id.clone()) {
            let volume_exists = {
                let core = core_ref.borrow();
                let volumes = core.list_volumes(&project_id).unwrap_or_default();
                volumes.iter().any(|v| v.id == volume_id)
            };
            if !volume_exists {
                self.selected_volume_id = None;
                self.clear_editor_state();
                self.selected_item_changed();
                self.chapter_path_changed();
                return false;
            }
        }

        if let (Some(project_id), Some(volume_id), Some(chapter_id)) = (
            self.selected_project_id.clone(),
            self.selected_volume_id.clone(),
            self.selected_chapter_id.clone(),
        ) {
            let chapter_exists = {
                let core = core_ref.borrow();
                let chapters = core.list_chapters(&project_id, &volume_id).unwrap_or_default();
                chapters.iter().any(|c| c.id == chapter_id)
            };
            if !chapter_exists {
                self.clear_editor_state();
                had_chapter_deleted = true;
            }
        }

        self.selected_item_changed();
        self.chapter_path_changed();
        had_chapter_deleted
    }

    fn has_workspace(&self) -> bool {
        self.current_has_workspace
    }

    fn trigger_projects_reloaded(&mut self) {
        self.projects_reloaded();
        self.projectsReloaded();
    }


    fn sync_status(&self) -> QString {
        self.current_sync_status.clone().into()
    }

    fn set_sync_status(&mut self, val: QString) {
        self.current_sync_status = val.to_string();
        self.sync_status_changed();
    }

    fn refresh_sync_status_from_config(&mut self) {
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

    fn pending_github_init_path(&self) -> QString {
        self.current_pending_github_init_path.clone().into()
    }

    fn query_system_color_scheme(&mut self) {
        // Priority:
        // 1. QML-side Qt.styleHints.colorScheme (Qt 6.5+) — handled in main.qml
        // 2. Bridge fallback: try gsettings (GNOME), kreadconfig5 (KDE), env vars
        // 3. Fallback: light
        let scheme = Self::detect_system_theme_from_platform();
        self.current_system_color_scheme = scheme;
        self.system_color_scheme_changed();
    }

    fn detect_system_theme_from_platform() -> String {
        // Priority: KDE6 kreadconfig6 > KDE5 kreadconfig5 > GNOME gsettings > GTK_THEME env >
        //           gsettings gtk-theme > light fallback
        // Do NOT return "light" early from KDE detection — continue checking other signals first.

        // 1. Try KDE kreadconfig6 (Plasma 6)
        if let Some("dark") = try_kreadconfig("kreadconfig6").as_deref() {
            return "dark".to_string();
        }

        // 2. Try KDE kreadconfig5 (Plasma 5)
        if let Some("dark") = try_kreadconfig("kreadconfig5").as_deref() {
            return "dark".to_string();
        }

        // 3. Try GNOME gsettings color-scheme
        if let Ok(output) = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .output()
        {
            if output.status.success() {
                let value = String::from_utf8_lossy(&output.stdout).trim().to_lowercase();
                if value.contains("dark") {
                    return "dark".to_string();
                }
            }
        }

        // 4. Try reading GTK_THEME env var
        if let Ok(theme) = std::env::var("GTK_THEME") {
            if theme.to_lowercase().contains("dark") || theme.to_lowercase().contains("-dark") {
                return "dark".to_string();
            }
        }

        // 5. Try gsettings gtk-theme
        if let Ok(output) = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
            .output()
        {
            if output.status.success() {
                let value = String::from_utf8_lossy(&output.stdout).trim().to_lowercase();
                if value.contains("dark") || value.contains("-dark") || value.contains("_dark") {
                    return "dark".to_string();
                }
            }
        }

        // 6. Fallback to light
        "light".to_string()
    }

    fn copy_text_to_clipboard(&mut self, text: QString) -> QString {
        let text_str = text.to_string();

        // Helper to build a success response
        let mk_success = |backend: &str| -> QString {
            serde_json::json!({
                "success": true,
                "backend": backend,
                "message": format!("已复制 (backend={})", backend),
            }).to_string().into()
        };

        // 1. Try wl-copy (Wayland — best clipboard manager handoff)
        if let Ok(mut child) = std::process::Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(text_str.as_bytes());
            }
            match child.wait() {
                Ok(status) if status.success() => return mk_success("wl-copy"),
                _ => {}
            }
        }

        // 2. Try xclip (X11)
        if let Ok(mut child) = std::process::Command::new("xclip")
            .args(["-selection", "clipboard", "-in"])
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(text_str.as_bytes());
            }
            match child.wait() {
                Ok(status) if status.success() => return mk_success("xclip"),
                _ => {}
            }
        }

        // 3. Try xsel (X11 fallback)
        if let Ok(mut child) = std::process::Command::new("xsel")
            .args(["--clipboard", "--input"])
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(text_str.as_bytes());
            }
            match child.wait() {
                Ok(status) if status.success() => return mk_success("xsel"),
                _ => {}
            }
        }

        // 4. Last resort: arboard (Rust clipboard API).
        //    On Linux, arboard can raise "Clipboard was dropped very quickly"
        //    warnings because the clipboard manager may not have read the
        //    contents before the Clipboard handle is dropped.  We keep the
        //    object alive by leaking it so the clipboard manager can pick up.
        if let Ok(mut clip) = arboard::Clipboard::new() {
            if clip.set_text(text_str.clone()).is_ok() {
                Box::leak(Box::new(clip));
                return mk_success("arboard");
            }
        }

        // All backends failed
        let result = serde_json::json!({
            "success": false,
            "backend": "none",
            "message": "复制失败：未找到可用的剪贴板后端。请安装 wl-copy (Wayland)、xclip 或 xsel (X11)。",
        });
        result.to_string().into()
    }

    fn get_workspace_diagnostics(&self) -> QString {
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
        let validate_workspace = if self.current_has_workspace && self.core.is_some() {
            self.core.as_ref().and_then(|c| {
                let core = c.borrow();
                core.validate_workspace().ok()
            }).unwrap_or(false)
        } else if path_exists && path_obj.is_dir() {
            let core = writer_core::facade::WriterCore::new(ws_path);
            core.validate_workspace().unwrap_or(false)
        } else {
            false
        };
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
            && self.core.is_some()
            && validate_workspace
            && path_exists
            && is_dir
            && manifest_exists
            && projects_dir_exists
            && writable;
        let diag = serde_json::json!({
            "hasWorkspace": self.current_has_workspace,
            "workspacePath": ws_path,
            "coreInitialized": self.core.is_some(),
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

    fn sync_enabled(&self) -> bool {
        self.current_sync_enabled
    }
    fn set_sync_enabled(&mut self, val: bool) {
        self.current_sync_enabled = val;
        self.sync_config_changed();
    }

    fn sync_backend_type(&self) -> QString {
        self.current_sync_backend_type.clone().into()
    }
    fn set_sync_backend_type(&mut self, val: QString) {
        self.current_sync_backend_type = val.to_string();
        self.sync_config_changed();
    }

    fn sync_remote_url(&self) -> QString {
        self.current_sync_remote_url.clone().into()
    }
    fn set_sync_remote_url(&mut self, val: QString) {
        self.current_sync_remote_url = val.to_string();
        self.sync_config_changed();
    }

    fn sync_branch(&self) -> QString {
        self.current_sync_branch.clone().into()
    }
    fn set_sync_branch(&mut self, val: QString) {
        self.current_sync_branch = val.to_string();
        self.sync_config_changed();
    }

    fn sync_auto_sync(&self) -> bool {
        self.current_sync_auto_sync
    }
    fn set_sync_auto_sync(&mut self, val: bool) {
        self.current_sync_auto_sync = val;
        self.sync_config_changed();
    }

    fn sync_interval(&self) -> u32 {
        self.current_sync_interval
    }
    fn set_sync_interval(&mut self, val: u32) {
        self.current_sync_interval = val;
        self.sync_config_changed();
    }

    fn sync_proxy_type(&self) -> QString {
        self.current_sync_proxy_type.clone().into()
    }
    fn set_sync_proxy_type(&mut self, val: QString) {
        self.current_sync_proxy_type = val.to_string();
        self.sync_config_changed();
    }

    fn sync_proxy_host(&self) -> QString {
        self.current_sync_proxy_host.clone().into()
    }
    fn set_sync_proxy_host(&mut self, val: QString) {
        self.current_sync_proxy_host = val.to_string();
        self.sync_config_changed();
    }

    fn sync_proxy_port(&self) -> u16 {
        self.current_sync_proxy_port
    }
    fn set_sync_proxy_port(&mut self, val: u16) {
        self.current_sync_proxy_port = val;
        self.sync_config_changed();
    }

    fn sync_proxy_enabled(&self) -> bool {
        self.current_sync_proxy_enabled
    }
    fn set_sync_proxy_enabled(&mut self, val: bool) {
        self.current_sync_proxy_enabled = val;
        self.sync_config_changed();
    }

    fn sync_username(&self) -> QString {
        self.current_sync_username.clone().into()
    }
    fn set_sync_username(&mut self, val: QString) {
        self.current_sync_username = val.to_string();
        self.sync_config_changed();
    }

    fn has_sync_token(&self) -> bool {
        !self.current_sync_token.is_empty()
    }
    fn set_sync_token(&mut self, val: QString) {
        self.current_sync_token = val.to_string();
        self.sync_config_changed();
    }

    fn sync_action_result(&self) -> QString {
        self.current_sync_action_result.clone().into()
    }


    fn setting_font_size(&self) -> f32 { self.current_setting_font_size }
    fn set_setting_font_size(&mut self, val: f32) { self.current_setting_font_size = val; self.settings_changed(); self.save_local_settings(); }

    fn setting_line_spacing(&self) -> f32 { self.current_setting_line_spacing }
    fn set_setting_line_spacing(&mut self, val: f32) { self.current_setting_line_spacing = val; self.settings_changed(); self.save_local_settings(); }

    fn setting_auto_save_enabled(&self) -> bool { self.current_setting_auto_save_enabled }
    fn set_setting_auto_save_enabled(&mut self, val: bool) { self.current_setting_auto_save_enabled = val; self.settings_changed(); self.save_local_settings(); }

    fn setting_auto_save_delay_ms(&self) -> u32 { self.current_setting_auto_save_delay_ms }
    fn set_setting_auto_save_delay_ms(&mut self, val: u32) { self.current_setting_auto_save_delay_ms = val; self.settings_changed(); self.save_local_settings(); }

    fn setting_auto_indent_enabled(&self) -> bool { self.current_setting_auto_indent_enabled }
    fn set_setting_auto_indent_enabled(&mut self, val: bool) { self.current_setting_auto_indent_enabled = val; self.settings_changed(); self.save_local_settings(); }

    fn setting_auto_indent_width(&self) -> f32 { self.current_setting_auto_indent_width }
    fn set_setting_auto_indent_width(&mut self, val: f32) { self.current_setting_auto_indent_width = val; self.settings_changed(); self.save_local_settings(); }

    fn setting_theme_mode(&self) -> QString {
        if self.current_setting_theme_mode.is_empty() {
            "system".into()
        } else {
            self.current_setting_theme_mode.clone().into()
        }
    }
    fn set_setting_theme_mode(&mut self, val: QString) { self.current_setting_theme_mode = val.to_string(); self.settings_changed(); }

    fn setting_monet_color(&self) -> QString { self.current_setting_monet_color.clone().into() }
    fn set_setting_monet_color(&mut self, val: QString) { self.current_setting_monet_color = val.to_string(); self.settings_changed(); }

    fn setting_typing_animation_enabled(&self) -> bool { self.current_setting_typing_animation_enabled }
    fn set_setting_typing_animation_enabled(&mut self, val: bool) { self.current_setting_typing_animation_enabled = val; self.settings_changed(); self.save_local_settings(); }

    fn setting_smooth_cursor_enabled(&self) -> bool { self.current_setting_smooth_cursor_enabled }
    fn set_setting_smooth_cursor_enabled(&mut self, val: bool) { self.current_setting_smooth_cursor_enabled = val; self.settings_changed(); self.save_local_settings(); }

    fn setting_typing_animation_duration_ms(&self) -> u32 { self.current_setting_typing_animation_duration_ms }
    fn set_setting_typing_animation_duration_ms(&mut self, val: u32) { self.current_setting_typing_animation_duration_ms = val; self.settings_changed(); self.save_local_settings(); }

    fn setting_smooth_cursor_duration_ms(&self) -> u32 { self.current_setting_smooth_cursor_duration_ms }
    fn set_setting_smooth_cursor_duration_ms(&mut self, val: u32) { self.current_setting_smooth_cursor_duration_ms = val; self.settings_changed(); self.save_local_settings(); }

    fn try_restore_last_workspace(&mut self) {
        self.debug_log("workspace", "try_restore_last_workspace_start", "");
        if let Some(path) = writer_core::app_config::get_last_workspace_path() {
            self.debug_log("workspace", "try_restore_last_workspace_path_found", &format!("path={}", path));
            let path_obj = std::path::Path::new(&path);
            if path_obj.exists() && path_obj.is_dir() {
                let core = writer_core::facade::WriterCore::new(&path);
                let val_res = core.validate_workspace().unwrap_or(false);
                self.debug_log("workspace", "try_restore_last_workspace_validate", &format!("path={}, is_valid={}", path, val_res));
                if val_res {
                    self.core = Some(Rc::new(RefCell::new(core)));
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

    fn load_app_theme_mode(&mut self) {
        // Load theme mode from app_config (when no workspace is open)
        // Default to "system"
        self.current_setting_theme_mode = "system".to_string();
        self.current_setting_monet_color = "".to_string();
        self.settings_changed();
    }

    fn internal_open_workspace(&mut self, path: &str, initialize: bool) {
        self.debug_log("workspace", "internal_open_workspace_start", &format!("path={}, initialize={}", path, initialize));
        let path_obj = std::path::Path::new(path);
        if !path_obj.exists() || !path_obj.is_dir() {
            let err_msg = format!("路径不存在或不是目录: {}", path);
            self.set_error(&err_msg);
            self.debug_error("workspace", "internal_open_workspace_failed", &err_msg);
            return;
        }

        let core = writer_core::facade::WriterCore::new(path);
        let is_valid = core.validate_workspace().unwrap_or(false);
        self.debug_log("workspace", "internal_open_workspace_validate", &format!("path={}, is_valid={}", path, is_valid));

        if !is_valid && !initialize {
            let err_msg = "不是有效工作区。请选择其他目录，或使用「新建工作区」初始化该目录。";
            self.set_error(err_msg);
            self.debug_error("workspace", "internal_open_workspace_failed", err_msg);
            return;
        }

        if !is_valid && initialize {
            self.debug_log("workspace", "internal_open_workspace_creating", path);
            if let Err(e) = core.create_workspace() {
                let err_msg = format!("无法创建工作区: {}", e);
                self.set_error(&err_msg);
                self.debug_error("workspace", "internal_open_workspace_failed", &err_msg);
                return;
            }
        }

        let core = writer_core::facade::WriterCore::new(path);
        let val_res = core.validate_workspace().unwrap_or(false);
        self.debug_log("workspace", "internal_open_workspace_revalidate", &format!("path={}, is_valid={}", path, val_res));
        if !val_res {
            let err_msg = "工作区验证失败";
            self.set_error(err_msg);
            self.debug_error("workspace", "internal_open_workspace_failed", err_msg);
            return;
        }

        
        self.core = Some(Rc::new(RefCell::new(core)));
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

    fn create_new_workspace(&mut self) {
        self.debug_log("workspace", "create_new_workspace_clicked", "");
        if let Some(path) = FileDialog::new().pick_folder() {
            self.internal_open_workspace(&path.to_string_lossy(), true);
        } else {
            self.debug_log("workspace", "create_new_workspace_cancelled", "");
        }
    }

    fn open_existing_workspace(&mut self) {
        self.debug_log("workspace", "open_existing_workspace_clicked", "");
        if let Some(path) = FileDialog::new().pick_folder() {
            self.internal_open_workspace(&path.to_string_lossy(), false);
        } else {
            self.debug_log("workspace", "open_existing_workspace_cancelled", "");
        }
    }

    fn close_workspace(&mut self) {
        self.debug_log("workspace", "close_workspace_start", "");
        self.flush_writing_stats();
        // Clear core
        self.core = None;
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

    fn clear_last_workspace(&mut self) {
        let _ = writer_core::app_config::clear_last_workspace_path();
    }

    fn switch_workspace(&mut self) {
        self.close_workspace();
        self.clear_last_workspace();
    }

    fn init_workspace_from_github(&mut self) {
        if let Some(path) = FileDialog::new().pick_folder() {
            self.current_pending_github_init_path = path.to_string_lossy().to_string();
            self.pending_github_init_path_changed();
        }
    }

    fn execute_github_init(&mut self, path: QString, remote_url: QString, branch: QString, token: QString, proxy_type: QString, proxy_host: QString, proxy_port: u16) {
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

    fn do_github_init(
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
            let core = writer_core::facade::WriterCore::new(path);
            core.validate_workspace().unwrap_or(false)
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
                        let core = writer_core::facade::WriterCore::new(path);
                        if !core.validate_workspace().unwrap_or(false) {
                            // Remote is empty or not a valid workspace — create workspace locally
                            if let Err(e) = core.create_workspace() {
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

    fn load_local_settings(&mut self) {
        self.debug_log("settings", "load_local_settings_start", "");
        if let Some(core) = self.core_api() {

            let local_load = core.load_local_settings();
            self.debug_log("settings", "load_local_settings_result", &format!("success={}", local_load.is_ok()));
            if let Ok(settings) = local_load {
                self.current_setting_line_spacing = settings.editor_line_spacing_multiplier;
                self.current_setting_auto_save_enabled = settings.auto_save_enabled;
                self.current_setting_auto_save_delay_ms = settings.auto_save_delay_ms as u32;
                self.current_setting_auto_indent_enabled = settings.auto_indent_enabled;
                self.current_setting_auto_indent_width = settings.auto_indent_width;
                self.current_setting_typing_animation_enabled = settings.editor_typing_animation_enabled;
                self.current_setting_smooth_cursor_enabled = settings.editor_smooth_cursor_enabled;
                self.current_setting_typing_animation_duration_ms = settings.editor_typing_animation_duration_ms as u32;
                self.current_setting_smooth_cursor_duration_ms = settings.editor_smooth_cursor_duration_ms as u32;
                self.current_ai_enabled = settings.ai_enabled;
                if let Some(ref device_id) = settings.stats_device_id {
                    if !device_id.is_empty() {
                        self.stats_device_id = device_id.clone();
                    }
                }
            }

            let syncable_load = core.load_syncable_settings();
            self.debug_log("settings", "load_syncable_settings_result", &format!("success={}", syncable_load.is_ok()));
            if let Ok(sync_settings) = syncable_load {
                self.current_setting_font_size = sync_settings.font_size as f32;
                if self.current_setting_font_size <= 0.0 {
                    if let Ok(local) = core.load_local_settings() {
                        self.current_setting_font_size = local.editor_font_size;
                    }
                    if self.current_setting_font_size <= 0.0 {
                        self.current_setting_font_size = 16.0;
                    }
                }
                self.current_setting_theme_mode = sync_settings.theme_mode.clone();
                self.current_setting_monet_color = sync_settings.monet_color.clone();
            } else {
                self.current_setting_monet_color = "".to_string();
                if let Ok(local) = core.load_local_settings() {
                    self.current_setting_font_size = local.editor_font_size;
                }
                if self.current_setting_font_size <= 0.0 {
                    self.current_setting_font_size = 16.0;
                }
                self.current_setting_theme_mode = "system".to_string();
            }

            self.settings_changed();
            self.debug_log("settings", "load_local_settings_success", &format!("fontSize={}, themeMode={}", self.current_setting_font_size, self.current_setting_theme_mode));
        } else {
            self.debug_warn("settings", "load_local_settings_failed", "core_not_initialized");
        }
    }

    fn save_local_settings(&mut self) -> bool {
        self.debug_log("settings", "save_local_settings_start", "");
        let mut error_msg: Option<String> = None;
        if let Some(core) = self.core_api() {

            let mut local = core.load_local_settings().unwrap_or_else(|_| writer_core::api::types::LocalSettingsDto::from(writer_core::settings::LocalSettings::default()));
            local.editor_font_size = self.current_setting_font_size;
            local.editor_line_spacing_multiplier = self.current_setting_line_spacing;
            local.auto_save_enabled = self.current_setting_auto_save_enabled;
            local.auto_save_delay_ms = self.current_setting_auto_save_delay_ms as u64;
            local.auto_indent_enabled = self.current_setting_auto_indent_enabled;
            local.auto_indent_width = self.current_setting_auto_indent_width;
            local.editor_typing_animation_enabled = self.current_setting_typing_animation_enabled;
            local.editor_smooth_cursor_enabled = self.current_setting_smooth_cursor_enabled;
            local.editor_typing_animation_duration_ms = self.current_setting_typing_animation_duration_ms as u64;
            local.editor_smooth_cursor_duration_ms = self.current_setting_smooth_cursor_duration_ms as u64;
            local.ai_enabled = self.current_ai_enabled;

            let local_save = core.save_local_settings(local.clone());
            self.debug_log("settings", "save_local_settings_result", &format!("success={}", local_save.is_ok()));
            if let Err(e) = local_save {
                error_msg = Some(format!("保存本地设置失败: {}", e));
            }

            let mut syncable = core.load_syncable_settings().unwrap_or_else(|_| writer_core::api::types::SyncableSettingsDto::from(writer_core::settings::SyncableSettings::default()));
            syncable.font_size = self.current_setting_font_size as f64;
            syncable.theme_mode = self.current_setting_theme_mode.clone();
            syncable.monet_color = self.current_setting_monet_color.clone();

            let syncable_save = core.save_syncable_settings(syncable.clone());
            self.debug_log("settings", "save_syncable_settings_result", &format!("success={}", syncable_save.is_ok()));
            if let Err(e) = syncable_save {
                error_msg = Some(format!("保存同步设置失败: {}", e));
            }
        } else {
            error_msg = Some("Core 未初始化".to_string());
        }

        if let Some(msg) = error_msg {
            self.set_error(&msg);
            self.debug_error("settings", "save_local_settings_failed", &msg);
            false
        } else {
            self.debug_log("settings", "save_local_settings_success", "");
            true
        }
    }

    fn perform_sync_diagnostics(&mut self) {
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
            let core = WriterCore::new(&workspace_path);
            let config = match core.load_sync_config() {
                Ok(c) => c,
                Err(e) => {
                    callback(SyncTaskOutcome {
                        sync_status: "error".to_string(),
                        action_result: format!("无法加载同步配置: {}", e),
                    });
                    return;
                }
            };

            match core.perform_sync_diagnostics(&config) {
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



    fn load_sync_config(&mut self) {
        self.debug_log("sync", "load_sync_config_start", "");
        if self.core.is_some() {
            // Extract config data without holding Ref borrow, then update self
            let (config_opt, token_opt) = {
                let core_ref = self.core.as_ref().unwrap();
                let core = core_ref.borrow();
                let cfg = core.load_sync_config().ok();
                let sec = core.load_sync_secrets().ok();
                let t = sec.and_then(|s| s.token);
                (cfg, t)
            };
            if let Some(config) = config_opt {
                self.current_sync_enabled = config.enabled;
                self.current_sync_backend_type = match config.backend_type {
                    writer_core::sync_service::BackendType::Git => "git".to_string(),
                    writer_core::sync_service::BackendType::GithubApi => "github_api".to_string(),
                    writer_core::sync_service::BackendType::WebDav => "webdav".to_string(),
                    writer_core::sync_service::BackendType::S3 => "s3".to_string(),
                    writer_core::sync_service::BackendType::LocalFolder => "local_folder".to_string(),
                };
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

    fn save_sync_config(&mut self) -> bool {
        self.debug_log("sync", "save_sync_config_start", "");
        let mut error_msg: Option<String> = None;
        if let Some(core) = self.core_facade() {

            let mut c = core
                .load_sync_config()
                .unwrap_or(writer_core::sync_service::SyncConfig {
                    enabled: false,
                    backend_type: writer_core::sync_service::BackendType::GithubApi,
                    remote_url: "".to_string(),
                    transport: writer_core::sync_service::SyncTransport::HttpsToken,
                    branch: "main".to_string(),
                    auto_sync: false,
                    sync_interval_seconds: 300,
                    proxy_enabled: false,
                    proxy_type: "none".to_string(),
                    proxy_host: "".to_string(),
                    proxy_port: 0,
                    username: "".to_string(),
                    android_has_internet_permission: true,
                    android_has_access_network_state_permission: true,
                });

            let raw_url = self.current_sync_remote_url.clone();
            let parsed = writer_core::sync_service::sanitize_remote_url(&raw_url);

            c.enabled = self.current_sync_enabled;
            c.backend_type = match self.current_sync_backend_type.as_str() {
                "github_api" => writer_core::sync_service::BackendType::GithubApi,
                "webdav" => writer_core::sync_service::BackendType::WebDav,
                "s3" => writer_core::sync_service::BackendType::S3,
                "local_folder" => writer_core::sync_service::BackendType::LocalFolder,
                _ => writer_core::sync_service::BackendType::GithubApi,
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

            let mut s = core.load_sync_secrets().unwrap_or_default();
            if let Some(ref extracted_token) = parsed.extracted_token {
                s.token = Some(extracted_token.clone());
            } else if self.current_sync_token.is_empty() {
                s.token = None;
            } else {
                s.token = Some(self.current_sync_token.clone());
            }

            if let Err(e) = core.save_sync_config(&c) {
                error_msg = Some(format!("保存同步配置失败: {}", e));
            } else if let Err(e) = core.save_sync_secrets(&s) {
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

    fn perform_sync_dry_run(&mut self) {
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
            let core = WriterCore::new(&workspace_path);
            let config = match core.load_sync_config() {
                Ok(c) => c,
                Err(e) => {
                    callback(SyncTaskOutcome {
                        sync_status: "configured_untested".to_string(),
                        action_result: format!("无法加载同步配置: {}", e),
                    });
                    return;
                }
            };

            match core.perform_sync_dry_run(&config) {
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

    fn perform_sync(&mut self) {
        self.perform_sync_internal("manual", false);
    }

    fn request_auto_sync(&mut self, reason: QString) {
        let reason_str = reason.to_string();
        self.trigger_auto_sync(&reason_str);
    }

    fn maybe_auto_sync_on_foreground(&mut self) {
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

    fn trigger_auto_sync(&mut self, reason: &str) {
        if !self.can_start_auto_sync(reason, 60) {
            self.debug_log("sync", "auto_sync_skipped", &format!("reason={}", reason));
            return;
        }
        self.current_last_auto_sync_reason = reason.to_string();
        self.current_last_auto_sync_started_at = Self::now_epoch_seconds();
        self.debug_log("sync", reason, "triggered");
        self.perform_sync_internal(reason, true);
    }

    fn perform_sync_internal(&mut self, trigger: &str, silent_success: bool) {
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
            let core = WriterCore::new(&workspace_path);
            let config = match core.load_sync_config() {
                Ok(c) => c,
                Err(e) => {
                    callback(SyncTaskOutcome {
                        sync_status: "error".to_string(),
                        action_result: format!("无法读取同步配置: {}", e),
                    });
                    return;
                }
            };
            let resolved_backend = writer_core::sync_service::resolved_backend_type(&config);
            let backend_label = match resolved_backend {
                writer_core::sync_service::BackendType::GithubApi => "github_api",
                writer_core::sync_service::BackendType::Git => "git",
                writer_core::sync_service::BackendType::WebDav => "webdav",
                writer_core::sync_service::BackendType::S3 => "s3",
                writer_core::sync_service::BackendType::LocalFolder => "local_folder",
            };
            debug_log_static(
                "sync",
                "perform_sync_backend",
                &format!("backend_type={}, sync_mode=lww_manifest", backend_label),
            );

            match core.perform_sync(&config) {
                Ok(result) => {
                    let (status, msg) = match result.status {
                        writer_core::sync_service::SyncStatus::Success => {
                            let m = format!(
                                "同步成功\n上传: {} 个文件\n下载: {} 个文件",
                                result.uploaded_files.len(),
                                result.downloaded_files.len()
                            );
                            ("success".to_string(), m)
                        }
                        writer_core::sync_service::SyncStatus::LatestWinsApplied => {
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
                        writer_core::sync_service::SyncStatus::NoChanges => {
                            ("success".to_string(), "同步完成：本地和远端均已是最新状态，无须更新。".to_string())
                        }
                        writer_core::sync_service::SyncStatus::ConfiguredUntested => {
                            ("configured_untested".to_string(), "同步配置已加载，尚未测试或执行同步。".to_string())
                        }
                        writer_core::sync_service::SyncStatus::Conflict => {
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
                            
                            let detail_str = if let Some(ref details) = result.settings_conflicts {
                                let mut lines = vec![];
                                for d in details {
                                    lines.push(format!("  • 键名: {}, 本地值: {:?}, 远程值: {:?}", d.key, d.local_value, d.remote_value));
                                }
                                format!("\n\n具体设置冲突:\n{}", lines.join("\n"))
                            } else {
                                "".to_string()
                            };

                            let m = format!(
                                "同步冲突，已停止，未覆盖任何文件\n\n原因:\n本地和远端都修改了同一批同步文件，Git 无法安全自动合并。{}\n\n冲突文件:\n  - {}\n\n下一步建议:\n1. 先备份当前工作区\n2. 运行诊断确认网络认证正常\n3. 手动处理冲突后重新同步",
                                detail_str,
                                file_str
                            );
                            ("conflict".to_string(), m)
                        }
                        writer_core::sync_service::SyncStatus::RecoverableError(ref e) => {
                            ("recoverable_error".to_string(), format!("可恢复的同步错误:\n{}\n请检查后重试。", mask_sync_error(e)))
                        }
                        writer_core::sync_service::SyncStatus::FatalError(ref e) => {
                            ("fatal_error".to_string(), format!("严重同步错误:\n{}\n建议备份数据并重新配置。", mask_sync_error(e)))
                        }
                        writer_core::sync_service::SyncStatus::DirtyRepoBlocked => {
                            ("dirty_repo_blocked".to_string(), "同步被阻止: 本地工作区存在未跟踪或未提交的修改，且这些修改不是同步安全文件。".to_string())
                        }
                        writer_core::sync_service::SyncStatus::BranchMissingRecovered => {
                            ("branch_missing_recovered".to_string(), "同步成功 (分支已恢复)\n已自动恢复并关联本地与远端分支。".to_string())
                        }
                        writer_core::sync_service::SyncStatus::Error(ref e) => {
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
                        writer_core::sync_service::SyncStatus::Idle => {
                            ("configured_untested".to_string(), "同步未执行".to_string())
                        }
                        _ => {
                            ("error".to_string(), format!("同步状态: {:?}", result.status))
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

    fn open_workspace_dir(&mut self) {
        let path = self.current_workspace.clone();
        if !path.is_empty() {
            let _ = std::process::Command::new("xdg-open")
                .arg(&path)
                .spawn();
        }
    }

        fn workspace_path(&self) -> QString {
        self.current_workspace.clone().into()
    }

    fn save_status(&self) -> QString {
        self.current_save_status.clone().into()
    }

    fn set_save_status(&mut self, status: QString) {
        self.current_save_status = status.to_string();
        self.save_status_changed();
    }

    fn word_count(&self) -> i32 {
        self.current_word_count
    }

    fn set_word_count(&mut self, count: i32) {
        self.current_word_count = count;
        self.word_count_changed();
    }

    fn error_message(&self) -> QString {
        self.current_error_message.clone().into()
    }

    fn has_selected_chapter(&self) -> bool {
        self.selected_chapter_id.is_some()
    }

    fn selected_chapter_exists(&self) -> bool {
        if let (Some(core_ref), Some(p), Some(v), Some(c)) = (
            &self.core,
            &self.selected_project_id,
            &self.selected_volume_id,
            &self.selected_chapter_id,
        ) {
            let core = core_ref.borrow();
            if let Ok(chapters) = core.list_chapters(p, v) {
                return chapters.iter().any(|chap| chap.id == *c);
            }
        }
        false
    }


    fn list_registered_actions(&mut self) -> QString {
        if let Some(core) = self.core_facade() {
            match core.list_registered_actions() {
                Ok(actions) => {
                    let json = serde_json::to_string(&actions).unwrap_or_else(|_| "[]".to_string());
                    json.into()
                },
                Err(_) => "[]".into()
            }
        } else {
            "[]".into()
        }
    }

    fn execute_action(&mut self, action_id: QString, args_json: QString, context_json: QString) -> QString {
        if let Some(core) = self.core_facade() {
            match core.execute_action(&action_id.to_string(), &args_json.to_string(), &context_json.to_string()) {
                Ok(result) => {
                    let json = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
                    json.into()
                },
                Err(e) => {
                    let err_json = serde_json::json!({
                        "success": false,
                        "message": e.to_string()
                    });
                    err_json.to_string().into()
                }
            }
        } else {
            let err_json = serde_json::json!({
                "success": false,
                "message": "Core not initialized"
            });
            err_json.to_string().into()
        }
    }

    fn clear_editor_state(&mut self) {
        self.selected_chapter_id = None;
        self.selected_item_changed();
        self.chapter_path_changed();
        self.clear_editor();
    }

    fn chapter_path(&self) -> QString {
        if let (Some(core_ref), Some(p), Some(v), Some(c)) = (
            &self.core,
            &self.selected_project_id,
            &self.selected_volume_id,
            &self.selected_chapter_id,
        ) {
            let core = core_ref.borrow();
            let mut path = String::new();
            if let Ok(projects) = core.list_projects() {
                if let Some(proj) = projects.iter().find(|x| x.id == *p) {
                    path.push_str(&proj.title);
                }
            }
            if let Ok(volumes) = core.list_volumes(p) {
                if let Some(vol) = volumes.iter().find(|x| x.id == *v) {
                    path.push_str(" > ");
                    path.push_str(&vol.title);
                }
            }
            if let Ok(chapters) = core.list_chapters(p, v) {
                if let Some(chap) = chapters.iter().find(|x| x.id == *c) {
                    path.push_str(" > ");
                    path.push_str(&chap.title);
                }
            }
            return path.into();
        }
        "".into()
    }

    fn has_selected_chapter_prop(&self) -> bool {
        self.selected_chapter_id.is_some()
    }

    fn selected_item_id(&self) -> QString {
        if let Some(ref id) = self.selected_chapter_id {
            return id.clone().into();
        }
        if let Some(ref id) = self.selected_volume_id {
            return id.clone().into();
        }
        if let Some(ref id) = self.selected_project_id {
            return id.clone().into();
        }
        "".into()
    }

    fn set_error(&mut self, msg: &str) {
        self.current_error_message = msg.to_string();
        self.error_occurred();
    }

    fn calculate_word_count(&mut self, text: QString) {
        let text_str = text.to_string();
        let count = if let Some(core_ref) = &self.core {
            core_ref.borrow().calculate_word_count(&text_str) as i32
        } else {
            writer_core::chapter::calculate_word_count(&text_str) as i32
        };
        self.set_word_count(count);
    }

    fn reload_tree(&mut self) {
        let before_count = self.cached_tree.len();
        let mut list = QJsonArray::default();
        if let Some(core) = self.core_api() {
            if let Ok(projects) = core.list_projects() {
                for p in projects {
                    let mut p_map = QJsonObject::default();
                    p_map.insert(
                        "title".into(),
                        QJsonValue::from(QString::from(p.title.clone())),
                    );
                    p_map.insert("id".into(), QJsonValue::from(QString::from(p.id.clone())));
                    p_map.insert("type".into(), QJsonValue::from(QString::from("project")));
                    list.push(QJsonValue::from(p_map));

                    if let Ok(volumes) = core.list_volumes(&p.id) {
                        for v in volumes {
                            let mut v_map = QJsonObject::default();
                            v_map.insert(
                                "title".into(),
                                QJsonValue::from(QString::from(v.title.clone())),
                            );
                            v_map
                                .insert("id".into(), QJsonValue::from(QString::from(v.id.clone())));
                            v_map.insert(
                                "projectId".into(),
                                QJsonValue::from(QString::from(p.id.clone())),
                            );
                            v_map.insert("type".into(), QJsonValue::from(QString::from("volume")));
                            list.push(QJsonValue::from(v_map));

                            if let Ok(chapters) = core.list_chapters(&p.id, &v.id) {
                                for c in chapters {
                                    let mut c_map = QJsonObject::default();
                                    c_map.insert(
                                        "title".into(),
                                        QJsonValue::from(QString::from(c.title.clone())),
                                    );
                                    c_map.insert(
                                        "id".into(),
                                        QJsonValue::from(QString::from(c.id.clone())),
                                    );
                                    c_map.insert(
                                        "projectId".into(),
                                        QJsonValue::from(QString::from(p.id.clone())),
                                    );
                                    c_map.insert(
                                        "volumeId".into(),
                                        QJsonValue::from(QString::from(v.id.clone())),
                                    );
                                    c_map.insert(
                                        "type".into(),
                                        QJsonValue::from(QString::from("chapter")),
                                    );
                                    list.push(QJsonValue::from(c_map));
                                }
                            }
                        }
                    }
                }
            }
        }
        self.cached_tree = list;
        let after_count = self.cached_tree.len();
        self.debug_log("tree", "reload_tree", &format!("before_count={}, after_count={}", before_count, after_count));
    }

    fn build_tree_model_json(&self) -> serde_json::Value {
        use serde_json::json;
        let mut tree = Vec::new();
        if let Some(core) = self.core_api() {
            if let Ok(projects) = core.list_projects() {
                for p in &projects {
                    let mut p_map = serde_json::Map::new();
                    p_map.insert("title".into(), json!(p.title));
                    p_map.insert("id".into(), json!(p.id));
                    p_map.insert("type".into(), json!("project"));
                    p_map.insert("projectId".into(), json!(""));
                    p_map.insert("volumeId".into(), json!(""));
                    tree.push(serde_json::Value::Object(p_map));

                    if let Ok(volumes) = core.list_volumes(&p.id) {
                        for v in &volumes {
                            let mut v_map = serde_json::Map::new();
                            v_map.insert("title".into(), json!(v.title));
                            v_map.insert("id".into(), json!(v.id));
                            v_map.insert("projectId".into(), json!(p.id));
                            v_map.insert("volumeId".into(), json!(v.id));
                            v_map.insert("type".into(), json!("volume"));
                            tree.push(serde_json::Value::Object(v_map));

                            if let Ok(chapters) = core.list_chapters(&p.id, &v.id) {
                                for c in &chapters {
                                    let mut c_map = serde_json::Map::new();
                                    c_map.insert("title".into(), json!(c.title));
                                    c_map.insert("id".into(), json!(c.id));
                                    c_map.insert("projectId".into(), json!(p.id));
                                    c_map.insert("volumeId".into(), json!(v.id));
                                    c_map.insert("type".into(), json!("chapter"));
                                    tree.push(serde_json::Value::Object(c_map));
                                }
                            }
                        }
                    }
                }
            }
        }
        serde_json::Value::Array(tree)
    }


    fn get_mind_map_snapshot_json(&self, project_id: QString) -> QString {
        let pid = project_id.to_string();
        if let Some(core) = self.core_facade() {
            match core.get_mind_map_snapshot(&pid) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    fn create_mind_map_graph_json(&mut self, project_id: QString, title: QString) -> QString {
        let pid = project_id.to_string();
        let t = title.to_string();
        if let Some(core) = self.core_facade() {
            match core.create_mind_map_graph(&pid, &t) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    fn list_mind_map_graphs_json(&self, project_id: QString) -> QString {
        let pid = project_id.to_string();
        if let Some(core) = self.core_facade() {
            match core.list_mind_map_graphs(&pid) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    fn set_default_mind_map_graph_json(&mut self, project_id: QString, graph_id: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        if let Some(core) = self.core_facade() {
            match core.set_default_mind_map_graph(&pid, &gid) {
                Ok(_) => serde_json::json!({ "success": true, "data": true, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    fn create_mind_map_node_json(&mut self, project_id: QString, graph_id: QString, node_json: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let nj = node_json.to_string();

        let node: writer_core::mind_map::graph::MindMapGraphNode = match serde_json::from_str(&nj) {
            Ok(n) => n,
            Err(e) => return serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("Invalid node JSON: {}", e) }).to_string().into(),
        };

        if let Some(core) = self.core_facade() {
            match core.create_mind_map_node(&pid, &gid, node) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    fn update_mind_map_node_json(&mut self, project_id: QString, graph_id: QString, node_id: QString, patch_json: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let nid = node_id.to_string();
        let pj = patch_json.to_string();

        // Define patch struct inline since we need to deserialize
        #[derive(serde::Deserialize)]
        struct NodePatch {
            title: Option<String>,
            kind: Option<writer_core::mind_map::graph::MindMapNodeKind>,
            payload: Option<serde_json::Value>,
            tags: Option<Vec<String>>,
        }

        let patch: NodePatch = match serde_json::from_str(&pj) {
            Ok(p) => p,
            Err(e) => return serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("Invalid patch JSON: {}", e) }).to_string().into(),
        };

        if let Some(core) = self.core_facade() {
            match core.update_mind_map_node(&pid, &gid, &nid, writer_core::mind_map::edit::MindMapGraphNodePatch { title: patch.title, kind: patch.kind, payload: patch.payload.map(Some), tags: patch.tags }) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    fn delete_mind_map_node_json(&mut self, project_id: QString, graph_id: QString, node_id: QString, cascade: bool) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let nid = node_id.to_string();

        if let Some(core) = self.core_facade() {
            match core.delete_mind_map_node(&pid, &gid, &nid, cascade) {
                Ok(_) => serde_json::json!({ "success": true, "data": true, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    fn create_mind_map_edge_json(&mut self, project_id: QString, graph_id: QString, edge_json: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let ej = edge_json.to_string();

        let edge: writer_core::mind_map::graph::MindMapGraphEdge = match serde_json::from_str(&ej) {
            Ok(e) => e,
            Err(e) => return serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("Invalid edge JSON: {}", e) }).to_string().into(),
        };

        if let Some(core) = self.core_facade() {
            match core.create_mind_map_edge(&pid, &gid, edge) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    fn update_mind_map_edge_json(&mut self, project_id: QString, graph_id: QString, edge_id: QString, patch_json: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let eid = edge_id.to_string();
        let pj = patch_json.to_string();

        #[derive(serde::Deserialize)]
        struct EdgePatch {
            kind: Option<writer_core::mind_map::graph::MindMapEdgeKind>,
            label: Option<String>,
            payload: Option<serde_json::Value>,
        }

        let patch: EdgePatch = match serde_json::from_str(&pj) {
            Ok(p) => p,
            Err(e) => return serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("Invalid patch JSON: {}", e) }).to_string().into(),
        };

        if let Some(core) = self.core_facade() {
            match core.update_mind_map_edge(&pid, &gid, &eid, writer_core::mind_map::edit::MindMapGraphEdgePatch { kind: patch.kind, label: patch.label.map(Some), payload: patch.payload.map(Some) }) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    fn delete_mind_map_edge_json(&mut self, project_id: QString, graph_id: QString, edge_id: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let eid = edge_id.to_string();

        if let Some(core) = self.core_facade() {
            match core.delete_mind_map_edge(&pid, &gid, &eid) {
                Ok(_) => serde_json::json!({ "success": true, "data": true, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    fn create_mind_map_anchor_json(&mut self, project_id: QString, graph_id: QString, anchor_json: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let aj = anchor_json.to_string();

        let anchor: writer_core::mind_map::anchor::MindMapAnchor = match serde_json::from_str(&aj) {
            Ok(a) => a,
            Err(e) => return serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("Invalid anchor JSON: {}", e) }).to_string().into(),
        };

        if let Some(core) = self.core_facade() {
            match core.create_mind_map_anchor(&pid, &gid, anchor) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    fn bind_mind_map_anchor_json(&mut self, project_id: QString, graph_id: QString, node_id: QString, anchor_id: QString, link_kind: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let nid = node_id.to_string();
        let aid = anchor_id.to_string();
        let lk = link_kind.to_string();

        if let Some(core) = self.core_facade() {
            match core.bind_mind_map_node_to_anchor(&pid, &gid, &nid, &aid, &lk) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    fn save_mind_map_layout_json(&mut self, project_id: QString, graph_id: QString, layout_json: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let lj = layout_json.to_string();

        let layout: writer_core::mind_map::layout::MindMapLayout = match serde_json::from_str(&lj) {
            Ok(l) => l,
            Err(e) => return serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("Invalid layout JSON: {}", e) }).to_string().into(),
        };

        if let Some(core) = self.core_facade() {
            match core.save_mind_map_layout(&pid, &gid, layout) {
                Ok(_) => serde_json::json!({ "success": true, "data": true, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    fn get_tree_model_json(&self) -> QString {
        let items = self.build_tree_model_json();
        let count = match &items {
            serde_json::Value::Array(arr) => arr.len(),
            _ => 0,
        };
        let val = serde_json::json!({
            "success": true,
            "treeCount": count,
            "items": items
        });
        val.to_string().into()
    }

    fn refresh_tree_model_json(&mut self) -> QString {
        self.reload_tree();
        self.get_tree_model_json()
    }

    
    fn refresh_app_state_json(&mut self) -> QString {
        self.reload_tree();
        let tree_json = self.build_tree_model_json();
        let state = serde_json::json!({
            "hasWorkspace": self.current_has_workspace,
            "workspacePath": self.current_workspace,
            "saveStatus": self.current_save_status,
            "selected": {
                "projectId": self.selected_project_id.clone().unwrap_or_default(),
                "volumeId": self.selected_volume_id.clone().unwrap_or_default(),
                "chapterId": self.selected_chapter_id.clone().unwrap_or_default()
            },
            "tree": tree_json,
            "settings": {
                "fontSize": self.current_setting_font_size,
                "themeMode": self.setting_theme_mode().to_string()
            },
            "sync": {
                "status": self.current_sync_status
            }
        });
        state.to_string().into()
    }

    fn create_project_json(&mut self, title: QString, action_id: QString) -> QString {
        let title_str = title.to_string();
        let build_err = |msg: &str| -> String {
            serde_json::json!({
                "success": false,
                "errorCode": "PROJECT_CREATION_FAILED",
                "userMessage": msg,
                "rawError": msg,
                "changedEntities": []
            }).to_string()
        };

        if title_str.trim().is_empty() {
            let msg = "作品名不能为空";
            self.set_error(msg);
            return build_err(msg).into();
        }

        if !self.current_has_workspace || self.current_workspace.is_empty() {
            let msg = "未打开工作区，无法创建作品。请先新建或打开一个工作区。";
            self.set_error(msg);
            return build_err(msg).into();
        }

        if let Some(api) = self.core_api() {
            match api.create_project(&title_str) {
                Ok(proj) => {
                    self.selected_project_id = Some(proj.id.clone());
                    self.selected_item_changed();
                    self.selected_volume_id = None;
                    self.selected_chapter_id = None;

                    let default_volume_id = {
                        if let Ok(volumes) = api.list_volumes(&proj.id) {
                            volumes.first().map(|v| v.id.clone())
                        } else {
                            None
                        }
                    };
                    if let Some(ref vol_id) = default_volume_id {
                        self.selected_volume_id = Some(vol_id.clone());
                    }

                    self.reload_tree();
                    self.trigger_projects_reloaded();

                    let state_val: serde_json::Value = serde_json::from_str(&self.refresh_app_state_json().to_string()).unwrap_or_default();

                    let final_res = serde_json::json!({
                        "success": true,
                        "data": { "project": proj },
                        "message": format!("作品「{}」创建成功", proj.title),
                        "state": state_val,
                        "changedEntities": ["ProjectList", "WorkspaceTree"]
                    });
                    final_res.to_string().into()
                }
                Err(e) => {
                    let err_display = format!("{}", e);
                    let msg = format!("创建作品失败: {}", e);
                    self.set_error(&msg);
                    serde_json::json!({
                        "success": false,
                        "errorCode": "CORE_ERROR",
                        "userMessage": msg,
                        "rawError": err_display,
                        "changedEntities": []
                    }).to_string().into()
                }
            }
        } else {
            let msg = "核心模块未初始化";
            self.set_error(msg);
            build_err(msg).into()
        }
    }

    fn create_volume_json(&mut self, project_id: QString, title: QString, action_id: QString) -> QString {
        let action_id_str = action_id.to_string();
        self.debug_log(
            "volume",
            "create_volume_start",
            &format!("[actionId={}] project_id={}, title={}", action_id_str, project_id.to_string(), title.to_string())
        );
        let err_before = self.current_error_message.clone();
        self.create_new_volume(project_id.clone(), title.clone());
        let success = self.current_error_message == err_before;
        if success {
            self.debug_log("volume", "create_volume_success", &format!("[actionId={}] created successfully", action_id_str));
        } else {
            self.debug_error("volume", "create_volume_failed", &format!("[actionId={}] error: {}", action_id_str, self.current_error_message));
        }
        let final_res = serde_json::json!({
            "success": true,
            "message": "创建卷成功",
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

    fn create_chapter_json(&mut self, project_id: QString, volume_id: QString, title: QString, action_id: QString) -> QString {
        let action_id_str = action_id.to_string();
        self.debug_log(
            "chapter",
            "create_chapter_start",
            &format!("[actionId={}] project_id={}, volume_id={}, title={}", action_id_str, project_id.to_string(), volume_id.to_string(), title.to_string())
        );
        let err_before = self.current_error_message.clone();
        self.create_new_chapter(project_id.clone(), volume_id.clone(), title.clone());
        let success = self.current_error_message == err_before;
        if success {
            self.debug_log("chapter", "create_chapter_success", &format!("[actionId={}] created successfully", action_id_str));
        } else {
            self.debug_error("chapter", "create_chapter_failed", &format!("[actionId={}] error: {}", action_id_str, self.current_error_message));
        }
        let final_res = serde_json::json!({
            "success": true,
            "message": "创建章节成功",
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

    fn select_tree_item_json(&mut self, item_type: QString, project_id: QString, volume_id: QString, chapter_id: QString, action_id: QString) -> QString {
        let t = item_type.to_string();
        let action_id_str = action_id.to_string();
        self.debug_log(
            "tree",
            "select_item_start",
            &format!(
                "[actionId={}] type={}, project_id={}, volume_id={}, chapter_id={}",
                action_id_str,
                t,
                project_id.to_string(),
                volume_id.to_string(),
                chapter_id.to_string()
            )
        );
        if t == "project" {
            self.select_project(project_id);
        } else if t == "volume" {
            self.select_volume(project_id, volume_id);
        } else if t == "chapter" {
            self.select_chapter(project_id, volume_id, chapter_id);
        }
        self.debug_log("tree", "select_item_success", &format!("[actionId={}] selection completed", action_id_str));
        let final_res = serde_json::json!({
            "success": true,
            "message": "选择成功",
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

    fn delete_project_json(&mut self, project_id: QString, action_id: QString) -> QString {
        let action_id_str = action_id.to_string();
        self.debug_log(
            "project",
            "delete_project_start",
            &format!("[actionId={}] project_id={}", action_id_str, project_id.to_string())
        );
        self.error_message = "".into();
        self.delete_project(project_id);
        let success = self.error_message.to_string().is_empty();
        let msg = if success {
            self.debug_log("project", "delete_project_success", &format!("[actionId={}] project deleted successfully", action_id_str));
            "删除成功".to_string()
        } else {
            let err = self.error_message.to_string();
            self.debug_error("project", "delete_project_failed", &format!("[actionId={}] error: {}", action_id_str, err));
            err
        };
        let final_res = serde_json::json!({
            "success": success,
            "message": msg,
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

    fn delete_volume_json(&mut self, project_id: QString, volume_id: QString, action_id: QString) -> QString {
        let action_id_str = action_id.to_string();
        self.debug_log(
            "volume",
            "delete_volume_start",
            &format!("[actionId={}] project_id={}, volume_id={}", action_id_str, project_id.to_string(), volume_id.to_string())
        );
        self.error_message = "".into();
        self.delete_volume(project_id, volume_id);
        let success = self.error_message.to_string().is_empty();
        let msg = if success {
            self.debug_log("volume", "delete_volume_success", &format!("[actionId={}] volume deleted successfully", action_id_str));
            "删除成功".to_string()
        } else {
            let err = self.error_message.to_string();
            self.debug_error("volume", "delete_volume_failed", &format!("[actionId={}] error: {}", action_id_str, err));
            err
        };
        let final_res = serde_json::json!({
            "success": success,
            "message": msg,
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }

    fn delete_chapter_json(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, action_id: QString) -> QString {
        let action_id_str = action_id.to_string();
        self.debug_log(
            "chapter",
            "delete_chapter_start",
            &format!("[actionId={}] project_id={}, volume_id={}, chapter_id={}", action_id_str, project_id.to_string(), volume_id.to_string(), chapter_id.to_string())
        );
        self.error_message = "".into();
        self.delete_chapter(project_id, volume_id, chapter_id);
        let success = self.error_message.to_string().is_empty();
        let msg = if success {
            self.debug_log("chapter", "delete_chapter_success", &format!("[actionId={}] chapter deleted successfully", action_id_str));
            "删除成功".to_string()
        } else {
            let err = self.error_message.to_string();
            self.debug_error("chapter", "delete_chapter_failed", &format!("[actionId={}] error: {}", action_id_str, err));
            err
        };
        let final_res = serde_json::json!({
            "success": success,
            "message": msg,
            "state": serde_json::from_str::<serde_json::Value>(&self.refresh_app_state_json().to_string()).unwrap_or_default()
        });
        final_res.to_string().into()
    }


    fn get_tree_model(&self) -> QJsonArray {
        self.cached_tree.clone()
    }

    fn create_new_volume(&mut self, project_id: QString, title: QString) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.create_volume(&project_id.to_string(), &title.to_string())
            };
            match result {
                Ok(vol) => {
                    self.selected_project_id = Some(project_id.to_string());
                    self.selected_volume_id = Some(vol.id.clone());
                    self.selected_item_changed();
                    self.selected_chapter_id = None;
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("创建分卷失败: {}", e)),
            }
        }
    }

    fn create_new_chapter(&mut self, project_id: QString, volume_id: QString, title: QString) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.create_chapter(
                    &project_id.to_string(),
                    &volume_id.to_string(),
                    &title.to_string(),
                )
            };
            match result {
                Ok(chap) => {
                    self.selected_project_id = Some(project_id.to_string());
                    self.selected_volume_id = Some(volume_id.to_string());
                    self.selected_chapter_id = Some(chap.id.clone());
                    self.selected_item_changed();
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("创建章节失败: {}", e)),
            }
        }
    }

    fn rename_project(&mut self, project_id: QString, new_title: QString) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.rename_project(&project_id.to_string(), &new_title.to_string())
            };
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重命名作品失败: {}", e)),
            }
        }
    }

    fn delete_project(&mut self, project_id: QString) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.delete_project(&project_id.to_string())
            };
            match result {
                Ok(_) => {
                    if self.selected_project_id.as_deref() == Some(&project_id.to_string()) {
                        self.selected_project_id = None;
                        self.selected_volume_id = None;
                        self.clear_editor_state();
                    }

                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("删除作品失败: {}", e)),
            }
        }
    }

    fn reorder_projects(&mut self, ordered_ids_joined: QString) {
        if let Some(core_ref) = &self.core {
            let ordered_ids_str = ordered_ids_joined.to_string();
            let ids: Vec<String> = ordered_ids_str
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            let result = {
                let core = core_ref.borrow();
                core.reorder_projects(&ids)
            };
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重排作品失败: {}", e)),
            }
        }
    }

    fn rename_volume(&mut self, project_id: QString, volume_id: QString, new_title: QString) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.rename_volume(
                    &project_id.to_string(),
                    &volume_id.to_string(),
                    &new_title.to_string(),
                )
            };
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重命名分卷失败: {}", e)),
            }
        }
    }

    fn delete_volume(&mut self, project_id: QString, volume_id: QString) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.delete_volume(&project_id.to_string(), &volume_id.to_string())
            };
            match result {
                Ok(_) => {
                    if self.selected_volume_id.as_deref() == Some(&volume_id.to_string()) {
                        self.selected_volume_id = None;
                        self.clear_editor_state();
                    }

                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("删除分卷失败: {}", e)),
            }
        }
    }

    fn reorder_volumes(&mut self, project_id: QString, ordered_ids_joined: QString) {
        if let Some(core_ref) = &self.core {
            let ordered_ids_str = ordered_ids_joined.to_string();
            let ids: Vec<String> = ordered_ids_str
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            let result = {
                let core = core_ref.borrow();
                core.reorder_volumes(&project_id.to_string(), &ids)
            };
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重排分卷失败: {}", e)),
            }
        }
    }

    fn rename_chapter(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        new_title: QString,
    ) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.rename_chapter(
                    &project_id.to_string(),
                    &volume_id.to_string(),
                    &chapter_id.to_string(),
                    &new_title.to_string(),
                )
            };
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重命名章节失败: {}", e)),
            }
        }
    }

    fn delete_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) {
        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.delete_chapter(
                    &project_id.to_string(),
                    &volume_id.to_string(),
                    &chapter_id.to_string(),
                )
            };
            match result {
                Ok(_) => {
                    if self.selected_chapter_id.as_deref() == Some(&chapter_id.to_string()) {
                        self.clear_editor_state();
                    }

                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("删除章节失败: {}", e)),
            }
        }
    }

    fn reorder_chapters(
        &mut self,
        project_id: QString,
        volume_id: QString,
        ordered_ids_joined: QString,
    ) {
        if let Some(core_ref) = &self.core {
            let ordered_ids_str = ordered_ids_joined.to_string();
            let ids: Vec<String> = ordered_ids_str
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            let result = {
                let core = core_ref.borrow();
                core.reorder_chapters(&project_id.to_string(), &volume_id.to_string(), &ids)
            };
            match result {
                Ok(_) => {
                    self.reload_tree();
                    self.trigger_projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重排章节失败: {}", e)),
            }
        }
    }

    fn select_project(&mut self, project_id: QString) {
        self.selected_project_id = Some(project_id.to_string());
        self.selected_volume_id = None;
        self.selected_chapter_id = None;
        self.selected_item_changed();
        self.chapter_path_changed();
    }

    fn select_volume(&mut self, project_id: QString, volume_id: QString) {
        self.selected_project_id = Some(project_id.to_string());
        self.selected_volume_id = Some(volume_id.to_string());
        self.selected_chapter_id = None;
        self.selected_item_changed();
        self.chapter_path_changed();
    }

    fn select_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) {
        self.selected_project_id = Some(project_id.to_string());
        self.selected_volume_id = Some(volume_id.to_string());
        self.selected_chapter_id = Some(chapter_id.to_string());
        self.selected_item_changed();
        self.chapter_path_changed();
    }

    fn get_chapter_content(
        &self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
    ) -> QString {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        self.debug_log("chapter", "get_chapter_content_start", &format!("project_id={}, volume_id={}, chapter_id={}", p, v, c));
        if let Some(core) = self.core_facade() {
            match core.read_chapter(&p, &v, &c) {
                Ok(content) => {
                    self.debug_log("chapter", "get_chapter_content_success", &format!("len={}", content.content.len()));
                    return content.content.into();
                }
                Err(e) => {
                    self.debug_error("chapter", "get_chapter_content_failed", &format!("error={}", e));
                }
            }
        } else {
            self.debug_error("chapter", "get_chapter_content_failed", "core_not_initialized");
        }
        "".into()
    }

    fn open_chapter_json(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        if let Some(core) = self.core_api() {
            return match core.open_chapter(&p, &v, &c) {
                Ok(content) => serde_json::json!({
                    "success": true,
                    "content": content.content,
                    "title": content.meta.title,
                    "projectId": p,
                    "volumeId": v,
                    "chapterId": c,
                    "meta": content.meta,
                }).to_string().into(),
                Err(e) => serde_json::json!({
                    "success": false,
                    "code": "CORE_ERROR",
                    "error": format!("读取章节失败: {}", e),
                    "message": format!("读取章节失败: {}", e),
                }).to_string().into(),
            };
        }
        serde_json::json!({
            "success": false,
            "code": "INVALID_WORKSPACE",
            "error": "后端未初始化",
            "message": "后端未初始化",
        }).to_string().into()
    }

    fn open_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QJsonObject {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        self.debug_log("chapter", "open_chapter_start", &format!("project_id={}, volume_id={}, chapter_id={}", p, v, c));
        
        if let Some(core) = self.core_api() {
            match writing_bridge::open_chapter(&core, &p, &v, &c) {
                Ok(data) => {
                    self.selected_project_id = Some(p.clone());
                    self.selected_volume_id = Some(v.clone());
                    self.selected_chapter_id = Some(c.clone());
                    self.selected_item_changed();
                    self.chapter_path_changed();
                    
                    self.debug_log("chapter", "open_chapter_success", "len_loaded");
                    
                    let mut obj = serde_to_qjson_object(serde_json::to_value(data).unwrap_or_default());
                    obj.insert("success", serde_value_to_qjson(serde_json::Value::Bool(true)));
                    return obj;
                }
                Err(e) => {
                    self.debug_error("chapter", "open_chapter_failed", &e.to_string());
                    return bridge_error_object(&format!("读取章节失败: {}", e), "CORE_ERROR");
                }
            }
        }
        
        bridge_error_object("后端未初始化", "INVALID_WORKSPACE")
    }

    fn save_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, content: QString) -> QJsonObject {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        let text_str = content.to_string();
        let len = text_str.len();
        self.debug_log("chapter", "save_chapter_start", &format!("len={}", len));
        
        let save_result = if let Some(core) = self.core_api() {
            Some(writing_bridge::save_chapter(&core, &p, &v, &c, &text_str))
        } else {
            None
        };
        
        let result_obj = match save_result {
            Some(Ok(receipt)) => {
                self.debug_log("chapter", "save_chapter_success", "");
                self.current_save_status = "已保存".to_string();
                self.workspace_content_changed();
                self.flush_writing_stats();
                bridge_success_object(serde_json::to_value(receipt).unwrap())
            }
            Some(Err(e)) => {
                self.debug_error("chapter", "save_chapter_failed", &format!("error={}", e));
                if is_empty_overwrite_blocked(&e) {
                    let msg = blocked_empty_overwrite_user_message();
                    self.current_save_status = msg.to_string();
                    self.set_error(msg);
                } else {
                    self.current_save_status = "保存失败".to_string();
                }
                bridge_error_object(&format!("保存失败: {}", e), "CORE_ERROR")
            }
            None => {
                self.debug_error("chapter", "save_chapter_failed", "core_not_initialized");
                bridge_error_object("后端未初始化", "INVALID_WORKSPACE")
            }
        };
        
        self.save_status_changed();
        result_obj
    }

    fn clear_chapter_content(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QJsonObject {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        self.debug_log("chapter", "clear_chapter_content_start", &format!("chapter_id={}", c));

        let clear_result = if let Some(core) = self.core_api() {
            Some(writing_bridge::clear_chapter_content(&core, &p, &v, &c))
        } else {
            None
        };

        match clear_result {
            Some(Ok(receipt)) => {
                self.debug_log("chapter", "clear_chapter_content_success", &format!("chapter_id={}", c));
                self.current_save_status = "已清空".to_string();
                self.save_status_changed();
                self.workspace_content_changed();
                bridge_success_object(serde_json::to_value(receipt).unwrap())
            }
            Some(Err(e)) => {
                let err_msg = format!("清空章节失败: {}", e);
                self.debug_error("chapter", "clear_chapter_content_failed", &err_msg);
                self.current_save_status = "清空失败".to_string();
                self.save_status_changed();
                self.set_error(&err_msg);
                bridge_error_object(&err_msg, "CORE_ERROR")
            }
            None => {
                self.debug_error("chapter", "clear_chapter_content_failed", "core_not_initialized");
                self.current_save_status = "清空失败".to_string();
                self.save_status_changed();
                self.set_error("清空章节失败: 后端未初始化");
                bridge_error_object("清空章节失败: 后端未初始化", "INVALID_WORKSPACE")
            }
        }
    }



    fn report_writing_event(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        source: QString,
        inserted_chars: u32,
        deleted_chars: u32,
        pasted_chars: u32,
    ) {
        let pid = project_id.to_string();
        let vid = volume_id.to_string();
        let cid = chapter_id.to_string();
        let src = source.to_string();

        if let Some(core) = self.core_facade() {
            writing_bridge::ensure_stats_session(&core, &mut self.stats_device_id, &mut self.stats_session_id, &mut self.stats_last_event_ms);
            
            if let Err(e) = writing_bridge::report_writing_event(
                &core,
                &pid,
                &vid,
                &cid,
                &src,
                inserted_chars,
                deleted_chars,
                pasted_chars,
                0,
                &self.stats_device_id,
                &self.stats_session_id,
            ) {
                self.debug_error("stats", "report_writing_event_failed", &e.to_string());
            }
            self.flush_writing_stats();
        }
    }

    fn process_writing_event_from_text(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        old_text: QString,
        new_text: QString,
    ) {
        let pid = project_id.to_string();
        let vid = volume_id.to_string();
        let cid = chapter_id.to_string();
        let ot = old_text.to_string();
        let nt = new_text.to_string();

        if let Some(core) = self.core_facade() {
            writing_bridge::ensure_stats_session(&core, &mut self.stats_device_id, &mut self.stats_session_id, &mut self.stats_last_event_ms);
            
            if let Err(e) = writing_bridge::process_writing_event_from_text(
                &core,
                &pid,
                &vid,
                &cid,
                &ot,
                &nt,
                &self.stats_device_id,
                &self.stats_session_id,
            ) {
                self.debug_error("stats", "process_writing_event_from_text_failed", &e.to_string());
            }
            self.flush_writing_stats();
        }
    }

    fn get_writing_stats_summary(&self, start_date: QString, end_date: QString) -> QString {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_facade() {
            match core.get_writing_stats_summary(&sd, &ed) {
                Ok(val) => val.to_string().into(),
                Err(_) => "{}".into(),
            }
        } else {
            "{}".into()
        }
    }

    fn get_writing_stats_summary_object(&self, start_date: QString, end_date: QString) -> QJsonObject {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_facade() {
            match core.get_writing_stats_summary(&sd, &ed) {
                Ok(val) => serde_to_qjson_object(val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }

    fn flush_writing_stats(&self) {
        if let Some(core) = self.core_facade() {
            let _ = core.flush_writing_stats();
        }
    }

    fn flush_recent_edits(&self) {
        if let Some(core) = self.core_facade() {
            let _ = core.flush_recent_edits();
        }
    }

    fn get_writing_stats_by_project(&self, start_date: QString, end_date: QString) -> QString {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_facade() {
            match core.get_writing_stats_by_project(&sd, &ed) {
                Ok(val) => val.to_string().into(),
                Err(_) => "{}".into(),
            }
        } else {
            "{}".into()
        }
    }

    fn get_writing_stats_by_project_object(&self, start_date: QString, end_date: QString) -> QJsonObject {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_facade() {
            match core.get_writing_stats_by_project(&sd, &ed) {
                Ok(val) => serde_to_qjson_object(val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }

    fn get_writing_stats_by_chapter(&self, start_date: QString, end_date: QString) -> QString {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_facade() {
            match core.get_writing_stats_by_chapter(&sd, &ed) {
                Ok(val) => val.to_string().into(),
                Err(_) => "{}".into(),
            }
        } else {
            "{}".into()
        }
    }

    fn get_writing_stats_by_chapter_object(&self, start_date: QString, end_date: QString) -> QJsonObject {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_facade() {
            match core.get_writing_stats_by_chapter(&sd, &ed) {
                Ok(val) => serde_to_qjson_object(val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }

    fn get_writing_stats_by_device(&self, start_date: QString, end_date: QString) -> QString {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_facade() {
            match core.get_writing_stats_by_device(&sd, &ed) {
                Ok(val) => val.to_string().into(),
                Err(_) => "{}".into(),
            }
        } else {
            "{}".into()
        }
    }

    fn get_writing_stats_by_device_object(&self, start_date: QString, end_date: QString) -> QJsonObject {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_facade() {
            match core.get_writing_stats_by_device(&sd, &ed) {
                Ok(val) => serde_to_qjson_object(val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }

    fn get_writing_speed_curve(&self, start_date: QString, end_date: QString, bucket_minutes: u32) -> QString {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_facade() {
            match core.get_writing_speed_curve(&sd, &ed, bucket_minutes) {
                Ok(val) => val.to_string().into(),
                Err(_) => "{}".into(),
            }
        } else {
            "{}".into()
        }
    }

    fn get_writing_speed_curve_object(&self, start_date: QString, end_date: QString, bucket_minutes: u32) -> QJsonObject {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_facade() {
            match core.get_writing_speed_curve(&sd, &ed, bucket_minutes) {
                Ok(val) => serde_to_qjson_object(val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }

    // --- StarMap methods ---
    fn list_starmaps_json(&self) -> QString {
        if let Some(core) = self.core_facade() {
            starmap_bridge::list_starmaps(&core).into()
        } else {
            "[]".into()
        }
    }

    fn list_starmaps(&self) -> QJsonArray {
        if let Some(core) = self.core_facade() {
            qjson_array_data_from_json(&starmap_bridge::list_starmaps(&core))
        } else {
            QJsonArray::default()
        }
    }

    fn list_starmaps_for_project_json(&self, project_id: QString) -> QString {
        let pid = project_id.to_string();
        if let Some(core) = self.core_facade() {
            starmap_bridge::list_starmaps_for_project(&core, &pid).into()
        } else {
            "[]".into()
        }
    }

    fn get_starmap_json(&self, starmap_id: QString) -> QString {
        let sid = starmap_id.to_string();
        if let Some(core) = self.core_facade() {
            starmap_bridge::get_starmap(&core, &sid).into()
        } else {
            "{}".into()
        }
    }

    fn create_starmap_json(&mut self, title: QString, description: QString, accent_color: QString) -> QString {
        let t = title.to_string();
        let d = description.to_string();
        let ac = accent_color.to_string();
        let color_ref = if ac.is_empty() { None } else { Some(ac.as_str()) };
        if let Some(core) = self.core_facade() {
            starmap_bridge::create_starmap(&core, &t, &d, color_ref).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

    fn create_starmap(&mut self, title: QString, description: QString, accent_color: QString) -> QJsonObject {
        let raw = self.create_starmap_json(title, description, accent_color).to_string();
        qjson_object_from_json(&raw)
    }

    fn create_child_starmap_json(&mut self, parent_id: QString, title: QString, description: QString, accent_color: QString) -> QString {
        let pid = parent_id.to_string();
        let t = title.to_string();
        let d = description.to_string();
        let ac = accent_color.to_string();
        let color_ref = if ac.is_empty() { None } else { Some(ac.as_str()) };
        if let Some(core) = self.core_facade() {
            starmap_bridge::create_child_starmap(&core, &pid, &t, &d, color_ref).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

    fn rename_starmap_json(&mut self, starmap_id: QString, new_title: QString) -> QString {
        let sid = starmap_id.to_string();
        let t = new_title.to_string();
        if let Some(core) = self.core_facade() {
            starmap_bridge::rename_starmap(&core, &sid, &t).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

    fn delete_starmap_json(&mut self, starmap_id: QString) -> QString {
        let sid = starmap_id.to_string();
        if let Some(core) = self.core_facade() {
            starmap_bridge::delete_starmap(&core, &sid).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

    fn get_starmap_graph_json(&self, starmap_id: QString) -> QString {
        let sid = starmap_id.to_string();
        if let Some(core) = self.core_facade() {
            starmap_bridge::get_starmap_graph_and_layout(&core, &sid).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

    fn get_starmap_graph(&self, starmap_id: QString) -> QJsonObject {
        let raw = self.get_starmap_graph_json(starmap_id).to_string();
        qjson_object_from_json(&raw)
    }

    fn create_starmap_node_json(&mut self, starmap_id: QString, title: QString, kind: QString, x: f64, y: f64) -> QString {
        let sid = starmap_id.to_string();
        let t = title.to_string();
        let k = kind.to_string();
        if let Some(core) = self.core_facade() {
            starmap_bridge::create_starmap_node(&core, &sid, &t, &k, x, y).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

    fn create_starmap_node(&mut self, starmap_id: QString, title: QString, kind: QString, x: f64, y: f64) -> QJsonObject {
        let raw = self.create_starmap_node_json(starmap_id, title, kind, x, y).to_string();
        qjson_object_from_json(&raw)
    }

    fn update_starmap_node_json(&mut self, starmap_id: QString, node_id: QString, patch_json: QString) -> QString {
        let sid = starmap_id.to_string();
        let nid = node_id.to_string();
        let p = patch_json.to_string();
        if let Some(core) = self.core_facade() {
            starmap_bridge::update_starmap_node(&core, &sid, &nid, &p).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

    fn update_starmap_node(&mut self, starmap_id: QString, node_id: QString, patch_json: QString) -> QJsonObject {
        let raw = self.update_starmap_node_json(starmap_id, node_id, patch_json).to_string();
        qjson_object_from_json(&raw)
    }

    fn delete_starmap_node_json(&mut self, starmap_id: QString, node_id: QString) -> QString {
        let sid = starmap_id.to_string();
        let nid = node_id.to_string();
        if let Some(core) = self.core_facade() {
            starmap_bridge::delete_starmap_node(&core, &sid, &nid).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

    fn delete_starmap_node(&mut self, starmap_id: QString, node_id: QString) -> QJsonObject {
        let raw = self.delete_starmap_node_json(starmap_id, node_id).to_string();
        qjson_object_from_json(&raw)
    }

    fn create_starmap_edge_json(&mut self, starmap_id: QString, from_node_id: QString, to_node_id: QString, kind: QString, label: QString) -> QString {
        let sid = starmap_id.to_string();
        let from_id = from_node_id.to_string();
        let to_id = to_node_id.to_string();
        let k = kind.to_string();
        let l = label.to_string();
        if let Some(core) = self.core_facade() {
            starmap_bridge::create_starmap_edge(&core, &sid, &from_id, &to_id, &k, &l).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

    fn create_starmap_edge(&mut self, starmap_id: QString, from_node_id: QString, to_node_id: QString, kind: QString, label: QString) -> QJsonObject {
        let raw = self.create_starmap_edge_json(starmap_id, from_node_id, to_node_id, kind, label).to_string();
        qjson_object_from_json(&raw)
    }

    fn update_starmap_edge_json(&mut self, starmap_id: QString, edge_id: QString, patch_json: QString) -> QString {
        let sid = starmap_id.to_string();
        let eid = edge_id.to_string();
        let p = patch_json.to_string();
        if let Some(core) = self.core_facade() {
            starmap_bridge::update_starmap_edge(&core, &sid, &eid, &p).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

    fn update_starmap_edge(&mut self, starmap_id: QString, edge_id: QString, patch_json: QString) -> QJsonObject {
        let raw = self.update_starmap_edge_json(starmap_id, edge_id, patch_json).to_string();
        qjson_object_from_json(&raw)
    }

    fn delete_starmap_edge_json(&mut self, starmap_id: QString, edge_id: QString) -> QString {
        let sid = starmap_id.to_string();
        let eid = edge_id.to_string();
        if let Some(core) = self.core_facade() {
            starmap_bridge::delete_starmap_edge(&core, &sid, &eid).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

    fn delete_starmap_edge(&mut self, starmap_id: QString, edge_id: QString) -> QJsonObject {
        let raw = self.delete_starmap_edge_json(starmap_id, edge_id).to_string();
        qjson_object_from_json(&raw)
    }

    fn save_starmap_layout_json(&mut self, starmap_id: QString, layout_json: QString) -> QString {
        let sid = starmap_id.to_string();
        let lj = layout_json.to_string();
        if let Some(core) = self.core_facade() {
            starmap_bridge::save_starmap_layout(&core, &sid, &lj).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

    fn save_starmap_layout(&mut self, starmap_id: QString, layout_json: QString) -> QJsonObject {
        let raw = self.save_starmap_layout_json(starmap_id, layout_json).to_string();
        qjson_object_from_json(&raw)
    }

    fn bind_starmap_to_project_json(&mut self, starmap_id: QString, project_id: QString) -> QString {
        let sid = starmap_id.to_string();
        let pid = project_id.to_string();
        if let Some(core) = self.core_facade() {
            starmap_bridge::bind_starmap_to_project(&core, &sid, &pid).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

    fn set_main_starmap_json(&mut self, starmap_id: QString, project_id: QString) -> QString {
        let sid = starmap_id.to_string();
        let pid = project_id.to_string();
        if let Some(core) = self.core_facade() {
            starmap_bridge::set_main_starmap(&core, &sid, &pid).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

    fn get_main_starmap_json(&self, project_id: QString) -> QString {
        let pid = project_id.to_string();
        if let Some(core) = self.core_facade() {
            starmap_bridge::get_main_starmap(&core, &pid).into()
        } else {
            "{}".into()
        }
    }

    fn unbind_starmap_json(&mut self, starmap_id: QString) -> QString {
        let sid = starmap_id.to_string();
        if let Some(core) = self.core_facade() {
            starmap_bridge::unbind_starmap(&core, &sid).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }
}

static QML_LOAD_FAILED: AtomicBool = AtomicBool::new(false);
static QML_HUB_HEADER_MISSING: AtomicBool = AtomicBool::new(false);

extern "C" fn qml_load_error_handler(
    msg_type: QtMsgType,
    _context: &QMessageLogContext,
    msg: &QString,
) {
    let s = format!("{}", msg);
    if matches!(msg_type, QtMsgType::QtWarningMsg | QtMsgType::QtCriticalMsg) {
        eprintln!("[Qt {}] {}", match msg_type {
            QtMsgType::QtWarningMsg => "WARNING",
            QtMsgType::QtCriticalMsg => "CRITICAL",
            _ => "INFO",
        }, s);
        debug_warn_static("app", "qml_warning_critical", &s);
        if s.contains("qrc:/main.qml") {
            QML_LOAD_FAILED.store(true, Ordering::SeqCst);
        }
        if s.contains("qrc:/HubPageHeader.qml") && s.contains("No such file") {
            QML_HUB_HEADER_MISSING.store(true, Ordering::SeqCst);
        }
    } else {
        eprintln!("[Qt DEBUG] {}", s);
        debug_log_static("app", "qml_debug", &s);
    }
}

fn probe_hub_header_resource() {
    QML_HUB_HEADER_MISSING.store(false, Ordering::SeqCst);
    let prev_handler = install_message_handler(Some(qml_load_error_handler));
    let mut probe_engine = QmlEngine::new();
    probe_engine.load_file("qrc:/HubPageHeader.qml".into());
    install_message_handler(prev_handler);

    if QML_HUB_HEADER_MISSING.load(Ordering::SeqCst) {
        debug_error_static("app", "qml_resource_probe", "qrc:/HubPageHeader.qml missing from embedded qrc");
    } else {
        debug_log_static("app", "qml_resource_probe", "qrc:/HubPageHeader.qml exists in embedded qrc");
    }
}

fn main() {
    debug_log_static("app", "app_startup", "Writer application starting...");
    std::env::set_var("QT_QUICK_CONTROLS_STYLE", "Basic");
    qml_resources();
    probe_hub_header_resource();
    qmetaobject::qml_register_type::<AppBackend>(
        CStr::from_bytes_with_nul(b"WriterApp\0").unwrap(),
        1,
        0,
        CStr::from_bytes_with_nul(b"AppBackend\0").unwrap(),
    );
    qmetaobject::qml_register_type::<document_handler::DocumentHandler>(
        CStr::from_bytes_with_nul(b"Writer\0").unwrap(),
        1,
        0,
        CStr::from_bytes_with_nul(b"DocumentHandler\0").unwrap(),
    );

    let qml_path = "qrc:/main.qml";
    debug_log_static("app", "qml_loading", &format!("Loading QML entry: {}", qml_path));

    let prev_handler = install_message_handler(Some(qml_load_error_handler));
    let mut engine = QmlEngine::new();
    engine.load_file(qml_path.into());
    install_message_handler(prev_handler);

    if QML_LOAD_FAILED.load(Ordering::SeqCst) {
        debug_error_static("app", "qml_load_failed", &format!("QQmlApplicationEngine failed to load {}", qml_path));
        std::process::exit(1);
    }

    debug_log_static("app", "event_loop_enter", "QML engine started, entering event loop");
    engine.exec();
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    #[test]
    fn test_create_project_success() {
        let dir = tempdir().unwrap();
        let ws_path = dir.path().to_str().unwrap().to_string();

        let mut backend = AppBackend::default();
        backend.current_workspace = ws_path.clone();
        backend.current_has_workspace = true;

        // Create workspace structure
        fs::write(dir.path().join("workspace_manifest.json"), "{}").unwrap();
        fs::create_dir_all(dir.path().join("projects")).unwrap();

        // Need to initialize core explicitly since we fake the workspace
        let core = WriterCore::new(&ws_path);
        backend.core = Some(Rc::new(RefCell::new(core)));

        // Create 3 projects
        for i in 1..=3 {
            let res_json = backend.create_project_json(format!("Test Project {}", i).into(), "".into());
            let res: serde_json::Value = serde_json::from_str(&res_json.to_string()).unwrap();
            assert_eq!(res["success"], true);
        }

        // Check if tree size increased
        let tree_len_after = backend.cached_tree.len();
        assert!(tree_len_after >= 3);
    }

    #[test]
    fn test_create_project_failure() {
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
        let res: serde_json::Value = serde_json::from_str(&res_json.to_string()).unwrap();

        assert_eq!(res["success"], false);
        // Ensure tree didn't wipe or change unexpectedly
        assert_eq!(backend.cached_tree.len(), 1);
    }

    #[test]
    fn test_create_project_empty_title() {
        let mut backend = AppBackend::default();
        backend.current_workspace = "/tmp".to_string();
        backend.current_has_workspace = true;

        let res_json = backend.create_project_json("   ".into(), "".into());
        let res: serde_json::Value = serde_json::from_str(&res_json.to_string()).unwrap();

        assert_eq!(res["success"], false);
        assert_eq!(res["errorCode"], "PROJECT_CREATION_FAILED");
        assert!(res["userMessage"].as_str().unwrap().contains("不能为空"));
    }

    #[test]
    fn test_handle_sync_outcome_success_pending_path() {
        let mut backend = AppBackend::default();
        backend.current_pending_github_init_path = "/tmp/test_workspace".to_string();
        backend.current_has_workspace = false;

        let outcome = SyncTaskOutcome {
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
        assert!(backend.current_sync_action_result.contains("未配置远程仓库 URL"));
    }
}
