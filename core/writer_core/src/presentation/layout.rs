//! # 布局契约 — 平台无关的产品壳层语义（#610 / #628）
//!
//! 本模块不碰 UI、不碰平台 API、不访问文件系统。
//! 只描述"产品有几栏、栏里放什么角色、一级导航放底栏还是侧栏、共用尺寸"，
//! 不描述平台控件和像素尺寸。
//!
//! ## 输入：原始窗口尺寸（dp）
//!
//! 各端测量自己的窗口系统后传入 [`resolver::WindowViewport`]（宽高 dp）。
//! 不再接收"Android 已经算好的 paneCount / has_separating_fold /
//! pointer_class / keyboard_visible"——这些平台判断不再参与
//! "窗口尺寸 → 页面结构"（#628）。
//!
//! ## 子模块
//!
//! - [`breakpoints`]：素笺自己的窗口宽度/高度 class 与分类函数。
//! - [`metrics`]：共用布局尺寸（列表栏宽度等）。
//! - [`resolver`]：`resolve_layout` 决策表，输入 [`resolver::WindowViewport`]，
//!   输出 [`LayoutContract`]。
//!
//! ## 输出：产品壳层契约
//!
//! [`LayoutContract`] 含产品角色：壳层模式、作品面板模式、
//! 一级导航放置（[`PrimaryNavigationPlacement`]）、共用尺寸（[`metrics::LayoutMetrics`]）。
//! 是否显示一级导航不再放在 `LayoutContract`，改由 [`super::screen::ScreenPolicy`] 提供
//! （#628 评论第 5 节）。

pub mod breakpoints;
pub mod metrics;
pub mod resolver;

#[cfg(test)]
mod tests;

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

/// 一级导航放置位置 — 平台无关（#628 评论第 4 节）。
///
/// Rust 根据窗口 class 决定 Bottom / Side；平台端只做映射：
/// - Android：`Bottom -> NavigationBar`，`Side -> NavigationRail`。
/// - Qt / Harmony：按各自控件库映射。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimaryNavigationPlacement {
    /// 底栏（手机/小平板）。
    Bottom,
    /// 侧栏（NavigationRail，桌面/大平板）。
    Side,
}

// ========== 输出结构体 ==========

/// 布局契约 — `resolve_layout` 的纯函数输出，平台端据此绘制 UI。
///
/// #628：删除 `show_primary_navigation`（改由 `ScreenPolicy` 提供），
/// 新增 `primary_navigation_placement` 与 `metrics`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutContract {
    pub shell_mode: ShellMode,
    pub workspace_pane_mode: WorkspacePaneMode,
    /// 一级导航放置位置（平台无关，由 Rust 根据窗口 class 决定）。
    pub primary_navigation_placement: PrimaryNavigationPlacement,
    /// 共用布局尺寸（dp），由 Core 决定，平台端只做 `.dp` 映射。
    pub metrics: metrics::LayoutMetrics,
}

// ========== 核心纯函数（重导出） ==========

/// 根据窗口视口解析布局契约。纯函数，无副作用。
///
/// 详见 [`resolver::resolve_layout`]。
pub fn resolve_layout(viewport: &resolver::WindowViewport) -> LayoutContract {
    resolver::resolve_layout(viewport)
}
