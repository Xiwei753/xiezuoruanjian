//! # 工作台布局计算 — 七角色 bounds 推导（  第 1-2 步，）
//!
//! 从 [`super::resolver`] 拆出的纯计算职责：输入 [`WindowViewport`] +
//! [`WorkbenchVisibility`]，处理全部 `separating == true` 遮挡，二维 free-region
//! 网格 cell 算法（收集 X/Y 切线→网格 cell→合并相邻可用 cell），给七个
//! [`WorkbenchRole`] 计算最终 [`LayoutRect`] bounds。
//!
//!   02:59:39Z 版：不再返回含糊的 `valid: bool`，改由
//! [`ResolvedWorkspaceMode`] 表达 Rust 决定的最终产品模式（Workbench / SinglePane）；
//! 平台端只按 mode 映射壳层、按 bounds measure/place，不允许自己再决定模式。
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
    LayoutRect, ResolvedWorkspaceMode, WindowOcclusion, WindowViewport, WorkbenchLayoutPlan,
    WorkbenchPaneWidths, WorkbenchPlacement, WorkbenchRole, WorkbenchVisibility,
};

/// 解析工作台布局计划（  第 1-2 步�，）。
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
/// 7. 放不下时 `mode = SinglePane`，placements 只返回 Editor 占最大连续安全 free-region
///    bounds（单栏），其余角色 bounds 为空——由 Rust 判定语义失效，而不是 Android 临时隐藏控件；
/// 8. 放得下时 `mode = Workbench`，七角色在该 region 内按 [`LayoutMetrics`] 尺寸排列，
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
    resolve_workbench_layout_with_pane_widths(viewport, visibility, WorkbenchPaneWidths::default())
}

/// 解析工作台布局计划（带平台端 pane 宽度请求）。
///
/// 在 [`resolve_workbench_layout`] 基础上增加 [`WorkbenchPaneWidths`] 参数：
/// 平台端可把用户拖拽后的 pane 宽度传入，Core 在 clamp 时优先采用用户请求值，
/// 再受 `list_pane_min_width_dp` / `tool_pane_min_width_dp` 和 `editor_min_width_dp` 约束。
/// `pane_widths` 全 0 时退化为纯 LayoutMetrics 行为，不改变现有各端输出。
pub fn resolve_workbench_layout_with_pane_widths(
    viewport: &WindowViewport,
    visibility: WorkbenchVisibility,
    pane_widths: WorkbenchPaneWidths,
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
        let placements = place_workbench_in_region(region, &metrics, &visibility, &pane_widths);
        WorkbenchLayoutPlan {
            placements,
            mode: ResolvedWorkspaceMode::Workbench,
        }
    } else {
        // mode=SinglePane：当前 free regions 放不下完整 Workbench，
        // 只返回 Editor 占最大连续安全 free-region bounds（或整个 viewport）。
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
            mode: ResolvedWorkspaceMode::SinglePane,
        }
    }
}

/// 计算二维 free regions。
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

/// 在 placement region 内放置七角色（`mode = Workbench` 路径）。
///
/// toolbar 在顶部 `toolbar_height_dp` 高度，content 在下方横向排列
/// ChapterNavigation | Editor | ToolPane | ToolRail。
/// pane 在 preferred 与 min 之间压缩（不压到 0 除非 visibility 不可见）；
/// Editor 拿剩余宽度（>= `editor_min_width_dp`，由调用方保证 region 足够放下）。
fn place_workbench_in_region(
    region: LayoutRect,
    metrics: &LayoutMetrics,
    visibility: &WorkbenchVisibility,
    pane_widths: &WorkbenchPaneWidths,
) -> Vec<WorkbenchPlacement> {
    let region_w = region.width();
    let region_h = region.height();
    let toolbar_h = metrics.toolbar_height_dp.min(region_h);
    let content_top = region.top_dp + toolbar_h;
    let content_bottom = region.bottom_dp;
    let toolbar_bottom = region.top_dp + toolbar_h;

    let (chapter_nav_w, tool_pane_w) =
        compute_content_pane_widths(region_w, metrics, visibility, pane_widths);
    let tool_rail_w = metrics.tool_rail_width_dp;
    let chapter_nav_right = region.left_dp + chapter_nav_w;
    let tool_rail_left = region.right_dp - tool_rail_w;
    let tool_pane_left = tool_rail_left - tool_pane_w;
    let editor_left = chapter_nav_right;
    let editor_right = tool_pane_left;

    let (toolbar_leading_bounds, toolbar_center_bounds, toolbar_trailing_bounds) =
        compute_toolbar_bounds(
            region,
            region_w,
            metrics,
            toolbar_bottom,
            chapter_nav_w,
            tool_pane_w,
            tool_rail_w,
        );

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

/// 计算 content 区域 chapter_nav / tool_pane 的实际宽度。
///
/// 优先采用 `pane_widths` 中用户请求的宽度（> 0 才用），
/// 再用 `list_pane_min_width_dp` / `tool_pane_min_width_dp` 和
/// "必须给 Editor 留 `editor_min_width_dp`" 做 clamp。
fn compute_content_pane_widths(
    region_w: f32,
    metrics: &LayoutMetrics,
    visibility: &WorkbenchVisibility,
    pane_widths: &WorkbenchPaneWidths,
) -> (f32, f32) {
    // 用户请求宽度：先看 visibility，不可见时 requested 直接为 0（不参与预算）。
    // 可见时优先采用 pane_widths 中用户拖拽后的宽度（> 0 才用），
    // 否则用 LayoutMetrics 默认 preferred 宽度。
    //
    //   问题 3a：旧实现先看 pane_widths > 0 再看 visibility，导致已收起的 pane
    // 仍把之前拖拽存的宽度计入 total_requested，误判"不够 requested"把另一栏压回 min。
    // 此处改为先看 visibility，隐藏 pane 的 requested 一律为 0。
    let chapter_nav_requested = if visibility.chapter_navigation_visible {
        if pane_widths.chapter_navigation_dp > 0.0 {
            pane_widths.chapter_navigation_dp
        } else {
            metrics.list_pane_width_dp
        }
    } else {
        0.0
    };
    let tool_pane_requested = if visibility.tool_pane_visible {
        if pane_widths.tool_pane_dp > 0.0 {
            pane_widths.tool_pane_dp
        } else {
            metrics.tool_pane_width_dp
        }
    } else {
        0.0
    };

    // 最小宽度：visibility 不可见时为 0。
    let chapter_nav_min = if visibility.chapter_navigation_visible {
        metrics.list_pane_min_width_dp
    } else {
        0.0
    };
    let tool_pane_min = if visibility.tool_pane_visible {
        metrics.tool_pane_min_width_dp
    } else {
        0.0
    };

    let tool_rail_w = metrics.tool_rail_width_dp;
    let editor_min_w = metrics.editor_min_width_dp;

    //   问题 3b：min + extra 连续 clamp。
    // 旧实现超预算时直接 return (min, min)，拖拽越过上限一像素就整栏跳回 min。
    // 改为：可见 pane 先各自拿 min，再把剩余 extra 预算按请求比例分配给超出 min 的部分，
    // 超预算时只压缩 extra（按比例），min 全给——拖到极限停在极限，不跳变。
    // Editor 始终保住 editor_min_width_dp，不参与 pane 压缩。

    // pane 可用总预算 = region_w - tool_rail - editor_min，clamp 到 >= 0。
    let budget = (region_w - tool_rail_w - editor_min_w).max(0.0);

    // 可见 pane 先各自拿自己的 min（不可见为 0）。
    let min_sum = chapter_nav_min + tool_pane_min;
    // extra 预算 = budget - min_sum。>= 0 表示两个 min 都能放下还有富余。
    let extra_budget = budget - min_sum;

    // 请求超出 min 的部分（>= 0）。requested 低于 min 时 extra 为 0，final 不低于 min。
    let left_extra = (chapter_nav_requested - chapter_nav_min).max(0.0);
    let right_extra = (tool_pane_requested - tool_pane_min).max(0.0);
    let total_extra = left_extra + right_extra;

    if extra_budget < 0.0 {
        // 连 min 都放不下：budget 不够给两个 min，按 min 比例压缩。
        // budget 已 clamp 到 >= 0。保留 visibility 不可见对应 pane 宽度为 0 的语义。
        if min_sum > 0.0 {
            let ratio = budget / min_sum;
            let chapter_nav_final = if visibility.chapter_navigation_visible {
                (chapter_nav_min * ratio).max(0.0)
            } else {
                0.0
            };
            let tool_pane_final = if visibility.tool_pane_visible {
                (tool_pane_min * ratio).max(0.0)
            } else {
                0.0
            };
            (chapter_nav_final, tool_pane_final)
        } else {
            // 两个 pane 都不可见（min_sum = 0），直接返回 (0, 0)。
            (0.0, 0.0)
        }
    } else if total_extra <= extra_budget {
        // extra 没超预算：原样给。final = min + extra（即 requested，但不低于 min）。
        let chapter_nav_final = chapter_nav_min + left_extra;
        let tool_pane_final = tool_pane_min + right_extra;
        (chapter_nav_final, tool_pane_final)
    } else {
        // total_extra > extra_budget（且 total_extra > 0，因为 extra_budget >= 0）：
        // 按比例压缩 extra 部分，不一刀切打回 min，保证拖拽连续不跳变。
        // min 部分全给，只压缩超出 min 的 extra，拖到极限停在极限。
        let ratio = extra_budget / total_extra;
        let chapter_nav_final = chapter_nav_min + left_extra * ratio;
        let tool_pane_final = tool_pane_min + right_extra * ratio;
        (chapter_nav_final, tool_pane_final)
    }
}

/// 计算 toolbar 三组 bounds（leading/center/trailing）。
///
/// leading 的右边界至少覆盖实际 ChapterNavigation 宽度（`chapter_nav_w`），
/// trailing 的左边界至少覆盖实际 ToolPane + ToolRail 宽度（`tool_pane_w + tool_rail_w`）。
/// `toolbar_leading_width_dp` / `toolbar_trailing_width_dp` 只当工具内容自己的最小宽度，
/// 不再当另一套独立栏宽——这样左栏/右栏拖宽以后，顶栏也跟着同一条分割线移动。
fn compute_toolbar_bounds(
    region: LayoutRect,
    region_w: f32,
    metrics: &LayoutMetrics,
    toolbar_bottom: f32,
    chapter_nav_w: f32,
    tool_pane_w: f32,
    tool_rail_w: f32,
) -> (LayoutRect, LayoutRect, LayoutRect) {
    // leading 至少覆盖 ChapterNavigation 宽度。
    let toolbar_leading_w = metrics
        .toolbar_leading_width_dp
        .max(chapter_nav_w)
        .min(region_w);
    // trailing 至少覆盖 ToolPane + ToolRail 宽度。
    let toolbar_trailing_w = metrics
        .toolbar_trailing_width_dp
        .max(tool_pane_w + tool_rail_w)
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

/// `mode = SinglePane` 退化：只返回 Editor 占满给定 region（最大连续安全 free-region），
/// 其余角色 bounds 为空。
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
