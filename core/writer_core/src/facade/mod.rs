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
//! - **无状态**：每个方法都是独立的，不依赖内部状态（除了 app_data_root/projects_root）
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
mod recent_edits_ops;
mod search_ops;
mod service_ops;
mod settings_ops;
mod starmap_ops;
mod sync_config_ops;
mod sync_ops;
mod sync_state_ops;
mod writing_stats_ops;

#[cfg(all(test, feature = "github-api"))]
mod sync_ops_tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};

use crate::chapter::Chapter;
use crate::search::service::SearchIndexService;
use crate::starmap::store::StarMapStore;
use crate::writing_stats::api::StatsApi;

/// Core 内部统一入口——薄 Facade，只做参数转发和类型转换。
///
/// **不是平台稳定 API 边界**：Android/Linux/Harmony 不得把此结构体当主暴露层，
/// 应走 `api::WriterCoreApi` 或其绑定适配层。
///
/// 无状态：每次 API 调用通过 `core()` 创建临时 `WriterCore` 实例，
/// 不持有可变状态。`stats_api` 使用 `OnceLock` 懒初始化，首次访问后复用。
pub struct WriterCore {
    pub(crate) app_data_root: PathBuf,
    pub(crate) projects_root: PathBuf,
    pub(crate) stats_api: OnceLock<StatsApi>,
    pub(crate) sync_transport: Option<writer_platform_api::SyncTransportFactory>,
    pub(crate) secure_storage: Option<Arc<dyn writer_platform_api::SecureStorage>>,
    /// #644 评论 5462823517 第1节：删除 facade 层 secrets_override —
    /// 进程级 override 唯一存在于 `api::service::WriterCoreApi.secrets_override`，
    /// 避免两份状态漂移。
    pub(crate) search_service: std::sync::Mutex<SearchIndexService>,
    pub(crate) starmap_stores: std::sync::Mutex<HashMap<String, StarMapStore>>,
    /// #644 评论 5490799656 问题1：Android 私有 Git metadata 根目录。
    ///
    /// 位于 `context.filesDir/sujian-git/`，所有项目的可写 Git metadata
    ///（`.git/`）的根目录。`None` 表示使用标准 Git 布局。
    /// 构造 `GitRepoLayout` 时：`Some(root)` →
    ///   `GitRepoLayout::with_external_git_dir(project_root, root.join(project_id))`
    /// `None` → `GitRepoLayout::new(project_root)`。
    pub(crate) git_metadata_root: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChapterOpenResult {
    pub meta: Chapter,
    pub content: String,
}

impl WriterCore {
    pub fn new<P1: AsRef<Path>, P2: AsRef<Path>>(app_data_root: P1, projects_root: P2) -> Self {
        Self {
            app_data_root: app_data_root.as_ref().to_path_buf(),
            projects_root: projects_root.as_ref().to_path_buf(),
            stats_api: OnceLock::new(),
            sync_transport: None,
            secure_storage: None,
            search_service: std::sync::Mutex::new(SearchIndexService::new()),
            starmap_stores: std::sync::Mutex::new(HashMap::new()),
            git_metadata_root: None,
        }
    }

    pub(crate) fn get_stats_api(&self) -> &StatsApi {
        self.stats_api
            .get_or_init(|| StatsApi::new(&self.app_data_root))
    }

    pub fn app_data_root(&self) -> &Path {
        &self.app_data_root
    }

    pub fn projects_root(&self) -> &Path {
        &self.projects_root
    }

    /// 计算指定作品的根目录路径。
    pub(crate) fn project_root(&self, project_id: &str) -> PathBuf {
        self.projects_root.join(project_id)
    }

    /// #644 评论 5490799656 问题1：为指定作品构造 `GitRepoLayout`。
    ///
    /// Android 端 `git_metadata_root` 为 `Some(root)` 时，`git_dir` 放在
    /// `root/<project_id>/`（应用私有 `filesDir/sujian-git/<project-id>/`）。
    /// 其他平台或 Android 未配置时使用标准布局（`project_root.join(".git")`）。
    pub(crate) fn project_git_layout(
        &self,
        project_id: &str,
    ) -> crate::storage::git_repo_layout::GitRepoLayout {
        let project_root = self.project_root(project_id);
        match &self.git_metadata_root {
            Some(root) => crate::storage::git_repo_layout::GitRepoLayout::with_external_git_dir(
                project_root,
                root.join(project_id),
            ),
            None => crate::storage::git_repo_layout::GitRepoLayout::new(project_root),
        }
    }

    /// #644 评论 5491531984 问题1：为 App target 构造 `GitRepoLayout`。
    ///
    /// Android 端 `git_metadata_root` 为 `Some(root)` 时，`git_dir` 放在
    /// `root/app/`（应用私有 `filesDir/sujian-git/app/`）。
    /// 其他平台或 Android 未配置时使用标准布局（`app_data_root.join(".git")`）。
    pub(crate) fn app_git_layout(&self) -> crate::storage::git_repo_layout::GitRepoLayout {
        match &self.git_metadata_root {
            Some(root) => crate::storage::git_repo_layout::GitRepoLayout::with_external_git_dir(
                self.app_data_root.clone(),
                root.join("app"),
            ),
            None => crate::storage::git_repo_layout::GitRepoLayout::new(self.app_data_root.clone()),
        }
    }
}

impl Drop for WriterCore {
    fn drop(&mut self) {
        let _ = self.flush_all_starmap_stores();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_facade_basic_flow() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));

        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

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
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

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
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

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
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));

        assert!(core.move_chapter_to_trash("c1").is_err());
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn test_facade_sync_config_flow() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

        let config = core.load_sync_config().unwrap();
        assert!(!config.enabled);
        assert_eq!(config.active_provider, "github_api");

        let mut new_config = config.clone();
        new_config.enabled = true;
        new_config.set_github_config(
            "https://example.com/repo.git".to_string(),
            "main".to_string(),
            String::new(),
            crate::sync::types::SyncProtocol::HttpsToken,
        );
        core.save_sync_config(&new_config).unwrap();

        let loaded = core.load_sync_config().unwrap();
        assert!(loaded.enabled);
        assert_eq!(loaded.github_remote_url(), "https://example.com/repo.git");

        let mut secrets = core.load_sync_secrets().unwrap();
        secrets.provider_secrets = Some(crate::sync::provider::ProviderSecrets::GitHub {
            token: "my_super_secret_token".to_string(),
        });
        core.save_sync_secrets(&secrets).unwrap();

        let loaded_secrets = core.load_sync_secrets().unwrap();
        assert_eq!(
            loaded_secrets.github_token().as_deref().unwrap(),
            "my_super_secret_token"
        );

        assert!(core.validate_sync_config(&loaded).unwrap());

        let mut bad_config = loaded.clone();
        bad_config.set_github_config(
            String::new(),
            "main".to_string(),
            String::new(),
            crate::sync::types::SyncProtocol::HttpsToken,
        );
        assert!(!core.validate_sync_config(&bad_config).unwrap());
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn test_facade_generation_secrets_save_load_delete() {
        // #595 五：generation 凭据生命周期 — save → load → delete。
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

        let secrets = crate::sync::SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "generation_token_7".to_string(),
            }),
        };
        core.save_sync_secrets_for_generation(7, &secrets).unwrap();

        let loaded = core
            .load_sync_secrets_for_generation(7)
            .unwrap()
            .expect("generation 7 secrets must exist after save");
        assert_eq!(
            loaded.github_token().as_deref().unwrap(),
            "generation_token_7"
        );

        // 未保存的 generation 读取为 None。
        assert!(core.load_sync_secrets_for_generation(99).unwrap().is_none());

        // 删除后读取为 None；重复删除是幂等成功。
        core.delete_sync_secrets_for_generation(7).unwrap();
        assert!(core.load_sync_secrets_for_generation(7).unwrap().is_none());
        core.delete_sync_secrets_for_generation(7).unwrap();

        // 删除不影响其他 generation。
        core.save_sync_secrets_for_generation(8, &secrets).unwrap();
        core.delete_sync_secrets_for_generation(7).unwrap();
        assert!(core.load_sync_secrets_for_generation(8).unwrap().is_some());
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn test_load_sync_config_migrates_git_backend_for_github_https_remote() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

        let config_path = temp_dir.path().join("app-meta/sync/config.local.json");
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
        assert_eq!(loaded.active_provider, "github_api");

        let persisted = core.load_sync_config().unwrap();
        assert_eq!(persisted.active_provider, "github_api");
    }

    #[test]
    fn test_facade_perform_sync_dry_run() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

        let _project = core.create_project("Test Project").unwrap();
        let config = core.load_sync_config().unwrap();
        let secrets = core.load_sync_secrets().unwrap_or_default();
        let plan = core.perform_full_sync_dry_run(&config, &secrets).unwrap();
        // App target + 1 Project target，两个 target 都无文件需上传
        assert!(plan
            .targets
            .iter()
            .all(|t| t.plan.files_to_upload.is_empty()));
    }

    #[test]
    fn test_execute_action_args_parsing() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

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

    /// Issue #630：全局同步配置唯一，所有作品共享同一份 config。
    #[test]
    #[cfg(feature = "github-api")]
    fn test_sync_config_isolated_per_project() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

        let _project_a = core.create_project("Project A").unwrap();
        let _project_b = core.create_project("Project B").unwrap();

        // 全局配置初始为默认值
        let config0 = core.load_sync_config().unwrap();
        assert!(!config0.enabled);

        // 修改全局配置
        let mut config = config0.clone();
        config.enabled = true;
        config.set_github_config(
            "https://example.com/a.git".to_string(),
            "main".to_string(),
            String::new(),
            crate::sync::types::SyncProtocol::HttpsToken,
        );
        core.save_sync_config(&config).unwrap();

        // 再次加载仍是同一份
        let loaded = core.load_sync_config().unwrap();
        assert!(loaded.enabled);
        assert_eq!(loaded.github_remote_url(), "https://example.com/a.git");
    }

    /// Issue #630：全局同步凭据唯一，所有作品共享同一份 secrets。
    #[test]
    #[cfg(feature = "github-api")]
    fn test_sync_secrets_isolated_per_project() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

        let _project_a = core.create_project("Project A").unwrap();
        let _project_b = core.create_project("Project B").unwrap();

        // 保存全局凭据
        let secrets = crate::sync::SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "token_global_123".to_string(),
            }),
        };
        core.save_sync_secrets(&secrets).unwrap();

        let loaded = core.load_sync_secrets().unwrap();
        assert_eq!(loaded.github_token().as_deref(), Some("token_global_123"));

        // generation 凭据也全局唯一
        let gen_secrets = crate::sync::SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "gen_token_global".to_string(),
            }),
        };
        core.save_sync_secrets_for_generation(1, &gen_secrets)
            .unwrap();

        let loaded_gen = core.load_sync_secrets_for_generation(1).unwrap();
        assert_eq!(
            loaded_gen.unwrap().github_token().as_deref(),
            Some("gen_token_global")
        );
    }

    /// Issue #600 评论 #3 问题四：应用级白名单/黑名单正确过滤路径。
    /// 应用级同步根 = app_data_root，白名单 settings.sync.json/starmaps/themes，
    /// 黑名单 作品目录/日志/导出/备份/settings.local.json/sync secrets/device/缓存统计。
    #[test]
    fn test_app_level_whitelist_blacklist() {
        use crate::sync::SyncScope;
        use crate::sync::SyncService;

        // ── 应用级白名单 ──
        assert!(SyncService::is_whitelisted_path(
            "settings.sync.json",
            SyncScope::App
        ));
        assert!(SyncService::is_whitelisted_path(
            "starmaps/global.json",
            SyncScope::App
        ));
        assert!(SyncService::is_whitelisted_path(
            "starmaps/constellations/orion.json",
            SyncScope::App
        ));
        assert!(SyncService::is_whitelisted_path(
            "themes/palettes/dark.json",
            SyncScope::App
        ));

        // 作品正文不在应用级白名单
        assert!(!SyncService::is_whitelisted_path(
            "project.json",
            SyncScope::App
        ));
        assert!(!SyncService::is_whitelisted_path(
            "volumes/v1/chapters/c1.txt",
            SyncScope::App
        ));

        // ── 应用级黑名单 ──
        // settings.local.json 被黑名单
        assert!(SyncService::is_blacklisted_path(
            "settings.local.json",
            SyncScope::App
        ));
        // recent_edits.json 被黑名单
        assert!(SyncService::is_blacklisted_path(
            "recent_edits.json",
            SyncScope::App
        ));
        // sync secrets 被黑名单
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/secrets.local.json",
            SyncScope::App
        ));
        // device 目录被黑名单
        assert!(SyncService::is_blacklisted_path(
            "device/id.local.json",
            SyncScope::App
        ));

        // ── 作品级白名单/黑名单不受应用级影响 ──
        // 作品正文在作品级白名单
        assert!(SyncService::is_whitelisted_path(
            "project.json",
            SyncScope::Project
        ));
        // settings.sync.json 不在作品级白名单
        assert!(!SyncService::is_whitelisted_path(
            "settings.sync.json",
            SyncScope::Project
        ));
    }

    /// Issue #630：全局同步配置唯一，不再有"应用级 vs 作品级"两套配置。
    #[test]
    #[cfg(feature = "github-api")]
    fn test_app_sync_config_independent_from_project() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path(), temp_dir.path().join("projects"));
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();

        let _project = core.create_project("Test Project").unwrap();

        // 全局配置即唯一配置
        let mut config = core.load_sync_config().unwrap();
        config.enabled = true;
        config.set_github_config(
            "https://example.com/global.git".to_string(),
            "main".to_string(),
            String::new(),
            crate::sync::types::SyncProtocol::HttpsToken,
        );
        core.save_sync_config(&config).unwrap();

        let loaded = core.load_sync_config().unwrap();
        assert_eq!(loaded.github_remote_url(), "https://example.com/global.git");
        assert!(loaded.enabled);
    }
}
