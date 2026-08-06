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
use std::sync::{Mutex, OnceLock};
use writer_platform_api::{
    FileConfigStore, HttpRequest, HttpResponse, NetworkState, PlatformInit, PlatformKind,
    PlatformServices, SecureStorage, SyncTransport, TransportError,
};

const APP_NAMESPACE: &str = "sujian";

static CACHED_NETWORK_STATE: OnceLock<Mutex<NetworkState>> = OnceLock::new();

fn get_or_init_cache() -> &'static Mutex<NetworkState> {
    CACHED_NETWORK_STATE.get_or_init(|| Mutex::new(NetworkState::default()))
}

fn cache_network_state(state: &NetworkState) {
    if let Ok(mut guard) = get_or_init_cache().lock() {
        *guard = state.clone();
    }
}

pub fn get_cached_network_state() -> NetworkState {
    CACHED_NETWORK_STATE
        .get()
        .and_then(|m| m.lock().ok())
        .map(|g| g.clone())
        .unwrap_or_default()
}

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
            std::sync::Arc::new(|| -> Result<Box<dyn SyncTransport>, TransportError> {
                ReqwestSyncTransport::new().map(|t| Box::new(t) as Box<dyn SyncTransport>)
            });
        Some(factory)
    };
    #[cfg(not(feature = "github-api"))]
    let sync_transport_factory: Option<writer_platform_api::SyncTransportFactory> = None;

    let secure_storage: Option<Box<dyn SecureStorage>> = create_secure_storage();

    let network_state = detect_network_state();
    cache_network_state(&network_state);

    PlatformServices {
        init,
        config_store,
        secure_storage,
        network_state: Some(network_state),
        sync_transport_factory,
    }
}

fn create_secure_storage() -> Option<Box<dyn SecureStorage>> {
    #[cfg(feature = "secret-service")]
    {
        match KeyringSecureStorage::new() {
            Ok(storage) => return Some(Box::new(storage)),
            Err(e) => {
                eprintln!(
                    "Warning: Secret Service unavailable, secure storage disabled: {}",
                    e
                );
            }
        }
    }
    #[cfg(not(feature = "secret-service"))]
    {
        eprintln!("Warning: Secret Service support not compiled, secure storage disabled");
    }
    None
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

pub fn refresh_network_state() -> NetworkState {
    let state = detect_network_state();
    cache_network_state(&state);
    state
}

fn detect_network_state() -> NetworkState {
    let (is_connected, is_metered) = detect_connectivity_and_metered();
    NetworkState {
        is_connected,
        is_metered,
        proxy_host: std::env::var("http_proxy")
            .or_else(|_| std::env::var("HTTP_PROXY"))
            .or_else(|_| std::env::var("https_proxy"))
            .or_else(|_| std::env::var("HTTPS_PROXY"))
            .ok()
            .and_then(|url| {
                let url = url
                    .strip_prefix("http://")
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
                let url = url
                    .strip_prefix("http://")
                    .or_else(|| url.strip_prefix("https://"))
                    .unwrap_or(&url);
                let without_auth = url.split('@').next_back().unwrap_or(url);
                without_auth
                    .split(':')
                    .nth(1)
                    .and_then(|s| s.trim().parse::<u16>().ok())
            }),
    }
}

fn detect_connectivity_and_metered() -> (bool, bool) {
    if let Ok(output) = std::process::Command::new("nmcli")
        .args(["-t", "-f", "STATE", "general", "status"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let is_connected = stdout
                .lines()
                .any(|line| line.trim() == "connected" || line.starts_with("connected"));
            let is_metered = check_nmcli_metered();
            return (is_connected, is_metered);
        }
    }
    (check_network_connectivity(), false)
}

fn check_nmcli_metered() -> bool {
    if let Ok(output) = std::process::Command::new("nmcli")
        .args(["-t", "-f", "METERED", "dev", "show"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Some(value) = line.strip_prefix("METERED:") {
                    let v = value.trim();
                    return v == "yes" || v == "guess-yes";
                }
            }
        }
    }
    false
}

fn check_network_connectivity() -> bool {
    if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str == "lo" {
                continue;
            }
            let operstate_path = std::path::Path::new("/sys/class/net")
                .join(name_str.as_ref())
                .join("operstate");
            if let Ok(state) = std::fs::read_to_string(&operstate_path) {
                let state = state.trim();
                if state == "up" {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(feature = "secret-service")]
struct KeyringSecureStorage;

#[cfg(feature = "secret-service")]
impl KeyringSecureStorage {
    fn new() -> Result<Self, String> {
        let test_entry = keyring::Entry::new("com.xiwei.sujian", "sujian.__availability_check__")
            .map_err(|e| format!("Secret Service unavailable: {}", e))?;
        match test_entry.get_secret() {
            Ok(_) => Ok(Self),
            Err(keyring::Error::NoEntry) => Ok(Self),
            Err(e) => Err(format!("Secret Service unavailable: {}", e)),
        }
    }

    fn entry_for_key(&self, key: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new("com.xiwei.sujian", &format!("sujian.{}", key))
            .map_err(|e| format!("Failed to create keyring entry: {}", e))
    }
}

#[cfg(feature = "secret-service")]
impl SecureStorage for KeyringSecureStorage {
    fn get_secret(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        let entry = self.entry_for_key(key)?;
        match entry.get_secret() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => {
                let migrated = self.migrate_from_plaintext(key)?;
                Ok(migrated)
            }
            Err(e) => Err(format!("Failed to get secret: {}", e)),
        }
    }

    fn set_secret(&self, key: &str, value: &[u8]) -> Result<(), String> {
        let entry = self.entry_for_key(key)?;
        entry
            .set_secret(value)
            .map_err(|e| format!("Failed to set secret: {}", e))
    }

    fn delete_secret(&self, key: &str) -> Result<(), String> {
        let entry = self.entry_for_key(key)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("Failed to delete secret: {}", e)),
        }
    }
}

#[cfg(feature = "secret-service")]
impl KeyringSecureStorage {
    fn migrate_from_plaintext(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        let legacy_path = xdg_config_dir()
            .join("secrets")
            .join(format!("{}.bin", key));
        if !legacy_path.exists() {
            return Ok(None);
        }
        let plaintext = std::fs::read(&legacy_path).map_err(|e| e.to_string())?;
        let entry = self.entry_for_key(key)?;
        entry
            .set_secret(&plaintext)
            .map_err(|e| format!("Migration failed: {}", e))?;
        let _ = std::fs::remove_file(&legacy_path);
        Ok(Some(plaintext))
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
            .map_err(|e| {
                TransportError::new("init", format!("Failed to build HTTP client: {}", e))
            })?;
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
        let body = resp
            .bytes()
            .map_err(|e| TransportError::new("response_read", e.to_string()))?;
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
