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

impl From<crate::presentation::screen_contract::ActionRole> for ActionRoleDto {
    fn from(r: crate::presentation::screen_contract::ActionRole) -> Self {
        match r {
            crate::presentation::screen_contract::ActionRole::Back => Self::Back,
            crate::presentation::screen_contract::ActionRole::CreateProject => Self::CreateProject,
            crate::presentation::screen_contract::ActionRole::CreateVolume => Self::CreateVolume,
            crate::presentation::screen_contract::ActionRole::CreateChapter => Self::CreateChapter,
            crate::presentation::screen_contract::ActionRole::Delete => Self::Delete,
            crate::presentation::screen_contract::ActionRole::Rename => Self::Rename,
            crate::presentation::screen_contract::ActionRole::Settings => Self::Settings,
            crate::presentation::screen_contract::ActionRole::Sync => Self::Sync,
            crate::presentation::screen_contract::ActionRole::Search => Self::Search,
        }
    }
}

impl From<ActionRoleDto> for crate::presentation::screen_contract::ActionRole {
    fn from(dto: ActionRoleDto) -> Self {
        match dto {
            ActionRoleDto::Back => Self::Back,
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

/// 动作的业务目标（#610 评论二）：平台层据此绑定业务操作，不靠区域/顺序猜身份。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum ActionTargetDto {
    #[default]
    App,
    Project,
    Volume,
    Chapter,
}

impl From<crate::presentation::screen_contract::ActionTarget> for ActionTargetDto {
    fn from(t: crate::presentation::screen_contract::ActionTarget) -> Self {
        match t {
            crate::presentation::screen_contract::ActionTarget::App => Self::App,
            crate::presentation::screen_contract::ActionTarget::Project => Self::Project,
            crate::presentation::screen_contract::ActionTarget::Volume => Self::Volume,
            crate::presentation::screen_contract::ActionTarget::Chapter => Self::Chapter,
        }
    }
}

impl From<ActionTargetDto> for crate::presentation::screen_contract::ActionTarget {
    fn from(dto: ActionTargetDto) -> Self {
        match dto {
            ActionTargetDto::App => Self::App,
            ActionTargetDto::Project => Self::Project,
            ActionTargetDto::Volume => Self::Volume,
            ActionTargetDto::Chapter => Self::Chapter,
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
    pub target: ActionTargetDto,
    pub region: ActionRegionDto,
    pub order: u16,
    pub requires_confirmation: bool,
}

impl From<crate::presentation::screen_contract::ActionSlot> for ActionSlotDto {
    fn from(s: crate::presentation::screen_contract::ActionSlot) -> Self {
        Self {
            role: s.role.into(),
            target: s.target.into(),
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
            target: dto.target.into(),
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
            crate::presentation::screen_contract::ActionRole::CreateProject,
            crate::presentation::screen_contract::ActionRole::CreateVolume,
            crate::presentation::screen_contract::ActionRole::CreateChapter,
            crate::presentation::screen_contract::ActionRole::Delete,
            crate::presentation::screen_contract::ActionRole::Rename,
            crate::presentation::screen_contract::ActionRole::Settings,
            crate::presentation::screen_contract::ActionRole::Sync,
            crate::presentation::screen_contract::ActionRole::Search,
        ];
        for role in roles {
            let dto: ActionRoleDto = role.into();
            let back: crate::presentation::screen_contract::ActionRole = dto.into();
            assert_eq!(back, role);
        }
    }

    #[test]
    fn test_action_target_dto_roundtrip() {
        // #610 评论二：业务目标身份必须穿过 DTO 往返。
        let targets = vec![
            crate::presentation::screen_contract::ActionTarget::App,
            crate::presentation::screen_contract::ActionTarget::Project,
            crate::presentation::screen_contract::ActionTarget::Volume,
            crate::presentation::screen_contract::ActionTarget::Chapter,
        ];
        for t in targets {
            let dto: ActionTargetDto = t.into();
            let back: crate::presentation::screen_contract::ActionTarget = dto.into();
            assert_eq!(back, t);
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
            role: crate::presentation::screen_contract::ActionRole::Delete,
            target: crate::presentation::screen_contract::ActionTarget::Chapter,
            region: crate::presentation::screen_contract::ActionRegion::Context,
            order: 20,
            requires_confirmation: true,
        };
        let dto: ActionSlotDto = slot.clone().into();
        let back: crate::presentation::screen_contract::ActionSlot = dto.into();
        assert_eq!(back.role, slot.role);
        assert_eq!(back.target, slot.target);
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
                target: ActionTargetDto::App,
                region: ActionRegionDto::HeaderLeading,
                order: 10,
                requires_confirmation: false,
            }],
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"screenRole\""));
        assert!(json.contains("\"actionSlots\""));
        assert!(json.contains("\"requiresConfirmation\""));
        assert!(json.contains("\"target\":\"App\""));
        assert!(json.contains("\"order\":10"));

        let deserialized: ScreenPolicyDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.screen_role, ScreenRoleDto::Writing);
        assert_eq!(deserialized.action_slots.len(), 1);
        assert_eq!(
            deserialized.action_slots[0].region,
            ActionRegionDto::HeaderLeading
        );
        assert_eq!(deserialized.action_slots[0].target, ActionTargetDto::App);
    }

    #[test]
    fn test_action_slot_dto_has_no_platform_fields() {
        let json = serde_json::to_string(&ActionSlotDto {
            role: ActionRoleDto::CreateProject,
            target: ActionTargetDto::Project,
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
        // 业务目标身份是产品语义，必须出现在序列化里。
        assert!(json.contains("\"target\""));
    }
}
