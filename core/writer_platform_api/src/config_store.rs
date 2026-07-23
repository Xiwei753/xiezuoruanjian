pub trait ConfigStore: Send + Sync {
    fn load(&self) -> Result<Option<Vec<u8>>, String>;
    fn save(&self, bytes: &[u8]) -> Result<(), String>;
}

use std::fs;
use std::io::Write;
use std::path::PathBuf;

const CONFIG_FILE_NAME: &str = "app_config.json";

pub struct FileConfigStore {
    config_dir: PathBuf,
}

impl FileConfigStore {
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    pub fn config_file_path(&self) -> PathBuf {
        self.config_dir.join(CONFIG_FILE_NAME)
    }
}

impl ConfigStore for FileConfigStore {
    fn load(&self) -> Result<Option<Vec<u8>>, String> {
        let path = self.config_file_path();
        if !path.exists() {
            return Ok(None);
        }
        fs::read(&path).map(Some).map_err(|e| e.to_string())
    }

    fn save(&self, bytes: &[u8]) -> Result<(), String> {
        let path = self.config_file_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp_path = path.with_extension("tmp");

        let mut file = fs::File::create(&tmp_path).map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);

        if path.exists() {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }

        fs::rename(&tmp_path, &path).map_err(|e| e.to_string())?;
        Ok(())
    }
}
