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
///
/// #628 评论 5301021120 问题 3：再收回 `editor_min_width_dp` /
/// `toolbar_height_dp` / `toolbar_leading_width_dp` / `toolbar_trailing_width_dp`
/// 以及左右 pane 的最小压缩宽度。
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
    /// 正文编辑器最小可编辑宽度，dp（#628 评论 5301021120 问题 3）。
    pub editor_min_width_dp: f32,
    /// 工作台顶栏高度，dp。
    pub toolbar_height_dp: f32,
    /// 顶栏左组宽度，dp。
    pub toolbar_leading_width_dp: f32,
    /// 顶栏右组宽度，dp。
    pub toolbar_trailing_width_dp: f32,
    /// 列表栏最小压缩宽度，dp。
    pub list_pane_min_width_dp: f32,
    /// 工具面板最小压缩宽度，dp。
    pub tool_pane_min_width_dp: f32,
}

impl Default for LayoutMetricsDto {
    fn default() -> Self {
        let m = crate::presentation::layout::metrics::LayoutMetrics::default();
        m.into()
    }
}

impl From<crate::presentation::layout::metrics::LayoutMetrics> for LayoutMetricsDto {
    fn from(m: crate::presentation::layout::metrics::LayoutMetrics) -> Self {
        Self {
            list_pane_width_dp: m.list_pane_width_dp,
            project_card_min_width_dp: m.project_card_min_width_dp,
            tool_pane_width_dp: m.tool_pane_width_dp,
            tool_rail_width_dp: m.tool_rail_width_dp,
            editor_min_width_dp: m.editor_min_width_dp,
            toolbar_height_dp: m.toolbar_height_dp,
            toolbar_leading_width_dp: m.toolbar_leading_width_dp,
            toolbar_trailing_width_dp: m.toolbar_trailing_width_dp,
            list_pane_min_width_dp: m.list_pane_min_width_dp,
            tool_pane_min_width_dp: m.tool_pane_min_width_dp,
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
            editor_min_width_dp: dto.editor_min_width_dp,
            toolbar_height_dp: dto.toolbar_height_dp,
            toolbar_leading_width_dp: dto.toolbar_leading_width_dp,
            toolbar_trailing_width_dp: dto.toolbar_trailing_width_dp,
            list_pane_min_width_dp: dto.list_pane_min_width_dp,
            tool_pane_min_width_dp: dto.tool_pane_min_width_dp,
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

/// #628 评论 5301021120 第 1 步：LayoutContractDto 删除 `show_primary_navigation`（改由 ScreenPolicy 提供），
/// 新增 `primary_navigation_placement` 与 `metrics`。
/// 验收点 1：`workspace_pane_mode` → `workspace_layout_mode`（类型 `WorkspaceLayoutModeDto`）。
/// 第 1 步：删除单数 `workbench_occlusion` 字段（死数据）。工作台布局计划改由
/// `WorkbenchLayoutPlanDto` 单独提供（`resolve_workbench_layout` 接口）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayoutContractDto {
    pub shell_mode: ShellModeDto,
    pub workspace_layout_mode: WorkspaceLayoutModeDto,
    pub primary_navigation_placement: PrimaryNavigationPlacementDto,
    pub metrics: LayoutMetricsDto,
}

impl From<crate::presentation::layout::LayoutContract> for LayoutContractDto {
    fn from(c: crate::presentation::layout::LayoutContract) -> Self {
        Self {
            shell_mode: c.shell_mode.into(),
            workspace_layout_mode: c.workspace_layout_mode.into(),
            primary_navigation_placement: c.primary_navigation_placement.into(),
            metrics: c.metrics.into(),
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
        }
    }
}

// ========== 单元测试 ==========
