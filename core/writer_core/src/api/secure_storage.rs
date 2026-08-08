//! 安全存储契约与 UniFFI callback 适配。
//!
//! 平台客户端通过 `SecureStorageProvider` callback interface 注入各自的安全存储
//! 实现（Android Keystore、Linux Secret Service 等），Core 仅消费
//! `writer_platform_api::SecureStorage` 接口。本模块负责把 callback 形态的
//! `SecureStorageProvider` 包装成 Core 内部使用的 `SecureStorage` trait 对象。

use writer_platform_api::SecureStorage;

/// 安全存储操作错误。
///
/// 使用错误枚举表达，不依赖错误文案包含关系作为主判断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecureStorageError {
    KeystoreKeyInvalidated,
    KeystoreError,
    StorageError,
    MigrationError { reason: String },
}

impl std::fmt::Display for SecureStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecureStorageError::KeystoreKeyInvalidated => write!(f, "Keystore key invalidated"),
            SecureStorageError::KeystoreError => write!(f, "Keystore error"),
            SecureStorageError::StorageError => write!(f, "Storage error"),
            SecureStorageError::MigrationError { reason } => write!(f, "Migration error: {reason}"),
        }
    }
}

impl std::error::Error for SecureStorageError {}

/// 平台侧安全存储能力契约。
///
/// 通过 UniFFI callback interface 由各平台客户端实现，Core 不自行猜测平台存储。
#[uniffi::export(callback_interface)]
pub trait SecureStorageProvider: Send + Sync {
    fn get_secret(&self, key: String) -> std::result::Result<Option<Vec<u8>>, SecureStorageError>;
    fn set_secret(
        &self,
        key: String,
        value: Vec<u8>,
    ) -> std::result::Result<(), SecureStorageError>;
    fn delete_secret(&self, key: String) -> std::result::Result<(), SecureStorageError>;
}

/// 把 `SecureStorageProvider` callback 适配为 Core 内部的 `SecureStorage` trait 对象。
///
/// 错误转换为字符串文案（仅用于跨边界诊断），主判断仍由调用方基于错误码进行。
struct CallbackSecureStorage(Box<dyn SecureStorageProvider>);

impl SecureStorage for CallbackSecureStorage {
    fn get_secret(&self, key: &str) -> std::result::Result<Option<Vec<u8>>, String> {
        self.0
            .get_secret(key.to_string())
            .map_err(|e| e.to_string())
    }

    fn set_secret(&self, key: &str, value: &[u8]) -> std::result::Result<(), String> {
        self.0
            .set_secret(key.to_string(), value.to_vec())
            .map_err(|e| e.to_string())
    }

    fn delete_secret(&self, key: &str) -> std::result::Result<(), String> {
        self.0
            .delete_secret(key.to_string())
            .map_err(|e| e.to_string())
    }
}

/// 把平台 callback 形态的安全存储包装为 Core 内部 `SecureStorage` trait 对象。
///
/// 供 `bootstrap` 启动入口在注入安全存储时调用，避免泄露内部适配类型。
pub(crate) fn wrap_secure_storage(
    provider: Box<dyn SecureStorageProvider>,
) -> Box<dyn SecureStorage> {
    Box::new(CallbackSecureStorage(provider))
}
