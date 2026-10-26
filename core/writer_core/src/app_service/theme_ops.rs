use crate::api::BuiltinThemeDto;

impl super::WriterAppService {
    pub fn list_builtin_themes(&self) -> Vec<BuiltinThemeDto> {
        self.api.list_builtin_themes()
    }
}
