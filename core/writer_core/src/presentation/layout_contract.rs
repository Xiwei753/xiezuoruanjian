//! # 布局契约 — 平台无关的产品壳层语义（#610）
//!
//! 本模块不碰 UI、不碰平台 API、不访问文件系统。
//! 只描述"产品有几栏、栏里放什么角色"，不描述平台控件和尺寸。
//!
//! ## 输入：平台已经判断好的窗口能力
//!
//! 各端测量自己的窗口系统后传入 [`WindowCapabilities`]：
//!
//! ```text
//! Android  WindowSizeClass / FoldingFeature / 指针类型 / IME 可见性
//! Qt       窗口宽高 / QScreen 折叠区域 / 指针类型 / 键盘可见性
//! Harmony  窗口 vp / 折叠状态 / 指针类型 / 键盘可见性
//! ```
//!
//! Material 断点（600/840/1200/1600）、pagePaddingDp、contentMaxWidthDp、
//! listPaneWidth dp、NavigationPresentation 不属于产品事务语义，
//! 由各平台在自己的 presentation 层计算（#610 评论"怎么改"第 2 节）。
//!
//! ## 输出：产品壳层契约
//!
//! [`LayoutContract`] 只含产品角色：壳层模式、作品面板模式、各面板可见性、
//! 是否显示一级导航。平台端把这份契约套到自己算好的具体尺寸上渲染。

use serde::{Deserialize, Serialize};

// ========== 输出枚举 ==========

/// 壳层模式 — 决定主界面框架结构。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellMode {
    SinglePane,
    SupportingPane,
    TwoPane,
    ThreePane,
}

/// 作品面板模式 — 决定项目列表/章节树/编辑器的组合方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspacePaneMode {
    SinglePane,
    ListDetail,
    ThreePane,
}

/// 各面板可见性 — 平台端据此决定哪些 UI 组件需要渲染。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisiblePaneRoles {
    pub show_project_list: bool,
    pub show_chapter_tree: bool,
    pub show_editor: bool,
    pub show_supporting: bool,
}

// ========== 输入枚举 ==========

/// 主输入类别 — 平台端判断后传入。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointerClass {
    Unknown,
    Touch,
    Stylus,
    Mouse,
}

// ========== 输入结构体 ==========

/// 窗口能力 — 平台端已经根据本平台窗口系统判断好的输入。
///
/// - `available_pane_count`：当前窗口可并排容纳的产品栏数（不含一级导航）。
/// - `has_separating_fold`：是否存在分隔式折叠铰链（半开折叠屏）。
/// - `pointer_class`：主输入类型。
/// - `keyboard_visible`：软键盘/输入法是否可见。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowCapabilities {
    pub available_pane_count: u8,
    pub has_separating_fold: bool,
    pub pointer_class: PointerClass,
    pub keyboard_visible: bool,
}

impl Default for WindowCapabilities {
    fn default() -> Self {
        Self {
            available_pane_count: 1,
            has_separating_fold: false,
            pointer_class: PointerClass::Touch,
            keyboard_visible: false,
        }
    }
}

// ========== 输出结构体 ==========

/// 布局契约 — `resolve_layout` 的纯函数输出，平台端据此绘制 UI。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutContract {
    pub shell_mode: ShellMode,
    pub workspace_pane_mode: WorkspacePaneMode,
    pub visible_pane_roles: VisiblePaneRoles,
    /// 是否显示一级导航（平台端决定具体呈现为底栏/侧栏/抽屉）。
    pub show_primary_navigation: bool,
}

// ========== 核心纯函数 ==========

/// 根据窗口能力解析布局契约。纯函数，无副作用。
///
/// 决策规则：
/// 1. 可用栏数决定基础 shell / workspace_pane_mode：
///    - 1 栏 → SinglePane，作品列表/章节树/编辑器依次栈式展示；
///    - 2 栏 → TwoPane + ListDetail（作品列表是入口，章节树+编辑器并排）；
///    - 3 栏 → ThreePane，作品列表/章节树/编辑器并排；
///    - 4 栏及以上 → ThreePane，并额外显示 supporting 面板。
/// 2. 分隔式折叠铰链 → 壳层降级为 SupportingPane（铰链两侧都是可用区）。
/// 3. 触摸类输入且软键盘可见、壳层为单栏时隐藏一级导航（避免底栏遮挡输入）。
pub fn resolve_layout(capabilities: &WindowCapabilities) -> LayoutContract {
    let (shell_mode, workspace_pane_mode, visible_pane_roles) =
        match capabilities.available_pane_count {
            0 | 1 => (
                ShellMode::SinglePane,
                WorkspacePaneMode::SinglePane,
                VisiblePaneRoles {
                    show_project_list: true,
                    show_chapter_tree: true,
                    show_editor: true,
                    show_supporting: false,
                },
            ),
            2 => (
                ShellMode::TwoPane,
                WorkspacePaneMode::ListDetail,
                VisiblePaneRoles {
                    show_project_list: false,
                    show_chapter_tree: true,
                    show_editor: true,
                    show_supporting: false,
                },
            ),
            n => (
                ShellMode::ThreePane,
                WorkspacePaneMode::ThreePane,
                VisiblePaneRoles {
                    show_project_list: true,
                    show_chapter_tree: true,
                    show_editor: true,
                    show_supporting: n >= 4,
                },
            ),
        };

    let shell_mode = if capabilities.has_separating_fold {
        ShellMode::SupportingPane
    } else {
        shell_mode
    };

    let touch_input = matches!(
        capabilities.pointer_class,
        PointerClass::Touch | PointerClass::Stylus
    );
    let show_primary_navigation = !(capabilities.keyboard_visible
        && touch_input
        && matches!(
            shell_mode,
            ShellMode::SinglePane | ShellMode::SupportingPane
        ));

    LayoutContract {
        shell_mode,
        workspace_pane_mode,
        visible_pane_roles,
        show_primary_navigation,
    }
}
