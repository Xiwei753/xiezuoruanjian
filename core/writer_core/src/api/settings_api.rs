use super::service::{ApiResult, WriterCoreApi};
use super::types::*;

impl WriterCoreApi {
    pub fn load_local_settings(&self) -> ApiResult<LocalSettingsDto> {
        crate::settings::load_local_settings(&self.app_data_root)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_local_settings(&self, settings: LocalSettingsDto) -> ApiResult<bool> {
        //   用 _with_changes 版本拿变更集，
        // 调 record_workspace_change_set_history 记录本地历史。
        let change_set = self
            .core_write()
            .save_local_settings_with_changes(&settings.clone().into())?;
        let body = serde_json::to_string(&settings).unwrap_or_default();
        for entry in crate::search::extractor::extract_settings_from_json("local", &body) {
            self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                action: crate::search::SearchIndexAction::Upsert,
                object_id: entry.object_id.clone(),
                scope: entry.scope,
                title: entry.title.clone(),
                body: entry.body.clone(),
                target: Some(entry.target.clone()),
            });
        }
        let _ = self.record_workspace_change_set_history(&change_set, "save_local_settings");
        Ok(true)
    }

    pub fn load_syncable_settings(&self) -> ApiResult<SyncableSettingsDto> {
        crate::settings::load_syncable_settings(&self.app_data_root)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_syncable_settings(&self, settings: SyncableSettingsDto) -> ApiResult<bool> {
        //   用 _with_changes 版本拿变更集。
        let change_set = self
            .core_write()
            .save_syncable_settings_with_changes(&settings.clone().into())?;
        let body = serde_json::to_string(&settings).unwrap_or_default();
        for entry in crate::search::extractor::extract_settings_from_json("sync", &body) {
            self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                action: crate::search::SearchIndexAction::Upsert,
                object_id: entry.object_id.clone(),
                scope: entry.scope,
                title: entry.title.clone(),
                body: entry.body.clone(),
                target: Some(entry.target.clone()),
            });
        }
        let _ = self.record_workspace_change_set_history(&change_set, "save_syncable_settings");
        Ok(true)
    }

    pub fn ensure_device_info(&self, platform: &str, device_class: &str) -> ApiResult<bool> {
        self.core_write()
            .ensure_device_info(platform, device_class, None)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn load_device_info(&self) -> ApiResult<DeviceInfoDto> {
        crate::settings::load_device_info(&self.app_data_root)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn list_builtin_themes(&self) -> Vec<BuiltinThemeDto> {
        crate::settings::list_builtin_themes()
            .into_iter()
            .map(Into::into)
            .collect()
    }
}
