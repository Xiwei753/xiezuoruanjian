//! # Android 平台适配层
//!
//! 提供 Android 端的平台初始化、配置存储、同步传输和最终库组装。
//!
//! ## 职责
//!
//! - 接收 Kotlin 层传入的 Context 目录信息构造 `PlatformInit`
//! - 使用 `writer_platform_api::FileConfigStore` 提供配置存储
//! - 通过 `ReqwestSyncTransport` 提供同步 HTTP 传输
//! - 通过 `AndroidEncryptedSecureStorage` 提供 AES‑256‑GCM 加密的安全存储
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
use std::sync::OnceLock;
use writer_platform_api::{
    FileConfigStore, HttpRequest, HttpResponse, NetworkState, PlatformInit, PlatformKind,
    PlatformServices, PlatformServicesResolver, SecureStorage, SyncTransport, TransportError,
    register_platform_services_resolver,
};

struct AndroidPlatformServicesResolver;

impl PlatformServicesResolver for AndroidPlatformServicesResolver {
    fn resolve(&self, init: &PlatformInit, network_state: &NetworkState) -> PlatformServices {
        create_platform_services(init.clone(), network_state.is_connected, network_state.is_metered)
    }
}

static ANDROID_RESOLVER_REGISTERED: OnceLock<()> = OnceLock::new();

pub fn ensure_android_resolver_registered() {
    ANDROID_RESOLVER_REGISTERED.get_or_init(|| {
        register_platform_services_resolver(Box::new(AndroidPlatformServicesResolver));
    });
}

#[::ctor::ctor]
fn auto_register_android_resolver() {
    ensure_android_resolver_registered();
}

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

/// AES‑256‑GCM 加密的安全存储。
///
/// 加密密钥存储在 `no_backup_dir/.enc_key`，密文存储在 `no_backup_dir/secrets/`。
/// 每个 secret 使用独立的随机 12 字节 nonce。
struct AndroidEncryptedSecureStorage {
    storage_dir: PathBuf,
    key: [u8; 32],
}

impl AndroidEncryptedSecureStorage {
    fn new(no_backup_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(no_backup_dir.join("secrets"))
            .map_err(|e| format!("create secrets dir: {}", e))?;
        let key_path = no_backup_dir.join(".enc_key");
        let key = if key_path.exists() {
            let data = std::fs::read(&key_path)
                .map_err(|e| format!("read enc_key: {}", e))?;
            if data.len() != 32 {
                return Err(format!("enc_key has unexpected length: {}", data.len()));
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&data);
            arr
        } else {
            let mut arr = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut arr);
            std::fs::write(&key_path, arr)
                .map_err(|e| format!("write enc_key: {}", e))?;
            arr
        };
        Ok(Self { storage_dir: no_backup_dir.join("secrets"), key })
    }
}

impl writer_platform_api::SecureStorage for AndroidEncryptedSecureStorage {
    fn get_secret(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        use aes_gcm::aead::Aead;
        use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};

        let path = self.storage_dir.join(format!("{}.enc", key));
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("read secret: {}", e)),
        };
        if data.len() < 12 {
            return Err("truncated ciphertext".into());
        }
        let (nonce_bytes, ciphertext) = data.split_at(12);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key));
        let nonce = Nonce::from_slice(nonce_bytes);
        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| format!("decrypt failed: {}", e))?;
        Ok(Some(plaintext))
    }

    fn set_secret(&self, key: &str, value: &[u8]) -> Result<(), String> {
        use aes_gcm::aead::Aead;
        use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};

        let mut nonce_bytes = [0u8; 12];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce_bytes);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key));
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, value)
            .map_err(|e| format!("encrypt failed: {}", e))?;
        let mut out = Vec::with_capacity(12 + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        std::fs::create_dir_all(&self.storage_dir)
            .map_err(|e| format!("create secrets dir: {}", e))?;
        let path = self.storage_dir.join(format!("{}.enc", key));
        std::fs::write(&path, &out)
            .map_err(|e| format!("write secret: {}", e))?;
        Ok(())
    }

    fn delete_secret(&self, key: &str) -> Result<(), String> {
        let path = self.storage_dir.join(format!("{}.enc", key));
        match std::fs::remove_file(&path) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("delete secret: {}", e)),
        }
    }
}

pub fn create_platform_services(
    platform_init: PlatformInit,
    is_connected: bool,
    is_metered: bool,
) -> PlatformServices {
    let config_dir = platform_init.app_data_dir.join("config");

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

    let secure_storage: Option<Box<dyn SecureStorage>> = platform_init
        .no_backup_dir
        .as_ref()
        .and_then(|dir| AndroidEncryptedSecureStorage::new(dir.clone()).ok())
        .map(|s| Box::new(s) as Box<dyn SecureStorage>);

    PlatformServices {
        init: platform_init,
        config_store,
        secure_storage,
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

