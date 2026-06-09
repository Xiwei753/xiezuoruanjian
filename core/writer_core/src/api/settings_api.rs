use super::service::{ApiResult, WriterCoreApi};
use super::types::*;
use super::{ChangedEntityDto, ResultEnvelope};

impl WriterCoreApi {
    fn settings_saved_envelope(result: ApiResult<bool>, path: &str) -> ResultEnvelope<bool> {
        match result {
            Ok(data) => ResultEnvelope::success_with_changes(
                data,
                vec![path.to_string()],
                vec![ChangedEntityDto {
                    entity_type: "SettingsSaved".to_string(),
                    entity_id: None,
                }],
            ),
            Err(error) => ResultEnvelope::error(error),
        }
    }

    pub fn load_local_settings(&self) -> ApiResult<LocalSettingsDto> {
        self.core()
            .load_local_settings()
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_local_settings(&self, settings: LocalSettingsDto) -> ApiResult<bool> {
        self.core()
            .save_local_settings(&settings.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn save_local_settings_envelope_json(&self, settings: LocalSettingsDto) -> String {
        Self::settings_saved_envelope(self.save_local_settings(settings), "settings.local.json")
            .to_json_string()
    }

    pub fn load_syncable_settings(&self) -> ApiResult<SyncableSettingsDto> {
        self.core()
            .load_syncable_settings()
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_syncable_settings(&self, settings: SyncableSettingsDto) -> ApiResult<bool> {
        self.core()
            .save_syncable_settings(&settings.into())
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn save_syncable_settings_envelope_json(&self, settings: SyncableSettingsDto) -> String {
        Self::settings_saved_envelope(
            self.save_syncable_settings(settings),
            "settings.syncable.json",
        )
        .to_json_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_load_local_settings() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let result = api.load_local_settings();
        assert!(result.is_ok());

        let settings = result.unwrap();
        // Since no settings file exists yet, it should return default values
        assert_eq!(settings.theme_mode.as_deref(), Some("system"));
    }

    #[test]
    fn test_save_and_load_local_settings() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let mut settings = api.load_local_settings().unwrap();
        settings.theme_mode = Some("dark".to_string());
        settings.editor_font_size = 20.0;

        let save_result = api.save_local_settings(settings.clone());
        assert!(save_result.is_ok());
        assert!(save_result.unwrap());

        let loaded_settings = api.load_local_settings().unwrap();
        assert_eq!(loaded_settings.theme_mode.as_deref(), Some("dark"));
        assert_eq!(loaded_settings.editor_font_size, 20.0);
    }
}
