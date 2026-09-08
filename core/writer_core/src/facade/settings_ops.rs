use crate::error::Result;
use crate::settings::{self, DeviceInfo, LocalSettings, SyncableSettings};
use crate::storage::workspace_git::WorkspaceChangeSet;

impl super::WriterCore {
    pub fn load_local_settings(&self) -> Result<LocalSettings> {
        settings::load_local_settings(&self.app_data_root)
    }

    pub fn save_local_settings(&self, settings: &LocalSettings) -> Result<()> {
        settings::save_local_settings(&self.app_data_root, settings)
    }

    pub fn load_syncable_settings(&self) -> Result<SyncableSettings> {
        settings::load_syncable_settings(&self.app_data_root)
    }

    pub fn save_syncable_settings(&self, settings: &SyncableSettings) -> Result<()> {
        settings::save_syncable_settings(&self.app_data_root, settings)
    }

    // #645 评论 5504296097 问题3：settings 的 *_with_changes 转发，
    // 返回 WorkspaceChangeSet 供 API 层记录本地历史。

    /// 保存本地设置并返回变更集。
    pub fn save_local_settings_with_changes(
        &self,
        settings: &LocalSettings,
    ) -> Result<WorkspaceChangeSet> {
        settings::save_local_settings_with_changes(&self.app_data_root, settings)
    }

    /// 保存可同步设置并返回变更集。
    pub fn save_syncable_settings_with_changes(
        &self,
        settings: &SyncableSettings,
    ) -> Result<WorkspaceChangeSet> {
        settings::save_syncable_settings_with_changes(&self.app_data_root, settings)
    }

    /// 保存 palette 记录并返回变更集。
    pub fn save_palette_record_with_changes(
        &self,
        record: &crate::settings::ThemePaletteRecord,
    ) -> Result<WorkspaceChangeSet> {
        settings::save_palette_record_with_changes(&self.app_data_root, record)
    }

    /// 删除 palette 记录并返回变更集。
    pub fn delete_palette_record_with_changes(
        &self,
        device_id: &str,
        fingerprint: &str,
    ) -> Result<WorkspaceChangeSet> {
        settings::delete_palette_record_with_changes(&self.app_data_root, device_id, fingerprint)
    }

    pub fn load_device_info(&self) -> Result<DeviceInfo> {
        settings::load_device_info(&self.app_data_root)
    }

    pub fn save_device_info(&self, info: &DeviceInfo) -> Result<()> {
        settings::save_device_info(&self.app_data_root, info)
    }

    pub fn ensure_device_info(
        &self,
        platform: &str,
        device_class: &str,
        preferred_device_id: Option<&str>,
    ) -> Result<DeviceInfo> {
        settings::ensure_device_info(
            &self.app_data_root,
            platform,
            device_class,
            preferred_device_id,
        )
    }

    pub fn list_registered_settings(&self) -> crate::settings_registry::SettingsRegistry {
        crate::settings_registry::SettingsRegistry::default_registry()
    }

    pub fn get_settings_presentation(&self) -> crate::presentation::settings::SettingsPresentation {
        crate::presentation::settings::default_settings_presentation()
    }

    /// 返回 SettingsPresentation 的 JSON 字符串，方便客户端通过 FFI 消费
    pub fn get_settings_presentation_json(&self) -> String {
        let presentation = crate::presentation::settings::default_settings_presentation();
        serde_json::to_string(&presentation).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn list_palette_records(&self) -> Result<Vec<crate::settings::ThemePaletteRecord>> {
        crate::settings::list_palette_records(&self.app_data_root)
    }

    pub fn load_palette_record(
        &self,
        device_id: &str,
        fingerprint: &str,
    ) -> Result<crate::settings::ThemePaletteRecord> {
        crate::settings::load_palette_record(&self.app_data_root, device_id, fingerprint)
    }

    pub fn delete_palette_record(&self, device_id: &str, fingerprint: &str) -> Result<()> {
        crate::settings::delete_palette_record(&self.app_data_root, device_id, fingerprint)
    }

    pub fn list_builtin_themes(&self) -> Vec<crate::settings::BuiltinTheme> {
        crate::settings::list_builtin_themes()
    }
}
