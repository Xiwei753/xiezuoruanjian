//! Windows Qt 客户端启动入口。
//!
//! 复用 Linux Qt 的 QML 资源清单、QObject 后端和星图渲染实现；Windows 只负责
//! 自己的进程生命周期、平台服务、QPA/IME 环境和 RHI 诊断。

use qmetaobject::log::{install_message_handler, QMessageLogContext, QtMsgType};
use qmetaobject::prelude::*;
use qmetaobject::QString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::app_main_cpp;
use crate::backend;
use crate::native::starmap_circle_clip::StarMapCircleClip;
use crate::native::starmap_edge_layer::StarMapEdgeLayer;
use crate::native::starmap_grid_item::StarMapGridItem;
use crate::native::starmap_object_layer::StarMapObjectLayer;
use crate::platform::linux_qt::runtime_environment;
use crate::sujian_editor_item;

use backend::app_backend::{debug_error_static, debug_log_static, debug_warn_static};
use backend::diagnostics;
use backend::{AppBackend, BackendRuntime};

include!(concat!(env!("OUT_DIR"), "/qrc_resources.rs"));

static QML_LOAD_FAILED: AtomicBool = AtomicBool::new(false);
static QML_LAST_LOAD_ERROR: OnceLock<Mutex<String>> = OnceLock::new();
static STARTUP_DIAG: OnceLock<writer_platform_linux::StartupDiagnostics> = OnceLock::new();
static APP_LIFECYCLE_MARKER: OnceLock<PathBuf> = OnceLock::new();

fn startup_mark(stage: &str, message: &str) {
    if let Some(diag) = STARTUP_DIAG.get() {
        diag.mark(stage, message);
    }
}

fn startup_mark_exit(code: i32) {
    finish_app_lifecycle(code);
    if let Some(diag) = STARTUP_DIAG.get() {
        diag.mark_exit(code);
    }
}

fn process_is_running(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            let text = String::from_utf8_lossy(&output.stdout);
            text.contains(&pid.to_string())
        })
        .unwrap_or(true)
}

fn begin_app_lifecycle(log_dir: &std::path::Path, session_id: &str) {
    let mut previous_run_unclean = false;
    if std::fs::create_dir_all(log_dir).is_ok() {
        if let Ok(entries) = std::fs::read_dir(log_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let Some(pid) = name
                    .strip_prefix("app-running-")
                    .and_then(|name| name.strip_suffix(".marker"))
                    .and_then(|pid| pid.parse::<u32>().ok())
                else {
                    continue;
                };
                if !process_is_running(pid) {
                    previous_run_unclean = true;
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }

    let marker = log_dir.join(format!("app-running-{}.marker", std::process::id()));
    let _ = APP_LIFECYCLE_MARKER.set(marker.clone());
    let _ = std::fs::write(&marker, session_id.as_bytes());

    let fields = std::collections::BTreeMap::from([
        (
            "previous_run_unclean".to_string(),
            serde_json::Value::Bool(previous_run_unclean),
        ),
        (
            "pid".to_string(),
            serde_json::Value::Number(serde_json::Number::from(std::process::id())),
        ),
    ]);
    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        sequence: 0,
        session_id: String::new(),
        level: writer_diagnostics::DiagnosticLevel::Info,
        origin: writer_diagnostics::DiagnosticOrigin::App,
        event: "app.lifecycle.start".to_string(),
        target: "app.lifecycle".to_string(),
        message: None,
        fields,
    });
}

fn finish_app_lifecycle(code: i32) {
    if code == 0 {
        writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            sequence: 0,
            session_id: String::new(),
            level: writer_diagnostics::DiagnosticLevel::Info,
            origin: writer_diagnostics::DiagnosticOrigin::App,
            event: "app.lifecycle.exit".to_string(),
            target: "app.lifecycle".to_string(),
            message: None,
            fields: std::collections::BTreeMap::from([(
                "outcome".to_string(),
                serde_json::Value::String("normal".to_string()),
            )]),
        });
    }
    if writer_diagnostics::flush() {
        if let Some(marker) = APP_LIFECYCLE_MARKER.get() {
            let _ = std::fs::remove_file(marker);
        }
    }
}

fn remember_qml_load_error(message: &str) {
    let lock = QML_LAST_LOAD_ERROR.get_or_init(|| Mutex::new(String::new()));
    if let Ok(mut last) = lock.lock() {
        *last = message.to_string();
    }
}

fn last_qml_load_error() -> String {
    QML_LAST_LOAD_ERROR
        .get_or_init(|| Mutex::new(String::new()))
        .lock()
        .map(|last| last.clone())
        .unwrap_or_default()
}

extern "C" fn qml_load_error_handler(
    msg_type: QtMsgType,
    _context: &QMessageLogContext,
    msg: &QString,
) {
    let text = format!("{}", msg);
    if matches!(msg_type, QtMsgType::QtWarningMsg | QtMsgType::QtCriticalMsg) {
        eprintln!("[Qt WARNING] {text}");
        debug_warn_static("app", "qml_warning_critical", &text);
        record_qt_event(
            writer_diagnostics::DiagnosticLevel::Warn,
            "qml_warning_critical",
            &text,
        );
        startup_mark("qt_warning_critical", &text);
        if text.contains("qrc:/main.qml")
            || text.contains("QQmlApplicationEngine failed")
            || text.contains("failed to load component")
            || text.contains("is not installed")
            || text.contains("import requires")
        {
            remember_qml_load_error(&text);
            QML_LOAD_FAILED.store(true, Ordering::SeqCst);
        }
    } else {
        eprintln!("[Qt DEBUG] {text}");
        debug_log_static("app", "qml_debug", &text);
        record_qt_event(
            writer_diagnostics::DiagnosticLevel::Debug,
            "qml_debug",
            &text,
        );
    }
}

fn record_qt_event(level: writer_diagnostics::DiagnosticLevel, event: &str, message: &str) {
    use std::collections::BTreeMap;
    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        sequence: 0,
        session_id: String::new(),
        level,
        origin: writer_diagnostics::DiagnosticOrigin::System,
        event: event.to_string(),
        target: "qt".to_string(),
        message: Some(message.to_string()),
        fields: BTreeMap::new(),
    });
}

fn fail_if_not_qt6() {
    let version = app_main_cpp::qt_runtime_version();
    if std::env::var("SUJIAN_EDITOR_DEBUG").is_ok()
        || std::env::var("WRITER_DEBUG").is_ok()
        || !version.starts_with("6.")
    {
        eprintln!("[QtDiagnostics] linked Qt runtime version: {version}");
    }
    if !version.starts_with("6.") {
        eprintln!("Windows binary requires Qt6, got {version}");
        startup_mark("qt_runtime_probe_failed", "Qt6 required");
        startup_mark_exit(1);
        std::process::exit(1);
    }
}

fn set_application_icon() {
    app_main_cpp::set_application_icon();
}

fn log_input_method_diagnostics() {
    app_main_cpp::log_input_method_diagnostics();
}

const QRC_RESOURCE_REVISION: &str = concat!(env!("CARGO_PKG_VERSION"), ":windows_qml_resources_v1");

struct DesktopRuntimeProfile {
    runtime_profile: String,
    qt_runtime_version: String,
    qt_build_version: String,
    qml_entry: String,
    qml_import_path: String,
    qt_plugin_path: String,
    qrc_revision: String,
    platform_name: String,
    input_method_module: String,
    bundled_qt: bool,
}

impl DesktopRuntimeProfile {
    fn collect(qt_version: &str, qml_entry: &str) -> Self {
        let pkg_type = diagnostics::effective_package_type();
        let runtime_profile = if cfg!(target_env = "msvc") {
            "windows-msvc"
        } else {
            "windows-gnu"
        };
        let _ = pkg_type;
        Self {
            runtime_profile: runtime_profile.to_string(),
            qt_runtime_version: qt_version.to_string(),
            qt_build_version: app_main_cpp::qt_build_version(),
            qml_entry: qml_entry.to_string(),
            qml_import_path: std::env::var("QML2_IMPORT_PATH")
                .or_else(|_| std::env::var("QML_IMPORT_PATH"))
                .unwrap_or_else(|_| "<unset>".to_string()),
            qt_plugin_path: std::env::var("QT_PLUGIN_PATH")
                .unwrap_or_else(|_| "<unset>".to_string()),
            qrc_revision: QRC_RESOURCE_REVISION.to_string(),
            platform_name: app_main_cpp::qt_platform_name(),
            input_method_module: std::env::var("QT_IM_MODULE")
                .or_else(|_| std::env::var("QT_IM_MODULES"))
                .unwrap_or_else(|_| "<windows-tsf>".to_string()),
            bundled_qt: false,
        }
    }

    fn summary(&self) -> String {
        format!(
            "runtimeProfile={} qtRuntimeVersion={} qtBuildVersion={} qmlEntry={} qmlImportPath={} qtPluginPath={} qrcRevision={} platformName={} inputMethodModule={} bundledQt={}",
            self.runtime_profile,
            self.qt_runtime_version,
            self.qt_build_version,
            self.qml_entry,
            self.qml_import_path,
            self.qt_plugin_path,
            self.qrc_revision,
            self.platform_name,
            self.input_method_module,
            self.bundled_qt
        )
    }
}

fn log_desktop_runtime_profile(qt_version: &str, qml_entry: &str) {
    let profile = DesktopRuntimeProfile::collect(qt_version, qml_entry);
    let summary = profile.summary();
    debug_log_static("app", "desktop_runtime_profile", &summary);
    log::info!(target: "app", "desktop_runtime_profile: {}", summary);
    diagnostics::set_runtime_info(diagnostics::RuntimeInfo {
        qt_runtime_version: profile.qt_runtime_version.clone(),
        qt_build_version: profile.qt_build_version.clone(),
        qpa_platform: profile.platform_name.clone(),
        input_method_module: profile.input_method_module.clone(),
        bundled_qt: profile.bundled_qt,
        package_type: diagnostics::effective_package_type().to_string(),
        rustc_version: env!("RUSTC_VERSION").to_string(),
    });
    diagnostics::set_system_info(app_main_cpp::collect_system_info());
}

fn install_translator() {
    app_main_cpp::install_translator();
}

pub fn run() {
    diagnostics::init_build_identity();
    let app_version = env!("CARGO_PKG_VERSION");
    let build_key = diagnostics::effective_build_key().to_string();
    let package_type = diagnostics::effective_package_type().to_string();
    let startup_diag =
        writer_platform_linux::begin_startup_diagnostics(app_version, &build_key, &package_type);
    startup_diag.mark("process_enter", "entered Windows Rust main");
    let _ = STARTUP_DIAG.set(startup_diag);

    startup_mark("runtime_env_begin", "configure Windows Qt runtime");
    let runtime_env_config = runtime_environment::configure_qpa_and_input_method();
    eprintln!("[RuntimeEnv] {}", runtime_env_config.summary());
    startup_mark("runtime_env_ready", &runtime_env_config.summary());

    startup_mark("platform_store_begin", "init default config store");
    writer_platform_linux::init_default_config_store();

    match std::panic::catch_unwind(writer_platform_linux::create_platform_services) {
        Ok(services) => {
            if let Some(factory) = services.sync_transport_factory {
                backend::app_backend::set_linux_sync_transport_factory(factory);
            }
            if let Some(secure_storage) = services.secure_storage {
                backend::app_backend::set_linux_secure_storage(std::sync::Arc::from(
                    secure_storage,
                ));
            }
            startup_mark("platform_services_ready", "platform services initialized");
        }
        Err(_) => startup_mark(
            "platform_services_failed",
            "create_platform_services panicked",
        ),
    }

    std::thread::Builder::new()
        .name("net-monitor".into())
        .spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            writer_platform_linux::refresh_network_state();
        })
        .ok();

    let platform_init = writer_platform_linux::resolve_platform_init();
    let session_id = uuid::Uuid::new_v4().to_string();
    writer_platform_linux::init_diagnostics(
        &platform_init,
        diagnostics::effective_build_key().to_string(),
        session_id.clone(),
        true,
        true,
    );
    begin_app_lifecycle(&platform_init.log_dir, &session_id);
    startup_mark("shared_diagnostics_ready", "writer_diagnostics initialized");

    debug_log_static(
        "app",
        "app_startup",
        "Sujian Windows application starting...",
    );
    log::info!(target: "app", "app_startup: Sujian Windows application starting...");

    let qt_ver = app_main_cpp::qt_runtime_version();
    diagnostics::set_qt_version(&qt_ver);
    debug_log_static(
        "app",
        "qt_version",
        &format!("Qt runtime version: {qt_ver}"),
    );

    fail_if_not_qt6();
    qml_resources();
    qmetaobject::qml_register_type::<AppBackend>(c"SujianApp", 1, 0, c"AppBackend");
    qmetaobject::qml_register_type::<backend::starmap_tree_model::StarmapTreeModel>(
        c"SujianApp",
        1,
        0,
        c"StarmapTreeModel",
    );
    qmetaobject::qml_register_type::<backend::starmap_library_model::StarmapLibraryModel>(
        c"SujianApp",
        1,
        0,
        c"StarmapLibraryModel",
    );
    qmetaobject::qml_register_type::<sujian_editor_item::SujianEditorItem>(
        c"Sujian",
        1,
        0,
        c"SujianEditorItem",
    );
    qmetaobject::qml_register_type::<StarMapCircleClip>(c"Sujian", 1, 0, c"StarMapCircleClip");
    qmetaobject::qml_register_type::<StarMapEdgeLayer>(c"Sujian", 1, 0, c"StarMapEdgeLayer");
    qmetaobject::qml_register_type::<StarMapObjectLayer>(c"Sujian", 1, 0, c"StarMapObjectLayer");
    qmetaobject::qml_register_type::<StarMapGridItem>(c"Sujian", 1, 0, c"StarMapGridItem");
    startup_mark("qt_runtime_probe_ready", "Qt runtime probed");

    let qml_path = "qrc:/main.qml";
    QML_LOAD_FAILED.store(false, Ordering::SeqCst);
    remember_qml_load_error("");

    let prev_handler = install_message_handler(Some(qml_load_error_handler));
    let mut engine = QmlEngine::new();
    engine.add_import_path("qrc:/".into());
    log_desktop_runtime_profile(&qt_ver, qml_path);
    log_input_method_diagnostics();
    set_application_icon();
    install_translator();

    let backend_runtime = BackendRuntime::new();
    backend_runtime.register_context_properties(&mut engine);
    startup_mark("qml_engine_ready", "QML engine created");

    engine.load_file(qml_path.into());
    install_message_handler(prev_handler);

    if QML_LOAD_FAILED.load(Ordering::SeqCst) {
        let last_error = last_qml_load_error();
        eprintln!("QML load failed for {qml_path}");
        if !last_error.is_empty() {
            eprintln!("Last QML error: {last_error}");
        }
        debug_error_static(
            "app",
            "qml_load_failed",
            &format!("QML load failed for {qml_path}: {last_error}"),
        );
        startup_mark("qml_load_failed", &format!("QML load failed: {last_error}"));
        startup_mark_exit(1);
        std::process::exit(1);
    }
    startup_mark("qml_load_ready", "QML loaded");

    app_main_cpp::install_window_state_probe();
    app_main_cpp::install_graphics_runtime_probe();
    startup_mark("event_loop_enter", "entering Qt event loop");
    engine.exec();
    startup_mark_exit(0);
    std::process::exit(0);
}
