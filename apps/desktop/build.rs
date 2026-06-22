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
        // Emit cargo link search paths so the linker can find Qt6 shared libraries
        for path in &self.library_paths {
            println!("cargo:rustc-link-search=native={}", path.display());
        }
    }
}

/// Recursively collect all files with the given extension under `dir`.
fn walkdir(dir: &str, ext: &str) -> Result<Vec<String>, std::io::Error> {
    let mut result = Vec::new();
    fn visit(path: &Path, ext: &str, result: &mut Vec<String>) -> std::io::Result<()> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit(&path, ext, result)?;
            } else if path.extension().is_some_and(|e| e == ext) {
                result.push(path.to_string_lossy().into_owned());
            }
        }
        Ok(())
    }
    visit(Path::new(dir), ext, &mut result)?;
    Ok(result)
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
    let mut cmd = Command::new(qmake);
    cmd.args(["-query", key]);
    // When qmake lives under /run/host/usr, it needs LD_LIBRARY_PATH to find libQt6Core.so.6
    if qmake.starts_with("/run/host") {
        cmd.env("LD_LIBRARY_PATH", "/run/host/usr/lib64");
    }
    let output = cmd.output().ok()?;
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
        "/run/host/usr/lib64/qt6/bin/qmake",
        "/run/host/usr/lib64/qt6/bin/qmake6",
        "/run/host/usr/bin/qmake6",
        "/run/host/usr/bin/qmake-qt6",
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

/// When qmake is located under /run/host, the paths it returns (e.g. /usr/include/qt6)
/// need to be remapped to /run/host/usr/include/qt6 so they are accessible inside the container.
fn remap_host_path(path: &Path, qmake: &Path) -> PathBuf {
    if qmake.starts_with("/run/host") && path.starts_with("/usr") {
        PathBuf::from("/run/host").join(path.strip_prefix("/").unwrap_or(path))
    } else {
        path.to_path_buf()
    }
}

fn detect_qt6_from_qmake() -> Option<Qt6BuildInfo> {
    let Some(qmake) = find_qt6_qmake() else {
        return None;
    };
    let version = qmake_query(&qmake, "QT_VERSION")?;
    let Some(headers) = qmake_query(&qmake, "QT_INSTALL_HEADERS") else {
        return None;
    };
    let header_root = remap_host_path(&PathBuf::from(&headers), &qmake);
    let mut include_paths = vec![header_root.clone()];
    for module in QT6_MODULES {
        include_paths.push(header_root.join(format!("Qt{}", module)));
    }
    let library_paths = qmake_query(&qmake, "QT_INSTALL_LIBS")
        .map(|libs| remap_host_path(&PathBuf::from(&libs), &qmake))
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

/// Find the Qt6 lrelease tool for compiling .ts → .qm translation files.
fn find_lrelease() -> Option<PathBuf> {
    if let Ok(lrelease) = std::env::var("LRELEASE") {
        let path = PathBuf::from(lrelease);
        if path.exists() {
            return Some(path);
        }
    }

    // Try to find lrelease alongside qmake
    if let Some(qmake) = find_qt6_qmake() {
        if let Some(bin_dir) = qmake.parent() {
            let candidate = bin_dir.join("lrelease");
            if candidate.exists() {
                return Some(candidate);
            }
            // Some distros use lrelease6
            let candidate6 = bin_dir.join("lrelease6");
            if candidate6.exists() {
                return Some(candidate6);
            }
        }
    }

    // Try common names on PATH and /run/host
    let host_candidates = [
        "lrelease6",
        "lrelease-qt6",
        "lrelease",
        "/run/host/usr/lib64/qt6/bin/lrelease",
        "/run/host/usr/lib64/qt6/bin/lrelease6",
        "/run/host/usr/bin/lrelease6",
        "/run/host/usr/bin/lrelease-qt6",
    ];
    for name in &host_candidates {
        let path = PathBuf::from(name);
        let mut cmd = Command::new(&path);
        cmd.arg("-version");
        if path.starts_with("/run/host") {
            cmd.env("LD_LIBRARY_PATH", "/run/host/usr/lib64");
        }
        if let Ok(output) = cmd.output() {
            if output.status.success() {
                return Some(path);
            }
        }
    }

    None
}

/// Compile .ts translation files to .qm using lrelease.
/// Returns the list of generated .qm file paths.
fn compile_translations() -> Vec<PathBuf> {
    let i18n_dir = Path::new("i18n");
    let mut qm_files = Vec::new();

    // Ensure the i18n directory exists
    if !i18n_dir.exists() {
        return qm_files;
    }

    // Check if there are .ts files that need compilation
    let has_ts_files = fs::read_dir(i18n_dir)
        .ok()
        .map(|entries| {
            entries
                .flatten()
                .any(|e| e.path().extension().is_some_and(|ext| ext == "ts"))
        })
        .unwrap_or(false);

    if !has_ts_files {
        return qm_files;
    }

    // Find lrelease — required when .ts files exist
    let Some(lrelease) = find_lrelease() else {
        panic!(
            "lrelease not found but .ts translation files exist in i18n/. \
             Install qt6-qttools-devel (or qt6-tools on some distros) or set the LRELEASE env var. \
             The .qm files are required by the embedded qrc resource."
        );
    };

    println!("cargo:warning=Using lrelease: {}", lrelease.display());

    // Compile each .ts file
    if let Ok(entries) = fs::read_dir(i18n_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "ts") {
                let qm_path = path.with_extension("qm");
                println!("cargo:rerun-if-changed={}", path.display());

                let mut cmd = Command::new(&lrelease);
                cmd.arg(&path)
                    .arg("-qm")
                    .arg(&qm_path)
                    .arg("-compress")
                    .arg("-removeidentical");
                // When lrelease lives under /run/host/usr, it needs LD_LIBRARY_PATH
                if lrelease.starts_with("/run/host") {
                    cmd.env("LD_LIBRARY_PATH", "/run/host/usr/lib64");
                }
                let output = cmd.output();

                match output {
                    Ok(out) => {
                        if out.status.success() {
                            println!("cargo:warning=Compiled {} → {}", path.display(), qm_path.display());
                            qm_files.push(qm_path);
                        } else {
                            let stderr = String::from_utf8_lossy(&out.stderr);
                            panic!("lrelease failed for {}: {}", path.display(), stderr);
                        }
                    }
                    Err(e) => {
                        panic!("Failed to run lrelease on {}: {}", path.display(), e);
                    }
                }
            }
        }
    }

    qm_files
}

fn main() {
    let qt_info = select_qt6_build_info();

    // Compile .ts → .qm translation files
    let _qm_files = compile_translations();

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
    println!("cargo:rerun-if-changed=qml/EditorWheelScroller.qml");
    println!("cargo:rerun-if-changed=qml/EditorTypingAnimator.qml");
    println!("cargo:rerun-if-changed=qml/EditorAnimationOverlay.qml");
    println!("cargo:rerun-if-changed=qml/TopWritingToolbar.qml");
    println!("cargo:rerun-if-changed=qml/EditorContextMenu.qml");
    println!("cargo:rerun-if-changed=qml/ScreenPolicyAdapter.qml");
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
    println!("cargo:rerun-if-changed=qml");
    if let Ok(entries) = fs::read_dir("qml") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "qml") {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    println!("cargo:rerun-if-changed=resources/icons/sujian.svg");
    // i18n translation files
    if let Ok(entries) = fs::read_dir("i18n") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "ts" || ext == "qm") {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }

    // Recursively scan src/**/*.rs for files containing cpp! macros.
    // rust-cpp's build.rs must re-scan these files whenever they change,
    // otherwise the cpp! metadata becomes stale and causes
    // "This cpp! macro is not found in the library's rust-cpp metadata" errors.
    if let Ok(entries) = walkdir("src", "rs") {
        for path in &entries {
            if let Ok(content) = fs::read_to_string(path) {
                if content.contains("cpp!") {
                    println!("cargo:rerun-if-changed={}", path);
                }
            }
        }
    }

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
    // Verify that zh_CN.qm was generated
    let zh_cn_qm = Path::new("i18n/zh_CN.qm");
    if !zh_cn_qm.exists() {
        panic!(
            "i18n/zh_CN.qm not found after build. Ensure lrelease successfully compiled the translation files."
        );
    }

    config.build("src/main.rs");
}
