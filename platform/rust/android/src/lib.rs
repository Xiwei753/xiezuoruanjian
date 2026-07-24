//! # Android 平台适配层
//!
//! 提供 Android 端的平台初始化、配置存储、同步传输和最终库组装。
//!
//! ## 职责
//!
//! - 接收 Kotlin 层传入的 Context 目录信息构造 `PlatformInit`
//! - 使用 `writer_platform_api::FileConfigStore` 提供配置存储
//! - 通过 `ReqwestSyncTransport` 提供同步 HTTP 传输
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
use writer_platform_api::{FileConfigStore, HttpRequest, HttpResponse, NetworkState, PlatformInit, PlatformKind, PlatformServices, SecureStorage, SyncTransport, TransportError};

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

pub fn create_platform_services(
    platform_init: PlatformInit,
    is_connected: bool,
    is_metered: bool,
) -> PlatformServices {
    let config_dir = platform_init.app_data_dir.join("config");
    let no_backup_dir = platform_init.no_backup_dir.clone()
        .unwrap_or_else(|| platform_init.app_data_dir.join("no_backup"));

    let config_store: Option<Box<dyn writer_platform_api::ConfigStore>> =
        Some(Box::new(FileConfigStore::new(config_dir)));

    #[cfg(feature = "github-api")]
    let sync_transport_factory: Option<writer_platform_api::SyncTransportFactory> = {
        let factory: writer_platform_api::SyncTransportFactory =
            std::sync::Arc::new(|| -> Box<dyn SyncTransport> {
                let transport = ReqwestSyncTransport::new()
                    .unwrap_or_else(|e| panic!("Failed to create Android SyncTransport: {}", e.message));
                Box::new(transport)
            });
        Some(factory)
    };
    #[cfg(not(feature = "github-api"))]
    let sync_transport_factory: Option<writer_platform_api::SyncTransportFactory> = None;

    PlatformServices {
        init: platform_init,
        config_store,
        secure_storage: Some(Box::new(AndroidFileSecureStorage::new(no_backup_dir))),
        network_state: Some(NetworkState {
            is_connected,
            is_metered,
            proxy_host: None,
            proxy_port: None,
        }),
        sync_transport_factory,
    }
}

pub fn init_default_config_store(config_dir: PathBuf) {
    let store = FileConfigStore::new(config_dir);
    writer_core::app_config::set_default_config_store(Box::new(store));
}

struct AndroidFileSecureStorage {
    storage_dir: PathBuf,
}

impl AndroidFileSecureStorage {
    fn new(no_backup_dir: PathBuf) -> Self {
        Self {
            storage_dir: no_backup_dir.join("secrets"),
        }
    }

    fn secret_path(&self, key: &str) -> PathBuf {
        self.storage_dir.join(format!("{}.bin", key))
    }
}

impl SecureStorage for AndroidFileSecureStorage {
    fn get_secret(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        let path = self.secret_path(key);
        if !path.exists() {
            return Ok(None);
        }
        std::fs::read(&path).map(Some).map_err(|e| e.to_string())
    }

    fn set_secret(&self, key: &str, value: &[u8]) -> Result<(), String> {
        let path = self.secret_path(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, value).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp_path, &path).map_err(|e| e.to_string())
    }

    fn delete_secret(&self, key: &str) -> Result<(), String> {
        let path = self.secret_path(key);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())
        } else {
            Ok(())
        }
    }
}

#[cfg(feature = "github-api")]
pub struct ReqwestSyncTransport {
    client: reqwest::blocking::Client,
}

#[cfg(feature = "github-api")]
impl ReqwestSyncTransport {
    pub fn new() -> Result<Self, TransportError> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("WriterApp/1.0")
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| TransportError::new("init", format!("Failed to build HTTP client: {}", e)))?;
        Ok(Self { client })
    }
}

#[cfg(feature = "github-api")]
impl SyncTransport for ReqwestSyncTransport {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        let mut req = match request.method.as_str() {
            "GET" => self.client.get(&request.url),
            "PUT" => self.client.put(&request.url),
            "DELETE" => self.client.delete(&request.url),
            "POST" => self.client.post(&request.url),
            "PATCH" => self.client.patch(&request.url),
            "HEAD" => self.client.head(&request.url),
            _ => {
                return Err(TransportError::new(
                    "invalid_method",
                    format!("Unsupported HTTP method: {}", request.method),
                ));
            }
        };

        for (key, value) in &request.headers {
            req = req.header(key.as_str(), value.as_str());
        }

        if let Some(body) = request.body {
            req = req.body(body);
        }

        let resp = req.send().map_err(|e| {
            if e.is_connect() {
                TransportError::new("dns_failed", e.to_string())
            } else if e.is_timeout() {
                TransportError::new("timeout", e.to_string())
            } else {
                TransportError::new("network", e.to_string())
            }
        })?;

        let status = resp.status().as_u16();
        let headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        let body = resp.bytes().map_err(|e| {
            TransportError::new("response_read", e.to_string())
        })?;
        Ok(HttpResponse {
            status,
            headers,
            body: body.to_vec(),
        })
    }
}

#[cfg(feature = "github-api")]
pub fn create_sync_transport() -> Result<Box<dyn SyncTransport>, TransportError> {
    let transport = ReqwestSyncTransport::new()?;
    Ok(Box::new(transport))
}

pub fn init_platform(platform_init: PlatformInit, config_dir: PathBuf) {
    init_default_config_store(config_dir);
    let _ = platform_init;
}
