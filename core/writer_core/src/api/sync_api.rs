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

    // --- Sync envelope_json methods ---

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

    pub fn save_sync_config_envelope_json(&self, config: SyncConfigDto) -> String {
        Self::sync_saved_envelope(self.save_sync_config(config), "sync_config.json")
            .to_json_string()
    }

    pub fn save_sync_secrets_envelope_json(&self, secrets: SyncSecretsDto) -> String {
        Self::sync_saved_envelope(self.save_sync_secrets(secrets), "sync_secrets.local.json")
            .to_json_string()
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
                        user_message: Some("同步冲突，请手动处理冲突文件后重试".to_string()),
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
                        user_message: Some("同步失败，请检查网络和配置".to_string()),
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

    pub fn perform_sync_envelope_json(&self, config: SyncConfigDto) -> String {
        Self::sync_result_envelope(self.perform_sync(config)).to_json_string()
    }

    pub fn perform_sync_dry_run_envelope_json(&self, config: SyncConfigDto) -> String {
        Self::sync_plan_envelope(self.perform_sync_dry_run(config)).to_json_string()
    }

    pub fn perform_sync_diagnostics_envelope_json(&self, config: SyncConfigDto) -> String {
        Self::sync_diagnostics_envelope(self.perform_sync_diagnostics(config)).to_json_string()
    }
}
#[cfg(test)]
mod tests {
    use crate::api::service::WriterCoreApi;
    use tempfile::tempdir;

    #[test]
    fn test_load_sync_config() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let config = api.load_sync_config().unwrap();

        assert_eq!(config.enabled, false);
        assert_eq!(config.backend_type, "github_api");
        assert_eq!(config.remote_url, "");
        assert_eq!(config.transport, "https_token");
        assert_eq!(config.branch, "main");
        assert_eq!(config.auto_sync, false);
        assert_eq!(config.sync_interval_seconds, 300);
        assert_eq!(config.proxy_enabled, false);
        assert_eq!(config.proxy_type, "auto");
        assert_eq!(config.proxy_host, "127.0.0.1");
        assert_eq!(config.proxy_port, 7890);
        assert_eq!(config.username, "");
    }
}
