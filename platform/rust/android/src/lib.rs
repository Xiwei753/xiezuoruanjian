//! # Android 平台适配层
//!
//! 提供 Android 端的平台初始化和配置存储。
//!
//! ## 职责
//!
//! - 接收 Kotlin 层传入的 Context 目录信息构造 `PlatformInit`
//! - 提供 `FileConfigStore` 的 Android 路径实现
//!
//! ## 依赖方向
//!
//! ```text
//! Kotlin/Compose → writer-platform-android → writer_core + writer_platform_api
//! ```

use std::path::PathBuf;
use writer_platform_api::{PlatformInit, PlatformKind};

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
    let store = writer_core::app_config::FileConfigStore::new(config_dir);
    writer_core::app_config::set_default_config_store(Box::new(store));
}
