use super::service::{ApiResult, WriterCoreApi};
use super::types::*;

/// 同步 API — 全量同步统一入口（Issue #630）。
///
/// 一个全局 `SyncConfig` + 一份全局凭据，`perform_full_sync` 内部按 `SyncTarget`
/// 把不同本地根映射到同一个远端仓库的不同前缀。
/// 旧的"作品同步 + 应用数据同步"两套用户配置 API 已删除。
impl WriterCoreApi {
    /// 旧→新同步 profile 一次性迁移（Issue #630 评论第 4 点 / D）。
    ///
    /// 详见 `crate::sync::legacy_migration`。失败时返回 `WriterError`；
    /// 冲突时返回 `NeedsReconfigure`（非 Err），由 UI 引导用户重选全局仓库。
    pub fn migrate_legacy_sync_profile(&self) -> ApiResult<LegacyMigrationOutcomeDto> {
        self.core()
            .migrate_legacy_sync_profile()
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 旧→新同步 profile 一次性迁移，接受精确 generation metadata（Issue #630 评论第 5 点 Part C）。
    ///
    /// 详见 `crate::sync::legacy_migration::LegacySyncProfileMigrator::migrate_with_metadata`。
    /// 当 metadata 中某 source 有 `active_generation = Some(n)` 时，精确读取
    /// `sync_token_<base>_g{n}`；当 `active_generation = None` 时回退 base key / 文件。
    pub fn migrate_legacy_sync_profile_with_metadata(
        &self,
        metadata: Vec<LegacyProfileMetadataDto>,
    ) -> ApiResult<LegacyMigrationOutcomeDto> {
        self.core()
            .migrate_legacy_sync_profile_with_metadata(
                &metadata.into_iter().map(Into::into).collect::<Vec<_>>(),
            )
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 加载全局同步配置。
    pub fn load_sync_config(&self) -> ApiResult<SyncConfigDto> {
        self.core()
            .load_sync_config()
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 保存全局同步配置。成功返回 true。
    pub fn save_sync_config(&self, config: SyncConfigDto) -> ApiResult<bool> {
        self.core()
            .save_sync_config(&config.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    /// 加载全局同步密钥（token 等）。
    pub fn load_sync_secrets(&self) -> ApiResult<SyncSecretsDto> {
        self.core()
            .load_sync_secrets()
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 保存全局同步密钥。成功返回 true。
    pub fn save_sync_secrets(&self, secrets: SyncSecretsDto) -> ApiResult<bool> {
        self.core()
            .save_sync_secrets(&secrets.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    /// #592 五：设置进程级 secrets override。
    pub fn set_sync_secrets_override(&self, secrets: SyncSecretsDto) -> ApiResult<()> {
        self.core().set_secrets_override(Some(secrets.into()));
        Ok(())
    }

    /// #595 十：清除进程级 secrets override。
    pub fn clear_sync_secrets_override(&self) -> ApiResult<()> {
        self.core().set_secrets_override(None);
        Ok(())
    }

    /// #592 五：按 generation 保存凭据到安全存储。
    pub fn save_sync_secrets_for_generation(
        &self,
        generation: u64,
        secrets: SyncSecretsDto,
    ) -> ApiResult<bool> {
        self.core()
            .save_sync_secrets_for_generation(generation, &secrets.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    /// #592 五：读取指定 generation 的安全存储凭据；缺失返回 None。
    pub fn load_sync_secrets_for_generation(
        &self,
        generation: u64,
    ) -> ApiResult<Option<SyncSecretsDto>> {
        self.core()
            .load_sync_secrets_for_generation(generation)
            .map(|opt| opt.map(Into::into))
            .map_err(Into::into)
    }

    /// #595 五：删除指定 generation 的安全存储凭据。
    pub fn delete_sync_secrets_for_generation(&self, generation: u64) -> ApiResult<()> {
        self.core()
            .delete_sync_secrets_for_generation(generation)
            .map_err(Into::into)
    }

    /// Project target 同步状态。
    pub fn load_sync_state(&self, project_id: &str) -> ApiResult<SyncStateDto> {
        self.core()
            .load_sync_state(project_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    /// App target 同步状态。
    pub fn load_app_sync_state(&self) -> ApiResult<SyncStateDto> {
        self.core()
            .load_app_sync_state()
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 保存 App target 同步状态。
    pub fn save_app_sync_state(&self, state: SyncStateDto) -> ApiResult<()> {
        self.core()
            .save_app_sync_state(&state.into())
            .map_err(Into::into)
    }

    /// 全量同步诊断 — 只测一次仓库、分支、token。
    pub fn perform_full_sync_diagnostics(
        &self,
        config: SyncConfigDto,
    ) -> ApiResult<FullSyncDiagnosticsResultDto> {
        self.core()
            .perform_full_sync_diagnostics(&config.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 全量同步 dry-run — 枚举 App target + 所有 Project target。
    pub fn perform_full_sync_dry_run(
        &self,
        config: SyncConfigDto,
    ) -> ApiResult<FullSyncDryRunResultDto> {
        self.core()
            .perform_full_sync_dry_run(&config.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 全量同步 — 先 App target，再所有 Project target，共享同一份 config / secrets。
    pub fn perform_full_sync(
        &self,
        config: SyncConfigDto,
        force_sync: bool,
    ) -> ApiResult<FullSyncResultDto> {
        self.core()
            .perform_full_sync(&config.into(), force_sync)
            .map(Into::into)
            .map_err(Into::into)
    }

    /// 冲突解决：保留本地版本。
    pub fn resolve_conflict_keep_local(&self, project_id: &str, path: &str) -> ApiResult<bool> {
        self.core()
            .resolve_conflict_keep_local(project_id, path)
            .map(|_| true)
            .map_err(Into::into)
    }

    /// 冲突解决：采用远端版本。
    pub fn resolve_conflict_take_remote(&self, project_id: &str, path: &str) -> ApiResult<bool> {
        self.core()
            .resolve_conflict_take_remote(project_id, path)
            .map(|_| true)
            .map_err(Into::into)
    }

    /// 冲突解决：标记为已合并。
    pub fn resolve_conflict_mark_merged(&self, project_id: &str, path: &str) -> ApiResult<bool> {
        self.core()
            .resolve_conflict_mark_merged(project_id, path)
            .map(|_| true)
            .map_err(Into::into)
    }

    /// 检查同步能力——综合 config 和 secrets 判断是否可执行全量同步。
    #[allow(clippy::unwrap_used)]
    pub fn get_sync_capability(&self) -> ApiResult<SyncCapabilityDto> {
        let config = self.load_sync_config()?;
        let secrets = self.load_sync_secrets()?;

        let mut block_reason_code = None;
        let mut block_message_key = None;
        let message_args = std::collections::HashMap::new();
        let mut can_run = true;

        if !config.enabled {
            can_run = false;
            block_reason_code = Some("DISABLED".to_string());
            block_message_key = Some("sync.block.disabled".to_string());
        } else if self.secure_storage.is_none() {
            can_run = false;
            block_reason_code = Some("SECURE_STORAGE_UNAVAILABLE".to_string());
            block_message_key = Some("sync.block.secure_storage_unavailable".to_string());
        } else if config.remote_url.is_empty() {
            can_run = false;
            block_reason_code = Some("REMOTE_URL_MISSING".to_string());
            block_message_key = Some("sync.block.remote_url_missing".to_string());
        } else if secrets.token.is_none() || secrets.token.as_ref().unwrap().is_empty() {
            can_run = false;
            block_reason_code = Some("TOKEN_MISSING".to_string());
            block_message_key = Some("sync.block.token_missing".to_string());
        }

        Ok(SyncCapabilityDto {
            can_run,
            block_reason_code,
            block_message_key,
            message_args,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_load_sync_secrets_global() {
        let temp_dir = tempdir().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
        let api = WriterCoreApi::new(temp_dir.path(), temp_dir.path().join("projects"));

        // Test loading when no secrets exist (should return default/empty struct)
        let loaded_empty = api.load_sync_secrets().unwrap();
        assert_eq!(loaded_empty.token, None);

        // Save some dummy secrets
        let dummy_secrets = SyncSecretsDto {
            token: Some("ghp_dummy123".to_string()),
        };
        api.save_sync_secrets(dummy_secrets.clone()).unwrap();

        // Test loading the saved secrets
        let loaded_secrets = api.load_sync_secrets().unwrap();
        assert_eq!(loaded_secrets.token, dummy_secrets.token);
    }

    #[test]
    fn save_sync_config_returns_true_on_success() {
        let temp_dir = tempdir().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("projects")).unwrap();
        let api = WriterCoreApi::new(temp_dir.path(), temp_dir.path().join("projects"));

        let config = SyncConfigDto {
            enabled: true,
            backend_type: "github_api".to_string(),
            remote_url: "https://github.com/test/repo.git".to_string(),
            transport: "https_token".to_string(),
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            username: "".to_string(),
            has_network_permission: true,
            has_network_state_permission: true,
        };

        let result = api.save_sync_config(config);
        assert!(result.unwrap());
    }
}
