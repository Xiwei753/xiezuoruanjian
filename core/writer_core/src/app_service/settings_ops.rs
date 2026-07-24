use crate::api::{
    LocalSettingsDto, SyncableSettingsDto, SyncConfigDto, SyncSecretsDto, SyncStateDto,
    SyncCapabilityDto, WriterError,
};

impl super::WriterAppService {
    pub fn load_local_settings(&self) -> Result<LocalSettingsDto, WriterError> {
        self.api.load_local_settings()
    }

    pub fn save_local_settings(&self, settings: LocalSettingsDto) -> Result<bool, WriterError> {
        self.api.save_local_settings(settings)
    }

    pub fn load_syncable_settings(&self) -> Result<SyncableSettingsDto, WriterError> {
        self.api.load_syncable_settings()
    }

    pub fn save_syncable_settings(
        &self,
        settings: SyncableSettingsDto,
    ) -> Result<bool, WriterError> {
        self.api.save_syncable_settings(settings)
    }

    pub fn load_sync_config(&self) -> Result<SyncConfigDto, WriterError> {
        self.api.load_sync_config()
    }

    pub fn save_sync_config(&self, config: SyncConfigDto) -> Result<bool, WriterError> {
        self.api.save_sync_config(config)
    }

    pub fn load_sync_secrets(&self) -> Result<SyncSecretsDto, WriterError> {
        if self.secure_storage.is_some() {
            let secrets = self.load_sync_secrets_with_secure_storage();
            Ok(secrets.into())
        } else {
            self.api.load_sync_secrets()
        }
    }

    pub fn save_sync_secrets(&self, secrets: SyncSecretsDto) -> Result<bool, WriterError> {
        if self.secure_storage.is_some() {
            let core_secrets: crate::sync::SyncSecrets = secrets.into();
            self.save_sync_secrets_via_secure_storage(&core_secrets)?;
            Ok(true)
        } else {
            self.api.save_sync_secrets(secrets)
        }
    }

    pub fn load_sync_state(&self) -> Result<SyncStateDto, WriterError> {
        self.api.load_sync_state()
    }

    pub fn get_sync_capability(&self) -> Result<SyncCapabilityDto, WriterError> {
        let config = self.api.load_sync_config()?;
        let secrets = self.load_sync_secrets()?;

        let mut block_reason_code = None;
        let mut block_message_key = None;
        let message_args = std::collections::HashMap::new();
        let mut can_run = true;

        if !config.enabled {
            can_run = false;
            block_reason_code = Some("DISABLED".to_string());
            block_message_key = Some("sync.block.disabled".to_string());
        } else if config.remote_url.is_empty() {
            can_run = false;
            block_reason_code = Some("REMOTE_URL_MISSING".to_string());
            block_message_key = Some("sync.block.remote_url_missing".to_string());
        } else if secrets.token.as_ref().is_none_or(|t| t.is_empty()) {
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
