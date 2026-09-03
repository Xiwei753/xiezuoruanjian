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

    /// 全局同步配置（Issue #630）。
    pub fn load_sync_config(&self) -> Result<SyncConfigDto, WriterError> {
        self.api.load_sync_config()
    }

    pub fn save_sync_config(&self, config: SyncConfigDto) -> Result<bool, WriterError> {
        self.api.save_sync_config(config)
    }

    /// 全局同步凭据。
    pub fn load_sync_secrets(&self) -> Result<SyncSecretsDto, WriterError> {
        self.api.load_sync_secrets()
    }

    pub fn save_sync_secrets(&self, secrets: SyncSecretsDto) -> Result<bool, WriterError> {
        self.api.save_sync_secrets(secrets)
    }

    /// Project target 同步状态。
    pub fn load_sync_state(&self, project_id: String) -> Result<SyncStateDto, WriterError> {
        self.api.load_sync_state(&project_id)
    }

    /// 检查同步能力——综合全局 config 和 secrets 判断是否可执行全量同步。
    pub fn get_sync_capability(&self) -> Result<SyncCapabilityDto, WriterError> {
        let config = self.api.load_sync_config()?;
        let secrets = self.load_sync_secrets()?;

        let mut block_reason_code = None;
        let mut block_message_key = None;
        let message_args = std::collections::HashMap::new();
        let mut can_run = true;

        // 从 provider_config 读 remote_url（Issue #645 评论第 2 点）。
        let remote_url = config
            .provider_config
            .as_ref()
            .map(|pc| match pc {
                #[cfg(feature = "github-api")]
                crate::api::ProviderConfigDto::GitHub { remote_url, .. } => remote_url.clone(),
                #[cfg(not(feature = "github-api"))]
                _ => String::new(),
            })
            .unwrap_or_default();
        // 从 provider_secrets 读 token。
        let token = secrets
            .provider_secrets
            .as_ref()
            .map(|ps| match ps {
                #[cfg(feature = "github-api")]
                crate::api::ProviderSecretsDto::GitHub { token } => token.clone(),
                #[cfg(not(feature = "github-api"))]
                _ => String::new(),
            })
            .unwrap_or_default();

        if !config.enabled {
            can_run = false;
            block_reason_code = Some("DISABLED".to_string());
            block_message_key = Some("sync.block.disabled".to_string());
        } else if !self.secure_storage_available() {
            can_run = false;
            block_reason_code = Some("SECURE_STORAGE_UNAVAILABLE".to_string());
            block_message_key = Some("sync.block.secure_storage_unavailable".to_string());
        } else if remote_url.is_empty() {
            can_run = false;
            block_reason_code = Some("REMOTE_URL_MISSING".to_string());
            block_message_key = Some("sync.block.remote_url_missing".to_string());
        } else if token.is_empty() {
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
