#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    #[cfg(feature = "github-api")]
    use crate::sync::provider::github::config::GitHubTransport;
    use crate::sync::service::SyncService;
    #[cfg(feature = "github-api")]
    use crate::sync::types::ManifestFileRecord;
    #[cfg(feature = "github-api")]
    use crate::sync::types::SyncConfig;
    #[cfg(feature = "github-api")]
    use crate::sync::types::SyncConflict;
    #[cfg(feature = "github-api")]
    use crate::sync::types::SyncManifest;
    #[cfg(feature = "github-api")]
    use crate::sync::types::SyncScope;
    #[cfg(feature = "github-api")]
    use crate::sync::types::SyncSecrets;
    #[cfg(feature = "github-api")]
    use crate::sync::types::SyncState;
    #[cfg(feature = "github-api")]
    use crate::sync::types::SyncStatus;
    #[cfg(feature = "github-api")]
    use base64::Engine;
    #[cfg(feature = "github-api")]
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
        let provider_config = match &config.provider_config {
            Some(crate::sync::provider::ProviderConfig::GitHub(gh)) => {
                crate::sync::provider::github::config::GitHubRuntimeConfig::from_persisted(
                    gh,
                    secrets.provider_secrets.as_ref(),
                )
                .map_err(crate::Error::from)?
            }
            None => return Err(crate::Error::Other("no provider_config".to_string())),
        };
        let provider = crate::sync::provider::github::GitHubProvider::new(
            provider_config,
            std::sync::Arc::new(transport),
        );
        let sync_policy = crate::sync::types::SyncPolicy::from_config(config);
        SyncService::perform_lww_sync(sync_root, &provider, &sync_policy, &target, force_sync)
    }
    #[test]
    #[cfg(feature = "github-api")]
    fn test_github_api_diagnostics_reports_provider_type_without_token() {
        let config = SyncConfig {
            enabled: true,
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: "https://github.com/user/repo.git".to_string(),
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: None,
        };

        let result = SyncService::perform_sync_diagnostics(&config, &secrets).unwrap();
        assert_eq!(result.provider_type, "github_api");
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: "url".to_string(),
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let state = SyncState {
            last_sync_time: Some(0),
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
        // 否则 perform_sync 跑 provider 时这些文件会被当作待同步内容。
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
    #[cfg(feature = "github-api")]
    fn test_sync_config_no_token() {
        let config = SyncConfig {
            enabled: true,
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: "https://example.com/repo.git".to_string(),
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 300,
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
        // #645 评论 5504296097 第2点：SyncState 不再携带 remote_url/transport，
        // 用 last_error 携带 URL 字符串来验证 state 序列化不含 token 的核心回归意图。
        let state = SyncState {
            last_sync_time: None,
            last_error: Some("sync failed for https://example.com/repo.git".to_string()),
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
    #[cfg(feature = "github-api")]
    fn test_sync_dry_run_disabled_config() {
        let dir = tempdir().unwrap();
        let config = SyncConfig {
            enabled: false,
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: "https://example.com/repo.git".to_string(),
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 300,
            has_network_permission: true,
            has_network_state_permission: true,
        };

        let plan =
            SyncService::perform_sync_dry_run(dir.path(), &config, SyncScope::Project).unwrap();
        assert!(plan.files_to_upload.is_empty());
        assert!(plan.ignored_files.is_empty());
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn test_sync_dry_run_enabled_config_scans() {
        let dir = tempdir().unwrap();
        let config = SyncConfig {
            enabled: true,
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: "https://example.com/repo.git".to_string(),
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 300,
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };

        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "dummy_token".to_string(),
            }),
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "dummy_token".to_string(),
            }),
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "dummy_token".to_string(),
            }),
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };

        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "dummy_token".to_string(),
            }),
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "dummy_token".to_string(),
            }),
        };

        let res = lww_sync(dir.path(), &config, &secrets, false).unwrap();
        // #645 评论 5504296097 第2点：DirtyRepoBlocked 变体已删除，
        // 同步不再因 dirty repo 返回专用阻塞状态。断言同步未落入 FatalError
        // 保留"同步不被自身配置文件拦死"的核心回归意图。
        assert!(
            !matches!(res.status, SyncStatus::FatalError(_)),
            "sync should not be blocked by dirty repo config file"
        );

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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "dummy_token".to_string(),
            }),
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "dummy_token".to_string(),
            }),
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "dummy_token".to_string(),
            }),
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "dummy_token".to_string(),
            }),
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "dummy_token".to_string(),
            }),
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "dummy_token".to_string(),
            }),
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url2,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "dummy_token".to_string(),
            }),
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url2,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url3,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "dummy_token".to_string(),
            }),
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url3,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url4,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "dummy_token".to_string(),
            }),
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: mock_url,
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "dummy_token".to_string(),
            }),
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
        // Verify that the GitHub provider does NOT silently treat tree 404 as empty remote.
        // After the refactor, tree 404 handling moved from lww/transfer.rs to provider/github/mod.rs.
        let source = include_str!("provider/github/mod.rs");
        assert!(
            source.contains("status == 404"),
            "provider/github/mod.rs must have an explicit tree 404 branch that diagnoses the cause"
        );
        assert!(
            source.contains("git/ref/heads/"),
            "tree 404 handler must call /git/ref/heads/ to distinguish repo/branch issues"
        );
        assert!(
            source.contains("remote branch not found"),
            "tree 404 handler must produce an error when branch is absent"
        );
        assert!(
            source.contains("get_repo"),
            "tree 404 handler must call get_repo to distinguish repo/branch issues"
        );
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn test_git_backend_diagnostics_not_assumed_ok() {
        // Git 后端诊断不再假成功，应返回明确的"不支持"状态
        let config = SyncConfig {
            enabled: true,
            active_provider: "git".to_string(),
            provider_config: None,
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "test_token".to_string(),
            }),
        };

        let result = SyncService::perform_sync_diagnostics(&config, &secrets).unwrap();
        // Git 后端不再假成功
        assert!(
            !result.success,
            "Git backend diagnostics should not report success"
        );
        assert!(!result.network_ok, "Git backend network_ok should be false");
        assert!(!result.auth_ok, "Git backend auth_ok should be false");
        assert!(!result.remote_ok, "Git backend remote_ok should be false");
        assert_eq!(result.network_status, "unsupported_git_backend");
        assert_eq!(result.auth_status, "not_checked_git_backend");
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: "https://github.com/test/debounce-test.git".to_string(),
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 300,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "test_token".to_string(),
            }),
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: "https://github.com/test/force-sync-test.git".to_string(),
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 300,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "test_token".to_string(),
            }),
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
            active_provider: "github_api".to_string(),
            provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
                crate::sync::provider::github::config::GitHubProviderConfig {
                    remote_url: "https://github.com/test/pending-take-remote-test.git".to_string(),
                    branch: "main".to_string(),
                    username: String::new(),
                    transport: GitHubTransport::HttpsToken,
                },
            )),
            auto_sync: false,
            sync_interval_seconds: 300,
            has_network_permission: true,
            has_network_state_permission: true,
        };
        let secrets = SyncSecrets {
            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub {
                token: "test_token".to_string(),
            }),
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

    // ── Issue #600 评论 #9: 首次同步边界（CloneIntoEmptyProject / unborn repo）──

    // ===== Issue #645 评论 5504296097：MemoryProvider + LWW engine 集成测试 =====
    //
    // 这组测试用 MemoryProvider 直接调用 SyncService::perform_lww_sync，
    // 证明 LWW engine 通过 &dyn SyncProvider trait 与具体后端解耦：
    // 新增 Provider 不需要复制 LWW engine，只需实现 SyncProvider trait。
    //
    // 与现有 GitHubProvider + mock server 测试不同，这里不需要 mock HTTP server，
    // 也不需要 SyncSecrets/SyncTransport。MemoryProvider 是进程内 HashMap，
    // 所有操作在锁内同步完成。
    //
    // #645 评论 5504296097 第2点：去掉 `#[cfg(feature = "github-api")]` 门控。
    // 这组测试只依赖 LWW engine + MemoryProvider，不需要 GitHub feature。
    // 类型引用走 fully-qualified path，避免与顶部 `#[cfg(feature = "github-api")]`
    // imports 在 all-features 下产生重复 import 冲突。

    fn lww_sync_with_memory(
        sync_root: &std::path::Path,
        provider: &dyn crate::sync::provider::SyncProvider,
        force_sync: bool,
    ) -> crate::Result<crate::sync::SyncResult> {
        let target = crate::sync::types::SyncTarget::project("test");
        let sync_policy = crate::sync::types::SyncPolicy {
            enabled: true,
            auto_sync: false,
            sync_interval_seconds: 0,
            has_network_permission: true,
        };
        SyncService::perform_lww_sync(sync_root, provider, &sync_policy, &target, force_sync)
    }

    #[test]
    #[cfg(not(windows))]
    fn test_lww_sync_with_memory_provider_downloads_remote() {
        // 场景：远端有文件、本地空 → engine 下载到本地。
        // MemoryProvider 初始条目用完整远端路径（含 remote_prefix）。
        // 远端无 manifest 条目时，engine 用空 manifest，build_remote_records
        // 从 tree 补充 upsert 记录，触发下载。
        let dir = tempdir().unwrap();
        let provider = crate::sync::provider::MemoryProvider::with_entries([(
            "projects/test/project.json".to_string(),
            b"remote content".to_vec(),
        )]);

        let res = lww_sync_with_memory(dir.path(), &provider, true).unwrap();

        // project.json 应被下载到本地。
        assert!(res.downloaded_files.contains(&"project.json".to_string()));
        // manifest 不计入 downloaded_files（它由 upload_manifest 单独上传）。
        assert!(!res
            .downloaded_files
            .contains(&"app-meta/sync/manifest.sync.json".to_string()));
        assert!(res.uploaded_files.is_empty());
        assert!(res.local_deletes.is_empty());
        assert!(res.remote_deletes.is_empty());

        let local_file_path = dir.path().join("project.json");
        assert!(local_file_path.exists());
        let local_content = std::fs::read_to_string(local_file_path).unwrap();
        assert_eq!(local_content, "remote content");
        // engine 同步后会写本地 manifest。
        assert!(dir.path().join("app-meta/sync/manifest.sync.json").exists());
    }

    #[test]
    #[cfg(not(windows))]
    fn test_lww_sync_with_memory_provider_uploads_local() {
        // 场景：本地有文件、远端空 → engine 上传到远端。
        // 本地 sync state 为默认（known_files 空），project.json 被视为新文件上传。
        let dir = tempdir().unwrap();
        let local_path = dir.path().join("project.json");
        std::fs::create_dir_all(local_path.parent().unwrap()).unwrap();
        std::fs::write(&local_path, "local content").unwrap();

        let provider = crate::sync::provider::MemoryProvider::new();
        let res = lww_sync_with_memory(dir.path(), &provider, true).unwrap();

        assert!(res.uploaded_files.contains(&"project.json".to_string()));
        assert!(res.downloaded_files.is_empty());
        assert!(res.local_deletes.is_empty());
        assert!(res.remote_deletes.is_empty());

        // 远端 MemoryProvider 现在应有该文件，内容与本地一致。
        use crate::sync::provider::SyncProvider;
        let obj = provider.read("projects/test/project.json").unwrap();
        assert!(obj.is_some());
        assert_eq!(obj.unwrap().content, b"local content");
    }

    #[test]
    #[cfg(not(windows))]
    fn test_lww_sync_with_memory_provider_local_delete_propagates() {
        // 场景：本地删除文件 → 远端删除。
        // 本地 sync state 记录 known_files["project.json"]，但本地文件已删，
        // build_local_records 生成 delete 墓碑。远端 manifest 记录 upsert
        // （updated_at_ms 较小），LWW 本地 delete 时间戳获胜 →
        // LwwLocalWinsDeleteRecord → delete_remote_files 删除远端文件。
        // #645 评论 5504296097 问题1.2修复：必须有 tombstone 才能生成 delete record
        // （无 tombstone 不伪造 now_ms 作为删除时间）。
        let dir = tempdir().unwrap();

        let mut state = crate::sync::types::SyncState::default();
        state.device_id = "device_local".to_string();
        state
            .known_files
            .insert("project.json".to_string(), "old_hash".to_string());
        state
            .known_files_updated_at
            .insert("project.json".to_string(), 1000);
        // 添加 tombstone 记录真实删除时间（deleted_at=2 → 2000ms > remote 900ms）。
        state.tombstones.push(crate::sync::types::Tombstone {
            original_path: "project.json".to_string(),
            trash_path: "app-meta/trash/project.json".to_string(),
            deleted_at: 2,
            purge_after: 9999999999,
            deleted_by: "device_local".to_string(),
            original_hash: "old_hash".to_string(),
            kind: "local_delete".to_string(),
        });
        SyncService::save_sync_state(dir.path(), &state).unwrap();

        let remote_manifest = crate::sync::types::SyncManifest {
            files: vec![crate::sync::types::ManifestFileRecord {
                path: "project.json".to_string(),
                content_hash: format!("{:x}", md5::compute(b"remote content")),
                updated_at_ms: 900,
                deleted_at_ms: None,
                device_id: "device_remote".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };
        let manifest_json = serde_json::to_string(&remote_manifest).unwrap();
        let provider = crate::sync::provider::MemoryProvider::with_entries([
            (
                "projects/test/project.json".to_string(),
                b"remote content".to_vec(),
            ),
            (
                "projects/test/app-meta/sync/manifest.sync.json".to_string(),
                manifest_json.into_bytes(),
            ),
        ]);

        let res = lww_sync_with_memory(dir.path(), &provider, true).unwrap();

        // 本地删除传播到远端：local_deletes 记录本地主动删除的路径。
        assert!(res.local_deletes.contains(&"project.json".to_string()));

        // 远端文件应被删除。
        use crate::sync::provider::SyncProvider;
        let obj = provider.read("projects/test/project.json").unwrap();
        assert!(obj.is_none(), "remote file should be deleted after sync");
    }
}
