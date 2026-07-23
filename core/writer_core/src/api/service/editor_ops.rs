use crate::api::service::{ApiResult, WriterCoreApi};
use crate::api::types::*;
use crate::platform_interaction::PlatformCapabilitiesExt;

impl WriterCoreApi {
    pub fn get_platform_capabilities(
        &self,
        platform: PlatformKindDto,
    ) -> ApiResult<PlatformCapabilitiesDto> {
        let kind: writer_platform_api::PlatformKind = platform.into();
        let caps = kind.default_capabilities();
        Ok(caps.into())
    }
}

impl From<PlatformKindDto> for writer_platform_api::PlatformKind {
    fn from(dto: PlatformKindDto) -> Self {
        match dto {
            PlatformKindDto::Desktop => Self::Desktop,
            PlatformKindDto::Android => Self::Android,
            PlatformKindDto::Windows => Self::Windows,
            PlatformKindDto::Harmony => Self::Harmony,
            PlatformKindDto::Apple => Self::Apple,
        }
    }
}
