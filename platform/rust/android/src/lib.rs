//! # Android 平台适配层
//!
//! 提供 Android 端的平台初始化、配置存储、同步传输和最终库组装。
//!
//! ## 职责
//!
//! - 接收 Kotlin 层传入的 Context 目录信息构造 `PlatformInit`
//! - 使用 `writer_platform_api::FileConfigStore` 提供配置存储
//! - 通过 `ReqwestSyncTransport` 提供同步 HTTP 传输
//! - 通过 `AndroidEncryptedSecureStorage` 提供安全存储（AES-256-GCM 加密，noBackup 目录）
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
use writer_platform_api::{FileConfigStore, HttpRequest, HttpResponse, NetworkState, PlatformInit, PlatformKind, PlatformServices, PlatformServicesResolver, SecureStorage, SyncTransport, TransportError, register_platform_services_resolver};

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
            std::sync::Arc::new(|| -> Result<Box<dyn SyncTransport>, TransportError> {
                ReqwestSyncTransport::new().map(|t| Box::new(t) as Box<dyn SyncTransport>)
            });
        Some(factory)
    };
    #[cfg(not(feature = "github-api"))]
    let sync_transport_factory: Option<writer_platform_api::SyncTransportFactory> = None;

    PlatformServices {
        init: platform_init,
        config_store,
        secure_storage: match AndroidEncryptedSecureStorage::new(no_backup_dir) {
            Ok(s) => Some(Box::new(s)),
            Err(e) => {
                eprintln!("Warning: Encrypted secure storage unavailable: {}", e);
                None
            }
        },
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

struct AndroidEncryptedSecureStorage {
    storage_dir: PathBuf,
    key: [u8; 32],
}

impl AndroidEncryptedSecureStorage {
    fn new(no_backup_dir: PathBuf) -> Result<Self, String> {
        let storage_dir = no_backup_dir.join("secrets");
        let key_path = no_backup_dir.join(".enc_key");
        let key = if key_path.exists() {
            let raw = std::fs::read(&key_path).map_err(|e| format!("Failed to read encryption key: {}", e))?;
            if raw.len() != 32 {
                return Err("Encryption key is corrupted (wrong length)".to_string());
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&raw);
            key
        } else {
            let mut key = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut key);
            if let Some(parent) = key_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create key dir: {}", e))?;
            }
            let tmp_path = key_path.with_extension("tmp");
            std::fs::write(&tmp_path, key).map_err(|e| format!("Failed to write encryption key: {}", e))?;
            std::fs::rename(&tmp_path, &key_path).map_err(|e| format!("Failed to rename encryption key: {}", e))?;
            key
        };
        Ok(Self { storage_dir, key })
    }

    fn secret_path(&self, key: &str) -> PathBuf {
        self.storage_dir.join(format!("{}.enc", key))
    }

    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
        use aes_gcm::aead::Aead;
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| format!("AES init failed: {}", e))?;
        let nonce_bytes = rand::random::<[u8; 12]>();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher.encrypt(nonce, plaintext)
            .map_err(|e| format!("Encryption failed: {}", e))?;
        let mut result = Vec::with_capacity(1 + 12 + ciphertext.len());
        result.push(1u8);
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
        use aes_gcm::aead::Aead;
        if data.is_empty() {
            return Err("Empty encrypted data".to_string());
        }
        let version = data[0];
        if version != 1 {
            return Err(format!("Unsupported encryption version: {}", version));
        }
        if data.len() < 1 + 12 {
            return Err("Encrypted data too short".to_string());
        }
        let nonce_bytes = &data[1..13];
        let ciphertext = &data[13..];
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| format!("AES init failed: {}", e))?;
        let nonce = Nonce::from_slice(nonce_bytes);
        cipher.decrypt(nonce, ciphertext)
            .map_err(|e| format!("Decryption failed: {}", e))
    }
}

impl SecureStorage for AndroidEncryptedSecureStorage {
    fn get_secret(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        let enc_path = self.secret_path(key);
        if !enc_path.exists() {
            let legacy_path = self.storage_dir.join(format!("{}.bin", key));
            if legacy_path.exists() {
                let plaintext = std::fs::read(&legacy_path).map_err(|e| e.to_string())?;
                let encrypted = self.encrypt(&plaintext)?;
                if let Some(parent) = enc_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                let tmp_path = enc_path.with_extension("tmp");
                std::fs::write(&tmp_path, &encrypted).map_err(|e| e.to_string())?;
                std::fs::rename(&tmp_path, &enc_path).map_err(|e| e.to_string())?;
                let _ = std::fs::remove_file(&legacy_path);
                return Ok(Some(plaintext));
            }
            return Ok(None);
        }
        let encrypted = std::fs::read(&enc_path).map_err(|e| e.to_string())?;
        let plaintext = self.decrypt(&encrypted)?;
        Ok(Some(plaintext))
    }

    fn set_secret(&self, key: &str, value: &[u8]) -> Result<(), String> {
        let enc_path = self.secret_path(key);
        if let Some(parent) = enc_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let encrypted = self.encrypt(value)?;
        let tmp_path = enc_path.with_extension("tmp");
        std::fs::write(&tmp_path, &encrypted).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp_path, &enc_path).map_err(|e| e.to_string())
    }

    fn delete_secret(&self, key: &str) -> Result<(), String> {
        let enc_path = self.secret_path(key);
        let legacy_path = self.storage_dir.join(format!("{}.bin", key));
        if enc_path.exists() {
            std::fs::remove_file(&enc_path).map_err(|e| e.to_string())?;
        }
        if legacy_path.exists() {
            let _ = std::fs::remove_file(&legacy_path);
        }
        Ok(())
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

