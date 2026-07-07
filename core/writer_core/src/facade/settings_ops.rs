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

    pub fn ensure_device_info(&self, platform: &str, device_class: &str) -> Result<DeviceInfo> {
        settings::ensure_device_info(&self.workspace_path, platform, device_class)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_settings_ops_all() {
        let temp_dir = tempdir().unwrap();
        let core = super::super::WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        // 1. local settings
        let mut local_settings = core.load_local_settings().unwrap();
        local_settings.window_width = 1200.0;
        core.save_local_settings(&local_settings).unwrap();
        let loaded_local = core.load_local_settings().unwrap();
        assert_eq!(loaded_local.window_width, 1200.0);

        // 2. syncable settings
        let mut syncable_settings = core.load_syncable_settings().unwrap();
        syncable_settings.font_size = 24.0;
        core.save_syncable_settings(&syncable_settings).unwrap();
        let loaded_syncable = core.load_syncable_settings().unwrap();
        assert_eq!(loaded_syncable.font_size, 24.0);

        // 3. device info
        let device_info = core.ensure_device_info("test_platform", "test_class").unwrap();
        assert_eq!(device_info.platform, "test_platform");
        assert_eq!(device_info.device_class, "test_class");

        let loaded_device_info = core.load_device_info().unwrap();
        assert_eq!(loaded_device_info.device_id, device_info.device_id);

        let mut mod_device_info = loaded_device_info.clone();
        mod_device_info.platform = "modified_platform".to_string();
        core.save_device_info(&mod_device_info).unwrap();

        let reloaded_device_info = core.load_device_info().unwrap();
        assert_eq!(reloaded_device_info.platform, "modified_platform");

        // 4. settings registry & presentation
        let registry = core.list_registered_settings();
        assert!(!registry.items.is_empty());

        let presentation = core.get_settings_presentation();
        assert!(!presentation.sections.is_empty());

        let presentation_json = core.get_settings_presentation_json();
        assert!(presentation_json.starts_with('{'));
        assert!(presentation_json.contains("sections"));
    }
}
