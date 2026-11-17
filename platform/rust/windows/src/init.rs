//! Windows 平台初始化与设备 ID 派生。

use std::io::Write;

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
        device_id: derive_or_load_device_id(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        locale: windows_locale(),
        timezone: windows_timezone(),
        tls_ca_bundle: None,
        tls_ca_dir: None,
        tls_ca_required: false,
    }
}

fn derive_or_load_device_id() -> String {
    let state_dir = xdg_state_dir();
    let device_id_path = state_dir.join("device_id");

    if let Ok(existing) = std::fs::read_to_string(&device_id_path) {
        let trimmed = existing.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }

    let new_id = uuid::Uuid::new_v4().to_string();

    if let Some(parent) = device_id_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&device_id_path)
    {
        let _ = file.write_all(new_id.as_bytes());
        let _ = file.sync_all();
        return new_id;
    }

    if let Ok(existing) = std::fs::read_to_string(&device_id_path) {
        let trimmed = existing.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }

    let _ = writer_core::storage::atomic_write_bytes(&device_id_path, new_id.as_bytes());

    if let Ok(existing) = std::fs::read_to_string(&device_id_path) {
        let trimmed = existing.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }

    new_id
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
