//! # 页面契约 — 平台无关的产品动作语义（#610）
//!
//! 本模块不碰 UI、不碰平台 API、不访问文件系统。
//! 只定义"动作属于哪个产品区域、按什么顺序"，不定义"动作长什么样"。
//!
//! 从共享契约删除的平台控件名：`BottomBar / NavigationRail /
//! PermanentDrawer / Floating / SidePanel`（#610 评论"怎么改"第 2 节）。
//! 控件呈现由各平台 presentation 层决定：例如 Android 把 `HeaderTrailing`
//! 映射到 TopAppBar actions（或窄窗口的扩展按钮），把 `Context` 映射到
//! DropdownMenu，把 `ItemTrailing` 映射到列表项尾部的图标按钮。
//!
//! "设置/搜索/同步位于页头右侧以及它们的顺序"（#597）是素笺自己的设计语言，
//! 由 `ActionSlot.order` 表达，跨端统一。

use serde::{Deserialize, Serialize};

// ========== 枚举定义 ==========

/// 页面角色
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenRole {
    Home,
    ProjectList,
    ProjectWorkspace,
    Writing,
    StarMap,
    Stats,
    Settings,
    Sync,
}

/// 面板角色
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneRole {
    PrimaryList,
    Detail,
    Editor,
    Inspector,
    Drawer,
    Supporting,
}

/// 动作角色
///
/// 只保留当前产品真实存在的动作（#610 评论二：`Save` 因正文自动保存、
/// `Sort` 因未实现而不再声明，避免 Core 与平台层出现"动作是否存在"的第二真相）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionRole {
    Back,
    CreateProject,
    CreateVolume,
    CreateChapter,
    Delete,
    Rename,
    Settings,
    Sync,
    Search,
}

/// 动作的业务目标 — 平台无关的身份（#610 评论二）。
///
/// 平台层据此把动作可靠绑定到对应业务操作（例如 Delete + Volume 绑定"删卷"、
/// Delete + Chapter 绑定"删章节"），不依赖区域/顺序猜身份。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionTarget {
    /// 没有业务对象的动作（设置/搜索/同步/返回等）。
    App,
    /// 作用于作品（作品列表的删除/重命名、新建作品等）。
    Project,
    /// 作用于卷（删卷/重命名卷、在卷内新建章节等）。
    Volume,
    /// 作用于章节（删章节/重命名章节）。
    Chapter,
}

/// 动作所属的产品区域 — 平台控件无关。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionRegion {
    /// 页头左侧（返回等）。
    HeaderLeading,
    /// 页头右侧（设置/搜索/同步等，顺序由 `ActionSlot.order` 表达）。
    HeaderTrailing,
    /// 列表头部区域（新建卷等）。
    ListHeader,
    /// 列表项尾部（新建章节等）。
    ItemTrailing,
    /// 上下文菜单（删除/重命名等）。
    Context,
    /// 空态区域（空列表的引导动作）。
    EmptyState,
}

// ========== 结构体定义 ==========

/// 动作槽位 — 描述产品区域、顺序与业务目标（#610 评论"怎么改"第 2 节 + 评论二）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionSlot {
    pub role: ActionRole,
    /// 平台无关的业务目标身份：Delete/Rename 等动作靠它区分"删卷/删章节"。
    pub target: ActionTarget,
    pub region: ActionRegion,
    /// 同一区域内从左到右（或从主到次）的显示顺序。
    pub order: u16,
    pub requires_confirmation: bool,
}

// ========== 核心纯函数 ==========

/// 根据页面角色解析动作槽位列表。纯函数，无副作用。
///
/// 槽位不随壳层模式变化：区域与顺序是产品语义，控件呈现由平台端决定
/// （#610：删除 `visible_in` 与壳层过滤的重复真相）。
// 槽位表是产品设计语言（#597）的平铺表达，行数多但无分支复杂度。
#[allow(clippy::too_many_lines)]
pub fn resolve_screen_policy(screen_role: ScreenRole) -> Vec<ActionSlot> {
    match screen_role {
        ScreenRole::Home => vec![
            ActionSlot {
                role: ActionRole::Search,
                target: ActionTarget::App,
                region: ActionRegion::HeaderTrailing,
                order: 20,
                requires_confirmation: false,
            },
            ActionSlot {
                role: ActionRole::Settings,
                target: ActionTarget::App,
                region: ActionRegion::HeaderTrailing,
                order: 30,
                requires_confirmation: false,
            },
        ],
        ScreenRole::ProjectList => vec![
            ActionSlot {
                role: ActionRole::CreateProject,
                target: ActionTarget::Project,
                region: ActionRegion::HeaderTrailing,
                order: 10,
                requires_confirmation: false,
            },
            // #610 评论二：删除/重命名目标就是 Project。
            ActionSlot {
                role: ActionRole::Delete,
                target: ActionTarget::Project,
                region: ActionRegion::Context,
                order: 10,
                requires_confirmation: true,
            },
            ActionSlot {
                role: ActionRole::Rename,
                target: ActionTarget::Project,
                region: ActionRegion::Context,
                order: 20,
                requires_confirmation: false,
            },
        ],
        // #597 正文：作品页顶栏右侧产品顺序（从右往左）为 设置 / 搜索 / 同步状态，
        // Material3 actions 按代码顺序从左往右摆，因此 order 升序为 同步 → 搜索 → 设置。
        // #610 评论二：Sort 未实现，不再在共享契约中声明；
        // Delete/Rename 各自通过 ActionTarget 区分卷与章节。
        ScreenRole::ProjectWorkspace => vec![
            ActionSlot {
                role: ActionRole::Sync,
                target: ActionTarget::App,
                region: ActionRegion::HeaderTrailing,
                order: 10,
                requires_confirmation: false,
            },
            ActionSlot {
                role: ActionRole::Search,
                target: ActionTarget::App,
                region: ActionRegion::HeaderTrailing,
                order: 20,
                requires_confirmation: false,
            },
            ActionSlot {
                role: ActionRole::Settings,
                target: ActionTarget::App,
                region: ActionRegion::HeaderTrailing,
                order: 30,
                requires_confirmation: false,
            },
            ActionSlot {
                role: ActionRole::CreateVolume,
                target: ActionTarget::Project,
                region: ActionRegion::ListHeader,
                order: 10,
                requires_confirmation: false,
            },
            ActionSlot {
                role: ActionRole::CreateChapter,
                target: ActionTarget::Volume,
                region: ActionRegion::ItemTrailing,
                order: 10,
                requires_confirmation: false,
            },
            ActionSlot {
                role: ActionRole::CreateChapter,
                target: ActionTarget::Volume,
                region: ActionRegion::EmptyState,
                order: 10,
                requires_confirmation: false,
            },
            ActionSlot {
                role: ActionRole::Delete,
                target: ActionTarget::Volume,
                region: ActionRegion::Context,
                order: 10,
                requires_confirmation: true,
            },
            ActionSlot {
                role: ActionRole::Delete,
                target: ActionTarget::Chapter,
                region: ActionRegion::Context,
                order: 20,
                requires_confirmation: true,
            },
            ActionSlot {
                role: ActionRole::Rename,
                target: ActionTarget::Volume,
                region: ActionRegion::Context,
                order: 30,
                requires_confirmation: false,
            },
            ActionSlot {
                role: ActionRole::Rename,
                target: ActionTarget::Chapter,
                region: ActionRegion::Context,
                order: 40,
                requires_confirmation: false,
            },
        ],
        // #597 正文：写作区只保留需要的图标层 — 同步、设置（搜索未实现不进入写作区）；
        // #610 评论二：正文自动保存，Save 不再是真实存在的动作，不再声明。
        // 返回箭头是否出现由平台端按工作区导航状态动态决定，不在静态契约里。
        ScreenRole::Writing => vec![
            ActionSlot {
                role: ActionRole::Back,
                target: ActionTarget::App,
                region: ActionRegion::HeaderLeading,
                order: 10,
                requires_confirmation: false,
            },
            ActionSlot {
                role: ActionRole::Sync,
                target: ActionTarget::App,
                region: ActionRegion::HeaderTrailing,
                order: 20,
                requires_confirmation: false,
            },
            ActionSlot {
                role: ActionRole::Settings,
                target: ActionTarget::App,
                region: ActionRegion::HeaderTrailing,
                order: 30,
                requires_confirmation: false,
            },
        ],
        // #597 正文四：星图根页没有返回动作（占位页无编辑态顶栏状态）。
        ScreenRole::StarMap => Vec::new(),
        // #597：统计根页是独立一级入口，不继承作品工作区的返回能力。
        ScreenRole::Stats => Vec::new(),
        ScreenRole::Settings => vec![ActionSlot {
            role: ActionRole::Back,
            target: ActionTarget::App,
            region: ActionRegion::HeaderLeading,
            order: 10,
            requires_confirmation: false,
        }],
        ScreenRole::Sync => vec![
            ActionSlot {
                role: ActionRole::Back,
                target: ActionTarget::App,
                region: ActionRegion::HeaderLeading,
                order: 10,
                requires_confirmation: false,
            },
            ActionSlot {
                role: ActionRole::Sync,
                target: ActionTarget::App,
                region: ActionRegion::HeaderTrailing,
                order: 10,
                requires_confirmation: false,
            },
        ],
    }
}
