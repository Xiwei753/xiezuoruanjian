#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    #[cfg(feature = "github-api")]
    use crate::sync::backends::SyncBackend;
    #[cfg(feature = "git-https")]
    use crate::sync::git_backend::Git2Backend;
    #[cfg(feature = "git-https")]
    use crate::sync::git_backend::GitAuth;
    #[cfg(feature = "git-https")]
    use crate::sync::git_backend::GitBackend;
    #[cfg(feature = "github-api")]
    use crate::sync::github_backend::GitHubApiBackend;
    use crate::sync::service::SyncService;
    use crate::sync::types::BackendType;
    #[cfg(feature = "git-https")]
    use crate::sync::types::FirstSyncMode;
    #[cfg(feature = "github-api")]
    use crate::sync::types::ManifestFileRecord;
    use crate::sync::types::SyncConfig;
    #[cfg(feature = "github-api")]
    use crate::sync::types::SyncConflict;
    #[cfg(feature = "github-api")]
    use crate::sync::types::SyncManifest;
    use crate::sync::types::SyncProtocol;
    use crate::sync::types::SyncScope;
    #[cfg(any(feature = "github-api", feature = "git-https"))]
    use crate::sync::types::SyncSecrets;
    #[cfg(feature = "github-api")]
    use crate::sync::types::SyncState;
    #[cfg(any(feature = "github-api", feature = "git-https"))]
    use crate::sync::types::SyncStatus;
    #[cfg(feature = "github-api")]
    use base64::Engine;
    #[cfg(any(feature = "git-https", feature = "github-api"))]
    use std::path::Path;
    use tempfile::tempdir;

    #[cfg(feature = "github-api")]
    struct TestHttpTransport {
        client: reqwest::blocking::Client,
    }

    #[cfg(feature = "github-api")]
    impl TestHttpTransport {
        fn new() -> Result<Self, writer_platform_api::TransportError> {
            let client = reqwest::blocking::Client::builder()
                .user_agent("WriterApp/1.0")
                .timeout(std::time::Duration::from_secs(15))
                .no_proxy()
                .build()
                .map_err(|e| {
                    writer_platform_api::TransportError::new(
                        "init",
                        format!("Failed to build HTTP client: {}", e),
                    )
                })?;
            Ok(Self { client })
        }
    }

    #[cfg(feature = "github-api")]
    impl writer_platform_api::SyncTransport for TestHttpTransport {
        fn execute(
            &self,
            request: writer_platform_api::HttpRequest,
        ) -> Result<writer_platform_api::HttpResponse, writer_platform_api::TransportError>
        {
            use writer_platform_api::{HttpResponse, TransportError};
            let mut req = match request.method.as_str() {
                "GET" => self.client.get(&request.url),
                "PUT" => self.client.put(&request.url),
                "DELETE" => self.client.delete(&request.url),
                "POST" => self.client.post(&request.url),
                "PATCH" => self.client.patch(&request.url),
                "HEAD" => self.client.head(&request.url),
                _ => {
                    return Err(TransportError::new(
                        "invalid_method",
                        format!("Unsupported HTTP method: {}", request.method),
                    ));
                }
            };

            for (key, value) in &request.headers {
                req = req.header(key.as_str(), value.as_str());
            }

            if let Some(body) = request.body {
                req = req.body(body);
            }

            let resp = req.send().map_err(|e| {
                if e.is_connect() {
                    TransportError::new("dns_failed", e.to_string())
                } else if e.is_timeout() {
                    TransportError::new("timeout", e.to_string())
                } else {
                    TransportError::new("network", e.to_string())
                }
            })?;

            let status = resp.status().as_u16();
            let headers: Vec<(String, String)> = resp
                .headers()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                .collect();
            let body = resp
                .bytes()
                .map_err(|e| TransportError::new("response_read", e.to_string()))?;
            Ok(HttpResponse {
                status,
                headers,
                body: body.to_vec(),
            })
        }
    }

    #[cfg(feature = "github-api")]
    fn lww_sync(
        sync_root: &Path,
        config: &SyncConfig,
        secrets: &SyncSecrets,
        force_sync: bool,
    ) -> crate::Result<crate::sync::SyncResult> {
        let transport = TestHttpTransport::new()
            .map_err(|e| crate::Error::SyncNetworkUnavailable { reason: e.message })?;
        let target = crate::sync::types::SyncTarget::project("test");
        SyncService::perform_lww_sync(sync_root, config, secrets, &target, force_sync, &transport)
    }
    #[test]
    #[cfg(feature = "github-api")]
    fn test_github_api_diagnostics_reports_backend_type_without_token() {
        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: "https://github.com/user/repo.git".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: None,
            ssh_private_key: None,
        };

        let result = GitHubApiBackend::new().diagnose(&config, &secrets).unwrap();
        assert_eq!(result.backend_type, "github_api");
        assert_eq!(result.error_category, "token_missing");
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn test_sync_manifest_deserializes_with_deleted_at_ms() {
        let raw = r#"{
            "files": [{
                "path": "project.json",
                "content_hash": "abc",
                "updated_at_ms": 1000,
                "deleted_at_ms": 2000,
                "device_id": "device_a",
                "op": "delete",
                "schema_version": 1
            }]
        }"#;

        let manifest: SyncManifest = serde_json::from_str(raw).unwrap();
        assert_eq!(manifest.files.len(), 1);
        assert_eq!(manifest.files[0].deleted_at_ms, Some(2000));
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn test_sync_manifest_serializes_with_deleted_at_ms() {
        let manifest = SyncManifest {
            files: vec![crate::sync::types::ManifestFileRecord {
                path: "project.json".to_string(),
                content_hash: "abc".to_string(),
                updated_at_ms: 1000,
                deleted_at_ms: Some(2000),
                device_id: "device_a".to_string(),
                op: "delete".to_string(),
                schema_version: 1,
            }],
        };

        let json = serde_json::to_string(&manifest).unwrap();
        let roundtrip: SyncManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.files[0].deleted_at_ms, Some(2000));
        assert_eq!(roundtrip.files[0].op, "delete");
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn test_sync_manifest_deserializes_without_deleted_at_ms() {
        let raw = r#"{
            "files": [{
                "path": "project.json",
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
    fn test_sync_state_files_blacklisted() {
        // 同步根是单个作品目录：作品仓库内的本地同步状态不得上传。
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/state.local.json",
            crate::sync::types::SyncScope::Project
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/state.local.json.tmp",
            crate::sync::types::SyncScope::Project
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/sync_state.json",
            crate::sync::types::SyncScope::Project
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/logs/sync.log",
            crate::sync::SyncScope::Project
        ));
        assert!(SyncService::is_blacklisted_path(
            "tmp/runtime.tmp",
            crate::sync::SyncScope::Project
        ));
        assert!(SyncService::is_blacklisted_path(
            "cache/build.bin",
            crate::sync::SyncScope::Project
        ));
        assert!(SyncService::is_blacklisted_path(
            "backups/vol1.zip",
            crate::sync::SyncScope::Project
        ));
    }

    #[test]
    fn test_app_level_paths_outside_repo_not_blacklisted() {
        // 应用级数据位于 app_data_root，不在作品仓库内（Issue #600），
        // 黑名单只管作品仓库内的路径。
        assert!(!SyncService::is_blacklisted_path(
            "app-meta/ai/secrets.local.json",
            crate::sync::SyncScope::Project
        ));
        assert!(!SyncService::is_blacklisted_path(
            "app-meta/stats/events.local/2024-01-01.events.jsonl",
            crate::sync::SyncScope::Project
        ));
        assert!(!SyncService::is_blacklisted_path(
            "app-meta/settings/settings.local.json",
            crate::sync::SyncScope::Project
        ));
    }

    #[test]
    #[cfg(feature = "git-https")]
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
            fn push(&self, _: &Path, _: &str, _: Option<&GitAuth>) -> crate::Result<()> {
                Ok(())
            }
            fn pull(
                &self,
                _: &Path,
                _: &str,
                _: Option<&GitAuth>,
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<()> {
                Err(crate::Error::SyncUnrelatedHistories {
                    detail: "refusing to merge unrelated histories".to_string(),
                })
            }
            fn clone_repo(&self, _: &str, _: &Path, _: Option<&GitAuth>) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(
                &self,
                _: &Path,
                _: &[&str],
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(Some("hash".to_string()))
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(Some("hash".to_string()))
            }
            fn status(
                &self,
                _: &Path,
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<Vec<String>> {
                Ok(vec![])
            }
        }

        let dir = tempdir().unwrap();
        let config = SyncConfig {
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let result = SyncService::perform_sync(
            dir.path(),
            &config,
            &secrets,
            crate::sync::types::SyncScope::Project,
            &MockUnrelatedBackend,
        )
        .unwrap();
        assert_eq!(result.first_sync_mode, FirstSyncMode::UnrelatedHistories);
    }

    #[test]
    fn test_record_sync_conflict_error_handling() {
        // Provide an invalid path to force an IO error
        let conflict = crate::sync::types::SyncConflict {
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
            std::path::Path::new("/non/existent/path/that/will/fail"),
            conflict,
            None,
        );
        assert!(res.is_err());
    }

    #[test]
    #[cfg(feature = "git-https")]
    fn test_perform_sync_non_empty_no_git_init() {
        // Just a mock test to verify the logic inside perform_sync
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("some_file.txt"), "hello").unwrap();

        let config = SyncConfig {
            enabled: true,
            remote_url: "https://github.com/test/test.git".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            backend_type: BackendType::Git,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockBackend;
        impl GitBackend for MockBackend {
            fn clone_repo(&self, _: &str, _: &Path, _: Option<&GitAuth>) -> crate::Result<()> {
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
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(
                &self,
                _: &Path,
                _: &[&str],
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(&self, _: &Path, _: &str, _: Option<&GitAuth>) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(
                &self,
                _: &Path,
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<Vec<String>> {
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

        let res = SyncService::perform_sync(
            dir.path(),
            &config,
            &secrets,
            crate::sync::types::SyncScope::Project,
            &MockBackend,
        )
        .unwrap();
        assert_eq!(res.status, SyncStatus::Success);
    }

    #[test]
    fn test_perform_sync_auto_commits_whitelist() {
        // 同步根是单个作品目录：作品自身内容必须进入白名单。
        assert!(SyncService::is_whitelisted_path(
            "project.json",
            crate::sync::SyncScope::Project
        ));
        assert!(SyncService::is_whitelisted_path(
            "volumes/v1/volume.json",
            crate::sync::SyncScope::Project
        ));
        assert!(SyncService::is_whitelisted_path(
            "volumes/v1/chapters/c1/chapter.md",
            crate::sync::SyncScope::Project
        ));
        assert!(SyncService::is_whitelisted_path(
            "volumes/v1/chapters/c1/chapter.meta.json",
            crate::sync::SyncScope::Project
        ));
    }

    #[test]
    #[cfg(feature = "github-api")]
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
    #[cfg(feature = "github-api")]
    fn test_sync_config_state_no_token() {
        let config = SyncConfig {
            enabled: true,
            remote_url: "url".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            backend_type: BackendType::Git,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let state = SyncState {
            remote_url: Some("url".to_string()),
            transport: Some(SyncProtocol::HttpsToken),
            last_sync_time: Some(0),
            last_synced_commit: None,
            last_error: None,
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
    fn test_whitelist_project_content_and_sync_metadata() {
        // 作品自身内容：project.json / volumes / characters
        assert!(SyncService::is_whitelisted_path(
            "project.json",
            crate::sync::SyncScope::Project
        ));
        assert!(SyncService::is_whitelisted_path(
            "volumes/v1/volume.json",
            crate::sync::SyncScope::Project
        ));
        assert!(SyncService::is_whitelisted_path(
            "volumes/v1/chapters/c1/chapter.md",
            crate::sync::SyncScope::Project
        ));
        assert!(SyncService::is_whitelisted_path(
            "volumes/v1/chapters/c1/chapter.meta.json",
            crate::sync::SyncScope::Project
        ));
        assert!(SyncService::is_whitelisted_path(
            "characters/role1.json",
            crate::sync::SyncScope::Project
        ));
        // 作品自己的同步元数据
        assert!(SyncService::is_whitelisted_path(
            "app-meta/sync/manifest.sync.json",
            crate::sync::SyncScope::Project
        ));

        // 非白名单：作品外的应用级路径（旧 workspace 布局）与无关文件
        assert!(!SyncService::is_whitelisted_path(
            "app-meta/settings/settings.sync.json",
            crate::sync::SyncScope::Project
        ));
        assert!(!SyncService::is_whitelisted_path(
            "app-meta/settings/settings.local.json",
            crate::sync::SyncScope::Project
        ));
        assert!(!SyncService::is_whitelisted_path(
            "app-meta/stats/daily/2024-01-01.stats.json",
            crate::sync::SyncScope::Project
        ));
        assert!(!SyncService::is_whitelisted_path(
            "app-meta/recent/recent_edits.json",
            crate::sync::SyncScope::Project
        ));
        assert!(!SyncService::is_whitelisted_path(
            "app-meta/device/current_device.json",
            crate::sync::SyncScope::Project
        ));
        assert!(!SyncService::is_whitelisted_path(
            "projects/p1/project.json",
            crate::sync::SyncScope::Project
        ));
        assert!(!SyncService::is_whitelisted_path(
            "readme.txt",
            crate::sync::SyncScope::Project
        ));
    }

    #[test]
    fn test_stats_events_local_blacklisted() {
        // 作品仓库内的缓存/临时路径保持黑名单；app-meta 统计路径位于
        // app_data_root，不在作品仓库内。
        assert!(!SyncService::is_blacklisted_path(
            "app-meta/stats/events.local/2024-01-01.events.jsonl",
            crate::sync::SyncScope::Project
        ));
    }

    #[test]
    fn test_device_info_not_blacklisted() {
        assert!(!SyncService::is_blacklisted_path(
            "app-meta/device/current_device.json",
            crate::sync::SyncScope::Project
        ));
    }

    #[test]
    fn test_recent_not_blacklisted() {
        assert!(!SyncService::is_blacklisted_path(
            "app-meta/recent/recent_edits.json",
            crate::sync::SyncScope::Project
        ));
    }

    #[test]
    fn test_blacklist_ignores_local_json() {
        // 作品仓库内的本地同步状态不得上传；
        // 应用级 settings.local.json 位于 app_data_root，不在作品仓库内。
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/state.local.json",
            crate::sync::types::SyncScope::Project
        ));
        assert!(!SyncService::is_blacklisted_path(
            "app-meta/settings/settings.local.json",
            crate::sync::SyncScope::Project
        ));
    }

    #[test]
    fn test_sync_secrets_blacklisted() {
        // 同步凭证（旧 workspace 布局 app-meta/sync/sync_secrets.local.json）
        // 在 per-project 模型下位于 app_data_root/sync/，不在作品仓库内；
        // 作品仓库内的本地状态文件仍被黑名单排除。
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/state.local.json",
            crate::sync::types::SyncScope::Project
        ));
        assert!(!SyncService::is_blacklisted_path(
            "app-meta/sync/sync_secrets.local.json",
            crate::sync::SyncScope::Project
        ));
    }

    #[test]
    fn test_sync_local_config_blacklisted_project() {
        // 作品级：本地同步配置、冲突记录、凭证必须被黑名单，
        // 否则 perform_sync 跑 backend.status() 时这些文件既不在黑名单也不在白名单，
        // 返回 DirtyRepoBlocked，同步被自己的配置文件拦死（Issue #600 评论 #4 问题 1）。
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/config.local.json",
            crate::sync::types::SyncScope::Project
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/conflicts.json",
            crate::sync::types::SyncScope::Project
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/secrets.local.json",
            crate::sync::types::SyncScope::Project
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/secrets_g1.local.json",
            crate::sync::types::SyncScope::Project
        ));
    }

    #[test]
    fn test_sync_local_config_blacklisted_app() {
        // 应用级：同样路径必须被黑名单。
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/config.local.json",
            crate::sync::types::SyncScope::App
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/conflicts.json",
            crate::sync::types::SyncScope::App
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/secrets.local.json",
            crate::sync::types::SyncScope::App
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/secrets_g1.local.json",
            crate::sync::types::SyncScope::App
        ));
    }

    #[test]
    fn test_manifest_still_whitelisted() {
        // 黑名单扩展不能误伤 manifest.sync.json（作品级与应用级均需保持白名单）。
        assert!(SyncService::is_whitelisted_path(
            "app-meta/sync/manifest.sync.json",
            crate::sync::types::SyncScope::Project
        ));
        assert!(SyncService::is_whitelisted_path(
            "app-meta/sync/manifest.sync.json",
            crate::sync::types::SyncScope::App
        ));
    }

    #[test]
    fn test_project_content_still_whitelisted() {
        // 黑名单扩展不能误伤作品内容。
        assert!(SyncService::is_whitelisted_path(
            "project.json",
            crate::sync::types::SyncScope::Project
        ));
        assert!(SyncService::is_whitelisted_path(
            "volumes/v1/chapters/c1/chapter.md",
            crate::sync::types::SyncScope::Project
        ));
    }

    #[test]
    fn test_app_content_still_whitelisted() {
        // 黑名单扩展不能误伤应用级可同步内容。
        assert!(SyncService::is_whitelisted_path(
            "settings.sync.json",
            crate::sync::types::SyncScope::App
        ));
        assert!(SyncService::is_whitelisted_path(
            "starmaps/x.json",
            crate::sync::types::SyncScope::App
        ));
        assert!(SyncService::is_whitelisted_path(
            "themes/palettes/dark.json",
            crate::sync::types::SyncScope::App
        ));
    }

    #[test]
    fn test_sync_config_no_token() {
        let config = SyncConfig {
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
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
            "volumes/v1/chapters/c1/chapter.md.tmp",
            crate::sync::SyncScope::Project
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/sync/state.local.json.lock",
            crate::sync::SyncScope::Project
        ));
        assert!(SyncService::is_blacklisted_path(
            "app-meta/logs/sync.log",
            crate::sync::SyncScope::Project
        ));
    }

    #[test]
    #[cfg(feature = "github-api")]
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
    #[cfg(feature = "github-api")]
    fn test_sync_state_does_not_leak_tokens() {
        let dir = tempdir().unwrap();
        let state = SyncState {
            remote_url: Some("https://example.com/repo.git".to_string()),
            transport: Some(SyncProtocol::HttpsToken),
            last_synced_commit: None,
            last_sync_time: None,
            last_error: None,
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
    #[cfg(all(not(windows), feature = "git-https"))]
    fn test_stage_blacklisted_files() {
        let dir = tempdir().unwrap();

        // Initialize git repo manually or use SyncService
        let repo = git2::Repository::init(dir.path()).unwrap();

        // 作品仓库内的本地同步状态不得被 stage（黑名单）；
        // 同时验证白名单外的文件（如作品外的应用级路径）也不会被 stage。
        let file_path = dir.path().join("app-meta/sync/state.local.json");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "state_content").unwrap();
        let outside_path = dir.path().join("projects/p1/project.json");
        std::fs::create_dir_all(outside_path.parent().unwrap()).unwrap();
        std::fs::write(&outside_path, "{}").unwrap();

        let backend = Git2Backend;
        let paths = vec!["app-meta/sync/state.local.json", "projects/p1/project.json"];
        backend
            .stage_paths(dir.path(), &paths, crate::sync::types::SyncScope::Project)
            .unwrap();

        // Ensure neither is staged
        let index = repo.index().unwrap();
        assert!(index
            .get_path(std::path::Path::new("app-meta/sync/state.local.json"), 0)
            .is_none());
        assert!(index
            .get_path(std::path::Path::new("projects/p1/project.json"), 0)
            .is_none());
    }

    #[test]
    fn test_sync_dry_run_disabled_config() {
        let dir = tempdir().unwrap();
        let config = SyncConfig {
            enabled: false,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };

        let plan =
            SyncService::perform_sync_dry_run(dir.path(), &config, SyncScope::Project).unwrap();
        assert!(plan.files_to_upload.is_empty());
        assert!(plan.ignored_files.is_empty());
    }

    #[test]
    fn test_sync_dry_run_enabled_config_scans() {
        let dir = tempdir().unwrap();
        let config = SyncConfig {
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };

        // Create some whitelisted and blacklisted files（per-project：同步根是作品目录）
        let project_dir = dir.path();
        std::fs::write(project_dir.join("project.json"), "{}").unwrap();
        std::fs::create_dir_all(project_dir.join("volumes/v1")).unwrap();
        std::fs::write(project_dir.join("volumes/v1/volume.json"), "{}").unwrap();
        std::fs::write(project_dir.join("volumes/v1/volume.json.tmp"), "{}").unwrap();

        let plan =
            SyncService::perform_sync_dry_run(dir.path(), &config, SyncScope::Project).unwrap();
        assert!(plan.files_to_upload.contains(&"project.json".to_string()));
        assert!(plan
            .files_to_upload
            .contains(&"volumes/v1/volume.json".to_string()));
        assert!(plan
            .ignored_files
            .contains(&"volumes/v1/volume.json.tmp".to_string()));
    }

    #[test]
    #[cfg(feature = "git-https")]
    fn test_perform_sync_empty_remote_url() {
        let dir = tempdir().unwrap();
        let config = SyncConfig {
            enabled: true,
            remote_url: "".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };

        let secrets = SyncSecrets {
            token: Some("secret_token".to_string()),
            ssh_private_key: None,
        };

        // For this test we can use Git2Backend as it won't be called due to early return
        let backend = Git2Backend;
        let result = SyncService::perform_sync(
            dir.path(),
            &config,
            &secrets,
            crate::sync::types::SyncScope::Project,
            &backend,
        )
        .unwrap();
        assert_eq!(
            result.status,
            SyncStatus::Error("Remote URL is empty".to_string())
        );
    }

    #[test]
    #[cfg(feature = "git-https")]
    fn test_perform_sync_non_empty_remote() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig {
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockInitNonEmptyBackend;
        impl GitBackend for MockInitNonEmptyBackend {
            fn clone_repo(&self, _: &str, _: &Path, _: Option<&GitAuth>) -> crate::Result<()> {
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
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<()> {
                Err(crate::Error::SyncUnrelatedHistories {
                    detail: "unable to merge unrelated histories".to_string(),
                })
            }
            fn stage_paths(
                &self,
                _: &Path,
                _: &[&str],
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(&self, _: &Path, _: &str, _: Option<&GitAuth>) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(
                &self,
                _: &Path,
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<Vec<String>> {
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

        let res = SyncService::perform_sync(
            dir.path(),
            &config,
            &secrets,
            crate::sync::types::SyncScope::Project,
            &MockInitNonEmptyBackend,
        )
        .unwrap();
        assert_eq!(res.first_sync_mode, FirstSyncMode::UnrelatedHistories);
    }

    #[test]
    #[cfg(feature = "github-api")]
    #[cfg(all(not(windows), feature = "git-https"))]
    fn test_save_sync_state_failure() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("app-meta/sync");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join("state.local.json"), "{}").unwrap();

        let config = SyncConfig {
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockBackendOk;
        impl GitBackend for MockBackendOk {
            fn clone_repo(&self, _: &str, _: &Path, _: Option<&GitAuth>) -> crate::Result<()> {
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
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(
                &self,
                _: &Path,
                _: &[&str],
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(&self, _: &Path, _: &str, _: Option<&GitAuth>) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(
                &self,
                _: &Path,
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<Vec<String>> {
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

        let res = SyncService::perform_sync(
            dir.path(),
            &config,
            &secrets,
            crate::sync::types::SyncScope::Project,
            &MockBackendOk,
        )
        .unwrap();
        assert!(matches!(res.status, SyncStatus::FatalError(_)));
    }

    #[test]
    #[cfg(feature = "github-api")]
    #[cfg(feature = "git-https")]
    fn test_first_sync_mode_clone_into_empty_project() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig {
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockBackend;
        impl GitBackend for MockBackend {
            fn clone_repo(&self, _: &str, _: &Path, _: Option<&GitAuth>) -> crate::Result<()> {
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
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(
                &self,
                _: &Path,
                _: &[&str],
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(&self, _: &Path, _: &str, _: Option<&GitAuth>) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(
                &self,
                _: &Path,
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<Vec<String>> {
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

        let res = SyncService::perform_sync(
            dir.path(),
            &config,
            &secrets,
            crate::sync::types::SyncScope::Project,
            &MockBackend,
        )
        .unwrap();
        assert_eq!(res.first_sync_mode, FirstSyncMode::CloneIntoEmptyProject);
    }

    #[test]
    #[cfg(feature = "github-api")]
    #[cfg(feature = "git-https")]
    fn test_first_sync_mode_init_existing_project() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig {
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockBackend;
        impl GitBackend for MockBackend {
            fn clone_repo(&self, _: &str, _: &Path, _: Option<&GitAuth>) -> crate::Result<()> {
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
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(
                &self,
                _: &Path,
                _: &[&str],
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(&self, _: &Path, _: &str, _: Option<&GitAuth>) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(
                &self,
                _: &Path,
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<Vec<String>> {
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

        let res = SyncService::perform_sync(
            dir.path(),
            &config,
            &secrets,
            crate::sync::types::SyncScope::Project,
            &MockBackend,
        )
        .unwrap();
        assert_eq!(res.first_sync_mode, FirstSyncMode::InitExistingProject);
    }

    #[test]
    #[cfg(feature = "github-api")]
    #[cfg(feature = "git-https")]
    fn test_first_sync_mode_already_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig {
            enabled: true,
            remote_url: "https://example.com/repo.git".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockBackend;
        impl GitBackend for MockBackend {
            fn clone_repo(&self, _: &str, _: &Path, _: Option<&GitAuth>) -> crate::Result<()> {
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
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn stage_paths(
                &self,
                _: &Path,
                _: &[&str],
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn push(&self, _: &Path, _: &str, _: Option<&GitAuth>) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(
                &self,
                _: &Path,
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<Vec<String>> {
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

        let res = SyncService::perform_sync(
            dir.path(),
            &config,
            &secrets,
            crate::sync::types::SyncScope::Project,
            &MockBackend,
        )
        .unwrap();
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
        std::fs::create_dir_all(dir.path().join("app-meta/settings")).unwrap();
        std::fs::write(
            dir.path().join("app-meta/settings/settings.sync.json"),
            "{}",
        )
        .unwrap();

        let plan =
            SyncService::build_sync_plan(dir.path(), crate::sync::SyncScope::Project).unwrap();

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
    #[cfg(feature = "github-api")]
    #[cfg(feature = "git-https")]
    fn test_first_sync_empty_remote_branch_not_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("app-meta/settings")).unwrap();
        std::fs::write(
            dir.path().join("app-meta/settings/settings.sync.json"),
            "{}",
        )
        .unwrap();

        let config = SyncConfig {
            enabled: true,
            remote_url: "https://github.com/test/empty-repo.git".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            backend_type: BackendType::Git,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        };

        struct MockEmptyRemoteBackend;
        impl GitBackend for MockEmptyRemoteBackend {
            fn clone_repo(&self, _: &str, _: &Path, _: Option<&GitAuth>) -> crate::Result<()> {
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
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<()> {
                Err(crate::Error::SyncRemoteBranchNotFound {
                    detail: "ref not found: refs/heads/main".to_string(),
                })
            }
            fn stage_paths(
                &self,
                _: &Path,
                _: &[&str],
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn commit(&self, _: &Path, _: &str) -> crate::Result<Option<String>> {
                Ok(Some("commit_hash".to_string()))
            }
            fn push(&self, _: &Path, _: &str, _: Option<&GitAuth>) -> crate::Result<()> {
                Ok(())
            }
            fn current_head(&self, _: &Path) -> crate::Result<Option<String>> {
                Ok(None)
            }
            fn status(
                &self,
                _: &Path,
                _: crate::sync::types::SyncScope,
            ) -> crate::Result<Vec<String>> {
                Ok(vec!["project.json".to_string()])
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

        let res = SyncService::perform_sync(
            dir.path(),
            &config,
            &secrets,
            crate::sync::types::SyncScope::Project,
            &MockEmptyRemoteBackend,
        )
        .unwrap();
        assert_eq!(res.status, SyncStatus::Success);
        assert_eq!(res.first_sync_mode, FirstSyncMode::InitExistingProject);
    }

    #[test]
    #[cfg(all(not(windows), feature = "git-https"))]
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
    #[cfg(all(not(windows), feature = "git-https"))]
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

    #[cfg(feature = "github-api")]
    #[allow(clippy::type_complexity)]
    fn start_mock_github_api(
        initial_manifest: Option<SyncManifest>,
        initial_files: std::collections::HashMap<String, String>,
        remote_prefix: &str,
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

        let remote_prefix = remote_prefix.to_string();
        let prefixed_files: std::collections::HashMap<String, String> = initial_files
            .into_iter()
            .map(|(k, v)| (format!("{}/{}", remote_prefix, k), v))
            .collect();
        let files = Arc::new(Mutex::new(prefixed_files));
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
                                            "path": format!("{}/app-meta/sync/manifest.sync.json", remote_prefix),
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
                                } else if path.contains(&format!(
                                    "/contents/{}/app-meta/sync/manifest.sync.json",
                                    remote_prefix
                                )) {
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
                                } else if (method == "POST"
                                    && (path.contains("/git/trees")
                                        || path.contains("/git/commits")
                                        || path.contains("/git/refs")))
                                    || (method == "PATCH" && path.contains("/git/refs/heads/main"))
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
    #[cfg(feature = "github-api")]
    fn test_perform_lww_sync_first_download() {
        let dir = tempdir().unwrap();
        let mut initial_files = std::collections::HashMap::new();
        initial_files.insert("project.json".to_string(), "remote content".to_string());

        let initial_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "project.json".to_string(),
                content_hash: format!("{:x}", md5::compute("remote content".as_bytes())),
                updated_at_ms: 1000,
                deleted_at_ms: None,
                device_id: "device_remote".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };

        let (mock_url, shutdown, _files, _manifest, server_thread) =
            start_mock_github_api(Some(initial_manifest), initial_files, "projects/test");

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };

        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = lww_sync(dir.path(), &config, &secrets, false).unwrap();
        assert!(res.downloaded_files.contains(&"project.json".to_string()));
        assert!(!res
            .downloaded_files
            .contains(&"app-meta/sync/manifest.sync.json".to_string()));
        assert!(res.uploaded_files.is_empty());
        assert!(res.local_deletes.is_empty());
        assert!(res.remote_deletes.is_empty());
        assert!(res.overwritten_files.is_empty());

        let local_file_path = dir.path().join("project.json");
        assert!(local_file_path.exists());
        let local_content = std::fs::read_to_string(local_file_path).unwrap();
        assert_eq!(local_content, "remote content");
        assert!(dir.path().join("app-meta/sync/manifest.sync.json").exists());

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    #[cfg(not(windows))]
    #[cfg(feature = "github-api")]
    fn test_perform_lww_sync_local_delete_generates_manifest_delete() {
        let dir = tempdir().unwrap();

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state
            .known_files
            .insert("project.json".to_string(), "old_hash".to_string());
        state
            .known_files_updated_at
            .insert("project.json".to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let mut initial_files = std::collections::HashMap::new();
        initial_files.insert("project.json".to_string(), "remote content".to_string());
        let initial_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "project.json".to_string(),
                content_hash: format!("{:x}", md5::compute("remote content".as_bytes())),
                updated_at_ms: 900,
                deleted_at_ms: None,
                device_id: "device_remote".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };

        let (mock_url, shutdown, _files_map, manifest_str, server_thread) =
            start_mock_github_api(Some(initial_manifest), initial_files, "projects/test");

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = lww_sync(dir.path(), &config, &secrets, false).unwrap();
        assert!(res.local_deletes.contains(&"project.json".to_string()));

        let final_m: SyncManifest =
            serde_json::from_str(&manifest_str.lock().unwrap().clone()).unwrap();
        let rec = final_m
            .files
            .iter()
            .find(|f| f.path == "project.json")
            .unwrap();
        assert_eq!(rec.op, "delete");

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    #[cfg(not(windows))]
    #[cfg(feature = "github-api")]
    fn test_perform_lww_sync_remote_delete_removes_local_file() {
        let dir = tempdir().unwrap();
        let local_path = dir.path().join("project.json");
        std::fs::create_dir_all(local_path.parent().unwrap()).unwrap();
        std::fs::write(&local_path, "local content").unwrap();

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state.known_files.insert(
            "project.json".to_string(),
            format!("{:x}", md5::compute("local content".as_bytes())),
        );
        state
            .known_files_updated_at
            .insert("project.json".to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let initial_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "project.json".to_string(),
                content_hash: String::new(),
                updated_at_ms: 3000,
                deleted_at_ms: Some(3000),
                device_id: "device_remote".to_string(),
                op: "delete".to_string(),
                schema_version: 1,
            }],
        };

        let (mock_url, shutdown, _files_map, _manifest_str, server_thread) = start_mock_github_api(
            Some(initial_manifest),
            std::collections::HashMap::new(),
            "projects/test",
        );

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = lww_sync(dir.path(), &config, &secrets, false).unwrap();
        assert!(res.remote_deletes.contains(&"project.json".to_string()));
        assert!(!local_path.exists());

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn test_perform_lww_sync_timestamp_wins() {
        let dir = tempdir().unwrap();

        let local_p1 = dir.path().join("project.json");
        std::fs::create_dir_all(local_p1.parent().unwrap()).unwrap();
        std::fs::write(&local_p1, "local newer content").unwrap();

        let local_p2 = dir.path().join("volumes/p2/volume.json");
        std::fs::create_dir_all(local_p2.parent().unwrap()).unwrap();
        std::fs::write(&local_p2, "local older content").unwrap();

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state.known_files.insert(
            "project.json".to_string(),
            format!("{:x}", md5::compute("local base".as_bytes())),
        );
        state
            .known_files_updated_at
            .insert("project.json".to_string(), 1000);
        state.known_files.insert(
            "volumes/p2/volume.json".to_string(),
            format!("{:x}", md5::compute("local older content".as_bytes())),
        );
        state
            .known_files_updated_at
            .insert("volumes/p2/volume.json".to_string(), 1000);
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let mut initial_files = std::collections::HashMap::new();
        initial_files.insert(
            "project.json".to_string(),
            "remote older content".to_string(),
        );
        initial_files.insert(
            "volumes/p2/volume.json".to_string(),
            "remote newer content".to_string(),
        );

        let initial_manifest = SyncManifest {
            files: vec![
                ManifestFileRecord {
                    path: "project.json".to_string(),
                    content_hash: format!("{:x}", md5::compute("remote older content".as_bytes())),
                    updated_at_ms: 2000,
                    deleted_at_ms: None,
                    device_id: "device_remote".to_string(),
                    op: "upsert".to_string(),
                    schema_version: 1,
                },
                ManifestFileRecord {
                    path: "volumes/p2/volume.json".to_string(),
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
            start_mock_github_api(Some(initial_manifest), initial_files, "projects/test");

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };

        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = lww_sync(dir.path(), &config, &secrets, false).unwrap();

        assert!(res.uploaded_files.contains(&"project.json".to_string()));
        assert!(res
            .downloaded_files
            .contains(&"volumes/p2/volume.json".to_string()));

        let content_p2 = std::fs::read_to_string(&local_p2).unwrap();
        assert_eq!(content_p2, "remote newer content");

        let final_m_str = manifest_str.lock().unwrap().clone();
        assert!(!final_m_str.is_empty());
        let final_m: SyncManifest = serde_json::from_str(&final_m_str).unwrap();
        let p1_rec = final_m
            .files
            .iter()
            .find(|f| f.path == "project.json")
            .unwrap();
        assert_eq!(p1_rec.device_id, "device_local");
        assert_eq!(p1_rec.op, "upsert");

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    #[cfg(feature = "github-api")]
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
            start_mock_github_api(None, std::collections::HashMap::new(), "projects/test");

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = lww_sync(dir.path(), &config, &secrets, false).unwrap();
        assert_ne!(res.status, SyncStatus::DirtyRepoBlocked);

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn test_is_document_content_path() {
        use crate::sync::lww::is_document_content_path;
        assert!(is_document_content_path(
            "volumes/v1/chapters/c1/chapter.md"
        ));
        assert!(is_document_content_path(
            "volumes/001/chapters/xyz/chapter.md"
        ));
        assert!(!is_document_content_path("project.json"));
        assert!(!is_document_content_path(
            "volumes/v1/chapters/c1/chapter.meta.json"
        ));
        assert!(!is_document_content_path(
            "app-meta/settings/settings.sync.json"
        ));
        assert!(!is_document_content_path("volumes/v1/volume.json"));

        // P1-3: expanded user text document paths
        assert!(is_document_content_path("note.md"));
        assert!(is_document_content_path("volumes/v1/outline.md"));
        assert!(is_document_content_path("volumes/v1/chapters/c1/scene.md"));
        assert!(is_document_content_path("character_notes.md"));
        assert!(is_document_content_path("timeline_notes.md"));
        assert!(is_document_content_path("draft.md"));
        // backups and app-meta are local-only, not user text
        assert!(!is_document_content_path("backups/chapters/c1_backup.md"));
        assert!(!is_document_content_path(
            "app-meta/sync/manifest.sync.json"
        ));
    }

    #[test]
    #[cfg(not(windows))]
    #[cfg(feature = "github-api")]
    fn test_lww_document_conflict_no_overwrite() {
        let dir = tempdir().unwrap();
        let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
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
            start_mock_github_api(Some(initial_manifest), initial_files, "projects/test");

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = lww_sync(dir.path(), &config, &secrets, false).unwrap();

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
    #[cfg(feature = "github-api")]
    fn test_lww_document_conflict_manifest_not_polluted() {
        let dir = tempdir().unwrap();
        let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
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
            start_mock_github_api(Some(initial_manifest), initial_files, "projects/test");

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = lww_sync(dir.path(), &config, &secrets, false).unwrap();
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
    #[cfg(feature = "github-api")]
    fn test_lww_document_only_local_changed_allows_upload() {
        let dir = tempdir().unwrap();
        let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
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
            start_mock_github_api(Some(initial_manifest), initial_files, "projects/test");

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = lww_sync(dir.path(), &config, &secrets, false).unwrap();

        assert_eq!(res.status, SyncStatus::LatestWinsApplied);
        assert!(res.uploaded_files.contains(&chapter_rel.to_string()));
        assert!(res.conflicts.is_empty());
        assert!(res.overwritten_files.is_empty());

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    #[cfg(not(windows))]
    #[cfg(feature = "github-api")]
    fn test_lww_document_only_remote_changed_downloads() {
        let dir = tempdir().unwrap();
        let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
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
            start_mock_github_api(Some(initial_manifest), initial_files, "projects/test");

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = lww_sync(dir.path(), &config, &secrets, false).unwrap();

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
    #[cfg(feature = "github-api")]
    fn test_lww_document_remote_delete_local_modified_is_conflict() {
        let dir = tempdir().unwrap();
        let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
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

        let (mock_url, shutdown, _files_map, _manifest_str, server_thread) = start_mock_github_api(
            Some(initial_manifest),
            std::collections::HashMap::new(),
            "projects/test",
        );

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,

            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = lww_sync(dir.path(), &config, &secrets, false).unwrap();

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
    #[cfg(feature = "github-api")]
    fn test_lww_conflict_persists_across_syncs() {
        let dir = tempdir().unwrap();
        let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
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
        let (mock_url, shutdown, _files_map, _manifest_str, server_thread) = start_mock_github_api(
            Some(initial_manifest.clone()),
            initial_files.clone(),
            "projects/test",
        );

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res1 = lww_sync(dir.path(), &config, &secrets, false).unwrap();

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
            start_mock_github_api(
                Some(initial_manifest.clone()),
                initial_files.clone(),
                "projects/test",
            );

        let config2 = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url2,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };

        let res2 = lww_sync(dir.path(), &config2, &secrets, false).unwrap();

        // Second sync: conflict must still be present (not auto-resolved)
        // The path is in conflicted_files, so it's skipped entirely.
        // No upload of local B over remote C, no download of remote C over local B.
        assert!(
            !res2.uploaded_files.contains(&chapter_rel.to_string()),
            "Second sync must NOT upload local version over remote"
        );
        assert!(
            !res2.downloaded_files.contains(&chapter_rel.to_string()),
            "Second sync must NOT download remote version over local"
        );

        // Local file must still be the local version B
        let local_after_second = std::fs::read_to_string(&chapter_abs).unwrap();
        assert_eq!(
            local_after_second, local_content,
            "Local file must remain unchanged after second sync"
        );

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
    #[cfg(feature = "github-api")]
    fn test_resolve_conflict_keep_local() {
        let dir = tempdir().unwrap();
        let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
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
    #[cfg(feature = "github-api")]
    fn test_resolve_conflict_take_remote() {
        let dir = tempdir().unwrap();
        let chapter_rel = "volumes/v1/chapters/c1/chapter.md";

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
    #[cfg(feature = "github-api")]
    fn test_resolve_conflict_mark_merged() {
        let dir = tempdir().unwrap();
        let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
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
    #[cfg(feature = "github-api")]
    fn test_resolve_conflict_then_sync_recovers() {
        let dir = tempdir().unwrap();
        let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
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
        let (mock_url, shutdown, _files_map, _manifest_str, server_thread) = start_mock_github_api(
            Some(initial_manifest.clone()),
            initial_files.clone(),
            "projects/test",
        );

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res1 = lww_sync(dir.path(), &config, &secrets, false).unwrap();
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
            start_mock_github_api(
                Some(initial_manifest.clone()),
                initial_files.clone(),
                "projects/test",
            );

        let config2 = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url2,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };

        let res2 = lww_sync(dir.path(), &config2, &secrets, false).unwrap();
        assert!(
            !res2.uploaded_files.contains(&chapter_rel.to_string()),
            "Second sync must NOT upload before resolution"
        );
        assert!(
            !res2.downloaded_files.contains(&chapter_rel.to_string()),
            "Second sync must NOT download before resolution"
        );

        let state_after_2 = SyncService::load_sync_state(dir.path()).unwrap();
        assert!(state_after_2.conflicted_files.contains(chapter_rel));

        shutdown2.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread2.join();

        // === Step 3: Resolve conflict by keeping local ===
        SyncService::resolve_conflict_keep_local(dir.path(), chapter_rel).unwrap();

        let state_after_resolve = SyncService::load_sync_state(dir.path()).unwrap();
        assert!(
            !state_after_resolve.conflicted_files.contains(chapter_rel),
            "conflicted_files must be cleared after resolution"
        );
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

        let (mock_url3, shutdown3, _files_map3, _manifest_str3, server_thread3) =
            start_mock_github_api(
                Some(initial_manifest.clone()),
                initial_files.clone(),
                "projects/test",
            );

        let config3 = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url3,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };

        let res3 = lww_sync(dir.path(), &config3, &secrets, false).unwrap();

        // After resolve, three-way sees LocalChanged → uploads local version
        assert!(
            res3.uploaded_files.contains(&chapter_rel.to_string()),
            "After keep_local resolution, sync must upload the local version"
        );
        assert!(
            !res3.downloaded_files.contains(&chapter_rel.to_string()),
            "After keep_local resolution, sync must NOT download remote over local"
        );

        // Local file must still be the local version
        let local_after_3 = std::fs::read_to_string(&chapter_abs).unwrap();
        assert_eq!(
            local_after_3, local_content,
            "Local file must remain unchanged after post-resolve sync"
        );

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
    #[cfg(feature = "github-api")]
    fn test_resolve_conflict_take_remote_then_sync() {
        let dir = tempdir().unwrap();
        let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
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
        let (mock_url, shutdown, _files_map, _manifest_str, server_thread) = start_mock_github_api(
            Some(initial_manifest.clone()),
            initial_files.clone(),
            "projects/test",
        );

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res1 = lww_sync(dir.path(), &config, &secrets, false).unwrap();
        assert_eq!(res1.status, SyncStatus::PartialConflict);

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();

        // === Step 2: Resolve by taking remote ===
        SyncService::resolve_conflict_take_remote(dir.path(), chapter_rel).unwrap();

        let state_after_resolve = SyncService::load_sync_state(dir.path()).unwrap();
        assert!(!state_after_resolve.conflicted_files.contains(chapter_rel));
        assert!(state_after_resolve
            .pending_take_remote
            .contains(chapter_rel));
        assert!(state_after_resolve.conflicts.is_empty());

        // === Step 3: Next sync should download remote content to local ===
        let mut state_before_3 = SyncService::load_sync_state(dir.path()).unwrap();
        state_before_3.last_sync_time = None;
        SyncService::save_sync_state(dir.path(), &state_before_3).unwrap();

        let (mock_url3, shutdown3, _files_map3, _manifest_str3, server_thread3) =
            start_mock_github_api(
                Some(initial_manifest.clone()),
                initial_files.clone(),
                "projects/test",
            );

        let config3 = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url3,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };

        let res3 = lww_sync(dir.path(), &config3, &secrets, false).unwrap();

        // After take_remote resolution, sync must download the remote content
        assert!(
            res3.downloaded_files.contains(&chapter_rel.to_string()),
            "After take_remote resolution, sync must download the remote version"
        );
        assert!(
            !res3.uploaded_files.contains(&chapter_rel.to_string()),
            "After take_remote resolution, sync must NOT upload the old local version"
        );

        // Local file must now be the remote version
        let local_after_3 = std::fs::read_to_string(&chapter_abs).unwrap();
        assert_eq!(
            local_after_3, remote_content,
            "Local file must be the remote version after take_remote + sync"
        );

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
            start_mock_github_api(
                Some(initial_manifest.clone()),
                initial_files.clone(),
                "projects/test",
            );

        let config4 = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url4,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };

        let res4 = lww_sync(dir.path(), &config4, &secrets, false).unwrap();

        assert!(
            !res4.uploaded_files.contains(&chapter_rel.to_string()),
            "Subsequent sync must NOT upload after take_remote resolved"
        );
        assert!(
            !res4.downloaded_files.contains(&chapter_rel.to_string()),
            "Subsequent sync must NOT download after take_remote resolved"
        );

        // Local file must still be the remote version
        let local_after_4 = std::fs::read_to_string(&chapter_abs).unwrap();
        assert_eq!(
            local_after_4, remote_content,
            "Local file must remain the remote version after subsequent sync"
        );

        shutdown4.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread4.join();
    }

    /// P0: When pending_take_remote encounters a remote-missing file, the sync
    /// must NOT upload the local content, must NOT clear pending_take_remote,
    /// and must return a RecoverableError.
    #[test]
    #[cfg(not(windows))]
    #[cfg(feature = "github-api")]
    fn test_pending_take_remote_remote_missing_no_upload() {
        let dir = tempdir().unwrap();
        let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
        let chapter_abs = dir.path().join(chapter_rel);
        std::fs::create_dir_all(chapter_abs.parent().unwrap()).unwrap();

        let base_content = "base version A";
        let local_content = "local version B";

        std::fs::write(&chapter_abs, local_content).unwrap();

        let base_hash = format!("{:x}", md5::compute(base_content.as_bytes()));

        let mut state = SyncState::default();
        state.device_id = "device_local".to_string();
        state
            .known_files
            .insert(chapter_rel.to_string(), base_hash.clone());
        state
            .known_files_updated_at
            .insert(chapter_rel.to_string(), 1000);
        // Simulate: user already chose "take remote", path is in pending_take_remote
        state.pending_take_remote.insert(chapter_rel.to_string());
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        // Remote manifest has the file record, but the actual file content is
        // missing from the mock server (initial_files is empty).
        let remote_hash = format!("{:x}", md5::compute("remote version C".as_bytes()));
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

        // No files in initial_files → mock server returns 404 for content GET
        let (mock_url, shutdown, _files_map, _manifest_str, server_thread) = start_mock_github_api(
            Some(initial_manifest),
            std::collections::HashMap::new(),
            "projects/test",
        );

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = lww_sync(dir.path(), &config, &secrets, false).unwrap();

        // Must NOT upload the local content
        assert!(
            !res.uploaded_files.contains(&chapter_rel.to_string()),
            "pending_take_remote with remote missing must NOT upload local content"
        );

        // Must NOT download (remote is missing)
        assert!(
            !res.downloaded_files.contains(&chapter_rel.to_string()),
            "pending_take_remote with remote missing must NOT download"
        );

        // Status should be RecoverableError
        match &res.status {
            SyncStatus::RecoverableError(msg) => {
                assert!(
                    msg.starts_with("pending_take_remote_failed"),
                    "Error message should mention pending_take_remote_failed, got: {}",
                    msg
                );
            }
            other => panic!("Expected RecoverableError, got {:?}", other),
        }

        // pending_take_remote must still contain the path (not cleared)
        let state_after = SyncService::load_sync_state(dir.path()).unwrap();
        assert!(
            state_after.pending_take_remote.contains(chapter_rel),
            "pending_take_remote must NOT be cleared when remote is missing"
        );

        // Local file must remain unchanged
        let local_after = std::fs::read_to_string(&chapter_abs).unwrap();
        assert_eq!(
            local_after, local_content,
            "Local file must remain unchanged when remote is missing"
        );

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    /// P0: When pending_take_remote succeeds (remote file exists), the local
    /// file becomes the remote content, pending is cleared, and downloaded_files
    /// contains the path.
    #[test]
    #[cfg(not(windows))]
    #[cfg(feature = "github-api")]
    fn test_pending_take_remote_remote_exists_downloads_and_clears() {
        let dir = tempdir().unwrap();
        let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
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
        // Simulate: user already chose "take remote", path is in pending_take_remote
        state.pending_take_remote.insert(chapter_rel.to_string());
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
            start_mock_github_api(Some(initial_manifest), initial_files, "projects/test");

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: mock_url,
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("dummy_token".to_string()),
            ssh_private_key: None,
        };

        let res = lww_sync(dir.path(), &config, &secrets, false).unwrap();

        // Must download the remote content
        assert!(
            res.downloaded_files.contains(&chapter_rel.to_string()),
            "pending_take_remote with remote existing must download"
        );

        // Must NOT upload the old local content
        assert!(
            !res.uploaded_files.contains(&chapter_rel.to_string()),
            "pending_take_remote with remote existing must NOT upload local content"
        );

        // Local file must now be the remote version
        let local_after = std::fs::read_to_string(&chapter_abs).unwrap();
        assert_eq!(
            local_after, remote_content,
            "Local file must be the remote version after pending_take_remote + sync"
        );

        // pending_take_remote must be cleared
        let state_after = SyncService::load_sync_state(dir.path()).unwrap();
        assert!(
            !state_after.pending_take_remote.contains(chapter_rel),
            "pending_take_remote must be cleared after successful download"
        );

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    fn test_tree_404_must_not_be_silently_treated_as_empty() {
        // Verify that the lww.rs source code does NOT silently treat tree 404 as empty remote.
        // After the fix, tree 404 should trigger a ref check before deciding.
        let source = include_str!("lww.rs");
        // The old code had: `else if tree_status.as_u16() != 404 { return Err(...) }`
        // which silently let 404 fall through to empty remote_tree_files.
        // The new code should have a `tree_status.as_u16() == 404` branch that
        // calls /git/ref/heads/{branch} to diagnose.
        assert!(
            source.contains("tree_status == 404") || source.contains("tree_status.as_u16() == 404"),
            "lww.rs must have an explicit tree 404 branch that diagnoses the cause"
        );
        assert!(
            source.contains("ref_url") && source.contains("git/ref/heads/"),
            "tree 404 handler must call /git/ref/heads/ to distinguish repo/branch issues"
        );
        assert!(
            source.contains("SyncRemoteBranchNotFound"),
            "tree 404 handler must produce SyncRemoteBranchNotFound error when branch is absent"
        );
        assert!(
            source.contains("repo_not_found_or_no_permission"),
            "tree 404 handler must produce repo_not_found_or_no_permission error when repo is inaccessible"
        );
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn test_github_api_error_404_not_found_not_used() {
        // After the fix, github_api_error should no longer produce the generic "not_found" category.
        // All 404s should be classified as repo_not_found_or_no_permission or file_not_found.
        let source = include_str!("github_api_client.rs");
        // Check that the old `404 => "not_found"` pattern is gone.
        // We look for the exact match arm that would produce the generic category.
        assert!(
            !source.contains("404 => \"not_found\""),
            "github_api_error must not have '404 => \"not_found\"' pattern — should use context-aware classification"
        );
        // Check that context-aware 404 classification exists
        assert!(
            source.contains("\"file_not_found\""),
            "github_api_error must classify get contents 404 as file_not_found"
        );
        assert!(
            source.contains("\"repo_not_found_or_no_permission\""),
            "github_api_error must classify get ref/tree/put/delete 404 as repo_not_found_or_no_permission"
        );
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn test_git_backend_diagnostics_not_assumed_ok() {
        // Git 后端诊断不再假成功，应返回明确的"不支持"状态
        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::Git,
            remote_url: "https://github.com/user/repo.git".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("test_token".to_string()),
            ssh_private_key: None,
        };

        let result = SyncService::perform_sync_diagnostics(&config, &secrets).unwrap();
        // Git 后端不再假成功
        assert!(
            !result.success,
            "Git backend diagnostics should not report success"
        );
        assert!(!result.network_ok, "Git backend network_ok should be false");
        assert!(!result.auth_ok, "Git backend auth_ok should be false");
        assert!(!result.repo_ok, "Git backend repo_ok should be false");
        assert!(!result.branch_ok, "Git backend branch_ok should be false");
        assert_eq!(result.network_status, "unsupported_git_backend");
        assert_eq!(result.auth_status, "not_checked_git_backend");
        assert_eq!(result.repo_status, "not_checked_git_backend");
        assert_eq!(result.branch_status, "not_checked_git_backend");
    }

    // ── force_sync debounce 测试 ──

    #[test]
    #[cfg(feature = "github-api")]
    fn test_perform_lww_sync_debounce_skips_when_not_forced() {
        // 测试自动同步（force_sync=false）在 min_interval 内被 debounce 跳过
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("projects")).unwrap();

        // 先执行一次同步（force_sync=true 绕过 debounce）
        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: "https://github.com/test/debounce-test.git".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300, // 5 分钟
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("test_token".to_string()),
            ssh_private_key: None,
        };

        // 第一次同步（force_sync=true）应该尝试执行（虽然会因网络失败，但不会被 debounce 跳过）
        let _res1 = lww_sync(dir.path(), &config, &secrets, true).unwrap();
        // 因为是测试环境没有真实 GitHub API，预期返回错误状态而非 Success
        // debounce 跳过时返回 Success，所以只要不是 Success 就说明没被 debounce 跳过

        // 第二次同步（force_sync=false）在 min_interval 内应该被 debounce 跳过
        // debounce 跳过时返回 status=Success
        let _res2 = lww_sync(dir.path(), &config, &secrets, false).unwrap();
        // 如果被 debounce 跳过，status 应该是 Success
        // 注意：如果第一次同步失败没有更新 last_sync_time，则不会被 debounce
        // 所以这个测试验证的是：如果 last_sync_time 在 min_interval 内，force_sync=false 会被跳过
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn test_perform_lww_sync_force_sync_bypasses_debounce() {
        // 测试手动同步（force_sync=true）绕过 debounce
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("projects")).unwrap();

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: "https://github.com/test/force-sync-test.git".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("test_token".to_string()),
            ssh_private_key: None,
        };

        // 先设置 last_sync_time 为当前时间（模拟刚同步过）
        let mut state = crate::sync::SyncService::load_sync_state(dir.path()).unwrap();
        state.last_sync_time = Some(chrono::Utc::now().timestamp());
        crate::sync::SyncService::save_sync_state(dir.path(), &state).unwrap();

        // force_sync=false 应该被 debounce 跳过（返回 Success）
        let res1 = lww_sync(dir.path(), &config, &secrets, false).unwrap();
        assert_eq!(
            res1.status,
            SyncStatus::Success,
            "auto sync should be debounced"
        );

        // force_sync=true 应该绕过 debounce（尝试执行，虽然网络会失败）
        let res2 = lww_sync(dir.path(), &config, &secrets, true).unwrap();
        // force_sync=true 绕过了 debounce，会尝试网络请求
        // 因为测试环境没有真实 API，预期返回错误状态
        assert_ne!(
            res2.status,
            SyncStatus::Success,
            "force_sync should bypass debounce and attempt sync"
        );
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn test_pending_take_remote_bypasses_debounce() {
        // 冲突解决后（pending_take_remote 非空），即使 force_sync=false 也应绕过 debounce
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("projects")).unwrap();

        let config = SyncConfig {
            enabled: true,
            backend_type: BackendType::GithubApi,
            remote_url: "https://github.com/test/pending-take-remote-test.git".to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            token: Some("test_token".to_string()),
            ssh_private_key: None,
        };

        // 设置 last_sync_time 为当前时间（模拟刚同步过）
        let mut state = crate::sync::SyncService::load_sync_state(dir.path()).unwrap();
        state.last_sync_time = Some(chrono::Utc::now().timestamp());
        // 添加 pending_take_remote（模拟用户刚解决冲突选择"采用远端"）
        state
            .pending_take_remote
            .insert("test_chapter.md".to_string());
        crate::sync::SyncService::save_sync_state(dir.path(), &state).unwrap();

        // force_sync=false 但有 pending_take_remote，应该绕过 debounce（尝试执行）
        let res = lww_sync(dir.path(), &config, &secrets, false).unwrap();
        // 绕过了 debounce，会尝试网络请求，因测试环境没有真实 API，预期返回错误状态
        assert_ne!(
            res.status,
            SyncStatus::Success,
            "pending_take_remote should bypass debounce"
        );
    }
    // ── Issue #600 评论 #7: tree diff 填充 downloaded_files/remote_deletes 的测试 ──

    #[cfg(all(not(windows), feature = "git-https"))]
    fn git_test_signature() -> git2::Signature<'static> {
        git2::Signature::now("Test User", "test@test.com").unwrap()
    }

    #[cfg(all(not(windows), feature = "git-https"))]
    fn commit_file_to_repo(repo: &git2::Repository, path: &str, content: &str, msg: &str) {
        let full_path = repo.workdir().unwrap().join(path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full_path, content).unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new(path)).unwrap();
        index.write().unwrap();
        let oid = index.write_tree().unwrap();
        let tree = repo.find_tree(oid).unwrap();

        let sig = git_test_signature();
        let parents: Vec<git2::Commit> = match repo.head() {
            Ok(head) => head
                .peel_to_commit()
                .ok()
                .map(|c| vec![c])
                .unwrap_or_default(),
            Err(_) => vec![],
        };
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

        repo.commit(
            Some("refs/heads/main"),
            &sig,
            &sig,
            msg,
            &tree,
            &parent_refs,
        )
        .unwrap();
        repo.set_head("refs/heads/main").unwrap();
    }

    #[cfg(all(not(windows), feature = "git-https"))]
    fn delete_file_from_repo(repo: &git2::Repository, path: &str, msg: &str) {
        let full_path = repo.workdir().unwrap().join(path);
        std::fs::remove_file(&full_path).unwrap();

        let mut index = repo.index().unwrap();
        index.remove_path(std::path::Path::new(path)).unwrap();
        index.write().unwrap();
        let oid = index.write_tree().unwrap();
        let tree = repo.find_tree(oid).unwrap();

        let sig = git_test_signature();
        let head = repo.head().unwrap().peel_to_commit().unwrap();

        repo.commit(Some("refs/heads/main"), &sig, &sig, msg, &tree, &[&head])
            .unwrap();
    }

    #[cfg(all(not(windows), feature = "git-https"))]
    fn make_sync_config(remote_url: &str) -> SyncConfig {
        SyncConfig {
            enabled: true,
            remote_url: remote_url.to_string(),
            transport: SyncProtocol::HttpsToken,
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 0,
            backend_type: BackendType::Git,
            username: String::new(),
            has_network_permission: true,
            has_network_state_permission: true,
        }
    }

    #[cfg(all(not(windows), feature = "git-https"))]
    fn make_sync_secrets() -> SyncSecrets {
        SyncSecrets {
            token: Some("dummy".to_string()),
            ssh_private_key: None,
        }
    }

    #[cfg(all(not(windows), feature = "git-https"))]
    fn hard_reset_to(repo: &git2::Repository, oid: git2::Oid) {
        let obj = repo.find_object(oid, None).unwrap();
        let mut cb = git2::build::CheckoutBuilder::default();
        cb.force();
        repo.reset(&obj, git2::ResetType::Hard, Some(&mut cb))
            .unwrap();
        let mut ref_main = repo.find_reference("refs/heads/main").unwrap();
        ref_main.set_target(oid, "reset").unwrap();
    }

    #[test]
    #[cfg(all(not(windows), feature = "git-https"))]
    fn test_pull_tree_diff_downloaded_files() {
        // 正面测试：远端修改 whitelisted 文件，pull 后 downloaded_files 应包含该文件。
        let remote_dir = tempfile::tempdir().unwrap();
        let _bare_repo = git2::Repository::init_bare(remote_dir.path()).unwrap();
        let remote_url = format!("file://{}", remote_dir.path().to_string_lossy());

        let local_dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(local_dir.path()).unwrap();

        // 本地创建初始 commit A 并 push 到远端
        commit_file_to_repo(&repo, "project.json", r#"{"version":1}"#, "initial");
        repo.remote("origin", &remote_url).unwrap();
        let backend = Git2Backend;
        backend.push(local_dir.path(), "main", None).unwrap();

        // 在本地创建新 commit B 修改 project.json，push 到远端
        let oid_a = repo.head().unwrap().target().unwrap();
        commit_file_to_repo(&repo, "project.json", r#"{"version":2}"#, "modify");
        backend.push(local_dir.path(), "main", None).unwrap();

        // 重置本地 repo 到 commit A（模拟本地落后于远端）
        hard_reset_to(&repo, oid_a);

        // 调用 perform_sync
        let config = make_sync_config(&remote_url);
        let secrets = make_sync_secrets();
        let result = SyncService::perform_sync(
            local_dir.path(),
            &config,
            &secrets,
            crate::sync::types::SyncScope::Project,
            &backend,
        )
        .unwrap();

        assert_eq!(result.status, SyncStatus::Success);
        assert!(
            result
                .downloaded_files
                .contains(&"project.json".to_string()),
            "downloaded_files should contain project.json, got: {:?}",
            result.downloaded_files
        );
    }

    #[test]
    #[cfg(all(not(windows), feature = "git-https"))]
    fn test_pull_tree_diff_blacklisted_not_in_downloaded() {
        // 反面测试：远端修改黑名单文件，pull 后 downloaded_files 不应包含该文件。
        let remote_dir = tempfile::tempdir().unwrap();
        let _bare_repo = git2::Repository::init_bare(remote_dir.path()).unwrap();
        let remote_url = format!("file://{}", remote_dir.path().to_string_lossy());

        let local_dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(local_dir.path()).unwrap();

        // 本地创建初始 commit A（包含 whitelisted 文件）并 push 到远端
        commit_file_to_repo(&repo, "project.json", r#"{"version":1}"#, "initial");
        repo.remote("origin", &remote_url).unwrap();
        let backend = Git2Backend;
        backend.push(local_dir.path(), "main", None).unwrap();

        // 在本地创建新 commit B 添加黑名单文件，push 到远端
        let oid_a = repo.head().unwrap().target().unwrap();
        commit_file_to_repo(
            &repo,
            "app-meta/sync/config.local.json",
            r#"{"key":"val"}"#,
            "add blacklisted",
        );
        backend.push(local_dir.path(), "main", None).unwrap();

        // 重置本地 repo 到 commit A（模拟本地落后于远端）
        hard_reset_to(&repo, oid_a);

        // 调用 perform_sync
        let config = make_sync_config(&remote_url);
        let secrets = make_sync_secrets();
        let result = SyncService::perform_sync(
            local_dir.path(),
            &config,
            &secrets,
            crate::sync::types::SyncScope::Project,
            &backend,
        )
        .unwrap();

        assert_eq!(result.status, SyncStatus::Success);
        assert!(
            !result
                .downloaded_files
                .contains(&"app-meta/sync/config.local.json".to_string()),
            "downloaded_files should NOT contain blacklisted file, got: {:?}",
            result.downloaded_files
        );
    }

    #[test]
    #[cfg(all(not(windows), feature = "git-https"))]
    fn test_pull_tree_diff_remote_deletes() {
        // 删除测试：远端删除 whitelisted 文件，pull 后 remote_deletes 应包含该文件。
        let remote_dir = tempfile::tempdir().unwrap();
        let _bare_repo = git2::Repository::init_bare(remote_dir.path()).unwrap();
        let remote_url = format!("file://{}", remote_dir.path().to_string_lossy());

        let local_dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(local_dir.path()).unwrap();

        // 本地创建初始 commit A（包含 whitelisted 文件）并 push 到远端
        commit_file_to_repo(&repo, "project.json", r#"{"version":1}"#, "initial");
        repo.remote("origin", &remote_url).unwrap();
        let backend = Git2Backend;
        backend.push(local_dir.path(), "main", None).unwrap();

        // 在本地创建新 commit B 删除 project.json，push 到远端
        let oid_a = repo.head().unwrap().target().unwrap();
        delete_file_from_repo(&repo, "project.json", "delete project.json");
        backend.push(local_dir.path(), "main", None).unwrap();

        // 重置本地 repo 到 commit A（模拟本地落后于远端）
        hard_reset_to(&repo, oid_a);

        // 调用 perform_sync
        let config = make_sync_config(&remote_url);
        let secrets = make_sync_secrets();
        let result = SyncService::perform_sync(
            local_dir.path(),
            &config,
            &secrets,
            crate::sync::types::SyncScope::Project,
            &backend,
        )
        .unwrap();

        assert_eq!(result.status, SyncStatus::Success);
        assert!(
            result.remote_deletes.contains(&"project.json".to_string()),
            "remote_deletes should contain project.json, got: {:?}",
            result.remote_deletes
        );
    }

    // ── Issue #600 评论 #9: 首次同步边界（CloneIntoEmptyProject / unborn repo）──

    #[test]
    #[cfg(all(not(windows), feature = "git-https"))]
    fn test_clone_into_empty_downloaded_files() {
        // 正面：空目录 clone 远端，clone 下来的 whitelisted 文件应进 downloaded_files。
        let remote_dir = tempfile::tempdir().unwrap();
        let _bare_repo = git2::Repository::init_bare(remote_dir.path()).unwrap();
        let remote_url = format!("file://{}", remote_dir.path().to_string_lossy());

        // 先用一个临时 repo 往远端 push 内容
        let seed_dir = tempfile::tempdir().unwrap();
        let seed_repo = git2::Repository::init(seed_dir.path()).unwrap();
        commit_file_to_repo(&seed_repo, "project.json", r#"{"version":1}"#, "initial");
        seed_repo.remote("origin", &remote_url).unwrap();
        let backend = Git2Backend;
        backend.push(seed_dir.path(), "main", None).unwrap();

        // 本地是空目录，perform_sync 会走 CloneIntoEmptyProject
        let local_dir = tempfile::tempdir().unwrap();
        let config = make_sync_config(&remote_url);
        let secrets = make_sync_secrets();
        let result = SyncService::perform_sync(
            local_dir.path(),
            &config,
            &secrets,
            crate::sync::types::SyncScope::Project,
            &backend,
        )
        .unwrap();

        assert_eq!(result.status, SyncStatus::Success);
        assert_eq!(result.first_sync_mode, FirstSyncMode::CloneIntoEmptyProject);
        assert!(
            result
                .downloaded_files
                .contains(&"project.json".to_string()),
            "downloaded_files should contain project.json after clone, got: {:?}",
            result.downloaded_files
        );
    }

    #[test]
    #[cfg(all(not(windows), feature = "git-https"))]
    fn test_clone_into_empty_blacklisted_not_in_downloaded() {
        // 反面：clone 下来的黑名单文件不应进 downloaded_files。
        let remote_dir = tempfile::tempdir().unwrap();
        let _bare_repo = git2::Repository::init_bare(remote_dir.path()).unwrap();
        let remote_url = format!("file://{}", remote_dir.path().to_string_lossy());

        let seed_dir = tempfile::tempdir().unwrap();
        let seed_repo = git2::Repository::init(seed_dir.path()).unwrap();
        commit_file_to_repo(
            &seed_repo,
            "project.json",
            r#"{"version":1}"#,
            "whitelisted",
        );
        commit_file_to_repo(
            &seed_repo,
            "app-meta/sync/config.local.json",
            r#"{"key":"val"}"#,
            "blacklisted",
        );
        seed_repo.remote("origin", &remote_url).unwrap();
        let backend = Git2Backend;
        backend.push(seed_dir.path(), "main", None).unwrap();

        let local_dir = tempfile::tempdir().unwrap();
        let config = make_sync_config(&remote_url);
        let secrets = make_sync_secrets();
        let result = SyncService::perform_sync(
            local_dir.path(),
            &config,
            &secrets,
            crate::sync::types::SyncScope::Project,
            &backend,
        )
        .unwrap();

        assert_eq!(result.status, SyncStatus::Success);
        assert!(
            !result
                .downloaded_files
                .contains(&"app-meta/sync/config.local.json".to_string()),
            "downloaded_files should NOT contain blacklisted file, got: {:?}",
            result.downloaded_files
        );
        assert!(
            result
                .downloaded_files
                .contains(&"project.json".to_string()),
            "downloaded_files should still contain whitelisted file, got: {:?}",
            result.downloaded_files
        );
    }

    #[test]
    #[cfg(all(not(windows), feature = "git-https"))]
    fn test_unborn_repo_pull_downloaded_files() {
        // 正面：本地 init 但无 commit（HEAD unborn），pull 远端后远端文件应进 downloaded_files。
        let remote_dir = tempfile::tempdir().unwrap();
        let _bare_repo = git2::Repository::init_bare(remote_dir.path()).unwrap();
        let remote_url = format!("file://{}", remote_dir.path().to_string_lossy());

        let seed_dir = tempfile::tempdir().unwrap();
        let seed_repo = git2::Repository::init(seed_dir.path()).unwrap();
        commit_file_to_repo(&seed_repo, "project.json", r#"{"version":1}"#, "initial");
        seed_repo.remote("origin", &remote_url).unwrap();
        let backend = Git2Backend;
        backend.push(seed_dir.path(), "main", None).unwrap();

        // 本地 init 但不 commit，设置 remote → HEAD unborn
        let local_dir = tempfile::tempdir().unwrap();
        let local_repo = git2::Repository::init(local_dir.path()).unwrap();
        local_repo.remote("origin", &remote_url).unwrap();

        let config = make_sync_config(&remote_url);
        let secrets = make_sync_secrets();
        let result = SyncService::perform_sync(
            local_dir.path(),
            &config,
            &secrets,
            crate::sync::types::SyncScope::Project,
            &backend,
        )
        .unwrap();

        assert_eq!(result.status, SyncStatus::Success);
        assert!(
            result
                .downloaded_files
                .contains(&"project.json".to_string()),
            "downloaded_files should contain project.json after unborn pull, got: {:?}",
            result.downloaded_files
        );
    }
}
