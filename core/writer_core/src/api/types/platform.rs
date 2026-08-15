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

// ── Layout Contract DTOs（#628：WindowCapabilitiesDto → WindowViewportDto） ──

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum ShellModeDto {
    #[default]
    SinglePane,
    SupportingPane,
    TwoPane,
    ThreePane,
}

impl From<crate::presentation::layout::ShellMode> for ShellModeDto {
    fn from(s: crate::presentation::layout::ShellMode) -> Self {
        match s {
            crate::presentation::layout::ShellMode::SinglePane => Self::SinglePane,
            crate::presentation::layout::ShellMode::SupportingPane => Self::SupportingPane,
            crate::presentation::layout::ShellMode::TwoPane => Self::TwoPane,
            crate::presentation::layout::ShellMode::ThreePane => Self::ThreePane,
        }
    }
}

impl From<ShellModeDto> for crate::presentation::layout::ShellMode {
    fn from(dto: ShellModeDto) -> Self {
        match dto {
            ShellModeDto::SinglePane => Self::SinglePane,
            ShellModeDto::SupportingPane => Self::SupportingPane,
            ShellModeDto::TwoPane => Self::TwoPane,
            ShellModeDto::ThreePane => Self::ThreePane,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum WorkspacePaneModeDto {
    #[default]
    SinglePane,
    ListDetail,
    ThreePane,
}

impl From<crate::presentation::layout::WorkspacePaneMode> for WorkspacePaneModeDto {
    fn from(w: crate::presentation::layout::WorkspacePaneMode) -> Self {
        match w {
            crate::presentation::layout::WorkspacePaneMode::SinglePane => Self::SinglePane,
            crate::presentation::layout::WorkspacePaneMode::ListDetail => Self::ListDetail,
            crate::presentation::layout::WorkspacePaneMode::ThreePane => Self::ThreePane,
        }
    }
}

impl From<WorkspacePaneModeDto> for crate::presentation::layout::WorkspacePaneMode {
    fn from(dto: WorkspacePaneModeDto) -> Self {
        match dto {
            WorkspacePaneModeDto::SinglePane => Self::SinglePane,
            WorkspacePaneModeDto::ListDetail => Self::ListDetail,
            WorkspacePaneModeDto::ThreePane => Self::ThreePane,
        }
    }
}

/// #628 评论第 4 节：一级导航放置位置（平台无关）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum PrimaryNavigationPlacementDto {
    #[default]
    Bottom,
    Side,
}

impl From<crate::presentation::layout::PrimaryNavigationPlacement>
    for PrimaryNavigationPlacementDto
{
    fn from(p: crate::presentation::layout::PrimaryNavigationPlacement) -> Self {
        match p {
            crate::presentation::layout::PrimaryNavigationPlacement::Bottom => Self::Bottom,
            crate::presentation::layout::PrimaryNavigationPlacement::Side => Self::Side,
        }
    }
}

impl From<PrimaryNavigationPlacementDto>
    for crate::presentation::layout::PrimaryNavigationPlacement
{
    fn from(dto: PrimaryNavigationPlacementDto) -> Self {
        match dto {
            PrimaryNavigationPlacementDto::Bottom => Self::Bottom,
            PrimaryNavigationPlacementDto::Side => Self::Side,
        }
    }
}

/// #628 评论第 6 节：共用布局尺寸 DTO。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayoutMetricsDto {
    pub list_pane_width_dp: f32,
}

impl Default for LayoutMetricsDto {
    fn default() -> Self {
        Self {
            list_pane_width_dp: 320.0,
        }
    }
}

impl From<crate::presentation::layout::metrics::LayoutMetrics> for LayoutMetricsDto {
    fn from(m: crate::presentation::layout::metrics::LayoutMetrics) -> Self {
        Self {
            list_pane_width_dp: m.list_pane_width_dp,
        }
    }
}

impl From<LayoutMetricsDto> for crate::presentation::layout::metrics::LayoutMetrics {
    fn from(dto: LayoutMetricsDto) -> Self {
        Self {
            list_pane_width_dp: dto.list_pane_width_dp,
        }
    }
}

/// #628：原始窗口尺寸 DTO，替代旧的 WindowCapabilitiesDto。
///
/// 平台端只传宽高（dp），不再传 paneCount / has_separating_fold /
/// pointer_class / keyboard_visible。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowViewportDto {
    pub width_dp: f32,
    pub height_dp: f32,
}

impl Default for WindowViewportDto {
    fn default() -> Self {
        // 默认按窄窗口（手机竖屏）算，与 Core 内部 WindowViewport::default() 对齐。
        Self {
            width_dp: 360.0,
            height_dp: 640.0,
        }
    }
}

impl From<crate::presentation::layout::resolver::WindowViewport> for WindowViewportDto {
    fn from(v: crate::presentation::layout::resolver::WindowViewport) -> Self {
        Self {
            width_dp: v.width_dp,
            height_dp: v.height_dp,
        }
    }
}

impl From<WindowViewportDto> for crate::presentation::layout::resolver::WindowViewport {
    fn from(dto: WindowViewportDto) -> Self {
        Self {
            width_dp: dto.width_dp,
            height_dp: dto.height_dp,
        }
    }
}

/// #628：LayoutContractDto 删除 `show_primary_navigation`（改由 ScreenPolicy 提供），
/// 新增 `primary_navigation_placement` 与 `metrics`。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayoutContractDto {
    pub shell_mode: ShellModeDto,
    pub workspace_pane_mode: WorkspacePaneModeDto,
    pub primary_navigation_placement: PrimaryNavigationPlacementDto,
    pub metrics: LayoutMetricsDto,
}

impl From<crate::presentation::layout::LayoutContract> for LayoutContractDto {
    fn from(c: crate::presentation::layout::LayoutContract) -> Self {
        Self {
            shell_mode: c.shell_mode.into(),
            workspace_pane_mode: c.workspace_pane_mode.into(),
            primary_navigation_placement: c.primary_navigation_placement.into(),
            metrics: c.metrics.into(),
        }
    }
}

impl From<LayoutContractDto> for crate::presentation::layout::LayoutContract {
    fn from(dto: LayoutContractDto) -> Self {
        Self {
            shell_mode: dto.shell_mode.into(),
            workspace_pane_mode: dto.workspace_pane_mode.into(),
            primary_navigation_placement: dto.primary_navigation_placement.into(),
            metrics: dto.metrics.into(),
        }
    }
}

// ========== 单元测试 ==========

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_viewport_dto_roundtrip() {
        let viewport = crate::presentation::layout::resolver::WindowViewport {
            width_dp: 1024.0,
            height_dp: 768.0,
        };
        let dto: WindowViewportDto = viewport.into();
        let back: crate::presentation::layout::resolver::WindowViewport = dto.into();
        assert_eq!(back.width_dp, viewport.width_dp);
        assert_eq!(back.height_dp, viewport.height_dp);
    }

    #[test]
    fn test_window_viewport_dto_camel_case_fields() {
        let dto = WindowViewportDto {
            width_dp: 360.0,
            height_dp: 640.0,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"widthDp\""));
        assert!(json.contains("\"heightDp\""));
        // #628：不得再出现旧字段。
        assert!(!json.contains("availablePaneCount"));
        assert!(!json.contains("hasSeparatingFold"));
        assert!(!json.contains("pointerClass"));
        assert!(!json.contains("keyboardVisible"));
    }

    #[test]
    fn test_primary_navigation_placement_dto_roundtrip() {
        for p in [
            crate::presentation::layout::PrimaryNavigationPlacement::Bottom,
            crate::presentation::layout::PrimaryNavigationPlacement::Side,
        ] {
            let dto: PrimaryNavigationPlacementDto = p.into();
            let back: crate::presentation::layout::PrimaryNavigationPlacement = dto.into();
            assert_eq!(back, p);
        }
    }

    #[test]
    fn test_layout_metrics_dto_roundtrip() {
        let m = crate::presentation::layout::metrics::LayoutMetrics {
            list_pane_width_dp: 320.0,
        };
        let dto: LayoutMetricsDto = m.into();
        let back: crate::presentation::layout::metrics::LayoutMetrics = dto.into();
        assert_eq!(back.list_pane_width_dp, m.list_pane_width_dp);
    }

    #[test]
    fn test_layout_metrics_dto_camel_case_fields() {
        let dto = LayoutMetricsDto {
            list_pane_width_dp: 320.0,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"listPaneWidthDp\""));
    }

    #[test]
    fn test_layout_contract_dto_roundtrip() {
        let contract = crate::presentation::layout::LayoutContract {
            shell_mode: crate::presentation::layout::ShellMode::TwoPane,
            workspace_pane_mode: crate::presentation::layout::WorkspacePaneMode::ListDetail,
            primary_navigation_placement:
                crate::presentation::layout::PrimaryNavigationPlacement::Side,
            metrics: crate::presentation::layout::metrics::LayoutMetrics {
                list_pane_width_dp: 320.0,
            },
        };
        let dto: LayoutContractDto = contract.clone().into();
        let back: crate::presentation::layout::LayoutContract = dto.into();
        assert_eq!(back.shell_mode, contract.shell_mode);
        assert_eq!(back.workspace_pane_mode, contract.workspace_pane_mode);
        assert_eq!(
            back.primary_navigation_placement,
            contract.primary_navigation_placement
        );
        assert_eq!(
            back.metrics.list_pane_width_dp,
            contract.metrics.list_pane_width_dp
        );
    }

    #[test]
    fn test_layout_contract_dto_no_legacy_fields() {
        // #628：LayoutContractDto 不得再含 showPrimaryNavigation（改由 ScreenPolicy 提供）。
        let contract = crate::presentation::layout::LayoutContract {
            shell_mode: crate::presentation::layout::ShellMode::SinglePane,
            workspace_pane_mode: crate::presentation::layout::WorkspacePaneMode::SinglePane,
            primary_navigation_placement:
                crate::presentation::layout::PrimaryNavigationPlacement::Bottom,
            metrics: crate::presentation::layout::metrics::LayoutMetrics::default(),
        };
        let dto: LayoutContractDto = contract.into();
        let json = serde_json::to_string(&dto).unwrap();
        assert!(!json.contains("showPrimaryNavigation"));
        assert!(json.contains("\"primaryNavigationPlacement\""));
        assert!(json.contains("\"metrics\""));
    }

    #[test]
    fn test_resolve_layout_end_to_end_through_dto() {
        // 端到端：WindowViewportDto → Core → LayoutContractDto。
        let dto = WindowViewportDto {
            width_dp: 1000.0,
            height_dp: 800.0,
        };
        let viewport: crate::presentation::layout::resolver::WindowViewport = dto.into();
        let contract = crate::presentation::layout::resolve_layout(&viewport);
        let contract_dto: LayoutContractDto = contract.into();
        assert_eq!(contract_dto.shell_mode, ShellModeDto::TwoPane);
        assert_eq!(
            contract_dto.workspace_pane_mode,
            WorkspacePaneModeDto::ListDetail
        );
        assert_eq!(
            contract_dto.primary_navigation_placement,
            PrimaryNavigationPlacementDto::Side
        );
        assert_eq!(contract_dto.metrics.list_pane_width_dp, 320.0);
    }
}
