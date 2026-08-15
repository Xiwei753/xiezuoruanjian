//! Linux_qt 客户端专用的布局 DTO（#610 / #628）
//!
//! 与 Core 的 LayoutContract 不同，LinuxQtLayoutPlanDto 在 Core 契约之上叠加
//! Qt 自己的平台值（纸面最大宽度、页面内边距），并确保输出为 camelCase。
//!
//! 分层（#610 / #628）：
//! - Core `presentation::layout::resolve_layout(WindowViewport)`
//!   产出产品壳层契约（ShellMode / WorkspacePaneMode / PrimaryNavigationPlacement / LayoutMetrics）；
//! - 本文件把契约 + Qt 窗口宽高换算成 QML 实际使用的字段。
//!   Material 断点与 dp/vp 值属于 Qt 平台决策，不出现在 Core。
//! - #628：`show_primary_navigation` 改由 `ScreenPolicy` 提供，
//!   `from_contract` 接收它作为参数，由调用方从 `ScreenPolicy` 传入。

use serde::Serialize;
use writer_core::presentation::layout::{LayoutContract, ShellMode, WorkspacePaneMode};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxQtLayoutPlanDto {
    pub shell_mode: String,
    pub workspace_pane_mode: String,
    pub show_primary_navigation: bool,
    /// 编辑纸面最大宽度（vp）。0 表示不限制（QML 自行回退）。
    pub content_max_width_vp: f32,
    /// 页面左右内边距（vp）。
    pub content_padding_vp: f32,
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
        let paper_max_width_vp = if contract.workspace_pane_mode == WorkspacePaneMode::SinglePane {
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
            workspace_pane_mode: match contract.workspace_pane_mode {
                WorkspacePaneMode::SinglePane => "SinglePane".to_string(),
                WorkspacePaneMode::ListDetail => "ListDetail".to_string(),
                WorkspacePaneMode::ThreePane => "ThreePane".to_string(),
            },
            show_primary_navigation,
            content_max_width_vp: paper_max_width_vp,
            content_padding_vp: Self::content_padding_vp(contract),
        }
    }

    fn content_padding_vp(contract: &LayoutContract) -> f32 {
        // Qt 平台页面内边距：栏数越多留白越大（桌面窗口大，不用手机级 16）。
        match contract.workspace_pane_mode {
            WorkspacePaneMode::SinglePane => 16.0,
            WorkspacePaneMode::ListDetail => 24.0,
            WorkspacePaneMode::ThreePane => 32.0,
        }
    }
}

/// Qt 侧窗口能力换算：窗口宽 → 可用栏数（Qt 平台自己的断点，不在 Core）。
pub fn qt_available_pane_count(window_width_vp: f32) -> u8 {
    if window_width_vp < 600.0 {
        1
    } else if window_width_vp < 840.0 {
        2
    } else {
        3
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
        };
        writer_core::presentation::layout::resolve_layout(&viewport)
    }

    #[test]
    fn test_linux_qt_layout_plan_dto_camel_case_output() {
        let contract = contract_for(360.0, 640.0);
        let dto = LinuxQtLayoutPlanDto::from_contract(&contract, 360.0, true);
        let json = serde_json::to_string(&dto).unwrap();

        assert!(json.contains("\"shellMode\""));
        assert!(json.contains("\"workspacePaneMode\""));
        assert!(json.contains("\"contentMaxWidthVp\""));
        assert!(json.contains("\"contentPaddingVp\""));
        assert!(json.contains("\"showPrimaryNavigation\""));

        assert!(!json.contains("\"shell_mode\""));
        assert!(!json.contains("\"content_max_width_vp\""));
        assert!(!json.contains("\"navigationPresentation\""));
        assert!(!json.contains("\"pagePaddingDp\""));
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
    fn test_qt_available_pane_count_breakpoints() {
        assert_eq!(qt_available_pane_count(360.0), 1);
        assert_eq!(qt_available_pane_count(599.9), 1);
        assert_eq!(qt_available_pane_count(600.0), 2);
        assert_eq!(qt_available_pane_count(700.0), 2);
        assert_eq!(qt_available_pane_count(839.9), 2);
        assert_eq!(qt_available_pane_count(840.0), 3);
        assert_eq!(qt_available_pane_count(1400.0), 3);
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
