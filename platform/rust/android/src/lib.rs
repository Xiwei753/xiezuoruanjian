//! # Android 平台适配层
//!
//! 提供 Android 端的平台初始化、配置存储、同步传输和最终库组装。
//!
//! ## 职责
//!
//! - 接收 Kotlin 层传入的 Context 目录信息构造 `PlatformInit`
//! - 使用 `writer_platform_api::FileConfigStore` 提供配置存储
//! - 通过 `writer_core::ReqwestSyncTransport` 提供同步 HTTP 传输
//! - 组装最终 `cdylib`：包含通用核心、Android 适配和 UniFFI 元数据
//!
//! ## 依赖方向
//!
//! ```text
//! Kotlin/Compose → writer-platform-android (cdylib) → writer_uniffi → writer_core + writer_platform_api
//! ```

#[allow(unused_imports)]
use writer_uniffi::WriterAppService;

use std::path::PathBuf;
use writer_platform_api::{FileConfigStore, PlatformInit, PlatformKind};

pub fn create_platform_init(
    files_dir: PathBuf,
    cache_dir: PathBuf,
    no_backup_dir: PathBuf,
    device_id: String,
    app_version: String,
    locale: String,
    timezone: String,
) -> PlatformInit {
    let log_dir = cache_dir.join("log");
    PlatformInit {
        platform: PlatformKind::Android,
        app_data_dir: files_dir,
        cache_dir,
        log_dir,
        no_backup_dir: Some(no_backup_dir),
        device_id,
        app_version,
        locale,
        timezone,
    }
}

pub fn init_default_config_store(config_dir: PathBuf) {
    let store = FileConfigStore::new(config_dir);
    writer_core::app_config::set_default_config_store(Box::new(store));
}

pub fn create_sync_transport() -> Result<Box<dyn writer_platform_api::SyncTransport>, writer_platform_api::TransportError> {
    let transport = writer_core::sync::github_backend::ReqwestSyncTransport::new()
        .map_err(|e| writer_platform_api::TransportError::new("init", e.to_string()))?;
    Ok(Box::new(transport))
}

pub fn init_platform(platform_init: PlatformInit, config_dir: PathBuf) {
    init_default_config_store(config_dir);
    let _ = platform_init;
}
