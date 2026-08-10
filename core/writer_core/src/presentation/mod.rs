//! # Presentation Contract — 平台无关的产品界面契约（#610 中间层）
//!
//! 三层结构：
//!
//! ```text
//! writer_core 业务语义（正文/作品/同步/编辑事务）
//!         ↓
//! presentation contract（页面角色、动作角色、面板角色、动作顺序、壳层角色）
//!         ↓
//! 平台 presentation/render（窗口适配、M3/Compose/QML/ArkUI 控件、Canvas 绘制）
//! ```
//!
//! 本模块只回答"事情是什么"：
//!
//! - [`layout_contract`]：产品壳层语义。输入是平台端已经测量好的窗口能力
//!   （可用栏数、折叠、指针、键盘），输出 `ShellMode / WorkspacePaneMode /
//!   VisiblePaneRoles`。Material 断点、dp 宽度、导航控件呈现
//!   （BottomBar/NavigationRail/Drawer）不属于产品事务语义，各平台自己算。
//! - [`screen_contract`]：产品页面动作语义。`ScreenRole / PaneRole /
//!   ActionRole / ActionSlot`，"设置/搜索/同步位于页头右侧以及它们的顺序"
//!   是素笺自己的设计语言，跨端统一；控件长什么样由各平台决定。

pub mod layout_contract;
pub mod screen_contract;

pub use layout_contract::*;
pub use screen_contract::*;

#[cfg(test)]
mod layout_contract_tests;
#[cfg(test)]
mod screen_contract_tests;
