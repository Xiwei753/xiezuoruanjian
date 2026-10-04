//! Linux_qt 客户端专用的工作台布局 DTO（Issue #825）
//!
//! Core 已经有完整的工作台布局内核：
//! `presentation::layout::resolve_workbench_layout(WindowViewport, WorkbenchVisibility)`
//! 输出七角色（ToolbarLeading / ToolbarCenter / ToolbarTrailing /
//! ChapterNavigation / Editor / ToolPane / ToolRail）的最终 bounds，
//! 外加 Rust 决定的最终产品模式（Workbench / SinglePane）。
//!
//! 分层：
//! - Core 决定"哪个角色放哪、每个角色多大、最终是 Workbench 还是 SinglePane"；
//! - 本文件只把 Core 的结果原样透成 QML 能直接读的 camelCase JSON，
//!   QML 按角色取 bounds 量/摆，**不再自己决定三栏各多宽**。
//!
//! `WorkbenchVisibility`（章节栏 / 工具 pane 是否展开）是端侧局部 UI 状态，
//! 由 QML 作为输入传给 [`crate::backend::app_backend::AppBackend::resolve_workbench_layout`]，
//! 不落 Core、不进同步，只用来重算 plan。

use serde::Serialize;
use writer_core::presentation::layout::resolver::{
    LayoutRect, ResolvedWorkspaceMode, WorkbenchLayoutPlan, WorkbenchPlacement, WorkbenchRole,
};

/// 平台无关的布局矩形（vp，QML 逻辑像素 = Qt 设备无关像素）。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxQtLayoutRectDto {
    pub left_dp: f32,
    pub top_dp: f32,
    pub right_dp: f32,
    pub bottom_dp: f32,
}

impl LinuxQtLayoutRectDto {
    fn from_rect(rect: &LayoutRect) -> Self {
        Self {
            left_dp: rect.left_dp,
            top_dp: rect.top_dp,
            right_dp: rect.right_dp,
            bottom_dp: rect.bottom_dp,
        }
    }
}

/// 单个角色的放置：角色名 + 该角色最终 bounds。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxQtWorkbenchPlacementDto {
    pub role: String,
    pub bounds: LinuxQtLayoutRectDto,
}

impl LinuxQtWorkbenchPlacementDto {
    fn from_placement(placement: &WorkbenchPlacement) -> Self {
        Self {
            role: role_name(placement.role).to_string(),
            bounds: LinuxQtLayoutRectDto::from_rect(&placement.bounds),
        }
    }
}

/// 工作台布局计划 —— `resolve_workbench_layout` 的输出直通 QML。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxQtWorkbenchPlanDto {
    /// 七角色（或 SinglePane 下的 Editor-only）放置列表。
    pub placements: Vec<LinuxQtWorkbenchPlacementDto>,
    /// Core 决定的最终产品模式：`Workbench` / `SinglePane`。
    /// QML 只按它映射壳层，不自己再判一次。
    pub mode: String,
}

impl LinuxQtWorkbenchPlanDto {
    /// 由 Core 的工作台布局计划原样透出。
    pub fn from_plan(plan: &WorkbenchLayoutPlan) -> Self {
        Self {
            placements: plan
                .placements
                .iter()
                .map(LinuxQtWorkbenchPlacementDto::from_placement)
                .collect(),
            mode: match plan.mode {
                ResolvedWorkspaceMode::Workbench => "Workbench".to_string(),
                ResolvedWorkspaceMode::SinglePane => "SinglePane".to_string(),
            },
        }
    }
}

/// 七角色名直通：QML 按这些字符串取 bounds。
///
/// 这里只做名字映射；角色语义、可见性和尺寸全部由 Core 决定，
/// Qt 侧不新增也不合并角色。
pub(crate) fn role_name(role: WorkbenchRole) -> &'static str {
    match role {
        WorkbenchRole::ToolbarLeading => "ToolbarLeading",
        WorkbenchRole::ToolbarCenter => "ToolbarCenter",
        WorkbenchRole::ToolbarTrailing => "ToolbarTrailing",
        WorkbenchRole::ChapterNavigation => "ChapterNavigation",
        WorkbenchRole::Editor => "Editor",
        WorkbenchRole::ToolPane => "ToolPane",
        WorkbenchRole::ToolRail => "ToolRail",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use writer_core::presentation::layout::resolve_workbench_layout;
    use writer_core::presentation::layout::resolver::{WindowViewport, WorkbenchVisibility};

    fn plan_for(
        width_vp: f32,
        height_vp: f32,
        chapter_nav: bool,
        tool_pane: bool,
    ) -> WorkbenchLayoutPlan {
        let viewport = WindowViewport {
            width_dp: width_vp,
            height_dp: height_vp,
            occlusions: Vec::new(),
        };
        let visibility = WorkbenchVisibility {
            chapter_navigation_visible: chapter_nav,
            tool_pane_visible: tool_pane,
        };
        resolve_workbench_layout(&viewport, visibility)
    }

    fn bounds_for<'a>(plan: &'a WorkbenchLayoutPlan, role: &str) -> &'a LayoutRect {
        plan.placements
            .iter()
            .find(|p| role_name(p.role) == role)
            .map(|p| &p.bounds)
            .unwrap_or_else(|| panic!("role {role} missing from plan"))
    }

    #[test]
    fn test_workbench_plan_dto_is_camel_case() {
        let dto = LinuxQtWorkbenchPlanDto::from_plan(&plan_for(1400.0, 900.0, true, true));
        let json = serde_json::to_string(&dto).unwrap();

        assert!(json.contains("\"placements\""));
        assert!(json.contains("\"mode\""));
        assert!(json.contains("\"role\""));
        assert!(json.contains("\"bounds\""));
        assert!(json.contains("\"leftDp\""));
        assert!(json.contains("\"topDp\""));
        assert!(json.contains("\"rightDp\""));
        assert!(json.contains("\"bottomDp\""));

        assert!(!json.contains("\"left_dp\""));
        assert!(!json.contains("\"leftDP\""));
        assert!(!json.contains("\"chapter_navigation_visible\""));
        assert!(!json.contains("chapterNavigationVisible"));
    }

    #[test]
    fn test_workbench_plan_passes_through_seven_roles() {
        let plan = plan_for(1400.0, 900.0, true, true);
        assert_eq!(plan.mode, ResolvedWorkspaceMode::Workbench);
        let dto = LinuxQtWorkbenchPlanDto::from_plan(&plan);
        let json = serde_json::to_string(&dto).unwrap();

        for role in [
            "ToolbarLeading",
            "ToolbarCenter",
            "ToolbarTrailing",
            "ChapterNavigation",
            "Editor",
            "ToolPane",
            "ToolRail",
        ] {
            assert!(json.contains(role), "missing role {role}");
        }
    }

    #[test]
    fn test_tool_rail_width_is_core_owned() {
        // Issue #825：右栏宽度不能再由 QML 手写常量决定。
        let plan = plan_for(1400.0, 900.0, true, true);
        let rail = bounds_for(&plan, "ToolRail");
        assert_eq!(rail.right_dp - rail.left_dp, 56.0);
    }

    #[test]
    fn test_chapter_navigation_and_editor_share_a_row() {
        // 收起章节栏时 Core 会把腾出的宽度给 Editor，Qt 只按 bounds 摆。
        let expanded = plan_for(1400.0, 900.0, true, true);
        let collapsed = plan_for(1400.0, 900.0, false, true);

        let expanded_nav = bounds_for(&expanded, "ChapterNavigation");
        let collapsed_nav = bounds_for(&collapsed, "ChapterNavigation");
        let expanded_editor = bounds_for(&expanded, "Editor");
        let collapsed_editor = bounds_for(&collapsed, "Editor");

        assert!(expanded_nav.right_dp - expanded_nav.left_dp > 0.0);
        assert_eq!(collapsed_nav.left_dp, collapsed_nav.right_dp);
        assert!(
            collapsed_editor.right_dp - collapsed_editor.left_dp
                > expanded_editor.right_dp - expanded_editor.left_dp
        );
    }

    #[test]
    fn test_narrow_viewport_falls_back_to_single_pane() {
        // 放不下七角色时 Core 明确给 SinglePane，Qt 不得自己"挤一挤"当 Workbench。
        let plan = plan_for(500.0, 600.0, true, true);
        assert_eq!(plan.mode, ResolvedWorkspaceMode::SinglePane);

        let dto = LinuxQtWorkbenchPlanDto::from_plan(&plan);
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"mode\":\"SinglePane\""));
        assert!(json.contains("\"role\":\"Editor\""));
    }
}
