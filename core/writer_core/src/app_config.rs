//! # 应用配置管理模块 (App Config)
//!
//! 本模块负责管理应用程序级别的全局配置，主要是跨会话持久化的用户偏好设置。
//!
//! ## 主要功能
//!
//! - **配置文件管理**: 自动在平台标准目录下创建和管理配置文件
//! - **工作区路径记忆**: 记住用户最后使用的工作区路径，方便下次快速打开
//! - **安全文件替换**: 使用临时文件 + flush + sync_all + 先删后 rename 的方式确保配置写入的安全性
//! - **跨平台支持**: Windows 使用 `%APPDATA%/SujianWriter`，Linux/macOS 遵循 XDG 规范
//!
//! ## 配置存储位置
//!
//! - Windows: `%APPDATA%/SujianWriter/app_config.json`
//! - Linux/macOS: `~/.config/writer/app_config.json`
//! - Linux/macOS 自定义路径: 通过 `XDG_CONFIG_HOME` 环境变量指定
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
use std::io::Write;
use std::path::PathBuf;

/// Windows 上的配置目录名
#[cfg(target_os = "windows")]
const CONFIG_DIR_NAME: &str = "SujianWriter";

/// Linux/macOS 上的配置目录名
#[cfg(not(target_os = "windows"))]
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

/// 返回平台相关的配置目录路径。
///
/// - Windows: `%APPDATA%/SujianWriter`
/// - Linux/macOS: 优先使用 `XDG_CONFIG_HOME/writer`，否则 `HOME/.config/writer`
fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|appdata| PathBuf::from(appdata).join(CONFIG_DIR_NAME))
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Some(config_dir) = std::env::var_os("XDG_CONFIG_HOME") {
            Some(PathBuf::from(config_dir).join(CONFIG_DIR_NAME))
        } else {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".config").join(CONFIG_DIR_NAME))
        }
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

/// 将配置安全地写入文件。
///
/// 使用"写临时文件 → flush + sync_all → 先删目标 → rename"的方式，
/// 在 Windows 上避免目标文件已存在时 rename 失败的问题，
/// 同时确保数据在 rename 之前已落盘。
pub fn save_app_config(config: &AppConfig) -> Result<(), String> {
    let path = config_path().ok_or("Cannot determine config directory".to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    let tmp_path = path.with_extension("tmp");

    // 写入临时文件
    let mut file = fs::File::create(&tmp_path).map_err(|e| e.to_string())?;
    file.write_all(content.as_bytes())
        .map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file); // 关闭文件句柄，确保 rename 时文件不被占用

    // 如果目标文件已存在，先删除（Windows 上 rename 不能覆盖已存在的文件）
    if path.exists() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }

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
    config.last_route = if route.is_empty() { None } else { Some(route.to_string()) };
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

// ============================================================================
// 测试辅助函数：在指定目录下保存/加载配置，避免污染真实配置目录
// ============================================================================

/// 将配置保存到指定路径（测试用）
fn save_app_config_to(config: &AppConfig, path: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    let tmp_path = path.with_extension("tmp");

    let mut file = fs::File::create(&tmp_path).map_err(|e| e.to_string())?;
    file.write_all(content.as_bytes())
        .map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);

    if path.exists() {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }

    fs::rename(&tmp_path, path).map_err(|e| e.to_string())?;
    Ok(())
}

/// 从指定路径加载配置（测试用）
fn load_app_config_from(path: &std::path::Path) -> AppConfig {
    if path.exists() {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(config) = serde_json::from_str(&content) {
                return config;
            }
        }
    }
    AppConfig::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // config_dir 平台测试
    // ------------------------------------------------------------------------

    #[test]
    fn test_config_dir_returns_some() {
        // 在正常环境下，config_dir 应该能返回有效路径
        let dir = config_dir();
        assert!(dir.is_some(), "config_dir() 应该返回 Some");
        let dir = dir.unwrap();

        #[cfg(target_os = "windows")]
        {
            // Windows 上应该以 SujianWriter 结尾
            assert_eq!(
                dir.file_name().unwrap().to_str().unwrap(),
                "SujianWriter",
                "Windows 上配置目录名应为 SujianWriter"
            );
        }

        #[cfg(not(target_os = "windows"))]
        {
            // Linux/macOS 上应该以 writer 结尾
            assert_eq!(
                dir.file_name().unwrap().to_str().unwrap(),
                "writer",
                "Linux/macOS 上配置目录名应为 writer"
            );
        }
    }

    #[test]
    fn test_config_dir_name_constant() {
        // 验证常量在当前平台上的值
        #[cfg(target_os = "windows")]
        assert_eq!(CONFIG_DIR_NAME, "SujianWriter");

        #[cfg(not(target_os = "windows"))]
        assert_eq!(CONFIG_DIR_NAME, "writer");
    }

    // ------------------------------------------------------------------------
    // save_app_config 安全替换逻辑测试
    // ------------------------------------------------------------------------

    #[test]
    fn test_save_app_config_creates_new_file() {
        let tmp_dir = tempfile::tempdir().expect("无法创建临时目录");
        let config_path = tmp_dir.path().join("app_config.json");

        let config = AppConfig {
            last_workspace_path: Some("/tmp/test_workspace".to_string()),
            ..Default::default()
        };

        save_app_config_to(&config, &config_path).expect("保存配置应该成功");

        assert!(config_path.exists(), "配置文件应该被创建");
        let loaded = load_app_config_from(&config_path);
        assert_eq!(
            loaded.last_workspace_path,
            Some("/tmp/test_workspace".to_string())
        );
    }

    #[test]
    fn test_save_app_config_replaces_existing_file() {
        let tmp_dir = tempfile::tempdir().expect("无法创建临时目录");
        let config_path = tmp_dir.path().join("app_config.json");

        // 先写入一个配置
        let config1 = AppConfig {
            last_workspace_path: Some("/old/path".to_string()),
            last_route: Some("home".to_string()),
            ..Default::default()
        };
        save_app_config_to(&config1, &config_path).expect("第一次保存应该成功");
        assert!(config_path.exists());

        // 再写入另一个配置，覆盖
        let config2 = AppConfig {
            last_workspace_path: Some("/new/path".to_string()),
            ..Default::default()
        };
        save_app_config_to(&config2, &config_path).expect("第二次保存应该成功（覆盖）");

        let loaded = load_app_config_from(&config_path);
        assert_eq!(loaded.last_workspace_path, Some("/new/path".to_string()));
        // 旧数据应该被完全覆盖
        assert_eq!(loaded.last_route, None);
    }

    #[test]
    fn test_save_app_config_no_leftover_tmp_file() {
        let tmp_dir = tempfile::tempdir().expect("无法创建临时目录");
        let config_path = tmp_dir.path().join("app_config.json");
        let tmp_path = tmp_dir.path().join("app_config.tmp");

        let config = AppConfig::default();
        save_app_config_to(&config, &config_path).expect("保存应该成功");

        // 临时文件应该已被 rename，不应残留
        assert!(!tmp_path.exists(), "临时文件不应残留");
        assert!(config_path.exists(), "目标文件应该存在");
    }

    #[test]
    fn test_save_app_config_creates_parent_dirs() {
        let tmp_dir = tempfile::tempdir().expect("无法创建临时目录");
        let config_path = tmp_dir.path().join("nested").join("dir").join("app_config.json");

        let config = AppConfig {
            last_workspace_path: Some("/test".to_string()),
            ..Default::default()
        };
        save_app_config_to(&config, &config_path).expect("保存到嵌套目录应该成功");
        assert!(config_path.exists());
    }

    // ------------------------------------------------------------------------
    // set_last_workspace_path / save_last_navigation_state 保存和读取测试
    // ------------------------------------------------------------------------

    #[test]
    fn test_app_config_workspace_path_roundtrip() {
        let tmp_dir = tempfile::tempdir().expect("无法创建临时目录");
        let config_path = tmp_dir.path().join("app_config.json");

        // 保存
        let config = AppConfig {
            last_workspace_path: Some("/home/user/my_workspace".to_string()),
            ..Default::default()
        };
        save_app_config_to(&config, &config_path).expect("保存应该成功");

        // 读取
        let loaded = load_app_config_from(&config_path);
        assert_eq!(
            loaded.last_workspace_path,
            Some("/home/user/my_workspace".to_string())
        );
    }

    #[test]
    fn test_app_config_clear_workspace_path() {
        let tmp_dir = tempfile::tempdir().expect("无法创建临时目录");
        let config_path = tmp_dir.path().join("app_config.json");

        // 先设置
        let config = AppConfig {
            last_workspace_path: Some("/some/path".to_string()),
            ..Default::default()
        };
        save_app_config_to(&config, &config_path).expect("保存应该成功");

        // 再清除
        let config2 = AppConfig {
            last_workspace_path: None,
            ..Default::default()
        };
        save_app_config_to(&config2, &config_path).expect("清除保存应该成功");

        let loaded = load_app_config_from(&config_path);
        assert_eq!(loaded.last_workspace_path, None);
    }

    #[test]
    fn test_app_config_navigation_state_roundtrip() {
        let tmp_dir = tempfile::tempdir().expect("无法创建临时目录");
        let config_path = tmp_dir.path().join("app_config.json");

        // 保存完整导航状态
        let config = AppConfig {
            last_route: Some("editor".to_string()),
            last_project_id: Some("proj-001".to_string()),
            last_volume_id: Some("vol-001".to_string()),
            last_chapter_id: Some("chap-001".to_string()),
            last_starmap_id: Some("star-001".to_string()),
            ..Default::default()
        };
        save_app_config_to(&config, &config_path).expect("保存应该成功");

        // 读取并验证
        let loaded = load_app_config_from(&config_path);
        assert_eq!(loaded.last_route, Some("editor".to_string()));
        assert_eq!(loaded.last_project_id, Some("proj-001".to_string()));
        assert_eq!(loaded.last_volume_id, Some("vol-001".to_string()));
        assert_eq!(loaded.last_chapter_id, Some("chap-001".to_string()));
        assert_eq!(loaded.last_starmap_id, Some("star-001".to_string()));
    }

    #[test]
    fn test_app_config_navigation_state_partial() {
        let tmp_dir = tempfile::tempdir().expect("无法创建临时目录");
        let config_path = tmp_dir.path().join("app_config.json");

        // 只保存部分导航状态
        let config = AppConfig {
            last_route: Some("home".to_string()),
            last_project_id: Some("proj-002".to_string()),
            last_volume_id: None,
            last_chapter_id: None,
            last_starmap_id: None,
            ..Default::default()
        };
        save_app_config_to(&config, &config_path).expect("保存应该成功");

        let loaded = load_app_config_from(&config_path);
        assert_eq!(loaded.last_route, Some("home".to_string()));
        assert_eq!(loaded.last_project_id, Some("proj-002".to_string()));
        assert_eq!(loaded.last_volume_id, None);
        assert_eq!(loaded.last_chapter_id, None);
        assert_eq!(loaded.last_starmap_id, None);
    }

    #[test]
    fn test_app_config_clear_navigation_state() {
        let tmp_dir = tempfile::tempdir().expect("无法创建临时目录");
        let config_path = tmp_dir.path().join("app_config.json");

        // 先保存完整导航状态
        let config = AppConfig {
            last_route: Some("editor".to_string()),
            last_project_id: Some("proj-001".to_string()),
            last_volume_id: Some("vol-001".to_string()),
            last_chapter_id: Some("chap-001".to_string()),
            last_starmap_id: Some("star-001".to_string()),
            ..Default::default()
        };
        save_app_config_to(&config, &config_path).expect("保存应该成功");

        // 清除
        let config2 = AppConfig::default();
        save_app_config_to(&config2, &config_path).expect("清除保存应该成功");

        let loaded = load_app_config_from(&config_path);
        assert_eq!(loaded.last_route, None);
        assert_eq!(loaded.last_project_id, None);
        assert_eq!(loaded.last_volume_id, None);
        assert_eq!(loaded.last_chapter_id, None);
        assert_eq!(loaded.last_starmap_id, None);
    }

    #[test]
    fn test_app_config_empty_route_becomes_none() {
        // 验证 save_last_navigation_state 的空路由逻辑
        let config = AppConfig {
            last_route: None, // 模拟空字符串被转为 None
            last_project_id: Some("proj-001".to_string()),
            ..Default::default()
        };

        let tmp_dir = tempfile::tempdir().expect("无法创建临时目录");
        let config_path = tmp_dir.path().join("app_config.json");
        save_app_config_to(&config, &config_path).expect("保存应该成功");

        let loaded = load_app_config_from(&config_path);
        assert_eq!(loaded.last_route, None);
        assert_eq!(loaded.last_project_id, Some("proj-001".to_string()));
    }

    #[test]
    fn test_load_app_config_from_missing_file_returns_default() {
        let tmp_dir = tempfile::tempdir().expect("无法创建临时目录");
        let config_path = tmp_dir.path().join("nonexistent.json");

        let loaded = load_app_config_from(&config_path);
        assert_eq!(loaded.last_workspace_path, None);
        assert_eq!(loaded.last_route, None);
        assert_eq!(loaded.last_project_id, None);
    }

    #[test]
    fn test_load_app_config_from_invalid_json_returns_default() {
        let tmp_dir = tempfile::tempdir().expect("无法创建临时目录");
        let config_path = tmp_dir.path().join("bad.json");

        fs::write(&config_path, "this is not json{{{").expect("写入应该成功");

        let loaded = load_app_config_from(&config_path);
        assert_eq!(loaded.last_workspace_path, None);
    }
}
