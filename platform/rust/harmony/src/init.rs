//! HarmonyOS 平台初始化构造与诊断后端接入。
//!
//! Harmony 平台通过 NAPI 桥接层把目录信息传进来，构造 `PlatformInit`
//! 并启动统一日志后端。

use std::path::PathBuf;

use writer_platform_api::{PlatformInit, PlatformKind};

/// 从 Harmony 目录信息构造平台初始化结构。
pub fn create_platform_init(
    files_dir: PathBuf,
    cache_dir: PathBuf,
    device_id: String,
    app_version: String,
    locale: String,
    timezone: String,
) -> PlatformInit {
    let log_dir = cache_dir.join("log");
    PlatformInit {
        platform: PlatformKind::Harmony,
        app_data_dir: files_dir,
        cache_dir,
        log_dir,
        no_backup_dir: None,
        device_id,
        app_version,
        locale,
        timezone,
    }
}

/// 把完整 `PlatformInit` 交给 `writer_diagnostics::init`，启动统一日志后端。
///
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
