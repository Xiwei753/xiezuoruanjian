//! Linux_qt 客户端专用的布局 DTO（#610 / #628）
//!
//! 与 Core 的 LayoutContract 不同，LinuxQtLayoutPlanDto 在 Core 契约之上叠加
//! Qt 自己的平台值（纸面最大宽度、页面内边距），并确保输出为 camelCase。
//!
//! 分层（#610 / #628）：
//! - Core `presentation::layout::resolve_layout(WindowViewport)`
//!   产出产品壳层契约（ShellMode / WorkspaceLayoutMode / PrimaryNavigationPlacement / LayoutMetrics）；
//! - 本文件把契约 + Qt 窗口宽高换算成 QML 实际使用的字段。
//!   Material 断点与 dp/vp 值属于 Qt 平台决策，不出现在 Core。
//! - #628：`show_primary_navigation` 改由 `ScreenPolicy` 提供，
//!   `from_contract` 接收它作为参数，由调用方从 `ScreenPolicy` 传入。

use serde::Serialize;
use writer_core::presentation::layout::{
    LayoutContract, PrimaryNavigationPlacement, ShellMode, WorkspaceLayoutMode,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxQtLayoutPlanDto {
    pub shell_mode: String,
    pub workspace_layout_mode: String,
    /// Issue #825 复核第4项：Core 的 `primary_navigation_placement` 直通。
    /// QML 用它决定一级导航放左侧还是放顶部，不用 `workspace_layout_mode` 猜
    /// （Core 在 600–839vp 宽度下已经是 Workbench，但一级导航仍给 Bottom）。
    pub primary_navigation_placement: String,
    pub show_primary_navigation: bool,
    /// 编辑纸面最大宽度（vp）。0 表示不限制（QML 自行回退）。
    pub content_max_width_vp: f32,
    /// 页面左右内边距（vp）。
    pub content_padding_vp: f32,
    /// Issue #827 评论 2：作品卡最小宽度（vp），Core `project_card_min_width_dp` 直通。
    /// 作品卡宽度是 Core 的共用尺寸（Android / Harmony 都读同一个值），
    /// QML 侧不再自己写死一套卡片宽度。
    pub project_card_min_width_vp: f32,
}

impl LinuxQtLayoutPlanDto {
    /// 由 Core 契约 + Qt 窗口宽度合成 Qt 布局 DTO。
    ///
    /// Qt 侧断点（Qt 平台自己的决策，不在 Core）：
    /// - 窗口宽 < 600vp：单栏；
    /// - 600–839vp：双栏（列表 + 详情）；
    /// - ≥ 840vp：三栏。
    ///
    /// 桌面端以鼠标为主，无软键盘，折叠屏能力按无处理。
    ///
    /// #628：`show_primary_navigation` 改由 `ScreenPolicy` 提供，
    /// 调用方从 `resolve_screen_policy` 获取后传入。
    pub fn from_contract(
        contract: &LayoutContract,
        window_width_vp: f32,
        show_primary_navigation: bool,
    ) -> Self {
        let paper_max_width_vp =
            if contract.workspace_layout_mode == WorkspaceLayoutMode::SinglePane {
                0.0
            } else {
                // 桌面写作纸面限宽 — Qt 平台值（QML 在 < 480 时还会再夹紧）。
                let padding = Self::content_padding_vp(contract) * 2.0;
                (840.0f32).min(window_width_vp - padding).max(0.0)
            };
        Self {
            shell_mode: match contract.shell_mode {
                ShellMode::SinglePane => "SinglePane".to_string(),
                ShellMode::SupportingPane => "SupportingPane".to_string(),
                ShellMode::TwoPane => "TwoPane".to_string(),
                ShellMode::ThreePane => "ThreePane".to_string(),
            },
            // #628 验收点 1：Core 已把 ListDetail/ThreePane 收口为 Workbench，
            // Qt 只剩 SinglePane / Workbench 两个产品语义。
            workspace_layout_mode: match contract.workspace_layout_mode {
                WorkspaceLayoutMode::SinglePane => "SinglePane".to_string(),
                WorkspaceLayoutMode::Workbench => "Workbench".to_string(),
            },
            show_primary_navigation,
            // Issue #825 复核第4项：Core 已经决定一级导航放哪，Qt 只做名字映射。
            primary_navigation_placement: match contract.primary_navigation_placement {
                PrimaryNavigationPlacement::Bottom => "Bottom".to_string(),
                PrimaryNavigationPlacement::Side => "Side".to_string(),
            },
            content_max_width_vp: paper_max_width_vp,
            content_padding_vp: Self::content_padding_vp(contract),
            project_card_min_width_vp: contract.metrics.project_card_min_width_dp,
        }
    }

    fn content_padding_vp(contract: &LayoutContract) -> f32 {
        // Qt 平台页面内边距：栏数越多留白越大（桌面窗口大，不用手机级 16）。
        // #628 验收点 1：ListDetail/ThreePane 已收口为 Workbench。
        // Workbench 是大屏工作台（章节导航 + 正文 + 工具 pane + 工具 rail），
        // 沿用原 ThreePane 的 32.0 留白，与 test_multi_pane_paper_clamps_to_window
        // 断言（900 - 2*32 = 836）一致。
        match contract.workspace_layout_mode {
            WorkspaceLayoutMode::SinglePane => 16.0,
            WorkspaceLayoutMode::Workbench => 32.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use writer_core::presentation::layout::resolver::WindowViewport;

    fn contract_for(width_vp: f32, height_vp: f32) -> LayoutContract {
        let viewport = WindowViewport {
            width_dp: width_vp,
            height_dp: height_vp,
            occlusions: Vec::new(),
        };
        writer_core::presentation::layout::resolve_layout(&viewport)
    }

    #[test]
    fn test_linux_qt_layout_plan_dto_camel_case_output() {
        let contract = contract_for(360.0, 640.0);
        let dto = LinuxQtLayoutPlanDto::from_contract(&contract, 360.0, true);
        let json = serde_json::to_string(&dto).unwrap();

        assert!(json.contains("\"shellMode\""));
        assert!(json.contains("\"workspaceLayoutMode\""));
        assert!(json.contains("\"primaryNavigationPlacement\""));
        assert!(json.contains("\"contentMaxWidthVp\""));
        assert!(json.contains("\"contentPaddingVp\""));
        assert!(json.contains("\"showPrimaryNavigation\""));
        assert!(json.contains("\"projectCardMinWidthVp\""));

        assert!(!json.contains("\"shell_mode\""));
        assert!(!json.contains("\"content_max_width_vp\""));
        assert!(!json.contains("\"primary_navigation_placement\""));
        assert!(!json.contains("\"navigationPresentation\""));
        assert!(!json.contains("\"pagePaddingDp\""));
    }

    #[test]
    fn test_primary_navigation_placement_follows_core_contract() {
        // Issue #825 复核第4项：600–839vp（Medium）Core 给 Bottom —— 即使已经是 Workbench，
        // QML 也不能因此把一级导航抬成左侧栏。
        let medium = LinuxQtLayoutPlanDto::from_contract(&contract_for(700.0, 600.0), 700.0, true);
        assert_eq!(medium.workspace_layout_mode, "Workbench");
        assert_eq!(medium.primary_navigation_placement, "Bottom");

        // ≥840vp（Wide）起 Core 给 Side。
        let wide = LinuxQtLayoutPlanDto::from_contract(&contract_for(1000.0, 800.0), 1000.0, true);
        assert_eq!(wide.primary_navigation_placement, "Side");

        let large = LinuxQtLayoutPlanDto::from_contract(&contract_for(1400.0, 900.0), 1400.0, true);
        assert_eq!(large.shell_mode, "ThreePane");
        assert_eq!(large.primary_navigation_placement, "Side");
    }

    #[test]
    fn test_project_card_min_width_follows_core_metrics() {
        // Issue #827 评论 2：作品卡宽度是 Core 的共用值，Qt 侧只做名字映射。
        // 作品卡和「+」卡必须共用同一个宽度，所以这个字段不能被 Qt 覆写成另一套尺寸。
        for width_vp in [360.0_f32, 700.0, 1000.0, 1400.0, 1920.0] {
            let contract = contract_for(width_vp, 900.0);
            let dto = LinuxQtLayoutPlanDto::from_contract(&contract, width_vp, true);
            assert_eq!(
                dto.project_card_min_width_vp,
                contract.metrics.project_card_min_width_dp
            );
            // Core 的默认值是 180dp，所有宽度下都应保持。
            assert_eq!(dto.project_card_min_width_vp, 180.0);
        }
    }

    #[test]
    fn test_linux_qt_layout_plan_dto_shell_mode_values() {
        // Narrow width → SinglePane
        assert_eq!(contract_for(360.0, 640.0).shell_mode, ShellMode::SinglePane);
        // Wide width → TwoPane
        assert_eq!(contract_for(1000.0, 800.0).shell_mode, ShellMode::TwoPane);
        // Large width → ThreePane
        assert_eq!(contract_for(1400.0, 900.0).shell_mode, ShellMode::ThreePane);
    }

    #[test]
    fn test_single_pane_paper_is_unbounded() {
        let contract = contract_for(360.0, 640.0);
        let dto = LinuxQtLayoutPlanDto::from_contract(&contract, 360.0, true);
        assert_eq!(dto.content_max_width_vp, 0.0);
        assert_eq!(dto.content_padding_vp, 16.0);
    }

    #[test]
    fn test_multi_pane_paper_clamps_to_window() {
        let contract = contract_for(1400.0, 900.0);
        let narrow = LinuxQtLayoutPlanDto::from_contract(&contract, 900.0, true);
        // 900 - 2*32 = 836 < 840 → 夹紧到窗口内。
        assert_eq!(narrow.content_max_width_vp, 836.0);

        let wide = LinuxQtLayoutPlanDto::from_contract(&contract, 1600.0, true);
        assert_eq!(wide.content_max_width_vp, 840.0);
        assert_eq!(wide.content_padding_vp, 32.0);
    }
}
