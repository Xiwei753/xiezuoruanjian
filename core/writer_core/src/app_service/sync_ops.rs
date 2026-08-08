use crate::api::{
    SyncConfigDto, SyncDiagnosticsResultDto, SyncPlanDto, SyncResultDto, SyncSecretsDto,
    WriterError,
};
use crate::sync::{SyncConfig, SyncSecrets};

impl super::WriterAppService {
    /**
     * #592 六：secrets override 只在该进程尚未显式设置时从磁盘填充。
     * 同步启动前 Android 层会把 snapshot 的凭据显式写入 override，
     * 使整个操作只使用同一份 snapshot，不再从磁盘二次读取。
     */
    fn refresh_secrets_override(&self, project_id: &str) {
        if self.api.secure_storage.is_some() {
            let mut core = self.api.core();
            if !core.has_secrets_override() {
                let secrets = core.load_sync_secrets(project_id).unwrap_or_default();
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
        project_id: String,
        generation: u64,
        secrets: SyncSecretsDto,
    ) -> Result<bool, WriterError> {
        self.api
            .save_sync_secrets_for_generation(&project_id, generation, secrets)
    }

    /** #592 五：读取指定 generation 的安全存储凭据；缺失返回 None。 */
    pub fn load_sync_secrets_for_generation(
        &self,
        project_id: String,
        generation: u64,
    ) -> Result<Option<SyncSecretsDto>, WriterError> {
        self.api
            .load_sync_secrets_for_generation(&project_id, generation)
    }

    /** #595 五：删除指定 generation 的安全存储凭据（旧版本清理）。 */
    pub fn delete_sync_secrets_for_generation(
        &self,
        project_id: String,
        generation: u64,
    ) -> Result<(), WriterError> {
        self.api
            .delete_sync_secrets_for_generation(&project_id, generation)
    }

    pub fn perform_sync_diagnostics(
        &self,
        project_id: String,
        config: SyncConfigDto,
    ) -> Result<SyncDiagnosticsResultDto, WriterError> {
        self.refresh_secrets_override(&project_id);
        self.api.perform_sync_diagnostics(&project_id, config)
    }

    pub fn perform_sync_dry_run(
        &self,
        project_id: String,
        config: SyncConfigDto,
    ) -> Result<SyncPlanDto, WriterError> {
        self.refresh_secrets_override(&project_id);
        self.api.perform_sync_dry_run(&project_id, config)
    }

    pub fn perform_sync(
        &self,
        project_id: String,
        config: SyncConfigDto,
        force_sync: bool,
    ) -> Result<SyncResultDto, WriterError> {
        self.refresh_secrets_override(&project_id);
        self.api.perform_sync(&project_id, config, force_sync)
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

    pub fn load_sync_token_from_secure_storage(&self, project_id: &str) -> Option<String> {
        let key = format!("sync_token_{}", project_id);
        self.api.secure_storage.as_ref().and_then(|storage| {
            storage
                .get_secret(&key)
                .ok()
                .flatten()
                .and_then(|bytes| String::from_utf8(bytes).ok())
        })
    }

    pub fn save_sync_token_to_secure_storage(
        &self,
        project_id: &str,
        token: &str,
    ) -> Result<(), WriterError> {
        let key = format!("sync_token_{}", project_id);
        if let Some(storage) = &self.api.secure_storage {
            storage
                .set_secret(&key, token.as_bytes())
                .map_err(WriterError::Other)?;
        }
        Ok(())
    }

    pub fn delete_sync_token_from_secure_storage(
        &self,
        project_id: &str,
    ) -> Result<(), WriterError> {
        let key = format!("sync_token_{}", project_id);
        if let Some(storage) = &self.api.secure_storage {
            storage.delete_secret(&key).map_err(WriterError::Other)?;
        }
        Ok(())
    }

    pub fn load_sync_secrets_with_secure_storage(&self, project_id: &str) -> SyncSecrets {
        self.api
            .core()
            .load_sync_secrets(project_id)
            .unwrap_or_default()
    }

    pub fn load_sync_config_core(&self, project_id: &str) -> Result<SyncConfig, WriterError> {
        self.api
            .core()
            .load_sync_config(project_id)
            .map_err(WriterError::from)
    }

    pub fn save_sync_config_core(
        &self,
        project_id: &str,
        config: &SyncConfig,
    ) -> Result<(), WriterError> {
        self.api
            .core()
            .save_sync_config(project_id, config)
            .map_err(WriterError::from)
    }

    pub fn save_sync_secrets_via_secure_storage(
        &self,
        project_id: &str,
        secrets: &SyncSecrets,
    ) -> Result<(), WriterError> {
        self.api
            .core()
            .save_sync_secrets(project_id, secrets)
            .map_err(WriterError::from)
    }

    // ── 应用级同步通道（Issue #600 评论 #3 问题四） ──

    pub fn load_app_sync_config(&self) -> Result<SyncConfigDto, WriterError> {
        self.api.load_app_sync_config()
    }

    pub fn save_app_sync_config(&self, config: SyncConfigDto) -> Result<bool, WriterError> {
        self.api.save_app_sync_config(config)
    }

    pub fn load_app_sync_secrets(&self) -> Result<SyncSecretsDto, WriterError> {
        self.api.load_app_sync_secrets()
    }

    pub fn save_app_sync_secrets(&self, secrets: SyncSecretsDto) -> Result<bool, WriterError> {
        self.api.save_app_sync_secrets(secrets)
    }

    pub fn save_app_sync_secrets_for_generation(
        &self,
        generation: u64,
        secrets: SyncSecretsDto,
    ) -> Result<bool, WriterError> {
        self.api
            .save_app_sync_secrets_for_generation(generation, secrets)
    }

    pub fn load_app_sync_secrets_for_generation(
        &self,
        generation: u64,
    ) -> Result<Option<SyncSecretsDto>, WriterError> {
        self.api.load_app_sync_secrets_for_generation(generation)
    }

    pub fn delete_app_sync_secrets_for_generation(
        &self,
        generation: u64,
    ) -> Result<(), WriterError> {
        self.api.delete_app_sync_secrets_for_generation(generation)
    }

    pub fn perform_app_sync_diagnostics(
        &self,
        config: SyncConfigDto,
    ) -> Result<SyncDiagnosticsResultDto, WriterError> {
        self.api.perform_app_sync_diagnostics(config)
    }

    pub fn perform_app_sync_dry_run(
        &self,
        config: SyncConfigDto,
    ) -> Result<SyncPlanDto, WriterError> {
        self.api.perform_app_sync_dry_run(config)
    }

    pub fn perform_app_sync(
        &self,
        config: SyncConfigDto,
        force_sync: bool,
    ) -> Result<SyncResultDto, WriterError> {
        self.api.perform_app_sync(config, force_sync)
    }
}
