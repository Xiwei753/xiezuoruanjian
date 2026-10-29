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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PhasedSnapshotRequestDto {
    pub target_phase: String,
    pub since_revision: u64,
}

impl From<crate::starmap::store::PhasedSnapshotRequest> for PhasedSnapshotRequestDto {
    fn from(r: crate::starmap::store::PhasedSnapshotRequest) -> Self {
        Self {
            target_phase: match r.target_phase {
                crate::starmap::store::LoadPhase::GraphMeta => "GraphMeta".to_string(),
                crate::starmap::store::LoadPhase::CurrentObjects => "CurrentObjects".to_string(),
                crate::starmap::store::LoadPhase::PrefetchNearbyObjects => {
                    "PrefetchNearbyObjects".to_string()
                }
                crate::starmap::store::LoadPhase::BackgroundFullLoad => {
                    "BackgroundFullLoad".to_string()
                }
            },
            since_revision: r.since_revision,
        }
    }
}

impl From<PhasedSnapshotRequestDto> for crate::starmap::store::PhasedSnapshotRequest {
    fn from(d: PhasedSnapshotRequestDto) -> Self {
        Self {
            target_phase: match d.target_phase.as_str() {
                "GraphMeta" => crate::starmap::store::LoadPhase::GraphMeta,
                "CurrentObjects" => crate::starmap::store::LoadPhase::CurrentObjects,
                "BackgroundFullLoad" => crate::starmap::store::LoadPhase::BackgroundFullLoad,
                _ => crate::starmap::store::LoadPhase::PrefetchNearbyObjects,
            },
            since_revision: d.since_revision,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapPhasedSnapshotDto {
    pub starmap_id: String,
    pub load_phase: String,
    pub package_revision: u64,
    pub complete: bool,
    pub since_revision: u64,
    pub schema_version: u32,
    pub nodes: Vec<StarMapNodeDto>,
    pub edges: Vec<StarMapEdgeDto>,
    pub embeds: Vec<StarMapEmbedDto>,
    pub links: Vec<StarMapLinkDto>,
    pub deleted_node_ids: Vec<String>,
    pub deleted_edge_ids: Vec<String>,
    pub deleted_embed_ids: Vec<String>,
    pub deleted_link_ids: Vec<String>,
    pub diagnostics: Vec<LoadDiagnosticDto>,
}

impl From<crate::starmap::store::StarMapPhasedSnapshot> for StarMapPhasedSnapshotDto {
    fn from(s: crate::starmap::store::StarMapPhasedSnapshot) -> Self {
        Self {
            starmap_id: s.starmap_id,
            load_phase: match s.load_phase {
                crate::starmap::store::LoadPhase::GraphMeta => "GraphMeta".to_string(),
                crate::starmap::store::LoadPhase::CurrentObjects => "CurrentObjects".to_string(),
                crate::starmap::store::LoadPhase::PrefetchNearbyObjects => {
                    "PrefetchNearbyObjects".to_string()
                }
                crate::starmap::store::LoadPhase::BackgroundFullLoad => {
                    "BackgroundFullLoad".to_string()
                }
            },
            package_revision: s.package_revision,
            complete: s.complete,
            since_revision: s.since_revision,
            schema_version: s.schema_version,
            nodes: s.nodes.into_iter().map(Into::into).collect(),
            edges: s.edges.into_iter().map(Into::into).collect(),
            embeds: s.embeds.into_iter().map(Into::into).collect(),
            links: s.links.into_iter().map(Into::into).collect(),
            deleted_node_ids: s.deleted_node_ids,
            deleted_edge_ids: s.deleted_edge_ids,
            deleted_embed_ids: s.deleted_embed_ids,
            deleted_link_ids: s.deleted_link_ids,
            diagnostics: s
                .diagnostics
                .into_iter()
                .map(LoadDiagnosticDto::from)
                .collect(),
        }
    }
}
