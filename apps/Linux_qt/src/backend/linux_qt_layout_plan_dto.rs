//! Linux_qt 客户端专用的 LayoutPlan DTO
//!
//! 与跨平台 LayoutPlanDto 不同，LinuxQtLayoutPlanDto 只包含 QML 实际使用的字段，
//! 并确保输出为 camelCase，QML 无需 snake_case fallback。

use serde::Serialize;
use writer_core::layout_policy::{LayoutPlan, ShellMode, WorkspacePaneMode, NavigationPresentation};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxQtLayoutPlanDto {
    pub shell_mode: String,
    pub navigation_presentation: String,
    pub workspace_pane_mode: String,
    pub show_chapter_tree: bool,
    pub show_editor: bool,
    pub show_project_list: bool,
    pub show_bottom_bar: bool,
    pub content_max_width_dp: f32,
    pub page_padding_dp: f32,
    pub list_pane_min_dp: f32,
    pub list_pane_preferred_dp: f32,
    pub list_pane_max_dp: f32,
    pub editor_content_max_width_dp: f32,
    pub primary_pane_min_dp: f32,
    pub primary_pane_preferred_dp: f32,
    pub primary_pane_max_dp: f32,
}

impl LinuxQtLayoutPlanDto {
    pub fn from_layout_plan(plan: &LayoutPlan) -> Self {
        Self {
            shell_mode: match plan.shell_mode {
                ShellMode::SinglePane => "SinglePane".to_string(),
                ShellMode::SupportingPane => "SupportingPane".to_string(),
                ShellMode::TwoPane => "TwoPane".to_string(),
                ShellMode::ThreePane => "ThreePane".to_string(),
            },
            navigation_presentation: match plan.navigation_presentation {
                NavigationPresentation::BottomBar => "BottomBar".to_string(),
                NavigationPresentation::NavigationRail => "NavigationRail".to_string(),
                NavigationPresentation::PermanentDrawer => "PermanentDrawer".to_string(),
            },
            workspace_pane_mode: match plan.workspace_pane_mode {
                WorkspacePaneMode::SinglePane => "SinglePane".to_string(),
                WorkspacePaneMode::ListDetail => "ListDetail".to_string(),
                WorkspacePaneMode::ThreePane => "ThreePane".to_string(),
            },
            show_chapter_tree: plan.visible_pane_roles.show_chapter_tree,
            show_editor: plan.visible_pane_roles.show_editor,
            show_project_list: plan.visible_pane_roles.show_project_list,
            show_bottom_bar: plan.show_bottom_bar,
            content_max_width_dp: plan.content_max_width_dp,
            page_padding_dp: plan.page_padding_dp,
            list_pane_min_dp: plan.list_pane_width.min_dp,
            list_pane_preferred_dp: plan.list_pane_width.preferred_dp,
            list_pane_max_dp: plan.list_pane_width.max_dp,
            editor_content_max_width_dp: plan.editor_content_max_width_dp,
            primary_pane_min_dp: plan.primary_pane_min_dp,
            primary_pane_preferred_dp: plan.primary_pane_preferred_dp,
            primary_pane_max_dp: plan.primary_pane_max_dp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use writer_core::layout_policy::{resolve_layout, WindowMetrics};

    #[test]
    fn test_linux_qt_layout_plan_dto_camel_case_output() {
        let metrics = WindowMetrics::default();
        let plan = resolve_layout(&metrics);
        let dto = LinuxQtLayoutPlanDto::from_layout_plan(&plan);
        let json = serde_json::to_string(&dto).unwrap();

        assert!(json.contains("\"shellMode\""));
        assert!(json.contains("\"navigationPresentation\""));
        assert!(json.contains("\"workspacePaneMode\""));
        assert!(json.contains("\"contentMaxWidthDp\""));
        assert!(json.contains("\"pagePaddingDp\""));
        assert!(json.contains("\"showBottomBar\""));
        assert!(json.contains("\"editorContentMaxWidthDp\""));

        assert!(!json.contains("\"shell_mode\""));
        assert!(!json.contains("\"content_max_width_dp\""));
        assert!(!json.contains("\"page_padding_dp\""));
    }

    #[test]
    fn test_linux_qt_layout_plan_dto_shell_mode_values() {
        let mut metrics = WindowMetrics::default();
        metrics.width_dp = 360.0;
        let plan = resolve_layout(&metrics);
        let dto = LinuxQtLayoutPlanDto::from_layout_plan(&plan);
        assert_eq!(dto.shell_mode, "SinglePane");

        metrics.width_dp = 700.0;
        let plan = resolve_layout(&metrics);
        let dto = LinuxQtLayoutPlanDto::from_layout_plan(&plan);
        assert_eq!(dto.shell_mode, "SupportingPane");

        metrics.width_dp = 1000.0;
        let plan = resolve_layout(&metrics);
        let dto = LinuxQtLayoutPlanDto::from_layout_plan(&plan);
        assert_eq!(dto.shell_mode, "TwoPane");

        metrics.width_dp = 1400.0;
        let plan = resolve_layout(&metrics);
        let dto = LinuxQtLayoutPlanDto::from_layout_plan(&plan);
        assert_eq!(dto.shell_mode, "ThreePane");
    }
}
