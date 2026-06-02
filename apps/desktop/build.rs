//! # 构建脚本（Desktop UI 层 - Build）
//!
//! Cargo 构建脚本，负责：
//! 1. 声明 QML 文件依赖（rerun-if-changed）
//! 2. 明确探测并配置 Qt6 头文件包含路径
//!
//! ## 使用场景
//! - `cargo build` 时自动执行
//! - QML 文件修改后触发重新编译

use semver::Version;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const QT6_MODULES: &[&str] = &["Core", "Gui", "Qml", "Quick", "QuickControls2"];
#[cfg(target_env = "msvc")]
const CPP_STANDARD_FLAG: &str = "/std:c++17";
#[cfg(not(target_env = "msvc"))]
const CPP_STANDARD_FLAG: &str = "-std=c++17";

#[derive(Debug)]
struct Qt6BuildInfo {
    source: &'static str,
    version: String,
    include_paths: Vec<PathBuf>,
    library_paths: Vec<PathBuf>,
}

impl Qt6BuildInfo {
    fn apply_to(&self, config: &mut cpp_build::Config) {
        for path in &self.include_paths {
            config.include(path);
        }
    }
}

fn format_paths(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return "not detected".to_string();
    }
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(":")
}

fn qmake_query(qmake: &Path, key: &str) -> Option<String> {
    let output = Command::new(qmake).args(["-query", key]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn find_qt6_qmake() -> Option<PathBuf> {
    if let Ok(qmake) = std::env::var("QMAKE") {
        let path = PathBuf::from(qmake);
        if qmake_query(&path, "QT_VERSION").is_some_and(|v| v.starts_with('6')) {
            return Some(path);
        }
    }

    let candidates = [
        "qmake6",
        "qmake-qt6",
        "/usr/lib64/qt6/bin/qmake",
        "/usr/lib64/qt6/bin/qmake6",
        "/usr/bin/qmake6",
        "/usr/bin/qmake-qt6",
    ];
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|path| qmake_query(path, "QT_VERSION").is_some_and(|v| v.starts_with('6')))
}

fn detect_qt6_from_pkg_config() -> Option<Qt6BuildInfo> {
    let mut found_all = true;
    let mut version = None;
    let mut include_paths = Vec::new();
    let mut library_paths = Vec::new();
    for module in QT6_MODULES {
        let lib = format!("Qt6{}", module);
        match pkg_config::Config::new().atleast_version("6").probe(&lib) {
            Ok(library) => {
                version.get_or_insert(library.version);
                for path in library.include_paths {
                    if !include_paths.contains(&path) {
                        include_paths.push(path);
                    }
                }
                for path in library.link_paths {
                    if !library_paths.contains(&path) {
                        library_paths.push(path);
                    }
                }
            }
            Err(err) => {
                println!("cargo:warning=Could not find {lib} via pkg-config: {err}");
                found_all = false;
            }
        }
    }
    found_all.then_some(Qt6BuildInfo {
        source: "pkg-config",
        version: version.unwrap_or_else(|| "6".to_string()),
        include_paths,
        library_paths,
    })
}

fn detect_qt6_from_qmake() -> Option<Qt6BuildInfo> {
    let Some(qmake) = find_qt6_qmake() else {
        return None;
    };
    let version = qmake_query(&qmake, "QT_VERSION")?;
    let Some(headers) = qmake_query(&qmake, "QT_INSTALL_HEADERS") else {
        return None;
    };
    let header_root = PathBuf::from(headers);
    let mut include_paths = vec![header_root.clone()];
    for module in QT6_MODULES {
        include_paths.push(header_root.join(format!("Qt{}", module)));
    }
    let library_paths = qmake_query(&qmake, "QT_INSTALL_LIBS")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    Some(Qt6BuildInfo {
        source: "qmake",
        version,
        include_paths,
        library_paths,
    })
}

fn detect_qt6_from_env() -> Option<Qt6BuildInfo> {
    // On MSVC Windows, the header structure is slightly different and we might not have qtcoreversion.h
    // in the exact same expected place, so we can just blindly trust the env vars if we are on MSVC
    if std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default() == "msvc" {
        // The MSVC fallback is handled below in select_qt6_build_info
        if std::env::var("QT_INCLUDE_PATH").is_ok() && std::env::var("QT_LIBRARY_PATH").is_ok() {
            return None;
        }
    }

    let Ok(include_path) = std::env::var("QT_INCLUDE_PATH").map(|v| v.trim().to_string()) else {
        return None;
    };
    if include_path.is_empty() {
        return None;
    }
    let version = match qt_version_from_include_path(&include_path) {
        Some(version) => version,
        None => {
            println!(
                "cargo:warning=QT_INCLUDE_PATH={include_path} does not contain a readable Qt6 QtCore/qtcoreversion.h; trying other Qt6 probes"
            );
            return None;
        }
    };
    let parsed = version
        .parse::<Version>()
        .unwrap_or_else(|_| panic!("Unable to parse Qt version from QT_INCLUDE_PATH: {version}"));
    if parsed.major != 6 {
        panic!("Desktop binary requires Qt6; Qt5 is no longer supported. QT_INCLUDE_PATH points to Qt {version}.");
    }
    let header_root = PathBuf::from(include_path);
    let mut include_paths = vec![header_root.clone()];
    for module in QT6_MODULES {
        include_paths.push(header_root.join(format!("Qt{}", module)));
    }
    let library_paths = std::env::var("QT_LIBRARY_PATH")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .into_iter()
        .collect();
    Some(Qt6BuildInfo {
        source: "QT_INCLUDE_PATH",
        version,
        include_paths,
        library_paths,
    })
}

fn qt_version_from_include_path(path: &str) -> Option<String> {
    let header = Path::new(path).join("QtCore").join("qtcoreversion.h");
    let content = fs::read_to_string(header).ok()?;
    content.lines().find_map(|line| {
        if !line.contains("QTCORE_VERSION_STR") {
            return None;
        }
        line.split('"').nth(1).map(str::to_string)
    })
}

fn select_qt6_build_info() -> Qt6BuildInfo {
    println!("cargo:rerun-if-env-changed=QMAKE");
    println!("cargo:rerun-if-env-changed=QT_INCLUDE_PATH");
    println!("cargo:rerun-if-env-changed=QT_LIBRARY_PATH");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=CXX");
    println!("cargo:rerun-if-env-changed=CXXFLAGS");

    if std::env::var("QMAKE").is_ok() {
        if let Some(info) = detect_qt6_from_qmake() {
            return info;
        }
    }

    if let Some(info) = detect_qt6_from_pkg_config() {
        return info;
    }

    if let Some(info) = detect_qt6_from_qmake() {
        return info;
    }

    if let Some(info) = detect_qt6_from_env() {
        return info;
    }

    // Add fallback for Windows if all else fails and we are under MSVC and QT_INCLUDE_PATH is present
    if std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default() == "msvc" {
        if let Ok(include_path) = std::env::var("QT_INCLUDE_PATH") {
            let header_root = PathBuf::from(&include_path);
            let mut include_paths = vec![header_root.clone()];
            for module in QT6_MODULES {
                include_paths.push(header_root.join(format!("Qt{}", module)));
            }
            let library_paths: Vec<PathBuf> =
                vec![std::env::var("QT_LIBRARY_PATH").unwrap_or_default().into()];
            return Qt6BuildInfo {
                source: "MSVC Fallback",
                version: "6.6.3".to_string(), // we can assume default or read from action
                include_paths,
                library_paths,
            };
        }
    }

    panic!(
        "Qt6 development files were not found. Install qt6-qtbase-devel qt6-qtdeclarative-devel qt6-qtquickcontrols2-devel qt6-qttools-devel and gcc-c++ on Fedora."
    );
}

fn configure_cpp_standard(config: &mut cpp_build::Config) {
    // cpp_build 0.5.x otherwise injects -std=c++11, which is too old for Qt6 headers.
    config.flag(CPP_STANDARD_FLAG);
    if std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default() == "msvc" {
        config.flag("/Zc:__cplusplus");
        config.flag("/permissive-");
    }
}

fn main() {
    let qt_info = select_qt6_build_info();

    // Pages
    println!("cargo:rerun-if-changed=qml/main.qml");
    println!("cargo:rerun-if-changed=qml/SettingsDialog.qml");
    println!("cargo:rerun-if-changed=qml/EditorPage.qml");
    println!("cargo:rerun-if-changed=qml/ActionRegistryPage.qml");
    println!("cargo:rerun-if-changed=qml/SyncPage.qml");
    println!("cargo:rerun-if-changed=qml/AppController.qml");
    println!("cargo:rerun-if-changed=qml/ProjectController.qml");
    println!("cargo:rerun-if-changed=qml/StarMapController.qml");
    println!("cargo:rerun-if-changed=qml/ProjectHomePage.qml");
    println!("cargo:rerun-if-changed=qml/StatsPreviewPage.qml");
    println!("cargo:rerun-if-changed=qml/StarMapPage.qml");
    println!("cargo:rerun-if-changed=qml/StarMapWorkspace.qml");
    println!("cargo:rerun-if-changed=qml/StarMapCanvas.qml");
    println!("cargo:rerun-if-changed=qml/StarMapGraphController.qml");
    println!("cargo:rerun-if-changed=qml/StarMapNode.qml");
    println!("cargo:rerun-if-changed=qml/StarMapInspector.qml");
    println!("cargo:rerun-if-changed=qml/WritingWorkspace.qml");
    println!("cargo:rerun-if-changed=qml/WritingTreeController.qml");
    println!("cargo:rerun-if-changed=qml/EditorController.qml");
    println!("cargo:rerun-if-changed=qml/SmoothCursor.qml");
    println!("cargo:rerun-if-changed=qml/TopWritingToolbar.qml");
    println!("cargo:rerun-if-changed=qml/RightDrawer.qml");
    println!("cargo:rerun-if-changed=qml/HubPageFrame.qml");
    println!("cargo:rerun-if-changed=qml/HubPageHeader.qml");
    println!("cargo:rerun-if-changed=qml/HubContentGrid.qml");
    println!("cargo:rerun-if-changed=qml/CardCollectionPage.qml");
    println!("cargo:rerun-if-changed=qml/StatCard.qml");
    println!("cargo:rerun-if-changed=qml/SettingCard.qml");
    println!("cargo:rerun-if-changed=qml/ModernSwitch.qml");
    println!("cargo:rerun-if-changed=qml/ModernComboBox.qml");
    println!("cargo:rerun-if-changed=qml/SettingsSection.qml");
    println!("cargo:rerun-if-changed=qml/DashboardGrid.qml");
    println!("cargo:rerun-if-changed=qml/DashboardSection.qml");
    // Components
    println!("cargo:rerun-if-changed=qml/AppButton.qml");
    println!("cargo:rerun-if-changed=qml/AppCard.qml");
    println!("cargo:rerun-if-changed=qml/AppTextField.qml");
    println!("cargo:rerun-if-changed=qml/AppSwitch.qml");
    println!("cargo:rerun-if-changed=qml/AppSlider.qml");
    println!("cargo:rerun-if-changed=qml/AppComboBox.qml");
    println!("cargo:rerun-if-changed=qml/AppDialog.qml");
    println!("cargo:rerun-if-changed=qml/AppText.qml");
    println!("cargo:rerun-if-changed=qml/SectionHeader.qml");
    println!("cargo:rerun-if-changed=qml/SettingsRow.qml");
    println!("cargo:rerun-if-changed=qml/SidebarItem.qml");
    println!("cargo:rerun-if-changed=qml/StatusPill.qml");
    println!("cargo:rerun-if-changed=qml/ToolbarButton.qml");

    let mut config = cpp_build::Config::new();
    configure_cpp_standard(&mut config);
    qt_info.apply_to(&mut config);
    println!(
        "cargo:warning=Desktop Qt binding selected Qt {} via {}",
        qt_info.version, qt_info.source
    );
    println!("cargo:warning=Desktop Qt C++ standard: {CPP_STANDARD_FLAG}");
    println!(
        "cargo:warning=Desktop Qt include path: {}",
        format_paths(&qt_info.include_paths)
    );
    println!(
        "cargo:warning=Desktop Qt library path: {}",
        format_paths(&qt_info.library_paths)
    );
    config.build("src/main.rs");
}
