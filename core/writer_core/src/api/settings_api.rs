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
        self.core().save_local_settings(&settings.clone().into())?;
        let body = serde_json::to_string(&settings).unwrap_or_default();
        let entry = crate::search::extractor::extract_setting_entry("local", &body);
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        Ok(true)
    }

    pub fn load_syncable_settings(&self) -> ApiResult<SyncableSettingsDto> {
        self.core()
            .load_syncable_settings()
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_syncable_settings(&self, settings: SyncableSettingsDto) -> ApiResult<bool> {
        self.core()
            .save_syncable_settings(&settings.clone().into())?;
        let body = serde_json::to_string(&settings).unwrap_or_default();
        let entry = crate::search::extractor::extract_setting_entry("syncable", &body);
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        Ok(true)
    }

    pub fn ensure_device_info(&self, platform: &str, device_class: &str) -> ApiResult<bool> {
        self.core()
            .ensure_device_info(platform, device_class, None)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn load_device_info(&self) -> ApiResult<DeviceInfoDto> {
        crate::settings::load_device_info(&self.app_data_root)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_palette_record(&self, record: ThemePaletteRecordDto) -> ApiResult<bool> {
        let r: crate::settings::ThemePaletteRecord = record.into();
        crate::settings::save_palette_record(&self.app_data_root, &r)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn load_palette_record(
        &self,
        device_id: &str,
        fingerprint: &str,
    ) -> ApiResult<ThemePaletteRecordDto> {
        crate::settings::load_palette_record(&self.app_data_root, device_id, fingerprint)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn list_palette_records(&self) -> ApiResult<Vec<ThemePaletteRecordDto>> {
        crate::settings::list_palette_records(&self.app_data_root)
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn delete_palette_record(&self, device_id: &str, fingerprint: &str) -> ApiResult<bool> {
        crate::settings::delete_palette_record(&self.app_data_root, device_id, fingerprint)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn migrate_legacy_theme_palette(&self) -> ApiResult<bool> {
        crate::settings::migrate_legacy_theme_palette(&self.app_data_root).map_err(Into::into)
    }

    pub fn compute_palette_fingerprint(
        &self,
        light_scheme: ThemeColorSchemeDto,
        dark_scheme: ThemeColorSchemeDto,
    ) -> String {
        let light: crate::settings::ThemeColorScheme = light_scheme.into();
        let dark: crate::settings::ThemeColorScheme = dark_scheme.into();
        crate::settings::compute_palette_fingerprint(&light, &dark)
    }

    pub fn list_builtin_themes(&self) -> Vec<BuiltinThemeDto> {
        crate::settings::list_builtin_themes()
            .into_iter()
            .map(Into::into)
            .collect()
    }
}
