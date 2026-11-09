//! Windows 用户目录解析。
//!
//! 配置与缓存使用 `%APPDATA%` / `%LOCALAPPDATA%`，诊断目录沿用
//! `%USERPROFILE%\.sujianxiezuo`，与 Linux/macOS 的可见支持目录保持一致。

use std::path::PathBuf;

const APP_NAMESPACE: &str = "sujian";

/// 应用在用户目录下的根目录名（`%USERPROFILE%\.sujianxiezuo`）。
const SUJIAN_HOME_DIR_NAME: &str = ".sujianxiezuo";

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub fn xdg_config_dir() -> PathBuf {
    env_path("APPDATA")
        .or_else(|| env_path("USERPROFILE").map(|home| home.join("AppData/Roaming")))
        .unwrap_or_else(|| PathBuf::from("AppData/Roaming"))
        .join("writer")
}

pub fn xdg_cache_dir() -> PathBuf {
    env_path("LOCALAPPDATA")
        .or_else(|| env_path("USERPROFILE").map(|home| home.join("AppData/Local")))
        .unwrap_or_else(|| PathBuf::from("AppData/Local"))
        .join("writer")
}

pub(crate) fn xdg_state_dir() -> PathBuf {
    env_path("LOCALAPPDATA")
        .or_else(|| env_path("USERPROFILE").map(|home| home.join("AppData/Local")))
        .unwrap_or_else(|| PathBuf::from("AppData/Local"))
        .join(APP_NAMESPACE)
}

/// 应用根目录 `%USERPROFILE%\.sujianxiezuo`。
pub fn sujian_home_dir() -> PathBuf {
    env_path("USERPROFILE")
        .map(|home| home.join(SUJIAN_HOME_DIR_NAME))
        .unwrap_or_else(|| PathBuf::from(SUJIAN_HOME_DIR_NAME))
}

pub fn diagnostics_dir() -> PathBuf {
    sujian_home_dir().join("diagnostics")
}

pub fn startup_diagnostics_dir() -> PathBuf {
    diagnostics_dir().join("startup")
}

pub fn runtime_log_dir() -> PathBuf {
    diagnostics_dir().join("runtime")
}

pub fn crash_diagnostics_dir() -> PathBuf {
    diagnostics_dir().join("crash")
}

pub fn diagnostics_export_dir() -> PathBuf {
    diagnostics_dir().join("exports")
}
