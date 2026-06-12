#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum PlatformDto {
    #[default]
    Linux,
    Android,
}

impl From<crate::writing_stats::Platform> for PlatformDto {
    fn from(p: crate::writing_stats::Platform) -> Self {
        match p {
            crate::writing_stats::Platform::Linux => Self::Linux,
            crate::writing_stats::Platform::Android => Self::Android,
        }
    }
}

impl From<PlatformDto> for crate::writing_stats::Platform {
    fn from(dto: PlatformDto) -> Self {
        match dto {
            PlatformDto::Linux => Self::Linux,
            PlatformDto::Android => Self::Android,
        }
    }
}
