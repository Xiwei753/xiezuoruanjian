//! Desktop 客户端专用的 LayoutPlan DTO
//!
//! 与跨平台 LayoutPlanDto 不同，DesktopLayoutPlanDto 只包含 QML 实际使用的字段，
//! 并确保输出为 camelCase，QML 无需 snake_case fallback。

use serde::Serialize;
use writer_core::layout_policy::{LayoutPlan, ShellMode};

/// Desktop 客户端专用的布局方案 DTO
///
/// 字段名使用 Rust snake_case 命名，通过 `#[serde(rename_all = "camelCase")]`
/// 序列化为 camelCase 输出给 QML。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLayoutPlanDto {
    /// 壳层模式：SinglePane / SupportingPane / TwoPane
    pub shell_mode: String,
    /// 正文最大宽度（vp），0 表示不限制
    pub content_max_width_vp: f32,
    /// 页面内边距（vp）
    pub page_padding_vp: f32,
    /// 是否显示侧面板
    pub show_side_panel: bool,
    /// 是否显示底栏
    pub show_bottom_bar: bool,
    /// 侧面板宽度（vp）
    pub side_panel_width_vp: f32,
    /// 主面板权重
    pub primary_pane_weight: f32,
    /// 详情面板最大宽度（vp）
    pub detail_panel_max_width_vp: f32,
}

impl DesktopLayoutPlanDto {
    pub fn from_layout_plan(plan: &LayoutPlan) -> Self {
        Self {
            shell_mode: match plan.shell_mode {
                ShellMode::SinglePane => "SinglePane".to_string(),
                ShellMode::SupportingPane => "SupportingPane".to_string(),
                ShellMode::TwoPane => "TwoPane".to_string(),
            },
            content_max_width_vp: plan.content_max_width_vp,
            page_padding_vp: plan.page_padding_vp,
            show_side_panel: plan.show_side_panel,
            show_bottom_bar: plan.show_bottom_bar,
            side_panel_width_vp: plan.side_panel_width_vp,
            primary_pane_weight: plan.primary_pane_weight,
            detail_panel_max_width_vp: plan.detail_panel_max_width_vp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use writer_core::layout_policy::{resolve_layout, WindowMetrics};

    #[test]
    fn test_desktop_layout_plan_dto_camel_case_output() {
        let metrics = WindowMetrics::default();
        let plan = resolve_layout(&metrics);
        let dto = DesktopLayoutPlanDto::from_layout_plan(&plan);
        let json = serde_json::to_string(&dto).unwrap();

        // 必须包含 camelCase 字段名
        assert!(
            json.contains("\"shellMode\""),
            "JSON must contain camelCase 'shellMode'"
        );
        assert!(
            json.contains("\"contentMaxWidthVp\""),
            "JSON must contain camelCase 'contentMaxWidthVp'"
        );
        assert!(
            json.contains("\"pagePaddingVp\""),
            "JSON must contain camelCase 'pagePaddingVp'"
        );
        assert!(
            json.contains("\"showSidePanel\""),
            "JSON must contain camelCase 'showSidePanel'"
        );
        assert!(
            json.contains("\"showBottomBar\""),
            "JSON must contain camelCase 'showBottomBar'"
        );
        assert!(
            json.contains("\"sidePanelWidthVp\""),
            "JSON must contain camelCase 'sidePanelWidthVp'"
        );
        assert!(
            json.contains("\"primaryPaneWeight\""),
            "JSON must contain camelCase 'primaryPaneWeight'"
        );
        assert!(
            json.contains("\"detailPanelMaxWidthVp\""),
            "JSON must contain camelCase 'detailPanelMaxWidthVp'"
        );

        // 绝不能包含 snake_case 字段名
        assert!(
            !json.contains("\"shell_mode\""),
            "JSON must NOT contain snake_case 'shell_mode'"
        );
        assert!(
            !json.contains("\"content_max_width_vp\""),
            "JSON must NOT contain snake_case 'content_max_width_vp'"
        );
        assert!(
            !json.contains("\"page_padding_vp\""),
            "JSON must NOT contain snake_case 'page_padding_vp'"
        );
        assert!(
            !json.contains("\"show_side_panel\""),
            "JSON must NOT contain snake_case 'show_side_panel'"
        );
        assert!(
            !json.contains("\"show_bottom_bar\""),
            "JSON must NOT contain snake_case 'show_bottom_bar'"
        );
        assert!(
            !json.contains("\"side_panel_width_vp\""),
            "JSON must NOT contain snake_case 'side_panel_width_vp'"
        );
        assert!(
            !json.contains("\"primary_pane_weight\""),
            "JSON must NOT contain snake_case 'primary_pane_weight'"
        );
        assert!(
            !json.contains("\"detail_panel_max_width_vp\""),
            "JSON must NOT contain snake_case 'detail_panel_max_width_vp'"
        );
    }

    #[test]
    fn test_desktop_layout_plan_dto_shell_mode_values() {
        // Compact width -> SinglePane
        let mut metrics = WindowMetrics::default();
        metrics.width_vp = 360.0;
        let plan = resolve_layout(&metrics);
        let dto = DesktopLayoutPlanDto::from_layout_plan(&plan);
        assert_eq!(dto.shell_mode, "SinglePane");

        // Medium width -> SupportingPane
        metrics.width_vp = 700.0;
        let plan = resolve_layout(&metrics);
        let dto = DesktopLayoutPlanDto::from_layout_plan(&plan);
        assert_eq!(dto.shell_mode, "SupportingPane");

        // Expanded width -> TwoPane
        metrics.width_vp = 1200.0;
        let plan = resolve_layout(&metrics);
        let dto = DesktopLayoutPlanDto::from_layout_plan(&plan);
        assert_eq!(dto.shell_mode, "TwoPane");
    }
}
