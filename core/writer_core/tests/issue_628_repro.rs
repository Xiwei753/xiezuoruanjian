//! Issue #628 正向回归测试 — 验证 `resolve_workbench_layout` 处理全部遮挡，
//! 七角色 bounds 不与任一 separating occlusion 相交，Editor 拿到连续可编辑区域。
//!
//! 修复前（缺陷）：`derive_workbench_occlusion` 用 `find()` 只取第一个 separating，
//! `LayoutContract.workbench_occlusion` 是 `Option<WindowOcclusion>`（单数），
//! 结构上无法表达多个遮挡；Android 端无消费路径。
//!
//! 修复后（正向）：删除 `workbench_occlusion`，新增 `resolve_workbench_layout`
//! 纯函数返回 `WorkbenchLayoutPlan`（含七角色 bounds），处理全部 separating occlusion。

use writer_core::presentation::layout::resolve_workbench_layout;
use writer_core::presentation::layout::resolver::{
    LayoutRect, WindowOcclusion, WindowViewport, WorkbenchRole, WorkbenchVisibility,
};

/// 构造一个 separating 遮挡（垂直铰链，横贯全高）。
fn vertical_hinge(left: f32, right: f32, height: f32) -> WindowOcclusion {
    WindowOcclusion {
        left_dp: left,
        top_dp: 0.0,
        right_dp: right,
        bottom_dp: height,
        separating: true,
    }
}

/// 从 plan 中取出指定角色的 bounds。
fn bounds_for(
    plan: &writer_core::presentation::layout::resolver::WorkbenchLayoutPlan,
    role: WorkbenchRole,
) -> LayoutRect {
    plan.placements
        .iter()
        .find(|p| p.role == role)
        .map(|p| p.bounds)
        .unwrap_or_default()
}

/// 断言 plan 中任意 role bounds 都不与任何 separating occlusion 相交。
fn assert_no_role_intersects_separating(
    plan: &writer_core::presentation::layout::resolver::WorkbenchLayoutPlan,
    viewport: &WindowViewport,
) {
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
fn issue_628_plan_has_seven_roles() {
    // 正向：plan 含七个角色。
    let viewport = WindowViewport {
        width_dp: 1000.0,
        height_dp: 800.0,
        occlusions: Vec::new(),
    };
    let plan = resolve_workbench_layout(&viewport, WorkbenchVisibility::default());
    assert_eq!(plan.placements.len(), 7);
}

#[test]
fn issue_628_multi_separating_all_processed_editor_continuous() {
    // 正向回归：多个 separating hinge 全部处理，Editor 连续不跨任一 hinge。
    //   场景：Wide 工作台 (1000x800 dp)，含两个 separating 垂直铰链。
    //     hinge1 在 x=300..320
    //     hinge2 在 x=700..720
    //   修复前：只处理 hinge1，hinge2 是死数据。
    //   修复后：hinge1 和 hinge2 都参与切列，七角色 bounds 不与任一 hinge 相交。
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

    // 1. 全部 separating 都被处理：七角色 bounds 不与 hinge1 也不与 hinge2 相交。
    assert_no_role_intersects_separating(&plan, &viewport);

    // 2. Editor 拿到连续可编辑区域（单一矩形，不跨两个物理区域）。
    let editor = bounds_for(&plan, WorkbenchRole::Editor);
    assert!(editor.width() > 0.0, "Editor 必须有连续可编辑区域");
    assert!(editor.right_dp > editor.left_dp);

    // 3. Editor 不跨任一铰链。
    assert!(
        editor.right_dp <= 300.0
            || (editor.left_dp >= 320.0 && editor.right_dp <= 700.0)
            || editor.left_dp >= 720.0,
        "Editor {:?} 不应跨任一铰链",
        editor
    );
}

#[test]
fn issue_628_second_hinge_affects_output_not_dead_data() {
    // 正向回归：第二个 hinge 影响输出，不是死数据。
    //   修复前：加第二个 hinge 后输出不变（只看第一个）。
    //   修复后：加第二个 hinge 后 plan 改变（hinge2 改变可用列结构）。
    let hinge1 = vertical_hinge(300.0, 320.0, 800.0);
    let hinge2 = vertical_hinge(700.0, 720.0, 800.0);

    let viewport_only_h1 = WindowViewport {
        width_dp: 1000.0,
        height_dp: 800.0,
        occlusions: vec![hinge1],
    };
    let viewport_both = WindowViewport {
        width_dp: 1000.0,
        height_dp: 800.0,
        occlusions: vec![hinge1, hinge2],
    };

    let visibility = WorkbenchVisibility {
        chapter_navigation_visible: true,
        tool_pane_visible: true,
    };
    let plan_only_h1 = resolve_workbench_layout(&viewport_only_h1, visibility);
    let plan_both = resolve_workbench_layout(&viewport_both, visibility);

    let editor_only_h1 = bounds_for(&plan_only_h1, WorkbenchRole::Editor);
    let editor_both = bounds_for(&plan_both, WorkbenchRole::Editor);

    // 两个 plan 的 Editor bounds 不同 → 证明 hinge2 影响了输出，不是死数据。
    assert_ne!(
        editor_both, editor_only_h1,
        "加第二个 separating hinge 后 plan 应改变 — 第二个 hinge 不是死数据"
    );
}

#[test]
fn issue_628_no_occlusion_degrades_to_full_workbench() {
    // 正向：无遮挡时退化成普通大屏工作台。
    let viewport = WindowViewport {
        width_dp: 1000.0,
        height_dp: 800.0,
        occlusions: Vec::new(),
    };
    let plan = resolve_workbench_layout(
        &viewport,
        WorkbenchVisibility {
            chapter_navigation_visible: true,
            tool_pane_visible: true,
        },
    );
    let editor = bounds_for(&plan, WorkbenchRole::Editor);
    // Editor 在章节树 (320) 右侧、工具面板 (240) + 工具栏 (56) 左侧。
    assert_eq!(editor.left_dp, 320.0);
    assert_eq!(editor.right_dp, 1000.0 - 240.0 - 56.0);
    assert!(editor.width() > 0.0);
}

#[test]
fn issue_628_visibility_controls_chapter_nav_and_tool_pane() {
    // 正向：visibility 控制章节导航和工具面板可见性。
    let viewport = WindowViewport {
        width_dp: 1000.0,
        height_dp: 800.0,
        occlusions: Vec::new(),
    };
    let plan = resolve_workbench_layout(
        &viewport,
        WorkbenchVisibility {
            chapter_navigation_visible: false,
            tool_pane_visible: false,
        },
    );
    let chapter_nav = bounds_for(&plan, WorkbenchRole::ChapterNavigation);
    let tool_pane = bounds_for(&plan, WorkbenchRole::ToolPane);
    assert_eq!(chapter_nav.width(), 0.0);
    assert_eq!(tool_pane.width(), 0.0);
}

#[test]
fn issue_628_clamps_out_of_bounds_and_drops_empty() {
    // 正向：越界矩形 clamp 到 viewport，空矩形丢弃。
    let viewport = WindowViewport {
        width_dp: 1000.0,
        height_dp: 800.0,
        occlusions: vec![
            // 越界：clamp 到 [0, 50]。
            WindowOcclusion {
                left_dp: -50.0,
                top_dp: 0.0,
                right_dp: 50.0,
                bottom_dp: 800.0,
                separating: true,
            },
            // 空矩形：丢弃。
            WindowOcclusion {
                left_dp: 500.0,
                top_dp: 0.0,
                right_dp: 500.0,
                bottom_dp: 800.0,
                separating: true,
            },
        ],
    };
    let plan = resolve_workbench_layout(
        &viewport,
        WorkbenchVisibility {
            chapter_navigation_visible: true,
            tool_pane_visible: true,
        },
    );
    // clamp 后 separating = [0, 50]，所有 role bounds 在 [50, 1000] 内或为空。
    for p in &plan.placements {
        assert!(
            p.bounds.left_dp >= 50.0 || p.bounds.is_empty(),
            "role {:?} bounds {:?} 应在 clamp 后的可用列 [50, 1000] 内",
            p.role,
            p.bounds
        );
    }
}
