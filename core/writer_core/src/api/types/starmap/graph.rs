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

impl TryFrom<StarMapGraphDto> for crate::starmap::types::StarMapGraph {
    type Error = crate::error::Error;

    fn try_from(d: StarMapGraphDto) -> Result<Self, Self::Error> {
        Ok(Self {
            schema_version: d.schema_version,
            starmap_id: d.starmap_id,
            nodes: d
                .nodes
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            edges: d
                .edges
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            embeds: d
                .embeds
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            links: d
                .links
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            hyperlinks: d
                .hyperlinks
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
        })
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

impl TryFrom<StarMapEdgeDto> for crate::starmap::types::StarMapEdge {
    type Error = crate::error::Error;

    fn try_from(d: StarMapEdgeDto) -> Result<Self, Self::Error> {
        let payload = d
            .payload
            .map(|s| serde_json::from_str(&s).map_err(crate::error::Error::from))
            .transpose()?;
        Ok(Self {
            id: d.id,
            from: d.from.try_into()?,
            to: d.to.try_into()?,
            kind: d.kind.into(),
            label: d.label,
            payload,
            created_at: d.created_at,
            updated_at: d.updated_at,
        })
    }
}
