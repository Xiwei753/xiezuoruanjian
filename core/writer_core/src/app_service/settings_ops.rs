use crate::api::{
    LocalSettingsDto, SyncCapabilityDto, SyncConfigDto, SyncSecretsDto, SyncStateDto,
    SyncableSettingsDto, WriterError,
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

    pub fn load_sync_config(&self, project_id: String) -> Result<SyncConfigDto, WriterError> {
        self.api.load_sync_config(&project_id)
    }

    pub fn save_sync_config(
        &self,
        project_id: String,
        config: SyncConfigDto,
    ) -> Result<bool, WriterError> {
        self.api.save_sync_config(&project_id, config)
    }

    pub fn load_sync_secrets(&self, project_id: String) -> Result<SyncSecretsDto, WriterError> {
        self.api.load_sync_secrets(&project_id)
    }

    pub fn save_sync_secrets(
        &self,
        project_id: String,
        secrets: SyncSecretsDto,
    ) -> Result<bool, WriterError> {
        self.api.save_sync_secrets(&project_id, secrets)
    }

    pub fn load_sync_state(&self, project_id: String) -> Result<SyncStateDto, WriterError> {
        self.api.load_sync_state(&project_id)
    }

    pub fn get_sync_capability(
        &self,
        project_id: String,
    ) -> Result<SyncCapabilityDto, WriterError> {
        let config = self.api.load_sync_config(&project_id)?;
        let secrets = self.load_sync_secrets(project_id.clone())?;

        let mut block_reason_code = None;
        let mut block_message_key = None;
        let message_args = std::collections::HashMap::new();
        let mut can_run = true;

        if !config.enabled {
            can_run = false;
            block_reason_code = Some("DISABLED".to_string());
            block_message_key = Some("sync.block.disabled".to_string());
        } else if !self.secure_storage_available() {
            can_run = false;
            block_reason_code = Some("SECURE_STORAGE_UNAVAILABLE".to_string());
            block_message_key = Some("sync.block.secure_storage_unavailable".to_string());
        } else if config.remote_url.is_empty() {
            can_run = false;
            block_reason_code = Some("REMOTE_URL_MISSING".to_string());
            block_message_key = Some("sync.block.remote_url_missing".to_string());
        } else if secrets.token.as_ref().is_none_or(|t: &String| t.is_empty()) {
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
