//! XDG Base Directory 规范的目录解析。
//!
//! 解析应用配置、缓存与状态目录，供平台初始化、配置存储和安全存储使用。
//! 同时提供 `~/.sujianxiezuo` 下的诊断目录解析，供最早期启动记录器使用。

use std::path::PathBuf;

const APP_NAMESPACE: &str = "sujian";

/// 应用在用户 HOME 下的根目录名（`~/.sujianxiezuo`）。
const SUJIAN_HOME_DIR_NAME: &str = ".sujianxiezuo";

pub fn xdg_config_dir() -> PathBuf {
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(config_home).join("writer")
    } else {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".config").join("writer"))
            .unwrap_or_else(|| PathBuf::from(".config/writer"))
    }
}

pub fn xdg_cache_dir() -> PathBuf {
    if let Some(cache_home) = std::env::var_os("XDG_CACHE_HOME") {
        PathBuf::from(cache_home).join("writer")
    } else {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".cache").join("writer"))
            .unwrap_or_else(|| PathBuf::from(".cache/writer"))
    }
}

pub(crate) fn xdg_state_dir() -> PathBuf {
    if let Some(state_home) = std::env::var_os("XDG_STATE_HOME") {
        PathBuf::from(state_home).join(APP_NAMESPACE)
    } else {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".local/state").join(APP_NAMESPACE))
            .unwrap_or_else(|| PathBuf::from(".local/state").join(APP_NAMESPACE))
    }
}

/// 应用根目录 `~/.sujianxiezuo`。
///
/// HOME 必须从运行用户环境解析（`std::env::var_os("HOME")`），不能硬编码
/// `/home/<name>`。HOME 缺失时回退到相对路径，与现有 xdg 函数风格一致。
pub fn sujian_home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(SUJIAN_HOME_DIR_NAME))
        .unwrap_or_else(|| PathBuf::from(SUJIAN_HOME_DIR_NAME))
}

/// 诊断根目录 `~/.sujianxiezuo/diagnostics`。
pub fn diagnostics_dir() -> PathBuf {
    sujian_home_dir().join("diagnostics")
}

/// 启动诊断目录 `~/.sujianxiezuo/diagnostics/startup`。
pub fn startup_diagnostics_dir() -> PathBuf {
    diagnostics_dir().join("startup")
}

/// 运行时日志目录 `~/.sujianxiezuo/diagnostics/runtime`。
pub fn runtime_log_dir() -> PathBuf {
    diagnostics_dir().join("runtime")
}

/// 崩溃诊断目录 `~/.sujianxiezuo/diagnostics/crash`。
pub fn crash_diagnostics_dir() -> PathBuf {
    diagnostics_dir().join("crash")
}

/// 诊断导出目录 `~/.sujianxiezuo/diagnostics/exports`。
pub fn diagnostics_export_dir() -> PathBuf {
    diagnostics_dir().join("exports")
}
