use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const CONFIG_DIR_NAME: &str = "writer";
const CONFIG_FILE_NAME: &str = "app_config.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default)]
    pub last_workspace_path: Option<String>,
}

fn config_dir() -> Option<PathBuf> {
    if let Some(config_dir) = std::env::var_os("XDG_CONFIG_HOME") {
        Some(PathBuf::from(config_dir).join(CONFIG_DIR_NAME))
    } else {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".config").join(CONFIG_DIR_NAME))
    }
}

fn config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join(CONFIG_FILE_NAME))
}

pub fn load_app_config() -> AppConfig {
    if let Some(path) = config_path() {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str(&content) {
                    return config;
                }
            }
        }
    }
    AppConfig::default()
}

pub fn save_app_config(config: &AppConfig) -> Result<(), String> {
    let path = config_path().ok_or("Cannot determine config directory".to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, &content).map_err(|e| e.to_string())?;
    fs::rename(&tmp_path, &path).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_last_workspace_path(path: &str) -> Result<(), String> {
    let mut config = load_app_config();
    config.last_workspace_path = Some(path.to_string());
    save_app_config(&config)
}

pub fn get_last_workspace_path() -> Option<String> {
    load_app_config().last_workspace_path
}

pub fn clear_last_workspace_path() -> Result<(), String> {
    let mut config = load_app_config();
    config.last_workspace_path = None;
    save_app_config(&config)
}
