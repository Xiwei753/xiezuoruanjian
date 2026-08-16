//! #628 评论 5301021120 第 3 步 + 02:59:39Z 版：workbench plan DTO 测试。
//! 独立 _tests.rs 文件（结构守卫 production-test-bloat：生产文件内嵌测试模块 >100 行应拆分）。

use super::workbench_plan::*;
// 跨模块 DTO：WindowViewportDto / WindowOcclusionDto 留在 platform.rs。
use crate::api::types::platform::{WindowOcclusionDto, WindowViewportDto};

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
fn test_resolved_workspace_mode_dto_roundtrip() {
    use crate::presentation::layout::resolver::ResolvedWorkspaceMode as R;
    for m in [R::Workbench, R::SinglePane] {
        let dto: ResolvedWorkspaceModeDto = m.into();
        let back: R = dto.into();
        assert_eq!(back, m);
    }
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
        mode: crate::presentation::layout::resolver::ResolvedWorkspaceMode::SinglePane,
    };
    let dto: WorkbenchLayoutPlanDto = plan.clone().into();
    let back: crate::presentation::layout::resolver::WorkbenchLayoutPlan = dto.into();
    assert_eq!(back, plan);
    assert_eq!(back.placements.len(), 1);
    assert_eq!(
        back.placements[0].role,
        crate::presentation::layout::resolver::WorkbenchRole::Editor
    );
    assert_eq!(
        back.mode,
        crate::presentation::layout::resolver::ResolvedWorkspaceMode::SinglePane
    );
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
