use qmetaobject::prelude::*;

use cpp::cpp;
use qmetaobject::{QJsonArray, QJsonObject, QJsonValue, QString};
use rfd::FileDialog;
use std::io::Write;
use std::thread;
use std::sync::OnceLock;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use writer_core::api::WriterCoreApi;

use super::json_utils::{bridge_error_object, bridge_success_object, qjson_array_data_from_json, qjson_object_from_json, serde_to_qjson_object, serde_value_to_qjson};
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
pub(crate) fn debug_log_static(module: &str, event: &str, message: &str) {
    if debug_level_enabled(module, DebugLevel::Info) {
        println!("[WriterDebug][static][module={}][event={}] {}", module, event, message);
    }
}

#[allow(dead_code)]
pub(crate) fn debug_warn_static(module: &str, event: &str, message: &str) {
    if debug_level_enabled(module, DebugLevel::Warn) {
        eprintln!("[WriterDebug][WARN][static][module={}][event={}] {}", module, event, message);
    }
}

#[allow(dead_code)]
pub(crate) fn debug_error_static(module: &str, event: &str, message: &str) {
    if debug_level_enabled(module, DebugLevel::Error) {
        eprintln!("[WriterDebug][ERROR][static][module={}][event={}] {}", module, event, message);
    }
}

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
    debug_module_enabled_qml: qt_method!(fn(&self, module: QString) -> bool),
    log_qml: qt_method!(fn(&self, level: QString, module: QString, event: QString, message: QString)),


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





















































        fn workspace_path(&self) -> QString {
        self.current_workspace.clone().into()
    }



































    













































    // --- StarMap methods ---




























    // Deprecated compatibility forwarding surface is split by domain.

}

#[path = "workspace_backend.rs"]
pub mod workspace_backend;
#[path = "project_backend.rs"]
pub mod project_backend;
#[path = "editor_backend.rs"]
pub mod editor_backend;
#[path = "settings_backend.rs"]
pub mod settings_backend;
#[path = "sync_backend.rs"]
pub mod sync_backend;
#[path = "starmap_backend.rs"]
pub mod starmap_backend;

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
    fn test_create_project_success() {
        let dir = tempdir().unwrap();
        let ws_path = dir.path().to_str().unwrap().to_string();

        let mut backend = AppBackend::default();
        backend.current_workspace = ws_path.clone();
        backend.current_has_workspace = true;

        WriterCoreApi::new(&ws_path)
            .create_workspace_if_needed()
            .unwrap();

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
