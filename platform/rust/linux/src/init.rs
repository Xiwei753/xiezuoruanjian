//! Linux 平台初始化与设备 ID 派生。

use writer_platform_api::{FileConfigStore, PlatformInit, PlatformKind};

use super::dirs::{xdg_cache_dir, xdg_config_dir, xdg_state_dir};

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
        device_id: derive_or_load_device_id(),
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
    let tmp_path = device_id_path.with_extension("tmp");
    if std::fs::write(&tmp_path, &new_id).is_ok() {
        let _ = std::fs::rename(&tmp_path, &device_id_path);
    }
    new_id
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
