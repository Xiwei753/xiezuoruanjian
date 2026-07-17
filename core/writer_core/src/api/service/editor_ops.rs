use crate::api::service::{ApiResult, WriterCoreApi};
use crate::api::types::*;
use crate::platform_interaction::PlatformKind;

impl WriterCoreApi {
    pub fn get_platform_capabilities(
        &self,
        platform: PlatformKindDto,
    ) -> ApiResult<PlatformCapabilitiesDto> {
        let kind: PlatformKind = platform.into();
        let caps = kind.default_capabilities();
        Ok(caps.into())
    }
}

impl From<PlatformKindDto> for PlatformKind {
    fn from(dto: PlatformKindDto) -> Self {
        match dto {
            PlatformKindDto::LinuxQt => Self::LinuxQt,
            PlatformKindDto::Android => Self::Android,
            PlatformKindDto::Windows => Self::Windows,
            PlatformKindDto::Harmony => Self::Harmony,
            PlatformKindDto::Unknown => Self::Unknown,
        }
    }
}
