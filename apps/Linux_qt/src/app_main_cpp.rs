//! Issue #707 评论 5723616999: main.rs 的 cpp! 块和调用移到本 lib 模块。
//!
//! `cpp_build` 的 `Config::build()` 会 `clean_artifacts()`，两次 build 会
//! 冲突。把 main.rs 的所有 cpp! 块和调用移到本 lib 模块后，build.rs 只需
//! build `src/lib.rs` 一次，就能覆盖所有 cpp! 宏（lib 模块 + app_main_cpp）。
//!
//! main.rs 通过 `use sujian_linux_qt::app_main_cpp::*` 调用这些 pub 函数。

use cpp::cpp;
use qmetaobject::QString;
use std::ffi::CStr;
use std::os::raw::c_char;

use crate::backend::app_backend::{debug_log_static};
use crate::backend::diagnostics;

cpp! {{
    #include <QCoreApplication>
    #include <QFileInfo>
    #include <QGuiApplication>
    #include <QIcon>
    #include <QStyleHints>
    #include <QStringList>
    #include <QSysInfo>
    #include <QTranslator>
    #include <QtGlobal>
}}

pub fn qt_runtime_version() -> String {
    let version_ptr = cpp!(unsafe [] -> *const c_char as "const char *" {
        return qVersion();
    });
    if version_ptr.is_null() {
        return "unknown".to_string();
    }
    // SAFETY: version_ptr was returned by Qt's qVersion() which returns a valid C string pointer; null check is above.
    unsafe { CStr::from_ptr(version_ptr).to_string_lossy().into_owned() }
}

pub fn qt_build_version() -> String {
    let version_ptr = cpp!(unsafe [] -> *const c_char as "const char *" {
        return QT_VERSION_STR;
    });
    if version_ptr.is_null() {
        return "unknown".to_string();
    }
    // SAFETY: version_ptr is QT_VERSION_STR macro which is always a valid C string; null check is above.
    unsafe { CStr::from_ptr(version_ptr).to_string_lossy().into_owned() }
}

pub fn set_application_icon() {
    // SAFETY: QGuiApplication::setWindowIcon requires QGuiApplication instance; called after engine creation in main.
    cpp!(unsafe [] {
        QGuiApplication::setWindowIcon(QIcon(":/icons/sujian.svg"));
    });
}

pub fn qt_library_paths_joined() -> String {
    let qt_library_paths = cpp!(unsafe [] -> QString as "QString" {
        return QCoreApplication::libraryPaths().join(QStringLiteral(";"));
    });
    qt_library_paths.to_string()
}

pub fn fcitx_plugins_joined() -> String {
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
    fcitx_plugins.to_string()
}

pub fn log_input_method_diagnostics() {
    let qt_im_module = std::env::var("QT_IM_MODULE").unwrap_or_else(|_| "<unset>".to_string());
    let xmodifiers = std::env::var("XMODIFIERS").unwrap_or_else(|_| "<unset>".to_string());
    let xdg_session_type =
        std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "<unset>".to_string());
    let qt_library_paths = qt_library_paths_joined();
    let fcitx_plugins = fcitx_plugins_joined();

    if std::env::var("SUJIAN_EDITOR_DEBUG").is_ok() {
        eprintln!(
            "[QtInputMethodDiagnostics] QT_IM_MODULE={} XMODIFIERS={} XDG_SESSION_TYPE={} qt_library_paths={} fcitx5_qt6_plugins={}",
            qt_im_module,
            xmodifiers,
            xdg_session_type,
            qt_library_paths,
            fcitx_plugins
        );
    }
}

pub fn qt_platform_name() -> String {
    let platform_name = cpp!(unsafe [] -> QString as "QString" {
        return QGuiApplication::platformName();
    });
    platform_name.to_string()
}

pub fn collect_system_info() -> diagnostics::SystemInfo {
    let product_type = cpp!(unsafe [] -> QString as "QString" {
        return QSysInfo::productType();
    })
    .to_string();
    let product_version = cpp!(unsafe [] -> QString as "QString" {
        return QSysInfo::productVersion();
    })
    .to_string();
    let pretty_product_name = cpp!(unsafe [] -> QString as "QString" {
        return QSysInfo::prettyProductName();
    })
    .to_string();
    let kernel_type = cpp!(unsafe [] -> QString as "QString" {
        return QSysInfo::kernelType();
    })
    .to_string();
    let kernel_version = cpp!(unsafe [] -> QString as "QString" {
        return QSysInfo::kernelVersion();
    })
    .to_string();
    let current_cpu_arch = cpp!(unsafe [] -> QString as "QString" {
        return QSysInfo::currentCpuArchitecture();
    })
    .to_string();
    let build_abi = cpp!(unsafe [] -> QString as "QString" {
        return QSysInfo::buildAbi();
    })
    .to_string();
    let xdg_current_desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let xdg_session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();

    diagnostics::SystemInfo {
        product_type,
        product_version,
        pretty_product_name,
        kernel_type,
        kernel_version,
        current_cpu_arch,
        build_abi,
        xdg_current_desktop,
        xdg_session_type,
    }
}

pub fn install_translator() {
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
        debug_log_static(
            "app",
            "i18n",
            "QTranslator not loaded; running with source strings",
        );
    }
}
