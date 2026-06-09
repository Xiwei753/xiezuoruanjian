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
    fn load_local_settings_returns_default_settings_on_empty_workspace() {
        let temp_dir = tempdir().unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        let result = api.load_local_settings();
        assert!(result.is_ok());
        let settings = result.unwrap();

        // Check some default values based on the DTO and core defaults
        assert_eq!(settings.auto_save_enabled, true);
        assert_eq!(settings.ai_enabled, false);
        assert_eq!(settings.editor_typing_animation_enabled, false);
    }

    #[test]
    fn load_local_settings_returns_saved_settings() {
        let temp_dir = tempdir().unwrap();
        let api = WriterCoreApi::new(temp_dir.path());

        // Get defaults first
        let mut settings = api.load_local_settings().unwrap();

        // Modify some settings
        settings.auto_save_enabled = false;
        settings.ai_enabled = false;
        settings.editor_typing_animation_enabled = false;
        settings.window_width = 1024.0;
        settings.window_height = 768.0;

        // Save modified settings
        let save_result = api.save_local_settings(settings);
        assert!(save_result.is_ok());
        assert!(save_result.unwrap());

        // Load again and verify changes persisted
        let loaded_settings = api.load_local_settings().unwrap();
        assert_eq!(loaded_settings.auto_save_enabled, false);
        assert_eq!(loaded_settings.ai_enabled, false);
        assert_eq!(loaded_settings.editor_typing_animation_enabled, false);
        assert_eq!(loaded_settings.window_width, 1024.0);
        assert_eq!(loaded_settings.window_height, 768.0);
    }
}
