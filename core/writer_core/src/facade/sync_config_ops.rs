//! 同步配置与凭据 facade — 全局唯一配置 / 凭据 / 旧 profile 迁移（Issue #630）。
//!
//! 旧"作品同步 + 应用数据同步"两套用户配置入口已删除，这里只有一份全局
//! `SyncConfig`（`<app_data_root>/app-meta/sync/config.local.json`）与一份全局凭据
//! （安全存储 key `sync_token_global` / generation key `sync_token_global_g{N}`）。

impl super::WriterCore {
    // ── 全局配置 + 全局凭据（Issue #630：唯一一份） ──

    /// 旧→新同步 profile 一次性迁移（Issue #630 评论第 4 点 / D）。
    ///
    /// 新全局 profile 已存在时返回 `NotNeeded`；否则依次探测旧应用级 / 旧作品级
    /// profile，多项目一致迁一份，不一致返回 `NeedsReconfigure`。提交成功后清理
    /// 旧凭据；失败/冲突时不删旧凭据。
    pub fn migrate_legacy_sync_profile(
        &self,
    ) -> crate::error::Result<crate::sync::legacy_migration::LegacyMigrationOutcome> {
        let migrator = crate::sync::legacy_migration::LegacySyncProfileMigrator::new(
            &self.app_data_root,
            &self.projects_root,
            self.secure_storage.as_deref(),
        );
        migrator.migrate()
    }

    /// 旧→新同步 profile 一次性迁移，接受精确 generation metadata（Issue #630 评论第 5 点 Part C）。
    ///
    /// 详见 `crate::sync::legacy_migration::LegacySyncProfileMigrator::migrate_with_metadata`。
    /// metadata 中每个项描述一个旧 profile 的 source 和 committed generation，
    /// 使 Core 精确读取 `sync_token_<base>_g<N>` 而不猜测枚举上限。
    pub fn migrate_legacy_sync_profile_with_metadata(
        &self,
        metadata: &[crate::sync::legacy_migration::LegacyProfileMetadata],
    ) -> crate::error::Result<crate::sync::legacy_migration::LegacyMigrationOutcome> {
        let migrator = crate::sync::legacy_migration::LegacySyncProfileMigrator::new(
            &self.app_data_root,
            &self.projects_root,
            self.secure_storage.as_deref(),
        );
        migrator.migrate_with_metadata(metadata)
    }

    /// 加载全局同步配置。路径：`<app_data_root>/app-meta/sync/config.local.json`。
    pub fn load_sync_config(&self) -> crate::error::Result<crate::sync::SyncConfig> {
        let config_path = self.app_data_root.join("app-meta/sync/config.local.json");
        if !config_path.exists() {
            return Ok(default_sync_config());
        }
        let content = std::fs::read_to_string(&config_path)?;
        let raw: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        let mut config: crate::sync::SyncConfig = serde_json::from_str(&content)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;

        let backend_missing = raw
            .as_object()
            .map(|obj| !obj.contains_key("backend_type"))
            .unwrap_or(false);
        let should_migrate = crate::sync::is_github_https_remote(&config.remote_url)
            && (backend_missing || config.backend_type == crate::sync::BackendType::Git);
        if should_migrate {
            config.backend_type = crate::sync::BackendType::GithubApi;
            self.save_sync_config(&config)?;
        }
        Ok(config)
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
        if config.enabled && config.remote_url.is_empty() {
            return Ok(false);
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
                        return Ok(crate::sync::SyncSecrets {
                            token: Some(token),
                            ssh_private_key: None,
                        });
                    }
                }
            }
            let file_secrets = self.load_sync_secrets_from_file()?;
            if let Some(token) = &file_secrets.token {
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
            if let Some(token) = &secrets.token {
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
            return Ok(token.map(|t| crate::sync::SyncSecrets {
                token: Some(t),
                ssh_private_key: None,
            }));
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
        let secrets: crate::sync::SyncSecrets = serde_json::from_str(&content)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
        Ok(secrets)
    }

    pub fn save_sync_secrets(
        &self,
        secrets: &crate::sync::SyncSecrets,
    ) -> crate::error::Result<()> {
        const GLOBAL_KEY: &str = "sync_token_global";
        if let Some(ref storage) = self.secure_storage {
            if let Some(token) = &secrets.token {
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
        backend_type: crate::sync::BackendType::GithubApi,
        remote_url: String::new(),
        transport: crate::sync::SyncProtocol::HttpsToken,
        branch: "main".to_string(),
        auto_sync: false,
        sync_interval_seconds: 300,
        username: String::new(),
        has_network_permission: true,
        has_network_state_permission: true,
    }
}
