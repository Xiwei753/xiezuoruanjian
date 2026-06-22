use super::service::{ApiResult, WriterCoreApi};
use super::types::*;
use super::{ChangedEntityDto, ResultEnvelope};

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

    // --- Sync envelope helpers (internal) ---

    fn sync_saved_envelope(result: ApiResult<bool>, path: &str) -> ResultEnvelope<bool> {
        match result {
            Ok(data) => ResultEnvelope::success_with_changes(
                data,
                vec![path.to_string()],
                vec![ChangedEntityDto {
                    entity_type: "SyncConfigSaved".to_string(),
                    entity_id: None,
                }],
            ),
            Err(error) => ResultEnvelope::error(error),
        }
    }

    fn sync_result_envelope(result: ApiResult<SyncResultDto>) -> ResultEnvelope<SyncResultDto> {
        match result {
            Ok(dto) => {
                let status = dto.status.clone();
                let is_success = matches!(
                    status.as_str(),
                    "success" | "no_changes" | "latest_wins_applied" | "branch_missing_recovered"
                );
                let is_conflict = status == "conflict";

                if is_success {
                    let mut changed_paths = Vec::new();
                    changed_paths.extend(dto.uploaded_files.clone());
                    changed_paths.extend(dto.downloaded_files.clone());
                    ResultEnvelope::success_with_changes(
                        dto,
                        changed_paths,
                        vec![ChangedEntityDto {
                            entity_type: "SyncCompleted".to_string(),
                            entity_id: None,
                        }],
                    )
                } else if is_conflict {
                    let conflict_files: Vec<String> =
                        dto.conflicts.iter().map(|c| c.local_path.clone()).collect();
                    let detail = if conflict_files.is_empty() {
                        "checkout_conflict".to_string()
                    } else {
                        format!("checkout_conflict: {}", conflict_files.join(", "))
                    };
                    ResultEnvelope {
                        success: false,
                        data: Some(dto),
                        error_code: Some("SYNC_CONFLICT".to_string()),
                        message_key: Some("error.sync_conflict".to_string()),
                        message_args: None,
                        user_message: None,
                        raw_error: Some(detail),
                        warnings: Vec::new(),
                        changed_paths: Vec::new(),
                        changed_entities: Vec::new(),
                    }
                } else {
                    let error_msg = dto
                        .error
                        .clone()
                        .unwrap_or_else(|| format!("sync status: {}", status));
                    let error_code =
                        dto.error_category
                            .clone()
                            .unwrap_or_else(|| match status.as_str() {
                                "dirty_repo_blocked" => "DIRTY_REPO_BLOCKED".to_string(),
                                "fatal_error" => "SYNC_FATAL_ERROR".to_string(),
                                "recoverable_error" => "SYNC_RECOVERABLE_ERROR".to_string(),
                                _ => "SYNC_FAILED".to_string(),
                            });
                    ResultEnvelope {
                        success: false,
                        data: Some(dto),
                        error_code: Some(error_code),
                        message_key: Some("error.sync_failed".to_string()),
                        message_args: None,
                        user_message: None,
                        raw_error: Some(error_msg),
                        warnings: Vec::new(),
                        changed_paths: Vec::new(),
                        changed_entities: Vec::new(),
                    }
                }
            }
            Err(error) => ResultEnvelope::error(error),
        }
    }

    fn sync_plan_envelope(result: ApiResult<SyncPlanDto>) -> ResultEnvelope<SyncPlanDto> {
        match result {
            Ok(dto) => ResultEnvelope::success(dto),
            Err(error) => ResultEnvelope::error(error),
        }
    }

    fn sync_diagnostics_envelope(
        result: ApiResult<SyncDiagnosticsResultDto>,
    ) -> ResultEnvelope<SyncDiagnosticsResultDto> {
        match result {
            Ok(dto) => ResultEnvelope::success(dto),
            Err(error) => ResultEnvelope::error(error),
        }
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
