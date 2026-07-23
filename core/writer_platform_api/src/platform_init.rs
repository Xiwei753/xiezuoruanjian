use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlatformKind {
    Android,
    #[default]
    Linux,
    Windows,
    Harmony,
    Apple,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInit {
    pub platform: PlatformKind,
    pub app_data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub log_dir: PathBuf,
    pub no_backup_dir: Option<PathBuf>,
    pub device_id: String,
    pub app_version: String,
    pub locale: String,
    pub timezone: String,
}

impl PlatformInit {
    pub fn paths(&self) -> PlatformPaths {
        PlatformPaths {
            app_data_dir: self.app_data_dir.clone(),
            cache_dir: self.cache_dir.clone(),
            log_dir: self.log_dir.clone(),
            no_backup_dir: self.no_backup_dir.clone(),
            config_dir: self.app_data_dir.join("config"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlatformPaths {
    pub app_data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub log_dir: PathBuf,
    pub no_backup_dir: Option<PathBuf>,
    pub config_dir: PathBuf,
}
