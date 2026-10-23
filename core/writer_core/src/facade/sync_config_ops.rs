//! 同步配置与凭据 facade — 全局唯一配置 / 凭据 / 旧 profile 迁移。
//!
//! 旧"作品同步 + 应用数据同步"两套用户配置入口已删除，这里只有一份全局
//! `SyncConfig`（`<app_data_root>/app-meta/sync/config.local.json`）与一份全局凭据
//! （安全存储 key `sync_token_global` / generation key `sync_token_global_g{N}`）。
//!
//!  ：旧 JSON 顶层 GitHub 字段（`remote_url`/`branch`/`username`/
//! `transport`/`backend_type`）在 `load_sync_config` 边界一次性迁移为
//! `provider_config = ProviderConfig::GitHub(...)`，保存新格式后旧字段不再出现。

impl super::WriterCore {
    // ── 全局配置 全局凭据（：唯一一份） ──

    /// 旧→新同步 profile 一次性迁移（  / D）。
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
    /// 旧 JSON 顶层 GitHub 字段在 load 边界一次性迁移为
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
        if let Ok(mut config) = serde_json::from_str::<crate::sync::SyncConfig>(&content) {
            // 旧版本可能持久化 active_provider == "git"。现在通用 Git HTTPS
            // 已成为正式传输：已有 provider_config 时按仓库类型迁移到对应路由，
            // 保留地址、分支、凭据与启用状态；只有完全无配置的死数据才禁用。
            if config.active_provider == "git" {
                if let Some(provider_config) = config.provider_config.as_ref() {
                    config.active_provider = match provider_config {
                        #[cfg(feature = "github-api")]
                        crate::sync::provider::ProviderConfig::GitHub(_) => {
                            "github_api".to_string()
                        }
                        crate::sync::provider::ProviderConfig::GitHttps(_) => {
                            "git_https".to_string()
                        }
                    };
                    log::warn!(
                        "load_sync_config: migrated legacy active_provider=\"git\" to {}",
                        config.active_provider
                    );
                    self.save_sync_config(&config)?;
                    return Ok(config);
                }
                if !has_legacy_top_level_github_fields(&raw) {
                    log::warn!(
                        "load_sync_config: found legacy active_provider=\"git\" without \
                         provider config, disabling sync"
                    );
                    config.active_provider = "git_https".to_string();
                    config.enabled = false;
                    self.save_sync_config(&config)?;
                    return Ok(config);
                }
            }
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
                Some(crate::sync::provider::ProviderConfig::GitHttps(git)) => {
                    git.remote_url.is_empty()
                }
                None => true,
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
        // facade 不再持有 secrets_override。
        // 进程级 override 由 api::service::WriterCoreApi.secrets_override_snapshot() 统一提供，
        // 同步编排层（full_sync）在 Prepare 阶段取 snapshot 后传入。
        const GLOBAL_KEY: &str = "sync_token_global";
        if let Some(ref storage) = self.secure_storage {
            // Issue #924 复核：区分 Ok(None)（新用户/需要迁移）与 Err（安全存储真故障）。
            // 安全存储故障必须明确返回错误，不能吞掉后误判为「Token 不存在」。
            match storage.get_secret(GLOBAL_KEY) {
                Ok(Some(bytes)) => {
                    let token = String::from_utf8(bytes).map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "secure storage value for {} is not valid UTF-8: {}",
                            GLOBAL_KEY, e
                        )))
                    })?;
                    if !token.is_empty() {
                        return Ok(crate::sync::SyncSecrets::from_secret(token));
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "secure storage read failed for {}: {}",
                        GLOBAL_KEY, error
                    ))));
                }
            }
            // 旧明文凭据一次性迁移：只有安全存储写入成功、读回一致且旧文件删除成功
            // 才认为迁移完成；任一步失败保留旧文件并返回错误（Issue #924），
            // 不再出现「set 失败仍删明文」或「悄悄写回明文」的旁路。
            let file_secrets = self.load_sync_secrets_from_file()?;
            if let Some(token) = file_secrets.any_secret() {
                if !token.is_empty() {
                    let legacy_path = self.legacy_secrets_file_path(None);
                    self.migrate_legacy_token_into_secure_storage(
                        storage.as_ref(),
                        GLOBAL_KEY,
                        &legacy_path,
                        token.as_str(),
                    )?;
                }
            }
            return Ok(file_secrets);
        }
        self.load_sync_secrets_from_file()
    }

    ///  五：按 generation 保存凭据到安全存储（key: sync_token_global_g{N}）。
    pub fn save_sync_secrets_for_generation(
        &self,
        generation: u64,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<()> {
        let key = format!("sync_token_global_g{}", generation);
        if let Some(ref storage) = self.secure_storage {
            if let Some(token) = secrets.any_secret() {
                storage
                    .set_secret(&key, token.as_bytes())
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            } else {
                storage
                    .delete_secret(&key)
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            }
            // 安全存储已是事实来源：清除可能残留的旧明文 generation 文件，
            // 避免后续读取时把旧 token 复活。
            let legacy_path = self.legacy_secrets_file_path(Some(generation));
            if legacy_path.exists() {
                std::fs::remove_file(&legacy_path)?;
            }
            return Ok(());
        }
        let secrets_path = self.legacy_secrets_file_path(Some(generation));
        write_secrets_atomic(
            &secrets_path,
            secrets,
            &format!("secrets_g{}.local.json", generation),
        )
    }

    ///  五：读取指定 generation 的安全存储凭据；缺失返回 None。
    pub fn load_sync_secrets_for_generation(
        &self,
        generation: u64,
    ) -> crate::error::Result<Option<crate::sync::SyncSecrets>> {
        let key = format!("sync_token_global_g{}", generation);
        if let Some(ref storage) = self.secure_storage {
            // Issue #924 复核：读取错误必须透出（Err），不能用 `.ok().flatten()` 吞掉后
            // 误判为「没有凭据」。只有 Ok(None) 才继续旧明文迁移路径。
            let token = match storage.get_secret(&key) {
                Ok(Some(bytes)) => {
                    let token = String::from_utf8(bytes).map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "secure storage value for {} is not valid UTF-8: {}",
                            key, e
                        )))
                    })?;
                    if token.is_empty() {
                        None
                    } else {
                        Some(token)
                    }
                }
                Ok(None) => None,
                Err(error) => {
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "secure storage read failed for {}: {}",
                        key, error
                    ))));
                }
            };
            if let Some(token) = token {
                return Ok(Some(crate::sync::SyncSecrets::from_secret(token)));
            }
            // generation 旧明文凭据一次性迁移（成功时序，Issue #924）：
            // 写入成功 → 读回一致 → 删除旧文件；任一步失败保留文件并返回错误。
            let secrets_path = self.legacy_secrets_file_path(Some(generation));
            if !secrets_path.exists() {
                return Ok(None);
            }
            let content = std::fs::read_to_string(&secrets_path)?;
            let secrets: crate::sync::SyncSecrets = serde_json::from_str(&content)
                .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            match secrets.any_secret() {
                Some(token) if !token.is_empty() => {
                    self.migrate_legacy_token_into_secure_storage(
                        storage.as_ref(),
                        &key,
                        &secrets_path,
                        token.as_str(),
                    )?;
                }
                _ => {
                    // 无 token 的旧 generation 文件：直接删除，避免后续重复读取。
                    std::fs::remove_file(&secrets_path)?;
                }
            }
            return Ok(Some(secrets));
        }
        let secrets_path = self.legacy_secrets_file_path(Some(generation));
        if !secrets_path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&secrets_path)?;
        let secrets: crate::sync::SyncSecrets = serde_json::from_str(&content)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        Ok(Some(secrets))
    }

    ///  五：删除指定 generation 的安全存储凭据。
    pub fn delete_sync_secrets_for_generation(&self, generation: u64) -> crate::error::Result<()> {
        let key = format!("sync_token_global_g{}", generation);
        if let Some(ref storage) = self.secure_storage {
            storage
                .delete_secret(&key)
                .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            // 同时清理旧明文 generation 文件（若存在）。
            let legacy_path = self.legacy_secrets_file_path(Some(generation));
            if legacy_path.exists() {
                std::fs::remove_file(&legacy_path)?;
            }
            return Ok(());
        }
        let secrets_path = self.legacy_secrets_file_path(Some(generation));
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
            .map(crate::sync::SyncSecrets::from_secret)
            .unwrap_or_default())
    }

    pub fn save_sync_secrets(
        &self,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<()> {
        const GLOBAL_KEY: &str = "sync_token_global";
        if let Some(ref storage) = self.secure_storage {
            if let Some(token) = secrets.any_secret() {
                storage
                    .set_secret(GLOBAL_KEY, token.as_bytes())
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            } else {
                storage
                    .delete_secret(GLOBAL_KEY)
                    .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
            }
            // 安全存储已是事实来源：清除可能残留的旧明文文件，
            // 避免后续读取时把旧 token 复活。
            let legacy_path = self.legacy_secrets_file_path(None);
            if legacy_path.exists() {
                std::fs::remove_file(&legacy_path)?;
            }
            return Ok(());
        }
        let secrets_path = self.legacy_secrets_file_path(None);
        write_secrets_atomic(&secrets_path, secrets, "sync_secrets")
    }

    /// 旧明文凭据文件路径：`None` 为全局凭据，`Some(g)` 为 generation 凭据。
    fn legacy_secrets_file_path(&self, generation: Option<u64>) -> std::path::PathBuf {
        match generation {
            Some(generation) => self
                .app_data_root
                .join(format!("app-meta/sync/secrets_g{}.local.json", generation)),
            None => self.app_data_root.join("app-meta/sync/secrets.local.json"),
        }
    }

    /// 把旧明文 token 迁入安全存储（成功时序，Issue #924）：
    /// `set_secret` 成功 → `get_secret` 读回一致 → 删除旧明文文件。
    /// 任一步失败保留旧文件并返回错误，不把明文当作已迁移。
    fn migrate_legacy_token_into_secure_storage(
        &self,
        storage: &dyn writer_platform_api::SecureStorage,
        secure_key: &str,
        legacy_path: &std::path::Path,
        token: &str,
    ) -> crate::error::Result<()> {
        storage
            .set_secret(secure_key, token.as_bytes())
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        let readback_matches = storage
            .get_secret(secure_key)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?
            .as_deref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .is_some_and(|value| value == token);
        if !readback_matches {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "secure storage readback mismatch after migrating {}",
                legacy_path.display()
            ))));
        }
        std::fs::remove_file(legacy_path)?;
        Ok(())
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
/// `backend_type == "git"` 且 remote_url 不是已支持的 Provider 时，
/// 禁用同步并清空 provider_config，要求用户重新配置（不写死 `"git"` active_provider）。
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

    // 旧 Git 配置迁移：
    // - GitHub HTTPS → 保留 GitHub 配置，运行时由 router 先试 Git、失败转 API；
    // - 其他 HTTPS Git 平台 → 迁移为通用 GitHttps 配置；
    // - SSH/非 HTTPS 的旧 Git 配置 → 无法走标准 Git HTTPS，禁用同步要求重配。
    let is_git = backend_type == "git";
    let is_github_https = remote_url
        .as_deref()
        .is_some_and(crate::sync::url::is_github_https_remote);
    let is_https = remote_url.as_deref().is_some_and(|url| {
        crate::sync::url::sanitize_remote_url(url)
            .sanitized_url
            .to_ascii_lowercase()
            .starts_with("https://")
    });
    if is_git && is_github_https {
        config.active_provider = "github_api".to_string();
    } else if is_git && is_https {
        config.active_provider = "git_https".to_string();
        config.enabled = true;
        config.provider_config = Some(crate::sync::provider::ProviderConfig::GitHttps(
            crate::sync::provider::GitHttpsProviderConfig::from_legacy_fields(
                remote_url, branch, username,
            ),
        ));
        return config;
    } else if is_git {
        config.active_provider = "github_api".to_string();
        config.enabled = false;
        config.provider_config = None;
        return config;
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

#[cfg(test)]
mod tests {
    #[cfg(feature = "github-api")]
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

    /// 旧非 GitHub Git HTTPS 配置迁移为通用 GitHttps。
    #[test]
    #[cfg(feature = "github-api")]
    fn migrate_legacy_non_github_git_to_git_https() {
        let raw: serde_json::Value = serde_json::json!({
            "enabled": true,
            "backend_type": "git",
            "remote_url": "https://gitlab.com/test/repo.git",
            "transport": "https_token",
            "branch": "main"
        });
        let config = migrate_legacy_sync_config(&raw, default_sync_config());
        assert_eq!(config.active_provider, "git_https");
        assert!(config.enabled);
        match config.provider_config {
            Some(crate::sync::provider::ProviderConfig::GitHttps(ref git)) => {
                assert_eq!(git.remote_url, "https://gitlab.com/test/repo.git");
                assert_eq!(git.branch, "main");
            }
            _ => panic!("expected GitHttps provider config"),
        }
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
