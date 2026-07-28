use super::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapGraphDto {
    pub schema_version: u32,
    pub id: String,
    pub starmap_id: String,
    pub title: String,
    pub nodes: Vec<StarMapNodeDto>,
    pub edges: Vec<StarMapEdgeDto>,
    #[serde(default)]
    pub embeds: Vec<StarMapEmbedDto>,
    #[serde(default)]
    pub links: Vec<StarMapLinkDto>,
    #[serde(default)]
    pub hyperlinks: Vec<StarMapHyperlinkDto>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapGraph> for StarMapGraphDto {
    fn from(g: crate::starmap::types::StarMapGraph) -> Self {
        Self {
            schema_version: g.schema_version,
            id: g.id,
            starmap_id: g.starmap_id,
            title: g.title,
            nodes: g.nodes.into_iter().map(Into::into).collect(),
            edges: g.edges.into_iter().map(Into::into).collect(),
            embeds: g.embeds.into_iter().map(Into::into).collect(),
            links: g.links.into_iter().map(Into::into).collect(),
            hyperlinks: g.hyperlinks.into_iter().map(Into::into).collect(),
            created_at: g.created_at,
            updated_at: g.updated_at,
        }
    }
}

impl From<StarMapGraphDto> for crate::starmap::types::StarMapGraph {
    fn from(d: StarMapGraphDto) -> Self {
        Self {
            schema_version: d.schema_version,
            id: d.id,
            starmap_id: d.starmap_id,
            title: d.title,
            nodes: d.nodes.into_iter().map(Into::into).collect(),
            edges: d.edges.into_iter().map(Into::into).collect(),
            embeds: d.embeds.into_iter().map(Into::into).collect(),
            links: d.links.into_iter().map(Into::into).collect(),
            hyperlinks: d.hyperlinks.into_iter().map(Into::into).collect(),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdgeDto {
    pub id: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub kind: StarMapEdgeKindDto,
    pub label: Option<String>,
    pub payload: Option<String>,
    #[serde(default)]
    pub from_target: Option<StarMapDeepTargetDto>,
    #[serde(default)]
    pub to_target: Option<StarMapDeepTargetDto>,
    #[serde(default)]
    pub from_endpoint: Option<StarMapEdgeEndpointDto>,
    #[serde(default)]
    pub to_endpoint: Option<StarMapEdgeEndpointDto>,
    #[serde(default)]
    pub from_endpoint_path: Option<StarMapEndpointPathDto>,
    #[serde(default)]
    pub to_endpoint_path: Option<StarMapEndpointPathDto>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapEdge> for StarMapEdgeDto {
    fn from(e: crate::starmap::types::StarMapEdge) -> Self {
        Self {
            id: e.id,
            from: e.from,
            to: e.to,
            kind: e.kind.into(),
            label: e.label,
            payload: e
                .payload
                .map(|v| serde_json::to_string(&v).unwrap_or_default()),
            from_target: e.from_target.map(Into::into),
            to_target: e.to_target.map(Into::into),
            from_endpoint: e.from_endpoint.map(Into::into),
            to_endpoint: e.to_endpoint.map(Into::into),
            from_endpoint_path: e.from_endpoint_path.map(Into::into),
            to_endpoint_path: e.to_endpoint_path.map(Into::into),
            created_at: e.created_at,
            updated_at: e.updated_at,
        }
    }
}

impl From<StarMapEdgeDto> for crate::starmap::types::StarMapEdge {
    fn from(d: StarMapEdgeDto) -> Self {
        Self {
            id: d.id,
            from: d.from,
            to: d.to,
            kind: d.kind.into(),
            label: d.label,
            payload: d
                .payload
                .map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            from_target: d.from_target.map(Into::into),
            to_target: d.to_target.map(Into::into),
            from_endpoint: d.from_endpoint.map(Into::into),
            to_endpoint: d.to_endpoint.map(Into::into),
            from_endpoint_path: d.from_endpoint_path.map(Into::into),
            to_endpoint_path: d.to_endpoint_path.map(Into::into),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdgeEndpointDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub node_id: Option<String>,
    pub anchor_id: Option<String>,
    pub target: Option<StarMapDeepTargetDto>,
}

impl From<crate::starmap::types::StarMapEdgeEndpoint> for StarMapEdgeEndpointDto {
    fn from(e: crate::starmap::types::StarMapEdgeEndpoint) -> Self {
        match e {
            crate::starmap::types::StarMapEdgeEndpoint::Node { node_id } => Self {
                kind: "node".to_string(),
                node_id: Some(node_id),
                anchor_id: None,
                target: None,
            },
            crate::starmap::types::StarMapEdgeEndpoint::Anchor { node_id, anchor_id } => Self {
                kind: "anchor".to_string(),
                node_id: Some(node_id),
                anchor_id: Some(anchor_id),
                target: None,
            },
            crate::starmap::types::StarMapEdgeEndpoint::Starmap => Self {
                kind: "starmap".to_string(),
                node_id: None,
                anchor_id: None,
                target: None,
            },
            crate::starmap::types::StarMapEdgeEndpoint::DeepTarget { target } => Self {
                kind: "deepTarget".to_string(),
                node_id: None,
                anchor_id: None,
                target: Some(target.into()),
            },
        }
    }
}

impl From<StarMapEdgeEndpointDto> for crate::starmap::types::StarMapEdgeEndpoint {
    fn from(d: StarMapEdgeEndpointDto) -> Self {
        match d.kind.as_str() {
            "anchor" => Self::Anchor {
                node_id: d.node_id.unwrap_or_default(),
                anchor_id: d.anchor_id.unwrap_or_default(),
            },
            "starmap" => Self::Starmap,
            "deepTarget" => Self::DeepTarget {
                target: d.target.map(Into::into).unwrap_or_else(|| {
                    crate::starmap::semantic::StarMapDeepTarget {
                        starmap_id: String::new(),
                        path: vec![],
                        target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
                    }
                }),
            },
            _ => Self::Node {
                node_id: d.node_id.unwrap_or_default(),
            },
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEndpointPathSegmentDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub starmap_id: Option<String>,
}

impl From<crate::starmap::types::StarMapEndpointPathSegment> for StarMapEndpointPathSegmentDto {
    fn from(s: crate::starmap::types::StarMapEndpointPathSegment) -> Self {
        match s {
            crate::starmap::types::StarMapEndpointPathSegment::EnterChildMap { starmap_id } => {
                Self {
                    kind: "enterChildMap".to_string(),
                    starmap_id: Some(starmap_id),
                }
            }
        }
    }
}

impl From<StarMapEndpointPathSegmentDto> for crate::starmap::types::StarMapEndpointPathSegment {
    fn from(d: StarMapEndpointPathSegmentDto) -> Self {
        Self::EnterChildMap {
            starmap_id: d.starmap_id.unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEndpointPathDto {
    #[serde(default)]
    pub segments: Vec<StarMapEndpointPathSegmentDto>,
    pub endpoint: StarMapEdgeEndpointDto,
}

impl From<crate::starmap::types::StarMapEndpointPath> for StarMapEndpointPathDto {
    fn from(p: crate::starmap::types::StarMapEndpointPath) -> Self {
        Self {
            segments: p.segments.into_iter().map(Into::into).collect(),
            endpoint: p.endpoint.into(),
        }
    }
}

impl From<StarMapEndpointPathDto> for crate::starmap::types::StarMapEndpointPath {
    fn from(d: StarMapEndpointPathDto) -> Self {
        Self {
            segments: d.segments.into_iter().map(Into::into).collect(),
            endpoint: d.endpoint.into(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdgeRenderDto {
    pub edge_id: String,
    pub from_cx: f32,
    pub from_cy: f32,
    pub to_cx: f32,
    pub to_cy: f32,
    pub start_x: f32,
    pub start_y: f32,
    pub end_x: f32,
    pub end_y: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub arrow_tip_x: f32,
    pub arrow_tip_y: f32,
    pub arrow_left_x: f32,
    pub arrow_left_y: f32,
    pub arrow_right_x: f32,
    pub arrow_right_y: f32,
    pub label_x: f32,
    pub label_y: f32,
    pub label: Option<String>,
    pub has_bidirectional: bool,
}

impl From<crate::starmap::render::EdgeRender> for StarMapEdgeRenderDto {
    fn from(r: crate::starmap::render::EdgeRender) -> Self {
        Self {
            edge_id: r.edge_id,
            from_cx: r.from_cx,
            from_cy: r.from_cy,
            to_cx: r.to_cx,
            to_cy: r.to_cy,
            start_x: r.start_x,
            start_y: r.start_y,
            end_x: r.end_x,
            end_y: r.end_y,
            offset_x: r.offset_x,
            offset_y: r.offset_y,
            arrow_tip_x: r.arrow_tip_x,
            arrow_tip_y: r.arrow_tip_y,
            arrow_left_x: r.arrow_left_x,
            arrow_left_y: r.arrow_left_y,
            arrow_right_x: r.arrow_right_x,
            arrow_right_y: r.arrow_right_y,
            label_x: r.label_x,
            label_y: r.label_y,
            label: r.label,
            has_bidirectional: r.has_bidirectional,
        }
    }
}
