//! Linux 安全存储实现。
//!
//! 在 `secret-service` feature 启用时基于 `keyring` 访问 Secret Service；
//! 否则安全存储不可用。提供从旧版明文密钥迁移的能力。

use writer_platform_api::SecureStorage;

use super::dirs::xdg_config_dir;

pub(crate) fn create_secure_storage() -> Option<Box<dyn SecureStorage>> {
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
