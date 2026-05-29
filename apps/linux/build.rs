//! # 构建脚本（Linux UI 层 - Build）
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

fn qmake_query(qmake: &Path, key: &str) -> Option<String> {
    let output = Command::new(qmake).args(["-query", key]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() { None } else { Some(value) }
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
    candidates.iter().map(PathBuf::from).find(|path| {
        qmake_query(path, "QT_VERSION").is_some_and(|v| v.starts_with('6'))
    })
}

fn include_qt6_from_pkg_config(config: &mut cpp_build::Config) -> bool {
    let mut found_all = true;
    for module in QT6_MODULES {
        let lib = format!("Qt6{}", module);
        match pkg_config::Config::new().atleast_version("6").probe(&lib) {
            Ok(library) => {
                for path in library.include_paths {
                    config.include(path);
                }
            }
            Err(err) => {
                println!("cargo:warning=Could not find {lib} via pkg-config: {err}");
                found_all = false;
            }
        }
    }
    found_all
}

fn include_qt6_from_qmake(config: &mut cpp_build::Config) -> bool {
    let Some(qmake) = find_qt6_qmake() else {
        return false;
    };
    let Some(headers) = qmake_query(&qmake, "QT_INSTALL_HEADERS") else {
        return false;
    };
    let header_root = PathBuf::from(headers);
    config.include(&header_root);
    for module in QT6_MODULES {
        config.include(header_root.join(format!("Qt{}", module)));
    }
    true
}

fn include_qt6_from_env(config: &mut cpp_build::Config) -> bool {
    let Ok(include_path) = std::env::var("QT_INCLUDE_PATH").map(|v| v.trim().to_string()) else {
        return false;
    };
    if include_path.is_empty() {
        return false;
    }
    let header_root = PathBuf::from(include_path);
    config.include(&header_root);
    for module in QT6_MODULES {
        config.include(header_root.join(format!("Qt{}", module)));
    }
    true
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

fn assert_qt6_build_chain_available() {
    println!("cargo:rerun-if-env-changed=QMAKE");
    println!("cargo:rerun-if-env-changed=QT_INCLUDE_PATH");
    println!("cargo:rerun-if-env-changed=QT_LIBRARY_PATH");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");

    if let (Ok(include_path), Ok(library_path)) = (
        std::env::var("QT_INCLUDE_PATH").map(|v| v.trim().to_string()),
        std::env::var("QT_LIBRARY_PATH").map(|v| v.trim().to_string()),
    ) {
        if !include_path.is_empty() && !library_path.is_empty() {
            let version = qt_version_from_include_path(&include_path)
                .unwrap_or_else(|| panic!("Unable to detect Qt version from QT_INCLUDE_PATH={include_path}"));
            let parsed = version
                .parse::<Version>()
                .unwrap_or_else(|_| panic!("Unable to parse Qt version from QT_INCLUDE_PATH: {version}"));
            if parsed.major != 6 {
                panic!("Linux binary is still linked against Qt5; Qt6 migration incomplete. QT_INCLUDE_PATH points to Qt {version}.");
            }
            println!("cargo:warning=Linux Qt binding selected Qt {version} from QT_INCLUDE_PATH");
            return;
        }
    }

    let Some(qmake) = find_qt6_qmake() else {
        panic!("Qt6 qmake was not found. Install Fedora Qt6 development tools or set QMAKE to qmake6 before building the Linux frontend.");
    };
    let version = qmake_query(&qmake, "QT_VERSION")
        .unwrap_or_else(|| panic!("Unable to query Qt version from {}", qmake.display()));
    let parsed = version
        .parse::<Version>()
        .unwrap_or_else(|_| panic!("Unable to parse Qt version reported by {}: {version}", qmake.display()));
    if parsed.major != 6 {
        panic!("Linux binary is still linked against Qt5; Qt6 migration incomplete. {} reports Qt {version}.", qmake.display());
    }
    println!("cargo:warning=Linux Qt binding selected Qt {version} via {}", qmake.display());
}

fn main() {
    assert_qt6_build_chain_available();

    // Pages
    println!("cargo:rerun-if-changed=qml/main.qml");
    println!("cargo:rerun-if-changed=qml/SettingsDialog.qml");
    println!("cargo:rerun-if-changed=qml/EditorPage.qml");
    println!("cargo:rerun-if-changed=qml/ActionRegistryPage.qml");
    println!("cargo:rerun-if-changed=qml/SyncPage.qml");
    println!("cargo:rerun-if-changed=qml/ProjectHomePage.qml");
    println!("cargo:rerun-if-changed=qml/StatsPreviewPage.qml");
    println!("cargo:rerun-if-changed=qml/StarMapPage.qml");
    println!("cargo:rerun-if-changed=qml/StarMapWorkspace.qml");
    println!("cargo:rerun-if-changed=qml/StarMapCanvas.qml");
    println!("cargo:rerun-if-changed=qml/StarMapNode.qml");
    println!("cargo:rerun-if-changed=qml/StarMapInspector.qml");
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
    println!("cargo:rerun-if-changed=qml/SectionHeader.qml");
    println!("cargo:rerun-if-changed=qml/SettingsRow.qml");
    println!("cargo:rerun-if-changed=qml/SidebarItem.qml");
    println!("cargo:rerun-if-changed=qml/StatusPill.qml");
    println!("cargo:rerun-if-changed=qml/ToolbarButton.qml");
    println!("cargo:rerun-if-changed=src/qml.qrc");

    let mut config = cpp_build::Config::new();
    if !include_qt6_from_pkg_config(&mut config)
        && !include_qt6_from_qmake(&mut config)
        && !include_qt6_from_env(&mut config)
    {
        panic!("Qt6 development files were not found. Install qt6-qtbase-devel qt6-qtdeclarative-devel qt6-qtquickcontrols2-devel qt6-qttools-devel on Fedora.");
    }
    config.build("src/main.rs");
}
