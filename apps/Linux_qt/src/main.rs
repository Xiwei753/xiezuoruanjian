// =============================================================================
// main.rs — Linux_qt 客户端应用主入口
// =============================================================================
// QML/C++ 通过 cpp! 和 qmetaobject 宏调用的方法对 Rust 编译器不可见，
// 因此 dead_code、unwrap_used 是误报；测试代码同理。
// Qt FFI 边界大量 u32/i64/f32/f64 类型转换属于平台协议，无法避免。
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless
)]
#![allow(
    clippy::too_many_arguments,
    clippy::module_inception,
    clippy::type_complexity,
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::many_single_char_names
)]
#![allow(
    clippy::redundant_closure,
    clippy::redundant_pattern,
    clippy::field_reassign_with_default
)]
#![allow(
    clippy::map_identity,
    clippy::clone_on_copy,
    clippy::needless_range_loop
)]
#![allow(
    clippy::identity_op,
    clippy::bool_assert_comparison,
    clippy::eq_op,
    clippy::double_must_use
)]
#![allow(
    clippy::items_after_test_module,
    clippy::same_functions_in_if_condition
)]
#![allow(
    clippy::option_map_unit_fn,
    clippy::match_same_arms,
    clippy::redundant_field_names
)]
#![allow(
    clippy::get_first,
    clippy::format_in_format_args,
    clippy::let_and_return
)]
#![allow(
    clippy::transmute_ptr_to_ptr,
    clippy::transmute_ptr_to_ref,
    clippy::useless_transmute
)]
#![allow(clippy::len_zero)]
#![allow(clippy::get_unwrap, clippy::redundant_clone)]
#![allow(clippy::if_same_then_else)]
#![allow(
    clippy::question_mark,
    clippy::vec_init_then_push,
    clippy::collapsible_if
)]
#![allow(clippy::manual_clamp, clippy::unnecessary_cast)]
#![allow(clippy::wrong_self_convention)]
// unreachable_patterns — Qt/cfg 条件编译造成的模式匹配冗余
#![allow(unreachable_patterns)]
#![allow(deprecated)]
//
//
// 引用了什么：
// - qmetaobject：用于提供 Rust 与 Qt/QML 引擎的高性能双向桥接。
// - cpp：允许 Rust 内部嵌入内联 C++ 代码调用 Qt 平台 API。
// - backend：引入 BackendRuntime 和 AppBackend，实现多领域薄后端的聚合管理。
//
// 干什么的：
// - 初始化日志并拦截 Qt 级别的调试/警示日志信息（QMessageLogContext）。
// - 执行 Qt 运行时链接版本安全检查，预防 Qt5 和 Qt6 资源混用。
// - 负责将所有静态 QML 和图形资源嵌入程序二进制文件（qml_resources qrc）。
// - 在加载 main.qml 之前，将领域后端（如 workspaceBackend、syncBackend 等）注册为 Qt Quick 上下文属性。
// - 启动 Qt 事件循环拉起客户端界面。
//
// 被什么引用：
// - 作为 apps/Linux_qt 二进制项目的独立编译与执行起点（main.rs）。
//
// ── LayoutPlan 边界约束 ──
//
// LayoutPlan（由 Core resolve_layout 产出）只决定壳层布局，包括：
//   - 导航模式（shellMode: compact/medium/expanded）
//   - 内容区域最大宽度（contentMaxWidthVp）
//   - 页面内边距（contentPaddingVp）
//   - 侧栏可见性与宽度
//
// LayoutPlan 绝对不干预编辑器底层渲染，具体包括：
//   - 不传递到 SujianEditorItem 的 QSG 渲染线程
//   - 不影响光标位置、IME 输入、动画帧率
//   - 不改变 QTextLayout 的排版计算
//   - 不驱动 EditorAnimationOverlay 的动画属性
//
// 编辑器渲染由 EditorController + SujianEditorItem 独立管理，
// 遵守 Qt QSG 线程边界，不受 LayoutPlan 影响。
// =============================================================================
#![recursion_limit = "8192"]
//! Linux_qt 客户端入口：只负责 Qt/QML 启动、资源注册和顶层 Backend 注册。

use qmetaobject::log::{install_message_handler, QMessageLogContext, QtMsgType};
use qmetaobject::prelude::*;
use qmetaobject::QString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use sujian_linux_qt::app_main_cpp;
use sujian_linux_qt::backend;
use sujian_linux_qt::sujian_editor_item;

use backend::app_backend::{debug_error_static, debug_log_static, debug_warn_static};
use backend::diagnostics;
use backend::{AppBackend, BackendRuntime};

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
    "qml/ProjectController.qml" as "ProjectController.qml",
    "qml/StarMapController.qml" as "StarMapController.qml",
    "qml/WritingWorkspace.qml" as "WritingWorkspace.qml",
    "qml/WritingTreeController.qml" as "WritingTreeController.qml",
    "qml/EditorController.qml" as "EditorController.qml",
    "qml/TopWritingToolbar.qml" as "TopWritingToolbar.qml",
    "qml/EditorContextMenu.qml" as "EditorContextMenu.qml",
    "qml/RightDrawer.qml" as "RightDrawer.qml",
    "qml/SettingsDialog.qml" as "SettingsDialog.qml",
    "qml/SettingsSection.qml" as "SettingsSection.qml",
    "qml/SettingCard.qml" as "SettingCard.qml",
    "qml/ModernSwitch.qml" as "ModernSwitch.qml",
    "qml/ModernComboBox.qml" as "ModernComboBox.qml",
    "qml/DashboardGrid.qml" as "DashboardGrid.qml",
    "qml/DashboardSection.qml" as "DashboardSection.qml",
    "qml/ActionRegistryPage.qml" as "ActionRegistryPage.qml",
    "qml/SyncPage.qml" as "SyncPage.qml",
    "qml/SyncMessageKeys.qml" as "SyncMessageKeys.qml",
    "qml/EmptyWorkspace.qml" as "EmptyWorkspace.qml",
    // Components
    "qml/AppButton.qml" as "AppButton.qml",
    "qml/AppCard.qml" as "AppCard.qml",
    "qml/AppTextField.qml" as "AppTextField.qml",
    "qml/AppSwitch.qml" as "AppSwitch.qml",
    "qml/AppSlider.qml" as "AppSlider.qml",
    "qml/AppComboBox.qml" as "AppComboBox.qml",
    "qml/AppDialog.qml" as "AppDialog.qml",
    "qml/AppText.qml" as "AppText.qml",
    "qml/AppShadow.qml" as "AppShadow.qml",
    "qml/SectionHeader.qml" as "SectionHeader.qml",
    "qml/SettingsRow.qml" as "SettingsRow.qml",
    "qml/DesktopWheelScrollHandler.qml" as "DesktopWheelScrollHandler.qml",
    "qml/SidebarItem.qml" as "SidebarItem.qml",
    "qml/WorkspaceTree.qml" as "WorkspaceTree.qml",
    "qml/CreateProjectDialog.qml" as "CreateProjectDialog.qml",
    "qml/StatusPill.qml" as "StatusPill.qml",
    "qml/ToolbarButton.qml" as "ToolbarButton.qml",
    "resources/icons/sujian.svg" as "icons/sujian.svg",
    // i18n translations
    "i18n/zh_CN.qm" as "i18n/zh_CN.qm",
});

static QML_LOAD_FAILED: AtomicBool = AtomicBool::new(false);
static QML_HUB_HEADER_MISSING: AtomicBool = AtomicBool::new(false);
static QML_LAST_LOAD_ERROR: OnceLock<Mutex<String>> = OnceLock::new();

fn qt_runtime_version() -> String {
    app_main_cpp::qt_runtime_version()
}

fn qt_build_version() -> String {
    app_main_cpp::qt_build_version()
}

fn fail_if_not_qt6() {
    let version = qt_runtime_version();
    // 只在调试模式或版本异常时输出 Qt 版本信息
    if std::env::var("SUJIAN_EDITOR_DEBUG").is_ok()
        || std::env::var("WRITER_DEBUG").is_ok()
        || !version.starts_with("6.")
    {
        eprintln!("[QtDiagnostics] linked Qt runtime version: {}", version);
    }
    if version.starts_with("5.") {
        eprintln!("Linux binary requires Qt6; Qt5 is no longer supported.");
        std::process::exit(1);
    }
    if !version.starts_with("6.") {
        eprintln!(
            "[QtDiagnostics] WARNING: expected Qt6 runtime, got {}",
            version
        );
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
    let s = format!("{}", msg);
    // Qt 消息是平台来源，origin = System。
    if matches!(msg_type, QtMsgType::QtWarningMsg | QtMsgType::QtCriticalMsg) {
        eprintln!(
            "[Qt {}] {}",
            match msg_type {
                QtMsgType::QtWarningMsg => "WARNING",
                QtMsgType::QtCriticalMsg => "CRITICAL",
                _ => "INFO",
            },
            s
        );
        debug_warn_static("app", "qml_warning_critical", &s);
        // Qt 消息转成共享 Rust 诊断事件（origin=System），由 writer_diagnostics 接管落盘。
        record_qt_event(
            writer_diagnostics::DiagnosticLevel::Warn,
            "qml_warning_critical",
            &s,
        );
        if s.contains("qrc:/main.qml")
            || s.contains("QQmlApplicationEngine failed")
            || s.contains("failed to load component")
            || s.contains("is not installed")
            || s.contains("import requires")
        {
            remember_qml_load_error(&s);
            QML_LOAD_FAILED.store(true, Ordering::SeqCst);
        }
        if s.contains("qrc:/HubPageHeader.qml") && s.contains("No such file") {
            QML_HUB_HEADER_MISSING.store(true, Ordering::SeqCst);
        }
    } else {
        eprintln!("[Qt DEBUG] {}", s);
        debug_log_static("app", "qml_debug", &s);
        record_qt_event(writer_diagnostics::DiagnosticLevel::Debug, "qml_debug", &s);
    }
}

/// 把 Qt 消息转成共享 Rust 诊断事件（origin=System，平台来源）。
///
/// Qt 消息是平台来源，不自己写文件，只入队 writer_diagnostics 统一后端。
/// Issue #670 评论 5651816143 修改 2：sequence / session_id 由
/// `writer_diagnostics::record_event` 统一补全，此处不再写死 0 / ""。
fn record_qt_event(level: writer_diagnostics::DiagnosticLevel, event: &str, message: &str) {
    use std::collections::BTreeMap;
    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        // sequence / session_id 由 writer_diagnostics::record_event 统一补全
        // （Issue #670 评论 5651816143 修改 2）。
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

fn probe_hub_header_resource() {
    QML_HUB_HEADER_MISSING.store(false, Ordering::SeqCst);
    let prev_handler = install_message_handler(Some(qml_load_error_handler));
    let mut probe_engine = QmlEngine::new();
    probe_engine.load_file("qrc:/HubPageHeader.qml".into());
    install_message_handler(prev_handler);

    if QML_HUB_HEADER_MISSING.load(Ordering::SeqCst) {
        debug_error_static(
            "app",
            "qml_resource_probe",
            "qrc:/HubPageHeader.qml missing from embedded qrc",
        );
    } else {
        debug_log_static(
            "app",
            "qml_resource_probe",
            "qrc:/HubPageHeader.qml exists in embedded qrc",
        );
    }
}

fn set_application_icon() {
    app_main_cpp::set_application_icon();
}

fn log_input_method_diagnostics() {
    app_main_cpp::log_input_method_diagnostics();
}

const QRC_RESOURCE_REVISION: &str = concat!(env!("CARGO_PKG_VERSION"), ":qml_resources_v1");

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
        let appimage = std::env::var_os("APPIMAGE").is_some();
        let bundled_qt = appimage;
        let runtime_profile = if appimage {
            "linux-appimage"
        } else {
            "linux-debug"
        };
        let qt_plugin_path =
            std::env::var("QT_PLUGIN_PATH").unwrap_or_else(|_| "<unset>".to_string());
        let qml_import_path = std::env::var("QML2_IMPORT_PATH")
            .or_else(|_| std::env::var("QML_IMPORT_PATH"))
            .unwrap_or_else(|_| "<unset>".to_string());
        let platform_name = app_main_cpp::qt_platform_name();
        let input_method_module = std::env::var("QT_IM_MODULE")
            .or_else(|_| std::env::var("QT_IM_MODULES"))
            .unwrap_or_else(|_| "<unset>".to_string());

        Self {
            runtime_profile: runtime_profile.to_string(),
            qt_runtime_version: qt_version.to_string(),
            qt_build_version: qt_build_version(),
            qml_entry: qml_entry.to_string(),
            qml_import_path,
            qt_plugin_path,
            qrc_revision: QRC_RESOURCE_REVISION.to_string(),
            platform_name,
            input_method_module,
            bundled_qt,
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
    // 应用内部事件，由 writer_diagnostics 接管落盘。
    log::info!(target: "app", "desktop_runtime_profile: {}", summary);

    // 收集 RuntimeInfo 和 SystemInfo 并注入 diagnostics 模块
    let runtime_info = collect_runtime_info(&profile);
    let system_info = collect_system_info();
    diagnostics::set_runtime_info(runtime_info);
    diagnostics::set_system_info(system_info);
}

/// 从 DesktopRuntimeProfile 转换为 diagnostics::RuntimeInfo
fn collect_runtime_info(profile: &DesktopRuntimeProfile) -> diagnostics::RuntimeInfo {
    diagnostics::RuntimeInfo {
        qt_runtime_version: profile.qt_runtime_version.clone(),
        qt_build_version: profile.qt_build_version.clone(),
        qpa_platform: profile.platform_name.clone(),
        input_method_module: profile.input_method_module.clone(),
        bundled_qt: profile.bundled_qt,
        // #665 评论 5643315523：使用运行时有效 packageType，与 bundled_qt（运行时
        // APPIMAGE 检测）语义一致，消除编译期 env!("PACKAGE_TYPE") 与运行时
        // APPIMAGE 的语义冲突。
        package_type: diagnostics::effective_package_type().to_string(),
        rustc_version: env!("RUSTC_VERSION").to_string(),
    }
}

/// 通过 QSysInfo 和环境变量收集系统信息
fn collect_system_info() -> diagnostics::SystemInfo {
    app_main_cpp::collect_system_info()
}
fn install_translator() {
    app_main_cpp::install_translator();
}

fn main() {
    // ===== 平台适配层初始化：注入配置存储和同步传输 =====
    writer_platform_linux::init_default_config_store();
    if let Ok(services) = std::panic::catch_unwind(writer_platform_linux::create_platform_services)
    {
        if let Some(factory) = services.sync_transport_factory {
            sujian_linux_qt::backend::app_backend::set_linux_sync_transport_factory(factory);
        }
        if let Some(secure_storage) = services.secure_storage {
            sujian_linux_qt::backend::app_backend::set_linux_secure_storage(std::sync::Arc::from(
                secure_storage,
            ));
        }
        // 初始网络状态已在 create_platform_services 内缓存
    }

    // 启动后台线程定时刷新网络状态（每 30 秒）
    std::thread::Builder::new()
        .name("net-monitor".into())
        .spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            writer_platform_linux::refresh_network_state();
        })
        .ok();

    // ===== 最早期初始化：初始化统一诊断后端 =====
    // #665 评论 5643315523：先初始化运行时有效 build identity（根据 APPIMAGE 环境变量
    // 收口 packageType/buildKey），确保后续所有日志写入和 manifest 字段使用同一份有效值。
    diagnostics::init_build_identity();
    // 初始化共享 Rust 诊断后端（接管 log::* 和 panic 落盘）。
    // 日志目录、平台名、设备 ID 等由 PlatformInit 决定，build_key 用运行时有效值。
    let platform_init = writer_platform_linux::resolve_platform_init();
    let session_id = uuid::Uuid::new_v4().to_string();
    writer_platform_linux::init_diagnostics(
        &platform_init,
        diagnostics::effective_build_key().to_string(),
        session_id,
        // alpha 阶段默认开启；后续由 SettingsBackend.load_local_settings 调用
        // writer_diagnostics::set_config 覆盖为用户设置值。
        true,
        true,
    );

    debug_log_static("app", "app_startup", "Sujian application starting...");
    log::info!(target: "app", "app_startup: Sujian application starting...");

    // 注入 Qt 运行时版本到 diagnostics 模块（避免运行时调用 qmake 命令）
    let qt_ver = qt_runtime_version();
    diagnostics::set_qt_version(&qt_ver);
    debug_log_static(
        "app",
        "qt_version",
        &format!("Qt runtime version: {}", qt_ver),
    );

    fail_if_not_qt6();
    qml_resources();
    probe_hub_header_resource();
    qmetaobject::qml_register_type::<AppBackend>(c"SujianApp", 1, 0, c"AppBackend");
    qmetaobject::qml_register_type::<sujian_editor_item::SujianEditorItem>(
        c"Sujian",
        1,
        0,
        c"SujianEditorItem",
    );

    let qml_path = "qrc:/main.qml";
    log_desktop_runtime_profile(&qt_ver, qml_path);
    debug_log_static(
        "app",
        "qml_loading",
        &format!("Loading QML entry: {}", qml_path),
    );

    QML_LOAD_FAILED.store(false, Ordering::SeqCst);
    remember_qml_load_error("");
    let prev_handler = install_message_handler(Some(qml_load_error_handler));
    let mut engine = QmlEngine::new();
    log_input_method_diagnostics();
    set_application_icon();

    // Install QTranslator for i18n — load compiled .qm from embedded qrc
    // This must happen after QmlEngine::new() (which creates QCoreApplication)
    // but before loading QML, so that qsTr() calls resolve correctly.
    install_translator();

    let backend_runtime = BackendRuntime::new();
    backend_runtime.register_context_properties(&mut engine);

    engine.load_file(qml_path.into());
    install_message_handler(prev_handler);

    if QML_LOAD_FAILED.load(Ordering::SeqCst) {
        let last_error = last_qml_load_error();
        eprintln!("QML load failed for {}", qml_path);
        if !last_error.is_empty() {
            eprintln!("Last QML error: {}", last_error);
        }
        eprintln!("Check that QML2_IMPORT_PATH points to Qt6 only, for example /usr/lib64/qt6/qml, and does not include Qt5 paths.");
        debug_error_static(
            "app",
            "qml_load_failed",
            &format!("QML load failed for {}: {}", qml_path, last_error),
        );
        std::process::exit(1);
    }

    debug_log_static(
        "app",
        "event_loop_enter",
        "QML engine started, entering event loop",
    );
    engine.exec();
}
