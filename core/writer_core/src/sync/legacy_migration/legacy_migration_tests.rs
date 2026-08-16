//! 旧同步配置迁移行为测试（Issue #630 评论第 4 点 / D、第 5 点 Part C）。
//!
//! 覆盖：app/project 探测优先级、精确 generation metadata、多项目一致/冲突、
//! 失败保留旧凭据、成功后清理旧凭据。

use super::*;

mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;
    use writer_platform_api::SecureStorage;

    /// 桩安全存储：HashMap 存 key→bytes，可注入 set_secret 失败。
    struct StubSecureStorage {
        map: Mutex<HashMap<String, Vec<u8>>>,
        set_fails: Mutex<bool>,
    }

    impl StubSecureStorage {
        fn new() -> Self {
            Self {
                map: Mutex::new(HashMap::new()),
                set_fails: Mutex::new(false),
            }
        }

        fn inject_set_failure(&self) {
            *self.set_fails.lock().expect("set_fails poisoned") = true;
        }

        fn contains_key(&self, key: &str) -> bool {
            self.map.lock().expect("map poisoned").contains_key(key)
        }
    }

    impl writer_platform_api::SecureStorage for StubSecureStorage {
        fn get_secret(&self, key: &str) -> std::result::Result<Option<Vec<u8>>, String> {
            Ok(self.map.lock().expect("map poisoned").get(key).cloned())
        }
        fn set_secret(&self, key: &str, value: &[u8]) -> std::result::Result<(), String> {
            if *self.set_fails.lock().expect("set_fails poisoned") {
                return Err("injected set_secret failure".to_string());
            }
            self.map
                .lock()
                .expect("map poisoned")
                .insert(key.to_string(), value.to_vec());
            Ok(())
        }
        fn delete_secret(&self, key: &str) -> std::result::Result<(), String> {
            self.map.lock().expect("map poisoned").remove(key);
            Ok(())
        }
    }

    fn sample_config(remote_url: &str, branch: &str) -> SyncConfig {
        SyncConfig {
            enabled: true,
            backend_type: crate::sync::BackendType::GithubApi,
            remote_url: remote_url.to_string(),
            transport: crate::sync::SyncProtocol::HttpsToken,
            branch: branch.to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        }
    }

    fn write_config_file(root: &Path, config: &SyncConfig) {
        let path = root.join("app-meta/sync/config.local.json");
        std::fs::create_dir_all(path.parent().expect("parent exists")).expect("mkdir");
        let content = serde_json::to_string_pretty(config).expect("serialize");
        std::fs::write(&path, content).expect("write config");
    }

    fn write_project_meta(projects_root: &Path, project_id: &str) {
        let dir = projects_root.join(project_id);
        std::fs::create_dir_all(&dir).expect("mkdir project");
        std::fs::write(
            dir.join("project.json"),
            format!(
                r#"{{"id":"{}","title":"{}","created_at":"2024-01-01T00:00:00Z","updated_at":"2024-01-01T00:00:00Z","order":0}}"#,
                project_id, project_id
            ),
        )
        .expect("write project.json");
    }

    fn app_metadata(generation: Option<u32>) -> LegacyProfileMetadata {
        LegacyProfileMetadata {
            source: "app".to_string(),
            project_id: None,
            active_generation: generation,
        }
    }

    fn project_metadata(project_id: &str, generation: Option<u32>) -> LegacyProfileMetadata {
        LegacyProfileMetadata {
            source: format!("project:{}", project_id),
            project_id: Some(project_id.to_string()),
            active_generation: generation,
        }
    }

    struct TestEnv {
        _tmp: TempDir,
        app_data_root: PathBuf,
        projects_root: PathBuf,
        storage: Arc<StubSecureStorage>,
    }

    impl TestEnv {
        fn new() -> Self {
            let tmp = tempfile::tempdir().expect("tempdir");
            let app_data_root = tmp.path().join("app-data");
            let projects_root = tmp.path().join("projects");
            std::fs::create_dir_all(&app_data_root).expect("mkdir app-data");
            std::fs::create_dir_all(&projects_root).expect("mkdir projects");
            Self {
                _tmp: tmp,
                app_data_root,
                projects_root,
                storage: Arc::new(StubSecureStorage::new()),
            }
        }

        fn migrator(&self) -> LegacySyncProfileMigrator<'_> {
            LegacySyncProfileMigrator::new(
                &self.app_data_root,
                &self.projects_root,
                Some(self.storage.as_ref() as &dyn writer_platform_api::SecureStorage),
            )
        }
    }

    /// 1. 旧 app token（base key）：新全局不存在；旧 `sync_token_app` 有值 + app 配置 → 迁移成功。
    #[test]
    fn test_legacy_app_token_migration() {
        let env = TestEnv::new();
        let config = sample_config("https://github.com/test/repo.git", "main");
        write_config_file(&env.app_data_root, &config);
        env.storage
            .set_secret("sync_token_app", b"legacy_app_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated {
                config: c,
                secrets: s,
            } => {
                assert_eq!(c.remote_url, "https://github.com/test/repo.git");
                assert_eq!(s.token.as_deref(), Some("legacy_app_token"));
            }
            other => panic!("expected Migrated, got {:?}", other),
        }

        assert!(env.storage.contains_key("sync_token_global"));
        assert!(!env.storage.contains_key("sync_token_app"));
    }

    /// 1b. 旧 app generation token：`sync_token_app_g3` + metadata active_generation=3 → 迁移成功。
    #[test]
    fn test_legacy_app_generation_token_migration_with_metadata() {
        let env = TestEnv::new();
        let config = sample_config("https://github.com/test/repo.git", "main");
        write_config_file(&env.app_data_root, &config);
        env.storage
            .set_secret("sync_token_app_g3", b"gen3_token")
            .expect("set");

        let metadata = vec![app_metadata(Some(3))];
        let outcome = env
            .migrator()
            .migrate_with_metadata(&metadata)
            .expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated { secrets: s, .. } => {
                assert_eq!(s.token.as_deref(), Some("gen3_token"));
            }
            other => panic!("expected Migrated, got {:?}", other),
        }
        assert!(env.storage.contains_key("sync_token_global"));
        assert!(!env.storage.contains_key("sync_token_app_g3"));
    }

    /// 1c. 无 metadata 时 generation key 不被读取（不猜测 generation）。
    #[test]
    fn test_no_metadata_does_not_guess_generation() {
        let env = TestEnv::new();
        let config = sample_config("https://github.com/test/repo.git", "main");
        write_config_file(&env.app_data_root, &config);
        env.storage
            .set_secret("sync_token_app_g3", b"gen3_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        assert_eq!(outcome, LegacyMigrationOutcome::NoLegacyConfig);
        assert!(env.storage.contains_key("sync_token_app_g3"));
    }

    /// 2. 单项目迁移：新全局不存在；app 级无旧配置；一个 project 有配置 → 迁移成功。
    #[test]
    fn test_single_project_migration() {
        let env = TestEnv::new();
        write_project_meta(&env.projects_root, "proj1");
        let config = sample_config("https://github.com/test/proj1.git", "main");
        write_config_file(&env.projects_root.join("proj1"), &config);
        env.storage
            .set_secret("sync_token_proj1", b"proj1_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated {
                config: c,
                secrets: s,
            } => {
                assert_eq!(c.remote_url, "https://github.com/test/proj1.git");
                assert_eq!(s.token.as_deref(), Some("proj1_token"));
            }
            other => panic!("expected Migrated, got {:?}", other),
        }
        assert!(env.storage.contains_key("sync_token_global"));
        assert!(!env.storage.contains_key("sync_token_proj1"));
        let old_config = env
            .projects_root
            .join("proj1/app-meta/sync/config.local.json");
        assert!(!old_config.exists());
        let new_config = env.app_data_root.join("app-meta/sync/config.local.json");
        assert!(new_config.exists());
    }

    /// 3. 多项目一致：两个 project 旧 profile 完全一致 → 迁一份，两个旧 token 都清理。
    #[test]
    fn test_multiple_projects_consistent_migration_cleans_all() {
        let env = TestEnv::new();
        write_project_meta(&env.projects_root, "proj1");
        write_project_meta(&env.projects_root, "proj2");
        let config = sample_config("https://github.com/test/shared.git", "main");
        write_config_file(&env.projects_root.join("proj1"), &config);
        write_config_file(&env.projects_root.join("proj2"), &config);
        env.storage
            .set_secret("sync_token_proj1", b"shared_token")
            .expect("set");
        env.storage
            .set_secret("sync_token_proj2", b"shared_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated {
                config: c,
                secrets: s,
            } => {
                assert_eq!(c.remote_url, "https://github.com/test/shared.git");
                assert_eq!(s.token.as_deref(), Some("shared_token"));
            }
            other => panic!("expected Migrated, got {:?}", other),
        }
        assert!(env.storage.contains_key("sync_token_global"));
        assert!(!env.storage.contains_key("sync_token_proj1"));
        assert!(!env.storage.contains_key("sync_token_proj2"));
        let old_config1 = env
            .projects_root
            .join("proj1/app-meta/sync/config.local.json");
        let old_config2 = env
            .projects_root
            .join("proj2/app-meta/sync/config.local.json");
        assert!(!old_config1.exists());
        assert!(!old_config2.exists());
    }

    /// 3b. 三个项目一致：3 个 project 旧 profile 完全一致 → 迁一份，3 个旧凭据全部清理。
    #[test]
    fn test_three_projects_consistent_migration_cleans_all() {
        let env = TestEnv::new();
        for id in ["proj1", "proj2", "proj3"] {
            write_project_meta(&env.projects_root, id);
            let config = sample_config("https://github.com/test/shared.git", "main");
            write_config_file(&env.projects_root.join(id), &config);
            env.storage
                .set_secret(&format!("sync_token_{}", id), b"shared_token")
                .expect("set");
        }

        let outcome = env.migrator().migrate().expect("migrate");
        assert!(matches!(outcome, LegacyMigrationOutcome::Migrated { .. }));

        assert!(env.storage.contains_key("sync_token_global"));
        for id in ["proj1", "proj2", "proj3"] {
            assert!(
                !env.storage.contains_key(&format!("sync_token_{}", id)),
                "old token for {} should be cleaned",
                id
            );
            let old_config = env
                .projects_root
                .join(format!("{}/app-meta/sync/config.local.json", id));
            assert!(
                !old_config.exists(),
                "old config for {} should be cleaned",
                id
            );
        }
    }

    /// 4. 不一致 NeedsReconfigure：两个 project 仓库或 token 不同 → NeedsReconfigure，不删旧凭据。
    #[test]
    fn test_multiple_projects_inconsistent_needs_reconfigure() {
        let env = TestEnv::new();
        write_project_meta(&env.projects_root, "proj1");
        write_project_meta(&env.projects_root, "proj2");
        let config1 = sample_config("https://github.com/test/repo1.git", "main");
        let config2 = sample_config("https://github.com/test/repo2.git", "main");
        write_config_file(&env.projects_root.join("proj1"), &config1);
        write_config_file(&env.projects_root.join("proj2"), &config2);
        env.storage
            .set_secret("sync_token_proj1", b"token1")
            .expect("set");
        env.storage
            .set_secret("sync_token_proj2", b"token2")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        match outcome {
            LegacyMigrationOutcome::NeedsReconfigure { reason } => {
                assert!(reason.contains("conflicting"));
                assert!(reason.contains("project:proj1"));
                assert!(reason.contains("project:proj2"));
            }
            other => panic!("expected NeedsReconfigure, got {:?}", other),
        }
        assert!(env.storage.contains_key("sync_token_proj1"));
        assert!(env.storage.contains_key("sync_token_proj2"));
        assert!(!env.storage.contains_key("sync_token_global"));
        let new_config = env.app_data_root.join("app-meta/sync/config.local.json");
        assert!(!new_config.exists());
    }

    /// 5. 迁移失败保留凭据：模拟 set_secret 返回 Err → 旧 token 仍在。
    #[test]
    fn test_migration_failure_preserves_credentials() {
        let env = TestEnv::new();
        let config = sample_config("https://github.com/test/repo.git", "main");
        write_config_file(&env.app_data_root, &config);
        env.storage
            .set_secret("sync_token_app", b"legacy_app_token")
            .expect("set");
        env.storage.inject_set_failure();

        let result = env.migrator().migrate();
        assert!(result.is_err(), "expected migration to fail");

        assert!(env.storage.contains_key("sync_token_app"));
        assert!(!env.storage.contains_key("sync_token_global"));
    }

    /// 5b. 多 project 一致但提交失败：所有旧凭据保留（失败前不清理）。
    #[test]
    fn test_migration_failure_preserves_all_participating() {
        let env = TestEnv::new();
        for id in ["proj1", "proj2"] {
            write_project_meta(&env.projects_root, id);
            let config = sample_config("https://github.com/test/shared.git", "main");
            write_config_file(&env.projects_root.join(id), &config);
            env.storage
                .set_secret(&format!("sync_token_{}", id), b"shared_token")
                .expect("set");
        }
        env.storage.inject_set_failure();

        let result = env.migrator().migrate();
        assert!(result.is_err(), "expected migration to fail");

        assert!(env.storage.contains_key("sync_token_proj1"));
        assert!(env.storage.contains_key("sync_token_proj2"));
        assert!(!env.storage.contains_key("sync_token_global"));
    }

    /// 6. 成功后清理：迁移成功并完整提交后 → 旧 `sync_token_app` 被删除。
    #[test]
    fn test_successful_migration_cleans_up() {
        let env = TestEnv::new();
        let config = sample_config("https://github.com/test/repo.git", "main");
        write_config_file(&env.app_data_root, &config);
        env.storage
            .set_secret("sync_token_app", b"legacy_app_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        assert!(matches!(outcome, LegacyMigrationOutcome::Migrated { .. }));

        assert!(!env.storage.contains_key("sync_token_app"));
        assert!(env.storage.contains_key("sync_token_global"));
    }

    /// 6b. 成功后清理作品级：旧 `sync_token_<id>` 和旧 config 文件被删除。
    #[test]
    fn test_successful_project_migration_cleans_up() {
        let env = TestEnv::new();
        write_project_meta(&env.projects_root, "proj1");
        let config = sample_config("https://github.com/test/proj1.git", "main");
        write_config_file(&env.projects_root.join("proj1"), &config);
        env.storage
            .set_secret("sync_token_proj1", b"proj1_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        assert!(matches!(outcome, LegacyMigrationOutcome::Migrated { .. }));

        assert!(!env.storage.contains_key("sync_token_proj1"));
        let old_config = env
            .projects_root
            .join("proj1/app-meta/sync/config.local.json");
        assert!(!old_config.exists());
    }

    /// 7. 新全局已存在 → NotNeeded：不读旧配置。
    #[test]
    fn test_new_global_exists_not_needed() {
        let env = TestEnv::new();
        let config = sample_config("https://github.com/test/existing.git", "main");
        write_config_file(&env.app_data_root, &config);
        env.storage
            .set_secret("sync_token_global", b"existing_global_token")
            .expect("set");
        env.storage
            .set_secret("sync_token_app", b"legacy_app_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        assert_eq!(outcome, LegacyMigrationOutcome::NotNeeded);

        assert!(env.storage.contains_key("sync_token_app"));
        assert!(env.storage.contains_key("sync_token_global"));
    }

    /// 8. 无任何旧配置 → NoLegacyConfig。
    #[test]
    fn test_no_legacy_config() {
        let env = TestEnv::new();
        let outcome = env.migrator().migrate().expect("migrate");
        assert_eq!(outcome, LegacyMigrationOutcome::NoLegacyConfig);
    }

    /// 9. fallback 文件路径：安全存储无 token，旧作品级 secrets.local.json 有 token → 迁移成功。
    #[test]
    fn test_fallback_secrets_file_migration() {
        let env = TestEnv::new();
        write_project_meta(&env.projects_root, "proj1");
        let config = sample_config("https://github.com/test/proj1.git", "main");
        write_config_file(&env.projects_root.join("proj1"), &config);
        let secrets_path = env
            .projects_root
            .join("proj1/app-meta/sync/secrets.local.json");
        std::fs::create_dir_all(secrets_path.parent().expect("parent")).expect("mkdir");
        std::fs::write(
            &secrets_path,
            r#"{"token":"file_token","ssh_private_key":null}"#,
        )
        .expect("write secrets");

        let outcome = env.migrator().migrate().expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated { secrets: s, .. } => {
                assert_eq!(s.token.as_deref(), Some("file_token"));
            }
            other => panic!("expected Migrated, got {:?}", other),
        }
        assert!(!secrets_path.exists());
    }

    /// 10. app 优先：旧 app profile 和旧 project profile 指向不同仓库 → 用 app profile 迁移成功
    ///     （不 NeedsReconfigure，app 优先级高于 project，不比较 app ↔ project）。
    #[test]
    fn test_app_priority_over_project_different_repos() {
        let env = TestEnv::new();
        let app_config = sample_config("https://github.com/app/repo.git", "main");
        write_config_file(&env.app_data_root, &app_config);
        env.storage
            .set_secret("sync_token_app", b"app_token")
            .expect("set");
        write_project_meta(&env.projects_root, "proj1");
        let proj_config = sample_config("https://github.com/proj/repo.git", "main");
        write_config_file(&env.projects_root.join("proj1"), &proj_config);
        env.storage
            .set_secret("sync_token_proj1", b"proj_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated {
                config: c,
                secrets: s,
            } => {
                assert_eq!(c.remote_url, "https://github.com/app/repo.git");
                assert_eq!(s.token.as_deref(), Some("app_token"));
            }
            other => panic!("expected Migrated (app priority), got {:?}", other),
        }
        assert!(!env.storage.contains_key("sync_token_app"));
        assert!(env.storage.contains_key("sync_token_proj1"));
        assert!(env.storage.contains_key("sync_token_global"));
    }

    /// 10b. app 优先 + metadata：旧 app profile 用精确 generation，project 不同 → 仍用 app 迁移。
    #[test]
    fn test_app_priority_with_metadata_different_repos() {
        let env = TestEnv::new();
        let app_config = sample_config("https://github.com/app/repo.git", "main");
        write_config_file(&env.app_data_root, &app_config);
        env.storage
            .set_secret("sync_token_app_g7", b"app_gen7_token")
            .expect("set");
        write_project_meta(&env.projects_root, "proj1");
        let proj_config = sample_config("https://github.com/proj/repo.git", "main");
        write_config_file(&env.projects_root.join("proj1"), &proj_config);
        env.storage
            .set_secret("sync_token_proj1", b"proj_token")
            .expect("set");

        let metadata = vec![app_metadata(Some(7)), project_metadata("proj1", None)];
        let outcome = env
            .migrator()
            .migrate_with_metadata(&metadata)
            .expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated { secrets: s, .. } => {
                assert_eq!(s.token.as_deref(), Some("app_gen7_token"));
            }
            other => panic!("expected Migrated (app priority), got {:?}", other),
        }
        assert!(!env.storage.contains_key("sync_token_app_g7"));
        assert!(env.storage.contains_key("sync_token_proj1"));
    }

    /// 11. 准确 generation > 10：`sync_token_app_g15` 有值，metadata active_generation=15 → 迁移成功。
    #[test]
    fn test_precise_generation_greater_than_ten() {
        let env = TestEnv::new();
        let config = sample_config("https://github.com/test/repo.git", "main");
        write_config_file(&env.app_data_root, &config);
        env.storage
            .set_secret("sync_token_app_g15", b"gen15_token")
            .expect("set");

        let metadata = vec![app_metadata(Some(15))];
        let outcome = env
            .migrator()
            .migrate_with_metadata(&metadata)
            .expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated { secrets: s, .. } => {
                assert_eq!(s.token.as_deref(), Some("gen15_token"));
            }
            other => panic!("expected Migrated, got {:?}", other),
        }
        assert!(env.storage.contains_key("sync_token_global"));
        assert!(!env.storage.contains_key("sync_token_app_g15"));
    }

    /// 11b. project 精确 generation > 10：`sync_token_<id>_g20` 有值 + metadata → 迁移成功。
    #[test]
    fn test_project_precise_generation_greater_than_ten() {
        let env = TestEnv::new();
        write_project_meta(&env.projects_root, "proj1");
        let config = sample_config("https://github.com/test/proj1.git", "main");
        write_config_file(&env.projects_root.join("proj1"), &config);
        env.storage
            .set_secret("sync_token_proj1_g20", b"gen20_token")
            .expect("set");

        let metadata = vec![project_metadata("proj1", Some(20))];
        let outcome = env
            .migrator()
            .migrate_with_metadata(&metadata)
            .expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated { secrets: s, .. } => {
                assert_eq!(s.token.as_deref(), Some("gen20_token"));
            }
            other => panic!("expected Migrated, got {:?}", other),
        }
        assert!(env.storage.contains_key("sync_token_global"));
        assert!(!env.storage.contains_key("sync_token_proj1_g20"));
    }

    /// 12. 无 metadata fallback：`migrate()` 无 metadata，只有 base key → 仍可迁移。
    #[test]
    fn test_no_metadata_fallback_base_key() {
        let env = TestEnv::new();
        write_project_meta(&env.projects_root, "proj1");
        let config = sample_config("https://github.com/test/proj1.git", "main");
        write_config_file(&env.projects_root.join("proj1"), &config);
        env.storage
            .set_secret("sync_token_proj1", b"proj1_token")
            .expect("set");

        let outcome = env.migrator().migrate().expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated { secrets: s, .. } => {
                assert_eq!(s.token.as_deref(), Some("proj1_token"));
            }
            other => panic!("expected Migrated, got {:?}", other),
        }
        assert!(env.storage.contains_key("sync_token_global"));
        assert!(!env.storage.contains_key("sync_token_proj1"));
    }

    /// 13. metadata 中 active_generation=None → 回退 base key / 文件。
    #[test]
    fn test_metadata_none_generation_falls_back_to_base_key() {
        let env = TestEnv::new();
        let config = sample_config("https://github.com/test/repo.git", "main");
        write_config_file(&env.app_data_root, &config);
        env.storage
            .set_secret("sync_token_app", b"base_token")
            .expect("set");
        env.storage
            .set_secret("sync_token_app_g5", b"gen5_token")
            .expect("set");

        let metadata = vec![app_metadata(None)];
        let outcome = env
            .migrator()
            .migrate_with_metadata(&metadata)
            .expect("migrate");
        match outcome {
            LegacyMigrationOutcome::Migrated { secrets: s, .. } => {
                assert_eq!(s.token.as_deref(), Some("base_token"));
            }
            other => panic!("expected Migrated, got {:?}", other),
        }
        assert!(!env.storage.contains_key("sync_token_app"));
        assert!(env.storage.contains_key("sync_token_app_g5"));
    }
}
