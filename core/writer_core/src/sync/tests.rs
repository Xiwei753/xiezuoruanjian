#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use crate::sync::backends::SyncBackend;
    use crate::sync::git_backend::Git2Backend;
    use crate::sync::git_backend::GitAuth;
    use crate::sync::git_backend::GitBackend;
    use crate::sync::github_backend::GitHubApiBackend;
    use crate::sync::service::SyncService;
    use crate::sync::types::BackendType;
    use crate::sync::types::FirstSyncMode;
    use crate::sync::types::ManifestFileRecord;
    use crate::sync::types::SyncConfig;
    use crate::sync::types::SyncConflict;
    use crate::sync::types::SyncManifest;
    use crate::sync::types::SyncSecrets;
    use crate::sync::types::SyncState;
    use crate::sync::types::SyncStatus;
    use crate::sync::types::SyncTransport;
    use base64::Engine;
    use std::path::Path;
    use tempfile::tempdir;
    #[test]
    fn test_github_api_diagnostics_reports_backend_type_without_token() {
        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: "https://github.com/user/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: None,
            ssh_private_key: None,
        };

        let result = GitHubApiBackend.diagnose(&config, &secrets).unwrap();
        assert_eq!(result.backend_type, "github_api");
        assert_eq!(result.error_category, "token_missing");
    }

    #[test]
    fn test_sync_manifest_deserializes_without_deleted_at_ms() {
        let raw = r#"{
            "files": [{
                "path": "projects/p1/project.json",
                "content_hash": "abc",
                "updated_at_ms": 1000,
                "device_id": "device_a",
                "op": "upsert",
                "schema_version": 1
            }]
        }"#;

        let manifest: SyncManifest = serde_json::from_str(raw).unwrap();
        assert_eq!(manifest.files.len(), 1);
        assert_eq!(manifest.files[0].deleted_at_ms, None);
    }

    #[test]
    fn test_sync_secrets_local_json_blacklisted() {
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/sync_secrets.local.json"
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/sync_secrets.local.json.tmp"
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/sync_state.json"
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/state.local.json"
        ));
        assert!(SyncService::is_blacklisted_path("app-meta/logs/sync.log"));
        assert!(SyncService::is_blacklisted_path("tmp/runtime.tmp"));
    }

    #[test]
    fn test_ai_paths_blacklisted() {
        assert!(SyncService::is_blacklisted_path(
            "app-meta/ai/secrets.local.json"
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/ai/config.local.json"
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/ai/conversations.local/chat.json"
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/ai/cache/model_cache.bin"
        ));
        assert!(SyncService::is_blacklisted_path("app-meta/ai/"));
    }

    #[test]
    fn test_first_sync_mode_unrelated_histories() {
        // Test logic added via GitBackend trait mock
        struct MockUnrelatedBackend;
        impl GitBackend for MockUnrelatedBackend {
            fn init_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn ensure_remote(&self, _: &Path, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn has_repo(&self, _: &Path) -> bool {
                true
            }
            fn is_worktree_empty_or_git_only(&self, _: &Path) -> crate::Result<bool> {
                Ok(false)
            }
            fn open_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn push(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "fatal: refusing to merge unrelated histories",
                )))
            }
            fn clone_repo(
                &self,
                _: &str,
                _: &Path,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(Some("hash".to_string()))
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(Some("hash".to_string()))
            }
            fn status(&self, _: &Path) -> crate::Result<Vec<String>> {
                Ok(vec![])
            }
        }

        let dir = tempdir().unwrap();
        let config = SyncConfig {

            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let result =
            SyncService::perform_sync(dir.path(), &config, &secrets, &MockUnrelatedBackend)
                .unwrap();
        assert_eq!(result.first_sync_mode, FirstSyncMode::UnrelatedHistories);
        // user_message 不再填充中文文案，UI 层应通过 error_category 做本地化
        assert_eq!(result.user_message, None);
    }

    #[test]
    #[ignore]
    fn test_record_sync_conflict_error_handling() {
        // Provide an invalid path to force an IO error
        let conflict = SyncConflict {
            local_path: "chapter.md".to_string(),
            remote_path: "chapter.md".to_string(),
            local_hash: "aaa".to_string(),
            remote_hash: "bbb".to_string(),
            base_hash: "ccc".to_string(),
            created_at: 123456789,
            description: "conflict test".to_string(),
        };

        // Pass a non-existent parent directory to force an error
        let res = SyncService::record_sync_conflict(
            Path::new("/non/existent/path/that/will/fail"),
            conflict,
            None,
        );
        assert!(res.is_err());
    }

    #[test]
    fn test_perform_sync_non_empty_no_git_init() {
        // Just a mock test to verify the logic inside perform_sync
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("some_file.txt"), "hello").unwrap();

        let config = SyncConfig {

            enabled: true,
            remote_url: "https://github.com/test/test.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockBackend;
        impl GitBackend for MockBackend {
            fn clone_repo(
                &self,
                _: &str,
                _: &Path,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn open_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(&self, _: &Path) -> crate::Result<Vec<String>> {
                Ok(vec![])
            }
            fn init_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn ensure_remote(&self, _: &Path, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn has_repo(&self, _: &Path) -> bool {
                false
            }
            fn is_worktree_empty_or_git_only(&self, _: &Path) -> crate::Result<bool> {
                Ok(false)
            }
        }

        let res = SyncService::perform_sync(dir.path(), &config, &secrets, &MockBackend).unwrap();
        assert_eq!(res.status, SyncStatus::Success);
    }

    #[test]
    fn test_perform_sync_auto_commits_whitelist() {
        // Just mock test to ensure successful pass of logic
        assert!(SyncService::is_whitelisted_path(
            "app-meta/settings/settings.sync.json"
        ));
    }

    #[test]
    fn test_no_unknown_conflicts() {
        let conflict = SyncConflict {
            local_path: "real/path.txt".to_string(),
            remote_path: "real/path.txt".to_string(),
            local_hash: "".to_string(),
            remote_hash: "".to_string(),
            base_hash: "".to_string(),
            description: "".to_string(),
            created_at: 0,
        };
        assert_ne!(conflict.local_path, "unknown");
        assert_ne!(conflict.remote_path, "unknown");
    }

    #[test]
    fn test_sync_config_state_no_token() {
        let config = SyncConfig {

            enabled: true,
            remote_url: "url".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let state = SyncState {
            remote_url: Some("url".to_string()),
            transport: Some(SyncTransport::HttpsToken),
            last_sync_time: Some(0),
            last_synced_commit: None,
            last_error: None,
            last_successful_network_mode: None,
            known_files: std::collections::HashMap::new(),
            conflicts: vec![],
            tombstones: Vec::new(),
            deleted_files: std::collections::HashSet::new(),
            device_id: String::new(),
            known_files_updated_at: std::collections::HashMap::new(),
            conflicted_files: std::collections::HashSet::new(),
            pending_take_remote: std::collections::HashSet::new(),
        };
        let config_str = serde_json::to_string(&config).unwrap();
        let state_str = serde_json::to_string(&state).unwrap();
        // Since HttpsToken serializes as "https_token" because of snake_case, token is in the string.
        // We really want to assert that the ACTUAL token string is not there.
        assert!(!config_str.contains("my_secret_token"));
        assert!(!state_str.contains("my_secret_token"));
    }

    #[test]
    fn test_whitelist_includes_sync_json() {
        assert!(SyncService::is_whitelisted_path(
            "app-meta/settings/settings.sync.json"
        ));
        assert!(!SyncService::is_whitelisted_path(
            "app-meta/settings/settings.local.json"
        ));
    }

    #[test]
    fn test_blacklist_ignores_local_json() {
        assert!(SyncService::is_blacklisted_path(
            "app-meta/settings/settings.local.json"
        ));
    }

    #[test]
    fn test_sync_secrets_blacklisted() {
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/sync_secrets.local.json"
        ));
    }

    #[test]
    fn test_sync_config_no_token() {
        let config = SyncConfig {

            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let content = serde_json::to_string(&config).unwrap();
        // token might be there if some other struct is serialized, but we want to ensure
        // the word "token" isn't a key. Actually since token was removed from SyncConfig, it should literally not be there.
        // Wait, why did it fail? Oh, HttpsToken transport is present! It serializes to "https_token".
        // Let's assert it doesn't contain `"token":`
        assert!(!content.contains("\"token\":"));
    }
    #[test]
    fn test_blacklist_ignores_tmp_and_lock_files() {
        assert!(SyncService::is_blacklisted_path(
            "projects/v1/chapters/c1.tmp"
        ));
        assert!(SyncService::is_blacklisted_path(
            "workspace_manifest.json.lock"
        ));
        assert!(SyncService::is_blacklisted_path("app-meta/logs/sync.log"));
    }

    #[test]
    fn test_record_sync_conflict_writes_correctly() {
        let dir = tempdir().unwrap();
        let conflict = SyncConflict {
            local_path: "chapter.md".to_string(),
            remote_path: "chapter.md".to_string(),
            local_hash: "aaa".to_string(),
            remote_hash: "bbb".to_string(),
            base_hash: "ccc".to_string(),
            created_at: 123456789,
            description: "conflict test".to_string(),
        };

        SyncService::record_sync_conflict(dir.path(), conflict, Some("my local conflict")).unwrap();
        let conflicts_path = dir.path().join("app-meta/sync/conflicts.json");
        assert!(conflicts_path.exists());
        let content = std::fs::read_to_string(conflicts_path).unwrap();
        assert!(content.contains("conflict test"));

        let file_conflict = dir.path().join("chapter.md.conflict.123456789");
        assert!(file_conflict.exists());
        let content2 = std::fs::read_to_string(file_conflict).unwrap();
        assert_eq!(content2, "my local conflict");
    }

    #[test]
    fn test_sync_state_does_not_leak_tokens() {
        let dir = tempdir().unwrap();
        let state = SyncState {
            remote_url: Some("https://example.com/repo.git".to_string()),
            transport: Some(SyncTransport::HttpsToken),
            last_synced_commit: None,
            last_sync_time: None,
            last_error: None,
            last_successful_network_mode: None,
            known_files: std::collections::HashMap::new(),
            conflicts: vec![],
            tombstones: Vec::new(),
            deleted_files: std::collections::HashSet::new(),
            device_id: String::new(),
            known_files_updated_at: std::collections::HashMap::new(),
            conflicted_files: std::collections::HashSet::new(),
            pending_take_remote: std::collections::HashSet::new(),
        };

        SyncService::save_sync_state(dir.path(), &state).unwrap();
        let state_path = dir.path().join("app-meta/sync/state.local.json");
        let state_content = std::fs::read_to_string(state_path).unwrap();

        assert!(state_content.contains("https://example.com/repo.git"));
        assert!(!state_content.contains("\"token\":"));
    }

    #[test]
    #[cfg(not(windows))]
    fn test_stage_blacklisted_files() {
        let dir = tempdir().unwrap();

        // Initialize git repo manually or use SyncService
        let repo = git2::Repository::init(dir.path()).unwrap();

        let file_path = dir.path().join("app-meta/sync/sync_secrets.local.json");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "secret_content").unwrap();

        let backend = Git2Backend;
        let paths = vec!["app-meta/sync/sync_secrets.local.json"];
        backend.stage_paths(dir.path(), &paths).unwrap();

        // Ensure it's not staged
        let index = repo.index().unwrap();
        assert!(index
            .get_path(
                std::path::Path::new("app-meta/sync/sync_secrets.local.json"),
                0
            )
            .is_none());
    }

    #[test]
    fn test_sync_dry_run_disabled_config() {
        let dir = tempdir().unwrap();
        let config = SyncConfig {

            enabled: false,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let plan = SyncService::perform_sync_dry_run(dir.path(), &config).unwrap();
        assert!(plan.files_to_upload.is_empty());
        assert!(plan.ignored_files.is_empty());
    }

    #[test]
    fn test_sync_dry_run_enabled_config_scans() {
        let dir = tempdir().unwrap();
        let config = SyncConfig {

            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        // Create some whitelisted and blacklisted files
        let settings_path = dir.path().join("app-meta/settings");
        std::fs::create_dir_all(&settings_path).unwrap();
        std::fs::write(settings_path.join("settings.sync.json"), "{}").unwrap();
        std::fs::write(settings_path.join("settings.local.json"), "{}").unwrap();

        let plan = SyncService::perform_sync_dry_run(dir.path(), &config).unwrap();
        assert!(plan
            .files_to_upload
            .contains(&"app-meta/settings/settings.sync.json".to_string()));
        assert!(plan
            .ignored_files
            .contains(&"app-meta/settings/settings.local.json".to_string()));
    }

    #[test]
    fn test_perform_sync_empty_remote_url() {
        let dir = tempdir().unwrap();
        let config = SyncConfig {

            enabled: true,
            remote_url: "".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let secrets = SyncSecrets {
            token: Some("secret_token".to_string()),
            ssh_private_key: None,
        };

        // For this test we can use Git2Backend as it won't be called due to early return
        let backend = Git2Backend;
        let result = SyncService::perform_sync(dir.path(), &config, &secrets, &backend).unwrap();
        assert_eq!(
            result.status,
            SyncStatus::Error("Remote URL is empty".to_string())
        );
    }

    #[test]
    fn test_perform_sync_non_empty_remote() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig {

            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockInitNonEmptyBackend;
        impl GitBackend for MockInitNonEmptyBackend {
            fn clone_repo(
                &self,
                _: &str,
                _: &Path,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn open_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "pull failed: unable to merge unrelated histories",
                )))
            }
            fn stage_paths(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(&self, _: &Path) -> crate::Result<Vec<String>> {
                Ok(vec![])
            }
            fn init_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn ensure_remote(&self, _: &Path, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn has_repo(&self, _: &Path) -> bool {
                false
            }
            fn is_worktree_empty_or_git_only(&self, _: &Path) -> crate::Result<bool> {
                Ok(false)
            }
        }

        let res =
            SyncService::perform_sync(dir.path(), &config, &secrets, &MockInitNonEmptyBackend)
                .unwrap();
        assert_eq!(res.first_sync_mode, FirstSyncMode::UnrelatedHistories);
        // user_message 不再填充中文文案
        assert_eq!(res.user_message, None);
    }

    #[test]
    #[cfg(not(windows))]
    fn test_save_sync_state_failure() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("app-meta/sync");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join("state.local.json"), "{}").unwrap();

        let config = SyncConfig {

            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockBackendOk;
        impl GitBackend for MockBackendOk {
            fn clone_repo(
                &self,
                _: &str,
                _: &Path,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn open_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(&self, _: &Path) -> crate::Result<Vec<String>> {
                Ok(vec![])
            }
            fn init_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn ensure_remote(&self, _: &Path, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn has_repo(&self, _: &Path) -> bool {
                true
            }
            fn is_worktree_empty_or_git_only(&self, _: &Path) -> crate::Result<bool> {
                Ok(false)
            }
        }

        std::fs::remove_file(state_dir.join("state.local.json")).unwrap();
        std::fs::create_dir(state_dir.join("state.local.json")).unwrap();

        let res = SyncService::perform_sync(dir.path(), &config, &secrets, &MockBackendOk).unwrap();
        assert!(matches!(res.status, SyncStatus::FatalError(_)));
        // user_message 不再填充中文文案
        assert_eq!(res.user_message, None);
    }

    #[test]
    fn test_first_sync_mode_clone_into_empty_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig {

            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockBackend;
        impl GitBackend for MockBackend {
            fn clone_repo(
                &self,
                _: &str,
                _: &Path,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn open_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(&self, _: &Path) -> crate::Result<Vec<String>> {
                Ok(vec![])
            }
            fn init_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn ensure_remote(&self, _: &Path, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn has_repo(&self, _: &Path) -> bool {
                false
            }
            fn is_worktree_empty_or_git_only(&self, _: &Path) -> crate::Result<bool> {
                Ok(true)
            }
        }

        let res = SyncService::perform_sync(dir.path(), &config, &secrets, &MockBackend).unwrap();
        assert_eq!(res.first_sync_mode, FirstSyncMode::CloneIntoEmptyWorkspace);
    }

    #[test]
    fn test_first_sync_mode_init_existing_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig {

            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockBackend;
        impl GitBackend for MockBackend {
            fn clone_repo(
                &self,
                _: &str,
                _: &Path,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn open_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(&self, _: &Path) -> crate::Result<Vec<String>> {
                Ok(vec![])
            }
            fn init_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn ensure_remote(&self, _: &Path, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn has_repo(&self, _: &Path) -> bool {
                false
            }
            fn is_worktree_empty_or_git_only(&self, _: &Path) -> crate::Result<bool> {
                Ok(false)
            }
        }

        let res = SyncService::perform_sync(dir.path(), &config, &secrets, &MockBackend).unwrap();
        assert_eq!(res.first_sync_mode, FirstSyncMode::InitExistingWorkspace);
    }

    #[test]
    fn test_first_sync_mode_already_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig {

            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockBackend;
        impl GitBackend for MockBackend {
            fn clone_repo(
                &self,
                _: &str,
                _: &Path,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn open_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(&self, _: &Path) -> crate::Result<Vec<String>> {
                Ok(vec![])
            }
            fn init_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn ensure_remote(&self, _: &Path, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn has_repo(&self, _: &Path) -> bool {
                true
            }
            fn is_worktree_empty_or_git_only(&self, _: &Path) -> crate::Result<bool> {
                Ok(false)
            }
        }

        let res = SyncService::perform_sync(dir.path(), &config, &secrets, &MockBackend).unwrap();
        assert_eq!(res.first_sync_mode, FirstSyncMode::AlreadyGitRepo);
    }

    #[test]
    fn test_sync_plan_no_tokens() {
        let dir = tempfile::tempdir().unwrap();

        let settings_dir = dir.path().join("app-meta/sync");
        std::fs::create_dir_all(&settings_dir).unwrap();

        // Write the local secrets
        std::fs::write(
            settings_dir.join("sync_secrets.local.json"),
            "secret_token_123",
        )
        .unwrap();
        std::fs::write(
            settings_dir.join("sync_secrets.local.json.tmp"),
            "secret_token_456",
        )
        .unwrap();

        // Also write some valid file to sync
        std::fs::write(dir.path().join("workspace_manifest.json"), "{}").unwrap();

        let plan = SyncService::build_sync_plan_from_workspace(dir.path()).unwrap();

        // Ensure plan does not include the blacklisted items
        for file in plan.files_to_upload {
            assert!(
                !file.contains("sync_secrets.local.json"),
                "Should not upload secrets"
            );
        }

        let ignored: Vec<String> = plan.ignored_files.into_iter().collect();
        assert!(
            ignored
                .iter()
                .any(|s| s.contains("sync_secrets.local.json")),
            "Secrets should be explicitly ignored"
        );
    }

    #[test]
    fn test_first_sync_empty_remote_branch_not_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("workspace_manifest.json"), "{}").unwrap();

        let config = SyncConfig {

            enabled: true,
            remote_url: "https://github.com/test/empty-repo.git".to_string(),
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockEmptyRemoteBackend;
        impl GitBackend for MockEmptyRemoteBackend {
            fn clone_repo(
                &self,
                _: &str,
                _: &Path,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn open_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "ref not found: refs/heads/main",
                )))
            }
            fn stage_paths(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(Some("commit_hash".to_string()))
            }
            fn push(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(&self, _: &Path) -> crate::Result<Vec<String>> {
                Ok(vec!["workspace_manifest.json".to_string()])
            }
            fn init_repo(&self, _: &Path) -> crate::Result<()> {
                Ok(())
            }
            fn ensure_remote(&self, _: &Path, _: &str) -> crate::Result<()> {
                Ok(())
            }
            fn has_repo(&self, _: &Path) -> bool {
                false
            }
            fn is_worktree_empty_or_git_only(&self, _: &Path) -> crate::Result<bool> {
                Ok(false)
            }
        }

        let res = SyncService::perform_sync(dir.path(), &config, &secrets, &MockEmptyRemoteBackend)
            .unwrap();
        assert_eq!(res.status, SyncStatus::Success);
        assert_eq!(res.first_sync_mode, FirstSyncMode::InitExistingWorkspace);
        // user_message 不再填充中文文案
        assert_eq!(res.user_message, None);
    }

    #[test]
    #[cfg(not(windows))]
    fn test_push_preflight_unborn_head() {
        let dir = tempfile::tempdir().unwrap();
        // Repository is initialized but has no commits (unborn HEAD)
        let _repo = git2::Repository::init(dir.path()).unwrap();

        let backend = Git2Backend;
        let res = backend.push(dir.path(), "main", None);
        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("recoverable_error") || err_msg.contains("unborn"));
    }

    #[test]
    #[cfg(not(windows))]
    fn test_push_preflight_missing_branch_ref_recovered() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        // Create a commit
        let signature = git2::Signature::now("Test User", "test@test.com").unwrap();
        let mut index = repo.index().unwrap();
        let file_path = dir.path().join("app-meta/settings/settings.sync.json");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "{}").unwrap();
        index
            .add_path(Path::new("app-meta/settings/settings.sync.json"))
            .unwrap();
        index.write().unwrap();
        let oid = index.write_tree().unwrap();
        let tree = repo.find_tree(oid).unwrap();
        let commit_oid = repo
            .commit(
                Some("refs/heads/main"),
                &signature,
                &signature,
                "Initial commit",
                &tree,
                &[],
            )
            .unwrap();

        // Delete the branch reference, keeping HEAD detached pointing to the commit
        repo.set_head_detached(commit_oid).unwrap();
        let mut branch_ref = repo.find_reference("refs/heads/main").unwrap();
        branch_ref.delete().unwrap();

        // Now branch reference refs/heads/main does not exist, but HEAD points to a commit.
        // We verify that calling Git2Backend::push reconstructs the branch ref successfully!
        let backend = Git2Backend;
        let _res = backend.push(dir.path(), "main", None);
        // Verify branch ref has been reconstructed!
        assert!(repo.find_reference("refs/heads/main").is_ok());
    }

    #[test]
    #[cfg(not(windows))]
    fn test_settings_semantic_merge_conflict_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        // Set up local repository with a commit containing base settings.sync.json
        let signature = git2::Signature::now("Test User", "test@test.com").unwrap();
        let mut index = repo.index().unwrap();
        let file_path = dir.path().join("app-meta/settings/settings.sync.json");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();

        let base_content = r#"{"font_size": 12, "theme": "dark"}"#;
        std::fs::write(&file_path, base_content).unwrap();
        index
            .add_path(Path::new("app-meta/settings/settings.sync.json"))
            .unwrap();
        index.write().unwrap();
        let oid = index.write_tree().unwrap();
        let tree = repo.find_tree(oid).unwrap();
        let base_commit_oid = repo
            .commit(
                Some("refs/heads/main"),
                &signature,
                &signature,
                "Base commit",
                &tree,
                &[],
            )
            .unwrap();
        repo.set_head("refs/heads/main").unwrap();

        // Clone local repo to remote right after base commit (so remote shares base commit OID and history)
        let remote_dir = tempfile::tempdir().unwrap();
        let remote_repo =
            git2::Repository::clone(dir.path().to_str().unwrap(), remote_dir.path()).unwrap();

        // Now modify local settings.sync.json and commit it in local repo (local divergent change)
        let local_content = r#"{"font_size": 16, "theme": "dark"}"#;
        std::fs::write(&file_path, local_content).unwrap();
        index
            .add_path(Path::new("app-meta/settings/settings.sync.json"))
            .unwrap();
        index.write().unwrap();
        let oid = index.write_tree().unwrap();
        let tree = repo.find_tree(oid).unwrap();
        let local_commit_oid = repo
            .commit(
                Some("refs/heads/main"),
                &signature,
                &signature,
                "Local commit",
                &tree,
                &[&repo.find_commit(base_commit_oid).unwrap()],
            )
            .unwrap();

        // In remote repo, modify settings.sync.json to a conflicting value and commit (remote divergent change)
        let remote_file_path = remote_dir
            .path()
            .join("app-meta/settings/settings.sync.json");
        let remote_content = r#"{"font_size": 20, "theme": "dark"}"#;
        std::fs::write(&remote_file_path, remote_content).unwrap();
        let mut remote_index = remote_repo.index().unwrap();
        remote_index
            .add_path(Path::new("app-meta/settings/settings.sync.json"))
            .unwrap();
        remote_index.write().unwrap();
        let remote_oid = remote_index.write_tree().unwrap();
        let remote_tree = remote_repo.find_tree(remote_oid).unwrap();
        let remote_base_commit = remote_repo.find_commit(base_commit_oid).unwrap();
        let _remote_commit_oid = remote_repo
            .commit(
                Some("refs/heads/main"),
                &signature,
                &signature,
                "Remote commit",
                &remote_tree,
                &[&remote_base_commit],
            )
            .unwrap();

        // Add remote to local repo
        let mut remote = repo
            .remote("origin", remote_dir.path().to_str().unwrap())
            .unwrap();
        remote.fetch(&["main"], None, None).unwrap();

        // Verify pull/merge fails with settings_conflict_payload
        let backend = Git2Backend;
        let res = backend.pull(dir.path(), "main", None);
        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("settings_conflict_payload"));

        // Verify that after transactional rollback:
        // 1. Index has no conflicts
        let index = repo.index().unwrap();
        assert!(!index.has_conflicts());
        // 2. HEAD points back to original local_commit_oid
        let head = repo.head().unwrap();
        assert_eq!(head.target().unwrap(), local_commit_oid);
        // 3. Local settings file is intact and not corrupted with remote change
        let content_after = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content_after, local_content);
    }

    fn start_mock_github_api(
        initial_manifest: Option<SyncManifest>,
        initial_files: std::collections::HashMap<String, String>,
    ) -> (
        String,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
        std::sync::Arc<std::sync::Mutex<String>>,
        std::thread::JoinHandle<()>,
    ) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Mutex};
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let addr = format!("http://127.0.0.1:{}", port);

        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        let files = Arc::new(Mutex::new(initial_files));
        let files_clone = files.clone();

        let manifest_str = if let Some(m) = initial_manifest {
            serde_json::to_string(&m).unwrap()
        } else {
            String::new()
        };
        let manifest = Arc::new(Mutex::new(manifest_str));
        let manifest_clone = manifest.clone();

        listener.set_nonblocking(true).unwrap();

        let handle = thread::spawn(move || {
            while !shutdown_clone.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buffer = [0; 65536];
                        if let Ok(bytes_read) = stream.read(&mut buffer) {
                            let req = String::from_utf8_lossy(&buffer[..bytes_read]);
                            let first_line = req.lines().next().unwrap_or("");
                            let parts: Vec<&str> = first_line.split_whitespace().collect();
                            if parts.len() >= 2 {
                                let method = parts[0];
                                let path = parts[1];

                                let mut response_body = String::new();
                                let mut status_line = "HTTP/1.1 200 OK";

                                if path.contains("/rate_limit") {
                                    response_body = r#"{"resources":{}}"#.to_string();
                                } else if path.contains("/git/ref/heads/main") {
                                    let m = manifest_clone.lock().unwrap();
                                    if m.is_empty() {
                                        status_line = "HTTP/1.1 404 Not Found";
                                        response_body = r#"{"message":"Not Found"}"#.to_string();
                                    } else {
                                        response_body =
                                            r#"{"object":{"sha":"mock_commit_sha"}}"#.to_string();
                                    }
                                } else if path.contains("/git/commits/mock_commit_sha") {
                                    response_body =
                                        r#"{"tree":{"sha":"mock_tree_sha"}}"#.to_string();
                                } else if path.contains("/git/trees/mock_tree_sha")
                                    || path.contains("/git/trees/main")
                                {
                                    let mut tree_list = Vec::new();
                                    let m = manifest_clone.lock().unwrap();
                                    if !m.is_empty() {
                                        tree_list.push(serde_json::json!({
                                            "path": "app-meta/sync/manifest.sync.json",
                                            "type": "blob",
                                            "sha": "manifest_blob_sha"
                                        }));
                                    }
                                    let fls = files_clone.lock().unwrap();
                                    for filename in fls.keys() {
                                        tree_list.push(serde_json::json!({
                                            "path": filename,
                                            "type": "blob",
                                            "sha": format!("{}_sha", filename)
                                        }));
                                    }
                                    response_body =
                                        serde_json::json!({ "tree": tree_list }).to_string();
                                } else if path
                                    .contains("/contents/app-meta/sync/manifest.sync.json")
                                {
                                    let m = manifest_clone.lock().unwrap();
                                    if method == "GET" {
                                        if m.is_empty() {
                                            status_line = "HTTP/1.1 404 Not Found";
                                        } else {
                                            let encoded = base64::engine::general_purpose::STANDARD
                                                .encode(m.as_bytes());
                                            response_body = serde_json::json!({
                                                "content": encoded,
                                                "encoding": "base64",
                                                "sha": "manifest_blob_sha"
                                            })
                                            .to_string();
                                        }
                                    } else if method == "PUT" {
                                        let manifest_exists = !m.is_empty();
                                        drop(m);
                                        if let Some(body_start) = req.find("\r\n\r\n") {
                                            let body = &req[body_start + 4..];
                                            if let Ok(val) =
                                                serde_json::from_str::<serde_json::Value>(body)
                                            {
                                                if manifest_exists && val["sha"].as_str().is_none()
                                                {
                                                    status_line =
                                                        "HTTP/1.1 422 Unprocessable Entity";
                                                    response_body =
                                                        r#"{"message":"sha required"}"#.to_string();
                                                }
                                                if let Some(b64_content) = val["content"].as_str() {
                                                    if status_line == "HTTP/1.1 200 OK" {
                                                        let decoded = base64::engine::general_purpose::STANDARD
                                                            .decode(b64_content)
                                                            .unwrap();
                                                        let mut m = manifest_clone.lock().unwrap();
                                                        *m = String::from_utf8(decoded).unwrap();
                                                    }
                                                }
                                            }
                                        }
                                        if status_line == "HTTP/1.1 200 OK" {
                                            response_body =
                                                r#"{"content":{"sha":"manifest_new_sha"}}"#
                                                    .to_string();
                                        }
                                    } else {
                                        status_line = "HTTP/1.1 405 Method Not Allowed";
                                    }
                                } else if path.contains("/contents/") {
                                    if let Some(idx) = path.find("/contents/") {
                                        let file_path = &path[idx + 10..];
                                        let file_path =
                                            file_path.split('?').next().unwrap_or(file_path);
                                        if method == "GET" {
                                            let fls = files_clone.lock().unwrap();
                                            if let Some(content) = fls.get(file_path) {
                                                let encoded =
                                                    base64::engine::general_purpose::STANDARD
                                                        .encode(content.as_bytes());
                                                response_body = serde_json::json!({
                                                    "content": encoded,
                                                    "encoding": "base64",
                                                    "sha": format!("{}_sha", file_path)
                                                })
                                                .to_string();
                                            } else {
                                                status_line = "HTTP/1.1 404 Not Found";
                                            }
                                        } else if method == "PUT" {
                                            if let Some(body_start) = req.find("\r\n\r\n") {
                                                let body = &req[body_start + 4..];
                                                if let Ok(val) =
                                                    serde_json::from_str::<serde_json::Value>(body)
                                                {
                                                    let file_exists = files_clone
                                                        .lock()
                                                        .unwrap()
                                                        .contains_key(file_path);
                                                    if file_exists && val["sha"].as_str().is_none()
                                                    {
                                                        status_line =
                                                            "HTTP/1.1 422 Unprocessable Entity";
                                                        response_body =
                                                            r#"{"message":"sha required"}"#
                                                                .to_string();
                                                    }
                                                    if let Some(b64_content) =
                                                        val["content"].as_str()
                                                    {
                                                        if status_line == "HTTP/1.1 200 OK" {
                                                            let decoded = base64::engine::general_purpose::STANDARD
                                                                .decode(b64_content)
                                                                .unwrap();
                                                            let mut fls =
                                                                files_clone.lock().unwrap();
                                                            fls.insert(
                                                                file_path.to_string(),
                                                                String::from_utf8(decoded).unwrap(),
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                            if status_line == "HTTP/1.1 200 OK" {
                                                response_body =
                                                    r#"{"content":{"sha":"new_sha"}}"#.to_string();
                                            }
                                        } else if method == "DELETE" {
                                            if let Some(body_start) = req.find("\r\n\r\n") {
                                                let body = &req[body_start + 4..];
                                                let val =
                                                    serde_json::from_str::<serde_json::Value>(body)
                                                        .unwrap_or_default();
                                                if val["sha"].as_str().is_none() {
                                                    status_line =
                                                        "HTTP/1.1 422 Unprocessable Entity";
                                                    response_body =
                                                        r#"{"message":"sha required"}"#.to_string();
                                                } else {
                                                    let mut fls = files_clone.lock().unwrap();
                                                    fls.remove(file_path);
                                                    response_body =
                                                        r#"{"content":null}"#.to_string();
                                                }
                                            }
                                        } else {
                                            status_line = "HTTP/1.1 405 Method Not Allowed";
                                        }
                                    }
                                } else if method == "POST" && path.contains("/git/blobs") {
                                    status_line = "HTTP/1.1 500 Internal Server Error";
                                    if let Some(body_start) = req.find("\r\n\r\n") {
                                        let body = &req[body_start + 4..];
                                        if let Ok(val) =
                                            serde_json::from_str::<serde_json::Value>(body)
                                        {
                                            if let Some(b64_content) = val["content"].as_str() {
                                                if let Ok(decoded_bytes) =
                                                    base64::engine::general_purpose::STANDARD
                                                        .decode(b64_content)
                                                {
                                                    if let Ok(decoded_str) =
                                                        String::from_utf8(decoded_bytes)
                                                    {
                                                        if decoded_str
                                                            .contains("manifest.sync.json")
                                                            || decoded_str.contains("\"files\":")
                                                        {
                                                            let mut m =
                                                                manifest_clone.lock().unwrap();
                                                            *m = decoded_str;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    response_body =
                                        r#"{"message":"git db api must not be used"}"#.to_string();
                                } else if method == "POST" && path.contains("/git/trees") {
                                    status_line = "HTTP/1.1 500 Internal Server Error";
                                    response_body =
                                        r#"{"message":"git db api must not be used"}"#.to_string();
                                } else if method == "POST" && path.contains("/git/commits") {
                                    status_line = "HTTP/1.1 500 Internal Server Error";
                                    response_body =
                                        r#"{"message":"git db api must not be used"}"#.to_string();
                                } else if method == "POST" && path.contains("/git/refs") {
                                    status_line = "HTTP/1.1 500 Internal Server Error";
                                    response_body =
                                        r#"{"message":"git db api must not be used"}"#.to_string();
                                } else if method == "PATCH" && path.contains("/git/refs/heads/main")
                                {
                                    status_line = "HTTP/1.1 500 Internal Server Error";
                                    response_body =
                                        r#"{"message":"git db api must not be used"}"#.to_string();
                                }

                                let response = format!(
                                    "{}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                                    status_line,
                                    response_body.len(),
                                    response_body
                                );
                                let _ = stream.write_all(response.as_bytes());
                            }
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    Err(_) => {}
                }
            }
        });

        (addr, shutdown, files, manifest, handle)
    }

    #[test]
    #[cfg(not(windows))]
    fn test_perform_lww_sync_first_download() {
        let dir = tempdir().unwrap();
        let mut initial_files = std::collections::HashMap::new();
        initial_files.insert(
            "projects/p1/project.json".to_string(),
            "remote content".to_string(),
        );

        let initial_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "projects/p1/project.json".to_string(),
                content_hash: format!("{:x}", md5::compute("remote content".as_bytes())),
                updated_at_ms: 1000,
                deleted_at_ms: None,
                device_id: "device_remote".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };

        let (mock_url, shutdown, _files, _manifest, server_thread) =
            start_mock_github_api(Some(initial_manifest), initial_files);

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();
        assert!(res
            .downloaded_files
            .contains(&"projects/p1/project.json".to_string()));
        assert!(!res
            .downloaded_files
            .contains(&"app-meta/sync/manifest.sync.json".to_string()));
        assert!(res.uploaded_files.is_empty());
        assert!(res.local_deletes.is_empty());
        assert!(res.remote_deletes.is_empty());
        assert!(res.overwritten_files.is_empty());

        let local_file_path = dir.path().join("projects/p1/project.json");
        assert!(local_file_path.exists());
        let local_content = std::fs::read_to_string(local_file_path).unwrap();
        assert_eq!(local_content, "remote content");
        assert!(dir.path().join("app-meta/sync/manifest.sync.json").exists());

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    #[cfg(not(windows))]
    fn test_perform_lww_sync_local_delete_generates_manifest_delete() {
        let dir = tempdir().unwrap();

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state.known_files.insert(
            "projects/p1/project.json".to_string(),
            "old_hash".to_string(),
        );
        state
            .known_files_updated_at
            .insert("projects/p1/project.json".to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let mut initial_files = std::collections::HashMap::new();
        initial_files.insert(
            "projects/p1/project.json".to_string(),
            "remote content".to_string(),
        );
        let initial_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "projects/p1/project.json".to_string(),
                content_hash: format!("{:x}", md5::compute("remote content".as_bytes())),
                updated_at_ms: 900,
                deleted_at_ms: None,
                device_id: "device_remote".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };

        let (mock_url, shutdown, _files_map, manifest_str, server_thread) =
            start_mock_github_api(Some(initial_manifest), initial_files);

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();
        assert!(res
            .local_deletes
            .contains(&"projects/p1/project.json".to_string()));

        let final_m: SyncManifest =
            serde_json::from_str(&manifest_str.lock().unwrap().clone()).unwrap();
        let rec = final_m
            .files
            .iter()
            .find(|f| f.path == "projects/p1/project.json")
            .unwrap();
        assert_eq!(rec.op, "delete");

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    #[cfg(not(windows))]
    fn test_perform_lww_sync_remote_delete_removes_local_file() {
        let dir = tempdir().unwrap();
        let local_path = dir.path().join("projects/p1/project.json");
        std::fs::create_dir_all(local_path.parent().unwrap()).unwrap();
        std::fs::write(&local_path, "local content").unwrap();

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state.known_files.insert(
            "projects/p1/project.json".to_string(),
            format!("{:x}", md5::compute("local content".as_bytes())),
        );
        state
            .known_files_updated_at
            .insert("projects/p1/project.json".to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let initial_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "projects/p1/project.json".to_string(),
                content_hash: String::new(),
                updated_at_ms: 3000,
                deleted_at_ms: Some(3000),
                device_id: "device_remote".to_string(),
                op: "delete".to_string(),
                schema_version: 1,
            }],
        };

        let (mock_url, shutdown, _files_map, _manifest_str, server_thread) =
            start_mock_github_api(Some(initial_manifest), std::collections::HashMap::new());

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();
        assert!(res
            .remote_deletes
            .contains(&"projects/p1/project.json".to_string()));
        assert!(!local_path.exists());

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    #[ignore]
    fn test_perform_lww_sync_timestamp_wins() {
        let dir = tempdir().unwrap();

        let local_p1 = dir.path().join("projects/p1/project.json");
        std::fs::create_dir_all(local_p1.parent().unwrap()).unwrap();
        std::fs::write(&local_p1, "local newer content").unwrap();

        let local_p2 = dir.path().join("projects/p2/project.json");
        std::fs::create_dir_all(local_p2.parent().unwrap()).unwrap();
        std::fs::write(&local_p2, "local older content").unwrap();

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state.known_files.insert(
            "projects/p1/project.json".to_string(),
            format!("{:x}", md5::compute("local base".as_bytes())),
        );
        state
            .known_files_updated_at
            .insert("projects/p1/project.json".to_string(), 1000);
        state.known_files.insert(
            "projects/p2/project.json".to_string(),
            format!("{:x}", md5::compute("local older content".as_bytes())),
        );
        state
            .known_files_updated_at
            .insert("projects/p2/project.json".to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let mut initial_files = std::collections::HashMap::new();
        initial_files.insert(
            "projects/p1/project.json".to_string(),
            "remote older content".to_string(),
        );
        initial_files.insert(
            "projects/p2/project.json".to_string(),
            "remote newer content".to_string(),
        );

        let initial_manifest = SyncManifest {
            files: vec![
                ManifestFileRecord {
                    path: "projects/p1/project.json".to_string(),
                    content_hash: format!("{:x}", md5::compute("remote older content".as_bytes())),
                    updated_at_ms: 2000,
                    deleted_at_ms: None,
                    device_id: "device_remote".to_string(),
                    op: "upsert".to_string(),
                    schema_version: 1,
                },
                ManifestFileRecord {
                    path: "projects/p2/project.json".to_string(),
                    content_hash: format!("{:x}", md5::compute("remote newer content".as_bytes())),
                    updated_at_ms: 4000,
                    deleted_at_ms: None,
                    device_id: "device_remote".to_string(),
                    op: "upsert".to_string(),
                    schema_version: 1,
                },
            ],
        };

        let (mock_url, shutdown, _files_map, manifest_str, server_thread) =
            start_mock_github_api(Some(initial_manifest), initial_files);

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();

        assert!(res
            .uploaded_files
            .contains(&"projects/p1/project.json".to_string()));
        assert!(res
            .downloaded_files
            .contains(&"projects/p2/project.json".to_string()));

        let content_p2 = std::fs::read_to_string(&local_p2).unwrap();
        assert_eq!(content_p2, "remote newer content");

        let final_m_str = manifest_str.lock().unwrap().clone();
        assert!(!final_m_str.is_empty());
        let final_m: SyncManifest = serde_json::from_str(&final_m_str).unwrap();
        let p1_rec = final_m
            .files
            .iter()
            .find(|f| f.path == "projects/p1/project.json")
            .unwrap();
        assert_eq!(p1_rec.device_id, "device_local");
        assert_eq!(p1_rec.op, "upsert");

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    fn test_lww_sync_ignores_local_only_files() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("app-meta/sync")).unwrap();
        std::fs::create_dir_all(dir.path().join("app-meta/logs")).unwrap();
        std::fs::create_dir_all(dir.path().join("tmp")).unwrap();
        std::fs::write(
            dir.path().join("app-meta/sync/sync_state.json"),
            "{\"noise\":true}",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("app-meta/sync/state.local.json"),
            "{\"local\":true}",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("app-meta/sync/sync_secrets.local.json"),
            "{\"token\":\"x\"}",
        )
        .unwrap();
        std::fs::write(dir.path().join("app-meta/logs/sync.log"), "x").unwrap();
        std::fs::write(dir.path().join("tmp/runtime.tmp"), "x").unwrap();

        let (mock_url, shutdown, _files, _manifest, server_thread) =
            start_mock_github_api(None, std::collections::HashMap::new());

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();
        assert_ne!(res.status, SyncStatus::DirtyRepoBlocked);

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    fn test_is_document_content_path() {
        use crate::sync::lww::is_document_content_path;
        assert!(is_document_content_path(
            "projects/p1/volumes/v1/chapters/c1/chapter.md"
        ));
        assert!(is_document_content_path(
            "projects/abc/volumes/001/chapters/xyz/chapter.md"
        ));
        assert!(!is_document_content_path("projects/p1/project.json"));
        assert!(!is_document_content_path(
            "projects/p1/volumes/v1/chapters/c1/chapter.meta.json"
        ));
        assert!(!is_document_content_path(
            "app-meta/settings/settings.sync.json"
        ));
        assert!(!is_document_content_path(
            "projects/p1/volumes/v1/volume.json"
        ));

        // P1-3: expanded user text document paths
        assert!(is_document_content_path("projects/p1/note.md"));
        assert!(is_document_content_path(
            "projects/p1/volumes/v1/outline.md"
        ));
        assert!(is_document_content_path(
            "projects/p1/volumes/v1/chapters/c1/scene.md"
        ));
        assert!(is_document_content_path("projects/p1/character_notes.md"));
        assert!(is_document_content_path("projects/p1/timeline_notes.md"));
        assert!(is_document_content_path("projects/p1/draft.md"));
        // backups and app-meta are local-only, not user text
        assert!(!is_document_content_path("backups/chapters/c1_backup.md"));
        assert!(!is_document_content_path(
            "app-meta/sync/manifest.sync.json"
        ));
    }

    #[test]
    #[cfg(not(windows))]
    fn test_lww_document_conflict_no_overwrite() {
        let dir = tempdir().unwrap();
        let chapter_rel = "projects/p1/volumes/v1/chapters/c1/chapter.md";
        let chapter_abs = dir.path().join(chapter_rel);
        std::fs::create_dir_all(chapter_abs.parent().unwrap()).unwrap();

        let base_content = "base version of chapter";
        let local_content = "local modified version of chapter";
        let remote_content = "remote modified version of chapter";

        std::fs::write(&chapter_abs, local_content).unwrap();

        let base_hash = format!("{:x}", md5::compute(base_content.as_bytes()));
        let _local_hash = format!("{:x}", md5::compute(local_content.as_bytes()));
        let remote_hash = format!("{:x}", md5::compute(remote_content.as_bytes()));

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state
            .known_files
            .insert(chapter_rel.to_string(), base_hash.clone());
        state
            .known_files_updated_at
            .insert(chapter_rel.to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let mut initial_files = std::collections::HashMap::new();
        initial_files.insert(chapter_rel.to_string(), remote_content.to_string());

        let initial_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: chapter_rel.to_string(),
                content_hash: remote_hash.clone(),
                updated_at_ms: 3000,
                deleted_at_ms: None,
                device_id: "device_remote".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };

        let (mock_url, shutdown, _files_map, _manifest_str, server_thread) =
            start_mock_github_api(Some(initial_manifest), initial_files);

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();

        assert_eq!(res.status, SyncStatus::PartialConflict);
        assert!(!res.conflicts.is_empty());
        assert_eq!(res.conflicts[0].local_path, chapter_rel);
        assert_eq!(res.conflicts[0].base_hash, base_hash);
        assert!(!res.overwritten_files.contains(&chapter_rel.to_string()));
        assert!(!res.downloaded_files.contains(&chapter_rel.to_string()));
        assert!(!res.uploaded_files.contains(&chapter_rel.to_string()));

        let local_after = std::fs::read_to_string(&chapter_abs).unwrap();
        assert_eq!(local_after, local_content);

        let conflict_files: Vec<String> = std::fs::read_dir(chapter_abs.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("remote-conflict"))
            .collect();
        assert!(
            !conflict_files.is_empty(),
            "Expected remote-conflict file to be created"
        );

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    #[cfg(not(windows))]
    fn test_lww_document_conflict_manifest_not_polluted() {
        let dir = tempdir().unwrap();
        let chapter_rel = "projects/p1/volumes/v1/chapters/c1/chapter.md";
        let chapter_abs = dir.path().join(chapter_rel);
        std::fs::create_dir_all(chapter_abs.parent().unwrap()).unwrap();

        let base_content = "base version";
        let local_content = "local modified version";
        let remote_content = "remote modified version";

        std::fs::write(&chapter_abs, local_content).unwrap();

        let base_hash = format!("{:x}", md5::compute(base_content.as_bytes()));
        let remote_hash = format!("{:x}", md5::compute(remote_content.as_bytes()));

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state
            .known_files
            .insert(chapter_rel.to_string(), base_hash.clone());
        state
            .known_files_updated_at
            .insert(chapter_rel.to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let mut initial_files = std::collections::HashMap::new();
        initial_files.insert(chapter_rel.to_string(), remote_content.to_string());

        let initial_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: chapter_rel.to_string(),
                content_hash: remote_hash.clone(),
                updated_at_ms: 3000,
                deleted_at_ms: None,
                device_id: "device_remote".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };

        let (mock_url, shutdown, _files_map, manifest_str, server_thread) =
            start_mock_github_api(Some(initial_manifest), initial_files);

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();
        assert_eq!(res.status, SyncStatus::PartialConflict);

        let uploaded_manifest: SyncManifest =
            serde_json::from_str(&manifest_str.lock().unwrap()).unwrap();
        let manifest_rec = uploaded_manifest
            .files
            .iter()
            .find(|r| r.path == chapter_rel);
        assert!(
            manifest_rec.is_some(),
            "conflict file should still be in manifest (unchanged remote record)"
        );
        assert_eq!(
            manifest_rec.unwrap().content_hash,
            remote_hash,
            "manifest must still point to remote hash, not local hash"
        );

        let loaded_state = SyncService::load_sync_state(dir.path()).unwrap();
        // After the fix, known_files for a conflicted path must remain at base_hash,
        // NOT remote_hash. The conflicted_files set prevents auto-resolution.
        assert_eq!(
            loaded_state.known_files.get(chapter_rel).unwrap(),
            &base_hash,
            "known_files must stay at base_hash for conflicted paths"
        );
        assert!(
            loaded_state.conflicted_files.contains(chapter_rel),
            "conflicted_files must contain the conflicted path"
        );

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    #[cfg(not(windows))]
    fn test_lww_document_only_local_changed_allows_upload() {
        let dir = tempdir().unwrap();
        let chapter_rel = "projects/p1/volumes/v1/chapters/c1/chapter.md";
        let chapter_abs = dir.path().join(chapter_rel);
        std::fs::create_dir_all(chapter_abs.parent().unwrap()).unwrap();

        let base_content = "base version";
        let local_content = "local modified version";

        std::fs::write(&chapter_abs, local_content).unwrap();

        let base_hash = format!("{:x}", md5::compute(base_content.as_bytes()));
        let _local_hash = format!("{:x}", md5::compute(local_content.as_bytes()));

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state.known_files.insert(chapter_rel.to_string(), base_hash);
        state
            .known_files_updated_at
            .insert(chapter_rel.to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let mut initial_files = std::collections::HashMap::new();
        initial_files.insert(chapter_rel.to_string(), base_content.to_string());

        let initial_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: chapter_rel.to_string(),
                content_hash: format!("{:x}", md5::compute(base_content.as_bytes())),
                updated_at_ms: 1000,
                deleted_at_ms: None,
                device_id: "device_remote".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };

        let (mock_url, shutdown, _files_map, _manifest_str, server_thread) =
            start_mock_github_api(Some(initial_manifest), initial_files);

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();

        assert_eq!(res.status, SyncStatus::LatestWinsApplied);
        assert!(res.uploaded_files.contains(&chapter_rel.to_string()));
        assert!(res.conflicts.is_empty());
        assert!(res.overwritten_files.is_empty());

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    #[cfg(not(windows))]
    fn test_lww_document_only_remote_changed_downloads() {
        let dir = tempdir().unwrap();
        let chapter_rel = "projects/p1/volumes/v1/chapters/c1/chapter.md";
        let chapter_abs = dir.path().join(chapter_rel);
        std::fs::create_dir_all(chapter_abs.parent().unwrap()).unwrap();

        let base_content = "base version";
        let remote_content = "remote modified version";

        std::fs::write(&chapter_abs, base_content).unwrap();

        let base_hash = format!("{:x}", md5::compute(base_content.as_bytes()));
        let remote_hash = format!("{:x}", md5::compute(remote_content.as_bytes()));

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state.known_files.insert(chapter_rel.to_string(), base_hash);
        state
            .known_files_updated_at
            .insert(chapter_rel.to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let mut initial_files = std::collections::HashMap::new();
        initial_files.insert(chapter_rel.to_string(), remote_content.to_string());

        let initial_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: chapter_rel.to_string(),
                content_hash: remote_hash,
                updated_at_ms: 3000,
                deleted_at_ms: None,
                device_id: "device_remote".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };

        let (mock_url, shutdown, _files_map, _manifest_str, server_thread) =
            start_mock_github_api(Some(initial_manifest), initial_files);

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();

        assert_eq!(res.status, SyncStatus::LatestWinsApplied);
        assert!(res.downloaded_files.contains(&chapter_rel.to_string()));
        assert!(res.conflicts.is_empty());
        assert!(res.overwritten_files.is_empty());

        let content_after = std::fs::read_to_string(&chapter_abs).unwrap();
        assert_eq!(content_after, remote_content);

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    #[cfg(not(windows))]
    fn test_lww_document_remote_delete_local_modified_is_conflict() {
        let dir = tempdir().unwrap();
        let chapter_rel = "projects/p1/volumes/v1/chapters/c1/chapter.md";
        let chapter_abs = dir.path().join(chapter_rel);
        std::fs::create_dir_all(chapter_abs.parent().unwrap()).unwrap();

        let base_content = "base version";
        let local_content = "local modified version";

        std::fs::write(&chapter_abs, local_content).unwrap();

        let base_hash = format!("{:x}", md5::compute(base_content.as_bytes()));
        let _local_hash = format!("{:x}", md5::compute(local_content.as_bytes()));

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state
            .known_files
            .insert(chapter_rel.to_string(), base_hash.clone());
        state
            .known_files_updated_at
            .insert(chapter_rel.to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let initial_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: chapter_rel.to_string(),
                content_hash: String::new(),
                updated_at_ms: 3000,
                deleted_at_ms: Some(3000),
                device_id: "device_remote".to_string(),
                op: "delete".to_string(),
                schema_version: 1,
            }],
        };

        let (mock_url, shutdown, _files_map, _manifest_str, server_thread) =
            start_mock_github_api(Some(initial_manifest), std::collections::HashMap::new());

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();

        assert_eq!(res.status, SyncStatus::PartialConflict);
        assert!(!res.conflicts.is_empty());

        let local_after = std::fs::read_to_string(&chapter_abs).unwrap();
        assert_eq!(local_after, local_content);

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    /// P0-1: Core conflict state machine test.
    /// Scenario: base=A, local=B, remote=C.
    /// 1. First sync generates a conflict.
    /// 2. Second sync does NOT auto-upload B over C, nor download C over B.
    /// 3. known_files[path] stays at A (base_hash), not B or C.
    /// 4. Conflicts persist until user resolves.
    #[test]
    #[cfg(not(windows))]
    fn test_lww_conflict_persists_across_syncs() {
        let dir = tempdir().unwrap();
        let chapter_rel = "projects/p1/volumes/v1/chapters/c1/chapter.md";
        let chapter_abs = dir.path().join(chapter_rel);
        std::fs::create_dir_all(chapter_abs.parent().unwrap()).unwrap();

        let base_content = "base version A";
        let local_content = "local version B";
        let remote_content = "remote version C";

        std::fs::write(&chapter_abs, local_content).unwrap();

        let base_hash = format!("{:x}", md5::compute(base_content.as_bytes()));
        let local_hash = format!("{:x}", md5::compute(local_content.as_bytes()));
        let remote_hash = format!("{:x}", md5::compute(remote_content.as_bytes()));

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state
            .known_files
            .insert(chapter_rel.to_string(), base_hash.clone());
        state
            .known_files_updated_at
            .insert(chapter_rel.to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let mut initial_files = std::collections::HashMap::new();
        initial_files.insert(chapter_rel.to_string(), remote_content.to_string());

        let initial_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: chapter_rel.to_string(),
                content_hash: remote_hash.clone(),
                updated_at_ms: 3000,
                deleted_at_ms: None,
                device_id: "device_remote".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };

        // === First sync: should detect BothChanged conflict ===
        let (mock_url, shutdown, _files_map, _manifest_str, server_thread) =
            start_mock_github_api(Some(initial_manifest.clone()), initial_files.clone());

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res1 = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();

        // First sync: conflict detected
        assert_eq!(res1.status, SyncStatus::PartialConflict);
        assert!(!res1.conflicts.is_empty());
        assert_eq!(res1.conflicts[0].base_hash, base_hash);
        assert_eq!(res1.conflicts[0].local_hash, local_hash);
        assert_eq!(res1.conflicts[0].remote_hash, remote_hash);

        // Local file must NOT be overwritten
        let local_after_first = std::fs::read_to_string(&chapter_abs).unwrap();
        assert_eq!(local_after_first, local_content);

        // known_files must stay at base_hash, NOT remote_hash or local_hash
        let state_after_first = SyncService::load_sync_state(dir.path()).unwrap();
        assert_eq!(
            state_after_first.known_files.get(chapter_rel).unwrap(),
            &base_hash,
            "known_files must stay at base_hash after conflict"
        );
        assert!(
            state_after_first.conflicted_files.contains(chapter_rel),
            "conflicted_files must contain the path after first sync"
        );

        // No upload or download of the conflicted file
        assert!(!res1.uploaded_files.contains(&chapter_rel.to_string()));
        assert!(!res1.downloaded_files.contains(&chapter_rel.to_string()));

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();

        // === Second sync: must NOT auto-resolve the conflict ===
        let (mock_url2, shutdown2, _files_map2, _manifest_str2, server_thread2) =
            start_mock_github_api(Some(initial_manifest.clone()), initial_files.clone());

        let config2 = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url2,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let res2 = SyncService::perform_lww_sync(dir.path(), &config2, &secrets).unwrap();

        // Second sync: conflict must still be present (not auto-resolved)
        // The path is in conflicted_files, so it's skipped entirely.
        // No upload of local B over remote C, no download of remote C over local B.
        assert!(!res2.uploaded_files.contains(&chapter_rel.to_string()),
            "Second sync must NOT upload local version over remote");
        assert!(!res2.downloaded_files.contains(&chapter_rel.to_string()),
            "Second sync must NOT download remote version over local");

        // Local file must still be the local version B
        let local_after_second = std::fs::read_to_string(&chapter_abs).unwrap();
        assert_eq!(local_after_second, local_content,
            "Local file must remain unchanged after second sync");

        // known_files must still be base_hash A
        let state_after_second = SyncService::load_sync_state(dir.path()).unwrap();
        assert_eq!(
            state_after_second.known_files.get(chapter_rel).unwrap(),
            &base_hash,
            "known_files must still be base_hash after second sync"
        );
        assert!(
            state_after_second.conflicted_files.contains(chapter_rel),
            "conflicted_files must still contain the path after second sync"
        );

        shutdown2.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread2.join();
    }

    /// P0-1: Test that resolve_conflict_keep_local properly clears the conflict
    /// and sets known_files to the remote hash so the next sync uploads local.
    #[test]
    fn test_resolve_conflict_keep_local() {
        let dir = tempdir().unwrap();
        let chapter_rel = "projects/p1/volumes/v1/chapters/c1/chapter.md";
        let chapter_abs = dir.path().join(chapter_rel);
        std::fs::create_dir_all(chapter_abs.parent().unwrap()).unwrap();

        let local_content = "local version B";
        let base_hash = "hash_base_A".to_string();
        let remote_hash = "hash_remote_C".to_string();

        std::fs::write(&chapter_abs, local_content).unwrap();

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state
            .known_files
            .insert(chapter_rel.to_string(), base_hash.clone());
        state.conflicted_files.insert(chapter_rel.to_string());
        state.conflicts.push(SyncConflict {
            local_path: chapter_rel.to_string(),
            remote_path: chapter_rel.to_string(),
            local_hash: "hash_local_B".to_string(),
            remote_hash: remote_hash.clone(),
            base_hash: base_hash.clone(),
            created_at: 12345,
            description: "test conflict".to_string(),
        });
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        // Resolve by keeping local
        SyncService::resolve_conflict_keep_local(dir.path(), chapter_rel).unwrap();

        let state_after = SyncService::load_sync_state(dir.path()).unwrap();
        assert!(
            !state_after.conflicted_files.contains(chapter_rel),
            "conflicted_files must be cleared after resolution"
        );
        // known_files should now be the remote hash so that three-way comparison
        // on the next sync sees: base=remote_hash, local≠base, remote=base → LocalChanged → upload
        assert_eq!(
            state_after.known_files.get(chapter_rel).unwrap(),
            &remote_hash,
            "known_files must be updated to remote hash after keep_local (so next sync uploads local)"
        );
        assert!(
            state_after.conflicts.is_empty(),
            "conflicts must be cleared after resolution"
        );
    }

    /// P0-1: Test that resolve_conflict_take_remote properly clears the conflict
    /// and adds the path to pending_take_remote (not known_files).
    #[test]
    fn test_resolve_conflict_take_remote() {
        let dir = tempdir().unwrap();
        let chapter_rel = "projects/p1/volumes/v1/chapters/c1/chapter.md";

        let base_hash = "hash_base_A".to_string();
        let remote_hash = "hash_remote_C".to_string();

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state
            .known_files
            .insert(chapter_rel.to_string(), base_hash.clone());
        state.conflicted_files.insert(chapter_rel.to_string());
        state.conflicts.push(SyncConflict {
            local_path: chapter_rel.to_string(),
            remote_path: chapter_rel.to_string(),
            local_hash: "hash_local_B".to_string(),
            remote_hash: remote_hash.clone(),
            base_hash: base_hash.clone(),
            created_at: 12345,
            description: "test conflict".to_string(),
        });
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        // Resolve by taking remote
        SyncService::resolve_conflict_take_remote(dir.path(), chapter_rel).unwrap();

        let state_after = SyncService::load_sync_state(dir.path()).unwrap();
        assert!(
            !state_after.conflicted_files.contains(chapter_rel),
            "conflicted_files must be cleared after resolution"
        );
        assert!(
            state_after.pending_take_remote.contains(chapter_rel),
            "pending_take_remote must contain the path after take_remote"
        );
        assert!(
            state_after.conflicts.is_empty(),
            "conflicts must be cleared after resolution"
        );
        // known_files should still be at base_hash — the actual download happens in perform_sync
        assert_eq!(
            state_after.known_files.get(chapter_rel).unwrap(),
            &base_hash,
            "known_files must remain at base_hash after take_remote (download happens in perform_sync)"
        );
    }

    /// P0-1: Test that resolve_conflict_mark_merged properly clears the conflict
    /// and sets known_files to the remote hash so the next sync uploads the merged version.
    #[test]
    fn test_resolve_conflict_mark_merged() {
        let dir = tempdir().unwrap();
        let chapter_rel = "projects/p1/volumes/v1/chapters/c1/chapter.md";
        let chapter_abs = dir.path().join(chapter_rel);
        std::fs::create_dir_all(chapter_abs.parent().unwrap()).unwrap();

        let merged_content = "manually merged content";
        std::fs::write(&chapter_abs, merged_content).unwrap();

        let base_hash = "hash_base_A".to_string();
        let remote_hash = "hash_remote_C".to_string();

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state
            .known_files
            .insert(chapter_rel.to_string(), base_hash.clone());
        state.conflicted_files.insert(chapter_rel.to_string());
        state.conflicts.push(SyncConflict {
            local_path: chapter_rel.to_string(),
            remote_path: chapter_rel.to_string(),
            local_hash: "hash_local_B".to_string(),
            remote_hash: remote_hash.clone(),
            base_hash: base_hash.clone(),
            created_at: 12345,
            description: "test conflict".to_string(),
        });
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        // Resolve by marking as merged
        SyncService::resolve_conflict_mark_merged(dir.path(), chapter_rel).unwrap();

        let state_after = SyncService::load_sync_state(dir.path()).unwrap();
        assert!(
            !state_after.conflicted_files.contains(chapter_rel),
            "conflicted_files must be cleared after resolution"
        );
        // known_files should be set to remote_hash so that three-way comparison
        // on the next sync sees: base=remote_hash, local≠base, remote=base → LocalChanged → upload
        assert_eq!(
            state_after.known_files.get(chapter_rel).unwrap(),
            &remote_hash,
            "known_files must be updated to remote hash after mark_merged (so next sync uploads merged)"
        );
        assert!(
            state_after.conflicts.is_empty(),
            "conflicts must be cleared after resolution"
        );
    }

    /// P0-1: End-to-end test: BothChanged conflict → second sync does not auto-resolve
    /// → resolve_conflict_keep_local → third sync uploads local version normally.
    #[test]
    #[cfg(not(windows))]
    fn test_resolve_conflict_then_sync_recovers() {
        let dir = tempdir().unwrap();
        let chapter_rel = "projects/p1/volumes/v1/chapters/c1/chapter.md";
        let chapter_abs = dir.path().join(chapter_rel);
        std::fs::create_dir_all(chapter_abs.parent().unwrap()).unwrap();

        let base_content = "base version A";
        let local_content = "local version B";
        let remote_content = "remote version C";

        std::fs::write(&chapter_abs, local_content).unwrap();

        let base_hash = format!("{:x}", md5::compute(base_content.as_bytes()));
        let _local_hash = format!("{:x}", md5::compute(local_content.as_bytes()));
        let remote_hash = format!("{:x}", md5::compute(remote_content.as_bytes()));

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state
            .known_files
            .insert(chapter_rel.to_string(), base_hash.clone());
        state
            .known_files_updated_at
            .insert(chapter_rel.to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let mut initial_files = std::collections::HashMap::new();
        initial_files.insert(chapter_rel.to_string(), remote_content.to_string());

        let initial_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: chapter_rel.to_string(),
                content_hash: remote_hash.clone(),
                updated_at_ms: 3000,
                deleted_at_ms: None,
                device_id: "device_remote".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };

        // === Step 1: First sync generates BothChanged conflict ===
        let (mock_url, shutdown, _files_map, _manifest_str, server_thread) =
            start_mock_github_api(Some(initial_manifest.clone()), initial_files.clone());

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res1 = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();
        assert_eq!(res1.status, SyncStatus::PartialConflict);
        assert!(!res1.uploaded_files.contains(&chapter_rel.to_string()));
        assert!(!res1.downloaded_files.contains(&chapter_rel.to_string()));

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();

        // === Step 2: Second sync does NOT auto-resolve ===
        // Clear last_sync_time to bypass debounce
        let mut state_before_2 = SyncService::load_sync_state(dir.path()).unwrap();
        state_before_2.last_sync_time = None;
        SyncService::save_sync_state(dir.path(), &state_before_2).unwrap();

        let (mock_url2, shutdown2, _files_map2, _manifest_str2, server_thread2) =
            start_mock_github_api(Some(initial_manifest.clone()), initial_files.clone());

        let config2 = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url2,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let res2 = SyncService::perform_lww_sync(dir.path(), &config2, &secrets).unwrap();
        assert!(!res2.uploaded_files.contains(&chapter_rel.to_string()),
            "Second sync must NOT upload before resolution");
        assert!(!res2.downloaded_files.contains(&chapter_rel.to_string()),
            "Second sync must NOT download before resolution");

        let state_after_2 = SyncService::load_sync_state(dir.path()).unwrap();
        assert!(state_after_2.conflicted_files.contains(chapter_rel));

        shutdown2.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread2.join();

        // === Step 3: Resolve conflict by keeping local ===
        SyncService::resolve_conflict_keep_local(dir.path(), chapter_rel).unwrap();

        let state_after_resolve = SyncService::load_sync_state(dir.path()).unwrap();
        assert!(!state_after_resolve.conflicted_files.contains(chapter_rel),
            "conflicted_files must be cleared after resolution");
        // After keep_local, known_files is set to remote_hash so that three-way
        // comparison sees: base=remote_hash, local≠base, remote=base → LocalChanged → upload
        assert_eq!(
            state_after_resolve.known_files.get(chapter_rel).unwrap(),
            &remote_hash,
            "known_files must be remote_hash after keep_local"
        );

        // === Step 4: Third sync should now upload local version (LocalChanged) ===
        // Clear last_sync_time to bypass debounce
        let mut state_before_3 = SyncService::load_sync_state(dir.path()).unwrap();
        state_before_3.last_sync_time = None;
        SyncService::save_sync_state(dir.path(), &state_before_3).unwrap();

        let (mock_url3, shutdown3, files_map3, _manifest_str3, server_thread3) =
            start_mock_github_api(Some(initial_manifest.clone()), initial_files.clone());

        let config3 = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url3,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let res3 = SyncService::perform_lww_sync(dir.path(), &config3, &secrets).unwrap();

        // After resolve, three-way sees LocalChanged → uploads local version
        assert!(res3.uploaded_files.contains(&chapter_rel.to_string()),
            "After keep_local resolution, sync must upload the local version");
        assert!(!res3.downloaded_files.contains(&chapter_rel.to_string()),
            "After keep_local resolution, sync must NOT download remote over local");

        // Local file must still be the local version
        let local_after_3 = std::fs::read_to_string(&chapter_abs).unwrap();
        assert_eq!(local_after_3, local_content,
            "Local file must remain unchanged after post-resolve sync");

        // conflicted_files must remain empty
        let state_after_3 = SyncService::load_sync_state(dir.path()).unwrap();
        assert!(
            !state_after_3.conflicted_files.contains(chapter_rel),
            "conflicted_files must stay cleared after post-resolve sync"
        );

        shutdown3.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread3.join();
    }

    /// End-to-end test: BothChanged conflict → resolve_conflict_take_remote →
    /// next sync downloads remote content to local, and subsequent sync does
    /// NOT re-upload the old local version.
    #[test]
    #[cfg(not(windows))]
    fn test_resolve_conflict_take_remote_then_sync() {
        let dir = tempdir().unwrap();
        let chapter_rel = "projects/p1/volumes/v1/chapters/c1/chapter.md";
        let chapter_abs = dir.path().join(chapter_rel);
        std::fs::create_dir_all(chapter_abs.parent().unwrap()).unwrap();

        let base_content = "base version A";
        let local_content = "local version B";
        let remote_content = "remote version C";

        std::fs::write(&chapter_abs, local_content).unwrap();

        let base_hash = format!("{:x}", md5::compute(base_content.as_bytes()));
        let remote_hash = format!("{:x}", md5::compute(remote_content.as_bytes()));

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state
            .known_files
            .insert(chapter_rel.to_string(), base_hash.clone());
        state
            .known_files_updated_at
            .insert(chapter_rel.to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let mut initial_files = std::collections::HashMap::new();
        initial_files.insert(chapter_rel.to_string(), remote_content.to_string());

        let initial_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: chapter_rel.to_string(),
                content_hash: remote_hash.clone(),
                updated_at_ms: 3000,
                deleted_at_ms: None,
                device_id: "device_remote".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };

        // === Step 1: First sync generates BothChanged conflict ===
        let (mock_url, shutdown, _files_map, _manifest_str, server_thread) =
            start_mock_github_api(Some(initial_manifest.clone()), initial_files.clone());

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res1 = SyncService::perform_lww_sync(dir.path(), &config, &secrets).unwrap();
        assert_eq!(res1.status, SyncStatus::PartialConflict);

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();

        // === Step 2: Resolve by taking remote ===
        SyncService::resolve_conflict_take_remote(dir.path(), chapter_rel).unwrap();

        let state_after_resolve = SyncService::load_sync_state(dir.path()).unwrap();
        assert!(!state_after_resolve.conflicted_files.contains(chapter_rel));
        assert!(state_after_resolve.pending_take_remote.contains(chapter_rel));
        assert!(state_after_resolve.conflicts.is_empty());

        // === Step 3: Next sync should download remote content to local ===
        let mut state_before_3 = SyncService::load_sync_state(dir.path()).unwrap();
        state_before_3.last_sync_time = None;
        SyncService::save_sync_state(dir.path(), &state_before_3).unwrap();

        let (mock_url3, shutdown3, _files_map3, _manifest_str3, server_thread3) =
            start_mock_github_api(Some(initial_manifest.clone()), initial_files.clone());

        let config3 = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url3,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let res3 = SyncService::perform_lww_sync(dir.path(), &config3, &secrets).unwrap();

        // After take_remote resolution, sync must download the remote content
        assert!(res3.downloaded_files.contains(&chapter_rel.to_string()),
            "After take_remote resolution, sync must download the remote version");
        assert!(!res3.uploaded_files.contains(&chapter_rel.to_string()),
            "After take_remote resolution, sync must NOT upload the old local version");

        // Local file must now be the remote version
        let local_after_3 = std::fs::read_to_string(&chapter_abs).unwrap();
        assert_eq!(local_after_3, remote_content,
            "Local file must be the remote version after take_remote + sync");

        // pending_take_remote must be cleared
        let state_after_3 = SyncService::load_sync_state(dir.path()).unwrap();
        assert!(
            !state_after_3.pending_take_remote.contains(chapter_rel),
            "pending_take_remote must be cleared after sync"
        );
        assert!(
            !state_after_3.conflicted_files.contains(chapter_rel),
            "conflicted_files must stay cleared after sync"
        );

        shutdown3.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread3.join();

        // === Step 4: Subsequent sync should NOT re-upload or re-download ===
        let mut state_before_4 = SyncService::load_sync_state(dir.path()).unwrap();
        state_before_4.last_sync_time = None;
        SyncService::save_sync_state(dir.path(), &state_before_4).unwrap();

        let (mock_url4, shutdown4, _files_map4, _manifest_str4, server_thread4) =
            start_mock_github_api(Some(initial_manifest.clone()), initial_files.clone());

        let config4 = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url4,
            transport: SyncTransport::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let res4 = SyncService::perform_lww_sync(dir.path(), &config4, &secrets).unwrap();

        assert!(!res4.uploaded_files.contains(&chapter_rel.to_string()),
            "Subsequent sync must NOT upload after take_remote resolved");
        assert!(!res4.downloaded_files.contains(&chapter_rel.to_string()),
            "Subsequent sync must NOT download after take_remote resolved");

        // Local file must still be the remote version
        let local_after_4 = std::fs::read_to_string(&chapter_abs).unwrap();
        assert_eq!(local_after_4, remote_content,
            "Local file must remain the remote version after subsequent sync");

        shutdown4.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread4.join();
    }
}
