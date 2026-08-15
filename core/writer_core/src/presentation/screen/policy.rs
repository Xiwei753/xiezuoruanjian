//! # 页面策略 — 平台无关的产品动作语义（#610 / #628）
//!
//! 本模块不碰 UI、不碰平台 API、不访问文件系统。
//! 只定义"动作属于哪个产品区域、按什么顺序"以及"该页面是否显示一级导航"，
//! 不定义"动作长什么样"。
//!
//! #628 评论第 5 节：`ScreenPolicy` 新增 `show_primary_navigation`，
//! 由 Rust 根据页面角色决定，平台端直接读，不再传
//! `contractShowsPrimaryNavigation` 参数。

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
/// #610 评论四：卷/章节的上移/下移是真实功能，用平台无关的
/// `MoveEarlier / MoveLater` 表达（不恢复笼统的 `Sort`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionRole {
    Back,
    CreateProject,
    CreateVolume,
    CreateChapter,
    Delete,
    Rename,
    /// 顺序动作：排到更前（Android 显示文字仍可为"上移"）。
    MoveEarlier,
    /// 顺序动作：排到更后（Android 显示文字仍可为"下移"）。
    MoveLater,
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
    /// 页面主操作区域（#610 评论四）：新建作品等主导航动作。
    /// Android compact 可画成 FAB，宽窗口按平台 M3 映射成合适的主操作控件；
    /// Core 不出现 `FloatingActionButton` 这类平台名。
    PrimaryAction,
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

/// 页面策略 — `resolve_screen_policy` 的输出（#628 新增 `show_primary_navigation`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenPolicy {
    pub screen_role: ScreenRole,
    pub action_slots: Vec<ActionSlot>,
    /// 该页面是否显示一级导航（#628 评论第 5 节）。
    ///
    /// 由 Rust 根据页面角色决定，平台端直接读，不再传
    /// `contractShowsPrimaryNavigation` 参数。
    pub show_primary_navigation: bool,
}

// ========== 核心纯函数 ==========

/// 根据页面角色解析动作槽位列表。纯函数，无副作用。
///
/// 槽位不随壳层模式变化：区域与顺序是产品语义，控件呈现由平台端决定
/// （#610：删除 `visible_in` 与壳层过滤的重复真相）。
// 槽位表是产品设计语言（#597）的平铺表达，行数多但无分支复杂度。
#[allow(clippy::too_many_lines)]
pub fn resolve_action_slots(screen_role: ScreenRole) -> Vec<ActionSlot> {
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
            // #610 评论五：作品列表顶栏右侧与 ProjectWorkspace 一致，
            // 同步 / 搜索 / 设置（order 升序）。
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
            // #610 评论四：新建作品是页面主操作（PrimaryAction），
            // 不再声明为 HeaderTrailing 而实际画在右下角。
            ActionSlot {
                role: ActionRole::CreateProject,
                target: ActionTarget::Project,
                region: ActionRegion::PrimaryAction,
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
        // #610 评论四：卷/章节的上移/下移是真实功能，以 MoveEarlier/MoveLater
        // 进入 Context 区域（不恢复笼统的 Sort）。
        ScreenRole::ProjectWorkspace => vec![
            // #610 评论五：作品工作区顶栏左侧返回动作。
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
            // #610 评论四：卷/章节的真实顺序动作（跨端语义 MoveEarlier/MoveLater）。
            ActionSlot {
                role: ActionRole::MoveEarlier,
                target: ActionTarget::Volume,
                region: ActionRegion::Context,
                order: 50,
                requires_confirmation: false,
            },
            ActionSlot {
                role: ActionRole::MoveLater,
                target: ActionTarget::Volume,
                region: ActionRegion::Context,
                order: 60,
                requires_confirmation: false,
            },
            ActionSlot {
                role: ActionRole::MoveEarlier,
                target: ActionTarget::Chapter,
                region: ActionRegion::Context,
                order: 70,
                requires_confirmation: false,
            },
            ActionSlot {
                role: ActionRole::MoveLater,
                target: ActionTarget::Chapter,
                region: ActionRegion::Context,
                order: 80,
                requires_confirmation: false,
            },
        ],
        // #624：写作区顶栏恢复 返回 + 同步 → 搜索 → 设置。搜索入口由 #477 接管，
        // 功能未完成时点击可暂无动作，但图标不得从产品契约消失。
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

/// 根据页面角色决定是否显示一级导航（#628 评论第 5 节）。
///
/// `Writing` 与 `Settings` 返回 false（沉浸态/二级页），其余按产品规则返回 true。
/// 不再依赖 `keyboard_visible` / `pointer_class`（已从输入删除）。
pub fn resolve_show_primary_navigation(screen_role: ScreenRole) -> bool {
    match screen_role {
        ScreenRole::Writing | ScreenRole::Settings => false,
        ScreenRole::Home
        | ScreenRole::ProjectList
        | ScreenRole::ProjectWorkspace
        | ScreenRole::StarMap
        | ScreenRole::Stats
        | ScreenRole::Sync => true,
    }
}

/// 根据页面角色解析完整页面策略。纯函数，无副作用。
///
/// 返回 [`ScreenPolicy`]，含动作槽位与 `show_primary_navigation`。
pub fn resolve_screen_policy(screen_role: ScreenRole) -> ScreenPolicy {
    ScreenPolicy {
        screen_role,
        action_slots: resolve_action_slots(screen_role),
        show_primary_navigation: resolve_show_primary_navigation(screen_role),
    }
}
