//! # 工作台布局计算 — 七角色 bounds 推导（#628 评论 5301021120 第 1-2 步，问题 2/3）
//!
//! 从 [`super::resolver`] 拆出的纯计算职责：输入 [`WindowViewport`] +
//! [`WorkbenchVisibility`]，处理全部 `separating == true` 遮挡，二维 free-region
//! 网格 cell 算法（收集 X/Y 切线→网格 cell→合并相邻可用 cell），给七个
//! [`WorkbenchRole`] 计算最终 [`LayoutRect`] bounds。
//!
//! - 越界矩形 clamp 到 viewport；空矩形丢弃；
//! - 二维 free-region 几何算法（[`compute_free_regions`]）：收集 X/Y 切线形成网格 cell，
//!   与任一 separating occlusion 相交的 cell 不可用，合并相邻可用 cell 成连续区域；
//! - 七角色 bounds 都不与任何 separating 相交；
//! - Editor 拿到连续可编辑区域，不跨两个物理区域；
//! - 多 separating 同时存在时同样处理，不退化成单 hinge；
//! - 竖直 hinge、横向 hinge、多个横竖混合 hinge 都走同一套几何算法，不新增平台分支；
//! - 无遮挡时退化成普通大屏工作台（free region = 整个 viewport）。

use super::metrics::LayoutMetrics;
use super::resolver::{
    LayoutRect, WindowOcclusion, WindowViewport, WorkbenchLayoutPlan, WorkbenchPlacement,
    WorkbenchRole, WorkbenchVisibility,
};

/// 解析工作台布局计划（#628 评论 5301021120 第 1-2 步�，问题 2/3）。
///
/// 平台无关纯函数，处理全部 `separating == true` 的遮挡：
///
/// 1. 越界矩形 clamp 到当前 viewport；空矩形丢弃；
/// 2. 只把 `separating == true` 的区域作为不可跨越分隔；
/// 3. 收集 0 / viewport edge / 所有 occlusion edge 形成 X、Y 两组切线；
/// 4. 用相邻 X/Y 区间形成网格 cell；与任一 separating occlusion 相交的 cell 标记不可用；
/// 5. 把相邻可用 cell 合并成连续 [`LayoutRect`] 区域（[`compute_free_regions`]）；
/// 6. 选一个能放下 Workbench 最小需求（`editor_min_width_dp` + 可见 pane min + tool_rail）
///    的 free region 作为 placement region；
/// 7. 放不下时 `valid = false`，placements 退化为 Editor 占满最大可用 free region（单栏），
///    其余角色 bounds 为空——由 Rust 判定语义失效，而不是 Android 临时隐藏控件；
/// 8. 放得下时 `valid = true`，七角色在该 region 内按 [`LayoutMetrics`] 尺寸排列，
///    pane 在 preferred 与 min 间压缩（不压到 0 除非 visibility 不可见），
///    所有 bounds 不与 separating 相交，Editor 连续。
///
/// 竖直 hinge、横向 hinge、多个横竖混合 hinge 都走同一套二维几何算法，
/// 不新增 Android/Foldable 分支，也不在 Rust 建 FoldingFeature.orientation 平台枚举。
///
/// 无遮挡时退化成普通大屏工作台（free region = 整个 viewport）。
///
/// 角色顺序：Toolbar [Leading][Center][Trailing]，Content [ChapterNavigation][Editor][ToolPane][ToolRail]。
pub fn resolve_workbench_layout(
    viewport: &WindowViewport,
    visibility: WorkbenchVisibility,
) -> WorkbenchLayoutPlan {
    let metrics = LayoutMetrics::default();
    let vw = viewport.width_dp.max(0.0);
    let vh = viewport.height_dp.max(0.0);

    let free_regions = compute_free_regions(&viewport.occlusions, vw, vh);

    // Workbench 最小需求宽度 = 可见 pane min + tool_rail + editor_min。
    let chapter_nav_min_w = if visibility.chapter_navigation_visible {
        metrics.list_pane_min_width_dp
    } else {
        0.0
    };
    let tool_pane_min_w = if visibility.tool_pane_visible {
        metrics.tool_pane_min_width_dp
    } else {
        0.0
    };
    let workbench_min_w = chapter_nav_min_w
        + tool_pane_min_w
        + metrics.tool_rail_width_dp
        + metrics.editor_min_width_dp;

    // 选面积最大的、能放下 Workbench 最小需求的 free region。
    let placement_region = free_regions
        .iter()
        .filter(|r| r.width() >= workbench_min_w && r.height() > metrics.toolbar_height_dp)
        .max_by(|a, b| {
            let area_a = a.width() * a.height();
            let area_b = b.width() * b.height();
            area_a
                .partial_cmp(&area_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied();

    if let Some(region) = placement_region {
        let placements = place_workbench_in_region(region, &metrics, &visibility);
        WorkbenchLayoutPlan {
            placements,
            valid: true,
        }
    } else {
        // valid=false：当前 free regions 放不下完整 Workbench，
        // 退化为 Editor 单栏占最大可用 free region（或整个 viewport）。
        let largest = free_regions
            .iter()
            .max_by(|a, b| {
                let area_a = a.width() * a.height();
                let area_b = b.width() * b.height();
                area_a
                    .partial_cmp(&area_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .copied()
            .unwrap_or(LayoutRect {
                left_dp: 0.0,
                top_dp: 0.0,
                right_dp: vw,
                bottom_dp: vh,
            });
        let placements = degrade_to_editor_only(largest);
        WorkbenchLayoutPlan {
            placements,
            valid: false,
        }
    }
}

/// 计算二维 free regions（#628 评论 5301021120 问题 2）。
///
/// 网格 cell 算法：
/// 1. 把 separating occlusion 的 left/top/right/bottom 全部 clamp 到 viewport，空矩形删除；
/// 2. 收集 0 / viewport edge / 所有 occlusion edge 形成 X、Y 两组切线（去重 + 排序）；
/// 3. 用相邻 X/Y 区间形成网格 cell；与任一 separating occlusion 相交的 cell 标记不可用；
/// 4. 对每个可用 cell，以它为左上角向右扩展到最远，再向下逐行扩展，得到最大矩形；
/// 5. 去重后返回所有候选 free region。
///
/// 竖直 hinge、横向 hinge、多个横竖混合 hinge 都走同一套几何算法。
/// 检查 row j 的 [i0, i_max) 列是否全部可用。
fn row_all_usable(usable: &[Vec<bool>], i0: usize, i_max: usize, j: usize) -> bool {
    usable[i0..i_max].iter().all(|row| row[j])
}

/// 从 row j0 向下扩展，返回最远的 j_max 使得 [j0, j_max) 每一行 [i0, i_max) 全部可用。
fn farthest_usable_row_down(
    usable: &[Vec<bool>],
    i0: usize,
    i_max: usize,
    j0: usize,
    ny: usize,
) -> usize {
    let mut j_max = j0;
    while j_max < ny && row_all_usable(usable, i0, i_max, j_max) {
        j_max += 1;
    }
    j_max
}

fn compute_free_regions(occlusions: &[WindowOcclusion], vw: f32, vh: f32) -> Vec<LayoutRect> {
    // 1. clamp separating occlusions to viewport, drop empty.
    let separating: Vec<LayoutRect> = occlusions
        .iter()
        .filter(|o| o.separating)
        .map(|o| LayoutRect {
            left_dp: o.left_dp.clamp(0.0, vw),
            top_dp: o.top_dp.clamp(0.0, vh),
            right_dp: o.right_dp.clamp(0.0, vw),
            bottom_dp: o.bottom_dp.clamp(0.0, vh),
        })
        .filter(|r| !r.is_empty())
        .collect();

    // 2. collect X and Y cut lines: 0, viewport edge, all occlusion edges.
    let mut xs: Vec<f32> = vec![0.0, vw];
    let mut ys: Vec<f32> = vec![0.0, vh];
    for r in &separating {
        xs.push(r.left_dp);
        xs.push(r.right_dp);
        ys.push(r.top_dp);
        ys.push(r.bottom_dp);
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    xs.dedup();
    ys.dedup();

    let nx = xs.len().saturating_sub(1);
    let ny = ys.len().saturating_sub(1);

    // 3. form grid cells; cell (i,j) covers [xs[i],xs[i+1]] x [ys[j],ys[j+1]].
    //    cell is usable iff it doesn't intersect any separating occlusion.
    let mut usable: Vec<Vec<bool>> = vec![vec![false; ny]; nx];
    for i in 0..nx {
        for j in 0..ny {
            let cell = LayoutRect {
                left_dp: xs[i],
                top_dp: ys[j],
                right_dp: xs[i + 1],
                bottom_dp: ys[j + 1],
            };
            if cell.is_empty() {
                usable[i][j] = false;
                continue;
            }
            usable[i][j] = !separating.iter().any(|s| cell.intersects(s));
        }
    }

    // 4. for each usable cell, compute maximal rectangle with that cell as top-left:
    //    extend right to farthest, then extend down row by row (each row must be fully usable).
    let mut regions: Vec<LayoutRect> = Vec::new();
    for i0 in 0..nx {
        for j0 in 0..ny {
            if !usable[i0][j0] {
                continue;
            }
            // extend right: rightmost i_max such that [i0, i_max) all usable in row j0
            let mut i_max = i0;
            while i_max < nx && usable[i_max][j0] {
                i_max += 1;
            }
            // extend down: farthest j_max such that every row in [j0, j_max)
            // has all cells [i0, i_max) usable
            let j_max = farthest_usable_row_down(&usable, i0, i_max, j0, ny);
            regions.push(LayoutRect {
                left_dp: xs[i0],
                top_dp: ys[j0],
                right_dp: xs[i_max],
                bottom_dp: ys[j_max],
            });
        }
    }

    // 5. dedup
    regions.sort_by(|a, b| {
        a.left_dp
            .partial_cmp(&b.left_dp)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.top_dp
                    .partial_cmp(&b.top_dp)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(
                a.right_dp
                    .partial_cmp(&b.right_dp)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(
                a.bottom_dp
                    .partial_cmp(&b.bottom_dp)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    regions.dedup();

    regions
}

/// 在 placement region 内放置七角色（valid=true 路径）。
///
/// toolbar 在顶部 `toolbar_height_dp` 高度，content 在下方横向排列
/// ChapterNavigation | Editor | ToolPane | ToolRail。
/// pane 在 preferred 与 min 之间压缩（不压到 0 除非 visibility 不可见）；
/// Editor 拿剩余宽度（>= `editor_min_width_dp`，由调用方保证 region 足够放下）。
fn place_workbench_in_region(
    region: LayoutRect,
    metrics: &LayoutMetrics,
    visibility: &WorkbenchVisibility,
) -> Vec<WorkbenchPlacement> {
    let region_w = region.width();
    let region_h = region.height();
    let toolbar_h = metrics.toolbar_height_dp.min(region_h);
    let content_top = region.top_dp + toolbar_h;
    let content_bottom = region.bottom_dp;
    let toolbar_bottom = region.top_dp + toolbar_h;

    let (chapter_nav_w, tool_pane_w) = compute_content_pane_widths(region_w, metrics, visibility);
    let tool_rail_w = metrics.tool_rail_width_dp;
    let chapter_nav_right = region.left_dp + chapter_nav_w;
    let tool_rail_left = region.right_dp - tool_rail_w;
    let tool_pane_left = tool_rail_left - tool_pane_w;
    let editor_left = chapter_nav_right;
    let editor_right = tool_pane_left;

    let (toolbar_leading_bounds, toolbar_center_bounds, toolbar_trailing_bounds) =
        compute_toolbar_bounds(region, region_w, metrics, toolbar_bottom);

    vec![
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
            bounds: LayoutRect {
                left_dp: region.left_dp,
                top_dp: content_top,
                right_dp: chapter_nav_right,
                bottom_dp: content_bottom,
            },
        },
        WorkbenchPlacement {
            role: WorkbenchRole::Editor,
            bounds: LayoutRect {
                left_dp: editor_left,
                top_dp: content_top,
                right_dp: editor_right,
                bottom_dp: content_bottom,
            },
        },
        WorkbenchPlacement {
            role: WorkbenchRole::ToolPane,
            bounds: LayoutRect {
                left_dp: tool_pane_left,
                top_dp: content_top,
                right_dp: tool_rail_left,
                bottom_dp: content_bottom,
            },
        },
        WorkbenchPlacement {
            role: WorkbenchRole::ToolRail,
            bounds: LayoutRect {
                left_dp: tool_rail_left,
                top_dp: content_top,
                right_dp: region.right_dp,
                bottom_dp: content_bottom,
            },
        },
    ]
}

/// 计算 content 区域 chapter_nav / tool_pane 的实际宽度（preferred 或压缩到 min）。
fn compute_content_pane_widths(
    region_w: f32,
    metrics: &LayoutMetrics,
    visibility: &WorkbenchVisibility,
) -> (f32, f32) {
    let chapter_nav_preferred = if visibility.chapter_navigation_visible {
        metrics.list_pane_width_dp
    } else {
        0.0
    };
    let chapter_nav_min = if visibility.chapter_navigation_visible {
        metrics.list_pane_min_width_dp
    } else {
        0.0
    };
    let tool_pane_preferred = if visibility.tool_pane_visible {
        metrics.tool_pane_width_dp
    } else {
        0.0
    };
    let tool_pane_min = if visibility.tool_pane_visible {
        metrics.tool_pane_min_width_dp
    } else {
        0.0
    };
    let total_preferred = chapter_nav_preferred
        + tool_pane_preferred
        + metrics.tool_rail_width_dp
        + metrics.editor_min_width_dp;

    // 空间够 preferred 时用 preferred；否则压 pane 到 min，editor 拿剩余（>= editor_min_w）。
    if region_w >= total_preferred {
        (chapter_nav_preferred, tool_pane_preferred)
    } else {
        (chapter_nav_min, tool_pane_min)
    }
}

/// 计算 toolbar 三组 bounds（leading/center/trailing）。
fn compute_toolbar_bounds(
    region: LayoutRect,
    region_w: f32,
    metrics: &LayoutMetrics,
    toolbar_bottom: f32,
) -> (LayoutRect, LayoutRect, LayoutRect) {
    let toolbar_leading_w = metrics.toolbar_leading_width_dp.min(region_w);
    let toolbar_trailing_w = metrics
        .toolbar_trailing_width_dp
        .min((region_w - toolbar_leading_w).max(0.0));
    let toolbar_leading_right = region.left_dp + toolbar_leading_w;
    let toolbar_trailing_left = region.right_dp - toolbar_trailing_w;
    let toolbar_center_left = toolbar_leading_right;
    let toolbar_center_right = toolbar_trailing_left.max(toolbar_center_left);

    let leading = LayoutRect {
        left_dp: region.left_dp,
        top_dp: region.top_dp,
        right_dp: toolbar_leading_right,
        bottom_dp: toolbar_bottom,
    };
    let center = LayoutRect {
        left_dp: toolbar_center_left,
        top_dp: region.top_dp,
        right_dp: toolbar_center_right,
        bottom_dp: toolbar_bottom,
    };
    let trailing = LayoutRect {
        left_dp: toolbar_trailing_left,
        top_dp: region.top_dp,
        right_dp: region.right_dp,
        bottom_dp: toolbar_bottom,
    };
    (leading, center, trailing)
}

/// valid=false 退化：Editor 占满给定 region，其余角色 bounds 为空（#628 评论 5301021120 问题 3）。
fn degrade_to_editor_only(region: LayoutRect) -> Vec<WorkbenchPlacement> {
    vec![
        WorkbenchPlacement {
            role: WorkbenchRole::ToolbarLeading,
            bounds: LayoutRect::default(),
        },
        WorkbenchPlacement {
            role: WorkbenchRole::ToolbarCenter,
            bounds: LayoutRect::default(),
        },
        WorkbenchPlacement {
            role: WorkbenchRole::ToolbarTrailing,
            bounds: LayoutRect::default(),
        },
        WorkbenchPlacement {
            role: WorkbenchRole::ChapterNavigation,
            bounds: LayoutRect::default(),
        },
        WorkbenchPlacement {
            role: WorkbenchRole::Editor,
            bounds: region,
        },
        WorkbenchPlacement {
            role: WorkbenchRole::ToolPane,
            bounds: LayoutRect::default(),
        },
        WorkbenchPlacement {
            role: WorkbenchRole::ToolRail,
            bounds: LayoutRect::default(),
        },
    ]
}
