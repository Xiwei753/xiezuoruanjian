// ── Screen Contract DTOs ──

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum ScreenRoleDto {
    Home,
    ProjectList,
    #[default]
    ProjectWorkspace,
    Writing,
    StarMap,
    Stats,
    Settings,
    Sync,
}

impl From<crate::presentation::screen_contract::ScreenRole> for ScreenRoleDto {
    fn from(r: crate::presentation::screen_contract::ScreenRole) -> Self {
        match r {
            crate::presentation::screen_contract::ScreenRole::Home => Self::Home,
            crate::presentation::screen_contract::ScreenRole::ProjectList => Self::ProjectList,
            crate::presentation::screen_contract::ScreenRole::ProjectWorkspace => {
                Self::ProjectWorkspace
            }
            crate::presentation::screen_contract::ScreenRole::Writing => Self::Writing,
            crate::presentation::screen_contract::ScreenRole::StarMap => Self::StarMap,
            crate::presentation::screen_contract::ScreenRole::Stats => Self::Stats,
            crate::presentation::screen_contract::ScreenRole::Settings => Self::Settings,
            crate::presentation::screen_contract::ScreenRole::Sync => Self::Sync,
        }
    }
}

impl From<ScreenRoleDto> for crate::presentation::screen_contract::ScreenRole {
    fn from(dto: ScreenRoleDto) -> Self {
        match dto {
            ScreenRoleDto::Home => Self::Home,
            ScreenRoleDto::ProjectList => Self::ProjectList,
            ScreenRoleDto::ProjectWorkspace => Self::ProjectWorkspace,
            ScreenRoleDto::Writing => Self::Writing,
            ScreenRoleDto::StarMap => Self::StarMap,
            ScreenRoleDto::Stats => Self::Stats,
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
    Supporting,
}

impl From<crate::presentation::screen_contract::PaneRole> for PaneRoleDto {
    fn from(r: crate::presentation::screen_contract::PaneRole) -> Self {
        match r {
            crate::presentation::screen_contract::PaneRole::PrimaryList => Self::PrimaryList,
            crate::presentation::screen_contract::PaneRole::Detail => Self::Detail,
            crate::presentation::screen_contract::PaneRole::Editor => Self::Editor,
            crate::presentation::screen_contract::PaneRole::Inspector => Self::Inspector,
            crate::presentation::screen_contract::PaneRole::Drawer => Self::Drawer,
            crate::presentation::screen_contract::PaneRole::Supporting => Self::Supporting,
        }
    }
}

impl From<PaneRoleDto> for crate::presentation::screen_contract::PaneRole {
    fn from(dto: PaneRoleDto) -> Self {
        match dto {
            PaneRoleDto::PrimaryList => Self::PrimaryList,
            PaneRoleDto::Detail => Self::Detail,
            PaneRoleDto::Editor => Self::Editor,
            PaneRoleDto::Inspector => Self::Inspector,
            PaneRoleDto::Drawer => Self::Drawer,
            PaneRoleDto::Supporting => Self::Supporting,
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
    Sort,
}

impl From<crate::presentation::screen_contract::ActionRole> for ActionRoleDto {
    fn from(r: crate::presentation::screen_contract::ActionRole) -> Self {
        match r {
            crate::presentation::screen_contract::ActionRole::Back => Self::Back,
            crate::presentation::screen_contract::ActionRole::Save => Self::Save,
            crate::presentation::screen_contract::ActionRole::CreateProject => Self::CreateProject,
            crate::presentation::screen_contract::ActionRole::CreateVolume => Self::CreateVolume,
            crate::presentation::screen_contract::ActionRole::CreateChapter => Self::CreateChapter,
            crate::presentation::screen_contract::ActionRole::Delete => Self::Delete,
            crate::presentation::screen_contract::ActionRole::Rename => Self::Rename,
            crate::presentation::screen_contract::ActionRole::Settings => Self::Settings,
            crate::presentation::screen_contract::ActionRole::Sync => Self::Sync,
            crate::presentation::screen_contract::ActionRole::Search => Self::Search,
            crate::presentation::screen_contract::ActionRole::Sort => Self::Sort,
        }
    }
}

impl From<ActionRoleDto> for crate::presentation::screen_contract::ActionRole {
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
            ActionRoleDto::Sort => Self::Sort,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum ActionRegionDto {
    #[default]
    HeaderLeading,
    HeaderTrailing,
    ListHeader,
    ItemTrailing,
    Context,
    EmptyState,
}

impl From<crate::presentation::screen_contract::ActionRegion> for ActionRegionDto {
    fn from(r: crate::presentation::screen_contract::ActionRegion) -> Self {
        match r {
            crate::presentation::screen_contract::ActionRegion::HeaderLeading => {
                Self::HeaderLeading
            }
            crate::presentation::screen_contract::ActionRegion::HeaderTrailing => {
                Self::HeaderTrailing
            }
            crate::presentation::screen_contract::ActionRegion::ListHeader => Self::ListHeader,
            crate::presentation::screen_contract::ActionRegion::ItemTrailing => Self::ItemTrailing,
            crate::presentation::screen_contract::ActionRegion::Context => Self::Context,
            crate::presentation::screen_contract::ActionRegion::EmptyState => Self::EmptyState,
        }
    }
}

impl From<ActionRegionDto> for crate::presentation::screen_contract::ActionRegion {
    fn from(dto: ActionRegionDto) -> Self {
        match dto {
            ActionRegionDto::HeaderLeading => Self::HeaderLeading,
            ActionRegionDto::HeaderTrailing => Self::HeaderTrailing,
            ActionRegionDto::ListHeader => Self::ListHeader,
            ActionRegionDto::ItemTrailing => Self::ItemTrailing,
            ActionRegionDto::Context => Self::Context,
            ActionRegionDto::EmptyState => Self::EmptyState,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActionSlotDto {
    pub role: ActionRoleDto,
    pub region: ActionRegionDto,
    pub order: u16,
    pub requires_confirmation: bool,
}

impl From<crate::presentation::screen_contract::ActionSlot> for ActionSlotDto {
    fn from(s: crate::presentation::screen_contract::ActionSlot) -> Self {
        Self {
            role: s.role.into(),
            region: s.region.into(),
            order: s.order,
            requires_confirmation: s.requires_confirmation,
        }
    }
}

impl From<ActionSlotDto> for crate::presentation::screen_contract::ActionSlot {
    fn from(dto: ActionSlotDto) -> Self {
        Self {
            role: dto.role.into(),
            region: dto.region.into(),
            order: dto.order,
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
            crate::presentation::screen_contract::ScreenRole::Home,
            crate::presentation::screen_contract::ScreenRole::ProjectList,
            crate::presentation::screen_contract::ScreenRole::ProjectWorkspace,
            crate::presentation::screen_contract::ScreenRole::Writing,
            crate::presentation::screen_contract::ScreenRole::StarMap,
            crate::presentation::screen_contract::ScreenRole::Stats,
            crate::presentation::screen_contract::ScreenRole::Settings,
            crate::presentation::screen_contract::ScreenRole::Sync,
        ];
        for role in roles {
            let dto: ScreenRoleDto = role.into();
            let back: crate::presentation::screen_contract::ScreenRole = dto.into();
            assert_eq!(back, role);
        }
    }

    #[test]
    fn test_action_role_dto_roundtrip() {
        let roles = vec![
            crate::presentation::screen_contract::ActionRole::Back,
            crate::presentation::screen_contract::ActionRole::Save,
            crate::presentation::screen_contract::ActionRole::CreateProject,
            crate::presentation::screen_contract::ActionRole::CreateVolume,
            crate::presentation::screen_contract::ActionRole::CreateChapter,
            crate::presentation::screen_contract::ActionRole::Delete,
            crate::presentation::screen_contract::ActionRole::Rename,
            crate::presentation::screen_contract::ActionRole::Settings,
            crate::presentation::screen_contract::ActionRole::Sync,
            crate::presentation::screen_contract::ActionRole::Search,
            crate::presentation::screen_contract::ActionRole::Sort,
        ];
        for role in roles {
            let dto: ActionRoleDto = role.into();
            let back: crate::presentation::screen_contract::ActionRole = dto.into();
            assert_eq!(back, role);
        }
    }

    #[test]
    fn test_action_region_dto_roundtrip() {
        let regions = vec![
            crate::presentation::screen_contract::ActionRegion::HeaderLeading,
            crate::presentation::screen_contract::ActionRegion::HeaderTrailing,
            crate::presentation::screen_contract::ActionRegion::ListHeader,
            crate::presentation::screen_contract::ActionRegion::ItemTrailing,
            crate::presentation::screen_contract::ActionRegion::Context,
            crate::presentation::screen_contract::ActionRegion::EmptyState,
        ];
        for r in regions {
            let dto: ActionRegionDto = r.into();
            let back: crate::presentation::screen_contract::ActionRegion = dto.into();
            assert_eq!(back, r);
        }
    }

    #[test]
    fn test_action_slot_dto_roundtrip() {
        let slot = crate::presentation::screen_contract::ActionSlot {
            role: crate::presentation::screen_contract::ActionRole::Save,
            region: crate::presentation::screen_contract::ActionRegion::HeaderTrailing,
            order: 10,
            requires_confirmation: false,
        };
        let dto: ActionSlotDto = slot.clone().into();
        let back: crate::presentation::screen_contract::ActionSlot = dto.into();
        assert_eq!(back.role, slot.role);
        assert_eq!(back.region, slot.region);
        assert_eq!(back.order, slot.order);
        assert_eq!(back.requires_confirmation, slot.requires_confirmation);
    }

    #[test]
    fn test_screen_policy_dto_serialization() {
        let dto = ScreenPolicyDto {
            screen_role: ScreenRoleDto::Writing,
            action_slots: vec![ActionSlotDto {
                role: ActionRoleDto::Back,
                region: ActionRegionDto::HeaderLeading,
                order: 10,
                requires_confirmation: false,
            }],
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"screenRole\""));
        assert!(json.contains("\"actionSlots\""));
        assert!(json.contains("\"requiresConfirmation\""));
        assert!(json.contains("\"order\":10"));

        let deserialized: ScreenPolicyDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.screen_role, ScreenRoleDto::Writing);
        assert_eq!(deserialized.action_slots.len(), 1);
        assert_eq!(
            deserialized.action_slots[0].region,
            ActionRegionDto::HeaderLeading
        );
    }

    #[test]
    fn test_action_slot_dto_has_no_platform_fields() {
        let json = serde_json::to_string(&ActionSlotDto {
            role: ActionRoleDto::CreateProject,
            region: ActionRegionDto::HeaderTrailing,
            order: 10,
            requires_confirmation: false,
        })
        .unwrap();
        for platform_field in ["actionId", "visibleIn", "placement"] {
            assert!(
                !json.contains(platform_field),
                "ActionSlotDto 不得包含平台字段 {platform_field}"
            );
        }
    }
}
