use crate::api::{
    SyncConfigDto, SyncDiagnosticsResultDto, SyncPlanDto, SyncResultDto, SyncSecretsDto, WriterError,
};
use crate::sync::{SyncConfig, SyncSecrets};

impl super::WriterAppService {
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

    pub fn perform_sync_diagnostics(
        &self,
        config: SyncConfigDto,
    ) -> Result<SyncDiagnosticsResultDto, WriterError> {
        self.refresh_secrets_override();
        self.api.perform_sync_diagnostics(config)
    }

    pub fn perform_sync_dry_run(&self, config: SyncConfigDto) -> Result<SyncPlanDto, WriterError> {
        self.refresh_secrets_override();
        self.api.perform_sync_dry_run(config)
    }

    pub fn perform_sync(&self, config: SyncConfigDto, force_sync: bool) -> Result<SyncResultDto, WriterError> {
        self.refresh_secrets_override();
        self.api.perform_sync(config, force_sync)
    }

    pub fn resolve_conflict_keep_local(&self, path: String) -> Result<bool, WriterError> {
        self.api.resolve_conflict_keep_local(&path)
    }

    pub fn resolve_conflict_take_remote(&self, path: String) -> Result<bool, WriterError> {
        self.api.resolve_conflict_take_remote(&path)
    }

    pub fn resolve_conflict_mark_merged(&self, path: String) -> Result<bool, WriterError> {
        self.api.resolve_conflict_mark_merged(&path)
    }

    pub fn load_sync_token_from_secure_storage(&self) -> Option<String> {
        self.api.secure_storage.as_ref().and_then(|storage| {
            storage.get_secret("sync_token").ok().flatten().and_then(|bytes| {
                String::from_utf8(bytes).ok()
            })
        })
    }

    pub fn save_sync_token_to_secure_storage(&self, token: &str) -> Result<(), WriterError> {
        if let Some(storage) = &self.api.secure_storage {
            storage.set_secret("sync_token", token.as_bytes())
                .map_err(WriterError::Other)?;
        }
        Ok(())
    }

    pub fn delete_sync_token_from_secure_storage(&self) -> Result<(), WriterError> {
        if let Some(storage) = &self.api.secure_storage {
            storage.delete_secret("sync_token")
                .map_err(WriterError::Other)?;
        }
        Ok(())
    }

    pub fn load_sync_secrets_with_secure_storage(&self) -> SyncSecrets {
        self.api.core().load_sync_secrets().unwrap_or_default()
    }

    pub fn load_sync_config_core(&self) -> Result<SyncConfig, WriterError> {
        self.api.core().load_sync_config().map_err(WriterError::from)
    }

    pub fn save_sync_config_core(&self, config: &SyncConfig) -> Result<(), WriterError> {
        self.api.core().save_sync_config(config).map_err(WriterError::from)
    }

    pub fn save_sync_secrets_via_secure_storage(&self, secrets: &SyncSecrets) -> Result<(), WriterError> {
        self.api.core().save_sync_secrets(secrets).map_err(WriterError::from)
    }
}
