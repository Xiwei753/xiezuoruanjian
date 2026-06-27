// ── Screen Policy DTOs ──

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]

pub enum ScreenRoleDto {
    Home,
    #[default]
    Workspace,
    Writing,
    Settings,
    Sync,
}

impl From<crate::screen_policy::ScreenRole> for ScreenRoleDto {
    fn from(r: crate::screen_policy::ScreenRole) -> Self {
        match r {
            crate::screen_policy::ScreenRole::Home => Self::Home,
            crate::screen_policy::ScreenRole::Workspace => Self::Workspace,
            crate::screen_policy::ScreenRole::Writing => Self::Writing,
            crate::screen_policy::ScreenRole::Settings => Self::Settings,
            crate::screen_policy::ScreenRole::Sync => Self::Sync,
        }
    }
}

impl From<ScreenRoleDto> for crate::screen_policy::ScreenRole {
    fn from(dto: ScreenRoleDto) -> Self {
        match dto {
            ScreenRoleDto::Home => Self::Home,
            ScreenRoleDto::Workspace => Self::Workspace,
            ScreenRoleDto::Writing => Self::Writing,
            ScreenRoleDto::Settings => Self::Settings,
            ScreenRoleDto::Sync => Self::Sync,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]

pub enum PaneRoleDto {
    #[default]
    PrimaryList,
    Detail,
    Editor,
    Inspector,
    Drawer,
}

impl From<crate::screen_policy::PaneRole> for PaneRoleDto {
    fn from(r: crate::screen_policy::PaneRole) -> Self {
        match r {
            crate::screen_policy::PaneRole::PrimaryList => Self::PrimaryList,
            crate::screen_policy::PaneRole::Detail => Self::Detail,
            crate::screen_policy::PaneRole::Editor => Self::Editor,
            crate::screen_policy::PaneRole::Inspector => Self::Inspector,
            crate::screen_policy::PaneRole::Drawer => Self::Drawer,
        }
    }
}

impl From<PaneRoleDto> for crate::screen_policy::PaneRole {
    fn from(dto: PaneRoleDto) -> Self {
        match dto {
            PaneRoleDto::PrimaryList => Self::PrimaryList,
            PaneRoleDto::Detail => Self::Detail,
            PaneRoleDto::Editor => Self::Editor,
            PaneRoleDto::Inspector => Self::Inspector,
            PaneRoleDto::Drawer => Self::Drawer,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]

pub enum ActionRoleDto {
    Back,
    Save,
    CreateProject,
    CreateVolume,
    CreateChapter,
    Delete,
    Rename,
    Settings,
    Sync,
    #[default]
    Search,
}

impl From<crate::screen_policy::ActionRole> for ActionRoleDto {
    fn from(r: crate::screen_policy::ActionRole) -> Self {
        match r {
            crate::screen_policy::ActionRole::Back => Self::Back,
            crate::screen_policy::ActionRole::Save => Self::Save,
            crate::screen_policy::ActionRole::CreateProject => Self::CreateProject,
            crate::screen_policy::ActionRole::CreateVolume => Self::CreateVolume,
            crate::screen_policy::ActionRole::CreateChapter => Self::CreateChapter,
            crate::screen_policy::ActionRole::Delete => Self::Delete,
            crate::screen_policy::ActionRole::Rename => Self::Rename,
            crate::screen_policy::ActionRole::Settings => Self::Settings,
            crate::screen_policy::ActionRole::Sync => Self::Sync,
            crate::screen_policy::ActionRole::Search => Self::Search,
        }
    }
}

impl From<ActionRoleDto> for crate::screen_policy::ActionRole {
    fn from(dto: ActionRoleDto) -> Self {
        match dto {
            ActionRoleDto::Back => Self::Back,
            ActionRoleDto::Save => Self::Save,
            ActionRoleDto::CreateProject => Self::CreateProject,
            ActionRoleDto::CreateVolume => Self::CreateVolume,
            ActionRoleDto::CreateChapter => Self::CreateChapter,
            ActionRoleDto::Delete => Self::Delete,
            ActionRoleDto::Rename => Self::Rename,
            ActionRoleDto::Settings => Self::Settings,
            ActionRoleDto::Sync => Self::Sync,
            ActionRoleDto::Search => Self::Search,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]

pub enum ActionPlacementDto {
    #[default]
    TopLeading,
    TopTrailing,
    Floating,
    BottomBar,
    ContextMenu,
    SidePanel,
}

impl From<crate::screen_policy::ActionPlacement> for ActionPlacementDto {
    fn from(p: crate::screen_policy::ActionPlacement) -> Self {
        match p {
            crate::screen_policy::ActionPlacement::TopLeading => Self::TopLeading,
            crate::screen_policy::ActionPlacement::TopTrailing => Self::TopTrailing,
            crate::screen_policy::ActionPlacement::Floating => Self::Floating,
            crate::screen_policy::ActionPlacement::BottomBar => Self::BottomBar,
            crate::screen_policy::ActionPlacement::ContextMenu => Self::ContextMenu,
            crate::screen_policy::ActionPlacement::SidePanel => Self::SidePanel,
        }
    }
}

impl From<ActionPlacementDto> for crate::screen_policy::ActionPlacement {
    fn from(dto: ActionPlacementDto) -> Self {
        match dto {
            ActionPlacementDto::TopLeading => Self::TopLeading,
            ActionPlacementDto::TopTrailing => Self::TopTrailing,
            ActionPlacementDto::Floating => Self::Floating,
            ActionPlacementDto::BottomBar => Self::BottomBar,
            ActionPlacementDto::ContextMenu => Self::ContextMenu,
            ActionPlacementDto::SidePanel => Self::SidePanel,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActionSlotDto {
    pub action_id: String,
    pub role: ActionRoleDto,
    pub placement: ActionPlacementDto,
    pub visible_in: Vec<super::platform::ShellModeDto>,
    pub requires_confirmation: bool,
}

impl From<crate::screen_policy::ActionSlot> for ActionSlotDto {
    fn from(s: crate::screen_policy::ActionSlot) -> Self {
        Self {
            action_id: s.action_id,
            role: s.role.into(),
            placement: s.placement.into(),
            visible_in: s.visible_in.into_iter().map(Into::into).collect(),
            requires_confirmation: s.requires_confirmation,
        }
    }
}

impl From<ActionSlotDto> for crate::screen_policy::ActionSlot {
    fn from(dto: ActionSlotDto) -> Self {
        Self {
            action_id: dto.action_id,
            role: dto.role.into(),
            placement: dto.placement.into(),
            visible_in: dto.visible_in.into_iter().map(Into::into).collect(),
            requires_confirmation: dto.requires_confirmation,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScreenPolicyDto {
    pub screen_role: ScreenRoleDto,
    pub action_slots: Vec<ActionSlotDto>,
}

// ========== 单元测试 ==========

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screen_role_dto_roundtrip() {
        let roles = vec![
            crate::screen_policy::ScreenRole::Home,
            crate::screen_policy::ScreenRole::Workspace,
            crate::screen_policy::ScreenRole::Writing,
            crate::screen_policy::ScreenRole::Settings,
            crate::screen_policy::ScreenRole::Sync,
        ];
        for role in roles {
            let dto: ScreenRoleDto = role.into();
            let back: crate::screen_policy::ScreenRole = dto.into();
            assert_eq!(back, role);
        }
    }

    #[test]
    fn test_action_role_dto_roundtrip() {
        let roles = vec![
            crate::screen_policy::ActionRole::Back,
            crate::screen_policy::ActionRole::Save,
            crate::screen_policy::ActionRole::CreateProject,
            crate::screen_policy::ActionRole::CreateVolume,
            crate::screen_policy::ActionRole::CreateChapter,
            crate::screen_policy::ActionRole::Delete,
            crate::screen_policy::ActionRole::Rename,
            crate::screen_policy::ActionRole::Settings,
            crate::screen_policy::ActionRole::Sync,
            crate::screen_policy::ActionRole::Search,
        ];
        for role in roles {
            let dto: ActionRoleDto = role.into();
            let back: crate::screen_policy::ActionRole = dto.into();
            assert_eq!(back, role);
        }
    }

    #[test]
    fn test_action_placement_dto_roundtrip() {
        let placements = vec![
            crate::screen_policy::ActionPlacement::TopLeading,
            crate::screen_policy::ActionPlacement::TopTrailing,
            crate::screen_policy::ActionPlacement::Floating,
            crate::screen_policy::ActionPlacement::BottomBar,
            crate::screen_policy::ActionPlacement::ContextMenu,
            crate::screen_policy::ActionPlacement::SidePanel,
        ];
        for p in placements {
            let dto: ActionPlacementDto = p.into();
            let back: crate::screen_policy::ActionPlacement = dto.into();
            assert_eq!(back, p);
        }
    }

    #[test]
    fn test_action_slot_dto_roundtrip() {
        let slot = crate::screen_policy::ActionSlot {
            action_id: "save".to_string(),
            role: crate::screen_policy::ActionRole::Save,
            placement: crate::screen_policy::ActionPlacement::TopTrailing,
            visible_in: vec![crate::layout_policy::ShellMode::SinglePane],
            requires_confirmation: false,
        };
        let dto: ActionSlotDto = slot.clone().into();
        let back: crate::screen_policy::ActionSlot = dto.into();
        assert_eq!(back.action_id, slot.action_id);
        assert_eq!(back.role, slot.role);
        assert_eq!(back.placement, slot.placement);
        assert_eq!(back.visible_in, slot.visible_in);
        assert_eq!(back.requires_confirmation, slot.requires_confirmation);
    }

    #[test]
    fn test_screen_policy_dto_serialization() {
        let dto = ScreenPolicyDto {
            screen_role: ScreenRoleDto::Writing,
            action_slots: vec![ActionSlotDto {
                action_id: "back".to_string(),
                role: ActionRoleDto::Back,
                placement: ActionPlacementDto::TopLeading,
                visible_in: vec![super::super::platform::ShellModeDto::SinglePane],
                requires_confirmation: false,
            }],
        };
        let json = serde_json::to_string(&dto).unwrap();
        // camelCase 字段名（struct 保留 rename_all = "camelCase"）
        assert!(json.contains("\"screenRole\""));
        assert!(json.contains("\"actionSlots\""));
        assert!(json.contains("\"actionId\""));
        assert!(json.contains("\"requiresConfirmation\""));
        assert!(json.contains("\"visibleIn\""));

        // PascalCase 枚举值（enum 已去掉 rename_all，serde 默认输出标识符名）
        assert!(
            json.contains("\"Writing\""),
            "enum ScreenRoleDto 应输出 PascalCase \"Writing\"，实际 JSON: {}",
            json
        );
        assert!(
            json.contains("\"Back\""),
            "enum ActionRoleDto 应输出 PascalCase \"Back\"，实际 JSON: {}",
            json
        );
        assert!(
            json.contains("\"TopLeading\""),
            "enum ActionPlacementDto 应输出 PascalCase \"TopLeading\"，实际 JSON: {}",
            json
        );
        assert!(
            json.contains("\"SinglePane\""),
            "enum ShellModeDto 应输出 PascalCase \"SinglePane\"，实际 JSON: {}",
            json
        );

        // 确保枚举值不是 camelCase（检查 enum 字段上下文）
        assert!(
            !json.contains("\"screenRole\":\"writing\""),
            "enum ScreenRoleDto 不应输出 camelCase \"writing\""
        );
        assert!(
            !json.contains("\"role\":\"back\""),
            "enum ActionRoleDto 不应输出 camelCase \"back\""
        );
        assert!(
            !json.contains("\"placement\":\"topLeading\""),
            "enum ActionPlacementDto 不应输出 camelCase \"topLeading\""
        );
        assert!(
            !json.contains("\"visibleIn\":[\"singlePane\"]"),
            "enum ShellModeDto 不应输出 camelCase \"singlePane\""
        );

        // 反序列化
        let deserialized: ScreenPolicyDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.screen_role, ScreenRoleDto::Writing);
        assert_eq!(deserialized.action_slots.len(), 1);
    }
}
