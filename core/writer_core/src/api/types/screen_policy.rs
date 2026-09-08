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

impl From<crate::presentation::screen::ScreenRole> for ScreenRoleDto {
    fn from(r: crate::presentation::screen::ScreenRole) -> Self {
        match r {
            crate::presentation::screen::ScreenRole::Home => Self::Home,
            crate::presentation::screen::ScreenRole::ProjectList => Self::ProjectList,
            crate::presentation::screen::ScreenRole::ProjectWorkspace => Self::ProjectWorkspace,
            crate::presentation::screen::ScreenRole::Writing => Self::Writing,
            crate::presentation::screen::ScreenRole::StarMap => Self::StarMap,
            crate::presentation::screen::ScreenRole::Stats => Self::Stats,
            crate::presentation::screen::ScreenRole::Settings => Self::Settings,
            crate::presentation::screen::ScreenRole::Sync => Self::Sync,
        }
    }
}

impl From<ScreenRoleDto> for crate::presentation::screen::ScreenRole {
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

impl From<crate::presentation::screen::PaneRole> for PaneRoleDto {
    fn from(r: crate::presentation::screen::PaneRole) -> Self {
        match r {
            crate::presentation::screen::PaneRole::PrimaryList => Self::PrimaryList,
            crate::presentation::screen::PaneRole::Detail => Self::Detail,
            crate::presentation::screen::PaneRole::Editor => Self::Editor,
            crate::presentation::screen::PaneRole::Inspector => Self::Inspector,
            crate::presentation::screen::PaneRole::Drawer => Self::Drawer,
            crate::presentation::screen::PaneRole::Supporting => Self::Supporting,
        }
    }
}

impl From<PaneRoleDto> for crate::presentation::screen::PaneRole {
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
    /// #610 评论四：真实存在的顺序动作（上移）。
    MoveEarlier,
    /// #610 评论四：真实存在的顺序动作（下移）。
    MoveLater,
    Settings,
    Sync,
    #[default]
    Search,
}

impl From<crate::presentation::screen::ActionRole> for ActionRoleDto {
    fn from(r: crate::presentation::screen::ActionRole) -> Self {
        match r {
            crate::presentation::screen::ActionRole::Back => Self::Back,
            crate::presentation::screen::ActionRole::CreateProject => Self::CreateProject,
            crate::presentation::screen::ActionRole::CreateVolume => Self::CreateVolume,
            crate::presentation::screen::ActionRole::CreateChapter => Self::CreateChapter,
            crate::presentation::screen::ActionRole::Delete => Self::Delete,
            crate::presentation::screen::ActionRole::Rename => Self::Rename,
            crate::presentation::screen::ActionRole::MoveEarlier => Self::MoveEarlier,
            crate::presentation::screen::ActionRole::MoveLater => Self::MoveLater,
            crate::presentation::screen::ActionRole::Settings => Self::Settings,
            crate::presentation::screen::ActionRole::Sync => Self::Sync,
            crate::presentation::screen::ActionRole::Search => Self::Search,
        }
    }
}

impl From<ActionRoleDto> for crate::presentation::screen::ActionRole {
    fn from(dto: ActionRoleDto) -> Self {
        match dto {
            ActionRoleDto::Back => Self::Back,
            ActionRoleDto::CreateProject => Self::CreateProject,
            ActionRoleDto::CreateVolume => Self::CreateVolume,
            ActionRoleDto::CreateChapter => Self::CreateChapter,
            ActionRoleDto::Delete => Self::Delete,
            ActionRoleDto::Rename => Self::Rename,
            ActionRoleDto::MoveEarlier => Self::MoveEarlier,
            ActionRoleDto::MoveLater => Self::MoveLater,
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

impl From<crate::presentation::screen::ActionTarget> for ActionTargetDto {
    fn from(t: crate::presentation::screen::ActionTarget) -> Self {
        match t {
            crate::presentation::screen::ActionTarget::App => Self::App,
            crate::presentation::screen::ActionTarget::Project => Self::Project,
            crate::presentation::screen::ActionTarget::Volume => Self::Volume,
            crate::presentation::screen::ActionTarget::Chapter => Self::Chapter,
        }
    }
}

impl From<ActionTargetDto> for crate::presentation::screen::ActionTarget {
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
    /// #610 评论四：页面主操作区域（新建作品等）。
    PrimaryAction,
    ListHeader,
    ItemTrailing,
    Context,
    EmptyState,
}

impl From<crate::presentation::screen::ActionRegion> for ActionRegionDto {
    fn from(r: crate::presentation::screen::ActionRegion) -> Self {
        match r {
            crate::presentation::screen::ActionRegion::HeaderLeading => Self::HeaderLeading,
            crate::presentation::screen::ActionRegion::HeaderTrailing => Self::HeaderTrailing,
            crate::presentation::screen::ActionRegion::PrimaryAction => Self::PrimaryAction,
            crate::presentation::screen::ActionRegion::ListHeader => Self::ListHeader,
            crate::presentation::screen::ActionRegion::ItemTrailing => Self::ItemTrailing,
            crate::presentation::screen::ActionRegion::Context => Self::Context,
            crate::presentation::screen::ActionRegion::EmptyState => Self::EmptyState,
        }
    }
}

impl From<ActionRegionDto> for crate::presentation::screen::ActionRegion {
    fn from(dto: ActionRegionDto) -> Self {
        match dto {
            ActionRegionDto::HeaderLeading => Self::HeaderLeading,
            ActionRegionDto::HeaderTrailing => Self::HeaderTrailing,
            ActionRegionDto::PrimaryAction => Self::PrimaryAction,
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

impl From<crate::presentation::screen::ActionSlot> for ActionSlotDto {
    fn from(s: crate::presentation::screen::ActionSlot) -> Self {
        Self {
            role: s.role.into(),
            target: s.target.into(),
            region: s.region.into(),
            order: s.order,
            requires_confirmation: s.requires_confirmation,
        }
    }
}

impl From<ActionSlotDto> for crate::presentation::screen::ActionSlot {
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

/// #628 评论第 5 节：`ScreenPolicyDto` 新增 `show_primary_navigation`，
/// 由 Rust 根据页面角色决定，平台端直接读，不再传 `contractShowsPrimaryNavigation`。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScreenPolicyDto {
    pub screen_role: ScreenRoleDto,
    pub action_slots: Vec<ActionSlotDto>,
    pub show_primary_navigation: bool,
}

// ========== 单元测试 ==========

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screen_role_dto_roundtrip() {
        let roles = vec![
            crate::presentation::screen::ScreenRole::Home,
            crate::presentation::screen::ScreenRole::ProjectList,
            crate::presentation::screen::ScreenRole::ProjectWorkspace,
            crate::presentation::screen::ScreenRole::Writing,
            crate::presentation::screen::ScreenRole::StarMap,
            crate::presentation::screen::ScreenRole::Stats,
            crate::presentation::screen::ScreenRole::Settings,
            crate::presentation::screen::ScreenRole::Sync,
        ];
        for role in roles {
            let dto: ScreenRoleDto = role.into();
            let back: crate::presentation::screen::ScreenRole = dto.into();
            assert_eq!(back, role);
        }
    }

    #[test]
    fn test_action_role_dto_roundtrip() {
        let roles = vec![
            crate::presentation::screen::ActionRole::Back,
            crate::presentation::screen::ActionRole::CreateProject,
            crate::presentation::screen::ActionRole::CreateVolume,
            crate::presentation::screen::ActionRole::CreateChapter,
            crate::presentation::screen::ActionRole::Delete,
            crate::presentation::screen::ActionRole::Rename,
            crate::presentation::screen::ActionRole::MoveEarlier,
            crate::presentation::screen::ActionRole::MoveLater,
            crate::presentation::screen::ActionRole::Settings,
            crate::presentation::screen::ActionRole::Sync,
            crate::presentation::screen::ActionRole::Search,
        ];
        for role in roles {
            let dto: ActionRoleDto = role.into();
            let back: crate::presentation::screen::ActionRole = dto.into();
            assert_eq!(back, role);
        }
    }

    #[test]
    fn test_action_target_dto_roundtrip() {
        // #610 评论二：业务目标身份必须穿过 DTO 往返。
        let targets = vec![
            crate::presentation::screen::ActionTarget::App,
            crate::presentation::screen::ActionTarget::Project,
            crate::presentation::screen::ActionTarget::Volume,
            crate::presentation::screen::ActionTarget::Chapter,
        ];
        for t in targets {
            let dto: ActionTargetDto = t.into();
            let back: crate::presentation::screen::ActionTarget = dto.into();
            assert_eq!(back, t);
        }
    }

    #[test]
    fn test_action_region_dto_roundtrip() {
        let regions = vec![
            crate::presentation::screen::ActionRegion::HeaderLeading,
            crate::presentation::screen::ActionRegion::HeaderTrailing,
            crate::presentation::screen::ActionRegion::PrimaryAction,
            crate::presentation::screen::ActionRegion::ListHeader,
            crate::presentation::screen::ActionRegion::ItemTrailing,
            crate::presentation::screen::ActionRegion::Context,
            crate::presentation::screen::ActionRegion::EmptyState,
        ];
        for r in regions {
            let dto: ActionRegionDto = r.into();
            let back: crate::presentation::screen::ActionRegion = dto.into();
            assert_eq!(back, r);
        }
    }

    #[test]
    fn test_action_slot_dto_roundtrip() {
        let slot = crate::presentation::screen::ActionSlot {
            role: crate::presentation::screen::ActionRole::Delete,
            target: crate::presentation::screen::ActionTarget::Chapter,
            region: crate::presentation::screen::ActionRegion::Context,
            order: 20,
            requires_confirmation: true,
        };
        let dto: ActionSlotDto = slot.clone().into();
        let back: crate::presentation::screen::ActionSlot = dto.into();
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
            show_primary_navigation: false,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"screenRole\""));
        assert!(json.contains("\"actionSlots\""));
        assert!(json.contains("\"requiresConfirmation\""));
        assert!(json.contains("\"target\":\"App\""));
        assert!(json.contains("\"order\":10"));
        // #628：ScreenPolicyDto 必须包含 showPrimaryNavigation。
        assert!(json.contains("\"showPrimaryNavigation\""));

        let deserialized: ScreenPolicyDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.screen_role, ScreenRoleDto::Writing);
        assert_eq!(deserialized.action_slots.len(), 1);
        assert_eq!(
            deserialized.action_slots[0].region,
            ActionRegionDto::HeaderLeading
        );
        assert_eq!(deserialized.action_slots[0].target, ActionTargetDto::App);
        assert!(!deserialized.show_primary_navigation);
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
