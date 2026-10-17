#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]

pub enum PlatformDto {
    #[default]
    Desktop,
    Android,
    Windows,
    Harmony,
    Apple,
}

impl From<PlatformDto> for writer_platform_api::PlatformKind {
    fn from(dto: PlatformDto) -> Self {
        match dto {
            PlatformDto::Desktop => Self::Desktop,
            PlatformDto::Android => Self::Android,
            PlatformDto::Windows => Self::Windows,
            PlatformDto::Harmony => Self::Harmony,
            PlatformDto::Apple => Self::Apple,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInitDto {
    pub platform: PlatformDto,
    pub app_data_dir: String,
    pub cache_dir: String,
    pub log_dir: String,
    pub no_backup_dir: Option<String>,
    pub device_id: String,
    pub app_version: String,
    pub locale: String,
    pub timezone: String,
    pub is_connected: bool,
    pub is_metered: bool,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<u16>,
}

impl From<PlatformInitDto> for writer_platform_api::PlatformInit {
    fn from(dto: PlatformInitDto) -> Self {
        Self {
            platform: dto.platform.into(),
            app_data_dir: dto.app_data_dir.into(),
            cache_dir: dto.cache_dir.into(),
            log_dir: dto.log_dir.into(),
            no_backup_dir: dto.no_backup_dir.map(Into::into),
            device_id: dto.device_id,
            app_version: dto.app_version,
            locale: dto.locale,
            timezone: dto.timezone,
        }
    }
}

impl From<PlatformInitDto> for writer_platform_api::NetworkState {
    fn from(dto: PlatformInitDto) -> Self {
        Self {
            is_connected: dto.is_connected,
            is_metered: dto.is_metered,
            proxy_host: dto.proxy_host,
            proxy_port: dto.proxy_port,
        }
    }
}

impl From<crate::writing_stats::Platform> for PlatformDto {
    fn from(p: crate::writing_stats::Platform) -> Self {
        match p {
            crate::writing_stats::Platform::Desktop => Self::Desktop,
            crate::writing_stats::Platform::Android => Self::Android,
            crate::writing_stats::Platform::Windows => Self::Windows,
            crate::writing_stats::Platform::Harmony => Self::Harmony,
            crate::writing_stats::Platform::Apple => Self::Apple,
        }
    }
}
