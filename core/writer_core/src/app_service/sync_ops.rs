use crate::api::{
    FullSyncDiagnosticsResultDto, FullSyncDryRunResultDto, FullSyncResultDto, FullSyncStateDto,
    LegacyMigrationOutcomeDto, LegacyProfileMetadataDto, SyncConfigDto, SyncSecretsDto,
    SyncStateDto, WriterError,
};
use crate::sync::{SyncConfig, SyncSecrets};

impl super::WriterAppService {
    /// 旧→新同步 profile 一次性迁移（Issue #630 评论第 4 点 / D）。
    ///
    /// 详见 `crate::sync::legacy_migration`。失败返回 `WriterError`；
    /// 冲突返回 `NeedsReconfigure`（非 Err），由 UI 引导用户重选全局仓库。
    pub fn migrate_legacy_sync_profile(&self) -> Result<LegacyMigrationOutcomeDto, WriterError> {
        self.api.migrate_legacy_sync_profile()
    }

    /// 旧→新同步 profile 一次性迁移，接受精确 generation metadata（Issue #630 评论第 5 点 Part C）。
    pub fn migrate_legacy_sync_profile_with_metadata(
        &self,
        metadata: Vec<LegacyProfileMetadataDto>,
    ) -> Result<LegacyMigrationOutcomeDto, WriterError> {
        self.api.migrate_legacy_sync_profile_with_metadata(metadata)
    }

    /**
     * #592 六：secrets override 只在该进程尚未显式设置时从磁盘填充。
     * 同步启动前 Android 层会把 snapshot 的凭据显式写入 override，
     * 使整个操作只使用同一份 snapshot，不再从磁盘二次读取。
     */
    fn refresh_secrets_override(&self) {
        if self.api.secure_storage.is_some() {
            let mut core = self.api.core();
            if !core.has_secrets_override() {
                let secrets = core.load_sync_secrets().unwrap_or_default();
                core.set_secrets_override(Some(secrets));
            }
        }
    }

    /** #592 六：显式设置进程级 secrets override（同步启动前由平台层调用）。 */
    pub fn set_sync_secrets_override(&self, secrets: SyncSecretsDto) -> Result<(), WriterError> {
        self.api.core().set_secrets_override(Some(secrets.into()));
        Ok(())
    }

    /** #595 十：清除进程级 secrets override（同步操作结束后由平台层调用）。 */
    pub fn clear_sync_secrets_override(&self) -> Result<(), WriterError> {
        self.api.core().set_secrets_override(None);
        Ok(())
    }

    /** #592 五：按 generation 保存凭据到安全存储。 */
    pub fn save_sync_secrets_for_generation(
        &self,
        generation: u64,
        secrets: SyncSecretsDto,
    ) -> Result<bool, WriterError> {
        self.api
            .save_sync_secrets_for_generation(generation, secrets)
    }

    /** #592 五：读取指定 generation 的安全存储凭据；缺失返回 None。 */
    pub fn load_sync_secrets_for_generation(
        &self,
        generation: u64,
    ) -> Result<Option<SyncSecretsDto>, WriterError> {
        self.api.load_sync_secrets_for_generation(generation)
    }

    /** #595 五：删除指定 generation 的安全存储凭据（旧版本清理）。 */
    pub fn delete_sync_secrets_for_generation(&self, generation: u64) -> Result<(), WriterError> {
        self.api.delete_sync_secrets_for_generation(generation)
    }

    /// 全量同步诊断。
    pub fn perform_full_sync_diagnostics(
        &self,
        config: SyncConfigDto,
    ) -> Result<FullSyncDiagnosticsResultDto, WriterError> {
        self.refresh_secrets_override();
        self.api.perform_full_sync_diagnostics(config)
    }

    /// 全量同步 dry-run。
    pub fn perform_full_sync_dry_run(
        &self,
        config: SyncConfigDto,
    ) -> Result<FullSyncDryRunResultDto, WriterError> {
        self.refresh_secrets_override();
        self.api.perform_full_sync_dry_run(config)
    }

    /// 全量同步。
    pub fn perform_full_sync(
        &self,
        config: SyncConfigDto,
        force_sync: bool,
    ) -> Result<FullSyncResultDto, WriterError> {
        self.refresh_secrets_override();
        self.api.perform_full_sync(config, force_sync)
    }

    pub fn resolve_conflict_keep_local(
        &self,
        project_id: String,
        path: String,
    ) -> Result<bool, WriterError> {
        self.api.resolve_conflict_keep_local(&project_id, &path)
    }

    pub fn resolve_conflict_take_remote(
        &self,
        project_id: String,
        path: String,
    ) -> Result<bool, WriterError> {
        self.api.resolve_conflict_take_remote(&project_id, &path)
    }

    pub fn resolve_conflict_mark_merged(
        &self,
        project_id: String,
        path: String,
    ) -> Result<bool, WriterError> {
        self.api.resolve_conflict_mark_merged(&project_id, &path)
    }

    pub fn load_sync_token_from_secure_storage(&self) -> Option<String> {
        const GLOBAL_KEY: &str = "sync_token_global";
        self.api.secure_storage.as_ref().and_then(|storage| {
            storage
                .get_secret(GLOBAL_KEY)
                .ok()
                .flatten()
                .and_then(|bytes| String::from_utf8(bytes).ok())
        })
    }

    pub fn save_sync_token_to_secure_storage(&self, token: &str) -> Result<(), WriterError> {
        const GLOBAL_KEY: &str = "sync_token_global";
        if let Some(storage) = &self.api.secure_storage {
            storage
                .set_secret(GLOBAL_KEY, token.as_bytes())
                .map_err(WriterError::Other)?;
        }
        Ok(())
    }

    pub fn delete_sync_token_from_secure_storage(&self) -> Result<(), WriterError> {
        const GLOBAL_KEY: &str = "sync_token_global";
        if let Some(storage) = &self.api.secure_storage {
            storage
                .delete_secret(GLOBAL_KEY)
                .map_err(WriterError::Other)?;
        }
        Ok(())
    }

    pub fn load_sync_secrets_with_secure_storage(&self) -> SyncSecrets {
        self.api.core().load_sync_secrets().unwrap_or_default()
    }

    pub fn load_sync_config_core(&self) -> Result<SyncConfig, WriterError> {
        self.api
            .core()
            .load_sync_config()
            .map_err(WriterError::from)
    }

    pub fn save_sync_config_core(&self, config: &SyncConfig) -> Result<(), WriterError> {
        self.api
            .core()
            .save_sync_config(config)
            .map_err(WriterError::from)
    }

    pub fn save_sync_secrets_via_secure_storage(
        &self,
        secrets: &SyncSecrets,
    ) -> Result<(), WriterError> {
        self.api
            .core()
            .save_sync_secrets(secrets)
            .map_err(WriterError::from)
    }

    /// App target 同步状态。
    pub fn load_app_sync_state(&self) -> Result<SyncStateDto, WriterError> {
        self.api.load_app_sync_state()
    }

    pub fn save_app_sync_state(&self, state: SyncStateDto) -> Result<(), WriterError> {
        self.api.save_app_sync_state(state)
    }

    /// 全量同步持久状态（Issue #630 评论 5307423953 Part B）。
    pub fn load_full_sync_state(&self) -> Result<Option<FullSyncStateDto>, WriterError> {
        self.api.load_full_sync_state()
    }

    /// #630 评论 5308040939 Part 1：平台预处理失败写同一份 Core FullSyncState 的窄接口。
    ///
    /// `status` 为线格式状态码（与 `FullSyncStateDto.overall_status` 同一映射）；
    /// `failed_target` 传 `"preflight"`。只更新同一个 `full_state.local.json`。
    pub fn record_full_sync_preflight_failure(
        &self,
        status: String,
        failed_target: String,
    ) -> Result<(), WriterError> {
        self.api
            .record_full_sync_preflight_failure(status, failed_target)
    }
}
