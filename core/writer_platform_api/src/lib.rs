//! # 平台能力契约 (Platform API)
//!
//! 本 crate 定义平台能力契约与初始化参数，是通用业务核心与平台适配层之间的稳定边界。
//!
//! ## 职责
//!
//! - 定义 `PlatformInit`：平台启动时注入的初始化上下文
//! - 定义 `PlatformPaths`：应用数据目录、缓存目录、日志目录等路径
//! - 定义 `ConfigStore`：配置存储契约
//! - 定义 `SecureStorage`：安全存储契约（令牌、凭据）
//! - 定义 `NetworkState`：网络状态信息
//! - 定义 `SyncTransport`：同步传输契约（HTTP 执行与同步协议分离）
//!
//! ## 依赖方向
//!
//! ```text
//! writer_platform_api <- writer_core
//! writer_platform_api <- platform/rust/<target>
//! ```
//!
//! `writer_platform_api` 不依赖 `writer_core` 或任何平台 crate。

mod config_store;
mod network_state;
mod platform_capabilities;
mod platform_init;
mod platform_paths;
mod secure_storage;
mod sync_transport;

pub use config_store::{ConfigStore, FileConfigStore};
pub use network_state::NetworkState;
pub use platform_capabilities::{PlatformCapabilities, PlatformCapabilitiesExt};
pub use platform_init::{PlatformInit, PlatformKind, PlatformPaths};
pub use secure_storage::SecureStorage;
pub use sync_transport::{HttpRequest, HttpResponse, SyncTransport, TransportError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_kind_from_str_name() {
        assert_eq!(PlatformKind::from_str_name("desktop"), Some(PlatformKind::Desktop));
        assert_eq!(PlatformKind::from_str_name("linux"), Some(PlatformKind::Desktop));
        assert_eq!(PlatformKind::from_str_name("linux_qt"), Some(PlatformKind::Desktop));
        assert_eq!(PlatformKind::from_str_name("android"), Some(PlatformKind::Android));
        assert_eq!(PlatformKind::from_str_name("windows"), Some(PlatformKind::Windows));
        assert_eq!(PlatformKind::from_str_name("harmony"), Some(PlatformKind::Harmony));
        assert_eq!(PlatformKind::from_str_name("apple"), Some(PlatformKind::Apple));
        assert_eq!(PlatformKind::from_str_name("unknown"), None);
    }

    #[test]
    fn platform_kind_display() {
        assert_eq!(PlatformKind::Desktop.to_string(), "desktop");
        assert_eq!(PlatformKind::Android.to_string(), "android");
    }

    #[test]
    fn platform_kind_default_is_desktop() {
        assert_eq!(PlatformKind::default(), PlatformKind::Desktop);
    }

    #[test]
    fn platform_init_paths() {
        let init = PlatformInit {
            platform: PlatformKind::Desktop,
            app_data_dir: "/data".into(),
            cache_dir: "/cache".into(),
            log_dir: "/log".into(),
            no_backup_dir: None,
            device_id: "test-device".to_string(),
            app_version: "1.0".to_string(),
            locale: "en_US".to_string(),
            timezone: "UTC".to_string(),
        };
        let paths = init.paths();
        assert_eq!(paths.app_data_dir, std::path::PathBuf::from("/data"));
        assert_eq!(paths.cache_dir, std::path::PathBuf::from("/cache"));
        assert_eq!(paths.config_dir, std::path::PathBuf::from("/data/config"));
    }

    #[test]
    fn platform_init_serialization_camel_case() {
        let init = PlatformInit {
            platform: PlatformKind::Android,
            app_data_dir: "/data".into(),
            cache_dir: "/cache".into(),
            log_dir: "/log".into(),
            no_backup_dir: Some("/nobackup".into()),
            device_id: "dev1".to_string(),
            app_version: "2.0".to_string(),
            locale: "zh_CN".to_string(),
            timezone: "Asia/Shanghai".to_string(),
        };
        let json = serde_json::to_string(&init).unwrap();
        assert!(json.contains("\"appDataDir\""));
        assert!(json.contains("\"cacheDir\""));
        assert!(json.contains("\"noBackupDir\""));
        assert!(json.contains("\"deviceId\""));
        assert!(json.contains("\"appVersion\""));
    }

    #[test]
    fn file_config_store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileConfigStore::new(dir.path().to_path_buf());
        assert!(store.load().unwrap().is_none());
        store.save(b"test data").unwrap();
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded, b"test data");
    }

    #[test]
    fn file_config_store_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileConfigStore::new(dir.path().to_path_buf());
        store.save(b"first").unwrap();
        store.save(b"second").unwrap();
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded, b"second");
    }

    #[test]
    fn network_state_default() {
        let state = NetworkState::default();
        assert!(!state.is_connected);
        assert!(!state.is_metered);
        assert!(state.proxy_host.is_none());
    }

    #[test]
    fn network_state_serialization() {
        let state = NetworkState {
            is_connected: true,
            is_metered: false,
            proxy_host: Some("proxy.example.com".to_string()),
            proxy_port: Some(8080),
        };
        let json = serde_json::to_string(&state).unwrap();
        let restored: NetworkState = serde_json::from_str(&json).unwrap();
        assert!(restored.is_connected);
        assert!(!restored.is_metered);
        assert_eq!(restored.proxy_host, Some("proxy.example.com".to_string()));
        assert_eq!(restored.proxy_port, Some(8080));
    }

    #[test]
    fn platform_capabilities_default_by_kind() {
        let desktop_caps = PlatformKind::Desktop.default_capabilities();
        assert!(desktop_caps.supports_ime_preedit);
        assert!(desktop_caps.supports_text_animation);

        let harmony_caps = PlatformKind::Harmony.default_capabilities();
        assert!(!harmony_caps.supports_text_animation);
        assert!(harmony_caps.supports_clipboard);
    }

    #[test]
    fn transport_error_creation() {
        let err = TransportError::new("network", "connection refused".to_string());
        assert_eq!(err.category, "network");
        assert_eq!(err.message, "connection refused");
    }

    #[test]
    fn http_request_serialization() {
        let req = HttpRequest {
            method: "GET".to_string(),
            url: "https://example.com".to_string(),
            headers: vec![("Authorization".to_string(), "Bearer token".to_string())],
            body: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let restored: HttpRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.method, "GET");
        assert_eq!(restored.headers.len(), 1);
    }
}
