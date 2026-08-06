use crate::error::Result;
use crate::settings::{self, DeviceInfo, LocalSettings, SyncableSettings};

impl super::WriterCore {
    pub fn load_local_settings(&self) -> Result<LocalSettings> {
        settings::load_local_settings(&self.workspace_path)
    }

    pub fn save_local_settings(&self, settings: &LocalSettings) -> Result<()> {
        settings::save_local_settings(&self.workspace_path, settings)
    }

    pub fn load_syncable_settings(&self) -> Result<SyncableSettings> {
        settings::load_syncable_settings(&self.workspace_path)
    }

    pub fn save_syncable_settings(&self, settings: &SyncableSettings) -> Result<()> {
        settings::save_syncable_settings(&self.workspace_path, settings)
    }

    pub fn load_device_info(&self) -> Result<DeviceInfo> {
        settings::load_device_info(&self.workspace_path)
    }

    pub fn save_device_info(&self, info: &DeviceInfo) -> Result<()> {
        settings::save_device_info(&self.workspace_path, info)
    }

    pub fn ensure_device_info(
        &self,
        platform: &str,
        device_class: &str,
        preferred_device_id: Option<&str>,
    ) -> Result<DeviceInfo> {
        settings::ensure_device_info(
            &self.workspace_path,
            platform,
            device_class,
            preferred_device_id,
        )
    }

    pub fn list_registered_settings(&self) -> crate::settings_registry::SettingsRegistry {
        crate::settings_registry::SettingsRegistry::default_registry()
    }

    pub fn get_settings_presentation(&self) -> crate::settings_presentation::SettingsPresentation {
        crate::settings_presentation::default_settings_presentation()
    }

    /// 返回 SettingsPresentation 的 JSON 字符串，方便客户端通过 FFI 消费
    pub fn get_settings_presentation_json(&self) -> String {
        let presentation = crate::settings_presentation::default_settings_presentation();
        serde_json::to_string(&presentation).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn list_palette_records(&self) -> Result<Vec<crate::settings::ThemePaletteRecord>> {
        crate::settings::list_palette_records(&self.workspace_path)
    }

    pub fn load_palette_record(
        &self,
        device_id: &str,
        fingerprint: &str,
    ) -> Result<crate::settings::ThemePaletteRecord> {
        crate::settings::load_palette_record(&self.workspace_path, device_id, fingerprint)
    }

    pub fn delete_palette_record(&self, device_id: &str, fingerprint: &str) -> Result<()> {
        crate::settings::delete_palette_record(&self.workspace_path, device_id, fingerprint)
    }

    pub fn list_builtin_themes(&self) -> Vec<crate::settings::BuiltinTheme> {
        crate::settings::list_builtin_themes()
    }
}
