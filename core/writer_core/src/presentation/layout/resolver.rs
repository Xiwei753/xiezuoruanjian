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
//! # Workbench 布局计划（#628 评论 5301021120 第 1-2 步，问题 2/3）
//!
//! [`resolve_workbench_layout`] 是平台无关纯函数，输入 [`WindowViewport`] +
//! [`WorkbenchVisibility`]，输出 [`WorkbenchLayoutPlan`]（含七个 [`WorkbenchRole`]
//! 的最终 [`LayoutRect`] bounds）。处理全部 `separating == true` 的遮挡：
//! - 越界矩形 clamp 到 viewport；空矩形丢弃；
//! - 二维 free-region 几何算法（[`compute_free_regions`]）：收集 X/Y 切线形成网格 cell，
//!   与任一 separating occlusion 相交的 cell 不可用，合并相邻可用 cell 成连续区域；
//! - 七角色 bounds 都不与任何 separating 相交；
//! - Editor 拿到连续可编辑区域，不跨两个物理区域；
//! - 多 separating 同时存在时同样处理，不退化成单 hinge；
//! - 竖直 hinge、横向 hinge、多个横竖混合 hinge 都走同一套几何算法，不新增平台分支；
//! - 无遮挡时退化成普通大屏工作台（free region = 整个 viewport）。
//!
//! #628 评论 5301021120 问题 3：当 free regions 在合理最小尺寸下已放不下完整 Workbench
//! （`editor_min_width_dp` + 可见 pane min + tool_rail），Rust 判定本次布局语义失效
//! （`WorkbenchLayoutPlan.valid = false`），placements 退化为 Editor 单栏占满最大可用
//! free region，而不是把侧栏压成细线只给正文留 1dp。

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
///
/// #628 评论 5301021120 问题 3：`valid` 表示本次布局语义是否成立。
/// `valid == false` 时 placements 退化为 Editor 单栏占满最大可用 free region，
/// 其余角色 bounds 为空——由 Rust 判定语义失效，而不是 Android 临时隐藏控件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkbenchLayoutPlan {
    pub placements: Vec<WorkbenchPlacement>,
    /// 本次布局语义是否成立（#628 评论 5301021120 问题 3）。
    ///
    /// - `true`：七角色正常放置，Editor 拿到 >= `editor_min_width_dp` 的连续可编辑区域。
    /// - `false`：当前 free regions 在合理最小尺寸下已放不下完整 Workbench，
    ///   placements 退化为 Editor 占满最大可用 free region（单栏），其余角色 bounds 为空。
    pub valid: bool,
}

impl Default for WorkbenchLayoutPlan {
    fn default() -> Self {
        // 默认 valid=true：无遮挡的常见场景应正常布局。
        Self {
            placements: Vec::new(),
            valid: true,
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
}
