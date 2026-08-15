//! # 页面契约 — 平台无关的产品动作语义（#610 / #628）
//!
//! 本模块不碰 UI、不碰平台 API、不访问文件系统。
//! 只定义"动作属于哪个产品区域、按什么顺序"以及"该页面是否显示一级导航"，
//! 不定义"动作长什么样"。
//!
//! 从共享契约删除的平台控件名：`BottomBar / NavigationRail /
//! PermanentDrawer / Floating / SidePanel`（#610 评论"怎么改"第 2 节）。
//! 控件呈现由各平台 presentation 层决定：例如 Android 把 `HeaderTrailing`
//! 映射到 TopAppBar actions（或窄窗口的扩展按钮），把 `Context` 映射到
//! DropdownMenu，把 `ItemTrailing` 映射到列表项尾部的图标按钮。
//!
//! "设置/搜索/同步位于页头右侧以及它们的顺序"（#597）是素笺自己的设计语言，
//! 由 `ActionSlot.order` 表达，跨端统一。
//!
//! #628 评论第 5 节：`ScreenPolicy` 新增 `show_primary_navigation`，
//! 由 Rust 根据页面角色决定，平台端直接读 `screenPolicy.showPrimaryNavigation`，
//! 删除 `contractShowsPrimaryNavigation` 参数。

pub mod policy;

#[cfg(test)]
mod tests;

pub use policy::{
    resolve_action_slots, resolve_screen_policy, resolve_show_primary_navigation, ActionRegion,
    ActionRole, ActionSlot, ActionTarget, PaneRole, ScreenPolicy, ScreenRole,
};
