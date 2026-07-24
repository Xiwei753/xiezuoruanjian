use std::path::PathBuf;
use writer_platform_api::SecureStorage;

pub struct FileSecureStorage {
    storage_dir: PathBuf,
}

impl FileSecureStorage {
    pub fn new(storage_dir: PathBuf) -> Self {
        Self {
            storage_dir: storage_dir.join("secrets"),
        }
    }

    fn secret_path(&self, key: &str) -> PathBuf {
        self.storage_dir.join(format!("{}.bin", key))
    }
}

impl SecureStorage for FileSecureStorage {
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
