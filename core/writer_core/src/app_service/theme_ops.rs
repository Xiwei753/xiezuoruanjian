use crate::api::{BuiltinThemeDto, ThemeColorSchemeDto, ThemePaletteRecordDto, WriterError};

impl super::WriterAppService {
    pub fn save_palette_record(&self, record: ThemePaletteRecordDto) -> Result<bool, WriterError> {
        self.api.save_palette_record(record)
    }

    pub fn load_palette_record(
        &self,
        device_id: String,
        fingerprint: String,
    ) -> Result<ThemePaletteRecordDto, WriterError> {
        self.api.load_palette_record(&device_id, &fingerprint)
    }

    pub fn list_palette_records(&self) -> Result<Vec<ThemePaletteRecordDto>, WriterError> {
        self.api.list_palette_records()
    }

    pub fn delete_palette_record(
        &self,
        device_id: String,
        fingerprint: String,
    ) -> Result<bool, WriterError> {
        self.api.delete_palette_record(&device_id, &fingerprint)
    }

    pub fn migrate_legacy_theme_palette(&self) -> Result<bool, WriterError> {
        self.api.migrate_legacy_theme_palette()
    }

    pub fn compute_palette_fingerprint(
        &self,
        light_scheme: ThemeColorSchemeDto,
        dark_scheme: ThemeColorSchemeDto,
    ) -> String {
        self.api
            .compute_palette_fingerprint(light_scheme, dark_scheme)
    }

    pub fn list_builtin_themes(&self) -> Vec<BuiltinThemeDto> {
        self.api.list_builtin_themes()
    }
}
