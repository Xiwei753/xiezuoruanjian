// =============================================================================
// main.rs — Desktop 客户端应用主入口
// =============================================================================
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
// - 作为 apps/desktop 二进制项目的独立编译与执行起点（main.rs）。
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

#![recursion_limit = "4096"]
//! Desktop 客户端入口：只负责 Qt/QML 启动、资源注册和顶层 Backend 注册。

use cpp::cpp;
use qmetaobject::log::{install_message_handler, QMessageLogContext, QtMsgType};
use qmetaobject::prelude::*;
use qmetaobject::QString;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

mod backend;
mod editor;
mod starmap_bridge;
mod sujian_editor_item;
mod sync_bridge;
mod platform_utils;
mod writing_bridge;

use backend::app_backend::{debug_error_static, debug_log_static, debug_warn_static};
use backend::diagnostics;
use backend::{AppBackend, BackendRuntime};

cpp! {{
    #include <QCoreApplication>
    #include <QFileInfo>
    #include <QGuiApplication>
    #include <QIcon>
    #include <QStyleHints>
    #include <QStringList>
    #include <QTranslator>
    #include <QtGlobal>
    #include <QTimer>
    #include <QWindow>
    #ifdef _WIN32
    #include <dwmapi.h>
    #pragma comment(lib, "dwmapi.lib")
    #endif
}}

qmetaobject::qrc!(qml_resources, "/" {
    "qtquickcontrols2.conf" as "qtquickcontrols2.conf",
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
    "qml/EditorWheelScroller.qml" as "EditorWheelScroller.qml",
    "qml/SmoothWheelScroller.qml" as "SmoothWheelScroller.qml",
    "qml/EditorAnimationOverlay.qml" as "EditorAnimationOverlay.qml",
    "qml/EditorGlyphGhost.qml" as "EditorGlyphGhost.qml",
    "qml/TopWritingToolbar.qml" as "TopWritingToolbar.qml",
    "qml/EditorContextMenu.qml" as "EditorContextMenu.qml",
    "qml/ScreenPolicyAdapter.qml" as "ScreenPolicyAdapter.qml",
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
    let version_ptr = cpp!(unsafe [] -> *const c_char as "const char *" {
        return qVersion();
    });
    if version_ptr.is_null() {
        return "unknown".to_string();
    }
    unsafe { CStr::from_ptr(version_ptr).to_string_lossy().into_owned() }
}

fn fail_if_not_qt6() {
    let version = qt_runtime_version();
    eprintln!("[QtDiagnostics] linked Qt runtime version: {}", version);
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
        // 同时写入文件日志，确保 Windows 双击 exe 崩溃后可查
        diagnostics::log_to_file("WARN", "app", "qml_warning_critical", &s);
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
        // 同时写入文件日志
        diagnostics::log_to_file("DEBUG", "app", "qml_debug", &s);
    }
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
    cpp!(unsafe [] {
        QGuiApplication::setWindowIcon(QIcon(":/icons/sujian.svg"));
    });
}

/// 设置 Windows 原生标题栏深色模式
///
/// 通过 DwmSetWindowAttribute 调用 DWM API：
/// - DWMWA_USE_IMMERSIVE_DARK_MODE (20)：启用沉浸式深色标题栏
/// - DWMWA_CAPTION_COLOR (35)：标题栏背景色（Windows 11 22H2+，可选）
/// - DWMWA_TEXT_COLOR (36)：标题栏文字色（Windows 11 22H2+，可选）
///
/// 非 Windows 平台为空操作。
pub(crate) fn apply_dwm_dark_mode(is_dark: bool) {
    let dark_value: i32 = if is_dark { 1 } else { 0 };
    cpp!(unsafe [dark_value as "int"] {
        #ifdef _WIN32
        QWindowList windows = QGuiApplication::topLevelWindows();
        if (!windows.isEmpty()) {
            QWindow* window = windows.first();
            HWND hwnd = (HWND)window->winId();
            if (hwnd) {
                // DWMWA_USE_IMMERSIVE_DARK_MODE = 20
                DwmSetWindowAttribute(hwnd, 20, &dark_value, sizeof(dark_value));
                // DWMWA_CAPTION_COLOR = 35, DWMWA_TEXT_COLOR = 36
                // 仅 Windows 11 22H2+ 支持，失败时忽略
                if (dark_value) {
                    COLORREF caption_color = RGB(28, 27, 31);
                    DwmSetWindowAttribute(hwnd, 35, &caption_color, sizeof(caption_color));
                    COLORREF text_color = RGB(224, 224, 224);
                    DwmSetWindowAttribute(hwnd, 36, &text_color, sizeof(text_color));
                } else {
                    COLORREF caption_color = RGB(243, 243, 243);
                    DwmSetWindowAttribute(hwnd, 35, &caption_color, sizeof(caption_color));
                    COLORREF text_color = RGB(0, 0, 0);
                    DwmSetWindowAttribute(hwnd, 36, &text_color, sizeof(text_color));
                }
            }
        } else {
            // 窗口尚未创建，延迟 100ms 重试一次
            QTimer::singleShot(100, [dark_value]() {
                QWindowList wins = QGuiApplication::topLevelWindows();
                if (!wins.isEmpty()) {
                    QWindow* w = wins.first();
                    HWND h = (HWND)w->winId();
                    if (h) {
                        DwmSetWindowAttribute(h, 20, &dark_value, sizeof(dark_value));
                        if (dark_value) {
                            COLORREF cc = RGB(28, 27, 31);
                            DwmSetWindowAttribute(h, 35, &cc, sizeof(cc));
                            COLORREF tc = RGB(224, 224, 224);
                            DwmSetWindowAttribute(h, 36, &tc, sizeof(tc));
                        } else {
                            COLORREF cc = RGB(243, 243, 243);
                            DwmSetWindowAttribute(h, 35, &cc, sizeof(cc));
                            COLORREF tc = RGB(0, 0, 0);
                            DwmSetWindowAttribute(h, 36, &tc, sizeof(tc));
                        }
                    }
                }
            });
        }
        #else
        (void)dark_value;
        #endif
    });
}

fn log_input_method_diagnostics() {
    let qt_im_module = std::env::var("QT_IM_MODULE").unwrap_or_else(|_| "<unset>".to_string());
    let xmodifiers = std::env::var("XMODIFIERS").unwrap_or_else(|_| "<unset>".to_string());
    let xdg_session_type =
        std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "<unset>".to_string());
    let qt_library_paths = cpp!(unsafe [] -> QString as "QString" {
        return QCoreApplication::libraryPaths().join(QStringLiteral(";"));
    });
    let fcitx_plugins = cpp!(unsafe [] -> QString as "QString" {
        QStringList matches;
        const QString relative = QStringLiteral("/platforminputcontexts/libfcitx5platforminputcontextplugin.so");
        for (const QString& base : QCoreApplication::libraryPaths()) {
            const QString candidate = base + relative;
            if (QFileInfo::exists(candidate)) {
                matches << candidate;
            }
        }
        return matches.join(QStringLiteral(";"));
    });

    eprintln!(
        "[QtInputMethodDiagnostics] QT_IM_MODULE={} XMODIFIERS={} XDG_SESSION_TYPE={} qt_library_paths={} fcitx5_qt6_plugins={}",
        qt_im_module,
        xmodifiers,
        xdg_session_type,
        qt_library_paths,
        fcitx_plugins
    );
}

fn install_translator() {
    // Install QTranslator for i18n support.
    // Loads the compiled .qm file from the embedded qrc resource.
    // The .qm file is generated by lrelease from .ts during build.
    let loaded = cpp!(unsafe [] -> bool as "bool" {
        QTranslator *translator = new QTranslator(QCoreApplication::instance());
        // Try loading from qrc embedded resource first
        bool ok = translator->load(QStringLiteral(":/i18n/zh_CN.qm"));
        if (!ok) {
            // Fallback: try from filesystem relative to executable
            ok = translator->load(QStringLiteral("zh_CN"),
                                   QCoreApplication::applicationDirPath() + QStringLiteral("/i18n"));
        }
        if (ok) {
            QCoreApplication::installTranslator(translator);
            return true;
        } else {
            delete translator;
            return false;
        }
    });
    if loaded {
        debug_log_static("app", "i18n", "QTranslator loaded successfully (zh_CN)");
    } else {
        debug_log_static("app", "i18n", "QTranslator not loaded; running with source strings");
    }
}

fn main() {
    // ===== 最早期初始化：确保崩溃/错误能写入日志文件 =====
    // 这两行必须在所有其他代码之前执行
    // Windows 双击 exe 没有控制台，崩溃后只能靠文件日志排查
    diagnostics::ensure_early_log_dir();
    diagnostics::install_panic_hook();

    debug_log_static("app", "app_startup", "Sujian application starting...");
    diagnostics::log_to_file("INFO", "app", "app_startup", "Sujian application starting...");

    // 注入 Qt 运行时版本到 diagnostics 模块（避免运行时调用 qmake 命令）
    let qt_ver = qt_runtime_version();
    diagnostics::set_qt_version(&qt_ver);
    debug_log_static("app", "qt_version", &format!("Qt runtime version: {}", qt_ver));

    fail_if_not_qt6();
    std::env::set_var("QT_QUICK_CONTROLS_STYLE", "Basic");
    qml_resources();
    probe_hub_header_resource();
    qmetaobject::qml_register_type::<AppBackend>(
        c"SujianApp",
        1,
        0,
        c"AppBackend",
    );
    qmetaobject::qml_register_type::<sujian_editor_item::SujianEditorItem>(
        c"Sujian",
        1,
        0,
        c"SujianEditorItem",
    );

    let qml_path = "qrc:/main.qml";
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

    // 设置 Windows 原生标题栏深色模式
    // 使用 Qt 系统主题检测确定初始值，QML isDark 变化时会再次更新
    let system_is_dark = cpp!(unsafe [] -> bool as "bool" {
        #if QT_VERSION >= QT_VERSION_CHECK(6, 5, 0)
        return QGuiApplication::styleHints()->colorScheme() == Qt::ColorScheme::Dark;
        #else
        return false;
        #endif
    });
    apply_dwm_dark_mode(system_is_dark);

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
