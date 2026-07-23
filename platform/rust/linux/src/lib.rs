//! # Linux 平台适配层
//!
//! 提供 Linux 桌面端的平台初始化、目录解析、配置存储和同步传输。
//!
//! ## 职责
//!
//! - 按 XDG Base Directory 规范解析应用目录
//! - 构造 `PlatformInit` 并注入 Core
//! - 使用 `writer_platform_api::FileConfigStore` 提供配置存储
//! - 通过 `writer_core::ReqwestSyncTransport` 提供同步 HTTP 传输
//!
//! ## 依赖方向
//!
//! ```text
//! Linux Qt 应用 → writer-platform-linux → writer_core + writer_platform_api
//! ```

use std::path::PathBuf;
use writer_platform_api::{FileConfigStore, PlatformInit, PlatformKind};

pub fn resolve_platform_init() -> PlatformInit {
    let config_dir = xdg_config_dir();
    let app_data_dir = config_dir.clone();
    let cache_dir = xdg_cache_dir();
    let log_dir = cache_dir.join("writer").join("log");

    PlatformInit {
        platform: PlatformKind::Desktop,
        app_data_dir,
        cache_dir,
        log_dir,
        no_backup_dir: None,
        device_id: derive_device_id(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        locale: std::env::var("LANG")
            .unwrap_or_else(|_| "en_US.UTF-8".to_string())
            .split('.')
            .next()
            .unwrap_or("en_US")
            .to_string(),
        timezone: local_timezone(),
    }
}

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

fn derive_device_id() -> String {
    let machine_id = std::fs::read_to_string("/etc/machine-id")
        .or_else(|_| std::fs::read_to_string("/var/lib/dbus/machine-id"))
        .unwrap_or_default();
    let trimmed = machine_id.trim().to_string();
    if !trimmed.is_empty() {
        return trimmed;
    }
    uuid::Uuid::new_v4().to_string()
}

fn local_timezone() -> String {
    std::fs::read_link("/etc/localtime")
        .ok()
        .and_then(|path| {
            path.to_str()
                .and_then(|s| s.split("zoneinfo/").nth(1).map(|s| s.to_string()))
        })
        .unwrap_or_else(|| "UTC".to_string())
}

pub fn init_default_config_store() {
    let config_dir = xdg_config_dir();
    let store = FileConfigStore::new(config_dir);
    writer_core::app_config::set_default_config_store(Box::new(store));
}

#[cfg(feature = "github-api")]
pub fn create_sync_transport() -> Box<dyn writer_platform_api::SyncTransport> {
    Box::new(writer_core::sync::github_backend::ReqwestSyncTransport::new().expect("Failed to create HTTP transport"))
}
