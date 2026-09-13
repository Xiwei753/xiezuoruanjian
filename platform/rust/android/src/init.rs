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

/// 把完整 `PlatformInit` 交给 `writer_diagnostics::init`，启动统一日志后端。
///
/// 日志目录、平台名、设备 ID、应用版本、locale/timezone 都从这一个入口进来。
/// `build_key` 由调用方传入（来自 BuildConfig），`session_id` 每次启动唯一。
/// 幂等：重复调用无副作用。
pub fn init_diagnostics(
    init: &PlatformInit,
    build_key: String,
    session_id: String,
    enabled: bool,
    verbose: bool,
) {
    let config = writer_diagnostics::DiagnosticsConfig {
        log_dir: init.log_dir.clone(),
        platform_name: init.platform.to_string(),
        device_id: init.device_id.clone(),
        app_version: init.app_version.clone(),
        build_key,
        locale: init.locale.clone(),
        timezone: init.timezone.clone(),
        session_id,
        enabled,
        verbose,
    };
    writer_diagnostics::init(config);
}
