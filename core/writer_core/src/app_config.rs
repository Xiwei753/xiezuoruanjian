//! # 应用配置管理模块 (App Config)
//!
//! 本模块负责管理应用程序级别的全局配置，主要是跨会话持久化的用户偏好设置。
//!
//! ## 主要功能
//!
//! - **配置文件管理**: 自动在 XDG 标准目录下创建和管理配置文件
//! - **工作区路径记忆**: 记住用户最后使用的工作区路径，方便下次快速打开
//! - **原子写入**: 使用临时文件 + rename 的方式确保配置写入的原子性
//! - **跨平台支持**: 遵循 XDG 规范，支持 Linux/macOS 等系统
//!
//! ## 配置存储位置
//!
//! - Linux/macOS: `~/.config/writer/app_config.json`
//! - 自定义路径: 通过 `XDG_CONFIG_HOME` 环境变量指定
//!
//! ## 依赖关系
//!
//! - `serde` / `serde_json`: 序列化/反序列化配置数据
//! - `std::fs`: 文件系统操作
//!
//! ## 使用场景
//!
//! - 应用启动时恢复上次的工作区
//! - 记录用户的应用级偏好设置
//! - 提供应用级别的配置持久化

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
    #[serde(default)]
    pub last_route: Option<String>,
    #[serde(default)]
    pub last_project_id: Option<String>,
    #[serde(default)]
    pub last_volume_id: Option<String>,
    #[serde(default)]
    pub last_chapter_id: Option<String>,
    #[serde(default)]
    pub last_starmap_id: Option<String>,
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

/// 保存上次导航状态（路由、项目ID、卷ID、章节ID、星图ID）
pub fn save_last_navigation_state(
    route: &str,
    project_id: Option<&str>,
    volume_id: Option<&str>,
    chapter_id: Option<&str>,
    starmap_id: Option<&str>,
) -> Result<(), String> {
    let mut config = load_app_config();
    config.last_route = if route.is_empty() {
        None
    } else {
        Some(route.to_string())
    };
    config.last_project_id = project_id.map(|s| s.to_string());
    config.last_volume_id = volume_id.map(|s| s.to_string());
    config.last_chapter_id = chapter_id.map(|s| s.to_string());
    config.last_starmap_id = starmap_id.map(|s| s.to_string());
    save_app_config(&config)
}

/// 获取上次导航状态
pub fn get_last_navigation_state() -> NavigationState {
    let config = load_app_config();
    NavigationState {
        route: config.last_route,
        project_id: config.last_project_id,
        volume_id: config.last_volume_id,
        chapter_id: config.last_chapter_id,
        starmap_id: config.last_starmap_id,
    }
}

/// 清除上次导航状态
pub fn clear_last_navigation_state() -> Result<(), String> {
    let mut config = load_app_config();
    config.last_route = None;
    config.last_project_id = None;
    config.last_volume_id = None;
    config.last_chapter_id = None;
    config.last_starmap_id = None;
    save_app_config(&config)
}

/// 上次导航状态
#[derive(Debug, Clone, Default)]
pub struct NavigationState {
    pub route: Option<String>,
    pub project_id: Option<String>,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    pub starmap_id: Option<String>,
}
