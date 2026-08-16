//! resolver 单元测试 — 从 resolver.rs 内嵌测试模块提取（#629 源码结构门禁）。
//!
//! 覆盖 resolve_layout 决策表与 resolve_workbench_layout 二维 free-region 遮挡/七角色 bounds。
//! workbench 计算已拆到 [`super::workbench`]，测试通过公共 API 验证。

use super::resolver::*;
use super::workbench::resolve_workbench_layout;
use super::{PrimaryNavigationPlacement, ShellMode, WorkspaceLayoutMode};

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

// ── #628 评论 5301021120 问题 2：二维 free-region 三类场景测试 ──

/// 测试辅助：构造一个 separating 横向铰链（横贯全宽）。
fn horizontal_hinge(top: f32, bottom: f32, width: f32) -> WindowOcclusion {
    WindowOcclusion {
        left_dp: 0.0,
        top_dp: top,
        right_dp: width,
        bottom_dp: bottom,
        separating: true,
    }
}

/// 测试辅助：断言 plan 中 Editor 非空。
fn assert_editor_non_empty(plan: &WorkbenchLayoutPlan) -> LayoutRect {
    let editor = bounds_for(plan, WorkbenchRole::Editor);
    assert!(
        editor.width() > 0.0 && editor.height() > 0.0,
        "Editor 必须非空，实际 = {:?}",
        editor
    );
    editor
}

#[test]
fn test_workbench_plan_full_height_vertical_hinge_valid() {
    // 场景 1：全高竖直 separating hinge。
    // viewport 2000x1000，hinge [990,1010] 横贯全高。
    // free regions = [0,990]x[0,1000] + [1010,2000]x[0,1000]，
    // 两列都宽 990 >= workbench_min_w=696，plan.valid=true，Editor 连续不跨 hinge。
    let viewport = WindowViewport {
        width_dp: 2000.0,
        height_dp: 1000.0,
        occlusions: vec![vertical_hinge(990.0, 1010.0, 1000.0)],
    };
    let plan = resolve_workbench_layout(
        &viewport,
        WorkbenchVisibility {
            chapter_navigation_visible: true,
            tool_pane_visible: true,
        },
    );
    assert!(plan.valid, "全高竖直 hinge 两侧都够宽，plan 应 valid=true");
    let editor = assert_editor_non_empty(&plan);
    // Editor 完全在 hinge 左侧或右侧。
    assert!(
        editor.right_dp <= 990.0 || editor.left_dp >= 1010.0,
        "Editor {:?} 不应跨竖直 hinge [990,1010]",
        editor
    );
    // 所有非空 placement 与 hinge 零相交。
    assert_no_role_intersects_separating(&plan, &viewport);
}

#[test]
fn test_workbench_plan_full_width_horizontal_hinge_valid() {
    // 场景 2：全宽横向 separating hinge（#628 评论 5301021120 问题 2 核心场景）。
    // viewport 2000x1000，hinge [0,2000]x[490,510] 横贯全宽。
    // 旧的一维算法会把 [0,2000] 当整条横向禁区，七角色全塌。
    // 新二维算法：free regions = [0,2000]x[0,490] + [0,2000]x[510,1000]，
    // 上下两条都宽 2000 >= 696、高 490 > toolbar_h=64，plan.valid=true。
    let viewport = WindowViewport {
        width_dp: 2000.0,
        height_dp: 1000.0,
        occlusions: vec![horizontal_hinge(490.0, 510.0, 2000.0)],
    };
    let plan = resolve_workbench_layout(
        &viewport,
        WorkbenchVisibility {
            chapter_navigation_visible: true,
            tool_pane_visible: true,
        },
    );
    assert!(
        plan.valid,
        "全宽横向 hinge 上下都够高，plan 应 valid=true，不应七角色全塌"
    );
    let editor = assert_editor_non_empty(&plan);
    // Editor 完全在 hinge 上方或下方。
    assert!(
        editor.bottom_dp <= 490.0 || editor.top_dp >= 510.0,
        "Editor {:?} 不应跨横向 hinge [490,510]",
        editor
    );
    assert_no_role_intersects_separating(&plan, &viewport);
}

#[test]
fn test_workbench_plan_vertical_plus_horizontal_hinge_valid() {
    // 场景 3：一个竖直 + 一个横向 separating occlusion（横竖混合 hinge）。
    // viewport 2000x2000，vertical hinge [990,1010]x[0,2000]，horizontal hinge [0,2000]x[990,1010]。
    // free regions = 四个象限 [0,990]x[0,990] / [1010,2000]x[0,990] /
    // [0,990]x[1010,2000] / [1010,2000]x[1010,2000]，每个 990x990。
    // 990 >= workbench_min_w=696 且 990 > toolbar_h=64，plan.valid=true。
    let viewport = WindowViewport {
        width_dp: 2000.0,
        height_dp: 2000.0,
        occlusions: vec![
            vertical_hinge(990.0, 1010.0, 2000.0),
            horizontal_hinge(990.0, 1010.0, 2000.0),
        ],
    };
    let plan = resolve_workbench_layout(
        &viewport,
        WorkbenchVisibility {
            chapter_navigation_visible: true,
            tool_pane_visible: true,
        },
    );
    assert!(
        plan.valid,
        "竖直+横向混合 hinge 四象限都够大，plan 应 valid=true"
    );
    let editor = assert_editor_non_empty(&plan);
    // Editor 完全在某个象限内，不跨竖直 hinge 也不跨横向 hinge。
    assert!(
        editor.right_dp <= 990.0 || editor.left_dp >= 1010.0,
        "Editor {:?} 不应跨竖直 hinge [990,1010]",
        editor
    );
    assert!(
        editor.bottom_dp <= 990.0 || editor.top_dp >= 1010.0,
        "Editor {:?} 不应跨横向 hinge [990,1010]",
        editor
    );
    assert_no_role_intersects_separating(&plan, &viewport);
}

#[test]
fn test_workbench_plan_valid_false_when_free_region_too_small() {
    // #628 评论 5301021120 问题 3：free region 放不下最小 workbench 时 valid=false。
    // viewport 600x800 无遮挡，free region = [0,600]x[0,800]，
    // workbench_min_w = list_pane_min(200) + tool_pane_min(200) + tool_rail(56) + editor_min(240) = 696。
    // 600 < 696，放不下，valid=false，Editor 占满整个 viewport，其余角色 bounds 为空。
    let viewport = viewport(600.0, 800.0);
    let plan = resolve_workbench_layout(
        &viewport,
        WorkbenchVisibility {
            chapter_navigation_visible: true,
            tool_pane_visible: true,
        },
    );
    assert!(
        !plan.valid,
        "600dp 宽放不下 696dp 最小 workbench，应 valid=false"
    );
    let editor = bounds_for(&plan, WorkbenchRole::Editor);
    // Editor 占满整个 viewport（单栏退化）。
    assert_eq!(editor.left_dp, 0.0);
    assert_eq!(editor.right_dp, 600.0);
    assert_eq!(editor.top_dp, 0.0);
    assert_eq!(editor.bottom_dp, 800.0);
    // 其余角色 bounds 为空。
    for p in &plan.placements {
        if p.role != WorkbenchRole::Editor {
            assert!(
                p.bounds.is_empty(),
                "valid=false 时 {:?} bounds 应为空，实际 = {:?}",
                p.role,
                p.bounds
            );
        }
    }
}

#[test]
fn test_workbench_plan_visibility_false_reduces_min_width() {
    // visibility 全 false 时 workbench_min_w = 0 + 0 + 56 + 240 = 296，
    // 600dp 宽能放下，valid=true（对比 test_workbench_plan_valid_false_when_free_region_too_small）。
    let viewport = viewport(600.0, 800.0);
    let plan = resolve_workbench_layout(
        &viewport,
        WorkbenchVisibility {
            chapter_navigation_visible: false,
            tool_pane_visible: false,
        },
    );
    assert!(
        plan.valid,
        "visibility 全 false 时 min_w=296，600dp 能放下，应 valid=true"
    );
    let editor = bounds_for(&plan, WorkbenchRole::Editor);
    assert!(
        editor.width() >= 240.0,
        "Editor 应 >= editor_min_width_dp=240"
    );
}
