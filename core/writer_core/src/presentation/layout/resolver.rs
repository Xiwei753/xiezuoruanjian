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
/// 输出 [`super::LayoutContract`] 含壳层模式、工作区布局模式、一级导航放置、共用尺寸、
/// 工作台分隔遮挡（#628 验收点 5）。
///
/// 遮挡处理：当工作区为 `Workbench` 且 `occlusions` 中存在 `separating == true` 的遮挡时，
/// 取第一个 separating 遮挡作为 `workbench_occlusion`，平台端据此避免正文/控件跨在 hinge 上。
/// 其余情况（SinglePane 或无 separating 遮挡）`workbench_occlusion` 为 `None`。
pub fn resolve_layout(viewport: &WindowViewport) -> super::LayoutContract {
    let width_class = classify_width(viewport.width_dp);
    let height_class = classify_height(viewport.height_dp);
    let (shell_mode, workspace_layout_mode) = derive_pane_modes(width_class, height_class);
    let primary_navigation_placement = derive_navigation_placement(width_class);
    let workbench_occlusion =
        derive_workbench_occlusion(workspace_layout_mode, &viewport.occlusions);
    super::LayoutContract {
        shell_mode,
        workspace_layout_mode,
        primary_navigation_placement,
        metrics: LayoutMetrics::default(),
        workbench_occlusion,
    }
}

/// 根据工作区布局模式与遮挡列表推导工作台分隔遮挡（#628 验收点 5）。
///
/// 仅在 `Workbench` 模式下，若存在 `separating == true` 的遮挡，取第一个作为
/// `workbench_occlusion`。其余情况返回 `None`。
///
/// 纯函数，无副作用：不修改输入，不访问外部状态。
fn derive_workbench_occlusion(
    workspace_layout_mode: WorkspaceLayoutMode,
    occlusions: &[WindowOcclusion],
) -> Option<WindowOcclusion> {
    if matches!(workspace_layout_mode, WorkspaceLayoutMode::Workbench) {
        occlusions.iter().copied().find(|o| o.separating)
    } else {
        None
    }
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

    #[test]
    fn test_workbench_occlusion_none_when_no_separating_occlusion() {
        // Workbench 模式但无 separating 遮挡 → workbench_occlusion 为 None。
        let viewport = viewport(1000.0, 800.0);
        let contract = resolve_layout(&viewport);
        assert_eq!(
            contract.workspace_layout_mode,
            WorkspaceLayoutMode::Workbench
        );
        assert!(contract.workbench_occlusion.is_none());
    }

    #[test]
    fn test_workbench_occlusion_some_when_separating_occlusion_present() {
        // Workbench 模式且有 separating 遮挡 → workbench_occlusion 记录该遮挡。
        let occlusion = WindowOcclusion {
            left_dp: 700.0,
            top_dp: 0.0,
            right_dp: 720.0,
            bottom_dp: 800.0,
            separating: true,
        };
        let viewport = WindowViewport {
            width_dp: 1000.0,
            height_dp: 800.0,
            occlusions: vec![occlusion],
        };
        let contract = resolve_layout(&viewport);
        assert_eq!(
            contract.workspace_layout_mode,
            WorkspaceLayoutMode::Workbench
        );
        assert_eq!(contract.workbench_occlusion, Some(occlusion));
    }

    #[test]
    fn test_workbench_occlusion_none_in_single_pane_even_with_separating_occlusion() {
        // SinglePane 模式即使有 separating 遮挡也不产生 workbench_occlusion。
        let occlusion = WindowOcclusion {
            left_dp: 100.0,
            top_dp: 0.0,
            right_dp: 120.0,
            bottom_dp: 640.0,
            separating: true,
        };
        let viewport = WindowViewport {
            width_dp: 360.0,
            height_dp: 640.0,
            occlusions: vec![occlusion],
        };
        let contract = resolve_layout(&viewport);
        assert_eq!(
            contract.workspace_layout_mode,
            WorkspaceLayoutMode::SinglePane
        );
        assert!(contract.workbench_occlusion.is_none());
    }

    #[test]
    fn test_workbench_occlusion_ignores_non_separating_occlusion() {
        // 非 separating 遮挡不作为 workbench_occlusion。
        let occlusion = WindowOcclusion {
            left_dp: 0.0,
            top_dp: 0.0,
            right_dp: 1000.0,
            bottom_dp: 24.0,
            separating: false,
        };
        let viewport = WindowViewport {
            width_dp: 1000.0,
            height_dp: 800.0,
            occlusions: vec![occlusion],
        };
        let contract = resolve_layout(&viewport);
        assert_eq!(
            contract.workspace_layout_mode,
            WorkspaceLayoutMode::Workbench
        );
        assert!(contract.workbench_occlusion.is_none());
    }

    #[test]
    fn test_workbench_occlusion_picks_first_separating() {
        // 多个遮挡时取第一个 separating 的。
        let non_separating = WindowOcclusion {
            left_dp: 0.0,
            top_dp: 0.0,
            right_dp: 1000.0,
            bottom_dp: 24.0,
            separating: false,
        };
        let separating = WindowOcclusion {
            left_dp: 700.0,
            top_dp: 0.0,
            right_dp: 720.0,
            bottom_dp: 800.0,
            separating: true,
        };
        let viewport = WindowViewport {
            width_dp: 1000.0,
            height_dp: 800.0,
            occlusions: vec![non_separating, separating],
        };
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.workbench_occlusion, Some(separating));
    }
}
