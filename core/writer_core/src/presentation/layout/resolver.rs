//! # 布局解析器 — 窗口尺寸 → 产品壳层契约（#628）
//!
//! `resolve_layout` 只吃 [`WindowViewport`]（原始窗口宽高，dp），
//! 不再接收"Android 已经算好的 paneCount / has_separating_fold / pointer_class /
//! keyboard_visible"。这些平台判断不再参与"窗口尺寸 → 页面结构"。
//!
//! 决策规则（与 Issue #628 评论一致）：
//!
//! | width class      | height class | shell        | workspace    | nav   |
//! |------------------|--------------|--------------|--------------|-------|
//! | Narrow           | *            | SinglePane   | SinglePane   | Bottom|
//! | Medium           | Compact      | SinglePane   | SinglePane   | Bottom|
//! | Medium           | Medium/Tall  | TwoPane      | Workbench   | Bottom|
//! | Wide             | *            | TwoPane      | Workbench   | Side  |
//! | Large            | *            | ThreePane    | Workbench   | Side  |
//! | ExtraLarge       | *            | ThreePane    | Workbench   | Side  |
//!
//! `available_pane_count` 不再由端侧提供，而是 Rust 内部推导结果。
//! `WorkspaceLayoutMode` 只有 `SinglePane` / `Workbench` 两个产品语义变体（#628 验收点 1），
//! 不再输出旧的 `ListDetail` / `ThreePane`。
//!
//! # Workbench 布局计划（#628 评论 5301021120 第 1-2 步）
//!
//! [`resolve_workbench_layout`] 是平台无关纯函数，输入 [`WindowViewport`] +
//! [`WorkbenchVisibility`]，输出 [`WorkbenchLayoutPlan`]（含七个 [`WorkbenchRole`]
//! 的最终 [`LayoutRect`] bounds）。处理全部 `separating == true` 的遮挡：
//! - 越界矩形 clamp 到 viewport；空矩形丢弃；
//! - 按 left_dp 排序后切出全部连续可用垂直列；
//! - 七角色 bounds 都不与任何 separating 相交；
//! - Editor 拿到连续可编辑区域，不跨两个物理区域；
//! - 多 separating 同时存在时同样处理，不退化成单 hinge；
//! - 无遮挡时退化成普通大屏工作台（整列 = viewport）。

use serde::{Deserialize, Serialize};

use super::breakpoints::{classify_height, classify_width, WindowHeightClass, WindowWidthClass};
use super::metrics::LayoutMetrics;
use super::{PrimaryNavigationPlacement, ShellMode, WorkspaceLayoutMode};

// ========== 输入结构体 ==========

/// 平台中立的窗口遮挡输入（#628 验收点 5）。
///
/// 描述窗口中被系统 UI（折叠铰链、状态栏、导航条、IME、悬浮窗等）遮挡的矩形区域。
/// `separating` 表示该遮挡是否把可用区域**分割**成互不连通的两部分
/// （典型场景：折叠屏铰链横贯中间）。
///
/// 各端把平台特定的遮挡几何归一成这个平台中立结构再传给 Core，
/// Core 不再接收"折叠/指针/键盘"等平台判断，只接收遮挡几何。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowOcclusion {
    /// 遮挡矩形左边界，dp。
    pub left_dp: f32,
    /// 遮挡矩形上边界，dp。
    pub top_dp: f32,
    /// 遮挡矩形右边界，dp。
    pub right_dp: f32,
    /// 遮挡矩形下边界，dp。
    pub bottom_dp: f32,
    /// 该遮挡是否把可用区域分割成互不连通的两部分（如折叠屏铰链）。
    pub separating: bool,
}

impl Default for WindowOcclusion {
    fn default() -> Self {
        // 默认无遮挡、非分隔。
        Self {
            left_dp: 0.0,
            top_dp: 0.0,
            right_dp: 0.0,
            bottom_dp: 0.0,
            separating: false,
        }
    }
}

/// 窗口视口 — 平台端测量好的原始窗口尺寸（dp），不再含折叠/指针/键盘等判断。
///
/// 各端只负责测量自己的窗口系统并传入宽高（dp）：
///
/// ```text
/// Android  LocalWindowInfo.containerDpSize → WindowViewportDto
/// Qt       QWindow.width/height / screen dp
/// Harmony  window vp
/// ```
///
/// `occlusions` 描述窗口中被系统 UI 遮挡的区域（#628 验收点 5），
/// 默认为空 Vec（无遮挡）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowViewport {
    /// 窗口宽度，dp。
    pub width_dp: f32,
    /// 窗口高度， dp。
    pub height_dp: f32,
    /// 窗口遮挡区域列表（#628 验收点 5），默认为空。
    pub occlusions: Vec<WindowOcclusion>,
}

impl Default for WindowViewport {
    fn default() -> Self {
        // 默认按窄窗口（手机竖屏）算，避免测试和未初始化场景误判为多栏。
        Self {
            width_dp: 360.0,
            height_dp: 640.0,
            occlusions: Vec::new(),
        }
    }
}

// ========== Workbench 布局计划类型（#628 评论 5301021120 第 1 步） ==========

/// 平台无关的布局矩形（dp 坐标系，左上角原点）。
///
/// 不变量：`right_dp >= left_dp` 且 `bottom_dp >= top_dp`。
/// 零宽度或零高度矩形合法（表示该角色不画）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LayoutRect {
    pub left_dp: f32,
    pub top_dp: f32,
    pub right_dp: f32,
    pub bottom_dp: f32,
}

impl LayoutRect {
    /// 宽度，dp。始终 >= 0。
    pub fn width(&self) -> f32 {
        (self.right_dp - self.left_dp).max(0.0)
    }

    /// 高度，dp。始终 >= 0。
    pub fn height(&self) -> f32 {
        (self.bottom_dp - self.top_dp).max(0.0)
    }

    /// 是否为空矩形（零面积）。
    pub fn is_empty(&self) -> bool {
        self.width() <= 0.0 || self.height() <= 0.0
    }

    /// 该矩形是否与另一矩形相交（含边界相切）。
    pub fn intersects(&self, other: &LayoutRect) -> bool {
        self.left_dp < other.right_dp
            && other.left_dp < self.right_dp
            && self.top_dp < other.bottom_dp
            && other.top_dp < self.bottom_dp
    }
}

impl Default for LayoutRect {
    fn default() -> Self {
        Self {
            left_dp: 0.0,
            top_dp: 0.0,
            right_dp: 0.0,
            bottom_dp: 0.0,
        }
    }
}

/// 工作台角色 — 七个产品语义角色（#628 评论 5301021120 第 1 步）。
///
/// 顺序：Toolbar [Leading][Center][Trailing]，Content [ChapterNavigation][Editor][ToolPane][ToolRail]。
/// 平台端按 [`WorkbenchPlacement`] 的 bounds 放对应 slot，不自行决定角色挪到哪一侧。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WorkbenchRole {
    /// 顶栏左组（返回/撤销/重做/章节栏收起）。
    #[default]
    ToolbarLeading,
    /// 顶栏中组（正文工具区域 content slot）。
    ToolbarCenter,
    /// 顶栏右组（同步/搜索/设置）。
    ToolbarTrailing,
    /// 章节导航（左侧章节树）。
    ChapterNavigation,
    /// 正文编辑器（中央，必须连续可编辑区域）。
    Editor,
    /// 工具面板（右侧工具内容）。
    ToolPane,
    /// 工具栏图标列（最右）。
    ToolRail,
}

/// 单个角色的放置 — 角色与其最终 bounds（dp）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorkbenchPlacement {
    pub role: WorkbenchRole,
    pub bounds: LayoutRect,
}

/// 工作台可见性 — 端侧局部 UI 状态（#628 评论 5301021120 第 1 步）。
///
/// `chapterTreeCollapsed` / `toolPaneCollapsed` 仍是端侧局部 UI 状态；
/// 只把当前 visible 布尔作为纯函数输入，不在 Core 持久化第二份状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WorkbenchVisibility {
    /// 章节导航（章节树）是否可见。
    pub chapter_navigation_visible: bool,
    /// 工具面板是否可见。
    pub tool_pane_visible: bool,
}

/// 工作台布局计划 — `resolve_workbench_layout` 的输出（#628 评论 5301021120 第 1 步）。
///
/// 含七个 [`WorkbenchPlacement`]，平台端按 bounds 放 slot，不再自行推导 hinge 布局。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorkbenchLayoutPlan {
    pub placements: Vec<WorkbenchPlacement>,
}

// ========== 内部推导 ==========

/// 根据宽度 class 与高度 class 推导壳层与工作区布局模式（#628 验收点 1）。
///
/// 关键规则：
/// - Narrow → (SinglePane, SinglePane)。
/// - Medium + Compact → (SinglePane, SinglePane)（中等宽度但高度过矮不能硬塞双栏）。
/// - 其余（Medium + 非 Compact, Wide, Large, ExtraLarge）→ Workbench。
///
/// `ShellMode` 仍保留 SinglePane/SupportingPane/TwoPane/ThreePane 用于壳层框架，
/// 但工作区布局模式（`WorkspaceLayoutMode`）只有 SinglePane/Workbench。
fn derive_pane_modes(
    width_class: WindowWidthClass,
    height_class: WindowHeightClass,
) -> (ShellMode, WorkspaceLayoutMode) {
    match width_class {
        WindowWidthClass::Narrow => (ShellMode::SinglePane, WorkspaceLayoutMode::SinglePane),
        WindowWidthClass::Medium => {
            if matches!(height_class, WindowHeightClass::Compact) {
                (ShellMode::SinglePane, WorkspaceLayoutMode::SinglePane)
            } else {
                (ShellMode::TwoPane, WorkspaceLayoutMode::Workbench)
            }
        }
        WindowWidthClass::Wide => (ShellMode::TwoPane, WorkspaceLayoutMode::Workbench),
        WindowWidthClass::Large | WindowWidthClass::ExtraLarge => {
            (ShellMode::ThreePane, WorkspaceLayoutMode::Workbench)
        }
    }
}

/// 根据宽度 class 推导一级导航放置位置。
///
/// - Narrow / Medium：`Bottom`（手机/小平板用底栏）。
/// - Wide 及以上：`Side`（NavigationRail）。
fn derive_navigation_placement(width_class: WindowWidthClass) -> PrimaryNavigationPlacement {
    match width_class {
        WindowWidthClass::Narrow | WindowWidthClass::Medium => PrimaryNavigationPlacement::Bottom,
        WindowWidthClass::Wide | WindowWidthClass::Large | WindowWidthClass::ExtraLarge => {
            PrimaryNavigationPlacement::Side
        }
    }
}

// ========== 核心纯函数 ==========

/// 根据窗口视口解析布局契约。纯函数，无副作用。
///
/// 输入只接收 [`WindowViewport`]（宽高 dp + 遮挡列表），不再接收折叠/指针/键盘等平台判断。
/// 输出 [`super::LayoutContract`] 含壳层模式、工作区布局模式、一级导航放置、共用尺寸。
///
/// #628 评论 5301021120 第 1 步：删除 `workbench_occlusion` 字段，
/// 工作台布局计划改由 [`resolve_workbench_layout`] 单独提供（平台端按需调用）。
pub fn resolve_layout(viewport: &WindowViewport) -> super::LayoutContract {
    let width_class = classify_width(viewport.width_dp);
    let height_class = classify_height(viewport.height_dp);
    let (shell_mode, workspace_layout_mode) = derive_pane_modes(width_class, height_class);
    let primary_navigation_placement = derive_navigation_placement(width_class);
    super::LayoutContract {
        shell_mode,
        workspace_layout_mode,
        primary_navigation_placement,
        metrics: LayoutMetrics::default(),
    }
}

/// 解析工作台布局计划（#628 评论 5301021120 第 1-2 步）。
///
/// 平台无关纯函数，处理全部 `separating == true` 的遮挡：
///
/// 1. 越界矩形 clamp 到当前 viewport；空矩形丢弃；
/// 2. 只把 `separating == true` 的区域作为不可跨越分隔；
/// 3. 按 `left_dp` 排序后切出全部连续可用垂直列；
/// 4. 根据 [`LayoutMetrics`] 和当前 [`WorkbenchVisibility`] 给七个 [`WorkbenchRole`]
///    计算最终 [`LayoutRect`] bounds；
/// 5. 任意 role 的 bounds 都不与 separating occlusion 相交；
/// 6. Editor 拿到连续可编辑区域，不跨两个物理区域；
/// 7. 多个 separating feature 同时存在时同样处理，不退化成单 hinge。
///
/// 无遮挡时退化成普通大屏工作台（整列 = viewport 宽度）。
/// 有遮挡时只改 bounds，不新增 FoldableScreen/TabletScreen。
///
/// 角色顺序：Toolbar [Leading][Center][Trailing]，Content [ChapterNavigation][Editor][ToolPane][ToolRail]。
pub fn resolve_workbench_layout(
    viewport: &WindowViewport,
    visibility: WorkbenchVisibility,
) -> WorkbenchLayoutPlan {
    let metrics = LayoutMetrics::default();
    let vw = viewport.width_dp.max(0.0);
    let vh = viewport.height_dp.max(0.0);

    let cols = compute_available_columns(&viewport.occlusions, vw);
    let (col_l, col_r) = select_placement_column(&cols, &metrics, &visibility);
    let col_w = (col_r - col_l).max(0.0);

    let toolbar_h = 64.0_f32;
    let content_top = toolbar_h.min(vh);
    let content_bottom = vh;

    let (chapter_nav_bounds, editor_bounds, tool_pane_bounds, tool_rail_bounds) =
        compute_content_role_bounds(
            col_l,
            col_r,
            col_w,
            content_top,
            content_bottom,
            &metrics,
            &visibility,
        );

    let (toolbar_leading_bounds, toolbar_center_bounds, toolbar_trailing_bounds) =
        compute_toolbar_role_bounds(col_l, col_r, col_w, toolbar_h, vh);

    WorkbenchLayoutPlan {
        placements: vec![
            WorkbenchPlacement {
                role: WorkbenchRole::ToolbarLeading,
                bounds: toolbar_leading_bounds,
            },
            WorkbenchPlacement {
                role: WorkbenchRole::ToolbarCenter,
                bounds: toolbar_center_bounds,
            },
            WorkbenchPlacement {
                role: WorkbenchRole::ToolbarTrailing,
                bounds: toolbar_trailing_bounds,
            },
            WorkbenchPlacement {
                role: WorkbenchRole::ChapterNavigation,
                bounds: chapter_nav_bounds,
            },
            WorkbenchPlacement {
                role: WorkbenchRole::Editor,
                bounds: editor_bounds,
            },
            WorkbenchPlacement {
                role: WorkbenchRole::ToolPane,
                bounds: tool_pane_bounds,
            },
            WorkbenchPlacement {
                role: WorkbenchRole::ToolRail,
                bounds: tool_rail_bounds,
            },
        ],
    }
}

/// 收集 separating occlusion，clamp 到 viewport，丢空，合并重叠/相邻区间，
/// 切出全部连续可用垂直列（#628 评论 5301021120 第 2 步）。
fn compute_available_columns(occlusions: &[WindowOcclusion], vw: f32) -> Vec<(f32, f32)> {
    let mut separating: Vec<(f32, f32)> = occlusions
        .iter()
        .filter(|o| o.separating)
        .map(|o| {
            let left = o.left_dp.clamp(0.0, vw);
            let right = o.right_dp.clamp(0.0, vw);
            (left, right)
        })
        .filter(|(l, r)| *r > *l)
        .collect();
    separating.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut merged: Vec<(f32, f32)> = Vec::with_capacity(separating.len());
    for (l, r) in separating {
        if let Some(last) = merged.last_mut() {
            if l <= last.1 {
                last.1 = last.1.max(r);
                continue;
            }
        }
        merged.push((l, r));
    }
    let mut cols: Vec<(f32, f32)> = Vec::with_capacity(merged.len() + 1);
    let mut cursor = 0.0;
    for (l, r) in &merged {
        if *l > cursor {
            cols.push((cursor, *l));
        }
        cursor = cursor.max(*r);
    }
    if cursor < vw {
        cols.push((cursor, vw));
    }
    if cols.is_empty() {
        cols.push((0.0, 0.0));
    }
    cols
}

/// 选择放置列：第一个宽度足够的可用列；全不够则用第一个列（Editor 仍连续）。
fn select_placement_column(
    cols: &[(f32, f32)],
    metrics: &LayoutMetrics,
    visibility: &WorkbenchVisibility,
) -> (f32, f32) {
    let chapter_nav_w = if visibility.chapter_navigation_visible {
        metrics.list_pane_width_dp
    } else {
        0.0
    };
    let tool_pane_w = if visibility.tool_pane_visible {
        metrics.tool_pane_width_dp
    } else {
        0.0
    };
    let needed_min = chapter_nav_w + tool_pane_w + metrics.tool_rail_width_dp + 1.0;
    cols.iter()
        .copied()
        .find(|(l, r)| r - l >= needed_min)
        .unwrap_or(cols[0])
}

/// 计算 Content 四角色 bounds（在 placement_col 内横向排列）。
/// 保证 Editor 至少 1dp 连续宽度；空间不够时按比例缩小两侧。
fn compute_content_role_bounds(
    col_l: f32,
    col_r: f32,
    col_w: f32,
    content_top: f32,
    content_bottom: f32,
    metrics: &LayoutMetrics,
    visibility: &WorkbenchVisibility,
) -> (LayoutRect, LayoutRect, LayoutRect, LayoutRect) {
    let chapter_nav_w_desired = if visibility.chapter_navigation_visible {
        metrics.list_pane_width_dp
    } else {
        0.0
    };
    let tool_pane_w_desired = if visibility.tool_pane_visible {
        metrics.tool_pane_width_dp
    } else {
        0.0
    };
    let tool_rail_w_desired = metrics.tool_rail_width_dp;
    let editor_min_w = 1.0_f32;
    let total_needed =
        chapter_nav_w_desired + tool_pane_w_desired + tool_rail_w_desired + editor_min_w;

    let (chapter_nav_actual, tool_pane_actual, tool_rail_actual) = if col_w >= total_needed {
        (
            chapter_nav_w_desired,
            tool_pane_w_desired,
            tool_rail_w_desired,
        )
    } else {
        let side_budget = (col_w - editor_min_w).max(0.0);
        let total_desired = chapter_nav_w_desired + tool_pane_w_desired + tool_rail_w_desired;
        if total_desired > 0.0 {
            let scale = side_budget / total_desired;
            (
                chapter_nav_w_desired * scale,
                tool_pane_w_desired * scale,
                tool_rail_w_desired * scale,
            )
        } else {
            (0.0, 0.0, 0.0)
        }
    };

    let chapter_nav_right = col_l + chapter_nav_actual;
    let tool_rail_left = col_r - tool_rail_actual;
    let tool_pane_left = tool_rail_left - tool_pane_actual;
    let editor_left = chapter_nav_right;
    let editor_right = tool_pane_left;

    let chapter_nav = LayoutRect {
        left_dp: col_l,
        top_dp: content_top,
        right_dp: chapter_nav_right,
        bottom_dp: content_bottom,
    };
    let editor = LayoutRect {
        left_dp: editor_left,
        top_dp: content_top,
        right_dp: editor_right,
        bottom_dp: content_bottom,
    };
    let tool_pane = LayoutRect {
        left_dp: tool_pane_left,
        top_dp: content_top,
        right_dp: tool_rail_left,
        bottom_dp: content_bottom,
    };
    let tool_rail = LayoutRect {
        left_dp: tool_rail_left,
        top_dp: content_top,
        right_dp: col_r,
        bottom_dp: content_bottom,
    };
    (chapter_nav, editor, tool_pane, tool_rail)
}

/// 计算 Toolbar 三角色 bounds（顶部 toolbar_h 高度，在 placement_col 内横向分配）。
fn compute_toolbar_role_bounds(
    col_l: f32,
    col_r: f32,
    col_w: f32,
    toolbar_h: f32,
    vh: f32,
) -> (LayoutRect, LayoutRect, LayoutRect) {
    let toolbar_leading_w = 200.0_f32.min(col_w);
    let toolbar_trailing_w = 200.0_f32.min((col_w - toolbar_leading_w).max(0.0));
    let toolbar_leading_right = col_l + toolbar_leading_w;
    let toolbar_trailing_left = col_r - toolbar_trailing_w;
    let toolbar_center_left = toolbar_leading_right;
    let toolbar_center_right = toolbar_trailing_left.max(toolbar_center_left);
    let bottom = toolbar_h.min(vh);

    let leading = LayoutRect {
        left_dp: col_l,
        top_dp: 0.0,
        right_dp: toolbar_leading_right,
        bottom_dp: bottom,
    };
    let center = LayoutRect {
        left_dp: toolbar_center_left,
        top_dp: 0.0,
        right_dp: toolbar_center_right,
        bottom_dp: bottom,
    };
    let trailing = LayoutRect {
        left_dp: toolbar_trailing_left,
        top_dp: 0.0,
        right_dp: col_r,
        bottom_dp: bottom,
    };
    (leading, center, trailing)
}

#[cfg(test)]
mod tests {
    use super::super::{PrimaryNavigationPlacement, ShellMode, WorkspaceLayoutMode};
    use super::*;

    /// 测试辅助：构造无遮挡的 viewport。
    fn viewport(width_dp: f32, height_dp: f32) -> WindowViewport {
        WindowViewport {
            width_dp,
            height_dp,
            occlusions: Vec::new(),
        }
    }

    /// 测试辅助：构造一个 separating 垂直铰链（横贯全高）。
    fn vertical_hinge(left: f32, right: f32, height: f32) -> WindowOcclusion {
        WindowOcclusion {
            left_dp: left,
            top_dp: 0.0,
            right_dp: right,
            bottom_dp: height,
            separating: true,
        }
    }

    /// 测试辅助：从 plan 中取出指定角色的 bounds。
    fn bounds_for(plan: &WorkbenchLayoutPlan, role: WorkbenchRole) -> LayoutRect {
        plan.placements
            .iter()
            .find(|p| p.role == role)
            .map(|p| p.bounds)
            .unwrap_or_default()
    }

    /// 测试辅助：断言 plan 中任意 role bounds 都不与任何 separating occlusion 相交。
    fn assert_no_role_intersects_separating(plan: &WorkbenchLayoutPlan, viewport: &WindowViewport) {
        let separating: Vec<LayoutRect> = viewport
            .occlusions
            .iter()
            .filter(|o| o.separating)
            .map(|o| LayoutRect {
                left_dp: o.left_dp,
                top_dp: o.top_dp,
                right_dp: o.right_dp,
                bottom_dp: o.bottom_dp,
            })
            .collect();
        for p in &plan.placements {
            for s in &separating {
                assert!(
                    !p.bounds.intersects(s),
                    "role {:?} bounds {:?} intersects separating {:?}",
                    p.role,
                    p.bounds,
                    s
                );
            }
        }
    }

    #[test]
    fn test_narrow_uses_single_pane_and_bottom_nav() {
        let viewport = viewport(360.0, 640.0);
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.shell_mode, ShellMode::SinglePane);
        assert_eq!(
            contract.workspace_layout_mode,
            WorkspaceLayoutMode::SinglePane
        );
        assert_eq!(
            contract.primary_navigation_placement,
            PrimaryNavigationPlacement::Bottom
        );
    }

    #[test]
    fn test_medium_with_compact_height_falls_back_to_single_pane() {
        // 中等宽度但高度过矮：不能硬塞双栏。
        let viewport = viewport(700.0, 400.0);
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.shell_mode, ShellMode::SinglePane);
        assert_eq!(
            contract.workspace_layout_mode,
            WorkspaceLayoutMode::SinglePane
        );
        assert_eq!(
            contract.primary_navigation_placement,
            PrimaryNavigationPlacement::Bottom
        );
    }

    #[test]
    fn test_medium_with_medium_height_uses_two_pane_and_bottom_nav() {
        let viewport = viewport(700.0, 600.0);
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.shell_mode, ShellMode::TwoPane);
        assert_eq!(
            contract.workspace_layout_mode,
            WorkspaceLayoutMode::Workbench
        );
        assert_eq!(
            contract.primary_navigation_placement,
            PrimaryNavigationPlacement::Bottom
        );
    }

    #[test]
    fn test_wide_uses_two_pane_and_side_nav() {
        let viewport = viewport(1000.0, 800.0);
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.shell_mode, ShellMode::TwoPane);
        assert_eq!(
            contract.workspace_layout_mode,
            WorkspaceLayoutMode::Workbench
        );
        assert_eq!(
            contract.primary_navigation_placement,
            PrimaryNavigationPlacement::Side
        );
    }

    #[test]
    fn test_large_uses_three_pane_and_side_nav() {
        let viewport = viewport(1400.0, 900.0);
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.shell_mode, ShellMode::ThreePane);
        assert_eq!(
            contract.workspace_layout_mode,
            WorkspaceLayoutMode::Workbench
        );
        assert_eq!(
            contract.primary_navigation_placement,
            PrimaryNavigationPlacement::Side
        );
    }

    #[test]
    fn test_extra_large_uses_three_pane_and_side_nav() {
        let viewport = viewport(2000.0, 1200.0);
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.shell_mode, ShellMode::ThreePane);
        assert_eq!(
            contract.workspace_layout_mode,
            WorkspaceLayoutMode::Workbench
        );
        assert_eq!(
            contract.primary_navigation_placement,
            PrimaryNavigationPlacement::Side
        );
    }

    #[test]
    fn test_metrics_are_populated() {
        let viewport = WindowViewport::default();
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.metrics.list_pane_width_dp, 320.0);
        assert_eq!(contract.metrics.project_card_min_width_dp, 180.0);
        assert_eq!(contract.metrics.tool_pane_width_dp, 240.0);
        assert_eq!(contract.metrics.tool_rail_width_dp, 56.0);
    }

    #[test]
    fn test_default_viewport_is_narrow() {
        let viewport = WindowViewport::default();
        assert_eq!(viewport.width_dp, 360.0);
        assert_eq!(viewport.height_dp, 640.0);
        assert!(viewport.occlusions.is_empty());
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.shell_mode, ShellMode::SinglePane);
    }

    #[test]
    fn test_wide_with_compact_height_still_two_pane() {
        // Wide 及以上不再因高度过矮降级（只 Medium 受影响）。
        let viewport = viewport(1000.0, 300.0);
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.shell_mode, ShellMode::TwoPane);
        assert_eq!(
            contract.workspace_layout_mode,
            WorkspaceLayoutMode::Workbench
        );
    }

    #[test]
    fn test_default_occlusion_is_empty_and_non_separating() {
        let o = WindowOcclusion::default();
        assert!(!o.separating);
        assert_eq!(o.left_dp, 0.0);
        assert_eq!(o.top_dp, 0.0);
        assert_eq!(o.right_dp, 0.0);
        assert_eq!(o.bottom_dp, 0.0);
    }

    // ── resolve_workbench_layout 单测（#628 评论 5301021120 第 2 步） ──

    #[test]
    fn test_workbench_plan_has_seven_roles() {
        let viewport = viewport(1000.0, 800.0);
        let plan = resolve_workbench_layout(&viewport, WorkbenchVisibility::default());
        assert_eq!(plan.placements.len(), 7, "plan 必须含七个角色");
        let roles: Vec<WorkbenchRole> = plan.placements.iter().map(|p| p.role).collect();
        assert!(roles.contains(&WorkbenchRole::ToolbarLeading));
        assert!(roles.contains(&WorkbenchRole::ToolbarCenter));
        assert!(roles.contains(&WorkbenchRole::ToolbarTrailing));
        assert!(roles.contains(&WorkbenchRole::ChapterNavigation));
        assert!(roles.contains(&WorkbenchRole::Editor));
        assert!(roles.contains(&WorkbenchRole::ToolPane));
        assert!(roles.contains(&WorkbenchRole::ToolRail));
    }

    #[test]
    fn test_workbench_plan_no_occlusion_degrades_to_full_viewport() {
        // 无遮挡时退化成普通大屏工作台：Editor 占据中间最大区域，
        // 七角色 bounds 都在 viewport 内。
        let viewport = viewport(1000.0, 800.0);
        let plan = resolve_workbench_layout(
            &viewport,
            WorkbenchVisibility {
                chapter_navigation_visible: true,
                tool_pane_visible: true,
            },
        );
        let editor = bounds_for(&plan, WorkbenchRole::Editor);
        // Editor 在章节树右侧、工具面板左侧，宽度 = 1000 - 320 - 240 - 56 = 384。
        assert_eq!(editor.left_dp, 320.0);
        assert_eq!(editor.right_dp, 1000.0 - 240.0 - 56.0);
        assert!(editor.width() > 0.0);
        // 所有 bounds 在 viewport 内。
        for p in &plan.placements {
            assert!(p.bounds.left_dp >= 0.0);
            assert!(p.bounds.right_dp <= 1000.0);
            assert!(p.bounds.top_dp >= 0.0);
            assert!(p.bounds.bottom_dp <= 800.0);
        }
    }

    #[test]
    fn test_workbench_plan_single_separating_editor_does_not_cross_hinge() {
        // 单 separating 铰链：Editor bounds 不与铰链相交。
        let viewport = WindowViewport {
            width_dp: 1000.0,
            height_dp: 800.0,
            occlusions: vec![vertical_hinge(490.0, 510.0, 800.0)],
        };
        let plan = resolve_workbench_layout(
            &viewport,
            WorkbenchVisibility {
                chapter_navigation_visible: true,
                tool_pane_visible: true,
            },
        );
        assert_no_role_intersects_separating(&plan, &viewport);
        let editor = bounds_for(&plan, WorkbenchRole::Editor);
        assert!(editor.width() > 0.0, "Editor 必须有连续可编辑区域");
        // Editor 完全在铰链左侧或右侧。
        assert!(
            editor.right_dp <= 490.0 || editor.left_dp >= 510.0,
            "Editor {:?} 不应跨铰链 [490, 510]",
            editor
        );
    }

    #[test]
    fn test_workbench_plan_multi_separating_all_processed() {
        // 多 separating：全部参与切列，所有 role bounds 不与任一铰链相交。
        let viewport = WindowViewport {
            width_dp: 1000.0,
            height_dp: 800.0,
            occlusions: vec![
                vertical_hinge(300.0, 320.0, 800.0),
                vertical_hinge(700.0, 720.0, 800.0),
            ],
        };
        let plan = resolve_workbench_layout(
            &viewport,
            WorkbenchVisibility {
                chapter_navigation_visible: true,
                tool_pane_visible: true,
            },
        );
        assert_no_role_intersects_separating(&plan, &viewport);
        let editor = bounds_for(&plan, WorkbenchRole::Editor);
        assert!(editor.width() > 0.0, "Editor 必须有连续可编辑区域");
        // Editor 不跨任一铰链。
        assert!(
            editor.right_dp <= 300.0
                || (editor.left_dp >= 320.0 && editor.right_dp <= 700.0)
                || editor.left_dp >= 720.0,
            "Editor {:?} 不应跨任一铰链",
            editor
        );
    }

    #[test]
    fn test_workbench_plan_clamps_out_of_bounds_occlusion() {
        // 越界 separating 矩形 clamp 到 viewport。
        let viewport = WindowViewport {
            width_dp: 1000.0,
            height_dp: 800.0,
            occlusions: vec![WindowOcclusion {
                left_dp: -50.0,
                top_dp: 0.0,
                right_dp: 50.0,
                bottom_dp: 800.0,
                separating: true,
            }],
        };
        let plan = resolve_workbench_layout(
            &viewport,
            WorkbenchVisibility {
                chapter_navigation_visible: true,
                tool_pane_visible: true,
            },
        );
        // clamp 后 separating = [0, 50]，可用列 = [50, 1000]。
        // 所有 role bounds 在 [50, 1000] 内。
        for p in &plan.placements {
            assert!(
                p.bounds.left_dp >= 50.0 || p.bounds.is_empty(),
                "role {:?} bounds {:?} 应在 clamp 后的可用列 [50, 1000] 内",
                p.role,
                p.bounds
            );
        }
    }

    #[test]
    fn test_workbench_plan_drops_empty_occlusion() {
        // 空 separating 矩形（right <= left）丢弃，不影响切列。
        let viewport = WindowViewport {
            width_dp: 1000.0,
            height_dp: 800.0,
            occlusions: vec![WindowOcclusion {
                left_dp: 500.0,
                top_dp: 0.0,
                right_dp: 500.0,
                bottom_dp: 800.0,
                separating: true,
            }],
        };
        let plan = resolve_workbench_layout(
            &viewport,
            WorkbenchVisibility {
                chapter_navigation_visible: true,
                tool_pane_visible: true,
            },
        );
        // 空矩形丢弃后无 separating，Editor 占整个 viewport 中间。
        let editor = bounds_for(&plan, WorkbenchRole::Editor);
        assert_eq!(editor.left_dp, 320.0);
        assert_eq!(editor.right_dp, 1000.0 - 240.0 - 56.0);
    }

    #[test]
    fn test_workbench_plan_editor_is_continuous() {
        // Editor 必须拿到一个连续可编辑区域（bounds 是单一矩形，不跨两个物理区域）。
        let viewport = WindowViewport {
            width_dp: 1500.0,
            height_dp: 1000.0,
            occlusions: vec![
                vertical_hinge(400.0, 420.0, 1000.0),
                vertical_hinge(900.0, 920.0, 1000.0),
                vertical_hinge(1100.0, 1120.0, 1000.0),
            ],
        };
        let plan = resolve_workbench_layout(
            &viewport,
            WorkbenchVisibility {
                chapter_navigation_visible: true,
                tool_pane_visible: true,
            },
        );
        let editor = bounds_for(&plan, WorkbenchRole::Editor);
        // Editor 是单一连续矩形。
        assert!(editor.width() > 0.0);
        assert!(editor.right_dp > editor.left_dp);
        // 不与任一 separating 相交。
        for o in &viewport.occlusions {
            let s = LayoutRect {
                left_dp: o.left_dp,
                top_dp: o.top_dp,
                right_dp: o.right_dp,
                bottom_dp: o.bottom_dp,
            };
            assert!(
                !editor.intersects(&s),
                "Editor {:?} 不应与 {:?} 相交",
                editor,
                s
            );
        }
    }

    #[test]
    fn test_workbench_plan_no_role_bounds_intersect_separating() {
        // 任意 role 的 bounds 都不与 separating occlusion 相交。
        let viewport = WindowViewport {
            width_dp: 1200.0,
            height_dp: 900.0,
            occlusions: vec![
                vertical_hinge(350.0, 370.0, 900.0),
                vertical_hinge(800.0, 820.0, 900.0),
            ],
        };
        let plan = resolve_workbench_layout(
            &viewport,
            WorkbenchVisibility {
                chapter_navigation_visible: true,
                tool_pane_visible: true,
            },
        );
        assert_no_role_intersects_separating(&plan, &viewport);
    }

    #[test]
    fn test_workbench_plan_visibility_controls_chapter_nav_and_tool_pane() {
        // visibility.chapter_navigation_visible=false 时 ChapterNavigation bounds 为空（零宽度）。
        // visibility.tool_pane_visible=false 时 ToolPane bounds 为空。
        let viewport = viewport(1000.0, 800.0);
        let plan = resolve_workbench_layout(
            &viewport,
            WorkbenchVisibility {
                chapter_navigation_visible: false,
                tool_pane_visible: false,
            },
        );
        let chapter_nav = bounds_for(&plan, WorkbenchRole::ChapterNavigation);
        let tool_pane = bounds_for(&plan, WorkbenchRole::ToolPane);
        assert_eq!(
            chapter_nav.width(),
            0.0,
            "chapter_navigation_visible=false 时 ChapterNavigation 宽度应为 0"
        );
        assert_eq!(
            tool_pane.width(),
            0.0,
            "tool_pane_visible=false 时 ToolPane 宽度应为 0"
        );
        // Editor 占据中间更大区域（chapter_nav 和 tool_pane 都收起）。
        let editor = bounds_for(&plan, WorkbenchRole::Editor);
        assert!(
            editor.width() > 600.0,
            "收起 chapter_nav 和 tool_pane 后 Editor 应占更大区域"
        );
    }

    #[test]
    fn test_workbench_plan_visibility_true_draws_chapter_nav_and_tool_pane() {
        // visibility 全 true 时 ChapterNavigation 和 ToolPane 都有正宽度。
        let viewport = viewport(1000.0, 800.0);
        let plan = resolve_workbench_layout(
            &viewport,
            WorkbenchVisibility {
                chapter_navigation_visible: true,
                tool_pane_visible: true,
            },
        );
        let chapter_nav = bounds_for(&plan, WorkbenchRole::ChapterNavigation);
        let tool_pane = bounds_for(&plan, WorkbenchRole::ToolPane);
        assert_eq!(chapter_nav.width(), 320.0);
        assert_eq!(tool_pane.width(), 240.0);
    }

    #[test]
    fn test_workbench_plan_multi_separating_does_not_degrade_to_single_hinge() {
        // 多 separating 时不退化成单 hinge：第二个 hinge 也参与切列，
        // 角色放置在第一个可用列（hinge1 左侧），不跨 hinge1 也不跨 hinge2。
        let hinge1 = vertical_hinge(300.0, 320.0, 800.0);
        let hinge2 = vertical_hinge(700.0, 720.0, 800.0);
        let viewport = WindowViewport {
            width_dp: 1000.0,
            height_dp: 800.0,
            occlusions: vec![hinge1, hinge2],
        };
        let plan = resolve_workbench_layout(
            &viewport,
            WorkbenchVisibility {
                chapter_navigation_visible: true,
                tool_pane_visible: true,
            },
        );
        // 所有人物都不与 hinge1 也不与 hinge2 相交。
        assert_no_role_intersects_separating(&plan, &viewport);

        // 对比：只有 hinge1 时，若 hinge2 也存在，plan 应不同（hinge2 影响可用列切分）。
        let viewport_only_hinge1 = WindowViewport {
            width_dp: 1000.0,
            height_dp: 800.0,
            occlusions: vec![hinge1],
        };
        let plan_only_hinge1 = resolve_workbench_layout(
            &viewport_only_hinge1,
            WorkbenchVisibility {
                chapter_navigation_visible: true,
                tool_pane_visible: true,
            },
        );
        // 两个 plan 的 placements 不应完全相同（hinge2 改变了可用列结构）。
        let editor_both = bounds_for(&plan, WorkbenchRole::Editor);
        let editor_only_h1 = bounds_for(&plan_only_hinge1, WorkbenchRole::Editor);
        // hinge1 only: 可用列 = [0,300] + [320,1000]，placement_col 选第一个 >= needed_min 的列。
        //   needed_min = 320 + 1 + 240 + 56 = 617。[320,1000] 宽 680 >= 617，选 [320,1000]。
        // hinge1+hinge2: 可用列 = [0,300] + [320,700] + [720,1000]。
        //   [320,700] 宽 380 < 617，[720,1000] 宽 280 < 617，[0,300] 宽 300 < 617。
        //   全部不够，fallback cols[0] = [0,300]，Editor 在 [0,300] 内。
        // 两者 Editor bounds 不同 → 证明 hinge2 影响了输出，不是死数据。
        assert_ne!(
            editor_both, editor_only_h1,
            "多 separating 时 plan 不应与单 hinge 相同 — 第二个 hinge 不是死数据"
        );
    }

    #[test]
    fn test_workbench_plan_overlapping_separating_merged() {
        // 重叠的 separating 区间合并成一个，避免重复切列。
        let viewport = WindowViewport {
            width_dp: 1000.0,
            height_dp: 800.0,
            occlusions: vec![
                vertical_hinge(300.0, 400.0, 800.0),
                vertical_hinge(350.0, 450.0, 800.0),
            ],
        };
        let plan = resolve_workbench_layout(
            &viewport,
            WorkbenchVisibility {
                chapter_navigation_visible: true,
                tool_pane_visible: true,
            },
        );
        // 合并后 separating = [300, 450]，可用列 = [0,300] + [450,1000]。
        assert_no_role_intersects_separating(&plan, &viewport);
    }

    #[test]
    fn test_layout_rect_intersects() {
        let a = LayoutRect {
            left_dp: 0.0,
            top_dp: 0.0,
            right_dp: 100.0,
            bottom_dp: 100.0,
        };
        let b = LayoutRect {
            left_dp: 50.0,
            top_dp: 50.0,
            right_dp: 150.0,
            bottom_dp: 150.0,
        };
        assert!(a.intersects(&b));
        let c = LayoutRect {
            left_dp: 200.0,
            top_dp: 0.0,
            right_dp: 300.0,
            bottom_dp: 100.0,
        };
        assert!(!a.intersects(&c));
    }

    #[test]
    fn test_layout_rect_is_empty() {
        let empty = LayoutRect {
            left_dp: 50.0,
            top_dp: 0.0,
            right_dp: 50.0,
            bottom_dp: 100.0,
        };
        assert!(empty.is_empty());
        let non_empty = LayoutRect {
            left_dp: 0.0,
            top_dp: 0.0,
            right_dp: 100.0,
            bottom_dp: 100.0,
        };
        assert!(!non_empty.is_empty());
    }
}
