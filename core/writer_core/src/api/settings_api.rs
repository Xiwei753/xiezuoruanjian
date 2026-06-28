use super::service::{ApiResult, WriterCoreApi};
use super::types::*;

impl WriterCoreApi {
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
}
