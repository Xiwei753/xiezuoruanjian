#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]

pub enum PlatformDto {
    #[default]
    Desktop,
    Android,
    Windows,
    Harmony,
    Apple,
}

impl From<PlatformDto> for writer_platform_api::PlatformKind {
    fn from(dto: PlatformDto) -> Self {
        match dto {
            PlatformDto::Desktop => Self::Desktop,
            PlatformDto::Android => Self::Android,
            PlatformDto::Windows => Self::Windows,
            PlatformDto::Harmony => Self::Harmony,
            PlatformDto::Apple => Self::Apple,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInitDto {
    pub platform: PlatformDto,
    pub app_data_dir: String,
    pub cache_dir: String,
    pub log_dir: String,
    pub no_backup_dir: Option<String>,
    pub device_id: String,
    pub app_version: String,
    pub locale: String,
    pub timezone: String,
    pub is_connected: bool,
    pub is_metered: bool,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<u16>,
}

impl From<PlatformInitDto> for writer_platform_api::PlatformInit {
    fn from(dto: PlatformInitDto) -> Self {
        Self {
            platform: dto.platform.into(),
            app_data_dir: dto.app_data_dir.into(),
            cache_dir: dto.cache_dir.into(),
            log_dir: dto.log_dir.into(),
            no_backup_dir: dto.no_backup_dir.map(Into::into),
            device_id: dto.device_id,
            app_version: dto.app_version,
            locale: dto.locale,
            timezone: dto.timezone,
        }
    }
}

impl From<PlatformInitDto> for writer_platform_api::NetworkState {
    fn from(dto: PlatformInitDto) -> Self {
        Self {
            is_connected: dto.is_connected,
            is_metered: dto.is_metered,
            proxy_host: dto.proxy_host,
            proxy_port: dto.proxy_port,
        }
    }
}

impl From<crate::writing_stats::Platform> for PlatformDto {
    fn from(p: crate::writing_stats::Platform) -> Self {
        match p {
            crate::writing_stats::Platform::Desktop => Self::Desktop,
            crate::writing_stats::Platform::Android => Self::Android,
            crate::writing_stats::Platform::Windows => Self::Windows,
            crate::writing_stats::Platform::Harmony => Self::Harmony,
            crate::writing_stats::Platform::Apple => Self::Apple,
        }
    }
}

// ── Layout Contract DTOs ──

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum ShellModeDto {
    #[default]
    SinglePane,
    SupportingPane,
    TwoPane,
    ThreePane,
}

impl From<crate::presentation::layout_contract::ShellMode> for ShellModeDto {
    fn from(s: crate::presentation::layout_contract::ShellMode) -> Self {
        match s {
            crate::presentation::layout_contract::ShellMode::SinglePane => Self::SinglePane,
            crate::presentation::layout_contract::ShellMode::SupportingPane => Self::SupportingPane,
            crate::presentation::layout_contract::ShellMode::TwoPane => Self::TwoPane,
            crate::presentation::layout_contract::ShellMode::ThreePane => Self::ThreePane,
        }
    }
}

impl From<ShellModeDto> for crate::presentation::layout_contract::ShellMode {
    fn from(dto: ShellModeDto) -> Self {
        match dto {
            ShellModeDto::SinglePane => Self::SinglePane,
            ShellModeDto::SupportingPane => Self::SupportingPane,
            ShellModeDto::TwoPane => Self::TwoPane,
            ShellModeDto::ThreePane => Self::ThreePane,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum WorkspacePaneModeDto {
    #[default]
    SinglePane,
    ListDetail,
    ThreePane,
}

impl From<crate::presentation::layout_contract::WorkspacePaneMode> for WorkspacePaneModeDto {
    fn from(w: crate::presentation::layout_contract::WorkspacePaneMode) -> Self {
        match w {
            crate::presentation::layout_contract::WorkspacePaneMode::SinglePane => Self::SinglePane,
            crate::presentation::layout_contract::WorkspacePaneMode::ListDetail => Self::ListDetail,
            crate::presentation::layout_contract::WorkspacePaneMode::ThreePane => Self::ThreePane,
        }
    }
}

impl From<WorkspacePaneModeDto> for crate::presentation::layout_contract::WorkspacePaneMode {
    fn from(dto: WorkspacePaneModeDto) -> Self {
        match dto {
            WorkspacePaneModeDto::SinglePane => Self::SinglePane,
            WorkspacePaneModeDto::ListDetail => Self::ListDetail,
            WorkspacePaneModeDto::ThreePane => Self::ThreePane,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VisiblePaneRolesDto {
    pub show_project_list: bool,
    pub show_chapter_tree: bool,
    pub show_editor: bool,
    pub show_supporting: bool,
}

impl Default for VisiblePaneRolesDto {
    fn default() -> Self {
        Self {
            show_project_list: true,
            show_chapter_tree: true,
            show_editor: true,
            show_supporting: false,
        }
    }
}

impl From<crate::presentation::layout_contract::VisiblePaneRoles> for VisiblePaneRolesDto {
    fn from(v: crate::presentation::layout_contract::VisiblePaneRoles) -> Self {
        Self {
            show_project_list: v.show_project_list,
            show_chapter_tree: v.show_chapter_tree,
            show_editor: v.show_editor,
            show_supporting: v.show_supporting,
        }
    }
}

impl From<VisiblePaneRolesDto> for crate::presentation::layout_contract::VisiblePaneRoles {
    fn from(dto: VisiblePaneRolesDto) -> Self {
        Self {
            show_project_list: dto.show_project_list,
            show_chapter_tree: dto.show_chapter_tree,
            show_editor: dto.show_editor,
            show_supporting: dto.show_supporting,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum PointerClassDto {
    #[default]
    Unknown,
    Touch,
    Stylus,
    Mouse,
}

impl From<crate::presentation::layout_contract::PointerClass> for PointerClassDto {
    fn from(p: crate::presentation::layout_contract::PointerClass) -> Self {
        match p {
            crate::presentation::layout_contract::PointerClass::Unknown => Self::Unknown,
            crate::presentation::layout_contract::PointerClass::Touch => Self::Touch,
            crate::presentation::layout_contract::PointerClass::Stylus => Self::Stylus,
            crate::presentation::layout_contract::PointerClass::Mouse => Self::Mouse,
        }
    }
}

impl From<PointerClassDto> for crate::presentation::layout_contract::PointerClass {
    fn from(dto: PointerClassDto) -> Self {
        match dto {
            PointerClassDto::Unknown => Self::Unknown,
            PointerClassDto::Touch => Self::Touch,
            PointerClassDto::Stylus => Self::Stylus,
            PointerClassDto::Mouse => Self::Mouse,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct WindowCapabilitiesDto {
    pub available_pane_count: u8,
    pub has_separating_fold: bool,
    pub pointer_class: PointerClassDto,
    pub keyboard_visible: bool,
}

impl From<crate::presentation::layout_contract::WindowCapabilities> for WindowCapabilitiesDto {
    fn from(c: crate::presentation::layout_contract::WindowCapabilities) -> Self {
        Self {
            available_pane_count: c.available_pane_count,
            has_separating_fold: c.has_separating_fold,
            pointer_class: c.pointer_class.into(),
            keyboard_visible: c.keyboard_visible,
        }
    }
}

impl From<WindowCapabilitiesDto> for crate::presentation::layout_contract::WindowCapabilities {
    fn from(dto: WindowCapabilitiesDto) -> Self {
        Self {
            available_pane_count: dto.available_pane_count,
            has_separating_fold: dto.has_separating_fold,
            pointer_class: dto.pointer_class.into(),
            keyboard_visible: dto.keyboard_visible,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayoutContractDto {
    pub shell_mode: ShellModeDto,
    pub workspace_pane_mode: WorkspacePaneModeDto,
    pub visible_pane_roles: VisiblePaneRolesDto,
    pub show_primary_navigation: bool,
}

impl From<crate::presentation::layout_contract::LayoutContract> for LayoutContractDto {
    fn from(c: crate::presentation::layout_contract::LayoutContract) -> Self {
        Self {
            shell_mode: c.shell_mode.into(),
            workspace_pane_mode: c.workspace_pane_mode.into(),
            visible_pane_roles: c.visible_pane_roles.into(),
            show_primary_navigation: c.show_primary_navigation,
        }
    }
}

impl From<LayoutContractDto> for crate::presentation::layout_contract::LayoutContract {
    fn from(dto: LayoutContractDto) -> Self {
        Self {
            shell_mode: dto.shell_mode.into(),
            workspace_pane_mode: dto.workspace_pane_mode.into(),
            visible_pane_roles: dto.visible_pane_roles.into(),
            show_primary_navigation: dto.show_primary_navigation,
        }
    }
}

// ========== 单元测试 ==========

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pointer_class_dto_roundtrip() {
        for p in [
            crate::presentation::layout_contract::PointerClass::Unknown,
            crate::presentation::layout_contract::PointerClass::Touch,
            crate::presentation::layout_contract::PointerClass::Stylus,
            crate::presentation::layout_contract::PointerClass::Mouse,
        ] {
            let dto: PointerClassDto = p.into();
            let back: crate::presentation::layout_contract::PointerClass = dto.into();
            assert_eq!(back, p);
        }
    }

    #[test]
    fn test_window_capabilities_dto_roundtrip() {
        let caps = crate::presentation::layout_contract::WindowCapabilities {
            available_pane_count: 3,
            has_separating_fold: true,
            pointer_class: crate::presentation::layout_contract::PointerClass::Mouse,
            keyboard_visible: false,
        };
        let dto: WindowCapabilitiesDto = caps.clone().into();
        let back: crate::presentation::layout_contract::WindowCapabilities = dto.into();
        assert_eq!(back.available_pane_count, caps.available_pane_count);
        assert_eq!(back.has_separating_fold, caps.has_separating_fold);
        assert_eq!(back.pointer_class, caps.pointer_class);
        assert_eq!(back.keyboard_visible, caps.keyboard_visible);
    }

    #[test]
    fn test_window_capabilities_dto_camel_case_fields() {
        let dto = WindowCapabilitiesDto {
            available_pane_count: 2,
            has_separating_fold: false,
            pointer_class: PointerClassDto::Touch,
            keyboard_visible: true,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"availablePaneCount\""));
        assert!(json.contains("\"hasSeparatingFold\""));
        assert!(json.contains("\"pointerClass\""));
        assert!(json.contains("\"keyboardVisible\""));
    }

    #[test]
    fn test_layout_contract_dto_roundtrip() {
        let contract = crate::presentation::layout_contract::LayoutContract {
            shell_mode: crate::presentation::layout_contract::ShellMode::TwoPane,
            workspace_pane_mode:
                crate::presentation::layout_contract::WorkspacePaneMode::ListDetail,
            visible_pane_roles: crate::presentation::layout_contract::VisiblePaneRoles {
                show_project_list: false,
                show_chapter_tree: true,
                show_editor: true,
                show_supporting: false,
            },
            show_primary_navigation: true,
        };
        let dto: LayoutContractDto = contract.clone().into();
        let back: crate::presentation::layout_contract::LayoutContract = dto.into();
        assert_eq!(back.shell_mode, contract.shell_mode);
        assert_eq!(back.workspace_pane_mode, contract.workspace_pane_mode);
        assert_eq!(
            back.visible_pane_roles.show_project_list,
            contract.visible_pane_roles.show_project_list
        );
        assert_eq!(
            back.show_primary_navigation,
            contract.show_primary_navigation
        );
    }
}
