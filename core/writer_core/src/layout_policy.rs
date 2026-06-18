//! # 布局策略模块 — 跨端共享的纯函数布局决策
//!
//! 本模块不碰 UI、不碰平台 API、不访问文件系统。
//! 各端测量窗口尺寸后传入 `WindowMetrics`，调用 `resolve_layout` 获取 `LayoutPlan`。
//!
//! ## 调用链路
//!
//! ```text
//! Qt 测窗口宽高 -> 调 Core resolve_layout -> QML 按 LayoutPlan 画
//! Android 测窗口尺寸 -> 调 Core resolve_layout -> View/XML 或 Compose 按 LayoutPlan 画
//! Harmony 测窗口 vp / 折叠状态 -> 调 Core resolve_layout -> ArkUI 按 LayoutPlan 画
//! ```

use serde::{Deserialize, Serialize};

// ========== 输入枚举 ==========

/// 折叠屏姿态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FoldPosture {
    /// 非折叠设备或未知
    Unknown,
    /// 折叠屏完全展开（平板态）
    FullyOpened,
    /// 折叠屏半展开（帐篷/笔记本态）
    HalfOpened,
    /// 折叠屏合上（手机态）
    Closed,
}

/// 屏幕方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    Unknown,
    Portrait,
    Landscape,
}

/// 指针类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointerKind {
    Unknown,
    Touch,
    Stylus,
    Mouse,
}

// ========== 输出枚举 ==========

/// 宽度断点
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WidthClass {
    /// < 600vp — 手机/折叠外屏
    Compact,
    /// 600–840vp — 折叠展开小屏/竖屏平板
    Medium,
    /// >= 840vp — 平板/折叠内屏横向
    Expanded,
}

/// 高度断点
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeightClass {
    /// < 480vp — 横屏手机
    Compact,
    /// 480–900vp — 常规
    Medium,
    /// >= 900vp — 竖屏平板/折叠内屏
    Expanded,
}

/// 壳层模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellMode {
    /// 单面板（手机/折叠外屏）
    SinglePane,
    /// 支持面板（Medium 断点，可能显示辅助信息）
    SupportingPane,
    /// 双面板（平板/折叠内屏横向）
    TwoPane,
}

/// 编辑器模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditorMode {
    /// 正文满宽
    FullWidth,
    /// 正文居中，纸张效果
    CenteredPaper,
}

/// 导航模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavigationMode {
    /// 栈式导航（push/pop）
    Stack,
    /// 列表+详情
    ListDetail,
}

// ========== 输入结构体 ==========

/// 窗口度量输入（由各端平台测量后传入）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowMetrics {
    /// 窗口可用宽度（vp）
    pub width_vp: f32,
    /// 窗口可用高度（vp）
    pub height_vp: f32,
    /// 顶部安全区高度（vp），如状态栏
    pub safe_top_vp: f32,
    /// 底部安全区高度（vp），如导航栏
    pub safe_bottom_vp: f32,
    /// 键盘是否可见
    pub keyboard_visible: bool,
    /// 折叠姿态
    pub fold_posture: FoldPosture,
    /// 屏幕方向
    pub orientation: Orientation,
    /// 指针类型
    pub pointer: PointerKind,
}

impl Default for WindowMetrics {
    fn default() -> Self {
        Self {
            width_vp: 360.0,
            height_vp: 800.0,
            safe_top_vp: 0.0,
            safe_bottom_vp: 0.0,
            keyboard_visible: false,
            fold_posture: FoldPosture::Unknown,
            orientation: Orientation::Portrait,
            pointer: PointerKind::Touch,
        }
    }
}

// ========== 输出结构体 ==========

/// 布局计划输出（各端按此执行布局）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutPlan {
    pub width_class: WidthClass,
    pub height_class: HeightClass,
    pub shell_mode: ShellMode,
    pub editor_mode: EditorMode,
    pub navigation_mode: NavigationMode,
    /// 正文最大宽度（vp），0 表示不限制
    pub content_max_width_vp: f32,
    /// 页面内边距（vp）
    pub page_padding_vp: f32,
    /// 网格列数
    pub grid_columns: u8,
    /// 是否显示侧面板
    pub show_side_panel: bool,
    /// 是否显示底栏
    pub show_bottom_bar: bool,
    /// 侧面板宽度（vp），TwoPane 模式下左侧面板的固定宽度。
    /// 0 表示使用 weight 比例而非固定宽度。
    pub side_panel_width_vp: f32,
    /// 主面板权重（TwoPane 模式下左侧面板的 weight 比例）。
    /// 与 detail 面板 weight（隐含为 5.0 - primary_pane_weight）配合使用。
    pub primary_pane_weight: f32,
    /// 详情面板最大宽度（vp），0 表示不限制。
    /// 用于限制右侧面板在超宽屏幕下无限拉伸。
    pub detail_panel_max_width_vp: f32,
}

// ========== 断点阈值 ==========

const WIDTH_COMPACT_MAX: f32 = 600.0;
const WIDTH_MEDIUM_MAX: f32 = 840.0;
const HEIGHT_COMPACT_MAX: f32 = 480.0;
const HEIGHT_MEDIUM_MAX: f32 = 900.0;

// ========== 核心纯函数 ==========

/// 根据窗口度量解析布局计划
///
/// 这是跨端共享的布局策略，纯函数，无副作用。
/// 各端测量窗口尺寸后调用此函数，按返回的 LayoutPlan 执行布局。
pub fn resolve_layout(metrics: &WindowMetrics) -> LayoutPlan {
    let width_class = resolve_width_class(metrics.width_vp);
    let height_class = resolve_height_class(metrics.height_vp);

    // 基础策略（按宽度断点）
    let (shell_mode, navigation_mode, editor_mode, content_max_width_vp, page_padding_vp, grid_columns, show_side_panel, show_bottom_bar, side_panel_width_vp, primary_pane_weight, detail_panel_max_width_vp) = match width_class {
        WidthClass::Compact => (
            ShellMode::SinglePane,
            NavigationMode::Stack,
            EditorMode::FullWidth,
            0.0,    // content_max_width_vp
            16.0,   // page_padding_vp
            2,      // grid_columns
            false,  // show_side_panel
            true,   // show_bottom_bar
            0.0,    // side_panel_width_vp
            1.0,    // primary_pane_weight
            0.0,    // detail_panel_max_width_vp
        ),
        WidthClass::Medium => (
            ShellMode::SupportingPane,
            NavigationMode::Stack,
            EditorMode::CenteredPaper,
            720.0,
            24.0,
            3,
            false,
            true,
            0.0,    // side_panel_width_vp
            1.0,    // primary_pane_weight
            0.0,    // detail_panel_max_width_vp
        ),
        WidthClass::Expanded => (
            ShellMode::TwoPane,
            NavigationMode::ListDetail,
            EditorMode::CenteredPaper,
            840.0,
            32.0,
            4,
            true,
            false,
            0.0,    // side_panel_width_vp
            2.0,    // primary_pane_weight — 替代 Android 硬编码的 2f
            960.0,  // detail_panel_max_width_vp
        ),
    };

    // 特殊规则覆盖
    let shell_mode = if metrics.fold_posture == FoldPosture::HalfOpened {
        // 折叠屏半展开：即使是 Expanded 宽度也用 SupportingPane
        ShellMode::SupportingPane
    } else {
        shell_mode
    };

    let show_bottom_bar = if metrics.keyboard_visible {
        // 键盘可见时隐藏底栏
        false
    } else {
        show_bottom_bar
    };

    LayoutPlan {
        width_class,
        height_class,
        shell_mode,
        editor_mode,
        navigation_mode,
        content_max_width_vp,
        page_padding_vp,
        grid_columns,
        show_side_panel,
        show_bottom_bar,
        side_panel_width_vp,
        primary_pane_weight,
        detail_panel_max_width_vp,
    }
}

/// 解析宽度断点
pub fn resolve_width_class(width_vp: f32) -> WidthClass {
    if width_vp < WIDTH_COMPACT_MAX {
        WidthClass::Compact
    } else if width_vp < WIDTH_MEDIUM_MAX {
        WidthClass::Medium
    } else {
        WidthClass::Expanded
    }
}

/// 解析高度断点
pub fn resolve_height_class(height_vp: f32) -> HeightClass {
    if height_vp < HEIGHT_COMPACT_MAX {
        HeightClass::Compact
    } else if height_vp < HEIGHT_MEDIUM_MAX {
        HeightClass::Medium
    } else {
        HeightClass::Expanded
    }
}

// ========== 单元测试 ==========

#[cfg(test)]
mod tests {
    use super::*;

    fn default_metrics() -> WindowMetrics {
        WindowMetrics::default()
    }

    #[test]
    fn test_compact_width() {
        let mut m = default_metrics();
        m.width_vp = 360.0;
        let plan = resolve_layout(&m);
        assert_eq!(plan.width_class, WidthClass::Compact);
        assert_eq!(plan.shell_mode, ShellMode::SinglePane);
        assert_eq!(plan.navigation_mode, NavigationMode::Stack);
        assert_eq!(plan.grid_columns, 2);
        assert_eq!(plan.content_max_width_vp, 0.0);
        assert_eq!(plan.page_padding_vp, 16.0);
        assert_eq!(plan.editor_mode, EditorMode::FullWidth);
        assert!(!plan.show_side_panel);
        assert!(plan.show_bottom_bar);
    }

    #[test]
    fn test_medium_width() {
        let mut m = default_metrics();
        m.width_vp = 700.0;
        let plan = resolve_layout(&m);
        assert_eq!(plan.width_class, WidthClass::Medium);
        assert_eq!(plan.shell_mode, ShellMode::SupportingPane);
        assert_eq!(plan.navigation_mode, NavigationMode::Stack);
        assert_eq!(plan.grid_columns, 3);
        assert_eq!(plan.content_max_width_vp, 720.0);
        assert_eq!(plan.page_padding_vp, 24.0);
        assert_eq!(plan.editor_mode, EditorMode::CenteredPaper);
        assert!(!plan.show_side_panel);
        assert!(plan.show_bottom_bar);
    }

    #[test]
    fn test_expanded_width() {
        let mut m = default_metrics();
        m.width_vp = 1200.0;
        let plan = resolve_layout(&m);
        assert_eq!(plan.width_class, WidthClass::Expanded);
        assert_eq!(plan.shell_mode, ShellMode::TwoPane);
        assert_eq!(plan.navigation_mode, NavigationMode::ListDetail);
        assert_eq!(plan.grid_columns, 4);
        assert_eq!(plan.content_max_width_vp, 840.0);
        assert_eq!(plan.page_padding_vp, 32.0);
        assert_eq!(plan.editor_mode, EditorMode::CenteredPaper);
        assert!(plan.show_side_panel);
        assert!(!plan.show_bottom_bar);
    }

    #[test]
    fn test_fold_half_opened() {
        let mut m = default_metrics();
        m.width_vp = 1200.0;
        m.fold_posture = FoldPosture::HalfOpened;
        let plan = resolve_layout(&m);
        assert_eq!(plan.width_class, WidthClass::Expanded);
        // 即使是 Expanded 宽度，半展开也用 SupportingPane
        assert_eq!(plan.shell_mode, ShellMode::SupportingPane);
    }

    #[test]
    fn test_keyboard_visible_hides_bottom_bar() {
        let mut m = default_metrics();
        m.width_vp = 360.0;
        m.keyboard_visible = true;
        let plan = resolve_layout(&m);
        assert!(!plan.show_bottom_bar);

        // Expanded + keyboard
        m.width_vp = 1200.0;
        let plan = resolve_layout(&m);
        assert!(!plan.show_bottom_bar);
    }

    #[test]
    fn test_height_class() {
        let mut m = default_metrics();

        m.height_vp = 400.0;
        assert_eq!(resolve_height_class(m.height_vp), HeightClass::Compact);

        m.height_vp = 800.0;
        assert_eq!(resolve_height_class(m.height_vp), HeightClass::Medium);

        m.height_vp = 1000.0;
        assert_eq!(resolve_height_class(m.height_vp), HeightClass::Expanded);
    }

    #[test]
    fn test_boundary_values() {
        // 宽度边界
        assert_eq!(resolve_width_class(599.9), WidthClass::Compact);
        assert_eq!(resolve_width_class(600.0), WidthClass::Medium);
        assert_eq!(resolve_width_class(839.9), WidthClass::Medium);
        assert_eq!(resolve_width_class(840.0), WidthClass::Expanded);

        // 高度边界
        assert_eq!(resolve_height_class(479.9), HeightClass::Compact);
        assert_eq!(resolve_height_class(480.0), HeightClass::Medium);
        assert_eq!(resolve_height_class(899.9), HeightClass::Medium);
        assert_eq!(resolve_height_class(900.0), HeightClass::Expanded);
    }

    #[test]
    fn test_layout_plan_serialization() {
        let mut m = default_metrics();
        m.width_vp = 1200.0;
        let plan = resolve_layout(&m);
        let json = serde_json::to_string(&plan).unwrap();
        assert!(json.contains("\"width_class\":\"Expanded\""));
        assert!(json.contains("\"shell_mode\":\"TwoPane\""));
        assert!(json.contains("\"content_max_width_vp\":840.0"));

        // 反序列化
        let deserialized: LayoutPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.width_class, WidthClass::Expanded);
        assert_eq!(deserialized.grid_columns, 4);
    }

    #[test]
    fn test_window_metrics_default() {
        let m = WindowMetrics::default();
        assert_eq!(m.width_vp, 360.0);
        assert_eq!(m.height_vp, 800.0);
        assert!(!m.keyboard_visible);
        assert_eq!(m.fold_posture, FoldPosture::Unknown);
    }

    #[test]
    fn test_layout_plan_new_fields() {
        // Compact (SinglePane)
        let mut m = default_metrics();
        m.width_vp = 360.0;
        let plan = resolve_layout(&m);
        assert_eq!(plan.side_panel_width_vp, 0.0);
        assert_eq!(plan.primary_pane_weight, 1.0);
        assert_eq!(plan.detail_panel_max_width_vp, 0.0);

        // Medium (SupportingPane)
        m.width_vp = 700.0;
        let plan = resolve_layout(&m);
        assert_eq!(plan.side_panel_width_vp, 0.0);
        assert_eq!(plan.primary_pane_weight, 1.0);
        assert_eq!(plan.detail_panel_max_width_vp, 0.0);

        // Expanded (TwoPane)
        m.width_vp = 1200.0;
        let plan = resolve_layout(&m);
        assert_eq!(plan.side_panel_width_vp, 0.0);
        assert_eq!(plan.primary_pane_weight, 2.0);
        assert_eq!(plan.detail_panel_max_width_vp, 960.0);
    }
}