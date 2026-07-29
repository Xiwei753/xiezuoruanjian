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

/// 折叠状态：None=无折叠屏, Flat=完全展开, HalfOpened=半开（桌面模式）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FoldState {
    None,
    Flat,
    HalfOpened,
}

/// 折叠方向：Horizontal=铰链水平（上下屏）, Vertical=铰链垂直（左右屏）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FoldOrientation {
    Horizontal,
    Vertical,
}

/// 折叠遮挡类型：None=不遮挡, Full=折叠区域完全遮挡内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FoldOcclusion {
    None,
    Full,
}

/// 设备方向：Unknown=初始/旋转中, Portrait=竖屏, Landscape=横屏。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    Unknown,
    Portrait,
    Landscape,
}

/// 主输入类型：影响导航目标和交互区域大小。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointerKind {
    Unknown,
    Touch,
    Stylus,
    Mouse,
}

/// 折叠屏特性信息。坐标单位为 vp（密度无关像素），由平台端从 Android
/// `FoldingFeature` / Qt `QScreen` 折叠区域转换后传入。
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

/// 宽度分类 — 参考 Material Design 3 响应式断点。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WidthClass {
    Compact,
    Medium,
    Expanded,
    Large,
    ExtraLarge,
}

/// 高度分类 — 影响底部导航栏和内容区最小高度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeightClass {
    Compact,
    Medium,
    Expanded,
}

/// Shell 模式 — 决定主界面框架结构。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellMode {
    SinglePane,
    SupportingPane,
    TwoPane,
    ThreePane,
}

/// 编辑器模式 — FullWidth=编辑器占满可用宽度, CenteredPaper=居中纸面模式（限宽）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditorMode {
    FullWidth,
    CenteredPaper,
}

/// 导航模式 — Stack=手机式栈导航, ListDetail=列表-详情双栏。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavigationMode {
    Stack,
    ListDetail,
}

/// 导航呈现方式 — 随宽度递增：BottomBar→NavigationRail→PermanentDrawer。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavigationPresentation {
    BottomBar,
    NavigationRail,
    PermanentDrawer,
}

/// 工作区面板模式 — 决定项目列表/章节树/编辑器的组合方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspacePaneMode {
    SinglePane,
    ListDetail,
    ThreePane,
}

/// 各面板可见性 — 平台端据此决定哪些 UI 组件需要渲染。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisiblePaneRoles {
    pub show_project_list: bool,
    pub show_chapter_tree: bool,
    pub show_editor: bool,
    pub show_supporting: bool,
}

// ========== 工作台约束 — Issue #568 ==========

/// 工作台约束 — 由窗口度量决定的面板尺寸限制。
/// Core 只提供约束，面板状态和布局由平台端自行管理。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchConstraints {
    pub side_panel_min_dp: f32,
    pub side_panel_max_dp: f32,
    pub bottom_panel_min_dp: f32,
    pub bottom_panel_max_ratio: f32,
    pub editor_min_dp: f32,
    pub allow_side_dock: bool,
    pub allow_both_side_docks: bool,
    pub allow_bottom_dock: bool,
}

const SIDE_PANEL_MIN_DP: f32 = 280.0;
const SIDE_PANEL_MAX_DP: f32 = 520.0;
const BOTTOM_PANEL_MIN_DP: f32 = 220.0;
const BOTTOM_PANEL_MAX_RATIO: f32 = 0.55;
const EDITOR_MIN_DP: f32 = 480.0;

/// 根据窗口度量计算工作台约束。纯函数，无副作用。
pub fn resolve_workbench_constraints(metrics: &WindowMetrics) -> WorkbenchConstraints {
    let width_class = resolve_width_class(metrics.width_dp);
    match width_class {
        WidthClass::Compact | WidthClass::Medium => WorkbenchConstraints {
            side_panel_min_dp: SIDE_PANEL_MIN_DP,
            side_panel_max_dp: SIDE_PANEL_MAX_DP,
            bottom_panel_min_dp: BOTTOM_PANEL_MIN_DP,
            bottom_panel_max_ratio: BOTTOM_PANEL_MAX_RATIO,
            editor_min_dp: EDITOR_MIN_DP,
            allow_side_dock: false,
            allow_both_side_docks: false,
            allow_bottom_dock: false,
        },
        WidthClass::Expanded => WorkbenchConstraints {
            side_panel_min_dp: SIDE_PANEL_MIN_DP,
            side_panel_max_dp: SIDE_PANEL_MAX_DP,
            bottom_panel_min_dp: BOTTOM_PANEL_MIN_DP,
            bottom_panel_max_ratio: BOTTOM_PANEL_MAX_RATIO,
            editor_min_dp: EDITOR_MIN_DP,
            allow_side_dock: true,
            allow_both_side_docks: false,
            allow_bottom_dock: false,
        },
        WidthClass::Large | WidthClass::ExtraLarge => WorkbenchConstraints {
            side_panel_min_dp: SIDE_PANEL_MIN_DP,
            side_panel_max_dp: SIDE_PANEL_MAX_DP,
            bottom_panel_min_dp: BOTTOM_PANEL_MIN_DP,
            bottom_panel_max_ratio: BOTTOM_PANEL_MAX_RATIO,
            editor_min_dp: EDITOR_MIN_DP,
            allow_side_dock: true,
            allow_both_side_docks: true,
            allow_bottom_dock: true,
        },
    }
}

// ========== 输入结构体 ==========

/// 窗口度量 — 平台端测量后传入。所有尺寸单位为 dp（密度无关像素）。
/// 折叠屏坐标为 vp，Core 层在当前实现中不区分 dp/vp 差异，
/// 由平台端在传入前完成必要的缩放。
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

/// 面板宽度约束 — min/preferred/max 三级，平台端在约束范围内自由分配。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneWidthConstraint {
    pub min_dp: f32,
    pub preferred_dp: f32,
    pub max_dp: f32,
}

/// 避让区域类型 — WindowInset=系统窗口内嵌, VerticalHinge=竖向铰链, HorizontalHinge=横向铰链。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AvoidRegionKind {
    WindowInset,
    VerticalHinge,
    HorizontalHinge,
}

/// 避让区域 — 折叠屏铰链或遮挡区域，平台端应避免在此放置交互元素。
/// 坐标单位为 dp（来自 FoldFeatureInfo 的 vp，Core 不做 dp/vp 转换）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvoidRegion {
    pub left_dp: f32,
    pub top_dp: f32,
    pub right_dp: f32,
    pub bottom_dp: f32,
    pub kind: AvoidRegionKind,
}

/// 布局计划 — `resolve_layout` 的纯函数输出，平台端据此绘制 UI。
/// 所有尺寸单位为 dp。`avoid_regions` 的坐标来自 `FoldFeatureInfo`（vp），
/// 平台端使用时需注意 dp/vp 差异。
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
// 宽度断点采用左闭右开区间 [0, MAX)：值 < MAX 属于该分类。
// 例如 599.9 → Compact，600.0 → Medium。
// 阈值参考 Material Design 3 响应式断点规范。

const WIDTH_COMPACT_MAX: f32 = 600.0;
const WIDTH_MEDIUM_MAX: f32 = 840.0;
const WIDTH_EXPANDED_MAX: f32 = 1200.0;
const WIDTH_LARGE_MAX: f32 = 1600.0;
const HEIGHT_COMPACT_MAX: f32 = 480.0;
const HEIGHT_MEDIUM_MAX: f32 = 900.0;

// ========== 核心纯函数 ==========

/// 根据窗口度量计算布局计划。纯函数，无副作用。
///
/// 决策优先级：
/// 1. 宽度断点决定基础 shell/navigation/workspace_pane 模式
/// 2. 折叠屏半开状态覆盖 shell_mode（水平半开→SupportingPane+SinglePane，
///    其他半开→保留宽度决定的 workspace_pane_mode）
/// 3. 虚拟键盘可见时隐藏底部导航栏
/// 4. 折叠屏铰链/遮挡区域生成 avoid_regions
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

    let (shell_mode, workspace_pane_mode) = if metrics.fold_feature.state == FoldState::HalfOpened
        && metrics.fold_feature.orientation == FoldOrientation::Horizontal
        && metrics.fold_feature.is_separating
    {
        (ShellMode::SupportingPane, WorkspacePaneMode::SinglePane)
    } else if metrics.fold_feature.state == FoldState::HalfOpened {
        (ShellMode::SupportingPane, workspace_pane_mode)
    } else {
        (shell_mode, workspace_pane_mode)
    };

    let show_bottom_bar = if metrics.keyboard_visible { false } else { show_bottom_bar };

    let mut avoid_regions = Vec::new();
    if metrics.fold_feature.is_separating {
        let kind = match metrics.fold_feature.orientation {
            FoldOrientation::Vertical => AvoidRegionKind::VerticalHinge,
            FoldOrientation::Horizontal => AvoidRegionKind::HorizontalHinge,
        };
        avoid_regions.push(AvoidRegion {
            left_dp: metrics.fold_feature.bounds_left_vp,
            top_dp: metrics.fold_feature.bounds_top_vp,
            right_dp: metrics.fold_feature.bounds_right_vp,
            bottom_dp: metrics.fold_feature.bounds_bottom_vp,
            kind,
        });
    } else if metrics.fold_feature.occlusion == FoldOcclusion::Full {
        avoid_regions.push(AvoidRegion {
            left_dp: metrics.fold_feature.bounds_left_vp,
            top_dp: metrics.fold_feature.bounds_top_vp,
            right_dp: metrics.fold_feature.bounds_right_vp,
            bottom_dp: metrics.fold_feature.bounds_bottom_vp,
            kind: AvoidRegionKind::WindowInset,
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

/// 宽度断点分类。左闭右开区间：`[0, 600)` → Compact, `[600, 840)` → Medium, 以此类推。
pub fn resolve_width_class(width_dp: f32) -> WidthClass {
    if width_dp < WIDTH_COMPACT_MAX { WidthClass::Compact }
    else if width_dp < WIDTH_MEDIUM_MAX { WidthClass::Medium }
    else if width_dp < WIDTH_EXPANDED_MAX { WidthClass::Expanded }
    else if width_dp < WIDTH_LARGE_MAX { WidthClass::Large }
    else { WidthClass::ExtraLarge }
}

/// 高度断点分类。左闭右开区间：`[0, 480)` → Compact, `[480, 900)` → Medium, `[900, ∞)` → Expanded。
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
        assert_eq!(plan.avoid_regions[0].kind, AvoidRegionKind::VerticalHinge);
    }

    #[test]
    fn test_fold_separating_vertical_hinge() {
        let mut m = default_metrics(); m.width_dp = 1200.0;
        m.fold_feature = FoldFeatureInfo {
            state: FoldState::Flat, orientation: FoldOrientation::Vertical,
            is_separating: true, occlusion: FoldOcclusion::None,
            bounds_left_vp: 500.0, bounds_top_vp: 0.0, bounds_right_vp: 520.0, bounds_bottom_vp: 800.0,
        };
        let plan = resolve_layout(&m);
        assert_eq!(plan.avoid_regions.len(), 1);
        assert_eq!(plan.avoid_regions[0].kind, AvoidRegionKind::VerticalHinge);
    }

    #[test]
    fn test_fold_separating_horizontal_hinge() {
        let mut m = default_metrics(); m.width_dp = 1200.0;
        m.fold_feature = FoldFeatureInfo {
            state: FoldState::Flat, orientation: FoldOrientation::Horizontal,
            is_separating: true, occlusion: FoldOcclusion::None,
            bounds_left_vp: 0.0, bounds_top_vp: 500.0, bounds_right_vp: 1000.0, bounds_bottom_vp: 520.0,
        };
        let plan = resolve_layout(&m);
        assert_eq!(plan.avoid_regions.len(), 1);
        assert_eq!(plan.avoid_regions[0].kind, AvoidRegionKind::HorizontalHinge);
    }

    #[test]
    fn test_fold_non_separating_full_occlusion() {
        let mut m = default_metrics(); m.width_dp = 1200.0;
        m.fold_feature = FoldFeatureInfo {
            state: FoldState::Flat, orientation: FoldOrientation::Vertical,
            is_separating: false, occlusion: FoldOcclusion::Full,
            bounds_left_vp: 500.0, bounds_top_vp: 0.0, bounds_right_vp: 520.0, bounds_bottom_vp: 800.0,
        };
        let plan = resolve_layout(&m);
        assert_eq!(plan.avoid_regions.len(), 1);
        assert_eq!(plan.avoid_regions[0].kind, AvoidRegionKind::WindowInset);
    }

    #[test]
    fn test_fold_half_opened_horizontal_single_pane() {
        let mut m = default_metrics(); m.width_dp = 1200.0;
        m.fold_feature = FoldFeatureInfo {
            state: FoldState::HalfOpened, orientation: FoldOrientation::Horizontal,
            is_separating: true, occlusion: FoldOcclusion::None,
            bounds_left_vp: 0.0, bounds_top_vp: 500.0, bounds_right_vp: 1000.0, bounds_bottom_vp: 520.0,
        };
        let plan = resolve_layout(&m);
        assert_eq!(plan.shell_mode, ShellMode::SupportingPane);
        assert_eq!(plan.workspace_pane_mode, WorkspacePaneMode::SinglePane);
    }

    #[test]
    fn test_default_metrics() {
        let m = WindowMetrics::default();
        assert_eq!(m.width_dp, 360.0);
        assert_eq!(m.fold_feature.state, FoldState::None);
    }

    // ── Workbench constraints tests ──

    #[test]
    fn test_workbench_constraints_compact() {
        let m = default_metrics();
        let c = resolve_workbench_constraints(&m);
        assert!(!c.allow_side_dock);
        assert!(!c.allow_both_side_docks);
        assert!(!c.allow_bottom_dock);
    }

    #[test]
    fn test_workbench_constraints_expanded() {
        let mut m = default_metrics(); m.width_dp = 1000.0;
        let c = resolve_workbench_constraints(&m);
        assert!(c.allow_side_dock);
        assert!(!c.allow_both_side_docks);
    }

    #[test]
    fn test_workbench_constraints_large() {
        let mut m = default_metrics(); m.width_dp = 1400.0;
        let c = resolve_workbench_constraints(&m);
        assert!(c.allow_side_dock);
        assert!(c.allow_both_side_docks);
        assert!(c.allow_bottom_dock);
    }
}
