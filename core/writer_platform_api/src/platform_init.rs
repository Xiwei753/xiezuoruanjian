use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlatformKind {
    Android,
    #[default]
    Desktop,
    Windows,
    Harmony,
    Apple,
}

impl std::fmt::Display for PlatformKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Desktop => write!(f, "desktop"),
            Self::Android => write!(f, "android"),
            Self::Windows => write!(f, "windows"),
            Self::Harmony => write!(f, "harmony"),
            Self::Apple => write!(f, "apple"),
        }
    }
}

impl PlatformKind {
    pub fn to_str_name(&self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Android => "android",
            Self::Windows => "windows",
            Self::Harmony => "harmony",
            Self::Apple => "apple",
        }
    }

    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "desktop" | "linux" | "linux_qt" => Some(Self::Desktop),
            "android" => Some(Self::Android),
            "windows" => Some(Self::Windows),
            "harmony" => Some(Self::Harmony),
            "apple" => Some(Self::Apple),
            _ => None,
        }
    }

    pub fn default_device_class(&self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Android => "phone",
            Self::Harmony => "tablet",
            Self::Windows => "desktop",
            Self::Apple => "desktop",
        }
    }
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
