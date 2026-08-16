//! platform DTO 单元测试 — 从 platform.rs 内嵌测试模块提取（#629 源码结构门禁）。
//!
//! 覆盖各 Layout DTO 的双向 From 转换、默认值与 resolve_workbench_layout 端到端契约。

use super::platform::*;

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
        editor_min_width_dp: 240.0,
        toolbar_height_dp: 64.0,
        toolbar_leading_width_dp: 200.0,
        toolbar_trailing_width_dp: 200.0,
        list_pane_min_width_dp: 200.0,
        tool_pane_min_width_dp: 200.0,
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
    assert_eq!(dto.editor_min_width_dp, 240.0);
    assert_eq!(dto.toolbar_height_dp, 64.0);
    assert_eq!(dto.toolbar_leading_width_dp, 200.0);
    assert_eq!(dto.toolbar_trailing_width_dp, 200.0);
    assert_eq!(dto.list_pane_min_width_dp, 200.0);
    assert_eq!(dto.tool_pane_min_width_dp, 200.0);
}

#[test]
fn test_layout_metrics_dto_camel_case_fields() {
    let dto = LayoutMetricsDto::default();
    let json = serde_json::to_string(&dto).unwrap();
    assert!(json.contains("\"listPaneWidthDp\""));
    assert!(json.contains("\"projectCardMinWidthDp\""));
    assert!(json.contains("\"toolPaneWidthDp\""));
    assert!(json.contains("\"toolRailWidthDp\""));
    assert!(json.contains("\"editorMinWidthDp\""));
    assert!(json.contains("\"toolbarHeightDp\""));
    assert!(json.contains("\"toolbarLeadingWidthDp\""));
    assert!(json.contains("\"toolbarTrailingWidthDp\""));
    assert!(json.contains("\"listPaneMinWidthDp\""));
    assert!(json.contains("\"toolPaneMinWidthDp\""));
}

#[test]
fn test_layout_contract_dto_roundtrip() {
    let contract = crate::presentation::layout::LayoutContract {
        shell_mode: crate::presentation::layout::ShellMode::TwoPane,
        workspace_layout_mode: crate::presentation::layout::WorkspaceLayoutMode::Workbench,
        primary_navigation_placement: crate::presentation::layout::PrimaryNavigationPlacement::Side,
        metrics: crate::presentation::layout::metrics::LayoutMetrics {
            list_pane_width_dp: 320.0,
            project_card_min_width_dp: 180.0,
            tool_pane_width_dp: 240.0,
            tool_rail_width_dp: 56.0,
            editor_min_width_dp: 240.0,
            toolbar_height_dp: 64.0,
            toolbar_leading_width_dp: 200.0,
            toolbar_trailing_width_dp: 200.0,
            list_pane_min_width_dp: 200.0,
            tool_pane_min_width_dp: 200.0,
        },
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
}

#[test]
fn test_layout_contract_dto_no_legacy_fields() {
    // #628：LayoutContractDto 不得再含 showPrimaryNavigation（改由 ScreenPolicy 提供），
    // 也不得再含旧字段 workspacePaneMode（已重命名为 workspaceLayoutMode）。
    // #628 评论 5301021120 第 1 步：不得再含 workbenchOcclusion（已删除）。
    let contract = crate::presentation::layout::LayoutContract {
        shell_mode: crate::presentation::layout::ShellMode::SinglePane,
        workspace_layout_mode: crate::presentation::layout::WorkspaceLayoutMode::SinglePane,
        primary_navigation_placement:
            crate::presentation::layout::PrimaryNavigationPlacement::Bottom,
        metrics: crate::presentation::layout::metrics::LayoutMetrics::default(),
    };
    let dto: LayoutContractDto = contract.into();
    let json = serde_json::to_string(&dto).unwrap();
    assert!(!json.contains("showPrimaryNavigation"));
    assert!(!json.contains("workspacePaneMode"));
    assert!(json.contains("\"workspaceLayoutMode\""));
    assert!(json.contains("\"primaryNavigationPlacement\""));
    assert!(json.contains("\"metrics\""));
    // #628 评论 5301021120 第 1 步：workbenchOcclusion 已删除。
    assert!(!json.contains("workbenchOcclusion"));
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
    assert_eq!(contract_dto.metrics.editor_min_width_dp, 240.0);
    assert_eq!(contract_dto.metrics.toolbar_height_dp, 64.0);
    assert_eq!(contract_dto.metrics.toolbar_leading_width_dp, 200.0);
    assert_eq!(contract_dto.metrics.toolbar_trailing_width_dp, 200.0);
    assert_eq!(contract_dto.metrics.list_pane_min_width_dp, 200.0);
    assert_eq!(contract_dto.metrics.tool_pane_min_width_dp, 200.0);
}

// ── Workbench Layout Plan DTO 测试（#628 评论 5301021120 第 3 步） ──

#[test]
fn test_layout_rect_dto_roundtrip() {
    let r = crate::presentation::layout::resolver::LayoutRect {
        left_dp: 10.0,
        top_dp: 20.0,
        right_dp: 100.0,
        bottom_dp: 200.0,
    };
    let dto: LayoutRectDto = r.into();
    let back: crate::presentation::layout::resolver::LayoutRect = dto.into();
    assert_eq!(back, r);
}

#[test]
fn test_workbench_role_dto_roundtrip() {
    use crate::presentation::layout::resolver::WorkbenchRole as R;
    for r in [
        R::ToolbarLeading,
        R::ToolbarCenter,
        R::ToolbarTrailing,
        R::ChapterNavigation,
        R::Editor,
        R::ToolPane,
        R::ToolRail,
    ] {
        let dto: WorkbenchRoleDto = r.into();
        let back: R = dto.into();
        assert_eq!(back, r);
    }
}

#[test]
fn test_workbench_visibility_dto_roundtrip() {
    let v = crate::presentation::layout::resolver::WorkbenchVisibility {
        chapter_navigation_visible: true,
        tool_pane_visible: false,
    };
    let dto: WorkbenchVisibilityDto = v.into();
    let back: crate::presentation::layout::resolver::WorkbenchVisibility = dto.into();
    assert_eq!(back, v);
}

#[test]
fn test_workbench_placement_dto_roundtrip() {
    let p = crate::presentation::layout::resolver::WorkbenchPlacement {
        role: crate::presentation::layout::resolver::WorkbenchRole::Editor,
        bounds: crate::presentation::layout::resolver::LayoutRect {
            left_dp: 320.0,
            top_dp: 64.0,
            right_dp: 700.0,
            bottom_dp: 800.0,
        },
    };
    let dto: WorkbenchPlacementDto = p.into();
    let back: crate::presentation::layout::resolver::WorkbenchPlacement = dto.into();
    assert_eq!(back, p);
}

#[test]
fn test_workbench_layout_plan_dto_roundtrip() {
    let plan = crate::presentation::layout::resolver::WorkbenchLayoutPlan {
        placements: vec![crate::presentation::layout::resolver::WorkbenchPlacement {
            role: crate::presentation::layout::resolver::WorkbenchRole::Editor,
            bounds: crate::presentation::layout::resolver::LayoutRect {
                left_dp: 0.0,
                top_dp: 64.0,
                right_dp: 1000.0,
                bottom_dp: 800.0,
            },
        }],
        valid: true,
    };
    let dto: WorkbenchLayoutPlanDto = plan.clone().into();
    let back: crate::presentation::layout::resolver::WorkbenchLayoutPlan = dto.into();
    assert_eq!(back, plan);
    assert_eq!(back.placements.len(), 1);
    assert_eq!(
        back.placements[0].role,
        crate::presentation::layout::resolver::WorkbenchRole::Editor
    );
    assert!(back.valid);
}

#[test]
fn test_resolve_workbench_layout_end_to_end_through_dto() {
    // 端到端：WindowViewportDto + WorkbenchVisibilityDto → Core → WorkbenchLayoutPlanDto。
    let viewport_dto = WindowViewportDto {
        width_dp: 1000.0,
        height_dp: 800.0,
        occlusions: vec![WindowOcclusionDto {
            left_dp: 490.0,
            top_dp: 0.0,
            right_dp: 510.0,
            bottom_dp: 800.0,
            separating: true,
        }],
    };
    let visibility_dto = WorkbenchVisibilityDto {
        chapter_navigation_visible: true,
        tool_pane_visible: true,
    };
    let viewport: crate::presentation::layout::resolver::WindowViewport = viewport_dto.into();
    let visibility: crate::presentation::layout::resolver::WorkbenchVisibility =
        visibility_dto.into();
    let plan = crate::presentation::layout::resolve_workbench_layout(&viewport, visibility);
    let plan_dto: WorkbenchLayoutPlanDto = plan.into();
    // 七角色。
    assert_eq!(plan_dto.placements.len(), 7);
    // Editor bounds 不与 separating [490, 510] 相交。
    let editor = plan_dto
        .placements
        .iter()
        .find(|p| p.role == WorkbenchRoleDto::Editor)
        .unwrap();
    assert!(
        editor.bounds.right_dp <= 490.0 || editor.bounds.left_dp >= 510.0,
        "Editor {:?} 不应跨铰链",
        editor.bounds
    );
}
