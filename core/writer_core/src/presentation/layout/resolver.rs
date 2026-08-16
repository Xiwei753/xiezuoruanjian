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
//! #628 评论 5301021120 问题 3（02:59:39Z 版）：当 free regions 在合理最小尺寸下已放不下完整
//! Workbench（`editor_min_width_dp` + 可见 pane min + tool_rail），Rust 判定本次布局语义失效，
//! 输出 `mode = ResolvedWorkspaceMode::SinglePane`，placements 只返回 Editor 占最大连续
//! 安全 free-region bounds，其余 role 空 bounds，而不是把侧栏压成细线只给正文留 1dp。

//!
//! 本模块只保留布局决策（`resolve_layout`）与共享类型定义；工作台计算见 [`super::workbench`]，测试见 [`super::resolver_tests`]。
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

/// 工作台布局计划的最终产品模式（#628 评论 5301021120 02:59:39Z 版）。
///
/// Rust 根据当前 viewport + occlusions + visibility 产出**最终 mode + bounds**；
/// 平台端只按 mode 映射壳层（外层顶栏归属）、按 bounds measure/place，
/// 不允许 Android 自己根据尺寸、hinge 或 `valid` 再决定模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ResolvedWorkspaceMode {
    /// free region 能满足最小 Workbench：七角色正常放置。
    #[default]
    Workbench,
    /// free region 已语义失效：只返回 Editor 的最大连续安全 free-region bounds，
    /// 其余 role 空 bounds。
    SinglePane,
}

/// 工作台布局计划 — `resolve_workbench_layout` 的输出（#628 评论 5301021120 第 1 步）。
///
/// 含 [`WorkbenchPlacement`]，平台端按 bounds 放 slot，不再自行推导 hinge 布局。
///
/// #628 评论 5301021120 02:59:39Z 版：不再返回含糊的 `valid: bool`，
/// 改由 [`ResolvedWorkspaceMode`] 表达 Rust 决定的最终产品模式。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkbenchLayoutPlan {
    pub placements: Vec<WorkbenchPlacement>,
    /// Rust 决定的最终产品模式（#628 评论 5301021120 02:59:39Z 版）。
    ///
    /// - [`ResolvedWorkspaceMode::Workbench`]：七角色正常放置，Editor 拿到
    ///   >= `editor_min_width_dp` 的连续可编辑区域。
    /// - [`ResolvedWorkspaceMode::SinglePane`]：当前 free regions 在合理最小尺寸下
    ///   已放不下完整 Workbench，placements 只返回 Editor 的最大连续安全 free-region
    ///   bounds（其余角色 bounds 为空）——由 Rust 判定语义失效，而不是 Android 临时隐藏控件。
    pub mode: ResolvedWorkspaceMode,
}

impl Default for WorkbenchLayoutPlan {
    fn default() -> Self {
        // 默认 Workbench：无遮挡的常见场景应正常布局。
        Self {
            placements: Vec::new(),
            mode: ResolvedWorkspaceMode::Workbench,
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
