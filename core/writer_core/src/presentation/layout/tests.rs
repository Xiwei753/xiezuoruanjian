//! #628：layout 模块整合测试 — 覆盖 resolve_layout 端到端决策表。
//!
//! 子模块 breakpoints/metrics/resolver 各自有单元测试，
//! 这里只做端到端契约检查（避免重复真相）。

use super::resolver::WindowViewport;
use super::{
    resolve_layout, LayoutContract, PrimaryNavigationPlacement, ShellMode, WorkspaceLayoutMode,
};

/// 测试辅助：构造无遮挡的 viewport。
fn viewport(width_dp: f32, height_dp: f32) -> WindowViewport {
    WindowViewport {
        width_dp,
        height_dp,
        occlusions: Vec::new(),
    }
}

#[test]
fn test_resolve_layout_returns_full_contract() {
    let viewport = viewport(1000.0, 800.0);
    let contract: LayoutContract = resolve_layout(&viewport);
    assert_eq!(contract.shell_mode, ShellMode::TwoPane);
    assert_eq!(
        contract.workspace_layout_mode,
        WorkspaceLayoutMode::Workbench
    );
    assert_eq!(
        contract.primary_navigation_placement,
        PrimaryNavigationPlacement::Side
    );
    assert_eq!(contract.metrics.list_pane_width_dp, 320.0);
}

#[test]
fn test_resolve_layout_decision_table_smoke() {
    // 决策表抽样：覆盖 5 个宽度 class 与 Compact/Medium/Tall 高度的关键组合。
    let cases: &[(
        f32,
        f32,
        ShellMode,
        WorkspaceLayoutMode,
        PrimaryNavigationPlacement,
    )] = &[
        // Narrow
        (
            360.0,
            640.0,
            ShellMode::SinglePane,
            WorkspaceLayoutMode::SinglePane,
            PrimaryNavigationPlacement::Bottom,
        ),
        // Medium + Compact → 降级
        (
            700.0,
            400.0,
            ShellMode::SinglePane,
            WorkspaceLayoutMode::SinglePane,
            PrimaryNavigationPlacement::Bottom,
        ),
        // Medium + Medium
        (
            700.0,
            600.0,
            ShellMode::TwoPane,
            WorkspaceLayoutMode::Workbench,
            PrimaryNavigationPlacement::Bottom,
        ),
        // Medium + Tall
        (
            700.0,
            1000.0,
            ShellMode::TwoPane,
            WorkspaceLayoutMode::Workbench,
            PrimaryNavigationPlacement::Bottom,
        ),
        // Wide
        (
            1000.0,
            800.0,
            ShellMode::TwoPane,
            WorkspaceLayoutMode::Workbench,
            PrimaryNavigationPlacement::Side,
        ),
        // Large
        (
            1400.0,
            900.0,
            ShellMode::ThreePane,
            WorkspaceLayoutMode::Workbench,
            PrimaryNavigationPlacement::Side,
        ),
        // ExtraLarge
        (
            2000.0,
            1200.0,
            ShellMode::ThreePane,
            WorkspaceLayoutMode::Workbench,
            PrimaryNavigationPlacement::Side,
        ),
    ];
    for &(w, h, shell, workspace, nav) in cases {
        let viewport = viewport(w, h);
        let contract = resolve_layout(&viewport);
        assert_eq!(
            contract.shell_mode, shell,
            "width={w}, height={h} shell mismatch"
        );
        assert_eq!(
            contract.workspace_layout_mode, workspace,
            "width={w}, height={h} workspace mismatch"
        );
        assert_eq!(
            contract.primary_navigation_placement, nav,
            "width={w}, height={h} nav placement mismatch"
        );
    }
}
