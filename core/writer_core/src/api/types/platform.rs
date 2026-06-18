#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]

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

// ── Layout Policy DTOs ──

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]

pub enum FoldPostureDto {
    #[default]
    Unknown,
    FullyOpened,
    HalfOpened,
    Closed,
}

impl From<crate::layout_policy::FoldPosture> for FoldPostureDto {
    fn from(p: crate::layout_policy::FoldPosture) -> Self {
        match p {
            crate::layout_policy::FoldPosture::Unknown => Self::Unknown,
            crate::layout_policy::FoldPosture::FullyOpened => Self::FullyOpened,
            crate::layout_policy::FoldPosture::HalfOpened => Self::HalfOpened,
            crate::layout_policy::FoldPosture::Closed => Self::Closed,
        }
    }
}

impl From<FoldPostureDto> for crate::layout_policy::FoldPosture {
    fn from(dto: FoldPostureDto) -> Self {
        match dto {
            FoldPostureDto::Unknown => Self::Unknown,
            FoldPostureDto::FullyOpened => Self::FullyOpened,
            FoldPostureDto::HalfOpened => Self::HalfOpened,
            FoldPostureDto::Closed => Self::Closed,
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
}

impl From<crate::layout_policy::WidthClass> for WidthClassDto {
    fn from(w: crate::layout_policy::WidthClass) -> Self {
        match w {
            crate::layout_policy::WidthClass::Compact => Self::Compact,
            crate::layout_policy::WidthClass::Medium => Self::Medium,
            crate::layout_policy::WidthClass::Expanded => Self::Expanded,
        }
    }
}

impl From<WidthClassDto> for crate::layout_policy::WidthClass {
    fn from(dto: WidthClassDto) -> Self {
        match dto {
            WidthClassDto::Compact => Self::Compact,
            WidthClassDto::Medium => Self::Medium,
            WidthClassDto::Expanded => Self::Expanded,
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
}

impl From<crate::layout_policy::ShellMode> for ShellModeDto {
    fn from(s: crate::layout_policy::ShellMode) -> Self {
        match s {
            crate::layout_policy::ShellMode::SinglePane => Self::SinglePane,
            crate::layout_policy::ShellMode::SupportingPane => Self::SupportingPane,
            crate::layout_policy::ShellMode::TwoPane => Self::TwoPane,
        }
    }
}

impl From<ShellModeDto> for crate::layout_policy::ShellMode {
    fn from(dto: ShellModeDto) -> Self {
        match dto {
            ShellModeDto::SinglePane => Self::SinglePane,
            ShellModeDto::SupportingPane => Self::SupportingPane,
            ShellModeDto::TwoPane => Self::TwoPane,
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowMetricsDto {
    pub width_vp: f32,
    pub height_vp: f32,
    pub safe_top_vp: f32,
    pub safe_bottom_vp: f32,
    pub keyboard_visible: bool,
    pub fold_posture: FoldPostureDto,
    pub orientation: OrientationDto,
    pub pointer: PointerKindDto,
}

impl From<crate::layout_policy::WindowMetrics> for WindowMetricsDto {
    fn from(m: crate::layout_policy::WindowMetrics) -> Self {
        Self {
            width_vp: m.width_vp,
            height_vp: m.height_vp,
            safe_top_vp: m.safe_top_vp,
            safe_bottom_vp: m.safe_bottom_vp,
            keyboard_visible: m.keyboard_visible,
            fold_posture: m.fold_posture.into(),
            orientation: m.orientation.into(),
            pointer: m.pointer.into(),
        }
    }
}

impl From<WindowMetricsDto> for crate::layout_policy::WindowMetrics {
    fn from(dto: WindowMetricsDto) -> Self {
        Self {
            width_vp: dto.width_vp,
            height_vp: dto.height_vp,
            safe_top_vp: dto.safe_top_vp,
            safe_bottom_vp: dto.safe_bottom_vp,
            keyboard_visible: dto.keyboard_visible,
            fold_posture: dto.fold_posture.into(),
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
    pub content_max_width_vp: f32,
    pub page_padding_vp: f32,
    pub grid_columns: u8,
    pub show_side_panel: bool,
    pub show_bottom_bar: bool,
    pub side_panel_width_vp: f32,
    pub primary_pane_weight: f32,
    pub detail_panel_max_width_vp: f32,
}

impl From<crate::layout_policy::LayoutPlan> for LayoutPlanDto {
    fn from(p: crate::layout_policy::LayoutPlan) -> Self {
        Self {
            width_class: p.width_class.into(),
            height_class: p.height_class.into(),
            shell_mode: p.shell_mode.into(),
            editor_mode: p.editor_mode.into(),
            navigation_mode: p.navigation_mode.into(),
            content_max_width_vp: p.content_max_width_vp,
            page_padding_vp: p.page_padding_vp,
            grid_columns: p.grid_columns,
            show_side_panel: p.show_side_panel,
            show_bottom_bar: p.show_bottom_bar,
            side_panel_width_vp: p.side_panel_width_vp,
            primary_pane_weight: p.primary_pane_weight,
            detail_panel_max_width_vp: p.detail_panel_max_width_vp,
        }
    }
}
