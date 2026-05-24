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
    println!("cargo:rerun-if-changed=qml/HubPageFrame.qml");
    println!("cargo:rerun-if-changed=qml/HubPageHeader.qml");
    println!("cargo:rerun-if-changed=qml/StatCard.qml");
    println!("cargo:rerun-if-changed=qml/SettingCard.qml");
    println!("cargo:rerun-if-changed=qml/ModernSwitch.qml");
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
    config.include(env::var("DEP_QT_INCLUDE").unwrap_or_else(|_| "".into()));
    config.build("src/main.rs");
}
