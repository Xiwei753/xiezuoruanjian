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

/// 工作区布局模式 DTO（#628 验收点 1）。
///
/// 不再输出旧的 `ListDetail` / `ThreePane`，改为产品语义 `SinglePane` / `Workbench`。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum WorkspaceLayoutModeDto {
    #[default]
    SinglePane,
    Workbench,
}

impl From<crate::presentation::layout::WorkspaceLayoutMode> for WorkspaceLayoutModeDto {
    fn from(w: crate::presentation::layout::WorkspaceLayoutMode) -> Self {
        match w {
            crate::presentation::layout::WorkspaceLayoutMode::SinglePane => Self::SinglePane,
            crate::presentation::layout::WorkspaceLayoutMode::Workbench => Self::Workbench,
        }
    }
}

impl From<WorkspaceLayoutModeDto> for crate::presentation::layout::WorkspaceLayoutMode {
    fn from(dto: WorkspaceLayoutModeDto) -> Self {
        match dto {
            WorkspaceLayoutModeDto::SinglePane => Self::SinglePane,
            WorkspaceLayoutModeDto::Workbench => Self::Workbench,
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

/// #628 评论第 6 节 / 验收点 4：共用布局尺寸 DTO。
///
/// 把新出现的结构尺寸继续收回 `LayoutMetricsDto`，避免平台端各自硬编码。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayoutMetricsDto {
    pub list_pane_width_dp: f32,
    /// 作品卡最小宽度，dp。原 Android `180.dp`。
    pub project_card_min_width_dp: f32,
    /// 工作台工具栏宽度，dp。原 Android `240.dp`。
    pub tool_pane_width_dp: f32,
    /// 工具 rail 宽度，dp。原 Android `56.dp`。
    pub tool_rail_width_dp: f32,
}

impl Default for LayoutMetricsDto {
    fn default() -> Self {
        Self {
            list_pane_width_dp: 320.0,
            project_card_min_width_dp: 180.0,
            tool_pane_width_dp: 240.0,
            tool_rail_width_dp: 56.0,
        }
    }
}

impl From<crate::presentation::layout::metrics::LayoutMetrics> for LayoutMetricsDto {
    fn from(m: crate::presentation::layout::metrics::LayoutMetrics) -> Self {
        Self {
            list_pane_width_dp: m.list_pane_width_dp,
            project_card_min_width_dp: m.project_card_min_width_dp,
            tool_pane_width_dp: m.tool_pane_width_dp,
            tool_rail_width_dp: m.tool_rail_width_dp,
        }
    }
}

impl From<LayoutMetricsDto> for crate::presentation::layout::metrics::LayoutMetrics {
    fn from(dto: LayoutMetricsDto) -> Self {
        Self {
            list_pane_width_dp: dto.list_pane_width_dp,
            project_card_min_width_dp: dto.project_card_min_width_dp,
            tool_pane_width_dp: dto.tool_pane_width_dp,
            tool_rail_width_dp: dto.tool_rail_width_dp,
        }
    }
}

/// #628 验收点 5：平台中立的窗口遮挡 DTO。
///
/// 描述窗口中被系统 UI（折叠铰链、状态栏、导航条、IME、悬浮窗等）遮挡的矩形区域。
/// `separating` 表示该遮挡是否把可用区域分割成互不连通的两部分。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct WindowOcclusionDto {
    pub left_dp: f32,
    pub top_dp: f32,
    pub right_dp: f32,
    pub bottom_dp: f32,
    pub separating: bool,
}

impl From<crate::presentation::layout::resolver::WindowOcclusion> for WindowOcclusionDto {
    fn from(o: crate::presentation::layout::resolver::WindowOcclusion) -> Self {
        Self {
            left_dp: o.left_dp,
            top_dp: o.top_dp,
            right_dp: o.right_dp,
            bottom_dp: o.bottom_dp,
            separating: o.separating,
        }
    }
}

impl From<WindowOcclusionDto> for crate::presentation::layout::resolver::WindowOcclusion {
    fn from(dto: WindowOcclusionDto) -> Self {
        Self {
            left_dp: dto.left_dp,
            top_dp: dto.top_dp,
            right_dp: dto.right_dp,
            bottom_dp: dto.bottom_dp,
            separating: dto.separating,
        }
    }
}

/// #628：原始窗口尺寸 DTO，替代旧的 WindowCapabilitiesDto。
///
/// 平台端只传宽高（dp）与遮挡列表，不再传 paneCount / has_separating_fold /
/// pointer_class / keyboard_visible。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowViewportDto {
    pub width_dp: f32,
    pub height_dp: f32,
    /// 窗口遮挡区域列表（#628 验收点 5），默认为空。
    pub occlusions: Vec<WindowOcclusionDto>,
}

impl Default for WindowViewportDto {
    fn default() -> Self {
        // 默认按窄窗口（手机竖屏）算，与 Core 内部 WindowViewport::default() 对齐。
        Self {
            width_dp: 360.0,
            height_dp: 640.0,
            occlusions: Vec::new(),
        }
    }
}

impl From<crate::presentation::layout::resolver::WindowViewport> for WindowViewportDto {
    fn from(v: crate::presentation::layout::resolver::WindowViewport) -> Self {
        Self {
            width_dp: v.width_dp,
            height_dp: v.height_dp,
            occlusions: v.occlusions.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<WindowViewportDto> for crate::presentation::layout::resolver::WindowViewport {
    fn from(dto: WindowViewportDto) -> Self {
        Self {
            width_dp: dto.width_dp,
            height_dp: dto.height_dp,
            occlusions: dto.occlusions.into_iter().map(Into::into).collect(),
        }
    }
}

/// #628：LayoutContractDto 删除 `show_primary_navigation`（改由 ScreenPolicy 提供），
/// 新增 `primary_navigation_placement` 与 `metrics`。
/// 验收点 1：`workspace_pane_mode` → `workspace_layout_mode`（类型 `WorkspaceLayoutModeDto`）。
/// 验收点 5：新增 `workbench_occlusion` 承认遮挡输入。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayoutContractDto {
    pub shell_mode: ShellModeDto,
    pub workspace_layout_mode: WorkspaceLayoutModeDto,
    pub primary_navigation_placement: PrimaryNavigationPlacementDto,
    pub metrics: LayoutMetricsDto,
    /// 工作台可用分区的分隔遮挡（#628 验收点 5），无遮挡时为 None。
    pub workbench_occlusion: Option<WindowOcclusionDto>,
}

impl From<crate::presentation::layout::LayoutContract> for LayoutContractDto {
    fn from(c: crate::presentation::layout::LayoutContract) -> Self {
        Self {
            shell_mode: c.shell_mode.into(),
            workspace_layout_mode: c.workspace_layout_mode.into(),
            primary_navigation_placement: c.primary_navigation_placement.into(),
            metrics: c.metrics.into(),
            workbench_occlusion: c.workbench_occlusion.map(Into::into),
        }
    }
}

impl From<LayoutContractDto> for crate::presentation::layout::LayoutContract {
    fn from(dto: LayoutContractDto) -> Self {
        Self {
            shell_mode: dto.shell_mode.into(),
            workspace_layout_mode: dto.workspace_layout_mode.into(),
            primary_navigation_placement: dto.primary_navigation_placement.into(),
            metrics: dto.metrics.into(),
            workbench_occlusion: dto.workbench_occlusion.map(Into::into),
        }
    }
}

// ========== 单元测试 ==========

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_occlusion_dto_roundtrip() {
        let o = crate::presentation::layout::resolver::WindowOcclusion {
            left_dp: 700.0,
            top_dp: 0.0,
            right_dp: 720.0,
            bottom_dp: 800.0,
            separating: true,
        };
        let dto: WindowOcclusionDto = o.into();
        let back: crate::presentation::layout::resolver::WindowOcclusion = dto.into();
        assert_eq!(back, o);
    }

    #[test]
    fn test_window_occlusion_dto_camel_case_fields() {
        let dto = WindowOcclusionDto {
            left_dp: 1.0,
            top_dp: 2.0,
            right_dp: 3.0,
            bottom_dp: 4.0,
            separating: true,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"leftDp\""));
        assert!(json.contains("\"topDp\""));
        assert!(json.contains("\"rightDp\""));
        assert!(json.contains("\"bottomDp\""));
        assert!(json.contains("\"separating\""));
    }

    #[test]
    fn test_window_occlusion_dto_default_is_non_separating() {
        let dto = WindowOcclusionDto::default();
        assert!(!dto.separating);
        assert_eq!(dto.left_dp, 0.0);
    }

    #[test]
    fn test_window_viewport_dto_roundtrip() {
        let viewport = crate::presentation::layout::resolver::WindowViewport {
            width_dp: 1024.0,
            height_dp: 768.0,
            occlusions: vec![crate::presentation::layout::resolver::WindowOcclusion {
                left_dp: 700.0,
                top_dp: 0.0,
                right_dp: 720.0,
                bottom_dp: 768.0,
                separating: true,
            }],
        };
        let dto: WindowViewportDto = viewport.clone().into();
        let back: crate::presentation::layout::resolver::WindowViewport = dto.into();
        assert_eq!(back, viewport);
    }

    #[test]
    fn test_window_viewport_dto_roundtrip_empty_occlusions() {
        let viewport = crate::presentation::layout::resolver::WindowViewport {
            width_dp: 1024.0,
            height_dp: 768.0,
            occlusions: Vec::new(),
        };
        let dto: WindowViewportDto = viewport.into();
        let back: crate::presentation::layout::resolver::WindowViewport = dto.into();
        assert_eq!(back.width_dp, 1024.0);
        assert_eq!(back.height_dp, 768.0);
        assert!(back.occlusions.is_empty());
    }

    #[test]
    fn test_window_viewport_dto_camel_case_fields() {
        let dto = WindowViewportDto {
            width_dp: 360.0,
            height_dp: 640.0,
            occlusions: Vec::new(),
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"widthDp\""));
        assert!(json.contains("\"heightDp\""));
        assert!(json.contains("\"occlusions\""));
        // #628：不得再出现旧字段。
        assert!(!json.contains("availablePaneCount"));
        assert!(!json.contains("hasSeparatingFold"));
        assert!(!json.contains("pointerClass"));
        assert!(!json.contains("keyboardVisible"));
    }

    #[test]
    fn test_window_viewport_dto_default_has_empty_occlusions() {
        let dto = WindowViewportDto::default();
        assert!(dto.occlusions.is_empty());
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
    fn test_workspace_layout_mode_dto_roundtrip() {
        for w in [
            crate::presentation::layout::WorkspaceLayoutMode::SinglePane,
            crate::presentation::layout::WorkspaceLayoutMode::Workbench,
        ] {
            let dto: WorkspaceLayoutModeDto = w.into();
            let back: crate::presentation::layout::WorkspaceLayoutMode = dto.into();
            assert_eq!(back, w);
        }
    }

    #[test]
    fn test_layout_metrics_dto_roundtrip() {
        let m = crate::presentation::layout::metrics::LayoutMetrics {
            list_pane_width_dp: 320.0,
            project_card_min_width_dp: 180.0,
            tool_pane_width_dp: 240.0,
            tool_rail_width_dp: 56.0,
        };
        let dto: LayoutMetricsDto = m.into();
        let back: crate::presentation::layout::metrics::LayoutMetrics = dto.into();
        assert_eq!(back, m);
    }

    #[test]
    fn test_layout_metrics_dto_default() {
        let dto = LayoutMetricsDto::default();
        assert_eq!(dto.list_pane_width_dp, 320.0);
        assert_eq!(dto.project_card_min_width_dp, 180.0);
        assert_eq!(dto.tool_pane_width_dp, 240.0);
        assert_eq!(dto.tool_rail_width_dp, 56.0);
    }

    #[test]
    fn test_layout_metrics_dto_camel_case_fields() {
        let dto = LayoutMetricsDto::default();
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"listPaneWidthDp\""));
        assert!(json.contains("\"projectCardMinWidthDp\""));
        assert!(json.contains("\"toolPaneWidthDp\""));
        assert!(json.contains("\"toolRailWidthDp\""));
    }

    #[test]
    fn test_layout_contract_dto_roundtrip() {
        let contract = crate::presentation::layout::LayoutContract {
            shell_mode: crate::presentation::layout::ShellMode::TwoPane,
            workspace_layout_mode: crate::presentation::layout::WorkspaceLayoutMode::Workbench,
            primary_navigation_placement:
                crate::presentation::layout::PrimaryNavigationPlacement::Side,
            metrics: crate::presentation::layout::metrics::LayoutMetrics {
                list_pane_width_dp: 320.0,
                project_card_min_width_dp: 180.0,
                tool_pane_width_dp: 240.0,
                tool_rail_width_dp: 56.0,
            },
            workbench_occlusion: Some(crate::presentation::layout::resolver::WindowOcclusion {
                left_dp: 700.0,
                top_dp: 0.0,
                right_dp: 720.0,
                bottom_dp: 800.0,
                separating: true,
            }),
        };
        let dto: LayoutContractDto = contract.clone().into();
        let back: crate::presentation::layout::LayoutContract = dto.into();
        assert_eq!(back.shell_mode, contract.shell_mode);
        assert_eq!(back.workspace_layout_mode, contract.workspace_layout_mode);
        assert_eq!(
            back.primary_navigation_placement,
            contract.primary_navigation_placement
        );
        assert_eq!(back.metrics, contract.metrics);
        assert_eq!(back.workbench_occlusion, contract.workbench_occlusion);
    }

    #[test]
    fn test_layout_contract_dto_no_legacy_fields() {
        // #628：LayoutContractDto 不得再含 showPrimaryNavigation（改由 ScreenPolicy 提供），
        // 也不得再含旧字段 workspacePaneMode（已重命名为 workspaceLayoutMode）。
        let contract = crate::presentation::layout::LayoutContract {
            shell_mode: crate::presentation::layout::ShellMode::SinglePane,
            workspace_layout_mode: crate::presentation::layout::WorkspaceLayoutMode::SinglePane,
            primary_navigation_placement:
                crate::presentation::layout::PrimaryNavigationPlacement::Bottom,
            metrics: crate::presentation::layout::metrics::LayoutMetrics::default(),
            workbench_occlusion: None,
        };
        let dto: LayoutContractDto = contract.into();
        let json = serde_json::to_string(&dto).unwrap();
        assert!(!json.contains("showPrimaryNavigation"));
        assert!(!json.contains("workspacePaneMode"));
        assert!(json.contains("\"workspaceLayoutMode\""));
        assert!(json.contains("\"primaryNavigationPlacement\""));
        assert!(json.contains("\"metrics\""));
        assert!(json.contains("\"workbenchOcclusion\""));
    }

    #[test]
    fn test_resolve_layout_end_to_end_through_dto() {
        // 端到端：WindowViewportDto → Core → LayoutContractDto。
        let dto = WindowViewportDto {
            width_dp: 1000.0,
            height_dp: 800.0,
            occlusions: Vec::new(),
        };
        let viewport: crate::presentation::layout::resolver::WindowViewport = dto.into();
        let contract = crate::presentation::layout::resolve_layout(&viewport);
        let contract_dto: LayoutContractDto = contract.into();
        assert_eq!(contract_dto.shell_mode, ShellModeDto::TwoPane);
        assert_eq!(
            contract_dto.workspace_layout_mode,
            WorkspaceLayoutModeDto::Workbench
        );
        assert_eq!(
            contract_dto.primary_navigation_placement,
            PrimaryNavigationPlacementDto::Side
        );
        assert_eq!(contract_dto.metrics.list_pane_width_dp, 320.0);
        assert_eq!(contract_dto.metrics.project_card_min_width_dp, 180.0);
        assert_eq!(contract_dto.metrics.tool_pane_width_dp, 240.0);
        assert_eq!(contract_dto.metrics.tool_rail_width_dp, 56.0);
        assert!(contract_dto.workbench_occlusion.is_none());
    }

    #[test]
    fn test_resolve_layout_end_to_end_with_separating_occlusion() {
        // 端到端：Workbench 模式 + separating 遮挡 → workbench_occlusion 透传到 DTO。
        let dto = WindowViewportDto {
            width_dp: 1000.0,
            height_dp: 800.0,
            occlusions: vec![WindowOcclusionDto {
                left_dp: 700.0,
                top_dp: 0.0,
                right_dp: 720.0,
                bottom_dp: 800.0,
                separating: true,
            }],
        };
        let viewport: crate::presentation::layout::resolver::WindowViewport = dto.into();
        let contract = crate::presentation::layout::resolve_layout(&viewport);
        let contract_dto: LayoutContractDto = contract.into();
        assert_eq!(
            contract_dto.workspace_layout_mode,
            WorkspaceLayoutModeDto::Workbench
        );
        let occlusion = contract_dto
            .workbench_occlusion
            .expect("occlusion must be present");
        assert!(occlusion.separating);
        assert_eq!(occlusion.left_dp, 700.0);
        assert_eq!(occlusion.right_dp, 720.0);
    }
}
