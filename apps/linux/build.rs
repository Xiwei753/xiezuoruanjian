//! # 构建脚本（Linux UI 层 - Build）
//!
//! Cargo 构建脚本，负责：
//! 1. 声明 QML 文件依赖（rerun-if-changed）
//! 2. 配置 Qt5 头文件包含路径
//!
//! ## 使用场景
//! - `cargo build` 时自动执行
//! - QML 文件修改后触发重新编译

use std::env;

fn main() {
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
    if let Ok(qt5_quick) = pkg_config::probe_library("Qt5Quick") {
        for p in qt5_quick.include_paths {
            config.include(p);
        }
    }
    config.build("src/main.rs");
}
