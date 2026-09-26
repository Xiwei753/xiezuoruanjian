use super::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapGraphDto {
    pub schema_version: u32,
    pub starmap_id: String,
    pub nodes: Vec<StarMapNodeDto>,
    pub edges: Vec<StarMapEdgeDto>,
    #[serde(default)]
    pub embeds: Vec<StarMapEmbedDto>,
    #[serde(default)]
    pub links: Vec<StarMapLinkDto>,
    #[serde(default)]
    pub hyperlinks: Vec<StarMapHyperlinkDto>,
}

impl From<crate::starmap::types::StarMapGraph> for StarMapGraphDto {
    fn from(g: crate::starmap::types::StarMapGraph) -> Self {
        Self {
            schema_version: g.schema_version,
            starmap_id: g.starmap_id,
            nodes: g.nodes.into_iter().map(Into::into).collect(),
            edges: g.edges.into_iter().map(Into::into).collect(),
            embeds: g.embeds.into_iter().map(Into::into).collect(),
            links: g.links.into_iter().map(Into::into).collect(),
            hyperlinks: g.hyperlinks.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<StarMapGraphDto> for crate::starmap::types::StarMapGraph {
    fn from(d: StarMapGraphDto) -> Self {
        Self {
            schema_version: d.schema_version,
            starmap_id: d.starmap_id,
            nodes: d.nodes.into_iter().map(Into::into).collect(),
            edges: d.edges.into_iter().map(Into::into).collect(),
            embeds: d.embeds.into_iter().map(Into::into).collect(),
            links: d.links.into_iter().map(Into::into).collect(),
            hyperlinks: d.hyperlinks.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdgeDto {
    pub id: String,
    pub from: StarMapTargetPathDto,
    pub to: StarMapTargetPathDto,
    pub kind: StarMapEdgeKindDto,
    pub label: Option<String>,
    pub payload: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapEdge> for StarMapEdgeDto {
    fn from(e: crate::starmap::types::StarMapEdge) -> Self {
        Self {
            id: e.id,
            from: e.from.into(),
            to: e.to.into(),
            kind: e.kind.into(),
            label: e.label,
            payload: e
                .payload
                .map(|v| serde_json::to_string(&v).unwrap_or_default()),
            created_at: e.created_at,
            updated_at: e.updated_at,
        }
    }
}

impl From<StarMapEdgeDto> for crate::starmap::types::StarMapEdge {
    fn from(d: StarMapEdgeDto) -> Self {
        Self {
            id: d.id,
            from: d.from.into(),
            to: d.to.into(),
            kind: d.kind.into(),
            label: d.label,
            payload: d
                .payload
                .map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            created_at: d.created_at,
            updated_at: d.updated_at,
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
