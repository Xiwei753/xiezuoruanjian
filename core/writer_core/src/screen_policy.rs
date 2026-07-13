//! # 页面策略模块 — 跨端共享的动作位置语义
//!
//! 本模块不碰 UI、不碰平台 API、不访问文件系统。
//! 只定义"动作放在哪"，不定义"动作长什么样"。
//!
//! ## 调用链路
//!
//! ```text
//! Android/Harmony/Linux_qt 测窗口 → 调 Core resolve_layout → 得到 ShellMode
//!   → 调 Core resolve_screen_policy(screen_role, shell_mode) → 得到 ActionSlot 列表
//!   → 各端 StyleAdapter 按 ActionSlot 渲染本平台控件
//! ```

use serde::{Deserialize, Serialize};

use crate::layout_policy::ShellMode;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionRole {
    Back,
    Save,
    CreateProject,
    CreateVolume,
    CreateChapter,
    Delete,
    Rename,
    Settings,
    Sync,
    Search,
    Sort,
}

/// 动作放置位置
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionPlacement {
    TopLeading,
    TopTrailing,
    Floating,
    BottomBar,
    ContextMenu,
    SidePanel,
    Navigation,
    ListHeader,
    ItemTrailing,
    EmptyState,
}

// ========== 结构体定义 ==========

/// 动作槽位
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionSlot {
    pub action_id: String,
    pub role: ActionRole,
    pub placement: ActionPlacement,
    pub visible_in: Vec<ShellMode>,
    pub requires_confirmation: bool,
}

// ========== 核心纯函数 ==========

/// 根据页面角色和壳层模式解析动作槽位列表
pub fn resolve_screen_policy(screen_role: ScreenRole, shell_mode: ShellMode) -> Vec<ActionSlot> {
    base_slots(screen_role)
        .into_iter()
        .filter(|slot| slot.visible_in.contains(&shell_mode))
        .collect()
}

/// 内部函数：返回某页面所有可能的 ActionSlot
fn base_slots(screen_role: ScreenRole) -> Vec<ActionSlot> {
    match screen_role {
        ScreenRole::Home => vec![
            ActionSlot {
                action_id: "settings".to_string(),
                role: ActionRole::Settings,
                placement: ActionPlacement::TopTrailing,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
            ActionSlot {
                action_id: "search".to_string(),
                role: ActionRole::Search,
                placement: ActionPlacement::TopTrailing,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
        ],
        ScreenRole::ProjectList => vec![
            ActionSlot {
                action_id: "create_project".to_string(),
                role: ActionRole::CreateProject,
                placement: ActionPlacement::Floating,
                visible_in: vec![ShellMode::SinglePane],
                requires_confirmation: false,
            },
            ActionSlot {
                action_id: "create_project".to_string(),
                role: ActionRole::CreateProject,
                placement: ActionPlacement::TopTrailing,
                visible_in: vec![ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
            ActionSlot {
                action_id: "delete_project".to_string(),
                role: ActionRole::Delete,
                placement: ActionPlacement::ContextMenu,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: true,
            },
            ActionSlot {
                action_id: "rename_project".to_string(),
                role: ActionRole::Rename,
                placement: ActionPlacement::ContextMenu,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
        ],
        ScreenRole::ProjectWorkspace => vec![
            ActionSlot {
                action_id: "create_volume".to_string(),
                role: ActionRole::CreateVolume,
                placement: ActionPlacement::Floating,
                visible_in: vec![ShellMode::SinglePane],
                requires_confirmation: false,
            },
            ActionSlot {
                action_id: "create_volume".to_string(),
                role: ActionRole::CreateVolume,
                placement: ActionPlacement::TopTrailing,
                visible_in: vec![ShellMode::SupportingPane],
                requires_confirmation: false,
            },
            ActionSlot {
                action_id: "create_volume".to_string(),
                role: ActionRole::CreateVolume,
                placement: ActionPlacement::ListHeader,
                visible_in: vec![ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
            ActionSlot {
                action_id: "create_chapter".to_string(),
                role: ActionRole::CreateChapter,
                placement: ActionPlacement::ItemTrailing,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
            ActionSlot {
                action_id: "create_chapter_empty".to_string(),
                role: ActionRole::CreateChapter,
                placement: ActionPlacement::EmptyState,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
            ActionSlot {
                action_id: "delete_volume".to_string(),
                role: ActionRole::Delete,
                placement: ActionPlacement::ContextMenu,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: true,
            },
            ActionSlot {
                action_id: "delete_chapter".to_string(),
                role: ActionRole::Delete,
                placement: ActionPlacement::ContextMenu,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: true,
            },
            ActionSlot {
                action_id: "rename_volume".to_string(),
                role: ActionRole::Rename,
                placement: ActionPlacement::ContextMenu,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
            ActionSlot {
                action_id: "rename_chapter".to_string(),
                role: ActionRole::Rename,
                placement: ActionPlacement::ContextMenu,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
            ActionSlot {
                action_id: "sort".to_string(),
                role: ActionRole::Sort,
                placement: ActionPlacement::TopTrailing,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
            ActionSlot {
                action_id: "search".to_string(),
                role: ActionRole::Search,
                placement: ActionPlacement::TopTrailing,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
        ],
        ScreenRole::Writing => vec![
            ActionSlot {
                action_id: "back".to_string(),
                role: ActionRole::Back,
                placement: ActionPlacement::TopLeading,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
            ActionSlot {
                action_id: "save".to_string(),
                role: ActionRole::Save,
                placement: ActionPlacement::TopTrailing,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
        ],
        ScreenRole::StarMap => vec![
            ActionSlot {
                action_id: "back".to_string(),
                role: ActionRole::Back,
                placement: ActionPlacement::TopLeading,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
        ],
        ScreenRole::Stats => vec![
            ActionSlot {
                action_id: "back".to_string(),
                role: ActionRole::Back,
                placement: ActionPlacement::TopLeading,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
        ],
        ScreenRole::Settings => vec![ActionSlot {
            action_id: "back".to_string(),
            role: ActionRole::Back,
            placement: ActionPlacement::TopLeading,
            visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
            requires_confirmation: false,
        }],
        ScreenRole::Sync => vec![
            ActionSlot {
                action_id: "back".to_string(),
                role: ActionRole::Back,
                placement: ActionPlacement::TopLeading,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
            ActionSlot {
                action_id: "sync".to_string(),
                role: ActionRole::Sync,
                placement: ActionPlacement::Floating,
                visible_in: vec![ShellMode::SinglePane, ShellMode::SupportingPane, ShellMode::TwoPane, ShellMode::ThreePane],
                requires_confirmation: false,
            },
        ],
    }
}

// ========== 单元测试 ==========

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screen_role_variants() {
        let variants = vec![
            ScreenRole::Home,
            ScreenRole::ProjectList,
            ScreenRole::ProjectWorkspace,
            ScreenRole::Writing,
            ScreenRole::StarMap,
            ScreenRole::Stats,
            ScreenRole::Settings,
            ScreenRole::Sync,
        ];
        assert_eq!(variants.len(), 8);
    }

    #[test]
    fn test_pane_role_variants() {
        let variants = vec![
            PaneRole::PrimaryList,
            PaneRole::Detail,
            PaneRole::Editor,
            PaneRole::Inspector,
            PaneRole::Drawer,
            PaneRole::Supporting,
        ];
        assert_eq!(variants.len(), 6);
    }

    #[test]
    fn test_action_role_variants() {
        let variants = vec![
            ActionRole::Back,
            ActionRole::Save,
            ActionRole::CreateProject,
            ActionRole::CreateVolume,
            ActionRole::CreateChapter,
            ActionRole::Delete,
            ActionRole::Rename,
            ActionRole::Settings,
            ActionRole::Sync,
            ActionRole::Search,
            ActionRole::Sort,
        ];
        assert_eq!(variants.len(), 11);
    }

    #[test]
    fn test_action_placement_variants() {
        let variants = vec![
            ActionPlacement::TopLeading,
            ActionPlacement::TopTrailing,
            ActionPlacement::Floating,
            ActionPlacement::BottomBar,
            ActionPlacement::ContextMenu,
            ActionPlacement::SidePanel,
            ActionPlacement::Navigation,
            ActionPlacement::ListHeader,
            ActionPlacement::ItemTrailing,
            ActionPlacement::EmptyState,
        ];
        assert_eq!(variants.len(), 10);
    }

    #[test]
    fn test_writing_single_pane() {
        let slots = resolve_screen_policy(ScreenRole::Writing, ShellMode::SinglePane);
        assert_eq!(slots.len(), 2);

        // Back → TopLeading
        assert_eq!(slots[0].action_id, "back");
        assert_eq!(slots[0].placement, ActionPlacement::TopLeading);
        assert_eq!(slots[0].role, ActionRole::Back);

        // Save → TopTrailing
        assert_eq!(slots[1].action_id, "save");
        assert_eq!(slots[1].placement, ActionPlacement::TopTrailing);
        assert_eq!(slots[1].role, ActionRole::Save);
    }

    #[test]
    fn test_workspace_single_pane() {
        let slots = resolve_screen_policy(ScreenRole::ProjectList, ShellMode::SinglePane);
        assert_eq!(slots.len(), 3);

        let create_project = slots
            .iter()
            .find(|s| s.role == ActionRole::CreateProject)
            .unwrap();
        assert_eq!(create_project.placement, ActionPlacement::Floating);
    }

    #[test]
    fn test_workspace_two_pane() {
        let slots = resolve_screen_policy(ScreenRole::ProjectList, ShellMode::TwoPane);
        assert_eq!(slots.len(), 3);

        let create_project = slots
            .iter()
            .find(|s| s.role == ActionRole::CreateProject)
            .unwrap();
        assert_eq!(create_project.placement, ActionPlacement::TopTrailing);

        let floating_create = slots.iter().find(|s| {
            s.role == ActionRole::CreateProject && s.placement == ActionPlacement::Floating
        });
        assert!(floating_create.is_none());
    }

    #[test]
    fn test_project_workspace_single_pane() {
        let slots = resolve_screen_policy(ScreenRole::ProjectWorkspace, ShellMode::SinglePane);
        let create_volume = slots.iter().find(|s| s.role == ActionRole::CreateVolume).unwrap();
        assert_eq!(create_volume.placement, ActionPlacement::Floating);

        let create_chapter = slots.iter().find(|s| s.role == ActionRole::CreateChapter && s.placement == ActionPlacement::ItemTrailing).unwrap();
        assert_eq!(create_chapter.action_id, "create_chapter");
    }

    #[test]
    fn test_project_workspace_two_pane() {
        let slots = resolve_screen_policy(ScreenRole::ProjectWorkspace, ShellMode::TwoPane);
        let create_volume = slots.iter().find(|s| s.role == ActionRole::CreateVolume).unwrap();
        assert_eq!(create_volume.placement, ActionPlacement::ListHeader);
    }

    #[test]
    fn test_home_policy() {
        let slots = resolve_screen_policy(ScreenRole::Home, ShellMode::SinglePane);
        assert_eq!(slots.len(), 2);

        // Settings → TopTrailing
        assert_eq!(slots[0].action_id, "settings");
        assert_eq!(slots[0].placement, ActionPlacement::TopTrailing);
        assert_eq!(slots[0].role, ActionRole::Settings);

        // Search → TopTrailing
        assert_eq!(slots[1].action_id, "search");
        assert_eq!(slots[1].placement, ActionPlacement::TopTrailing);
        assert_eq!(slots[1].role, ActionRole::Search);
    }

    #[test]
    fn test_settings_policy() {
        let slots = resolve_screen_policy(ScreenRole::Settings, ShellMode::SinglePane);
        assert_eq!(slots.len(), 1);

        // Back → TopLeading
        assert_eq!(slots[0].action_id, "back");
        assert_eq!(slots[0].placement, ActionPlacement::TopLeading);
        assert_eq!(slots[0].role, ActionRole::Back);
    }

    #[test]
    fn test_sync_policy() {
        let slots = resolve_screen_policy(ScreenRole::Sync, ShellMode::SinglePane);
        assert_eq!(slots.len(), 2);

        // Back → TopLeading
        assert_eq!(slots[0].action_id, "back");
        assert_eq!(slots[0].placement, ActionPlacement::TopLeading);
        assert_eq!(slots[0].role, ActionRole::Back);

        // Sync → Floating
        assert_eq!(slots[1].action_id, "sync");
        assert_eq!(slots[1].placement, ActionPlacement::Floating);
        assert_eq!(slots[1].role, ActionRole::Sync);
    }

    #[test]
    fn test_delete_requires_confirmation() {
        let slots = resolve_screen_policy(ScreenRole::ProjectWorkspace, ShellMode::SinglePane);
        let delete_slots: Vec<_> = slots.iter().filter(|s| s.role == ActionRole::Delete).collect();
        for slot in delete_slots {
            assert!(slot.requires_confirmation);
        }
    }

    #[test]
    fn test_shell_mode_filtering() {
        let single_slots = resolve_screen_policy(ScreenRole::ProjectList, ShellMode::SinglePane);
        let single_create = single_slots
            .iter()
            .find(|s| s.role == ActionRole::CreateProject)
            .unwrap();
        assert_eq!(single_create.placement, ActionPlacement::Floating);

        let two_slots = resolve_screen_policy(ScreenRole::ProjectList, ShellMode::TwoPane);
        let two_create = two_slots
            .iter()
            .find(|s| s.role == ActionRole::CreateProject)
            .unwrap();
        assert_eq!(two_create.placement, ActionPlacement::TopTrailing);

        assert!(single_slots
            .iter()
            .all(|s| !(s.role == ActionRole::CreateProject
                && s.placement == ActionPlacement::TopTrailing)));
        assert!(two_slots
            .iter()
            .all(|s| !(s.role == ActionRole::CreateProject
                && s.placement == ActionPlacement::Floating)));
    }

    #[test]
    fn test_action_slot_serialization() {
        let slot = ActionSlot {
            action_id: "back".to_string(),
            role: ActionRole::Back,
            placement: ActionPlacement::TopLeading,
            visible_in: vec![ShellMode::SinglePane],
            requires_confirmation: false,
        };
        let json = serde_json::to_string(&slot).unwrap();

        let deserialized: ActionSlot = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.action_id, "back");
        assert_eq!(deserialized.role, ActionRole::Back);
        assert_eq!(deserialized.placement, ActionPlacement::TopLeading);
        assert_eq!(deserialized.visible_in, vec![ShellMode::SinglePane]);
        assert!(!deserialized.requires_confirmation);
    }
}
