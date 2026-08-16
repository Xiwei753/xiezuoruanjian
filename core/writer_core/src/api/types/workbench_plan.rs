//! # Workbench Layout Plan DTOs（#628 评论 5301021120 第 1-3 步 + 02:59:39Z 版）
//!
//! `resolve_workbench_layout` 输出链路的跨语言 DTO：布局矩形、七角色、可见性、
//! 最终产品模式（Workbench/SinglePane）与布局计划。每个 `*_dto` 类型对应
//! Core 内部类型（`crate::presentation::layout::resolver`），提供 `From` 双向转换。
//!
//! #628 评论 5301021120 02:59:39Z 版：`WorkbenchLayoutPlanDto.valid: bool` 删除，
//! 改由 [`ResolvedWorkspaceModeDto`] 表达 Rust 决定的最终产品模式（Workbench / SinglePane）；
//! 平台端只按 mode 映射壳层、按 bounds measure/place，不允许自己再决定模式。
/// 平台无关的布局矩形 DTO（dp 坐标系）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct LayoutRectDto {
    pub left_dp: f32,
    pub top_dp: f32,
    pub right_dp: f32,
    pub bottom_dp: f32,
}

impl From<crate::presentation::layout::resolver::LayoutRect> for LayoutRectDto {
    fn from(r: crate::presentation::layout::resolver::LayoutRect) -> Self {
        Self {
            left_dp: r.left_dp,
            top_dp: r.top_dp,
            right_dp: r.right_dp,
            bottom_dp: r.bottom_dp,
        }
    }
}

impl From<LayoutRectDto> for crate::presentation::layout::resolver::LayoutRect {
    fn from(dto: LayoutRectDto) -> Self {
        Self {
            left_dp: dto.left_dp,
            top_dp: dto.top_dp,
            right_dp: dto.right_dp,
            bottom_dp: dto.bottom_dp,
        }
    }
}

/// 工作台角色 DTO — 七个产品语义角色（#628 评论 5301021120 第 1 步）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum WorkbenchRoleDto {
    #[default]
    ToolbarLeading,
    ToolbarCenter,
    ToolbarTrailing,
    ChapterNavigation,
    Editor,
    ToolPane,
    ToolRail,
}

impl From<crate::presentation::layout::resolver::WorkbenchRole> for WorkbenchRoleDto {
    fn from(r: crate::presentation::layout::resolver::WorkbenchRole) -> Self {
        use crate::presentation::layout::resolver::WorkbenchRole as R;
        match r {
            R::ToolbarLeading => Self::ToolbarLeading,
            R::ToolbarCenter => Self::ToolbarCenter,
            R::ToolbarTrailing => Self::ToolbarTrailing,
            R::ChapterNavigation => Self::ChapterNavigation,
            R::Editor => Self::Editor,
            R::ToolPane => Self::ToolPane,
            R::ToolRail => Self::ToolRail,
        }
    }
}

impl From<WorkbenchRoleDto> for crate::presentation::layout::resolver::WorkbenchRole {
    fn from(dto: WorkbenchRoleDto) -> Self {
        use WorkbenchRoleDto as D;
        match dto {
            D::ToolbarLeading => Self::ToolbarLeading,
            D::ToolbarCenter => Self::ToolbarCenter,
            D::ToolbarTrailing => Self::ToolbarTrailing,
            D::ChapterNavigation => Self::ChapterNavigation,
            D::Editor => Self::Editor,
            D::ToolPane => Self::ToolPane,
            D::ToolRail => Self::ToolRail,
        }
    }
}

/// 单个角色的放置 DTO — 角色与其最终 bounds。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchPlacementDto {
    pub role: WorkbenchRoleDto,
    pub bounds: LayoutRectDto,
}

impl From<crate::presentation::layout::resolver::WorkbenchPlacement> for WorkbenchPlacementDto {
    fn from(p: crate::presentation::layout::resolver::WorkbenchPlacement) -> Self {
        Self {
            role: p.role.into(),
            bounds: p.bounds.into(),
        }
    }
}

impl From<WorkbenchPlacementDto> for crate::presentation::layout::resolver::WorkbenchPlacement {
    fn from(dto: WorkbenchPlacementDto) -> Self {
        Self {
            role: dto.role.into(),
            bounds: dto.bounds.into(),
        }
    }
}

/// 工作台可见性 DTO — 端侧局部 UI 状态（#628 评论 5301021120 第 1 步）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchVisibilityDto {
    pub chapter_navigation_visible: bool,
    pub tool_pane_visible: bool,
}

impl From<crate::presentation::layout::resolver::WorkbenchVisibility> for WorkbenchVisibilityDto {
    fn from(v: crate::presentation::layout::resolver::WorkbenchVisibility) -> Self {
        Self {
            chapter_navigation_visible: v.chapter_navigation_visible,
            tool_pane_visible: v.tool_pane_visible,
        }
    }
}

impl From<WorkbenchVisibilityDto> for crate::presentation::layout::resolver::WorkbenchVisibility {
    fn from(dto: WorkbenchVisibilityDto) -> Self {
        Self {
            chapter_navigation_visible: dto.chapter_navigation_visible,
            tool_pane_visible: dto.tool_pane_visible,
        }
    }
}

/// 工作台布局计划的最终产品模式 DTO（#628 评论 5301021120 02:59:39Z 版）。
///
/// Rust 根据当前 viewport + occlusions + visibility 产出最终 mode + bounds；
/// 平台端只按 mode 映射壳层、按 bounds measure/place，不允许自己再决定模式。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Default)]
pub enum ResolvedWorkspaceModeDto {
    #[default]
    Workbench,
    SinglePane,
}

impl From<crate::presentation::layout::resolver::ResolvedWorkspaceMode>
    for ResolvedWorkspaceModeDto
{
    fn from(m: crate::presentation::layout::resolver::ResolvedWorkspaceMode) -> Self {
        use crate::presentation::layout::resolver::ResolvedWorkspaceMode as R;
        match m {
            R::Workbench => Self::Workbench,
            R::SinglePane => Self::SinglePane,
        }
    }
}

impl From<ResolvedWorkspaceModeDto>
    for crate::presentation::layout::resolver::ResolvedWorkspaceMode
{
    fn from(dto: ResolvedWorkspaceModeDto) -> Self {
        use ResolvedWorkspaceModeDto as D;
        match dto {
            D::Workbench => Self::Workbench,
            D::SinglePane => Self::SinglePane,
        }
    }
}

/// 工作台布局计划 DTO — `resolve_workbench_layout` 的输出（#628 评论 5301021120 第 1 步）。
///
/// #628 评论 5301021120 02:59:39Z 版：不再返回含糊的 `valid: bool`，
/// 改由 [`ResolvedWorkspaceModeDto`] 表达 Rust 决定的最终产品模式。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchLayoutPlanDto {
    pub placements: Vec<WorkbenchPlacementDto>,
    /// Rust 决定的最终产品模式（#628 评论 5301021120 02:59:39Z 版）。
    ///
    /// - `Workbench`：七角色正常放置。
    /// - `SinglePane`：只返回 Editor 的最大连续安全 free-region bounds，其余 role 空。
    pub mode: ResolvedWorkspaceModeDto,
}

impl Default for WorkbenchLayoutPlanDto {
    fn default() -> Self {
        Self {
            placements: Vec::new(),
            mode: ResolvedWorkspaceModeDto::Workbench,
        }
    }
}

impl From<crate::presentation::layout::resolver::WorkbenchLayoutPlan> for WorkbenchLayoutPlanDto {
    fn from(p: crate::presentation::layout::resolver::WorkbenchLayoutPlan) -> Self {
        Self {
            placements: p.placements.into_iter().map(Into::into).collect(),
            mode: p.mode.into(),
        }
    }
}

impl From<WorkbenchLayoutPlanDto> for crate::presentation::layout::resolver::WorkbenchLayoutPlan {
    fn from(dto: WorkbenchLayoutPlanDto) -> Self {
        Self {
            placements: dto.placements.into_iter().map(Into::into).collect(),
            mode: dto.mode.into(),
        }
    }
}
