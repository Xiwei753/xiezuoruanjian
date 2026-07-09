//! # Facade 层 - 客户端统一 API 入口（Core 层）
//!
//! 这是 Core 内部统一入口。
//! **注意：**它不是平台稳定 API 边界。
//! Android / Linux / 未来平台不得把 Facade 当主暴露层。
//! 平台应走 `api::WriterCoreApi` 或其绑定适配层。
//!
//! ## 设计原则
//!
//! - **薄 Facade**：只做参数转发和类型转换，不包含业务逻辑
//! - **统一错误处理**：所有操作返回 `Result<T>`，客户端必须处理错误
//! - **无状态**：每个方法都是独立的，不依赖内部状态（除了 workspace_path）
//!
//! ## 调用链示例
//!
//! ```text
//! Linux (Legacy): AppBackend/Linux adapter → facade::WriterCore::create_chapter() → chapter::create_chapter()
//! Linux (New):    AppBackend/Linux adapter → api::WriterCoreApi::create_chapter() → facade::WriterCore::create_chapter() → chapter::create_chapter()
//! ```
//!
//! ## 禁止事项
//!
//! - 客户端不允许绕过 Facade 直接调用子模块
//! - Facade 不允许添加 UI 逻辑（动画、窗口管理等）
//! - Facade 不允许吞掉错误（必须返回 Result）

mod action_ops;
mod project_ops;
mod service_ops;
mod settings_ops;
mod starmap_ops;
mod sync_ops;
mod workspace_ops;
mod writing_stats_ops;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::chapter::Chapter;
use crate::writing_stats::api::StatsApi;

pub struct WriterCore {
    pub(crate) workspace_path: PathBuf,
    pub(crate) stats_api: OnceLock<StatsApi>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChapterOpenResult {
    pub meta: Chapter,
    pub content: String,
}

impl WriterCore {
    pub fn new<P: AsRef<Path>>(workspace_path: P) -> Self {
        Self {
            workspace_path: workspace_path.as_ref().to_path_buf(),
            stats_api: OnceLock::new(),
        }
    }

    pub(crate) fn get_stats_api(&self) -> &StatsApi {
        self.stats_api
            .get_or_init(|| StatsApi::new(&self.workspace_path))
    }

    pub fn workspace_path(&self) -> &Path {
        &self.workspace_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_facade_basic_flow() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());

        assert!(core.create_workspace().is_ok());
        assert!(core.validate_workspace().unwrap());

        let project = core.create_project("My Project").unwrap();
        let projects = core.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, project.id);

        let volume = core.create_volume(&project.id, "Vol 1").unwrap();
        let volumes = core.list_volumes(&project.id).unwrap();
        assert_eq!(volumes.len(), 2);
        assert!(volumes.iter().any(|v| v.id == volume.id));

        let chapter = core
            .create_chapter(&project.id, &volume.id, "Ch 1")
            .unwrap();
        let chapters = core.list_chapters(&project.id, &volume.id).unwrap();
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].id, chapter.id);

        core.write_chapter(&project.id, &volume.id, &chapter.id, "Content here")
            .unwrap();
        let content = core
            .read_chapter(&project.id, &volume.id, &chapter.id)
            .unwrap();
        assert_eq!(content.content, "Content here");
    }

    #[test]
    fn test_facade_open_save_receipt_and_error_code() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        let project = core.create_project("My Project").unwrap();
        let volume = core.create_volume(&project.id, "Vol 1").unwrap();
        let chapter = core
            .create_chapter(&project.id, &volume.id, "Ch 1")
            .unwrap();

        let receipt = core
            .write_chapter_verified(&project.id, &volume.id, &chapter.id, "Content here")
            .unwrap();
        assert_eq!(receipt.word_count, 11);
        assert!(receipt.content_len > 0);

        let opened = core
            .open_chapter(&project.id, &volume.id, &chapter.id)
            .unwrap();
        assert_eq!(opened.meta.id, chapter.id);
        assert_eq!(opened.content, "Content here");

        let err = core
            .write_chapter_verified(&project.id, &volume.id, &chapter.id, "")
            .unwrap_err();
        assert_eq!(err.code(), "EMPTY_OVERWRITE_BLOCKED");

        let clear_receipt = core
            .clear_chapter_content_verified(&project.id, &volume.id, &chapter.id)
            .unwrap();
        assert_eq!(clear_receipt.word_count, 0);
        assert_eq!(
            core.open_chapter(&project.id, &volume.id, &chapter.id)
                .unwrap()
                .content,
            ""
        );
    }

    #[test]
    fn test_facade_settings_flow() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        let mut local_settings = core.load_local_settings().unwrap();
        local_settings.window_width = 1000.0;
        local_settings.theme_mode = Some("dark".to_string());
        local_settings.auto_save_enabled = true;
        local_settings.editor_font_size = 14.0;
        core.save_local_settings(&local_settings).unwrap();

        let loaded_local = core.load_local_settings().unwrap();
        assert_eq!(loaded_local.window_width, 1000.0);
        assert_eq!(loaded_local.theme_mode.unwrap(), "dark");
        assert!(loaded_local.auto_save_enabled);
        assert_eq!(loaded_local.editor_font_size, 14.0);

        let mut syncable_settings = core.load_syncable_settings().unwrap();
        syncable_settings.font_size = 18.0;
        syncable_settings.theme_mode = "system".to_string();
        core.save_syncable_settings(&syncable_settings).unwrap();

        let loaded_syncable = core.load_syncable_settings().unwrap();
        assert_eq!(loaded_syncable.font_size, 18.0);
        assert_eq!(loaded_syncable.theme_mode, "system");

        let get_result = core
            .execute_action("settings.editor.font_size.get", "", "")
            .unwrap();
        assert!(get_result.success);
        let data = get_result.data.unwrap();
        assert_eq!(data.get("fontSize").unwrap().as_f64().unwrap(), 18.0);
        assert_eq!(data.get("source").unwrap().as_str().unwrap(), "syncable");
    }

    #[test]
    fn test_facade_not_implemented() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());

        assert!(core.move_chapter_to_trash("c1").is_err());
    }

    #[test]
    fn test_facade_sync_config_flow() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        let config = core.load_sync_config().unwrap();
        assert!(!config.enabled);
        assert_eq!(config.backend_type, crate::sync::BackendType::GithubApi);

        let mut new_config = config.clone();
        new_config.enabled = true;
        new_config.remote_url = "https://example.com/repo.git".to_string();
        core.save_sync_config(&new_config).unwrap();

        let loaded = core.load_sync_config().unwrap();
        assert!(loaded.enabled);
        assert_eq!(loaded.remote_url, "https://example.com/repo.git");

        let mut secrets = core.load_sync_secrets().unwrap();
        secrets.token = Some("my_super_secret_token".to_string());
        core.save_sync_secrets(&secrets).unwrap();

        let loaded_secrets = core.load_sync_secrets().unwrap();
        assert_eq!(
            loaded_secrets.token.as_ref().unwrap(),
            "my_super_secret_token"
        );

        assert!(core.validate_sync_config(&loaded).unwrap());

        let mut bad_config = loaded.clone();
        bad_config.remote_url = "".to_string();
        assert!(!core.validate_sync_config(&bad_config).unwrap());
    }

    #[test]
    fn test_load_sync_config_migrates_git_backend_for_github_https_remote() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        let config_path = temp_dir.path().join("app-meta/sync/sync_config.json");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(
            &config_path,
            r#"{
  "enabled": true,
  "backend_type": "git",
  "remote_url": "https://github.com/test/repo.git",
  "transport": "https_token",
  "branch": "main",
  "auto_sync": false,
  "sync_interval_seconds": 300,
  "username": ""
}"#,
        )
        .unwrap();

        let loaded = core.load_sync_config().unwrap();
        assert_eq!(loaded.backend_type, crate::sync::BackendType::GithubApi);

        let persisted = core.load_sync_config().unwrap();
        assert_eq!(persisted.backend_type, crate::sync::BackendType::GithubApi);
    }

    #[test]
    fn test_facade_perform_sync_dry_run() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        let config = core.load_sync_config().unwrap();
        let plan = core.perform_sync_dry_run(&config).unwrap();
        assert!(plan.files_to_upload.is_empty());
    }

    #[test]
    fn test_execute_action_args_parsing() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        let result_empty = core
            .execute_action("settings.editor.font_size.set", "", "")
            .unwrap();
        assert!(!result_empty.success);
        assert_eq!(
            result_empty.message.unwrap(),
            "Missing or invalid fontSize parameter"
        );

        let result_ws = core
            .execute_action("settings.editor.font_size.set", "   ", "")
            .unwrap();
        assert!(!result_ws.success);
        assert_eq!(
            result_ws.message.unwrap(),
            "Missing or invalid fontSize parameter"
        );

        let result_null = core
            .execute_action("settings.editor.font_size.set", "null", "")
            .unwrap();
        assert!(!result_null.success);
        assert_eq!(
            result_null.message.unwrap(),
            "Missing or invalid fontSize parameter"
        );

        let result_invalid = core
            .execute_action("settings.editor.font_size.set", "{ invalid }", "")
            .unwrap();
        assert!(!result_invalid.success);
        assert_eq!(result_invalid.message.unwrap(), "invalid args json");

        let result_valid_json = core
            .execute_action("settings.editor.font_size.set", "{}", "")
            .unwrap();
        assert!(!result_valid_json.success);
        assert_eq!(
            result_valid_json.message.unwrap(),
            "Missing or invalid fontSize parameter"
        );

        let result_valid = core
            .execute_action("settings.editor.font_size.set", "{\"fontSize\": 14.0}", "")
            .unwrap();
        assert!(result_valid.success);
        assert_eq!(result_valid.message.unwrap(), "Font size updated");
    }
}
