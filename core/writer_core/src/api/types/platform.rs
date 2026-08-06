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

// ── Layout Policy DTOs ──

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum FoldStateDto {
    #[default]
    None,
    Flat,
    HalfOpened,
}

impl From<crate::layout_policy::FoldState> for FoldStateDto {
    fn from(s: crate::layout_policy::FoldState) -> Self {
        match s {
            crate::layout_policy::FoldState::None => Self::None,
            crate::layout_policy::FoldState::Flat => Self::Flat,
            crate::layout_policy::FoldState::HalfOpened => Self::HalfOpened,
        }
    }
}

impl From<FoldStateDto> for crate::layout_policy::FoldState {
    fn from(dto: FoldStateDto) -> Self {
        match dto {
            FoldStateDto::None => Self::None,
            FoldStateDto::Flat => Self::Flat,
            FoldStateDto::HalfOpened => Self::HalfOpened,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum FoldOrientationDto {
    #[default]
    Horizontal,
    Vertical,
}

impl From<crate::layout_policy::FoldOrientation> for FoldOrientationDto {
    fn from(o: crate::layout_policy::FoldOrientation) -> Self {
        match o {
            crate::layout_policy::FoldOrientation::Horizontal => Self::Horizontal,
            crate::layout_policy::FoldOrientation::Vertical => Self::Vertical,
        }
    }
}

impl From<FoldOrientationDto> for crate::layout_policy::FoldOrientation {
    fn from(dto: FoldOrientationDto) -> Self {
        match dto {
            FoldOrientationDto::Horizontal => Self::Horizontal,
            FoldOrientationDto::Vertical => Self::Vertical,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum FoldOcclusionDto {
    #[default]
    None,
    Full,
}

impl From<crate::layout_policy::FoldOcclusion> for FoldOcclusionDto {
    fn from(o: crate::layout_policy::FoldOcclusion) -> Self {
        match o {
            crate::layout_policy::FoldOcclusion::None => Self::None,
            crate::layout_policy::FoldOcclusion::Full => Self::Full,
        }
    }
}

impl From<FoldOcclusionDto> for crate::layout_policy::FoldOcclusion {
    fn from(dto: FoldOcclusionDto) -> Self {
        match dto {
            FoldOcclusionDto::None => Self::None,
            FoldOcclusionDto::Full => Self::Full,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FoldFeatureInfoDto {
    pub state: FoldStateDto,
    pub orientation: FoldOrientationDto,
    pub is_separating: bool,
    pub occlusion: FoldOcclusionDto,
    pub bounds_left_vp: f32,
    pub bounds_top_vp: f32,
    pub bounds_right_vp: f32,
    pub bounds_bottom_vp: f32,
}

impl Default for FoldFeatureInfoDto {
    fn default() -> Self {
        Self {
            state: FoldStateDto::None,
            orientation: FoldOrientationDto::Vertical,
            is_separating: false,
            occlusion: FoldOcclusionDto::None,
            bounds_left_vp: 0.0,
            bounds_top_vp: 0.0,
            bounds_right_vp: 0.0,
            bounds_bottom_vp: 0.0,
        }
    }
}

impl From<crate::layout_policy::FoldFeatureInfo> for FoldFeatureInfoDto {
    fn from(f: crate::layout_policy::FoldFeatureInfo) -> Self {
        Self {
            state: f.state.into(),
            orientation: f.orientation.into(),
            is_separating: f.is_separating,
            occlusion: f.occlusion.into(),
            bounds_left_vp: f.bounds_left_vp,
            bounds_top_vp: f.bounds_top_vp,
            bounds_right_vp: f.bounds_right_vp,
            bounds_bottom_vp: f.bounds_bottom_vp,
        }
    }
}

impl From<FoldFeatureInfoDto> for crate::layout_policy::FoldFeatureInfo {
    fn from(dto: FoldFeatureInfoDto) -> Self {
        Self {
            state: dto.state.into(),
            orientation: dto.orientation.into(),
            is_separating: dto.is_separating,
            occlusion: dto.occlusion.into(),
            bounds_left_vp: dto.bounds_left_vp,
            bounds_top_vp: dto.bounds_top_vp,
            bounds_right_vp: dto.bounds_right_vp,
            bounds_bottom_vp: dto.bounds_bottom_vp,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum OrientationDto {
    #[default]
    Unknown,
    Portrait,
    Landscape,
}

impl From<crate::layout_policy::Orientation> for OrientationDto {
    fn from(o: crate::layout_policy::Orientation) -> Self {
        match o {
            crate::layout_policy::Orientation::Unknown => Self::Unknown,
            crate::layout_policy::Orientation::Portrait => Self::Portrait,
            crate::layout_policy::Orientation::Landscape => Self::Landscape,
        }
    }
}

impl From<OrientationDto> for crate::layout_policy::Orientation {
    fn from(dto: OrientationDto) -> Self {
        match dto {
            OrientationDto::Unknown => Self::Unknown,
            OrientationDto::Portrait => Self::Portrait,
            OrientationDto::Landscape => Self::Landscape,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum PointerKindDto {
    #[default]
    Unknown,
    Touch,
    Stylus,
    Mouse,
}

impl From<crate::layout_policy::PointerKind> for PointerKindDto {
    fn from(p: crate::layout_policy::PointerKind) -> Self {
        match p {
            crate::layout_policy::PointerKind::Unknown => Self::Unknown,
            crate::layout_policy::PointerKind::Touch => Self::Touch,
            crate::layout_policy::PointerKind::Stylus => Self::Stylus,
            crate::layout_policy::PointerKind::Mouse => Self::Mouse,
        }
    }
}

impl From<PointerKindDto> for crate::layout_policy::PointerKind {
    fn from(dto: PointerKindDto) -> Self {
        match dto {
            PointerKindDto::Unknown => Self::Unknown,
            PointerKindDto::Touch => Self::Touch,
            PointerKindDto::Stylus => Self::Stylus,
            PointerKindDto::Mouse => Self::Mouse,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum WidthClassDto {
    #[default]
    Compact,
    Medium,
    Expanded,
    Large,
    ExtraLarge,
}

impl From<crate::layout_policy::WidthClass> for WidthClassDto {
    fn from(w: crate::layout_policy::WidthClass) -> Self {
        match w {
            crate::layout_policy::WidthClass::Compact => Self::Compact,
            crate::layout_policy::WidthClass::Medium => Self::Medium,
            crate::layout_policy::WidthClass::Expanded => Self::Expanded,
            crate::layout_policy::WidthClass::Large => Self::Large,
            crate::layout_policy::WidthClass::ExtraLarge => Self::ExtraLarge,
        }
    }
}

impl From<WidthClassDto> for crate::layout_policy::WidthClass {
    fn from(dto: WidthClassDto) -> Self {
        match dto {
            WidthClassDto::Compact => Self::Compact,
            WidthClassDto::Medium => Self::Medium,
            WidthClassDto::Expanded => Self::Expanded,
            WidthClassDto::Large => Self::Large,
            WidthClassDto::ExtraLarge => Self::ExtraLarge,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum HeightClassDto {
    #[default]
    Compact,
    Medium,
    Expanded,
}

impl From<crate::layout_policy::HeightClass> for HeightClassDto {
    fn from(h: crate::layout_policy::HeightClass) -> Self {
        match h {
            crate::layout_policy::HeightClass::Compact => Self::Compact,
            crate::layout_policy::HeightClass::Medium => Self::Medium,
            crate::layout_policy::HeightClass::Expanded => Self::Expanded,
        }
    }
}

impl From<HeightClassDto> for crate::layout_policy::HeightClass {
    fn from(dto: HeightClassDto) -> Self {
        match dto {
            HeightClassDto::Compact => Self::Compact,
            HeightClassDto::Medium => Self::Medium,
            HeightClassDto::Expanded => Self::Expanded,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum ShellModeDto {
    #[default]
    SinglePane,
    SupportingPane,
    TwoPane,
    ThreePane,
}

impl From<crate::layout_policy::ShellMode> for ShellModeDto {
    fn from(s: crate::layout_policy::ShellMode) -> Self {
        match s {
            crate::layout_policy::ShellMode::SinglePane => Self::SinglePane,
            crate::layout_policy::ShellMode::SupportingPane => Self::SupportingPane,
            crate::layout_policy::ShellMode::TwoPane => Self::TwoPane,
            crate::layout_policy::ShellMode::ThreePane => Self::ThreePane,
        }
    }
}

impl From<ShellModeDto> for crate::layout_policy::ShellMode {
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
pub enum EditorModeDto {
    #[default]
    FullWidth,
    CenteredPaper,
}

impl From<crate::layout_policy::EditorMode> for EditorModeDto {
    fn from(e: crate::layout_policy::EditorMode) -> Self {
        match e {
            crate::layout_policy::EditorMode::FullWidth => Self::FullWidth,
            crate::layout_policy::EditorMode::CenteredPaper => Self::CenteredPaper,
        }
    }
}

impl From<EditorModeDto> for crate::layout_policy::EditorMode {
    fn from(dto: EditorModeDto) -> Self {
        match dto {
            EditorModeDto::FullWidth => Self::FullWidth,
            EditorModeDto::CenteredPaper => Self::CenteredPaper,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum NavigationModeDto {
    #[default]
    Stack,
    ListDetail,
}

impl From<crate::layout_policy::NavigationMode> for NavigationModeDto {
    fn from(n: crate::layout_policy::NavigationMode) -> Self {
        match n {
            crate::layout_policy::NavigationMode::Stack => Self::Stack,
            crate::layout_policy::NavigationMode::ListDetail => Self::ListDetail,
        }
    }
}

impl From<NavigationModeDto> for crate::layout_policy::NavigationMode {
    fn from(dto: NavigationModeDto) -> Self {
        match dto {
            NavigationModeDto::Stack => Self::Stack,
            NavigationModeDto::ListDetail => Self::ListDetail,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum NavigationPresentationDto {
    #[default]
    BottomBar,
    NavigationRail,
    PermanentDrawer,
}

impl From<crate::layout_policy::NavigationPresentation> for NavigationPresentationDto {
    fn from(n: crate::layout_policy::NavigationPresentation) -> Self {
        match n {
            crate::layout_policy::NavigationPresentation::BottomBar => Self::BottomBar,
            crate::layout_policy::NavigationPresentation::NavigationRail => Self::NavigationRail,
            crate::layout_policy::NavigationPresentation::PermanentDrawer => Self::PermanentDrawer,
        }
    }
}

impl From<NavigationPresentationDto> for crate::layout_policy::NavigationPresentation {
    fn from(dto: NavigationPresentationDto) -> Self {
        match dto {
            NavigationPresentationDto::BottomBar => Self::BottomBar,
            NavigationPresentationDto::NavigationRail => Self::NavigationRail,
            NavigationPresentationDto::PermanentDrawer => Self::PermanentDrawer,
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

impl From<crate::layout_policy::WorkspacePaneMode> for WorkspacePaneModeDto {
    fn from(w: crate::layout_policy::WorkspacePaneMode) -> Self {
        match w {
            crate::layout_policy::WorkspacePaneMode::SinglePane => Self::SinglePane,
            crate::layout_policy::WorkspacePaneMode::ListDetail => Self::ListDetail,
            crate::layout_policy::WorkspacePaneMode::ThreePane => Self::ThreePane,
        }
    }
}

impl From<WorkspacePaneModeDto> for crate::layout_policy::WorkspacePaneMode {
    fn from(dto: WorkspacePaneModeDto) -> Self {
        match dto {
            WorkspacePaneModeDto::SinglePane => Self::SinglePane,
            WorkspacePaneModeDto::ListDetail => Self::ListDetail,
            WorkspacePaneModeDto::ThreePane => Self::ThreePane,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VisiblePaneRolesDto {
    pub show_project_list: bool,
    pub show_chapter_tree: bool,
    pub show_editor: bool,
    pub show_supporting: bool,
}

impl Default for VisiblePaneRolesDto {
    fn default() -> Self {
        Self {
            show_project_list: true,
            show_chapter_tree: true,
            show_editor: true,
            show_supporting: false,
        }
    }
}

impl From<crate::layout_policy::VisiblePaneRoles> for VisiblePaneRolesDto {
    fn from(v: crate::layout_policy::VisiblePaneRoles) -> Self {
        Self {
            show_project_list: v.show_project_list,
            show_chapter_tree: v.show_chapter_tree,
            show_editor: v.show_editor,
            show_supporting: v.show_supporting,
        }
    }
}

impl From<VisiblePaneRolesDto> for crate::layout_policy::VisiblePaneRoles {
    fn from(dto: VisiblePaneRolesDto) -> Self {
        Self {
            show_project_list: dto.show_project_list,
            show_chapter_tree: dto.show_chapter_tree,
            show_editor: dto.show_editor,
            show_supporting: dto.show_supporting,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PaneWidthConstraintDto {
    pub min_dp: f32,
    pub preferred_dp: f32,
    pub max_dp: f32,
}

impl Default for PaneWidthConstraintDto {
    fn default() -> Self {
        Self {
            min_dp: 0.0,
            preferred_dp: 0.0,
            max_dp: 0.0,
        }
    }
}

impl From<crate::layout_policy::PaneWidthConstraint> for PaneWidthConstraintDto {
    fn from(p: crate::layout_policy::PaneWidthConstraint) -> Self {
        Self {
            min_dp: p.min_dp,
            preferred_dp: p.preferred_dp,
            max_dp: p.max_dp,
        }
    }
}

impl From<PaneWidthConstraintDto> for crate::layout_policy::PaneWidthConstraint {
    fn from(dto: PaneWidthConstraintDto) -> Self {
        Self {
            min_dp: dto.min_dp,
            preferred_dp: dto.preferred_dp,
            max_dp: dto.max_dp,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum AvoidRegionKindDto {
    WindowInset,
    VerticalHinge,
    HorizontalHinge,
}

impl From<crate::layout_policy::AvoidRegionKind> for AvoidRegionKindDto {
    fn from(k: crate::layout_policy::AvoidRegionKind) -> Self {
        match k {
            crate::layout_policy::AvoidRegionKind::WindowInset => Self::WindowInset,
            crate::layout_policy::AvoidRegionKind::VerticalHinge => Self::VerticalHinge,
            crate::layout_policy::AvoidRegionKind::HorizontalHinge => Self::HorizontalHinge,
        }
    }
}

impl From<AvoidRegionKindDto> for crate::layout_policy::AvoidRegionKind {
    fn from(dto: AvoidRegionKindDto) -> Self {
        match dto {
            AvoidRegionKindDto::WindowInset => Self::WindowInset,
            AvoidRegionKindDto::VerticalHinge => Self::VerticalHinge,
            AvoidRegionKindDto::HorizontalHinge => Self::HorizontalHinge,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AvoidRegionDto {
    pub left_dp: f32,
    pub top_dp: f32,
    pub right_dp: f32,
    pub bottom_dp: f32,
    pub kind: AvoidRegionKindDto,
}

impl Default for AvoidRegionDto {
    fn default() -> Self {
        Self {
            left_dp: 0.0,
            top_dp: 0.0,
            right_dp: 0.0,
            bottom_dp: 0.0,
            kind: AvoidRegionKindDto::WindowInset,
        }
    }
}

impl From<crate::layout_policy::AvoidRegion> for AvoidRegionDto {
    fn from(a: crate::layout_policy::AvoidRegion) -> Self {
        Self {
            left_dp: a.left_dp,
            top_dp: a.top_dp,
            right_dp: a.right_dp,
            bottom_dp: a.bottom_dp,
            kind: a.kind.into(),
        }
    }
}

impl From<AvoidRegionDto> for crate::layout_policy::AvoidRegion {
    fn from(dto: AvoidRegionDto) -> Self {
        Self {
            left_dp: dto.left_dp,
            top_dp: dto.top_dp,
            right_dp: dto.right_dp,
            bottom_dp: dto.bottom_dp,
            kind: dto.kind.into(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowMetricsDto {
    pub width_dp: f32,
    pub height_dp: f32,
    pub safe_top_dp: f32,
    pub safe_bottom_dp: f32,
    pub keyboard_visible: bool,
    pub fold_feature: FoldFeatureInfoDto,
    pub orientation: OrientationDto,
    pub pointer: PointerKindDto,
}

impl From<crate::layout_policy::WindowMetrics> for WindowMetricsDto {
    fn from(m: crate::layout_policy::WindowMetrics) -> Self {
        Self {
            width_dp: m.width_dp,
            height_dp: m.height_dp,
            safe_top_dp: m.safe_top_dp,
            safe_bottom_dp: m.safe_bottom_dp,
            keyboard_visible: m.keyboard_visible,
            fold_feature: m.fold_feature.into(),
            orientation: m.orientation.into(),
            pointer: m.pointer.into(),
        }
    }
}

impl From<WindowMetricsDto> for crate::layout_policy::WindowMetrics {
    fn from(dto: WindowMetricsDto) -> Self {
        Self {
            width_dp: dto.width_dp,
            height_dp: dto.height_dp,
            safe_top_dp: dto.safe_top_dp,
            safe_bottom_dp: dto.safe_bottom_dp,
            keyboard_visible: dto.keyboard_visible,
            fold_feature: dto.fold_feature.into(),
            orientation: dto.orientation.into(),
            pointer: dto.pointer.into(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayoutPlanDto {
    pub width_class: WidthClassDto,
    pub height_class: HeightClassDto,
    pub shell_mode: ShellModeDto,
    pub editor_mode: EditorModeDto,
    pub navigation_mode: NavigationModeDto,
    pub navigation_presentation: NavigationPresentationDto,
    pub workspace_pane_mode: WorkspacePaneModeDto,
    pub visible_pane_roles: VisiblePaneRolesDto,
    pub content_max_width_dp: f32,
    pub page_padding_dp: f32,
    pub grid_columns: u8,
    pub show_bottom_bar: bool,
    pub list_pane_width: PaneWidthConstraintDto,
    pub editor_content_max_width_dp: f32,
    pub primary_pane_min_dp: f32,
    pub primary_pane_preferred_dp: f32,
    pub primary_pane_max_dp: f32,
    pub supporting_pane_mode: Option<WorkspacePaneModeDto>,
    pub avoid_regions: Vec<AvoidRegionDto>,
}

impl From<crate::layout_policy::LayoutPlan> for LayoutPlanDto {
    fn from(p: crate::layout_policy::LayoutPlan) -> Self {
        Self {
            width_class: p.width_class.into(),
            height_class: p.height_class.into(),
            shell_mode: p.shell_mode.into(),
            editor_mode: p.editor_mode.into(),
            navigation_mode: p.navigation_mode.into(),
            navigation_presentation: p.navigation_presentation.into(),
            workspace_pane_mode: p.workspace_pane_mode.into(),
            visible_pane_roles: p.visible_pane_roles.into(),
            content_max_width_dp: p.content_max_width_dp,
            page_padding_dp: p.page_padding_dp,
            grid_columns: p.grid_columns,
            show_bottom_bar: p.show_bottom_bar,
            list_pane_width: p.list_pane_width.into(),
            editor_content_max_width_dp: p.editor_content_max_width_dp,
            primary_pane_min_dp: p.primary_pane_min_dp,
            primary_pane_preferred_dp: p.primary_pane_preferred_dp,
            primary_pane_max_dp: p.primary_pane_max_dp,
            supporting_pane_mode: p.supporting_pane_mode.map(Into::into),
            avoid_regions: p.avoid_regions.into_iter().map(Into::into).collect(),
        }
    }
}
