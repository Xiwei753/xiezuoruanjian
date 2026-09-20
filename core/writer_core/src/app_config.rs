//! # 应用配置管理模块 (App Config)
//!
//! 本模块负责管理应用程序级别的全局配置，主要是跨会话持久化的用户偏好设置。
//!
//! ## 架构
//!
//! - **配置模型**：`AppConfig` 和 `NavigationState` 是纯数据模型，与存储无关。
//! - **配置存储**：通过 `writer_platform_api::ConfigStore` trait 注入，Core 不再自行猜测平台目录。
//! - **便利函数**：`load_app_config()` / `save_app_config()` 等函数使用全局默认 `ConfigStore`，
//!   该默认存储由平台适配层在启动时通过 `set_default_config_store` 注入。
//!
//! ## 依赖方向
//!
//! ```text
//! 平台适配层 → set_default_config_store() → app_config 便利函数
//! ```
//!
//! Core 业务模块通过 `ConfigStore` trait 消费配置，不直接访问文件系统或环境变量。
//!
//! `FileConfigStore` 实现已迁移至 `writer_platform_api::FileConfigStore`，
//! 平台适配层直接使用该实现或提供自己的 `ConfigStore` 实现。

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use writer_platform_api::ConfigStore;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
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
    #[serde(default)]
    pub last_workspace_path: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct NavigationState {
    pub route: Option<String>,
    pub project_id: Option<String>,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    pub starmap_id: Option<String>,
}

static DEFAULT_CONFIG_STORE: Mutex<Option<Box<dyn ConfigStore>>> = Mutex::new(None);

pub fn set_default_config_store(store: Box<dyn ConfigStore>) {
    let mut guard = DEFAULT_CONFIG_STORE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    *guard = Some(store);
}

fn with_default_store<R, F: FnOnce(&dyn ConfigStore) -> R>(f: F) -> Option<R> {
    let guard = DEFAULT_CONFIG_STORE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    guard.as_ref().map(|s| f(s.as_ref()))
}

pub fn load_app_config() -> AppConfig {
    if let Some(result) = with_default_store(|store| match store.load() {
        Ok(Some(bytes)) => serde_json::from_slice::<AppConfig>(&bytes).unwrap_or_default(),
        Ok(None) => AppConfig::default(),
        Err(_) => AppConfig::default(),
    }) {
        return result;
    }
    AppConfig::default()
}

pub fn save_app_config(config: &AppConfig) -> Result<(), String> {
    if let Some(result) = with_default_store(|store| {
        let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
        store.save(content.as_bytes())
    }) {
        return result;
    }
    Err("No default ConfigStore configured".to_string())
}

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

pub fn clear_last_navigation_state() -> Result<(), String> {
    let mut config = load_app_config();
    config.last_route = None;
    config.last_project_id = None;
    config.last_volume_id = None;
    config.last_chapter_id = None;
    config.last_starmap_id = None;
    save_app_config(&config)
}

/// 持久化上次打开的工作区（数据根）路径，供下次启动自动恢复。
/// 空字符串视为无路径，存为 None。
pub fn save_last_workspace_path(path: &str) -> Result<(), String> {
    let mut config = load_app_config();
    config.last_workspace_path = if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    };
    save_app_config(&config)
}

/// 读取上次打开的工作区（数据根）路径。返回 None 表示无可恢复路径。
pub fn get_last_workspace_path() -> Option<String> {
    load_app_config().last_workspace_path
}

/// 清除持久化的上次工作区路径。显式关闭工作区时调用，避免下次启动自动恢复。
pub fn clear_last_workspace_path() -> Result<(), String> {
    let mut config = load_app_config();
    config.last_workspace_path = None;
    save_app_config(&config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use writer_platform_api::FileConfigStore;

    /// 全局 config store 是进程级单例，多个测试并行设置会互相覆盖。
    /// 用互斥锁串行化所有使用全局 store 的测试，避免竞争条件。
    static GLOBAL_STORE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    fn global_store_lock() -> &'static Mutex<()> {
        GLOBAL_STORE_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_test_store<R, F: FnOnce(&FileConfigStore) -> R>(f: F) -> R {
        let dir = tempfile::tempdir().expect("无法创建临时目录");
        let store = FileConfigStore::new(dir.path().to_path_buf());
        f(&store)
    }

    #[test]
    fn test_file_config_store_roundtrip() {
        let dir = tempfile::tempdir().expect("无法创建临时目录");
        let store = FileConfigStore::new(dir.path().to_path_buf());

        assert!(store.load().unwrap().is_none());

        let config = AppConfig {
            last_route: Some("home".to_string()),
            ..Default::default()
        };
        let content = serde_json::to_string_pretty(&config).unwrap();
        store.save(content.as_bytes()).unwrap();

        let loaded_bytes = store.load().unwrap().unwrap();
        let loaded: AppConfig = serde_json::from_slice(&loaded_bytes).unwrap();
        assert_eq!(loaded.last_route, Some("home".to_string()));
    }

    #[test]
    fn test_file_config_store_replaces_existing() {
        let dir = tempfile::tempdir().expect("无法创建临时目录");
        let store = FileConfigStore::new(dir.path().to_path_buf());

        let config1 = AppConfig {
            last_route: Some("home".to_string()),
            ..Default::default()
        };
        let content1 = serde_json::to_string_pretty(&config1).unwrap();
        store.save(content1.as_bytes()).unwrap();

        let config2 = AppConfig {
            last_route: Some("editor".to_string()),
            ..Default::default()
        };
        let content2 = serde_json::to_string_pretty(&config2).unwrap();
        store.save(content2.as_bytes()).unwrap();

        let loaded_bytes = store.load().unwrap().unwrap();
        let loaded: AppConfig = serde_json::from_slice(&loaded_bytes).unwrap();
        assert_eq!(loaded.last_route, Some("editor".to_string()));
    }

    #[test]
    fn test_file_config_store_no_leftover_tmp_file() {
        let dir = tempfile::tempdir().expect("无法创建临时目录");
        let store = FileConfigStore::new(dir.path().to_path_buf());

        let config = AppConfig::default();
        let content = serde_json::to_string_pretty(&config).unwrap();
        store.save(content.as_bytes()).unwrap();

        let tmp_path = dir.path().join("app_config.tmp");
        assert!(!tmp_path.exists(), "临时文件不应残留");
        let config_path = dir.path().join("app_config.json");
        assert!(config_path.exists(), "目标文件应该存在");
    }

    #[test]
    fn test_file_config_store_creates_parent_dirs() {
        let dir = tempfile::tempdir().expect("无法创建临时目录");
        let nested = dir.path().join("nested").join("dir");
        let store = FileConfigStore::new(nested.clone());

        let config = AppConfig {
            last_route: Some("test".to_string()),
            ..Default::default()
        };
        let content = serde_json::to_string_pretty(&config).unwrap();
        store.save(content.as_bytes()).unwrap();

        let config_path = nested.join("app_config.json");
        assert!(config_path.exists());
    }

    #[test]
    fn test_app_config_route_roundtrip() {
        with_test_store(|store| {
            let config = AppConfig {
                last_route: Some("editor".to_string()),
                ..Default::default()
            };
            let content = serde_json::to_string_pretty(&config).unwrap();
            store.save(content.as_bytes()).unwrap();

            let loaded_bytes = store.load().unwrap().unwrap();
            let loaded: AppConfig = serde_json::from_slice(&loaded_bytes).unwrap();
            assert_eq!(loaded.last_route, Some("editor".to_string()));
        });
    }

    #[test]
    fn test_app_config_clear_route() {
        with_test_store(|store| {
            let config = AppConfig {
                last_route: Some("home".to_string()),
                ..Default::default()
            };
            let content = serde_json::to_string_pretty(&config).unwrap();
            store.save(content.as_bytes()).unwrap();

            let config2 = AppConfig {
                last_route: None,
                ..Default::default()
            };
            let content2 = serde_json::to_string_pretty(&config2).unwrap();
            store.save(content2.as_bytes()).unwrap();

            let loaded_bytes = store.load().unwrap().unwrap();
            let loaded: AppConfig = serde_json::from_slice(&loaded_bytes).unwrap();
            assert_eq!(loaded.last_route, None);
        });
    }

    #[test]
    fn test_app_config_navigation_state_roundtrip() {
        with_test_store(|store| {
            let config = AppConfig {
                last_route: Some("editor".to_string()),
                last_project_id: Some("proj-001".to_string()),
                last_volume_id: Some("vol-001".to_string()),
                last_chapter_id: Some("chap-001".to_string()),
                last_starmap_id: Some("star-001".to_string()),
                ..Default::default()
            };
            let content = serde_json::to_string_pretty(&config).unwrap();
            store.save(content.as_bytes()).unwrap();

            let loaded_bytes = store.load().unwrap().unwrap();
            let loaded: AppConfig = serde_json::from_slice(&loaded_bytes).unwrap();
            assert_eq!(loaded.last_route, Some("editor".to_string()));
            assert_eq!(loaded.last_project_id, Some("proj-001".to_string()));
            assert_eq!(loaded.last_volume_id, Some("vol-001".to_string()));
            assert_eq!(loaded.last_chapter_id, Some("chap-001".to_string()));
            assert_eq!(loaded.last_starmap_id, Some("star-001".to_string()));
        });
    }

    #[test]
    fn test_app_config_navigation_state_partial() {
        with_test_store(|store| {
            let config = AppConfig {
                last_route: Some("home".to_string()),
                last_project_id: Some("proj-002".to_string()),
                ..Default::default()
            };
            let content = serde_json::to_string_pretty(&config).unwrap();
            store.save(content.as_bytes()).unwrap();

            let loaded_bytes = store.load().unwrap().unwrap();
            let loaded: AppConfig = serde_json::from_slice(&loaded_bytes).unwrap();
            assert_eq!(loaded.last_route, Some("home".to_string()));
            assert_eq!(loaded.last_project_id, Some("proj-002".to_string()));
            assert_eq!(loaded.last_volume_id, None);
            assert_eq!(loaded.last_chapter_id, None);
            assert_eq!(loaded.last_starmap_id, None);
        });
    }

    #[test]
    fn test_app_config_clear_navigation_state() {
        with_test_store(|store| {
            let config = AppConfig {
                last_route: Some("editor".to_string()),
                last_project_id: Some("proj-001".to_string()),
                last_volume_id: Some("vol-001".to_string()),
                last_chapter_id: Some("chap-001".to_string()),
                last_starmap_id: Some("star-001".to_string()),
                ..Default::default()
            };
            let content = serde_json::to_string_pretty(&config).unwrap();
            store.save(content.as_bytes()).unwrap();

            let config2 = AppConfig::default();
            let content2 = serde_json::to_string_pretty(&config2).unwrap();
            store.save(content2.as_bytes()).unwrap();

            let loaded_bytes = store.load().unwrap().unwrap();
            let loaded: AppConfig = serde_json::from_slice(&loaded_bytes).unwrap();
            assert_eq!(loaded.last_route, None);
            assert_eq!(loaded.last_project_id, None);
            assert_eq!(loaded.last_volume_id, None);
            assert_eq!(loaded.last_chapter_id, None);
            assert_eq!(loaded.last_starmap_id, None);
        });
    }

    #[test]
    fn test_load_app_config_from_missing_file_returns_default() {
        with_test_store(|store| {
            let loaded_bytes = store.load().unwrap();
            assert!(loaded_bytes.is_none());
        });
    }

    #[test]
    fn test_load_app_config_from_invalid_json_returns_default() {
        let dir = tempfile::tempdir().expect("无法创建临时目录");
        let config_path = dir.path().join("app_config.json");
        std::fs::write(&config_path, "this is not json{{{").unwrap();

        let loaded: AppConfig = std::fs::read(&config_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<AppConfig>(&bytes).ok())
            .unwrap_or_default();
        assert_eq!(loaded.last_route, None);
    }

    #[test]
    fn test_last_workspace_path_roundtrip() {
        let _lock = global_store_lock().lock().unwrap();
        let dir = tempfile::tempdir().expect("无法创建临时目录");
        let store = Box::new(FileConfigStore::new(dir.path().to_path_buf()));
        set_default_config_store(store);

        // 初始无保存路径
        assert_eq!(get_last_workspace_path(), None);

        // save 后 get 应返回相同路径
        save_last_workspace_path("/home/user/sujian-data").expect("save 应成功");
        assert_eq!(
            get_last_workspace_path(),
            Some("/home/user/sujian-data".to_string())
        );

        // 覆盖写入新路径
        save_last_workspace_path("/data/another").expect("save 覆盖应成功");
        assert_eq!(get_last_workspace_path(), Some("/data/another".to_string()));

        // clear 后应为 None
        clear_last_workspace_path().expect("clear 应成功");
        assert_eq!(get_last_workspace_path(), None);
    }

    #[test]
    fn test_save_last_workspace_path_empty_string_is_none() {
        let _lock = global_store_lock().lock().unwrap();
        let dir = tempfile::tempdir().expect("无法创建临时目录");
        let store = Box::new(FileConfigStore::new(dir.path().to_path_buf()));
        set_default_config_store(store);

        save_last_workspace_path("/some/path").expect("save 应成功");
        assert_eq!(get_last_workspace_path(), Some("/some/path".to_string()));

        // 空字符串视为无路径
        save_last_workspace_path("").expect("save 空串应成功");
        assert_eq!(get_last_workspace_path(), None);
    }

    #[test]
    fn test_last_workspace_path_preserves_navigation_state() {
        let _lock = global_store_lock().lock().unwrap();
        let dir = tempfile::tempdir().expect("无法创建临时目录");
        let store = Box::new(FileConfigStore::new(dir.path().to_path_buf()));
        set_default_config_store(store);

        // 先保存导航状态
        save_last_navigation_state("editor", Some("p1"), Some("v1"), Some("c1"), Some("s1"))
            .expect("save nav 应成功");
        // 再保存工作区路径
        save_last_workspace_path("/workspace/path").expect("save ws 应成功");

        // 导航状态不应被工作区路径保存破坏
        let nav = get_last_navigation_state();
        assert_eq!(nav.route, Some("editor".to_string()));
        assert_eq!(nav.project_id, Some("p1".to_string()));
        assert_eq!(nav.volume_id, Some("v1".to_string()));
        assert_eq!(nav.chapter_id, Some("c1".to_string()));
        assert_eq!(nav.starmap_id, Some("s1".to_string()));
        assert_eq!(
            get_last_workspace_path(),
            Some("/workspace/path".to_string())
        );

        // 清工作区路径不应破坏导航状态
        clear_last_workspace_path().expect("clear ws 应成功");
        let nav2 = get_last_navigation_state();
        assert_eq!(nav2.route, Some("editor".to_string()));
        assert_eq!(nav2.project_id, Some("p1".to_string()));
        assert_eq!(get_last_workspace_path(), None);
    }

    #[test]
    fn test_app_config_workspace_path_serde_roundtrip() {
        with_test_store(|store| {
            let config = AppConfig {
                last_workspace_path: Some("/home/user/data".to_string()),
                ..Default::default()
            };
            let content = serde_json::to_string_pretty(&config).unwrap();
            store.save(content.as_bytes()).unwrap();

            let loaded_bytes = store.load().unwrap().unwrap();
            let loaded: AppConfig = serde_json::from_slice(&loaded_bytes).unwrap();
            assert_eq!(
                loaded.last_workspace_path,
                Some("/home/user/data".to_string())
            );
        });
    }

    #[test]
    fn test_app_config_workspace_path_absent_defaults_none() {
        with_test_store(|store| {
            // 不含 lastWorkspacePath 字段的旧配置应默认为 None
            let legacy_json = r#"{"lastRoute":"editor"}"#;
            store.save(legacy_json.as_bytes()).unwrap();

            let loaded_bytes = store.load().unwrap().unwrap();
            let loaded: AppConfig = serde_json::from_slice(&loaded_bytes).unwrap();
            assert_eq!(loaded.last_workspace_path, None);
            assert_eq!(loaded.last_route, Some("editor".to_string()));
        });
    }
}
