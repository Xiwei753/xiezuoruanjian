//! # 布局策略模块 — 跨端共享的纯函数布局决策
//!
//! 本模块不碰 UI、不碰平台 API、不访问文件系统。
//! 各端测量窗口尺寸后传入 `WindowMetrics`，调用 `resolve_layout` 获取 `LayoutPlan`。
//!
//! ## 调用链路
//!
//! ```text
//! Qt 测窗口宽高 -> 调 Core resolve_layout -> QML 按 LayoutPlan 画
//! Android 测窗口尺寸 -> 调 Core resolve_layout -> Compose 按 LayoutPlan 画
//! Harmony 测窗口 vp / 折叠状态 -> 调 Core resolve_layout -> ArkUI 按 LayoutPlan 画
//! ```

use serde::{Deserialize, Serialize};

// ========== 输入枚举 ==========

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FoldState {
    None,
    Flat,
    HalfOpened,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FoldOrientation {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FoldOcclusion {
    None,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    Unknown,
    Portrait,
    Landscape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointerKind {
    Unknown,
    Touch,
    Stylus,
    Mouse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoldFeatureInfo {
    pub state: FoldState,
    pub orientation: FoldOrientation,
    pub is_separating: bool,
    pub occlusion: FoldOcclusion,
    pub bounds_left_vp: f32,
    pub bounds_top_vp: f32,
    pub bounds_right_vp: f32,
    pub bounds_bottom_vp: f32,
}

impl Default for FoldFeatureInfo {
    fn default() -> Self {
        Self {
            state: FoldState::None,
            orientation: FoldOrientation::Vertical,
            is_separating: false,
            occlusion: FoldOcclusion::None,
            bounds_left_vp: 0.0,
            bounds_top_vp: 0.0,
            bounds_right_vp: 0.0,
            bounds_bottom_vp: 0.0,
        }
    }
}

// ========== 输出枚举 ==========

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WidthClass {
    Compact,
    Medium,
    Expanded,
    Large,
    ExtraLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeightClass {
    Compact,
    Medium,
    Expanded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellMode {
    SinglePane,
    SupportingPane,
    TwoPane,
    ThreePane,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditorMode {
    FullWidth,
    CenteredPaper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavigationMode {
    Stack,
    ListDetail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavigationPresentation {
    BottomBar,
    NavigationRail,
    PermanentDrawer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspacePaneMode {
    SinglePane,
    ListDetail,
    ThreePane,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisiblePaneRoles {
    pub show_project_list: bool,
    pub show_chapter_tree: bool,
    pub show_editor: bool,
    pub show_supporting: bool,
}

// ========== 输入结构体 ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowMetrics {
    pub width_dp: f32,
    pub height_dp: f32,
    pub safe_top_dp: f32,
    pub safe_bottom_dp: f32,
    pub keyboard_visible: bool,
    pub fold_feature: FoldFeatureInfo,
    pub orientation: Orientation,
    pub pointer: PointerKind,
}

impl Default for WindowMetrics {
    fn default() -> Self {
        Self {
            width_dp: 360.0,
            height_dp: 800.0,
            safe_top_dp: 0.0,
            safe_bottom_dp: 0.0,
            keyboard_visible: false,
            fold_feature: FoldFeatureInfo::default(),
            orientation: Orientation::Portrait,
            pointer: PointerKind::Touch,
        }
    }
}

// ========== 输出结构体 ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneWidthConstraint {
    pub min_dp: f32,
    pub preferred_dp: f32,
    pub max_dp: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvoidRegion {
    pub left_dp: f32,
    pub top_dp: f32,
    pub right_dp: f32,
    pub bottom_dp: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutPlan {
    pub width_class: WidthClass,
    pub height_class: HeightClass,
    pub shell_mode: ShellMode,
    pub editor_mode: EditorMode,
    pub navigation_mode: NavigationMode,
    pub navigation_presentation: NavigationPresentation,
    pub workspace_pane_mode: WorkspacePaneMode,
    pub visible_pane_roles: VisiblePaneRoles,
    pub content_max_width_dp: f32,
    pub page_padding_dp: f32,
    pub grid_columns: u8,
    pub show_bottom_bar: bool,
    pub list_pane_width: PaneWidthConstraint,
    pub editor_content_max_width_dp: f32,
    pub primary_pane_min_dp: f32,
    pub primary_pane_preferred_dp: f32,
    pub primary_pane_max_dp: f32,
    pub supporting_pane_mode: Option<WorkspacePaneMode>,
    pub avoid_regions: Vec<AvoidRegion>,
}

// ========== 断点阈值 ==========

const WIDTH_COMPACT_MAX: f32 = 600.0;
const WIDTH_MEDIUM_MAX: f32 = 840.0;
const WIDTH_EXPANDED_MAX: f32 = 1200.0;
const WIDTH_LARGE_MAX: f32 = 1600.0;
const HEIGHT_COMPACT_MAX: f32 = 480.0;
const HEIGHT_MEDIUM_MAX: f32 = 900.0;

// ========== 核心纯函数 ==========

pub fn resolve_layout(metrics: &WindowMetrics) -> LayoutPlan {
    let width_class = resolve_width_class(metrics.width_dp);
    let height_class = resolve_height_class(metrics.height_dp);

    let (
        shell_mode,
        navigation_mode,
        editor_mode,
        navigation_presentation,
        workspace_pane_mode,
        visible_pane_roles,
        content_max_width_dp,
        page_padding_dp,
        grid_columns,
        show_bottom_bar,
        list_pane_width,
        editor_content_max_width_dp,
        primary_pane_min_dp,
        primary_pane_preferred_dp,
        primary_pane_max_dp,
        supporting_pane_mode,
    ) = match width_class {
        WidthClass::Compact => (
            ShellMode::SinglePane,
            NavigationMode::Stack,
            EditorMode::FullWidth,
            NavigationPresentation::BottomBar,
            WorkspacePaneMode::SinglePane,
            VisiblePaneRoles { show_project_list: true, show_chapter_tree: true, show_editor: true, show_supporting: false },
            0.0, 16.0, 2, true,
            PaneWidthConstraint { min_dp: 0.0, preferred_dp: 0.0, max_dp: 0.0 },
            0.0, 0.0, 0.0, 0.0, None,
        ),
        WidthClass::Medium => (
            ShellMode::SupportingPane,
            NavigationMode::Stack,
            EditorMode::CenteredPaper,
            NavigationPresentation::NavigationRail,
            WorkspacePaneMode::SinglePane,
            VisiblePaneRoles { show_project_list: true, show_chapter_tree: true, show_editor: true, show_supporting: false },
            720.0, 24.0, 3, true,
            PaneWidthConstraint { min_dp: 0.0, preferred_dp: 0.0, max_dp: 0.0 },
            0.0, 0.0, 0.0, 0.0, None,
        ),
        WidthClass::Expanded => (
            ShellMode::TwoPane,
            NavigationMode::ListDetail,
            EditorMode::CenteredPaper,
            NavigationPresentation::NavigationRail,
            WorkspacePaneMode::ListDetail,
            VisiblePaneRoles { show_project_list: false, show_chapter_tree: true, show_editor: true, show_supporting: false },
            840.0, 32.0, 4, false,
            PaneWidthConstraint { min_dp: 240.0, preferred_dp: 320.0, max_dp: 400.0 },
            840.0, 240.0, 320.0, 400.0, None,
        ),
        WidthClass::Large => (
            ShellMode::ThreePane,
            NavigationMode::ListDetail,
            EditorMode::CenteredPaper,
            NavigationPresentation::NavigationRail,
            WorkspacePaneMode::ThreePane,
            VisiblePaneRoles { show_project_list: true, show_chapter_tree: true, show_editor: true, show_supporting: false },
            840.0, 32.0, 4, false,
            PaneWidthConstraint { min_dp: 280.0, preferred_dp: 360.0, max_dp: 420.0 },
            840.0, 280.0, 360.0, 420.0, Some(WorkspacePaneMode::ThreePane),
        ),
        WidthClass::ExtraLarge => (
            ShellMode::ThreePane,
            NavigationMode::ListDetail,
            EditorMode::CenteredPaper,
            NavigationPresentation::PermanentDrawer,
            WorkspacePaneMode::ThreePane,
            VisiblePaneRoles { show_project_list: true, show_chapter_tree: true, show_editor: true, show_supporting: true },
            840.0, 40.0, 5, false,
            PaneWidthConstraint { min_dp: 320.0, preferred_dp: 400.0, max_dp: 480.0 },
            840.0, 320.0, 400.0, 480.0, Some(WorkspacePaneMode::ThreePane),
        ),
    };

    let shell_mode = if metrics.fold_feature.state == FoldState::HalfOpened {
        ShellMode::SupportingPane
    } else {
        shell_mode
    };

    let show_bottom_bar = if metrics.keyboard_visible { false } else { show_bottom_bar };

    let mut avoid_regions = Vec::new();
    if metrics.fold_feature.occlusion == FoldOcclusion::Full {
        avoid_regions.push(AvoidRegion {
            left_dp: metrics.fold_feature.bounds_left_vp,
            top_dp: metrics.fold_feature.bounds_top_vp,
            right_dp: metrics.fold_feature.bounds_right_vp,
            bottom_dp: metrics.fold_feature.bounds_bottom_vp,
        });
    }

    LayoutPlan {
        width_class, height_class, shell_mode, editor_mode, navigation_mode,
        navigation_presentation, workspace_pane_mode, visible_pane_roles,
        content_max_width_dp, page_padding_dp, grid_columns, show_bottom_bar,
        list_pane_width, editor_content_max_width_dp,
        primary_pane_min_dp, primary_pane_preferred_dp, primary_pane_max_dp,
        supporting_pane_mode, avoid_regions,
    }
}

pub fn resolve_width_class(width_dp: f32) -> WidthClass {
    if width_dp < WIDTH_COMPACT_MAX { WidthClass::Compact }
    else if width_dp < WIDTH_MEDIUM_MAX { WidthClass::Medium }
    else if width_dp < WIDTH_EXPANDED_MAX { WidthClass::Expanded }
    else if width_dp < WIDTH_LARGE_MAX { WidthClass::Large }
    else { WidthClass::ExtraLarge }
}

pub fn resolve_height_class(height_dp: f32) -> HeightClass {
    if height_dp < HEIGHT_COMPACT_MAX { HeightClass::Compact }
    else if height_dp < HEIGHT_MEDIUM_MAX { HeightClass::Medium }
    else { HeightClass::Expanded }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_metrics() -> WindowMetrics { WindowMetrics::default() }

    #[test]
    fn test_compact_width() {
        let mut m = default_metrics(); m.width_dp = 360.0;
        let plan = resolve_layout(&m);
        assert_eq!(plan.width_class, WidthClass::Compact);
        assert_eq!(plan.shell_mode, ShellMode::SinglePane);
        assert_eq!(plan.navigation_presentation, NavigationPresentation::BottomBar);
    }

    #[test]
    fn test_medium_width() {
        let mut m = default_metrics(); m.width_dp = 700.0;
        let plan = resolve_layout(&m);
        assert_eq!(plan.width_class, WidthClass::Medium);
        assert_eq!(plan.shell_mode, ShellMode::SupportingPane);
        assert_eq!(plan.navigation_presentation, NavigationPresentation::NavigationRail);
    }

    #[test]
    fn test_expanded_width() {
        let mut m = default_metrics(); m.width_dp = 1000.0;
        let plan = resolve_layout(&m);
        assert_eq!(plan.width_class, WidthClass::Expanded);
        assert_eq!(plan.shell_mode, ShellMode::TwoPane);
        assert_eq!(plan.workspace_pane_mode, WorkspacePaneMode::ListDetail);
    }

    #[test]
    fn test_large_width() {
        let mut m = default_metrics(); m.width_dp = 1400.0;
        let plan = resolve_layout(&m);
        assert_eq!(plan.width_class, WidthClass::Large);
        assert_eq!(plan.shell_mode, ShellMode::ThreePane);
        assert!(plan.visible_pane_roles.show_project_list);
    }

    #[test]
    fn test_extra_large_width() {
        let mut m = default_metrics(); m.width_dp = 1800.0;
        let plan = resolve_layout(&m);
        assert_eq!(plan.width_class, WidthClass::ExtraLarge);
        assert_eq!(plan.navigation_presentation, NavigationPresentation::PermanentDrawer);
        assert!(plan.visible_pane_roles.show_supporting);
    }

    #[test]
    fn test_fold_half_opened() {
        let mut m = default_metrics(); m.width_dp = 1200.0;
        m.fold_feature = FoldFeatureInfo {
            state: FoldState::HalfOpened, orientation: FoldOrientation::Horizontal,
            is_separating: true, occlusion: FoldOcclusion::None,
            bounds_left_vp: 0.0, bounds_top_vp: 500.0, bounds_right_vp: 1000.0, bounds_bottom_vp: 520.0,
        };
        let plan = resolve_layout(&m);
        assert_eq!(plan.shell_mode, ShellMode::SupportingPane);
    }

    #[test]
    fn test_keyboard_hides_bottom_bar() {
        let mut m = default_metrics(); m.keyboard_visible = true;
        assert!(!resolve_layout(&m).show_bottom_bar);
    }

    #[test]
    fn test_boundary_values() {
        assert_eq!(resolve_width_class(599.9), WidthClass::Compact);
        assert_eq!(resolve_width_class(600.0), WidthClass::Medium);
        assert_eq!(resolve_width_class(839.9), WidthClass::Medium);
        assert_eq!(resolve_width_class(840.0), WidthClass::Expanded);
        assert_eq!(resolve_width_class(1199.9), WidthClass::Expanded);
        assert_eq!(resolve_width_class(1200.0), WidthClass::Large);
        assert_eq!(resolve_width_class(1599.9), WidthClass::Large);
        assert_eq!(resolve_width_class(1600.0), WidthClass::ExtraLarge);
    }

    #[test]
    fn test_fold_occlusion_avoid_region() {
        let mut m = default_metrics(); m.width_dp = 1200.0;
        m.fold_feature = FoldFeatureInfo {
            state: FoldState::Flat, orientation: FoldOrientation::Vertical,
            is_separating: true, occlusion: FoldOcclusion::Full,
            bounds_left_vp: 500.0, bounds_top_vp: 0.0, bounds_right_vp: 520.0, bounds_bottom_vp: 800.0,
        };
        let plan = resolve_layout(&m);
        assert_eq!(plan.avoid_regions.len(), 1);
    }

    #[test]
    fn test_default_metrics() {
        let m = WindowMetrics::default();
        assert_eq!(m.width_dp, 360.0);
        assert_eq!(m.fold_feature.state, FoldState::None);
    }
}
