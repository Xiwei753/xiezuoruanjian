use crate::error::Result;
use crate::settings::{self, LocalSettings, SyncableSettings};

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
}
