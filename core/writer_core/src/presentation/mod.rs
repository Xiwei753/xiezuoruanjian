//! # Presentation Contract — 平台无关的产品界面契约（#610 / #628）
//!
//! 三层结构：
//!
//! ```text
//! writer_core 业务语义（正文/作品/同步/编辑事务）
//!         ↓
//! presentation contract（页面角色、动作角色、面板角色、动作顺序、壳层角色、布局尺寸）
//!         ↓
//! 平台 presentation/render（窗口适配、M3/Compose/QML/ArkUI 控件、Canvas 绘制）
//! ```
//!
//! 本模块只回答"事情是什么"：
//!
//! - [`layout`]：产品壳层语义。输入是平台端测量好的原始窗口尺寸
//!   （宽高 dp，[`layout::resolver::WindowViewport`]），输出 `ShellMode /
//!   WorkspacePaneMode / PrimaryNavigationPlacement / LayoutMetrics`。
//!   Material 断点、dp 宽度、导航控件呈现
//!   （BottomBar/NavigationRail/Drawer）不属于产品事务语义，
//!   各平台自己算；但"底栏还是侧栏"由 Rust 决定（#628 评论第 4 节）。
//! - [`screen`]：产品页面动作语义。`ScreenRole / PaneRole /
//!   ActionRole / ActionTarget / ActionSlot / ScreenPolicy`，
//!   "设置/搜索/同步位于页头右侧以及它们的顺序"是素笺自己的设计语言，
//!   跨端统一；动作的业务目标身份（Delete+Volume vs
//!   Delete+Chapter）由 `ActionTarget` 表达；控件长什么样由各平台决定。
//!   #628 评论第 5 节：`ScreenPolicy.show_primary_navigation` 由 Rust 决定。
//! - [`settings`]：设置页展示契约（section/item 顺序、控件类型、平台可见性）。
//!
//! #628：目录整理成固定入口。`mod.rs` 只负责导出 `layout / screen / settings`，
//! 以后找 UI 契约只进这个目录。Rust 模块用 `layout.rs + layout/*.rs`、
//! `screen.rs + screen/*.rs` 这种结构，不继续新增嵌套 mod.rs。

pub mod layout;
pub mod screen;
pub mod settings;
