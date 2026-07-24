//! # Linux 平台适配层
//!
//! 提供 Linux 桌面端的平台初始化、目录解析、配置存储、同步传输和最终库组装。
//!
//! ## 职责
//!
//! - 按 XDG Base Directory 规范解析应用目录
//! - 构造 `PlatformInit` 并注入 Core
//! - 使用 `writer_platform_api::FileConfigStore` 提供配置存储
//! - 通过 `ReqwestSyncTransport` 提供同步 HTTP 传输（尊重系统/环境代理）
//! - 组装最终 `cdylib`：包含通用核心、Linux 适配和 UniFFI 元数据
//!
//! ## 依赖方向
//!
//! ```text
//! Linux Qt 应用 → writer-platform-linux (cdylib) → writer_uniffi → writer_core + writer_platform_api
//! ```

#[allow(unused_imports)]
use writer_uniffi::WriterAppService;

use std::path::PathBuf;
use writer_platform_api::{FileConfigStore, HttpRequest, HttpResponse, NetworkState, PlatformInit, PlatformKind, PlatformServices, SecureStorage, SyncTransport, TransportError};

const APP_NAMESPACE: &str = "sujian";

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

pub fn create_platform_services() -> PlatformServices {
    let init = resolve_platform_init();
    let config_dir = xdg_config_dir();
    let config_store: Option<Box<dyn writer_platform_api::ConfigStore>> =
        Some(Box::new(FileConfigStore::new(config_dir)));

    #[cfg(feature = "github-api")]
    let sync_transport_factory: Option<writer_platform_api::SyncTransportFactory> = {
        let factory: writer_platform_api::SyncTransportFactory =
            std::sync::Arc::new(|| -> Box<dyn SyncTransport> {
                let transport = ReqwestSyncTransport::new()
                    .unwrap_or_else(|e| panic!("Failed to create Linux SyncTransport: {}", e.message));
                Box::new(transport)
            });
        Some(factory)
    };
    #[cfg(not(feature = "github-api"))]
    let sync_transport_factory: Option<writer_platform_api::SyncTransportFactory> = None;

    PlatformServices {
        init,
        config_store,
        secure_storage: Some(Box::new(LinuxFileSecureStorage::new(xdg_config_dir()))),
        network_state: Some(detect_network_state()),
        sync_transport_factory,
    }
}

pub fn xdg_config_dir() -> PathBuf {
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(config_home).join("writer")
    } else {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".config").join("writer"))
            .unwrap_or_else(|| PathBuf::from(".config/writer"))
    }
}

pub fn xdg_cache_dir() -> PathBuf {
    if let Some(cache_home) = std::env::var_os("XDG_CACHE_HOME") {
        PathBuf::from(cache_home).join("writer")
    } else {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".cache").join("writer"))
            .unwrap_or_else(|| PathBuf::from(".cache/writer"))
    }
}

fn xdg_state_dir() -> PathBuf {
    if let Some(state_home) = std::env::var_os("XDG_STATE_HOME") {
        PathBuf::from(state_home).join(APP_NAMESPACE)
    } else {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".local/state").join(APP_NAMESPACE))
            .unwrap_or_else(|| PathBuf::from(".local/state").join(APP_NAMESPACE))
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

fn detect_network_state() -> NetworkState {
    NetworkState {
        is_connected: true,
        is_metered: false,
        proxy_host: std::env::var("http_proxy")
            .or_else(|_| std::env::var("HTTP_PROXY"))
            .or_else(|_| std::env::var("https_proxy"))
            .or_else(|_| std::env::var("HTTPS_PROXY"))
            .ok()
            .and_then(|url| {
                let url = url.strip_prefix("http://")
                    .or_else(|| url.strip_prefix("https://"))
                    .unwrap_or(&url);
                let without_auth = url.split('@').next_back().unwrap_or(url);
                let host = without_auth.split(':').next().unwrap_or("");
                let host = host.trim();
                if host.is_empty() {
                    None
                } else {
                    Some(host.to_string())
                }
            }),
        proxy_port: std::env::var("http_proxy")
            .or_else(|_| std::env::var("HTTP_PROXY"))
            .or_else(|_| std::env::var("https_proxy"))
            .or_else(|_| std::env::var("HTTPS_PROXY"))
            .ok()
            .and_then(|url| {
                let url = url.strip_prefix("http://")
                    .or_else(|| url.strip_prefix("https://"))
                    .unwrap_or(&url);
                let without_auth = url.split('@').next_back().unwrap_or(url);
                without_auth.split(':')
                    .nth(1)
                    .and_then(|s| s.trim().parse::<u16>().ok())
            }),
    }
}

struct LinuxFileSecureStorage {
    storage_dir: PathBuf,
}

impl LinuxFileSecureStorage {
    fn new(storage_dir: PathBuf) -> Self {
        Self { storage_dir }
    }

    fn secret_path(&self, key: &str) -> PathBuf {
        self.storage_dir.join("secrets").join(format!("{}.bin", key))
    }
}

impl SecureStorage for LinuxFileSecureStorage {
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
