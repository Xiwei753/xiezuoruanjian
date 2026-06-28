use super::service::{ApiResult, WriterCoreApi};
use super::types::*;

impl WriterCoreApi {
    pub fn load_sync_config(&self) -> ApiResult<SyncConfigDto> {
        self.core()
            .load_sync_config()
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_sync_config(&self, config: SyncConfigDto) -> ApiResult<bool> {
        self.core()
            .save_sync_config(&config.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn load_sync_secrets(&self) -> ApiResult<SyncSecretsDto> {
        self.core()
            .load_sync_secrets()
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_sync_secrets(&self, secrets: SyncSecretsDto) -> ApiResult<bool> {
        self.core()
            .save_sync_secrets(&secrets.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn load_sync_state(&self) -> ApiResult<SyncStateDto> {
        self.core()
            .load_sync_state()
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn perform_sync_diagnostics(
        &self,
        config: SyncConfigDto,
    ) -> ApiResult<SyncDiagnosticsResultDto> {
        self.core()
            .perform_sync_diagnostics(&config.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn perform_sync_dry_run(&self, config: SyncConfigDto) -> ApiResult<SyncPlanDto> {
        self.core()
            .perform_sync_dry_run(&config.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn perform_sync(&self, config: SyncConfigDto) -> ApiResult<SyncResultDto> {
        self.core()
            .perform_sync(&config.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn resolve_conflict_keep_local(&self, path: &str) -> ApiResult<bool> {
        self.core()
            .resolve_conflict_keep_local(path)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn resolve_conflict_take_remote(&self, path: &str) -> ApiResult<bool> {
        self.core()
            .resolve_conflict_take_remote(path)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn resolve_conflict_mark_merged(&self, path: &str) -> ApiResult<bool> {
        self.core()
            .resolve_conflict_mark_merged(path)
            .map(|_| true)
            .map_err(Into::into)
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_load_sync_secrets() {
        let temp_dir = tempdir().unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

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
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let config = SyncConfigDto {
            enabled: true,
            backend_type: "github_api".to_string(),
            remote_url: "https://github.com/test/repo.git".to_string(),
            transport: "https_token".to_string(),
            branch: "main".to_string(),
            auto_sync: false,
            sync_interval_seconds: 300,
            username: "".to_string(),
            android_has_internet_permission: true,
            android_has_access_network_state_permission: true,
        };

        let result = api.save_sync_config(config);
        assert_eq!(result.unwrap(), true);
    }
}
