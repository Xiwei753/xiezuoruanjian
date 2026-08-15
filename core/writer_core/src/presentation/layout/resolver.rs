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
//! | Medium           | Medium/Tall  | TwoPane      | ListDetail   | Bottom|
//! | Wide             | *            | TwoPane      | ListDetail   | Side  |
//! | Large            | *            | ThreePane    | ThreePane    | Side  |
//! | ExtraLarge       | *            | ThreePane    | ThreePane    | Side  |
//!
//! `available_pane_count` 不再由端侧提供，而是 Rust 内部推导结果。

use serde::{Deserialize, Serialize};

use super::breakpoints::{classify_height, classify_width, WindowHeightClass, WindowWidthClass};
use super::metrics::LayoutMetrics;
use super::{PrimaryNavigationPlacement, ShellMode, WorkspacePaneMode};

// ========== 输入结构体 ==========

/// 窗口视口 — 平台端测量好的原始窗口尺寸（dp），不再含折叠/指针/键盘等判断。
///
/// 各端只负责测量自己的窗口系统并传入宽高（dp）：
///
/// ```text
/// Android  LocalWindowInfo.containerDpSize → WindowViewportDto
/// Qt       QWindow.width/height / screen dp
/// Harmony  window vp
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowViewport {
    /// 窗口宽度，dp。
    pub width_dp: f32,
    /// 窗口高度， dp。
    pub height_dp: f32,
}

impl Default for WindowViewport {
    fn default() -> Self {
        // 默认按窄窗口（手机竖屏）算，避免测试和未初始化场景误判为多栏。
        Self {
            width_dp: 360.0,
            height_dp: 640.0,
        }
    }
}

// ========== 内部推导 ==========

/// 根据宽度 class 与高度 class 推导壳层与作品面板模式。
///
/// 关键规则：中等宽度但高度过矮（Compact）时不能硬塞双栏，
/// 降级为 SinglePane + SinglePane。
fn derive_pane_modes(
    width_class: WindowWidthClass,
    height_class: WindowHeightClass,
) -> (ShellMode, WorkspacePaneMode) {
    match width_class {
        WindowWidthClass::Narrow => (ShellMode::SinglePane, WorkspacePaneMode::SinglePane),
        WindowWidthClass::Medium => {
            if matches!(height_class, WindowHeightClass::Compact) {
                (ShellMode::SinglePane, WorkspacePaneMode::SinglePane)
            } else {
                (ShellMode::TwoPane, WorkspacePaneMode::ListDetail)
            }
        }
        WindowWidthClass::Wide => (ShellMode::TwoPane, WorkspacePaneMode::ListDetail),
        WindowWidthClass::Large | WindowWidthClass::ExtraLarge => {
            (ShellMode::ThreePane, WorkspacePaneMode::ThreePane)
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
/// 输入只接收 [`WindowViewport`]（宽高 dp），不再接收折叠/指针/键盘等平台判断。
/// 输出 [`super::LayoutContract`] 含壳层模式、作品面板模式、一级导航放置、共用尺寸。
pub fn resolve_layout(viewport: &WindowViewport) -> super::LayoutContract {
    let width_class = classify_width(viewport.width_dp);
    let height_class = classify_height(viewport.height_dp);
    let (shell_mode, workspace_pane_mode) = derive_pane_modes(width_class, height_class);
    let primary_navigation_placement = derive_navigation_placement(width_class);
    super::LayoutContract {
        shell_mode,
        workspace_pane_mode,
        primary_navigation_placement,
        metrics: LayoutMetrics::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{PrimaryNavigationPlacement, ShellMode, WorkspacePaneMode};
    use super::*;

    #[test]
    fn test_narrow_uses_single_pane_and_bottom_nav() {
        let viewport = WindowViewport {
            width_dp: 360.0,
            height_dp: 640.0,
        };
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.shell_mode, ShellMode::SinglePane);
        assert_eq!(contract.workspace_pane_mode, WorkspacePaneMode::SinglePane);
        assert_eq!(
            contract.primary_navigation_placement,
            PrimaryNavigationPlacement::Bottom
        );
    }

    #[test]
    fn test_medium_with_compact_height_falls_back_to_single_pane() {
        // 中等宽度但高度过矮：不能硬塞双栏。
        let viewport = WindowViewport {
            width_dp: 700.0,
            height_dp: 400.0,
        };
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.shell_mode, ShellMode::SinglePane);
        assert_eq!(contract.workspace_pane_mode, WorkspacePaneMode::SinglePane);
        assert_eq!(
            contract.primary_navigation_placement,
            PrimaryNavigationPlacement::Bottom
        );
    }

    #[test]
    fn test_medium_with_medium_height_uses_two_pane_and_bottom_nav() {
        let viewport = WindowViewport {
            width_dp: 700.0,
            height_dp: 600.0,
        };
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.shell_mode, ShellMode::TwoPane);
        assert_eq!(contract.workspace_pane_mode, WorkspacePaneMode::ListDetail);
        assert_eq!(
            contract.primary_navigation_placement,
            PrimaryNavigationPlacement::Bottom
        );
    }

    #[test]
    fn test_wide_uses_two_pane_and_side_nav() {
        let viewport = WindowViewport {
            width_dp: 1000.0,
            height_dp: 800.0,
        };
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.shell_mode, ShellMode::TwoPane);
        assert_eq!(contract.workspace_pane_mode, WorkspacePaneMode::ListDetail);
        assert_eq!(
            contract.primary_navigation_placement,
            PrimaryNavigationPlacement::Side
        );
    }

    #[test]
    fn test_large_uses_three_pane_and_side_nav() {
        let viewport = WindowViewport {
            width_dp: 1400.0,
            height_dp: 900.0,
        };
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.shell_mode, ShellMode::ThreePane);
        assert_eq!(contract.workspace_pane_mode, WorkspacePaneMode::ThreePane);
        assert_eq!(
            contract.primary_navigation_placement,
            PrimaryNavigationPlacement::Side
        );
    }

    #[test]
    fn test_extra_large_uses_three_pane_and_side_nav() {
        let viewport = WindowViewport {
            width_dp: 2000.0,
            height_dp: 1200.0,
        };
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.shell_mode, ShellMode::ThreePane);
        assert_eq!(contract.workspace_pane_mode, WorkspacePaneMode::ThreePane);
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
    }

    #[test]
    fn test_default_viewport_is_narrow() {
        let viewport = WindowViewport::default();
        assert_eq!(viewport.width_dp, 360.0);
        assert_eq!(viewport.height_dp, 640.0);
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.shell_mode, ShellMode::SinglePane);
    }

    #[test]
    fn test_wide_with_compact_height_still_two_pane() {
        // Wide 及以上不再因高度过矮降级（只 Medium 受影响）。
        let viewport = WindowViewport {
            width_dp: 1000.0,
            height_dp: 300.0,
        };
        let contract = resolve_layout(&viewport);
        assert_eq!(contract.shell_mode, ShellMode::TwoPane);
        assert_eq!(contract.workspace_pane_mode, WorkspacePaneMode::ListDetail);
    }
}
