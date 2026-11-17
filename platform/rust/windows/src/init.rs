//! Windows 平台初始化与设备 ID 派生。

use writer_platform_api::{FileConfigStore, PlatformInit, PlatformKind};

use super::dirs::{runtime_log_dir, xdg_cache_dir, xdg_config_dir, xdg_state_dir};

pub fn resolve_platform_init() -> PlatformInit {
    let config_dir = xdg_config_dir();
    let app_data_dir = config_dir.clone();
    let cache_dir = xdg_cache_dir();
    let log_dir = runtime_log_dir();

    PlatformInit {
        platform: PlatformKind::Windows,
        app_data_dir,
        cache_dir,
        log_dir,
        no_backup_dir: None,
        device_id: super::device_id::load_or_create_device_id(&xdg_state_dir())
            .unwrap_or_else(|e| panic!("Failed to load or create device_id: {e}")),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        locale: windows_locale(),
        timezone: windows_timezone(),
        tls_ca_bundle: None,
        tls_ca_dir: None,
        tls_ca_required: false,
    }
}

fn windows_locale() -> String {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;

    let mut buf = [0u16; 85];
    let len = unsafe { GetUserDefaultLocaleName(&mut buf) };
    if len > 0 {
        let locale = String::from_utf16_lossy(&buf[..(len - 1) as usize]);
        locale.replace('-', "_")
    } else {
        "zh_CN".to_string()
    }
}

fn windows_timezone() -> String {
    iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string())
}

pub fn init_default_config_store() {
    let config_dir = xdg_config_dir();
    let store = FileConfigStore::new(config_dir);
    writer_core::app_config::set_default_config_store(Box::new(store));
}

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
