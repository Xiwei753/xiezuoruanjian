//! XDG Base Directory 规范的目录解析。
//!
//! 解析应用配置、缓存与状态目录，供平台初始化、配置存储和安全存储使用。

use std::path::PathBuf;

const APP_NAMESPACE: &str = "sujian";

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
