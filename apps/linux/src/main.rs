use qmetaobject::log::{install_message_handler, QMessageLogContext, QtMsgType};
use qmetaobject::prelude::*;
use qmetaobject::{QJsonArray, QJsonObject, QJsonValue, QString};
use rfd::FileDialog;
use std::cell::RefCell;
use std::ffi::CStr;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use writer_core::facade::WriterCore;
use writer_core::sync_service::{SyncConfig, SyncSecrets};

qmetaobject::qrc!(qml_resources, "/" {
    // Pages
    "qml/main.qml" as "main.qml",
    "qml/SettingsDialog.qml" as "SettingsDialog.qml",
    "qml/EditorPage.qml" as "EditorPage.qml",
    "qml/ActionRegistryPage.qml" as "ActionRegistryPage.qml",
    "qml/SyncPage.qml" as "SyncPage.qml",
    "qml/EmptyWorkspace.qml" as "EmptyWorkspace.qml",
    // Components
    "qml/AppButton.qml" as "AppButton.qml",
    "qml/AppCard.qml" as "AppCard.qml",
    "qml/AppTextField.qml" as "AppTextField.qml",
    "qml/AppSwitch.qml" as "AppSwitch.qml",
    "qml/SectionHeader.qml" as "SectionHeader.qml",
    "qml/SettingsRow.qml" as "SettingsRow.qml",
    "qml/SidebarItem.qml" as "SidebarItem.qml",
    "qml/StatusPill.qml" as "StatusPill.qml",
    "qml/ToolbarButton.qml" as "ToolbarButton.qml",
});

struct SyncTaskOutcome {
    sync_status: String,
    action_result: String,
}

fn mask_sync_error(msg: &str) -> String {
    writer_core::sync_service::redact_secrets_from_message(msg, None, None)
}

fn sync_error_category(msg: &str) -> String {
    let lower = msg.to_lowercase();
    // Token missing / not provided
    if lower.contains("token") && (lower.contains("missing") || lower.contains("empty") || lower.contains("not provided")) {
        return "configured_untested".to_string();
    }
    // Repository not found or no permission
    if lower.contains("repository not found") || (lower.contains("not found") && lower.contains("repo")) || lower.contains("404") ||
       lower.contains("permission denied") || lower.contains("403") {
        return "auth_failed".to_string();
    }
    // Branch not found
    if lower.contains("ref not found") || lower.contains("couldn't find remote ref") ||
       lower.contains("remote branch not found") ||
       (lower.contains("branch") && lower.contains("not found")) {
        return "branch_missing".to_string();
    }
    // non-fast-forward
    if lower.contains("non-fast-forward") || lower.contains("non fast forward") || lower.contains("nonfastforward") ||
       (lower.contains("fetch first") && lower.contains("push")) {
        return "non_fast_forward".to_string();
    }
    // Checkout conflict / local blocking file
    if lower.contains("checkout_conflict") || lower.contains("local_blocking_file") {
        return "conflict".to_string();
    }
    // Conflict / merge / unrelated histories
    if lower.contains("conflict") || lower.contains("merge conflict") {
        return "conflict".to_string();
    }
    // Unrelated histories
    if lower.contains("unrelated") {
        return "unrelated_histories".to_string();
    }
    // Authentication errors
    if lower.contains("authentication") || lower.contains("auth failed") || lower.contains("401") ||
       lower.contains("credentials") || lower.contains("could not authenticate") ||
       lower.contains("bad credentials") {
        return "auth_failed".to_string();
    }
    // Network errors
    if lower.contains("resolve") || lower.contains("timeout") || lower.contains("connection refused") ||
       lower.contains("dns") || lower.contains("network") || lower.contains("proxy") ||
       lower.contains("eof") || lower.contains("tls") || lower.contains("ssl") ||
       lower.contains("certificate") || lower.contains("unreachable") ||
       lower.contains("connection reset") || lower.contains("no route to host") {
        return "network_failed".to_string();
    }
    "error".to_string()
}

fn save_sync_configs(path: &str, config: &SyncConfig, secrets: &SyncSecrets) -> Result<(), String> {
    let core = WriterCore::new(path);
    core.save_sync_config(config).map_err(|e| format!("保存同步配置失败: {}", e))?;
    core.save_sync_secrets(secrets).map_err(|e| format!("保存同步凭证失败: {}", e))?;
    Ok(())
}

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
    workspace_state_changed: qt_signal!(),
    projects_reloaded: qt_signal!(),
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

    setting_typing_animation_enabled: qt_property!(bool; READ setting_typing_animation_enabled WRITE set_setting_typing_animation_enabled NOTIFY settings_changed),
    setting_smooth_cursor_enabled: qt_property!(bool; READ setting_smooth_cursor_enabled WRITE set_setting_smooth_cursor_enabled NOTIFY settings_changed),

    settings_changed: qt_signal!(),

    system_color_scheme: qt_property!(QString; READ system_color_scheme NOTIFY system_color_scheme_changed),
    system_color_scheme_changed: qt_signal!(),

    load_local_settings: qt_method!(fn(&mut self)),
    save_local_settings: qt_method!(fn(&mut self) -> bool),
    perform_sync_diagnostics: qt_method!(fn(&mut self)),

    load_sync_config: qt_method!(fn(&mut self)),
    save_sync_config: qt_method!(fn(&mut self) -> bool),
    perform_sync_dry_run: qt_method!(fn(&mut self)),
    perform_sync: qt_method!(fn(&mut self)),

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
    poll_sync_result: qt_method!(fn(&mut self)),
    query_system_color_scheme: qt_method!(fn(&mut self)),

    create_new_project: qt_method!(fn(&mut self, title: QString)),
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
    calculate_word_count: qt_method!(fn(&mut self, text: QString)),

    select_project: qt_method!(fn(&mut self, project_id: QString)),
    select_volume: qt_method!(fn(&mut self, project_id: QString, volume_id: QString)),
    select_chapter:
        qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString)),

    get_chapter_content: qt_method!(
        fn(&self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString
    ),
    save_current_chapter: qt_method!(fn(&mut self, content: QString)),

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

    current_system_color_scheme: String,
    current_pending_github_init_path: String,
    sync_task_rx: Option<Receiver<SyncTaskOutcome>>,

    current_setting_font_size: f32,
    current_setting_line_spacing: f32,
    current_setting_auto_save_enabled: bool,
    current_setting_auto_save_delay_ms: u32,
    current_setting_auto_indent_enabled: bool,
    current_setting_auto_indent_width: f32,
    current_setting_theme_mode: String,
    current_setting_typing_animation_enabled: bool,
    current_setting_smooth_cursor_enabled: bool,
}

impl AppBackend {
    fn has_workspace(&self) -> bool {
        self.current_has_workspace
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
    fn set_setting_font_size(&mut self, val: f32) { self.current_setting_font_size = val; self.settings_changed(); }

    fn setting_line_spacing(&self) -> f32 { self.current_setting_line_spacing }
    fn set_setting_line_spacing(&mut self, val: f32) { self.current_setting_line_spacing = val; self.settings_changed(); }

    fn setting_auto_save_enabled(&self) -> bool { self.current_setting_auto_save_enabled }
    fn set_setting_auto_save_enabled(&mut self, val: bool) { self.current_setting_auto_save_enabled = val; self.settings_changed(); }

    fn setting_auto_save_delay_ms(&self) -> u32 { self.current_setting_auto_save_delay_ms }
    fn set_setting_auto_save_delay_ms(&mut self, val: u32) { self.current_setting_auto_save_delay_ms = val; self.settings_changed(); }

    fn setting_auto_indent_enabled(&self) -> bool { self.current_setting_auto_indent_enabled }
    fn set_setting_auto_indent_enabled(&mut self, val: bool) { self.current_setting_auto_indent_enabled = val; self.settings_changed(); }

    fn setting_auto_indent_width(&self) -> f32 { self.current_setting_auto_indent_width }
    fn set_setting_auto_indent_width(&mut self, val: f32) { self.current_setting_auto_indent_width = val; self.settings_changed(); }

    fn setting_theme_mode(&self) -> QString {
        if self.current_setting_theme_mode.is_empty() {
            "system".into()
        } else {
            self.current_setting_theme_mode.clone().into()
        }
    }
    fn set_setting_theme_mode(&mut self, val: QString) { self.current_setting_theme_mode = val.to_string(); self.settings_changed(); }

    fn setting_typing_animation_enabled(&self) -> bool { self.current_setting_typing_animation_enabled }
    fn set_setting_typing_animation_enabled(&mut self, val: bool) { self.current_setting_typing_animation_enabled = val; self.settings_changed(); }

    fn setting_smooth_cursor_enabled(&self) -> bool { self.current_setting_smooth_cursor_enabled }
    fn set_setting_smooth_cursor_enabled(&mut self, val: bool) { self.current_setting_smooth_cursor_enabled = val; self.settings_changed(); }

    fn try_restore_last_workspace(&mut self) {
        if let Some(path) = writer_core::app_config::get_last_workspace_path() {
            let path_obj = std::path::Path::new(&path);
            if path_obj.exists() && path_obj.is_dir() {
                let core = WriterCore::new(&path);
                if core.validate_workspace().unwrap_or(false) {
                    self.core = Some(Rc::new(RefCell::new(core)));
                    self.current_workspace = path.clone();
                    self.current_has_workspace = true;
                    self.current_save_status = "已保存".to_string();
                    self.save_status_changed();
                    self.reload_tree();
                    self.load_sync_config();
                    self.load_local_settings();
                    self.workspace_opened();
                    self.workspace_state_changed();
                    return;
                }
            }
            // Restore failed: clear the invalid lastWorkspacePath to avoid being stuck
            let _ = writer_core::app_config::clear_last_workspace_path();
        }
        // No valid workspace to restore
        self.current_has_workspace = false;
        self.current_sync_status = "not_configured".to_string();
        self.sync_status_changed();
        self.workspace_state_changed();
        // Load app-level theme mode even without workspace
        self.load_app_theme_mode();
    }

    fn load_app_theme_mode(&mut self) {
        // Load theme mode from app_config (when no workspace is open)
        // Default to "system"
        self.current_setting_theme_mode = "system".to_string();
        self.settings_changed();
    }

    fn internal_open_workspace(&mut self, path: &str, initialize: bool) {
        let path_obj = std::path::Path::new(path);
        if !path_obj.exists() || !path_obj.is_dir() {
            self.set_error(&format!("路径不存在或不是目录: {}", path));
            return;
        }

        let core = WriterCore::new(path);
        let is_valid = core.validate_workspace().unwrap_or(false);

        if !is_valid && !initialize {
            self.set_error("不是有效工作区。请选择其他目录，或使用「新建工作区」初始化该目录。");
            return;
        }

        if !is_valid && initialize {
            if let Err(e) = core.create_workspace() {
                self.set_error(&format!("无法创建工作区: {}", e));
                return;
            }
        }

        let core = WriterCore::new(path);
        if !core.validate_workspace().unwrap_or(false) {
            self.set_error("工作区验证失败");
            return;
        }

        // Ensure necessary directories exist even for valid workspaces
        let _ = std::fs::create_dir_all(path_obj.join("projects"));
        let _ = std::fs::create_dir_all(path_obj.join("app-meta/settings"));
        let _ = std::fs::create_dir_all(path_obj.join("app-meta/sync"));

        self.core = Some(Rc::new(RefCell::new(core)));
        self.current_workspace = path.to_string();
        self.current_has_workspace = true;
        self.current_save_status = "已保存".to_string();
        self.save_status_changed();
        self.reload_tree();
        self.load_sync_config();
        self.load_local_settings();
        self.workspace_opened();
        self.workspace_state_changed();

        let _ = writer_core::app_config::set_last_workspace_path(path);
    }

    fn create_new_workspace(&mut self) {
        if let Some(path) = FileDialog::new().pick_folder() {
            self.internal_open_workspace(&path.to_string_lossy(), true);
        }
    }

    fn open_existing_workspace(&mut self) {
        if let Some(path) = FileDialog::new().pick_folder() {
            self.internal_open_workspace(&path.to_string_lossy(), false);
        }
    }

    fn close_workspace(&mut self) {
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
        self.projects_reloaded();
        self.sync_status_changed();
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

        let (tx, rx) = mpsc::channel();
        self.sync_task_rx = Some(rx);

        thread::spawn(move || {
            let result = Self::do_github_init(
                &path_str, &remote_url_str, &branch_str, &token_str,
                &proxy_type_str, &proxy_host_str, proxy_port_val,
            );
            tx.send(result).ok();
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
            let core = WriterCore::new(path);
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
            backend_type: BackendType::Git,
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
                        let core = WriterCore::new(path);
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
                        SyncTaskOutcome {
                            sync_status: "conflict".to_string(),
                            action_result: "同步冲突，需要手动处理".to_string(),
                        }
                    } else {
                        let err = result.error.unwrap_or_default();
                        SyncTaskOutcome {
                            sync_status: sync_error_category(&err),
                            action_result: format!("同步失败: {}", mask_sync_error(&err)),
                        }
                    }
                }
                Err(e) => SyncTaskOutcome {
                    sync_status: sync_error_category(&e.to_string()),
                    action_result: format!("同步失败: {}", mask_sync_error(&e.to_string())),
                },
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
        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();

            if let Ok(settings) = core.load_local_settings() {
                self.current_setting_line_spacing = settings.editor_line_spacing_multiplier;
                self.current_setting_auto_save_enabled = settings.auto_save_enabled;
                self.current_setting_auto_save_delay_ms = settings.auto_save_delay_ms as u32;
                self.current_setting_auto_indent_enabled = settings.auto_indent_enabled;
                self.current_setting_auto_indent_width = settings.auto_indent_width;
                self.current_setting_typing_animation_enabled = settings.editor_typing_animation_enabled;
                self.current_setting_smooth_cursor_enabled = settings.editor_smooth_cursor_enabled;
            }

            if let Ok(sync_settings) = core.load_syncable_settings() {
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
            } else {
                if let Ok(local) = core.load_local_settings() {
                    self.current_setting_font_size = local.editor_font_size;
                }
                if self.current_setting_font_size <= 0.0 {
                    self.current_setting_font_size = 16.0;
                }
                self.current_setting_theme_mode = "system".to_string();
            }

            self.settings_changed();
        }
    }

    fn save_local_settings(&mut self) -> bool {
        let mut error_msg: Option<String> = None;
        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();

            let mut local = core.load_local_settings().unwrap_or_default();
            local.editor_line_spacing_multiplier = self.current_setting_line_spacing;
            local.auto_save_enabled = self.current_setting_auto_save_enabled;
            local.auto_save_delay_ms = self.current_setting_auto_save_delay_ms as u64;
            local.auto_indent_enabled = self.current_setting_auto_indent_enabled;
            local.auto_indent_width = self.current_setting_auto_indent_width;
            local.editor_typing_animation_enabled = self.current_setting_typing_animation_enabled;
            local.editor_smooth_cursor_enabled = self.current_setting_smooth_cursor_enabled;

            if let Err(e) = core.save_local_settings(&local) {
                error_msg = Some(format!("保存本地设置失败: {}", e));
            }

            let mut syncable = core.load_syncable_settings().unwrap_or_default();
            syncable.font_size = self.current_setting_font_size as f64;
            syncable.theme_mode = self.current_setting_theme_mode.clone();

            if let Err(e) = core.save_syncable_settings(&syncable) {
                error_msg = Some(format!("保存同步设置失败: {}", e));
            }
        }

        if let Some(msg) = error_msg {
            self.set_error(&msg);
            false
        } else {
            true
        }
    }

    fn perform_sync_diagnostics(&mut self) {
        let workspace_path = self.current_workspace.clone();
        if workspace_path.is_empty() {
            self.current_sync_action_result = "请先打开工作区".to_string();
            self.sync_action_completed();
            return;
        }

        self.current_sync_status = "syncing".to_string();
        self.sync_status_changed();
        self.current_sync_action_result = "正在诊断...".to_string();

        let (tx, rx) = mpsc::channel();
        self.sync_task_rx = Some(rx);

        thread::spawn(move || {
            let core = WriterCore::new(&workspace_path);
            let config = match core.load_sync_config() {
                Ok(c) => c,
                Err(e) => {
                    tx.send(SyncTaskOutcome {
                        sync_status: "error".to_string(),
                        action_result: format!("无法加载同步配置: {}", e),
                    }).ok();
                    return;
                }
            };

            match core.perform_sync_diagnostics(&config) {
                Ok(result) => {
                    let status = if !result.success {
                        match result.error_category.as_str() {
                            "token_missing" => "configured_untested",
                            "empty_url" => "not_configured",
                            cat if cat.contains("auth") || cat == "token_missing" => "configured_untested",
                            cat if cat.contains("network") || cat.contains("proxy") || cat.contains("connect") => "network_failed",
                            "repo_not_found_or_no_permission" => "auth_failed",
                            _ => "error",
                        }
                    } else {
                        "configured_untested"
                    };

                    let mut msg = format!("诊断结果: {}", if result.success { "成功" } else { "失败" });

                    msg.push_str(&format!("\n后端类型: {}", result.backend_type));
                    msg.push_str(&format!("\n应用内代理: {}", result.app_proxy_status));

                    if !result.remote_url_sanitized.is_empty() {
                        msg.push_str(&format!("\nRemote URL: {}", result.remote_url_sanitized));
                    }
                    msg.push_str(&format!("\nTransport: {}", result.transport));

                    if result.proxy_used && result.proxy_type != "none" {
                        if result.proxy_type == "auto" {
                            msg.push_str("\n代理配置: auto (注意：auto 代表 git config 自动代理，不是 Clash 自动代理，不是 TUN，不是 Android VPN，不是系统代理)");
                        } else {
                            let protocol = if result.proxy_type == "socks5" { "socks5h" } else { "http" };
                            msg.push_str(&format!("\n代理配置: {}://{}:{}", protocol, result.proxy_host, result.proxy_port));
                            if protocol == "http" || protocol == "socks5h" {
                                msg.push_str(&format!("\n  TCP 连通: {} ({})", if result.tcp_probe_ok { "成功" } else { "失败" }, result.tcp_probe_status));
                                if protocol == "http" {
                                    msg.push_str(&format!("\n  HTTP CONNECT: {} ({})", if result.http_connect_probe_ok { "成功" } else { "失败" }, result.http_connect_probe_status));
                                }
                            }
                        }
                        msg.push_str(&format!("\n  libgit2 访问: {} ({})\n", if result.libgit2_probe_ok { "成功" } else { "失败" }, result.libgit2_probe_status));
                    }

                    msg.push_str(&format!("\n网络连接: {}", if result.network_ok { "正常" } else { "异常" }));
                    msg.push_str(&format!("\n身份认证: {}", if result.auth_ok { "正常" } else { "异常" }));
                    msg.push_str(&format!("\n仓库访问: {}", if result.repo_ok { "正常" } else { "异常" }));
                    msg.push_str(&format!("\n分支存在: {}", if result.branch_ok { "正常" } else { "异常" }));

                    if !result.error_category.is_empty() && result.error_category != "none" {
                        msg.push_str(&format!("\n错误分类: {}", result.error_category));
                    }

                    if !result.user_message.is_empty() {
                        msg.push_str(&format!("\n\n说明:\n{}", result.user_message));
                    }
                    if let Some(err) = result.raw_error {
                        msg.push_str(&format!("\n\n错误详情:\n{}", mask_sync_error(&err)));
                    }

                    tx.send(SyncTaskOutcome {
                        sync_status: status.to_string(),
                        action_result: msg,
                    }).ok();
                }
                Err(e) => {
                    tx.send(SyncTaskOutcome {
                        sync_status: sync_error_category(&e.to_string()),
                        action_result: format!("诊断过程发生错误:\n{}", mask_sync_error(&e.to_string())),
                    }).ok();
                }
            }
        });
    }



    fn load_sync_config(&mut self) {
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
        } else {
            // No workspace open - ensure branch defaults to main
            self.current_sync_branch = "main".to_string();
        }
    }

    fn save_sync_config(&mut self) -> bool {
        let mut error_msg: Option<String> = None;
        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();

            let mut c = core
                .load_sync_config()
                .unwrap_or(writer_core::sync_service::SyncConfig {
                    enabled: false,
                    backend_type: writer_core::sync_service::BackendType::Git,
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
                _ => writer_core::sync_service::BackendType::Git,
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
        }

        if let Some(msg) = error_msg {
            self.set_error(&msg);
            self.current_sync_action_result = msg;
            self.sync_action_completed();
            return false;
        }

        self.refresh_sync_status_from_config();
        self.current_sync_action_result = "配置保存成功".to_string();
        self.sync_action_completed();
        true
    }

    fn perform_sync_dry_run(&mut self) {
        let workspace_path = self.current_workspace.clone();
        if workspace_path.is_empty() {
            self.current_sync_action_result = "请先打开工作区".to_string();
            self.sync_action_completed();
            return;
        }

        self.current_sync_status = "syncing".to_string();
        self.sync_status_changed();
        self.current_sync_action_result = "正在检查同步计划...".to_string();

        let (tx, rx) = mpsc::channel();
        self.sync_task_rx = Some(rx);

        thread::spawn(move || {
            let core = WriterCore::new(&workspace_path);
            let config = match core.load_sync_config() {
                Ok(c) => c,
                Err(e) => {
                    tx.send(SyncTaskOutcome {
                        sync_status: "configured_untested".to_string(),
                        action_result: format!("无法加载同步配置: {}", e),
                    }).ok();
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
                    tx.send(SyncTaskOutcome {
                        sync_status: "configured_untested".to_string(),
                        action_result: msg,
                    }).ok();
                }
                Err(e) => {
                    tx.send(SyncTaskOutcome {
                        sync_status: "configured_untested".to_string(),
                        action_result: format!("检查同步计划失败: {}", mask_sync_error(&e.to_string())),
                    }).ok();
                }
            }
        });
    }

    fn perform_sync(&mut self) {
        let workspace_path = self.current_workspace.clone();
        if workspace_path.is_empty() {
            self.current_sync_action_result = "请先打开工作区".to_string();
            self.sync_action_completed();
            return;
        }

        self.current_sync_status = "syncing".to_string();
        self.sync_status_changed();
        self.current_sync_action_result = "正在同步...".to_string();

        let (tx, rx) = mpsc::channel();
        self.sync_task_rx = Some(rx);

        thread::spawn(move || {
            let core = WriterCore::new(&workspace_path);
            let config = match core.load_sync_config() {
                Ok(c) => c,
                Err(e) => {
                    tx.send(SyncTaskOutcome {
                        sync_status: "error".to_string(),
                        action_result: format!("无法读取同步配置: {}", e),
                    }).ok();
                    return;
                }
            };

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
                        writer_core::sync_service::SyncStatus::Conflict => {
                            let m = format!(
                                "同步冲突\n冲突文件: {}",
                                result.conflicts.iter().map(|c| c.local_path.clone()).collect::<Vec<_>>().join(", ")
                            );
                            ("conflict".to_string(), m)
                        }
                        writer_core::sync_service::SyncStatus::Error(ref e) => {
                            let cat = sync_error_category(e);
                            let m = format!("同步失败:\n{}", mask_sync_error(e));
                            (cat, m)
                        }
                        writer_core::sync_service::SyncStatus::Idle => {
                            ("configured_untested".to_string(), "同步未执行".to_string())
                        }
                        _ => {
                            ("error".to_string(), format!("同步状态: {:?}", result.status))
                        }
                    };

                    tx.send(SyncTaskOutcome {
                        sync_status: status.to_string(),
                        action_result: msg,
                    }).ok();
                }
                Err(e) => {
                    tx.send(SyncTaskOutcome {
                        sync_status: sync_error_category(&e.to_string()),
                        action_result: format!("同步操作失败:\n{}", mask_sync_error(&e.to_string())),
                    }).ok();
                }
            }
        });
    }

    fn poll_sync_result(&mut self) {
        if let Some(rx) = &self.sync_task_rx {
            if let Ok(outcome) = rx.try_recv() {
                self.current_sync_status = outcome.sync_status;
                self.current_sync_action_result = outcome.action_result;
                self.sync_status_changed();
                self.sync_action_completed();
                self.sync_task_rx = None;

                if self.current_sync_status == "success" && self.has_workspace() {
                    self.reload_tree();
                    self.projects_reloaded();
                }

                if self.current_sync_status == "success" && !self.current_pending_github_init_path.is_empty() {
                    // GitHub init successful - open the workspace
                    let path = self.current_pending_github_init_path.clone();
                    self.current_pending_github_init_path.clear();
                    self.pending_github_init_path_changed();
                    self.internal_open_workspace(&path, false);
                    // Load sync config so bridge properties reflect saved config
                    self.load_sync_config();
                }

                // On unrelated_histories or conflict, reload tree to reflect current state
                if (self.current_sync_status == "unrelated_histories" || self.current_sync_status == "conflict") && self.has_workspace() {
                    self.reload_tree();
                    self.projects_reloaded();
                }
            }
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
        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();
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
        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();
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
        let count = text_str.chars().filter(|c| !c.is_whitespace()).count() as i32;
        self.set_word_count(count);
    }

    fn reload_tree(&mut self) {
        let mut list = QJsonArray::default();
        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();
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
    }

    fn get_tree_model(&self) -> QJsonArray {
        self.cached_tree.clone()
    }

    fn create_new_project(&mut self, title: QString) {
        if !self.current_has_workspace {
            self.set_error("未打开工作区，无法创建作品。请先新建或打开一个工作区。");
            return;
        }
        if self.current_workspace.is_empty() {
            self.set_error("工作区路径为空，无法创建作品。");
            return;
        }
        let ws_path = std::path::Path::new(&self.current_workspace);
        if !ws_path.exists() || !ws_path.is_dir() {
            self.set_error(&format!("工作区目录不存在: {}", self.current_workspace));
            return;
        }
        let core_check = WriterCore::new(&self.current_workspace);
        if !core_check.validate_workspace().unwrap_or(false) {
            let projects_dir = ws_path.join("projects");
            let projects_exists = projects_dir.exists();
            self.set_error(&format!(
                "创建作品失败: 工作区验证失败\n工作区: {}\nprojects 目录存在: {}\n请检查工作区结构是否正确。",
                self.current_workspace, projects_exists
            ));
            return;
        }

        if self.core.is_none() {
            // Re-initialize core if it was dropped
            let new_core = WriterCore::new(&self.current_workspace);
            if !new_core.validate_workspace().unwrap_or(false) {
                self.set_error(&format!("无法重新打开工作区: {}", self.current_workspace));
                return;
            }
            self.core = Some(Rc::new(RefCell::new(new_core)));
        }

        if let Some(core_ref) = &self.core {
            let result = {
                let core = core_ref.borrow();
                core.create_project(&title.to_string())
            };
            match result {
                Ok(proj) => {
                    self.selected_project_id = Some(proj.id.clone());
                    self.selected_item_changed();
                    self.selected_volume_id = None;
                    self.selected_chapter_id = None;
                    self.reload_tree();
                    self.projects_reloaded();
                }
                Err(e) => {
                    let ws_path = std::path::Path::new(&self.current_workspace);
                    let projects_dir = ws_path.join("projects");
                    let projects_exists = projects_dir.exists();
                    let validate_result = core_check.validate_workspace();
                    let msg = format!(
                        "创建作品失败: {}\n工作区: {}\n工作区验证: {:?}\nprojects 目录存在: {}",
                        e, self.current_workspace, validate_result, projects_exists
                    );
                    self.set_error(&msg);
                }
            }
        }
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
                    self.projects_reloaded();
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
                    self.projects_reloaded();
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
                    self.projects_reloaded();
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
                    self.projects_reloaded();
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
                    self.projects_reloaded();
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
                    self.projects_reloaded();
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
                    self.projects_reloaded();
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
                    self.projects_reloaded();
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
                    self.projects_reloaded();
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
                    self.projects_reloaded();
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
                    self.projects_reloaded();
                }
                Err(e) => self.set_error(&format!("重排章节失败: {}", e)),
            }
        }
    }

    fn select_project(&mut self, project_id: QString) {
        self.selected_project_id = Some(project_id.to_string());
        self.selected_item_changed();
        self.chapter_path_changed();
    }

    fn select_volume(&mut self, project_id: QString, volume_id: QString) {
        self.selected_project_id = Some(project_id.to_string());
        self.selected_volume_id = Some(volume_id.to_string());
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
        if let Some(core_ref) = &self.core {
            let core = core_ref.borrow();
            if let Ok(content) = core.read_chapter(
                &project_id.to_string(),
                &volume_id.to_string(),
                &chapter_id.to_string(),
            ) {
                return content.content.into();
            }
        }
        "".into()
    }

    fn save_current_chapter(&mut self, content: QString) {
        if !self.selected_chapter_exists() {
            self.clear_editor_state();
            self.set_error("当前章节已不存在，已停止保存。");
            return;
        }

        let save_result = if let (Some(core_ref), Some(p), Some(v), Some(c)) = (
            &self.core,
            &self.selected_project_id,
            &self.selected_volume_id,
            &self.selected_chapter_id,
        ) {
            let core = core_ref.borrow();
            core.write_chapter(p, v, c, &content.to_string())
        } else {
            return;
        };

        match save_result {
            Ok(_) => {
                self.current_save_status = "已保存".to_string();
                self.save_status_changed();
            }
            Err(e) => {
                self.set_error(&format!("保存失败: {}", e));
            }
        }
    }
}

static QML_LOAD_FAILED: AtomicBool = AtomicBool::new(false);

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
        if s.contains("qrc:/main.qml") {
            QML_LOAD_FAILED.store(true, Ordering::SeqCst);
        }
    } else {
        eprintln!("[Qt DEBUG] {}", s);
    }
}

fn main() {
    qml_resources();
    qmetaobject::qml_register_type::<AppBackend>(
        CStr::from_bytes_with_nul(b"WriterApp\0").unwrap(),
        1,
        0,
        CStr::from_bytes_with_nul(b"AppBackend\0").unwrap(),
    );

    let qml_path = "qrc:/main.qml";
    eprintln!("[Linux] Loading QML entry: {}", qml_path);

    let prev_handler = install_message_handler(Some(qml_load_error_handler));
    let mut engine = QmlEngine::new();
    engine.load_file(qml_path.into());
    install_message_handler(prev_handler);

    if QML_LOAD_FAILED.load(Ordering::SeqCst) {
        eprintln!("[Linux] ERROR: QQmlApplicationEngine failed to load {}", qml_path);
        std::process::exit(1);
    }

    eprintln!("[Linux] QML engine started, entering event loop");
    engine.exec();
}
