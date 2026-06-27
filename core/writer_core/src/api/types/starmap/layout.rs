#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLayoutDto {
    pub kind: StarMapLayoutKindDto,
    pub nodes: Vec<StarMapLayoutNodeDto>,
}

impl From<crate::starmap::types::StarMapLayout> for StarMapLayoutDto {
    fn from(l: crate::starmap::types::StarMapLayout) -> Self {
        Self {
            kind: l.kind.into(),
            nodes: l.nodes.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<StarMapLayoutDto> for crate::starmap::types::StarMapLayout {
    fn from(d: StarMapLayoutDto) -> Self {
        Self {
            kind: d.kind.into(),
            nodes: d.nodes.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLayoutNodeDto {
    pub node_id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
    pub collapsed: bool,
    pub z_index: i32,
    pub scale: f32,
    pub depth: f32,
    pub focus_weight: f32,
    pub orbit_group: Option<String>,
}

impl From<crate::starmap::types::StarMapLayoutNode> for StarMapLayoutNodeDto {
    fn from(n: crate::starmap::types::StarMapLayoutNode) -> Self {
        Self {
            node_id: n.node_id,
            x: n.x,
            y: n.y,
            width: n.width,
            height: n.height,
            radius: n.radius,
            collapsed: n.collapsed,
            z_index: n.z_index,
            scale: n.scale,
            depth: n.depth,
            focus_weight: n.focus_weight,
            orbit_group: n.orbit_group,
        }
    }
}

impl From<StarMapLayoutNodeDto> for crate::starmap::types::StarMapLayoutNode {
    fn from(d: StarMapLayoutNodeDto) -> Self {
        Self {
            node_id: d.node_id,
            x: d.x,
            y: d.y,
            width: d.width,
            height: d.height,
            radius: d.radius,
            collapsed: d.collapsed,
            z_index: d.z_index,
            scale: d.scale,
            depth: d.depth,
            focus_weight: d.focus_weight,
            orbit_group: d.orbit_group,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]

pub enum StarMapLayoutKindDto {
    Freeform,
    AutoRadial,
    Custom,
}

impl From<crate::starmap::types::StarMapLayoutKind> for StarMapLayoutKindDto {
    fn from(k: crate::starmap::types::StarMapLayoutKind) -> Self {
        match k {
            crate::starmap::types::StarMapLayoutKind::Freeform => Self::Freeform,
            crate::starmap::types::StarMapLayoutKind::AutoRadial => Self::AutoRadial,
            crate::starmap::types::StarMapLayoutKind::Custom => Self::Custom,
        }
    }
}

impl From<StarMapLayoutKindDto> for crate::starmap::types::StarMapLayoutKind {
    fn from(dto: StarMapLayoutKindDto) -> Self {
        match dto {
            StarMapLayoutKindDto::Freeform => Self::Freeform,
            StarMapLayoutKindDto::AutoRadial => Self::AutoRadial,
            StarMapLayoutKindDto::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapViewportDto {
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub width: f32,
    pub height: f32,
}

impl From<crate::starmap::types::StarMapViewport> for StarMapViewportDto {
    fn from(v: crate::starmap::types::StarMapViewport) -> Self {
        Self {
            scale: v.scale,
            offset_x: v.offset_x,
            offset_y: v.offset_y,
            width: v.width,
            height: v.height,
        }
    }
}

impl From<StarMapViewportDto> for crate::starmap::types::StarMapViewport {
    fn from(dto: StarMapViewportDto) -> Self {
        Self {
            scale: dto.scale,
            offset_x: dto.offset_x,
            offset_y: dto.offset_y,
            width: dto.width,
            height: dto.height,
        }
    }
}
