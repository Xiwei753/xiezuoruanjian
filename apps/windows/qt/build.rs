//! Windows Qt 前端构建脚本。
//!
//! 负责：
//! 1. 生成 QRC 资源清单（Linux 通用 QML + Windows 平台 QML 覆盖）。
//! 2. 在 Windows 目标上探测 Qt6，编译 Windows 平台 C++，并校验 `.qsb`。
//! 3. 注入诊断所需的构建身份环境变量。
//!
//! 非 Windows 目标（例如 Linux 工作区的 `cargo check`）只生成 QRC 清单后直接返回，
//! 不要求本机安装 Windows Qt。

use semver::Version;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const QT6_MODULES: &[&str] = &["Core", "Gui", "Widgets", "Qml", "Quick", "QuickControls2"];
const CPP_STANDARD_FLAG: &str = "/std:c++17";
const LINUX_QT_DIR: &str = "../../Linux_qt";

fn main() {
    prepare_shared_sources();
    generate_qrc_resources();
    emit_qsb_rerun();
    inject_build_env_vars();

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        println!("cargo:warning=sujian-windows-qt: 非 Windows 目标，跳过 Qt/C++ 探测与 cpp_build");
        return;
    }

    let qt = detect_qt6().unwrap_or_else(|| {
        panic!(
            "未找到 Qt6（>= 6.7）。请设置 QMAKE 或 QT_ROOT，或把 qmake.exe 加入 PATH。\
             需要 MSVC x64 版 Qt6 与 Rust x86_64-pc-windows-msvc。"
        )
    });
    println!(
        "cargo:warning=Windows Qt binding selected Qt {} via {}",
        qt.version, qt.source
    );

    let qmetaobject_dir = qmetaobject_include_dir();
    println!(
        "cargo:rerun-if-changed={}",
        qmetaobject_dir.join("qmetaobject_rust.hpp").display()
    );

    let mut config = cpp_build::Config::new();
    config.include(&qmetaobject_dir);
    for include in &qt.include_paths {
        config.include(include);
    }
    config.flag(CPP_STANDARD_FLAG);
    config.file("src/windows_overrides/windows_appearance.cpp");
    config.build("src/lib.rs");
}

/// 把 `apps/Linux_qt/src` 镜像到 `src/shared`，再用 Windows 覆盖文件替换
/// 平台差异模块。镜像目录由 .gitignore 忽略，构建时始终从唯一共享源刷新。
fn prepare_shared_sources() {
    let linux_src = Path::new(LINUX_QT_DIR).join("src");
    let dst = Path::new("src/shared");
    println!("cargo:rerun-if-changed={}", linux_src.display());
    println!("cargo:rerun-if-changed=src/windows_overrides");
    let _ = fs::remove_dir_all(dst);
    copy_rs_tree(&linux_src, dst).unwrap_or_else(|e| panic!("镜像 Linux 源码失败：{e}"));

    for (from, to) in [
        ("src/windows_overrides/shared_mod.rs", "src/shared/mod.rs"),
        (
            "src/windows_overrides/platform_utils.rs",
            "src/shared/platform_utils.rs",
        ),
        (
            "src/windows_overrides/platform/desktop_settings.rs",
            "src/shared/platform/linux_qt/desktop_settings.rs",
        ),
        (
            "src/windows_overrides/platform/runtime_environment.rs",
            "src/shared/platform/linux_qt/runtime_environment.rs",
        ),
        (
            "src/windows_overrides/agent_runtime/workspace_access.rs",
            "src/shared/agent_runtime/workspace_access.rs",
        ),
        (
            "src/windows_overrides/agent_runtime/mcp_binding.rs",
            "src/shared/agent_runtime/mcp_binding.rs",
        ),
        (
            "src/windows_overrides/editor/platform.rs",
            "src/shared/editor/input/platform.rs",
        ),
        (
            "src/windows_overrides/editor/windows_input_surface.rs",
            "src/shared/editor/input/platform/windows/input_surface.rs",
        ),
        (
            "src/windows_overrides/native/mod.rs",
            "src/shared/native/mod.rs",
        ),
        (
            "src/windows_overrides/native/windows_render_host.rs",
            "src/shared/native/windows_render_host.rs",
        ),
    ] {
        if let Some(parent) = Path::new(to).parent() {
            fs::create_dir_all(parent).expect("创建共享覆盖目录失败");
        }
        fs::copy(from, to).unwrap_or_else(|e| panic!("覆盖 {to} 失败：{e}"));
    }
}

fn copy_rs_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            copy_rs_tree(&path, &dst.join(entry.file_name()))?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            fs::copy(&path, dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// 生成 `$OUT_DIR/qrc_resources.rs`。
///
/// qmetaobject 的 qrc! 宏按 Cargo.toml 所在目录解析物理路径；这里把
/// Linux_qt 的通用 QML/JS、Windows 专有 QML、着色器、图标和翻译统一登记到
/// 同一个虚拟根路径。Windows qml 中同名 alias 优先，避免通用文件覆盖平台文件。
fn generate_qrc_resources() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR 未设置"));
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut aliases = std::collections::BTreeSet::new();

    // Windows 专有 QML：递归登记，保持 org/kde/... 模块目录结构。
    collect_files(Path::new("qml"), &mut |path| {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        if !matches!(ext, "qml" | "js" | "qmldir") {
            return;
        }
        let rel = path
            .strip_prefix("qml")
            .expect("qml 路径前缀")
            .to_string_lossy()
            .replace('\\', "/");
        push_entry(&mut entries, &mut aliases, path, &rel);
    });

    // Linux 通用 QML：只登记目录顶层文件（与 Linux main.rs 的 qrc 清单一致）。
    let linux_qml = Path::new(LINUX_QT_DIR).join("qml");
    collect_top_level_files(&linux_qml, &mut |path| {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        if !matches!(ext, "qml" | "js") {
            return;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name.starts_with("tst_") {
            return;
        }
        push_entry(&mut entries, &mut aliases, path, &format!("{name}"));
    });

    // 星图 QSG 着色器（qsb 已包含 HLSL/D3D11 变体）。
    for name in [
        "starmap_edge.vert.qsb",
        "starmap_edge.frag.qsb",
        "starmap_object.vert.qsb",
        "starmap_object.frag.qsb",
        "starmap_grid.vert.qsb",
        "starmap_grid.frag.qsb",
    ] {
        let path = Path::new(LINUX_QT_DIR)
            .join("src/native/shaders")
            .join(name);
        push_entry(
            &mut entries,
            &mut aliases,
            &path,
            &format!("shaders/{name}"),
        );
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let icon = Path::new(LINUX_QT_DIR).join("resources/icons/sujian.svg");
    push_entry(&mut entries, &mut aliases, &icon, "icons/sujian.svg");
    println!("cargo:rerun-if-changed={}", icon.display());

    let translation = Path::new(LINUX_QT_DIR).join("i18n/zh_CN.qm");
    push_entry(&mut entries, &mut aliases, &translation, "i18n/zh_CN.qm");
    println!("cargo:rerun-if-changed={}", translation.display());

    let mut generated = String::from(
        "// 由 apps/windows/qt/build.rs 生成。\n\
         qmetaobject::qrc!(qml_resources, \"/\" {\n",
    );
    for (path, alias) in &entries {
        generated.push_str(&format!("    \"{}\" as \"{}\",\n", path, alias));
    }
    generated.push_str("});\n");

    let output = out_dir.join("qrc_resources.rs");
    fs::write(&output, generated).expect("写入 qrc_resources.rs 失败");
    println!("cargo:rerun-if-changed=build.rs");
}

fn push_entry(
    entries: &mut Vec<(String, String)>,
    aliases: &mut std::collections::BTreeSet<String>,
    path: &Path,
    alias: &str,
) {
    if !path.is_file() {
        panic!("QRC 资源不存在：{}", path.display());
    }
    if !aliases.insert(alias.to_string()) {
        panic!("QRC alias 冲突：{alias}");
    }
    println!("cargo:rerun-if-changed={}", path.display());
    entries.push((path.to_string_lossy().replace('\\', "/"), alias.to_string()));
}

fn collect_top_level_files(dir: &Path, f: &mut impl FnMut(&Path)) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            f(&path);
        }
    }
}

fn collect_files(dir: &Path, f: &mut impl FnMut(&Path)) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, f);
        } else {
            f(&path);
        }
    }
}

fn emit_qsb_rerun() {
    let qsb = find_qsb_tool();
    for name in [
        "starmap_edge.vert.qsb",
        "starmap_edge.frag.qsb",
        "starmap_object.vert.qsb",
        "starmap_object.frag.qsb",
        "starmap_grid.vert.qsb",
        "starmap_grid.frag.qsb",
    ] {
        let path = Path::new(LINUX_QT_DIR)
            .join("src/native/shaders")
            .join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let Some(qsb) = qsb.as_ref() else {
            println!(
                "cargo:warning=未找到 qsb 工具，跳过 {} 的 HLSL 变体校验",
                path.display()
            );
            continue;
        };
        let output = Command::new(qsb).arg("-d").arg(&path).output();
        let Ok(output) = output else {
            println!(
                "cargo:warning=无法运行 qsb -d 校验 {}，跳过",
                path.display()
            );
            continue;
        };
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if output.status.success() && text.contains("HLSL") {
            continue;
        }
        panic!(
            "{} 缺少 HLSL/D3D11 变体，Windows Qt Quick 无法使用。\
             请在 Windows Qt 环境用 `qsb -b --qt6` 重新生成该着色器。",
            path.display()
        );
    }
    println!("cargo:rerun-if-env-changed=QSB");
}

fn find_qsb_tool() -> Option<PathBuf> {
    if let Ok(value) = std::env::var("QSB") {
        let path = PathBuf::from(value.trim());
        if path.is_file() {
            return Some(path);
        }
    }
    let mut candidates = Vec::new();
    if let Ok(root) = std::env::var("QT_ROOT") {
        let root = PathBuf::from(root);
        candidates.push(root.join("bin/qsb.exe"));
        candidates.push(root.join("bin/qsb"));
    }
    for name in ["qsb.exe", "qsb"] {
        if let Some(found) = find_in_path(name) {
            candidates.push(found);
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

struct Qt6BuildInfo {
    source: &'static str,
    version: String,
    include_paths: Vec<PathBuf>,
}

fn detect_qt6() -> Option<Qt6BuildInfo> {
    let mut candidates = Vec::new();
    if let Ok(value) = std::env::var("QMAKE") {
        candidates.push(PathBuf::from(value));
    }
    if let Ok(root) = std::env::var("QT_ROOT") {
        candidates.push(PathBuf::from(root).join("bin/qmake.exe"));
    }
    if let Ok(root) = std::env::var("QTDIR") {
        candidates.push(PathBuf::from(root).join("bin/qmake.exe"));
    }
    for name in ["qmake.exe", "qmake6.exe", "qmake"] {
        if let Some(found) = find_in_path(name) {
            candidates.push(found);
        }
    }

    for qmake in candidates {
        if !qmake.is_file() {
            continue;
        }
        let version = qmake_query(&qmake, "QT_VERSION")?;
        if !qt_version_meets_minimum(&version) {
            continue;
        }
        let headers = PathBuf::from(qmake_query(&qmake, "QT_INSTALL_HEADERS")?);
        let mut include_paths = vec![headers.clone()];
        for module in QT6_MODULES {
            let path = headers.join(format!("Qt{module}"));
            if path.is_dir() {
                include_paths.push(path);
            }
        }
        if include_paths.is_empty() {
            continue;
        }
        return Some(Qt6BuildInfo {
            source: "qmake",
            version,
            include_paths,
        });
    }
    None
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn qmake_query(qmake: &Path, key: &str) -> Option<String> {
    let output = Command::new(qmake).args(["-query", key]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn qt_version_meets_minimum(version: &str) -> bool {
    let numeric = version.split(['-', '+']).next().unwrap_or(version);
    let mut parts = numeric.split('.');
    let major: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Version::new(major, minor, patch) >= Version::new(6, 7, 0)
}

fn qmetaobject_include_dir() -> PathBuf {
    let mut roots = Vec::new();
    if let Ok(cargo_home) = std::env::var("CARGO_HOME") {
        roots.push(PathBuf::from(cargo_home).join("registry/src"));
    }
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        roots.push(PathBuf::from(home).join(".cargo/registry/src"));
    }
    for root in roots {
        if let Some(found) = find_file_recursive(&root, "qmetaobject_rust.hpp", 4) {
            return found
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
        }
    }
    panic!("找不到 qmetaobject_rust.hpp。请确认 qmetaobject 0.2.10 已下载到 CARGO_HOME registry。");
}

fn find_file_recursive(root: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 || !root.is_dir() {
        return None;
    }
    for entry in fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file_recursive(&path, name, depth - 1) {
                return Some(found);
            }
        } else if path.file_name().is_some_and(|n| n == name) {
            return Some(path);
        }
    }
    None
}

fn inject_build_env_vars() {
    println!("cargo:rerun-if-env-changed=SUJIAN_GIT_COMMIT_SHA");
    println!("cargo:rerun-if-env-changed=SUJIAN_PACKAGE_TYPE");

    let git_sha = std::env::var("SUJIAN_GIT_COMMIT_SHA")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "unknown".to_string())
        });
    let build_profile = if std::env::var("PROFILE").as_deref() == Ok("release") {
        "release"
    } else {
        "debug"
    };
    if git_sha == "unknown" && build_profile == "release" {
        panic!(
            "正式 release 包必须有有效 git sha，但 git_sha 为 \"unknown\"。\
             请通过 SUJIAN_GIT_COMMIT_SHA 显式传入有效 sha。"
        );
    }
    let rustc_version = Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let package_type = std::env::var("SUJIAN_PACKAGE_TYPE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "windows".to_string());
    let app_version = env!("CARGO_PKG_VERSION");
    let build_key = format!(
        "{}-{}-{}-{}",
        app_version, git_sha, package_type, build_profile
    );

    println!("cargo:rustc-env=GIT_COMMIT_SHA={git_sha}");
    println!("cargo:rustc-env=BUILD_PROFILE={build_profile}");
    println!("cargo:rustc-env=RUSTC_VERSION={rustc_version}");
    println!("cargo:rustc-env=PACKAGE_TYPE={package_type}");
    println!("cargo:rustc-env=BUILD_KEY={build_key}");
}
