use super::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPlacementDto {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub z_index: i32,
    pub collapsed: bool,
}

impl From<crate::starmap::types::StarMapEmbedPlacement> for StarMapEmbedPlacementDto {
    fn from(p: crate::starmap::types::StarMapEmbedPlacement) -> Self {
        Self {
            x: p.x,
            y: p.y,
            width: p.width,
            height: p.height,
            scale: p.scale,
            z_index: p.z_index,
            collapsed: p.collapsed,
        }
    }
}

impl From<StarMapEmbedPlacementDto> for crate::starmap::types::StarMapEmbedPlacement {
    fn from(d: StarMapEmbedPlacementDto) -> Self {
        Self {
            x: d.x,
            y: d.y,
            width: d.width,
            height: d.height,
            scale: d.scale,
            z_index: d.z_index,
            collapsed: d.collapsed,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedViewportDto {
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl From<crate::starmap::types::StarMapEmbedViewport> for StarMapEmbedViewportDto {
    fn from(v: crate::starmap::types::StarMapEmbedViewport) -> Self {
        Self {
            scale: v.scale,
            offset_x: v.offset_x,
            offset_y: v.offset_y,
        }
    }
}

impl From<StarMapEmbedViewportDto> for crate::starmap::types::StarMapEmbedViewport {
    fn from(d: StarMapEmbedViewportDto) -> Self {
        Self {
            scale: d.scale,
            offset_x: d.offset_x,
            offset_y: d.offset_y,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedDto {
    pub instance_id: String,
    pub target_starmap_id: String,
    pub label: Option<String>,
    pub display_policy: StarMapDisplayPolicyDto,
    pub open_behavior: StarMapOpenBehaviorDto,
    pub placement: StarMapEmbedPlacementDto,
    pub target_viewport: StarMapEmbedViewportDto,
    pub host_path: StarMapTargetPathDto,
    pub provenance: StarMapProvenanceDto,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapEmbed> for StarMapEmbedDto {
    fn from(e: crate::starmap::types::StarMapEmbed) -> Self {
        Self {
            instance_id: e.instance_id,
            target_starmap_id: e.target_starmap_id,
            label: e.label,
            display_policy: e.display_policy.into(),
            open_behavior: e.open_behavior.into(),
            placement: e.placement.into(),
            target_viewport: e.target_viewport.into(),
            host_path: e.host_path.into(),
            provenance: e.provenance.into(),
            created_at: e.created_at,
            updated_at: e.updated_at,
        }
    }
}

impl From<StarMapEmbedDto> for crate::starmap::types::StarMapEmbed {
    fn from(d: StarMapEmbedDto) -> Self {
        Self {
            instance_id: d.instance_id,
            target_starmap_id: d.target_starmap_id,
            label: d.label,
            display_policy: d.display_policy.into(),
            open_behavior: d.open_behavior.into(),
            placement: d.placement.into(),
            target_viewport: d.target_viewport.into(),
            host_path: d.host_path.into(),
            provenance: d.provenance.into(),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPatchDto {
    pub label: Option<Option<String>>,
    pub display_policy: Option<StarMapDisplayPolicyDto>,
    pub open_behavior: Option<StarMapOpenBehaviorDto>,
    pub placement: Option<Option<StarMapEmbedPlacementDto>>,
    pub target_viewport: Option<Option<StarMapEmbedViewportDto>>,
    pub host_path: Option<StarMapTargetPathDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPatchInputDto {
    pub label: Option<String>,
    pub clear_label: bool,
    pub display_policy: Option<StarMapDisplayPolicyDto>,
    pub open_behavior: Option<StarMapOpenBehaviorDto>,
    pub placement: Option<StarMapEmbedPlacementDto>,
    pub clear_placement: bool,
    pub target_viewport: Option<StarMapEmbedViewportDto>,
    pub clear_target_viewport: bool,
    pub host_path: Option<StarMapTargetPathDto>,
}

impl From<StarMapEmbedPatchInputDto> for StarMapEmbedPatchDto {
    fn from(d: StarMapEmbedPatchInputDto) -> Self {
        Self {
            label: if d.clear_label {
                Some(None)
            } else {
                d.label.map(Some)
            },
            display_policy: d.display_policy,
            open_behavior: d.open_behavior,
            placement: if d.clear_placement {
                Some(None)
            } else {
                d.placement.map(Some)
            },
            target_viewport: if d.clear_target_viewport {
                Some(None)
            } else {
                d.target_viewport.map(Some)
            },
            host_path: d.host_path,
        }
    }
}

impl From<StarMapEmbedPatchDto> for crate::starmap::types::StarMapEmbedPatch {
    fn from(d: StarMapEmbedPatchDto) -> Self {
        Self {
            label: d.label,
            display_policy: d.display_policy.map(Into::into),
            open_behavior: d.open_behavior.map(Into::into),
            placement: d.placement.map(|p| p.map(Into::into)),
            target_viewport: d.target_viewport.map(|v| v.map(Into::into)),
            host_path: d.host_path.map(Into::into),
        }
    }
}
