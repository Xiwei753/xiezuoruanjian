//! Android 平台初始化构造。
//!
//! 接收 Kotlin 层传入的 Context 目录信息构造 `PlatformInit`，
//! 供服务组装与 Core 注入使用。

use std::path::PathBuf;

use writer_platform_api::{PlatformInit, PlatformKind};

/// 从 Android Context 目录信息构造平台初始化结构。
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
