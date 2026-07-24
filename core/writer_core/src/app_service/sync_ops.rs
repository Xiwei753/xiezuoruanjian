use crate::api::{
    SyncConfigDto, SyncDiagnosticsResultDto, SyncPlanDto, SyncResultDto, WriterError,
};
use crate::sync::{SyncConfig, SyncSecrets};

impl super::WriterAppService {
    fn refresh_secrets_override(&self) {
        if self.api.secure_storage.is_some() {
            let secrets = self.api.core().load_sync_secrets().unwrap_or_default();
            self.api.set_secrets_override(Some(secrets));
        }
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
