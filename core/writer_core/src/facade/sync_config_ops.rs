//! 同步配置与凭据 facade — 全局唯一配置 / 凭据 / 旧 profile 迁移（Issue #630）。
//!
//! 旧"作品同步 + 应用数据同步"两套用户配置入口已删除，这里只有一份全局
//! `SyncConfig`（`<app_data_root>/app-meta/sync/config.local.json`）与一份全局凭据
//! （安全存储 key `sync_token_global` / generation key `sync_token_global_g{N}`）。
//!
//! Issue #645 评论第 2 点：旧 JSON 顶层 GitHub 字段（`remote_url`/`branch`/`username`/
//! `transport`/`backend_type`）在 `load_sync_config` 边界一次性迁移为
//! `provider_config = ProviderConfig::GitHub(...)`，保存新格式后旧字段不再出现。

impl super::WriterCore {
    // ── 全局配置 + 全局凭据（Issue #630：唯一一份） ──

    /// 旧→新同步 profile 一次性迁移（Issue #630 评论第 4 点 / D）。
    ///
    /// 新全局 profile 已存在时返回 `NotNeeded`；否则依次探测旧应用级 / 旧作品级
    /// profile，多项目一致迁一份，不一致返回 `NeedsReconfigure`。提交成功后清理
    /// 旧凭据；失败/冲突时不删旧凭据。
    pub fn migrate_legacy_sync_profile(
        &self,
    ) -> crate::error::Result<crate::storage::migration::LegacyMigrationOutcome> {
        let migrator = crate::storage::migration::LegacySyncProfileMigrator::new(
            &self.app_data_root,
            &self.projects_root,
            self.secure_storage.as_deref(),
        );
        migrator.migrate()
    }

    /// 详见 `crate::storage::migration::LegacySyncProfileMigrator::migrate_with_metadata`。
    pub fn migrate_legacy_sync_profile_with_metadata(
        &self,
        metadata: &[crate::storage::migration::LegacyProfileMetadata],
    ) -> crate::error::Result<crate::storage::migration::LegacyMigrationOutcome> {
        let migrator = crate::storage::migration::LegacySyncProfileMigrator::new(
            &self.app_data_root,
            &self.projects_root,
            self.secure_storage.as_deref(),
        );
        migrator.migrate_with_metadata(metadata)
    }

    /// 加载全局同步配置。路径：`<app_data_root>/app-meta/sync/config.local.json`。
    ///
    /// Issue #645 评论第 2 点：旧 JSON 顶层 GitHub 字段在 load 边界一次性迁移为
    /// `provider_config = ProviderConfig::GitHub(...)`，保存新格式后旧字段不再出现。
    /// 检测旧格式的判据：JSON 中有顶层 `remote_url`/`backend_type`/`branch`/`username`/
    /// `transport` 任一字段，且 `provider_config` 不存在或为 null。
    pub fn load_sync_config(&self) -> crate::error::Result<crate::sync::SyncConfig> {
        let config_path = self.app_data_root.join("app-meta/sync/config.local.json");
        if !config_path.exists() {
            return Ok(default_sync_config());
        }
        let content = std::fs::read_to_string(&config_path)?;
        let raw: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;

        // 新格式：直接反序列化（provider_config 字段已存在或为 None）。
        if let Ok(config) = serde_json::from_str::<crate::sync::SyncConfig>(&content) {
            if !has_legacy_top_level_github_fields(&raw) {
                return Ok(config);
            }
            // 同时有旧顶层字段和新 provider_config — 新格式优先，直接返回。
            if config.provider_config.is_some() {
                return Ok(config);
            }
            // 有旧顶层字段但 provider_config 为 None — 迁移。
            #[cfg(feature = "github-api")]
            {
                let migrated = migrate_legacy_sync_config(&raw, config);
                self.save_sync_config(&migrated)?;
                return Ok(migrated);
            }
            #[cfg(not(feature = "github-api"))]
            {
                return Ok(config);
            }
        }

        // 反序列化失败 — 尝试纯旧格式迁移（顶层 GitHub 字段 → provider_config）。
        #[cfg(feature = "github-api")]
        {
            let fallback = default_sync_config();
            let migrated = migrate_legacy_sync_config(&raw, fallback);
            self.save_sync_config(&migrated)?;
            Ok(migrated)
        }
        #[cfg(not(feature = "github-api"))]
        {
            Ok(default_sync_config())
        }
    }

    /// 保存全局同步配置。路径：`<app_data_root>/app-meta/sync/config.local.json`。
    pub fn save_sync_config(&self, config: &crate::sync::SyncConfig) -> crate::error::Result<()> {
        let config_path = self.app_data_root.join("app-meta/sync/config.local.json");
        save_config_atomic(&config_path, config)
    }

    pub fn validate_sync_config(
        &self,
        config: &crate::sync::SyncConfig,
    ) -> crate::error::Result<bool> {
        if config.enabled {
            let remote_url_empty = match &config.provider_config {
                #[cfg(feature = "github-api")]
                Some(crate::sync::provider::ProviderConfig::GitHub(gh)) => gh.remote_url.is_empty(),
                _ => true,
            };
            if remote_url_empty {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// 加载全局同步凭据。安全存储 key = `sync_token_global`。
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn load_sync_secrets(&self) -> crate::error::Result<crate::sync::SyncSecrets> {
        // #644 评论 5462823517 第1节：facade 不再持有 secrets_override。
        // 进程级 override 由 api::service::WriterCoreApi.secrets_override_snapshot() 统一提供，
        // 同步编排层（full_sync）在 Prepare 阶段取 snapshot 后传入。
        const GLOBAL_KEY: &str = "sync_token_global";
        if let Some(ref storage) = self.secure_storage {
            if let Ok(Some(bytes)) = storage.get_secret(GLOBAL_KEY) {
                if let Ok(token) = String::from_utf8(bytes) {
                    if !token.is_empty() {
                        return Ok(crate::sync::SyncSecrets::from_github_token(token));
                    }
                }
            }
            let file_secrets = self.load_sync_secrets_from_file()?;
            if let Some(token) = file_secrets.github_token() {
                if !token.is_empty() {
                    let _ = storage.set_secret(GLOBAL_KEY, token.as_bytes());
                    let secrets_path = self.app_data_root.join("app-meta/sync/secrets.local.json");
                    let _ = std::fs::remove_file(&secrets_path);
                }
            }
            return Ok(file_secrets);
        }
        self.load_sync_secrets_from_file()
    }

    /// #592 五：按 generation 保存凭据到安全存储（key: sync_token_global_g{N}）。
    pub fn save_sync_secrets_for_generation(
        &self,
        generation: u64,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<()> {
        let key = format!("sync_token_global_g{}", generation);
        if let Some(ref storage) = self.secure_storage {
            if let Some(token) = secrets.github_token() {
                storage
                    .set_secret(&key, token.as_bytes())
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            } else {
                storage
                    .delete_secret(&key)
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            }
            return Ok(());
        }
        let secrets_path = self
            .app_data_root
            .join(format!("app-meta/sync/secrets_g{}.local.json", generation));
        write_secrets_atomic(
            &secrets_path,
            secrets,
            &format!("secrets_g{}.local.json", generation),
        )
    }

    /// #592 五：读取指定 generation 的安全存储凭据；缺失返回 None。
    pub fn load_sync_secrets_for_generation(
        &self,
        generation: u64,
    ) -> crate::error::Result<Option<crate::sync::SyncSecrets>> {
        let key = format!("sync_token_global_g{}", generation);
        if let Some(ref storage) = self.secure_storage {
            let token = storage
                .get_secret(&key)
                .ok()
                .flatten()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .filter(|t| !t.is_empty());
            return Ok(token.map(crate::sync::SyncSecrets::from_github_token));
        }
        let secrets_path = self
            .app_data_root
            .join(format!("app-meta/sync/secrets_g{}.local.json", generation));
        if !secrets_path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&secrets_path)?;
        let secrets: crate::sync::SyncSecrets = serde_json::from_str(&content)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        Ok(Some(secrets))
    }

    /// #595 五：删除指定 generation 的安全存储凭据。
    pub fn delete_sync_secrets_for_generation(&self, generation: u64) -> crate::error::Result<()> {
        let key = format!("sync_token_global_g{}", generation);
        if let Some(ref storage) = self.secure_storage {
            storage
                .delete_secret(&key)
                .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            return Ok(());
        }
        let secrets_path = self
            .app_data_root
            .join(format!("app-meta/sync/secrets_g{}.local.json", generation));
        if secrets_path.exists() {
            std::fs::remove_file(&secrets_path)?;
        }
        Ok(())
    }

    fn load_sync_secrets_from_file(&self) -> crate::error::Result<crate::sync::SyncSecrets> {
        let secrets_path = self.app_data_root.join("app-meta/sync/secrets.local.json");
        if !secrets_path.exists() {
            return Ok(crate::sync::SyncSecrets::default());
        }
        let content = std::fs::read_to_string(&secrets_path)?;
        // 先尝试新格式（provider_secrets），失败则尝试旧格式（token/ssh_private_key）。
        if let Ok(secrets) = serde_json::from_str::<crate::sync::SyncSecrets>(&content) {
            return Ok(secrets);
        }
        // 旧格式迁移：{"token":"...","ssh_private_key":null} → provider_secrets
        #[derive(serde::Deserialize)]
        struct LegacySecrets {
            #[serde(default)]
            token: Option<String>,
        }
        let legacy: LegacySecrets = serde_json::from_str(&content)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        Ok(legacy
            .token
            .map(crate::sync::SyncSecrets::from_github_token)
            .unwrap_or_default())
    }

    pub fn save_sync_secrets(
        &self,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<()> {
        const GLOBAL_KEY: &str = "sync_token_global";
        if let Some(ref storage) = self.secure_storage {
            if let Some(token) = secrets.github_token() {
                storage
                    .set_secret(GLOBAL_KEY, token.as_bytes())
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            } else {
                storage
                    .delete_secret(GLOBAL_KEY)
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            }
            return Ok(());
        }
        let secrets_path = self.app_data_root.join("app-meta/sync/secrets.local.json");
        write_secrets_atomic(&secrets_path, secrets, "sync_secrets")
    }
}

/// 写 secrets 到文件的原子操作。
fn write_secrets_atomic(
    secrets_path: &std::path::Path,
    secrets: &crate::sync::SyncSecrets,
    tmp_prefix: &str,
) -> crate::error::Result<()> {
    if let Some(parent) = secrets_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(secrets)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
    let parent = secrets_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new(""));
    let mut tmp_file = tempfile::Builder::new()
        .prefix(tmp_prefix)
        .suffix(".tmp")
        .tempfile_in(parent)?;

    use std::io::Write;
    tmp_file.write_all(content.as_bytes())?;
    tmp_file.persist(secrets_path).map_err(|e| e.error)?;

    Ok(())
}

/// 原子写入 sync config。
fn save_config_atomic(
    config_path: &std::path::Path,
    config: &crate::sync::SyncConfig,
) -> crate::error::Result<()> {
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
    let tmp_path = config_path.with_extension("tmp");
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(tmp_path, config_path)?;
    Ok(())
}

/// 默认同步配置（未配置时返回）。
fn default_sync_config() -> crate::sync::SyncConfig {
    crate::sync::SyncConfig {
        enabled: false,
        active_provider: "github_api".to_string(),
        provider_config: None,
        auto_sync: false,
        sync_interval_seconds: 300,
        has_network_permission: true,
        has_network_state_permission: true,
    }
}

/// 检测 JSON 是否含旧顶层 GitHub 字段（`remote_url`/`backend_type`/`branch`/
/// `username`/`transport`）。
fn has_legacy_top_level_github_fields(raw: &serde_json::Value) -> bool {
    let Some(obj) = raw.as_object() else {
        return false;
    };
    obj.contains_key("remote_url")
        || obj.contains_key("backend_type")
        || obj.contains_key("branch")
        || obj.contains_key("username")
        || obj.contains_key("transport")
}

/// 从旧顶层 GitHub 字段构造 `ProviderConfig::GitHub(...)` 并迁移 `SyncConfig`。
///
/// 旧格式中 `remote_url`/`branch`/`username`/`transport`/`backend_type` 在 `SyncConfig`
/// 顶层，新格式统一收进 `provider_config = ProviderConfig::GitHub(...)`。
/// `backend_type == "git"` 且 `remote_url` 为 GitHub HTTPS 时自动升级为 `github_api`
/// （复刻旧 `resolved_backend_type` 逻辑）。
#[cfg(feature = "github-api")]
fn migrate_legacy_sync_config(
    raw: &serde_json::Value,
    mut config: crate::sync::SyncConfig,
) -> crate::sync::SyncConfig {
    let obj = raw.as_object();
    let remote_url = obj
        .and_then(|o| o.get("remote_url"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let branch = obj
        .and_then(|o| o.get("branch"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let username = obj
        .and_then(|o| o.get("username"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let transport = obj
        .and_then(|o| o.get("transport"))
        .and_then(|v| v.as_str())
        .and_then(|s| match s {
            "https_token" => {
                Some(crate::sync::provider::github::config::GitHubTransport::HttpsToken)
            }
            "ssh" | "ssh_deploy_key" => {
                Some(crate::sync::provider::github::config::GitHubTransport::SshDeployKey)
            }
            _ => None,
        });
    let backend_type = obj
        .and_then(|o| o.get("backend_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("github_api");

    // backend_type == "git" 且 remote_url 为 GitHub HTTPS → 自动升级为 github_api。
    let is_git = backend_type == "git";
    let is_github_https = remote_url
        .as_deref()
        .is_some_and(crate::sync::url::is_github_https_remote);
    if is_git && is_github_https {
        config.active_provider = "github_api".to_string();
    } else if is_git {
        config.active_provider = "git".to_string();
    } else {
        config.active_provider = "github_api".to_string();
    }

    // 仅当有 GitHub 字段且 provider_config 为 None 时构造 GitHub 配置。
    if config.provider_config.is_none()
        && (remote_url.is_some() || branch.is_some() || username.is_some() || transport.is_some())
    {
        let gh = crate::sync::provider::github::config::GitHubProviderConfig::from_legacy_fields(
            remote_url, branch, username, transport,
        );
        config.provider_config = Some(crate::sync::provider::ProviderConfig::GitHub(gh));
    }
    config
}

/// 非 github-api feature 下无法构造 `ProviderConfig::GitHub`，迁移为空操作，
/// 直接返回原 config（旧顶层 GitHub 字段被忽略）。
#[cfg(not(feature = "github-api"))]
fn migrate_legacy_sync_config(
    _raw: &serde_json::Value,
    config: crate::sync::SyncConfig,
) -> crate::sync::SyncConfig {
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "github-api")]
    fn migrate_legacy_top_level_fields_to_provider_config() {
        let raw: serde_json::Value = serde_json::json!({
            "enabled": true,
            "backend_type": "github_api",
            "remote_url": "https://github.com/o/r.git",
            "transport": "https_token",
            "branch": "main",
            "username": "alice",
            "auto_sync": false,
            "sync_interval_seconds": 300
        });
        let config = migrate_legacy_sync_config(&raw, default_sync_config());
        let gh = match config.provider_config {
            Some(crate::sync::provider::ProviderConfig::GitHub(ref gh)) => gh,
            _ => panic!("expected GitHub provider config"),
        };
        assert_eq!(gh.remote_url, "https://github.com/o/r.git");
        assert_eq!(gh.branch, "main");
        assert_eq!(gh.username, "alice");
        assert_eq!(
            gh.transport,
            crate::sync::provider::github::config::GitHubTransport::HttpsToken
        );
        assert_eq!(config.active_provider, "github_api");
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn migrate_legacy_git_backend_with_github_https_upgrades_to_github_api() {
        let raw: serde_json::Value = serde_json::json!({
            "enabled": true,
            "backend_type": "git",
            "remote_url": "https://github.com/test/repo.git",
            "transport": "https_token",
            "branch": "main"
        });
        let config = migrate_legacy_sync_config(&raw, default_sync_config());
        assert_eq!(config.active_provider, "github_api");
    }

    #[test]
    #[cfg(feature = "github-api")]
    fn migrate_legacy_skips_when_provider_config_already_present() {
        let raw: serde_json::Value = serde_json::json!({
            "enabled": true,
            "active_provider": "github_api",
            "provider_config": {
                "type": "github",
                "remote_url": "https://github.com/new/repo.git",
                "branch": "dev",
                "username": "x-access-token",
                "transport": "https_token"
            },
            "auto_sync": false,
            "sync_interval_seconds": 300,
            "has_network_permission": true,
            "has_network_state_permission": true,
            "remote_url": "https://github.com/old/repo.git"
        });
        let base = serde_json::from_value::<crate::sync::SyncConfig>(raw.clone())
            .expect("parse new format");
        let config = migrate_legacy_sync_config(&raw, base);
        let gh = match config.provider_config {
            Some(crate::sync::provider::ProviderConfig::GitHub(ref gh)) => gh,
            _ => panic!("expected GitHub provider config"),
        };
        // provider_config 已存在 — 不被旧顶层字段覆盖。
        assert_eq!(gh.remote_url, "https://github.com/new/repo.git");
        assert_eq!(gh.branch, "dev");
    }
}
